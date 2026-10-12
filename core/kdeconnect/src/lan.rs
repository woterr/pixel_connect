// Pixel Connect — KDE Connect protocol v8, LAN backend.
//
// Reproduces the behaviour of kdeconnect-kde `core/backends/lan/` and
// kdeconnect-android `backends/lan/`:
//
//   * TCP listener on 1716..=1764 (`MIN_TCP_PORT` / `MAX_TCP_PORT`)
//   * the TCP *opener* becomes the TLS *server*, the TCP *accepter* becomes the
//     TLS client (role inversion, `lanlinkprovider.cpp`)
//   * identity is exchanged twice: once in cleartext on the control socket and
//     again inside TLS
//   * the peer certificate's CommonName must equal the announced deviceId and,
//     for paired devices, the certificate DER must match the pinned copy
//   * packet payloads travel on a second dedicated TLS connection whose port is
//     published in `payloadTransferInfo`
//
// UDP broadcast and mDNS live in `discovery.rs`.

use arc_swap::ArcSwap;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::Semaphore;

/// How long an inbound connection has to identify itself and complete TLS
/// before it is abandoned. A peer that opens a socket and then says nothing
/// must not be able to occupy a slot indefinitely.
pub const INBOUND_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of inbound KDE Connect connections handled concurrently.
pub const MAX_INBOUND_CONNECTIONS: usize = 24;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

use crate::config::KdeConnectConfig;
use crate::deviceinfo::{connection_packet_target, DeviceInfo, PROTOCOL_VERSION};
use crate::framing::{LineReader, PacketWriter};
use crate::packet::{NetworkPacket, MAX_IDENTITY_PACKET_SIZE};
use crate::tls::{TlsContext, TlsRole};
use crate::types::{KcDevice, LinkState, Transport};

pub const UDP_PORT: u16 = 1716;
pub const MIN_TCP_PORT: u16 = 1716;
pub const MAX_TCP_PORT: u16 = 1764;
pub const PAYLOAD_TRANSFER_MIN_PORT: u16 = 1739;
/// The KDE Connect mDNS service type.
///
/// Upstream KDE Connect writes this as the short form `_kdeconnect._udp`, but
/// `mdns-sd` (and RFC 6763 §4.1) require the fully qualified
/// `<type>.<domain>`. Both forms resolve to the same service on the wire: the
/// short form is expanded to `.local.` before anything is sent, so this stays
/// interoperable with kdeconnect-android and kdeconnect-kde.
pub const MDNS_SERVICE_TYPE: &str = "_kdeconnect._udp";
pub const MDNS_SERVICE_TYPE_FQDN: &str = "_kdeconnect._udp.local.";

const IDENTITY_BROADCAST_INTERVAL: Duration = Duration::from_secs(5);

/// Commands the application layer can push into a live device link.
///
/// Not `Debug`: `Adopt` carries live stream halves, which are deliberately not
/// formatted anywhere - a partially consumed TLS stream has no useful
/// representation and printing one risks leaking session material into a log.
pub enum LinkCommand {
    Send(Box<NetworkPacket>),
    /// Announce `packet` with a payload, then stream `payload` to the peer.
    SendWithPayload(Box<NetworkPacket>, tokio::fs::File),
    RequestPairing,
    AcceptPairing,
    CancelPairing,
    /// Send a typed reply to a repliable notification.
    ReplyToNotification {
        notification_id: String,
        reply_id: String,
        text: String,
    },
    /// Adopt a new socket for an already-established link.
    ///
    /// This is upstream's `LanDeviceLink::reset()`. A phone reconnects by
    /// dialling us; refusing that connection leaves the phone unable to recover
    /// until our side notices its old socket is dead, which it cannot do while
    /// that socket still looks open.
    ///
    /// The caller must already have verified the peer's certificate matches the
    /// one the established link was created with. Adopting a socket presenting a
    /// different certificate would let anything able to reach port 1716 take
    /// over a trusted session.
    Adopt {
        reader: LineReader<tokio::io::ReadHalf<crate::tls::TlsStream<tokio::net::TcpStream>>>,
        writer: PacketWriter<tokio::io::WriteHalf<crate::tls::TlsStream<tokio::net::TcpStream>>>,
    },
    Shutdown,
}

/// Events surfaced to the application layer.
#[derive(Debug, Clone)]
pub enum KcEvent {
    Device(Box<KcDevice>),
    Removed(String),
    PairingRequest {
        device: Box<KcDevice>,
        verification_key: String,
    },
    Clipboard {
        device_id: String,
        device_name: String,
        content: String,
    },
    ShareOffer(Box<crate::types::ShareOffer>),
    Notification {
        device_id: String,
        device_name: String,
        notification: Box<crate::types::RemoteNotification>,
    },
    /// A typed reply was delivered to the peer.
    NotificationReplied {
        device_id: String,
        notification_id: String,
    },
    Connectivity(Box<crate::types::ConnectivityReport>),
    Battery {
        device_id: String,
        level: u8,
        charging: bool,
    },
    TransferProgress(Box<crate::types::TransferProgress>),
    Log {
        level: String,
        message: String,
    },
}

/// Runtime-tunable behaviour shared with the link tasks.
#[derive(Default)]
pub struct RuntimeSettings {
    pub incoming_path: std::sync::Mutex<std::path::PathBuf>,
    pub clipboard_enabled: AtomicBool,
    pub auto_clipboard: AtomicBool,
}

impl RuntimeSettings {
    pub fn incoming_path(&self) -> std::path::PathBuf {
        self.incoming_path
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default()
    }
    pub fn set_incoming_path(&self, p: std::path::PathBuf) {
        if let Ok(mut g) = self.incoming_path.lock() {
            *g = p;
        }
    }
    pub fn clipboard_enabled(&self) -> bool {
        self.clipboard_enabled.load(Ordering::SeqCst)
    }
    pub fn auto_clipboard(&self) -> bool {
        self.auto_clipboard.load(Ordering::SeqCst)
    }
}

