// Pixel Connect — structured logging.
//
// Logs go to stdout and to a rotating file under the platform log directory.
// Sensitive values (clipboard contents, certificate material, credentials) are
// never written; see the call sites that produce log lines.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;

use fern::colors::{Color, ColoredLevelConfig};
use tauri::AppHandle;
use tauri::Manager;
use time::OffsetDateTime;

use crate::settings;

/// Environment variable that overrides the configured log level, matching the
/// name rquickshare used.
pub const LOG_ENV: &str = "PIXEL_CONNECT_LOG";

pub fn set_up_logging(app: &AppHandle) -> Result<(), anyhow::Error> {
    let level = match std::env::var(LOG_ENV) {
        Ok(v) => log::LevelFilter::from_str(&v).unwrap_or(log::LevelFilter::Debug),
        Err(_) => match settings::load(app).debug_level {
            Some(level) => log::LevelFilter::from_str(&level).unwrap_or(log::LevelFilter::Info),
            None => {
                if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                }
            }
        },
    };

    let colors = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        .info(Color::Green)
        .debug(Color::Blue)
        .trace(Color::Cyan);

    let dispatch = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "\x1B[2m{date}\x1b[0m {level: >5} \x1B[2m{target}:\x1b[0m {message}",
                date = humantime::format_rfc3339_seconds(SystemTime::now()),
                target = record.target(),
                level = colors.color(record.level()),
                message = message,
            ));
        })
        .level(level)
        .level_for("mdns_sd", log::LevelFilter::Error)
        .level_for("polling", log::LevelFilter::Error)
        .level_for("neli", log::LevelFilter::Error)
        .level_for("bluez_async", log::LevelFilter::Error)
        .level_for("bluer", log::LevelFilter::Error)
        .level_for("btleplug", log::LevelFilter::Error)
        .level_for("rustls", log::LevelFilter::Error)
        .chain(std::io::stdout());

    if let Ok(dir) = app.path().app_log_dir() {
        std::fs::create_dir_all(&dir)?;
        let name = app.package_info().name.clone();
        let path = rotate(&dir, &name, 4 * 1024 * 1024)?;
        let file = fern::log_file(path)?;
        dispatch.chain(file).apply()?;
    } else {
        dispatch.apply()?;
    }

    Ok(())
}

fn rotate(dir: &Path, name: &str, max_bytes: u128) -> Result<PathBuf, anyhow::Error> {
    let path = dir.join(format!("{name}.log"));
    if path.exists() {
        let size = File::open(&path)?.metadata()?.len() as u128;
        if size > max_bytes {
            let stamp = OffsetDateTime::now_utc()
                .format(&time::format_description::parse(
                    "[year]-[month]-[day]_[hour]-[minute]-[second]",
                ).expect("valid format string"))
                .expect("a timestamp always formats");
            let archived = dir.join(format!("{name}_{stamp}.log"));
            if archived.exists() {
                let bak = archived.with_extension("log.bak");
                let _ = std::fs::rename(&archived, bak);
            }
            let _ = std::fs::rename(&path, archived);
        }
    }
    Ok(path)
}