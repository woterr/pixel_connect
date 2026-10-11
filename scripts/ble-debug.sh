#!/usr/bin/env bash
#
# Diagnose the Quick Share BLE receiver path.
#
# Why this exists: with the receiver advertised, the Pixel completes discovery
# and lists the computer, but a send then stalls at "connecting" forever. The
# application's own log shows nothing at all at that point - not even the
# INFO line emitted when the phone subscribes to the weave notify
# characteristic - which means the question is not "where in our code does it
# stall" but "does anything from the phone reach us at all".
#
# That question can only be answered below our code, on the air. This script
# captures the ATT traffic between the phone and this machine and filters it
# down to the handful of exchanges that matter.
#
# btmon needs root, so this must be run with sudo. It records nothing outside
# this directory and stops on Ctrl-C.
#
#   sudo ./scripts/ble-debug.sh
#
# When it says READY, open the Quick Share sheet on the phone and pick
# "fedora". Wait for it to sit at "connecting", then come back and press
# Ctrl-C here.

set -uo pipefail

OUT_DIR="${BLE_DEBUG_DIR:-/tmp/opencode/ble-debug}"
mkdir -p "$OUT_DIR"
HCI_LOG="$OUT_DIR/btmon.log"
APP_LOG="$OUT_DIR/app.log"

BLUE="${BLUE:-0}"

banner() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

# --- 1. Bluetooth adapter must be up --------------------------------------
banner "Bluetooth adapter"
if ! bluetoothctl show >/dev/null 2>&1; then
    echo "bluetoothctl cannot reach BlueZ. Is bluetooth.service running?"
    systemctl status bluetooth --no-pager 2>&1 | head -5
    exit 1
fi
bluetoothctl show 2>/dev/null | grep -E '^\s*(Name|Power|Address)' | sed 's/^/  /'

if [ "$(bluetoothctl show 2>/dev/null | sed -n 's/.*Powered: \([A-Za-z]*\)/\1/p')" != "Yes" ]; then
    echo "  the adapter is powered off; turning it on"
    bluetoothctl power on >/dev/null 2>&1
fi

# --- 2. Confirm what BlueZ has actually registered for us -----------------
# If the GATT service is absent here, nothing the phone does can reach our
# code, and no amount of reading the application log will help.
banner "Registered GATT services (from BlueZ, via bluer's org.bluez)"
bluetoothctl <<'EOS' 2>/dev/null | sed 's/^/  /'
show
quit
EOS

# --- 3. Start the application with BLE enabled and verbose logging --------
banner "Starting Pixel Connect"
if [ -x "$HOME/.local/bin/pixel-connect" ]; then
    APP_BIN="$HOME/.local/bin/pixel-connect"
elif [ -x "./app/main/src-tauri/target/release/pixel-connect" ]; then
    APP_BIN="./app/main/src-tauri/target/release/pixel-connect"
else
    echo "  no pixel-connect binary found; build it first"
    exit 1
fi

pkill -x pixel-connect >/dev/null 2>&1
sleep 1

# PIXEL_CONNECT_BLE_RX=1 opts into the 0xFEF3 advertisement, which is off by
# default because the data path is unfinished and advertising it makes the
# phone prefer BLE. PIXEL_CONNECT_LOG=trace turns on the per-decision logging.
setsid env PIXEL_CONNECT_BLE_RX=1 PIXEL_CONNECT_LOG=trace \
    "$APP_BIN" >"$APP_LOG" 2>&1 < /dev/null &

sleep 6
if ! pgrep -x pixel-connect >/dev/null; then
    echo "  the application exited immediately; last lines of its log:"
    tail -20 "$APP_LOG" | sed 's/^/    /'
    exit 1
fi
echo "  running, logging to $APP_LOG"

banner "Does BlueZ report our Quick Share service?"
sleep 2
grep -iE 'FEF3|registering GATT|advertising QuickShare' "$APP_LOG" \
    | sed -E 's/\x1b\[[0-9;]*m//g; s/^/  /' | tail -5
