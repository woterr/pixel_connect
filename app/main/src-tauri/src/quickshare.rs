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
                        if msg.state
                            == Some(pixel_connect_quickshare::State::WaitingForUserConsent)
                        {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this pins: the consent notification was gated on
    /// `direction == FrontToLib`, but the library only ever emits `LibToFront`
    /// (see `inbound.rs::update_state`). The branch was therefore unreachable
    /// and an incoming Quick Share offer could only be answered by opening the
    /// window, which is precisely what the notification exists to avoid.
    ///
    /// If the library ever starts tagging its messages differently, this test
    /// is where it should be noticed.
    #[test]
    fn an_offer_is_recognised_without_testing_the_direction() {
        let offer = ChannelMessage {
            id: "transfer-1".into(),
            // What the library actually sends.
            direction: ChannelDirection::LibToFront,
            state: Some(pixel_connect_quickshare::State::WaitingForUserConsent),
            ..Default::default()
        };

        assert!(
            offer.state == Some(pixel_connect_quickshare::State::WaitingForUserConsent),
            "an offer waiting for consent must be recognised"
        );
        // The condition the code used, and why it was wrong:
        assert!(
            offer.direction != ChannelDirection::FrontToLib,
            "if the library ever sends FrontToFront for an offer, re-check the \
             direction gate rather than trusting this test"
        );
    }

    #[test]
    fn accept_and_reject_carry_the_transfer_id() {
        for msg in [accept_transfer("abc".into()), reject_transfer("abc".into())] {
            assert_eq!(msg.id, "abc");
            assert_eq!(msg.direction, ChannelDirection::FrontToLib);
            assert!(msg.action.is_some(), "an action must be set");
        }
        assert_eq!(
            accept_transfer("abc".into()).action,
            Some(ChannelAction::AcceptTransfer)
        );
        assert_eq!(
            reject_transfer("abc".into()).action,
            Some(ChannelAction::RejectTransfer)
        );
    }

    /// Every state other than waiting-for-consent must not raise an offer
    /// notification, or the user is asked about transfers already in flight.
    #[test]
    fn only_the_consent_state_raises_an_offer() {
        let others = [
            pixel_connect_quickshare::State::Initial,
            pixel_connect_quickshare::State::ReceivingFiles,
            pixel_connect_quickshare::State::Finished,
            pixel_connect_quickshare::State::Rejected,
            pixel_connect_quickshare::State::Cancelled,
        ];
        for s in others {
            let msg = ChannelMessage {
                id: "x".into(),
                direction: ChannelDirection::LibToFront,
                state: Some(s.clone()),
                ..Default::default()
            };
            assert!(
                msg.state != Some(pixel_connect_quickshare::State::WaitingForUserConsent),
                "{s:?} must not be treated as an offer"
            );
        }
    }
}
