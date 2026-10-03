# iPhone → Linux test checklist

End-to-end check of the iPhone app against the real `airmicd` on Ubuntu.
Covers mobile tasks M1.6, M2.8, M2.9, M3.1 and desktop D1.3. About 15 minutes.

**Before you start**
- Ubuntu with PipeWire, Rust stable, this repo at the latest `main`.
- iPhone with an AirMic build from the latest `main` (installed from the Mac with Xcode).
- Phone and PC on the same Wi-Fi, with no guest network or client isolation.

## 1. Run the daemon

```sh
cd desktop
cargo build --release -p airmicd
./target/release/airmicd
```

- [ ] The log shows `control channel on TCP 47800`, `audio channel on UDP 47801` and `IPC on …`.
- [ ] `wpctl status` lists **AirMic** under Audio → Sources, marked `*` as the default.
- [ ] `wpctl inspect @DEFAULT_AUDIO_SOURCE@ | grep node.name` prints `airmic`.

A firewall is only in the way if one is active. With `ufw` enabled, ask before changing it, then open 47800/tcp, 47801/udp and 5353/udp (mDNS).

## 2. Find and pair (M3.1, M3.3)

- [ ] The daemon log has no `mDNS advert failed` warning. (`avahi-browse -rt _airmic._tcp` shows the TXT record, if `avahi-utils` is installed.)
- [ ] On the phone, tap **Connect to a computer**. The PC's hostname appears under **Nearby**.
- [ ] Tap it. The daemon logs `pairing code NNNN`; type that code on the phone.
- [ ] The daemon logs `phone paired` and `session 0x… ready`.
- [ ] The phone shows **On air · `<hostname>` · NN ms**. The latency figure updates every 2 s (M2.8).

## 3. Hear it (M1.6, D1.3)

```sh
pw-record --target airmic --rate 48000 --channels 1 /tmp/airmic.wav   # talk for 10 s, then Ctrl+C
pw-play /tmp/airmic.wav
```

- [ ] The recording is your voice, clear, without crackles or gaps.
- [ ] In Claude Code, `/voice` transcribes what you say into the phone.
- [ ] Note the quantum and rate the graph actually runs at from `pw-top` (QUANT and RATE on the AirMic line). `clock.quantum` in `pw-metadata` is only a setting and can differ.

## 4. Behaviour

- [ ] **Mute:** tap the mute button. The phone turns orange and `pw-record` hears silence. Unmute and the audio is back at once.
- [ ] **Wi-Fi blip:** turn Wi-Fi off on the phone for 5 s, then on. The phone shows **Reconnecting** and is back on air within about 3 s, with no code asked.
- [ ] **Daemon restart:** stop `airmicd` with Ctrl+C and start it again. The phone reconnects on its own, with no code asked (`~/.config/airmic/paired.json` keeps the token).
- [ ] **Locked screen:** lock the phone and keep talking for 1 minute. The audio keeps flowing.
- [ ] **Call or Siri (M2.9):** trigger Siri. The phone shows **Paused**, then goes back on air by itself.
- [ ] **AirPods (M2.9):** connect AirPods mid-session. The audio continues, now from the AirPods mic.

## If something fails

| Symptom | Likely cause |
|---|---|
| PC not under Nearby | mDNS blocked: open 5353/udp, or use **Enter IP address** (`ip -4 addr`, port 47800) |
| "Couldn't reach …" | `airmicd` not running, or TCP 47800 blocked |
| On air, but `pw-record` is silent | UDP 47801 blocked by a firewall |
| "in use by another phone" | Another session is active. Restart `airmicd` |
| Code rejected | Codes last 2 minutes. Reconnect to get a fresh one. After 5 wrong codes, restart `airmicd` |

## Results

Copy this into `docs/notes/m1.md` and commit it with a short note:

```
Date:            YYYY-MM-DD
PC / Ubuntu:     <model> / <version>, PipeWire <pw-cli --version>
Phone / iOS:     <model> / <version>, app commit <sha>
Quantum / rate:  <clock.quantum> / <clock.rate>
Latency (pill):  <ms>
Each check:      pass / fail + note
```
