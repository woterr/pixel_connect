// Pixel Connect — Tauri command surface.
//
// Every command the Vue front end can invoke lives here. Commands never trust
// the front end: paths are validated, arguments are passed as argv, and all
// errors come back as strings that can be shown verbatim in the UI.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
#[allow(unused_imports)]
use tauri::Manager as _ManagerTrait;
use tauri::{AppHandle, State};

use crate::adb::ToolAvailability;
use crate::kdc::KdcState;
use crate::nautilus::{SendRequest, SendResponse};
use crate::settings::Settings;
use crate::ssh::{SshProbe, SshTarget};
use crate::AppState;

type Shared<'a> = State<'a, Arc<AppState>>;
type Kdc<'a> = State<'a, Arc<KdcState>>;

// ---------------------------------------------------------------- application

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub identifier: String,
    pub kdeconnect_protocol: i64,
    pub quickshare_service: bool,
    pub clipboard_backend: String,
    pub filemanager_socket: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: Shared<'_>) -> AppInfo {
    let _ = &state;
    let info = app.package_info();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
        identifier: app.config().identifier.clone(),
        kdeconnect_protocol: pixel_connect_kdeconnect::PROTOCOL_VERSION,
        quickshare_service: true,
        clipboard_backend: crate::clipboard::ClipboardMechanism::detect()
            .label()
            .to_string(),
        filemanager_socket: crate::nautilus::socket_path().to_string_lossy().to_string(),
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    crate::settings::load(&app)
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: Shared<'_>,
    kdc: Kdc<'_>,
    settings: Settings,
) -> Result<Settings, String> {
    crate::settings::save(&app, &settings).map_err(|e| e.to_string())?;

    crate::kdc::set_clipboard_enabled(kdc.inner(), settings.clipboard_enabled).await;
    crate::kdc::set_clipboard_auto_share(kdc.inner(), settings.clipboard_auto_share).await;

    if let Some(path) = &settings.incoming_path {
        let path = PathBuf::from(path);
        let svc = kdc.inner().service.clone();
        let svc = svc.lock().await;
        svc.set_incoming_path(path).await;
    }
    if !settings.kde_device_name.is_empty() {
        let svc = kdc.inner().service.clone();
        let svc = svc.lock().await;
        svc.set_device_name(&settings.kde_device_name)
            .await
            .map_err(|e| e.to_string())?;
    }
    crate::quickshare::set_visibility(state.inner(), visibility_from_u8(settings.visibility));
    crate::quickshare::set_download_path(
        state.inner(),
        Some(
            settings
                .download_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(crate::settings::default_download_dir),
        ),
    );
    crate::kdc::persist_clipboard_prefs(&app);

    Ok(settings)
}

fn visibility_from_u8(v: u8) -> pixel_connect_quickshare::Visibility {
    match v {
        1 => pixel_connect_quickshare::Visibility::Invisible,
        2 => pixel_connect_quickshare::Visibility::Temporarily,
        _ => pixel_connect_quickshare::Visibility::Visible,
    }
}

