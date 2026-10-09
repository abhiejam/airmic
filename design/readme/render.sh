#!/bin/sh
# Renders docs/images/airmic-iphone.gif from phones.html: a 2.2 s loop, 20 fps.
# Needs google-chrome and ffmpeg. Run from the repo root: design/readme/render.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

i=0
while [ $i -lt 44 ]; do
    t=$(awk "BEGIN{print $i * 0.05}")
    google-chrome --headless=new --disable-gpu --hide-scrollbars --no-first-run \
        --user-data-dir="$tmp/profile" --window-size=940,964 --force-device-scale-factor=1 \
        --screenshot="$tmp/f$(printf %02d $i).png" "file://$here/phones.html#$t" >/dev/null 2>&1
    i=$((i + 1))
done

ffmpeg -loglevel error -y -framerate 20 -i "$tmp/f%02d.png" \
    -vf "scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=256:stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle" \
    -loop 0 docs/images/airmic-iphone.gif
