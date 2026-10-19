#!/usr/bin/env bash
# Regenerate the derived app icons from assets/app-icon/bimcraft-1024.png: the hicolor PNGs Linux
# desktops read, and bimcraft.icns for macOS. Needs ImageMagick (magick) and Python with Pillow, on
# any OS. The outputs are committed, so builds and packaging never need these tools.
#
#   packaging/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/assets/app-icon"
SRC="$DIR/bimcraft-1024.png"
ID="io.github.ipedrax.bimcraft"
command -v magick >/dev/null || { echo "error: magick (ImageMagick 7) not found" >&2; exit 1; }

# -strip leaves out the time and the tool, so the same source writes the same bytes.
resize() { magick "$1" -filter Lanczos -resize "$2x$2" -strip "$3"; }

for s in 16 24 32 48 64 128 256 512; do
  mkdir -p "$DIR/hicolor/${s}x${s}/apps"
  resize "$SRC" "$s" "$DIR/hicolor/${s}x${s}/apps/$ID.png"
done
mkdir -p "$DIR/hicolor/scalable/apps"
cp "$DIR/bimcraft.svg" "$DIR/hicolor/scalable/apps/$ID.svg"

# macOS draws app icons on Apple's grid, an 824 px body on a 1024 px canvas, where the tile fills
# 960: shrunk to 824 and centred, it sits as large as the Dock's other icons.
MAC="$DIR/bimcraft-macos-1024.png"
magick "$SRC" -filter Lanczos -resize "$((1024 * 824 / 960))x" -background none -gravity center \
  -extent 1024x1024 -strip "$MAC"
python3 - "$MAC" "$DIR/bimcraft.icns" <<'EOF'
import sys
from PIL import Image
# Pillow writes every size an .icns holds (16 to 1024, with the @2x ones) from the 1024 px master.
Image.open(sys.argv[1]).save(sys.argv[2], format="ICNS")
EOF
echo "icons written to $DIR"
