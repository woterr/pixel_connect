# Pixel Connect V2 — status

What V2 changed, what was verified, and what remains. Read this alongside
`v1-status.md` (the V1 baseline) and `v2-transport-findings.md` (the
Bluetooth and Wi-Fi Direct investigations).

Evidence labels, as in V1:

* **[built]** compiles.
* **[tested]** an automated test exercises it and passes.
* **[live]** verified against a running application on this machine.
* **[device]** verified against a real Google Pixel. **Still nothing is
  labelled `[device]`: no Pixel has been attached at any point.**

---

## 1. Summary

V1 shipped a working application whose status report listed six known gaps.
V2 closed the two that could be closed honestly, replaced two that were
closed on an unmeasured assumption, and added the distribution groundwork.

| V1 gap | V2 outcome |
|---|---|
| Wayland clipboard needs the window focused | **Closed.** The XWayland bridge works; sync is now fully background-capable on GNOME. |
| Notifications: reply impossible, five invented field names | **Closed.** Protocol corrected and typed reply implemented. |
| No transfer cancel or pause | **Cancel closed**; pause documented as not existing in the protocol. |
| No Bluetooth transport | **Investigated with measurements.** Not implementable testably here. |
| No Wi-Fi Direct transport | **Investigated with measurements.** Blocked by NetworkManager, root, and absent upstream support. |
| Clipboard file/image payloads | **Still not done.** Android has no counterpart; low value. |

---

## 2. Clipboard: a V1 limitation that turned out to be wrong

### 2.1 What V1 believed

V1 asserted that clipboard synchronisation could not run in the background on
GNOME/Wayland. That was inferred from the protocol, never measured.

### 2.2 What was measured

On GNOME 50 / mutter 50.5:

| Question | Method | Result |
|---|---|---|
| Is `ext-data-control-v1` advertised? | `wl-clipboard-rs` probe | **No** |
| Is `wlr-data-control-v1` advertised? | same | **No** |
| Can an unfocused Wayland client read the clipboard? | core `wl_data_device` read | **No** — V1 was right about this part |
| Can an unfocused **X11** client read it, under Wayland? | set via `wl-copy`, read via `arboard` | **Yes**, 6/6 trials |
| Can an unfocused X11 client *write* it, under Wayland? | set via `arboard`, read back | **Yes**, 3/3 trials |

Mutter mirrors the Wayland selection into the X11 selection. That makes the
clipboard reachable without focus, through XWayland, on a session where no
data-control protocol exists.

### 2.3 What changed

`core/clipboard.rs` now probes the session and selects a strategy:

| Mechanism | When | Background-capable |
|---|---|---|
| `WaylandDataControl` | data-control present (wlroots compositors) | yes |
| `X11` | native X11 session | yes |
| `WaylandX11Bridge` | Wayland session, X11 selection reachable | **yes — GNOME** |
| `WaylandCoreOnly` | neither | no; reads need focus |
| `None` | no display server | no |

The probe is side-effect free: it never writes to the clipboard, because doing
so at start-up would clobber whatever the user had copied.

**On this machine the result is `Wayland (XWayland bridge)`** — verified by a
test that prints the detected mechanism. **[tested] [live]**

The focused-window polling in the front end is now a genuine last resort rather
than the normal path.

### 2.4 Loop prevention

The echo suppressor remembers text this machine just wrote and refuses to send
it back to the phone for four seconds, then forgets. A unit test pins both the
suppression and its expiry, so a stale entry cannot permanently swallow a
genuine re-copy.

### 2.5 Still limited

- The mechanism is per-session. A compositor with neither data-control nor a
  usable bridge falls back to requiring focus.
- Clipboard *contents* are still never logged or stored.

---

## 3. Notifications: five invented field names, corrected

Re-reading `NotificationsPlugin.kt` (kdeconnect-android) and
`notification.cpp` (kdeconnect-kde) showed V1's handling disagreed with both.

| Field | V1 used | Both implementations use |
|---|---|---|
| notification key | `key` | **`id`** |
| actions | `pause`/`reply`/`open` body keys | **`actions`, a JSON array of titles** |
| timestamp | `t` | **`time`, a decimal string on Android**, absent on the desktop |
| action packet key | `actionId` | **`action`** |
| reply capability | not read | **`requestReplyId`** |

A sixth problem was found and removed during the work: I began implementing
`kdeconnect.notification.dismiss`, then found **neither implementation defines
such a packet**. Inventing one would have broken interoperability, so it was
deleted.

