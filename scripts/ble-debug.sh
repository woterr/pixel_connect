#!/usr/bin/env bash
#
# Diagnose the Quick Share BLE receiver path.
#
# Why this exists: with the receiver advertised, the Pixel completes discovery
# and lists the computer, but a send then stalls at "connecting" forever. The
# application's own log shows nothing at all at that point - not even the INFO
# line emitted when the phone subscribes to the weave notify characteristic -
# which means the question is not "where in our code does it stall" but "does
# anything from the phone reach us at all".
#
# That question is answerable one layer below our code, on the D-Bus interface
# between the application and BlueZ. Every GATT operation the phone triggers
# arrives there as a method call: ReadValue for the endpoint slot,
# WriteValue for the weave stream, StartNotify for the notify characteristic.
#
# This script watches that interface. It needs no root: dbus-monitor falls back
# to eavesdropping, which this user is permitted to do on the system bus.
# (btmon was the first choice and cannot be used here - without CAP_NET_ADMIN
# it prints its version and exits, and it would need a sudo password mid-run.)
#
#   ./scripts/ble-debug.sh
#
# When it says READY, open Quick Share on the phone and pick "fedora". Leave it
# on "connecting" for about 20 seconds, then press Ctrl-C here.

set -uo pipefail

# A fresh directory per run.
#
# Writing into one shared directory broke as soon as this script was run both
# ways. The earlier sudo run created /tmp/opencode/ble-debug owned by root, and
# the unprivileged run then could not write its own log into it, so it failed
# with "Permission denied" and reported the stale root-owned log as if it were
# the current run's - which is exactly how a failure gets attributed to the
# wrong thing.
#
# Timestamping keeps every run's captures intact and removes the collision
# entirely, so runs cannot read each other's files by accident.
OUT_BASE="${BLE_DEBUG_DIR:-/tmp/opencode/ble-debug}"
OUT_DIR="$OUT_BASE/run-$(date +%H%M%S)"

# Fall back rather than fail. A run made with sudo before that became
# unnecessary left /tmp/opencode/ble-debug owned by root, and every unprivileged
# run after it died with "Permission denied" on its own log file. Falling back
# keeps the tool usable instead of leaving it broken for whoever hit it next.
if ! mkdir -p "$OUT_DIR" 2>/dev/null; then
    OUT_BASE="$HOME/.cache/pixel-connect/ble-debug"
    OUT_DIR="$OUT_BASE/run-$(date +%H%M%S)"
    if ! mkdir -p "$OUT_DIR" 2>/dev/null; then
        echo "Cannot create a capture directory under $OUT_BASE either."
        echo "Set BLE_DEBUG_DIR to somewhere writable and try again."
        exit 1
    fi
    echo "  note: the usual capture directory is not writable, using $OUT_BASE"
fi

HCI_LOG="$OUT_DIR/dbus.log"
APP_LOG="$OUT_DIR/app.log"

echo "  captures will be written to $OUT_DIR"

banner() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

# Strip dbus-monitor's timestamp/serial noise, keep the readable part.
pretty() { sed -E 's/^time=[0-9]+ [0-9.]+ sender=[^ ]+ //; s/\x1b\[[0-9;]*m//g'; }

