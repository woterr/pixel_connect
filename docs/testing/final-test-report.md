# Final test report

Generated 2026-10-11. Companion documents:
[`environment-report.md`](environment-report.md),
[`full-test-matrix.md`](full-test-matrix.md),
[`failure-analysis.md`](failure-analysis.md).

---

## Summary

| Result | Count |
|---|---|
| PASS | 70 |
| PARTIAL | 12 |
| FAIL | 2 |
| BLOCKED | 5 |
| NOT IMPLEMENTED | 1 |
| **Total classified** | **90** |

180 automated tests, all passing. 11 hardware defects found and fixed.

Counts are computed from the matrix rows rather than estimated.

## What works, verified

- KDE Connect v8 implemented in Rust: pairing, 7 capabilities, TLS, discovery
- File transfer **from** the PC — home WLAN and phone hotspot
- Quick Share **text** phone → PC → clipboard
- Notifications — all five events, per-category toggles, accept/reject from the
  tray. User-confirmed.
- Clipboard sync, with a loop guard tested against concurrent applies
- Nautilus "Send to Pixel", launching from the GNOME app grid
- USB and wireless scrcpy mirroring

## The two failures

**F-11 — pairing with Pixel Connect has never succeeded.** The trust store is
empty. Every KDE Connect result above was obtained against a peer this
application has not pinned, so **certificate pinning has never been exercised
end to end**. This is the highest-value remaining item: it gates the security
properties of every other KDE Connect feature.

Root cause not established. Two defects that each blocked it (F-01 loopback
broadcasts, F-02 idle drops) are fixed, and pairing has not been retried since.

**5.7 — accepting an incoming file is not a gate.** KDE Connect has no consent
step for a received file; the receiver writes bytes on arrival and Android does
the same. `kde_accept_share` records the decision and cannot refuse the
transfer. Fixing it means holding the payload socket unopened until the user
accepts, which works against stock Android at the cost of a sender timeout.

## Not verified on hardware

Both hotspot fixes are unit-tested and inspected, and **neither has been
confirmed on a real hotspot connection**:

- **F-01** loopback broadcasts — verified by packet capture on the WLAN
- **F-02** keepalive — the phone was not present when it was built

Also unverified: image-to-clipboard end to end (the decode is unit-tested with
a real PNG, but a phone has never sent one), and any long-duration soak.

## Not implemented

- **Bluetooth and Wi-Fi Direct** — blockers documented
- **Quick Share over a NAT that will not forward a reversed connection** — not
  implementable against the stock phone app, because Android dials the announced
  port and has no inline-payload path. The peer chooses the transport, so a relay
  would work only against another build of this application while breaking
  `org.kde.kdeconnect_tp`. See
  [`../diagnosis/nat-relay-not-available.md`](../diagnosis/nat-relay-not-available.md).

## Honest limitations of this report

**No screen capture is possible** on this compositor, so the UI is verified by
log, file descriptor and process inspection. Visual regressions would not be
caught.

**Several rows are PASS by unit test rather than by hardware**, and are marked
PARTIAL where that distinction matters.

**Pairing is unproven**, which means the security properties of KDE Connect —
the part this project is most careful about — are the least verified.

## Recommended order

1. Retry pairing with Pixel Connect, on the hotspot, now that F-01 and F-02 are
   fixed. This gates the most.
2. Soak test: hotspot, idle, 10+ minutes, confirm the keepalive holds.
3. Send a photo from the phone and confirm it reaches the clipboard.
4. Only then treat the remaining PARTIAL rows as candidates to close.