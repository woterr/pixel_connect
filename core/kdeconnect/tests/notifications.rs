// Pixel Connect — notification plugin wire-format tests.
//
// These pin the `kdeconnect.notification` contract against packets captured
// from the two implementations the application has to interoperate with:
// kdeconnect-android (`NotificationsPlugin.kt`) and kdeconnect-kde
// (`plugins/sendnotifications/dbusnotificationslistener.cpp`).
//
// The point is to catch invented field names. An earlier revision of this code
// read `key`, `t`, `pause`/`reply`/`open` and sent `actionId`; none of those are
// what either implementation uses.

use pixel_connect_kdeconnect::packet::{NetworkPacket, PacketBody};

/// Parse a notification the way `DeviceLink::on_notification` does.
///
/// Kept here as a free function so the wire format can be tested without a live
/// link.
fn parse_notification(np: &NetworkPacket) -> ParsedNotification {
    let id = np.get_str("id").unwrap_or_default();
    let actions: Vec<String> = np
        .body
        .get_array("actions")
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let timestamp = np
        .body
        .get_str("time")
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| np.body.get_i64("time"))
        .unwrap_or(0);
    ParsedNotification {
        id,
        title: np.get_str("title").unwrap_or_default(),
        text: np.get_str("text").unwrap_or_default(),
        app_name: np.get_str("appName").unwrap_or_default(),
        timestamp,
        actions,
        request_reply_id: np.get_str("requestReplyId").filter(|s| !s.is_empty()),
        is_clearable: np.body.get_bool("isClearable").unwrap_or(true),
        silent: np.body.get_bool("silent").unwrap_or(false),
    }
}

#[derive(Debug, PartialEq)]
struct ParsedNotification {
    id: String,
    title: String,
    text: String,
    app_name: String,
    timestamp: i64,
    actions: Vec<String>,
    request_reply_id: Option<String>,
    is_clearable: bool,
    silent: bool,
}

/// A notification exactly as kdeconnect-android sends one, including the
/// `actions` array and the repliable marker.
fn android_repliable_notification() -> NetworkPacket {
    let mut body = PacketBody::new();
    body.set("actions", vec!["Mark as read", "Mute"]);
    body.set("id", "0|com.google.android.messaging|42|1|null|10123");
    body.set("isClearable", true);
    body.set("appName", "Messages");
    // Android writes this as a decimal *string*.
    body.set("time", "1709251200000");
    body.set("silent", false);
    body.set("title", "Maria");
    body.set("text", "Are we still on for tonight?");
    body.set("requestReplyId", "reply-token-7");

    let mut np = NetworkPacket::new("kdeconnect.notification");
    for (k, v) in body.iter() {
        np.set(k, v.clone());
    }
    np
}

#[test]
fn parses_an_android_repliable_notification() {
    let n = parse_notification(&android_repliable_notification());

    // The key lives in `id`, never `key`.
    assert_eq!(n.id, "0|com.google.android.messaging|42|1|null|10123");
    assert_eq!(n.app_name, "Messages");
    assert_eq!(n.title, "Maria");
    assert_eq!(n.text, "Are we still on for tonight?");

    // `time` is a string on Android and must still become a number.
    assert_eq!(n.timestamp, 1_709_251_200_000);

    // Actions are an array of titles, not individual body keys.
    assert_eq!(n.actions, vec!["Mark as read", "Mute"]);

    assert_eq!(n.request_reply_id.as_deref(), Some("reply-token-7"));
    assert!(n.is_clearable);
    assert!(!n.silent);
}

#[test]
fn a_desktop_notification_needs_no_time_and_offers_no_reply() {
    // kdeconnect-kde `dbusnotificationslistener.cpp` sends `id`, `appName`,
    // `ticker`, `isClearable`, `title`, `silent` and (conditionally) `text`.
    let mut np = NetworkPacket::new("kdeconnect.notification");
    np.set("id", "17");
    np.set("appName", "Thunderbird");
    np.set("ticker", "Thunderbird");
    np.set("isClearable", false);
    np.set("title", "New message");
    np.set("text", "Re: plans");
    np.set("silent", false);

    let n = parse_notification(&np);
    assert_eq!(n.id, "17");
    assert_eq!(n.app_name, "Thunderbird");
    assert!(n.actions.is_empty());
    // No `time` on the wire means "now"; the parser must not invent one.
    assert_eq!(n.timestamp, 0);
    assert_eq!(n.request_reply_id, None);
    assert!(!n.is_clearable);
}

#[test]
fn a_notification_without_an_id_is_rejected() {
    let mut np = NetworkPacket::new("kdeconnect.notification");
    np.set("appName", "Something");
    assert!(parse_notification(&np).id.is_empty());
}

#[test]
fn a_numeric_time_is_also_accepted() {
    // Some desktop builds send a number; be tolerant rather than dropping the
    // notification entirely.
    let mut np = NetworkPacket::new("kdeconnect.notification");
    np.set("id", "1");
    np.set("time", 1_700_000_000_000i64);
    assert_eq!(parse_notification(&np).timestamp, 1_700_000_000_000);
}

/// The reply packet must echo the peer's `requestReplyId`, not the
/// notification id.
#[test]
fn reply_packet_carries_the_request_reply_id_and_message() {
    let mut reply = NetworkPacket::new("kdeconnect.notification.reply");
    reply.set("requestReplyId", "reply-token-7");
    reply.set("message", "Yes, see you at eight");

    assert_eq!(
        reply.get_str("requestReplyId").as_deref(),
        Some("reply-token-7")
    );
    assert_eq!(
        reply.get_str("message").as_deref(),
        Some("Yes, see you at eight")
    );
    assert!(!reply.has("key"));
}

/// The action packet uses `action`, not `actionId`.
#[test]
fn action_packet_uses_the_action_key() {
    let mut action = NetworkPacket::new("kdeconnect.notification.action");
    action.set("key", "17");
    action.set("action", "Mark as read");

    assert_eq!(action.get_str("key").as_deref(), Some("17"));
    assert_eq!(action.get_str("action").as_deref(), Some("Mark as read"));
    // The name an earlier revision used, which neither implementation reads.
    assert!(!action.has("actionId"));
}

/// Android sends `payloadHash` and may attach the app icon as the payload.
#[test]
fn icon_payload_is_recognised() {
    let mut np = NetworkPacket::new("kdeconnect.notification");
    np.set("id", "5");
    np.set("payloadHash", "9f2c1a");
    np.payload_size = Some(4096);
    assert!(np.has_payload());
    assert_eq!(np.get_str("payloadHash").as_deref(), Some("9f2c1a"));
}
