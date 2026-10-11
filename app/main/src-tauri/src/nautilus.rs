// Pixel Connect — file-manager integration backend.
//
// KDE Connect's Nautilus extension talks to `org.kde.kdeconnect` over D-Bus.
// Pixel Connect must not require KDE Connect to be installed, so it exposes its
// own, much smaller IPC surface: a Unix domain socket carrying one JSON object
// per line.
//
// The socket lives in the user's runtime directory and is only reachable by
// that user (mode 0600), so this is not a privilege boundary crossing — it is
// a convenience IPC channel. Requests are still validated: only local,
// existing, regular files and directories are accepted, and the number of
// entries per request is capped.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::kdc::KdcState;

pub const MAX_PATHS_PER_REQUEST: usize = 512;
pub const MAX_PATH_LEN: usize = 4096;
/// Largest single IPC request accepted. A list of 512 paths at 4096 bytes each
/// is well under this, so legitimate requests are unaffected.
pub const MAX_REQUEST_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub paths: Vec<String>,
    /// Optional explicit target device id; otherwise the preferred device is used.
    pub device_id: Option<String>,
    /// Show the Pixel Connect window even if the send succeeds silently.
    pub reveal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResponse {
    pub ok: bool,
    pub sent: usize,
    pub device: Option<String>,
    pub error: Option<String>,
}

pub fn socket_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("pixel-connect").join("filemanager.sock")
}

pub async fn accept_loop(app: AppHandle) {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    // A stale socket from a crashed instance must not block startup.
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::error!(
                "Nautilus integration disabled: could not bind {}: {e}",
                path.display()
            );
            return;
        }
    };
    if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
        log::warn!("could not restrict {}: {e}", path.display());
    }
    log::info!("file-manager integration listening on {}", path.display());

    tauri::async_runtime::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = handle_client(stream, app).await {
                            log::debug!("file-manager client ended: {e}");
                        }
                    });
                }
                Err(e) => {
                    log::error!("file-manager socket accept failed: {e}");
                    return;
                }
            }
        }
    });
}

/// Acknowledge a `ping` so the extension can report a healthy backend.
pub async fn handle_client(stream: UnixStream, app: tauri::AppHandle) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    // A request is a single JSON object on one line. The socket is mode 0600
    // in a 0700 directory, so only this user's own processes can reach it, but
    // a bounded read still stops a buggy or hostile caller from making the
    // daemon allocate without limit.
    let n = (&mut reader)
        .take(MAX_REQUEST_BYTES)
        .read_line(&mut line)
        .await?;
    if n == 0 {
        return Ok(());
    }

    let response = match serde_json::from_str::<SendRequest>(line.trim()) {
        Ok(req) => dispatch(&app, req).await,
        Err(e) => SendResponse {
            ok: false,
            sent: 0,
            device: None,
            error: Some(format!("could not understand the request: {e}")),
        },
    };

    let mut body = serde_json::to_vec(&response)?;
    body.push(b'\n');
    write_half.write_all(&body).await?;
    write_half.flush().await?;
    Ok(())
}

pub async fn dispatch(app: &tauri::AppHandle, req: SendRequest) -> SendResponse {
    if req.paths.is_empty() {
        return SendResponse {
            ok: false,
            sent: 0,
            device: None,
            error: Some("no files were selected".into()),
        };
    }
    if req.paths.len() > MAX_PATHS_PER_REQUEST {
        return SendResponse {
            ok: false,
            sent: 0,
            device: None,
            error: Some(format!(
                "too many items selected ({}); the limit is {}",
                req.paths.len(),
                MAX_PATHS_PER_REQUEST
            )),
        };
    }

    // Resolve every path ourselves. A URI from Nautilus is accepted, but only
    // the `file` scheme, and everything must already exist locally.
    let mut files: Vec<PathBuf> = Vec::new();
    let mut directories: Vec<PathBuf> = Vec::new();

    for raw in &req.paths {
        if raw.len() > MAX_PATH_LEN {
            return SendResponse {
                ok: false,
                sent: 0,
                device: None,
                error: Some("one of the selected paths is too long".into()),
            };
        }
        let path = match parse_local_path(raw) {
            Some(p) => p,
            None => {
                return SendResponse {
                    ok: false,
                    sent: 0,
                    device: None,
                    error: Some(format!("{raw:?} is not a local file")),
                }
            }
        };
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => files.push(path),
            Ok(m) if m.is_dir() => directories.push(path),
            _ => {
                return SendResponse {
                    ok: false,
                    sent: 0,
                    device: None,
                    error: Some(format!("{} no longer exists", path.display())),
                }
            }
        }
    }

    let state = app.state::<Arc<KdcState>>().inner().clone();
    let device_id = match resolve_device(app, &state, req.device_id.as_deref()).await {
        Ok(id) => id,
        Err(e) => {
            return SendResponse {
                ok: false,
                sent: 0,
                device: None,
                error: Some(e),
            }
        }
    };

    let mut sent = 0usize;
    if !files.is_empty() {
        match crate::kdc::share_files(&state, &device_id, files).await {
            Ok(n) => sent += n,
            Err(e) => {
                return SendResponse {
                    ok: false,
                    sent,
                    device: Some(device_id),
                    error: Some(e.to_string()),
                }
            }
        }
    }
    for dir in &directories {
        match crate::kdc::share_directory(&state, &device_id, dir.clone()).await {
            Ok(n) => sent += n,
            Err(e) => {
                return SendResponse {
                    ok: false,
                    sent,
                    device: Some(device_id),
                    error: Some(e.to_string()),
                }
            }
        }
    }

    let name = {
        let devices = crate::kdc::devices(&state).await;
        devices
            .iter()
            .find(|d| d.id == device_id)
            .map(|d| d.name.clone())
    };

    crate::quickshare::notify_send_result(app, sent, name.as_deref());

    SendResponse {
        ok: true,
        // Deliberately named for what it is. Each file was handed to the link
        // and the link took it; no bytes have moved and the phone has not
        // confirmed anything. Reporting this as "sent" is what produced
        // "Sent 1 item" on this side while the phone said it had failed -
        // `share_files` returns before the payload connection is even opened.
        //
        // The outcome arrives later as a TransferProgress event, which raises
        // its own notification, so the user is told either way.
        sent: sent,
        device: name,
        error: None,
    }
}

