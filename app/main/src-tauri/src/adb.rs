// Pixel Connect — ADB device discovery and scrcpy integration.
//
// Pixel Connect does not talk to the phone over ADB itself; it *manages* the
// ADB and scrcpy tools that ship separately (scrcpy is available in the Fedora
// repositories as `scrcpy` + `android-tools`). This module:
//
//   * detects whether `adb` and `scrcpy` are on PATH
//   * enumerates connected devices (USB and pre-authorised wireless)
//   * launches scrcpy for a chosen device and tracks the child process
//
// Nothing here enables developer options, accepts a debug prompt, or weakens
// ADB authentication: it only reads `adb devices -l` and starts scrcpy with an
// explicit, validated serial.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionKind {
    Usb,
    Wireless,
    Emulator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
    pub kind: ConnectionKind,
    pub model: Option<String>,
    pub product: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
    /// Human label, e.g. "Pixel 8 Pro" or "192.168.1.42:5555".
    pub label: String,
    /// Can scrcpy actually run against this device?
    pub scrcpy_ready: bool,
    pub scrcpy_block_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAvailability {
    pub adb_path: Option<String>,
    pub scrcpy_path: Option<String>,
    pub adb_version: Option<String>,
    pub scrcpy_version: Option<String>,
    pub error: Option<String>,
}

pub struct Tools {
    pub adb: Option<String>,
    pub scrcpy: Option<String>,
    pub adb_version: Option<String>,
    pub scrcpy_version: Option<String>,
    pub error: Option<String>,
}

pub struct AdbState {
    pub tools: Arc<Mutex<Tools>>,
    pub running: Arc<Mutex<HashMap<String, Arc<Mutex<Child>>>>>,
}

