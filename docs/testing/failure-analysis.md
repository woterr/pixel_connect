# Failure analysis

Every defect found during QA, with the evidence that identified it and what the
fix was. Ordered by severity.

---

## F-01 — KDE Connect identity broadcasts went to loopback

**Severity: critical.** Discovery was completely broken on a phone hotspot.
The phone could not see the computer at all, so pairing was impossible.

### Evidence

Listening on UDP 1716 while the app ran showed every identity packet arriving
from loopback:

```console
from 127.0.0.1:1716  {"type":"kdeconnect.identity","body":{"deviceName":"fedora1",...}}
```

A direct socket test proved the mechanism:

```console
0.0.0.0      -> received from 127.0.0.1
10.141.5.255 -> received from 10.141.5.105
```

On Linux, a datagram sent to the unspecified address is delivered to loopback
only.

### Cause

`discovery::broadcast_addresses()` returned `Ipv4Addr::UNSPECIFIED` as its first
entry.

### Fix

Each usable interface now contributes `255.255.255.255`, its own directed
broadcast, and a unicast to the interface address. The unicast matters because a
hotspot is more likely to filter broadcast frames than a home router.

The netmask is read from the interface rather than assumed: a guessed mask yields
a broadcast address that goes nowhere, which is indistinguishable from a silent
network. `/0` and `/32` are rejected — `/32` because the arithmetic returns the
address itself, which is a unicast, not a broadcast.

After the fix the same capture shows `10.141.5.105: 7 identity packet(s)`.

### Lesson

The application bound port 1716, announced itself, and reported success. Only
observing *who sent the packet* revealed that it had gone nowhere. "Bound and
listening" was not evidence of working.

---

## F-02 — The hotspot link dropped after 30–60 s and never returned

**Severity: critical.** The most disruptive symptom reported.

### Evidence

Device found, paired, connected; transfers worked; then the link died after
30–60 s and only returned after `pkill pixel-connect`.

### Cause

Two defects, one confirmed by inspection:

**No keepalive existed.** `link.rs` set `set_nodelay` and then never sent
anything on an idle control channel. A hotspot drops an idle NAT mapping after
roughly that long, so the socket stopped carrying traffic while both ends still
believed it was open. Nothing noticed, so nothing reconnected.

This also explains why transfers worked: a transfer sends packets
continuously, which refreshes the mapping. The failure only ever appeared while
idle — which is why it looked intermittent.

**Reconnection did not cover dropped links.** `refresh()` runs on a 5 s tick and
re-broadcasts identity, but nothing recorded that a link had existed and failed.

### Fix

Keepalive on the link's `select!` loop: 20 s timer, ping once idle 25 s, link
declared dead after 75 s of silence. No new packet type — the peer already
replies to `kdeconnect.ping`. Tearing the link down is what lets the existing
reconnect path run again.

`LinkContext::report_link_end` now records how a link ended, so a link that
worked and dropped is distinguishable from one that never came up. Without that,
retrying an absent phone becomes a reconnect storm.

### Not verified

The phone was not on the hotspot when this was built. **Unit-tested and
inspected only.**

---

## F-03 — Quick Share's accept notification could never fire

**Severity: high.** A notification existed since V1 and was unreachable.

### Evidence

`app/main/src-tauri/src/quickshare.rs`:

```rust
if msg.direction == ChannelDirection::FrontToLib {
    if msg.state == Some(State::WaitingForUserConsent) { ... notify ... }
}
```

The library's only sender, `inbound.rs::update_state`, hardcodes
`direction: ChannelDirection::LibToFront`. No message could ever satisfy the
condition.

### Cause

The direction test was wrong: the condition should have been on state alone.
The branch had never executed since it was written.

### Fix

Gate on the consent state. Three tests pin it, one asserting the direction the
library actually uses so a future change is noticed rather than silently
re-breaking the path.

### Lesson

Inherited code with a plausible-looking guard can be dead for years. A test
that asserts *how the peer behaves* is worth more than one that asserts what
the handler does.

---

## F-04 — No notifications existed at all

**Severity: high.** The user asked for notifications; there were none.

### Evidence

Both functions in `notification.rs` were unreferenced, and both announced
themselves as **"RQuickShare"** — inherited from upstream rquickshare.

### Cause

The module was copied from upstream, where it is driven by rquickshare's own
event loop. Pixel Connect's event flow never called it.

### Fix