# ---------------------------------------------------------------- analysis --
# Defined before anything that can block, because bash reads a script
# incrementally: a function defined after an infinite loop does not exist yet
# when an interrupt arrives.
analyse() {
    banner "What the phone actually did"

    # The application log is the authoritative source here, not the D-Bus
    # capture. An earlier run showed all-zero GATT call counts and looked like
    # the phone had never connected - while the application log for the same
    # window showed a complete weave handshake, a PIN, a payload transfer and
    # the end of the data. dbus-monitor is not a reliable witness for this
    # traffic on this machine, so its counts are reported for completeness but
    # are never the basis for a conclusion.
    strip() { sed -E 's/\x1b\[[0-9;]*m//g'; }

    echo
    echo "  Which route the phone used:"
    printf '    %-38s %s\n' "TCP/mDNS (LAN)" "$(grep -c 'TcpServer: new client' "$APP_LOG")"
    printf '    %-38s %s\n' "BLE weave (notify socket)" "$(grep -c 'weave: notify session open' "$APP_LOG")"

    echo
    echo "  Handshake (either transport):"
    printf '    %-38s %s\n' "connection started" \
        "$(( $(grep -c 'Handling State::Initial frame' "$APP_LOG") + $(grep -c 'weave: INTRODUCTION' "$APP_LOG") ))"
    printf '    %-38s %s\n' "peer identified" "$(grep -c 'RemoteDeviceInfo' "$APP_LOG")"
    printf '    %-38s %s\n' "pairing PIN issued" "$(grep -c 'Pin code' "$APP_LOG")"
    printf '    %-38s %s\n' "payload bytes received" "$(grep -c 'PayloadType::Bytes' "$APP_LOG")"
    printf '    %-38s %s\n' "files offered" "$(grep -c 'PayloadType::File' "$APP_LOG")"
    printf '    %-38s %s\n' "end of transfer reached" "$(grep -c 'End of data' "$APP_LOG")"

    echo
    echo "  What was delivered:"
    if grep -q 'Created file' "$APP_LOG"; then
        strip < "$APP_LOG" | grep -oE 'path: "[^"]+"' | sed 's/^/    /'
        echo "    (the path above is a staging path; check your Downloads folder)"
        echo "    newest in Downloads:"
        ls -lt "$HOME/Downloads" 2>/dev/null | head -4 | sed 's/^/      /'
    else
        echo "    nothing was written"
    fi

    echo
    echo "  Anything the application could not handle:"
    strip < "$APP_LOG" | grep -iE 'unhandled|not implemented|unsupported' \
        | sed 's/OfflineFrame {.*/OfflineFrame { ... }/' | cut -c1-120 | sort -u | head -5 | sed 's/^/    /' \
        || echo "    nothing"

    echo
    echo "  D-Bus GATT operations (background only; not authoritative):"
    printf '    %-38s %s\n' "GattCharacteristic1.ReadValue" "$(grep -c 'member=ReadValue' "$HCI_LOG" 2>/dev/null)"
    printf '    %-38s %s\n' "GattCharacteristic1.WriteValue" "$(grep -c 'member=WriteValue' "$HCI_LOG" 2>/dev/null)"
    printf '    %-38s %s\n' "GattCharacteristic1.StartNotify" "$(grep -c 'member=StartNotify' "$HCI_LOG" 2>/dev/null)"

    echo
    banner "Reading this"
    cat <<'EOS'
  The phone picks a route on its own. A phone on the same network will use
  TCP/mDNS and never touch BLE, so a run showing no BLE activity is only
  meaningful if it also shows no TCP client - check the first two counters
  before concluding anything.

  If "end of transfer reached" is greater than zero and something was
  delivered, the transfer worked and the problem is in the user interface, not
  the protocol. An earlier version of this script reported all-zero counters
  during a transfer that had in fact completed successfully over TCP, because
  it only counted the BLE weave strings.

  Where the handshake stops before "end of transfer reached" is the real fault.
  "Anything the application could not handle" lists frame types that arrived
  and were dropped; a frame type the protocol does not define may be a newer
  message than this build understands.

  Captures kept for reference:
    D-Bus traffic : $HCI_LOG
    application  : $APP_LOG
EOS
}

# Put the machine back the way it was.
#
# The debug run starts Pixel Connect with PIXEL_CONNECT_BLE_RX=1 so the phone
# can find it over BLE. Leaving that instance running afterwards would leave
# the application advertising BLE by default, which makes the phone prefer
# BLE over the network route. That route is the one known to work, so a
# diagnostic run must not silently become the permanent configuration.
restore() {
    pkill -x pixel-connect >/dev/null 2>&1
    sleep 2
    setsid "$APP_BIN" >/dev/null 2>&1 < /dev/null &
    sleep 3
    if pgrep -x pixel-connect >/dev/null; then
        echo "  Pixel Connect restarted with default settings (BLE not advertised)."
    else
        echo "  WARNING: could not restart Pixel Connect. Start it yourself:"
        echo "      $APP_BIN"
    fi
}

cleanup() {
    echo
    kill "${DBUS_PID:-0}" >/dev/null 2>&1
    wait "${DBUS_PID:-0}" 2>/dev/null
    analyse
    echo
    banner "Restoring defaults"
    restore
    echo
    echo "Captures kept in: $OUT_DIR"
    exit 0
}