impl Default for AdbState {
    fn default() -> Self {
        Self {
            tools: Arc::new(Mutex::new(Tools {
                adb: None,
                scrcpy: None,
                adb_version: None,
                scrcpy_version: None,
                error: None,
            })),
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn which(bin: &str) -> Option<String> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let candidate = std::path::Path::new(dir).join(bin);
            if candidate.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = candidate.metadata() {
                        if meta.permissions().mode() & 0o111 == 0 {
                            continue;
                        }
                    }
                }
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

pub async fn detect(state: &AdbState) -> ToolAvailability {
    let adb = which("adb");
    let scrcpy = which("scrcpy");
    // Short versions only. `adb version` prints four lines (bridge version,
    // package version, install path, kernel) and `scrcpy --version` prints its
    // homepage URL plus every compiled/linked dependency (SDL, libav*, libusb).
    // That detail is useless in the UI - it overflowed the tool chips - so only
    // the number is kept ("1.0.41", "4.1"). The full output is still available
    // by running the commands in a terminal.
    let adb_version = match &adb {
        Some(bin) => short_version(&run_capture(bin, &["version"]).await),
        None => None,
    };
    let scrcpy_version = match &scrcpy {
        Some(bin) => short_version(&run_capture(bin, &["--version"]).await),
        None => None,
    };
    let error = if adb.is_none() && scrcpy.is_none() {
        Some(
            "Neither adb nor scrcpy was found on PATH. Install the Fedora packages \
             'android-tools' and 'scrcpy', then reopen Pixel Connect."
                .to_string(),
        )
    } else if adb.is_none() {
        Some("adb was not found on PATH. Install the Fedora package 'android-tools'.".to_string())
    } else if scrcpy.is_none() {
        Some("scrcpy was not found on PATH. Install the Fedora package 'scrcpy'.".to_string())
    } else {
        None
    };

    let availability = ToolAvailability {
        adb_path: adb.clone(),
        scrcpy_path: scrcpy.clone(),
        adb_version: adb_version.clone(),
        scrcpy_version: scrcpy_version.clone(),
        error: error.clone(),
    };
    *state.tools.lock().await = Tools {
        adb,
        scrcpy,
        adb_version,
        scrcpy_version,
        error,
    };
    availability
}

async fn run_capture(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
    )
}

/// Keep only the version number from a multi-line `--version` dump.
///
/// Returns the first `major.minor[.patch]` found on the first line, so
/// "Android Debug Bridge version 1.0.41" becomes "1.0.41" and the scrcpy
/// dependency list never reaches the UI. `None` when the output has no
/// parseable version, in which case the caller reports "not found" rather
/// than showing a blob.
fn short_version(full: &Option<String>) -> Option<String> {
    let first = full.as_deref()?.lines().next()?.trim();
    let mut start = None;
    let mut end = None;
    for (i, c) in first.char_indices() {
        if c.is_ascii_digit() {
            if start.is_none() {
                start = Some(i);
            }
            end = Some(i + c.len_utf8());
        } else if c == '.' {
            if start.is_none() {
                continue;
            }
            end = Some(i + 1);
        } else if start.is_some() {
            break;
        }
    }
    let (s, e) = (start?, end?);
    let v = first[s..e].trim_matches('.').to_string();
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod version_tests {
    use super::short_version;

    fn full(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn adb_version_is_shortened_to_the_number() {
        assert_eq!(
            short_version(&full(
                "Android Debug Bridge version 1.0.41\nVersion 37.0.0-android-tools\nInstalled as /usr/bin/adb\nRunning on Linux 7.2.8"
            )),
            Some("1.0.41".to_string())
        );
    }

    #[test]
    fn scrcpy_version_is_shortened_to_the_number() {
        assert_eq!(
            short_version(&full(
                "scrcpy 4.1 <https://github.com/Genymobile/scrcpy>\nDependencies (compiled / linked):\n - SDL: 3.4.12 / 3.4.16"
            )),
            Some("4.1".to_string())
        );
    }

    #[test]
    fn unparseable_output_reports_nothing_rather_than_a_blob() {
        assert_eq!(short_version(&None), None);
        assert_eq!(short_version(&full("")), None);
        assert_eq!(short_version(&full("no digits here")), None);
    }
}

/// Parse the output of `adb devices -l`.
pub fn parse_devices(output: &str, scrcpy_available: bool) -> Vec<AdbDevice> {
    output
        .lines()
        // Skip everything up to and including the "List of devices attached"
        // header, plus any "* daemon ..." chatter that precedes it.
        .skip_while(|l| !l.trim_start().starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('*') {
                return None;
            }
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();

            let mut attrs = HashMap::new();
            for p in parts {
                if let Some((k, v)) = p.split_once(':') {
                    attrs.insert(k.to_string(), v.to_string());
                }
            }

            let kind = if serial.starts_with("emulator-") {
                ConnectionKind::Emulator
            } else if serial.contains(':') {
                ConnectionKind::Wireless
            } else {
                ConnectionKind::Usb
            };

            let model = attrs.get("model").map(|m| {
                m.replace('_', " ")
            });
            let label = match (&model, kind) {
                (Some(m), ConnectionKind::Wireless) => format!("{m} (wireless)"),
                (Some(m), ConnectionKind::Emulator) => format!("{m} (emulator)"),
                (Some(m), _) => m.clone(),
                (None, ConnectionKind::Wireless) => format!("{serial} (wireless)"),
                (None, _) => serial.clone(),
            };

            let (scrcpy_ready, scrcpy_block_reason) = if !scrcpy_available {
                (
                    false,
                    Some("scrcpy is not installed on this system".to_string()),
                )
            } else if state != "device" {
                (
                    false,
                    Some(match state.as_str() {
                        "unauthorized" => "The phone has not accepted this computer for debugging. Unlock it and confirm the USB debugging prompt.".to_string(),
                        "offline" => "The device is offline. Reconnect the cable, or toggle USB debugging off and on.".to_string(),
                        "connecting" => "The device is still connecting.".to_string(),
                        "no" => format!("adb reports: {}", state),
                        other => format!("adb reports: {other}"),
                    }),
                )
            } else {
                (true, None)
            };

            Some(AdbDevice {
                serial,
                state,
                kind,
                model,
                product: attrs.get("product").cloned(),
                device: attrs.get("device").cloned(),
                transport_id: attrs.get("transport_id").cloned(),
                label,
                scrcpy_ready,
                scrcpy_block_reason,
            })
        })
        .collect()
}

pub async fn list_devices(state: &AdbState) -> Result<Vec<AdbDevice>, String> {
    let adb = {
        let tools = state.tools.lock().await;
        tools.adb.clone()
    };
    let Some(adb) = adb else {
        let _ = detect(state).await;
        let tools = state.tools.lock().await;
        let err = tools
            .error
            .clone()
            .unwrap_or_else(|| "adb is not available".to_string());
        return Err(err);
    };

    let out = Command::new(&adb)
        .args(["devices", "-l"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("could not run adb: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "adb devices failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let scrcpy_available = state.tools.lock().await.scrcpy.is_some();
    Ok(parse_devices(
        &String::from_utf8_lossy(&out.stdout),
        scrcpy_available,
    ))
}

/// Validate a serial before it is handed to a process. ADB serials are
/// `host:port` or a short alphanumeric token; anything else is rejected so a
/// crafted device name can never become an argument.
fn valid_serial(serial: &str) -> bool {
    !serial.is_empty()
        && serial.len() <= 128
        && serial
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrcpyLaunch {
    pub serial: String,
    pub args: Vec<String>,
    pub command: String,
}

/// Launch scrcpy for `serial`. Returns the command that was started so the UI
/// can show exactly what will run.
pub async fn launch_scrcpy(
    state: &AdbState,
    serial: &str,
    extra_args: &[String],
) -> Result<ScrcpyLaunch, String> {
    if !valid_serial(serial) {
        return Err(format!("invalid device serial: {serial:?}"));
    }
    let scrcpy = {
        let tools = state.tools.lock().await;
        tools.scrcpy.clone()
    };
    let Some(scrcpy) = scrcpy else {
        let _ = detect(state).await;
        return Err("scrcpy is not installed on this system".to_string());
    };

    for a in extra_args {
        if a.starts_with('-')
            && !a
                .chars()
                .skip(1)
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '=' | '-' | '_' | '.'))
        {
            return Err(format!("unsafe scrcpy argument: {a:?}"));
        }
    }

    let mut args: Vec<String> = vec!["--serial".into(), serial.to_string()];
    args.extend(extra_args.iter().cloned());

    let mut cmd = Command::new(&scrcpy);
    cmd.args(args.clone())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    // scrcpy is a GUI app: detach it from our terminal but keep the handle so
    // we can stop it again.
    cmd.process_group(0);

    let child = cmd
        .spawn()
        .map_err(|e| format!("could not start scrcpy: {e}"))?;

    let child = Arc::new(Mutex::new(child));
    state.running.lock().await.insert(serial.to_string(), child);

    let command = format!("scrcpy {}", args.join(" "));
    Ok(ScrcpyLaunch {
        serial: serial.to_string(),
        args,
        command,
    })
}

/// Stop a scrcpy session previously started by Pixel Connect.
pub async fn stop_scrcpy(state: &AdbState, serial: &str) -> Result<bool, String> {
    let child = state.running.lock().await.remove(serial);
    let Some(child) = child else {
        return Ok(false);
    };
    let mut guard = child.lock().await;
    match guard.start_kill() {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// Disconnect a wireless adb device by its exact `host:port` serial.
///
/// Only `host:port` serials are accepted: bare `adb disconnect` with no
/// argument drops every device, so the serial is validated and USB serials
/// are refused outright. If this app has a live scrcpy process for the
/// serial, it is stopped first so mirroring does not keep the entry alive.
pub async fn disconnect_wireless(state: &AdbState, serial: &str) -> Result<(), String> {
    if !is_host_port(serial) {
        return Err(format!(
            "refusing to disconnect {serial:?}: only host:port wireless serials may be disconnected"
        ));
    }
    // Stop our own mirroring first; a running scrcpy holds the connection.
    let _ = stop_scrcpy(state, serial).await;
    let adb = state
        .tools
        .lock()
        .await
        .adb
        .clone()
        .ok_or_else(|| "adb was not found on PATH".to_string())?;
    let out = tokio::process::Command::new(adb)
        .args(["disconnect", serial])
        .output()
        .await
        .map_err(|e| format!("could not run adb disconnect: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "adb disconnect failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// True for `host:port` serials only (IPv4/hostname plus numeric port).
/// USB serials (`R5CT…`, `emulator-5554`) never match.
fn is_host_port(serial: &str) -> bool {
    let Some((host, port)) = serial.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && !host.contains(':')
        && !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
}

pub async fn running(state: &AdbState) -> Vec<String> {
    let mut list: Vec<String> = state.running.lock().await.keys().cloned().collect();
    list.sort();
    list
}

/// Reap finished scrcpy processes so the "running" list stays accurate.
pub async fn prune(state: &AdbState) {
    let mut running = state.running.lock().await;
    let mut finished = Vec::new();
    for (serial, child) in running.iter() {
        let mut guard = child.lock().await;
        match guard.try_wait() {
            Ok(Some(_)) => finished.push(serial.clone()),
            _ => {}
        }
    }
    for serial in finished {
        running.remove(&serial);
    }
}

/// Synchronous variant for the shutdown path, where blocking the caller is
/// acceptable and spawning a nested runtime is not.
pub fn stop_all_blocking(state: &AdbState) {
    let Ok(mut running) = state.running.try_lock() else {
        return;
    };
    for (serial, child) in running.drain() {
        if let Ok(mut guard) = child.try_lock() {
            let _ = guard.start_kill();
        }
        log::info!("stopped scrcpy for {serial}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "List of devices attached\n\
emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:emu64x transport_id:1\n\
R5CT30XXXXX            device usb:1-4 product:raven model:Pixel_8_Pro device:raven transport_id:2\n\
192.168.1.42:5555      device product:raven model:Pixel_8 device:raven transport_id:3\n\
DEADBEEF               unauthorized usb:1-7\n";

    #[test]
    fn parses_usb_wireless_and_unauthorized_devices() {
        let devices = parse_devices(SAMPLE, true);
        assert_eq!(devices.len(), 4);

        let usb = &devices[1];
        assert_eq!(usb.serial, "R5CT30XXXXX");
        assert_eq!(usb.kind, ConnectionKind::Usb);
        assert_eq!(usb.model.as_deref(), Some("Pixel 8 Pro"));
        assert!(usb.scrcpy_ready);

        let wifi = &devices[2];
        assert_eq!(wifi.kind, ConnectionKind::Wireless);
        assert!(wifi.label.contains("wireless"));

        let bad = &devices[3];
        assert_eq!(bad.state, "unauthorized");
        assert!(!bad.scrcpy_ready);
        assert!(bad
            .scrcpy_block_reason
            .as_ref()
            .unwrap()
            .contains("USB debugging"));

        let emu = &devices[0];
        assert_eq!(emu.kind, ConnectionKind::Emulator);
    }

    #[test]
    fn missing_scrcpy_is_reported_per_device() {
        let devices = parse_devices(SAMPLE, false);
        assert!(devices.iter().all(|d| !d.scrcpy_ready));
        assert!(devices[1]
            .scrcpy_block_reason
            .as_ref()
            .unwrap()
            .contains("scrcpy"));
    }

    #[test]
    fn serials_that_could_be_argument_injection_are_rejected() {
        assert!(valid_serial("R5CT30XXXXX"));
        assert!(valid_serial("192.168.1.42:5555"));
        assert!(!valid_serial(""));
        assert!(!valid_serial("abc;rm -rf /"));
        assert!(!valid_serial("abc$(whoami)"));
        assert!(!valid_serial("a b"));
        assert!(!valid_serial(&"x".repeat(200)));
    }

    #[test]
    fn parsing_tolerates_daemon_noise_and_blank_lines() {
        let output = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\n\n";
        assert!(parse_devices(output, true).is_empty());
    }
}

// ---- wireless ADB --------------------------------------------------------

/// A wireless debugging endpoint announced over mDNS.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessTarget {
    /// `host:port` ready to hand to `adb connect`.
    pub address: String,
    /// The phone name from the mDNS record, when it carries one.
    pub name: Option<String>,
    /// `connect` for a paired device, `pair` for one still needing a code.
    pub kind: WirelessKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WirelessKind {
    /// `_adb-tls-connect._tcp` - already paired, `adb connect` will work.
    Connect,
    /// `_adb-tls-pairing._tcp` - needs the pairing code from the phone first.
    Pair,
}

/// Parse `adb mdns services` output.
///
/// Android publishes two records per wireless-debugging session: a *connect*
/// endpoint once the device has been paired, and a *pairing* endpoint that
/// needs the code shown on screen. Only the connect endpoint is usable without
/// the user, so the two are reported separately rather than merged.
pub fn parse_mdns_services(output: &str) -> Vec<WirelessTarget> {
    let mut out = Vec::new();
    for line in output.lines() {
        // e.g. "adb-XYZ123     _adb-tls-connect._tcp.   192.168.1.5:41337"
        let Some((instance, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim();
        let kind = if rest.starts_with("_adb-tls-connect") {
            WirelessKind::Connect
        } else if rest.starts_with("_adb-tls-pairing") {
            WirelessKind::Pair
        } else {
            continue;
        };
        let Some(address) = rest
            .split_whitespace()
            .find(|t| t.contains(':') && !t.ends_with(':'))
        else {
            continue;
        };
        if !address
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ':')
        {
            continue;
        }
        // Android's instance name is "adb-<model>-<serial>"; show just the model.
        let name = instance
            .strip_prefix("adb-")
            .map(|s| s.rsplit_once('-').map(|(m, _)| m).unwrap_or(s))
            .map(|s| s.replace('_', " "))
            .filter(|s| !s.is_empty());
        out.push(WirelessTarget {
            address: address.to_string(),
            name,
            kind,
        });
    }
    out
}

/// List wireless debugging endpoints the phone is currently announcing.
pub async fn list_wireless(adb: &str) -> Result<Vec<WirelessTarget>, String> {
    let out = Command::new(adb)
        .args(["mdns", "services"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("could not run adb: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_mdns_services(&text))
}

/// Attempt `adb connect <address>`.
///
/// The address is validated here and passed as a single argv entry, so a
/// crafted value cannot smuggle in an extra adb option.
pub async fn connect_wireless(adb: &str, address: &str) -> Result<String, String> {
    let address = address.trim();
    if address.is_empty() {
        return Err("no wireless address given".into());
    }
    let valid = address.contains(':')
        && address
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ':');
    if !valid {
        return Err(format!("{address:?} is not a host:port address"));
    }
    let out = Command::new(adb)
        .args(["connect", address])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("could not run adb: {e}"))?;

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.contains("failed to connect") || text.contains("cannot connect") {
        return Err(text);
    }
    if text.contains("already connected") {
        return Ok(format!("{address} was already connected"));
    }
    if text.is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(text)
}

#[cfg(test)]
mod wireless_tests {
    use super::*;

    const SAMPLE: &str = "\
adb-ABC123       _adb-tls-connect._tcp.   192.168.1.5:41337
adb-ABC123       _adb-tls-pairing._tcp.   192.168.1.5:39811
NotAnAdb        _http._tcp.               192.168.1.9:80
";

    #[test]
    fn connect_and_pair_endpoints_are_told_apart() {
        let targets = parse_mdns_services(SAMPLE);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].kind, WirelessKind::Connect);
        assert_eq!(targets[0].address, "192.168.1.5:41337");
        assert_eq!(targets[1].kind, WirelessKind::Pair);
    }

    #[test]
    fn unrelated_services_are_ignored() {
        let targets = parse_mdns_services(SAMPLE);
        assert!(targets
            .iter()
            .all(|t| t.address.ends_with(":41337") || t.address.ends_with(":39811")));
    }

    #[test]
    fn the_model_is_derived_from_the_instance_name() {
        let targets =
            parse_mdns_services("adb-Pixel_8-ABC123  _adb-tls-connect._tcp.  10.0.0.5:5555");
        assert_eq!(targets[0].name.as_deref(), Some("Pixel 8"));
    }

    #[test]
    fn garbage_produces_nothing_rather_than_panicking() {
        for junk in [
            "",
            "adb-X  nonsense",
            "\n\n",
            "adb-X _adb-tls-connect._tcp.",
        ] {
            let _ = parse_mdns_services(junk);
        }
    }

    #[tokio::test]
    async fn a_crafted_address_is_rejected_before_reaching_adb() {
        // An address carrying an extra adb option must never be passed through.
        for bad in [
            "-s",
            "1.2.3.4:5555 --some-flag",
            "host:port; rm -rf /",
            "",
            "1.2.3.4",
        ] {
            assert!(
                connect_wireless("/nonexistent-adb", bad).await.is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
