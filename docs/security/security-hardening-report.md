# Pixel Connect — security hardening report

What was audited, what was changed, what was tested, and what remains.

Companion documents:
[`threat-model.md`](threat-model.md) — what is protected and from whom.
[`security-audit.md`](security-audit.md) — the findings themselves, with the
evidence for each.

Branch: `security-audit`, built on `v2`.

---

## 1. Summary

The audit covered the KDE Connect protocol implementation, the Tauri backend and
front end, the local IPC layer, external process invocation, packaging and the
dependency tree.

**Twelve findings. One high, four medium, six low, one informational. All are
fixed or, in the case of the dependency advisories, documented and accepted.**

The most important result is the one that was *not* a finding: the pairing and
authorisation model is sound. Certificate pinning is applied before a link is
published, the CommonName is bound to the announced device id, and a peer
claiming a trusted identity while presenting a different certificate is
rejected. That code came from V1 and was verified, not rewritten.

The real problems were concentrated in the file-receive path, which trusted the
peer to be reasonable about *how much* it sent and *whether it finished*.

| Severity | Count | Fixed |
|---|---|---|
| High | 1 | 1 |
| Medium | 4 | 4 |
| Low | 6 | 6 |
| Informational | 1 | 1 |

---

## 2. What changed

### 2.1 Incoming transfers (F-01, F-02, F-03)

**New file:** `core/kdeconnect/src/transfer_guard.rs`

A peer is trusted to *offer* a file; it is not trusted about how large that file
is or whether it arrives whole. Three separate checks now enforce that:

- a size above the ceiling is refused **before** anything is written (default
  8 GiB, configurable via `maxIncomingBytes`);
- `statvfs(2)` free space is checked with a 256 MiB reserve held back, so a
  transfer cannot leave the desktop unable to write its own logs or swap;
- the ceiling is enforced **while streaming**, because a peer may announce a
  small size and then keep sending.

A stream that ends cleanly having sent fewer bytes than announced is now a
**failure**: the partial file is deleted and the user is told both sizes, rather
than a corrupt file being renamed into place and reported as intact.

The temporary file is now unique per attempt. Previously it was derived from the
destination with `.with_extension("part")`, so two transfers of `photo.jpg`
starting together shared one temporary file and interleaved their bytes.

### 2.2 Network (F-04)

`core/kdeconnect/src/lan.rs` now holds a `Semaphore` of 24 permits for inbound
connections, with a 30-second timeout on establishing a link.

This matters because the device's own id is broadcast on the LAN in both the UDP
discovery packet and the mDNS TXT record — an attacker knows exactly which
`deviceId` to claim, and each accepted connection reaches a TLS handshake, which
is asymmetric cryptography and therefore expensive to force.

`try_acquire_owned` is used deliberately: waiting on a permit would let a flood
accumulate as waiting tasks, reproducing the problem the limit exists to solve.

### 2.3 Front-end isolation (F-05)

`tauri-plugin-shell` was registered and granted `shell:allow-open` while the
front end never imported it. Removed from the builder, the capability file,
`Cargo.toml` and `Cargo.lock`. The CSP was already restrictive and was **not**
loosened to compensate — that would have been the wrong direction.

### 2.4 Filenames (F-06)

`sanitize_filename` stripped NUL and `:` but let `\r` and `\n` through, letting a
peer forge log lines with a crafted filename — which corrupts incident response
by making a failed transfer look successful. It now strips **all** control
characters, and a name made only of dots is rejected.

This one was found by a test that failed against the original code.

### 2.5 Process invocation (F-07)

The SSH username was validated for shell metacharacters but not for a leading
`-`. Because the destination is passed as `user@host` and ssh parses a leading
dash as an option, `-oProxyCommand=…` would have been read as an option rather
than as part of the hostname — and `ProxyCommand` runs a command locally.

Usernames may no longer contain `-`, `=` or `?`. The host was already safe by
construction: it is always appended to the username, so the combined argument
can never begin with a dash.

Host key checking was **already correct** (`StrictHostKeyChecking=accept-new`)
and is now pinned by a test so it cannot be weakened silently.

### 2.6 Local IPC and packaging (F-09, F-10)

- The Nautilus socket read is bounded at 4 MiB. Low severity — the socket is
  mode 0600 in a mode 0700 directory — but a bound is free.
- The RPM `%post` scriptlet ran as root and created root-owned files in the
  user's home directory. It now performs the creation as the user via `runuser`,
  with a `su` fallback.

