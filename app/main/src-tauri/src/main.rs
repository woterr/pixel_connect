// Pixel Connect — application entry point.
//
// Pixel Connect combines, in one process:
//   * Google Quick Share (pixel_connect_quickshare, derived from rquickshare)
//   * KDE Connect protocol v8 device integration (pixel_connect_kdeconnect)
//   * ADB device discovery and scrcpy lifecycle management
//   * an optional, explicitly configured SSH client integration
//   * a file-manager ("Send to Pixel") integration backend
//
// None of these require KDE Connect, kdeconnectd, kdeconnect-cli or GSConnect to
// be installed.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[macro_use]
extern crate log;

mod adb;
mod clipboard;
mod cmds;
mod kdc;
mod logger;
mod nautilus;
mod quickshare;
mod settings;
mod ssh;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pixel_connect_kdeconnect::KdeConnectService;
use pixel_connect_quickshare::channel::ChannelMessage;
use pixel_connect_quickshare::{EndpointInfo, QuickShareService, SendInfo, Visibility};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tokio::sync::{broadcast, mpsc, watch};

use crate::adb::AdbState;
use crate::kdc::KdcState;
use crate::logger::set_up_logging;

/// Shared backend state for the whole application.
pub struct AppState {
    pub quickshare: Mutex<QuickShareService>,
    pub message_sender: broadcast::Sender<ChannelMessage>,
    pub endpoint_sender: broadcast::Sender<EndpointInfo>,
    pub visibility_sender: Arc<Mutex<watch::Sender<Visibility>>>,
    pub send_sender: mpsc::Sender<SendInfo>,
    pub ble_receiver: broadcast::Receiver<()>,
    pub adb: Arc<AdbState>,
}

