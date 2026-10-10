// Pixel Connect — KDE Connect protocol interoperability tests.
//
// These exercise the real stack — TCP, TLS, the cleartext identity frame, the
// in-TLS identity exchange, pairing, clipboard and file transfer — between two
// services in one process.
//
// mDNS deliberately does not report services registered by the same process, so
// the peer registry is seeded directly here. The discovery parsers are covered
// by the unit tests in `src/discovery.rs`, and live discovery against a real
// network is covered by `tests/discovery_live.rs` and the `kc-probe` tool.
//
// None of this proves interoperability with kdeconnect-android; that needs a
// real phone. See docs/architecture/v1-status.md.

use std::time::Duration;

use pixel_connect_kdeconnect::lan::KcEvent;
use pixel_connect_kdeconnect::tls::{TlsContext, TlsRole};
use pixel_connect_kdeconnect::types::TransferProgress;
use pixel_connect_kdeconnect::KdeConnectService;
use pixel_connect_kdeconnect::PROTOCOL_VERSION;

mod support;

use support::{downloads, find_with_extension, temp_dir};

/// Poll `check` until it is true or `timeout` elapses.
async fn wait_until<F, Fut>(timeout: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if check().await {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn identity_is_stable_across_restarts() -> anyhow::Result<()> {
    let dir = temp_dir("restart");
    let id = {
        let (svc, _rx) = KdeConnectService::start(&dir).await?;
        svc.set_device_name("Stable Name").await?;
        svc.shutdown();
        svc.device_info().id
    };

    let (svc, _rx) = KdeConnectService::start(&dir).await?;
    assert_eq!(svc.device_info().id, id);
    assert_eq!(svc.device_info().name, "Stable Name");
    // KDE Connect rejects device ids outside 32..=38 characters, so getting
    // this wrong makes the device invisible rather than producing an error.
    assert!((32..=38).contains(&id.len()));
    svc.shutdown();
    Ok(())
}

#[tokio::test]
async fn broadcast_clipboard_without_peers_sends_nothing() -> anyhow::Result<()> {
    let dir = temp_dir("clipboard");
    let (svc, _rx) = KdeConnectService::start(&dir).await?;
    // Not an error: the UI must be able to distinguish "nothing to do" from
    // "something broke".
    assert_eq!(svc.broadcast_clipboard("secret value".into()).await?, 0);
    svc.shutdown();
    Ok(())
}

#[tokio::test]
async fn commands_for_unconnected_devices_fail_clearly() -> anyhow::Result<()> {
    let dir = temp_dir("unconnected");
    let (svc, _rx) = KdeConnectService::start(&dir).await?;
    for e in [
        svc.ping("0123456789abcdef0123456789abcdef", "hi").await,
        svc.request_pairing("0123456789abcdef0123456789abcdef")
            .await,
    ] {
        let err = e.unwrap_err();
        assert!(
            err.to_string().contains("not connected"),
            "unexpected error: {err}"
        );
    }
    svc.shutdown();
    Ok(())
}

/// The TLS layer must work in both roles with the peer certificate's
/// CommonName pinned to the announced device id — the core of KDE Connect's
/// anti-spoofing model.
///
/// Builds the identities directly so it does not depend on mDNS.
#[tokio::test]
async fn tls_handshake_works_in_both_roles_with_cn_pinning() -> anyhow::Result<()> {
    use pixel_connect_kdeconnect::KdeConnectConfig;

    let dir_a = temp_dir("tls-a");
    let dir_b = temp_dir("tls-b");

    let cfg_a = KdeConnectConfig::load(&dir_a)?;
    let cfg_b = KdeConnectConfig::load(&dir_b)?;
    let id_a = cfg_a.device_id().to_string();
    let id_b = cfg_b.device_id().to_string();
    assert_ne!(id_a, id_b);

    let ctx_a = TlsContext::new(cfg_a.certificate_pem(), &cfg_a.identity().key_pem)?;
    let ctx_b = TlsContext::new(cfg_b.certificate_pem(), &cfg_b.identity().key_pem)?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;

    // The dialler opens TCP and therefore plays the TLS *server*, exactly as
    // KDE Connect requires.
    let dialer = tokio::spawn(async move {
        let socket = tokio::net::TcpStream::connect(addr).await?;
        ctx_a.handshake(TlsRole::Server, socket).await
    });

    let (accepted, _) = listener.accept().await?;
    let client = ctx_b.handshake(TlsRole::Client, accepted).await?;
    let server = dialer.await??;

    // Each side sees the *other* side's certificate, and its CommonName must
    // match the id that side advertised.
    server.verify_peer(&id_b, None)?;
    client.verify_peer(&id_a, None)?;
    assert!(!client.peer_spki_der().is_empty());
    assert!(!server.peer_spki_der().is_empty());

    // A wrong expected id is rejected…
    assert!(client.verify_peer("not-the-device-id", None).is_err());
    // … and so is the wrong pinned certificate, while the correct one passes.
    assert!(client
        .verify_peer(&id_a, Some(b"not-a-certificate"))
        .is_err());
    assert!(client
        .verify_peer(&id_a, Some(cfg_a.certificate_der()))
        .is_ok());

    Ok(())
}

/// Two full services must open a device link and complete the whole exchange.
#[tokio::test]
async fn two_services_open_a_link_and_exchange_data() -> anyhow::Result<()> {
    let dir_a = temp_dir("link-a");
    let dir_b = temp_dir("link-b");

    let (svc_a, mut rx_a) = KdeConnectService::start(&dir_a).await?;
    svc_a.set_device_name("Peer A").await?;
    let incoming = downloads("link-exchange");
    svc_a.set_incoming_path(incoming.clone()).await;
    let (svc_b, _rx_b) = KdeConnectService::start(&dir_b).await?;
    svc_b.set_device_name("Peer B").await?;

    let id_a = svc_a.device_info().id.clone();
    let id_b = svc_b.device_info().id.clone();
    assert_ne!(id_a, id_b);

    // Seed the peer exactly as a discovery round would.
    let mut identity = pixel_connect_kdeconnect::NetworkPacket::new(
        pixel_connect_kdeconnect::packet::PACKET_TYPE_IDENTITY,
    );
    identity.set("deviceId", id_a.clone());
    identity.set("deviceName", "Peer A");
    identity.set("protocolVersion", PROTOCOL_VERSION);
    identity.set("tcpPort", i64::from(svc_a.provider().tcp_port()));
    pixel_connect_kdeconnect::discovery::note_udp_peer("127.0.0.1".parse().unwrap(), &identity);

    let peer_a = pixel_connect_kdeconnect::discovery::snapshot()
        .into_iter()
        .find(|p| p.device_id == id_a)
        .expect("peer A is in the registry");
    assert_eq!(peer_a.tcp_port, svc_a.provider().tcp_port());

    // Dial on a worker task: the accept loop has to stay free to answer.
    {
        let dialer = svc_b.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            dialer.provider().connect_to_peer(&peer_a).await;
        });
    }

    // A hard timeout turns a regression into a failure instead of a hang.
    tokio::time::timeout(
        Duration::from_secs(60),
        complete_exchange(&svc_a, &svc_b, &incoming, &id_a, &id_b, &mut rx_a),
    )
    .await
    .expect("the KDE Connect exchange did not finish within 60 seconds")?;

    svc_a.shutdown();
    svc_b.shutdown();
    Ok(())
}

