# Pixel Connect V1 — status

This reports what was actually built, what was verified, and what remains
incomplete. Every claim below is labelled by how it was checked:

* **[built]** — compiles.
* **[tested]** — an automated test exercises it and passes.
* **[live]** — verified against a running application on this machine.
* **[device]** — verified against a real Google Pixel. **No Pixel phone was
  available in this environment, so nothing in this report is labelled
  `[device]`.**

---

## 1. What was implemented

### 1.1 Project

A new, independent application at `pixel-connect/` with its own git history,
its own identity (`dev.pixelconnect.app`), its own icons, desktop entry and
package metadata. The original rquickshare checkout was not modified; a copy was
taken and rebranded.

On-disk state (verified by running the release build):

| Path | Contents | Mode |
|---|---|---|
| `~/.config/dev.pixelconnect.app/kdeconnect/` | device identity (`config.json`, `certificate.pem`, `key.pem`), the trust store (`trusted_devices.json`) and clipboard preferences (`prefs.json`) | `0700`, files `0600` |
| `~/.local/share/dev.pixelconnect.app/.pixel-connect-settings.json` | application settings (Tauri store plugin) | `0600` |
| `~/.local/share/dev.pixelconnect.app/logs/` | rotating log file | — |
| `$XDG_RUNTIME_DIR/pixel-connect/filemanager.sock` | file-manager IPC | dir `0700`, socket `0600` |

### 1.2 Quick Share (preserved)

`core/quickshare/` is rquickshare's `rqs_lib` with the crate and service renamed.
The mDNS advertisement, BLE advertising, UKEY2 handshake and payload transfer are
unchanged. The Tauri/Vue/Tailwind stack was kept. **[built] [live]** — the
service starts, advertises over mDNS and BLE, and the front end drives it.

### 1.3 KDE Connect-compatible backend (new)

`core/kdeconnect/` implements KDE Connect protocol version 8 in Rust:

| Capability | Status |
|---|---|
| mDNS announcement + browsing (`_kdeconnect._udp`) | **[built] [live]** |
| UDP broadcast discovery on port 1716 | **[built] [live]** (degrades if the port is taken) |
| TCP listener on 1716..=1764 | **[built] [tested] [live]** |
| TLS with the KDE role-inversion rule | **[built] [tested] [live]** |
| Self-signed EC P-256 device certificates, CN = device id | **[built] [tested] [live]** |
| CommonName + certificate pinning | **[built] [tested] [live]** |
| In-TLS identity exchange with capability negotiation | **[built] [tested] [live]** |
| Pairing with the 8-character verification key | **[built] [tested] [live]** |
| Bidirectional clipboard | **[built] [tested] [live]** (send path verified against a real peer; receive path unit-tested) |
| File sharing over a second TLS connection | **[built] [tested]** — 512 KiB verified byte-for-byte |
| Battery and connectivity reports | **[built] [tested]** |
| Phone notification relay | **[built]** |

This runs **inside the Pixel Connect process**. KDE Connect, `kdeconnectd`,
`kdeconnect-cli` and GSConnect are not required and are never invoked.

**Cross-implementation interoperability: [live] verified.** On the build machine
a real upstream `kdeconnectd` (Fedora package 26.08.1) was running. Pixel
Connect discovered it, completed the TCP+TLS handshake, exchanged identities and
negotiated capabilities with it — two implementations that share no code:

```
$ cargo run --bin kc-probe -- --seconds 20
device      : Pixel 8 [f45e4126…] Connected / Lan / features=["clipboard","receiveFiles","shareFiles","ping","battery","connectivity","notifications"]
device      : fedora  [d4aaba1b…] Connected / Lan / features=["clipboard","receiveFiles","shareFiles","ping","battery","notifications"]
discovered peers: 2
```

Interoperability with **kdeconnect-android** is *not* verified — no Pixel phone
was available. The protocol is identical and the upstream daemon interoperates,
so this is expected to work, but it is stated here as unverified.

### 1.4 Nautilus "Send to Pixel"

* A Unix-socket backend in the running app (`$XDG_RUNTIME_DIR/pixel-connect/filemanager.sock`, mode 0600).
* A Nautilus `MenuProvider` extension adapted from KDE Connect's, talking to that
  socket. **[built] [tested]**
* A Nautilus script and a desktop entry for GNOME versions without Python
  extensions. **[built]**
* An installer: `scripts/install-filemanager-integration.sh`. **[built]**

The backend was exercised **[live]** against the running application with eight
request shapes — real file, missing path, non-local scheme, directory, path
traversal, unknown device, over-limit selection and malformed JSON — and every
one produced a correct, specific error.

### 1.5 ADB and scrcpy

