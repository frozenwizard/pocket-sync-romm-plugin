# Running RomM Sync locally

Run your build in `demo_host`, in the terminal, or in Pocket Sync itself. Both run it on the test Pocket in `test-pocket/`.

You also need a RomM server, version 5.4 or newer, and a RomM user to approve the Pocket when it pairs.

Build first, from the repo's root:

```sh
cargo build -p plugin --release --target wasm32-wasip1
```

## The test Pocket

`test-pocket/` stands in for a Pocket's SD card:

- The plugin runs only on a folder with `Assets/`, `Cores/` and `Platforms/` in it. The `.gitkeep` files keep those folders in git, and Pocket Sync ignores them.
- Pocket Sync opens a folder as a Pocket only when it has `Analogue_Pocket.json`, `Assets/` and `Cores/`. It reads the firmware version from `Analogue_Pocket.json`: `byte` 37 is 0x25, firmware 2.5.

`test-pocket/.gitignore` keeps out of git what runs write there:

- `romm-sync/` holds the Pocket's ID (`device.json`) and the RomM address and sign-in (`server.json`). The sign-in is a secret.
- `pocket-sync.json` is Pocket Sync's settings for this Pocket.

To start again as a new Pocket, delete `test-pocket/romm-sync/`. The next run gives the Pocket a new ID, asks for the RomM address, and pairs.

## In demo_host

[pocket-plugin](https://github.com/openfpga-library/pocket-plugin)'s `demo_host` runs a plugin in the terminal. Clone it once next to this repo, then, from this repo:

```sh
mkdir -p target/run/host
cargo run --manifest-path ../pocket-plugin/Cargo.toml -p demo_host -- \
  --folder-plugin target/wasm32-wasip1/release \
  --pocket-path test-pocket \
  --host-folder target/run/host
```

- Up and Down highlight a button, and Enter sends it. Nothing is highlighted at first: pressing Enter then clears the question without sending an answer, and only Esc gets out.
- Esc sends `Kill` and quits.
- `demo_host` checks for answers once a second, so replies can take a moment.
- `target/run/host` stands in for the plugin's private `host/` folder. `cargo clean` deletes it.

## In Pocket Sync

Pocket Sync installs plugins only from a release in a public GitHub repo. It downloads the release's files without the GitHub token in its settings (v7.2.1), so a private repo's files fail to download. This repo has no release yet, so install pocket-plugin's example plugin and link your build in its place.

Once:

1. In Pocket Sync, choose `test-pocket/` as the Pocket.
2. Under **Plugins → Add**, add `https://github.com/openfpga-library/pocket-plugin`. Pocket Sync downloads the example plugin into its data folder.
3. Replace the example's files with links to your build. On macOS, from this repo:

   ```sh
   plugin_folder="$HOME/Library/Application Support/today.neil.pocket-sync/openfpga-library-pocket-plugin"
   ln -sf "$PWD/target/wasm32-wasip1/release/plugin.wasm" "$plugin_folder/plugin.wasm"
   ln -sf "$PWD/target/wasm32-wasip1/release/plugin.json" "$plugin_folder/plugin.json"
   ```

   On Linux, Pocket Sync's data folder is `~/.local/share/today.neil.pocket-sync/`. On Windows it's `%APPDATA%\today.neil.pocket-sync\`, and making links needs Developer Mode; without it, copy the two files there after each build.
4. Press **Refresh** on the Plugins page. The plugin shows as RomM Sync, with the example's GitHub link and version.

After each build, close the plugin's window and press play again. Pocket Sync reads `plugin.wasm` again on every run, so it doesn't need restarting.

- A blank plugin window means Pocket Sync couldn't load the plugin, most often because `target/wasm32-wasip1/release/plugin.wasm` is gone, after a `cargo clean` for one. Pocket Sync doesn't show the error, and its log doesn't record it. Build again.
- The plugin uses the example's `host/` folder, next to the links.
- When the example plugin publishes a new release, Pocket Sync writes it through the links, over your build. Run `cargo clean -p plugin --release --target wasm32-wasip1` and build again.
- **Plugins → Remove** deletes the example's folder and the links in it, not your build.

Once this repo has a release, add `https://github.com/frozenwizard/pocket-sync-romm-plugin` instead, and link your build into its folder, `frozenwizard-pocket-sync-romm-plugin/`.
