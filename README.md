# AirMic

Use your iPhone as a wireless microphone for your Linux computer.

Want it on the App Store without building it yourself? [Join the waitlist](https://abhishekejam.com/airmic?ref=readme)

<p align="center">
  <img src="docs/images/airmic-iphone.gif" width="600" alt="The AirMic iPhone app: on air and streaming with a live waveform, next to the muted screen">
</p>

The iPhone app streams its mic over local Wi-Fi to a small Linux daemon, `airmic daemon`, which runs as the `airmicd` user service.
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

Run this as your normal user (not with sudo):

```sh
curl -fsSL https://raw.githubusercontent.com/abhiejam/airmic/main/install.sh | sh
```

Ubuntu doesn't ship `curl` by default. If it's missing, use `wget` instead:

```sh
wget -qO- https://raw.githubusercontent.com/abhiejam/airmic/main/install.sh | sh
```

The script downloads the latest release and checks its checksum. It puts `airmic` in `~/.local/bin` and runs `airmic install`. That sets up a systemd user service, so the daemon starts each time you log in. Run the same command again to update.

Check that it runs:

```sh
airmic status
```

If your shell can't find `airmic`, log out and back in so `~/.local/bin` joins your `PATH`, or use `~/.local/bin/airmic`.

<details>
<summary>Or install the .deb package</summary>

Download `airmic_<version>_amd64.deb` from the [latest release](https://github.com/abhiejam/airmic/releases/latest), then:

```sh
sudo apt install ./airmic_*_amd64.deb
systemctl --user start airmicd
```

The package puts `airmic` in `/usr/bin` and starts the service at every user's login. After an upgrade, run `systemctl --user restart airmicd`. Remove it with `sudo apt remove airmic`. Don't also run `airmic install` with the package.

</details>

<details>
<summary>Or install by hand</summary>

Download `airmic-<version>-x86_64-linux.tar.gz` from the [latest release](https://github.com/abhiejam/airmic/releases/latest), then:

```sh
tar xzf airmic-*-x86_64-linux.tar.gz
mkdir -p ~/.local/bin
cp airmic-*/airmic ~/.local/bin/
~/.local/bin/airmic install
```

If you move the binary later, run `airmic install` again.

</details>

### Or let an AI agent install it

If you use a coding agent such as Claude Code, Codex or Cursor, paste this into it:

```text
Install AirMic on this Linux computer by following
https://raw.githubusercontent.com/abhiejam/airmic/main/docs/agent-install.md
```

The agent installs the daemon, checks it runs and gives you the pairing code. It asks before anything that needs sudo.

### 2. Install the iPhone app

You need a Mac with Xcode, a USB cable and a free Apple ID. No paid developer account. Plug the iPhone into the Mac and paste this into Terminal:

```sh
git clone https://github.com/abhiejam/airmic.git ~/airmic 2>/dev/null || git -C ~/airmic pull
~/airmic/ios/install.sh
```

The script builds AirMic, signs it with your Apple ID and installs it on the phone. When you need to do something (sign in to Xcode, tap Trust, turn on Developer Mode) it prints **ACTION NEEDED** and stops; do it and run the script again. The script is new, so if it fails, please [open an issue](https://github.com/abhiejam/airmic/issues) with its output.

Or paste this into your AI agent (Claude Code, Codex, Cursor):

```text
Install the AirMic iPhone app for me. Run
`git clone https://github.com/abhiejam/airmic.git ~/airmic 2>/dev/null || git -C ~/airmic pull`,
then run `~/airmic/ios/install.sh`. If it stops with ACTION NEEDED, tell me what to do
in short, plain steps, wait until I say done, then run it again. Repeat until it prints
DONE. If the build fails, read the log it names and help me fix it.
```

Apps signed with a free Apple ID stop opening after **7 days**. To renew, run `~/airmic/ios/install.sh` again. Your pairings are kept. To do it by hand in Xcode instead, see **[docs/ios-install.md](docs/ios-install.md)**.

### 3. Pair the phone

On the computer, run:

```sh
airmic pair
```

It prints a 4 digit code and a QR code, then waits.
On the phone, open AirMic and tap **Connect to a computer**. Allow Local Network access when iOS asks.
Your computer appears under **Nearby**. Tap it and type the code, or scan the QR code. Allow the microphone when asked.
`airmic pair` tells you when the phone has paired. The code expires after 2 minutes. Run `airmic pair` again for a new one.

## Daily use

1. Open AirMic on the phone. It reconnects to your computer on its own.
2. Use the "AirMic" input in any app. The daemon makes it the default mic, so most apps pick it up without any change.
3. Tap the mute button on the phone to mute. The computer hears silence and the connection stays up.

The phone keeps streaming with its screen locked.

## CLI reference

Plain `airmic` prints help. `airmic daemon` runs the daemon itself, and the `airmicd` systemd service does that for you. The other subcommands talk to the running daemon.

| Command | What it does |
|---|---|
| `airmic status` | Shows the connected phone (or idle), whether it is muted, latency, packet loss, and whether AirMic is the default mic. Prints a hint when something needs fixing, such as a blocked firewall port. |
| `airmic pair` | Prints a 4 digit pairing code and a QR code, and waits until a phone pairs or the code expires. |
| `airmic devices` | Lists the paired phones with their ids. |
| `airmic forget <id>` | Removes a paired phone. If it is connected, it is disconnected. It needs a new code to pair again. |
| `airmic make-default` | Makes AirMic the default mic. |
| `airmic install` | Writes, enables and starts the systemd user service for this binary's path. |
| `airmic uninstall` | Stops, disables and removes the systemd user service. Your pairings and settings are kept. |

Files:
- Settings: `~/.config/airmic/config.toml`. Set `set_default_source = false` if you don't want AirMic to become the default mic when the daemon starts. The daemon restores your previous default when it stops.
- Paired phones: `~/.config/airmic/paired.json`.
- Logs: `journalctl --user -u airmicd`.

## Troubleshooting

Run `airmic status` first. It shows most problems and how to fix them.

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

Run `airmic make-default`. Some apps keep their own mic setting, so also check the app's audio settings and pick "AirMic". In GNOME, Settings → Sound → Input also lets you choose it.

### The phone connects but no audio arrives

A firewall is most likely blocking the audio port. AirMic uses TCP 47800 for control and UDP 47801 for audio. `airmic status` shows a hint when the phone is connected but no audio arrives.

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
- Check that the daemon runs: `airmic status`, or `journalctl --user -u airmicd` for its log.

### The app won't open on the iPhone

- **"Untrusted Developer"** on first launch: on the iPhone, Settings → General → VPN & Device Management → your Apple ID → Trust.
- **It opened before but won't now:** the free signing expired after 7 days. Run `~/airmic/ios/install.sh` again, see [docs/ios-install.md](docs/ios-install.md#the-7-day-limit).

## Feedback

AirMic v1 is a test of whether people want this. If you try it, please tell us how it went.

Would you pay for an App Store build, so you don't need a Mac and Xcode or the 7 day re-sign? Please [answer in a feedback issue](https://github.com/abhiejam/airmic/issues/new?template=feedback.yml). Bugs and ideas are welcome there too: [open an issue](https://github.com/abhiejam/airmic/issues/new/choose).

## Build from source

The daemon needs Rust stable and the PipeWire development files:

```sh
sudo apt install libpipewire-0.3-dev libclang-dev pkg-config
cd desktop
cargo build --release -p airmicd
./target/release/airmic install
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for tests and pull requests.

| Path | What |
|---|---|
| `ios/` | iPhone app (Swift, SwiftUI) |
| `desktop/` | Rust workspace: `airmicd` crate (the `airmic` binary), `airmic-proto`, `airmic-send` test sender, Tauri app (v2) |
| `docs/PRD.md` | Product plan and architecture |
| `docs/protocol.md` | Wire protocol between phone and computer |
| `packaging/` | systemd user unit |
| `tools/` | Development scripts |

## License

MIT, see [LICENSE](LICENSE).
