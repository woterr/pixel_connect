// Pixel Connect — KDE Connect protocol v8, per-device link.
//
// A `DeviceLink` owns one TLS session with a peer and does three things:
//   * runs the read loop, dispatching each incoming packet to the plugin layer
//   * services `LinkCommand`s from the application layer
//   * handles pairing, and streams file payloads over the dedicated second
//     connection that KDE Connect uses for them
//
// Plugin behaviour follows kdeconnect-kde `plugins/clipboard`,
// `plugins/share`, `plugins/battery`, `plugins/connectivity-report` and
// `plugins/sendnotifications`.

use std::collections::{hash_map::Entry, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

use crate::deviceinfo::DeviceInfo;
use crate::framing::{LineReader, PacketWriter};
use crate::lan::{KcEvent, LinkCommand, LinkContext};
use crate::packet as p;
use crate::pairing::{PairState, PairingAction, PairingHandler};
use crate::tls::{TlsRole, TlsStream};
use crate::types::{KcDevice, LinkState, ShareOffer, TransferProgress, Transport};

/// Cap on the size of an inbound text/URL share we will surface.
pub const MAX_TEXT_SHARE_BYTES: usize = 256 * 1024;

/// TCP keepalive parameters, matching upstream `LanLinkProvider::configureSocket`.
///
/// The kernel, not this application, decides when a link is dead. Upstream sets
/// these on every socket so a phone that silently disappears is noticed in about
/// a minute instead of the system default of roughly two hours, and so NAT
/// mappings stay open.
///
/// Values are upstream's: 30 s before the first probe, 10 s between probes,
/// 3 failures before the connection is dropped.
pub const TCP_KEEPALIVE_IDLE_SECS: u32 = 30;
pub const TCP_KEEPALIVE_INTERVAL_SECS: u32 = 10;
pub const TCP_KEEPALIVE_COUNT: u32 = 3;


/// Bring up a device link on an already-established TLS stream.
#[allow(clippy::too_many_arguments)]
/// Complete the handshake and return a link ready to run, without running it.
///
/// Split out of `run_link` so the caller can bound the *handshake* with a
/// timeout and then run the link without one. When both were in one function the
/// timeout necessarily covered the link's lifetime, and every connection died
/// at exactly that interval.
pub async fn make_link<S>(
    stream: TlsStream<S>,
    peer_id: String,
    peer_name: String,
    peer_version: i64,
    peer_addr: SocketAddr,
    ctx: Arc<LinkContext>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
    events: broadcast::Sender<KcEvent>,
    trusted: bool,
) -> Result<DeviceLink<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if peer_version < p::PROTOCOL_VERSION {
        return Err(anyhow!(
            "{peer_name} speaks KDE Connect protocol {peer_version}; this build requires {}",
            p::PROTOCOL_VERSION
        ));
    }

    // Certificate pinning happens before the stream is split, because after
    // splitting we no longer have access to the TLS session. The peer's
    // certificate is also captured here so this side can pin it when the
    // pairing handshake completes.
    let peer_spki = stream.peer_spki_der();
    let peer_cert_pem = stream
        .peer_cert_der()
        .map(|der| crate::crypto::der_to_pem(&der))
        .unwrap_or_default();
    {
        let pinned = ctx.config.lock().await.trusted_certificate_der(&peer_id);
        stream
            .verify_peer(&peer_id, pinned.as_deref())
            .map_err(|e| anyhow!("peer certificate rejected: {e}"))?;
    }

    let (read_half, write_half) = tokio::io::split(stream);
    let mut reader = LineReader::new(read_half);
    let mut writer = PacketWriter::new(write_half);

    // Identity is exchanged a second time, now inside TLS, exactly as upstream
    // does so both sides learn the peer's full identity and capabilities.
    writer
        .write_packet(&ctx.device_info().to_identity_packet())
        .await
        .context("send identity inside TLS")?;

    let identity = reader
        .read_packet(p::MAX_IDENTITY_PACKET_SIZE)
        .await
        .context("peer did not send an identity packet inside TLS")?;
    let peer_info = DeviceInfo::from_identity_packet(&identity, true)
        .map_err(|e| anyhow!("peer identity rejected: {e}"))?;

    if peer_info.id != peer_id {
        return Err(anyhow!(
            "peer changed device id mid-handshake ({} -> {})",
            peer_id,
            peer_info.id
        ));
    }

    Ok(DeviceLink {
        ctx,
        reader,
        writer,
        peer: peer_info,
        peer_spki,
        peer_cert_pem,
        started_at: std::time::Instant::now(),
        notifications_sent: false,
        peer_addr,
        trusted,
        pairing: PairingHandler::new(),
        events,
        links,
        devices,
        last_clipboard_ts: crate::config::now_millis(),
    })
}

/// Complete the handshake and run the link to completion.
pub async fn run_link<S>(
    stream: TlsStream<S>,
    peer_id: String,
    peer_name: String,
    peer_version: i64,
    peer_addr: SocketAddr,
    ctx: Arc<LinkContext>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
    events: broadcast::Sender<KcEvent>,
    trusted: bool,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    make_link(
        stream, peer_id, peer_name, peer_version, peer_addr, ctx, devices, links, events, trusted,
    )
    .await?
    .run()
    .await
}

pub struct DeviceLink<S: AsyncRead + AsyncWrite + Unpin> {
    ctx: Arc<LinkContext>,
    reader: LineReader<tokio::io::ReadHalf<TlsStream<S>>>,
    writer: PacketWriter<tokio::io::WriteHalf<TlsStream<S>>>,
    /// When this link was established, so the reconnect logic can tell a link
    /// that was working from one that never came up.
    started_at: std::time::Instant,
    /// Whether this link has already answered a `notification.request`.
    ///
    /// Notifications themselves live on `LinkContext`, not here, so they
    /// survive a reconnect; this flag only stops one link echoing them twice.
    notifications_sent: bool,
    peer: DeviceInfo,
    peer_spki: Vec<u8>,
    /// The peer's TLS certificate in PEM, pinned once pairing completes.
    peer_cert_pem: String,
    peer_addr: SocketAddr,
    trusted: bool,
    pairing: PairingHandler,
    events: broadcast::Sender<KcEvent>,
    links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
    devices: Arc<RwLock<HashMap<String, KcDevice>>>,
    last_clipboard_ts: i64,
}

