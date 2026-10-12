# Connection state: root cause

**Symptom.** The phone was connected and working, but the UI said otherwise:
sidebar "Not reachable" (grey), Files "isn't reachable", Devices "Not
reachable", while Notifications showed a green dot and live items.

**Root cause: two independent defects, one backend and one frontend.**

## Backend: failed handshakes clobbered live link state

`establish_incoming` (`core/kdeconnect/src/lan.rs`) published `Connecting`
for a trusted peer *before the TLS handshake ran*. Every failure path after
that point — TLS error, or the refused-replacement-socket return — left the
record at `Connecting` and never restored it, even when the peer's existing
link was healthy and still delivering packets. `handle_outgoing` had the same
shape: `Connecting` before the dial, stuck there on failure.

Concretely: the phone opens a fresh connection (normal after network changes
or our restarts); we publish `Connecting`, then refuse the socket because a
link already exists; the record now says unreachable while the old link keeps
working — including delivering the notifications the user could see arriving.

**Fix (minimal):** no `Connecting` is published before a handshake exists.
Every failure path calls the new `LinkContext::republish_link_state`, which
derives the state from the live link map and nothing else — `Connected` iff a
link sender exists, else `Disconnected`. This matches upstream, where
`Device::isReachable()` is `!m_deviceLinks.isEmpty()`, kept separate from
paired state. No wire, pairing, certificate, plugin or Quick Share code
changed.

## Frontend: the Notifications dot was hardcoded green

The group header dot was `class="... bg-online"` unconditionally, so a
disconnected phone looked connected there while every other screen said
otherwise. It now reads the same device record (`linkState === 'Connected'`)
as the sidebar, Files hero and Devices cards. One model, four readers.

## Hypothesis log

1. **Stale or one-shot status — CONFIRMED (backend half).** The state was
   written pre-handshake and never recomputed on failure. The frontend half
   (polling races) was not observed.
2. **Wrong signal (heartbeat/battery-gated reachability) — REJECTED.** No
   path derives reachability from battery or heartbeats; battery `None`
   renders as unknown, not 0%. The "0%" in the report predates that guard.
3. **Identity mismatch / duplicate records — REJECTED.** The peer id comes
   from the pre-TLS identity packet and is verified against the in-TLS
   identity (`peer changed device id mid-handshake` is an error); one record
   per id exists in the map.
4. **Duplicate-link flap — REJECTED for the current policy.** With refusal
   (no adoption) two links for one peer cannot coexist (`links.entry`
   stands down), so an old link ending cannot remove a newer link's sender.
   The *visible* flap was hypothesis 1's stuck `Connecting`.
5. **Send path vs control link / cached notifications — REJECTED.** `timeAgo`
   renders real timestamps ("now" only <5s old), so notifications showing
   "now" were arriving live over a working link — consistent with the backend
   finding, not a cache. Quick Share sends are a separate stack and were
   never implicated.
6. **Phone side — UNVERIFIED.** The phone's own device list was not
   inspected in this pass; if it shows the PC disconnected, that is a
   separate fact worth reporting.

## Evidence

- `core/kdeconnect/src/lan.rs`: `republish_link_state` + failure-path calls.
- `app/main/src/components/NotificationsPage.vue`: `groupOnline()`.
- Tests: `lan::tests::link_state_replay` (4 tests: live→Connected,
  absent→Disconnected + transport cleared, connect/refuse/close flap replay,
  unknown peers gain no record). Full suite: 94 kde + 47 backend + 22
  quickshare + 29 frontend, all passing.
- Upstream references: `kdeconnect-kde` `lanlinkprovider.cpp::addLink`
  (cert-gated `reset()`, reachability untouched), `device.cpp::isReachable`,
  android `LanLinkProvider.kt::addOrUpdateLink` + `onConnectionLost`
  (conditional remove). Cloned 2026-10-12 to /tmp (scratch; not vendored).

## Still needs the phone in hand

A live link event sequence (UDP → TCP → TLS → identity → pairing → close with
reason) against the new build, confirming all four surfaces flip together
both ways. That is acceptance item 1 and cannot be done without the device.
