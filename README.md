# RomM Sync

A [Pocket Sync](https://github.com/neil-morrison44/pocket-sync) plugin that syncs games and saves between a [RomM](https://github.com/rommapp/romm) library and an Analogue Pocket. The design is in [docs/Architecture.md](docs/Architecture.md).

## Layout

| Path | What it is |
|---|---|
| `plugin/` | The plugin crate, built to `plugin.wasm`, and its `plugin.json` |
| `test_host/` | A headless host that loads the plugin the way Pocket Sync does and answers its questions from a script |

## Build

```sh
cargo build -p plugin --release --target wasm32-wasip1
```

This writes `target/wasm32-wasip1/release/plugin.wasm`, and `plugin/build.rs` copies `plugin.json` beside it. `rust-toolchain.toml` installs the `wasm32-wasip1` target.

## Test

```sh
cargo test
```

Tests run on your computer's own target, not wasm. Plain `cargo` commands only build `plugin`, so the test host's Extism runtime isn't compiled unless you ask for it.

## Run in the test host

```sh
mkdir -p target/run/pocket target/run/host
cargo run -p test_host -- target/wasm32-wasip1/release target/run/pocket target/run/host
```

The three folders are the plugin, the Pocket's SD card and the plugin's `host/` folder. Anything after them answers the plugin's questions in turn, and `KILL` sends `Kill`.

## Release

Publish a normal GitHub release, not a draft or a pre-release. `.github/workflows/add_to_release.yml` builds the plugin and attaches `plugin.wasm` and `plugin.json`, which is what Pocket Sync installs. In Pocket Sync, add the plugin under **Plugins → Add** with this repo's URL.
