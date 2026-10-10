// Pixel Connect — KDE Connect integration layer.
//
// Owns the `KdeConnectService`, converts its events into serialisable payloads
// for the Vue front end, and exposes the commands the UI needs.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use pixel_connect_kdeconnect::lan::LinkCommand;
use pixel_connect_kdeconnect::types::RemoteNotification;
use pixel_connect_kdeconnect::{
    KcDevice, KcEvent, KdeConnectService, NetworkPacket, ShareOffer, TransferProgress,
};

use crate::settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingShare {
    pub id: String,
    pub device_id: String,
    pub device_name: String,
    pub kind: String,
    pub filename: Option<String>,
    pub filesize: Option<u64>,
    pub text: Option<String>,
    pub url: Option<String>,
    pub number_of_files: Option<u64>,
    pub total_payload_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityState {
    pub device_id: String,
    pub wifi_strength: Option<i64>,
    pub wifi_name: Option<String>,
    pub bluetooth_strength: Option<i64>,
    pub cellular_strength: Option<i64>,
    pub cellular_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryState {
    pub device_id: String,
    pub level: u8,
    pub charging: bool,
}

pub struct KdcState {
    pub service: Arc<Mutex<KdeConnectService>>,
    pub shares: Arc<Mutex<Vec<PendingShare>>>,
    pub notifications: Arc<Mutex<Vec<RemoteNotification>>>,
    pub connectivity: Arc<Mutex<Vec<ConnectivityState>>>,
    pub battery: Arc<Mutex<Vec<BatteryState>>>,
}

impl KdcState {
    pub fn new(service: KdeConnectService) -> Self {
        Self {
            service: Arc::new(Mutex::new(service)),
            shares: Arc::new(Mutex::new(Vec::new())),
            notifications: Arc::new(Mutex::new(Vec::new())),
            connectivity: Arc::new(Mutex::new(Vec::new())),
            battery: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

pub fn kdeconnect_config_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("kdeconnect")
}

/// Persist the user's clipboard preferences next to the trust store so they
/// survive restarts without touching the Tauri store mid-flight.
pub fn persist_clipboard_prefs(app: &AppHandle) {
    let dir = kdeconnect_config_dir(app);
    let _ = std::fs::create_dir_all(&dir);
    let s = settings::load(app);
    let body = serde_json::json!({
        "clipboardEnabled": s.clipboard_enabled,
        "clipboardAutoShare": s.clipboard_auto_share,
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&body) {
        let _ = std::fs::write(dir.join("prefs.json"), bytes);
    }
}

pub fn spawn_event_forwarder(app: AppHandle) {
    let state = app.state::<Arc<KdcState>>().inner().clone();
    // `subscribe` only needs a shared borrow, and the lock must be taken on a
    // worker thread because `setup` may still be inside the async runtime.
    let mut rx = {
        let svc = state.service.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let joined = std::thread::spawn(move || {
            let _ = tx.send(svc.blocking_lock().subscribe());
        })
        .join();
        match joined.ok().and_then(|_| rx.recv().ok()) {
            Some(receiver) => receiver,
            None => {
                error!("could not subscribe to KDE Connect events");
                return;
            }
        }
    };

    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(e) = handle_event(&app, &state, event).await {
                        log::error!("kdeconnect event handling failed: {e}");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("kdeconnect event stream lagged by {n} events");
                }
                Err(e) => {
                    log::error!("kdeconnect event stream closed: {e}");
                    return;
                }
            }
        }
    });
}

async fn handle_event(app: &AppHandle, state: &Arc<KdcState>, event: KcEvent) -> Result<()> {
    match event {
        KcEvent::Device(device) => {
            let _ = app.emit("kdeconnect:device", *device);
        }
        KcEvent::Removed(id) => {
            let _ = app.emit("kdeconnect:removed", id);
        }
        KcEvent::PairingRequest { device, .. } => {
            let _ = app.emit("kdeconnect:pairing_request", *device);
        }
        KcEvent::Clipboard {
            device_id,
            device_name,
            content,
        } => {
            // Apply to the desktop clipboard, and remember it so the resulting
            // local change is not sent straight back to the phone.
            crate::clipboard::remember_applied(&content);
            crate::clipboard::set_text(content.clone());
            let _ = app.emit(
                "kdeconnect:clipboard",
                serde_json::json!({ "deviceId": device_id, "deviceName": device_name }),
            );
        }
        KcEvent::ShareOffer(offer) => {
            let pending = to_pending(&offer);
            state.shares.lock().await.push(pending.clone());
            let _ = app.emit("kdeconnect:share", pending);
        }
        KcEvent::Notification {
            device_id,
            device_name,
            notification,
        } => {
            // Namespace by device so the UI can tell identical ids from two
            // phones apart and every reaction can find its own notification.
            let mut n = *notification;
            n.id = format!("{device_id}:{}", n.id);
            let mut list = state.notifications.lock().await;
            list.retain(|e| e.id != n.id);
            list.insert(0, n.clone());
            if list.len() > 100 {
                list.truncate(100);
            }
            drop(list);
            let _ = app.emit("kdeconnect:notification", n);
            let _ = app.emit("kdeconnect:notification_from", device_name);
        }
        KcEvent::NotificationReplied {
            device_id,
            notification_id,
        } => {
            let qualified = format!("{device_id}:{notification_id}");
            state
                .notifications
                .lock()
                .await
                .retain(|e| e.id != qualified);
            let _ = app.emit("kdeconnect:notification_replied", qualified);
        }
        KcEvent::Connectivity(c) => {
            let entry = ConnectivityState {
                device_id: c.device_id.clone(),
                wifi_strength: c.wifi_strength,
                wifi_name: c.wifi_name.clone(),
                bluetooth_strength: c.bluetooth_strength,
                cellular_strength: c.cellular_strength,
                cellular_name: c.cellular_name.clone(),
            };
            let _ = app.emit("kdeconnect:connectivity", *c);
            let mut list = state.connectivity.lock().await;
            list.retain(|e| e.device_id != entry.device_id);
            list.push(entry);
        }
        KcEvent::Battery {
            device_id,
            level,
            charging,
        } => {
            let entry = BatteryState {
                device_id: device_id.clone(),
                level,
                charging,
            };
            let _ = app.emit("kdeconnect:battery", entry.clone());
            let mut list = state.battery.lock().await;
            list.retain(|e| e.device_id != entry.device_id);
            list.push(entry);
        }
        KcEvent::TransferProgress(t) => {
            let _ = app.emit("kdeconnect:transfer", *t);
        }
        KcEvent::Log { level, message } => match level.as_str() {
            "error" => log::error!("kdeconnect: {message}"),
            "warn" => log::warn!("kdeconnect: {message}"),
            _ => log::info!("kdeconnect: {message}"),
        },
    }
    Ok(())
}

fn to_pending(offer: &ShareOffer) -> PendingShare {
    PendingShare {
        id: format!(
            "{}:{}:{}",
            offer.device_id,
            offer.kind,
            offer.filename.clone().unwrap_or_default()
        ),
        device_id: offer.device_id.clone(),
        device_name: offer.device_name.clone(),
        kind: offer.kind.clone(),
        filename: offer.filename.clone(),
        filesize: offer.filesize,
        text: offer.text.clone(),
        url: offer.url.clone(),
        number_of_files: offer.number_of_files,
        total_payload_size: offer.total_payload_size,
    }
}

pub async fn devices(state: &KdcState) -> Vec<KcDevice> {
    let svc = state.service.lock().await;
    svc.devices().await
}

pub async fn device_info(state: &KdcState) -> pixel_connect_kdeconnect::DeviceInfo {
    let svc = state.service.lock().await;
    svc.device_info()
}

pub async fn share_text(state: &KdcState, device_id: &str, text: &str) -> Result<()> {
    let svc = state.service.lock().await;
    svc.share_text(device_id, text).await
}

pub async fn share_files(state: &KdcState, device_id: &str, paths: Vec<PathBuf>) -> Result<usize> {
    let svc = state.service.lock().await;
    svc.share_files(device_id, &paths, false).await
}

pub async fn share_directory(state: &KdcState, device_id: &str, dir: PathBuf) -> Result<usize> {
    let svc = state.service.lock().await;
    svc.share_directory(device_id, &dir).await
}

pub async fn request_clipboard(state: &KdcState) -> Result<usize> {
    let svc = state.service.lock().await;
    svc.request_clipboard().await
}

pub async fn broadcast_clipboard(state: &KdcState, content: String) -> Result<usize> {
    let svc = state.service.lock().await;
    svc.broadcast_clipboard(content).await
}

pub async fn request_pairing(state: &KdcState, device_id: &str) -> Result<()> {
    let svc = state.service.lock().await;
    svc.request_pairing(device_id).await
}

pub async fn accept_pairing(state: &KdcState, device_id: &str) -> Result<()> {
    let svc = state.service.lock().await;
    svc.accept_pairing(device_id).await
}

pub async fn reject_pairing(state: &KdcState, device_id: &str) -> Result<()> {
    let svc = state.service.lock().await;
    svc.reject_pairing(device_id).await
}

pub async fn unpair(state: &KdcState, device_id: &str) -> Result<bool> {
    let svc = state.service.lock().await;
    svc.unpair(device_id).await
}

pub async fn refresh(state: &KdcState) {
    let svc = state.service.lock().await;
    svc.refresh().await;
}

pub async fn set_device_name(state: &KdcState, name: &str) -> Result<()> {
    let svc = state.service.lock().await;
    svc.set_device_name(name).await
}

pub async fn set_clipboard_enabled(state: &KdcState, enabled: bool) {
    let svc = state.service.lock().await;
    svc.settings()
        .clipboard_enabled
        .store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub async fn set_clipboard_auto_share(state: &KdcState, enabled: bool) {
    let svc = state.service.lock().await;
    svc.settings()
        .auto_clipboard
        .store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub async fn pending_shares(state: &KdcState) -> Vec<PendingShare> {
    state.shares.lock().await.clone()
}

pub async fn dismiss_share(state: &KdcState, id: &str) {
    state.shares.lock().await.retain(|s| s.id != id);
}

pub async fn notifications(state: &KdcState) -> Vec<RemoteNotification> {
    state.notifications.lock().await.clone()
}

pub async fn clear_notifications(state: &KdcState) {
    state.notifications.lock().await.clear();
}

/// Invoke a notification action. `key` is the qualified `<deviceId>:<id>` key
/// the UI holds.
pub async fn notification_action(
    state: &KdcState,
    device_id: &str,
    key: &str,
    action: &str,
) -> Result<()> {
    let svc = state.service.lock().await;
    svc.notification_action(device_id, key, action).await
}

/// Send a typed reply to a repliable notification.
///
/// Looks the notification up so the caller only has to supply the message; the
/// `requestReplyId` the phone issued is taken from the stored notification,
/// which is what the reply packet must echo.
pub async fn notification_reply(
    state: &KdcState,
    device_id: &str,
    notification_id: &str,
    text: &str,
) -> Result<()> {
    let reply_id = {
        let list = state.notifications.lock().await;
        list.iter()
            .find(|n| n.id == notification_id)
            .and_then(|n| n.request_reply_id.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("that notification is no longer available to reply to")
            })?
    };
    let svc = state.service.lock().await;
    svc.notification_reply(device_id, notification_id, &reply_id, text)
        .await
}

pub async fn ping(state: &KdcState, device_id: &str, message: &str) -> Result<()> {
    let svc = state.service.lock().await;
    svc.ping(device_id, message).await
}

#[allow(dead_code)]
fn _unused(_: LinkCommand, _: NetworkPacket, _: TransferProgress) {}