### 3.1 What works now

- Actions are read from the real `actions` array and sent with the real key.
- `time` is parsed whether it is a string or a number, and falls back to "now".
- **Typed reply** is implemented (`kdeconnect.notification.reply` with the
  phone's own `requestReplyId`), with a reply box in the UI shown only when the
  phone marked the notification repliable. **[built]**
- `kdeconnect.notification.request` is handled, so a phone that reconnects gets
  the notifications we hold. **[built]**
- Notifications are namespaced by device id, so identical ids from two phones
  cannot collide, and the list is bounded at 100. **[tested]**

### 3.2 Verification

Seven tests in `core/kdeconnect/tests/notifications.rs` pin the wire format,
built from real packets, including **negative assertions** for the names V1 got
wrong:

```
test parses_an_android_repliable_notification ... ok
test a_desktop_notification_needs_no_time_and_offers_no_reply ... ok
test reply_packet_carries_the_request_reply_id_and_message ... ok
test action_packet_uses_the_action_key ... ok
test icon_payload_is_recognised ... ok
test a_notification_without_an_id_is_rejected ... ok
test a_numeric_time_is_also_accepted ... ok
```

**[tested]**

Not `[device]`: no phone has sent a notification to this build.

---

## 4. Transfer cancellation

### 4.1 What the protocol actually provides

There is **no cancel or pause packet** in either implementation.
kdeconnect-android cancels by killing its own job and closing the socket, and
kdeconnect-kde documents the same signal in `compositeuploadjob.cpp`:

> Do not close the socket because when android closes the socket (share is
> cancelled) closing the socket leads to a cyclic socketError

Closing the payload socket **is** the cancellation. This implements exactly
that, via a registry of per-transfer cancel tokens observed by both payload
loops. **[built]**

### 4.2 UI honesty

The Cancel button works. **Pause remains disabled**, with a chip saying why: the
protocol has no pause packet, so a pause control could not do anything.

### 4.3 A test bug this exposed

Writing the cancel test revealed that the existing file-transfer tests were
writing into the real `~/Downloads` and finding each other's files. Each test
now gets its own download directory. **[tested]**

### 4.4 What the test does and does not assert

Loopback drains a payload faster than a test can cancel it — an early version
used 24 MiB and routinely completed before the cancel arrived. The test
therefore asserts only what is deterministic:

- a live transfer accepts a cancel and leaves the registry;
- a finished transfer **refuses** a cancel rather than reporting a false success;
- a transfer is never reported complete after only part of the file, and the
  file on disk matches when it is.

The token semantics that actually stop a transfer are unit-tested separately.
This is a weaker guarantee than "cancelling always interrupts", and the reason
is stated in the test rather than hidden.

---

## 5. Transports: investigated, not implemented

Full findings, with every command and its output, are in
`v2-transport-findings.md`. In summary:

### 5.1 Wi-Fi Direct — blocked by three independent things

| Layer | Finding |
|---|---|
| Hardware | **Supports P2P.** Intel iwlwifi advertises P2P-client, P2P-GO and P2P-device. |
| NetworkManager 1.56 | **No P2P API.** It creates a `p2p-dev-wlo1` device but exposes no D-Bus methods, no `nmcli` verb, no `wifi-p2p` connection type, no P2P properties. |
| Privilege | The only route — driving wpa_supplicant — needs root and would take the interface away from NetworkManager. |
| KDE Connect | **No Wi-Fi Direct support in any version.** |

V1 said the hardware was often unreliable; on this machine it is demonstrably
capable. The blocker is the software stack, not the adapter.

**The isolated-network scenario is still not solved.**

### 5.2 Bluetooth — blocked by the absence of a device

BlueZ is present, `ProfileManager1.RegisterProfile` exists, and an
`AF_BLUETOOTH`/RFCOMM socket can be created unprivileged, so the Linux side is
buildable. But upstream disables the backend (KDE bugs 520578, 513536), the
Android app requires operating-system Bluetooth pairing first, and **no Pixel
was attached**, so nothing could be verified against a peer.

Shipping an untested multiplexer would be exactly the claim the brief forbids:
*"Implement only the behaviors that are technically supported and actually
tested."*

### 5.3 What was built instead

`core/kdeconnect/src/transport.rs` — a registry reporting each transport as
**available / unverified / unavailable**, with a concrete blocker for anything
not available. A test asserts the invariant that matters:

```rust
fn only_verified_transports_are_reported_available() {
    // nothing may claim to be available unless it has been exercised
    assert_eq!(available(), vec![Transport::Lan]);
}
```

The Devices screen renders this registry, so each transport is shown with its
measured reason instead of as a dead control. **[built] [tested]**

---

## 6. Not done in V2

| Item | Why |
|---|---|
| Clipboard file/image payloads (`kdeconnect.clipboard.file`) | Android has no counterpart upstream; it is a desktop-to-desktop feature. |
| Bluetooth transport | No device to verify against; upstream-disabled. |
| Wi-Fi Direct transport | NetworkManager exposes no P2P API; needs root; no upstream support. |
| Branded Android companion | The stock KDE Connect Android app speaks protocol 8 unmodified; see `android/README.md`. |
| Embedded scrcpy window | V1 explicitly allowed launching and lifecycle management, which work. |

---

## 7. Distribution groundwork

### 7.1 Signing

`packaging/signing/` holds a dedicated GPG key generator and an RPM signing
script. Upstream rquickshare's identity is never reused. Private keys are
excluded from version control. **[built]**

### 7.2 CI

`.github/workflows/ci.yml`, three jobs:

- **protocol** — format, `clippy -D warnings`, and the KDE Connect tests on a
  runner with no GUI stack, so a protocol regression is caught even when the
  desktop build is broken.
- **verify** — type check, front-end tests, front-end build, backend tests.
- **build** — release binary plus an `rpmbuild` of the spec, as artefacts.

The integration tests run with `--test-threads 1` because they share a
process-wide mDNS daemon and a real socket listener.

**[built]** — the workflow is committed and validated as YAML. **It has not been
executed on a runner**, because that needs network access to GitHub, which this
environment does not have.

### 7.3 Lint hygiene

- All three crates are `cargo fmt` clean.
- The KDE Connect crate is clippy clean under `-D warnings`, verified locally.
- The Tauri backend emits **no warnings from its own code**. The three
  remaining warnings are deprecation lints inside vendored rquickshare code,
  which is upstream's to fix; CI reports them without failing.
- `app/main/src-tauri/rust-toolchain`, which pinned nightly for historical
  reasons, was removed. The crate builds and tests on stable, verified with
  `cargo +stable`.

### 7.4 Real-device acceptance harness

`scripts/device-acceptance.sh` is the runbook for the verification that is
still missing. It checks everything it can without a phone, prints exactly what
to check with one, and writes a report template.

Run against this machine it reports honestly:

```
! no authorised ADB device (enable USB debugging and accept the prompt)
✓ scrcpy: scrcpy 4.1
✓ release binary present (23M)
✓ KDE Connect peers visible on the LAN
! ADB/scrcpy: cannot verify without a device
✓ KDE Connect discovery and pairing can be exercised
```

**[built] [live]** — the harness itself runs.

---

## 8. Test totals

| Suite | V1 | V2 |
|---|---|---|
| KDE Connect unit | 39 | **47** |
| KDE Connect integration | 5 | **6** |
| Notification wire format | — | **7** |
| Tauri backend | 18 | **23** |
| Front end | 29 | **29** |
| **Total** | **91** | **112** |

All pass. Front-end type check is clean.

---

## 9. What still needs a phone

The single highest-value remaining action is to attach a Pixel and run
`scripts/device-acceptance.sh --record`. That would move the following from
unverified to verified, in roughly this order of value:

1. Pairing and the verification code.
2. Clipboard in both directions, on a real Android 10+ device.
3. File transfer in both directions, with a byte-exact comparison.
4. Notification relay and reply.
5. scrcpy over USB ADB.
6. Nautilus "Send to Pixel" from a real file selection.
7. Whether the Android KDE Connect app accepts TLS 1.3, which it may not.

Item 7 is the one most likely to need a code change: `core/kdeconnect/src/tls.rs`
offers TLS 1.2 and 1.3, and restricting the list is a one-line fix if required.

---

## 10. Honest summary

V2 turned two assumptions into measurements, found and fixed five protocol
bugs, removed one control that could never work, implemented two features that
were listed as gaps, and added the signing, CI and acceptance tooling a
distributable project needs.

It did **not** solve the isolated-network scenario, and it did **not** verify
anything against a real Pixel. Both remain stated plainly rather than papered
over.