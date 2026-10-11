// Pixel Connect — persistent application settings.
//
// Pixel Connect owns its own settings file; it never reads or writes KDE
// Connect's or rquickshare's configuration.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::{Store, StoreExt};

pub const SETTINGS_FILE: &str = ".pixel-connect-settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Close the window instead of hiding to the tray.
    pub realclose: bool,
    /// Start hidden in the tray.
    pub startminimized: bool,
    /// Launch Pixel Connect at login.
    pub autostart: bool,
    /// Google Quick Share advertising visibility (0 visible, 1 invisible, 2 temporary).
    pub visibility: u8,
    /// Optional fixed TCP port for the Quick Share service.
    pub port: Option<u32>,
    /// The single folder both protocols download into.
    ///
    /// Quick Share and KDE Connect kept separate locations, so setting one
    /// ignored the other and files arrived in a folder the user had not chosen.
    /// `download_folder` is authoritative; `download_path` and `incoming_path`
    /// are retained so an existing setting is not lost.
    pub download_folder: Option<String>,
    /// Where Quick Share files are written (superseded by `download_folder`).
    pub download_path: Option<String>,
    /// Where KDE Connect shares are written (superseded by `download_folder`).
    pub incoming_path: Option<String>,
    /// `trace` | `debug` | `info` | `warn` | `error`
    pub debug_level: Option<String>,
    /// KDE Connect clipboard synchronisation master switch.
    pub clipboard_enabled: bool,
    /// Push clipboard changes to the phone without asking.
    pub clipboard_auto_share: bool,
    /// The KDE Connect device name advertised on the LAN.
    pub kde_device_name: String,
    /// Nautilus "Send to Pixel" integration installed?
    pub nautilus_enabled: bool,
    /// Preferred device for "Send to Pixel" and Quick Share.
    pub preferred_device_id: Option<String>,
    /// Accept incoming shares without asking.
    ///
    /// Off by default. A share writes to disk without a prompt, and the phone
    /// only has to be *paired*, not actively held by its owner, so accepting
    /// silently turns a paired device into one that can drop files at will.
    /// The user opts in explicitly.
    pub auto_accept_shares: bool,
    /// Copy a received image to the clipboard as well as saving it.
    pub clipboard_images_on_receive: bool,
    /// Notifications. Each event is separately switchable so the app can be
    /// silenced per category instead of all-or-nothing.
    pub notify_shared_text: bool,
    pub notify_share_request: bool,
    pub notify_transfer_complete: bool,
    pub notify_transfer_failed: bool,
    pub notify_progress: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            realclose: false,
            startminimized: false,
            // On by default. A Quick Share receiver is only discoverable over
            // Bluetooth while this application is running, so starting at
            // login is what makes it discoverable at all without the user
            // having to launch it first.
            autostart: true,
            visibility: 0,
            port: None,
            download_folder: None,
            download_path: None,
            incoming_path: None,
            debug_level: None,
            clipboard_enabled: true,
            clipboard_auto_share: true,
            kde_device_name: String::new(),
            nautilus_enabled: false,
            preferred_device_id: None,
            // Silent acceptance is opt-in: see the field docs.
            auto_accept_shares: false,
            clipboard_images_on_receive: true,
            // Only the events that are otherwise invisible get a notification.
            // Progress is off by default because it fires repeatedly during a
            // transfer, which is the "bombardment" case.
            notify_shared_text: true,
            notify_share_request: true,
            notify_transfer_complete: true,
            notify_transfer_failed: true,
            notify_progress: false,
        }
    }
}

fn store(app: &AppHandle) -> Arc<Store<Wry>> {
    app.store_builder(SETTINGS_FILE)
        .auto_save(Duration::from_millis(150))
        .build()
        .expect("could not open the Pixel Connect settings store")
}

