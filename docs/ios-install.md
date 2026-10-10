# Install the iPhone app

AirMic isn't on the App Store yet. You build it from this repo and install it on your own iPhone with a free Apple ID. No paid developer account is needed.

## What you need
- A Mac with Xcode 26 or later.
- An iPhone on iOS 18 or later, and a USB cable.
- An Apple ID (the free one you already have is fine).

## Quick install (beta)
Plug the iPhone into your Mac and paste this into Terminal:

```sh
git clone https://github.com/abhiejam/airmic.git ~/airmic 2>/dev/null || git -C ~/airmic pull
~/airmic/ios/install.sh
```

The script checks Xcode, your Apple ID and the iPhone, then builds, installs and opens AirMic. When you need to do something (sign in to Xcode, tap Trust, turn on Developer Mode) it prints **ACTION NEEDED** and stops. Do that and run it again. Run it again every 7 days to renew the app. The script is new: if it fails, please open an issue with its output.

**With an AI agent** (Claude Code, Cursor, Codex and so on), paste this:

> Install the AirMic iPhone app for me. Run `git clone https://github.com/abhiejam/airmic.git ~/airmic 2>/dev/null || git -C ~/airmic pull`, then run `~/airmic/ios/install.sh`. If it stops with ACTION NEEDED, tell me what to do in short, plain steps, wait until I say done, then run it again. Repeat until it prints DONE. If the build fails, read the log it names and help me fix it.

The steps below are the same thing done by hand in Xcode.

## One-time setup
1. **Add your Apple ID to Xcode:** Xcode → Settings → Accounts → **+** → Apple ID. This creates a free "Personal Team".
2. **Open the project:** `ios/AirMic.xcodeproj`. Select the **AirMic** target → **Signing & Capabilities** → set **Team** to *Your Name (Personal Team)*.
   - If Xcode says the bundle identifier is not available, change `io.airmic.AirMic` to something unique, such as `io.airmic.AirMic.yourname`.
3. **Connect the iPhone** by USB, unlock it and tap **Trust This Computer**.
4. **Turn on Developer Mode** on the iPhone: Settings → Privacy & Security → **Developer Mode** → on, then restart and confirm. The option only appears after Xcode has seen the phone once.
5. **Run:** pick the iPhone as the run destination and press **⌘R**.
6. **Trust your certificate** the first time: the app won't open until you do. On the iPhone, Settings → General → **VPN & Device Management** → your Apple ID → **Trust**. Then open AirMic.

From the command line, steps 5 and 6 are:

```sh
cd ios
xcodebuild -scheme AirMic -destination 'platform=iOS,name=<your iPhone>' -allowProvisioningUpdates build
xcrun devicectl device install app --device <your iPhone> <DerivedData>/Build/Products/Debug-iphoneos/AirMic.app
```

## The 7 day limit
Apps signed with a free Apple ID stop opening after **7 days**. To renew, run `ios/install.sh` again (or press **⌘R** in Xcode). After the first install over USB, the phone can stay unplugged if it is on the same Wi-Fi as the Mac. Your paired computers and session history are kept.

Other free-account limits: at most 3 of your own apps on a device at once, and no TestFlight. AirMic needs nothing that a free account can't do: microphone, background audio, local network, Bonjour and the camera all work.

## First run
1. **Install and start the daemon** on the computer: see [Install in the README](../README.md#install).
2. **Open AirMic** and tap **Connect to a computer**.
3. **Allow Local Network** when iOS asks. Your computer shows up under **Nearby**.
4. **Pair:** tap the computer and type the 4 digit code it shows. Allow the microphone when asked.
5. **Next time,** AirMic reconnects to that computer when it opens, with no taps.

**Computer not listed?** Use **Enter IP address** (port 47800), or check the troubleshooting table in [the checklist](notes/iphone-linux-checklist.md#if-something-fails).

## Updating
Pull the repo and press **⌘R** again. Pairings and history survive updates.
