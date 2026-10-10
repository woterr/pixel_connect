// Pixel Connect — KDE Connect protocol v8, device identity.
//
// Derived from KDE Connect's `core/deviceinfo.h` and the Android
// `DeviceInfo.kt`. Licensed upstream as GPL-2.0-only OR GPL-3.0-only OR
// LicenseRef-KDE-Accepted-GPL; see ../NOTICE.md.

pub use crate::packet::PROTOCOL_VERSION;
use crate::packet::{Capabilities, NetworkPacket, PacketBody, PACKET_TYPE_IDENTITY};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Device IDs are 32..=38 characters of `[A-Za-z0-9_-]`. Upstream rejects any
/// identity packet that does not match this, so we must emit a conforming one.
const DEVICE_ID_LEN: (usize, usize) = (32, 38);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename_all = "lowercase")]
pub enum DeviceType {
    /// A generic desktop is the safe default: it matches the widest set of
    /// KDE Connect peers and advertises no capabilities we have not checked.
    #[default]
    Desktop,
    Laptop,
    Phone,
    Tablet,
    Tv,
}

impl DeviceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::Desktop => "desktop",
            DeviceType::Laptop => "laptop",
            DeviceType::Phone => "phone",
            DeviceType::Tablet => "tablet",
            DeviceType::Tv => "tv",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "desktop" => Some(DeviceType::Desktop),
            "laptop" => Some(DeviceType::Laptop),
            "phone" | "smartphone" => Some(DeviceType::Phone),
            "tablet" => Some(DeviceType::Tablet),
            "tv" => Some(DeviceType::Tv),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    #[serde(rename = "protocolVersion")]
    pub protocol_version: i64,
    /// Packet types this device is willing to receive.
    #[serde(rename = "incomingCapabilities")]
    pub incoming_capabilities: Vec<String>,
    /// Packet types this device is willing to send.
    #[serde(rename = "outgoingCapabilities")]
    pub outgoing_capabilities: Vec<String>,
}

impl DeviceInfo {
    /// The full, encrypted identity packet exchanged after the TLS handshake.
    pub fn to_identity_packet(&self) -> NetworkPacket {
        let mut np = NetworkPacket::new(PACKET_TYPE_IDENTITY);
        np.set("deviceId", &self.id);
        np.set("deviceName", &self.name);
        np.set("deviceType", self.device_type.as_str());
        np.set("protocolVersion", self.protocol_version);
        np.set("incomingCapabilities", &self.incoming_capabilities);
        np.set("outgoingCapabilities", &self.outgoing_capabilities);
        np
    }

    /// The cleartext identity packet sent over UDP and as the first frame of a
    /// TCP connection. Matches `DeviceInfo::toUdpDiscoveryPacket()` and
    /// `DeviceInfo::toConnectionPacket()` upstream.
    pub fn to_discovery_packet(&self, tcp_port: u16) -> NetworkPacket {
        let mut np = NetworkPacket::new(PACKET_TYPE_IDENTITY);
        np.set("deviceId", &self.id);
        np.set("deviceName", &self.name);
        np.set("protocolVersion", self.protocol_version);
        np.set("tcpPort", i64::from(tcp_port));
        np
    }

    pub fn to_connection_packet(
        &self,
        target_device_id: &str,
        target_protocol_version: i64,
    ) -> NetworkPacket {
        let mut np = NetworkPacket::new(PACKET_TYPE_IDENTITY);
        np.set("deviceId", &self.id);
        np.set("deviceName", &self.name);
        np.set("protocolVersion", self.protocol_version);
        np.set("targetDeviceId", target_device_id);
        np.set("targetProtocolVersion", target_protocol_version);
        np
    }

    /// Validate and extract a peer's identity from an identity packet.
    ///
    /// `strict` additionally requires the full v8 field set (used after the TLS
    /// handshake); the cleartext TCP/UDP variants only carry a subset.
    pub fn from_identity_packet(np: &NetworkPacket, strict: bool) -> Result<Self, String> {
        if !np.is_identity() {
            return Err(format!(
                "expected {}, got {}",
                PACKET_TYPE_IDENTITY, np.packet_type
            ));
        }

        let id = np.get_str("deviceId").ok_or("identity has no deviceId")?;
        if !is_valid_device_id(&id) {
            return Err(format!("invalid device id: {id:?}"));
        }

        let name = np
            .get_str("deviceName")
            .ok_or("identity has no deviceName")?;
        if name.len() > 256 {
            return Err("device name too long".into());
        }

        let protocol_version = np
            .body
            .get_i64("protocolVersion")
            .ok_or("identity has no protocolVersion")?;

        if !strict {
            return Ok(DeviceInfo {
                id,
                name: sanitize_device_name(&name),
                device_type: DeviceType::Desktop,
                protocol_version,
                incoming_capabilities: Vec::new(),
                outgoing_capabilities: Vec::new(),
            });
        }

        if protocol_version < PROTOCOL_VERSION {
            return Err(format!(
                "peer speaks KDE Connect protocol {protocol_version}, this build requires {PROTOCOL_VERSION}"
            ));
        }

        let device_type = np
            .get_str("deviceType")
            .and_then(|t| DeviceType::parse(&t))
            .unwrap_or(DeviceType::Desktop);

        let incoming_capabilities = np
            .body
            .get_array("incomingCapabilities")
            .unwrap_or_default();
        let outgoing_capabilities = np
            .body
            .get_array("outgoingCapabilities")
            .unwrap_or_default();

        Ok(DeviceInfo {
            id,
            name: sanitize_device_name(&name),
            device_type,
            protocol_version,
            incoming_capabilities,
            outgoing_capabilities,
        })
    }

