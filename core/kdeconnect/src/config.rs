// Pixel Connect — KDE Connect protocol v8, identity and trust store.
//
// Mirrors the persistence layout of KDE Connect's `KdeConnectConfig`
// (kdeconnect-kde `core/kdeconnectconfig.cpp`): a config directory holding the
// device id/name, the TLS key + certificate, and one entry per trusted device
// with its pinned certificate. Pixel Connect uses its own directory so it never
// touches a real KDE Connect installation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::crypto::{certificate_der_from_pem, generate_device_id, generate_identity, DeviceIdentity};
use crate::deviceinfo::{sanitize_device_name, DeviceInfo, DeviceType};
use crate::packet::PROTOCOL_VERSION;

pub const PROTOCOL_VERSION_DEFAULT: i64 = PROTOCOL_VERSION;

/// SNI placeholder. KDE Connect peers do not use SNI and are matched on the
/// certificate CommonName instead (see `tls::verify_peer_identity*`), so this
/// name only has to satisfy rustls' DnsName parsing.
pub const DEFAULT_TLS_SERVER_NAME: &str = "pixel-connect.kdeconnect.local";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    #[serde(rename = "protocolVersion")]
    pub protocol_version: i64,
    /// PEM of the peer's self-signed certificate, pinned on pairing.
    pub certificate: String,
    /// Unix seconds of the last successful connection; purely informational.
    #[serde(rename = "lastSeen", default)]
    pub last_seen: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "deviceName")]
    pub device_name: String,
    #[serde(rename = "deviceId")]
    pub device_id: String,
    #[serde(rename = "deviceType")]
    pub device_type: DeviceType,
    #[serde(rename = "incomingPath", default)]
    pub incoming_path: Option<PathBuf>,
}

impl Default for DeviceType {
    fn default() -> Self {
        DeviceType::Desktop
    }
}

pub struct KdeConnectConfig {
    dir: PathBuf,
    settings: Settings,
    identity: DeviceIdentity,
    trusted: BTreeMap<String, TrustedDevice>,
}

impl KdeConnectConfig {
    /// Load (or create) the configuration rooted at `dir`.
    pub fn load(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create config dir {}", dir.display()))?;
        harden_dir(dir);

        let settings_path = dir.join("config.json");
        let settings: Settings = match std::fs::read_to_string(&settings_path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                log::warn!("config.json unreadable ({e}), regenerating");
                Settings::default()
            }),
            Err(_) => Settings {
                device_name: default_device_name(),
                device_id: String::new(),
                device_type: DeviceType::Desktop,
                incoming_path: None,
            },
        };

        let device_id = if settings.device_id.is_empty() {
            generate_device_id()
        } else {
            settings.device_id.clone()
        };

        let settings = Settings {
            device_name: sanitize_device_name(&settings.device_name),
            device_id,
            ..settings
        };

        let identity_path = dir.join("certificate.pem");
        let key_path = dir.join("key.pem");
        let identity = match (
            std::fs::read_to_string(&identity_path),
            std::fs::read_to_string(&key_path),
        ) {
            (Ok(cert_pem), Ok(key_pem)) if !cert_pem.is_empty() && !key_pem.is_empty() => {
                match certificate_der_from_pem(&cert_pem) {
                    Ok(der) => {
                        let public_key_der = rcgen::KeyPair::from_pem(&key_pem)
                            .ok()
                            .map(|kp| kp.public_key_der())
                            .unwrap_or_default();
                        DeviceIdentity {
                            cert_pem,
                            key_pem,
                            cert_der: der,
                            public_key_der,
                        }
                    }
                    Err(e) => {
                        log::warn!("stored certificate unusable ({e}), regenerating identity");
                        generate_identity(&settings.device_id, None)?
                    }
                }
            }
            _ => generate_identity(&settings.device_id, None)?,
        };

        let mut cfg = Self {
            dir: dir.to_path_buf(),
            settings,
            identity,
            trusted: BTreeMap::new(),
        };
        cfg.save_identity()?;
        cfg.save_settings()?;
        cfg.load_trusted()?;
        Ok(cfg)
    }

    fn save_settings(&self) -> Result<()> {
        let path = self.dir.join("config.json");
        write_private(&path, serde_json::to_vec_pretty(&self.settings)?.as_slice())
    }

    fn save_identity(&self) -> Result<()> {
        write_private(&self.dir.join("certificate.pem"), self.identity.cert_pem.as_bytes())?;
        write_private(&self.dir.join("key.pem"), self.identity.key_pem.as_bytes())
    }

    fn trusted_path(&self) -> PathBuf {
        self.dir.join("trusted_devices.json")
    }

    fn load_trusted(&mut self) -> Result<()> {
        let path = self.trusted_path();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<BTreeMap<String, TrustedDevice>>(&raw) {
                Ok(map) => self.trusted = map,
                Err(e) => log::warn!("trusted_devices.json unreadable ({e}), starting empty"),
            }
        }
        Ok(())
    }

    fn save_trusted(&self) -> Result<()> {
        let path = self.trusted_path();
        write_private(&path, serde_json::to_vec_pretty(&self.trusted)?.as_slice())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn device_id(&self) -> &str {
        &self.settings.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.settings.device_name
    }

    pub fn device_type(&self) -> DeviceType {
        self.settings.device_type
    }

    pub fn set_device_name(&mut self, name: &str) -> Result<()> {
        self.settings.device_name = sanitize_device_name(name);
        self.save_settings()
    }

    pub fn set_incoming_path(&mut self, path: Option<PathBuf>) -> Result<()> {
        self.settings.incoming_path = path;
        self.save_settings()
    }

    pub fn incoming_path(&self) -> Option<&PathBuf> {
        self.settings.incoming_path.as_ref()
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub fn certificate_pem(&self) -> &str {
        &self.identity.cert_pem
    }

    pub fn certificate_der(&self) -> &[u8] {
        &self.identity.cert_der
    }

    pub fn public_key_der(&self) -> &[u8] {
        &self.identity.public_key_der
    }

    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.trusted.contains_key(device_id)
    }

    pub fn trusted_devices(&self) -> Vec<TrustedDevice> {
        self.trusted.values().cloned().collect()
    }

    pub fn trusted_certificate_der(&self, device_id: &str) -> Option<Vec<u8>> {
        self.trusted
            .get(device_id)
            .and_then(|t| certificate_der_from_pem(&t.certificate).ok())
    }

    pub fn trust_device(&mut self, info: &DeviceInfo, certificate_pem: &str) -> Result<()> {
        let entry = TrustedDevice {
            id: info.id.clone(),
            name: info.name.clone(),
            device_type: info.device_type,
            protocol_version: info.protocol_version,
            certificate: certificate_pem.to_string(),
            last_seen: now_secs(),
        };
        self.trusted.insert(info.id.clone(), entry);
        self.save_trusted()
    }

    pub fn untrust_device(&mut self, device_id: &str) -> Result<bool> {
        let removed = self.trusted.remove(device_id).is_some();
        if removed {
            self.save_trusted()?;
        }
        Ok(removed)
    }

    pub fn touch_device(&mut self, device_id: &str) {
        if let Some(entry) = self.trusted.get_mut(device_id) {
            entry.last_seen = now_secs();
            let _ = self.save_trusted();
        }
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub fn default_device_name() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "Linux PC".to_string());
    sanitize_device_name(&host)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    harden_file(path);
    Ok(())
}

