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

## Branches, stacks and conflicts
PRs are merged with "Rebase and merge", and several sessions or agents work on the desktop track at once. Both cause conflicts unless you follow these rules.
- Work in your own `git worktree`, never in another session's. Before you rebase or `checkout -B` a branch, run `git worktree list`. Never move a branch that another worktree has checked out.
- Branch off a fresh `origin/main` (`git fetch` first). Stack a PR on another open PR's branch only when it builds on that work, and pass `--base <that-branch>`.
- After any PR merges to `main`, rebase every open stack onto it. The merge rewrote the merged commits' ids, so lift each branch off its old parent: record each branch's sha first, then `git rebase --onto origin/main <old-parent-sha> <branch>`, then the next branch onto the one below it. Check `git log --oneline origin/main..<top>` shows one commit per PR. Push with `--force-with-lease=<branch>:<old-sha>`.
- Shared hotspots: the `mod` list and wiring in `desktop/crates/airmicd/src/main.rs`, `Cargo.toml`, `Cargo.lock`, and the task files. Keep `main.rs` changes to one `mod` line plus one wiring call. In a conflict, keep both sides' `mod` lines and task ticks. Never hand-merge `Cargo.lock`: take `main`'s copy and run `cargo build` to regenerate it.
- After resolving, run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on every rebased branch tip before pushing.

## Environment
- Mac (this machine): Xcode 27 at `/Applications/Xcode.app`, iOS 27 simulator runtime installed. `xcode-select` still points at Command Line Tools, so prefix Xcode commands with `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` (or the user runs `sudo xcode-select -s /Applications/Xcode.app` in a real terminal). Rust is not installed on the Mac.
- iPhone: free Apple ID signing (7 day expiry), Developer Mode on.
- Linux desk machine: Ubuntu, PipeWire, GNOME. PipeWire code can only be built and tested there.
- Locale: Australia (AEST).

## Conventions
- iOS: Swift 6, SwiftUI, iOS 18 minimum, no third-party packages in v1. Font is SF Pro (system); the user dislikes serif fonts.
- Desktop: Rust stable, tokio; audio output, IPC and service install each behind a trait (future macOS/Windows support, PRD §10).
- License MIT.
