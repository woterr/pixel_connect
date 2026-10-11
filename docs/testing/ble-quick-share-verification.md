# Quick Share over Bluetooth — verified on hardware

**Status: PASS.** A Google Pixel 8 shared text to this machine over Bluetooth,
with no shared network, and the text arrived in the application.

Verified 2026-10-11 on a Pixel 8 (Android 17, serial `47221VDJH00348`,
`org.kde.kdeconnect_tp` 1.35.17) against a Fedora GNOME laptop. Capture:
`/tmp/opencode/ble-debug/run-232814/app.log`.

This is the first verified end-to-end Bluetooth Quick Share transfer for this
project. Everything below is quoted from that log.

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

## 4. What is still not verified

Stated plainly, because the point of this document is to be trustworthy.

- **A file over Bluetooth is not yet verified.** The run above shared text. A
  file share follows a different payload path (`PayloadType::File`) and writes
  to disk. The network route has delivered a 132 KB PNG correctly; the
  Bluetooth file path has not been exercised.
- **Transfer latency is not characterised.** Only the coarse timeline above.
- **Battery, range and reconnection behaviour over Bluetooth are untested.**
- **The advertisement is still opt-in** (`PIXEL_CONNECT_BLE_RX=1`). It is off by
  default because advertising `0xFEF3` makes the phone prefer Bluetooth, and
  until the file path is proven that would regress the working network route.
- **Frame type 12** arrives in an encrypted frame, is not defined by the
  protocol this build implements, and is logged as unhandled. Non-fatal in this
  run. Its meaning is unknown and has not been guessed at.

## 5. How it was diagnosed

`scripts/ble-debug.sh` captures the application's own log and reports which
route the phone took, how far the handshake got, and what was delivered. It
needs no root.

Two earlier versions of that script reported healthy transfers as failures —
once because it only counted Bluetooth strings while the phone had used TCP,
and once because it only looked for written files while the phone had shared
text. Both are fixed; a run that now says "nothing was written" for a text
share is stating it correctly.

## 6. Regression tests

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