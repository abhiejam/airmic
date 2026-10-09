# AirMic

Use your iPhone as a wireless microphone for your Linux computer.

The iPhone app streams its mic over local Wi-Fi to a small Linux daemon, `airmicd`.
The daemon adds a virtual input called "AirMic" that any app can use: Zoom, Discord, browsers, Claude Code `/voice`.
Audio stays on your local network. There are no accounts and no cloud services.

Status: v1. It needs Ubuntu 24.04 or later (or another Linux with glibc 2.39 and PipeWire), and an iPhone on iOS 18 or later.
The iPhone app is not on the App Store yet, so you install it from Xcode on a Mac.

## Install

You need three things:
- Ubuntu 24.04 or later, or another Linux with PipeWire and glibc 2.39 or newer. The release binary is built on Ubuntu 24.04. On older systems, build from source.
- An iPhone on iOS 18 or later, on the same Wi-Fi as the computer.
- **A Mac with Xcode**, to install the iPhone app. This is needed until there is an App Store build.

### 1. Install the daemon on Linux

Download `airmic-<version>-x86_64-linux.tar.gz` from the [latest release](https://github.com/abhiejam/airmic/releases/latest). Then unpack it and put `airmicd` somewhere it will stay:

```sh
tar xzf airmic-*-x86_64-linux.tar.gz
mkdir -p ~/.local/bin
cp airmic-*/airmicd ~/.local/bin/
~/.local/bin/airmicd install
```

`airmicd install` writes a systemd user service for the binary at its current path, enables it and starts it. The daemon then starts each time you log in. If you move the binary later, run `airmicd install` again.

Check that it runs:

```sh
airmicd status
```

If your shell can't find `airmicd`, use the full path `~/.local/bin/airmicd`, or log out and back in so `~/.local/bin` joins your `PATH`.

### 2. Install the iPhone app

Follow **[docs/ios-install.md](docs/ios-install.md)**. In short: open `ios/AirMic.xcodeproj` in Xcode, sign it with your free Apple ID and run it on your iPhone. No paid developer account is needed.

Apps signed with a free Apple ID stop opening after **7 days**. To renew, plug the phone into the Mac and press **⌘R** in Xcode again. Your pairings are kept.

### 3. Pair the phone

On the computer, run:

```sh
airmicd pair
```

It prints a 4 digit code and a QR code, then waits.
On the phone, open AirMic and tap **Connect to a computer**. Allow Local Network access when iOS asks.
Your computer appears under **Nearby**. Tap it and type the code, or scan the QR code. Allow the microphone when asked.
`airmicd pair` tells you when the phone has paired. The code expires after 2 minutes. Run `airmicd pair` again for a new one.

## Daily use

1. Open AirMic on the phone. It reconnects to your computer on its own.
2. Use the "AirMic" input in any app. The daemon makes it the default mic, so most apps pick it up without any change.
3. Tap the mute button on the phone to mute. The computer hears silence and the connection stays up.

The phone keeps streaming with its screen locked.

## CLI reference

Plain `airmicd` runs the daemon itself. The systemd service does that for you. The subcommands below talk to the running daemon.

| Command | What it does |
|---|---|
| `airmicd status` | Shows the connected phone (or idle), whether it is muted, latency, packet loss, and whether AirMic is the default mic. Prints a hint when something needs fixing, such as a blocked firewall port. |
| `airmicd pair` | Prints a 4 digit pairing code and a QR code, and waits until a phone pairs or the code expires. |
| `airmicd devices` | Lists the paired phones with their ids. |
| `airmicd forget <id>` | Removes a paired phone. If it is connected, it is disconnected. It needs a new code to pair again. |
| `airmicd make-default` | Makes AirMic the default mic. |
| `airmicd install` | Writes, enables and starts the systemd user service for this binary's path. |
| `airmicd uninstall` | Stops, disables and removes the systemd user service. Your pairings and settings are kept. |

Files:
- Settings: `~/.config/airmic/config.toml`. Set `set_default_source = false` if you don't want AirMic to become the default mic when the daemon starts. The daemon restores your previous default when it stops.
- Paired phones: `~/.config/airmic/paired.json`.
- Logs: `journalctl --user -u airmicd`.

## Troubleshooting

Run `airmicd status` first. It shows most problems and how to fix them.

### Check that apps can hear AirMic

```sh
wpctl status
```

AirMic should be listed under Audio → Sources. A `*` in front of it means it is the default. To record what apps hear:

```sh
pw-record --target airmic --rate 48000 --channels 1 /tmp/airmic.wav   # talk, then Ctrl+C
pw-play /tmp/airmic.wav
```

### AirMic is not the default mic

Run `airmicd make-default`. Some apps keep their own mic setting, so also check the app's audio settings and pick "AirMic". In GNOME, Settings → Sound → Input also lets you choose it.

### The phone connects but no audio arrives

A firewall is most likely blocking the audio port. AirMic uses TCP 47800 for control and UDP 47801 for audio. `airmicd status` shows a hint when the phone is connected but no audio arrives.

If you use ufw, check whether it is active, then open both ports:

```sh
sudo ufw status
sudo ufw allow 47800/tcp
sudo ufw allow 47801/udp
```

### The computer doesn't appear under Nearby

- Make sure the phone and the computer are on the same Wi-Fi network.
- Guest networks and "AP isolation" (also called client isolation) stop devices on the same Wi-Fi from talking to each other. Use your main network, or turn isolation off in the router settings.
- Some networks block the discovery traffic (mDNS) but still allow direct connections. In the app, tap **Enter IP address** and type the computer's address (`ip -4 addr` shows it) with port 47800.
- Check that the daemon runs: `airmicd status`, or `journalctl --user -u airmicd` for its log.

### The app won't open on the iPhone

The free signing has expired after 7 days. Run it from Xcode again, see [docs/ios-install.md](docs/ios-install.md#the-7-day-limit).

## Feedback

AirMic v1 is a test of whether people want this. If you try it, please tell us how it went.

Would you pay for an App Store build, so you don't need a Mac and Xcode or the 7 day re-sign? Please [answer in a feedback issue](https://github.com/abhiejam/airmic/issues/new?template=feedback.yml). Bugs and ideas are welcome there too: [open an issue](https://github.com/abhiejam/airmic/issues/new/choose).

## Build from source

The daemon needs Rust stable and the PipeWire development files:

```sh
sudo apt install libpipewire-0.3-dev libclang-dev pkg-config
cd desktop
cargo build --release -p airmicd
./target/release/airmicd install
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for tests and pull requests.

| Path | What |
|---|---|
| `ios/` | iPhone app (Swift, SwiftUI) |
| `desktop/` | Rust workspace: `airmicd` daemon, `airmic-proto`, `airmic-send` test sender, Tauri app (v2) |
| `docs/PRD.md` | Product plan and architecture |
| `docs/protocol.md` | Wire protocol between phone and computer |
| `packaging/` | systemd user unit |
| `tools/` | Development scripts |

## License

MIT, see [LICENSE](LICENSE).