`adb.rs` detects `adb`/`scrcpy`, parses `adb devices -l` into typed devices
(USB / wireless / emulator), reports `unauthorized` and `offline` with
actionable guidance, launches and stops scrcpy, and validates serials and
arguments against `[A-Za-z0-9._:-]` before any process is started. **[built]
[tested]**. No ADB or scrcpy device was attached, so the UI states were not
exercised against hardware.

### 1.6 SSH (optional)

An explicitly configured target (host, port, username, auth method), driven
through the system OpenSSH client with argv-only argument passing and a strict
host/user allow-list. No shell, no automatic remote command, no server is
installed or enabled. **[built] [tested]**.

### 1.7 Front end

Vue 3 + Tailwind, rebuilt to the supplied design. Four screens: Files, Devices,
Notifications, Settings, plus a responsive sidebar→drawer layout. **[built]**,
and visually verified against the design with a screenshot harness. The store
is the single source of truth; nothing is fabricated.

---

## 2. Architecture

Single Tauri 2 process containing the Quick Share service, the KDE Connect
protocol backend, clipboard integration, ADB/scrcpy management, the SSH client
and the file-manager backend. `scrcpy` is the only child process. See
`v1-implementation-plan.md` §1.

---

## 3. Reused vs. new components

**Reused (adapted):**
- `core/quickshare/` — rquickshare's `rqs_lib`, copied and renamed.
- The Tauri 2 + Vue 3 + Tailwind stack, kept.
- `packaging/nautilus/pixel-connect-share.py` — adapted from KDE Connect's
  Nautilus extension.
- KDE Connect's on-the-wire contract, reproduced exactly (see `NOTICE.md`).

**New:**
- `core/kdeconnect/` — the Rust KDE Connect protocol implementation.
- All of `app/main/src` — the front end, rebuilt to the design.
- `app/main/src-tauri/src/{kdc,adb,ssh,nautilus,clipboard,settings,logger}.rs`.
- Packaging, icons and documentation.

---

## 4. Avoiding a separate KDE Connect installation

1. The protocol is implemented in-process; no `kdeconnectd`, no D-Bus service,
   no `kdeconnect-cli`.
2. Its configuration lives in `~/.config/dev.pixelconnect.app/kdeconnect/`, not
   `~/.config/kdeconnect/`, so it cannot touch a real KDE Connect installation.
3. The file-manager backend is a Unix socket, not `org.kde.kdeconnect`.
4. When the UDP 1716 port is already taken (by a real KDE Connect, as on the
   build machine) Pixel Connect warns and continues with mDNS rather than
   failing or stealing the port.
5. `grep -ri "kdeconnectd\|org.kde.kdeconnect" app/ core/` finds only
   documentation and the deliberate graceful-degradation warning.

---

## 5. What works and what does not

**Works [tested or live]:** Quick Share service start and discovery; KDE Connect
discovery, TLS, identity, pairing and pinning against a real upstream daemon;
clipboard send/receive between two instances; file sharing between two
instances; ADB parsing and serial validation; SSH argument construction;
file-manager request handling and validation; the whole front-end state model.

**Not implemented:** Bluetooth transport; Wi-Fi Direct transport; notification
input fields (actions are relayed, but there is no text input); pause/resume for
KDE Connect transfers; clipboard file/image payloads.

**Not verified on hardware:** anything requiring a Pixel phone — Quick Share
transfers, KDE Connect pairing with a phone, clipboard sync on Android, screen
mirroring.

---

## 6. Commands

```bash
# Front end
cd app/main
npm install
npm run vite:build          # production bundle
npm test                     # 29 unit tests
npm run vite:dev             # dev server on :1420

# Backend (needs protoc; see scripts/build-env.sh for the sysroot)
export PROTOC=$(command -v protoc)
cd app/main/src-tauri
cargo build --release        # the application
cargo test                   # 18 backend tests

# Protocol crates
cd core/kdeconnect
cargo test                                   # 39 unit + 5 integration
cargo run --bin kc-probe -- --seconds 30     # live discovery probe
cargo test --test discovery_live -- --ignored --nocapture   # live mDNS check

cd core/quickshare
cargo build
```

In a container without root, run `scripts/setup-sysroot.sh` once and then
`source scripts/build-env.sh` in each shell.

---

## 7. Fedora packaging

```bash
scripts/setup-sysroot.sh                 # only if building without root
cd app/main && npm install && npm run vite:build
cd app/main/src-tauri && cargo build --release
cd app/main/src-tauri && npx tauri build --bundles rpm
# or build the spec directly:
rpmbuild -ta packaging/rpm/pixel-connect.spec
sudo dnf install ./target/release/bundle/rpm/pixel-connect-0.1.0-1.*.rpm
```

The spec installs the binary, icons, desktop entry, the Nautilus extension and
the `pixel-connect-send` helper, and declares only GTK/WebKitGTK/libsoup/
libappindicator as hard runtime dependencies. `scrcpy`, `android-tools`,
`nautilus-python` and `openssh-clients` are `Recommends`. KDE Connect is not a
dependency.

