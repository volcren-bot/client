#!/bin/sh
# Regenerates the art the bank PIN panel draws (src/bank_pin_overlay.rs). Needs ImageMagick.
# Each .bin is a u16 little endian width, a u16 height, then straight alpha RGBA, row by row.
# The colours are Tokyo Night (blue and foreground); the logo keeps its own.
# The text is Fira Code Bold from the loader's assets (OFL, see ../../loader/assets/FiraCode-LICENSE.txt).
set -e
cd "$(dirname "$0")"
FONT=../../loader/assets/FiraCodeNerdFontMono-Bold.ttf
LOGO=../../../../assets/logo.png
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

pack() { # image out.bin
    w=$(magick "$1" -format %w info:)
    h=$(magick "$1" -format %h info:)
    {
        printf "$(printf '\\%03o\\%03o\\%03o\\%03o' $((w & 255)) $((w >> 8)) $((h & 255)) $((h >> 8)))"
        magick "$1" -depth 8 rgba:-
    } > "$2"
}

magick "$LOGO" -trim +repage -resize x56 -background none "$TMP/logo.png"
pack "$TMP/logo.png" bank_pin_logo.bin

magick -background none -fill '#7aa2f7' -font "$FONT" -pointsize 30 label:VOLCREN -trim +repage "$TMP/title.png"
pack "$TMP/title.png" bank_pin_title.bin

magick -background none -fill '#c0caf5' -font "$FONT" -pointsize 20 "label:Bank pin handler running..." -trim +repage "$TMP/message.png"
pack "$TMP/message.png" bank_pin_message.bin
