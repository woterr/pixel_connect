// Pixel Connect — live interoperability harness.
//
// A small binary that starts a `KdeConnectService` and prints what it
// discovers and connects to. It is a development tool for checking a
// Pixel Connect build against a real phone or against an upstream
// kdeconnectd; it is not part of the application.
//
//   cargo run --bin kc-probe -- --seconds 30
//
// Exit status is 0 when at least one peer was discovered.

use std::str::FromStr;
use std::time::{Duration, Instant};

use pixel_connect_kdeconnect::{KcEvent, KdeConnectService};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The probe is a diagnostic tool, so it installs a simple logger that honours
    // RUST_LOG and defaults to `info` so mDNS activity is visible.
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let _ = fern::Dispatch::new()
        .format(|out, message, record| out.finish(format_args!("[{}] {}", record.level(), message)))
        .level(log::LevelFilter::from_str(&level).unwrap_or(log::LevelFilter::Info))
        .chain(std::io::stdout())
        .apply();

    let mut seconds: u64 = 30;
    let mut dir: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seconds" => seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(seconds),
            "--dir" => dir = args.next(),
            "--help" | "-h" => {
                eprintln!("usage: kc-probe [--seconds N] [--dir PATH]");
                return Ok(());
            }
            other => eprintln!("kc-probe: ignoring unknown argument {other:?}"),
        }
    }

    let dir =
        std::path::PathBuf::from(dir.unwrap_or_else(|| "/tmp/pixel-connect-kc-probe".to_string()));
    std::fs::create_dir_all(&dir)?;

    let (service, mut events) = KdeConnectService::start(&dir).await?;
    println!("device id   : {}", service.device_info().id);
    println!("device name : {}", service.device_info().name);
    println!("protocol    : v{}", service.device_info().protocol_version);
    println!("config      : {}", dir.display());
    println!("listening   : TCP port {}", service.provider().tcp_port());
    println!();

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut refresh = tokio::time::interval(Duration::from_secs(3));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline.into()) => break,
            _ = refresh.tick() => service.refresh().await,
            event = events.recv() => match event {
                Ok(KcEvent::Device(d)) => {
                    let me = &service.device_info().id;
                    if d.id != *me {
                        println!(
                            "device      : {} [{}] {:?} / {:?} / features={:?}",
                            d.name, d.id, d.link_state, d.transport, d.features
                        );
                    }
                }
                Ok(KcEvent::PairingRequest { device, verification_key }) => {
                    println!("pairing     : {} asked to pair, code {}", device.name, verification_key);
                }
                Ok(KcEvent::Clipboard { device_id: _, device_name, content }) => {
                    // Deliberately does not print the content.
                    println!("clipboard   : {} bytes received from {}", content.len(), device_name);
                }
                Ok(KcEvent::ShareOffer(o)) => {
                    println!("share       : {} offered {:?}", o.device_name, o.kind);
                }
                Ok(KcEvent::TransferProgress(t)) => {
                    if t.done {
                        println!(
                            "transfer    : {} {} ({}/{}){}",
                            t.filename,
                            if t.error.is_some() { "failed" } else { "complete" },
                            t.bytes,
                            t.total,
                            t.error.clone().unwrap_or_default()
                        );
                    }
                }
                Ok(KcEvent::Log { level, message }) => println!("{level:<11}: {message}"),
                Ok(_) => {}
                Err(_) => break,
            },
        }
    }

    let devices = service.devices().await;
    let me = service.device_info().id;
    let discovered = devices.iter().filter(|d| d.id != me).count();

    println!();
    println!("discovered peers: {discovered}");
    for d in &devices {
        if d.id == service.device_info().id {
            continue;
        }
        println!(
            "  {} — {} ({:?}, {:?})",
            d.name, d.id, d.link_state, d.transport
        );
    }

    service.shutdown();
    if discovered == 0 {
        std::process::exit(2);
    }
    Ok(())
}
