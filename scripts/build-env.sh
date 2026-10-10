# Pixel Connect — build environment for environments without root
#
# Tauri's Linux build needs the *-devel packages for GTK, WebKitGTK, libsoup and
# libayatana-appindicator. On a normal Fedora workstation those are installed
# system-wide; in a restricted container they can be unpacked into a local
# sysroot and pointed at with PKG_CONFIG_PATH / LD_LIBRARY_PATH.
#
# Usage:
#   source scripts/setup-sysroot.sh          # downloads + unpacks (needs network)
#   source scripts/build-env.sh              # exports the env vars for a session

set -euo pipefail

SYSROOT="${PIXEL_CONNECT_SYSROOT:-/tmp/opencode/sysroot}"
export SYSROOT

if [ ! -d "$SYSROOT" ]; then
    echo "sysroot $SYSROOT does not exist; run scripts/setup-sysroot.sh first" >&2
    return 1 2>/dev/null || exit 1
fi

export PKG_CONFIG_PATH="$SYSROOT/usr/lib64/pkgconfig:$SYSROOT/usr/share/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="$SYSROOT/usr/lib64:${LD_LIBRARY_PATH:-}"
export LIBRARY_PATH="$SYSROOT/usr/lib64:${LIBRARY_PATH:-}"
export CMAKE_PREFIX_PATH="$SYSROOT/usr:${CMAKE_PREFIX_PATH:-}"

# patchelf is needed by Tauri when bundling AppImage/rpm targets.
if [ -x "$SYSROOT/usr/bin/patchelf" ]; then
    export PATH="$SYSROOT/usr/bin:$PATH"
fi

echo "Pixel Connect sysroot ready: $SYSROOT"
for m in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0 appindicator3-0.1 dbusmenu-glib-0.4; do
    printf '  %-28s %s\n' "$m" "$(pkg-config --modversion "$m" 2>/dev/null || echo MISSING)"
done