# Desktop tasks: receiver and desktop app

Track: `desktop/` · Rust (tokio), Tauri v2 + React/TS · Spec: [PRD §8](../PRD.md#8-desktop-back-end), protocol [PRD §6](../PRD.md#6-wire-protocol-v1)
Other track: [mobile.md](mobile.md)

Legend: `[ ]` todo, `[x]` done. **Unblocks** = a mobile task waiting on this. **Done when** = acceptance check.

## Sync points with mobile

| # | What | Desktop task | Unblocks mobile |
|---|---|---|---|
| S1 | `docs/protocol.md` + `docs/protocol/vectors.json` frozen | D0.4 | M2.1, M2.2 |
| S2 | Control server without pairing (`airmicd --no-auth`) | D3.1 | M2.4 |
| S3 | Pairing + Bonjour advert | D3.3, D3.4 | M3.1, M3.3 |
| S4 | `transcript` messages to phone | D6.4 | M6.1 |

Do S1 and S2 first: they are what lets both tracks run in parallel.

---

## D0 · Setup
- [x] **D0.1** Cargo workspace `desktop/` with crates `airmic-proto`, `airmicd`, `airmic-send`
- [x] **D0.2** GitHub Actions: `cargo fmt --check`, `clippy -D warnings`, `cargo test`; Xcode build job later
  - `.github/workflows/desktop.yml`, runs only on `desktop/**` changes.
- [x] **D0.3** Root `README.md`, `LICENSE` (MIT), `.gitignore`
  - `.gitignore` already existed and covered everything; left unchanged.
- [x] **D0.4** Write `docs/protocol.md` from PRD §6 and `docs/protocol/vectors.json` (header bytes and JSON messages with expected encodings); review with mobile → **S1**
  - Added an `error {code, message}` message (needed for D3.5 "busy"); also in PRD §6.

Done when: CI green on an empty workspace, protocol doc agreed.

## D1 · Audio path spike (Linux, no code)
- [x] **D1.1** `tools/spike-receiver.sh`: FIFO + `pactl load-module module-pipe-source source_name=airmic … rate=48000 channels=1` + `nc -klu 5555` (unblocks M1.6)
  - `pactl` is not installed here, so it loads `module-pipe-tunnel` through `pw-cli` instead. Verified locally: a 440 Hz sine over UDP recorded back exactly with `pw-record`.
- [x] **D1.2** Check firewall (`ufw status`), open the spike port if needed
  - ufw is installed but disabled (`ENABLED=no`). No rule needed.
- [ ] **D1.3** Verify with `pw-record --target airmic` and Claude Code `/voice`; note PipeWire quantum and latency in `docs/notes/m1.md`

## D2 · Protocol crate and daemon core
- [ ] **D2.1** `airmic-proto`: header struct, encode/decode, control message enums (serde, newline JSON)
- [ ] **D2.2** Tests against `vectors.json`
- [ ] **D2.3** `airmic-send` CLI: stream a sine tone or WAV with the real protocol; flags for loss %, jitter ms, reorder
- [ ] **D2.4** `airmicd` skeleton: tokio, `config.toml` (`directories` crate for paths), tracing to journald
- [ ] **D2.5** UDP receiver on 47801: validate magic, version, session id
- [ ] **D2.6** Jitter buffer: reorder by sequence, adaptive 20–120 ms target, silence + short fade on loss, stats (loss, jitter)
- [ ] **D2.7** Jitter buffer tests: loss, duplicates, reorder, late packets, sequence wrap
- [ ] **D2.8** `AudioSink` trait (write frames, report underruns)
- [ ] **D2.9** PipeWire backend: virtual source `node.name=airmic`, `node.description=AirMic`, silence when no phone
- [ ] **D2.10** Fallback backend: `module-pipe-source` FIFO (if pipewire-rs gives trouble)
- [ ] **D2.11** Set AirMic as default source by name on start (configurable)
- [ ] **D2.12** systemd user unit `packaging/airmicd.service`, start on login

Done when: `airmic-send` with 5% loss sounds clean through the "AirMic" input, and it survives a reboot.

## D3 · Control, discovery, pairing
- [ ] **D3.1** Control server TCP 47800: `hello`, `ready`, `ping/pong`, `mute`, `bye`, with `--no-auth` flag → **S2**
- [ ] **D3.2** Periodic `stats` to the phone every 2 s
- [ ] **D3.3** Pairing: 4 digit code (2 min, 5 attempts), 128 bit tokens, `~/.config/airmic/paired.json` → **S3**
- [ ] **D3.4** mDNS advert `_airmic._tcp` with TXT `id`, `name`, `v` (`mdns-sd` crate) → **S3**
- [ ] **D3.5** One active phone at a time; new phone gets a clear "busy" error
- [ ] **D3.6** Mute flag and header-only packets → silence output
- [ ] **D3.7** IPC server behind a trait (Unix socket `$XDG_RUNTIME_DIR/airmic.sock`), JSON-RPC: `status`, `level`, `pairing_code`, `paired_devices`, `forget_device`, `settings`

Done when: phone discovers the PC, pairs with the code, reconnects with its token; `airmic-send` covers the same in tests.

## D5 · Desktop app (Tauri)
- [ ] **D5.1** Scaffold Tauri v2 + React + TS + Vite in `desktop/app`
- [ ] **D5.2** Rust side: IPC client to `airmicd`, events to the web UI
- [ ] **D5.3** Design tokens matching the phone (system sans, cream / dark, indigo accent)
- [ ] **D5.4** Status screen: phone name, live level meter, latency, loss, "default mic" check + fix button
- [ ] **D5.5** Pair screen: QR code + 4 digit code, paired phones list, forget
- [ ] **D5.6** Settings: start at login, set as default mic, ports, transcription toggle and model
- [ ] **D5.7** Tray icon (connected / muted / idle); closing window keeps daemon running
- [ ] **D5.8** First launch: install/enable the systemd user service, no CLI
- [ ] **D5.9** Firewall detection (phone connected on TCP but no UDP) → show the `ufw` command
- [ ] **D5.10** Packaging: `.deb` and AppImage bundling `airmicd` and the unit

Done when: on a clean Ubuntu VM, install the `.deb`, open the app, pair the phone, use the mic, with no terminal.

## D6 · Transcription
- [ ] **D6.1** `whisper-rs` behind a cargo feature; model download (`base.en` default, `small.en` option) with progress
- [ ] **D6.2** Simple voice activity detection to cut chunks on silence
- [ ] **D6.3** Transcript stream over IPC; Transcript screen in the app (scroll, copy, clear)
- [ ] **D6.4** Send `transcript {text, final}` to the phone → **S4**
- [ ] **D6.5** CPU usage check; auto-disable suggestion on slow machines

## D7 · Hardening and release
- [ ] **D7.1** 2 h soak with `airmic-send` and with the phone; memory and CPU flat
- [ ] **D7.2** Measure end to end latency (target < 150 ms) and idle CPU (< 1%)
- [ ] **D7.3** Run the end to end checklist (PRD §12) on the desktop side
- [ ] **D7.4** README: install, pair, troubleshooting (firewall, AP isolation, default device)
- [ ] **D7.5** GitHub release v1.0 with `.deb` and AppImage

## Later (v2)
- [ ] **L.1** Dictation: study `whisrs`, Wayland text injection, push-to-talk from phone, desktop hotkey
- [ ] **L.2** macOS quick path: `cpal` sink into BlackHole, launchd agent, `.dmg`
- [ ] **L.3** Windows quick path: `cpal` (WASAPI) sink into VB-Cable, named pipe IPC, startup task, `.msi`, firewall rule
- [ ] **L.4** macOS own HAL plug-in (needs Developer ID, notarization)
- [ ] **L.5** Opus decode (codec flag 1)
- [ ] **L.6** Audio decryption (ChaCha20-Poly1305)
