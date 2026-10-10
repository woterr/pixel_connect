# Pixel Connect — outstanding work

Status as of 2026-10-11. Ordered by what is blocking the user, not by when it
was found.

---

## P1 — Blocking

### Hotspot link drops after 30–60 s and never comes back

**Reported 2026-10-11, on a Pixel 8 hotspot. Not yet diagnosed to a cause.**

The device is found, pairs, and connects. File sharing works. Then after
roughly 30–60 seconds the link drops, and does not re-establish on its own.
Recovering requires `pkill pixel-connect` and reconnecting the phone.

Two suspects, both found by reading the code, neither confirmed:

**1. No keepalive exists.** `core/kdeconnect/src/link.rs` sets `set_nodelay`
and then never sends anything on an idle control channel. There is no
keepalive, ping or heartbeat anywhere in the link or the LAN provider. A NAT
mapping on a phone hotspot is typically short-lived, so an idle TCP connection
is dropped by the gateway after a period with no packets. Nothing in the code
keeps the mapping warm.

This fits the reported timing well — 30–60 s is a plausible NAT idle timeout on
Android hotspots — and it also explains why **file sharing works**: a transfer
sends packets continuously, which refreshes the mapping. The failure appears
only when the link is idle.

**2. Reconnection is weak.** `svc.refresh()` runs on a 5 s tick and
re-broadcasts identity, but nothing retries a *failed* link for a device that
was already connected. A dropped link appears to stay dropped.

Next step: confirm which by watching whether a TCP FIN/RST arrives from the
phone at the moment of the drop (client closed, so NAT or app) or whether the
socket simply goes silent (NAT). Add a keepalive regardless — it is the fix for
suspect 1 and is harmless if the cause is 2.

### `docs/testing/*.md` — not started

Four reports required by the original brief and still unwritten:
`environment-report`, `full-test-matrix`, `failure-analysis`, `final-test-report`.

Every test must be classified PASS/FAIL/BLOCKED/NOT IMPLEMENTED/PARTIAL. A
successful build is not a passing test.

---

## P2 — Incomplete

### Image → clipboard on receive

`clipboard_images_on_receive` exists in settings and has a UI toggle, but the
clipboard stack is text-only (`clipboard.rs` exposes `read_text`/`set_text`
only). Not implemented. Deliberately left half-wired rather than faked.

### Download folder — command only, no UI

`set_download_folder` applies one folder to both protocols and is wired to
`get_download_folder`, but there is no folder picker in Settings. The user
cannot yet change it through the interface.

### Honest transfer reporting

Nautilus reports `sent: N` counting files **queued**, not delivered.
`share_files` returns before any bytes move. This is why the PC once showed
"Sent 1 item" for a transfer the phone rejected. The payload path is fixed;
this reporting bug is not.

### Incoming-share accept is not a gate

KDE Connect has no consent step for a received file — the receiver writes bytes
on arrival, and Android does the same. `kde_accept_share` records the decision
and clears the request; it cannot refuse the transfer. Documented in
[`docs/diagnosis/incoming-share-consent.md`](docs/diagnosis/incoming-share-consent.md).

A real gate would mean holding the payload socket unopened until the user
accepts. That works against stock Android, at the cost of a sender timeout.

---

## P3 — Investigated, deliberately not implemented

### Bluetooth and Wi-Fi Direct

Not implemented; blockers documented. Requires the phone to act as the
discovering party on both transports.

### Quick Share over the reversed-connection NAT problem

File transfer reverses the connection: the receiver dials back. Through a
gateway that only pins the mapping the phone initiated, the return path is
unreachable. Documented in
[`docs/diagnosis/nat-payload-failure.md`](docs/diagnosis/nat-payload-failure.md).

Fixing it in-app would mean relaying the payload over the already-established
control channel, which diverges from the protocol and would only work against
our own build, not stock `org.kde.kdeconnect_tp`.

---

## Known environment notes

- `kdeconnectd` is redundant and should be removed. It holds TCP/UDP 1716 and
  keeps a separate trust store, so the phone sees two computers and pairing
  happens against the wrong app. Nothing delegates to it.
- The Pixel is paired with **stock KDE Connect**, not with Pixel Connect, so
  `~/.config/dev.pixelconnect.app/kdeconnect/trusted_devices.json` is empty
  (`{}`). Transfers work because the phone tolerates the link; there is no
  certificate pinned on our side. Pairing with Pixel Connect fixes this.
- `/tmp/opencode` is scratch space and was cleared by a server restart,
  taking the build sysroot with it. `scripts/setup-sysroot.sh` rebuilds it.