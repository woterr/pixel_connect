# Pixel Connect — threat model

What Pixel Connect is protecting, who is trying to take it, and where the
boundaries are. Written against the actual code in this repository, not against
a design document.

Scope: the Linux application (`app/main`, `core/kdeconnect`, `core/quickshare`,
`packaging/`) as it exists on the `security-audit` branch. The Android side is
out of scope because there is no companion application — see
[`android/README.md`](../../android/README.md).

---

## 1. What the application is

A local-first desktop application that pairs with a user's own Google Pixel over
the local network and moves files, clipboard text and notifications between
them, mirrors the phone's screen via scrcpy, and offers a `Send to Pixel` action
in the file manager.

Two independent device-integration protocols run side by side and are
deliberately kept apart:

| | KDE Connect | Quick Share |
|---|---|---|
| Identity | self-signed EC P-256 certificate, `CN = deviceId`, pinned after pairing | vendor key-pair based, from rquickshare |
| Pairing | explicit, user-confirmed, with a verification code | implicit, on reaching Android |
| Trust store | `~/.config/dev.pixelconnect.app/kdeconnect/` | rquickshare's own state |
| Transport | TCP + TLS on 1716–1764, mDNS/UDP discovery | TCP + BLE advertisement |

**Pairing in one protocol never implies trust in the other.** A device trusted
for KDE Connect cannot receive Quick Share traffic without Quick Share's own
handshake, and vice versa. This separation is a deliberate security boundary,
not an implementation detail.

---

## 2. Assets

Ranked by what would actually hurt the user if it leaked or was tampered with.

| # | Asset | Where it lives | What leaks it |
|---|---|---|---|
| A1 | **Clipboard contents** | in memory, never persisted | a peer, a log, a crash dump |
| A2 | **Device identity private key** | `kdeconnect/key.pem`, mode 0600 | filesystem access, a backup, an error message |
| A3 | **Received and sent files** | user-chosen download directory | a peer writing outside it, a symlink |
| A4 | **Notification content** | in memory, rendered in the UI | logs, an over-broad read |
| A5 | **Paired-device trust list** | `kdeconnect/config.json`, mode 0600 | filesystem access |
| A6 | **Pairing verification code** | shown once in the UI | shoulder-surfing, a spoofed prompt |
| A7 | **Local IPC surface** | `$XDG_RUNTIME_DIR/pixel-connect/filemanager.sock`, mode 0600 | any same-user process |
| A8 | **Application integrity** | the binary, the RPM | supply chain, a hijacked `PATH` |
| A9 | **SSH credentials** | never stored; the agent and the user hold them | an argument-injection flaw, a log line |

A1 deserves particular attention: a clipboard routinely holds a password from a
password manager, a one-time code from an authenticator app, or a line from a
private message. It is the single highest-value target in the application.

---

## 3. Adversaries

### 3.1 In scope — Pixel Connect can meaningfully resist these

**A. Another device on the same Wi-Fi network.** The realistic primary
adversary. No authentication is required to send UDP discovery broadcasts or
call the mDNS responder, so any device on the LAN can learn the machine's
identity, its open TCP port, and its capabilities, and can attempt a TLS
connection. Threats: impersonation of a paired device, denial of service, disk
exhaustion, log forging, reading files outside the download directory.

**B. A hostile or compromised phone.** Once paired, the phone is trusted at the
protocol level — by design, and identically in upstream KDE Connect. But the
*phone* can still be compromised later: a malicious app granted notification
access, or a device whose KDE Connect install is replaced. Threats: oversized or
truncated transfers, abusive clipboard writes, notification content crafted to
attack the UI.

**C. An unpaired device, or a device whose pairing was revoked.** Threats:
reaching functionality that should require pairing, and forging packets that
look authenticated.

**D. A malformed or hostile protocol peer.** Anyone who can open a TCP
connection can send arbitrary bytes. Threats: parser crashes, unbounded memory
allocation, task exhaustion, and logic errors that skip authorisation checks.

**E. A compromised front end.** The Vue application renders strings that a
device controls: device names, notification titles and bodies, filenames. If
any of those reached an HTML injection sink, script running in the webview would
inherit the application's Tauri permissions. This is the classic Tauri threat.

### 3.2 Out of scope — operating system isolation, not application design

**F. Malicious code already running as this user.** Such code can read the
private key directly, read every file the user can read, and connect to the
application's own IPC socket. No application-level control helps. The mitigation
is the OS: separate accounts, sandboxing (Flatpak), and not running untrusted
software.

**G. A compromised kernel, X server or display server.** Out of reach.

**H. Physical access to an unlocked session.** Out of reach.

Finding F is not an excuse to ignore the same-user IPC socket, though: the
socket should still refuse anything unreasonable, because a *legitimate*
same-user program with a bug is a more likely event than a determined attacker.

---

## 4. Trust boundaries

