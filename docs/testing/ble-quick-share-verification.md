# Quick Share over Bluetooth — verified on hardware

**Status: PASS.** A Google Pixel 8 shared text **and a file** to this machine
over Bluetooth, with no shared network, and both arrived.

Verified 2026-10-11 on a Pixel 8 (Android 17, serial `47221VDJH00348`,
`org.kde.kdeconnect_tp` 1.35.17) against a Fedora GNOME laptop. Captures:

- text: `/tmp/opencode/ble-debug/run-232814/app.log`
- file: `/tmp/opencode/ble-debug/run-233208/app.log`

Both were confirmed by the user, not inferred from the log. This is the first
verified end-to-end Bluetooth Quick Share for this project.

**The file path works. The speed does not** — see §4.

---

## 1. What success required

Stock Android Quick Share, no custom app, and **no shared LAN or internet**.
The phone finds the computer over BLE, completes the weave handshake, offers
the payload, the user accepts, and the payload arrives.

## 2. The verified run

The phone and laptop were on different networks, so the transfer could not fall
back to TCP. The debug script confirmed the route taken:

```
Which route the phone used:
  TCP/mDNS (LAN)                         0
  BLE weave (notify socket)              1
```

Bluetooth, and only Bluetooth.

### 2.1 Discovery and the weave socket

```
weave: notify session open
weave: connected (selected=509)
```

The phone subscribed to weave notify characteristic `0102` and the session
opened.

### 2.2 Handshake

```
Handling State::Initial frame
RemoteDeviceInfo: RemoteDeviceInfo { name: "Shreyas's Pixel 8", device_type: Phone }
Handling State::ReceivedConnectionRequest frame
CipherCommitment: P256Sha512
Handling State::SentUkeyServerInit frame
Pin code: Some("1201")
Handling State::ReceivedUkeyClientFinish frame
```

Discovery, device identification, the ECDH key agreement and the pairing PIN
all completed.

### 2.3 The offer reaches the user

```
process_introduction: handling text_metadata
Asking for user consent: TransferMetadata { id: "ble-weave", source: Some(RemoteDeviceInfo { name: ...
quickshare offer: Shreyas's Pixel 8 wants to share most precious thing
```

The payload metadata was parsed and the offer surfaced in the application.

### 2.4 The accept reaches the session

This is the step that had never worked. At 17:59:21:

```
inbound: got: ChannelMessage { id: "ble-weave", direction: FrontToLib, action: Some(AcceptTransfer...
```

### 2.5 Completion

```
Transfer finished
```

The user received the text. **Confirmed by the user**, not inferred from the log.

### Timing

| Step | Time (UTC) |
| --- | --- |
| Application start, GATT service registered | 17:58:16 |
| Offer surfaced for consent | 17:59:18 |
| Accept delivered to the BLE session | 17:59:21 |
| Transfer finished | 17:59:21 |

The offer was raised about a minute after start-up. Discovery latency on the
phone side is largely outside our control; the reference implementation reports
15–20 s. **Not measured precisely** — only the coarse points above.

---

## 3. The two faults that had to be fixed

Both were introduced by the Bluetooth work itself and both were silent.

### 3.1 Events discarded where they were produced

The receiver was created with:

```rust
let (msg_sender, _) = broadcast::channel(50);
```

The `Receiver` was bound to `_` and dropped immediately. A tokio broadcast
channel with **no receivers is not an unread queue** — `send` on it returns
`Err` and the message is discarded at the point it is produced.

Nothing failed loudly. The handshake completed on the wire, the payload was
transferred, and the application was told nothing, so the UI sat on
"connecting" indefinitely.

Fixed by keeping the `Receiver` alive.

### 3.2 The accept had no path back

The first fix gave the BLE session a **private** message channel, on this
reasoning:

> every `InboundRequest` subscribes to the channel it was given and each
> `recv()` takes the first message, so two sessions sharing one channel steal
> each other's traffic

**This is false.** Every `broadcast::Receiver` keeps its own cursor, so two
sessions on one channel each see every message and neither can starve the
other. Proven by `core/quickshare/tests/broadcast_cursor.rs`:

```
two_receivers_on_one_channel_both_see_every_message ... ok
```

