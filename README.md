# RomM Sync

A [Pocket Sync](https://github.com/neil-morrison44/pocket-sync) plugin that syncs games and saves between a [RomM](https://github.com/rommapp/romm) library and an Analogue Pocket. The design is in [docs/Architecture.md](docs/Architecture.md).

## Layout

| Path | What it is |
|---|---|
| `plugin/` | The plugin crate, built to `plugin.wasm`, and its `plugin.json` |
| `docs/` | The design (`Architecture.md`), decision records (`adr/`), research notes (`research/`), and how to run the plugin locally (`running-locally.md`) |
| `test-pocket/` | A folder that stands in for a Pocket's SD card when you run the plugin locally |

## Build, test and run

[CONTRIBUTING.md](CONTRIBUTING.md) covers building the plugin and the checks every change passes. [docs/running-locally.md](docs/running-locally.md) covers running it in `demo_host` or Pocket Sync.

## Release

Publish a normal GitHub release, not a draft or a pre-release. `.github/workflows/add_to_release.yml` builds the plugin and attaches `plugin.wasm` and `plugin.json`, which is what Pocket Sync installs. In Pocket Sync, add the plugin under **Plugins → Add** with this repo's URL.

The repo has to be public first. Pocket Sync downloads release files without its GitHub token, so it can't install from a private repo. Until the first release, [docs/running-locally.md](docs/running-locally.md#in-pocket-sync) shows how to run a local build in Pocket Sync.