    /// Capabilities the peer announced it can *receive* from us.
    pub fn accepts_from_us(&self, packet_type: &str) -> bool {
        self.incoming_capabilities.iter().any(|c| c == packet_type)
    }

    /// Capabilities the peer announced it can *send* to us.
    pub fn sends_to_us(&self, packet_type: &str) -> bool {
        self.outgoing_capabilities.iter().any(|c| c == packet_type)
    }

    pub fn incoming(&self) -> Capabilities {
        Capabilities::from_iter_str(self.incoming_capabilities.clone())
    }
}

pub fn is_valid_device_id(id: &str) -> bool {
    let len = id.chars().count();
    if !(DEVICE_ID_LEN.0..=DEVICE_ID_LEN.1).contains(&len) {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// KDE Connect strips `"',;:.!?()[]<>` and truncates to 32 characters.
pub fn sanitize_device_name(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '"' | '\'' | ',' | ';' | ':' | '.' | '!' | '?' | '(' | ')' | '[' | ']' | '<' | '>'
            )
        })
        .take(32)
        .collect();
    if filtered.is_empty() {
        "Unknown device".to_string()
    } else {
        filtered
    }
}

/// Read the target device id out of the cleartext first frame of a TCP
/// connection.
pub fn connection_packet_target(np: &NetworkPacket) -> Option<(String, i64)> {
    if !np.is_identity() {
        return None;
    }
    let target = np.get_str("targetDeviceId")?;
    let version = np
        .body
        .get_i64("targetProtocolVersion")
        .unwrap_or(PROTOCOL_VERSION);
    Some((target, version))
}

pub fn discovery_packet_port(np: &NetworkPacket) -> Option<u16> {
    np.body
        .get_i64("tcpPort")
        .and_then(|p| u16::try_from(p).ok())
}

pub fn identity_body_summary(np: &NetworkPacket) -> PacketBody {
    np.body.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_are_generated_in_range() {
        let id = crate::crypto::generate_device_id();
        assert!(is_valid_device_id(&id), "{id}");
    }

    #[test]
    fn rejects_short_device_ids() {
        assert!(!is_valid_device_id("abc"));
        assert!(is_valid_device_id(&"a".repeat(32))); // 32 is the minimum
        assert!(is_valid_device_id(&"a".repeat(38))); // 38 is the maximum
        assert!(!is_valid_device_id(&"a".repeat(39)));
        assert!(!is_valid_device_id(&"a".repeat(31)));
    }

    #[test]
    fn identity_round_trip_is_strict() {
        let info = DeviceInfo {
            id: "a".repeat(32),
            name: "Pixel".into(),
            device_type: DeviceType::Phone,
            protocol_version: PROTOCOL_VERSION,
            incoming_capabilities: vec!["kdeconnect.clipboard".into()],
            outgoing_capabilities: vec!["kdeconnect.share.request".into()],
        };
        let raw = info.to_identity_packet().serialize();
        let np = NetworkPacket::unserialize(&raw).unwrap();
        let back = DeviceInfo::from_identity_packet(&np, true).unwrap();
        assert_eq!(info, back);
        assert!(back.accepts_from_us("kdeconnect.clipboard"));
        assert!(back.sends_to_us("kdeconnect.share.request"));
        assert!(!back.accepts_from_us("kdeconnect.share.request"));
    }

    #[test]
    fn rejects_downgrade_to_old_protocol() {
        let mut np = NetworkPacket::new(PACKET_TYPE_IDENTITY);
        np.set("deviceId", "b".repeat(32));
        np.set("deviceName", "old");
        np.set("protocolVersion", 7);
        np.set("deviceType", "phone");
        assert!(DeviceInfo::from_identity_packet(&np, true).is_err());
        // ... but a discovery packet from an old peer is still surfaced so the
        // user can be told what happened.
        assert!(DeviceInfo::from_identity_packet(&np, false).is_ok());
    }

    #[test]
    fn device_names_are_sanitised() {
        assert_eq!(sanitize_device_name("My Phone!"), "My Phone");
        assert_eq!(sanitize_device_name(""), "Unknown device");
        assert_eq!(sanitize_device_name(&"x".repeat(50)).len(), 32);
    }
}
