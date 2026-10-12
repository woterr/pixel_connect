# Quick Share engine migration: audit + integration map

Migration branch: `migration/open-quickshare`, from `41b4f6e`.
Reference pins (read-only clones under `/tmp/opencode/ref`, never modified):
- `open-quickshare@5a31145` (2026-09-01) — the migration target.
- `rquickshare-ble@61b9f66` (branch `ble-receiver`).

## 1. Lineage (verified in git, not assumed)

`open-quickshare` **contains** `rquickshare-ble` as an ancestor: fork tip
`61b9f66` is the parent of open's `d5251fa`, plus 59 commits. There is nothing
unique to the BLE fork at these pins — it is a strict subset. **Single
migration target: open-quickshare@5a31145.** No two-source merge is needed.

Attribution chain is preserved in-history (Martichou → martinalderson →
ignotusbucius); both LICENSE files are byte-identical GPL-3.0-or-later.

## 2. What we have vs what open has

Our `core/quickshare` (v0.11.5, edition 2021) is the old rquickshare engine
with our BLE receiver port. Open is ~2× the code:

| File | Ours | Open | Verdict |
|---|---|---|---|
| `blea.rs` | 229 | 770 | port (PSM advert, visibility gating, ghost handling) |
| `ble.rs` | 84 | 272 | port (scan suppressor, duty cycling) |
| `gatt.rs` | 423 | 570 | port (confirm+INTRODUCTION, BWU hooks) |
| `inbound.rs` | 1368 | 2229 | port (do_bwu, drain/ack/swap, fallback ladders) |
| `outbound.rs` | 1255 | 2241 | port (BWU client, chunk sizes, teardown) |
| `lib.rs` | — | — | port (L2CAP server, visibility-gated advertiser) |
| `manager.rs` | — | — | port (3-round scan ladder, SMALL_SEND gate) |
| `ble_client.rs` | — | 569 | NEW (scan/dial/bridge for sending over BLE) |
| `l2cap.rs` | — | 539 | NEW (CoC server, GmsCore dialect) |
| `hotspot.rs` | — | 255 | NEW (Wi-Fi Direct join / hotspot host) |
| `migratable.rs` | — | ~30 | NEW (BLE↔TCP channel switch) |
| `mdns.rs` / `mdns_discovery.rs` / `info.rs` / `mod.rs` | — | — | trivial diffs, port mechanically |
| `channel.rs` | ours | open | **KEEP OURS** (UI contract, see §4) |

Missing entirely from ours: BLE sending (`SendInfo.ble` path), all bandwidth
upgrades (the reason a 395 KB file takes 65 s at 5.94 KB/s over BLE), Wi-Fi
Direct/hotspot, 3 KiB BLE chunking (ours uses 512 KiB unconditionally —
large BLE sends undeliverable), ghost-wait, 6 s connect timeout (ours
unbounded in places), LAN-preference dance, teardown etiquette.

## 3. Where our 25 s goes (code, not speculation)

Open's docs (measured on a Pixel 9 Pro): handshake + upgrade ~1 s;
user-visible 15–20 s is almost entirely **phone-side BLE discovery + the
per-connection GATT service-discovery walk**. Our engine pays all of that
plus its own extras: unbounded TCP connects, a single 20 s scan with no
ghost short-circuit, no L2CAP fast path (GATT weave only), 15 s GATT
handshake waits, and no transport upgrade at all (payload stays on BLE
forever). The 25 s+ figure is consistent with: phone discovery (~15–20 s) +
our sequential waits + slow BLE payload with 512 KiB chunks over a ~4 K MTU.

## 4. Integration boundary (the key decision)

Our Tauri layer (`app/main/src-tauri/src/quickshare.rs`) and frontend
(`types.ts`, store, FilesPage) speak **our** channel protocol
(`ChannelMessage`/`ChannelAction`/`State` with ts-rs bindings). Open speaks
`Message::{Lib,Client}`/`TransferAction`/`TransferState`. Rewriting the
frontend integration doubles the migration for no protocol benefit.

**Decision: replace the engine, keep our public surface.** Port open's
`hdl/*` + `lib.rs` + `manager.rs` internals into `pixel_connect_quickshare`
(crate name kept — no Cargo churn), preserving `QuickShareService`,
`ChannelMessage`, `State`, `SendInfo`, `Visibility` as the UI contract, with
a thin translation at the channel boundary (open's `Message::Client` →
our `ChannelMessage`). New capabilities surface additively
(`SendInfo.ble`, transport display, PSM plumbing).

## 5. Licensing

- Open: GPL-3.0-or-later (LICENSE, 674 lines; `Cargo.toml` has no license field).
- Ours: crate declares AGPL-3.0; root LICENSE is GPL-3.0; NOTICE.md +
  repo-audit.md record the rquickshare derivation.
- AGPL-3.0 §13 permits combining GPL-3.0 code into the project; direction is
  compatible. Preserve copyright headers, keep NOTICE.md updated with the new
  source + commit hash, do not change the project license.
- Open's `proto_src/*.proto` are Apache-2.0 (Google 2020) — already the case
  in our tree (same files).

## 6. What is NOT changing

KDE Connect (`core/kdeconnect`) — discovery, pairing, certs, trust, links,
clipboard, notifications — untouched. Frontend except additive Quick Share
capability surfacing. Settings/persistence, ADB/scrcpy, Nautilus, SSH,
branding, security controls. No second engine in production; no simultaneous
Bluetooth ownership (port first, cut over, delete dead code last).

## 7. Port order

1. `migratable.rs`, `channel` boundary shim (translation layer).
2. `lib.rs` (lifecycle) + `blea.rs` (advert) + `gatt.rs` (session).
3. `l2cap.rs` + `ble_client.rs` (CoC + dial).
4. `inbound.rs` / `outbound.rs` (BWU, framing, teardown).
5. `manager.rs` + `hotspot.rs`.
6. Wire-up, LAN parity, BLE timing comparison, Direct/fallback, KDE regression,
   dead-code removal, docs.