#[cfg(unix)]
fn harden_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn harden_file(_path: &Path) {}

#[cfg(unix)]
fn harden_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn harden_dir(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "pixel-connect-kc-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn creates_and_reloads_identity() {
        let dir = tmpdir("identity");
        let a = KdeConnectConfig::load(&dir).unwrap();
        let id = a.device_id().to_string();
        let der = a.certificate_der().to_vec();
        drop(a);

        let b = KdeConnectConfig::load(&dir).unwrap();
        assert_eq!(b.device_id(), id);
        assert_eq!(b.certificate_der(), der.as_slice());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trust_store_round_trips() {
        let dir = tmpdir("trust");
        let mut cfg = KdeConnectConfig::load(&dir).unwrap();
        let peer = DeviceInfo {
            id: "c".repeat(32),
            name: "Pixel".into(),
            device_type: DeviceType::Phone,
            protocol_version: PROTOCOL_VERSION,
            incoming_capabilities: vec![],
            outgoing_capabilities: vec![],
        };
        let pem = cfg.certificate_pem().to_string();
        cfg.trust_device(&peer, &pem).unwrap();
        assert!(cfg.is_trusted(&peer.id));
        assert!(cfg.trusted_certificate_der(&peer.id).is_some());
        drop(cfg);

        let cfg = KdeConnectConfig::load(&dir).unwrap();
        assert_eq!(cfg.trusted_devices().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpair_removes_trust() {
        let dir = tmpdir("unpair");
        let mut cfg = KdeConnectConfig::load(&dir).unwrap();
        let pem = cfg.certificate_pem().to_string();
        cfg.trust_device(
            &DeviceInfo {
                id: "d".repeat(32),
                name: "Phone".into(),
                device_type: DeviceType::Phone,
                protocol_version: PROTOCOL_VERSION,
                incoming_capabilities: vec![],
                outgoing_capabilities: vec![],
            },
            &pem,
        )
        .unwrap();
        assert!(cfg.untrust_device(&"d".repeat(32)).unwrap());
        assert!(!cfg.is_trusted(&"d".repeat(32)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}