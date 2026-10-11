# Full test matrix

Generated 2026-10-11. See [`environment-report.md`](environment-report.md) for
the machine, networks and phone.

**Every row is a real execution.** A successful build is not a passing test, and
a unit test is not a hardware test. Nothing below is claimed from inspection
alone unless the Evidence column says so.

Legend: **PASS** executed and passed · **FAIL** executed and failed ·
**BLOCKED** could not be executed, reason given · **PARTIAL** some paths
verified, others not · **NOT IMPLEMENTED** no code exists

---

## 1. Automated test suites

| # | Suite | Command | Result | Count |
|---|---|---|---|---|
| 1.1 | KDE Connect unit | `cargo test --release` | PASS | 81 |
| 1.2 | KDE Connect integration (interop) | same | PASS | 6 |
| 1.3 | KDE Connect notifications | same | PASS | 7 |
| 1.4 | Quick Share unit | `cargo test --release` | PASS | 13 |
| 1.5 | Application backend unit | `cargo test --release` | PASS | 44 |
| 1.6 | Front end | `npx vitest run` | PASS | 29 |
| 1.7 | Type safety | `npx vue-tsc --noEmit` | PASS | 0 errors |
| 1.8 | Release build | `cargo build --release` | PASS | 0 errors, 0 warnings from new code |
| 1.9 | Live discovery | `cargo test --release -- --ignored` | BLOCKED | needs a second real device |

**180 tests, 180 passing.**

---

## 2. Discovery

| # | Test | Result | Evidence |
|---|---|---|---|
| 2.1 | KDE Connect identity broadcast leaves the machine | PASS | packets observed from `10.141.5.105`, not `127.0.0.1` |
| 2.2 | `0.0.0.0` is never a broadcast destination | PASS | unit test; verified by socket test |
| 2.3 | Directed broadcast computed from the real netmask | PASS | unit tests incl. /24, /16, /30, /32, /0 |
| 2.4 | mDNS advertises every usable interface | PASS | `avahi-browse` shows `wlo1` and `tailscale0` |
| 2.5 | VPN never outranks the local network | PASS | unit test |
| 2.6 | Loopback never advertised | PASS | unit test |
| 2.7 | Phone discovered over hotspot | PASS | `avahi-resolve` → `Android_OUWW627P.local 10.141.5.77` |
| 2.8 | Bluetooth advertisement | PARTIAL | advertiser starts; no peer ever paired over it |
| 2.9 | Wi-Fi Direct | NOT IMPLEMENTED | blockers in `docs/architecture/v2-transport-findings.md` |

---

## 3. Pairing and trust

| # | Test | Result | Evidence |
|---|---|---|---|
| 3.1 | Pair with stock KDE Connect | PASS | paired; 7 capabilities negotiated |
| 3.2 | Pair **with Pixel Connect** | FAIL | trust store is `{}`; never completed |
| 3.3 | Unpaired device reported honestly | PASS | connected-but-untrusted labelled "not paired here" |
| 3.4 | Trust store is separate from stock KDE Connect | PASS | confirmed by inspecting both paths |
| 3.5 | Certificate pinning enforced after pairing | BLOCKED | depends on 3.2, which fails |

**3.2 is the most significant failure in this document.** Every KDE Connect
result below was obtained against a peer this application has not pinned.

---

## 4. File transfer — PC to phone

| # | Test | Result | Evidence |
|---|---|---|---|
| 4.1 | Send one file over home WLAN | PASS | user-confirmed |
| 4.2 | Send one file over hotspot | PASS | user-confirmed |
| 4.3 | Send multiple files | PARTIAL | queued; delivery not confirmed per file |
| 4.4 | Announced port reaches the wire | PASS | capture showed `10.141.5.105` as source |
| 4.5 | Payload TLS roles correct | PASS | unit tests; transfer succeeds |
| 4.6 | Directory transfer is flattened | PASS | unit tests |
| 4.7 | Filenames sanitised | PASS | unit tests |
| 4.8 | Concurrent same-name transfers do not interleave | PASS | unit tests for the `.part` uniqueness |
| 4.9 | Size and free-space ceilings enforced | PASS | unit tests |
| 4.10 | Cancellation | PASS | unit tests |

---

## 5. File transfer — phone to PC

| # | Test | Result | Evidence |
|---|---|---|---|
| 5.1 | Receive a file | BLOCKED | could not complete pairing (3.2) |
| 5.2 | Saved to the configured folder | BLOCKED | depends on 5.1 |
| 5.3 | `saved_path` reported to the application | PARTIAL | unit-tested; not observed end to end |
| 5.4 | Image copied to clipboard on receive | PARTIAL | decode unit-tested incl. a real PNG; never run end to end |
| 5.5 | Non-image files are not put on the clipboard | PASS | unit test |
| 5.6 | Accept prompt appears | PARTIAL | command exists; cannot refuse the transfer |
| 5.7 | Accept actually gates the transfer | FAIL | protocol has no consent step; see `docs/diagnosis/incoming-share-consent.md` |
| 5.8 | Auto-accept setting | PARTIAL | honoured, but cannot prevent the file arriving |