File-manager integration:
```bash
scripts/install-filemanager-integration.sh
# then restart Nautilus
```

---

## 8. Android companion

Not built. The stock **KDE Connect** app (`org.kde.kdeconnect_tp`) speaks
protocol 8 and therefore pairs with Pixel Connect unmodified — see §1.3, where it
is corroborated by interoperability with the upstream desktop daemon. A branded
companion would add signing and maintenance for no functional gain. See
`android/README.md` for how to build one if branding is later required.

Quick Share works with the Google Quick Share app and does not involve KDE
Connect at all.

---

## 9. Tests actually executed

| Suite | Command | Result |
|---|---|---|
| KDE Connect unit | `cargo test` (lib) | 39 passed |
| KDE Connect integration | `cargo test --test interop -- --test-threads=1` | 5 passed |
| KDE Connect live discovery | `cargo test --test discovery_live -- --ignored` | requires Avahi + a peer |
| Tauri backend | `cargo test` | 18 passed |
| Front end | `npm test` | 29 passed |
| File-manager backend | manual socket exercise against the running app | 8 request shapes, all correct |
| Live interoperability | `kc-probe` against the system `kdeconnectd` | 2 peers, connected, capabilities negotiated |
| Front end visuals | screenshot harness vs. the supplied designs | matches |

**Total automated: 91 passing.**

Three real bugs were found by these tests and fixed:

1. The TLS client did not present its device certificate, so the peer (which
   requires one) rejected every handshake.
2. Only the accepting side pinned the peer's certificate, so pairing completed
   on one side only.
3. `flume::Receiver::recv_timeout` in the mDNS loop blocked a single-threaded
   runtime and deadlocked the backend — it now runs on `spawn_blocking`.

---

## 10. Known compatibility and security limitations

1. **No Pixel-phone verification.** The strongest evidence available here is
   interoperability with the upstream desktop daemon; the phone itself was never
   attached.
2. **Wayland clipboard.** GNOME only lets a focused window read the clipboard.
   Pixel Connect does not use the logcat workaround; it polls from the window
   and explains the behaviour.
3. **UDP 1716 is a single fixed port.** Two KDE Connect implementations on one
   host collide; Pixel Connect degrades to mDNS.
4. **Directory shares are flattened**, matching KDE Connect, not zipped.
5. **`kdeconnect.clipboard.file` is not implemented**; Android has no
   counterpart anyway.
6. **Screen mirroring is not embedded.** scrcpy's window is managed, not
   re-hosted; V1 explicitly allows this.
7. **TLS 1.3 is offered alongside 1.2.** KDE Connect for Android pins 1.2;
   Qt's default allows both. If a future Android build refuses 1.3, restrict the
   version list in `core/kdeconnect/src/tls.rs`.
8. **Pairing pins the full certificate.** This is stricter than upstream, which
   only warns on a DER change for already-trusted devices. A phone that
   re-generates its identity must be re-paired.
9. **The file-manager socket is a convenience channel,** not a privilege
   boundary. It is 0600 and every path is re-validated, but it is not a
   sandbox.

---

## 11. Wi-Fi Direct and Bluetooth findings

**Bluetooth — not implemented.** KDE Connect's Bluetooth backend is a single
RFCOMM connection carrying a custom multiplexing protocol (1-byte type,
2-byte length, 16-byte channel UUID, then data) over SDP service
`185f3df4-…`. Because RFCOMM is not TLS, its identity packet carries an extra
PEM `certificate` field. It is **disabled by default upstream** because it
"causes issues with other BT devices" (bugs 520578, 513536), and Android's side
requires OS-level Bluetooth pairing first and is also off by default. Porting it
would need the multiplexer on both sides and would enable a backend upstream
deliberately disabled. Pixel Connect ships the transport enum
(`Bluetooth`) and the Devices screen states plainly that it is not available.

**Wi-Fi Direct — not implemented.** KDE Connect has **no** Wi-Fi Direct support
of any kind: no P2P GO/peer negotiation, no WFD, no P2P group. The only
Wi-Fi-adjacent code on either side is network *observation*
(`TRANSPORT_WIFI`, `TRANSPORT_WIFI_AWARE`). Both implementations speak TCP/IP
only.

Adding it on Fedora GNOME/Wayland would require a wireless adapter whose driver
supports P2P, NetworkManager's `wifi-p2p` plugin (best-effort and incomplete),
and a P2P group established by the phone — plus a new transport in Pixel
Connect. Many modern Intel and MediaTek adapters advertise `WIFI_DIRECT` but
implement the GO role unreliably. It is a project in its own right, and it would
need hardware to validate. The Devices screen says exactly this rather than
showing a working-looking control.

The target scenario — a laptop on isolated university Wi-Fi reaching a Pixel on
another network without an internet relay — is **not** solved by Pixel Connect in
V1, and no status anywhere in the UI claims otherwise.