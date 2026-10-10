// Pixel Connect — transfer cancellation.
//
// KDE Connect's protocol has **no** cancel or pause packet. Both
// implementations cancel by closing the payload TCP connection: the receiver
// sees the socket close and treats the transfer as abandoned. kdeconnect-kde
// documents this explicitly in `compositeuploadjob.cpp`:
//
//     // Do not close the socket because when android closes the socket
//     // (share is cancelled) closing the socket leads to a cyclic socketError
//
// So cancellation here means "stop transferring and drop the payload
// connection", which is exactly what the peer expects. Pausing is not
// implemented anywhere in the protocol and is not offered.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// A handle a transfer can check at any point to notice it was cancelled.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Process-wide registry of in-flight transfers, keyed by an id the UI holds.
///
/// Transfers are identified by `<deviceId>:<direction>:<filename or counter>`
/// so a UI row maps to exactly one transfer and two files with the same name
/// cannot collide.
fn registry() -> &'static Mutex<HashMap<String, CancelToken>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, CancelToken>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Create and register a token. The caller keeps the token and deletes the
/// entry when the transfer finishes.
pub fn register(id: &str) -> CancelToken {
    let token = CancelToken::new();
    if let Ok(mut map) = registry().lock() {
        map.insert(id.to_string(), token.clone());
    }
    token
}

/// Remove a finished transfer so the registry cannot grow without bound.
pub fn finish(id: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(id);
    }
}

/// Cancel a transfer by id. Returns false when the id is unknown or already
/// finished, so the UI can report "that transfer is no longer running"
/// instead of silently succeeding.
pub fn cancel(id: &str) -> bool {
    match registry().lock() {
        Ok(map) => match map.get(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// Ids of everything currently in flight.
pub fn in_flight() -> Vec<String> {
    match registry().lock() {
        Ok(map) => map.keys().cloned().collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_starts_uncancelled_and_latches_once_cancelled() {
        let token = register("test:latch");
        assert!(!token.is_cancelled());
        assert!(cancel("test:latch"));
        assert!(token.is_cancelled());
        // Cancelling again is idempotent.
        assert!(token.is_cancelled());
        finish("test:latch");
    }

    #[test]
    fn cancelling_an_unknown_transfer_reports_failure() {
        // The UI must be able to say "that transfer is gone" rather than
        // showing a success that did nothing.
        assert!(!cancel("no-such-transfer"));
    }

    #[test]
    fn finishing_removes_the_entry() {
        let id = "test:finish";
        register(id);
        assert!(in_flight().iter().any(|i| i == id));
        finish(id);
        assert!(!in_flight().iter().any(|i| i == id));
        assert!(!cancel(id));
    }

    #[test]
    fn cancelling_only_affects_the_named_transfer() {
        let a = register("test:a");
        let b = register("test:b");
        assert!(cancel("test:a"));
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
        finish("test:a");
        finish("test:b");
    }
}
