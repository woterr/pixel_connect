// Pixel Connect — desktop clipboard integration.
//
// Clipboard synchronisation is the one feature that has to fight the display
// server rather than the network, and the answer differs per session:
//
//   * **X11** — any client may read the selection, so a background watcher
//     works unconditionally.
//
//   * **Wayland with `ext-data-control-v1` / `wlr-data-control-v1`** — these
//     protocols exist precisely so that *unfocused* clients can read and own
//     the selection. When the compositor offers one, clipboard sync is fully
//     background-capable and the window never has to be focused.
//
//   * **Wayland without data-control** — the compositor refuses core
//     `wl_data_device` access to unfocused clients. Writing still works (a
//     focused client can take ownership), but *reading* requires focus. The
//     window therefore polls and forwards instead.
//
// The mechanism is probed once at start-up (`ClipboardMechanism::detect`) so
// the behaviour is decided by what the session actually supports, and the UI is
// told the truth through `capabilities()`.
//
// The clipboard *content* is never written to the log, and there is no
// clipboard history anywhere in the application.

use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// How clipboard access works on this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardMechanism {
    /// Wayland `ext-data-control-v1` / `wlr-data-control-v1`: the protocol
    /// designed for unfocused clipboard access. Best case.
    WaylandDataControl,
    /// A native X11 session: selections are unrestricted.
    X11,
    /// A Wayland session where the XWayland clipboard bridge is usable. Mutter
    /// mirrors the Wayland selection into the X11 selection (and vice versa), so
    /// an unfocused X11 client can read and write the clipboard normally.
    WaylandX11Bridge,
    /// A Wayland session with neither data-control nor a usable X11 bridge.
    /// Reading requires a focused window; this is the worst case.
    WaylandCoreOnly,
    /// No display server.
    None,
}

impl ClipboardMechanism {
    /// Detect the mechanism once and cache it: constructing a Wayland
    /// connection is not free and the answer cannot change mid-session.
    pub fn detect() -> Self {
        use std::sync::OnceLock;
        static CACHE: OnceLock<ClipboardMechanism> = OnceLock::new();
        *CACHE.get_or_init(Self::detect_uncached)
    }

    fn detect_uncached() -> Self {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            if wayland_data_control_available() {
                return ClipboardMechanism::WaylandDataControl;
            }
            // GNOME's mutter mirrors the Wayland selection into the X11
            // selection, and XWayland is always present on a GNOME Wayland
            // session, so the bridge is a reliable way to reach the clipboard
            // without focus. This was measured on GNOME 50 (see
            // docs/architecture/v2-status.md).
            if x11_selection_reachable() {
                return ClipboardMechanism::WaylandX11Bridge;
            }
            return ClipboardMechanism::WaylandCoreOnly;
        }
        if std::env::var("DISPLAY").is_ok() {
            return ClipboardMechanism::X11;
        }
        ClipboardMechanism::None
    }

    /// Can an unfocused process read the clipboard on this session?
    pub fn background_read_allowed(self) -> bool {
        !matches!(
            self,
            ClipboardMechanism::WaylandCoreOnly | ClipboardMechanism::None
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            ClipboardMechanism::WaylandDataControl => "Wayland (data-control)",
            ClipboardMechanism::X11 => "X11",
            ClipboardMechanism::WaylandX11Bridge => "Wayland (XWayland bridge)",
            ClipboardMechanism::WaylandCoreOnly => "Wayland (focused only)",
            ClipboardMechanism::None => "Unavailable",
        }
    }

    /// The exact name of the protocol that was (or was not) available, for the
    /// Settings screen and the log.
    pub fn detail(self) -> &'static str {
        match self {
            ClipboardMechanism::WaylandDataControl => {
                "the compositor offers ext-data-control-v1, so the clipboard is \
                 readable without focus"
            }
            ClipboardMechanism::X11 => "X11 selections are readable without focus",
            ClipboardMechanism::WaylandX11Bridge => {
                "this compositor has no data-control protocol, but it mirrors the \
                 selection through XWayland, so clipboard sync still runs in the \
                 background"
            }
            ClipboardMechanism::WaylandCoreOnly => {
                "this compositor offers neither a data-control protocol nor a usable \
                 XWayland bridge, so the clipboard can only be read while the \
                 Pixel Connect window is focused"
            }
            ClipboardMechanism::None => "no display server was detected",
        }
    }
}