Replaced with a layer covering five events, each independently switchable.
Progress notifications default to **off**: they fire repeatedly during one
transfer, which is the bombardment the user asked to avoid.

### Lesson

Copying a module is not porting it. The functions were correct and reachable in
upstream's architecture, and dead in this one.

---

## F-05 — Nautilus extension raised `AttributeError` on every activation

**Severity: high.** Nothing worked from the file manager.

### Evidence

```text
File "pixel-connect-share.py", line 204, in _on_activate
  _notify(self.get_window(), "INFO" if ok else "ERROR", message)
AttributeError: 'PixelConnectMenuProvider' object has no attribute 'get_window'
```

### Cause

`get_window()` is not a `MenuProvider` method. The send had already completed;
only the notification failed, so failures looked like nothing happening.

### Fix

Pass the provider; `_notify` resolves the window defensively and falls back to
stdout.

---

## F-06 — `get_background_items` treated a single FileInfo as a list

**Severity: high.** The context menu never worked.

### Evidence

```text
TypeError: 'NautilusVFSFile' object is not iterable
```

### Cause

Nautilus 4.x passes a **single** `Nautilus.FileInfo` to
`get_background_items` but a **list** to `get_file_items`. The extension
treated both as a list, so every menu build raised.

### Fix

Normalise either shape.

---

## F-07 — Nautilus changes were not taking effect

**Severity: medium.** I reported this as fixed while it was not.

### Evidence

The journal showed the same traceback at 23:31, minutes after the fix was
written at 23:16.

### Cause

Nautilus started at 23:12:32 and loads extensions **once at start-up**. It never
re-reads them, so the running process kept the old bytecode.

### Fix

Restarted Nautilus.

### Lesson

I declared this fixed without testing it, having only edited the file. Two
corrections in this project came from the same mistake: claiming the hotspot
was fixed after editing mDNS, and claiming the Nautilus fix after editing the
extension. **An edit is not a verification.**

---

## F-08 — `pixel-connect-send` rejected correctly chosen files

**Severity: medium.** *"…jpg" is not a local file.*

### Evidence

The CLI forwarded arguments unchanged. Run from Nautilus's Scripts menu, or
from a shell with a relative path, a valid file was refused.

### Cause

The backend correctly accepts only an absolute path or a `file://` URI. The
CLI did not normalise before sending.

### Fix

Resolve relative paths, expand `~`, unwrap `file://`. The backend stays strict —
refusing an unresolvable path is correct.

---

## F-09 — A connected device was labelled "Not paired"

**Severity: medium.** Reported as a UI bug; it was not.

### Evidence

```json
{}   ← ~/.config/dev.pixelconnect.app/kdeconnect/trusted_devices.json
```

### Cause

The phone is paired with **stock KDE Connect**, which keeps its own trust store.
`is_trusted()` correctly returned false. Transfers still work because the
*phone* tolerates the link; nothing here is pinned.

### Fix

None to the label — it was accurate. A connected-but-untrusted device now says
"not paired here" explicitly, because "Connected · Not paired" read as a
contradiction when it is a true and important statement.

**The real fix is to pair with Pixel Connect**, which is F-11.

---

## F-10 — "Sent 1 item" for a transfer that had not started

**Severity: medium.** Made a failed transfer look successful.

### Cause

`share_files` hands each file to the link and returns before the payload
connection is opened. `sent` counts **queued**, but the UI said "Sent".

### Fix

Now says "Sending", in the extension and the CLI, and points at the notification
that carries the outcome.

---

## F-11 — Pairing with Pixel Connect has never succeeded

**Severity: high, and still open.**

The trust store is empty. Every KDE Connect result in the test matrix was
obtained against an unpinned peer, so certificate pinning has never been
exercised end to end.

Root cause not yet established. F-01 and F-02 each prevented discovery and
stability respectively; both are fixed, and pairing has not been retried since.

---

## Method notes

Three findings came from watching the network rather than reading code:

- **F-01** — the packet source address, not the bind address
- **F-02** — the correlation between idleness and failure
- **F-03** — grepping for what the library actually sends, versus what the
  handler assumed

Two came from the journal rather than the application log, because two failures
occurred in *other* processes:

- **F-05, F-06** — the Nautilus traceback

Two were caused by my own reporting:

- **F-07** — declared fixed after editing, not testing
- **F-09** — investigated as a UI bug when the UI was correct

Both of those were caught by the user reporting that something still did not
work, which is the argument for verifying rather than asserting.