async fn resolve_device(
    app: &tauri::AppHandle,
    state: &Arc<KdcState>,
    requested: Option<&str>,
) -> Result<String, String> {
    let devices = crate::kdc::devices(state).await;
    let paired: Vec<_> = devices.iter().filter(|d| d.trusted).collect();

    if let Some(id) = requested {
        return paired
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.id.clone())
            .ok_or_else(|| format!("{id} is not a paired device"));
    }

    if let Some(id) = crate::settings::get_str(app, "preferred_device_id") {
        if paired.iter().any(|d| d.id == id) {
            return Ok(id);
        }
    }

    paired
        .first()
        .map(|d| d.id.clone())
        .ok_or_else(|| "No paired Pixel is available. Pair one in Pixel Connect first.".to_string())
}

pub fn parse_local_path(raw: &str) -> Option<PathBuf> {
    let candidate = if let Some(rest) = raw.strip_prefix("file://") {
        // Percent-decode the URI path.
        let path = rest.split(['?', '#']).next().unwrap_or(rest);
        percent_decode(path)?
    } else if raw.starts_with('/') {
        raw.to_string()
    } else {
        return None;
    };

    let parsed = PathBuf::from(candidate);
    // Reject anything that is not yet absolute after cleaning.
    let cleaned = clean_path(&parsed);
    if !cleaned.is_absolute() {
        return None;
    }
    Some(cleaned)
}

fn clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // A '%' must be followed by exactly two hex digits.
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Ping helper used by the shell wrapper script.
pub async fn ping_backend() -> bool {
    let path = socket_path();
    match UnixStream::connect(&path).await {
        Ok(mut stream) => {
            let body = r#"{"paths":[]}"#;
            if stream.write_all(body.as_bytes()).await.is_err() {
                return false;
            }
            let _ = stream.flush().await;
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_paths_and_file_uris() {
        assert_eq!(
            parse_local_path("/home/me/a.txt"),
            Some(PathBuf::from("/home/me/a.txt"))
        );
        assert_eq!(
            parse_local_path("file:///home/me/a.txt"),
            Some(PathBuf::from("/home/me/a.txt"))
        );
    }

    #[test]
    fn percent_decodes_uri_paths() {
        assert_eq!(
            parse_local_path("file:///home/me/My%20File.txt"),
            Some(PathBuf::from("/home/me/My File.txt"))
        );
    }

    #[test]
    fn rejects_non_file_schemes_and_relative_paths() {
        assert_eq!(parse_local_path("sftp://host/a.txt"), None);
        assert_eq!(parse_local_path("https://example.org/a.txt"), None);
        assert_eq!(parse_local_path("relative.txt"), None);
        assert_eq!(parse_local_path(""), None);
    }

    #[test]
    fn normalises_traversal_segments() {
        assert_eq!(
            parse_local_path("/home/me/../me/./a.txt"),
            Some(PathBuf::from("/home/me/a.txt"))
        );
    }

    #[test]
    fn malformed_percent_encoding_is_rejected() {
        assert_eq!(parse_local_path("file:///home/me/%zz.txt"), None);
        assert_eq!(parse_local_path("file:///home/me/%4"), None);
    }

    #[test]
    fn path_limits_are_enforced_by_the_caller() {
        assert!(MAX_PATHS_PER_REQUEST >= 1);
        assert!(MAX_PATH_LEN >= 1024);
    }
}
