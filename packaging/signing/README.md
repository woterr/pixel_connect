# Release signing

Pixel Connect's own signing identity. **Upstream rquickshare's keys are never
reused** — that is both a licence obligation and a practical requirement: a
released package must be attributable to whoever built it.

## What is signed

| Artefact | Mechanism | Command |
|---|---|---|
| RPM package | RPM signature with a GPG key | `rpmsign --addsign …rpm` |
| Release tarball | detached GPG signature | `gpg --detach-sign` |
| Git tags | GPG signature | `git tag -s` |

## Creating the project's key

Run once, on the machine that will sign releases:

```bash
./packaging/signing/create-key.sh
```

The script creates a dedicated subkey with no passphrase stored on disk and
prints the fingerprint to record in `pixel-connect.spec`'s `%_gpg_name`.

## Signing a release

```bash
packaging/signing/sign-rpm.sh app/main/src-tauri/target/release/bundle/rpm/pixel-connect-*.rpm
```

Users verify with:

```bash
rpm --checksig pixel-connect-0.1.0-1.fc*.rpm
rpm -q gpg-pubkey --qf '%{SUMMARY}\n'
```

## Android

See `android/README.md`. The companion must use its own keystore; the Android
debug keystore that ships in upstream source trees must never be used for a
release build.

## What is deliberately not committed

Private keys, `*.asc` key material, and keystores are excluded by `.gitignore`.
Only public keys and fingerprints belong in the repository.
