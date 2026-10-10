# Incoming shares cannot be refused: KDE Connect has no consent step

**Status: protocol limitation, documented not "fixed".** Found 2026-10-11.

## The request

Accept or reject a file sent from the phone, with an auto-accept setting.

## What actually happens today

KDE Connect v8's share protocol has **no accept/reject round trip for a
received file**. The sender announces the payload, and the receiver opens a
connection and writes the bytes immediately.

Android does the same when it receives — there is no consent branch in
`SharePlugin.onPacketReceived` either. The desktop side had no gate at all, so
`spawn_payload_receiver` writes to `incoming_path` the moment the payload
arrives. By the time a `ShareOffer` reaches the UI, the file is already on disk.

That is why the app could only ever offer *Dismiss*: there was nothing to
decline. The button that looked like a choice did not exist.

## What was implemented

- `kde_accept_share` records the decision and clears the request.
- The Files page now shows **Accept** and **Dismiss** instead of **Review**,
  which navigated to the Devices page and did nothing.
- Notifications carry **Accept** / **Reject** actions.
- `auto_accept_shares` skips asking, and defaults **off**.

`auto_accept_shares` is deliberately described in Settings as changing whether
a request is *acknowledged* automatically, **not** whether the file arrives. It
is not a security control, and presenting it as one would be a lie: the file
arrives either way.

## What would actually gate it

Two options, both requiring changes the current pairing partner cannot make.

**Answer the protocol — two extra packets.** Upstream KDE Connect has no
accept/reject packet for an incoming share, so there is nothing to honour. Adding
one means this implementation can only gate shares from another Pixel Connect;
stock `org.kde.kdeconnect_tp` would still send without waiting, because it has
no code to wait.

**Defer the payload instead of the consent.** Announce the offer, hold the
connection unopened, and only dial when the user accepts. This works against
stock Android — the sender waits, and the receiver controls when it connects.
It costs a modified sender and a timeout, because the sender will eventually
give up.

Neither is in place. Until one is, an incoming file is saved to the incoming
folder without asking, and the Accept button is a record rather than a gate.

## Related

- [`nat-payload-failure.md`](nat-payload-failure.md) — file transfer failing
  because the phone cannot reach a reversed connection through the NAT.