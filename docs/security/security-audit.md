# Pixel Connect — security audit

Findings from reading the implementation, ordered by severity. Each entry says
what was checked, what was found, and how confident the finding is.

**This is not a claim that the application is secure.** It is a record of what
was examined on the `security-audit` branch and what was changed. Section 5
lists what was *not* verified and why.

Severity reflects realistic exploitability against the adversary model in
[`threat-model.md`](threat-model.md): an attacker on the same Wi-Fi, or a
compromised paired phone. An attacker who already runs code as this user is out
of scope, since nothing an application does can stop them.

| ID | Severity | Finding | Status |
|---|---|---|---|
| F-01 | High | Incoming transfers could exhaust the disk | Fixed |
| F-02 | Medium | Truncated transfers were reported as complete | Fixed |
| F-03 | Medium | Concurrent transfers of one name corrupted each other | Fixed |
| F-04 | Medium | Unbounded connection accept allowed task exhaustion | Fixed |
| F-05 | Medium | Privileged but unused Tauri shell permission | Fixed |
| F-06 | Low | Newlines in filenames allowed log forging | Fixed |
| F-07 | Low | SSH username could inject an option | Fixed |
| F-08 | Low | Clipboard polling caused unbounded log growth | Fixed |
| F-09 | Low | IPC request read was unbounded | Fixed |
| F-10 | Low | RPM install created root-owned files in the user's home | Fixed |
| F-11 | Informational | Five unmaintained or unsound transitive dependencies | Accepted, documented |
| F-12 | Informational | Dead `let _ = pinned;` in the outgoing connection path | Fixed |

---

## 1. High

### F-01 — Incoming transfers could exhaust the disk

**Where:** `core/kdeconnect/src/link.rs`, the payload receive loop.

**What was checked.** The loop read from the TLS stream in 256 KiB chunks into
a `tokio::fs::File` until `Ok(0)`, with no ceiling on `total` and no check of
the announced size against the free space on the target filesystem.

**Finding.** `total` comes from the peer's `totalPayloadSize` field and is
attacker-controlled. A peer — including a *paired* peer whose phone has since
been compromised — could announce and stream an arbitrarily large file until the
volume was full. The user-visible symptoms (desktop unable to write logs, swap
failing, the session becoming unusable) are severe and the action needed is
recovery from a full root filesystem.

The fact that the peer must be paired limits who can do it, but does not
mitigate it: a paired phone is a routine compromise target precisely because
pairing is long-lived and the phone is rarely re-imaged.

**Fix.** `core/kdeconnect/src/transfer_guard.rs` now checks three independent
things:

1. **Volume.** An announced size above the ceiling is refused before anything is
   written. The default is 8 GiB — generous enough that ordinary transfers are
   unaffected — and it is configurable via `maxIncomingBytes` in
   `kdeconnect/config.json`.
2. **Free space.** `statvfs(2)` reports the space available to an unprivileged
   user, and a 256 MiB reserve is held back so the desktop can still write its
   own logs and swap.
3. **Volume, again, while streaming.** A peer may announce a small size and
   then keep sending, so the ceiling is also enforced incrementally by
   `DownloadGuard::record`.

When free space cannot be determined — some network and fuse mounts — the
transfer is *not* refused, because refusing would break legitimate transfers on
filesystems `statvfs` does not describe. The volume ceiling remains the backstop.

**Tests.** 13 unit tests in `transfer_guard.rs`, including that a peer sending
more than it announced is stopped mid-stream, that a peer announcing
`u64::MAX` cannot wrap the byte counter into a passing state, and that a zero
announced size means "unknown" rather than "empty".

---

## 2. Medium

### F-02 — Truncated transfers were reported as complete

**Where:** `core/kdeconnect/src/link.rs`, after the receive loop.

**What was checked.** On a clean end-of-stream (`Ok(0)`) the temporary file was
renamed into place and a `TransferProgress` with `done: true` and no error was
emitted, without comparing the bytes received against the bytes announced.

There *was* handling for a socket reset, but it only applied when the connection
failed; the ordinary truncation case fell through.

**Finding.** A file that arrived short was presented to the user as intact. The
consequences depend on the file: a truncated archive is a nuisance, a truncated
document or photo is silently wrong, and a partially written executable is worse
than either.

**Fix.** `DownloadGuard::is_complete()` compares the received count against the
announced count. A short transfer now deletes the partial file and reports an
error naming both sizes, so the user learns the transfer failed rather than
opening a corrupt file. A peer that announced no size is never treated as short,
because there is nothing to compare against.

