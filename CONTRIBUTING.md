# Contributing to AirMic

Thanks for helping. Bug reports, fixes and small improvements are all welcome.

## Where things are

- `desktop/`: Rust workspace with the `airmicd` daemon, `airmic-proto` (wire protocol) and `airmic-send` (a test sender that plays the phone).
- `ios/`: the iPhone app (Swift 6, SwiftUI, no third-party packages).
- `docs/PRD.md` is the product plan. `docs/protocol.md` is the contract between phone and computer. A protocol change needs an update there and in `docs/protocol/vectors.json`.

## Build and test the daemon

You need Ubuntu (or another Linux) with PipeWire, Rust stable, and:

```sh
sudo apt install libpipewire-0.3-dev libclang-dev pkg-config
```

Then, from `desktop/`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same three commands. Run them before you open a pull request.

To try the daemon without the phone, run it on spare ports and leave your default mic alone. Write a scratch `config.toml`:

```toml
control_port = 47860
audio_port = 47861
set_default_source = false
```

Then run `cargo run -p airmicd -- --config <that file> --no-auth`, and in another terminal `cargo run -p airmic-send -- --port 47860 --seconds 10`. `airmic-send --help` lists the flags for loss, jitter and reordering.

If the AirMic service is installed, stop it first with `systemctl --user stop airmicd`. Both daemons would use the same IPC socket.

## Build the iPhone app

Open `ios/AirMic.xcodeproj` in Xcode and follow [docs/ios-install.md](docs/ios-install.md).

## Pull requests

- Keep each pull request small and about one thing.
- Branch off the latest `main`.
- Use a semantic prefix in commit messages: `feat:`, `fix:`, `docs:`, `ci:`, `refactor:`, `test:`.
- Pull requests are merged with **Rebase and merge**, so history stays linear. Each commit should build and pass the tests on its own.
- Say in the description what changed and why, and how you tested it. For audio changes, say whether you tested with a real iPhone or with `airmic-send`.

## Reporting bugs

Use the [bug report form](https://github.com/abhiejam/airmic/issues/new?template=bug_report.yml). The output of `airmicd status` and your Ubuntu, PipeWire and iOS versions make most bugs much faster to find.

## License

By contributing, you agree that your work is released under the [MIT License](LICENSE).
