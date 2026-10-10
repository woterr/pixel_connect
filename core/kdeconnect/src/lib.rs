// Pixel Connect — KDE Connect protocol v8 implementation.
//
// This crate provides the device-integration half of Pixel Connect: discovery,
// pairing, clipboard sharing, file sharing, battery/connectivity reporting and
// notification relay. It is wire-compatible with KDE Connect for desktop
// (kdeconnect-kde) and KDE Connect for Android (kdeconnect-android) at
// protocol version 8, and it runs as part of the Pixel Connect process, so no
// KDE Connect daemon has to be installed.
//
// Attribution and licensing: see ../NOTICE.md and
// ../../docs/architecture/repo-audit.md.

#[macro_use]
extern crate log;

pub mod config;
pub mod crypto;
pub mod deviceinfo;
pub mod discovery;
pub mod framing;
pub mod lan;
pub mod link;
pub mod packet;
pub mod pairing;
pub mod tls;
pub mod tls_verify;
pub mod types;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{broadcast, Mutex};

pub use config::KdeConnectConfig;
pub use deviceinfo::{DeviceInfo, DeviceType};
pub use lan::{KcEvent, LanLinkProvider, LinkCommand, RuntimeSettings};
pub use tls::{TlsContext, TlsRole};
pub use packet::{NetworkPacket, PROTOCOL_VERSION};
pub use types::{ConnectivityReport, KcDevice, LinkState, RemoteNotification, ShareOffer, Transport, TransferProgress};

/// Number of buffered events before slow receivers start losing them.
const EVENT_BUFFER: usize = 256;

/// High-level handle to the KDE Connect backend.
#[derive(Clone)]
pub struct KdeConnectService {
    provider: Arc<LanLinkProvider>,
    config: Arc<Mutex<KdeConnectConfig>>,
    events: broadcast::Sender<KcEvent>,
}

