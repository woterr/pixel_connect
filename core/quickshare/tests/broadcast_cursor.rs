//! Does a broadcast Receiver consume messages another Receiver has not read?
//!
//! This is the premise the BLE session's private channel was built on.
//!
//! The original reasoning was: "every InboundRequest subscribes to the channel
//! it was given and each `recv()` takes the first message, so two sessions
//! sharing one channel steal each other's traffic." That led to giving the BLE
//! receiver a private channel, and then to a relay - and the relay is only
//! wired in one direction. The result, measured on a Pixel 8, was that the
//! offer reached the application but the accept never reached the BLE session:
//!
//!     Asking for user consent: TransferMetadata { id: "ble-weave", ... }
//!     quickshare offer: Shreyas's Pixel 8 wants to share 1 file
//!     Unhandled connection state in process_transfer_setup: WaitingForUserConsent
//!     ... 55 seconds of keepalives ...
//!     weave: DISCONNECTION
//!
//! If the premise is false - if each Receiver has an independent cursor and
//! both see every message - then the private channel and the relay are not
//! needed, and are in fact the cause.

use pixel_connect_quickshare::channel::{
    ChannelMessage, Message, MessageClient, TransferKind,
};
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};

fn message(id: &str) -> ChannelMessage {
    ChannelMessage {
        id: id.to_string(),
        msg: Message::Client(MessageClient {
            kind: TransferKind::Inbound,
            state: None,
            metadata: None,
        }),
    }
}

/// The premise the private channel rested on, stated as a test.
///
/// Two sessions on one channel must BOTH see every message. If this fails, the
/// private channel is load-bearing and the one-way relay is the bug. If it
/// passes, the private channel can be removed.
#[tokio::test]
async fn two_receivers_on_one_channel_both_see_every_message() {
    let (tx, mut a) = broadcast::channel::<ChannelMessage>(50);
    let mut b = tx.subscribe();

    tx.send(message("first")).expect("send with two receivers");
    tx.send(message("second")).expect("send with two receivers");

    // Read them in the opposite order, so the test fails if a receiver is
    // draining a shared queue rather than keeping its own cursor.
    let from_b = timeout(Duration::from_secs(5), b.recv())
        .await
        .expect("receiver B timed out")
        .expect("receiver B got nothing");
    let from_a = timeout(Duration::from_secs(5), a.recv())
        .await
        .expect("receiver A timed out")
        .expect("receiver A got nothing");

    assert_eq!(from_b.id, "first", "receiver B must see the first message");
    assert_eq!(from_a.id, "first", "receiver A must see the first message too");

    let from_a2 = timeout(Duration::from_secs(5), a.recv())
        .await
        .expect("receiver A timed out on the second message")
        .expect("receiver A got nothing");
    assert_eq!(from_a2.id, "second", "receiver A must still see later messages");
}

/// The consequence, stated accurately.
///
/// `subscribe()` starts at the tail: a receiver created *after* a message was
/// sent never sees it. This is why the BLE session must be handed the channel
/// and subscribed before the application can send to it, rather than
/// subscribing lazily at the point of first use - a session that subscribes
/// when it is already connected would miss the accept waiting for it.
#[tokio::test]
async fn a_late_subscriber_does_not_see_earlier_messages() {
    let (tx, mut early) = broadcast::channel::<ChannelMessage>(50);
    tx.send(message("before")).expect("send");

    let mut late = tx.subscribe();

    // The original receiver still sees it.
    let seen_by_early = timeout(Duration::from_secs(5), early.recv())
        .await
        .expect("original receiver timed out")
        .expect("original receiver got nothing");
    assert_eq!(seen_by_early.id, "before");

    // The late subscriber does not - it starts at the tail.
    let result = timeout(Duration::from_millis(250), late.recv()).await;
    assert!(
        result.is_err(),
        "subscribe() starts at the tail; a receiver created after a send must \
         not see it, or sessions would race the messages meant for them"
    );
}
