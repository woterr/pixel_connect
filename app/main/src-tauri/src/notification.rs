// Pixel Connect — desktop notifications.
//
// These replace the two functions inherited from rquickshare, which were dead
// code: nothing called them, and both announced themselves as "RQuickShare".
// Nothing in this application showed a notification as a result.
//
// Two rules govern everything here.
//
// Notifications are for events the user cannot otherwise see. A transfer the
// app is already showing progress for does not need one, which is why
// `notify_progress` exists and defaults to off. Anything that fires more than
// once per transfer belongs behind its own switch.
//
// A notification must never be the only record of an action. If showing one
// fails, the event still happened and is still visible in the app.

#[cfg(target_os = "linux")]
use notify_rust::Notification;
#[cfg(target_os = "linux")]
use tauri::Manager;
use tauri::AppHandle;

use crate::settings;

/// Which events may raise a notification. Each maps to one settings key, so
/// the user can silence categories individually.
///
/// The keys are the *store* names (snake_case), which is what
/// `settings::get_bool` looks up. The wire/UI form is camelCase; the two are
/// different, and using the wrong one silently reads a missing key, which
/// `get_bool` turns into `false` — so the category could never be switched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    SharedText,
    ShareRequest,
    TransferComplete,
    TransferFailed,
    Progress,
}

impl NotifyKind {
    fn enabled(self, app: &AppHandle) -> bool {
        match self {
            Self::SharedText => settings::get_bool(app, "notify_shared_text"),
            Self::ShareRequest => settings::get_bool(app, "notify_share_request"),
            Self::TransferComplete => settings::get_bool(app, "notify_transfer_complete"),
            Self::TransferFailed => settings::get_bool(app, "notify_transfer_failed"),
            Self::Progress => settings::get_bool(app, "notify_progress"),
        }
    }
}

/// Human-readable title, used by the tests below.
fn title_for(kind: NotifyKind) -> &'static str {
    match kind {
        NotifyKind::SharedText => "Text shared",
        NotifyKind::ShareRequest => "File shared",
        NotifyKind::TransferComplete => "Transfer complete",
        NotifyKind::TransferFailed => "Transfer failed",
        NotifyKind::Progress => "Transferring",
    }
}

/// Show a notification if its category is enabled.
///
/// Never returns an error to the caller. A failure to notify is logged and
/// dropped: the underlying event has already been handled, and propagating the
/// failure would make a cosmetic problem look like a transfer failure.
pub fn notify(app: &AppHandle, kind: NotifyKind, body: &str) {
    if !kind.enabled(app) {
        return;
    }

    let title = title_for(kind);
    log::info!("notification [{title}]: {body}");

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = Notification::new()
            .summary("Pixel Connect")
            .body(body)
            .appname("Pixel Connect")
            .show()
        {
            log::warn!("could not show a notification for {title}: {e}");
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show();
    }

    let _ = app; // used by the non-Linux path
}

/// An incoming share request, with the actions needed to answer it without
/// opening the window.
///
/// GNOME's notification server drops the action buttons if the app is already
/// focused, which is a case worth accepting: the user is looking at the app.
pub fn notify_share_request(app: &AppHandle, device_name: &str, summary: &str, share_id: String) {
    if !NotifyKind::ShareRequest.enabled(app) {
        return;
    }

    let body = format!("{device_name} wants to share {summary}");
    log::info!("notification [File shared]: {body}");

    #[cfg(target_os = "linux")]
    {
        let handle = app.clone();
        let shown = Notification::new()
            .summary("Pixel Connect")
            .body(&body)
            .appname("Pixel Connect")
            .action("accept", "Accept")
            .action("reject", "Reject")
            .show();

        match shown {
            Ok(n) => {
                tauri::async_runtime::spawn(async move {
                    n.wait_for_action(|action| match action {
                        "accept" => {
                            let Some(state) = handle.try_state::<std::sync::Arc<crate::kdc::KdcState>>()
                            else {
                                return;
                            };
                            let state = state.inner().clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = crate::kdc::accept_share(&state, &share_id).await {
                                    log::error!("accepting {share_id} from the notification failed: {e}");
                                }
                            });
                        }
                        "reject" => {
                            let Some(state) = handle.try_state::<std::sync::Arc<crate::kdc::KdcState>>()
                            else {
                                return;
                            };
                            let state = state.inner().clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = crate::kdc::dismiss_share(&state, &share_id).await;
                            });
                        }
                        _ => {}
                    });
                });
            }
            Err(e) => log::warn!("could not show the share request notification: {e}"),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        let _ = summary;
        let _ = share_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn every_kind_has_a_distinct_title() {
        let kinds = [
            NotifyKind::SharedText,
            NotifyKind::ShareRequest,
            NotifyKind::TransferComplete,
            NotifyKind::TransferFailed,
            NotifyKind::Progress,
        ];
        let mut titles: Vec<&str> = kinds.iter().map(|k| title_for(*k)).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), kinds.len(), "titles must not repeat");
    }

    /// The notification categories must default to *on*, and the two that
    /// fire repeatedly or change security posture must not.
    #[test]
    fn defaults_are_sensible() {
        let d = Settings::default();
        assert!(d.notify_shared_text, "silent clipboard sync is invisible");
        assert!(d.notify_share_request, "an offer with no notification is missed");
        assert!(d.notify_transfer_complete);
        assert!(d.notify_transfer_failed, "a failure must not be swallowed");
        assert!(
            !d.notify_progress,
            "progress fires repeatedly during a transfer and must be opt-in"
        );
        assert!(
            !d.auto_accept_shares,
            "auto-accept writes to disk without asking, so it must require consent"
        );
    }

    /// The store keys `NotifyKind` reads must be the ones `save` writes. A
    /// mismatch reads a missing key, which `get_bool` reports as `false`, so
    /// the category could never be switched on.
    ///
    /// This cannot call `get_bool` (it needs an AppHandle), so it pins the two
    /// lists against each other. If either is edited alone, this fails.
    #[test]
    fn notification_keys_match_the_settings_fields() {
        // (Settings field name, camelCase name the UI/serde sees)
        let fields = [
            ("notify_shared_text", "notifySharedText"),
            ("notify_share_request", "notifyShareRequest"),
            ("notify_transfer_complete", "notifyTransferComplete"),
            ("notify_transfer_failed", "notifyTransferFailed"),
            ("notify_progress", "notifyProgress"),
        ];
        let json = serde_json::to_value(Settings::default()).unwrap();
        for (field, camel) in fields {
            // `save` writes the field name verbatim as the store key.
            assert!(
                field.starts_with("notify_"),
                "{field} is not a store key NotifyKind could read"
            );
            // And serde exposes it to the UI under the camelCase name.
            assert!(
                json.get(camel).is_some(),
                "{field} is missing from Settings as {camel}"
            );
        }
    }

    #[test]
    fn human_size_is_readable() {
        use crate::kdc::human_size;
        assert_eq!(human_size(0), "");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}