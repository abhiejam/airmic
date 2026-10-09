#!/bin/sh
# Builds airmic_<version>_amd64.deb from a built airmicd binary.
# Usage: packaging/build-deb.sh <version without v> <path to airmicd> <output dir>
# The user unit is enabled globally, so it starts at every user's next login.
set -eu
umask 022

[ $# -eq 3 ] || { echo "usage: $0 <version> <airmicd binary> <output dir>" >&2; exit 2; }
version=$1
binary=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d)
chmod 755 "$root"
trap 'rm -rf "$root"' EXIT

install -Dm755 "$binary" "$root/usr/bin/airmicd"
install -Dm644 "$here/airmicd.service" "$root/usr/lib/systemd/user/airmicd.service"
install -Dm644 "$here/../README.md" "$root/usr/share/doc/airmic/README.md"
install -Dm644 "$here/../LICENSE" "$root/usr/share/doc/airmic/copyright"

mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<EOF
Package: airmic
Version: $version
Architecture: amd64
Maintainer: abhiejam <ejam.abhishek@gmail.com>
Depends: libc6 (>= 2.39), libgcc-s1, libpipewire-0.3-0t64
Section: sound
Priority: optional
Homepage: https://github.com/abhiejam/airmic
Description: iPhone as a wireless microphone
 airmicd receives audio from the AirMic iPhone app over Wi-Fi and exposes it
 as a PipeWire microphone called "AirMic".
EOF

cat > "$root/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
    systemctl --global enable airmicd.service >/dev/null 2>&1 || true
    echo "AirMic installed. Run 'systemctl --user start airmicd' (or log out and in), then 'airmicd pair'."
fi
EOF

cat > "$root/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ]; then
    systemctl --global disable airmicd.service >/dev/null 2>&1 || true
fi
EOF
chmod 755 "$root/DEBIAN/postinst" "$root/DEBIAN/prerm"

mkdir -p "$out"
dpkg-deb --root-owner-group --build "$root" "$out/airmic_${version}_amd64.deb"
