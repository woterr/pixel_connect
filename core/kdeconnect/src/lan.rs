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
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::Semaphore;

/// How long an inbound connection has to identify itself and complete TLS
/// before it is abandoned. A peer that opens a socket and then says nothing
/// must not be able to occupy a slot indefinitely.
const INBOUND_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

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
#[derive(Debug)]
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

        Ok(Self {
            cert_pem,
            key_pem,
            tls: Arc::new(tls_ctx),
            identity: Arc::new(ArcSwap::from_pointee(info)),
            events,
            settings: Arc::new(settings),
            tcp_port: AtomicU16::new(0),
            config,
            links: Arc::new(Mutex::new(HashMap::new())),
            devices: Arc::new(RwLock::new(HashMap::new())),
            shutdown: Arc::new(AtomicBool::new(false)),
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

        // The UDP discovery port is fixed by the protocol and may already be
        // held by another KDE Connect implementation on this machine. That
        // must not stop Pixel Connect: we simply cannot *discover* peers by
        // broadcast, and peers will still find *us* through mDNS, which does
        // not need a privileged fixed port.
        let udp = match UdpSocket::bind(("0.0.0.0", UDP_PORT)).await {
            Ok(sock) => {
                let _ = sock.set_broadcast(true);
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

        #[cfg(target_os = "linux")]
        {
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

        Ok(())
    }

    pub(crate) fn link_context(&self) -> Arc<LinkContext> {
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
        })
    }

    pub async fn refresh_devices(&self) {
        let peers = crate::discovery::snapshot();
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

        if self.is_connected(&info.id).await {
            return;
        }

        let trusted = self.config.lock().await.is_trusted(&info.id);
        let ctx = self.link_context();
        tokio::spawn(async move {
            match TcpStream::connect(addr).await {
                Ok(socket) => {
                    let _ = socket.set_nodelay(true);
                    if let Err(e) = handle_outgoing(socket, info, addr, ctx).await {
                        debug!("KDE Connect outgoing link to {addr} failed: {e}");
                    }
                }
                Err(e) => debug!("KDE Connect could not reach {addr}: {e}"),
            }
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
}

impl LinkContext {
    /// The live identity of this computer, so a rename is picked up at once.
    pub fn device_info(&self) -> DeviceInfo {
        self.identity.load().as_ref().clone()
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
            let result = tokio::time::timeout(
                INBOUND_HANDSHAKE_TIMEOUT,
                handle_incoming(socket, peer, ctx.clone(), events.clone(), devices, links),
            )
            .await
            .unwrap_or_else(|_| Err(anyhow!("timed out establishing the link")));
            if let Err(e) = result {
                debug!("incoming KDE Connect connection from {peer} ended: {e}");
            }
        });
    }
}

/// We accepted the TCP connection, so we must play the TLS **client**.
async fn handle_incoming(
    socket: TcpStream,
    peer: SocketAddr,
    ctx: Arc<LinkContext>,
    events: broadcast::Sender<KcEvent>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
) -> Result<()> {
    let mut reader = LineReader::new(socket);
    let first = reader
        .read_packet(MAX_IDENTITY_PACKET_SIZE)
        .await
        .context("no identity packet on incoming connection")?;

    let (target_id, _target_version) = connection_packet_target(&first)
        .ok_or_else(|| anyhow!("first packet was not a connection identity"))?;

    if target_id != ctx.device_info().id {
        // Busy-LAN noise: every KDE Connect peer dials every port in range.
        return Ok(());
    }

    let peer_id = first
        .get_str("deviceId")
        .ok_or_else(|| anyhow!("identity has no deviceId"))?;
    if !crate::deviceinfo::is_valid_device_id(&peer_id) {
        return Err(anyhow!("peer sent an invalid device id"));
    }
    let peer_name = first.get_str("deviceName").unwrap_or_default();
    let peer_version = first.body.get_i64("protocolVersion").unwrap_or(0);

    if links.lock().await.contains_key(&peer_id) {
        // A link already exists for this device; KDE Connect keeps only one.
        return Ok(());
    }

    let socket = reader.into_inner();
    let trusted = ctx.config.lock().await.is_trusted(&peer_id);
    if trusted {
        ctx.publish_partial(&peer_id, |d| {
            d.link_state = LinkState::Connecting;
            d.transport = Transport::Lan;
            d.address = Some(peer.to_string());
            d.trusted = true;
        })
        .await;
    }

    // TLS client on the accepted socket.
    let tls_stream = ctx
        .tls
        .handshake(TlsRole::Client, socket)
        .await
        .context("TLS handshake (client role)")?;
    let _ = &peer_id;

    crate::link::run_link(
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
    writer
        .write_packet(
            &ctx.device_info()
                .to_connection_packet(&info.id, info.protocol_version),
        )
        .await
        .context("send connection identity")?;
    let socket = writer.into_inner();

    // Pinning is applied by `run_link`, which fetches the stored certificate
    // for this peer and rejects the handshake if it does not match — before the
    // link is published. Deliberately not fetched here: holding a local
    // variable that is immediately discarded invites a reader to believe this
    // path is unpinned, when the enforcement is one call away and unconditional.
    let tls_stream = ctx
        .tls
        .handshake(TlsRole::Server, socket)
        .await
        .context("TLS handshake (server role)")?;

    let trusted = ctx.config.lock().await.is_trusted(&info.id);
    let devices = ctx.devices.clone();
    let links = ctx.links.clone();
    let events = ctx.events.clone();
    let name = info.name.clone();
    let version = info.protocol_version;
    let id = info.id.clone();

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
        crate::discovery::primary_local_ipv4().to_string(),
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
}
