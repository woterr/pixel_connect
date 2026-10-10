#!/usr/bin/env bash
# Pixel Connect — refresh the local visual-check server with the current build.
set -euo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
OUT="${1:-/tmp/opencode/preview}"
mkdir -p "$OUT"
rm -rf "$OUT"/*
cp -r "$ROOT/app/main/dist/." "$OUT/"
cp "$ROOT/scripts/visual-check-stub.js" "$OUT/visual-stub.js"
python3 - "$OUT/index.html" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1]); s = p.read_text()
if 'visual-stub.js' not in s:
    s = s.replace('<script type="module"',
                  '<script src="./visual-stub.js"></script>\n    <script type="module"', 1)
    p.write_text(s)
PY
cat > "$OUT/frame.html" <<'EOF'
<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Pixel Connect frame</title>
<style>html,body{margin:0;background:#101010;color:#888;font:12px system-ui}
iframe{border:0;display:block}label{display:block;padding:4px 8px}</style></head>
<body><label id="lbl"></label><iframe id="f" src="./index.html"></iframe>
<script>
const q=new URLSearchParams(location.search);
const w=parseInt(q.get('w')||'1600',10),h=parseInt(q.get('h')||'1200',10);
const f=document.getElementById('f');f.width=w;f.height=h;
document.getElementById('lbl').textContent=w+' x '+h;
</script></body></html>
EOF
echo "refreshed $OUT"