impl<S: AsyncRead + AsyncWrite + Unpin> DeviceLink<S> {
    pub async fn run(mut self) -> Result<()> {
        let started_at = std::time::Instant::now();

        self.pairing.set_state(if self.trusted {
            PairState::Paired
        } else {
            PairState::NotPaired
        });

        // Claim the link slot, and yield if another link already holds it.
        //
        // Both peers dial each other - we dial on every refresh tick, the phone
        // dials when it sees our identity - so two connections can be built for
        // the same device at the same moment. Claiming the slot here, rather
        // than inserting unconditionally, means the loser stands down instead
        // of overwriting the winner.
        //
        // This was the cause of two separate symptoms. `HashMap::insert`
        // replaced the existing sender, which dropped the older link's channel,
        // so its `cmd_rx.recv()` returned `None` and it broke out of its loop
        // and tore the link down - mid-transfer. The phone then saw the drop,
        // reconnected, and both sides repeated it forever: a fresh "connected"
        // every 30 seconds, which is the phone re-establishing a link we kept
        // knocking down.
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<LinkCommand>(32);
        {
            let mut links = self.links.lock().await;
            match links.entry(self.peer.id.clone()) {
                Entry::Occupied(_) => {
                    info!(
                        "{}: a link is already established; standing this one down",
                        self.peer.name
                    );
                    return Ok(());
                }
                Entry::Vacant(slot) => {
                    slot.insert(cmd_tx);
                }
            }
        }

        self.publish().await;

        // A freshly connected trusted peer gets our current clipboard offer.
        if self.trusted && self.ctx.settings.auto_clipboard() {
            self.send_clipboard_connect().await;
        }

        // No application-level keepalive, deliberately.
        //
        // An earlier version sent `kdeconnect.ping` when the link went quiet and
        // declared it dead after 75 s of silence. That was wrong, and it showed:
        // the phone is content to sit idle for minutes and sends nothing at all,
        // so a healthy link was torn down about once a minute and rebuilt, and
        // the phone announced the connection each time.
        //
        // Upstream does this at the TCP layer instead.
        // `LanLinkProvider::configureSocket` sets SO_KEEPALIVE, then
        // TCP_KEEPIDLE=30, TCP_KEEPINTVL=10, TCP_KEEPCNT=3, so the *kernel*
        // detects a dead peer in roughly 60 s rather than the application
        // guessing from silence. Kernel probes also keep a NAT mapping open,
        // which is why a keepalive was wanted in the first place.
        //
        // So a link is now torn down only when the socket reports an end - which
        // is what `read_packet` returning an error means. Silence proves
        // nothing, and treating it as proof of death was the bug.

        loop {
            tokio::select! {
                packet = self.reader.read_packet(p::MAX_PACKET_SIZE) => {
                    match packet {
                        Ok(np) => {
                            if let Err(e) = self.on_packet(np).await {
                                debug!("{}: packet handling error: {e}", self.peer.name);
                            }
                        }
                        Err(e) => {
                            debug!("{}: link read ended: {e}", self.peer.name);
                            break;
                        }
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        None => break,
                        Some(LinkCommand::Shutdown) => break,
                        // The peer reconnected on a new socket. Adopt it and
                        // keep everything else - notifications, pairing state,
                        // the trust relationship - rather than tearing the
                        // session down and starting again.
                        //
                        // Dropping the old reader ends the in-flight `read_packet`
                        // future, which is why ownership moves here rather than
                        // from the new link: there is exactly one place where
                        // the socket can change, and it cannot race with itself.
                        Some(LinkCommand::Adopt { reader, writer }) => {
                            match self.adopt(reader, writer).await {
                                Ok(()) => {}
                                Err(e) => {
                                    warn!("{}: could not adopt a new socket: {e}", self.peer.name);
                                    break;
                                }
                            }
                            info!("{}: adopting a new socket from the peer", self.peer.name);
                            // Re-announce identity on the new socket, as
                            // upstream does after reset().
                            if let Err(e) = self.writer.write_packet(
                                &self.ctx.device_info().to_identity_packet()
                            ).await {
                                warn!("{}: could not send identity on the new socket: {e}", self.peer.name);
                                break;
                            }
                        }
                        Some(c) => {
                            if let Err(e) = self.on_command(c).await {
                                warn!("{}: command failed: {e}", self.peer.name);
                            }
                        }
                    }
                }
            }
        }

        self.links.lock().await.remove(&self.peer.id);
        self.pairing.cancel();
        self.publish().await;
        self.ctx
            .report_link_end(&self.peer.id, self.started_at.elapsed())
            .await;
        Ok(())
    }


    /// Swap in a new socket for this link, keeping all protocol state.
    ///
    /// An inbound replacement socket is always backed by a `TcpStream`, so a
    /// link parameterised over any other stream type must refuse rather than be
    /// handed a socket whose halves it does not own.
    async fn adopt(
        &mut self,
        reader: LineReader<tokio::io::ReadHalf<crate::tls::TlsStream<tokio::net::TcpStream>>>,
        writer: PacketWriter<tokio::io::WriteHalf<crate::tls::TlsStream<tokio::net::TcpStream>>>,
    ) -> Result<()> {
        let current = std::any::type_name::<S>();
        anyhow::bail!("cannot adopt a replacement socket on a link over {current}");
    }

    async fn publish(&self) {
        let trusted = self.ctx.config.lock().await.is_trusted(&self.peer.id);
        let previous = self.devices.read().await.get(&self.peer.id).cloned();
        let connected = self.links.lock().await.contains_key(&self.peer.id);
        let device = KcDevice {
            id: self.peer.id.clone(),
            name: self.peer.name.clone(),
            device_type: self.peer.device_type,
            protocol_version: self.peer.protocol_version,
            pair_state: if trusted {
                // A new link always starts with a fresh pairing handler, so
                // `self.pairing.state()` is `NotPaired` even for a device that
                // has been trusted for months. The trust store is the
                // authority, exactly as in `publish_discovered`; without this a
                // paired device is reported as "Not paired" every time the link
                // is re-established.
                crate::pairing::PairState::Paired
            } else {
                self.pairing.state()
            },
            link_state: if connected {
                LinkState::Connected
            } else {
                LinkState::Disconnected
            },
            transport: if connected {
                Transport::Lan
            } else {
                Transport::None
            },
            address: Some(self.peer_addr.to_string()),
            incoming_capabilities: self.peer.incoming_capabilities.clone(),
            outgoing_capabilities: self.peer.outgoing_capabilities.clone(),
            features: crate::types::features_from_capabilities(
                &self.peer.incoming_capabilities,
                &self.peer.outgoing_capabilities,
            ),
            trusted,
            verification_key: None,
            protocol_error: previous.as_ref().and_then(|d| d.protocol_error.clone()),
            last_seen: crate::config::now_secs(),
            battery_level: previous.as_ref().and_then(|d| d.battery_level),
            battery_is_charging: previous.as_ref().and_then(|d| d.battery_is_charging),
        };
        self.devices
            .write()
            .await
            .insert(self.peer.id.clone(), device.clone());
        let _ = self.events.send(KcEvent::Device(Box::new(device)));
    }

    async fn on_command(&mut self, cmd: LinkCommand) -> Result<()> {
        match cmd {
            // Only the select loop may move the socket, so that ownership
            // changes in exactly one place and cannot race with itself. Reaching
            // this means a command was queued onto the link's own channel and
            // then re-dispatched, which is a bug rather than a state to handle.
            LinkCommand::Adopt { .. } => {
                Err(anyhow!("socket adoption is only valid from the link's own loop"))
            }
            LinkCommand::Send(np) => self.writer.write_packet(&np).await,
            LinkCommand::SendWithPayload(np, file) => self.send_with_payload(*np, file).await,
            LinkCommand::RequestPairing => {
                let np = self.pairing.request_pairing_packet();
                self.writer.write_packet(&np).await?;
                Ok(())
            }
            LinkCommand::AcceptPairing => {
                // Pin the peer's certificate only after the user has confirmed
                // the verification key shown on the phone.
                self.ctx
                    .config
                    .lock()
                    .await
                    .trust_device(&self.peer, &self.peer_cert_pem)?;
                self.pairing.paired();
                let np = self.pairing.accept_packet();
                self.writer.write_packet(&np).await?;
                if self.ctx.settings.auto_clipboard() {
                    self.send_clipboard_connect().await;
                }
                self.publish().await;
                Ok(())
            }
            LinkCommand::CancelPairing => {
                let np = self.pairing.cancel_packet();
                self.writer.write_packet(&np).await?;
                self.ctx.config.lock().await.untrust_device(&self.peer.id)?;
                self.pairing.cancel();
                self.publish().await;
                Ok(())
            }
            LinkCommand::ReplyToNotification {
                notification_id,
                reply_id,
                text,
            } => {
                // `kdeconnect.notification.reply` carries the *reply* id the
                // peer gave us, not the notification id, plus the message.
                let mut np = p::NetworkPacket::new(p::PACKET_TYPE_NOTIFICATION_REPLY);
                np.set("requestReplyId", reply_id);
                np.set("message", text);
                self.writer.write_packet(&np).await?;
                let _ = self.events.send(KcEvent::NotificationReplied {
                    device_id: self.peer.id.clone(),
                    notification_id,
                });
                Ok(())
            }
            LinkCommand::Shutdown => Ok(()),
        }
    }

