// Pixel Connect — live discovery test against the machine's mDNS.
//
// This test is *not* part of the normal `cargo test` run: it needs a running
// Avahi daemon and it looks for real KDE Connect instances on the network. It
// exists so that interoperability with an upstream daemon can be verified on a
// developer machine.
//
//   cargo test --test discovery_live -- --ignored --nocapture
//
// It asserts only what can be proven without a phone: that Pixel Connect's
// mDNS browser resolves KDE Connect services on the LAN.

#[tokio::test]
#[ignore = "requires a running Avahi daemon and other KDE Connect devices"]
async fn resolves_kde_connect_services_on_the_network() -> anyhow::Result<()> {
    use pixel_connect_kdeconnect::discovery;

    // Force the shared registry to be observed over a window long enough for
    // Avahi to answer and for this process's browser to see the reply.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut found = Vec::new();
    while std::time::Instant::now() < deadline && found.is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        found = discovery::snapshot()
            .into_iter()
            // Ignore this machine's own instance.
            .filter(|p| !p.device_id.is_empty())
            .collect();
    }

    for p in &found {
        println!(
            "resolved {} ({} v{}, {}:{}) via {}",
            p.name, p.device_id, p.protocol_version, p.address, p.tcp_port, p.source
        );
    }

    assert!(
        !found.is_empty(),
        "no KDE Connect services were resolved; is Avahi running?"
    );
    // A resolved peer must carry a usable device id and a port in the range the
    // protocol allows.
    for p in &found {
        assert!(
            pixel_connect_kdeconnect::deviceinfo::is_valid_device_id(&p.device_id),
            "{} has an invalid device id",
            p.device_id
        );
        assert!((1716..=1764).contains(&p.tcp_port), "{} advertises port {}", p.device_id, p.tcp_port);
    }
    Ok(())
}