pub struct LanLinkProvider {
    /// Owned copies of this device's TLS material, kept for the provider's
    /// lifetime so tests and callers can inspect the identity without reaching
    /// into the config lock.
    cert_pem: Arc<str>,
    key_pem: Arc<str>,
    /// Mutable so a name change is advertised on the next discovery round.
    identity: Arc<ArcSwap<DeviceInfo>>,
    pub config: Arc<Mutex<KdeConnectConfig>>,
    pub tls: Arc<TlsContext>,
    pub events: broadcast::Sender<KcEvent>,
    pub settings: Arc<RuntimeSettings>,
    tcp_port: AtomicU16,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    shutdown: Arc<AtomicBool>,
    /// The one context every task shares.
    ///
    /// It used to be built fresh by `link_context()` on each call, which meant
    /// the outbound-dial guard, the link history, the notification store and
    /// the inbound-connection cap were *separate instances* per caller. The
    /// dial guard therefore guarded nothing - each dial checked and set its own
    /// private set - and the connection cap capped nothing at all, because the
    /// accept loop held a different semaphore from the one it was counting
    /// against.
    ///
    /// Building it once here is what makes those actually shared.
    ctx: Arc<LinkContext>,
}

impl LanLinkProvider {
    pub async fn new(
        config: Arc<Mutex<KdeConnectConfig>>,
        events: broadcast::Sender<KcEvent>,
    ) -> Result<Self> {
        let (info, tls_ctx, cert_pem, key_pem) = {
            let cfg = config.lock().await;
            let info = DeviceInfo {
                id: cfg.device_id().to_string(),
                name: cfg.device_name().to_string(),
                device_type: cfg.device_type(),
                protocol_version: PROTOCOL_VERSION,
                incoming_capabilities: capabilities_incoming(),
                outgoing_capabilities: capabilities_outgoing(),
            };
            let cert = cfg.certificate_pem().to_string();
            let key = cfg.identity().key_pem.clone();
            let tls_ctx = TlsContext::new(&cert, &key).context("build TLS context")?;
            (info, tls_ctx, Arc::<str>::from(cert), Arc::<str>::from(key))
        };

        let incoming = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let settings = RuntimeSettings {
            incoming_path: std::sync::Mutex::new(incoming),
            clipboard_enabled: AtomicBool::new(true),
            auto_clipboard: AtomicBool::new(true),
        };

        let settings = Arc::new(settings);
        let links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let devices: Arc<RwLock<HashMap<String, KcDevice>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let identity = Arc::new(ArcSwap::from_pointee(info));
        let tls = Arc::new(tls_ctx);

        // Built once and shared. See the note on the field.
        let ctx = Arc::new(LinkContext {
            config: config.clone(),
            tls: tls.clone(),
            identity: identity.clone(),
            tcp_port: 0,
            events: events.clone(),
            devices: devices.clone(),
            links: links.clone(),
            settings: settings.clone(),
            inbound_connections: Arc::new(Semaphore::new(MAX_INBOUND_CONNECTIONS)),
            link_history: Arc::new(Mutex::new(HashMap::new())),
            dialing: Arc::new(Mutex::new(HashSet::new())),
            notifications: Arc::new(Mutex::new(HashMap::new())),
        });

        Ok(Self {
            cert_pem,
            key_pem,
            tls,
            identity,
            events,
            settings,
            tcp_port: AtomicU16::new(0),
            config,
            links,
            devices,
            shutdown: Arc::new(AtomicBool::new(false)),
            ctx,
        })
    }

    pub fn tcp_port(&self) -> u16 {
        self.tcp_port.load(Ordering::SeqCst)
    }

    /// A frozen copy of the identity for the discovery loops.
    fn discovery_snapshot(&self) -> DeviceInfo {
        self.device_info()
    }

    /// The advertised identity.
    ///
    /// Deliberately lock-free: the identity only changes when the user renames
    /// this computer, so a lock-free snapshot avoids `blocking_read()` panics
    /// when this is called from inside an async runtime.
    pub fn device_info(&self) -> DeviceInfo {
        self.identity.load().as_ref().clone()
    }

    /// Update the advertised device identity (used by the settings screen).
    pub async fn set_device_name(&self, name: &str) {
        let updated = DeviceInfo {
            name: name.to_string(),
            ..self.device_info()
        };
        self.identity.store(Arc::new(updated));
        info!("KDE Connect identity is now advertising as {name:?}");
    }

    /// The device's own TLS certificate (PEM), used to pin the peer on trust.
    pub fn certificate_pem(&self) -> &str {
        &self.cert_pem
    }

    pub fn private_key_pem(&self) -> &str {
        &self.key_pem
    }

    pub async fn devices(&self) -> Vec<KcDevice> {
        self.devices.read().await.values().cloned().collect()
    }

    pub async fn device(&self, id: &str) -> Option<KcDevice> {
        self.devices.read().await.get(id).cloned()
    }

    pub async fn command(&self, device_id: &str, cmd: LinkCommand) -> Result<()> {
        let tx = self.links.lock().await.get(device_id).cloned();
        match tx {
            Some(tx) => tx
                .send(cmd)
                .await
                .map_err(|_| anyhow!("device {device_id} is no longer connected")),
            None => Err(anyhow!("device {device_id} is not connected")),
        }
    }

    pub async fn is_connected(&self, device_id: &str) -> bool {
        self.links.lock().await.contains_key(device_id)
    }

    /// True while an outbound dial to this peer is in flight.
    async fn is_dialing(&self, device_id: &str) -> bool {
        self.link_context().is_dialing(device_id).await
    }

    /// Reserve the single outbound dial slot; `false` if one is already in flight.
    async fn mark_dialing(&self, device_id: &str) -> bool {
        self.link_context().mark_dialing(device_id).await
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.events
            .send(KcEvent::Log {
                level: "info".into(),
                message: "KDE Connect LAN backend stopping".into(),
            })
            .ok();
    }

    // ---- lifecycle -------------------------------------------------------

