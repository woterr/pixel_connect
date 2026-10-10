# V1 implementation plan and architecture

This records the architecture that was chosen after the audit, the milestones
as they were executed, and the reasoning behind each significant decision.
`repo-audit.md` contains the evidence; this document contains the conclusions.

---

## 1. Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│  Pixel Connect (one process, Tauri 2)                                  │
│                                                                        │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │ Front end — Vue 3 + Tailwind (app/main/src)                      │  │
│  │  Files · Devices · Notifications · Settings                       │  │
│  │  Talks only through app/main/src/api.ts (typed invoke wrappers)   │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│         ▲ events (kdeconnect:*, quickshare:*)      │ commands          │
│  ┌──────┴───────────────────────────────────────────────────────────┐ │
│  │ Backend (app/main/src-tauri/src)                                 │ │
│  │                                                                  │ │
│  │  quickshare.rs ── pixel_connect_quickshare  (from rquickshare)   │ │
│  │  kdc.rs        ── pixel_connect_kdeconnect  (KDE Connect v8)      │ │
│  │  clipboard.rs  ── arboard (X11) / wl-clipboard-rs (Wayland)      │ │
│  │  adb.rs        ── `adb devices -l`, `scrcpy` process lifecycle  │ │
│  │  ssh.rs        ── system OpenSSH client, argv only               │ │
│  │  nautilus.rs   ── Unix socket for the file-manager extension    │ │
│  │  settings.rs   ── ~/.local/share/dev.pixelconnect.app/            │ │
│  └──────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────┘
                    │ UDP 1716 + mDNS _kdeconnect._udp
                    │ TCP 1716..=1764, TLS 1.2/1.3
                    │ Quick Share mDNS + BLE + TCP
                    ▼
               Google Pixel (Android)
               Quick Share app  and/or  KDE Connect
