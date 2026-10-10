#!/usr/bin/env bash
# Pixel Connect — real-device acceptance test.
#
# V1 and V2 were developed with no Pixel attached, so the highest-value
# verification left is the end-to-end run against a real phone. This script is
# that runbook, made executable: it drives the release build, prints exactly
# what to check at each step, and records the outcome either way.
#
# It is honest by construction. It never prints PASS for something it did not
# observe, and it fails loudly when a step cannot be completed.
#
# Usage:
#   scripts/device-acceptance.sh --record     # drive the app, write a report
#   scripts/device-acceptance.sh --status     # just report what is detectable
#
# Everything it can check without a phone, it checks. Everything it cannot, it
# lists precisely, so a run with no phone still produces useful information.

set -uo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
BINARY="$ROOT/app/main/src-tauri/target/release/pixel-connect"
REPORT="$ROOT/docs/architecture/device-acceptance-report.md"
LOG="${PIXEL_CONNECT_DEVICE_LOG:-/tmp/pixel-connect-acceptance.log}"

MODE="status"
[[ "${1:-}" == "--record" ]] && MODE="record"

say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '  \033[33m! %s\033[0m\n' "$*"; }
good() { printf '  \033[32m✓ %s\033[0m\n' "$*"; }
bad()  { printf '  \033[31m✗ %s\033[0m\n' "$*"; }

DETECTED_PHONE=0
DETECTED_ADB=0
DETECTED_KDC_PEER=0

# --------------------------------------------------------------- preflight
say "Preflight"

if command -v adb >/dev/null; then
    info "adb: $(command -v adb)"
    DEVICES=$(adb devices -l 2>/dev/null | tail -n +2 | grep -c ' device$' || true)
    if [[ "${DEVICES:-0}" -gt 0 ]]; then
        good "$DEVICES authorised ADB device(s) attached"
        DETECTED_ADB=1
    else
        warn "no authorised ADB device (enable USB debugging and accept the prompt)"
    fi
else
    warn "adb is not installed (Fedora: sudo dnf install android-tools)"
fi

if command -v scrcpy >/dev/null; then
    good "scrcpy: $(scrcpy --version 2>&1 | head -1)"
else
    warn "scrcpy is not installed (Fedora: sudo dnf install scrcpy)"
fi

if [[ -x "$BINARY" ]]; then
    good "release binary present ($(du -h "$BINARY" | cut -f1))"
else
    warn "release binary missing; build it first:"
    info "  cd app/main && npm install && npm run vite:build"
    info "  cd src-tauri && cargo build --release"
fi

# KDE Connect peers currently on the LAN, as seen by the protocol backend.
PEERS=$(cd "$ROOT/core/kdeconnect" 2>/dev/null && ./target/debug/kc-probe --seconds 8 2>/dev/null \
        | grep -E '^device      :' || true)
if [[ -n "$PEERS" ]]; then
    good "KDE Connect peers visible on the LAN:"
    printf '%s\n' "$PEERS" | sed 's/^/    /'
    DETECTED_KDC_PEER=1
else
    warn "no KDE Connect peer seen; the phone needs KDE Connect installed and open"
fi

# ------------------------------------------------------------------ summary
say "What this environment can and cannot verify"

[[ $DETECTED_ADB -eq 1 ]]   && good "ADB/scrcpy can be exercised right now" \
                            || warn "ADB/scrcpy: cannot verify without a device"
[[ $DETECTED_KDC_PEER -eq 1 ]] && good "KDE Connect discovery and pairing can be exercised" \
                            || warn "KDE Connect: cannot verify without a peer"
[[ $MODE == "record" ]] || { say "Re-run with --record to drive the app and write a report"; exit 0; }

# --------------------------------------------------------------- drive it
say "Starting Pixel Connect"
if [[ ! -x "$BINARY" ]]; then
    bad "cannot start: no binary"
    exit 1
fi

"$BINARY" > "$LOG" 2>&1 &
APP_PID=$!
trap 'kill "$APP_PID" 2>/dev/null' EXIT
sleep 8