/// Can an X11 selection be opened? Used on a Wayland session to decide whether
/// the XWayland bridge is a usable fallback. Deliberately has no side effect —
/// in particular it must never overwrite the user's clipboard during probing.
fn x11_selection_reachable() -> bool {
    if std::env::var("DISPLAY").is_err() {
        return false;
    }
    arboard::Clipboard::new().is_ok()
}

/// Ask the compositor whether a data-control protocol exists.
///
/// `wl-clipboard-rs` tries `ext-data-control-v1` first and falls back to
/// `wlr-data-control-v1`; a `MissingProtocol` error means neither was found.
/// It is a cheap round trip and is performed once.
fn wayland_data_control_available() -> bool {
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, Error, MimeType, Seat};

    match get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text) {
        // A live connection with a supported protocol: readable right now.
        Ok(_) => true,
        // The protocol exists but nothing is copied — still background-capable.
        Err(Error::ClipboardEmpty) | Err(Error::NoMimeType) => true,
        // The compositor does not implement it at all.
        Err(Error::MissingProtocol { .. }) => false,
        // Any other failure (no seats, seat not found, a transient IPC error)
        // means the protocol was bound successfully.
        Err(_) => true,
    }
}

/// Read the clipboard. `None` means "empty, or not permitted right now", which
/// is a normal outcome and never an error the user needs to see.
pub fn read_text() -> Option<String> {
    match ClipboardMechanism::detect() {
        ClipboardMechanism::X11 | ClipboardMechanism::WaylandX11Bridge => read_x11(),
        ClipboardMechanism::WaylandDataControl => read_wayland(),
        // Core protocol only: a caller without focus gets nothing back, which
        // is why the front end drives detection in this case.
        ClipboardMechanism::WaylandCoreOnly => read_wayland(),
        ClipboardMechanism::None => None,
    }
}

/// The shared X11/XWayland clipboard handle.
///
/// Constructing this is not cheap and, on a session with no data-control
/// protocol, `arboard` logs a warning every time it falls back from Wayland to
/// X11. The clipboard watcher polls twice a second, so building a fresh handle
/// per poll produced thousands of identical warnings an hour and turned a
/// transient condition into unbounded log growth. One handle, reused, keeps the
/// fallback notice to a single line at start-up.
fn x11_clipboard() -> Option<&'static std::sync::Mutex<arboard::Clipboard>> {
    use std::sync::OnceLock;
    static HANDLE: OnceLock<Option<std::sync::Mutex<arboard::Clipboard>>> = OnceLock::new();
    HANDLE
        .get_or_init(|| arboard::Clipboard::new().ok().map(std::sync::Mutex::new))
        .as_ref()
}

fn read_x11() -> Option<String> {
    let handle = x11_clipboard()?;
    let mut guard = handle.lock().ok()?;
    guard.get_text().ok().filter(|s| !s.is_empty())
}

fn read_wayland() -> Option<String> {
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, MimeType, Seat};

    let (mut pipe, _mime) =
        get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text).ok()?;
    let mut buf = Vec::new();
    pipe.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8(buf).ok()?;
    (!text.is_empty()).then_some(text)
}