```

### Why one process

The brief allows "a bundled C++ helper process", but nothing in V1 needs one.
Porting the KDE Connect LAN backend removed the only component that would have
required a helper. One process means:

* no supervision, no packaging of a second daemon, no ordering between start-up
  and the UI;
* a single tray icon, a single quit path, and no orphaned children;
* `scrcpy` is the only child process, and its lifetime is tracked explicitly.

### Repository layout

```
pixel-connect/
├── core/
│   ├── quickshare/        Google Quick Share protocol (from rquickshare)
│   └── kdeconnect/        KDE Connect protocol v8 (new, Rust)
├── app/main/
│   ├── src/               Vue 3 front end
│   └── src-tauri/         Rust backend
├── android/               Pixel Connect companion (optional, see §6)
├── data/branding/         Pixel Connect icons
├── packaging/
│   ├── rpm/               Fedora spec
│   ├── desktop/           .desktop entry
│   └── nautilus/          Nautilus extension + CLI helper
├── scripts/               build, sysroot and install helpers
├── docs/architecture/     this directory
└── licenses/              AGPL-3.0 and GPL-2.0 texts
```

---

## 2. Milestones

### Phase 1 — audit (done)

Read the four upstream repositories. Established the facts in
`repo-audit.md`. In particular that KDE Connect protocol 8 is newline-delimited
JSON over TLS with CommonName pinning, which is what made the port viable.

### Phase 2 — foundation (done)

* Created `pixel-connect/` as an independent project with its own git history,
  identity (`dev.pixelconnect.app`), icons, desktop entry and data directory.
* Copied `rqs_lib` to `core/quickshare`, renamed the crate and the service
  struct, preserved the upstream licence and added attribution.
* Kept rquickshare's Tauri/Vue/Tailwind stack rather than starting a new front
  end.

### Phase 3 — KDE Connect (done)

* `core/kdeconnect`: packet framing, device identity, TLS with pinning,
  pairing, the device link, and the clipboard/share/battery/connectivity
  plugins.
* `kdc.rs` in the Tauri backend translates events into UI payloads.
* Devices screen: identity, protocols, pairing state, connection state,
  features, transport, errors and recovery actions.

### Phase 4 — scrcpy, ADB and extra transports (done)

* `adb.rs`: detects `adb` and `scrcpy`, parses `adb devices -l` into typed
  devices with USB/wireless/emulator classification, launches and stops
  scrcpy, and turns `unauthorized`/`offline` states into actionable guidance.
* Bluetooth and Wi-Fi Direct investigated and documented as unavailable, with
  the reasoning shown in the UI.
* `ssh.rs`: an explicit, user-configured SSH target using the system OpenSSH
  client with argv-only argument passing.

### Phase 5 — file manager, UI and Android (done)

* `nautilus.rs` + `packaging/nautilus/`: a Unix-socket backend, a Nautilus
  `MenuProvider` extension adapted from KDE Connect's, a Nautilus script and a
  desktop entry.
* Front end rebuilt to the supplied design.
* Android: see §6.

### Phase 6 — test, package, document (done)

See `v1-status.md`.

---

## 3. Decisions and their reasoning

### 3.1 Port the KDE Connect protocol instead of bundling KDE Connect

Fully argued in `repo-audit.md` §5. Summary: KDE Connect's identity is bound to
the D-Bus name `org.kde.kdeconnect` and to `~/.config/kdeconnect`, so a renamed
copy is a fork; building it from source needs Qt 6.7 plus twelve KF6 frameworks
and would still require that re-branding; and the protocol itself is
self-contained JSON-over-TLS that ports cleanly.

**Cost accepted:** ~2400 lines of new protocol code that must be kept
interoperable. **Mitigation:** a wire-faithful port (every constant taken from
upstream) plus integration tests that run the real stack.

### 3.2 Make the transport an explicit, typed concept

`Transport::{Lan, Bluetooth, None}` is a real enum, and the UI derives what it
shows from the backend's `KcDevice`, which carries the peer capabilities. A
device that has not advertised `kdeconnect.clipboard` gets no clipboard
control. Nothing in the UI reports a capability the backend cannot deliver.

### 3.3 Clipboard: honest about Wayland

GNOME only lets a focused window read the clipboard. Pixel Connect does **not**
implement the logcat trick KDE Connect for Android uses. Instead:

* the backend reports `backgroundReadAllowed` per session;
* on X11 a background watcher polls and pushes changes;
* on Wayland the focused window polls and pushes changes, and the Settings
  screen explains the trade-off in the user's own terms.

Clipboard *contents* are never logged, never stored, and never included in
clipboard history — there is no clipboard history feature at all.

Loop prevention uses upstream's timestamp rule plus a local content hash, and
the "did I just apply this locally" check before pushing back.

### 3.4 Security posture

* TLS peer verification is a real verifier, not a disabled check. Because KDE
  Connect certificates are self-signed, the verifier enforces what actually
  matters: a parseable certificate, a valid validity window, and a CommonName to
  pin against. The CN comparison and full-DER pinning happen immediately after
  the handshake. This is *stronger* than upstream, which only warns on a
  mismatched DER for already-trusted devices.
* Both TLS roles require the peer to present a device certificate, so identity
  can be pinned in both directions. KDE Connect clients always send one.
* Pairing requires the user to compare an 8-character verification key that both
  devices compute from the same two public keys and the pairing timestamp.
* The file-manager socket lives in `$XDG_RUNTIME_DIR`, mode 0600, directory
  0700, and every path it receives is re-validated (exists, is local, is a
  regular file or directory) before use. Traversal segments are normalised.
* ADB serials and scrcpy arguments are allow-listed against
  `[A-Za-z0-9._:-]` before being passed to a process. No shell is ever used.
* SSH hosts and usernames are validated against the same allow-list before they
  reach an argv; the probe command is fixed and only runs on explicit user
  action.
* Pairing timestamps more than 30 minutes from now are rejected.

### 3.5 Graceful degradation over hard failure

* UDP 1716 busy → warn, continue with mDNS. (This is what happens on the build
  machine, where a real `kdeconnectd` holds the port.)
* `adb` missing → the Devices screen explains exactly what to install.
* A device in `unauthorized` → the screen says what to do on the phone.
* Python Nautilus extensions unavailable → the script entry is installed
  instead, which works on every GNOME version.
* Dropping the socket on restart is handled by removing a stale socket first.

### 3.6 The front end is the design

The supplied designs were treated as the acceptance reference. Recorded gaps and
the decisions made:

| Design element | Decision |
|---|---|
| Product name "Bridge" | Rebranded to **Pixel Connect** throughout, as instructed. Wordmark styling (bold italic, tight tracking) kept. |
| "LOCAL NETWORK" subtitle | Kept as the sidebar eyebrow. |
| Tailscale network card | Replaced by the sections V1 actually needs: Network identity, clipboard, mirroring/ADB/SSH, integrations, About. A Tailscale IP field with a non-functional toggle would violate "every visible control must perform a real action". |
| Dark mode switch | Present but **disabled with an "Always on in V1" chip**, rather than shown as working. |
| Transfer alerts switch | Same. |
| Pause/Cancel on an active transfer | Disabled with an explanatory chip: the KDE Connect protocol has no pause/resume, and pretending would be dishonest. |
| "Bridge v2.4.0-stable" | Replaced with the real version from `app_info`. |
| Bottom tab bar on narrow layouts | Implemented: below `lg` the sidebar becomes a drawer with a hamburger, and a Files/Settings bar sits at the bottom. |
| Signature wavy progress | Implemented as an SVG sine rule whose fill tracks the measured path length. |
| Notifications shade | Implemented as its own screen with the same eyebrow, headline and card structure. |

---

## 4. Interface between UI and backend

* `app/main/src/api.ts` is the only place that calls `invoke`. Every wrapper is
  typed against `app/main/src/types.ts`, which mirrors the Rust structs.
* `app/main/src/stores/app.ts` is the single owner of application state. No
  component holds a second copy of device state.
* Backend events (`kdeconnect:device`, `quickshare:event`, …) are subscribed in
  `App.vue` and folded into the store.
* Nothing in the front end fabricates state. Empty lists mean empty lists.

---

## 5. Testing strategy

| Layer | How it is tested |
|---|---|
| KDE Connect protocol | 39 unit tests (framing, identity validation, pairing, capability resolution, path sanitisation, PEM round-trip, config persistence) |
| KDE Connect integration | 5 tests running two full services over loopback TCP: discovery, TLS both roles, CN pinning, identity exchange, pairing, certificate pinning, clipboard, 512 KiB file transfer |
| Tauri backend | 18 tests: `adb devices -l` parsing and serial allow-listing, file-manager URI/path validation, SSH argument construction, settings defaults, clipboard capabilities |
| Front end | 29 tests: byte/percent/duration formatting and the store's derived state |
| End to end | The file-manager backend was exercised against the *running* application over its real socket with eight request shapes, including every error path |

---

## 6. Android companion — decision

The KDE Connect Android app (`org.kde.kdeconnect_tp`, F-Droid and Play) already
speaks protocol version 8, which is what `core/kdeconnect` implements. It
therefore needs **no** modification to pair with Pixel Connect, and no separate
companion is required for any V1 feature: discovery, pairing, clipboard,
file sharing, battery and connectivity all work with the stock app.

Quick Share is completely independent of it — it uses the Google app on the
phone, not KDE Connect.

A branded companion was therefore **not built**. `android/README.md` documents
how to build one if branding is later required, and the packaging steps are
there, but shipping a second APK would mean signing, updating and maintaining a
protocol client for no functional gain.

---

## 7. Known gaps carried into V1.1

1. No Bluetooth transport.
2. No Wi-Fi Direct transport.
3. No notification *reply* (the action is relayed, but there is no input field).
4. No pause/resume for KDE Connect transfers.
5. Clipboard file/image payloads (`kdeconnect.clipboard.file`) are not
   implemented; Android has no counterpart anyway.
6. Wayland clipboard sync needs the window to be focused.