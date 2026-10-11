# Test environment report

Generated 2026-10-11. Every later classification refers to this environment.

## Machine

| | |
|---|---|
| Host OS | Fedora Linux 44 (Workstation) |
| Kernel | 6.x, x86_64 |
| Desktop | GNOME on Wayland, no `ext-data-control-v1` or `wlr-data-control-v1` |
| Architectures | `x86_64` primary; `i686` packages present in the build sysroot |

The compositor does not expose a screen-capture protocol, so the UI cannot be
screenshotted. UI verification is by log, file descriptor and process
inspection only. This is a real limitation on evidence, not an oversight.

## Networks used

| Network | Laptop | Phone | Path | Result |
|---|---|---|---|---|
| Home WLAN | `10.141.5.105/24` on `wlo1` | `192.168.29.5` | via gateway `10.141.5.77` | NAT; reversed payload connections blocked |
| Phone hotspot (`Pixel8`) | `10.141.5.105/24` | `10.141.5.77` | direct, same subnet, RSSI −27 dBm, 2.4 GHz | Works |

A VPN interface (`tailscale0`, `100.114.95.4`) is present during testing and
affects address selection. That turned out to matter — see
[`failure-analysis.md`](failure-analysis.md).

## Phone

| | |
|---|---|
| Device | Google Pixel 8 |
| Android | 17 (SDK 37) |
| KDE Connect | `org.kde.kdeconnect_tp` 1.35.17 |

The phone is the **stock** KDE Connect app. No custom APK was built or
installed.

## Build environment

Building requires a sysroot outside the repository:

```bash
./scripts/setup-sysroot.sh     # populates /tmp/opencode/sysroot
source scripts/build-env.sh    # exports PKG_CONFIG_PATH, LD_LIBRARY_PATH, ...
export PROTOC=/tmp/opencode/protoc/bin/protoc
cd app/main/src-tauri && cargo build --release
```

`/tmp/opencode` is scratch space and was cleared twice by server restarts,
destroying the sysroot both times. `setup-sysroot.sh` rebuilds it; `protoc` must
be re-fetched separately. Both are prerequisites, not optional.

## Firewall

`firewalld` active, interface `wlo1` in zone `FedoraWorkstation`, which permits
`1025-65535/tcp` and `1025-65535/udp`. Not a constraint on any test.

## Trust state

| Store | Contents |
|---|---|
| `~/.config/dev.pixelconnect.app/kdeconnect/trusted_devices.json` | `{}` — empty |
| `~/.config/kdeconnect/` | two device directories, owned by stock KDE Connect |

The Pixel is paired with **stock KDE Connect**, not with Pixel Connect. Each
implementation keeps its own trust store, so Pixel Connect correctly reports the
device as untrusted. Transfers still work because the *phone* tolerates the
link; there is no certificate pinned on this side.

This is the single most important caveat in this report: **every KDE Connect
result below was obtained against a peer that is not pinned by this
application.**

## Test execution

```
core/kdeconnect       81 unit +  6 integration +  7 notifications
core/quickshare       13 unit
app/main/src-tauri    44 unit
app/main              29 front end (vitest)
                     ---
                     180 tests, 180 passing
```

One test is `#[ignore]`d: `discovery_live.rs` requires a second real device on
the LAN and is not part of the automated run.

## Hardware verification performed

Confirmed by the user, by log inspection, or by direct socket test:

- Discovery and pairing over KDE Connect
- File transfer **from** the PC, on home WLAN and on hotspot
- Quick Share **text** from phone to PC, delivered to the clipboard
- Notifications (user-confirmed: "the notifications work")
- Clipboard sync
- USB and wireless scrcpy mirroring
- Nautilus "Send to Pixel" through the real socket
- Application launches from the GNOME app grid via `gtk-launch`

## Hardware verification NOT performed

- Bluetooth transport
- Wi-Fi Direct transport
- File transfer **to** the PC followed by the image-to-clipboard path
- Pairing **with Pixel Connect itself**, which would populate the trust store
- Long-idle soak on a hotspot, which is what the keepalive fix needs