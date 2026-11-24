# AGENTS.md

RomM Sync is a Pocket Sync plugin: Rust built for `wasm32-wasip1` with Extism, syncing an Analogue Pocket's games and saves with a RomM server.

## Before changing code

- Read the sections of `docs/Architecture.md` the change touches. It's the spec: flows, card files, RomM calls, safety rules (§13) and milestones (§18).
- Read `CONTRIBUTING.md` for the code conventions. A change is done when every check in its [Build and check](CONTRIBUTING.md#build-and-check) section passes.
- Name things with the words in `GLOSSARY.md`, in code, docs and UI text alike.
- Before calling a RomM endpoint, read its entry in `docs/research/romm-api.md`: shapes, permissions and gotchas.

## Gotchas

- **Extism traps on network failure.** A request that gets no response (refused connection, unknown server, bad certificate, invalid URL, host not in `allowed_hosts`) ends the plugin's call, and the plugin can't catch it. Check an address with `romm::base_url` before requesting it, and log the server you're contacting first, so the log names it when a run ends this way.
- **Extism memory is freed by hand.** Extism's `Memory` isn't freed on drop: copy a response's body out and free it, as `Extism::get` in `lib.rs` does.
- **Every write survives being cut off.** Pocket Sync's Stop ends the plugin mid-call, and people pull the card out mid-write. Write card files through `Card`'s helpers, which write `<name>.part` and rename it into place.
- **`server.json` is edited by hand.** Read it as plain JSON and write back every field you don't know.

## Working with the maintainer

- Work one milestone step at a time, on a branch off `main` in the main checkout. The maintainer reviews and commits each step.
- When code has to differ from `docs/Architecture.md`, say so and propose the change to the spec. Edit the spec once the maintainer agrees, in the same branch as the code.
