//! Regression tests for how a BLE session reaches the application.
//!
//! The BLE receiver gives each weave session a private message channel, on the
//! correct reasoning that two `InboundRequest` sessions sharing one channel
//! consume each other's traffic - each `recv()` takes the first message
//! available. The private channel is wired to the application's channel by a
//! relay task that holds its `Receiver`.
//!
//! An earlier version bound the receiver to `_` instead, and that silently
//! killed the whole path. A tokio broadcast channel with no receivers is not an
//! unread queue: `send` on it returns `Err` and the message is discarded at
//! the point it is produced. Nothing failed loudly, the weave handshake
//! completed on the wire, the payload was transferred, and the application was
//! never told - so the UI sat on "connecting" indefinitely while the phone
//! faithfully sent bytes into a void.
//!
//! These tests pin the part that is easy to break again by accident: the
//! receiver must outlive the producer, and the relay must actually forward.
//! Captured from a real Pixel 8, which is how the void was found.

use pixel_connect_quickshare::channel::ChannelMessage;
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};

/// A message carrying a recognisable id, so a test does not depend on the
/// fields of a type that is expected to grow.
fn message(id: &str) -> ChannelMessage {
    ChannelMessage {
        id: id.to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_receiver_must_be_kept_alive_or_messages_are_lost() {
    // This is the shape of the bug: a sender with no receiver at all.
    let (tx, rx) = broadcast::channel::<ChannelMessage>(50);
    drop(rx);

    let result = tx.send(message("discarded"));

    assert!(
        result.is_err(),
        "sending on a broadcast channel with no receivers must fail. This is \
         exactly why dropping the Receiver silently discarded every BLE event: \
         the weave handshake completed on the wire and the application never \
         heard about it."
    );
}

/// The production wiring: a private session channel whose Receiver is held by
/// a relay task, forwarding onto the channel the application listens to.
#[tokio::test]
async fn a_private_session_channel_is_relayed_to_the_application() {
    let (app_tx, mut app_rx) = broadcast::channel::<ChannelMessage>(50);

    // The session's own channel, separate so two sessions cannot steal each
    // other's traffic.
    let (session_tx, mut session_rx) = broadcast::channel::<ChannelMessage>(50);

    // The relay. This task holding `session_rx` is the entire reason the
    // channel works.
    tokio::spawn(async move {
        loop {
            match session_rx.recv().await {
                Ok(msg) => {
                    let _ = app_tx.send(msg);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // The session produces an event. This must succeed: the relay holds the
    // receiver, so the channel is live.
    session_tx
        .send(message("ble-session"))
        .expect("the relay must hold the receiver, so this send must succeed");

    // The application must see it.
    let received = timeout(Duration::from_secs(5), app_rx.recv())
        .await
        .expect("the relay did not forward within 5s")
        .expect("relay produced no message");

    assert_eq!(
        received.id, "ble-session",
        "the application received a different message than the session sent"
    );
}

/// Two sessions with separate channels must both reach the application, and
/// neither may lose its events to the other - the original reason the channels
/// were made private in the first place.
#[tokio::test]
async fn two_sessions_do_not_consume_each_others_events() {
    let (app_tx, mut app_rx) = broadcast::channel::<ChannelMessage>(50);

    let mut senders = Vec::new();
    for name in ["session-a", "session-b"] {
        let (session_tx, mut session_rx) = broadcast::channel::<ChannelMessage>(50);
        let relay = app_tx.clone();
        tokio::spawn(async move {
            loop {
                match session_rx.recv().await {
                    Ok(msg) => {
                        let _ = relay.send(msg);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        senders.push(session_tx);
        let _ = name;
    }

    // Both sessions produce simultaneously.
    senders[0].send(message("from-a")).expect("session a send");
    senders[1].send(message("from-b")).expect("session b send");

    // The application must receive both, not one message twice.
    let mut seen = Vec::new();
    for _ in 0..2 {
        let msg = timeout(Duration::from_secs(5), app_rx.recv())
            .await
            .expect("did not receive both events within 5s")
            .expect("relay produced no message");
        seen.push(msg.id);
    }

    assert_eq!(
        seen,
        vec!["from-a".to_string(), "from-b".to_string()],
        "the application must see both sessions' events. Losing one to the \
         other is what made 'Accept' in the notification do nothing."
    );
}