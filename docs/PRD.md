# AirMic: product and technical plan

Status: draft v1, 2026-10-03
Owner: Abhishek
Design: [AirMic iPhone mockups](https://claude.ai/artifact/SFuouTH2921fG9QzJidshH) (approved)

---

## 1. Summary

AirMic turns an iPhone into a wireless microphone for a desktop computer.
The iPhone app captures the mic and streams it over local Wi-Fi to a small receiver on the computer.
The receiver exposes a virtual microphone ("AirMic") that any app can use: Claude Code `/voice`, Cursor, Zoom, Discord, browsers.
A desktop window shows connection status, pairing and a live transcript.
Later, the same pipeline powers system-wide dictation into any text box.

It is an open source personal tool and portfolio piece, not a business.
Existing mic-streaming apps cap free streaming at 1 hour, show 2 minute ads, and have a disliked UI.

## 2. Goals and non-goals

### Goals (v1)
1. Stream iPhone mic audio to Linux (Ubuntu, PipeWire, GNOME) with low latency (< 150 ms mouth to app).
2. Show up as a normal input device, set as default automatically, selectable by name.
3. Keep streaming with the iPhone screen locked, for hours, with no time limits and no ads.
4. Zero CLI setup on the desktop: install a package, open the app, scan or type a code.
5. Calm, focused iPhone UI: one big mic, sleek mute, focus session timer, session summary.
6. Live transcript in the desktop app.

### Later (v2+)
- System-wide dictation into the focused text box (Wayland), superwhisper style.
- Opus encoding, macOS and Windows receivers, TestFlight / App Store.

### Non-goals
- Streaming over the internet (LAN only).
- Several phones streaming to one computer at the same time.
- Recording or storing audio on either side.
- Accounts, cloud services, analytics.

## 3. Users and key flows

Primary user: a developer at a desk with a Linux machine who wants a good mic for voice coding, dictation and calls, without buying hardware.

### Flow A: first-time setup
1. Install AirMic on Linux (`.deb` or AppImage). The desktop app opens and starts the background service.
2. Desktop app shows "Waiting for your phone" with a QR code and a 4 digit pairing code.
3. Install the iPhone app (sideloaded from Xcode for now). Grant microphone and Local Network permission.
4. Phone lists nearby computers (Bonjour). User taps one and enters the 4 digit code, or scans the QR.
5. Paired. Phone remembers the computer; desktop remembers the phone.

### Flow B: daily use
1. Open AirMic on the phone. It reconnects to the last computer automatically.
2. Home screen shows "Live on ubuntu-desk", the focus timer starts, the big mic reacts to the voice.
3. On the desktop, the "AirMic" input is already the default. Use `/voice`, a call, anything.
4. Tap the floating mute button to mute (the PC hears silence, the connection stays up).
5. Tap "End session" to see the session summary.

### Flow C: phone not connected
Home screen shows "No computer connected" and a "Connect to a computer" button leading to the Connect screen (nearby list, pairing code, Scan QR, Enter IP).

## 4. System architecture

```
┌──────────────────────── iPhone (Swift, SwiftUI) ───────────────────────┐
│  AudioCapture ──► Encoder/Packetizer ──► AudioSender (UDP)             │
│  (AVAudioEngine)   (PCM 10 ms frames)          │                        │
│        │ level (RMS)                           │                        │
│        ▼                                       │                        │
│  SessionStore ◄── ControlClient (TCP, JSON) ◄──┼── Discovery (NWBrowser)│
│  (timer, stats)        │                       │                        │
│        ▼               │                       │                        │
│  SwiftUI screens: Home · Connect · Summary · Settings                   │
└────────────────────────┼───────────────────────┼────────────────────────┘
                 TCP 47800 (control)     UDP 47801 (audio)      Wi-Fi LAN
┌────────────────────────┼───────────────────────┼──── Linux ────────────┐
│ airmicd (Rust, user systemd service)           ▼                        │
│  ControlServer ◄─► PairingStore    AudioReceiver ─► JitterBuffer        │
│       │                                              │                  │
│  mDNS advert (_airmic._tcp)                          ▼                  │
│       │                                  PipeWire virtual source "AirMic"│
│       │                                              │ (tap)            │
│  IPC server (Unix socket) ◄──── Transcriber (whisper.cpp, optional)     │
└───────┼─────────────────────────────────────────────────────────────────┘
        ▼
  AirMic desktop app (Tauri v2): status · pairing QR · level · transcript · settings
```

Key choice: the **daemon owns everything audio-related** and runs without the UI.
The Tauri app is only a client of the daemon, so closing the window never drops the mic.

## 5. Repository layout

Monorepo, MIT license.

```
airmic/
  ios/                     Xcode project "AirMic" (SwiftUI app)
    AirMic/
      App/                 entry point, app state
      Audio/               AudioCapture, Packetizer, LevelMeter
      Network/             ControlClient, AudioSender, Discovery, Protocol
      Session/             SessionStore, stats, persistence
      UI/                  Home, Connect, Summary, Settings, components
    AirMicTests/
  desktop/                 Rust workspace
    crates/
      airmic-proto/        wire protocol types, encode/decode (shared, tested)
      airmicd/             the daemon
      airmic-send/         test sender CLI: streams a WAV file or tone (no phone needed)
    app/                   Tauri v2 app (src-tauri/ + web UI)
    packaging/             systemd unit, .deb / AppImage config
  docs/
    PRD.md                 this file
    tasks/                 mobile.md, desktop.md (parallel task lists)
    protocol.md            wire protocol spec (from section 6)
```

## 6. Wire protocol (v1)

Two channels between phone (client) and daemon (server).

### 6.1 Discovery
- Daemon advertises Bonjour/mDNS service `_airmic._tcp` on port 47800 via Avahi.
- TXT record: `id=<device uuid>`, `name=<hostname>`, `v=1`.
- iPhone browses with `NWBrowser`. Fallbacks: QR code (contains host, port, id, pairing code) and manual IP entry.

### 6.2 Control channel: TCP 47800
Newline-delimited JSON messages. Phone connects, daemon answers.

| Message | Direction | Purpose |
|---|---|---|
| `hello {v, phone_id, phone_name}` | phone → PC | open session |
| `pair_required` / `pair {code}` / `paired {token}` | both | first-time pairing with 4 digit code shown on PC |
| `auth {token}` | phone → PC | reconnect with stored token |
| `ready {session_id, udp_port, sample_rate}` | PC → phone | start sending audio |
| `mute {on}` | phone → PC | mute state (PC outputs silence) |
| `stats {loss_pct, jitter_ms, latency_ms}` | PC → phone | every 2 s, for the status pill |
| `transcript {text, final}` | PC → phone | optional, for "Last thing you said" and word count |
| `ping` / `pong` | both | keepalive every 2 s, timeout 6 s |
| `bye` | both | end session |
| `error {code, message}` | PC → phone | `busy`, `bad_code`, `pair_locked`, `unsupported_version`, `bad_message` |

- Exact framing, flows and test vectors: [`protocol.md`](protocol.md), [`protocol/vectors.json`](protocol/vectors.json).
- Pairing token: 128 bit random, stored in iOS Keychain and in `~/.config/airmic/paired.json`.
- Pairing code: 4 digits, shown in desktop app, valid 2 minutes, 5 attempts.

### 6.3 Audio channel: UDP 47801
- Format: PCM signed 16 bit little endian, 48 kHz, mono.
- Frame: **10 ms = 480 samples = 960 bytes** (keeps each packet under the 1472 byte Wi-Fi MTU, no fragmentation).
- About 100 packets/s, about 100 KB/s. Fine on any home Wi-Fi.
- Header, 16 bytes, big endian:

| Field | Size | Notes |
|---|---|---|
| magic `"AM"` | 2 | reject anything else |
| version | 1 | 1 |
| flags | 1 | bit0 muted, bit1-3 codec (0 = PCM, 1 = Opus later) |
| session_id | 4 | from `ready`; packets with another id are dropped |
| sequence | 4 | +1 per packet, detects loss and reordering |
| timestamp | 4 | sample index since session start |

- Muted: phone keeps sending header-only packets at 10 per second; the daemon outputs silence. No reconnect lag on unmute.

### 6.4 Security (v1)
- LAN only, unencrypted audio. Acceptable for a personal tool on a home network.
- Only paired phones get a `session_id`, so strangers on the network cannot inject audio.
- v2: encrypt audio with ChaCha20-Poly1305 using a key from pairing.

## 7. iPhone app (front end)

### 7.1 Tech
- Swift 6, SwiftUI, iOS 18 minimum. No third-party dependencies in v1.
- `AVAudioEngine` for capture, `Network.framework` for TCP, UDP and Bonjour, Keychain for tokens, SwiftData for session history.

### 7.2 Modules
| Module | Responsibility |
|---|---|
| `AudioCapture` | `AVAudioSession` (`.playAndRecord`, mode `.voiceChat` for echo cancellation, toggle `.measurement` in settings), input tap, `AVAudioConverter` to 48 kHz mono Int16, emits 10 ms frames and RMS level. Handles interruptions (calls, Siri) and route changes (AirPods), resumes automatically. |
| `Packetizer` | adds the 16 byte header, sequence and timestamp. Pure and unit tested. |
| `AudioSender` | `NWConnection` UDP to the daemon. |
| `ControlClient` | TCP JSON channel: hello, pairing, auth, mute, stats, keepalive, reconnect with backoff. |
| `Discovery` | `NWBrowser` for `_airmic._tcp`; QR scanner (`AVCaptureMetadataOutput`); manual IP. |
| `SessionStore` | focus session: start time, goal (default 50 min), mutes, dropouts, words; saved to SwiftData for the weekly chart. |
| `UI` | screens below. |

### 7.3 Screens (from the approved mockups)
1. **Home, streaming:** status pill ("Live on ubuntu-desk · 12 ms"), focus timer with goal bar, big accent mic with pulse rings and level bars driven by the real RMS level, "End session", floating mute button.
2. **Home, muted:** grey mic, no rings, orange mute button, caption "Muted · your PC hears silence".
3. **Home, no computer:** dashed mic, "Tap the mic to find your computer", "Connect to a computer" (280 px).
4. **Connect:** nearby list, inline 4 digit pairing code, "Computer not listed? Get the desktop app", Scan QR code, Enter IP address.
5. **Summary:** "52 minutes of deep work", stats (words, times muted, dropouts), this week chart, last thing you said, "Start another session" (280 px).
6. **Settings** (not mocked yet): paired computers, focus goal, audio mode (voice / raw), haptics, about.

Visual system: SF Pro (system font), cream `#F6F2EA` light / `#111114` dark, accent indigo `#5146E5`, muted orange `#C2410C`. Respect Reduce Motion (no pulse) and Dynamic Type.

### 7.4 Info.plist and capabilities
- `NSMicrophoneUsageDescription`: "AirMic streams your voice to your computer."
- `NSLocalNetworkUsageDescription`: "AirMic finds and connects to your computer on Wi-Fi."
- `NSBonjourServices`: `_airmic._tcp`
- `NSCameraUsageDescription` (QR scanning)
- `UIBackgroundModes`: `audio` (keeps the mic live with the screen locked)

### 7.5 Later
- Live Activity on the Lock Screen with mute button and timer.
- Control Center control and Action button shortcut (App Intents) for mute.
- Push-to-talk mode for dictation (hold the big mic).

## 8. Desktop (back end)

### 8.1 `airmicd` daemon (Rust)
- Runtime: `tokio`. Runs as a user systemd service (`systemctl --user`), starts on login.
- **ControlServer:** TCP 47800, the JSON protocol, pairing and auth, one active phone at a time.
- **AudioReceiver:** UDP 47801, validates header and session id, hands frames to the jitter buffer.
- **JitterBuffer:** reorders by sequence, target delay 40 ms (adaptive 20 to 120 ms), fills lost frames with silence (simple fade), reports loss and jitter.
- **Audio output abstraction:** the jitter buffer writes to an `AudioSink` trait, never to PipeWire directly. v1 ships the PipeWire backend; other platforms add backends (section 10). Same rule for IPC (`Unix socket` behind a trait, named pipe on Windows) and service install (systemd / launchd / Windows startup).
- **PipeWire output:** `pipewire` crate (pipewire-rs). Creates a node with `media.class = Audio/Source/Virtual`, `node.name = airmic`, `node.description = AirMic`. When no phone is connected it outputs silence, so apps never lose the device.
- **Default device:** on start, sets AirMic as the default source by name (PipeWire metadata `default.configured.audio.source`), configurable. Node ids change across restarts, so the name is the stable key.
- **mDNS:** advertises via Avahi (`zeroconf` or `mdns-sd` crate).
- **IPC:** Unix socket `$XDG_RUNTIME_DIR/airmic.sock`, JSON-RPC: `status`, `level` stream, `pairing_code`, `paired_devices`, `forget_device`, `transcript` stream, `settings`.
- **Config:** `~/.config/airmic/config.toml` (ports, default device, transcription on/off, model).
- **Logs:** journald.

### 8.2 Transcriber (in daemon, optional feature)
- `whisper-rs` (whisper.cpp), model `base.en` by default, `small.en` optional, downloaded on first enable.
- Taps the same audio, splits on silence with a simple voice activity detector, transcribes chunks.
- Sends text to the desktop app (IPC) and to the phone (`transcript` message) for word count and "Last thing you said".
- Off by default if the machine is slow; CPU only in v1.

### 8.3 Desktop app (Tauri v2)
- Front end: TypeScript, React, Vite. Same visual language as the phone (system sans, cream / dark, indigo accent).
- Talks only to `airmicd` over the Unix socket (Rust side of Tauri), never to the phone directly.
- Screens:
  1. **Status:** connected phone, live level meter, latency, loss, "AirMic is your default microphone" with a fix button.
  2. **Pair a phone:** QR code and 4 digit code, list of paired phones, forget.
  3. **Transcript:** live, scrolling, copy button, clear.
  4. **Settings:** start at login, set as default mic, ports, transcription model.
- Tray icon with connected / muted state. Closing the window keeps the daemon running.

### 8.4 Packaging
- `.deb` and AppImage from the Tauri bundler. The package installs `airmicd` and the systemd user unit; the app enables the service on first launch.
- Firewall: document `ufw allow 47800/tcp` and `47801/udp`; the desktop app detects a blocked port (phone sees the computer but no audio arrives) and shows the command.

## 9. Dictation (v2)
- Goal: speak, text appears in the focused input on Linux, any app.
- Read `whisrs` first and reuse its Wayland text injection (uinput / `ydotool`, or an input method) instead of writing our own.
- Trigger: hold the big mic on the phone (push to talk) or a desktop hotkey.

## 10. Other desktops: Mac mini, Windows (v2)

The phone app, wire protocol, jitter buffer, pairing, mDNS (`mdns-sd` is pure Rust), transcription and the Tauri app are all cross-platform already.
The one hard, platform-specific piece is **creating a virtual microphone**, because only Linux lets a normal app do that.

| Platform | Quick path (no driver of our own) | Full path (own driver) |
|---|---|---|
| Linux | PipeWire virtual source (v1) | done |
| macOS | Play into the free BlackHole virtual device with a `cpal` backend; user picks "BlackHole" as mic | Own Core Audio HAL plug-in (AudioServerPlugIn, C/C++ or `libASPL`), installed in `/Library/Audio/Plug-Ins/HAL`; needs admin install and a Developer ID ($99/yr) to sign and notarize |
| Windows | Play into VB-Audio Virtual Cable with a `cpal` (WASAPI) backend; user picks "CABLE Output" as mic | Own kernel audio driver (based on Microsoft's SysVAD sample); needs driver signing (EV certificate + Microsoft attestation). Hard and costly; avoid unless the project grows |

Other per-platform work: background service (launchd agent on macOS, startup task on Windows), IPC (named pipe on Windows), installer (`.dmg`, `.msi` from Tauri), firewall prompt on Windows.

Plan: ship the quick path first for each (about 1 week each), consider an own macOS driver later. Keeping `AudioSink`, IPC and service install behind traits from day one is what makes this cheap.

## 11. Performance and quality targets

| Metric | Target |
|---|---|
| End to end latency | < 150 ms (budget: 10 capture + 5 network + 40 jitter buffer + 20 PipeWire + slack) |
| Packet loss handled | up to 5% without audible breakup |
| Locked-screen streaming | 2 h continuous, no drop |
| iPhone battery | < 10% per hour streaming |
| Reconnect after Wi-Fi blip | < 3 s, automatic |
| Daemon idle CPU | < 1% |

## 12. Testing

- **Rust:** unit tests for `airmic-proto` (encode/decode round trips) and the jitter buffer (loss, reorder, late packets).
- **`airmic-send`:** CLI that streams a WAV or sine tone using the real protocol, so the daemon can be tested without a phone. Can simulate loss and jitter.
- **iOS:** unit tests for `Packetizer` and format conversion; UI previews for each state.
- **End to end checklist:** pair, stream to `pw-record`, Claude Code `/voice`, a Zoom/Meet call, lock screen 1 h, phone call interruption, AirPods connect, Wi-Fi off/on, PC reboot.

## 13. Milestones

| # | Milestone | Done when | Est. |
|---|---|---|---|
| M0 | Setup | Xcode + iOS platform installed, phone in Developer Mode, `git init`, repo layout, MIT license | ½ day |
| M1 | Audio path spike | Bare iPhone app (IP field + Start) sends raw PCM over UDP; Linux uses `module-pipe-source` + `nc`; voice heard in `pw-record` and `/voice`; screen-lock test passes | 1–2 days |
| M2 | Protocol + daemon | `airmic-proto`, `airmicd` with UDP receiver, jitter buffer, PipeWire source, default device by name, systemd unit; `airmic-send` works | 3–5 days |
| M3 | Control, discovery, pairing | TCP control channel, Bonjour, 4 digit pairing, tokens, reconnect, mute flag | 3–4 days |
| M4 | iPhone UI | Home (3 states), Connect, Summary, Settings built from the mockups, real level meter, focus session store | 4–6 days |
| M5 | Desktop app | Tauri app: status, pairing QR, settings, tray; IPC to daemon; `.deb` / AppImage | 1–2 weeks |
| M6 | Transcription | whisper.cpp in daemon, transcript in desktop app and phone summary | 3–5 days |
| M7 | Hardening | test checklist passes, battery and latency measured, README with GIFs, v1.0 GitHub release | 3–5 days |
| M8 | Dictation (v2) | text injection into focused field on Wayland | later |
| M9 | Mac and Windows (v2) | quick-path backends (BlackHole, VB-Cable), launchd / Windows startup, installers | ~1 week each |

Task breakdown for parallel work: [`tasks/mobile.md`](tasks/mobile.md) and [`tasks/desktop.md`](tasks/desktop.md).

## 14. Risks

| Risk | Mitigation |
|---|---|
| Free Apple ID builds expire every 7 days | Re-run from Xcode weekly; move to $99/yr program if it gets annoying |
| iOS stops the mic in the background | `audio` background mode, test lock screen early (M1) |
| Calls / Siri take the audio session | handle interruptions, auto-resume, show "Paused by a call" |
| Router client isolation blocks phone to PC | detect (discovery works, no audio), suggest phone hotspot |
| Wi-Fi power saving causes latency spikes | adaptive jitter buffer; measure in M1 |
| PipeWire API churn in Rust bindings | keep PipeWire code in one module; fallback to `module-pipe-source` |
| GNOME Settings not listing the virtual source | set default by name ourselves; desktop app shows a "make default" button |
| Name clash with existing "AirMic" apps | check before any App Store release |

## 15. Decisions log

| Decision | Choice | Why |
|---|---|---|
| iPhone app | Native Swift / SwiftUI | Background mic capture with screen locked; Safari can't |
| Desktop language | Rust | Fits Tauri, whisper-rs, `whisrs`; one language for daemon and app |
| Audio v1 | Raw PCM, 10 ms frames | Simplest, no codec on either side, LAN has the bandwidth |
| Transport | UDP audio + TCP control | Audio must never wait on retransmits; control must be reliable |
| Discovery | Bonjour first, QR and manual IP fallback | Zero typing in the common case |
| Transcription | On desktop (whisper.cpp) | Audio is already there; feeds transcript and later dictation |
| Font | SF Pro (system) | User preference; free in SwiftUI |
| Distribution | Sideload, GitHub releases | No App Store until people ask |

## 16. Open questions
1. Repo name and GitHub visibility (public from day one?).
2. Focus features beyond timer and goal: Do Not Disturb integration, session history screen, streaks?
3. Should the phone also work as a speaker (hear the PC) for calls? Not planned for v1.
4. Settings screen design (not mocked yet).
