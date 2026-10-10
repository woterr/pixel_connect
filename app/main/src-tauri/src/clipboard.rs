// Pixel Connect — desktop clipboard integration.
//
// Clipboard synchronisation is the one feature that has to fight the display
// server rather than the network. Two facts drive the design:
//
//   * On X11 any client may read the selection, so a background watcher works.
//   * On Wayland (GNOME) the compositor refuses clipboard reads from
//     unfocused clients, so the *window* has to do the reading and push the
//     result to the backend. The backend still owns the "what to do with it"
//     policy.
//
// In both cases the clipboard *content* is never written to the log.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardBackend {
    Wayland,
    X11,
    None,
}

impl ClipboardBackend {
    fn detect() -> Self {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            ClipboardBackend::Wayland
        } else if std::env::var("DISPLAY").is_ok() {
            ClipboardBackend::X11
        } else {
            ClipboardBackend::None
        }
    }
}

pub fn backend() -> ClipboardBackend {
    ClipboardBackend::detect()
}

pub fn backend_label() -> &'static str {
    match backend() {
        ClipboardBackend::Wayland => "Wayland",
        ClipboardBackend::X11 => "X11",
        ClipboardBackend::None => "Unavailable",
    }
}

/// Whether a background (unfocused) process is allowed to read the clipboard on
/// this session. On Wayland it is not, so the UI drives the read instead.
pub fn background_read_allowed() -> bool {
    backend() == ClipboardBackend::X11
}

/// Read the clipboard. `Ok(None)` means "access denied or empty", which is a
/// normal outcome on Wayland rather than an error.
pub fn read_text() -> Option<String> {
    match backend() {
        ClipboardBackend::X11 => arboard::Clipboard::new()
            .ok()
            .and_then(|mut c| c.get_text().ok())
            .filter(|s| !s.is_empty()),
        ClipboardBackend::Wayland => read_wayland(),
        ClipboardBackend::None => None,
    }
}

#[cfg(target_os = "linux")]
fn read_wayland() -> Option<String> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, Error, MimeType, Seat};

    match get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text) {
        Ok((mut pipe, _)) => {
            let mut buf = Vec::new();
            pipe.read_to_end(&mut buf).ok()?;
            let text = String::from_utf8(buf).ok()?;
            (!text.is_empty()).then_some(text)
        }
        Err(Error::NoSeats)
        | Err(Error::ClipboardEmpty)
        | Err(Error::NoMimeType)
        | Err(Error::PrimarySelectionUnsupported) => None,
        Err(e) => {
            log::debug!("wayland clipboard read failed: {e}");
            None
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn read_wayland() -> Option<String> {
    None
}

/// Take ownership of the clipboard so pasting works elsewhere.
///
/// On X11 arboard does this directly. On Wayland we use the data-control
/// protocols through wl-clipboard-rs, which hands the data to the compositor
/// immediately so this process does not have to stay alive holding the
/// selection.
pub fn set_text_blocking(text: String) {
    if text.is_empty() {
        return;
    }
    match backend() {
        ClipboardBackend::X11 => {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Err(e) = cb.set_text(text.clone()) {
                    log::error!("could not set the X11 clipboard: {e}");
                }
            }
        }
        ClipboardBackend::Wayland => set_wayland(text),
        ClipboardBackend::None => log::warn!("no display server; clipboard not updated"),
    }
}

#[cfg(target_os = "linux")]
fn set_wayland(text: String) {
    use wl_clipboard_rs::copy::{MimeType, Options, Source};
    let opts = Options::new();
    match opts.copy(
        Source::Bytes(text.into_bytes().into()),
        MimeType::Specific("text/plain;charset=utf-8".to_string()),
    ) {
        Ok(()) => {}
        Err(e) => log::error!("could not set the Wayland clipboard: {e}"),
    }
}

#[cfg(not(target_os = "linux"))]
fn set_wayland(_text: String) {}

pub fn set_text(text: String) {
    tauri::async_runtime::spawn(async move { set_text_blocking(text) });
}

/// Watch the clipboard for local changes.
///
/// Only wired up on X11, where a background process may read the selection.
/// On Wayland the front end observes changes and calls `kde_set_clipboard` /
// `broadcast_clipboard` directly.
pub fn spawn_x11_watcher<F>(on_change: F)
where
    F: Fn(String) + Send + 'static,
{
    if backend() != ClipboardBackend::X11 {
        debug!("clipboard watcher not started: background reads are not allowed on Wayland");
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            let Some(text) = read_text() else { continue };
            if text != last {
                last = text.clone();
                on_change(text);
            }
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub backend: &'static str,
    pub wayland: bool,
    pub x11: bool,
    /// True when a background process may read the clipboard on this session.
    pub background_read_allowed: bool,
    pub note: &'static str,
}

pub fn capabilities() -> Capabilities {
    let b = backend();
    Capabilities {
        backend: backend_label(),
        wayland: b == ClipboardBackend::Wayland,
        x11: b == ClipboardBackend::X11,
        background_read_allowed: background_read_allowed(),
        note: match b {
            ClipboardBackend::Wayland =>
                "GNOME only lets a focused window read the clipboard, so Pixel Connect \
                 watches for changes while the window is open. Copying in another app is \
                 picked up the next time you focus Pixel Connect, or automatically when you \
                 copy inside it.",
            ClipboardBackend::X11 =>
                "Clipboard synchronisation runs in the background on this session.",
            ClipboardBackend::None => "No display server was detected.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_serialisable() {
        let json = serde_json::to_value(capabilities()).unwrap();
        assert!(json.get("backend").is_some());
        assert!(json.get("backgroundReadAllowed").is_some());
    }

    #[test]
    fn empty_text_is_never_published() {
        // Setting an empty clipboard would wipe the user's selection.
        // `set_text_blocking` returns early; here we only assert the guard is
        // reachable without a display server.
        let _ = set_text_blocking(String::new());
    }
}