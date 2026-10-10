// Shared helpers for the KDE Connect integration tests.

use std::path::PathBuf;

/// A unique temporary directory that is removed when the process exits.
pub fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("pixel-connect-kc-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("could not create the test directory");
    dir
}

/// Sleep for `d`. Used to give background tasks a chance to run.
#[allow(dead_code)]
pub async fn wait_for(d: std::time::Duration) {
    tokio::time::sleep(d).await;
}

/// Where a test's received files should land.
///
/// Each test gets its own directory. Without this, several tests would write
/// into the real `~/Downloads` and find each other's files — an isolation bug
/// that produced two confusing failures before it was found.
pub fn downloads(tag: &str) -> PathBuf {
    let dir = temp_dir(tag).join("incoming");
    std::fs::create_dir_all(&dir).expect("could not create the download directory");
    dir
}

/// Find the single file in `dir` with the given extension.
#[allow(dead_code)]
pub fn find_with_extension(dir: &std::path::Path, ext: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == ext))
}
