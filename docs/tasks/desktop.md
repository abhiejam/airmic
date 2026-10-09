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

## v1 scope: CLI first (decided 2026-10-09)

v1 ships as an open source release of the daemon with a small CLI, not the Tauri app. The goal is to gauge interest cheaply. iPhone users sideload the app from Xcode (free Apple ID, re-sign every 7 days). An App Store build and the desktop app follow only if v1 gets traction.

- **v1:** D2.12, D4, D7.
- **Deferred to v2:** D5 (desktop app) and D6 (transcription, so S4 waits too).
- **Dropped:** D2.10 (pipewire-rs works).

## Status and handover (2026-10-03)

Read this before starting desktop work. Everything below was checked on this machine on 2026-10-03; re-check anything you rely on.

**Works end to end with the real iPhone:** discovery (mDNS), pairing with the 4 digit code from the daemon log, token reconnect, streaming into PipeWire, Claude Code `/voice` dictation through AirMic, mute, Wi-Fi blip recovery. S1, S2 and S3 are done. PC-side results: [`docs/notes/m1.md`](../notes/m1.md).

**Known issues, in priority order**
1. **Audio underruns (D2.6).** The phone stream has about 10 underruns and 23 dropped frames per minute on `main` (each a short gap). Jitter is low (~1.7 ms), so the cause is bursty arrival, drift, or both. Branch `desktop/jitter-target` (pushed, no PR) sizes the target from the worst arrival delay over 10 s: it fixes a synthetic 80 ms stall test, but on the real phone it only cut underruns to ~6/min while the target sat at the 120 ms cap, so it is not the whole answer. Investigated 2026-10-03 with `AIRMIC_PACKET_TRACE=<file> airmicd` and `tools/analyze-packet-trace.py`: the cause is Wi-Fi link stalls, not clock drift (about -14 ppm). Details and numbers in [`docs/notes/underruns.md`](../notes/underruns.md). **Low priority** (the user has not heard it while dictating with `/voice`). Options if it is picked up again: Ethernet for the PC, a higher target cap (160 ms would prevent about 23 of 28 underruns in the capture, at that much latency), or concealing short gaps. Do not merge `desktop/jitter-target` as is.
2. **Phone drops the session right after reconnecting to a restarted daemon** (mobile track). The daemon log shows `session … ready` then `ended` 2.5 s later with no error, so the phone closes it cleanly. A prompt with this evidence was handed to the iOS session. Desktop needs no change.
3. The daemon sets AirMic as the default source on start and does not restore the previous default on exit. Fix written in D4.6, live check pending.
4. ~~If PipeWire is unreachable at start, the daemon exits with the right error but also prints a Tokio "context is being shutdown" panic.~~ Fixed in D4.7.
5. `docs/notes/iphone-linux-checklist.md` (mobile's file) uses `pactl`, `avahi-browse` and `sudo ufw`, none of which work here, and its `clock.quantum` read shows 1024 while the graph runs at 480 (`pw-top` shows the real value).

**Next, in order (CLI-first v1, see above):** D4 CLI and fixes for issues 3 and 4 → D2.12 install and reboot test (ask the user before installing anything) → D7.

**This machine:** Ubuntu 24.04.5, PipeWire 1.0.5, WirePlumber 0.4.17, Rust 1.99. `pactl` and `avahi-browse` are not installed; use `pw-cli`, `pw-dump`, `wpctl`, `pw-metadata`, `pw-top`. ufw is installed but disabled. LAN IP 192.168.20.42 on `wlp3s0`, hostname `nuc`. The user's own default mic is not AirMic: never change it (or install units, or use sudo) without asking, and restore it after a test.

**Running and testing**
- Real run: `cd desktop && cargo build --release -p airmicd && ./target/release/airmicd` (pairing on; the code appears in the log as `pairing code NNNN`). `--no-auth` skips pairing. `airmic-send --port <p> --seconds N [--loss 5 --jitter 20 --reorder 2]` plays the phone.
- For test daemons use spare ports and keep the default mic alone: a scratch `config.toml` with `control_port`, `audio_port` (e.g. 47860/47861) and `set_default_source = false`, passed with `--config`.
- A test `XDG_RUNTIME_DIR` must give a socket path under 108 bytes, and then PipeWire needs `PIPEWIRE_RUNTIME_DIR=/run/user/1000` to still find its own socket.
- Record what apps hear: `pw-record --target airmic --rate 48000 --channels 1 out.wav`.
- Before each PR: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` (53 tests on `main`).

**Workflow:** see "Branches, stacks and conflicts" in `CLAUDE.md`. Each PR is merged with "Rebase and merge", so restack open branches after every merge. `gh` here must use `GH_TOKEN=$(gh auth token --user abhiejam)`. No `Co-Authored-By` lines.

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
- [x] **D1.3** Verify with `pw-record --target airmic` and Claude Code `/voice`; note PipeWire quantum and latency in `docs/notes/m1.md`
  - Done with the real daemon instead of the spike script: phone speech dictated through `/voice` on 2026-10-03. Graph quantum 480/48000; see `docs/notes/m1.md`.

## D2 · Protocol crate and daemon core
- [x] **D2.1** `airmic-proto`: header struct, encode/decode, control message enums (serde, newline JSON)
- [x] **D2.2** Tests against `vectors.json`
- [x] **D2.3** `airmic-send` CLI: stream a sine tone or WAV with the real protocol; flags for loss %, jitter ms, reorder
- [x] **D2.4** `airmicd` skeleton: tokio, `config.toml` (`directories` crate for paths), tracing to journald
  - Logs go to stderr, which journald captures under systemd (no time prefix there). No journald crate.
- [x] **D2.5** UDP receiver on 47801: validate magic, version, session id
- [x] **D2.6** Jitter buffer: reorder by sequence, adaptive 20–120 ms target, silence + short fade on loss, stats (loss, jitter)
  - Pull model: the sink reads samples at its own clock. Frames above target + 40 ms are dropped on every push and read (so the buffer stays short even when nothing records), and an underrun rebuffers. Target = 20 ms + 4 × jitter, clamped to 20–120 ms.
  - Open: underruns on the real phone stream, see "Known issues" 1 above.
- [x] **D2.7** Jitter buffer tests: loss, duplicates, reorder, late packets, sequence wrap
- [x] **D2.8** `AudioSink` trait (write frames, report underruns)
  - Pull model: `AudioSink::run` reads from the jitter buffer at the device clock, so it has no `write`. Underruns are counted in the jitter buffer stats.
- [x] **D2.9** PipeWire backend: virtual source `node.name=airmic`, `node.description=AirMic`, silence when no phone
  - `media.class` is `Audio/Source`, not `Audio/Source/Virtual`: WirePlumber 0.4.17 (Ubuntu 24.04) never creates ports for the virtual class. Requests 10 ms periods (`node.latency=480/48000`).
- [ ] ~~**D2.10** Fallback backend: `module-pipe-source` FIFO (if pipewire-rs gives trouble)~~
  - Dropped for v1: pipewire-rs 0.10 works on PipeWire 1.0.5.
- [x] **D2.11** Set AirMic as default source by name on start (configurable)
  - Via `pw-metadata` (`default.configured.audio.source`), config `set_default_source` (default true). Verified live 2026-10-03 (`wpctl status` marks AirMic as default). D4.6 restores the previous default on exit.
- [ ] **D2.12** systemd user unit `packaging/airmicd.service`, start on login
  - `packaging/airmicd.service` written (`/usr/bin/airmicd`, restart on failure). Pairing has landed, so the daemon now starts without `--no-auth`. `airmicd install` (D4.5) writes it with `ExecStart` set to the running binary. Real install and reboot test pending user approval: `airmicd install`, reboot, log in, `airmicd status`.

Done when: `airmic-send` with 5% loss sounds clean through the "AirMic" input, and it survives a reboot.

## D3 · Control, discovery, pairing
- [x] **D3.1** Control server TCP 47800: `hello`, `ready`, `ping/pong`, `mute`, `bye`, with `--no-auth` flag → **S2**
  - `--no-auth` skips pairing (development only).
- [x] **D3.2** Periodic `stats` to the phone every 2 s
  - Sent with each ping once the session is ready.
- [x] **D3.3** Pairing: 4 digit code (2 min, 5 attempts), 128 bit tokens, `~/.config/airmic/paired.json` → **S3**
  - Until D3.7, a `pair_required` issues a code and the daemon logs it. After a lockout only a new code from IPC (or a daemon restart) unlocks. A code pairs one phone, then is retired.
- [x] **D3.4** mDNS advert `_airmic._tcp` with TXT `id`, `name`, `v` (`mdns-sd` crate) → **S3**
  - Computer id is a UUID v4 kept in `~/.config/airmic/device_id`; instance name is the hostname.
- [x] **D3.5** One active phone at a time; new phone gets a clear "busy" error
- [x] **D3.6** Mute flag and header-only packets → silence output
- [x] **D3.7** IPC server behind a trait (Unix socket `$XDG_RUNTIME_DIR/airmic.sock`), JSON-RPC: `status`, `level`, `pairing_code`, `paired_devices`, `forget_device`, `settings`
  - Contract in `docs/ipc.md` (adds `make_default`, `subscribe`; `settings` split into get/set). The app (D5) builds against it in parallel.
  - `paired.json` keeps `paired_at` and `last_seen` in Unix seconds (older files without `last_seen` still load); IPC converts to ms. `last_seen` is written when a phone's session ends. `forget_device` makes the control server send `bye` and close that phone's connection. The QR `host` is the default-route address (UDP connect trick), `port` the running control port.

Done when: phone discovers the PC, pairs with the code, reconnects with its token; `airmic-send` covers the same in tests.

## D4 · CLI (v1)
Subcommands on the `airmicd` binary, so the release ships one binary. Each one is a thin client over the IPC calls in `docs/ipc.md`. Plain `airmicd` still runs the daemon.
- [x] **D4.1** `airmicd status`: phone name, connected or idle, muted, latency, loss, whether AirMic is the default mic
  - Code in `crates/airmicd/src/cli.rs`. The CLI honours `AIRMIC_SOCKET` like the app.
- [x] **D4.2** `airmicd pair`: print the 4 digit code and a terminal QR code, and wait until the phone pairs or the code expires
  - QR via the `qrcode` crate without default features (no dependencies). It is drawn light on dark for a dark terminal and has not been scanned with the real phone yet.
- [x] **D4.3** `airmicd devices` and `airmicd forget <id>`
  - `forget` also takes a unique start of the id.
- [x] **D4.4** `airmicd make-default`, plus a hint in `status` when AirMic is not the default
  - `make-default` is tested against a fake daemon only, so the live run left the default mic alone.
- [x] **D4.5** `airmicd install` and `airmicd uninstall`: write and enable or remove the systemd user unit from D2.12 for the current binary path
  - `src/install.rs` renders `packaging/airmicd.service` via `include_str!`. Re-running is safe; a unit for another binary is replaced and restarted. `install --dry-run` prints the unit. Unit-tested against a temp dir with a fake systemctl; a real install is pending user approval (see D2.12).
- [ ] **D4.6** Restore the previous default source on exit (known issue 3)
  - Written 2026-10-09: the first `make_default` (start or IPC) records the configured default, and exit on SIGINT, SIGTERM or a sink failure writes it back, only while AirMic is still the default. When none was set, or it was already AirMic after a crash, it deletes the key so WirePlumber picks one. Unit tests cover the restore decision. Not live tested: the user's own daemon was running and owns the "airmic" node. Live check: note `pw-metadata 0 default.configured.audio.source`, start and stop the daemon, confirm the value is back.
- [x] **D4.7** Exit cleanly when PipeWire is unreachable, without the Tokio panic (known issue 4)
  - Cause: the status poller ran `pw-metadata` inside `block_in_place`, so its task outlived runtime shutdown and then touched a timer. Now `spawn_blocking`. Verified 2026-10-09: 20 runs with an empty `PIPEWIRE_RUNTIME_DIR` all exit 1 with only the error (before: 2 panics in 3 runs).
- [x] **D4.8** Firewall hint: phone connected on TCP but no UDP → log a warning and show it in `status` with the command to open the port
  - Daemon side: IPC `status` has `audio_blocked` (no packet 5 s after `ready`) and `audio_port`; the warning with `sudo ufw allow <port>/udp` is logged once per session. Showing it in `airmicd status` is D4.1.

Done when: on a clean Ubuntu machine, unpack the release, run `airmicd install` and `airmicd pair`, pair the phone, and dictate through AirMic. It still works after a reboot.

## D5 · Desktop app (Tauri), deferred to v2
- [x] **D5.1** Scaffold Tauri v2 + React + TS + Vite in `desktop/app`
  - `desktop/app/src-tauri` has its own `[workspace]`, so the daemon's `cargo ... --workspace` CI does not need the webkit libraries. App CI is the `app` job in `desktop.yml`.
  - Run: `cd desktop/app && npm ci && npm run tauri dev`. Set `AIRMIC_SOCKET` to talk to a test daemon instead of `$XDG_RUNTIME_DIR/airmic.sock`.
- [x] **D5.2** Rust side: IPC client to `airmicd`, events to the web UI
  - `src-tauri/src/ipc_client.rs`: one connection, auto reconnect, subscribes to `status` and `level` itself (add `transcript` with D6.3). The web UI calls any daemon method through the `ipc_call` command (`src/ipc.ts`) and gets the `daemon-connection` and `daemon-notification` events (`src/useDaemon.ts`).
- [x] **D5.3** Design tokens matching the phone (system sans, cream / dark, indigo accent)
  - `src/theme.css` copies the values in `ios/AirMic/UI/Theme.swift`, including increase-contrast. There are no desktop mockups in the design canvas (phone only), so the desktop screens follow PRD §8.3.
- [ ] **D5.4** Status screen: phone name, live level meter, latency, loss, "default mic" check + fix button
- [ ] **D5.5** Pair screen: QR code + 4 digit code, paired phones list, forget
- [ ] **D5.6** Settings: start at login, set as default mic, ports, transcription toggle and model
- [ ] **D5.7** Tray icon (connected / muted / idle); closing window keeps daemon running
- [ ] **D5.8** First launch: install/enable the systemd user service, no CLI
- [ ] **D5.9** Firewall detection (phone connected on TCP but no UDP) → show the `ufw` command
- [ ] **D5.10** Packaging: `.deb` and AppImage bundling `airmicd` and the unit

Done when: on a clean Ubuntu VM, install the `.deb`, open the app, pair the phone, use the mic, with no terminal.

## D6 · Transcription, deferred to v2
- [ ] **D6.1** `whisper-rs` behind a cargo feature; model download (`base.en` default, `small.en` option) with progress
- [ ] **D6.2** Simple voice activity detection to cut chunks on silence
- [ ] **D6.3** Transcript stream over IPC; Transcript screen in the app (scroll, copy, clear)
- [ ] **D6.4** Send `transcript {text, final}` to the phone → **S4**
- [ ] **D6.5** CPU usage check; auto-disable suggestion on slow machines

## D7 · Hardening and release
- [ ] **D7.1** 2 h soak with `airmic-send` and with the phone; memory and CPU flat
- [ ] **D7.2** Measure end to end latency (target < 150 ms) and idle CPU (< 1%)
- [ ] **D7.3** Run the end to end checklist (PRD §12) on the desktop side
- [x] **D7.4** README: install, pair, troubleshooting (firewall, AP isolation, default device)
  - Written against the D4 subcommand names. Re-check the CLI reference and the firewall hint wording once D4 lands.
  - The iPhone sideload guide already exists (`docs/ios-install.md`, M7.5). Make it prominent in the install steps and state that a Mac with Xcode is needed for now.
- [ ] **D7.5** GitHub release v1.0: `airmicd` x86_64 Linux tarball with the unit file and README
  - `.deb` and AppImage move to v2 with D5.10.
  - `.github/workflows/release.yml` builds the tarball (plus a `.sha256`) and creates a draft release on a `v*` tag. No tag pushed yet.
- [x] **D7.6** Open source prep: contributing notes, issue templates, and a feedback ask in the README ("would you pay for an App Store build?")
  - `CONTRIBUTING.md`; bug report, feature request and feedback forms in `.github/ISSUE_TEMPLATE/`. The README feedback ask links the feedback form (Discussions may not be enabled).

## Later (v2)
- [ ] **L.1** Dictation: study `whisrs`, Wayland text injection, push-to-talk from phone, desktop hotkey
- [ ] **L.2** macOS quick path: `cpal` sink into BlackHole, launchd agent, `.dmg`
- [ ] **L.3** Windows quick path: `cpal` (WASAPI) sink into VB-Cable, named pipe IPC, startup task, `.msi`, firewall rule
- [ ] **L.4** macOS own HAL plug-in (needs Developer ID, notarization)
- [ ] **L.5** Opus decode (codec flag 1)
- [ ] **L.6** Audio decryption (ChaCha20-Poly1305)
