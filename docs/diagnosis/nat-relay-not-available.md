# File transfer through NAT: why relaying is not available

**Status: not implementable against the stock phone app. Recorded 2026-10-11
after P3 was investigated, so it is not re-investigated as an oversight.**

## The problem

KDE Connect's file transfer reverses the connection. The sender listens on a
new port and announces it in `payloadTransferInfo`; the receiver dials back to
it.

On a network where the phone is reached through a gateway, that return path may
not exist. Measured on this machine, with the laptop at `10.141.5.105` and the
phone at `192.168.29.5` behind the gateway `10.141.5.77`:

```
ESTAB  192.168.29.8:47806 -> 192.168.29.5:1716
```

The local address `192.168.29.8` is on no interface this machine owns — it is
the gateway's NAT address. The phone therefore believes it is talking to
`192.168.29.8`, and dialling `192.168.29.8:<new port>` reaches nothing:

```
dial 192.168.29.8:35393  -> TimeoutError: timed out
dial 10.141.5.105:35393 -> CONNECTED
```

The control channel survives only because the phone initiated it: one pinned
mapping the gateway keeps. A second port in the same direction is not mapped.

## Why the obvious fix does not work

Relaying the payload over the already-established control channel would avoid
the reversed connection entirely, and the framing supports it — a single framed
line may be up to 32 MiB (`MAX_PACKET_SIZE`).

**The peer decides the transport, not the sender.** Android's
`LanLink.receivedNetworkPacket` does:

```kotlin
if (np.hasPayloadTransferInfo()) {
    val tcpPort = np.payloadTransferInfo.getInt("port")
    payloadSocket.connect(InetSocketAddress(deviceAddress.address, tcpPort))
    ...
}
```

If the announcement carries a port, the phone dials it and waits for TLS. It
has no path for inline bytes and no way to be told to use one. So:

- Sending **without** `payloadTransferInfo` and inlining the bytes would leave
  stock `org.kde.kdeconnect_tp` connecting to nothing, and failing.
- The receiver here has no inline-payload path at all — it only ever reads a
  payload from a second connection.

Implementing the relay would therefore only work against another build of
*this* application, while breaking the stock phone app it is meant to pair with.
That is a worse outcome than the NAT problem: the app would stop working with
the app the user actually has.

## What would work

- **Hole punching or relay negotiation added to the protocol.** Needs changes
  at both ends. Cannot be done unilaterally, and stock Android will not speak
  it.
- **Port forwarding on the gateway.** Fixes one network, is not a property of
  the application, and is out of scope.
- **A phone hotspot, where both ends share a subnet.** Works today — see
  [`hotspot-link-drop.md`](hotspot-link-drop.md) for the keepalive that keeps it
  up. This is the supported offline configuration, and it is why hotspot support
  was prioritised over relaying.

## Related

- [`incoming-share-consent.md`](incoming-share-consent.md) — the receiver side
  has no consent step, for the same structural reason: the protocol puts the
  choice in the peer's hands.
- [`../TODO.md`](../TODO.md)