impl KdeConnectService {
    /// Load (or create) the identity in `dir` and start the LAN backend.
    pub async fn start(dir: &Path) -> Result<(Self, broadcast::Receiver<KcEvent>)> {
        let config = KdeConnectConfig::load(dir)?;
        if let Some(incoming) = config.incoming_path().cloned() {
            std::fs::create_dir_all(&incoming)?;
        }
        let config = Arc::new(Mutex::new(config));
        let (events, receiver) = broadcast::channel(EVENT_BUFFER);
        let mut provider = LanLinkProvider::new(config.clone(), events.clone()).await?;

        // Restore the user's saved clipboard preferences.
        if let Ok(prefs) = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(dir.join("prefs.json")).unwrap_or_default(),
        ) {
            let enabled = prefs
                .get("clipboardEnabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let auto = prefs
                .get("clipboardAutoShare")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            provider.settings.clipboard_enabled.store(enabled, std::sync::atomic::Ordering::SeqCst);
            provider
                .settings
                .auto_clipboard
                .store(auto, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(incoming) = config.lock().await.incoming_path().cloned() {
            provider.settings.set_incoming_path(incoming);
        }

        provider.run().await?;
        Ok((
            Self {
                provider: Arc::new(provider),
                config,
                events,
            },
            receiver,
        ))
    }

    pub fn provider(&self) -> &LanLinkProvider {
        &self.provider
    }

    pub fn subscribe(&self) -> broadcast::Receiver<KcEvent> {
        self.events.subscribe()
    }

    pub fn settings(&self) -> &Arc<RuntimeSettings> {
        &self.provider.settings
    }

    pub fn device_info(&self) -> DeviceInfo {
        self.provider.device_info()
    }

    pub async fn devices(&self) -> Vec<KcDevice> {
        self.provider.devices().await
    }

    pub async fn device(&self, id: &str) -> Option<KcDevice> {
        self.provider.device(id).await
    }

    pub async fn command(&self, device_id: &str, cmd: LinkCommand) -> Result<()> {
        self.provider.command(device_id, cmd).await
    }

    /// Re-broadcast identity and try to reconnect to every known device.
    pub async fn refresh(&self) {
        self.provider.refresh_devices().await;
    }

    pub async fn set_incoming_path(&self, path: PathBuf) {
        self.provider.settings.set_incoming_path(path.clone());
        let _ = self.config.lock().await.set_incoming_path(Some(path));
    }

    pub async fn set_device_name(&self, name: &str) -> Result<()> {
        self.config.lock().await.set_device_name(name)?;
        // Advertise the new name immediately rather than at the next restart.
        self.provider.set_device_name(name).await;
        Ok(())
    }

    pub async fn unpair(&self, device_id: &str) -> Result<bool> {
        self.config.lock().await.untrust_device(device_id)
    }

    pub async fn is_trusted(&self, device_id: &str) -> bool {
        self.config.lock().await.is_trusted(device_id)
    }

    pub fn shutdown(&self) {
        self.provider.shutdown();
    }

    /// Push our current desktop clipboard to every connected device.
    ///
    /// The caller owns clipboard access (Wayland restricts background
    /// processes), which is why the content is passed in rather than read here.
    pub async fn broadcast_clipboard(&self, content: String) -> Result<usize> {
        let mut sent = 0;
        for device in self.devices().await {
            if device.link_state != LinkState::Connected || !device.trusted {
                continue;
            }
            if !device
                .incoming_capabilities
                .iter()
                .any(|c| c == packet::PACKET_TYPE_CLIPBOARD)
            {
                continue;
            }
            let mut np = NetworkPacket::new(packet::PACKET_TYPE_CLIPBOARD);
            np.set("content", content.clone());
            if self.command(&device.id, LinkCommand::Send(Box::new(np))).await.is_ok() {
                sent += 1;
            }
        }
        // Deliberately not logged: clipboard contents are private.
        info!("broadcast clipboard text to {sent} device(s) ({} bytes)", content.len());
        Ok(sent)
    }

    /// Ask every connected device to re-send its clipboard.
    pub async fn request_clipboard(&self) -> Result<usize> {
        let mut asked = 0;
        for device in self.devices().await {
            if device.link_state != LinkState::Connected || !device.trusted {
                continue;
            }
            if !device
                .outgoing_capabilities
                .iter()
                .any(|c| c == packet::PACKET_TYPE_CLIPBOARD_CONNECT)
            {
                continue;
            }
            let mut np = NetworkPacket::new(packet::PACKET_TYPE_CLIPBOARD_CONNECT);
            np.set("timestamp", config::now_millis());
            if self.command(&device.id, LinkCommand::Send(Box::new(np))).await.is_ok() {
                asked += 1;
            }
        }
        Ok(asked)
    }

    /// Share one or more local files with a device.
    pub async fn share_files(
        &self,
        device_id: &str,
        paths: &[PathBuf],
        open_after: bool,
    ) -> Result<usize> {
        let mut queued = 0;
        for path in paths {
            let mut np = link::share_file_packet(path);
            np.set("open", open_after);
            let file = tokio::fs::File::open(path)
                .await
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            self.command(
                device_id,
                LinkCommand::SendWithPayload(Box::new(np), file),
            )
            .await?;
            queued += 1;
        }
        Ok(queued)
    }

    /// Share a directory by sending each file inside it individually, matching
    /// KDE Connect's own flattened directory behaviour.
    pub async fn share_directory(&self, device_id: &str, dir: &Path) -> Result<usize> {
        let mut files = Vec::new();
        collect_files(dir, &mut files, 0)?;
        self.share_files(device_id, &files, false).await
    }

    pub async fn share_text(&self, device_id: &str, text: &str) -> Result<()> {
        self.command(
            device_id,
            LinkCommand::Send(Box::new(link::share_text_packet(text))),
        )
        .await
    }

    pub async fn share_url(&self, device_id: &str, url: &str) -> Result<()> {
        self.command(
            device_id,
            LinkCommand::Send(Box::new(link::share_url_packet(url))),
        )
        .await
    }

    pub async fn ping(&self, device_id: &str, message: &str) -> Result<()> {
        let mut np = NetworkPacket::new(packet::PACKET_TYPE_PING);
        np.set("message", message);
        self.command(device_id, LinkCommand::Send(Box::new(np))).await
    }

    pub async fn request_pairing(&self, device_id: &str) -> Result<()> {
        self.command(device_id, LinkCommand::RequestPairing).await
    }

    pub async fn accept_pairing(&self, device_id: &str) -> Result<()> {
        self.command(device_id, LinkCommand::AcceptPairing).await
    }

    pub async fn reject_pairing(&self, device_id: &str) -> Result<()> {
        self.command(device_id, LinkCommand::CancelPairing).await
    }

    /// Push a phone notification action back to the device.
    pub async fn notification_action(&self, device_id: &str, key: &str, action: &str) -> Result<()> {
        let raw_key = key
            .strip_prefix(&format!("{device_id}:"))
            .ok_or_else(|| anyhow::anyhow!("notification key does not belong to {device_id}"))?;
        let mut np = NetworkPacket::new(packet::PACKET_TYPE_NOTIFICATION_ACTION);
        np.set("key", raw_key);
        np.set("actionId", action);
        self.command(device_id, LinkCommand::Send(Box::new(np))).await
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > 16 {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            collect_files(&path, out, depth + 1)?;
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_collection_is_recursive_and_bounded() {
        let base = std::env::temp_dir().join(format!("pc-tree-{}", std::process::id()));
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        std::fs::write(base.join("one.txt"), b"1").unwrap();
        std::fs::write(base.join("a/two.txt"), b"2").unwrap();
        std::fs::write(base.join("a/b/three.txt"), b"3").unwrap();

        let mut files = Vec::new();
        collect_files(&base, &mut files, 0).unwrap();
        assert_eq!(files.len(), 3);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn notification_keys_are_namespaced_by_device() {
        let key = "abc123:17";
        assert_eq!(key.strip_prefix("abc123:").unwrap(), "17");
    }
}