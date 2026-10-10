#!/usr/bin/env bash
# Pixel Connect — install the "Send to Pixel" file-manager integration.
#
# Installs into the *user's* home directory, so no root is required and the
# integration is removed by deleting two files.
#
# What gets installed, and why both:
#
#   1. ~/.local/share/nautilus-python/extensions/pixel-connect-share.py
#      A Nautilus MenuProvider, the same extension mechanism KDE Connect uses.
#      This gives a top-level "Send to Pixel" entry in the file context menu.
#      It only works where the `nautilus-python` package is installed (GNOME
#      Files shipped Python extension support up to GNOME 46; Fedora still
#      packages `nautilus-python` separately).
#
#   2. ~/.local/share/nautilus/scripts/pixel-connect-send
#      A Nautilus *script*, which every version of GNOME Files still supports,
#      so the action is available even without Python extensions.
#
#   3. ~/.local/share/applications/pixel-connect-send.desktop
#      A desktop entry, so "Send to Pixel" also appears in "Open With Other
#      Application" and can be bound to a keyboard shortcut.
#
# KDE Connect is NOT required and is not touched: this never registers with
# org.kde.kdeconnect, and there is exactly one "Send to Pixel" entry.
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
SRC="$ROOT/packaging/nautilus"

EXT_DIR="$HOME/.local/share/nautilus-python/extensions"
SCRIPT_DIR="$HOME/.local/share/nautilus/scripts"
APP_DIR="$HOME/.local/share/applications"

has_nautilus_python=0
if python3 - <<'PY' >/dev/null 2>&1
import gi
gi.require_version("Nautilus", "4.0")
from gi.repository import Nautilus  # noqa: F401
PY
then
    has_nautilus_python=1
fi

echo "Pixel Connect file-manager integration"
echo "------------------------------------"
mkdir -p "$SCRIPT_DIR" "$APP_DIR"

if [ "$has_nautilus_python" = "1" ]; then
    mkdir -p "$EXT_DIR"
    install -m 0644 "$SRC/pixel-connect-share.py" "$EXT_DIR/pixel-connect-share.py"
    echo "  installed Nautilus context-menu extension -> $EXT_DIR/pixel-connect-share.py"
else
    echo "  skipped the Python extension: the 'nautilus-python' package is not installed"
    echo "     (sudo dnf install nautilus-python python3-nautilus)"
fi

install -m 0755 "$SRC/pixel-connect-send" "$SCRIPT_DIR/pixel-connect-send"

cat > "$APP_DIR/pixel-connect-send.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Send to Pixel
GenericName=Send Files to a Pixel
Comment=Send the selected files to your Google Pixel over the local network
Exec=$SCRIPT_DIR/pixel-connect-send %U
Icon=$APP_DIR/../../../icons/hicolor/128x128/apps/pixel-connect.png
Terminal=false
Categories=Network;FileTransfer;
MimeType=inode/directory;application/octet-stream;
NoDisplay=false
EOF
echo "  installed Nautilus script           -> $SCRIPT_DIR/pixel-connect-send"
echo "  installed desktop entry              -> $APP_DIR/pixel-connect-send.desktop"

cat <<'EOF'

Next steps
----------
  * Restart GNOME Files (Nautilus) for the context-menu entry to appear.
  * Start Pixel Connect at least once so it creates its backend socket.
  * If you do not see "Send to Pixel" in the menu, it will be under
    "Scripts" — that is the script entry, which works on every GNOME version.

To uninstall, delete:
EOF
echo "  $EXT_DIR/pixel-connect-share.py"
echo "  $SCRIPT_DIR/pixel-connect-send"
echo "  $APP_DIR/pixel-connect-send.desktop"