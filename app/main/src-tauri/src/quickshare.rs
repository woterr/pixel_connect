// Pixel Connect — Google Quick Share integration.
//
// The protocol implementation itself lives in the `pixel_connect_quickshare`
// crate (derived from rquickshare). This module owns the running service,
// forwards its events to the front end, and adds the "Send to Pixel" entry
// point that the file-manager integration and the CLI use.

use std::path::PathBuf;
use std::sync::Arc;

use pixel_connect_quickshare::channel::{ChannelAction, ChannelDirection, ChannelMessage};
use pixel_connect_quickshare::{SendInfo, Visibility};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings;

pub fn notify_send_result(app: &AppHandle, sent: usize, device: Option<&str>) {
    crate::notify_send_result(app, sent, device);
}

pub fn spawn_forwarders(app: AppHandle) {
    let message_sender = app
        .try_state::<Arc<crate::AppState>>()
        .map(|s| s.message_sender.clone());
    let endpoint_sender = app
        .try_state::<Arc<crate::AppState>>()
        .map(|s| s.endpoint_sender.clone());
    let visibility_sender = app
        .try_state::<Arc<crate::AppState>>()
        .map(|s| s.visibility_sender.clone());

    if let Some(sender) = message_sender {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut rx = sender.subscribe();
            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        if msg.direction == ChannelDirection::FrontToLib {
                            if msg.state
                                == Some(pixel_connect_quickshare::State::WaitingForUserConsent)
                            {
                                let name = msg
                                    .meta
                                    .as_ref()
                                    .and_then(|m| m.source.as_ref())
                                    .map(|s| s.name.clone())
                                    .unwrap_or_else(|| "A device".into());
                                crate::notify_request(&app, &name, &msg.id);
                            }
                            crate::show_window(&app);
                        }
                        if let Err(e) = app.emit("quickshare:event", &msg) {
                            log::error!("could not forward Quick Share event: {e}");
                        }
                    }
                    Err(e) => {
                        log::error!("Quick Share channel closed: {e}");
                        return;
                    }
                }
            }
        });
    }

    if let Some(sender) = endpoint_sender {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut rx = sender.subscribe();
            loop {
                match rx.recv().await {
                    Ok(info) => {
                        if let Err(e) = app.emit("quickshare:endpoint", &info) {
                            log::error!("could not forward Quick Share endpoint: {e}");
                        }
                    }
                    Err(e) => {
                        log::error!("Quick Share endpoint channel closed: {e}");
                        return;
                    }
                }
            }
        });
    }

    if let Some(sender) = visibility_sender {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut rx = sender.lock().unwrap().subscribe();
            loop {
                match rx.changed().await {
                    Ok(_) => {
                        let v = *rx.borrow_and_update();
                        settings::set(&app, "visibility", serde_json::json!(v as u8));
                    }
                    Err(_) => return,
                }
            }
        });
    }
}

pub async fn start_discovery(state: &Arc<crate::AppState>) -> Result<(), String> {
    state
        .quickshare
        .lock()
        .unwrap()
        .discovery(state.endpoint_sender.clone())
        .map_err(|e| format!("could not start Quick Share discovery: {e}"))
}

pub fn stop_discovery(state: &Arc<crate::AppState>) {
    state.quickshare.lock().unwrap().stop_discovery();
}

pub fn set_visibility(state: &Arc<crate::AppState>, v: Visibility) {
    state.quickshare.lock().unwrap().change_visibility(v);
}

pub fn set_download_path(state: &Arc<crate::AppState>, path: Option<PathBuf>) {
    state.quickshare.lock().unwrap().set_download_path(path);
}

pub async fn send_payload(state: &Arc<crate::AppState>, payload: SendInfo) -> Result<(), String> {
    state
        .send_sender
        .send(payload)
        .await
        .map_err(|e| format!("could not queue the Quick Share payload: {e}"))
}

pub fn send_to_lib(state: &Arc<crate::AppState>, message: ChannelMessage) -> Result<(), String> {
    state
        .message_sender
        .send(message)
        .map(|_| ())
        .map_err(|e| format!("could not deliver the message: {e}"))
}

pub fn accept_transfer(id: String) -> ChannelMessage {
    ChannelMessage {
        id,
        direction: ChannelDirection::FrontToLib,
        action: Some(ChannelAction::AcceptTransfer),
        ..Default::default()
    }
}

pub fn reject_transfer(id: String) -> ChannelMessage {
    ChannelMessage {
        id,
        direction: ChannelDirection::FrontToLib,
        action: Some(ChannelAction::RejectTransfer),
        ..Default::default()
    }
}