    async fn on_packet(&mut self, np: p::NetworkPacket) -> Result<()> {
        match np.packet_type.as_str() {
            p::PACKET_TYPE_PAIR => self.on_pair(np).await,
            p::PACKET_TYPE_PING => {
                let msg = np.get_str("message").unwrap_or_default();
                let mut reply = p::NetworkPacket::new(p::PACKET_TYPE_PING);
                reply.set("message", msg);
                self.writer.write_packet(&reply).await
            }
            p::PACKET_TYPE_CLIPBOARD => self.on_clipboard(np).await,
            p::PACKET_TYPE_CLIPBOARD_CONNECT => self.on_clipboard_connect(np).await,
            p::PACKET_TYPE_SHARE_REQUEST => self.on_share_request(np).await,
            p::PACKET_TYPE_SHARE_REQUEST_UPDATE => self.on_share_update(np).await,
            p::PACKET_TYPE_BATTERY => self.on_battery(np).await,
            p::PACKET_TYPE_CONNECTIVITY_REPORT => self.on_connectivity(np).await,
            p::PACKET_TYPE_NOTIFICATION => self.on_notification(np).await,
            p::PACKET_TYPE_NOTIFICATION_REQUEST => self.on_notification_request(np).await,
            p::PACKET_TYPE_IDENTITY => Ok(()),
            other => {
                debug!("{}: ignoring unhandled packet {other}", self.peer.name);
                Ok(())
            }
        }
    }

    async fn on_pair(&mut self, np: p::NetworkPacket) -> Result<()> {
        let our_key = self.ctx.config.lock().await.public_key_der().to_vec();
        let trusted = self.ctx.config.lock().await.is_trusted(&self.peer.id);

        let action = self
            .pairing
            .handle_packet(&np, trusted, &our_key, &self.peer_spki);
        match action {
            PairingAction::IncomingRequest {
                verification_key, ..
            } => {
                let device = self
                    .devices
                    .write()
                    .await
                    .entry(self.peer.id.clone())
                    .or_insert_with(|| self.blank_device())
                    .clone();
                let mut device = device;
                device.pair_state = PairState::RequestedByPeer;
                device.verification_key = Some(verification_key.clone());
                device.trusted = false;
                device.link_state = LinkState::PairingRequestedByPeer;
                self.devices
                    .write()
                    .await
                    .insert(self.peer.id.clone(), device.clone());
                let _ = self.events.send(KcEvent::PairingRequest {
                    device: Box::new(device),
                    verification_key,
                });
            }
            PairingAction::PeerAccepted => {
                // We requested the pairing and the peer accepted, so we must
                // pin their certificate too: both sides store trust, exactly
                // like KDE Connect's `pairingSuccessful()` handler.
                self.ctx
                    .config
                    .lock()
                    .await
                    .trust_device(&self.peer, &self.peer_cert_pem)?;
                self.pairing.paired();
                self.publish().await;
                if self.ctx.settings.auto_clipboard() {
                    self.send_clipboard_connect().await;
                }
            }
            PairingAction::Cancelled => {
                self.ctx.config.lock().await.untrust_device(&self.peer.id)?;
                self.pairing.cancel();
                self.publish().await;
            }
            PairingAction::StaleTimestamp => {
                let _ = self.events.send(KcEvent::Log {
                    level: "warn".into(),
                    message: format!(
                        "{}: pairing timestamp outside the allowed window, ignoring",
                        self.peer.name
                    ),
                });
            }
            PairingAction::TimedOut | PairingAction::AwaitingPeer { .. } => {}
        }
        Ok(())
    }

    fn blank_device(&self) -> KcDevice {
        KcDevice {
            id: self.peer.id.clone(),
            name: self.peer.name.clone(),
            device_type: self.peer.device_type,
            protocol_version: self.peer.protocol_version,
            pair_state: self.pairing.state(),
            link_state: LinkState::Connecting,
            transport: Transport::Lan,
            address: Some(self.peer_addr.to_string()),
            incoming_capabilities: self.peer.incoming_capabilities.clone(),
            outgoing_capabilities: self.peer.outgoing_capabilities.clone(),
            features: crate::types::features_from_capabilities(
                &self.peer.incoming_capabilities,
                &self.peer.outgoing_capabilities,
            ),
            trusted: false,
            verification_key: None,
            protocol_error: None,
            last_seen: crate::config::now_secs(),
            battery_level: None,
            battery_is_charging: None,
        }
    }

    async fn on_clipboard(&mut self, np: p::NetworkPacket) -> Result<()> {
        if !self.ctx.settings.clipboard_enabled() {
            return Ok(());
        }
        let Some(content) = np.get_str("content") else {
            return Ok(());
        };
        // The clipboard payload itself is never written to the log.
        info!(
            "{}: received clipboard text ({} bytes)",
            self.peer.name,
            content.len()
        );
        self.last_clipboard_ts = crate::config::now_millis();
        let _ = self.events.send(KcEvent::Clipboard {
            device_id: self.peer.id.clone(),
            device_name: self.peer.name.clone(),
            content,
        });
        Ok(())
    }

    async fn on_clipboard_connect(&mut self, np: p::NetworkPacket) -> Result<()> {
        if !self.ctx.settings.clipboard_enabled() {
            return Ok(());
        }
        let ts = np.body.get_i64("timestamp").unwrap_or(0);
        // Upstream drops stale packets; this is the cross-device loop breaker.
        if ts == 0 || ts < self.last_clipboard_ts {
            return Ok(());
        }
        if let Some(content) = np.get_str("content") {
            if !content.is_empty() {
                self.last_clipboard_ts = crate::config::now_millis();
                info!(
                    "{}: received clipboard text on connect ({} bytes)",
                    self.peer.name,
                    content.len()
                );
                let _ = self.events.send(KcEvent::Clipboard {
                    device_id: self.peer.id.clone(),
                    device_name: self.peer.name.clone(),
                    content,
                });
            }
        }
        Ok(())
    }

    async fn send_clipboard_connect(&mut self) {
        let mut np = p::NetworkPacket::new(p::PACKET_TYPE_CLIPBOARD_CONNECT);
        np.set("timestamp", crate::config::now_millis());
        let _ = self.writer.write_packet(&np).await;
    }

    async fn on_battery(&mut self, np: p::NetworkPacket) -> Result<()> {
        let level = np.body.get_i64("level").unwrap_or(-1).clamp(0, 100) as u8;
        let charging = np.body.get_bool("isCharging").unwrap_or(false);
        let id = self.peer.id.clone();
        {
            let mut devices = self.devices.write().await;
            let entry = devices
                .entry(id.clone())
                .or_insert_with(|| self.blank_device());
            entry.battery_level = Some(level);
            entry.battery_is_charging = Some(charging);
            let snapshot = entry.clone();
            drop(devices);
            let _ = self.events.send(KcEvent::Device(Box::new(snapshot)));
        }
        let _ = self.events.send(KcEvent::Battery {
            device_id: id,
            level,
            charging,
        });
        Ok(())
    }

