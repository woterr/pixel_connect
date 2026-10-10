// Pixel Connect — KDE Connect protocol v8, front-end facing types.
//
// These are serialised straight to the Vue front end by the Tauri layer, so
// they intentionally mirror what the Pixel Connect UI needs to display and are
// generated as TypeScript declarations via ts-rs.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::deviceinfo::DeviceType;
use crate::pairing::PairState;

/// How the current session reaches the peer.
///
/// Defined in `crate::transport` alongside the registry that reports which
/// transports are actually implemented.
pub use crate::transport::Transport;

/// Why a device is not reachable, if it isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LinkState {
    /// Not paired; nothing is being attempted.
    Unpaired,
    /// A pairing request is with the user.
    PairingRequested,
    /// The peer asked to pair and we are waiting for the user to confirm.
    PairingRequestedByPeer,
    /// Paired but no connection is currently open.
    Disconnected,
    /// Paired and connected.
    Connected,
    /// A connection attempt is in progress.
    Connecting,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct KcDevice {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    #[serde(rename = "protocolVersion")]
    pub protocol_version: i64,
    #[serde(rename = "pairState")]
    pub pair_state: PairState,
    pub link_state: LinkState,
    pub transport: Transport,
    /// `"192.168.1.42:1716"` when a LAN link is live, else `None`.
    #[serde(rename = "address")]
    pub address: Option<String>,
    #[serde(rename = "incomingCapabilities")]
    pub incoming_capabilities: Vec<String>,
    #[serde(rename = "outgoingCapabilities")]
    pub outgoing_capabilities: Vec<String>,
    /// Human-visible features resolved from the capability lists.
    pub features: Vec<String>,
    /// True when the peer presented a device id we already trust.
    pub trusted: bool,
    /// 8-character pairing verification key, only while pairing.
    #[serde(rename = "verificationKey")]
    pub verification_key: Option<String>,
    #[serde(rename = "protocolError")]
    pub protocol_error: Option<String>,
    #[serde(rename = "lastSeen")]
    pub last_seen: i64,
    #[serde(rename = "batteryLevel")]
    pub battery_level: Option<u8>,
    #[serde(rename = "batteryIsCharging")]
    pub battery_is_charging: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShareOffer {
    pub device_id: String,
    pub device_name: String,
    /// `file` | `text` | `url`
    pub kind: String,
    pub filename: Option<String>,
    pub filesize: Option<u64>,
    /// Remaining files in a multi-file share announced with `.update`.
    #[serde(rename = "pendingFiles")]
    pub pending_files: Option<u64>,
    pub text: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "numberOfFiles")]
    pub number_of_files: Option<u64>,
    #[serde(rename = "totalPayloadSize")]
    pub total_payload_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub id: String,
    pub device_id: String,
    pub filename: String,
    pub bytes: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// A notification received from a peer via the notification plugin.
///
/// Field names follow `kdeconnect.notification` as both implementations send
/// it (kdeconnect-android `NotificationsPlugin.kt`, kdeconnect-kde
/// `plugins/notifications/notification.cpp`):
///
/// * `id`   — the notification key; every reaction refers back to it
/// * `actions` — a JSON **array of action titles**, not separate body keys
/// * `requestReplyId` — present only when the notification can be replied to
/// * `time` — Android sends this as a *string*; the desktop omits it entirely
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteNotification {
    /// The peer's `id` for this notification, without any local prefix.
    pub id: String,
    pub title: String,
    pub text: String,
    #[serde(rename = "appName")]
    pub app_name: String,
    /// Unix milliseconds. Android sends `time` as a decimal string; the desktop
    /// omits it, so this is "now" when absent.
    pub timestamp: i64,
    /// Action titles offered by the notification, in the order the peer sent them.
    pub actions: Vec<String>,
    /// Set when the notification supports a typed reply. The value is the
    /// identifier to echo back in `kdeconnect.notification.reply`.
    #[serde(rename = "requestReplyId", skip_serializing_if = "Option::is_none")]
    pub request_reply_id: Option<String>,
    #[serde(rename = "isClearable")]
    pub is_clearable: bool,
    /// True when the peer marked the notification as silent/preexisting.
    pub silent: bool,
    /// Display name of the group, when the notification belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
}

/// Aggregate connectivity report from the phone.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityReport {
    pub device_id: String,
    #[serde(rename = "wifiStrength")]
    pub wifi_strength: Option<i64>,
    #[serde(rename = "wifiName")]
    pub wifi_name: Option<String>,
    #[serde(rename = "bluetoothStrength")]
    pub bluetooth_strength: Option<i64>,
    #[serde(rename = "cellularStrength")]
    pub cellular_strength: Option<i64>,
    #[serde(rename = "cellularName")]
    pub cellular_name: Option<String>,
}

/// Resolve the user-visible feature list from the peer's advertised
/// capabilities. Anything we cannot implement is simply absent from the list,
/// so the UI never shows a control it cannot honour.
pub fn features_from_capabilities(
    incoming: &[String],
    outgoing: &[String],
) -> Vec<String> {
    let has = |list: &[String], t: &str| list.iter().any(|c| c == t);

    let mut features = Vec::new();
    if has(incoming, crate::packet::PACKET_TYPE_CLIPBOARD)
        || has(outgoing, crate::packet::PACKET_TYPE_CLIPBOARD)
    {
        features.push("clipboard".into());
    }
    if has(incoming, crate::packet::PACKET_TYPE_SHARE_REQUEST) {
        features.push("receiveFiles".into());
    }
    if has(outgoing, crate::packet::PACKET_TYPE_SHARE_REQUEST) {
        features.push("shareFiles".into());
    }
    if has(outgoing, crate::packet::PACKET_TYPE_PING) {
        features.push("ping".into());
    }
    if has(outgoing, crate::packet::PACKET_TYPE_BATTERY) {
        features.push("battery".into());
    }
    if has(outgoing, crate::packet::PACKET_TYPE_CONNECTIVITY_REPORT) {
        features.push("connectivity".into());
    }
    if has(outgoing, crate::packet::PACKET_TYPE_NOTIFICATION) {
        features.push("notifications".into());
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_only_list_capabilities_the_peer_advertises() {
        let f = features_from_capabilities(
            &["kdeconnect.clipboard".into(), "kdeconnect.share.request".into()],
            &["kdeconnect.share.request".into(), "kdeconnect.ping".into()],
        );
        assert!(f.contains(&"clipboard".to_string()));
        assert!(f.contains(&"receiveFiles".to_string()));
        assert!(f.contains(&"shareFiles".to_string()));
        assert!(f.contains(&"ping".to_string()));
        assert!(!f.contains(&"battery".to_string()));
    }

    #[test]
    fn no_capabilities_means_no_features() {
        assert!(features_from_capabilities(&[], &[]).is_empty());
    }
}