### F-03 — Concurrent transfers of one name corrupted each other

**Where:** `core/kdeconnect/src/link.rs`, temporary file construction.

**What was checked.** `let tmp = dest.with_extension("part");`

**Finding.** The temporary path was derived from the *destination*, and
`unique_destination` only avoids collisions against files that already exist.
Two transfers of `photo.jpg` starting at the same moment — from two paired
phones, or retried quickly — therefore computed the same `photo.jpg.part` and
wrote into one file concurrently. The result was two interleaved files that
each appeared to transfer successfully.

**Fix.** The temporary name is now allocated through `unique_destination` and so
is unique per attempt. Test: `concurrent_transfers_of_one_name_get_distinct_temporaries`.

### F-04 — Unbounded connection accept allowed task exhaustion

**Where:** `core/kdeconnect/src/lan.rs`, `accept_loop`.

**What was checked.** The loop called `listener.accept().await` and
`tokio::spawn`ed a task per connection with no limit.

**Finding.** Every KDE Connect peer on a LAN dials every port in 1716–1764, and
an attacker can dial arbitrarily many more. Each accepted connection reaches a
TLS handshake, which is asymmetric cryptography and therefore deliberately
expensive to force. There was also no timeout on establishing a link, so a peer
could open a socket, send a valid identity packet, and then say nothing.

Crucially, the device's own id is broadcast on the LAN in both the UDP
discovery packet and the mDNS TXT record, so an attacker knows exactly which
`deviceId` to claim in the identity packet.

**Fix.** A `tokio::sync::Semaphore` of 24 permits bounds concurrency.
`try_acquire_owned` is used rather than `acquire`, so a flood of connections is
refused immediately instead of accumulating as waiting tasks — which would
reproduce the very problem the limit exists to prevent. A handshake that has not
completed within 30 seconds is abandoned and its permit released.

24 is deliberately generous: a busy network legitimately has several peers.

### F-05 — Privileged but unused Tauri shell permission

**Where:** `app/main/src-tauri/capabilities/migrated.json`, `src/main.rs`,
`Cargo.toml`.

**What was checked.** The capability granted `shell:allow-open` and the builder
registered `tauri_plugin_shell::init()`. Grepping the front end for any import
of the shell plugin found nothing.

**Finding.** `shell:allow-open` lets the webview hand an arbitrary file path or
URL to the desktop's default handler. The front end never used it, so this was
privilege with no purpose. It matters because the webview is the *low*-trust
component of a Tauri application: it renders device names, notification text and
filenames that a remote peer controls. Any future injection sink would inherit
this permission for no design reason.

**Fix.** The plugin is removed from the builder, from the capability file, from
`Cargo.toml`, and from `Cargo.lock`. The remaining capability set is unchanged.
The CSP was already restrictive and is left as it was — loosening it to keep a
removed plugin working would have been the wrong direction.

---

## 3. Low

### F-06 — Newlines in filenames allowed log forging

`core/kdeconnect/src/link.rs`, `sanitize_filename`.

