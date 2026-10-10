// Pixel Connect — KDE Connect protocol v8, packet layer.
//
// Derived from the wire format implemented by
//   https://github.com/KDE/kdeconnect-kde      (core/networkpacket.cpp)
//   https://github.com/KDE/kdeconnect-android  (NetworkPacket.kt)
// which is licensed GPL-2.0-only OR GPL-3.0-only OR
// LicenseRef-KDE-Accepted-GPL.
//
// KDE Connect protocol version 8 framing has no length prefix and no packet
// type byte: every packet is one line of compact UTF-8 JSON terminated by a
// single '\n' byte. Payload bytes never appear inside this frame; they are
// streamed over a second, dedicated TLS connection whose port number is
// announced in `payloadTransferInfo`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// KDE Connect protocol version implemented here. Must be >= 8.
pub const PROTOCOL_VERSION: i64 = 8;

/// Hard cap on a single framed line (matches `MAX_PACKET_SIZE` upstream).
pub const MAX_PACKET_SIZE: usize = 32 * 1024 * 1024;

/// Identity packets are validated with a much smaller cap.
pub const MAX_IDENTITY_PACKET_SIZE: usize = 8192;

pub const PACKET_TYPE_IDENTITY: &str = "kdeconnect.identity";
pub const PACKET_TYPE_PAIR: &str = "kdeconnect.pair";

// Plugin packet types used by Pixel Connect.
pub const PACKET_TYPE_PING: &str = "kdeconnect.ping";
pub const PACKET_TYPE_BATTERY: &str = "kdeconnect.battery";
pub const PACKET_TYPE_CLIPBOARD: &str = "kdeconnect.clipboard";
pub const PACKET_TYPE_CLIPBOARD_CONNECT: &str = "kdeconnect.clipboard.connect";
pub const PACKET_TYPE_SHARE_REQUEST: &str = "kdeconnect.share.request";
pub const PACKET_TYPE_SHARE_REQUEST_UPDATE: &str = "kdeconnect.share.request.update";
pub const PACKET_TYPE_CONNECTIVITY_REPORT: &str = "kdeconnect.connectivity_report";
pub const PACKET_TYPE_NOTIFICATION: &str = "kdeconnect.notification";
pub const PACKET_TYPE_NOTIFICATION_ACTION: &str = "kdeconnect.notification.action";
/// Sent by a peer that wants the notifications the other end holds.
pub const PACKET_TYPE_NOTIFICATION_REQUEST: &str = "kdeconnect.notification.request";
/// Typed reply to a repliable notification.
pub const PACKET_TYPE_NOTIFICATION_REPLY: &str = "kdeconnect.notification.reply";

/// Body of a KDE Connect packet. `serde_json::Value` is used rather than
/// `QVariantMap`; it maps 1:1 onto the JSON object KDE Connect sends.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PacketBody(pub Map<String, Value>);

impl PacketBody {
    pub fn new() -> Self {
        Self(Map::new())
    }

    pub fn insert<V: Serialize>(&mut self, key: &str, value: V) -> &mut Self {
        self.0.insert(
            key.to_string(),
            serde_json::to_value(value).unwrap_or(Value::Null),
        );
        self
    }

    pub fn set<V: Serialize>(&mut self, key: &str, value: V) {
        self.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.0.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        match self.0.get(key) {
            Some(Value::Number(n)) => n.as_i64(),
            Some(Value::String(s)) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        match self.0.get(key) {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(s)) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.0.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::Number(n)) => n.as_i64().map(|v| v != 0),
            Some(Value::String(s)) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// Iterate the body entries, for serialising or comparing a whole body.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    pub fn get_array(&self, key: &str) -> Option<Vec<String>> {
        match self.0.get(key) {
            Some(Value::Array(a)) => Some(
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
            ),
            _ => None,
        }
    }
}

impl FromIterator<(String, Value)> for PacketBody {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        PacketBody(iter.into_iter().collect())
    }
}

/// A KDE Connect network packet (protocol >= 7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPacket {
    /// Milliseconds since the Unix epoch, filled in by `serialize()`.
    #[serde(default = "now_millis")]
    pub id: i64,
    #[serde(rename = "type")]
    pub packet_type: String,
    #[serde(default)]
    pub body: PacketBody,
    #[serde(
        rename = "payloadSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payload_size: Option<u64>,
    #[serde(
        rename = "payloadTransferInfo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub payload_transfer_info: Option<PacketBody>,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

impl NetworkPacket {
    pub fn new(packet_type: &str) -> Self {
        Self {
            id: now_millis(),
            packet_type: packet_type.to_string(),
            body: PacketBody::new(),
            payload_size: None,
            payload_transfer_info: None,
        }
    }

