# Pixel Connect

Pixel Connect is a single Linux application for a Google Pixel phone. It
combines:

* **Quick Share** — Google's Nearby Connections file and text sharing, using the
  protocol implementation derived from
  [rquickshare](https://github.com/Martichou/rquickshare).
* **Device integration** — discovery, pairing, bidirectional clipboard
  synchronisation and file sharing over the **KDE Connect protocol (version 8)**,
  implemented in process. KDE Connect, `kdeconnectd`, `kdeconnect-cli` and
  GSConnect are **not** required and are never invoked.
* **Screen mirroring** — ADB device detection and scrcpy lifecycle management.
* **"Send to Pixel"** — a single, unambiguous entry in the GNOME Files (Nautilus)
  context menu.
* **Optional SSH** — an explicitly configured SSH client for a phone you have
  already given an SSH server.

Everything works locally. There is no cloud service, no account, and no relay
server.

---

## Status

V1 is complete and documented. See:

* [`docs/architecture/repo-audit.md`](docs/architecture/repo-audit.md) — what was
  found in the upstream repositories, and why the KDE Connect protocol was ported
  rather than bundled.
* [`docs/architecture/v1-implementation-plan.md`](docs/architecture/v1-implementation-plan.md)
  — the chosen architecture and the reasoning behind each decision.
* [`docs/architecture/v1-status.md`](docs/architecture/v1-status.md) — what is
  implemented, what was verified and how, and what remains incomplete.

**Read the status document before relying on anything here.** In particular,
no Pixel phone was attached during development: interoperability with
`kdeconnect-android` is therefore unverified, although interoperability with
the upstream KDE Connect desktop daemon *is* verified.

---

## Building

### Dependencies

On Fedora Workstation:

```bash
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel \
                 libappindicator-gtk3-devel openssl-devel patchelf \
                 protobuf-compiler nodejs npm
```

Optional at runtime (the application detects their absence and explains it):
`scrcpy`, `android-tools`, `nautilus-python`, `python3-nautilus`,
`openssh-clients`.

### Build

```bash
# 1. Front end
cd app/main
npm install
npm run vite:build

# 2. Application
cd src-tauri
cargo build --release

# 3. Run it
./target/release/pixel-connect
```

For an RPM:

```bash
cd app/main/src-tauri
npx tauri build --bundles rpm
sudo dnf install ./target/release/bundle/rpm/pixel-connect-*.rpm
```

or build the spec directly with `rpmbuild -ta packaging/rpm/pixel-connect.spec`.

### Building without root (containers)

The Tauri build needs `-devel` packages. If they cannot be installed
system-wide:

```bash
scripts/setup-sysroot.sh    # downloads and unpacks them into a sysroot
source scripts/build-env.sh # exports PKG_CONFIG_PATH / LD_LIBRARY_PATH
```

`scripts/setup-sysroot.sh` needs `dnf download`; `protoc` can be obtained the
same way, or from the protobuf releases.

---

## Tests

```bash
cd core/kdeconnect && cargo test -- --test-threads=1   # 39 unit + 5 integration
cd app/main/src-tauri && cargo test                   # 18 backend tests
cd app/main && npm test                               # 29 front-end tests
```

The KDE Connect integration tests start two real services and exercise TCP, TLS,
identity exchange, pairing, clipboard and a 512 KiB file transfer end to end.

Two developer tools:

```bash
cd core/kdeconnect
cargo run --bin kc-probe -- --seconds 30   # report what is discovered on the LAN
cargo test --test discovery_live -- --ignored --nocapture
```

`kc-probe` is the quickest way to check a build against a real phone or a real
`kdeconnectd`.

---

## File-manager integration

```bash
scripts/install-filemanager-integration.sh
```

This installs a `Send to Pixel` entry for GNOME Files. It works two ways so it
covers every GNOME version:

* a Nautilus `MenuProvider` (top-level context-menu entry) where
  `nautilus-python` is available;
* a Nautilus script (appears under **Scripts**) everywhere.

Restart Nautilus afterwards. Uninstall by deleting the files the script lists.

---

## Where it stores things

| Path | Contents |
|---|---|
| `~/.config/dev.pixelconnect.app/kdeconnect/` | device identity and the paired-device trust store (mode `0700`, files `0600`) |
| `~/.local/share/dev.pixelconnect.app/.pixel-connect-settings.json` | application settings |
| `~/.local/share/dev.pixelconnect.app/logs/` | rotating log file |
| `$XDG_RUNTIME_DIR/pixel-connect/filemanager.sock` | "Send to Pixel" IPC (dir `0700`, socket `0600`) |

`PIXEL_CONNECT_LOG=debug` raises the log level for one run.

Pixel Connect never reads or writes KDE Connect's or rquickshare's
configuration.

---

## Project layout

```
core/quickshare/    Google Quick Share protocol (derived from rquickshare)
core/kdeconnect/    KDE Connect protocol v8 (Rust)
app/main/src/       Vue 3 front end
app/main/src-tauri/ Tauri backend (Rust)
android/            notes on an optional branded companion
packaging/          RPM spec, desktop entry, Nautilus extension
data/branding/      icons
scripts/            build, sysroot and install helpers
docs/architecture/  audit, plan, status
```

---

## Licensing and attribution

Pixel Connect is **AGPL-3.0**. It contains a derivative work of rquickshare
(AGPL-3.0) and implements the KDE Connect wire protocol, whose projects are
GPL-2.0-only OR GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL.

Full details, including every upstream file and author, are in
[`NOTICE.md`](NOTICE.md). Licence texts are in `licenses/`.