    pub async fn run(&mut self) -> Result<()> {
        let listener = bind_tcp_listener(MIN_TCP_PORT, MAX_TCP_PORT).await?;
        let port = listener.local_addr()?.port();
        self.tcp_port.store(port, Ordering::SeqCst);
        crate::discovery::note_local_tcp_port(port);
        info!("KDE Connect listening on TCP port {port}");

        // The UDP discovery port is fixed by the protocol and is very likely
        // already held by another KDE Connect implementation on this machine
        // (a stock `kdeconnectd`, for example).
        //
        // Losing it entirely used to mean "mDNS only", which on this desktop
        // silently does not work: Quick Share links a *different* `mdns-sd`
        // implementation that also binds UDP 5353, and the two daemons split
        // the multicast traffic between them, so KDE Connect's browse saw no
        // replies at all and no phone was ever discovered.
        //
        // We only ever *receive* on this socket, so there is no reason to
        // refuse to share the port. SO_REUSEADDR plus SO_REUSEPORT lets us
        // bind it alongside the other daemon and receive the broadcast
        // announcements directly, which is what upstream KDE Connect relies
        // on primarily.
        let udp = match bind_shared_udp(UDP_PORT).await {
            Ok(sock) => {
                let _ = sock.set_broadcast(true);
                info!("UDP discovery socket bound on port {UDP_PORT} (shared)");
                Some(sock)
            }
            Err(e) => {
                warn!(
                    "UDP discovery port {UDP_PORT} is unavailable ({e}); continuing with mDNS-only discovery. \
                     Stop any other KDE Connect daemon to restore UDP discovery."
                );
                let _ = self.events.send(KcEvent::Log {
                    level: "warn".into(),
                    message: format!(
                        "UDP discovery port {UDP_PORT} is already in use, so Pixel Connect is \
                         discovering over mDNS only"
                    ),
                });
                None
            }
        };

        {
            let ctx = self.link_context();
            let shutdown = self.shutdown.clone();
            let events = self.events.clone();
            let devices = self.devices.clone();
            let links = self.links.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = sleep_until_cancelled(shutdown) => {}
                    r = accept_loop(listener, ctx.clone(), events, devices, links) => {
                        if let Err(e) = r { error!("KDE Connect accept loop stopped: {e}"); }
                    }
                }
            });
        }

        let udp_available = udp.is_some();
        if let Some(udp) = udp {
            let shutdown = self.shutdown.clone();
            let identity = Arc::new(self.discovery_snapshot());
            tokio::spawn(async move {
                tokio::select! {
                    _ = sleep_until_cancelled(shutdown) => {}
                    r = udp_discovery_loop(udp, identity) => {
                        if let Err(e) = r { error!("KDE Connect UDP discovery stopped: {e}"); }
                    }
                }
            });
        }

        // mDNS is KDE Connect's *secondary* discovery mechanism. It matters only when
        // the fixed UDP port could not be bound, which is now rare because
        // `bind_shared_udp` can share the port with another daemon.
        //
        // When UDP 1716 is available we deliberately skip mDNS. Two reasons:
        //
        //  1. It is redundant. Upstream treats the UDP broadcast as the primary
        //     mechanism; avahi is the fallback, not the other way round.
        //  2. It is actively harmful here. This process links Quick Share's
        //     forked `mdns-sd` alongside the crates.io one used above, and each
        //     `ServiceDaemon` binds UDP 5353 independently. Measured on this
        //     machine the application held 18 sockets on that port and the
        //     kernel split the multicast between them, starving Quick Share
        //     discovery completely - zero peers were ever found. Quick Share
        //     works correctly when run standalone, which is exactly the
        //     configuration this restores.
        //
        // If the UDP port is genuinely unavailable, fall back to mDNS so
        // discovery still works; the contention is then the lesser problem.
        if !udp_available {
            #[cfg(target_os = "linux")]
            {
                warn!("UDP discovery unavailable; falling back to mDNS discovery");
                let ctx = self.link_context();
                let shutdown = self.shutdown.clone();
                let identity = Arc::new(self.discovery_snapshot());
                tokio::spawn(async move {
                    tokio::select! {
                        _ = sleep_until_cancelled(shutdown) => {}
                        r = mdns_loop(ctx, identity) => {
                            if let Err(e) = r { error!("KDE Connect mDNS stopped: {e}"); }
                        }
                    }
                });
            }
        }

        Ok(())
    }

    pub(crate) fn link_context(&self) -> Arc<LinkContext> {
        self.ctx.clone()
    }

    #[allow(dead_code)]
    fn build_link_context(&self) -> Arc<LinkContext> {
        Arc::new(LinkContext {
            config: self.config.clone(),
            tls: self.tls.clone(),
            identity: self.identity.clone(),
            tcp_port: self.tcp_port(),
            events: self.events.clone(),
            devices: self.devices.clone(),
            links: self.links.clone(),
            settings: self.settings.clone(),
            inbound_connections: Arc::new(Semaphore::new(MAX_INBOUND_CONNECTIONS)),
            link_history: Arc::new(Mutex::new(HashMap::new())),
            dialing: Arc::new(Mutex::new(HashSet::new())),
            notifications: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn refresh_devices(&self) {
        // Drop peers that have gone quiet before deciding whom to dial. Without
        // this a peer seen once on a network that no longer exists is dialled on
        // every tick forever, which is how several hundred half-open connections
        // to a dead address accumulated.
        let dropped = crate::discovery::expire_stale();
        if dropped > 0 {
            debug!("dropped {dropped} discovered peer(s) that stopped announcing");
        }
        let peers = crate::discovery::snapshot();
        log::debug!("refresh_devices: {} discovered peer(s)", peers.len());
        if peers.is_empty() {
            return;
        }
        for peer in peers {
            self.connect_to_peer(&peer).await;
        }
    }

    /// Dial a discovered peer and, if it is trusted, open a device link.
    pub async fn connect_to_peer(&self, peer: &crate::discovery::DiscoveredPeer) {
        if peer.device_id == self.device_info().id {
            return;
        }
        if peer.protocol_version > 0 && peer.protocol_version < PROTOCOL_VERSION {
            let _ = self.events.send(KcEvent::Log {
                level: "warn".into(),
                message: format!(
                    "{} speaks KDE Connect protocol {}, which is older than the required {PROTOCOL_VERSION}",
                    peer.name, peer.protocol_version
                ),
            });
        }
        let addr = crate::discovery::target_addr(peer);
        let info = crate::discovery::to_device_info(peer);

        self.publish_discovered(&info).await;

        // Do not dial a peer that is already connected, and do not dial one we are
        // *about* to connect: the refresh tick runs every few seconds, and a
        // dial that takes a moment to complete would otherwise be started again
        // on the next tick, producing exactly the duplicate-link churn this
        // whole change exists to prevent.
        if self.is_connected(&info.id).await || self.is_dialing(&info.id).await {
            return;
        }
        if !self.mark_dialing(&info.id).await {
            return;
        }

        let trusted = self.config.lock().await.is_trusted(&info.id);
        let ctx = self.link_context();
        let reservation = ctx.clone();
        let peer_id = info.id.clone();
        tokio::spawn(async move {
            match TcpStream::connect(addr).await {
                Ok(socket) => {
                    let _ = socket.set_nodelay(true);
                    // Same as the inbound path: keepalive belongs on the socket,
                    // set before TLS wraps it.
                    #[cfg(unix)]
                    {
                        use std::os::fd::AsRawFd;
                        if let Err(e) =
                            crate::framing::configure_socket_keepalive(socket.as_raw_fd())
                        {
                            debug!("could not enable TCP keepalive on an outgoing link: {e}");
                        }
                    }
                    if let Err(e) = handle_outgoing(socket, info, addr, ctx).await {
                        debug!("KDE Connect outgoing link to {addr} failed: {e}");
                    }
                }
                Err(e) => debug!("KDE Connect could not reach {addr}: {e}"),
            }
            // Release the reservation whatever happened, so a failed dial does
            // not permanently block future attempts.
            reservation.clear_dialing(&peer_id).await;
            let _ = trusted;
        });
    }

    async fn publish_discovered(&self, info: &DeviceInfo) {
        let trusted = self.config.lock().await.is_trusted(&info.id);
        let mut devices = self.devices.write().await;
        let existing = devices.get(&info.id);
        let (pair_state, link_state, transport, address, capabilities, features, battery, charging) =
            match existing {
                Some(d) => (
                    d.pair_state,
                    d.link_state,
                    d.transport,
                    d.address.clone(),
                    if d.incoming_capabilities.is_empty() {
                        info.incoming_capabilities.clone()
                    } else {
                        d.incoming_capabilities.clone()
                    },
                    d.features.clone(),
                    d.battery_level,
                    d.battery_is_charging,
                ),
                None => (
                    if trusted {
                        crate::pairing::PairState::Paired
                    } else {
                        crate::pairing::PairState::NotPaired
                    },
                    LinkState::Disconnected,
                    Transport::None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                ),
            };

        let device = KcDevice {
            id: info.id.clone(),
            name: info.name.clone(),
            device_type: info.device_type,
            protocol_version: info.protocol_version,
            pair_state,
            link_state,
            transport,
            address,
            incoming_capabilities: capabilities,
            outgoing_capabilities: existing
                .map(|d| d.outgoing_capabilities.clone())
                .unwrap_or_default(),
            features,
            trusted,
            verification_key: None,
            protocol_error: existing.and_then(|d| d.protocol_error.clone()),
            last_seen: crate::config::now_secs(),
            battery_level: battery,
            battery_is_charging: charging,
        };
        devices.insert(info.id.clone(), device.clone());
        drop(devices);
        let _ = self.events.send(KcEvent::Device(Box::new(device)));
    }
}

/// State shared with every device link task.
pub struct LinkContext {
    pub config: Arc<Mutex<KdeConnectConfig>>,
    pub tls: Arc<TlsContext>,
    /// Live identity snapshot, shared so a rename takes effect immediately.
    identity: Arc<ArcSwap<DeviceInfo>>,
    pub tcp_port: u16,
    pub events: broadcast::Sender<KcEvent>,
    pub devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    pub links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
    pub settings: Arc<RuntimeSettings>,
    /// Caps how many inbound connections may be handled at once.
    ///
    /// Every KDE Connect peer on the LAN dials every port in the range, and an
    /// attacker can dial far more. Without a ceiling each connection costs a
    /// task, a socket and eventually a TLS handshake — which is asymmetric
    /// cryptography and therefore expensive — so an unbounded accept loop is a
    /// cheap denial of service against the desktop. The limit is generous
    /// because a busy network legitimately has several peers connecting.
    pub inbound_connections: Arc<Semaphore>,
    /// Peers whose link ended, and whether it had been working.
    ///
    /// A link that drops should be retried; one that never came up should not
    /// be retried in a tight loop, which is what turns a phone switching Wi-Fi
    /// off into a reconnect storm.
    link_history: Arc<Mutex<HashMap<String, LinkOutcome>>>,
    /// Peers with an outbound dial in progress.
    ///
    /// A dial takes long enough that the refresh tick can start another for the
    /// same peer, and two simultaneous links fight over the slot. Reserving
    /// before dialling closes that window.
    dialing: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Notifications received from a peer, newest first.
    ///
    /// Held here rather than on `DeviceLink` so they **survive a reconnect**.
    /// Per-link storage started empty on every new link, so when the phone sent
    /// `notification.request` after reconnecting it was answered with an empty
    /// list and the phone re-fetched everything - showing the same handful of
    /// notifications afresh each time, several times a minute.
    notifications: Arc<Mutex<HashMap<String, Vec<crate::types::RemoteNotification>>>>,
}

/// What happened when a link to a peer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// The link was established and worked for a while before ending.
    Dropped,
    /// The link ended almost immediately, so retrying is unlikely to help.
    Failed,
    /// The link was closed deliberately.
    Shutdown,
}

impl LinkContext {
    /// Record how a link ended, so reconnect decisions can use it.
    pub async fn report_link_end(&self, device_id: &str, ran_for: std::time::Duration) {
        // A link that ran for a while is worth retrying; one that died at once
        // is not. The threshold is generous because the first thing a new link
        // does is send its identity.
        let outcome = if ran_for > std::time::Duration::from_secs(10) {
            LinkOutcome::Dropped
        } else {
            LinkOutcome::Failed
        };
        self.link_history
            .lock()
            .await
            .insert(device_id.to_string(), outcome);
    }

    /// How a previous link to this peer ended, if there was one.
    pub async fn last_link_outcome(&self, device_id: &str) -> Option<LinkOutcome> {
        self.link_history.lock().await.get(device_id).copied()
    }

    /// Forget a peer's history, so a later failure is judged on its own.
    pub async fn clear_link_history(&self, device_id: &str) {
        self.link_history.lock().await.remove(device_id);
    }

    pub async fn is_dialing(&self, device_id: &str) -> bool {
        self.dialing.lock().await.contains(device_id)
    }

    /// Reserve the single outbound dial slot for this peer.
    ///
    /// Returns `false` when a dial is already in flight, so the caller stands
    /// down rather than starting a second connection.
    pub async fn mark_dialing(&self, device_id: &str) -> bool {
        self.dialing.lock().await.insert(device_id.to_string())
    }

    pub async fn clear_dialing(&self, device_id: &str) {
        self.dialing.lock().await.remove(device_id);
    }

    /// Store a notification for a peer, replacing any earlier one with the same
    /// id and bounding the list.
    pub async fn note_notification(
        &self,
        device_id: &str,
        notification: crate::types::RemoteNotification,
    ) {
        /// Bound the list: a chatty app must not grow memory without limit.
        const MAX_NOTIFICATIONS: usize = 100;

        let mut per_peer = self.notifications.lock().await;
        let list = per_peer.entry(device_id.to_string()).or_default();
        list.retain(|n| n.id != notification.id);
        list.insert(0, notification);
        list.truncate(MAX_NOTIFICATIONS);
    }

    /// Notifications held for a peer, oldest last.
    pub async fn notifications_for(
        &self,
        device_id: &str,
    ) -> Vec<crate::types::RemoteNotification> {
        self.notifications
            .lock()
            .await
            .get(device_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl LinkContext {
    /// The live identity of this computer, so a rename is picked up at once.
    pub fn device_info(&self) -> DeviceInfo {
        self.identity.load().as_ref().clone()
    }

    /// Publish the truth about a peer's link, derived from the live link map.
    ///
    /// `establish_incoming` and `handle_outgoing` used to set `Connecting`
    /// before the handshake and leave it there on every failure path, so a
    /// refused replacement socket or a failed dial flipped a healthy device to
    /// unreachable and never flipped it back - while its existing link kept
    /// delivering packets. The only honest states here come from the link map:
    /// a sender exists or it does not. Upstream derives reachability the same
    /// way (`Device::isReachable` is `!m_deviceLinks.isEmpty()`), separately
    /// from paired state.
    pub(crate) async fn republish_link_state(&self, peer_id: &str) {
        let connected = self.links.lock().await.contains_key(peer_id);
        let mut devices = self.devices.write().await;
        if let Some(d) = devices.get_mut(peer_id) {
            d.link_state = if connected {
                LinkState::Connected
            } else {
                LinkState::Disconnected
            };
            if !connected {
                d.transport = Transport::None;
            }
            let snapshot = d.clone();
            drop(devices);
            let _ = self.events.send(KcEvent::Device(Box::new(snapshot)));
        }
    }

    pub(crate) async fn publish_partial(&self, id: &str, f: impl FnOnce(&mut KcDevice)) {
        let mut devices = self.devices.write().await;
        if let Some(d) = devices.get_mut(id) {
            f(d);
            let snapshot = d.clone();
            drop(devices);
            let _ = self.events.send(KcEvent::Device(Box::new(snapshot)));
        }
    }
}

async fn sleep_until_cancelled(shutdown: Arc<AtomicBool>) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Reduce a device name to something usable as an mDNS host label.
fn sanitize_hostname(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() {
        "pixel-connect".to_string()
    } else {
        cleaned.chars().take(63).collect()
    }
}

/// Bind a UDP port that may already be held by another process.
///
/// KDE Connect's discovery port is a fixed protocol constant, so a stock
/// `kdeconnectd` on the same machine will usually have it first. Failing to
/// bind at all used to push discovery onto mDNS alone, which is unreliable
/// here, so the socket is opened for sharing instead. This only affects who
/// may *receive* on the port; nothing is sent to it except the standard
/// discovery broadcast.
async fn bind_shared_udp(port: u16) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    // SAFETY: `setsockopt` only reads well-formed option values; `libc`'s
    // `c_int` is the type the kernel expects for SO_REUSEPORT.
    let enabled: libc::c_int = 1;
    unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_REUSEPORT,
            &enabled as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
    socket.set_broadcast(true)?;
    socket.bind(&socket2::SockAddr::from(SocketAddr::from((
        [0, 0, 0, 0],
        port,
    ))))?;
    // `socket2` sockets start in blocking mode; tokio refuses to register a
    // blocking file descriptor on its reactor.
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

async fn bind_tcp_listener(min: u16, max: u16) -> Result<TcpListener> {
    let mut last_err = None;
    for port in min..=max {
        match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => return Ok(l),
            Err(e) => last_err = Some(e),
        }
    }
    Err(anyhow!(
        "no free TCP port in {min}..={max}: {}",
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}

async fn accept_loop(
    listener: TcpListener,
    ctx: Arc<LinkContext>,
    events: broadcast::Sender<KcEvent>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
) -> Result<()> {
    loop {
        let (socket, peer) = listener.accept().await?;
        let _ = socket.set_nodelay(true);

        // Refuse rather than queue when we are already at the ceiling. Waiting
        // on the permit here would let a flood of connections pile up as
        // waiting tasks, which is the very thing the limit exists to prevent.
        let permit = match ctx.inbound_connections.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                debug!("refusing a KDE Connect connection from {peer}: too many in flight");
                drop(socket);
                continue;
            }
        };

        let ctx = ctx.clone();
        let events = events.clone();
        let devices = devices.clone();
        let links = links.clone();
        tokio::spawn(async move {
            // The permit is held for the whole connection and released on drop.
            let _permit = permit;

            // The timeout covers **only** the handshake - identity packet, TLS,
            // and pinning. It must not wrap the link itself.
            //
            // It used to wrap `handle_incoming`, which returns only when the
            // link ends, because `run_link` is called from inside it. Every
            // inbound connection was therefore torn down at exactly 30 seconds
            // no matter what it was doing. Confirmed by measurement: a 30 s
            // `wait_for` around a sleeping task kills it at 30.0 s.
            //
            // The symptom looked like a network problem - the link died "after
            // about 30 seconds" on a hotspot *and* on a home network - which
            // sent the investigation after NAT timeouts rather than at a
            // constant sitting in this file.
            let handshake = tokio::time::timeout(INBOUND_HANDSHAKE_TIMEOUT, async {
                establish_incoming(
                    socket, peer, ctx.clone(), events.clone(), devices, links,
                )
                .await
            })
            .await;

            let link = match handshake {
                Err(_) => {
                    debug!(
                        "incoming KDE Connect connection from {peer} never finished \
                         the handshake; abandoning it"
                    );
                    return;
                }
                Ok(Err(e)) => {
                    debug!("incoming KDE Connect handshake from {peer} failed: {e}");
                    return;
                }
                Ok(Ok(Some(link))) => link,
                // The socket was adopted by a link that already exists.
                Ok(Ok(None)) => return,
            };

            // No timeout from here on: this is the link's lifetime.
            if let Err(e) = link.run().await {
                debug!("incoming KDE Connect link from {peer} ended: {e}");
            }
        });
    }
}

/// Complete the handshake only, then hand back a link ready to run.
///
/// Split from the link's lifetime on purpose: the caller applies
/// `INBOUND_HANDSHAKE_TIMEOUT` to *this* function, and running the link inside
/// it would put every connection's lifetime under that timeout.
async fn establish_incoming(
    socket: TcpStream,
    peer: SocketAddr,
    ctx: Arc<LinkContext>,
    events: broadcast::Sender<KcEvent>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
) -> Result<Option<crate::link::DeviceLink<tokio::net::TcpStream>>> {
    // `Ok(None)` means the socket was adopted by an already-established link,
    // so there is nothing new to run. `Err` is a genuine failure.
    let mut reader = LineReader::new(socket);
    let first = reader
        .read_packet(MAX_IDENTITY_PACKET_SIZE)
        .await
        .context("no identity packet on incoming connection")?;

    let (target_id, _target_version) = connection_packet_target(&first)
        .ok_or_else(|| anyhow!("first packet was not a connection identity"))?;

    if target_id != ctx.device_info().id {
        // Busy-LAN noise: every KDE Connect peer dials every port in range.
        // Not an error, and not a link - just drop it.
        return Err(anyhow!("connection was addressed to another device"));
    }

    let peer_id = first
        .get_str("deviceId")
        .ok_or_else(|| anyhow!("identity has no deviceId"))?;
    if !crate::deviceinfo::is_valid_device_id(&peer_id) {
        return Err(anyhow!("peer sent an invalid device id"));
    }
    let peer_name = first.get_str("deviceName").unwrap_or_default();
    let peer_version = first.body.get_i64("protocolVersion").unwrap_or(0);

    // A link already exists for this device.
    //
    // This is the socket-replacement path, not a duplicate to refuse: the peer
    // has reconnected and needs the new socket adopted, or it can never recover
    // while our end of the old one still looks healthy. Upstream does exactly
    // this in `LanLinkProvider::addLink`, calling `LanDeviceLink::reset()`.
    let existing = links.lock().await.get(&peer_id).cloned();

    let socket = reader.into_inner();
    let trusted = ctx.config.lock().await.is_trusted(&peer_id);
    // Deliberately no Connecting publish here. An earlier version set it before
    // the handshake and never restored it on failure, so every refused
    // replacement socket or failed handshake flipped a healthy device to
    // unreachable while its existing link kept delivering. The link's own
    // start publishes the state once it actually exists.

    // Enable TCP keepalive before TLS wraps the socket, matching upstream
    // `LanLinkProvider::configureSocket`. The kernel probes an idle link and
    // eventually reports a peer that has gone, which is what keeps a NAT
    // mapping open and what detects a silently disconnected phone. Guessing
    // from application-level silence does not work: an idle phone sends nothing.
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if let Err(e) = crate::framing::configure_socket_keepalive(socket.as_raw_fd()) {
            debug!("could not enable TCP keepalive on an incoming link: {e}");
        }
    }

    // TLS client on the accepted socket.
    let tls_stream = match ctx.tls.handshake(TlsRole::Client, socket).await {
        Ok(s) => s,
        Err(e) => {
            // The replacement socket failed; say so about the socket, and
            // republish whatever the link map actually holds - which may be a
            // live link this failure must not clobber.
            ctx.republish_link_state(&peer_id).await;
            return Err(e).context("TLS handshake (client role)");
        }
    };
    let _ = &peer_id;

    // A link already exists for this peer.
    //
    // Refuse the new connection rather than adopting it.
    //
    // Adoption was implemented to match upstream's LanLinkProvider::addLink,
    // and it does adopt - but measured on a Pixel 8 it does not converge: the
    // phone opens a fresh connection every 5 s, each is adopted, and closing the
    // previous socket is something the peer observes, so it reconnects again.
    // Upstream only ever adopts a socket the peer has *already* replaced, which
    // is a condition that has not been shown to hold here.
    //
    // Refusing is the prior, stable behaviour: no loop, and no leaked sockets
    // since the refused socket is closed immediately. Re-enable adoption only
    // once who initiates has been established from a capture.
    if existing.is_some() {
        // The existing link stays up, so the record must keep saying so.
        // Without this the pre-handshake state (or a stale Connecting from an
        // earlier attempt) survives the refusal and the UI reports a device
        // with a live link as unreachable.
        ctx.republish_link_state(&peer_id).await;
        return Err(anyhow!(
            "a link to {peer_id} already exists; refusing the replacement socket"
        ));
    }

    // Return the link rather than running it: the accept loop bounds the
    // handshake with a timeout and must not bound the link's lifetime.
    crate::link::make_link(
        tls_stream,
        peer_id,
        peer_name,
        peer_version,
        peer,
        ctx,
        devices,
        links,
        events,
        trusted,
    )
    .await
    .map(Some)
}

/// We opened the TCP connection, so we must play the TLS **server**.
async fn handle_outgoing(
    socket: TcpStream,
    info: DeviceInfo,
    peer_addr: SocketAddr,
    ctx: Arc<LinkContext>,
) -> Result<()> {
    ctx.publish_partial(&info.id, |d| {
        d.link_state = LinkState::Connecting;
        d.transport = Transport::Lan;
        d.address = Some(peer_addr.to_string());
    })
    .await;

    let mut writer = PacketWriter::new(socket);
    if let Err(e) = writer
        .write_packet(
            &ctx.device_info()
                .to_connection_packet(&info.id, info.protocol_version),
        )
        .await
    {
        // The dial failed; republish whatever the link map holds rather than
        // leaving the Connecting set above stuck on a dead attempt.
        ctx.republish_link_state(&info.id).await;
        return Err(e).context("send connection identity");
    }
    let socket = writer.into_inner();

    // Pinning is applied by `run_link`, which fetches the stored certificate
    // for this peer and rejects the handshake if it does not match — before the
    // link is published. Deliberately not fetched here: holding a local
    // variable that is immediately discarded invites a reader to believe this
    // path is unpinned, when the enforcement is one call away and unconditional.
    let tls_stream = match ctx.tls.handshake(TlsRole::Server, socket).await {
        Ok(s) => s,
        Err(e) => {
            ctx.republish_link_state(&info.id).await;
            return Err(e).context("TLS handshake (server role)");
        }
    };

    let trusted = ctx.config.lock().await.is_trusted(&info.id);
    let devices = ctx.devices.clone();
    let links = ctx.links.clone();
    let events = ctx.events.clone();
    let name = info.name.clone();
    let version = info.protocol_version;
    let id = info.id.clone();

    // We dialled out, so there is no accept-loop timeout involved: run the link
    // to completion here. Only the *inbound* path needs the handshake split,
    // because only there is a timeout in scope.
    crate::link::run_link(
        tls_stream, id, name, version, peer_addr, ctx, devices, links, events, trusted,
    )
    .await
}

/// Capabilities this implementation is willing to *receive*.
pub fn capabilities_incoming() -> Vec<String> {
    use crate::packet as p;
    vec![
        p::PACKET_TYPE_IDENTITY.to_string(),
        p::PACKET_TYPE_PAIR.to_string(),
        p::PACKET_TYPE_PING.to_string(),
        p::PACKET_TYPE_BATTERY.to_string(),
        p::PACKET_TYPE_CLIPBOARD.to_string(),
        p::PACKET_TYPE_CLIPBOARD_CONNECT.to_string(),
        p::PACKET_TYPE_SHARE_REQUEST.to_string(),
        p::PACKET_TYPE_CONNECTIVITY_REPORT.to_string(),
        p::PACKET_TYPE_NOTIFICATION.to_string(),
        p::PACKET_TYPE_NOTIFICATION_REQUEST.to_string(),
        p::PACKET_TYPE_NOTIFICATION_ACTION.to_string(),
        p::PACKET_TYPE_NOTIFICATION_REPLY.to_string(),
    ]
}

/// Capabilities this implementation is willing to *send*.
pub fn capabilities_outgoing() -> Vec<String> {
    use crate::packet as p;
    vec![
        p::PACKET_TYPE_PING.to_string(),
        p::PACKET_TYPE_CLIPBOARD.to_string(),
        p::PACKET_TYPE_CLIPBOARD_CONNECT.to_string(),
        p::PACKET_TYPE_SHARE_REQUEST.to_string(),
        p::PACKET_TYPE_SHARE_REQUEST_UPDATE.to_string(),
        p::PACKET_TYPE_PAIR.to_string(),
        p::PACKET_TYPE_NOTIFICATION.to_string(),
        p::PACKET_TYPE_NOTIFICATION_REQUEST.to_string(),
    ]
}

async fn udp_discovery_loop(socket: UdpSocket, identity: Arc<DeviceInfo>) -> Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut ticker = tokio::time::interval(IDENTITY_BROADCAST_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let local_port = crate::discovery::local_tcp_port();
                let packet = identity.to_discovery_packet(local_port).serialize();
                for addr in crate::discovery::broadcast_addresses() {
                    if let Err(e) = socket.send_to(&packet, (addr, UDP_PORT)).await {
                        debug!("KDE Connect UDP broadcast to {addr} failed: {e}");
                    }
                }
            }
            r = socket.recv_from(&mut buf) => {
                let (n, from) = match r {
                    Ok(v) => v,
                    Err(e) => { debug!("KDE Connect UDP receive error: {e}"); continue; }
                };
                if let Ok(np) = NetworkPacket::unserialize(&buf[..n]) {
                    crate::discovery::note_udp_peer(from.ip(), &np);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
async fn mdns_loop(_ctx: Arc<LinkContext>, identity: Arc<DeviceInfo>) -> Result<()> {
    use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

    let device_info = identity.as_ref().clone();

    // KDE Connect announces `_kdeconnect._udp` with the instance name set to
    // the device id and TXT records id/name/type/protocol
    // (kdeconnect-kde `core/backends/lan/avahidiscovery.cpp`).
    let daemon = ServiceDaemon::new()?;
    let properties: HashMap<String, String> = HashMap::from([
        ("id".to_string(), device_info.id.clone()),
        ("name".to_string(), device_info.name.clone()),
        (
            "type".to_string(),
            device_info.device_type.as_str().to_string(),
        ),
        ("protocol".to_string(), PROTOCOL_VERSION.to_string()),
    ]);
    // `mdns-sd` validates both the service type and the hostname, so both need
    // the fully qualified form even though KDE Connect only ever compares the
    // instance name and the TXT records.
    let host_name = format!("{}.local.", sanitize_hostname(&device_info.name));
    let service = ServiceInfo::new(
        MDNS_SERVICE_TYPE_FQDN,
        &device_info.id,
        &host_name,
        // Deliberately empty: `mdns-sd` only fills in addresses from the host
        // when none were supplied *and* `addr_auto` is set (see
        // `ServiceDaemon::register`). Naming a single address here is what
        // broke phone-hotspot use: with a VPN or a second interface up, the
        // chosen address is often not the one the phone can reach, so the
        // phone was told to dial an address on a network it was not on and
        // discovery silently never happened.
        "",
        crate::discovery::local_tcp_port(),
        properties,
    )?
    .enable_addr_auto();

    daemon.register(service)?;

    let events = daemon.browse(MDNS_SERVICE_TYPE_FQDN)?;
    loop {
        // `flume::Receiver::recv_timeout` blocks the calling thread. On a
        // single-threaded runtime that starves every other task, so the wait
        // happens on `spawn_blocking`.
        let next = match tokio::task::spawn_blocking({
            let events = events.clone();
            move || events.recv_timeout(Duration::from_millis(500))
        })
        .await
        {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        match next {
            Ok(ServiceEvent::ServiceFound(_, full)) => {
                debug!("mDNS: found {full}");
            }
            Ok(ServiceEvent::ServiceResolved(info)) => {
                debug!("mDNS: resolved {}", info.get_fullname());
                // TXT records are `id`, `name`, `type`, `protocol`.
                let get = |k: &str| info.get_property_val_str(k).map(|s| s.to_string());
                let txt: HashMap<String, String> = [
                    ("id", get("id")),
                    ("name", get("name")),
                    ("type", get("type")),
                    ("protocol", get("protocol")),
                ]
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| (k.to_string(), v)))
                .collect();
                let address = info
                    .get_addresses()
                    .iter()
                    .find(|a| a.is_ipv4())
                    .copied()
                    .or_else(|| info.get_addresses().iter().next().copied());
                crate::discovery::note_mdns_peer(
                    address,
                    info.get_port(),
                    info.get_fullname(),
                    &txt,
                );
            }
            Ok(ServiceEvent::SearchStarted(_)) => {
                debug!("mDNS: browsing for {MDNS_SERVICE_TYPE}");
            }
            Ok(_) => {}
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => {
                debug!("mDNS daemon went away");
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_lists_are_disjoint_where_the_protocol_requires_it() {
        let incoming = capabilities_incoming();
        let outgoing = capabilities_outgoing();
        assert!(incoming.contains(&"kdeconnect.clipboard".to_string()));
        // Upstream only *sends* share.request.update, never receives it.
        assert!(!incoming.contains(&"kdeconnect.share.request.update".to_string()));
        assert!(outgoing.contains(&"kdeconnect.share.request.update".to_string()));
        // ... and the reverse for connectivity reports.
        assert!(!outgoing.contains(&"kdeconnect.connectivity_report".to_string()));
    }

    #[tokio::test]
    async fn binds_inside_the_kdeconnect_port_range() {
        let l = bind_tcp_listener(MIN_TCP_PORT, MAX_TCP_PORT).await.unwrap();
        let p = l.local_addr().unwrap().port();
        assert!((MIN_TCP_PORT..=MAX_TCP_PORT).contains(&p));
    }

    /// Replay harness for the connection-state model: a device record plus a
    /// link map, exercising exactly what `republish_link_state` derives.
    mod link_state_replay {
        use super::*;
        use crate::deviceinfo::DeviceType;
        use crate::pairing::PairState;

        fn test_ctx() -> Arc<LinkContext> {
            let dir = std::env::temp_dir().join(format!(
                "pixel-connect-linkstate-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let config = crate::config::KdeConnectConfig::load(&dir).unwrap();
            let ident = crate::crypto::generate_identity("local-test-id", None).unwrap();
            let tls = crate::tls::TlsContext::new(&ident.cert_pem, &ident.key_pem).unwrap();
            let info = crate::deviceinfo::DeviceInfo {
                id: "local-test-id".into(),
                name: "test".into(),
                device_type: DeviceType::Desktop,
                protocol_version: 8,
                incoming_capabilities: vec![],
                outgoing_capabilities: vec![],
            };
            Arc::new(LinkContext {
                config: Arc::new(Mutex::new(config)),
                tls: Arc::new(tls),
                identity: Arc::new(arc_swap::ArcSwap::from_pointee(info)),
                tcp_port: 0,
                events: broadcast::channel(8).0,
                devices: Arc::new(RwLock::new(HashMap::new())),
                links: Arc::new(Mutex::new(HashMap::new())),
                settings: Arc::new(RuntimeSettings {
                    incoming_path: std::sync::Mutex::new(std::path::PathBuf::new()),
                    clipboard_enabled: AtomicBool::new(true),
                    auto_clipboard: AtomicBool::new(true),
                }),
                inbound_connections: Arc::new(Semaphore::new(MAX_INBOUND_CONNECTIONS)),
                link_history: Arc::new(Mutex::new(HashMap::new())),
                dialing: Arc::new(Mutex::new(HashSet::new())),
                notifications: Arc::new(Mutex::new(HashMap::new())),
            })
        }

        fn record(id: &str, state: LinkState) -> KcDevice {
            KcDevice {
                id: id.into(),
                name: "Pixel 8".into(),
                device_type: DeviceType::Phone,
                protocol_version: 8,
                pair_state: PairState::Paired,
                link_state: state,
                transport: Transport::Lan,
                address: Some("192.168.1.2:1716".into()),
                incoming_capabilities: vec![],
                outgoing_capabilities: vec![],
                features: vec![],
                trusted: true,
                verification_key: None,
                protocol_error: None,
                last_seen: 0,
                battery_level: None,
                battery_is_charging: None,
            }
        }

        async fn claim_link(ctx: &LinkContext, id: &str) {
            let (tx, _rx) = mpsc::channel::<LinkCommand>(1);
            ctx.links.lock().await.insert(id.into(), tx);
        }

        fn state_of(ctx: &LinkContext, id: &str) -> Option<LinkState> {
            ctx.devices.try_read().ok()?.get(id).map(|d| d.link_state)
        }

        /// The reported bug: a refused replacement socket left a healthy
        /// device unreachable. Republish with a live link must say Connected.
        #[tokio::test]
        async fn a_live_link_republishes_to_connected() {
            let ctx = test_ctx();
            ctx.devices.write().await.insert("p".into(), record("p", LinkState::Connecting));
            claim_link(&ctx, "p").await;
            ctx.republish_link_state("p").await;
            assert_eq!(state_of(&ctx, "p"), Some(LinkState::Connected));
        }

        /// The same call with no link must say Disconnected, and drop the
        /// stale transport rather than claiming a route that is gone.
        #[tokio::test]
        async fn no_link_republishes_to_disconnected() {
            let ctx = test_ctx();
            ctx.devices.write().await.insert("p".into(), record("p", LinkState::Connecting));
            ctx.republish_link_state("p").await;
            assert_eq!(state_of(&ctx, "p"), Some(LinkState::Disconnected));
            let t = ctx.devices.read().await.get("p").map(|d| d.transport);
            assert_eq!(t, Some(Transport::None));
        }

        /// Full flap replay: connect, failed replacement (refused), link end.
        /// The middle step is what used to stick the UI on unreachable.
        #[tokio::test]
        async fn flap_connect_refuse_close_tracks_the_link_map() {
            let ctx = test_ctx();
            ctx.devices.write().await.insert("p".into(), record("p", LinkState::Disconnected));
            // Link established.
            claim_link(&ctx, "p").await;
            ctx.republish_link_state("p").await;
            assert_eq!(state_of(&ctx, "p"), Some(LinkState::Connected));
            // Replacement refused while the link is live: still Connected.
            ctx.republish_link_state("p").await;
            assert_eq!(state_of(&ctx, "p"), Some(LinkState::Connected));
            // Link really ends: now Disconnected.
            ctx.links.lock().await.remove("p");
            ctx.republish_link_state("p").await;
            assert_eq!(state_of(&ctx, "p"), Some(LinkState::Disconnected));
        }

        /// Unknown peers are left alone: no record is invented for them.
        #[tokio::test]
        async fn unknown_peers_gain_no_record() {
            let ctx = test_ctx();
            ctx.republish_link_state("ghost").await;
            assert!(ctx.devices.read().await.get("ghost").is_none());
        }
    }
}