# ------------------------------------------------------------------- setup --
# This script no longer needs root - it used btmon, which did, and was run with
# sudo for that reason. Running it as root starts Pixel Connect as root, which
# silently changes where received files land: the app used root's home, so a
# transfer that completed successfully wrote to /root instead of ~/Downloads.
# A transfer that appeared to fail was in fact delivered to the wrong place.
if [ "$(id -u)" -eq 0 ]; then
    echo "This script must not be run as root."
    echo
    if [ -n "${SUDO_USER:-}" ]; then
        echo "Run it as $SUDO_USER instead:"
        echo
        echo "    sudo -u $SUDO_USER $0 \$@"
        echo
        echo "Running as root starts Pixel Connect as root, so received files are"
        echo "written to /root rather than to $SUDO_USER's Downloads folder - which"
        echo "looks exactly like a failed transfer."
    else
        echo "    ./scripts/ble-debug.sh"
    fi
    exit 1
fi

banner "Bluetooth adapter"
if ! bluetoothctl show >/dev/null 2>&1; then
    echo "bluetoothctl cannot reach BlueZ. Is bluetooth.service running?"
    systemctl status bluetooth --no-pager 2>&1 | head -5
    exit 1
fi

# bluetoothctl reports "Powered: yes" in some versions and "Yes" in others,
# so compare case-insensitively or the script switches the adapter on when it
# is already on and makes its own output look unreliable.
powered=$(bluetoothctl show 2>/dev/null | sed -n 's/^[[:space:]]*Powered: *\([A-Za-z]*\).*/\1/p')
if [ "$(printf '%s' "$powered" | tr '[:upper:]' '[:lower:]')" != "yes" ]; then
    echo "  adapter is powered off; turning it on"
    bluetoothctl power on >/dev/null 2>&1
    sleep 2
fi
bluetoothctl show 2>/dev/null \
    | grep -E '^\s*(Controller|Name|Alias|Powered|PowerState)' | sed 's/^/  /'

# ------------------------------------------------------------------- start --
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
# default because the payload path is unfinished and advertising it makes the
# phone prefer BLE. PIXEL_CONNECT_LOG=trace turns on per-decision logging.
setsid env PIXEL_CONNECT_BLE_RX=1 PIXEL_CONNECT_LOG=trace \
    "$APP_BIN" >"$APP_LOG" 2>&1 < /dev/null &
sleep 7

if ! pgrep -x pixel-connect >/dev/null; then
    echo "  the application exited immediately; last lines of its log:"
    tail -20 "$APP_LOG" | sed 's/^/    /'
    exit 1
fi
echo "  running, logging to $APP_LOG"

banner "Did our Quick Share service register?"
grep -iE 'registering GATT|advertising QuickShare' "$APP_LOG" \
    | sed -E 's/\x1b\[[0-9;]*m//g; s/^/  /' | tail -4
if ! grep -q 'registering GATT service 0xFEF3' "$APP_LOG"; then
    echo "  the GATT service did NOT register - nothing below can work."
    exit 1
fi

# ----------------------------------------------------------------- capture --
banner "Watching BlueZ over D-Bus"
echo "  writing to $HCI_LOG"
setsid dbus-monitor --system \
    "type='method_call',interface='org.bluez.GattCharacteristic1'" \
    "type='method_call',interface='org.bluez.GattService1'" \
    "type='method_call',interface='org.bluez.Device1'" \
    "type='signal',interface='org.freedesktop.DBus.Properties'" \
    >"$HCI_LOG" 2>&1 < /dev/null &
DBUS_PID=$!
sleep 3

if ! kill -0 "$DBUS_PID" 2>/dev/null; then
    echo "  dbus-monitor did not start. It said:"
    head -5 "$HCI_LOG" | sed 's/^/    /'
    exit 1
fi
echo "  running (pid $DBUS_PID)"

trap cleanup INT TERM

banner "READY"
cat <<'EOS'

  Open the Quick Share sheet on the phone and choose "fedora".
  Leave it on "connecting" for about 20 seconds.
  Then press Ctrl-C here.

  This only reads D-Bus traffic. It sends nothing, changes no setting, and
  touches no stored data.

EOS

while true; do
    if ! kill -0 "$DBUS_PID" 2>/dev/null; then
        echo
        echo "dbus-monitor exited; analysing what was captured"
        analyse
        exit 0
    fi
    sleep 2
done