pub struct SharedState(pub Arc<AppState>);

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            trace!("another Pixel Connect instance was launched: {argv:?}");
            open_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            // Application
            cmds::app_info,
            cmds::get_settings,
            cmds::update_settings,
            cmds::open_help,
            cmds::quit_app,
            // Quick Share
            cmds::quickshare_start_discovery,
            cmds::quickshare_stop_discovery,
            cmds::quickshare_send_payload,
            cmds::quickshare_send_to_lib,
            cmds::quickshare_set_visibility,
            cmds::quickshare_set_download_path,
            cmds::quickshare_accept_transfer,
            cmds::quickshare_reject_transfer,
            cmds::get_hostname,
            // KDE Connect
            cmds::kde_devices,
            cmds::kde_device_info,
            cmds::kde_transports,
            cmds::kde_cancel_transfer,
            cmds::kde_transfers_in_flight,
            cmds::kde_refresh,
            cmds::kde_request_pairing,
            cmds::kde_accept_pairing,
            cmds::kde_reject_pairing,
            cmds::kde_unpair,
            cmds::kde_ping,
            cmds::kde_share_text,
            cmds::kde_share_files,
            cmds::kde_share_directory,
            cmds::kde_send_notification_action,
            cmds::kde_reply_to_notification,
            cmds::kde_pending_shares,
            cmds::kde_dismiss_share,
            cmds::kde_notifications,
            cmds::kde_clear_notifications,
            cmds::kde_request_clipboard,
            cmds::kde_set_clipboard_enabled,
            cmds::kde_set_clipboard_auto_share,
            cmds::kde_set_device_name,
            cmds::kde_set_incoming_path,
            // Clipboard
            cmds::clipboard_read,
            cmds::clipboard_write,
            cmds::clipboard_capabilities,
            // ADB / scrcpy
            cmds::adb_detect,
            cmds::adb_devices,
            cmds::scrcpy_launch,
            cmds::scrcpy_stop,
            cmds::scrcpy_running,
            cmds::adb_list_wireless,
            cmds::adb_connect_wireless,
            // SSH
            cmds::ssh_probe,
            cmds::ssh_open,
            cmds::ssh_available,
            // File manager integration
            cmds::filemanager_status,
            cmds::filemanager_send,
            cmds::filemanager_send_paths,
        ])
        .setup(|app| {
            set_up_logging(app.app_handle())?;
            let handle = app.app_handle().clone();

            // ---- settings -------------------------------------------------
            let s = settings::load(&handle);
            if s.kde_device_name.is_empty() {
                let default = crate::settings::default_device_name();
                settings::set(&handle, "kde_device_name", serde_json::json!(default));
            }

            // ---- tray icon ------------------------------------------------
            let name = MenuItemBuilder::new("Pixel Connect")
                .enabled(false)
                .build(app)?;
            let show = MenuItemBuilder::with_id("show", "Show Pixel Connect").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&name)
                .separator()
                .items(&[&show, &quit])
                .build()?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Pixel Connect has no window icon configured"))?;
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("Pixel Connect")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => open_main_window(app),
                    "quit" => quit_app(app),
                    _ => {}
                })
                .build(app)?;

            // ---- Quick Share service --------------------------------------
            let visibility = if s.visibility == 1 {
                Visibility::Invisible
            } else {
                Visibility::Visible
            };
            let download = Some(
                s.download_path
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(settings::default_download_dir),
            );

            let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<QuickShareService>>();
            let qs_vis = visibility;
            let qs_port = s.port;
            let qs_download = download.clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow::Error::from(e)));
                        return;
                    }
                };
                let result = rt.block_on(async move {
                    let mut qs = QuickShareService::new(qs_vis, qs_port, qs_download);
                    let _ = qs.run().await?;
                    Ok(qs)
                });
                let started = result.is_ok();
                let _ = tx.send(result);

                // `QuickShareService::run` spawns its TCP server, mDNS
                // advertising and BLE advertising onto this runtime and then
                // returns. Dropping the runtime cancels all of them, so the
                // service announces a port it is no longer listening on and
                // neither side can discover the other. Hold it open, exactly as
                // the KDE Connect backend thread above now does.
                if started {
                    rt.block_on(async { std::future::pending::<()>().await });
                }
            });
            let qs = rx
                .recv()
                .map_err(|_| anyhow::anyhow!("the Quick Share thread stopped unexpectedly"))??;
            let ble_receiver = qs.subscribe_ble();

            app.manage(Arc::new(AppState {
                message_sender: qs.message_sender.clone(),
                endpoint_sender: broadcast::channel(10).0,
                visibility_sender: qs.visibility_sender.clone(),
                send_sender: qs
                    .send_receiver()
                    .ok_or_else(|| anyhow::anyhow!("Quick Share did not provide a send channel"))?,
                ble_receiver,
                quickshare: Mutex::new(qs),
                adb: Arc::new(AdbState::default()),
            }));

            // ---- KDE Connect service --------------------------------------
            //
            // `setup` is synchronous, so the async start is driven on a worker
            // thread while the main thread waits on the channel. This avoids
            // nesting one runtime inside another.
            let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<KdcState>>();
            let kc_dir = kdc::kdeconnect_config_dir(&handle);
            let settings_snapshot = s.clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow::Error::from(e)));
                        return;
                    }
                };
                let result = rt.block_on(async move {
                    let (service, _rx) = KdeConnectService::start(&kc_dir).await?;
                    if !settings_snapshot.kde_device_name.is_empty() {
                        let _ = service
                            .set_device_name(&settings_snapshot.kde_device_name)
                            .await;
                    }
                    if let Some(incoming) = &settings_snapshot.incoming_path {
                        service
                            .set_incoming_path(std::path::PathBuf::from(incoming))
                            .await;
                    }
                    service.settings().clipboard_enabled.store(
                        settings_snapshot.clipboard_enabled,
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    service.settings().auto_clipboard.store(
                        settings_snapshot.clipboard_auto_share,
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    Ok(KdcState::new(service))
                });
                let started = result.is_ok();
                let _ = tx.send(result);

                // `KdeConnectService::start` spawns the accept loop, the UDP
                // discovery loop and the mDNS loop onto *this* runtime. Dropping
                // a Tokio runtime cancels every task still running on it, so
                // holding it for the life of the process is what keeps
                // discovery working. Letting it fall out of scope here is why
                // the application never saw a phone on the network despite
                // reporting a successful start: the TCP port was bound, the UDP
                // socket was created, and both were dropped microseconds later.
                if started {
                    rt.block_on(async { std::future::pending::<()>().await });
                }
            });
            let kdc_state = rx.recv().map_err(|_| {
                anyhow::anyhow!("the KDE Connect backend thread stopped unexpectedly")
            })??;
            kdc::persist_clipboard_prefs(&handle);
            app.manage(Arc::new(kdc_state));

            // ---- background work -----------------------------------------
            quickshare::spawn_forwarders(handle.clone());
            kdc::spawn_event_forwarder(handle.clone());
            tauri::async_runtime::spawn(nautilus::accept_loop(handle.clone()));

            // Clipboard -> phone. On X11 a watcher can read the clipboard from
            // the background; on Wayland it cannot, so the front end pushes
            // changes here instead (see clipboard_capabilities).
            let watcher_handle = handle.clone();
            clipboard::spawn_background_watcher(move |text| {
                let handle = watcher_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let Some(state) = handle.try_state::<Arc<KdcState>>() else {
                        return;
                    };
                    let content = clipboard::read_text().unwrap_or(text);
                    if let Err(e) = kdc::broadcast_clipboard(state.inner(), content).await {
                        log::error!("could not push the clipboard to the phone: {e}");
                    }
                });
            });

            // Periodic housekeeping.
            {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
                    loop {
                        tick.tick().await;
                        let Some(state) = handle.try_state::<Arc<KdcState>>() else {
                            continue;
                        };
                        kdc::refresh(state.inner()).await;
                    }
                });
            }
            {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<Arc<AppState>>();
                    let _ = adb::detect(state.adb.as_ref()).await;
                });
            }

            info!("Pixel Connect backend ready");
            Ok(())
        })
        .on_window_event(handle_window_event)
        .build(tauri::generate_context!())
        .map_err(|e| anyhow::anyhow!("could not build the Pixel Connect window: {e}"))?
        .run(|app, event| match event {
            tauri::RunEvent::Ready { .. } => {
                if settings::load(app).startminimized {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            }
            tauri::RunEvent::ExitRequested { code, .. } => {
                if code != Some(-1) {
                    shutdown(app);
                }
            }
            tauri::RunEvent::Exit => shutdown(app),
            _ => {}
        });

    info!("Pixel Connect stopped");
    Ok(())
}