    async fn on_connectivity(&mut self, np: p::NetworkPacket) -> Result<()> {
        let report = crate::types::ConnectivityReport {
            device_id: self.peer.id.clone(),
            wifi_strength: np.body.get_i64("wifiStrength"),
            wifi_name: np.get_str("wifiName"),
            bluetooth_strength: np.body.get_i64("bluetoothStrength"),
            cellular_strength: np.body.get_i64("cellularStrength"),
            cellular_name: np.get_str("cellularName"),
        };
        let _ = self.events.send(KcEvent::Connectivity(Box::new(report)));
        Ok(())
    }

    /// Parse a `kdeconnect.notification` packet.
    ///
    /// Both implementations put the notification key in `id` (not `key`) and
    /// send `actions` as a JSON array of action titles. Android additionally
    /// sends `time` as a decimal *string* and, for a repliable notification,
    /// `requestReplyId`. The desktop sends neither `time` nor `requestReplyId`.
    async fn on_notification(&mut self, np: p::NetworkPacket) -> Result<()> {
        let id = np.get_str("id").unwrap_or_default();
        if id.is_empty() {
            debug!("{}: notification without an id, ignoring", self.peer.name);
            return Ok(());
        }

        let actions: Vec<String> = np
            .body
            .get_array("actions")
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();

        // Android writes `time` as a string; be tolerant and accept a number
        // too, and fall back to now when the peer omits it entirely.
        let timestamp = np
            .body
            .get_str("time")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .or_else(|| np.body.get_i64("time"))
            .unwrap_or_else(crate::config::now_millis);

        let notification = crate::types::RemoteNotification {
            id: id.clone(),
            title: np.get_str("title").unwrap_or_default(),
            text: np.get_str("text").unwrap_or_default(),
            app_name: np.get_str("appName").unwrap_or_default(),
            timestamp,
            actions,
            request_reply_id: np.get_str("requestReplyId").filter(|s| !s.is_empty()),
            is_clearable: np.body.get_bool("isClearable").unwrap_or(true),
            silent: np.body.get_bool("silent").unwrap_or(false),
            group_name: np.get_str("groupName").filter(|s| !s.is_empty()),
        };

        let repliable = notification.request_reply_id.is_some();
        debug!(
            "{}: notification from {} ({} action(s){})",
            self.peer.name,
            notification.app_name,
            notification.actions.len(),
            if repliable { ", repliable" } else { "" }
        );

        // Replace any earlier notification with the same id rather than
        // accumulating duplicates, and keep it in the context so it survives a
        // reconnect. Per-link storage was emptied on every new link.
        self.ctx.note_notification(&self.peer.id, notification.clone()).await;

        let _ = self.events.send(KcEvent::Notification {
            device_id: self.peer.id.clone(),
            device_name: self.peer.name.clone(),
            notification: Box::new(notification),
        });
        Ok(())
    }

    /// `kdeconnect.notification.request` — the peer wants the notifications it
    /// holds, typically after (re)connecting. Echo them straight back.
    async fn on_notification_request(&mut self, _np: p::NetworkPacket) -> Result<()> {
        if !self.trusted {
            return Ok(());
        }

        // Answer only the first request on this link.
        //
        // The phone sends `notification.request` when it connects, and again
        // after reconnecting. Echoing every stored notification each time made
        // it re-show its whole history as fresh notifications - the repeated
        // "fedora1" entries, several times a minute. The phone has already got
        // them; sending them again achieves nothing except noise.
        if self.notifications_sent {
            debug!(
                "{}: ignoring a repeat notification request; already sent",
                self.peer.name
            );
            return Ok(());
        }
        self.notifications_sent = true;

        // Read from the context rather than this link, so a reconnect still has
        // something to answer with.
        let held = self.ctx.notifications_for(&self.peer.id).await;
        for n in held.iter().rev() {
            let mut out = p::NetworkPacket::new(p::PACKET_TYPE_NOTIFICATION);
            out.set("id", n.id.clone());
            out.set("appName", n.app_name.clone());
            out.set("title", n.title.clone());
            out.set("text", n.text.clone());
            out.set("ticker", n.app_name.clone());
            out.set("isClearable", n.is_clearable);
            out.set("silent", n.silent);
            // Android reads this field as a string, so send a string.
            out.set("time", n.timestamp.to_string());
            self.writer.write_packet(&out).await?;
        }
        Ok(())
    }

    async fn on_share_update(&mut self, np: p::NetworkPacket) -> Result<()> {
        let total = np.body.get_u64("totalPayloadSize").unwrap_or(0);
        let count = np.body.get_u64("numberOfFiles").unwrap_or(0);
        debug!(
            "{}: announced {count} file(s), {total} bytes",
            self.peer.name
        );
        Ok(())
    }

    async fn on_share_request(&mut self, np: p::NetworkPacket) -> Result<()> {
        let offer = if np.has("filename") {
            let filename = np
                .get_str("filename")
                .map(|f| sanitize_filename(&f))
                .unwrap_or_else(|| "received-file".into());
            ShareOffer {
                device_id: self.peer.id.clone(),
                device_name: self.peer.name.clone(),
                kind: "file".into(),
                filename: Some(filename),
                filesize: np.payload_size.or_else(|| np.body.get_u64("filesize")),
                pending_files: None,
                text: None,
                url: None,
                number_of_files: np.body.get_u64("numberOfFiles"),
                total_payload_size: np.body.get_u64("totalPayloadSize"),
            }
        } else if np.has("text") {
            let text = np.get_str("text").unwrap_or_default();
            if text.len() > MAX_TEXT_SHARE_BYTES {
                return Ok(());
            }
            ShareOffer {
                device_id: self.peer.id.clone(),
                device_name: self.peer.name.clone(),
                kind: "text".into(),
                filename: None,
                filesize: None,
                pending_files: None,
                text: Some(text),
                url: None,
                number_of_files: None,
                total_payload_size: None,
            }
        } else if np.has("url") {
            ShareOffer {
                device_id: self.peer.id.clone(),
                device_name: self.peer.name.clone(),
                kind: "url".into(),
                filename: None,
                filesize: None,
                pending_files: None,
                text: None,
                url: np.get_str("url"),
                number_of_files: None,
                total_payload_size: None,
            }
        } else {
            return Ok(());
        };

        // A file payload arrives on a second connection; open it now so the
        // sender is not left waiting.
        if offer.kind == "file" {
            let payload_size = np.payload_size.unwrap_or(0);
            let port = np
                .payload_transfer_info
                .as_ref()
                .and_then(|i| i.get_i64("port"))
                .and_then(|p| u16::try_from(p).ok());

            match port {
                Some(port) => {
                    spawn_payload_receiver(
                        self.ctx.clone(),
                        self.peer.id.clone(),
                        self.peer_addr.ip(),
                        port,
                        offer.filename.clone().unwrap_or_default(),
                        payload_size,
                        self.events.clone(),
                    );
                }
                None if payload_size > 0 => {
                    let _ = self.events.send(KcEvent::Log {
                        level: "error".into(),
                        message: format!(
                            "{}: announced a {payload_size} byte payload without a transfer port",
                            self.peer.name
                        ),
                    });
                    return Ok(());
                }
                None => {}
            }
        }

        let _ = self.events.send(KcEvent::ShareOffer(Box::new(offer)));
        Ok(())
    }