#[tauri::command]
pub fn open_help(app: AppHandle) {
    crate::notify(
        "Pixel Connect",
        "Documentation is bundled with the application under docs/.",
    );
    crate::open_main_window(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit_app(&app);
}

// ---------------------------------------------------------------- Quick Share

#[tauri::command]
pub async fn quickshare_start_discovery(state: Shared<'_>) -> Result<(), String> {
    crate::quickshare::start_discovery(state.inner()).await
}

#[tauri::command]
pub fn quickshare_stop_discovery(state: Shared<'_>) {
    crate::quickshare::stop_discovery(state.inner());
}

#[tauri::command]
pub async fn quickshare_send_payload(
    state: Shared<'_>,
    payload: pixel_connect_quickshare::SendInfo,
) -> Result<(), String> {
    crate::quickshare::send_payload(state.inner(), payload).await
}

#[tauri::command]
pub fn quickshare_send_to_lib(
    state: Shared<'_>,
    message: pixel_connect_quickshare::channel::ChannelMessage,
) -> Result<(), String> {
    crate::quickshare::send_to_lib(state.inner(), message)
}

#[tauri::command]
pub fn quickshare_set_visibility(app: AppHandle, state: Shared<'_>, visibility: u8) {
    crate::quickshare::set_visibility(state.inner(), visibility_from_u8(visibility));
    crate::settings::set(&app, "visibility", serde_json::json!(visibility));
}

#[tauri::command]
pub fn quickshare_set_download_path(app: AppHandle, state: Shared<'_>, path: Option<String>) {
    let p = path.map(PathBuf::from);
    crate::quickshare::set_download_path(state.inner(), p.clone());
    crate::settings::set(
        &app,
        "download_path",
        serde_json::json!(p.map(|p| p.to_string_lossy().to_string())),
    );
}

#[tauri::command]
pub fn quickshare_accept_transfer(state: Shared<'_>, id: String) -> Result<(), String> {
    crate::quickshare::send_to_lib(state.inner(), crate::quickshare::accept_transfer(id))
}

#[tauri::command]
pub fn quickshare_reject_transfer(state: Shared<'_>, id: String) -> Result<(), String> {
    crate::quickshare::send_to_lib(state.inner(), crate::quickshare::reject_transfer(id))
}

#[tauri::command]
pub fn get_hostname() -> String {
    sys_metrics::host::get_hostname().unwrap_or_else(|_| "Linux PC".to_string())
}

// ---------------------------------------------------------------- KDE Connect

#[tauri::command]
pub async fn kde_devices(kdc: Kdc<'_>) -> Result<Vec<pixel_connect_kdeconnect::KcDevice>, String> {
    Ok(crate::kdc::devices(kdc.inner()).await)
}

#[tauri::command]
pub async fn kde_device_info(kdc: Kdc<'_>) -> Result<pixel_connect_kdeconnect::DeviceInfo, String> {
    Ok(crate::kdc::device_info(kdc.inner()).await)
}

#[tauri::command]
pub async fn kde_refresh(kdc: Kdc<'_>) -> Result<(), String> {
    crate::kdc::refresh(kdc.inner()).await;
    Ok(())
}

#[tauri::command]
pub async fn kde_request_pairing(kdc: Kdc<'_>, device_id: String) -> Result<(), String> {
    crate::kdc::request_pairing(kdc.inner(), &device_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_accept_pairing(kdc: Kdc<'_>, device_id: String) -> Result<(), String> {
    crate::kdc::accept_pairing(kdc.inner(), &device_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_reject_pairing(kdc: Kdc<'_>, device_id: String) -> Result<(), String> {
    crate::kdc::reject_pairing(kdc.inner(), &device_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_unpair(kdc: Kdc<'_>, device_id: String) -> Result<bool, String> {
    crate::kdc::unpair(kdc.inner(), &device_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_ping(kdc: Kdc<'_>, device_id: String, message: String) -> Result<(), String> {
    crate::kdc::ping(kdc.inner(), &device_id, &message)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_share_text(kdc: Kdc<'_>, device_id: String, text: String) -> Result<(), String> {
    crate::kdc::share_text(kdc.inner(), &device_id, &text)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_share_files(
    kdc: Kdc<'_>,
    device_id: String,
    paths: Vec<String>,
) -> Result<usize, String> {
    let parsed = resolve_local_paths(&paths)?;
    crate::kdc::share_files(kdc.inner(), &device_id, parsed)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_share_directory(
    kdc: Kdc<'_>,
    device_id: String,
    path: String,
) -> Result<usize, String> {
    let dir = PathBuf::from(path);
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    crate::kdc::share_directory(kdc.inner(), &device_id, dir)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_reply_to_notification(
    kdc: Kdc<'_>,
    notification_id: String,
    text: String,
) -> Result<(), String> {
    // The key is `<deviceId>:<notificationId>`; the device is the prefix.
    let device_id = notification_id
        .split_once(':')
        .map(|(d, _)| d.to_string())
        .ok_or_else(|| "that notification key is malformed".to_string())?;
    crate::kdc::notification_reply(kdc.inner(), &device_id, &notification_id, &text)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_send_notification_action(
    kdc: Kdc<'_>,
    device_id: String,
    key: String,
    action: String,
) -> Result<(), String> {
    crate::kdc::notification_action(kdc.inner(), &device_id, &key, &action)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_pending_shares(kdc: Kdc<'_>) -> Result<Vec<crate::kdc::PendingShare>, String> {
    Ok(crate::kdc::pending_shares(kdc.inner()).await)
}

#[tauri::command]
pub async fn kde_dismiss_share(kdc: Kdc<'_>, id: String) -> Result<(), String> {
    crate::kdc::dismiss_share(kdc.inner(), &id).await;
    Ok(())
}

/// Accept an incoming share.
///
/// Note what this can and cannot do: KDE Connect has no consent step for a
/// received file, so the bytes are already on disk by the time this is called.
/// It records the decision and clears the request; it cannot refuse the
/// transfer. See docs/diagnosis/incoming-share-consent.md.
#[tauri::command]
pub async fn kde_accept_share(kdc: Kdc<'_>, id: String) -> Result<(), String> {
    crate::kdc::accept_share(kdc.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_notifications(
    kdc: Kdc<'_>,
) -> Result<Vec<pixel_connect_kdeconnect::types::RemoteNotification>, String> {
    Ok(crate::kdc::notifications(kdc.inner()).await)
}

#[tauri::command]
pub async fn kde_clear_notifications(kdc: Kdc<'_>) -> Result<(), String> {
    crate::kdc::clear_notifications(kdc.inner()).await;
    Ok(())
}

#[tauri::command]
pub async fn kde_request_clipboard(kdc: Kdc<'_>) -> Result<usize, String> {
    crate::kdc::request_clipboard(kdc.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kde_set_clipboard_enabled(kdc: Kdc<'_>, enabled: bool) -> Result<(), String> {
    crate::kdc::set_clipboard_enabled(kdc.inner(), enabled).await;
    Ok(())
}

#[tauri::command]
pub async fn kde_set_clipboard_auto_share(
    app: AppHandle,
    kdc: Kdc<'_>,
    enabled: bool,
) -> Result<(), String> {
    crate::kdc::set_clipboard_auto_share(kdc.inner(), enabled).await;
    crate::kdc::persist_clipboard_prefs(&app);
    Ok(())
}

#[tauri::command]
pub async fn kde_set_device_name(app: AppHandle, kdc: Kdc<'_>, name: String) -> Result<(), String> {
    crate::kdc::set_device_name(kdc.inner(), &name)
        .await
        .map_err(|e| e.to_string())?;
    crate::settings::set(&app, "kde_device_name", serde_json::json!(name));
    Ok(())
}

#[tauri::command]
pub async fn kde_set_incoming_path(
    app: AppHandle,
    kdc: Kdc<'_>,
    path: String,
) -> Result<(), String> {
    let path = PathBuf::from(path);
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let svc = kdc.inner().service.clone();
    let guard = svc.lock().await;
    guard.set_incoming_path(path.clone()).await;
    drop(guard);
    let _ = &app;
    crate::settings::set(
        &app,
        "incoming_path",
        serde_json::json!(path.to_string_lossy().to_string()),
    );
    Ok(())
}

/// Point both backends at one folder.
///
/// Quick Share and KDE Connect used to keep separate download locations, so a
/// user setting one saw the other silently ignore it and files arriving through
/// the other protocol went somewhere else. One setting, applied to both.
#[tauri::command]
pub async fn set_download_folder(
    app: AppHandle,
    state: Shared<'_>,
    kdc: Kdc<'_>,
    path: String,
) -> Result<(), String> {
    let path = PathBuf::from(path);
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;

    // Quick Share.
    crate::quickshare::set_download_path(state.inner(), Some(path.clone()));

    // KDE Connect.
    let svc = kdc.inner().service.clone();
    let guard = svc.lock().await;
    guard.set_incoming_path(path.clone()).await;
    drop(guard);

    // The single stored value. Both legacy keys are written as well, so a
    // downgrade does not resurrect the old, divergent folder.
    let as_str = path.to_string_lossy().to_string();
    crate::settings::set(&app, "download_folder", serde_json::json!(as_str.clone()));
    crate::settings::set(&app, "download_path", serde_json::json!(as_str.clone()));
    crate::settings::set(&app, "incoming_path", serde_json::json!(as_str));
    Ok(())
}

/// The folder both backends currently download into.
#[tauri::command]
pub fn get_download_folder(app: AppHandle) -> Option<String> {
    crate::settings::get_str(&app, "download_folder")
        .or_else(|| crate::settings::get_str(&app, "download_path"))
        .or_else(|| crate::settings::get_str(&app, "incoming_path"))
}

fn resolve_local_paths(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    if paths.is_empty() {
        return Err("no files were selected".into());
    }
    if paths.len() > crate::nautilus::MAX_PATHS_PER_REQUEST {
        return Err("too many files were selected".into());
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let path = PathBuf::from(p);
        if !path.exists() {
            return Err(format!("{} does not exist", path.display()));
        }
        out.push(path);
    }
    Ok(out)
}

// ---------------------------------------------------------------- clipboard

#[tauri::command]
pub fn clipboard_read() -> Option<String> {
    crate::clipboard::read_text()
}

#[tauri::command]
pub fn clipboard_write(text: String) {
    crate::clipboard::set_text_blocking(text);
}

/// Cancel an in-flight transfer. Errors when it had already finished, so the UI
/// never claims a cancellation that did not happen.
///
/// Synchronous on purpose: cancelling is a registry flag, not an await, and a
/// synchronous Tauri command cannot deadlock the async runtime.
#[tauri::command]
pub fn kde_cancel_transfer(kdc: Kdc<'_>, transfer_id: String) -> Result<bool, String> {
    match kdc.inner().service.try_lock() {
        Ok(svc) if svc.cancel_transfer(&transfer_id) => Ok(true),
        Ok(_) => Err("that transfer has already finished".into()),
        Err(_) => Err("the device backend is busy; try again".into()),
    }
}

/// Ids of everything currently transferring.
#[tauri::command]
pub fn kde_transfers_in_flight(kdc: Kdc<'_>) -> Result<Vec<String>, String> {
    match kdc.inner().service.try_lock() {
        Ok(svc) => Ok(svc.transfers_in_flight()),
        Err(_) => Err("the device backend is busy; try again".into()),
    }
}

#[tauri::command]
pub fn kde_transports() -> Vec<pixel_connect_kdeconnect::TransportStatus> {
    pixel_connect_kdeconnect::transport::registry()
}

#[tauri::command]
pub fn clipboard_capabilities() -> serde_json::Value {
    serde_json::to_value(crate::clipboard::capabilities()).unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------- ADB/scrcpy

#[tauri::command]
pub async fn adb_detect(state: Shared<'_>) -> Result<ToolAvailability, String> {
    Ok(crate::adb::detect(state.inner().adb.as_ref()).await)
}

#[tauri::command]
pub async fn adb_devices(state: Shared<'_>) -> Result<Vec<crate::adb::AdbDevice>, String> {
    crate::adb::list_devices(state.inner().adb.as_ref()).await
}

#[tauri::command]
pub async fn scrcpy_launch(
    state: Shared<'_>,
    serial: String,
    args: Option<Vec<String>>,
) -> Result<crate::adb::ScrcpyLaunch, String> {
    crate::adb::launch_scrcpy(
        state.inner().adb.as_ref(),
        &serial,
        args.as_deref().unwrap_or(&[]),
    )
    .await
}

#[tauri::command]
pub async fn scrcpy_stop(state: Shared<'_>, serial: String) -> Result<bool, String> {
    crate::adb::stop_scrcpy(state.inner().adb.as_ref(), &serial).await
}

#[tauri::command]
pub async fn adb_disconnect_wireless(state: Shared<'_>, serial: String) -> Result<(), String> {
    crate::adb::disconnect_wireless(state.inner().adb.as_ref(), &serial).await
}

/// Put text on the desktop clipboard.
///
/// Used when the user asks for shared text to be copied, rather than reaching
/// for the clipboard from the front end, so the same mechanism-detection and
/// error handling apply to both directions.
#[tauri::command]
pub async fn clipboard_write_text(text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("there is nothing to copy".into());
    }
    crate::clipboard::set_text_blocking(text);
    Ok(())
}

/// Wireless debugging endpoints the phone is currently advertising over mDNS.
#[tauri::command]
pub async fn adb_list_wireless() -> Result<Vec<crate::adb::WirelessTarget>, String> {
    let st = crate::adb::detect(&crate::adb::AdbState::default()).await;
    match st.adb_path {
        Some(bin) => crate::adb::list_wireless(&bin).await,
        None => Err("adb is not installed".into()),
    }
}

/// Connect to a wireless debugging endpoint discovered over mDNS.
#[tauri::command]
pub async fn adb_connect_wireless(address: String) -> Result<String, String> {
    let st = crate::adb::detect(&crate::adb::AdbState::default()).await;
    match st.adb_path {
        Some(bin) => crate::adb::connect_wireless(&bin, &address).await,
        None => Err("adb is not installed".into()),
    }
}

#[tauri::command]
pub async fn scrcpy_running(state: Shared<'_>) -> Result<Vec<String>, String> {
    crate::adb::prune(state.inner().adb.as_ref()).await;
    Ok(crate::adb::running(state.inner().adb.as_ref()).await)
}

// ---------------------------------------------------------------- SSH

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshAvailability {
    pub available: bool,
    pub path: Option<String>,
}

#[tauri::command]
pub async fn ssh_available() -> SshAvailability {
    let path = crate::ssh::ssh_binary();
    SshAvailability {
        available: path.is_some(),
        path,
    }
}

#[tauri::command]
pub async fn ssh_probe(target: SshTarget) -> SshProbe {
    crate::ssh::probe(&target).await
}

#[tauri::command]
pub async fn ssh_open(target: SshTarget, terminal: Option<String>) -> Result<String, String> {
    crate::ssh::open_session(&target, terminal.as_deref()).await
}

// ------------------------------------------------------- file manager backend

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileManagerStatus {
    pub socket: String,
    pub listening: bool,
    pub python_extension_installed: bool,
    pub script_installed: bool,
    pub note: String,
}

#[tauri::command]
pub async fn filemanager_status() -> Result<FileManagerStatus, String> {
    let socket = crate::nautilus::socket_path();
    let listening = crate::nautilus::ping_backend().await;
    let python_ext = home_file(".local/share/nautilus-python/extensions/pixel-connect-share.py")
        .map(|p| p.exists())
        .unwrap_or(false);
    let script = home_file(".local/share/nautilus/scripts/pixel-connect-send")
        .map(|p| p.exists())
        .unwrap_or(false);
    let note = if python_ext {
        "The Nautilus context-menu entry \"Send to Pixel\" is installed."
    } else if script {
        "The Nautilus script is installed; the entry appears under \"Scripts\"."
    } else {
        "No file-manager integration is installed yet. Run scripts/install-filemanager-integration.sh."
    };
    Ok(FileManagerStatus {
        socket: socket.to_string_lossy().to_string(),
        listening,
        python_extension_installed: python_ext,
        script_installed: script,
        note: note.to_string(),
    })
}

fn home_file(rel: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(rel))
}

#[tauri::command]
pub async fn filemanager_send(
    app: AppHandle,
    request: SendRequest,
) -> Result<SendResponse, String> {
    Ok(crate::nautilus::dispatch(&app, request).await)
}

#[tauri::command]
pub async fn filemanager_send_paths(
    app: AppHandle,
    paths: Vec<String>,
    device_id: Option<String>,
) -> Result<SendResponse, String> {
    Ok(crate::nautilus::dispatch(
        &app,
        SendRequest {
            paths,
            device_id,
            reveal: true,
        },
    )
    .await)
}