fn handle_window_event(w: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if settings::get_bool(w.app_handle(), "realclose") {
            return;
        }
        let _ = w.hide();
        api.prevent_close();
    }
}

pub fn open_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn show_window(app: &AppHandle) {
    open_main_window(app);
}

pub fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<Arc<KdcState>>() {
        // `try_lock` avoids blocking inside the async runtime; if the service
        // is busy a worker thread finishes the job.
        let service = state.service.clone();
        let needs_worker = service.try_lock().is_err();
        if needs_worker {
            let _ = std::thread::spawn(move || {
                service.blocking_lock().shutdown();
            })
            .join();
        } else {
            service.blocking_lock().shutdown();
        }
    }
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        adb::stop_all_blocking(state.adb.as_ref());
    }
}

pub fn quit_app(app: &AppHandle) {
    shutdown(app);
    app.exit(-1);
}

pub fn notify(title: &str, body: &str) {
    match notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show()
    {
        Ok(_) => {}
        Err(e) => log::error!("could not show a notification: {e}"),
    }
}

/// Notify the user about the result of a "Send to Pixel" action.
pub fn notify_send_result(app: &AppHandle, sent: usize, device: Option<&str>) {
    let body = match (sent, device) {
        (0, Some(d)) => format!("Nothing was sent to {d}."),
        (1, Some(d)) => format!("1 item sent to {d}."),
        (n, Some(d)) => format!("{n} items sent to {d}."),
        (0, None) => "Nothing was sent.".to_string(),
        (n, None) => format!("{n} items queued for the phone."),
    };
    notify("Pixel Connect", &body);
    let _ = app;
}

pub fn notify_request(app: &AppHandle, name: &str, id: &str) {
    #[cfg(target_os = "linux")]
    {
        use notify_rust::Notification;
        match Notification::new()
            .summary("Pixel Connect")
            .body(&format!("{name} wants to send you files"))
            .action("accept", "Accept")
            .action("reject", "Reject")
            .show()
        {
            Ok(n) => {
                let handle = app.clone();
                let transfer_id = id.to_string();
                tauri::async_runtime::spawn(async move {
                    n.wait_for_action(|action| match action {
                        "accept" => {
                            let _ = crate::quickshare::send_to_lib(
                                handle.state::<Arc<AppState>>().inner(),
                                crate::quickshare::accept_transfer(transfer_id),
                            );
                        }
                        "reject" => {
                            let _ = crate::quickshare::send_to_lib(
                                handle.state::<Arc<AppState>>().inner(),
                                crate::quickshare::reject_transfer(transfer_id),
                            );
                        }
                        _ => {}
                    });
                });
            }
            Err(e) => log::error!("could not show the transfer request: {e}"),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (name, id);
        notify("Pixel Connect", "A device wants to send you files");
    }
}