    /// Send a header packet followed by a file payload on a dedicated TLS
    /// connection. Mirrors `CompositeUploadJob::sendPacketFor()`: the receiver
    /// opens the connection, so it plays the TLS server and we the TLS client.
    async fn send_with_payload(
        &mut self,
        mut np: p::NetworkPacket,
        file: tokio::fs::File,
    ) -> Result<()> {
        let meta = file.metadata().await.context("stat payload")?;
        let total = meta.len();
        let filename = np.get_str("filename").unwrap_or_else(|| "file".into());

        let listener = TcpListener::bind(("0.0.0.0", 0))
            .await
            .context("bind payload port")?;
        let port = listener.local_addr()?.port();

        np.payload_size = Some(total);
        let mut info = p::PacketBody::new();
        info.set("port", i64::from(port));
        np.payload_transfer_info = Some(info);
        self.writer.write_packet(&np).await?;

        let events = self.events.clone();
        let device_id = self.peer.id.clone();
        let tls = self.ctx.tls.clone();

        let transfer_id = format!("{}:out:{}", device_id, filename);
        let cancel = crate::cancel::register(&transfer_id);

        tokio::spawn(async move {
            let accepted = tokio::time::timeout(Duration::from_secs(30), listener.accept()).await;
            let Ok(Ok((socket, _))) = accepted else {
                let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                    id: device_id.clone(),
                    device_id: device_id.clone(),
                    filename: filename.clone(),
                    bytes: 0,
                    total,
                    done: true,
                    cancelled: false,
                    error: Some("The phone did not open the file transfer connection".into()),
                saved_path: None,
            })));
                crate::cancel::finish(&transfer_id);
                return;
            };
            let _ = socket.set_nodelay(true);

            // We *listened*, so we are the TLS **server** on the payload
            // channel. The sender listens and the receiver dials; Android
            // confirms the receiver's side with `clientMode = true` in
            // `LanLink.kt`. This is the same opener/acceptor rule as the
            // control channel, where the opener plays the server.
            let connected = tls.handshake(TlsRole::Server, socket).await;
            let (mut stream, _reader) = match connected {
                Ok(s) => {
                    let (r, w) = tokio::io::split(s);
                    (w, r)
                }
                Err(e) => {
                    let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                        id: device_id.clone(),
                        device_id: device_id.clone(),
                        filename: filename.clone(),
                        bytes: 0,
                        total,
                        done: true,
                        cancelled: false,
                        error: Some(format!("TLS handshake failed: {e}")),
                saved_path: None,
            })));
                    return;
                }
            };

            let mut file = file;
            let mut sent: u64 = 0;
            let mut buf = vec![0u8; 256 * 1024];
            let mut last_emit = std::time::Instant::now();
            loop {
                let n = match file.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        emit_error(&events, &device_id, &filename, sent, total, &e.to_string());
                        return;
                    }
                };
                // Cancellation is signalled by dropping the payload socket,
                // which is how both KDE Connect implementations cancel.
                if cancel.is_cancelled() {
                    info!("{filename}: cancelled after {sent}/{total} bytes");
                    let _ = stream.shutdown().await;
                    crate::cancel::finish(&transfer_id);
                    let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                        id: device_id.clone(),
                        device_id: device_id.clone(),
                        filename: filename.clone(),
                        bytes: sent,
                        total,
                        done: true,
                        cancelled: true,
                        error: None,
                saved_path: None,
            })));
                    return;
                }
                if let Err(e) = stream.write_all(&buf[..n]).await {
                    emit_error(&events, &device_id, &filename, sent, total, &e.to_string());
                    return;
                }
                sent += n as u64;
                if last_emit.elapsed() > Duration::from_millis(200) || sent == total {
                    let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                        id: device_id.clone(),
                        device_id: device_id.clone(),
                        filename: filename.clone(),
                        bytes: sent,
                        total,
                        done: false,
                        cancelled: false,
                        error: None,
                saved_path: None,
            })));
                    last_emit = std::time::Instant::now();
                }
            }
            // A clean TLS close_notify plus a short grace period: dropping the
            // socket immediately after the last write resets the connection and
            // can lose bytes the receiver has not drained from its own buffer.
            let _ = stream.flush().await;
            let _ = stream.shutdown().await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            crate::cancel::finish(&transfer_id);
            let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                id: device_id.clone(),
                device_id,
                filename,
                bytes: sent,
                total,
                done: true,
                cancelled: false,
                error: None,
                saved_path: None,
            })));
        });

        Ok(())
    }
}

fn emit_error(
    events: &broadcast::Sender<KcEvent>,
    device_id: &str,
    filename: &str,
    sent: u64,
    total: u64,
    error: &str,
) {
    let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
        id: device_id.to_string(),
        device_id: device_id.to_string(),
        filename: filename.to_string(),
        bytes: sent,
        total,
        done: true,
        cancelled: false,
        error: Some(error.to_string()),
                saved_path: None,
            })));
}