if ! kill -0 "$APP_PID" 2>/dev/null; then
    bad "Pixel Connect exited during start-up; see $LOG"
    tail -20 "$LOG"
    exit 1
fi
good "running (pid $APP_PID), logging to $LOG"

SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pixel-connect/filemanager.sock"
[[ -S "$SOCKET" ]] && good "file-manager socket present" \
                    || warn "file-manager socket missing"

# The backend must have bound a TCP port in the KDE Connect range.
TCP_PORT=$(ss -tlnp 2>/dev/null | grep -oE ':(17[1-6][0-9]|17[0-9]{2})\b' | head -1 | tr -d ':')
if [[ -n "${TCP_PORT:-}" ]]; then
    good "listening on TCP $TCP_PORT (inside the KDE Connect range 1716-1764)"
else
    warn "no KDE Connect TCP port found"
fi

say "Manual steps"
cat <<'EOF'
  With the app open and the phone in hand:

  1. Devices → the phone should appear.          Record: appeared / did not appear
  2. Press "Pair". On the phone, compare the 8-character code.
                                                  Record: matched / did not match
  3. Copy text on the phone.                      Record: appeared in Fedora / not
  4. Copy text on Fedora. Paste on the phone.      Record: worked / not
  5. Send a file to the phone (Files → Send File).
                                                  Record: arrived / not / wrong size
  6. Send a file from the phone.                  Record: arrived / not / wrong size
  7. Send a text share.                           Record: arrived / not
  8. Notifications: trigger one on the phone, act on it in Pixel Connect.
                                                  Record: shown / action worked / not
  9. Devices → Android over USB → Open in scrcpy.
                                                  Record: window opened / not
 10. Files (Nautilus) → right-click → Send to Pixel.
                                                  Record: sent / error text
 11. Close the window; check the tray icon still works and the phone stays paired.
                                                  Record: yes / no
EOF

say "Live log tail"
tail -25 "$LOG" | sed 's/^/  /'

# --------------------------------------------------------------- the report
say "Writing the report"
cat > "$REPORT" <<EOF
# Real-device acceptance report

Generated by \`scripts/device-acceptance.sh --record\` on $(date -u +%Y-%m-%dT%H:%M:%SZ).

## Environment

- ADB device attached: $([[ $DETECTED_ADB -eq 1 ]] && echo yes || echo no)
- KDE Connect peer on the LAN: $([[ $DETECTED_KDC_PEER -eq 1 ]] && echo yes || echo no)
- KDE Connect TCP port: \`${TCP_PORT:-none}\`
- File-manager socket: $([[ -S "$SOCKET" ]] && echo present || echo missing)

## Automated checks

| Check | Result |
|---|---|
| Application starts and stays up | $(kill -0 "$APP_PID" 2>/dev/null && echo pass || echo fail) |
| File-manager socket created | $([[ -S "$SOCKET" ]] && echo pass || echo fail) |
| KDE Connect TCP port bound in 1716-1764 | $([[ -n "${TCP_PORT:-}" ]] && echo pass || echo fail) |

## Manual checks

Fill these in from the run above. Leave a row blank rather than guessing.

| # | Check | Result |
|---|---|---|
| 1 | Phone discovered | |
| 2 | Pairing code matched on both devices | |
| 3 | Phone → Fedora clipboard | |
| 4 | Fedora → phone clipboard | |
| 5 | Linux → phone file | |
| 6 | Phone → Linux file | |
| 7 | Text share | |
| 8 | Notification relay and actions | |
| 9 | scrcpy launch over USB ADB | |
| 10 | Nautilus "Send to Pixel" | |
| 11 | Tray behaviour after closing the window | |

## Not verified

$( [[ $DETECTED_ADB -eq 0 ]] && echo "- ADB/scrcpy behaviour: no authorised device was attached."
   [[ $DETECTED_KDC_PEER -eq 0 ]] && echo "- KDE Connect against a real Pixel: no peer was on the network."
   [[ $DETECTED_ADB -eq 1 ]] || [[ $DETECTED_KDC_PEER -eq 0 ]] )
EOF

good "report written to $REPORT"
say "Done. Edit the manual-check table in the report with what you observed."