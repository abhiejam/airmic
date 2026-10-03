#!/usr/bin/env bash
# M1 spike: raw PCM (s16le, 48 kHz, mono, no header) over UDP -> PipeWire source "airmic".
# Uses module-pipe-tunnel through pw-cli, so pactl (pulseaudio-utils) is not needed.
# The module lives inside the pw-cli process: the source disappears when this script exits.
set -euo pipefail

port="${1:-5555}"
fifo="${XDG_RUNTIME_DIR:-/tmp}/airmic-spike.fifo"

cleanup() {
  kill "${nc_pid:-}" "${pw_pid:-}" 2>/dev/null || true
  rm -f "$fifo"
}
trap cleanup EXIT INT TERM

rm -f "$fifo"
mkfifo "$fifo"

pw-cli -m load-module libpipewire-module-pipe-tunnel "{
  tunnel.mode = source
  pipe.filename = \"$fifo\"
  audio.format = S16LE
  audio.rate = 48000
  audio.channels = 1
  audio.position = [ MONO ]
  stream.props = { node.name = airmic node.description = \"AirMic (spike)\" }
}" >/dev/null &
pw_pid=$!

for _ in $(seq 50); do
  pw-cli ls Node 2>/dev/null | grep -q 'node.name = "airmic"' && break
  sleep 0.1
done
pw-cli ls Node | grep -q 'node.name = "airmic"' || { echo "airmic source did not appear" >&2; exit 1; }

ip=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "src") print $(i + 1)}')
echo "AirMic spike: source 'airmic' is up. Point the phone at ${ip:-<this-machine-ip>}:$port (UDP)."
echo "Listen: pw-record --target airmic --rate 48000 --channels 1 test.wav   (Ctrl-C to stop)"

# OpenBSD nc locks onto the first sender: restart the script if the phone's source port changes.
nc -klu "$port" > "$fifo" &
nc_pid=$!
wait "$nc_pid"