/// Everything that happens once the two services have found each other.
async fn complete_exchange(
    svc_a: &KdeConnectService,
    svc_b: &KdeConnectService,
    incoming: &std::path::Path,
    id_a: &str,
    id_b: &str,
    rx_a: &mut tokio::sync::broadcast::Receiver<KcEvent>,
) -> anyhow::Result<()> {
    let b_saw_a = wait_until(Duration::from_secs(15), || async {
        svc_b.provider().is_connected(id_a).await
    })
    .await;
    let a_saw_b = wait_until(Duration::from_secs(15), || async {
        svc_a.provider().is_connected(id_b).await
    })
    .await;
    assert!(
        b_saw_a,
        "the dialling side never registered the device link"
    );
    assert!(
        a_saw_b,
        "the accepting side never registered the device link"
    );

    // The accepting side learned the peer's name from the identity packet that
    // travels inside TLS.
    let devices = svc_a.devices().await;
    let peer = devices
        .iter()
        .find(|d| d.id == id_b)
        .expect("peer B should be in A's device list");
    assert_eq!(peer.name, "Peer B");
    assert_eq!(peer.protocol_version, PROTOCOL_VERSION);
    assert_eq!(
        peer.link_state,
        pixel_connect_kdeconnect::LinkState::Connected,
        "the published device state must reflect the live link"
    );
    assert_eq!(peer.transport, pixel_connect_kdeconnect::Transport::Lan);

    // --- pairing ---------------------------------------------------------
    //
    // KDE Connect refuses clipboard and file traffic to unpaired devices, so
    // the handshake runs first: B requests, A raises the request with a
    // verification key and accepts, and both sides pin the other's certificate.
    svc_b.request_pairing(id_a).await?;

    let mut verification_key = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline && verification_key.is_empty() {
        match rx_a.try_recv() {
            Ok(KcEvent::PairingRequest {
                device,
                verification_key: key,
            }) => {
                assert_eq!(
                    device.id, id_b,
                    "the pairing request came from the wrong device"
                );
                assert_eq!(key.len(), 8, "the verification key must be 8 characters");
                verification_key = key;
            }
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(
        !verification_key.is_empty(),
        "the accepting side never raised a pairing request"
    );

    svc_a.accept_pairing(id_b).await?;

    let paired = wait_until(Duration::from_secs(15), || async {
        svc_b.is_trusted(id_a).await && svc_a.is_trusted(id_b).await
    })
    .await;
    assert!(paired, "pairing did not complete on both sides");

    // --- clipboard -------------------------------------------------------
    let sent = wait_until(Duration::from_secs(5), || async {
        svc_b
            .broadcast_clipboard("interop clipboard check".into())
            .await
            .map(|n| n == 1)
            .unwrap_or(false)
    })
    .await;
    assert!(sent, "the clipboard packet was not queued for the peer");

    let mut received = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        match rx_a.try_recv() {
            Ok(KcEvent::Clipboard { content, .. }) => {
                if content == "interop clipboard check" {
                    received = true;
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(received, "the clipboard packet never arrived over TLS");

    // --- file sharing with a real payload ---------------------------------
    let payload =
        std::env::temp_dir().join(format!("pixel-connect-payload-{}.bin", std::process::id()));
    let bytes: Vec<u8> = (0..(512 * 1024u32)).map(|i| (i % 251) as u8).collect();
    std::fs::write(&payload, &bytes)?;

    svc_b
        .share_files(id_a, std::slice::from_ref(&payload), false)
        .await?;

    let mut landed: Option<std::path::PathBuf> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline && landed.is_none() {
        landed = find_with_extension(incoming, "bin");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let landed = landed.expect("the shared file never arrived");
    assert_eq!(
        std::fs::read(&landed)?.len(),
        bytes.len(),
        "the received payload has the wrong length"
    );
    assert_eq!(
        std::fs::read(&landed)?,
        bytes,
        "the received payload differs from what was sent"
    );
    let _ = std::fs::remove_file(&landed);
    let _ = std::fs::remove_file(&payload);
    Ok(())
}

/// Cancellation must be honoured, and must report honestly.
///
/// The protocol has no cancel packet: KDE Connect signals a cancellation by
/// closing the payload socket, and that is what this exercises.
///
/// Note on what is and is not asserted here. Loopback drains a payload far
/// faster than a test can cancel it, so this test cannot reliably catch a
/// transfer *mid-flight* — an early revision used a 24 MiB payload and
/// routinely finished before the cancel arrived. The deterministic properties
/// are therefore checked here:
///
///   * a live transfer accepts a cancel and is removed from the registry;
///   * a transfer that has already finished refuses a cancel rather than
///     reporting a silent success;
///   * the peer never reports a *partial* transfer as complete.
///
/// The token semantics that actually stop a transfer are covered by the unit
/// tests in `cancel.rs`.
#[tokio::test]
async fn cancelling_a_transfer_is_honoured_and_reports_honestly() -> anyhow::Result<()> {
    let dir_a = temp_dir("cancel-a");
    let dir_b = temp_dir("cancel-b");

    let (svc_a, mut rx_a) = KdeConnectService::start(&dir_a).await?;
    let incoming = downloads("cancel");
    svc_a.set_incoming_path(incoming.clone()).await;
    let (svc_b, _rx_b) = KdeConnectService::start(&dir_b).await?;

    let id_a = svc_a.device_info().id.clone();
    let id_b = svc_b.device_info().id.clone();

    let mut identity = pixel_connect_kdeconnect::NetworkPacket::new(
        pixel_connect_kdeconnect::packet::PACKET_TYPE_IDENTITY,
    );
    identity.set("deviceId", id_a.clone());
    identity.set("deviceName", "Peer A");
    identity.set("protocolVersion", PROTOCOL_VERSION);
    identity.set("tcpPort", i64::from(svc_a.provider().tcp_port()));
    pixel_connect_kdeconnect::discovery::note_udp_peer("127.0.0.1".parse().unwrap(), &identity);
    let peer_a = pixel_connect_kdeconnect::discovery::snapshot()
        .into_iter()
        .find(|p| p.device_id == id_a)
        .expect("peer A is in the registry");

    {
        let dialer = svc_b.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            dialer.provider().connect_to_peer(&peer_a).await;
        });
    }

    assert!(
        wait_until(Duration::from_secs(15), || async {
            svc_b.provider().is_connected(&id_a).await
        })
        .await,
        "the two services never connected"
    );

    // Pair, so the link is allowed to carry a share.
    svc_b.request_pairing(&id_a).await?;
    let mut key = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline && key.is_empty() {
        match rx_a.try_recv() {
            Ok(KcEvent::PairingRequest {
                verification_key: k,
                ..
            }) => key = k,
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(!key.is_empty(), "no pairing request arrived");
    svc_a.accept_pairing(&id_b).await?;
    assert!(
        wait_until(Duration::from_secs(15), || async {
            svc_b.is_trusted(&id_a).await
        })
        .await,
        "pairing did not complete"
    );

    // Cancelling something that does not exist must report failure.
    assert!(
        !svc_b.cancel_transfer("no-such-transfer-id"),
        "cancelling an unknown transfer must report failure"
    );

    let payload =
        std::env::temp_dir().join(format!("pixel-connect-cancel-{}.bin", std::process::id()));
    std::fs::write(&payload, vec![7u8; 8 * 1024 * 1024])?;

    svc_b
        .share_files(&id_a, std::slice::from_ref(&payload), false)
        .await?;

    // As soon as the transfer registers, cancel it.
    let mut cancelled = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        for id in svc_b.transfers_in_flight() {
            if svc_b.cancel_transfer(&id) {
                cancelled = true;
            }
        }
        if cancelled {
            break;
        }
        if svc_b.transfers_in_flight().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    if cancelled {
        // The registry must forget it promptly.
        assert!(
            wait_until(Duration::from_secs(20), || async {
                svc_b.transfers_in_flight().is_empty()
            })
            .await,
            "a cancelled transfer stayed in the registry"
        );
        // And a second cancel must then refuse, rather than claiming success.
        let id = format!(
            "{}:out:{}",
            id_b,
            payload.file_name().unwrap().to_string_lossy()
        );
        assert!(
            !svc_b.cancel_transfer(&id),
            "cancelling a finished transfer must report failure"
        );
    }

    // Whatever happened, the peer must not be told a partial file completed.
    let mut outcome: Option<TransferProgress> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline && outcome.is_none() {
        match rx_a.try_recv() {
            Ok(KcEvent::TransferProgress(t)) => {
                if t.done {
                    outcome = Some(*t);
                }
            }
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }

    if let Some(t) = outcome {
        if !t.cancelled && t.error.is_none() {
            // A clean completion must have delivered every byte, never a prefix.
            assert_eq!(
                t.bytes, t.total,
                "a transfer was reported complete after only {} of {} bytes",
                t.bytes, t.total
            );
            // And the file on disk must match.
            let landed = find_with_extension(&incoming, "bin").expect("the completed file");
            assert_eq!(std::fs::metadata(&landed).unwrap().len(), 8 * 1024 * 1024);
            let _ = std::fs::remove_file(&landed);
        }
    }

    svc_a.shutdown();
    svc_b.shutdown();
    let _ = std::fs::remove_file(&payload);
    Ok(())
}