### 2.7 Logging (F-08)

The clipboard watcher built a fresh `arboard` handle every 600 ms. On GNOME,
where no data-control protocol exists, every construction logged a fallback
warning — about 1,400 identical lines an hour, indefinitely, burying real
diagnostics. One handle is now held for the process lifetime.

This one was visible in the live run log before the audit began.

### 2.8 Code clarity (F-12)

`let _ = pinned;` in the outgoing connection path fetched the peer's pinned
certificate and discarded it. **Not a vulnerability** — `run_link` fetches and
enforces it — but it reads exactly like a comment claiming the path is unpinned.
Removed and replaced with an explanation.

---

## 3. Tests added

All new tests assert *behaviour*, not just that code runs.

### Transfer safety — `transfer_guard.rs` (13 tests)

| Test | What it proves |
|---|---|
| `a_transfer_over_the_ceiling_is_refused_with_both_sizes` | The refusal names both the announced size and the limit. |
| `a_file_larger_than_the_free_space_is_refused` | Disk exhaustion is refused, not merely bounded. |
| `the_default_ceiling_is_generous_enough_for_real_files` | A 4K video is allowed; a 32 GiB file is not. The ceiling is not so tight it breaks real use. |
| `unknown_free_space_does_not_refuse_by_itself` | A filesystem `statvfs` cannot describe does not block every transfer. |
| `the_guard_stops_a_peer_that_sends_more_than_it_announced` | Under-announcing is caught *while streaming*. |
| `the_guard_stops_a_peer_that_ignores_the_ceiling` | The ceiling holds mid-stream, not only up front. |
| `a_truncated_stream_is_not_complete` | A short transfer is never reported as whole. |
| `byte_counts_cannot_overflow_into_a_pass` | A peer announcing `u64::MAX` cannot wrap the counter into a passing state. |
| `a_announced_size_of_zero_means_unbounded_not_empty` | Peers that send `filesize: 0` to mean "unknown" are not broken. |

### Path safety — `link.rs` (4 tests)

| Test | What it proves |
|---|---|
| `every_traversal_and_injection_shape_is_stripped` | 14 hostile spellings — `../`, `..\`, absolute, `....//`, Windows drive, mixed separators — each reduce to a plain name, asserted by joining and checking it stays under the target directory. |
| `nuls_and_control_characters_are_removed` | NUL and newline cannot survive into a filename (log forging). |
| `a_dangling_symlink_does_not_redirect_the_destination` | `Path::exists` follows symlinks, so a dangling one reports "missing" and would be chosen as the destination. The link is not followed and its target is unmodified. |
| `concurrent_transfers_of_one_name_get_distinct_temporaries` | The `.part` collision is closed. |
| `an_overlong_name_is_bounded` | Names are capped at 200 characters. |

### Process invocation — `ssh.rs` (4 tests)

| Test | What it proves |
|---|---|
| `rejects_a_username_that_would_be_read_as_an_option` | Six option-injection shapes are refused. |
| `an_injected_username_never_reaches_the_command_line` | Even if validation were bypassed, the destination never begins with a dash. |
| `host_key_verification_is_never_disabled` | No `StrictHostKeyChecking=no`, no `UserKnownHostsFile=/dev/null`. |
| `the_probe_never_asks_for_a_password` | `BatchMode=yes`, and the remote command is the literal `true`. |

---

## 4. Verification

### Automated

| Suite | Before | After |
|---|---|---|
| KDE Connect unit | 47 | **65** |
| KDE Connect integration | 6 | 6 |
| Notification wire format | 7 | 7 |
| Tauri backend | 23 | **27** |
| Front end | 29 | 29 |
| **Total** | **112** | **134** |

All pass. `vue-tsc --noEmit` is clean. `cargo clippy --all-targets` reports
**0 warnings** across the KDE Connect crate, including with `-D warnings`.

### Dependencies

```
$ cargo audit --file app/main/src-tauri/Cargo.lock
Vulnerabilities: 0    Warnings: 5

$ npm audit --omit=dev
found 0 vulnerabilities
```

The five Rust warnings and fifteen npm advisories are all in build or test
tooling; each is listed with its justification in
[`security-audit.md`](security-audit.md) §4. None is suppressed — `cargo audit`
fails on any new advisory, and `npm audit` gates on production dependencies
only.

### Interoperability preserved

This was the constraint that mattered most: no security change may break a real
phone.

- The 6 integration tests, which pair two real service instances over real TLS
  and exchange a real file, still pass.
