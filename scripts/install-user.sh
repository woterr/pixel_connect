#!/usr/bin/env bash
# Pixel Connect — install for the current user.
#
# Registers the application with GNOME: binary on PATH, a desktop entry in the
# applications menu, and icons in the hicolor theme. Everything lands under
# $HOME, so no root is needed and nothing outside the user's own directories is
# touched.
#
# The Fedora RPM (packaging/rpm/pixel-connect.spec) installs the same files
# system-wide; this script exists so the app is launchable from the app grid
# before any package is built, and for testing.
#
# Usage:
#   scripts/install-user.sh            install
#   scripts/install-user.sh --uninstall remove everything it installed
#   scripts/install-user.sh --no-build use the binary as it is, do not rebuild

set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
BIN_SRC="$ROOT/app/main/src-tauri/target/release/pixel-connect"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor"
AUTOSTART="$HOME/.config/autostart/dev.pixelconnect.app.desktop"
ENTRY_ID="dev.pixelconnect.app.desktop"

say()  { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '  \033[33m! %s\033[0m\n' "$*"; }

uninstall() {
    say "Removing Pixel Connect"
    rm -f "$BIN_DIR/pixel-connect" "$APP_DIR/$ENTRY_ID" "$AUTOSTART"
    rm -f "$ICON_DIR/32x32/apps/pixel-connect.png" \
          "$ICON_DIR/128x128/apps/pixel-connect.png" \
          "$ICON_DIR/256x256/apps/pixel-connect.png" \
          "$ICON_DIR/512x512/apps/pixel-connect.png" \
          "$ICON_DIR/scalable/apps/pixel-connect-symbolic.svg"
    # Tauri also installs a 2x variant used by some panels.
    rm -f "$ICON_DIR/128x128/apps/pixel-connect@2x.png"
    gtk-update-icon-cache -q -f -t "$ICON_DIR" 2>/dev/null || true
    update-desktop-database "$APP_DIR" 2>/dev/null || true
    info "done"
}

[ "${1:-}" = "--uninstall" ] && { uninstall; exit 0; }

if [ "${1:-}" != "--no-build" ]; then
    say "Building"
    if ! (cd "$ROOT/app/main/src-tauri" && cargo build --release); then
        warn "build failed; install the system dependencies first:"
        warn "  Fedora: sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel \\"
        warn "                     libappindicator-gtk3-devel librsvg2-devel patchelf protobuf-compiler"
        exit 1
    fi
fi

[ -x "$BIN_SRC" ] || { warn "no binary at $BIN_SRC"; exit 1; }

say "Installing for $USER"

mkdir -p "$BIN_DIR" "$APP_DIR" \
         "$ICON_DIR/32x32/apps" "$ICON_DIR/128x128/apps" \
         "$ICON_DIR/256x256/apps" "$ICON_DIR/512x512/apps" \
         "$ICON_DIR/scalable/apps"

install -m 0755 "$BIN_SRC" "$BIN_DIR/pixel-connect"
info "binary     -> $BIN_DIR/pixel-connect"

# The desktop entry resolves the binary by name, so it only works because the
# binary is on PATH. Verify rather than assume.
if ! command -v pixel-connect >/dev/null; then
    warn "WARNING: $BIN_DIR is not on PATH; the launcher will not find the app."
    warn "Add this to your shell profile:"
    warn "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# Write the entry with an absolute Exec path.
#
# The shipped entry uses `Exec=pixel-connect`, which only works when the binary
# is on PATH. GNOME resolves the app grid through its own environment, which is
# not always the login shell's, so a PATH lookup can silently fail to launch.
# A hard path is what a real installation does.
sed "s|^Exec=pixel-connect$|Exec=$BIN_DIR/pixel-connect|" \
    "$ROOT/packaging/desktop/$ENTRY_ID" > "$APP_DIR/$ENTRY_ID"
chmod 0644 "$APP_DIR/$ENTRY_ID"
info "desktop    -> $APP_DIR/$ENTRY_ID (Exec=$BIN_DIR/pixel-connect)"

install -m 0644 "$ROOT/data/branding/pixel-connect-32.png"  "$ICON_DIR/32x32/apps/pixel-connect.png"
install -m 0644 "$ROOT/data/branding/pixel-connect-128.png" "$ICON_DIR/128x128/apps/pixel-connect.png"
install -m 0644 "$ROOT/data/branding/pixel-connect-128.png" "$ICON_DIR/128x128/apps/pixel-connect@2x.png"
install -m 0644 "$ROOT/data/branding/pixel-connect-256.png" "$ICON_DIR/256x256/apps/pixel-connect.png"
install -m 0644 "$ROOT/app/main/src-tauri/icons/icon.png"   "$ICON_DIR/512x512/apps/pixel-connect.png"
install -m 0644 "$ROOT/data/branding/pixel-connect-symbolic.svg" "$ICON_DIR/scalable/apps/pixel-connect-symbolic.svg"
info "icons      -> $ICON_DIR/{32x32,128x128,256x256,512x512,scalable}/apps"

# Refresh the caches the shell reads. Without this the entry can be invisible
# until the next login.
gtk-update-icon-cache -q -f -t "$ICON_DIR" 2>/dev/null || true
update-desktop-database "$APP_DIR" 2>/dev/null || true
info "caches refreshed"

say "Verifying"
if desktop-file-validate "$APP_DIR/$ENTRY_ID" 2>&1; then
    info "desktop entry validates"
else
    warn "desktop-file-validate reported problems (see above)"
fi

say "Done"
info "Launch it from the GNOME app grid (Super, then 'Pixel Connect'), or run:"
info "    pixel-connect"
info ""
info "Autostart on login is off by default. To enable it, run:"
info "    mkdir -p ~/.config/autostart"
info "    cp $APP_DIR/$ENTRY_ID $AUTOSTART"