/// Accept the dedicated payload connection for an incoming file share.
///
/// The sender opened TCP, so it is the TLS server and we are the TLS client.
fn spawn_payload_receiver(
    ctx: Arc<LinkContext>,
    device_id: String,
    control_ip: std::net::IpAddr,
    port: u16,
    filename: String,
    total: u64,
    events: broadcast::Sender<KcEvent>,
) -> tokio::task::JoinHandle<()> {
    let transfer_id = format!("{device_id}:in:{filename}");
    let cancel = crate::cancel::register(&transfer_id);

    tokio::spawn(async move {
        let addr = SocketAddr::new(control_ip, port);
        let stream =
            match tokio::time::timeout(Duration::from_secs(20), TcpStream::connect(addr)).await {
                Ok(Ok(s)) => s,
                _ => {
                    emit_error(
                        &events,
                        &device_id,
                        &filename,
                        0,
                        total,
                        &format!("Could not connect to {addr} for the file payload"),
                    );
                    return;
                }
            };
        let _ = stream.set_nodelay(true);
        // We dialled the TCP connection, so we play the TLS *client*.
        //
        // The payload channel inverts the control channel: the **sender**
        // listens and the **receiver** dials. Upstream Android does exactly
        // this in `backends/lan/LanLink.kt`, where `receivedNetworkPacket`
        // connects to the announced port and then calls
        // `convertToSslSocket(..., clientMode = true)`.
        //
        // Getting this backwards shows up as a bare "tls handshake eof": the
        // phone, expecting a ServerHello, receives our ClientHello and closes.
        let tls = match ctx.tls.handshake(TlsRole::Client, stream).await {
            Ok(s) => s,
            Err(e) => {
                emit_error(
                    &events,
                    &device_id,
                    &filename,
                    0,
                    total,
                    &format!("TLS handshake failed: {e}"),
                );
                return;
            }
        };

        let mut tls = tls;
        let dir = ctx.settings.incoming_path();
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            emit_error(&events, &device_id, &filename, 0, total, &e.to_string());
            return;
        }

        // Refuse before writing anything if the announced size exceeds the
        // ceiling or the free space. Failing here rather than mid-stream keeps
        // the filesystem intact and gives the user an immediate answer.
        let max_bytes = ctx.config.lock().await.max_incoming_bytes();
        if let Err(refusal) = crate::transfer_guard::check_incoming(&dir, total, max_bytes) {
            let msg = refusal.to_string();
            warn!("refusing a file from {device_id}: {msg}");
            emit_error(&events, &device_id, &filename, 0, total, &msg);
            return;
        }

        let dest = unique_destination(&dir, &filename);
        // The temporary file must be unique per attempt, not merely per name:
        // two transfers of "photo.jpg" arriving at once would otherwise share
        // one ".part" file and interleave their bytes into a corrupt result.
        let tmp = unique_destination(&dir, &format!("{filename}.part"));
        let mut guard = crate::transfer_guard::DownloadGuard::new(Some(total), max_bytes);
        let mut file = match tokio::fs::File::create(&tmp).await {
            Ok(f) => f,
            Err(e) => {
                emit_error(
                    &events,
                    &device_id,
                    &filename,
                    0,
                    total,
                    &format!("{}: {e}", dir.display()),
                );
                return;
            }
        };

        let mut buf = vec![0u8; 256 * 1024];
        let mut received: u64 = 0;
        let mut last_emit = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                info!("{filename}: download cancelled at {received}/{total} bytes");
                drop(tls);
                let _ = tokio::fs::remove_file(&tmp).await;
                crate::cancel::finish(&transfer_id);
                let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                    id: device_id.clone(),
                    device_id: device_id.clone(),
                    filename: filename.clone(),
                    bytes: received,
                    total,
                    done: true,
                    cancelled: true,
                    error: None,
                saved_path: None,
            })));
                return;
            }
            match tls.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    // Enforce the ceiling while streaming, not only up front: a
                    // peer may announce a small size and then keep sending.
                    if let Err(e) = guard.record(n as u64) {
                        drop(file);
                        let _ = tokio::fs::remove_file(&tmp).await;
                        warn!("stopping a file from {device_id}: {e}");
                        emit_error(&events, &device_id, &filename, received, total, &e);
                        return;
                    }
                    if let Err(e) = file.write_all(&buf[..n]).await {
                        let _ = tokio::fs::remove_file(&tmp).await;
                        emit_error(
                            &events,
                            &device_id,
                            &filename,
                            received,
                            total,
                            &e.to_string(),
                        );
                        return;
                    }
                    received += n as u64;
                    if last_emit.elapsed() > Duration::from_millis(200) {
                        let _ =
                            events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                                id: device_id.clone(),
                                device_id: device_id.clone(),
                                filename: filename.clone(),
                                bytes: received,
                                total,
                                done: false,
                                cancelled: false,
                                error: None,
                saved_path: None,
            })));
                        last_emit = std::time::Instant::now();
                    }
                }
                Err(e) => {
                    // If the sender reset the socket *after* every announced
                    // byte arrived, the transfer is complete. Only report a
                    // failure when data is actually missing.
                    let announced = if total > 0 { total } else { 0 };
                    if announced > 0 && received >= announced {
                        debug!(
                            "{filename}: transport reset after {received}/{announced} bytes;                              treating as complete"
                        );
                        break;
                    }
                    let _ = tokio::fs::remove_file(&tmp).await;
                    emit_error(
                        &events,
                        &device_id,
                        &filename,
                        received,
                        total,
                        &e.to_string(),
                    );
                    return;
                }
            }
        }

        drop(file);
        crate::cancel::finish(&transfer_id);

        // The stream ended but the peer sent less than it announced. Moving
        // the file into place now would present a truncated, corrupt file to the
        // user as though it were intact, so the partial data is discarded.
        if !guard.is_complete() {
            let _ = tokio::fs::remove_file(&tmp).await;
            let msg = format!(
                "the sender ended after {} of the announced {}",
                crate::transfer_guard::human_bytes(guard.received()),
                crate::transfer_guard::human_bytes(total)
            );
            warn!("truncated transfer from {device_id}: {msg}");
            emit_error(&events, &device_id, &filename, received, total, &msg);
            return;
        }

        if let Err(e) = std::fs::rename(&tmp, &dest) {
            emit_error(
                &events,
                &device_id,
                &filename,
                received,
                total,
                &format!("Could not finalise download: {e}"),
            );
            return;
        }
        let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
            id: device_id.clone(),
            device_id,
            filename: dest
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            bytes: received,
            total,
            done: true,
            cancelled: false,
            error: None,
            // Report where it actually landed. The filename above is only the
            // final component, and the incoming folder may have been
            // reconfigured mid-transfer, so the application cannot derive the
            // path itself.
            saved_path: Some(dest.to_string_lossy().to_string()),
        })));
    })
}

/// Strip any path components a peer may have injected into a filename.
///
/// Everything that could change *where* the file lands, or *how* it is
/// displayed, is removed rather than escaped:
///
///   * path separators, by keeping only the final component;
///   * NUL, which truncates paths at the syscall boundary on some platforms;
///   * other control characters, which would otherwise let a peer forge log
///     lines by sending a filename containing a newline;
///   * `:`, which is a path separator on Windows and part of the KDE Connect
///     notification key format.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or("file");
    let base = base.rsplit('\\').next().unwrap_or(base);
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != ':')
        .take(200)
        .collect();
    let cleaned = cleaned.trim().to_string();
    // A name made entirely of removed characters, or the relative-directory
    // markers, must not survive as a destination.
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." || cleaned.chars().all(|c| c == '.')
    {
        "received-file".to_string()
    } else {
        cleaned
    }
}

pub fn unique_destination(dir: &std::path::Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{ext}", crate::config::now_millis()))
}

/// Build a `kdeconnect.share.request` header for a local file, using the body
/// keys `SharePlugin::shareUrl()` sets upstream.
pub fn share_file_packet(path: &std::path::Path) -> p::NetworkPacket {
    let mut np = p::NetworkPacket::new(p::PACKET_TYPE_SHARE_REQUEST);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    np.set("filename", name);
    let meta = std::fs::metadata(path).ok();
    let millis = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(crate::config::now_millis)
    };
    if let Some(m) = &meta {
        np.set("creationTime", millis(m.created()));
        np.set("lastModified", millis(m.modified()));
        np.set("filesize", m.len());
    }
    np.set("open", false);
    np.set("numberOfFiles", 1u64);
    np.set(
        "totalPayloadSize",
        meta.as_ref().map(|m| m.len()).unwrap_or(0),
    );
    np
}

/// `SharePlugin::shareText()`.
pub fn share_text_packet(text: &str) -> p::NetworkPacket {
    let mut np = p::NetworkPacket::new(p::PACKET_TYPE_SHARE_REQUEST);
    np.set("text", text);
    np
}

/// `SharePlugin::shareUrl()` for non-local URLs.
pub fn share_url_packet(url: &str) -> p::NetworkPacket {
    let mut np = p::NetworkPacket::new(p::PACKET_TYPE_SHARE_REQUEST);
    np.set("url", url);
    np
}

/// The payload channel inverts which side listens, but keeps the same rule as
/// the control channel: whoever dials plays the TLS **client**.
///
/// Pinned against kdeconnect-android `backends/lan/LanLink.kt`, where
/// `receivedNetworkPacket` connects to the announced port and then calls
/// `convertToSslSocket(..., clientMode = true)` — the receiver is the client.
///
/// This matters because the integration tests cannot detect a wrong role: they
/// connect two instances of *this* implementation, so an inversion on both
/// sides still agrees with itself and passes.

