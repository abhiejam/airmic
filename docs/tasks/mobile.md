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

## Status and handover (2026-10-03)

Read this before starting mobile work. Checked on 2026-10-03; re-check anything you rely on.

**Works end to end on a real iPhone 12 Pro Max with the real `airmicd` on `nuc`:** Bonjour discovery, pairing with the code from the daemon log, Keychain token reconnect, streaming into PipeWire, Claude Code `/voice` dictation, mute, Wi-Fi blip recovery. Against `tools/mock_control.py` also: auto-connect on launch and `--transcripts`. PC-side results are in [`docs/notes/m1.md`](../notes/m1.md).

**Open issues, in priority order**
1. **Session dropped after a daemon restart (M2.6).** Restart `airmicd` while the phone is live: the phone reconnects (`session … ready` in the daemon log), then the daemon logs `ended` about 2.5 s later with no error, so the phone closed the TCP connection itself. Any `ControlClient.finish` sends a FIN, so this can be a clean `close()` *or* a failure path. Start by capturing the phone's log: `devicectl device process launch --console --environment-variables '{"OS_ACTIVITY_DT_MODE":"YES"}' io.airmic.AirMic` and look for `Control closed: <reason>` (category `control`). Suspects: `ControlClient.handle(_:)` treats `.waiting` as fatal, and a Bonjour service endpoint can briefly go `.waiting` while the restarted daemon re-advertises; a second `ControlClient` from a stale `reconnectTask`; `StreamSession.connectionLost` running for a client that was already replaced.
2. **Audio underruns (desktop D2.6, maybe partly phone).** About 10 underruns per minute on the phone stream while jitter is only ~1.7 ms. Check the phone's send pacing: `FrameProcessor` emits frames as converted chunks arrive, so packets may leave in bursts (for example 2 to 3 at once every 20 to 30 ms with voice processing). Logging send times per sequence for 60 s would show it; pacing sends to a 10 ms clock could help.
3. **Not yet tested on the phone:** locked screen for 1 h (M1.7), Siri or a call and AirPods (M2.9), QR scan (M3.5), the Local Network explainer and denied state (M3.2), the latency figure in the pill (M2.8), a VoiceOver walk-through (M4.10).
4. **Open PR #36:** saves computer addresses without the `%en0` interface scope. Merge it first.

**Remaining tasks:** M1.7, M1.8, M2.8, M2.9, M3.2, M3.5, M4.10 (on-phone checks above) · M7.1 2 h soak and battery · M7.2 PRD §12 checklist · M7.4 README screenshots and GIF: finish `mobile/readme-screenshots-wip` (UI test `AirMicUITests`, scheme `AirMicScreenshots`, `tools/readme-screenshots.sh`; speech is opt-in with `--speak` because it plays aloud on the Mac; not run yet) · later L.1 to L.6.

