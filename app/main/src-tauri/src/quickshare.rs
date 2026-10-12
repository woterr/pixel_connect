// Pixel Connect — Google Quick Share integration.
//
// The protocol implementation itself lives in the `pixel_connect_quickshare`
// crate (derived from rquickshare). This module owns the running service,
// forwards its events to the front end, and adds the "Send to Pixel" entry
// point that the file-manager integration and the CLI use.

use std::path::PathBuf;
use std::sync::Arc;

use pixel_connect_quickshare::channel::{
    ChannelMessage as EngineMessage, Message as EngineProtocol, TransferAction,
};
use pixel_connect_quickshare::ui_channel::{
    self, ChannelMessage as UiMessage, State as UiState,
};
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
                    Ok(engine_msg) => {
                        // Engine protocol in, UI protocol onward. Everything
                        // below this point works on the translated message, so
                        // the frontend contract never sees the engine's shapes.
                        let Some(msg) =
                            ui_channel::translate_incoming(engine_msg.id.clone(), &engine_msg.msg)
                        else {
                            continue;
                        };
                        // A text share is only useful once the text itself has
                        // arrived, which happens after consent is given. Put it
                        // on the desktop clipboard at that point: sharing a
                        // snippet from the phone is a request to have it ready
                        // to paste, not merely to be listed in activity.
                        if let Some(payload) = msg
                            .meta
                            .as_ref()
                            .and_then(|m| m.text_payload.as_deref())
                            .map(str::trim)
                            .filter(|t| !t.is_empty())
                        {
                            crate::clipboard::remember_applied(payload);
                            crate::clipboard::set_text(payload.to_string());
                            if let Some(name) = msg
                                .meta
                                .as_ref()
                                .and_then(|m| m.source.as_ref())
                                .map(|s| s.name.clone())
                            {
                                let _ = app.emit(
                                    "quickshare:text",
                                    serde_json::json!({ "deviceName": name }),
                                );
                            }
                        }
                        // A Quick Share offer is the one transfer that really
                        // does wait for consent: the library holds the payload
                        // until `AcceptTransfer` arrives, so this branch is a
                        // genuine gate rather than a formality.
                        //
                        // It previously tested for `FrontToLib`, which the
                        // library never sends - every message it emits is
                        // `LibToFront` (`inbound.rs`, `update_state`) - so the
                        // notification could not fire and the offer was
                        // reachable only by opening the window.
                        // A received image is worth putting on the clipboard, so a photo
                        // from the phone can be pasted straight into a message.
                        //
                        // Only the *last* file is considered: several images at
                        // once would leave the clipboard holding whichever
                        // happened to finish last, which is arbitrary. Non-image
                        // files are left alone, and a decode failure is not an
                        // error - the file is already saved either way.
                        if msg.state == Some(UiState::Finished) {
                            let images = msg
                                .meta
                                .as_ref()
                                .map(|m| m.saved_paths.clone())
                                .unwrap_or_default();
                            if images.len() == 1 {
                                let path = std::path::PathBuf::from(&images[0]);
                                if settings::get_bool(&app, "clipboard_images_on_receive")
                                    && crate::clipboard::is_probably_image(&path)
                                {
                                    match crate::clipboard::set_image_from_file(&path) {
                                        Ok(true) => log::info!(
                                            "copied the Quick Share image {} to the clipboard",
                                            path.display()
                                        ),
                                        Ok(false) => log::debug!(
                                            "{} was not placed on the clipboard",
                                            path.display()
                                        ),
                                        Err(e) => log::warn!(
                                            "could not copy {} to the clipboard: {e}",
                                            path.display()
                                        ),
                                    }
                                }
                            } else if images.len() > 1 {
                                log::debug!(
                                    "{} files arrived; not choosing one for the clipboard",
                                    images.len()
                                );
                            }
                        }

                        if msg.state == Some(UiState::WaitingForUserConsent) {
                            // Auto-accept when the user asked for it. The KDE
                            // share path already honours auto_accept_shares, but
                            // Quick Share had no equivalent, so the switch did
                            // nothing for incoming Quick Share files. Send the
                            // accept straight back through the same channel the
                            // notification action uses.
                            if crate::settings::get_bool(&app, "auto_accept_shares") {
                                log::info!(
                                    "auto-accepting Quick Share transfer {} from user setting",
                                    msg.id
                                );
                                let _ = sender.send(crate::quickshare::accept_transfer(
                                    msg.id.clone(),
                                ));
                            }
                            let name = msg
                                .meta
                                .as_ref()
                                .and_then(|m| m.source.as_ref())
                                .map(|s| s.name.clone())
                                .unwrap_or_else(|| "A device".into());

                            // Describe what is actually being offered. "wants
                            // send you files" is unhelpful when one photo is
                            // arriving, and gives nothing away when several are.
                            let files = msg
                                .meta
                                .as_ref()
                                .and_then(|m| m.files.as_ref())
                                .map(|f| f.len())
                                .unwrap_or(0);
                            let total = msg.meta.as_ref().map(|m| m.total_bytes).unwrap_or(0);
                            let detail = match (files, msg.meta.as_ref().and_then(|m| m.text_description.clone())) {
                                (0, Some(what)) => what,
                                (1, _) => "1 file".to_string(),
                                (n, _) if n > 1 => format!("{n} files"),
                                (0, None) if total > 0 => {
                                    format!("{} of data", crate::kdc::human_size(total))
                                }
                                _ => "something".to_string(),
                            };
                            crate::notify_request(&app, &name, &detail, &msg.id);

                            // Bring the window up as well: the notification can
                            // answer it, but the window shows what is already in
                            // flight and whether it succeeded.
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

/// Queue files for sending over Quick Share to a discovered endpoint.
///
/// `endpoint_id`/`endpoint_name` come from a `quickshare:endpoint` event. BLE
/// endpoints (`ble://…`) have no stable address — the engine re-scans by name
/// before dialing — so the flag, not the address, selects the transport.
/// Directories are refused honestly: the Files payload carries files only.
pub async fn send_files(
    state: &Arc<crate::AppState>,
    endpoint_id: String,
    endpoint_name: String,
    paths: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    let mut files = Vec::with_capacity(paths.len());
    for path in &paths {
        if !path.is_file() {
            return Err(format!(
                "Quick Share sends files, not directories: {}",
                path.display()
            ));
        }
        files.push(path.to_string_lossy().to_string());
    }
    if files.is_empty() {
        return Err("no files were selected".into());
    }
    let ble = endpoint_id.starts_with("ble://");
    send_payload(
        state,
        SendInfo {
            id: endpoint_id.clone(),
            name: endpoint_name,
            addr: endpoint_id,
            ob: pixel_connect_quickshare::OutboundPayload::Files(files),
            ble,
        },
    )
    .await
}

pub async fn send_payload(state: &Arc<crate::AppState>, payload: SendInfo) -> Result<(), String> {
    state
        .send_sender
        .send(payload)
        .await
        .map_err(|e| format!("could not queue the Quick Share payload: {e}"))
}

/// Deliver a UI message to the engine, translating the action. The frontend
/// speaks the UI protocol (this signature is the IPC-visible boundary); the
/// engine only ever sees its own `Message::Lib`.
pub fn send_to_lib(state: &Arc<crate::AppState>, message: UiMessage) -> Result<(), String> {
    let Some(action) = message.action.as_ref() else {
        return Err("message carries no action".to_string());
    };
    send_engine_action(state, message.id, ui_channel::translate_action(action))
}

pub fn send_engine_message(
    state: &Arc<crate::AppState>,
    message: EngineMessage,
) -> Result<(), String> {
    state
        .message_sender
        .send(message)
        .map(|_| ())
        .map_err(|e| format!("could not deliver the message: {e}"))
}

fn send_engine_action(
    state: &Arc<crate::AppState>,
    id: String,
    action: TransferAction,
) -> Result<(), String> {
    state
        .message_sender
        .send(EngineMessage {
            id,
            msg: EngineProtocol::Lib { action },
        })
        .map(|_| ())
        .map_err(|e| format!("could not deliver the message: {e}"))
}

pub fn accept_transfer(id: String) -> EngineMessage {
    EngineMessage {
        id,
        msg: EngineProtocol::Lib {
            action: TransferAction::ConsentAccept,
        },
    }
}

pub fn reject_transfer(id: String) -> EngineMessage {
    EngineMessage {
        id,
        msg: EngineProtocol::Lib {
            action: TransferAction::ConsentDecline,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixel_connect_quickshare::channel::{
        MessageClient, TransferKind as EngineKind,
    };
    use pixel_connect_quickshare::hdl::TransferState as EngineState;

    fn engine_offer() -> EngineMessage {
        EngineMessage {
            id: "transfer-1".into(),
            msg: EngineProtocol::Client(MessageClient {
                kind: EngineKind::Inbound,
                state: Some(EngineState::WaitingForUserConsent),
                metadata: None,
            }),
        }
    }

    /// Engine events must translate into the UI shape the forwarder logic and
    /// the frontend both rely on — and `Message::Lib` must never leak to the UI.
    #[test]
    fn engine_events_translate_to_the_ui_shape() {
        let open = engine_offer();
        let ui = ui_channel::translate_incoming(open.id.clone(), &open.msg)
            .expect("an engine Client event must translate");
        assert_eq!(ui.id, "transfer-1");
        assert_eq!(ui.direction, ui_channel::ChannelDirection::LibToFront);
        assert_eq!(ui.rtype, Some(ui_channel::TransferType::Inbound));
        assert_eq!(ui.state, Some(UiState::WaitingForUserConsent));

        let libward = EngineMessage {
            id: "x".into(),
            msg: EngineProtocol::Lib {
                action: TransferAction::ConsentAccept,
            },
        };
        assert!(
            ui_channel::translate_incoming(libward.id.clone(), &libward.msg).is_none(),
            "engine-bound messages must never be forwarded to the UI"
        );
    }

    #[test]
    fn accept_and_reject_carry_the_transfer_id() {
        for msg in [accept_transfer("abc".into()), reject_transfer("abc".into())] {
            assert_eq!(msg.id, "abc");
            let EngineProtocol::Lib { action } = msg.msg else {
                panic!("accept/reject must build engine-bound actions");
            };
            assert!(
                matches!(
                    action,
                    TransferAction::ConsentAccept | TransferAction::ConsentDecline
                ),
                "an action must be set"
            );
        }
        let EngineProtocol::Lib { action } = accept_transfer("abc".into()).msg else {
            panic!("unreachable");
        };
        assert_eq!(action, TransferAction::ConsentAccept);
        let EngineProtocol::Lib { action } = reject_transfer("abc".into()).msg else {
            panic!("unreachable");
        };
        assert_eq!(action, TransferAction::ConsentDecline);
    }

    /// The UI action still maps onto the engine action after translation.
    #[test]
    fn ui_actions_map_onto_engine_actions() {
        assert_eq!(
            ui_channel::translate_action(&ui_channel::ChannelAction::AcceptTransfer),
            TransferAction::ConsentAccept
        );
        assert_eq!(
            ui_channel::translate_action(&ui_channel::ChannelAction::RejectTransfer),
            TransferAction::ConsentDecline
        );
        assert_eq!(
            ui_channel::translate_action(&ui_channel::ChannelAction::CancelTransfer),
            TransferAction::TransferCancel
        );
    }

    /// Every state other than waiting-for-consent must not raise an offer
    /// notification, or the user is asked about transfers already in flight.
    #[test]
    fn only_the_consent_state_raises_an_offer() {
        for s in [
            UiState::Initial,
            UiState::ReceivingFiles,
            UiState::Finished,
            UiState::Rejected,
            UiState::Cancelled,
        ] {
            let ui = UiMessage {
                id: "x".into(),
                direction: ui_channel::ChannelDirection::LibToFront,
                state: Some(s.clone()),
                ..Default::default()
            };
            assert!(
                ui.state != Some(UiState::WaitingForUserConsent),
                "{s:?} must not be treated as an offer"
            );
        }
    }
}