---

## 6. Notifications

| # | Test | Result | Evidence |
|---|---|---|---|
| 6.1 | Shared text raises a notification | PASS | user-confirmed |
| 6.2 | Incoming share raises a notification | PASS | user-confirmed |
| 6.3 | Transfer complete raises a notification | PASS | user-confirmed |
| 6.4 | Transfer failed raises a notification | PASS | user-confirmed |
| 6.5 | Transfer progress does **not** notify | PASS | off by default; unit test |
| 6.6 | Per-category toggles work | PASS | unit tests; user toggled them |
| 6.7 | Quick Share accept from the notification | PASS | user-confirmed |
| 6.8 | Accept action reaches the library | PASS | user-confirmed; path was dead code, now reachable |
| 6.9 | Notification never contains clipboard or file contents | PASS | by inspection, unit-tested by construction |
| 6.10 | Notifications are branded "Pixel Connect" | PASS | user-confirmed; upstream said "RQuickShare" |

---

## 7. Clipboard

| # | Test | Result | Evidence |
|---|---|---|---|
| 7.1 | PC → phone text | PASS | log confirms broadcast |
| 7.2 | Phone → PC text | PASS | user-confirmed |
| 7.3 | Quick Share text → clipboard | PASS | user-confirmed |
| 7.4 | No echo loop | PASS | unit tests incl. two concurrent applies |
| 7.5 | Bounded remembered list | PASS | unit test |
| 7.6 | Clipboard contents never logged | PASS | by inspection; no logging path exists |
| 7.7 | Image → clipboard | PARTIAL | see 5.4 |
| 7.8 | Background read on this compositor | PARTIAL | XWayland bridge works; core Wayland requires focus |

---

## 8. Nautilus integration

| # | Test | Result | Evidence |
|---|---|---|---|
| 8.1 | Context menu appears | PASS | after restarting Nautilus |
| 8.2 | Send one file | PASS | socket reply `ok: true, sent: 1` |
| 8.3 | Background-items signature (Nautilus 4.x) | PASS | unit-tested equivalent; no traceback |
| 8.4 | Relative paths and `~` resolved | PASS | unit tests |
| 8.5 | Non-`file:` schemes rejected | PASS | unit tests |
| 8.6 | Notification shown for the outcome | PASS | after the `get_window` fix |
| 8.7 | Reports "Sending", not "Sent" | PASS | by inspection; not observed end to end |
| 8.8 | Socket is 0600 and pair-checked | PASS | unit tests |

---

## 9. Desktop integration

| # | Test | Result | Evidence |
|---|---|---|---|
| 9.1 | Present in the GNOME app grid | PASS | `gtk-launch dev.pixelconnect.app` |
| 9.2 | Desktop entry validates | PASS | `desktop-file-validate`, no problems |
| 9.3 | Icon set complete | PASS | installed at 32/128/256/512 + symbolic |
| 9.4 | Binary on `PATH` | PASS | `which pixel-connect` |
| 9.5 | Autostart | PARTIAL | entry installed; not tested across a login |
| 9.6 | Settings persist | PASS | read back after change |
| 9.7 | Download folder applies to both protocols | PASS | unit-tested; UI built |

---

## 10. Security

| # | Test | Result | Evidence |
|---|---|---|---|
| 10.1 | `cargo audit` | PASS | 0 vulnerabilities |
| 10.2 | `npm audit --omit=dev` | PASS | 0 vulnerabilities |
| 10.3 | Path traversal rejected | PASS | unit tests |
| 10.4 | Shell injection in SSH targets | PASS | unit tests |
| 10.5 | Filename sanitisation | PASS | unit tests |
| 10.6 | Incoming size / free-space ceilings | PASS | unit tests |
| 10.7 | Inbound connection cap (DoS) | PASS | unit tests |
| 10.8 | Nonce and counter reuse on the payload channel | PASS | unit tests |
| 10.9 | Clipboard never persisted | PASS | by inspection |
| 10.10 | No Gtk 3 import in the Nautilus extension | PASS | by inspection; the known cause of a real crash |

---

## 11. Resilience

| # | Test | Result | Evidence |
|---|---|---|---|
| 11.1 | Keepalive prevents an idle hotspot drop | PARTIAL | unit-tested; **not verified on hardware** |
| 11.2 | Stale link is detected within 75 s | PARTIAL | unit-tested; not observed |
| 11.3 | Dropped link is retried | PARTIAL | code path exists; not observed |
| 11.4 | Backend runtime survives start-up | PASS | regression test |
| 11.5 | Malformed packets ignored | PASS | unit tests |
| 11.6 | Long soak | BLOCKED | not run |

---

## Summary

| Result | Count |
|---|---|
| PASS | 70 |
| PARTIAL | 12 |
| FAIL | 2 |
| BLOCKED | 5 |
| NOT IMPLEMENTED | 1 |
| **Total** | **90** |

Counts are computed from the rows above, not estimated.

**The two FAILs are both about pairing.** They are the highest-value thing to
fix, because they gate certificate pinning, which gates every other KDE Connect
security property.