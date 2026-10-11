# The hotspot link dropping after 30–60 s

**Status: fixed by a keepalive. Not yet verified on hardware.** 2026-10-11.

## Symptom

On a Pixel 8 hotspot: the device is found, pairs, and connects, and file
sharing works. Then after roughly 30–60 seconds the link drops and does not
reconnect. Only `pkill pixel-connect` and reconnecting the phone brought it
back.

## Cause

There was no keepalive. `core/kdeconnect/src/link.rs` set `set_nodelay` on the
socket and then never sent anything on an idle control channel.

A phone hotspot drops an idle NAT mapping after roughly that long. The socket
therefore stopped carrying traffic while both ends still believed it was open.
Nothing noticed, so nothing reconnected.

This also explains why **transfers kept working**: a transfer sends packets
continuously, which refreshes the mapping. The failure only ever appeared while
idle.

## Fix

A keepalive on the link's `select!` loop:

| Constant | Value | Purpose |
|---|---|---|
| `KEEPALIVE_INTERVAL` | 20 s | How often the timer fires. Under a typical hotspot idle timeout, so the mapping is refreshed in time. |
| `KEEPALIVE_WHEN_IDLE` | 25 s | Send a ping only once the link has been quiet this long, so an active transfer carries no extra packets. |
| `LINK_STALE_AFTER` | 75 s | Roughly three missed keepalives. The peer answers a ping, so silence this long means the mapping is gone even though neither end closed the socket. |

No new packet type is involved: the peer already replies to `kdeconnect.ping`.

Tearing the link down on silence is what lets the existing reconnect path run
again. `LinkContext::report_link_end` also records how a link ended, so a link
that worked and dropped can be told from one that never came up — without that
distinction, retrying a peer whose phone has left the network becomes a
reconnect storm.

## Not verified

The phone was not on the hotspot when this was built, so it is tested by unit
test and inspection only. **The fix is unproven on the hardware it is for.**

To confirm: connect by hotspot, leave it idle for several minutes, and check
that the link is still up and that no `link has been silent` line appears in the
log without the link actually being gone.

## A second, unrelated hotspot bug

Discovery on a hotspot was separately broken: `broadcast_addresses()` returned
`0.0.0.0`, which on Linux is delivered to **loopback only**, so every identity
broadcast was looped back to this machine and never reached the phone. Fixed in
the same day; see the commit *"Fix KDE Connect discovery on a phone hotspot:
identity broadcasts went to loopback"*.

## Related

- [`nat-payload-failure.md`](nat-payload-failure.md)
- [`nat-relay-not-available.md`](nat-relay-not-available.md)