```
   ┌──────────────────────────────────────────────────────────────────┐
   │  User's desktop session (GNOME/Wayland)                         │
   │                                                                  │
   │  ┌────────────────────┐        ┌──────────────────────────┐     │
   │  │ Webview (Vue)      │        │ adb / scrcpy / ssh       │     │
   │  │  UNTRUSTED for     │        │  child processes         │     │
   │  │  authorisation     │        └──────────────────────────┘     │
   │  └─────────┬──────────┘                                          │
   │            │ Tauri IPC — bounded by capabilities + CSP          │
   │  ══════════╪════════════════════ BOUNDARY 1 ════════════════    │
   │            ▼                                                     │
   │  ┌──────────────────────────────────────────────────────────┐   │
   │  │ Rust core  (commands, protocol, clipboard, storage)      │   │
   │  │  TRUSTED — enforces every authorisation decision         │   │
   │  └───┬──────────────────────────────────────────────┬───────┘   │
   │      │                                              │           │
   │  ════╪══════════════ BOUNDARY 2 ═══════════════   ╪════════   │
   │      ▼                                              ▼           │
   │  Unix socket, mode 0600                      TCP/TLS + mDNS    │
   │  (Nautilus extension)                        KDE Connect/QS     │
   │  TRUST: same user only                       TRUST: pinned cert │
   └──────────────────────────────────────────────────────────────────┘
                                  ║
                    ══════════════╪══════════════ BOUNDARY 3
                                  ▼
                       Any device on the LAN
```

**Boundary 1 — front end to Rust.** The webview is a presentation layer and is
*not* the security authority. The CSP is
`default-src 'self'; img-src 'self' asset: https://asset.localhost data:;
style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self'
ipc: http://ipc.localhost` — no remote script, no remote frames, no arbitrary
connect targets. Tauri capabilities grant a named set of commands to the `main`
window only. Every command re-validates its arguments in Rust regardless of what
the UI checked.

**Boundary 2 — Rust to the local system.** Process spawning uses
`std::process::Command` with explicit argument vectors. No shell is
interpolated anywhere. The Nautilus socket is the only long-lived local IPC
listener and it is mode 0600 inside a mode 0700 directory.

**Boundary 3 — the network.** Everything crossing this boundary is untrusted
input. Discovery announcements are treated as *hints*, never as authorisation.
A device is trusted only when its TLS certificate matches the one pinned at
pairing time, and only then do protected operations become available.

---

## 5. Attack surfaces

| Surface | Reachable by | What must hold |
|---|---|---|
| UDP 1716 discovery broadcast | anyone on the LAN | parsed defensively; cannot confer trust |
| mDNS `_kdeconnect._udp` | anyone on the LAN | same |
| TCP 1716–1764 accept loop | anyone on the LAN | bounded concurrency; handshake timeout; pinning before any privileged operation |
| Packet framing / JSON parsing | anyone who can connect | size caps, no unbounded allocation, no panic |
| Pairing exchange | any connecting device | user confirmation; verification code; rate limiting |
| Payload transfer ports | any connecting device | pinned cert; size ceiling; free-space check; path confined to the download directory |
| Clipboard apply | the webview and the backend | contents never logged or persisted; echo suppression |
| Tauri command surface | the webview | capabilities allow-list; argument re-validation in Rust |
| Filemanager socket | same user | mode 0600; bounded request; path validation; bounded item count |
| adb / scrcpy / ssh | user action only | argv arrays; validated serials and targets; host key checking kept |
| RPM install scriptlet | root, once | creates files as the user, not as root |

---

## 6. Existing mitigations, verified in this codebase

These were checked against the source rather than assumed.

- **Certificate pinning.** `core/kdeconnect/src/link.rs` fetches
  `trusted_certificate_der` and calls `verify_peer` *before* the TLS stream is
  split and before the link is published. For a paired device the full
  certificate DER must equal the stored copy. A device claiming a trusted
  `deviceId` while presenting a different certificate is rejected.
- **CN binding.** The certificate's CommonName must equal the announced
  `deviceId`, so a certificate cannot be lifted from one device onto another.
- **Device id validation.** `is_valid_device_id` enforces
  `^[a-zA-Z0-9_-]{32,38}$` before a peer's claim is used as a map key.
- **Fail-closed authorisation.** `LinkCommand`s are routed through a
  per-device map; an unknown or untrusted device produces an error, never a
  silent success.
- **No clipboard logging.** Clipboard handling logs byte counts, never content
  (`core/kdeconnect/src/lib.rs:191` carries an explicit comment saying so). No
  clipboard history is stored anywhere.
- **No HTML injection sink.** The front end uses Vue's default text
  interpolation throughout; `v-html`, `innerHTML` and `eval` appear nowhere in
  `app/main/src`. Device-controlled strings cannot become executable markup.
- **No shell interpolation.** adb, scrcpy and ssh are invoked with argument
  vectors; the string forms found are log messages and tests only.
- **Secret file permissions.** `key.pem` and `certificate.pem` are mode 0600 in
  a mode 0700 directory; verified on the running system.
- **Text shares are bounded.** A text or URL share larger than 256 KiB is
  dropped rather than surfaced.
- **Untrusted identifier shape is checked before use.** Device ids, scrcpy
  serials and scrcpy arguments are validated before reaching a command line.

---

## 7. Out of scope by design

- **Relaying.** There is no cloud component, no telemetry, and no outbound
  connection that is not to a device on the local network. Verified: the only
  network activity is discovery and the two protocols.
- **At-rest encryption of user data.** Downloaded files are stored unencrypted
  in a user-chosen directory, as they would be in any file manager. Encrypting
  them would break the primary use and add a key-management problem without a
  corresponding threat — the attacker model here is a network peer, which TLS
  already excludes.
- **The Android companion.** None exists. The stock `org.kde.kdeconnect_tp`
  speaks protocol 8 unmodified and is out of this project's control.