**Building and testing (Mac)**
- Xcode commands need `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`.
- Unit tests: `xcodebuild -project ios/AirMic.xcodeproj -scheme AirMic -destination 'platform=iOS Simulator,name=iPhone 17' -parallel-testing-enabled NO test` (46 tests with PR #36). They read `docs/protocol/vectors.json` from the repo. A main-actor-isolated function called from a test traps the test host: mark helpers `nonisolated`.
- Phone: `xcodebuild -scheme AirMic -destination 'id=00008101-001A102A3EF2001E' -allowProvisioningUpdates build`, then `xcrun devicectl device install app --device 00008101-001A102A3EF2001E <DerivedData>/Build/Products/Debug-iphoneos/AirMic.app`. Free signing expires every 7 days. Installing restarts the app, so ask before installing while the user is testing.
- Fake computer: `python3 tools/mock_control.py [--pair 0427] [--transcripts] [--wav FILE] [--drop-after S] [--name NAME]`. It advertises over Bonjour and prints the QR link with `--pair`. `--wav` records the user's voice: delete those files after testing.
- Never play sound on the Mac (`say`, `afplay`) without asking first.

**Workflow:** see "Branches, stacks and conflicts" in `CLAUDE.md`: a `git worktree` per branch, branch off fresh `origin/main`, PRs merged with "Rebase and merge" (sync local `main` with `git pull --rebase` after a merge). Mobile commits end with the `Co-Authored-By` line.

---

## M0 · Setup
- [x] **M0.1** Create Xcode project `ios/AirMic` (SwiftUI app, iOS 18 min, Swift 6 language mode, bundle id `io.airmic.AirMic`)
  - `ios/AirMic.xcodeproj` uses folder-synced groups: new files under `ios/AirMic/` and `ios/AirMicTests/` need no project edits. Team `U9YF5FHVZY` (free Personal Team).
- [x] **M0.2** Signing with the personal team; iPhone in Developer Mode; app runs on device
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
  - 2026-10-03: verified on iPhone 12 Pro Max → Mac UDP receiver: ~100 packets/s of 960 bytes, speech peaks -25 to -11 dBFS, 45 s recording played back.
- [x] **M1.6** Test: voice heard via `pw-record` and Claude Code `/voice` on Linux (needs D1.1)
  - 2026-10-03: phone speech dictated through Claude Code `/voice` via the real `airmicd` on `nuc` (Ubuntu 24.04, PipeWire 1.0.5). Results in `docs/notes/m1.md`.
- [ ] **M1.7** Test: screen locked for 1 h, audio keeps flowing
- [ ] **M1.8** Rough latency check (clap test with a recording) and note result in `docs/notes/m1.md`

Done when: you can talk into the phone with the screen locked and use it as the Linux mic.

## M2 · Protocol client
- [x] **M2.1** `Protocol.swift`: 16 byte header encode/decode, control message `Codable` types (needs S1)
  - `Network/Protocol.swift`: `AudioHeader`, `ControlMessage`, `LineBuffer`, `Packetizer`.
- [x] **M2.2** Unit tests against `docs/protocol/vectors.json` (same vectors the Rust side uses)
  - `AirMicTests/ProtocolTests.swift` reads `docs/protocol/vectors.json` from the repo (no copy). All pass (30 tests total).
- [x] **M2.3** `Packetizer`: sequence, timestamp, session id, muted flag; `AudioSender` uses it
- [x] **M2.4** `ControlClient`: TCP `NWConnection`, newline JSON framing, `hello` → `ready`, `ping/pong` keepalive (2 s, 6 s timeout), `bye` (needs S2 for real testing)
  - 2026-10-03: hello → ready → audio → bye verified on iPhone against `tools/mock_control.py`.
- [x] **M2.5** `tools/mock_control.py`: tiny fake control server for testing before S2
  - Options: `--pair CODE`, `--wav FILE`, `--drop-after SECONDS`; advertises over Bonjour and prints the QR link.
- [x] **M2.6** Connection state machine: idle → connecting → live → muted → reconnecting; exponential backoff, resume within 3 s after Wi-Fi blip
  - Backoff 0.25/0.5/1/2 s, capped at 2 s. 2026-10-03: Wi-Fi off/on on the iPhone reconnected with the stored token 3 s after the mock timed out.
  - **Bug (open):** after the daemon restarts, the phone reconnects (`ready`) and then closes the session cleanly about 2.5 s later. See Status and handover, issue 1.
- [x] **M2.7** Mute: send `mute {on}` and header-only packets at 10/s while muted
  - Verified on the iPhone: `mute {on}` reaches the mock, before and after a reconnect.
- [ ] **M2.8** Handle `stats` (latency for the status pill)
  - Built: latency in the status pill. Phone test pending.
- [ ] **M2.9** Audio interruptions (call, Siri) and route changes (AirPods): pause, show reason, auto-resume
  - Built in `AudioCapture` (interruption, engine configuration change, media reset). Phone test pending.

Done when: phone streams to `airmicd`, mute works, a Wi-Fi toggle reconnects by itself.

## M3 · Discovery and pairing
- [x] **M3.1** `Discovery`: `NWBrowser` for `_airmic._tcp`, list with name and IP (needs S3)
  - 2026-10-03: the real daemon's mDNS advert shows under Nearby and pairs from there.
  - Built (`Discovery`, merges with known computers). The mock advertises via `dns-sd`/`avahi-publish`. Confirm on the iPhone.
- [ ] **M3.2** Local Network permission: first-run explainer, denied state with link to Settings
  - Built: explainer card before the first browse, denied state (`kDNSServiceErr_PolicyDenied`) with Open Settings.
- [x] **M3.3** Pairing: `pair_required` → 4 digit code entry → `paired {token}`; error and retry states (needs S3)
  - Inline code entry in the Connect card; `bad_code` keeps the connection, `pair_locked` ends it. Verified on the iPhone with `mock_control.py --pair 0427`.
- [x] **M3.4** Keychain store for tokens; paired computers list; auto-connect to last computer on launch
  - `PairingTokens` (Keychain, after first unlock), `knownComputers` list in Settings with swipe to forget, `autoConnect()` on launch. Token reconnect verified on the iPhone.
- [ ] **M3.5** QR scanner (host, port, id, code) with camera permission handling
  - Built: `QRScannerView` + `PairingLink`; `airmic://` URL scheme opens the app from the Camera. Scan test pending.
- [x] **M3.6** Manual IP entry sheet with validation
  - `ManualEntrySheet`: IPv4 or hostname, port, optional name.

Done when: fresh install → pick computer → enter code → streaming, and next launch reconnects with no taps.

## M4 · UI from the mockups
- [x] **M4.1** Design tokens: colors (light cream / dark), SF Pro type scale, spacing, radii, 56 px buttons, 280 px primary width
- [x] **M4.2** Home · streaming: status pill, focus timer + goal bar, big mic with pulse rings, level bars, "End session", floating mute button
- [x] **M4.3** Drive mic rings and level bars from real RMS (smoothed, 30 fps)
- [x] **M4.4** Home · muted and Home · no computer states
- [x] **M4.5** Mute button: haptic, animation, accessibility label/state
- [x] **M4.6** Connect screen (nearby list, inline code, other ways, desktop app link)
  - Nearby list with live discovery, inline code, QR and IP buttons.
  - Built; "Nearby" shows only the recent computer until Discovery (S3). QR button disabled until M3.5.
- [x] **M4.7** `SessionStore` + SwiftData model: start/end, goal, mutes, dropouts, words
  - `StreamSession` (live state) + SwiftData `FocusSession`. Words and last transcript stay nil until S4.
- [x] **M4.8** Summary screen with Swift Charts weekly bars
- [x] **M4.9** Design and build Settings: paired computers, focus goal, audio mode (voice / raw), haptics, about
  - Cards in the mockup style: computers with Paired / Connected and a Forget confirmation, goal stepper, Voice / Raw explained, haptics, About, Debug stream. Checked on the iPhone 2026-10-03.
- [ ] **M4.10** Accessibility pass: Reduce Motion (no pulse), Dynamic Type, VoiceOver, contrast
  - Built: Dynamic Type via `scaledFont` (capped at accessibility2, mic shrinks at accessibility sizes), VoiceOver headers, timer and chart values, card status, announcements, Voice Control names for mute, Increase Contrast variants of secondary text and lines. All theme pairs pass WCAG (lowest 3.97:1 for the status dot, 5.09:1 for text). Checked on the simulator at large and accessibility-XXL. VoiceOver walk-through on the phone pending.
- [x] **M4.11** App icon and launch screen
  - Rendered from `design/logo/airmic-app-icon.svg` with light, dark and tinted variants. Launch screen: Signal mark on cream / near-black (`LaunchBackground`, `LaunchMark`).

Done when: every mockup screen exists in light and dark and works with real data.

## M6 · Transcript on phone
- [x] **M6.1** Handle `transcript {text, final}`; word count into `SessionStore` (needs S4)
  - Final lines only; partials ignored. `Transcript.wordCount` counts runs with a letter or digit. Words stay nil (shown as —) until a transcript arrives. Tested with `mock_control.py --transcripts`; real test waits for D6.4.
- [x] **M6.2** "Last thing you said" on Summary from the last final transcript
  - From `StreamSession.lastTranscript`, saved on `FocusSession`.

## M7 · Hardening and release
- [ ] **M7.1** 2 h locked-screen soak test; battery use per hour noted (target < 10%)
- [ ] **M7.2** Run the end to end checklist (PRD §12) on the phone side
- [x] **M7.3** Unit test coverage for Packetizer, codecs, state machine
  - `ControlClientTests`: 12 tests against an in-process fake server (greeting, ready, pairing, stats, transcripts, ping/pong, own pings, peer timeout, bye, unknown type, bad JSON, refused, close). Timings are injectable. Codecs and packetizer were already covered: 44 tests in all.
- [ ] **M7.4** Screenshots and screen recording GIF for the README
  - Pipeline on branch `mobile/readme-screenshots-wip` (pushed, no PR, not run yet). See Status and handover.
- [x] **M7.5** Sideload install guide in the README (free Apple ID, 7 day re-sign)
  - `docs/ios-install.md`, linked from the README.

## Later (v2)
- [ ] **L.1** Live Activity on Lock Screen: timer and mute
- [ ] **L.2** App Intents: Control Center mute control, Action button shortcut
- [ ] **L.3** Push to talk (hold big mic) for dictation
- [ ] **L.4** Opus encoding (codec flag 1)
- [ ] **L.5** Audio encryption (ChaCha20-Poly1305, key from pairing)
- [ ] **L.6** TestFlight via the $99/yr developer program