- **The protocol itself was not touched.** Pinning, the packet format, the
  pairing exchange and the payload handshake are unchanged. The transfer limits
  act on a file the peer has *already* been authorised to send; they do not
  alter the conversation.
- The one place where a protocol-adjacent change was tempting — adding a
  `kdeconnect.notification.dismiss` packet during V2 — had already been caught
  and removed, because neither implementation defines it.

### Not verified

Stated plainly rather than implied:

1. **No real Pixel was attached at any point.** Every claim about a legitimate
   phone's behaviour is unverified against a device.
2. **No penetration test of the running binary.** Findings come from reading the
   source and from tests.
3. **The new limits were not exercised against a deliberately hostile sender.**
   They were verified by unit tests over the decision logic; the wiring is
   covered by the existing integration test, which still passes.
4. **No fuzzing.** The packet parser is bounded and tested, but no property-based
   or coverage-guided campaign was run. Since any device on the LAN can reach
   the parser, this is the highest-value remaining investment.

---

## 5. Remaining risks

### Accepted

| Risk | Why accepted |
|---|---|
| Paired peers can still send up to 8 GiB and fill a nearly-full disk | A paired peer is trusted by design, identically in upstream. The ceiling and free-space check bound the damage rather than eliminating it. |
| Clipboard contents are readable by any paired device | This is the feature. It is bounded by pairing, can be switched off, and is never logged or persisted. |
| Filename collisions are resolved by appending ` (1)` rather than asking | Matches upstream behaviour; overwriting a received file would be worse. |
| Build-tooling advisories remain | Fixing them needs Tailwind 4 and Vite 8 migrations, unrelated to security. None reaches a user. |
| No rate limiting on pairing requests | The exchange is cheap and the connection ceiling bounds the cost. Deferred deliberately. |

### Not mitigated — and not mitigable here

- **Malware running as this user** can read the private key, every user file, and
  the IPC socket. No application-level control helps; this is what OS account
  isolation and Flatpak are for.
- **A compromised phone.** Pairing is long-lived and phones are rarely
  re-imaged, so a paired device stays trusted. The transfer limits bound what
  such a device can do to the filesystem; they do not revoke its access.
- **Traffic analysis.** KDE Connect announces the device id, device name, type
  and capabilities in cleartext over the LAN by protocol design. Pinning
  protects payloads, not discovery.

### Recommended next

1. **Attach a Pixel and run `scripts/device-acceptance.sh --record`.** Nothing in
   this report substitutes for that.
2. **Fuzz the packet parser** (`packet.rs`, `framing.rs`). Highest value per
   hour: it is the one component any device on the network can reach.
3. **Add pairing rate limiting** — cap concurrent pending pairings per device id
   and refuse a repeat inside a cooldown window.
4. **Move the trust list and SSH targets to the Secret Service** if a threat that
   motivates it is ever identified. Not done now because it protects a file that
   sits next to the key it points at.

---

## 6. Reproducing this work

```bash
# Protocol crate: unit, integration and wire-format tests, plus clippy.
cd core/kdeconnect
cargo test -- --test-threads 1
cargo clippy --all-targets -- -D warnings

# Backend.
cd app/main/src-tauri
cargo test --bin pixel-connect

# Front end.
cd app/main
npx vue-tsc --noEmit
npx vitest run
npm audit --omit=dev

# Dependency advisories.
cd app/main/src-tauri && cargo audit
```

---

## 7. Assurance level

The security work in this report was verified by source review and automated
tests, on one machine, with no real device and no adversarial traffic.

That supports these claims:

- The pairing and authorisation logic enforces pinning, CN binding and
  fail-closed routing — verified by reading the code and by integration tests
  that exercise a real TLS handshake in both roles.
- Incoming transfers are bounded, path-confined and completeness-checked —
  verified by unit tests over the decision logic and by the existing end-to-end
  transfer test.
- External processes are invoked without a shell, with validated arguments —
  verified by reading the call sites and by tests pinning the argument vectors.
- The shipped dependency tree has no known vulnerabilities — verified by
  `cargo audit` and `npm audit --omit=dev`.

It does **not** support these claims:

- That the application is free of vulnerabilities. Nobody has attacked it.
- That it interoperates correctly with a real Pixel. No Pixel was attached.
- That the new limits behave correctly against a hostile peer. They were tested
  as a decision function, not under attack.

**The application should be treated as reviewed and tested, not as audited-safe.**