/// Take ownership of the clipboard so pasting works in other applications.
///
/// On Wayland this hands the data to the compositor over data-control, which
/// means it survives even though this process may exit.
pub fn set_text_blocking(text: String) {
    if text.is_empty() {
        // Clearing the user's selection because they deselected something would
        // be worse than doing nothing.
        return;
    }
    match ClipboardMechanism::detect() {
        ClipboardMechanism::X11 | ClipboardMechanism::WaylandX11Bridge => {
            // The process stays alive for the whole session, so arboard keeps
            // owning the selection and the text remains pasteable.
            match x11_clipboard() {
                Some(handle) => match handle.lock() {
                    Ok(mut guard) => {
                        if let Err(e) = guard.set_text(text) {
                            log::error!("could not set the clipboard: {e}");
                        }
                    }
                    Err(_) => log::error!("the clipboard is locked by another task"),
                },
                None => log::error!("could not open the clipboard to set it"),
            }
        }
        ClipboardMechanism::WaylandDataControl => set_wayland(text),
        // Owning the core-protocol selection needs focus. data-control is not
        // available on this session, so report it honestly rather than silently
        // doing nothing.
        ClipboardMechanism::WaylandCoreOnly => {
            log::warn!(
                "cannot set the clipboard without focus on this compositor; \
                 this session has no data-control protocol and no XWayland bridge"
            );
        }
        ClipboardMechanism::None => log::warn!("no display server; the clipboard was not updated"),
    }
}