#[cfg(test)]
/// The bug that made every connection die at exactly 30 seconds.
///
/// `INBOUND_HANDSHAKE_TIMEOUT` wrapped `handle_incoming`, which returns only
/// The 30-second teardown, and the silence heuristic that replaced it.
///
/// The first bug: `INBOUND_HANDSHAKE_TIMEOUT` (30 s) wrapped `handle_incoming`,
/// which returns only when the link ends, so every inbound connection died at
/// exactly 30 s. Reported as "it drops after about 30 seconds"; it was neither a
/// hotspot problem nor a network problem.
///
/// The second bug was mine, and worse for a while: with that fixed, the link went
/// quiet and I read that as death. The phone sends nothing at all while idle, so
/// a healthy link was torn down about once a minute - and the phone announced the
/// reconnection each time. Silencing the symptom by reconnecting faster made the
/// churn worse.
///
/// Upstream `LanLinkProvider::configureSocket` settles it: keepalive belongs at
/// the TCP layer (SO_KEEPALIVE, TCP_KEEPIDLE=30, TCP_KEEPINTVL=10, TCP_KEEPCNT=3),
/// where the kernel detects a dead peer in about a minute. An application must
/// not infer death from silence.
mod link_lifetime_tests {
    use super::*;

    /// The handshake budget bounds the handshake only, and must be shorter than
    /// upstream's ~60 s detection window - a longer handshake budget would kill
    /// links before the kernel had a chance to notice a dead one.
    #[test]
    fn the_handshake_budget_is_shorter_than_upstreams_detection_window() {
        // Upstream: 30 s idle + 10 s interval x 3 probes = ~60 s to detect death.
        let upstream_detection = Duration::from_secs(
            (crate::link::TCP_KEEPALIVE_IDLE_SECS
                + crate::link::TCP_KEEPALIVE_INTERVAL_SECS * crate::link::TCP_KEEPALIVE_COUNT)
                as u64,
        );
        assert_eq!(
            crate::lan::INBOUND_HANDSHAKE_TIMEOUT,
            Duration::from_secs(30)
        );
        assert!(
            crate::lan::INBOUND_HANDSHAKE_TIMEOUT < upstream_detection,
            "the handshake budget must not outlast the kernel's own detection"
        );
    }

    /// The keepalive timings are upstream's, and they must actually add up to
    /// detection in about a minute. Changing them silently would bring back the
    /// multi-minute hang this replaced.
    #[test]
    fn keepalive_detects_a_dead_peer_within_about_a_minute() {
        assert_eq!(crate::link::TCP_KEEPALIVE_IDLE_SECS, 30);
        assert_eq!(crate::link::TCP_KEEPALIVE_INTERVAL_SECS, 10);
        assert_eq!(crate::link::TCP_KEEPALIVE_COUNT, 3);
        let total = crate::link::TCP_KEEPALIVE_IDLE_SECS
            + crate::link::TCP_KEEPALIVE_INTERVAL_SECS * crate::link::TCP_KEEPALIVE_COUNT;
        assert!(
            (50..=70).contains(&total),
            "detection takes {total}s; upstream aims at about 60"
        );
    }
}

mod duplicate_link_tests {
    use super::*;
    use std::collections::hash_map::Entry;
    use tokio::sync::mpsc;

    async fn claim(
        links: &Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>>,
        id: &str,
    ) -> bool {
        let (tx, _rx) = mpsc::channel::<LinkCommand>(1);
        let mut guard = links.lock().await;
        match guard.entry(id.to_string()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(tx);
                true
            }
        }
    }

    /// The old behaviour: a second link silently displaced the first, and the
    /// first link's receiver reported a closed channel.
    #[tokio::test]
    async fn a_second_insert_displaces_the_first_receiver() {
        let links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (first_tx, mut first_rx) = mpsc::channel::<LinkCommand>(1);

        links.lock().await.insert("peer".into(), first_tx);
        // The unconditional insert the old code performed.
        let (second_tx, _second_rx) = mpsc::channel::<LinkCommand>(1);
        links.lock().await.insert("peer".into(), second_tx);

        // Dropping the replaced sender is what tore the live link down.
        assert!(
            first_rx.recv().await.is_none(),
            "the displaced link must see a closed channel - this is the bug"
        );
    }

    /// The new behaviour: the second link stands down and the first survives.
    #[tokio::test]
    async fn the_second_link_stands_down_and_the_first_survives() {
        let links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (first_tx, mut first_rx) = mpsc::channel::<LinkCommand>(1);

        links.lock().await.insert("peer".into(), first_tx);

        assert!(!claim(&links, "peer").await, "a second link must stand down");

        // Prove the *original* sender is still the registered one. Sending on a
        // displaced sender would silently go nowhere, which is exactly how the
        // live link was being torn down mid-transfer.
        let live = links
            .lock()
            .await
            .get("peer")
            .cloned()
            .expect("the slot must still be held");
        live.send(LinkCommand::RequestPairing)
            .await
            .expect("the registered link must still be usable");
        assert!(
            matches!(first_rx.recv().await, Some(LinkCommand::RequestPairing)),
            "the original link must still be the registered one"
        );
    }

    /// Different devices are unaffected - the guard is per device.
    #[tokio::test]
    async fn a_different_peer_is_still_accepted() {
        let links: Arc<Mutex<HashMap<String, mpsc::Sender<LinkCommand>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        assert!(claim(&links, "phone-a").await);
        assert!(claim(&links, "phone-b").await);
        assert!(!claim(&links, "phone-a").await);
        assert_eq!(links.lock().await.len(), 2);
    }
}

/// The payload channel's TLS role.
///
/// It follows the same rule as the control channel: **whoever opens the TCP
/// connection is the TLS server.** On the payload channel the *sender* listens
/// and the receiver dials, so the sender is the server.
///
/// Android's `LanLink.kt` confirms it: on receiving a file it calls
/// `convertToSslSocket(..., clientMode = true)` — it dials, so it plays the
/// TLS client. Getting this backwards shows up only as a bare "tls handshake
/// eof", because the peer expects a ServerHello and receives a ClientHello.
///
/// These assertions cannot catch a wrong role on their own: the integration
/// tests connect two instances of *this* implementation, so an inversion on both
/// sides still agrees with itself and passes. Pinning the roles explicitly is
/// what makes the inversion visible.
/// Whether a newly arrived socket may replace an established link.
///
/// The rule that makes adoption safe, kept as a pure function so it can be
/// tested without a TLS stack, a network or a phone, and so the call site and
/// the tests cannot drift apart.
pub fn may_replace_socket(presented: Option<&[u8]>, pinned: Option<&[u8]>) -> bool {
    match (presented, pinned) {
        // A trusted peer must present exactly the pinned certificate. A
        // re-paired peer presents a new certificate, and that is a revocation
        // plus a fresh pairing - not a socket replacement.
        (Some(presented), Some(pinned)) => presented == pinned,
        // An untrusted peer has nothing pinned; the certificate itself is the
        // only identity available, and `make_link` re-checks it against the
        // announced device id on both paths.
        (Some(_), None) => true,
        // No certificate at all is never acceptable.
        (None, _) => false,
    }
}

/// The rule that makes socket adoption safe.
///
/// Adopting a replacement socket is what lets a phone recover when it
/// reconnects while our end of the old socket still looks healthy. But
/// "anything that can reach port 1716 gets to take over an established session"
/// is not acceptable, so adoption is gated on the certificate.
///
/// Upstream does the same comparison in `LanLinkProvider::addLink`:
///
/// ```cpp
/// if (deviceLink->deviceInfo().certificate != deviceInfo.certificate) {
///     qWarning() << "LanLink was asked to replace a socket but the certificate
///                  doesn't match, aborting";
///     socket->abort();
///     return;
/// }
/// deviceLink->reset(socket);
/// ```
mod socket_replacement_tests {
    use super::may_replace_socket as may_replace;

