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

use std::collections::HashMap;
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
use crate::pairing::{PairingAction, PairingHandler, PairState};
use crate::packet as p;
use crate::tls::{TlsRole, TlsStream};
use crate::types::{KcDevice, LinkState, ShareOffer, Transport, TransferProgress};

/// Cap on the size of an inbound text/URL share we will surface.
pub const MAX_TEXT_SHARE_BYTES: usize = 256 * 1024;

/// Bring up a device link on an already-established TLS stream.
#[allow(clippy::too_many_arguments)]
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

    let link = DeviceLink {
        ctx,
        reader,
        writer,
        peer: peer_info,
        peer_spki,
        peer_cert_pem,
        notifications: Vec::new(),
        peer_addr,
        trusted,
        pairing: PairingHandler::new(),
        events,
        links,
        devices,
        last_clipboard_ts: crate::config::now_millis(),
    };
    link.run().await
}

struct DeviceLink<S: AsyncRead + AsyncWrite + Unpin> {
    ctx: Arc<LinkContext>,
    reader: LineReader<tokio::io::ReadHalf<TlsStream<S>>>,
    writer: PacketWriter<tokio::io::WriteHalf<TlsStream<S>>>,
    /// Notifications seen on this link, newest first. Reactions look the peer
    /// keys up here so a stale UI cannot act on a notification that is gone.
    notifications: Vec<crate::types::RemoteNotification>,
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
    async fn run(mut self) -> Result<()> {
        self.pairing.set_state(if self.trusted {
            PairState::Paired
        } else {
            PairState::NotPaired
        });

        // Register the command channel *before* announcing the device, so the
        // published state really does reflect a usable link.
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<LinkCommand>(32);
        self.links
            .lock()
            .await
            .insert(self.peer.id.clone(), cmd_tx);

        self.publish().await;

        // A freshly connected trusted peer gets our current clipboard offer.
        if self.trusted && self.ctx.settings.auto_clipboard() {
            self.send_clipboard_connect().await;
        }

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
        Ok(())
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
            pair_state: self.pairing.state(),
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
            PairingAction::IncomingRequest { verification_key, .. } => {
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
            request_reply_id: np
                .get_str("requestReplyId")
                .filter(|s| !s.is_empty()),
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
        // accumulating duplicates.
        self.notifications
            .retain(|n: &crate::types::RemoteNotification| n.id != notification.id);
        self.notifications.insert(0, notification.clone());
        // Bound the list: a chatty app must not grow memory without limit.
        const MAX_NOTIFICATIONS: usize = 100;
        self.notifications.truncate(MAX_NOTIFICATIONS);

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
        for n in self.notifications.iter().rev() {
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
                })));
                crate::cancel::finish(&transfer_id);
                return;
            };
            let _ = socket.set_nodelay(true);

            let connected = tls.handshake(TlsRole::Client, socket).await;
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
                        emit_error(
                            &events, &device_id, &filename, sent, total, &e.to_string(),
                        );
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
                    })));
                    return;
                }
                if let Err(e) = stream.write_all(&buf[..n]).await {
                    emit_error(
                        &events, &device_id, &filename, sent, total, &e.to_string(),
                    );
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
        let stream = match tokio::time::timeout(Duration::from_secs(20), TcpStream::connect(addr))
            .await
        {
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
        // We dialled the TCP connection, so we play the TLS *server* here.
        // This is the inverse of the control channel: the payload transfer
        // dials from the receiver to the sender.
        let tls = match ctx.tls.handshake(TlsRole::Server, stream).await {
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
        let dest = unique_destination(&dir, &filename);
        let tmp = dest.with_extension("part");
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
                })));
                return;
            }
            match tls.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = file.write_all(&buf[..n]).await {
                        let _ = tokio::fs::remove_file(&tmp).await;
                        emit_error(&events, &device_id, &filename, received, total, &e.to_string());
                        return;
                    }
                    received += n as u64;
                    if last_emit.elapsed() > Duration::from_millis(200) {
                        let _ = events.send(KcEvent::TransferProgress(Box::new(TransferProgress {
                            id: device_id.clone(),
                            device_id: device_id.clone(),
                            filename: filename.clone(),
                            bytes: received,
                            total,
                            done: false,
                            cancelled: false,
                            error: None,
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
                    emit_error(&events, &device_id, &filename, received, total, &e.to_string());
                    return;
                }
            }
        }

        drop(file);
        crate::cancel::finish(&transfer_id);
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
        })));
    })
}

/// Strip any path components a peer may have injected into a filename.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or("file");
    let base = base.rsplit('\\').next().unwrap_or(base);
    let cleaned: String = base
        .chars()
        .filter(|c| *c != '\0' && *c != ':')
        .take(200)
        .collect();
    if cleaned.trim().is_empty() || cleaned == "." || cleaned == ".." {
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
    np.set("totalPayloadSize", meta.as_ref().map(|m| m.len()).unwrap_or(0));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_cannot_escape_the_download_directory() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a/b/c.txt"), "c.txt");
        assert_eq!(sanitize_filename(".."), "received-file");
        assert_eq!(sanitize_filename(""), "received-file");
        assert_eq!(sanitize_filename("normal.png"), "normal.png");
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
        assert_eq!(
            share_text_packet("hi").get_str("text").unwrap(),
            "hi"
        );
        assert!(!share_text_packet("hi").has("url"));
        assert_eq!(
            share_url_packet("https://example.org").get_str("url").unwrap(),
            "https://example.org"
        );
        assert!(!share_url_packet("https://example.org").has("filename"));
    }
}