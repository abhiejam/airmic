# Install AirMic: instructions for AI agents

You are helping a user install AirMic on their Linux computer. AirMic turns their iPhone into a wireless microphone. `airmic` is a small daemon that receives the phone's audio over Wi-Fi and adds a PipeWire microphone called "AirMic".

Follow these steps in order. Run every command as the user, never as root. Report what you did and what the user has to do next.

## Rules

- Ask the user before any command that needs `sudo`, changes the firewall, or installs system packages.
- Do not change the user's audio settings beyond what `airmic` does itself. It makes AirMic the default mic while it runs, and restores the previous one when it stops.
- Do not edit files in `~/.config/airmic/` by hand.
- If a step fails, show the user the exact error and stop. Do not try workarounds that the steps below don't list.

## 1. Check the computer

```sh
uname -sm                       # must print: Linux x86_64
ldd --version | head -n 1       # glibc must be 2.39 or newer (Ubuntu 24.04 or later)
command -v pw-cli               # PipeWire tools must exist
systemctl --user is-system-running   # must work: airmic runs as a systemd user service
```

If the machine is not x86_64, or glibc is older than 2.39, there is no ready-made binary. Tell the user to build from source (README, "Build from source") and stop.

## 2. Install the daemon

Use `curl` if it exists, else `wget`:

```sh
curl -fsSL https://raw.githubusercontent.com/abhiejam/airmic/main/install.sh | sh
# or
wget -qO- https://raw.githubusercontent.com/abhiejam/airmic/main/install.sh | sh
```

The script installs `~/.local/bin/airmic` and starts it as the `airmicd` user service. It does not need sudo. Running it again updates AirMic.

## 3. Check that it runs

```sh
~/.local/bin/airmic status
```

Expected: `State: idle, no phone connected` and a `Daemon:` version line. If it says `airmicd is not running`, show the user the output of:

```sh
systemctl --user status airmicd --no-pager
journalctl --user -u airmicd -n 50 --no-pager
```

## 4. The iPhone app

The iPhone app is not on the App Store yet. The user needs a Mac with Xcode to install it. Point them to https://github.com/abhiejam/airmic/blob/main/docs/ios-install.md. You cannot do this step for them.

## 5. Pair the phone

Pairing shows a QR code and waits up to 2 minutes, so the user should run it in their own terminal:

```sh
~/.local/bin/airmic pair
```

Tell the user: open AirMic on the iPhone, tap **Connect to a computer**, pick this computer under **Nearby**, and type the 4 digit code (or scan the QR code). `airmic pair` prints "Paired with …" when it works.

## 6. Confirm audio

Once the phone is connected, run `~/.local/bin/airmic status` again. Expected: `State: connected`, `Audio: arriving`, `Default mic: yes`.

## Fixing common problems

`airmic status` prints a `Hint:` line when something is wrong. Act on it like this:

| What `status` shows | What to do |
|---|---|
| Hint about `sudo ufw allow <port>/udp` | A firewall blocks the audio. Run `sudo ufw status`. If ufw is active, ask the user before running `sudo ufw allow 47800/tcp` and `sudo ufw allow <port>/udp`. If ufw is inactive, the Wi-Fi network probably isolates its clients (AP isolation or a guest network): tell the user to use their main network. |
| `Default mic: no` | Run `~/.local/bin/airmic make-default`. |
| The phone never sees the computer | Same Wi-Fi network? Guest networks and AP isolation block discovery. The app has **Enter IP address**: give the user the address from `ip -4 route get 1.1.1.1` (the `src` field) and port 47800. |

To check that apps hear AirMic: `wpctl status` lists "AirMic" under Audio → Sources, with a `*` when it is the default.

## Uninstall

```sh
~/.local/bin/airmic uninstall
rm ~/.local/bin/airmic
```

Pairings stay in `~/.config/airmic/`. Delete that folder only if the user asks.
