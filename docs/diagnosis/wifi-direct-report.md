# Wi-Fi Direct as a Quick Share transport

**Recommendation: do not implement it.** Wi-Fi Direct is defined by the
protocol and is implementable in principle, but it would not solve the problem
it appears to solve here, and the thing that would actually fix throughput is
a smaller and more valuable piece of work that is currently missing.

This report is based on the wire format in
`core/quickshare/src/proto_src/offline_wire_formats.proto` and on what this
codebase does and does not do. Claims about Linux Wi-Fi Direct are marked
according to whether they were verified here.

---

## 1. The protocol does define it

The `Medium` enum, verbatim from the wire format:

```
UNKNOWN_MEDIUM = 0;
MDNS        = 1 [deprecated = true];
BLUETOOTH   = 2;
WIFI_HOTSPOT= 3;
BLE         = 4;
WIFI_LAN    = 5;
WIFI_AWARE  = 6;
NFC         = 7;
WIFI_DIRECT = 8;
WEB_RTC     = 9;
BLE_L2CAP   = 10;
USB         = 11;
```

So `WIFI_DIRECT = 8` exists, alongside `WIFI_LAN = 5`. This is the correction
to an earlier reading of this file, which truncated at `NFC` and appeared to
show no Wi-Fi Direct support at all. The enum continues.

The medium appears in two places that matter:

```
message ConnectionRequestFrame {
  // The mediums this device supports upgrading to.
  repeated Medium mediums = 5;
}
```

and a `BandwidthUpgradeNegotiationFrame` with events including
`UPGRADE_PATH_AVAILABLE`.

The design is therefore clear and is **not** "connect over Wi-Fi Direct instead
of Bluetooth". It is: **connect over Bluetooth, then negotiate the payload onto
a faster medium.** Bluetooth is the bootstrap; the medium carries the data.

## 2. What this codebase does today

Advertised mediums, in the BLE service data
(`core/quickshare/src/hdl/blea.rs`, `QS_MEDIUMS_TRAILING`) and in the outbound
connection request (`core/quickshare/src/hdl/outbound.rs`):

```rust
mediums: vec![Medium::WifiLan.into()],
```

So we advertise `WIFI_LAN` as an upgrade target. **Wi-Fi Direct is not
advertised.**

The receive side is where it stops:

```
$ grep -niE "bandwidth|upgrade" core/quickshare/src/hdl/inbound.rs
(no matches)
```

**The bandwidth upgrade is not implemented on the inbound path.** There is no
handling of `BandwidthUpgradeNegotiationFrame` anywhere in `inbound.rs`.

This is not a Wi-Fi Direct question. It is the explanation for the measured
throughput problem, and it is the same explanation regardless of which faster
medium one would target.

Measured on a Pixel 8 (`docs/testing/ble-quick-share-verification.md` §4):

| Phase | Elapsed |
| --- | --- |
| Discovery | 49 s (includes human time; not purely machine) |
| Payload, 395,176 bytes | 65 s |
| **Throughput** | **5.94 KB/s** |

A 10 MB file takes roughly 29 minutes. The cause is that the negotiation that
would move the payload off Bluetooth is never performed, so it stays on
Bluetooth for the whole transfer.

## 3. Why Wi-Fi Direct specifically is the wrong answer

### It does not help when both devices share a network

If the phone and the computer are on the same network, `WIFI_LAN` is already
available, is already advertised, is vastly simpler, and does not require the
computer to join a peer-to-peer group. Implementing Wi-Fi Direct to serve this
case would be strictly worse.

### The case it would serve is the one where the LAN path already works

The only situation where Wi-Fi Direct is genuinely needed is two devices with no
shared network. In that situation Wi-Fi Direct would have to create a direct
link, which requires acting as a Wi-Fi Direct Group Owner.

### The cost is high and mostly outside this project

**Not verified here, from general Linux knowledge:** Wi-Fi Direct on Linux is
`wpa_supplicant` built with P2P support plus a second virtual interface managed
alongside the normal one. It needs the wireless driver and hardware to support
P2P, and it must coexist with NetworkManager without taking the normal
connection down — which is where the real difficulty lies.

Against that, the Bluetooth link needed for the negotiation is **already
working and verified**, and `WIFI_LAN` needs **nothing new**.

### It would still be limited by Bluetooth during setup

The medium is negotiated *after* a Bluetooth connection exists. So Wi-Fi Direct
would improve bulk throughput while leaving the slow part — discovery and
negotiation — exactly as slow as it is now. The 49 s discovery figure would
not improve.

## 4. What to do instead

### 4.1 Implement the bandwidth upgrade (recommended, highest value)

This is the change that makes Bluetooth transfers usable, and it is a
prerequisite for Wi-Fi Direct being worth anything.

Work:

- Handle `BandwidthUpgradeNegotiationFrame` in `inbound.rs`, including
  `UPGRADE_PATH_AVAILABLE`.
- Honour the mediums we advertise. Today only `WIFI_LAN` is offered; when the
  phone is on the same network, upgrading the payload to it is exactly the
  behaviour wanted.
- Rebind the payload stream to the new medium once negotiated, and keep
  encryption and framing across the switch.

Effect: on a shared network, Bluetooth becomes the bootstrap and LAN carries the
payload — which is what the protocol was designed for and what makes the 5.94
KB/s figure a fallback rather than the normal case.

### 4.2 Address the acknowledged write, first

Each weave write is currently acknowledged:

```rust
write_without_response: false,
```

A round trip per write is a plausible contributor to 5.94 KB/s. **Not
established** — the relative contribution of this and the absent upgrade has
not been measured, because the upgrade never happens and so cannot be compared
against.

### 4.3 Consider `USB = 11`

Also defined in the enum, and materially simpler than Wi-Fi Direct: the Pixel
was connected over USB during this work. A wired medium needs no peer group, no
supplicant P2P support, and no virtual interface. Worth investigating before
Wi-Fi Direct, though it changes the use case rather than the transport.

### 4.4 Wi-Fi Direct, if it is ever wanted

Sequence, assuming the upgrade negotiation from §4.1 exists first:

1. Detect P2P support in `wpa_supplicant` and the driver.
2. Bring up a P2P interface alongside the managed one, under NetworkManager.
3. Act as Group Owner, or negotiate as a client to the phone.
4. Advertise `Medium::WifiDirect` in the connection request once (3) works.
5. Hand the negotiated address to the existing upgrade path from §4.1.

**Every step above is unverified in this project.** None has been attempted.
The honest position is that this is a substantial piece of systems work whose
benefit is confined to the no-shared-network case, available only after §4.1
is done, and never improving discovery latency.

## 5. Summary

| Question | Answer |
| --- | --- |
| Is Wi-Fi Direct in the protocol? | **Yes** — `WIFI_DIRECT = 8` |
| Is it implemented here? | **No**, and it is not advertised |
| Would it fix the 5.94 KB/s throughput? | Only after the missing upgrade negotiation exists — which affects every medium, not just Wi-Fi Direct |
| Would it fix discovery latency? | **No.** Discovery happens over Bluetooth before any medium is negotiated |
| Is it needed when both devices share a network? | **No** — `WIFI_LAN` already covers it |
| Recommended? | **No.** Implement the bandwidth upgrade first; that is the actual gap |

The single most valuable observation in this report is not about Wi-Fi Direct.
It is that **`inbound.rs` has no bandwidth upgrade handling at all**, and that
one omission is why a 395 KB file takes 65 seconds.