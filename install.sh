#!/bin/sh
# Installs the latest airmic release for the current user and starts it as a systemd user service.
#   curl -fsSL https://raw.githubusercontent.com/abhiejam/airmic/main/install.sh | sh
#   wget -qO- https://raw.githubusercontent.com/abhiejam/airmic/main/install.sh | sh
# AIRMIC_VERSION=v1.0.0 picks a release, AIRMIC_BIN_DIR the install folder (default ~/.local/bin).
set -eu

repo_url="${AIRMIC_RELEASE_URL:-https://github.com/abhiejam/airmic/releases}"
bin_dir="${AIRMIC_BIN_DIR:-$HOME/.local/bin}"

fail() {
    echo "airmic: $*" >&2
    exit 1
}

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL -o "$2" "$1"; }
    latest_tag() { curl -fsSLI -o /dev/null -w '%{url_effective}' "$repo_url/latest" | sed 's|.*/tag/||'; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
    latest_tag() { wget -S --spider "$repo_url/latest" 2>&1 | sed -n 's|.*Location: .*/tag/||p' | tail -n 1; }
else
    fail "needs curl or wget"
fi

[ "$(uname -s)" = Linux ] || fail "airmic runs on Linux only"
[ "$(uname -m)" = x86_64 ] || fail "there is no release for $(uname -m) yet. Build from source: see the README"
[ "$(id -u)" -ne 0 ] || fail "run this as your normal user, not root: airmic is a per-user service"
command -v systemctl >/dev/null 2>&1 || fail "needs systemd"
command -v pw-cli >/dev/null 2>&1 || echo "airmic: warning: PipeWire tools not found. airmic needs PipeWire." >&2

version="${AIRMIC_VERSION:-$(latest_tag)}"
case "$version" in
    v[0-9]*) ;;
    *) fail "could not find the latest release at $repo_url" ;;
esac
archive="airmic-${version#v}-x86_64-linux.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "Downloading AirMic $version..."
fetch "$repo_url/download/$version/$archive" "$tmp/$archive" || fail "download failed: $repo_url/download/$version/$archive"
fetch "$repo_url/download/$version/$archive.sha256" "$tmp/$archive.sha256" || fail "checksum download failed"
(cd "$tmp" && sha256sum -c --quiet "$archive.sha256") || fail "checksum mismatch, not installing"
tar -xzf "$tmp/$archive" -C "$tmp"

unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/airmicd.service"
upgrading=false
[ -f "$unit" ] && upgrading=true

mkdir -p "$bin_dir"
install -m 755 "$tmp/airmic-${version#v}-x86_64-linux/airmic" "$bin_dir/airmic"
echo "Installed $bin_dir/airmic"

"$bin_dir/airmic" install
# `airmic install` leaves an unchanged unit alone, so the old binary would keep running.
if $upgrading; then
    systemctl --user restart airmicd
fi

case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir is not on your PATH yet. Log out and back in, or run $bin_dir/airmic directly." ;;
esac
