#!/usr/bin/env bash
# Pixel Connect — visual verification harness.
#
# Renders the *production* front end bundle in a plain browser so the layout can
# be screenshot and compared against the supplied designs. The Rust backend is
# not running in this mode; `scripts/visual-check-stub.js` answers the same
# `invoke` commands so the components render with representative values.
#
# This is a development tool. It is never referenced by index.html, by any
# component, or by the packaged application.
#
# Usage:
#   scripts/visual-check.sh            # serve app/main/dist on :4173
#   scripts/visual-check.sh --shot F   # also save a screenshot to F (needs a
#                                      # browser with --headless support)
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
DIST="$ROOT/app/main/dist"
PORT="${PIXEL_CONNECT_PREVIEW_PORT:-4173}"
STUB="$ROOT/scripts/visual-check-stub.js"

if [ ! -f "$DIST/index.html" ]; then
    echo "No front end build found. Run: (cd app/main && npm run vite:build)" >&2
    exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp -r "$DIST/." "$WORK/"
cp "$STUB" "$WORK/visual-stub.js"

# Inject the stub before the app bundle.
python3 - "$WORK/index.html" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
s = p.read_text()
s = s.replace('<script type="module"', '<script src="./visual-stub.js"></script>\n    <script type="module"', 1)
p.write_text(s)
PY

echo "Pixel Connect visual harness: http://127.0.0.1:${PORT}/"
cd "$WORK"
exec python3 -m http.server "$PORT" --bind 127.0.0.1