    pub fn with_body(packet_type: &str, body: PacketBody) -> Self {
        let mut p = Self::new(packet_type);
        p.body = body;
        p
    }

    pub fn set<V: Serialize>(&mut self, key: &str, value: V) -> &mut Self {
        self.body.set(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.body.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        self.body.get_str(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.body.has(key)
    }

    /// Whether a payload (for example an app icon) accompanies this packet.
    pub fn has_payload(&self) -> bool {
        self.payload_size.is_some_and(|n| n > 0)
    }

    /// Serialize to the wire format: compact JSON plus a single `\n`.
    pub fn serialize(&self) -> Vec<u8> {
        let mut obj = Map::new();
        obj.insert("id".into(), Value::from(self.id));
        obj.insert("type".into(), Value::from(self.packet_type.clone()));
        obj.insert("body".into(), Value::Object(self.body.0.clone()));
        if let Some(size) = self.payload_size {
            obj.insert("payloadSize".into(), Value::from(size));
            if let Some(info) = &self.payload_transfer_info {
                obj.insert("payloadTransferInfo".into(), Value::Object(info.0.clone()));
            }
        }
        let mut out = serde_json::to_vec(&Value::Object(obj)).unwrap_or_default();
        out.push(b'\n');
        out
    }

    /// Parse a frame (with or without the trailing newline).
    pub fn unserialize(raw: &[u8]) -> Result<Self, PacketError> {
        if raw.len() > MAX_PACKET_SIZE {
            return Err(PacketError::TooLarge(raw.len()));
        }
        let value: Value =
            serde_json::from_slice(raw).map_err(|e| PacketError::Malformed(e.to_string()))?;
        serde_json::from_value(value).map_err(|e| PacketError::Malformed(e.to_string()))
    }

    pub fn is_identity(&self) -> bool {
        self.packet_type == PACKET_TYPE_IDENTITY
    }
}

/// Capability sets advertised in the identity packet. Order is normalised so
/// that serialisation is stable (upstream uses QSet, which is unordered too).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities(pub BTreeMap<String, ()>);

impl Capabilities {
    pub fn from_iter_str<I, S>(items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut m = BTreeMap::new();
        for i in items {
            m.insert(i.into(), ());
        }
        Capabilities(m)
    }

    pub fn as_slice(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn contains(&self, s: &str) -> bool {
        self.0.contains_key(s)
    }

    pub fn insert(&mut self, s: &str) {
        self.0.insert(s.to_string(), ());
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PacketError {
    #[error("packet exceeds maximum size ({0} bytes)")]
    TooLarge(usize),
    #[error("malformed packet: {0}")]
    Malformed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_matches_upstream_wire_shape() {
        let mut np = NetworkPacket::new(PACKET_TYPE_CLIPBOARD);
        np.set("content", "hello");
        let raw = np.serialize();
        assert_eq!(raw.last(), Some(&b'\n'));

        let parsed = NetworkPacket::unserialize(&raw).unwrap();
        assert_eq!(parsed.packet_type, PACKET_TYPE_CLIPBOARD);
        assert_eq!(parsed.get_str("content").unwrap(), "hello");
    }

    #[test]
    fn qjson_style_string_numbers_are_tolerated() {
        // KDE Connect Android has historically sent protocolVersion as a string.
        let raw = br#"{"id":1,"type":"kdeconnect.identity","body":{"protocolVersion":"8","deviceId":"abc"}}"#;
        let np = NetworkPacket::unserialize(raw).unwrap();
        assert_eq!(np.body.get_i64("protocolVersion"), Some(8));
    }

    #[test]
    fn payload_size_without_transfer_info_is_not_serialized() {
        let mut np = NetworkPacket::new(PACKET_TYPE_SHARE_REQUEST);
        np.set("filename", "a.txt");
        assert!(!np.serialize().windows(11).any(|w| w == b"payloadSize"));
    }

    #[test]
    fn payload_transfer_info_is_serialized_with_payload() {
        let mut np = NetworkPacket::new(PACKET_TYPE_SHARE_REQUEST);
        np.payload_size = Some(12);
        let mut info = PacketBody::new();
        info.set("port", 1739);
        np.payload_transfer_info = Some(info);
        let raw = np.serialize();
        assert!(String::from_utf8_lossy(&raw).contains("payloadTransferInfo"));
        let back = NetworkPacket::unserialize(&raw).unwrap();
        assert_eq!(
            back.payload_transfer_info.unwrap().get_i64("port"),
            Some(1739)
        );
    }
}
