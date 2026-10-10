// Shared helpers for the KDE Connect integration tests.

use std::path::PathBuf;

/// A unique temporary directory that is removed when the process exits.
pub fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "pixel-connect-kc-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("could not create the test directory");
    dir
}

/// Sleep for `d`. Used to give background tasks a chance to run.
#[allow(dead_code)]
pub async fn wait_for(d: std::time::Duration) {
    tokio::time::sleep(d).await;
}
