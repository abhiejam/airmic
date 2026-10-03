# AirMic

Use your iPhone as a wireless microphone for your computer.

The iPhone app streams its mic over local Wi-Fi to a small Linux daemon.
The daemon exposes a virtual input called "AirMic" that any app can use: Zoom, Discord, browsers, Claude Code `/voice`.
A desktop app shows status, pairing and a live transcript.

Status: early development. See [docs/PRD.md](docs/PRD.md) for the plan.

## Layout

| Path | What |
|---|---|
| `ios/` | iPhone app (Swift, SwiftUI) |
| `desktop/` | Rust workspace: `airmicd` daemon, `airmic-proto`, `airmic-send` test sender, Tauri app |
| `docs/protocol.md` | Wire protocol between phone and computer |
| `tools/` | Development scripts |

## Build (desktop)

```sh
cd desktop
cargo test --workspace
```

## License

MIT, see [LICENSE](LICENSE).
