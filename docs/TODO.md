# Pixel Connect — outstanding work

Status as of 2026-10-11, after working through the P1/P2/P3 list.

---

## Done since the last update

### Link drops after 30–60 s on a hotspot — fixed, not verified

There was no keepalive at all, so a hotspot's idle NAT mapping expired while
both ends still believed the socket was open. Nothing noticed, so nothing
reconnected. Transfers kept working because they keep the mapping warm.

Added a keepalive (20 s timer, ping once idle 25 s, link declared dead after
75 s of silence) and a record of how a link ended, so a dropped link can be told
from one that never came up.

**Not verified on hardware** — the phone was not on the hotspot. See
[`docs/diagnosis/hotspot-link-drop.md`](docs/diagnosis/hotspot-link-drop.md).

### Notifications — working, confirmed by the user

Five events, each with its own setting. Quick Share's accept-from-notification
path was dead code: it tested `direction == FrontToLib` while the library only
ever sends `LibToFront`.

### Image to clipboard on receive — done

`clipboard_images_on_receive` was a toggle wired to nothing. Added the `image`
crate and a decoder path; `TransferProgress` now carries `saved_path` so the app
knows where the file landed.

On the echo loop that was asked about: the watcher only reads *text*, so an
image is never pushed back to the phone on its own account. Decoding before
writing the clipboard closes the remaining race. No text is suppressed.

### One download folder for both protocols — done

Quick Share and KDE Connect had separate settings, so choosing one left the
other writing somewhere the user had not picked. `set_download_folder` applies
to both, and the Settings page has one picker.

### Honest transfer reporting — done

Nautilus said "Sent 1 item" for a transfer that had not started: the reply means
*queued*, not delivered. It now says "Sending", and the outcome arrives as its
own notification.

---

## P3 — investigated, not implementable as asked

### Quick Share over a NAT that will not forward a reversed connection

**Not implementable against the stock phone app, and that is the finding.**

KDE Connect's file transfer reverses the connection: the sender listens and the
receiver dials back. Android's `LanLink.receivedNetworkPacket` dials the
announced port and waits for TLS. It has no inline-payload path, so **the peer
decides the transport, not the sender**.

Relaying over the already-open control channel — which the 32 MiB framing would
allow — would therefore only work against another build of this application,
while breaking the `org.kde.kdeconnect_tp` it is meant to pair with. That is
worse than the NAT problem.

Full reasoning and measurements in
[`docs/diagnosis/nat-relay-not-available.md`](docs/diagnosis/nat-relay-not-available.md).

### Bluetooth and Wi-Fi Direct

Not implemented; blockers documented in
[`docs/architecture/v2-transport-findings.md`](docs/architecture/v2-transport-findings.md).
Both require the phone to act as the discovering party.

---

## Still outstanding

### `docs/testing/*.md` — not started

Four reports required by the original brief and still unwritten:
`environment-report`, `full-test-matrix`, `failure-analysis`, `final-test-report`.

Every test must be classified PASS/FAIL/BLOCKED/NOT IMPLEMENTED/PARTIAL. A
successful build is not a passing test.

### Incoming-share accept is not a gate

KDE Connect has no consent step for a received file — the receiver writes bytes
on arrival, and Android does the same. `kde_accept_share` records the decision
and clears the request; it cannot refuse the transfer.

A real gate would hold the payload socket unopened until the user accepts. That
works against stock Android, at the cost of a sender timeout. Not implemented.

See [`docs/diagnosis/incoming-share-consent.md`](docs/diagnosis/incoming-share-consent.md).

### Verify the hotspot fixes on hardware

Both hotspot fixes — the loopback broadcast and the keepalive — are unit-tested
and inspected, but neither has been confirmed on a real hotspot connection.

---

## Known environment notes

- `kdeconnectd` is redundant and should be removed. It holds TCP/UDP 1716 and
  keeps a separate trust store, so the phone sees two computers and pairing
  happens against the wrong app. Nothing delegates to it.
- The Pixel is paired with **stock KDE Connect**, not with Pixel Connect, so
  `~/.config/dev.pixelconnect.app/kdeconnect/trusted_devices.json` is empty
  (`{}`). Transfers work because the phone tolerates the link; there is no
  certificate pinned on our side. Pairing with Pixel Connect fixes this.
- `/tmp/opencode` is scratch space and has been cleared twice by server
  restarts, taking the build sysroot with it. `scripts/setup-sysroot.sh`
  rebuilds it and protoc must be re-fetched; both are needed before building.