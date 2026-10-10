#!/usr/bin/env bash
# Pixel Connect — populate a local sysroot with the Tauri build dependencies.
#
# On a Fedora Workstation these packages are normally already installed; this
# script exists so the project can also be built in a container without root.
# The sysroot lives outside the repository so it is never committed.
set -euo pipefail

SYSROOT="${PIXEL_CONNECT_SYSROOT:-/tmp/opencode/sysroot}"
RUNDIR="${PIXEL_CONNECT_RPMDIR:-/tmp/opencode/rpms}"
PACKAGES=(webkit2gtk4.1-devel gtk3-devel gtk4-devel libsoup3-devel libappindicator-gtk3-devel patchelf)

mkdir -p "$RUNDIR" "$SYSROOT"
cd "$RUNDIR"

if ! ls ./*.rpm >/dev/null 2>&1; then
    echo "Downloading build dependencies into $RUNDIR ..."
    dnf download --resolve --alldeps --destdir="$RUNDIR" "${PACKAGES[@]}"
fi

echo "Unpacking into $SYSROOT ..."
# cpio restores the RPMs' own (read-only) mode bits, which would then block the
# extraction of later packages. Reset write permission between each package.
for f in ./*.rpm; do
    chmod -R u+w "$SYSROOT" 2>/dev/null || true
    rpm2cpio "$f" | cpio -id --quiet -D "$SYSROOT" 2>/dev/null || true
done
chmod -R u+w "$SYSROOT" 2>/dev/null || true

echo "Done. Now run: source scripts/build-env.sh"
