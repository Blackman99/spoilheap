#!/usr/bin/env bash
# Render the share cards at 2x through headless Chrome, then Lanczos-downscale.
# needs: google-chrome, python3 + Pillow. usage: scripts/share/render.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
port=${PORT:-47614}
python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 & srv=$!
trap 'kill $srv' EXIT; sleep 1
shoot() { # w h out
  google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --window-size="$1,$2" --virtual-time-budget=4000 --screenshot="$3.2x.png" \
    "http://127.0.0.1:$port/scripts/share/card.html?w=$1&h=$2" >/dev/null 2>&1
  python3 -c "from PIL import Image; im=Image.open('$3.2x.png').convert('RGB'); im.resize(($1,$2), Image.LANCZOS).save('$3', optimize=True)"
  rm "$3.2x.png"
}
shoot 1200 630 site/og.png
shoot 1280 640 site/social-preview.png
python3 -c "from PIL import Image; [print(f, Image.open(f).size) for f in ('site/og.png','site/social-preview.png')]"
