//! Properties the shared application channel now depends on.
//!
//! The BLE receiver and the TCP inbound are handed the same
//! `broadcast::Sender<ChannelMessage>`. Two facts have to hold for that to
//! work, and both were previously assumed rather than checked - the wrong
//! assumption about the second one cost a day of debugging, so it is checked
//! here. See also `broadcast_cursor.rs`, which covers cursor independence in
//! detail.
//!
//! 1. The channel must have a live Receiver. A tokio broadcast channel with no
//!    receivers is not an unread queue: `send` returns `Err` and the message is
//!    discarded at the point it is produced. That exact mistake was made here
//!    once, by binding a Receiver to `_`, and it silently swallowed every event
//!    of a transfer that had completed on the wire.
//!
//! 2. The application must hold its Receiver for the life of the process. The
//!    inbound sessions subscribe; the application listens. If the application
//!    dropped its Receiver, sends would start failing with no error surfacing
//!    anywhere near the transfer.

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

#[tokio::test]
async fn a_channel_with_no_receiver_discards_messages() {
    let (tx, rx) = broadcast::channel::<ChannelMessage>(50);
    drop(rx);

    assert!(
        tx.send(message("lost")).is_err(),
        "sending with no Receiver must fail. This is why dropping the \
         Receiver made a completed transfer invisible to the application."
    );
}

/// The production wiring, in the direction that actually failed: the
/// application sends a command, and a session subscribed to the same channel
/// receives it.
#[tokio::test]
async fn an_application_command_reaches_a_session() {
    // One channel, shared - as the TCP inbound and the BLE receiver now share it.
    let (tx, mut session) = broadcast::channel::<ChannelMessage>(50);

    // The application sends the accept.
    tx.send(message("accept-transfer-1"))
        .expect("the session's Receiver keeps the channel live");

    let got = timeout(Duration::from_secs(5), session.recv())
        .await
        .expect("the session never received the application's command")
        .expect("session got nothing");

    assert_eq!(
        got.id, "accept-transfer-1",
        "the session must receive what the application sends"
    );
}

/// And the other direction, which is what the one-way relay broke.
#[tokio::test]
async fn a_session_event_reaches_the_application() {
    let (tx, mut application) = broadcast::channel::<ChannelMessage>(50);
    let mut session = tx.subscribe();

    tx.send(message("quickshare-offer"))
        .expect("application Receiver keeps the channel live");

    let got = timeout(Duration::from_secs(5), application.recv())
        .await
        .expect("the application never received the session's event")
        .expect("application got nothing");

    assert_eq!(got.id, "quickshare-offer");

    // The session must see its own send too, since it shares the channel.
    let seen_by_session = timeout(Duration::from_secs(5), session.recv())
        .await
        .expect("session timed out")
        .expect("session got nothing");
    assert_eq!(seen_by_session.id, "quickshare-offer");
}

/// Both directions at once, which is the state a transfer is actually in when
/// the user presses Accept.
#[tokio::test]
async fn offer_then_accept_crosses_in_both_directions() {
    let (tx, mut application) = broadcast::channel::<ChannelMessage>(50);
    let mut session = tx.subscribe();

    // The session offers a file.
    tx.send(message("offer:ble-weave")).expect("offer send");

    // The application receives it and the user accepts.
    let offer = timeout(Duration::from_secs(5), application.recv())
        .await
        .expect("application never got the offer")
        .expect("application got nothing");
    assert_eq!(offer.id, "offer:ble-weave");

    tx.send(message("accept:ble-weave")).expect("accept send");

    // The session sees every message on the channel, including the offer it
    // sent itself. It therefore has to pick out what concerns it rather than
    // assume the next message is the answer - which is how the TCP inbound
    // already works, and is why this shared wiring is not new behaviour.
    //
    // What matters is that the accept is in there at all. This is the step
    // that failed when the session had a private channel with a one-way relay:
    // the transfer hung in WaitingForUserConsent until the peer disconnected.
    let mut seen = Vec::new();
    while !seen.iter().any(|id| id == "accept:ble-weave") {
        let msg = timeout(Duration::from_secs(5), session.recv())
            .await
            .expect("the session never received the accept - the transfer would \
                     hang in WaitingForUserConsent until the peer disconnected")
            .expect("session got nothing");
        seen.push(msg.id);
    }

    assert_eq!(
        seen,
        vec!["offer:ble-weave".to_string(), "accept:ble-weave".to_string()],
        "the session should see its own offer followed by the accept"
    );
}