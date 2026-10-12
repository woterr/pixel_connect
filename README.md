# Pixel Connect

One Linux app for a Google Pixel phone. It combines two protocols that are
usually two separate apps:

- **Quick Share** — Google's Nearby Connections sharing (files and text, both
  directions, over Wi-Fi LAN and over Bluetooth with automatic upgrade to the
  faster path).
- **KDE Connect (protocol v8)** — pairing, clipboard sync, notifications, file
  sharing and plugins, implemented in-process. No `kdeconnectd`, no GSConnect,
  nothing else to install.

Plus: screen mirroring (ADB + scrcpy management), a "Send to Pixel" entry in
the GNOME Files context menu, and optional SSH.

Everything works locally. No cloud, no account, no relay.

## How it is different

Most setups give you Quick Share *or* KDE Connect, each with its own window,
its own device list and its own quirks. Pixel Connect runs **both protocol
engines in one backend behind one UI**:

| | Quick Share engine | KDE Connect engine |
|---|---|---|
| Source | `open-quickshare` (`rqs_lib` @ `5a31145`, GPL-3.0-or-later, includes the `rquickshare-ble` BLE work) | Written for this project, KDE-compatible |
| Does | Nearby sharing, BLE discovery, L2CAP fast-connect, transport upgrades | Pairing, clipboard, notifications, shares, plugins |
| Independent? | Yes — separate modules, separate state, separate Bluetooth/network ownership | Yes |

The UI never mixes them up: each screen says which protocol it is using, and
anything not implemented is labelled as not implemented rather than faked.

## What works right now

Verified against a real Pixel 8 (no mocks, no emulators):

- Quick Share phone → laptop over LAN and over Bluetooth — **PASS** (Bluetooth is fast)
- Quick Share laptop → phone — **PASS**
- KDE Connect ping, clipboard both directions, file send both ways — **PASS**
- Bluetooth file transfer (~395 KB, verified byte-complete) — **PASS**
- Text shares arrive on the desktop clipboard automatically

Not yet verified:

- **Wi-Fi Direct upgrade timing** — the engine supports it; the measurement
  against the phone is still to come.
- Large-file Bluetooth throughput is slow (~6 KB/s fallback); the upgrade
  path is what makes big files practical.

## Requirements

- Fedora Linux (GNOME, Wayland) — other Linux desktops should work but are untested.
- Bluetooth adapter with BlueZ 5.x for the BLE paths.
- A Pixel / Android phone with Quick Share and the KDE Connect app.
- Build tools: Rust (1.86+), Node 20+, `protoc` 25.x, system webkit/GTK dev
  packages (see `scripts/setup-sysroot.sh`).

## Build and run

```bash
# one-time: build sysroot + fetch protoc (both live outside the repo)
scripts/setup-sysroot.sh

# backend + frontend tests
source scripts/build-env.sh
export PROTOC=/tmp/opencode/protoc/bin/protoc
cargo test --release -p pixel_connect_quickshare
cargo test --release -p pixel_connect_kdeconnect
(cd app/main && npm test)

# release build, install for the current user, and run
(cd app/main/src-tauri && cargo build --release)
./scripts/install-user.sh --no-build
~/.local/bin/pixel-connect
```

`PIXEL_CONNECT_LOG=trace ~/.local/bin/pixel-connect` for full protocol logs.
`./scripts/ble-debug.sh` captures a Quick Share attempt into a timestamped
directory with a verdict (no root needed).

## Layout

```
pixel-connect/
  app/main/            Tauri frontend (Vue 3) + backend glue (src-tauri/)
  core/quickshare/     Quick Share engine (open-quickshare based)
  core/kdeconnect/     KDE Connect engine (project-written)
  docs/architecture/   audits, migration map, transport findings
  docs/diagnosis/      root-cause write-ups with log evidence
  docs/testing/        verification reports (what passed, what didn't)
  scripts/             setup, install, ble-debug, visual-check
```

Docs worth reading first: `docs/architecture/quickshare-migration.md`
(engine swap map), `docs/diagnosis/connection-state-root-cause.md`,
`docs/testing/ble-quick-share-verification.md`.

## License

AGPL-3.0-or-later (see `LICENSE`). Third-party attributions in `NOTICE.md`.