if ! grep -q 'registering GATT service 0xFEF3' "$APP_LOG"; then
    echo "  the GATT service did NOT register - stop here, nothing below can work."
    exit 1
fi

# --- 4. Capture the air interface ----------------------------------------
banner "Capturing Bluetooth traffic"
echo "  writing to $HCI_LOG"
# -r raw gives undecoded frames including the ATT payloads, which is what we
# need to see characteristic UUIDs and values.
sudo -v
setsid sudo -E btmon -r raw >"$HCI_LOG" 2>&1 < /dev/null &
BTMON_PID=$!
sleep 4

banner "READY"
cat <<'EOS'

  Open the Quick Share sheet on the phone and choose "fedora".
  Leave it on "connecting" for about 20 seconds.
  Then press Ctrl-C here.

  Nothing in this script sends anything to the phone, changes any setting,
  or touches stored data - it only reads the air interface.

EOS

# Wait for the user to interrupt.
cleanup() {
    echo
    kill "$BTMON_PID" >/dev/null 2>&1
    sleep 1
    analyse
    exit 0
}
trap cleanup INT TERM

while true; do
    # btmon dies if the adapter is reset; notice rather than hang forever.
    if ! kill -0 "$BTMON_PID" 2>/dev/null; then
        echo
        echo "btmon exited unexpectedly; analysing what was captured"
        analyse
        exit 0
    fi
    sleep 2
done

analyse() {
    banner "What the phone actually did"

    if [ ! -s "$HCI_LOG" ]; then
        echo "  btmon captured nothing at all."
        echo "  If it was never granted access, re-run with sudo and watch for"
        echo "  an authorisation error at the start."
        return
    fi

    # ATT Read/Write against our service UUID. btmon prints found-service and
    # found-characteristic lines as the phone walks our GATT table.
    echo
    echo "  Service discovery (looking for 0xFEF3):"
    grep -icE 'FEF3|fef3' "$HCI_LOG" | sed 's/^/    lines mentioning FEF3: /'
    grep -iE 'FEF3' "$HCI_LOG" | tail -10 | sed -E 's/\x1b\[[0-9;]*m//g; s/^/    /'

    echo
    echo "  Weave characteristics (0101 write / 0102 notify):"
    grep -iE '0x0101|0x0102' "$HCI_LOG" | tail -15 | sed -E 's/\x1b\[[0-9;]*m//g; s/^/    /'

    # ATT Write Request / Write Command are how the weave session is fed.
    # ATT Notification is how the phone subscribes to what we send.
    echo
    echo "  ATT writes and notifications:"
    grep -icE 'ATT: Write|ATT: Notification' "$HCI_LOG" | sed 's/^/    count: /'
    grep -iE 'ATT: Write|ATT: Notification' "$HCI_LOG" | tail -12 \
        | sed -E 's/\x1b\[[0-9;]*m//g; s/^/    /'

    echo
    echo "  Application log - weave activity:"
    grep -iE 'weave|slot0|session open|IncomingRequest|slot 0' "$APP_LOG" \
        | sed -E 's/\x1b\[[0-9;]*m//g; s/^/    /' | tail -15

    echo
    banner "Reading this output"
    cat <<'EOS'
  The lines that decide what is wrong:

  * No mention of FEF3, and no ATT writes at all
        The phone never opened a connection to our GATT service. The problem
        is discovery or advertising - the service is registered (we verified
        that above) but the phone is not connecting to it.

  * FEF3 is mentioned, but no 0101 writes and no 0102 notifications
        The phone connected and read our slot 0, then stopped. It did not
        open a weave session. This is where "connecting..." comes from.

  * 0101 writes present, but the application log has no weave lines
        The phone sent us weave bytes that never reached our handler.

  * weave lines present in the application log
        Our side ran; the stall is further along. Share the application log.

  Full captures kept for reference:
    air traffic : $HCI_LOG
    application: $APP_LOG
EOS
}