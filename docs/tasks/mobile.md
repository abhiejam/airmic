# Mobile tasks: iPhone app

Track: `ios/` · Swift 6, SwiftUI, iOS 18+ · Spec: [PRD §7](../PRD.md#7-iphone-app-front-end), protocol [PRD §6](../PRD.md#6-wire-protocol-v1)
Other track: [desktop.md](desktop.md)

Legend: `[ ]` todo, `[x]` done. **Needs** = blocked by a task in the other track. **Done when** = acceptance check.

## Sync points with desktop

| # | What | Mobile needs it for | Desktop task |
|---|---|---|---|
| S1 | `docs/protocol.md` + `docs/protocol/vectors.json` frozen | M2.1 header and message codecs | D0.4 |
| S2 | Control server running without pairing (`airmicd --no-auth`) | M2.4 control client testing | D3.1 |
| S3 | Pairing + Bonjour advert | M3.1, M3.3 | D3.3, D3.4 |
| S4 | `transcript` messages | M6.1 | D6.4 |

Until S2 lands, test audio with the zero-code Linux receiver (D1.1) and the control client against `tools/mock_control.py` (M2.5).

---

## M0 · Setup
- [x] **M0.1** Create Xcode project `ios/AirMic` (SwiftUI app, iOS 18 min, Swift 6 language mode, bundle id `io.airmic.AirMic`)
  - `ios/AirMic.xcodeproj` uses folder-synced groups: new files under `ios/AirMic/` and `ios/AirMicTests/` need no project edits. Team `U9YF5FHVZY` (free Personal Team).
- [ ] **M0.2** Signing with the personal team; iPhone in Developer Mode; app runs on device
- [x] **M0.3** Info.plist: `NSMicrophoneUsageDescription`, `NSLocalNetworkUsageDescription`, `NSBonjourServices = _airmic._tcp`, `NSCameraUsageDescription`; `UIBackgroundModes = audio`
- [x] **M0.4** Folders `App/ Audio/ Network/ Session/ UI/` and a test target `AirMicTests`

Done when: blank app installs on the iPhone from Xcode.

## M1 · Audio path spike
- [x] **M1.1** `AudioCapture`: `AVAudioSession` `.playAndRecord` + `.voiceChat`, start/stop, permission request
- [x] **M1.2** `AVAudioEngine` input tap → `AVAudioConverter` to 48 kHz mono Int16
  - Changed: `AVAudioSinkNode` instead of an input tap (tap buffers are 100–400 ms). Realtime thread copies into `SampleQueue`; `FrameProcessor` thread converts. IO buffer 5 ms.
- [x] **M1.3** Frame accumulator: exact 10 ms frames (480 samples, 960 bytes)
- [x] **M1.4** Raw UDP sender (`NWConnection`) to a typed IP:port, **no header** (feeds the netcat receiver)
- [x] **M1.5** Debug screen: IP field, port field, Start/Stop, live RMS number
- [ ] **M1.6** Test: voice heard via `pw-record` and Claude Code `/voice` on Linux (needs D1.1)
- [ ] **M1.7** Test: screen locked for 1 h, audio keeps flowing
- [ ] **M1.8** Rough latency check (clap test with a recording) and note result in `docs/notes/m1.md`

Done when: you can talk into the phone with the screen locked and use it as the Linux mic.

## M2 · Protocol client
- [ ] **M2.1** `Protocol.swift`: 16 byte header encode/decode, control message `Codable` types (needs S1)
- [ ] **M2.2** Unit tests against `docs/protocol/vectors.json` (same vectors the Rust side uses)
- [ ] **M2.3** `Packetizer`: sequence, timestamp, session id, muted flag; `AudioSender` uses it
- [ ] **M2.4** `ControlClient`: TCP `NWConnection`, newline JSON framing, `hello` → `ready`, `ping/pong` keepalive (2 s, 6 s timeout), `bye` (needs S2 for real testing)
- [ ] **M2.5** `tools/mock_control.py`: tiny fake control server for testing before S2
- [ ] **M2.6** Connection state machine: idle → connecting → live → muted → reconnecting; exponential backoff, resume within 3 s after Wi-Fi blip
- [ ] **M2.7** Mute: send `mute {on}` and header-only packets at 10/s while muted
- [ ] **M2.8** Handle `stats` (latency for the status pill)
- [ ] **M2.9** Audio interruptions (call, Siri) and route changes (AirPods): pause, show reason, auto-resume

Done when: phone streams to `airmicd`, mute works, a Wi-Fi toggle reconnects by itself.

## M3 · Discovery and pairing
- [ ] **M3.1** `Discovery`: `NWBrowser` for `_airmic._tcp`, list with name and IP (needs S3)
- [ ] **M3.2** Local Network permission: first-run explainer, denied state with link to Settings
- [ ] **M3.3** Pairing: `pair_required` → 4 digit code entry → `paired {token}`; error and retry states (needs S3)
- [ ] **M3.4** Keychain store for tokens; paired computers list; auto-connect to last computer on launch
- [ ] **M3.5** QR scanner (host, port, id, code) with camera permission handling
- [ ] **M3.6** Manual IP entry sheet with validation

Done when: fresh install → pick computer → enter code → streaming, and next launch reconnects with no taps.

## M4 · UI from the mockups
- [ ] **M4.1** Design tokens: colors (light cream / dark), SF Pro type scale, spacing, radii, 56 px buttons, 280 px primary width
- [ ] **M4.2** Home · streaming: status pill, focus timer + goal bar, big mic with pulse rings, level bars, "End session", floating mute button
- [ ] **M4.3** Drive mic rings and level bars from real RMS (smoothed, 30 fps)
- [ ] **M4.4** Home · muted and Home · no computer states
- [ ] **M4.5** Mute button: haptic, animation, accessibility label/state
- [ ] **M4.6** Connect screen (nearby list, inline code, other ways, desktop app link)
- [ ] **M4.7** `SessionStore` + SwiftData model: start/end, goal, mutes, dropouts, words
- [ ] **M4.8** Summary screen with Swift Charts weekly bars
- [ ] **M4.9** Design and build Settings: paired computers, focus goal, audio mode (voice / raw), haptics, about
- [ ] **M4.10** Accessibility pass: Reduce Motion (no pulse), Dynamic Type, VoiceOver, contrast
- [ ] **M4.11** App icon and launch screen

Done when: every mockup screen exists in light and dark and works with real data.

## M6 · Transcript on phone
- [ ] **M6.1** Handle `transcript {text, final}`; word count into `SessionStore` (needs S4)
- [ ] **M6.2** "Last thing you said" on Summary from the last final transcript

## M7 · Hardening and release
- [ ] **M7.1** 2 h locked-screen soak test; battery use per hour noted (target < 10%)
- [ ] **M7.2** Run the end to end checklist (PRD §12) on the phone side
- [ ] **M7.3** Unit test coverage for Packetizer, codecs, state machine
- [ ] **M7.4** Screenshots and screen recording GIF for the README
- [ ] **M7.5** Sideload install guide in the README (free Apple ID, 7 day re-sign)

## Later (v2)
- [ ] **L.1** Live Activity on Lock Screen: timer and mute
- [ ] **L.2** App Intents: Control Center mute control, Action button shortcut
- [ ] **L.3** Push to talk (hold big mic) for dictation
- [ ] **L.4** Opus encoding (codec flag 1)
- [ ] **L.5** Audio encryption (ChaCha20-Poly1305, key from pairing)
- [ ] **L.6** TestFlight via the $99/yr developer program
