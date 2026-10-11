# Investigation: Quick Share offline discovery over BLE

**Status: investigation complete, root cause identified, implementation not yet
done.** 2026-10-11. This is the Priority 1 decision gate.

**Hardware limitation stated up front:** the Pixel was off the network for this
investigation (`adb devices` empty, port 1716 unreachable). Nothing here is
confirmed against a real phone. Every "confirmed" below means confirmed **from
source code**, which is a different and weaker claim than confirmed on hardware.
Both are labelled.

---

## 1. Answering the questions directly

### Does the current advertiser implement Quick Share receiver discovery, or just emit a beacon?

**It emits a beacon. Confirmed from source.**

`core/quickshare/src/hdl/blea.rs`:

```rust
const SERVICE_UUID_SHARING: ... = 0xFE2C;
advertisement_type: bluer::adv::Type::Broadcast,
service_data: [(service_uuid, SERVICE_DATA)].into(),
```

That is a **non-connectable, non-scannable broadcast** carrying 24 bytes of
static service data. There is no `Type::Peripheral`, no `local_name`, and no
scan response.

**Hypothesis B is confirmed.** The advertiser is a beacon, and the rest of the
handshake depends on IP. The log line the user was told was evidence of support
—

```
BleAdvertiser: advertising on Bluetooth adapter hci0 with address 30:E3:A4:0D:55:B5
```

— proves only that advertising started. It says nothing about whether Android
Quick Share will find, connect to, or complete a handshake with this machine.

### Can stock Android discover this without mDNS on a shared LAN?

**No. Not with the current code.** For Android to treat this as a receiver it
must connect and exchange data, and a `Broadcast` advertisement offers nothing
to connect to. There is no GATT server in this codebase at all.

### Does the reference branch add the missing functionality?

**Yes, and it is precisely the missing piece. Confirmed from source.**

Cloned both trees at:

| Tree | HEAD |
|---|---|
| `Martichou/rquickshare` (upstream) | default branch |
| `martinalderson/rquickshare` `feat/ble-receiver-connect-back` | `66e8b6b` |

`core_lib/src/hdl/gatt.rs` — **337 lines, present only on the branch**:

```rust
const QS_ADV_SLOT0_UUID: &str = "00000000-0000-3000-8000-000000000000";
const QS_GATT_SERVICE = ...;
const QS_WEAVE_TO_PERIPHERAL = ...;
const QS_WEAVE_FROM_PERIPHERAL = ...;
```

It registers a **GATT server** (`adapter.serve_gatt_application`) with a primary
service and three characteristics:

| Characteristic | Role |
|---|---|
| `slot0` (read) | returns the advertisement payload |
| weave "to peripheral" (write, **with response only**) | phone → laptop Weave packets |
| weave "from peripheral" (notify + indicate) | laptop → phone Weave replies |

It then bridges the BLE socket to the real Quick Share handshake:

```rust
// Bridges the weave BLE socket to the (transport-generic) inbound handshake:
// answers the Connection Request, then shuttles the `[hash][len][OfflineFrame]`
// stream between the phone and an `InboundRequest` running over an in-memory duplex.
let (inbound_side, weave_side) = tokio::io::duplex(64 * 1024);
```

### Is a working BLE implementation already here, broken by a bug or config?

**Partly.** `hdl/ble.rs` (the `BleListener`, a *scanner* for other devices'
advertisements) exists and is correct for its purpose — but **it is never
called.** `mod.rs` re-exports it and nothing else references it. So there is no
*receiver* implementation to repair; it is simply absent.

### BLE discovery only, or a data socket too?

**Both, and the data socket is the point.** The branch's GATT server carries the
actual Weave connection handshake and then the Quick Share frames. This is
"Google Weave over GATT", and it is a **full transfer path**, not just discovery.

### Is the branch's transfer needed for discovery, for offline transfer, or optional?

**It is required for both, and they are the same mechanism.** Because the
advertisement is a bare beacon, Android cannot proceed without connecting — so
there is no "discovery succeeded" state short of a working GATT session. The
prompt's warning is right: BLE discovery alone cannot be the milestone here.

### Which changes can be ported without replacing working Quick Share logic?

Confirmed by diffing the two trees:

- `gatt.rs` is **new** — a clean port.
- `migratable.rs` is **new**.
- `inbound.rs` changed from `pub struct InboundRequest { socket: TcpStream }`
  to `pub struct InboundRequest<S = TcpStream> { socket: S }` — a **generic
  parameter**, additive and backwards compatible.
- `blea.rs`, `ble.rs`, `mdns.rs`, `outbound.rs`, `info.rs` also differ and must
  be reviewed individually rather than wholesale replaced.

So the protocol logic is preserved; what changes is that the *transport* becomes
generic instead of hardcoded to `TcpStream`.