The private channel then needed a relay to carry events out to the application,
and **the relay was only wired one way**. The offer travelled BLE → app; the
accept had no route back. The observable symptom:

```
Unhandled connection state in process_transfer_setup: WaitingForUserConsent
... 55 seconds of keepalives ...
weave: DISCONNECTION
```

The user saw an accept prompt, pressed it, and nothing happened until the phone
gave up.

Fixed by **deleting** the private channel and the relay, and handing the BLE
receiver the application's own channel — which is what the TCP inbound has
always used, and the wiring that makes the LAN path work.

---

## 4. Performance — measured, and poor

The user reported that transfers "took VERY long to even start". The log
quantifies it. Two separate costs:

| Phase | Elapsed |
| --- | --- |
| Discovery (application start → weave session open) | **49 s** |
| Payload (395,176 bytes, file created → transfer finished) | **65 s** |

**Measured throughput: 5.94 KB/s.** At that rate a 10 MB file takes about
**29 minutes**.

This is the honest limitation of the current implementation. It is correct,
not fast.

### What is known and not known about the cause

Known from the code: the weave write characteristic is declared with
`write_without_response: false`, so every write from the phone is acknowledged.
A round trip per write costs throughput, and received weave packets of
100–500 bytes indicate the MTU is negotiated above the 23-byte default, so a
small MTU is not the explanation.

**Not investigated.** Candidate causes, none confirmed:

- the acknowledged write above,
- the BLE connection interval, which is left at whatever the controller picks,
- the Wi-Fi bandwidth upgrade, which was **deliberately not ported** — the
  payload stays on Bluetooth regardless of the network being available.

Which of these dominates has not been measured. No fix has been attempted, so
no claim is made about what would help.

**Not verified:** battery impact, range, and behaviour on reconnection.

## 5. The file transfer, in full

From `run-233208`. Route taken: Bluetooth only (`TCP/mDNS (LAN) 0`).

```
18:03:00  weave: notify session open
18:03:00  weave: connected (selected=509)
18:03:00  weave: INTRODUCTION
18:03:01  File name: Screenshot_20261011-232257.png
18:03:01  Destination: "/home/user/Downloads/Screenshot_20261011-232257.png"
18:03:01  Asking for user consent: TransferMetadata { id: "ble-weave", ...
18:03:01  quickshare offer: Shreyas's Pixel 8 wants to share 1 file
18:03:03  inbound: got: ChannelMessage { id: "ble-weave", action: Some(AcceptTransfer...
18:03:03  Created file: File { fd: 38, path: "/home/user/Downloads/Screenshot_...
18:04:08  Transfer finished
```

The delivered file was checked independently and is complete, not truncated:

```
size: 395176 bytes
PNG image data, 864 x 1920, 8-bit/color RGBA, non-interlaced
PNG signature ok: True
ends with IEND: True
```

A valid PNG with its terminating `IEND` chunk present is the minimum evidence
that every byte arrived; a truncated transfer would lack it. **Not verified
beyond that:** the image was not compared against the original, so pixel
correctness is assumed from the intact container.

### A metric that was wrong

The run report printed `files offered 778`. That is 778 payload **chunks**, not
778 files — roughly 508 bytes each for one 395 KB file. The counter is now
labelled `file chunks received`.

## 6. Discovery latency

The 49 s in §4 is **not all machine time**. The advertisement was live from the
moment the application started; after that the user still had to read the
script's output, open the share sheet, find the computer and tap it. A single
run cannot separate those, so **the machine-side figure is not known** and the
49 s should not be quoted as pure discovery latency.

What was examined, and what it showed.

### Advertisement interval — already fast

The connectable set already advertises at 100–150 ms, deliberately faster than
BlueZ's ~1–2 s default. There is little left here; going faster trades power
for latency and the specification's floor is far below what the controller will
use in practice.

### A non-connectable companion set — rejected by BlueZ

The standard technique for faster Android discovery is to advertise a
non-connectable set alongside the connectable one. Android's scanner runs two
passes, looking for non-connectable devices first and connectable ones second;
advertising only as connectable means the computer is invisible during the first
pass.

This was implemented and **BlueZ refuses it**:

