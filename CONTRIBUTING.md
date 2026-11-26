# Contributing to RomM Sync

Thanks for helping. RomM Sync is a [Pocket Sync](https://github.com/neil-morrison44/pocket-sync) plugin that keeps an Analogue Pocket's games and saves in step with a [RomM](https://github.com/rommapp/romm) library.

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Read [docs/Architecture.md](docs/Architecture.md). It says what the plugin does and how, and [§18](docs/Architecture.md#18-milestones) says which milestone is in progress.
- [GLOSSARY.md](GLOSSARY.md) has the words the project uses. Code, docs and the plugin's text all use them.
- For anything bigger than a small fix, open an issue first so we can agree on the approach. Once a change to how the plugin behaves is agreed there, update `docs/Architecture.md` in the same pull request as the code.

## Set up

- Install Rust with [rustup](https://rustup.rs). `rust-toolchain.toml` picks stable Rust and adds the `wasm32-wasip1` target.

## Build and check

Run these before opening a pull request. All of them must pass. CI (`.github/workflows/ci.yml`) runs the same checks on every pull request.

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo clippy --target wasm32-wasip1 -- -D warnings
cargo build -p plugin --release --target wasm32-wasip1
```

- If `cargo fmt --check` fails, `cargo fmt` fixes the formatting.
- Tests run on your computer's own target, not wasm.
- Clippy runs twice because the test build and the wasm build compile different code.
- The build writes `target/wasm32-wasip1/release/plugin.wasm`, and `plugin/build.rs` copies `plugin.json` beside it.

## Run the plugin

[docs/running-locally.md](docs/running-locally.md) shows how to run your build in `demo_host` or Pocket Sync, on the test Pocket in `test-pocket/`.

## Code conventions

- **Logic builds and tests natively.** `plugin/src/lib.rs` is the only module that talks to Extism. Everything the plugin uses from Pocket Sync (the log, HTTP, `open_url`, waiting) goes through the `PocketSync` trait in `pocket_sync.rs`, and tests use `FakePocketSync` in its place.
- **The plugin is a state machine.** Pocket Sync shows a question only when a call returns one, so `flow.rs` returns one question per call, and `Run` holds what the plugin remembers between calls. Each question has a name that comes back with its answer.
- **The plugin's text lives in locale files.** Every string the user sees is in `plugin/locales/en/index.json`, reached through a `Key` in the `keys!` table in `plugin/src/ui/strings.rs`. Placeholders look like `{host}` and are filled by `Strings::fill`. Pocket Sync sends back a button's label as the answer, so the buttons of one question need different labels; a test checks this.
- **Safety rules come first.** [Architecture §13](docs/Architecture.md#13-safety-rules) lists them: never touch a file the plugin didn't write, never leave half a file on the card, never lose a save.
- **Tests sit beside the code** in a `#[cfg(test)] mod tests`, with names that read as sentences, such as `a_typed_address_is_saved_and_connected_to`.
- **Comments say why.** Doc comments describe what an item is for, in plain sentences, using the glossary's words.

## Translations

English is the source: `plugin/locales/en/index.json`. To add a language:

1. Add `plugin/locales/<lang>/index.json` with the same keys. A key you leave out or leave empty shows in English.
2. Add the file to `LOCALES` in `plugin/src/ui/strings.rs`.

The layout matches Pocket Sync's, so its translation tools work here too. For now the plugin always shows English, until Pocket Sync tells plugins which language the user chose.

## Pull requests

- Branch off `main`, and keep each pull request to one change.
- Make small commits, each with its tests passing.
- In the description, say what changed, why, and how you tested it: tests, `demo_host`, Pocket Sync, or a real Pocket.

Report security problems privately, as [SECURITY.md](SECURITY.md) describes, not in an issue or pull request.

## License

RomM Sync is licensed under the [GNU Affero General Public License, version 3 only](LICENSE) (AGPL-3.0-only). By contributing, you agree that your contributions are licensed under it too. Parts that came from the pocket-plugin template stay under the MIT License; see [NOTICE](NOTICE).