pub fn load(app: &AppHandle) -> Settings {
    let s = store(app);
    let d = Settings::default();
    Settings {
        realclose: s
            .get("realclose")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.realclose),
        startminimized: s
            .get("startminimized")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.startminimized),
        autostart: s
            .get("autostart")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.autostart),
        visibility: s
            .get("visibility")
            .and_then(|v| v.as_u64())
            .map(|v| v as u8)
            .unwrap_or(d.visibility),
        port: s.get("port").and_then(|v| v.as_u64()).map(|v| v as u32),
        download_folder: s
            .get("download_folder")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        download_path: s
            .get("download_path")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        incoming_path: s
            .get("incoming_path")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        debug_level: s
            .get("debug_level")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        clipboard_enabled: s
            .get("clipboard_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.clipboard_enabled),
        clipboard_auto_share: s
            .get("clipboard_auto_share")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.clipboard_auto_share),
        kde_device_name: s
            .get("kde_device_name")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(d.kde_device_name),
        nautilus_enabled: s
            .get("nautilus_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.nautilus_enabled),
        preferred_device_id: s
            .get("preferred_device_id")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        auto_accept_shares: s
            .get("auto_accept_shares")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.auto_accept_shares),
        clipboard_images_on_receive: s
            .get("clipboard_images_on_receive")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.clipboard_images_on_receive),
        notify_shared_text: s
            .get("notify_shared_text")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.notify_shared_text),
        notify_share_request: s
            .get("notify_share_request")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.notify_share_request),
        notify_transfer_complete: s
            .get("notify_transfer_complete")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.notify_transfer_complete),
        notify_transfer_failed: s
            .get("notify_transfer_failed")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.notify_transfer_failed),
        notify_progress: s
            .get("notify_progress")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.notify_progress),
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> anyhow::Result<()> {
    let s = store(app);
    s.set("realclose", settings.realclose);
    s.set("startminimized", settings.startminimized);
    s.set("autostart", settings.autostart);
    s.set("visibility", settings.visibility);
    s.set("port", settings.port);
    s.set("download_folder", settings.download_folder.clone());
    s.set("download_path", settings.download_path.clone());
    s.set("incoming_path", settings.incoming_path.clone());
    s.set("debug_level", settings.debug_level.clone());
    s.set("clipboard_enabled", settings.clipboard_enabled);
    s.set("clipboard_auto_share", settings.clipboard_auto_share);
    s.set("kde_device_name", settings.kde_device_name.clone());
    s.set("nautilus_enabled", settings.nautilus_enabled);
    s.set("preferred_device_id", settings.preferred_device_id.clone());
    s.set("auto_accept_shares", settings.auto_accept_shares);
    s.set(
        "clipboard_images_on_receive",
        settings.clipboard_images_on_receive,
    );
    s.set("notify_shared_text", settings.notify_shared_text);
    s.set("notify_share_request", settings.notify_share_request);
    s.set("notify_transfer_complete", settings.notify_transfer_complete);
    s.set("notify_transfer_failed", settings.notify_transfer_failed);
    s.set("notify_progress", settings.notify_progress);
    Ok(())
}

pub fn set(app: &AppHandle, key: &str, value: serde_json::Value) {
    store(app).set(key, value);
}

pub fn get_bool(app: &AppHandle, key: &str) -> bool {
    store(app)
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or_default()
}

pub fn get_str(app: &AppHandle, key: &str) -> Option<String> {
    store(app)
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Default KDE Connect device name: the host name, sanitised.
pub fn default_device_name() -> String {
    let host = sys_metrics::host::get_hostname().unwrap_or_else(|_| "Linux PC".into());
    pixel_connect_kdeconnect::deviceinfo::sanitize_device_name(&host)
}

/// Default download directory: `~/Downloads`, falling back to the home
/// directory and finally `/`.
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let d = Settings::default();
        assert!(d.clipboard_enabled);
        assert!(d.clipboard_auto_share);
        assert!(!d.realclose);
        assert_eq!(d.visibility, 0);
    }
}