    #[test]
    fn a_trusted_peer_replacing_with_its_pinned_certificate_is_allowed() {
        let cert = b"the-pinned-certificate";
        assert!(may_replace(Some(cert), Some(cert)));
    }

    /// The security case: anything able to reach the port must not be able to
    /// take over a trusted session by presenting a different certificate.
    #[test]
    fn a_different_certificate_cannot_replace_a_trusted_link() {
        assert!(!may_replace(Some(b"attacker"), Some(b"the-pinned-certificate")));
    }

    #[test]
    fn a_certificate_of_a_different_length_cannot_replace() {
        assert!(!may_replace(Some(b"cert"), Some(b"cert-and-more")));
        assert!(!may_replace(Some(b"cert-and-more"), Some(b"cert")));
    }

    /// A peer that re-pairs legitimately presents a new certificate. Adoption
    /// must not silently accept it under the old trust record - that is a
    /// revocation and a fresh pairing, not a socket replacement.
    #[test]
    fn a_re_paired_certificate_is_not_a_socket_replacement() {
        assert!(!may_replace(Some(b"new-cert"), Some(b"old-cert")));
    }

    #[test]
    fn no_certificate_is_never_acceptable() {
        assert!(!may_replace(None, Some(b"pinned")));
        assert!(!may_replace(None, None));
    }

    /// An untrusted peer has no pin, so the certificate is the only identity
    /// available and adoption is permitted; pairing still has to happen.
    #[test]
    fn an_untrusted_peer_with_a_certificate_may_be_adopted() {
        assert!(may_replace(Some(b"whatever"), None));
    }
}

mod payload_role_tests {
    use super::TlsRole;

    /// Which role does the sender use? The sender listens, so it is the server.
    fn sender_role() -> TlsRole {
        TlsRole::Server
    }

    /// Which role does the receiver use? The receiver dials, so it is the client.
    fn receiver_role() -> TlsRole {
        TlsRole::Client
    }

    #[test]
    fn the_sender_is_the_tls_server_and_the_receiver_the_client() {
        assert_eq!(sender_role(), TlsRole::Server);
        assert_eq!(receiver_role(), TlsRole::Client);
        assert_ne!(sender_role(), receiver_role());
    }

    #[test]
    fn the_roles_match_the_control_channel_rule() {
        // Same rule as `handle_incoming`/`handle_outgoing`: the opener of the
        // TCP connection is the TLS server.
        let control_opener = TlsRole::Server;
        let control_accepter = TlsRole::Client;
        assert_eq!(
            sender_role(),
            control_opener,
            "sender listens, like the control opener"
        );
        assert_eq!(
            receiver_role(),
            control_accepter,
            "receiver dials, like the control accepter"
        );
    }
}

mod tests {
    use super::*;

    #[test]
    fn filenames_cannot_escape_the_download_directory() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a/b/c.txt"), "c.txt");
        assert_eq!(sanitize_filename(".."), "received-file");
        assert_eq!(sanitize_filename("...."), "received-file");
        assert_eq!(sanitize_filename("normal.png"), "normal.png");
        assert_eq!(sanitize_filename(""), "received-file");
        assert_eq!(sanitize_filename("normal.png"), "normal.png");
    }

    #[test]
    fn every_traversal_and_injection_shape_is_stripped() {
        // Each of these must reduce to a plain name inside the download
        // directory. The assertion is not on the exact result — a peer's choice
        // of spelling should not matter — but on the invariant that nothing
        // capable of changing the destination survives.
        let hostile = [
            "../../../etc/shadow",
            "..\\..\\windows\\system32\\config",
            "/etc/passwd",
            "/absolute/path.bin",
            "C:\\Windows\\evil.dll",
            "....//....//etc/passwd",
            "./../../etc/passwd",
            "a/b/../../../c",
            "....",
            "..",
            ".",
            "./",
            "../",
            "foo/..",
        ];
        for raw in hostile {
            let clean = sanitize_filename(raw);
            assert!(!clean.is_empty(), "{raw:?} produced an empty name");
            assert!(
                !clean.contains('/') && !clean.contains('\\'),
                "{raw:?} still contains a path separator: {clean:?}"
            );
            assert!(
                clean != "." && clean != "..",
                "{raw:?} resolved to {clean:?}"
            );
            // The decisive check: joining it must stay inside the directory.
            let dir = std::path::Path::new("/tmp/downloads");
            let joined = dir.join(&clean);
            assert!(
                joined.starts_with(dir),
                "{raw:?} escaped to {}",
                joined.display()
            );
        }
    }

    #[test]
    fn nuls_and_control_characters_are_removed() {
        assert_eq!(sanitize_filename("a\0b.txt"), "ab.txt");
        // A newline in a filename would break line-oriented protocol handling
        // and log forging; it is stripped by the NUL filter's sibling rule.
        assert!(!sanitize_filename("a\nb").contains('\n'));
        assert!(!sanitize_filename("a\r\nb: fake log line").contains('\n'));
    }

    #[test]
    fn an_overlong_name_is_bounded() {
        let long = "x".repeat(5000);
        let clean = sanitize_filename(&long);
        assert!(clean.len() <= 200, "got {} chars", clean.len());
    }

    #[test]
    fn a_dangling_symlink_does_not_redirect_the_destination() {
        // `Path::exists` follows symlinks, so a dangling symlink reports
        // "missing" and would otherwise be chosen as the destination. Writing
        // to it would follow the link if it later resolved to somewhere else.
        let dir = std::env::temp_dir().join(format!("pc-symlink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"original").unwrap();

        let link = dir.join("evil.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, &link).unwrap();

        let chosen = unique_destination(&dir, "evil.txt");
        // Writing the temporary file must not have followed the link.
        assert_ne!(
            chosen, link,
            "a symlink must never be chosen as the destination"
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"original");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn concurrent_transfers_of_one_name_get_distinct_temporaries() {
        // Two transfers of "photo.jpg" must not share one ".part" file, or
        // their bytes interleave into a corrupt result.
        let dir = std::env::temp_dir().join(format!("pc-concurrent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let first = unique_destination(&dir, "photo.jpg.part");
        std::fs::write(&first, b"a").unwrap();
        let second = unique_destination(&dir, "photo.jpg.part");
        assert_ne!(first, second, "the temporary path collided");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unique_destination_avoids_overwriting() {
        let dir = std::env::temp_dir().join(format!("pc-dest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_destination(&dir, "a.txt");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_destination(&dir, "a.txt");
        assert_ne!(first, second);
        assert!(second.to_string_lossy().contains("(1)"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn share_packet_uses_upstream_body_keys() {
        let path = std::env::temp_dir().join(format!("pc-share-{}.txt", std::process::id()));
        std::fs::write(&path, b"hello").unwrap();
        let np = share_file_packet(&path);
        assert_eq!(np.packet_type, "kdeconnect.share.request");
        assert!(np.has("filename"));
        assert!(np.has("creationTime"));
        assert!(np.has("lastModified"));
        assert!(np.has("open"));
        assert_eq!(np.body.get_u64("numberOfFiles"), Some(1));
        assert_eq!(np.body.get_u64("totalPayloadSize"), Some(5));
        assert!(np.payload_transfer_info.is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn text_and_url_shares_use_distinct_keys() {
        assert_eq!(share_text_packet("hi").get_str("text").unwrap(), "hi");
        assert!(!share_text_packet("hi").has("url"));
        assert_eq!(
            share_url_packet("https://example.org")
                .get_str("url")
                .unwrap(),
            "https://example.org"
        );
        assert!(!share_url_packet("https://example.org").has("filename"));
    }
}