The function stripped `/`, `\`, NUL and `:`, but let `\r` and `\n` through. A
peer could send a filename containing a newline and inject fabricated lines into
the log, where filenames appear in `info!` and `debug!` calls. This corrupts
incident response: an attacker can make a failed transfer look like a successful
one, or hide one entirely.

Fixed by stripping **all** control characters, not just NUL. A name consisting
only of dots (`....`) is now rejected as well, since `unique_destination` treats
it ambiguously. Covered by `every_traversal_and_injection_shape_is_stripped` and
`nuls_and_control_characters_are_removed`.

Note that this test failed on the first run against the original code — the fix
came from the test, not the other way round.

### F-07 — SSH username could inject an option

`app/main/src-tauri/src/ssh.rs`, `validate`.

The username was checked for whitespace, NUL, `:`, `@`, `/` and `\` — all
characters relevant to shell splitting — but not for a leading `-`. The
destination is passed to ssh as `user@host`; ssh parses any argument beginning
with `-` as an option. A username of `-oProxyCommand=...` would therefore have
been read as an option rather than as part of the hostname, and
`ProxyCommand` executes a command locally.

This requires the user to type the value into their own SSH target settings, so
it is not remotely reachable, but it is a genuine argument-injection defect and
the fix is free. Usernames may no longer contain `-`, `=` or `?`.

The host was checked separately and is safe by construction: it is always
appended to the username, so the combined argument can never begin with a dash.

Host key checking was **already correct** — `StrictHostKeyChecking=accept-new`,
which records a new host but still rejects a changed key. This is now pinned by
a test so it cannot be weakened silently.

### F-08 — Clipboard polling caused unbounded log growth

`app/main/src-tauri/src/clipboard.rs`.

The background watcher polls every 600 ms and constructed a fresh
`arboard::Clipboard` each time. On a session with no data-control protocol — the
normal case on GNOME, see
[`v2-status.md`](../architecture/v2-status.md) §2 — every construction logged a
warning about falling back from Wayland to X11.

The consequence was visible in the live run log: identical warnings roughly
1,400 times an hour, indefinitely, from a condition that never changes. That is
a denial of service against the log store, and it buries real diagnostics.

Fixed by holding one clipboard handle for the process lifetime. The warning now
appears once at start-up, where it is actually informative.

### F-09 — IPC request read was unbounded

`app/main/src-tauri/src/nautilus.rs`.

`read_line` on the file-manager socket had no ceiling. The socket is mode 0600
inside a mode 0700 directory, so only this user's own processes can reach it,
which makes this low severity. It is bounded at 4 MiB anyway, since a
legitimate request — 512 paths at 4096 bytes — is far smaller.

### F-10 — RPM install created root-owned files in the user's home

`packaging/rpm/pixel-connect.spec`, `%post`.

The scriptlet runs as root and created `~/.local/share/nautilus/scripts/…` and
the Python extension symlink directly, leaving root-owned entries in a user's
home directory. The user could not update or replace them without sudo, which is
surprising on a desktop and is a needless use of privilege.

Fixed by performing the creation as the user via `runuser` (with a `su`
fallback). The integration itself was never root-only and remains reversible.

### F-12 — Dead `let _ = pinned;`

`core/kdeconnect/src/lan.rs`, `handle_outgoing`.

The outgoing path fetched `trusted_certificate_der` and immediately discarded
it. **This was not a vulnerability**: `run_link` fetches the pinned
certificate again and enforces it before publishing the link, so pinning was
applied. But a line that reads "fetch the peer's pinned certificate, then throw
it away" is exactly the kind of thing a future edit could mistake for a comment,
and it invites the reader to believe the outgoing path is unpinned. Removed.

---

## 4. Dependency review

`cargo audit` against the committed lockfile:

```
Vulnerabilities: 0
Warnings: 5
```

All five are transitive and are recorded rather than suppressed. None has a
compatible fix that does not require a broad upgrade.

| Crate | Advisory | Kind | Why it is acceptable here |
|---|---|---|---|
| `gcc` 0.3.2 | RUSTSEC-2025-0121 | unmaintained | Build-time only. Never linked into the shipped binary. |
| `mach` 0.3.2 | RUSTSEC-2020-0168 | unmaintained | macOS-only. Not reachable on Fedora. |
| `proc-macro-error` 1.0.4 | RUSTSEC-2024-0370 | unmaintained | Build-time only, compile-time code generation. |
| `rustls-pemfile` 2.2.0 | RUSTSEC-2025-0134 | unmaintained | Reached through `tokio-rustls`. This project does not use PEM trust stores; its certificates are pinned by DER. |
| `glib` 0.18.5 | RUSTSEC-2024-0429 | unsound | Pulled in by GTK bindings. The unsound `VariantStrIter` iterators are not used by this application. |

None is a scoring input for an attacker of this application's threat model: no
code execution, no memory-safety issue reachable from a network peer, and no
path to a secret. They are recorded so the decision is deliberate and can be
revisited — the trigger for revisiting is any of them acquiring a fix that can
be taken without a major-version jump across the Tauri or GTK stack.

No crates were added for security purposes. `libc` was used for `statvfs(2)`
because it is already in the dependency tree transitively, rather than
introducing a new package for one call.

### 4.1 Front-end dependencies

`npm audit` initially reported **18 advisories, including 2 critical**. The
critical ones were `tinypool` (prototype pollution leading to RCE) and
`vitest`'s `@vitest/mocker`; the high ones included `vite` (path traversal in
optimised dependency `.map` handling), `postcss`, `@vue/server-renderer` (XSS via
a missing CR in the attribute-name blacklist), `tailwindcss` and
`unplugin-auto-import`.

Three had a non-breaking fix, which was applied:

| Package | From | To | Advisory cleared |
|---|---|---|---|
| `vue` | 3.5.12 | 3.5.43 | XSS via `@vue/server-renderer` |
| `postcss` | 8.4.47 | 8.5.29 | XSS via unescaped `</style>` in stringify output |
| `vitest` | 2.1.4 | 2.1.9 | — |

The versions were pinned exactly, which is why `npm audit fix` had nothing to
do; they had to be raised deliberately.

`@tauri-apps/plugin-shell` and `@tauri-apps/plugin-opener` were also removed
from `package.json`. Neither is imported by the front end, neither is registered
in the Rust builder, and neither appears in `capabilities/migrated.json`. A
package that is installed but unreachable is one more thing to audit and one
more supply-chain surface for no benefit.

**The result that matters:**

```
$ npm audit --omit=dev
found 0 vulnerabilities
```

The 15 advisories that remain are all in `devDependencies` — the test runner
and the CSS/build toolchain — and none reaches the shipped artifact. This was
verified rather than assumed: the flagged package names appear nowhere in
`dist/assets/*.js`, and the production dependency set is `@tauri-apps/api`, five
Tauri plugins, `pinia` and `vue`.

They are accepted with this reasoning:

| Package | Severity | Why accepted |
|---|---|---|
| `vitest`, `tinypool` | critical | Test runner. Never present in a release build and unreachable by an attacker. |
| `vite` | high | The advisory affects the **dev server**, not the static bundle. The shipped application loads a pre-built `dist/`. |
| `tailwindcss`, `unplugin-auto-import`, `braces`, `chokidar`, `fast-glob`, `micromatch`, `unimport` | high | Build-time only. Fixing them needs major migrations across Tailwind 3→4 and Vite 5→8 — a large, unrelated change that should not be smuggled into a security fix. |

CI now gates on `npm audit --omit=dev --audit-level=moderate`, so a *new*
advisory in shipped code fails the build while the accepted build-tooling
advisories stay documented rather than suppressed.

---

## 5. What was not verified

Stated plainly, because the difference between "reviewed" and "tested" matters
here.

1. **No real Pixel was attached at any point.** Every statement about the
   behaviour of a *legitimate* phone — whether the Android KDE Connect app
   accepts the certificate pinning, whether it interoperates with the new
   transfer limits, whether notification reply works end to end — is
   unverified against a device. The wire-format tests use packets built from the
   upstream source, which is strong evidence about the format but not proof of
   interoperation.
2. **No penetration testing of the running binary.** Findings come from reading
   the source and from tests, not from attacking a live process.
3. **The transport limits were not exercised against a hostile peer.** They were
   verified by unit tests over the decision logic. The wiring that calls them is
   covered by the existing integration test, which still passes, but a
   deliberately malicious sender was never run against the application.
4. **No fuzzing.** The packet parser is bounded and tested, but no coverage or
   property-based fuzzing campaign was run. Given that any device on the LAN can
   reach the parser, this is the most valuable remaining testing investment.
5. **The Android side was not audited** because there is no companion
   application to audit.

---

## 6. Findings reviewed and deliberately not changed

Recording these matters as much as the fixes: an audit that only lists problems
gives no picture of the judgement involved.

- **`0.0.0.0` binds on the KDE Connect and Quick Share listeners.** Correct and
  required — a phone on the same network has to be able to connect. Binding to
  loopback would break the product.
- **Clipboard content is held in memory** without encryption. Encryption in
  process memory buys nothing against an attacker who can already read the
  process.
- **`kdeconnect/config.json` stores the trust list in plaintext**, mode 0600.
  Moving it to the Secret Service would protect a file that is only as sensitive
  as the key sitting next to it in the same directory. Not a meaningful
  improvement.
- **Self-signed certificates.** Inherent to KDE Connect and to its Android
  counterpart. The application's answer is pinning plus CN binding, which is
  strictly stronger than what upstream does.
- **Downloads are not scanned or sandboxed.** Received files land in a
  user-chosen directory with the user's ordinary permissions, exactly as they
  would from a browser. The application never executes or auto-opens what it
  receives.
- **`kdeconnect.notification.dismiss` does not exist.** Discovered during V2
  that neither implementation defines such a packet, and it was removed rather
  than shipped as an invention that would break interoperation.
- **No rate limiting on pairing requests.** Deferred. The pairing exchange is
  cheap and the 30-second handshake timeout plus 24-connection ceiling bound the
  cost; a dedicated pairing-rate limiter would be the natural next step but has
  no measured abuse case yet.