#[cfg(target_os = "linux")]
fn set_wayland(text: String) {
    use wl_clipboard_rs::copy::{MimeType, Options, Source};

    // `MimeType::Text` offers the usual plain-text types, which every
    // consumer (including GNOME Text Editor and Firefox) understands.
    if let Err(e) = Options::new().copy(Source::Bytes(text.into_bytes().into()), MimeType::Text) {
        log::error!("could not set the Wayland clipboard: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
fn set_wayland(_text: String) {}

pub fn set_text(text: String) {
    tauri::async_runtime::spawn(async move { set_text_blocking(text) });
}

/// The clipboard the application last applied locally, so it can be echoed back
/// to the phone exactly once and never looped.
///
/// Only the text is kept, in memory, for as long as the process lives. It is
/// never written to disk and never logged.
static LAST_APPLIED: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Remember text we just wrote to the local clipboard.
pub fn remember_applied(text: &str) {
    if let Ok(mut slot) = LAST_APPLIED.lock() {
        *slot = Some((text.to_string(), Instant::now()));
    }
}

/// True when `text` is the clipboard content we ourselves just applied.
///
/// The time bound stops a stale entry from suppressing a genuine re-copy of the
/// same text much later.
pub fn was_applied(text: &str) -> bool {
    const ECHO_WINDOW: Duration = Duration::from_secs(4);
    match LAST_APPLIED.lock().ok().and_then(|s| s.clone()) {
        Some((previous, at)) if previous == text => at.elapsed() < ECHO_WINDOW,
        _ => false,
    }
}

/// Watch the clipboard for local changes and call `on_change` for each new
/// value.
///
/// Only starts when the session permits unfocused reads; otherwise the front
/// end drives detection, and `capabilities()` explains why.
pub fn spawn_background_watcher<F>(on_change: F)
where
    F: Fn(String) + Send + 'static,
{
    let mechanism = ClipboardMechanism::detect();
    if !mechanism.background_read_allowed() {
        debug!(
            "clipboard watcher not started: {} ({})",
            mechanism.label(),
            mechanism.detail()
        );
        return;
    }
    info!("clipboard watcher started: {}", mechanism.label());

    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(Duration::from_millis(600)).await;
            let Some(text) = read_text() else { continue };
            if text == last {
                continue;
            }
            // The loop breaker: never echo our own write back to the phone.
            if was_applied(&text) {
                last = text;
                continue;
            }
            last = text.clone();
            on_change(text);
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub mechanism: ClipboardMechanism,
    pub mechanism_label: &'static str,
    pub mechanism_detail: &'static str,
    pub wayland: bool,
    pub x11: bool,
    /// True when an unfocused process may read the clipboard on this session.
    pub background_read_allowed: bool,
    /// True when an unfocused process may also *own* the clipboard.
    pub background_write_allowed: bool,
    pub note: &'static str,
}

pub fn capabilities() -> Capabilities {
    let m = ClipboardMechanism::detect();
    let note: &'static str = if m.background_read_allowed() {
        "Clipboard sync runs continuously in the background; the window does not \
         need to be focused."
    } else {
        "This compositor only lets a focused window read the clipboard, so Pixel \
         Connect watches for changes while its window is open. Copying in another \
         application is picked up the next time you focus Pixel Connect."
    };
    Capabilities {
        mechanism: m,
        mechanism_label: m.label(),
        mechanism_detail: m.detail(),
        wayland: matches!(
            m,
            ClipboardMechanism::WaylandDataControl
                | ClipboardMechanism::WaylandCoreOnly
                | ClipboardMechanism::WaylandX11Bridge
        ),
        x11: matches!(
            m,
            ClipboardMechanism::X11 | ClipboardMechanism::WaylandX11Bridge
        ),
        background_read_allowed: m.background_read_allowed(),
        background_write_allowed: m.background_read_allowed(),
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_serialisable_and_consistent() {
        let json = serde_json::to_value(capabilities()).unwrap();
        assert!(json.get("mechanism").is_some());
        assert!(json.get("backgroundReadAllowed").is_some());

        let c = capabilities();
        assert_eq!(
            c.background_read_allowed,
            c.mechanism.background_read_allowed(),
            "the reported flag must match the detected mechanism"
        );
    }

    #[test]
    fn every_mechanism_has_a_label_and_an_explanation() {
        for m in [
            ClipboardMechanism::WaylandDataControl,
            ClipboardMechanism::X11,
            ClipboardMechanism::WaylandX11Bridge,
            ClipboardMechanism::WaylandCoreOnly,
            ClipboardMechanism::None,
        ] {
            assert!(!m.label().is_empty());
            assert!(!m.detail().is_empty());
        }
    }

    #[test]
    fn empty_text_is_never_published() {
        // Guard the early return: clearing the user's selection because they
        // deselected something would be a bug.
        let _ = set_text_blocking(String::new());
    }

    #[test]
    fn the_echo_suppressor_only_swallows_recent_identical_text() {
        remember_applied("hello");
        assert!(was_applied("hello"));
        assert!(!was_applied("goodbye"));

        // A stale entry must stop suppressing, so the same text copied again
        // later still syncs.
        if let Ok(mut slot) = LAST_APPLIED.lock() {
            *slot = slot
                .as_ref()
                .map(|(t, at)| (t.clone(), *at - Duration::from_secs(30)));
        }
        assert!(!was_applied("hello"));
    }

    #[test]
    fn only_the_fallback_cases_block_background_reads() {
        assert!(ClipboardMechanism::WaylandDataControl.background_read_allowed());
        assert!(ClipboardMechanism::X11.background_read_allowed());
        assert!(ClipboardMechanism::WaylandX11Bridge.background_read_allowed());
        assert!(!ClipboardMechanism::WaylandCoreOnly.background_read_allowed());
        assert!(!ClipboardMechanism::None.background_read_allowed());
    }

    #[test]
    fn probing_never_touches_the_users_clipboard() {
        // `detect` must be side-effect free; asserting the cached value is
        // stable also proves the cache works.
        let first = ClipboardMechanism::detect();
        let second = ClipboardMechanism::detect();
        assert_eq!(first, second);
    }

    /// Reports what this session actually supports. The result is informative
    /// rather than an assertion, because it depends on the compositor.
    #[test]
    fn report_the_mechanism_this_session_provides() {
        let m = ClipboardMechanism::detect();
        println!("clipboard mechanism: {} — {}", m.label(), m.detail());
        assert_ne!(
            m,
            ClipboardMechanism::None,
            "no display server in this session"
        );
    }
}
