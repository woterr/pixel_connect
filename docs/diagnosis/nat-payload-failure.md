# Why "The phone did not open the file transfer connection"

**Status: root cause identified, not a code defect.** Reproduced 2026-10-10.

## Symptom

Sending a file from Pixel Connect, or from Files → "Send to Pixel", fails with:

    Transfer failed: The phone did not open the file transfer connection

The Nautilus path additionally reported success on the PC (see
[Honest reporting](#honest-reporting-is-still-misleading)).

## Root cause: the laptop and the phone are on opposite sides of a NAT

The test network puts the phone behind the wireless gateway, so it is not on the
same broadcast domain as the laptop.

```
laptop  wlo1  10.141.5.105/24
gateway       10.141.5.77      (the laptop's default route)
phone         192.168.29.5     (reached only via the gateway)
```

Evidence, all from this machine:

```console
$ ip -br addr show wlo1
wlo1   UP   10.141.5.105/24

$ ip route get 192.168.29.5
192.168.29.5 via 10.141.5.77 dev wlo1 src 10.141.5.105

$ getent hosts 10.141.5.77
10.141.5.77   _gateway

$ ss -tnpo | grep 1716          # the KDE Connect control channel
ESTAB  192.168.29.8:47806 -> 192.168.29.5:1716
```

Read that last line carefully. The socket's **local** address is
`192.168.29.8`, which is **not an address on any interface this machine
owns**. The machine has exactly four interfaces: `lo`, `enp108s0` (down),
`wlo1`, `tailscale0`. `192.168.29.8` appears on none of them.

`192.168.29.8` is the address the gateway NATs us to. The phone therefore
believes it is talking to `192.168.29.8` when it is really talking to
`10.141.5.105`.

## Why that breaks file transfer specifically

KDE Connect's file transfer is a **connection reversal**: the sender listens,
and the receiver dials back. Android does this in `LanLink.kt`:

```kotlin
// kdeconnect-android .../backends/lan/LanLink.kt:234
val deviceAddress = socket.getRemoteSocketAddress() as InetSocketAddress
payloadSocket.connect(InetSocketAddress(deviceAddress.address, tcpPort))
```

It dials `getRemoteSocketAddress()` of the existing control socket — the
address it was given by the connection it already trusts. On this network that
resolves to the NAT address `192.168.29.8`, on a *different* port, and the
gateway has no rule to map an arbitrary new inbound port back.

The control channel survives because the phone **initiated** it: the phone
dialled out to `192.168.29.8:1716`, a single pinned mapping the gateway
establishes once and keeps. The payload channel needs a *second*, different
port opened in the same direction, and that never gets through.

## The proof

A direct dial test, independent of KDE Connect, on an ephemeral port exactly
as `send_with_payload` binds:

```
  we listen on ephemeral port 35393
  dial 192.168.29.8:35393  -> TimeoutError: timed out
  dial 10.141.5.105:35393 -> CONNECTED
```

The address the phone uses times out. Our real address answers immediately.

So the sequence is:

1. Pixel Connect binds `0.0.0.0:<ephemeral>` and announces that port in
   `payloadTransferInfo`.
2. Android reads the port, dials `192.168.29.8:<ephemeral>`.
3. The packet is absorbed by the gateway, which has no mapping for it.
4. Nothing reaches our listener.
5. `listener.accept()` hits its 30 s timeout and emits
   "The phone did not open the file transfer connection".

The transfer never starts. No byte of the file is ever sent, and TLS is never
reached — so the TLS role fix from earlier is correct but untested here; it is
not implicated in this failure.

## Honest reporting is still misleading

The Nautilus response reports `sent: 1` when it means *queued*, not
*delivered*. `share_files` hands each file to the link and returns before any
bytes move. The PC therefore shows "Sent 1 item" for a transfer that is
guaranteed to fail 30 seconds later. This is tracked separately in
[honest-reporting.md](honest-reporting.md).

## Confirming on a network without NAT

Put both devices on the same LAN, with no gateway in between:

- laptop `192.168.1.10`, phone `192.168.1.20`, same `/24`
- the KDE Connect link's local address must then equal `ip -br addr`'s wlo1
  address

Then a transfer succeeding proves this diagnosis. Until that is done, file
transfer in this environment is **BLOCKED by the network**, not by the
implementation.

## What would fix it without changing networks

Not implemented. Listed so the option is on the record:

- **Relay through the existing control channel.** Send the payload over the
  already-established, already-NATed TCP connection instead of opening a new
  one. Upstream KDE Connect does not do this; it is a divergence from the
  protocol and would only work against our own build of the app, not stock
  `org.kde.kdeconnect_tp`.
- **Upstream NAT traversal.** Add a hole-punching or relay negotiation to the
  protocol. Requires changes to both ends and cannot be done unilaterally.
- **Port-forward the payload range on the gateway.** Fixes this machine's
  network only, is not a property of the application, and is out of scope.

Only the first is implementable in this repository without touching the phone.