```
could not advertise the non-connectable companion set
(Bluetooth operation failed: Failed to parse advertisement.)
```

The endpoint data is 118 bytes and a legacy advertising payload is 31. The
connectable set is accepted because it may carry more; the non-connectable one
is not, and the payload layout is fixed by the protocol so it cannot be
shrunk. The attempt was reverted rather than left in place emitting a warning
every 30 seconds. **Discovery cost is not reducible this way on this adapter.**

The error was initially invisible because the first implementation discarded it
with `.ok()`. Replacing that with a logged error is what revealed the cause, and
it is the behaviour the rest of this codebase follows.

### Start at login — the setting existed and did nothing

The dominant factor is not on the air at all: **a Quick Share receiver is only
discoverable over Bluetooth while this application is running**, because the
advertisement comes from us. If the share sheet is opened before the
application has been started, there is nothing to find and the phone waits for
a target that cannot appear.

`tauri-plugin-autostart` was initialised at start-up and then **never used** —
no code path called `enable()`. The `autostart` setting was stored and offered
in the interface but had no effect whatsoever. On this machine there was no
saved `settings.json`, so the default was the only thing in play.

Both are fixed: the setting is applied at start-up, and it now defaults to on.
A failure to apply it is logged with the consequence spelled out, because
silently not registering would leave discovery broken in a way nothing else
would reveal.

**Verified** on this machine — the plugin wrote the entry and the setting round
trips:

```
start at login is on (autostart registered as false)
```
```
~/.config/autostart/Pixel Connect.desktop
  Exec=/home/user/.local/bin/pixel-connect
```

**A caveat that matters for upgrades.** Settings are persisted, so the new
default only reaches installations that have no stored value. This machine had
`"autostart": false` saved from an earlier run and kept the old behaviour until
it was changed by hand. Existing users will keep start-at-login off, and the
setting has to be flipped in the interface. Changing a default cannot reach
people who have already run the application once.

**Not verified:** any improvement in measured latency. The claim that this makes
discovery faster is not established — what is established is that the computer
is discoverable without having been launched first.

### A note on the other autostart entry

`~/.config/autostart/` also contains `kdeconnect.desktop`, so stock KDE
Connect starts at login too. That is a separate finding from the duplication
already recorded elsewhere, and is left alone: removing another program's
autostart entry is not this project's decision to make.

## 7. What is still not verified

Stated plainly, because the point of this document is to be trustworthy.

- **Large files are impractical over Bluetooth.** Measured at 5.94 KB/s (§4).
  Correct but slow. No fix attempted.
- **Pixel correctness of the delivered image was not checked**, only that the
  container is intact.
- **Battery, range and reconnection behaviour over Bluetooth are untested.**
- **The advertisement is still opt-in** (`PIXEL_CONNECT_BLE_RX=1`). It is off by
  default because advertising `0xFEF3` makes the phone prefer Bluetooth, and
  until the file path is proven that would regress the working network route.
- **Frame type 12** arrives in an encrypted frame, is not defined by the
  protocol this build implements, and is logged as unhandled. Non-fatal in this
  run. Its meaning is unknown and has not been guessed at.

## 8. How it was diagnosed

`scripts/ble-debug.sh` captures the application's own log and reports which
route the phone took, how far the handshake got, and what was delivered. It
needs no root.

Two earlier versions of that script reported healthy transfers as failures —
once because it only counted Bluetooth strings while the phone had used TCP,
and once because it only looked for written files while the phone had shared
text. Both are fixed; a run that now says "nothing was written" for a text
share is stating it correctly.

## 9. Regression tests

| Test | Pins |
| --- | --- |
| `tests/broadcast_cursor.rs` | Two receivers on one channel both see every message — the false premise |
| `tests/broadcast_cursor.rs` | `subscribe()` starts at the tail; a late subscriber misses earlier messages |
| `tests/ble_session_channel.rs` | A channel with no receiver discards messages |
| `tests/ble_session_channel.rs` | An application command reaches a session |
| `tests/ble_session_channel.rs` | A session event reaches the application |
| `tests/ble_session_channel.rs` | Offer then accept crosses in **both** directions |

The last one failed before the fix and passes after it. It is the direct test
of the fault in §3.2.