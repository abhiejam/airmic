# Install the iPhone app

AirMic isn't on the App Store yet. You build it from this repo and install it on your own iPhone with a free Apple ID. No paid developer account is needed.

## What you need
- A Mac with Xcode 26 or later.
- An iPhone on iOS 18 or later, and a USB cable.
- An Apple ID (the free one you already have is fine).

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
Apps signed with a free Apple ID stop opening after **7 days**. To renew, plug the phone in and press **⌘R** in Xcode again. Your paired computers and session history are kept.

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
