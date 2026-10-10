#!/bin/bash
# Builds AirMic and installs it on your iPhone, signed with your free Apple ID.
#
#   ios/install.sh
#
# When it needs you to do something it prints "ACTION NEEDED" and stops; do it
# and run the script again. Every step is safe to repeat. It prints "DONE" when
# the app is on the phone. Run it again every 7 days to renew the app.
set -euo pipefail

cd "$(dirname "$0")"
SCRIPT="$(pwd)/install.sh"
WORK="$HOME/Library/Caches/AirMicInstall"
mkdir -p "$WORK"

step() { printf '\n==> %s\n' "$*"; }
action() {
	printf '\nACTION NEEDED:\n' >&2
	for line in "$@"; do printf '  %s\n' "$line" >&2; done
	printf '\nThen run this again: %s\n' "$SCRIPT" >&2
	exit 2
}
fail() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

# Reads one value from a JSON or plist file, or prints nothing.
jget() { plutil -extract "$2" raw -o - "$1" 2>/dev/null || true; }

[ "$(uname)" = Darwin ] || fail "This script runs on a Mac. Building iPhone apps needs Xcode, which is macOS only."

# --- Xcode -----------------------------------------------------------------
step "Checking Xcode"
if [ -z "${DEVELOPER_DIR:-}" ] && ! [[ "$(xcode-select -p 2>/dev/null)" == *.app/* ]]; then
	# xcode-select points at the Command Line Tools; use the newest Xcode app instead.
	xcode_app="$(ls -d /Applications/Xcode*.app 2>/dev/null | sort | tail -1 || true)"
	if [ -z "$xcode_app" ]; then
		open "macappstore://apps.apple.com/app/id497799835" 2>/dev/null || true
		action "Install Xcode from the Mac App Store (free, it's big, so it takes a while)." \
			"Open Xcode once and let it finish installing components."
	fi
	export DEVELOPER_DIR="$xcode_app/Contents/Developer"
fi
xcodebuild -version | sed -n 1p

if ! xcodebuild -checkFirstLaunchStatus >/dev/null 2>&1; then
	action "Xcode needs a one-time setup. Paste this into Terminal (it asks for your Mac password):" \
		"sudo xcodebuild -license accept && sudo xcodebuild -runFirstLaunch"
fi

if ! [[ "$(xcodebuild -showsdks 2>/dev/null)" == *iphoneos* ]]; then
	step "Downloading iOS support for Xcode (one time, a few GB)"
	xcodebuild -downloadPlatform iOS
fi

# --- Apple ID --------------------------------------------------------------
step "Checking your Apple ID in Xcode"
prefs="$HOME/Library/Preferences/com.apple.dt.Xcode.plist"
teams="$(plutil -p "$prefs" 2>/dev/null | sed -n 's/.*"teamID" => "\([A-Z0-9]*\)".*/\1/p' | sort -u)"
team="${AIRMIC_TEAM:-$(printf '%s\n' "$teams" | sed -n 1p)}"
if [ -z "$team" ]; then
	open -a Xcode 2>/dev/null || true
	action "Add your Apple ID to Xcode (a free one is fine):" \
		"Xcode menu (top left) > Settings... > Accounts > the + button > Apple ID > sign in." \
		"You can close the window afterwards."
fi
if [ "$(printf '%s\n' "$teams" | wc -l)" -gt 1 ] && [ -z "${AIRMIC_TEAM:-}" ]; then
	echo "Several teams found ($(echo $teams)); using $team. Set AIRMIC_TEAM=<id> to pick another."
fi
echo "Team $team"

# The bundle id must be unique per Apple ID, so everyone but the project owner gets their own.
owner_team="$(sed -n 's/.*DEVELOPMENT_TEAM = \([A-Z0-9]*\);.*/\1/p' AirMic.xcodeproj/project.pbxproj | sed -n 1p)"
bundle_id="io.airmic.AirMic"
[ "$team" = "$owner_team" ] || bundle_id="io.airmic.AirMic.$(echo "$team" | tr 'A-Z' 'a-z')"

# --- iPhone ----------------------------------------------------------------
step "Looking for your iPhone"
devices="$WORK/devices.json"
find_phone() {
	rm -f "$devices"
	local rc=0
	xcrun devicectl list devices --json-output "$devices" -q >/dev/null 2>&1 || rc=$?
	if [ "$rc" -ge 128 ]; then
		# devicectl aborts in CoreDevice when the running device service is older
		# than the installed Xcode, which a restart fixes.
		action "Xcode's device tool crashed (exit $rc). This usually happens after Xcode updates" \
			"until the Mac restarts. Restart the Mac, then run this again." \
			"Still crashing after a restart? Paste this into Terminal (asks for your password):" \
			"sudo xcodebuild -runFirstLaunch"
	fi
	udid="" name="" pairing="" devmode=""
	local count i platform transport best=""
	count="$(jget "$devices" result.devices)"
	for ((i = 0; i < ${count:-0}; i++)); do
		platform="$(jget "$devices" "result.devices.$i.hardwareProperties.platform")"
		[ "$platform" = iOS ] || continue
		transport="$(jget "$devices" "result.devices.$i.connectionProperties.transportType")"
		# Prefer a phone on the USB cable, then one reachable over Wi-Fi.
		if [ "$transport" = wired ]; then best=$i; break; fi
		if [ -z "$best" ] && [ "$transport" = localNetwork ]; then best=$i; fi
	done
	[ -n "$best" ] || return 1
	udid="$(jget "$devices" "result.devices.$best.hardwareProperties.udid")"
	name="$(jget "$devices" "result.devices.$best.deviceProperties.name")"
	pairing="$(jget "$devices" "result.devices.$best.connectionProperties.pairingState")"
	devmode="$(jget "$devices" "result.devices.$best.deviceProperties.developerModeStatus")"
}

if ! find_phone; then
	action "Plug your iPhone into this Mac with a USB cable and unlock it." \
		"If the iPhone asks \"Trust This Computer?\", tap Trust and enter your passcode."
fi
echo "Found ${name:-iPhone} ($udid)"

if [ "$pairing" != paired ]; then
	step "Asking the iPhone to trust this Mac"
	xcrun devicectl manage pair --device "$udid" >/dev/null 2>&1 || true
	find_phone || true
	if [ "$pairing" != paired ]; then
		action "Unlock your iPhone, tap Trust on the \"Trust This Computer?\" prompt and enter your passcode." \
			"No prompt? Unplug the cable and plug it back in."
	fi
fi

if [ "$devmode" = disabled ]; then
	action "Turn on Developer Mode on the iPhone:" \
		"Settings > Privacy & Security > Developer Mode (near the bottom) > on." \
		"The iPhone restarts. After it restarts, unlock it and tap Turn On." \
		"Not in the list? Open Xcode with the phone plugged in, wait a minute, then look again."
fi

# --- Build -----------------------------------------------------------------
step "Building AirMic (the first build takes a few minutes)"
log="$WORK/build.log"
if ! xcodebuild -project AirMic.xcodeproj -scheme AirMic -configuration Debug \
	-destination "id=$udid" -derivedDataPath "$WORK/DerivedData" \
	-allowProvisioningUpdates -allowProvisioningDeviceRegistration \
	DEVELOPMENT_TEAM="$team" PRODUCT_BUNDLE_IDENTIFIER="$bundle_id" \
	build >"$log" 2>&1; then
	if grep -qiE "maximum (number of )?app|App ID limit" "$log"; then
		action "A free Apple ID can only have 3 sideloaded apps on a phone at a time." \
			"Delete one you no longer need from the iPhone, then try again." \
			"(The limit on new app IDs resets after 7 days.)"
	elif grep -qiE "No Account for Team|No accounts|sign in" "$log"; then
		action "Xcode needs you to sign in to your Apple ID again:" \
			"Xcode menu > Settings... > Accounts > select your Apple ID and sign in."
	elif grep -qiE "locked" "$log"; then
		action "Unlock your iPhone and keep it unlocked while the app installs."
	elif grep -qiE "Developer Mode" "$log"; then
		action "Turn on Developer Mode: iPhone Settings > Privacy & Security > Developer Mode > on, then restart."
	fi
	errors="$(grep -E "error:" "$log" | sort -u | tail -15 || true)"
	if [ -n "$errors" ]; then echo "$errors" >&2; else tail -30 "$log" >&2; fi
	fail "The build failed. The full log is at $log"
fi
app="$WORK/DerivedData/Build/Products/Debug-iphoneos/AirMic.app"

# --- Install ---------------------------------------------------------------
step "Installing on ${name:-your iPhone}"
# The phone's installer sometimes drops the connection ("Connection interrupted",
# error 3002); a retry usually goes through.
installed=""
for attempt in 1 2 3; do
	if xcrun devicectl device install app --device "$udid" "$app" >"$WORK/install.log" 2>&1; then
		installed=1
		break
	fi
	[ "$attempt" = 3 ] || { echo "Install attempt $attempt failed, retrying..."; sleep 5; }
done
if [ -z "$installed" ]; then
	tail -20 "$WORK/install.log" >&2
	action "Unlock your iPhone and keep it unlocked until the install finishes." \
		"If AirMic is open on the phone, close it (swipe it away in the app switcher)." \
		"Still failing? Unplug and replug the cable, or restart the iPhone."
fi

step "Opening AirMic"
if ! xcrun devicectl device process launch --device "$udid" "$bundle_id" >"$WORK/launch.log" 2>&1; then
	if grep -qiE "trust|profile|invalid code signature|Security" "$WORK/launch.log"; then
		printf '\nDONE: AirMic is installed. One last step on the iPhone, only needed the first time:\n'
		printf '  Settings > General > VPN & Device Management > your Apple ID > Trust.\n'
		printf '  Then open AirMic from the home screen.\n'
	else
		printf '\nDONE: AirMic is installed. Unlock the iPhone and open AirMic from the home screen.\n'
	fi
else
	printf '\nDONE: AirMic is installed and open on %s.\n' "${name:-your iPhone}"
fi
printf '  A free Apple ID signs the app for 7 days. Run this script again to renew it;\n'
printf '  your paired computers and history are kept.\n'
