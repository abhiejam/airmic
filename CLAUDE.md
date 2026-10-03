# AirMic

iPhone as a wireless mic for a desktop computer. Native iPhone app streams mic audio over Wi-Fi to a Rust daemon that exposes a PipeWire virtual mic on Linux; a Tauri desktop app shows status, pairing and a transcript.

## Read first
- `docs/PRD.md`: product, architecture, wire protocol (§6), milestones. Source of truth.
- `docs/tasks/mobile.md`: iPhone track (`ios/`).
- `docs/tasks/desktop.md`: Linux track (`desktop/`).
- Approved UI mockups: https://claude.ai/artifact/SFuouTH2921fG9QzJidshH (read with the Artifact tool).
- Logo: the "Signal" mark (mic with two arcs) + wordmark "AirMic" in SF Pro semibold, board 1 of https://claude.ai/artifact/J6CQZq1NW4ka29idedE5Ga. Accent #5146E5.

## Two parallel tracks
The project is built by two Claude Code sessions at once, one per track.
- Work only on your track's tasks and directory: mobile → `ios/`, desktop → `desktop/`, `tools/spike-receiver.sh`, `packaging/`.
- Tick tasks (`[x]`) in your own task file as they are done; add short notes under a task if something changed.
- The tracks meet at sync points S1–S4 (listed at the top of each task file). The wire protocol is the contract: desktop owns `docs/protocol.md` and `docs/protocol/vectors.json`; mobile reads them and does not change them without agreement. Any protocol change goes into the PRD §6 and both task files.
- Commit small and often, only files of your own track plus your task file, so the two sessions don't conflict.

## Environment
- Mac (this machine): Xcode 27 at `/Applications/Xcode.app`, iOS 27 simulator runtime installed. `xcode-select` still points at Command Line Tools, so prefix Xcode commands with `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` (or the user runs `sudo xcode-select -s /Applications/Xcode.app` in a real terminal). Rust is not installed on the Mac.
- iPhone: free Apple ID signing (7 day expiry), Developer Mode on.
- Linux desk machine: Ubuntu, PipeWire, GNOME. PipeWire code can only be built and tested there.
- Locale: Australia (AEST).

## Conventions
- iOS: Swift 6, SwiftUI, iOS 18 minimum, no third-party packages in v1. Font is SF Pro (system); the user dislikes serif fonts.
- Desktop: Rust stable, tokio; audio output, IPC and service install each behind a trait (future macOS/Windows support, PRD §10).
- License MIT.
