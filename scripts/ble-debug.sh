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

OUT_DIR="${BLE_DEBUG_DIR:-/tmp/opencode/ble-debug}"
HCI_LOG="$OUT_DIR/dbus.log"
APP_LOG="$OUT_DIR/app.log"

mkdir -p "$OUT_DIR"

banner() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

# Strip dbus-monitor's timestamp/serial noise, keep the readable part.
pretty() { sed -E 's/^time=[0-9]+ [0-9.]+ sender=[^ ]+ //; s/\x1b\[[0-9;]*m//g'; }

# ---------------------------------------------------------------- analysis --
# Defined before anything that can block, because bash reads a script
# incrementally: a function defined after an infinite loop does not exist yet
# when an interrupt arrives.
analyse() {
    banner "What the phone actually did"

    if [ ! -s "$HCI_LOG" ]; then
        echo "  Nothing was captured."
        echo "  If dbus-monitor printed an access error, the eavesdrop permission"
        echo "  is denied and there is nothing this script can do about it."
        return
    fi

    local reads writes notifys connects

    reads=$(grep -c "member=ReadValue" "$HCI_LOG")
    writes=$(grep -c "member=WriteValue" "$HCI_LOG")
    notifys=$(grep -c "member=StartNotify" "$HCI_LOG")
    connects=$(grep -c "member=Connect" "$HCI_LOG")

    echo
    echo "  GATT operations requested by the phone:"
    printf '    %-34s %s\n' "Device1.Connect" "$connects"
    printf '    %-34s %s\n' "GattCharacteristic1.ReadValue" "$reads"
    printf '    %-34s %s\n' "GattCharacteristic1.WriteValue" "$writes"
    printf '    %-34s %s\n' "GattCharacteristic1.StartNotify" "$notifys"

    echo
    echo "  Endpoint slot read (the 118-byte Quick Share endpoint):"
    grep -B 2 -A 8 "member=ReadValue" "$HCI_LOG" | pretty | head -20 | sed 's/^/    /'

    if [ "$writes" -gt 0 ]; then
        echo
        echo "  Weave writes from the phone (characteristic 0101):"
        grep -B 2 -A 10 "member=WriteValue" "$HCI_LOG" | pretty | head -30 | sed 's/^/    /'
    fi

    if [ "$notifys" -gt 0 ]; then
        echo
        echo "  Notify subscriptions (characteristic 0102):"
        grep -B 2 -A 4 "member=StartNotify" "$HCI_LOG" | pretty | head -15 | sed 's/^/    /'
    fi

    echo
    echo "  Notifications delivered to the phone:"
    grep -c "member=PropertiesChanged" "$HCI_LOG" | sed 's/^/    PropertiesChanged count: /'

    echo
    echo "  Application log - weave activity:"
    if grep -qiE 'weave|session open|InboundRequest' "$APP_LOG"; then
        grep -iE 'weave|session open|InboundRequest|slot0' "$APP_LOG" \
            | sed -E 's/\x1b\[[0-9;]*m//g; s/^/    /' | tail -15
    else
        echo "    none - the phone never reached our handler"
    fi

    echo
    banner "Reading this"
    cat <<'EOS'
  Three outcomes, and they mean different things:

  * Connect 0, ReadValue 0, WriteValue 0
        The phone never connected to our GATT service. Our service is
        registered - the script verified that before capturing - so this is a
        discovery or advertising problem, not a service problem.

  * ReadValue > 0, WriteValue 0, StartNotify 0
        The phone connected and read our endpoint slot, then stopped without
        opening a weave session. This is what "connecting..." looks like: the
        slot read is the discovery step completing, and the weave session that
        should follow it never happens.

  * WriteValue > 0
        The phone sent weave bytes. If the application log still shows no
        weave lines, those bytes never reached our handler, which is a bug in
        our GATT server rather than in the phone.

  Captures kept for reference:
    D-Bus traffic : $HCI_LOG
    application  : $APP_LOG
EOS
}

cleanup() {
    echo
    kill "${DBUS_PID:-0}" >/dev/null 2>&1
    wait "${DBUS_PID:-0}" 2>/dev/null
    analyse
    exit 0
}

# ------------------------------------------------------------------- setup --
banner "Bluetooth adapter"
if ! bluetoothctl show >/dev/null 2>&1; then
    echo "bluetoothctl cannot reach BlueZ. Is bluetooth.service running?"
    systemctl status bluetooth --no-pager 2>&1 | head -5
    exit 1
fi

powered=$(bluetoothctl show 2>/dev/null | sed -n 's/.*Powered: \([A-Za-z]*\)/\1/p')
if [ "$powered" != "Yes" ]; then
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