---

## 2. What each hypothesis turned out to be

| Hypothesis | Verdict |
|---|---|
| **A** Android uses a radio-based discovery stage | **CONFIRMED (from source).** Android connects to the advertised GATT server and exchanges Weave frames. |
| **B** The existing advertiser is incomplete | **CONFIRMED.** It is a `Type::Broadcast` beacon with static service data. No GATT server exists in this tree. |
| **C** The hotspot failure is about topology, not internet | **NOT TESTED — BLOCKED.** No phone. This remains open and must not be folded into any other finding. |
| **D** A broadcast-address bug independently affects IP discovery | **CONFIRMED (from source and measurement), and unrelated to BLE.** `0.0.0.0` was in `broadcast_addresses()`; on Linux it is delivered to loopback. See `nat-payload-failure.md` / the loopback commit. BLE is untouched by this and its fix proves nothing about BLE. |
| **E** Android may behave differently while acting as a hotspot | **NOT TESTED — BLOCKED.** Speculation would be unjustified; deliberately left unasserted. |

---

## 3. Where discovery currently fails

**Precise stage: the GATT connection stage, which is never offered.**

```
Android Share Sheet
   -> BLE scan                 [we advertise; a beacon is seen or not]
   -> BLE connect              [FAILS: advertisement_type is Broadcast]
   -> Weave Connection Request [never reached]
   -> slot0 read / frames      [never reached]
   -> Quick Share handshake    [never reached]
   -> payload transfer         [never reached]
```

The current code cannot progress past the second line, because a broadcast
advertisement is not connectable.

---

## 4. Existing functionality that is reusable

- The whole Quick Share handshake, consent flow, and frame handling in
  `inbound.rs` / `outbound.rs` — **untouched**, and reusable once the transport
  is generic.
- `blea.rs` — reusable in part. It must become a `Peripheral`/`Scoped`
  advertisement carrying the slot0 payload; the current static 24-byte service
  data is not the advertisement Android reads back through slot0.
- mDNS and UDP discovery — **must be kept intact** for ordinary LAN use, and the
  prompt is right that BLE and mDNS must reconcile into one logical device
  rather than showing duplicates.
- KDE Connect — entirely separate, and unaffected by any of this.

---

## 5. The smallest correct change

1. **Port `gatt.rs`** (337 lines) — the GATT server, slot0 characteristic, and
   the two Weave characteristics. GPL-3.0, compatible with this project's AGPL-3.0
   (AGPLv3 §13 permits linking GPLv3 work).
2. **Port the `InboundRequest<S>` generic** and `migratable.rs`.
3. **Rework `blea.rs`** from a broadcast beacon into a connectable peripheral
   advertisement whose payload is what slot0 returns.
4. **Reconcile BLE and mDNS identities** into one device entry.

Not in the minimum, and deliberately deferred: changing the Weave framing,
rewriting the handshake, or replacing the IP transport.

**Size estimate:** roughly 400–500 lines ported, mostly mechanical, plus the
generic parameter change. This is a day of work, not a rewrite.

---

## 6. Tests that will establish whether it works

| Test | Establishes |
|---|---|
| Advertisement is connectable (`Type::Peripheral`/`Scoped`, not `Broadcast`) | the phone is *able* to connect |
| Advertised payload round-trips through a slot0 read | identity is what Android expects |
| GATT server registers and BlueZ exposes the three characteristics | the service exists at the OS level |
| `btmon` capture during a real Share Sheet scan | the phone actually finds it |
| Weave Connection Request → Confirm over GATT | the BLE session is protocol-valid |
| Hash of a received file matches the source | the transfer is correct, not merely "no error" |
| BLE and mDNS resolve to one device entry | no duplicate UI rows |
| mDNS path still works with BLE disabled | LAN use is not regressed |

A simulated BLE peer is useful for the first four and **cannot** establish
Android compatibility for the rest.

---

## 7. Current status by the required reporting states

| Item | State |
|---|---|
| Advertiser exists and starts | **PASS** — it advertises |
| Advertiser is Quick Share-compatible | **FAIL** — a beacon, not a receiver |
| GATT receiver implementation | **NOT IMPLEMENTED** |
| Offline discovery via Android Share Sheet | **NOT IMPLEMENTED** |
| Offline transfer | **NOT IMPLEMENTED** |
| mDNS / LAN discovery | **PASS** |
| Weave framing and handshake logic | **PASS** — present and correct, unused over BLE |
| Any of the above verified on the Pixel | **BLOCKED** — phone off network |

**Note on the distinction the prompt draws:** "advertising" is PASS and
"discoverable by stock Android Quick Share" is NOT IMPLEMENTED. These are
different outcomes and must never be merged.