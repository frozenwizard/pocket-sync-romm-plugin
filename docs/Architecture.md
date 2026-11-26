# Architecture: RomM Sync, a Pocket Sync plugin

**Status:** draft, revised 2026-11-18 after a design review, and on 2026-11-23 to keep the server address and sign-in on the card, in `romm-sync/server.json`. Terms follow [GLOSSARY.md](../GLOSSARY.md). Decisions with lasting trade-offs are in [docs/adr/](adr/). [Open questions](#19-open-questions) lists what is still undecided.

## 1. Purpose

A plugin for [Pocket Sync](https://github.com/neil-morrison44/pocket-sync) that keeps an Analogue Pocket in step with a self-hosted [RomM](https://github.com/rommapp/romm) library:

- **Games:** the games in the Pocket's RomM collection are copied onto it, and games taken out of the collection are removed.
- **Saves:** the Pocket's in-game saves go to RomM and come back from it, so a lost or replaced card loses nothing. When both copies changed since the last sync, you choose which one to keep.

No tool does this today. Pocket Sync manages cores, firmware and save backups, but games are copied by hand or from local folders. RomM's own clients (Grout, Argosy, RomM Desktop) run on devices with a network; the Pocket has none.

## 2. Scope

### First release (v1)

1. Sign in to RomM by pairing each Pocket as its own RomM device.
2. Mirror one regular RomM collection onto the Pocket: add, update, rename and remove games.
3. Sync the Pocket's in-game saves with RomM, both ways, in a slot of the Pocket's own, asking only when both copies changed.
4. Never lose data: no half-written games, no deleted files the plugin didn't write, and the copy not chosen in a save conflict is always kept.
5. Keep slow SD card work to a minimum: download, unpack and check on the computer's disk, then copy finished files to the card.
6. Every single-file cartridge system whose RomM-to-Pocket mapping is confirmed.

### Later milestones

- **Install on device** (M6): designed in [section 11](#11-install-on-device), built after the first release.
- **Shared saves:** carry a game on between the Pocket and Rommarch, RomP or RomM's web player, switched on one system at a time after testing on a real Pocket ([ADR 0001](adr/0001-pocket-saves-stay-separate.md), [10.7](#107-shared-saves-later)).
- **Restore deleted saves:** a Settings option that brings back RomM's copy of saves deleted on the Pocket, one button per game plus **All**.
- **BIOS files** from RomM's firmware.
- **Translations** into other languages (v1 is English only). Support for them is built in M0.
- **Save states** (the Pocket's Memories).
- **pupdate** as a second plugin host.

### Not planned

- Devices other than the Analogue Pocket. This plugin only runs in Pocket Sync.
- Browsing the library inside the plugin. Pocket Sync gives plugins a log and buttons, nothing more, so games are chosen in RomM.
- Syncing in the background or when the card is connected. Pocket Sync runs plugins only when the user starts them.
- Arcade, Neo Geo and CD-based systems (large files, conversions, per-core layouts).
- Smart and virtual collections as the Pocket collection.

## 3. Context

### 3.1 The Pocket's SD card

The Pocket and its SD card are treated as one thing: the Pocket's identity is a file on the card, and whichever computer the card is plugged into syncs the same Pocket.

The parts of the card this plugin reads or writes:

| Path | What it holds | Plugin use |
|---|---|---|
| `Cores/<author>.<core>/core.json` | The core's metadata, including `metadata.platform_ids` | Which systems this Pocket can play |
| `Cores/<author>.<core>/data.json` | The core's data slots, including the file extensions it accepts | Which file to copy, and with what extension |
| `Assets/<platform>/common/` | Games and BIOS files shared by every core of that system | Where games go |
| `Saves/<platform>/common/<rom stem>.sav` | In-game saves, named after the game file, shared by every core of that system | Save sync |
| `Saves/<platform>/common/<rom stem>.rtc` | A separate clock file some cores write (mincer_ray.GBA) | Not touched in v1 |
| `Memories/` | Save states | Not touched |
| `pocket_sync.json` | Pocket Sync's own settings | Not touched |
| `romm-sync/` | This plugin's files on the card | See [7.1](#71-on-the-card) |

Things to remember about the card:

- It's FAT32 or exFAT. File times are local time with coarse precision, so the plugin decides by hashes and sizes, never by file times.
- The Pocket is case-sensitive about file extensions: `.GBA` and `.gba` differ.
- A core writes its save when it quits, or when the Pocket sleeps or powers off. The file's size is set by the core, not by the file it loaded.
- Writing to it is slow, and people pull it out mid-write.

### 3.2 The plugin host: Pocket Sync

Pocket Sync has run plugins since v6.0.0 (June 2026); v6.1.1 fixed running them in the signed macOS app. The interface is defined in [openfpga-library/pocket-plugin](https://github.com/openfpga-library/pocket-plugin) and the host is `src-tauri/src/commands/plugins.rs` in Pocket Sync. pupdate is meant to run the same plugins (untested here).

A plugin is Rust compiled to WebAssembly (`wasm32-wasip1`) with [Extism](https://extism.org) (`extism-pdk`). What the host gives it, and what it doesn't:

| Area | What the plugin gets | Effect on this design |
|---|---|---|
| Entry points | `start()` and `handle_response(HostMessage)`, each returning a `PluginMessage` | The plugin is a state machine: every question ends a call ([section 12](#12-control-flow)) |
| Instance | One Extism plugin instance per run, used for `start` and every `handle_response`, dropped on `Exit`, an error or Stop | A run's state lives in memory: Extism resets an instance only after a call to `_start`, which the plugin doesn't export |
| User interface | `PluginMessage::Choice { name, query, choices }` (one button per choice), `Text { name, query }`, `Exit`; `print_msg` writes to a log | Menus, confirmations and conflict questions are buttons; progress is log lines; no game lists or box art |
| Answers | `HostMessage::Answer { name, value }` or `Kill` | `Kill` only arrives while the plugin waits for an answer |
| Stopping | Pocket Sync's Stop cancels the plugin at once, even mid-call (Extism cancel handle) | Every write must survive being cut off ([section 13](#13-safety-rules)) |
| Files | Two folders, through WASI: `pocket/` is the card root, `host/` is a private folder on the computer | Card data, including the server address and sign-in, in `pocket/romm-sync/`; temp files and backups in `host/` |
| `host/` | `<Pocket Sync app data>/<owner>-<repo>/host`: created at install, kept across plugin updates, deleted at uninstall, never cleaned otherwise | The plugin cleans its own temp files. On Windows it's in the roaming profile, so no big permanent cache. |
| Network | HTTP through Extism, only to hosts in `plugin.json`'s `allowed_hosts` (patterns matched against the host name, port excluded) | `"allowed_hosts": ["*"]`, since every user's RomM is at a different address |
| Network failures | A request that gets no response (refused connection, unknown server, bad certificate, invalid URL, or a host not in `allowed_hosts`) ends the plugin's call: Extism returns the error from its host function, which traps, so the plugin can't catch it. No timeout is set, so a server that accepts the connection but never answers holds the call until Stop | The address must start with `http://` or `https://`; the plugin can't try one and fall back to the other. "Connecting to …" is logged before the first request, so the log names the server when a run ends this way. The first screen offers Options before connecting, so a bad address can be changed on the next run |
| Response size | 50 MB per HTTP response (Extism's default; Pocket Sync doesn't change it) | Large files are downloaded in pieces with `Range` |
| Other | `open_url` opens the browser, and ends the plugin's call when the browser can't be opened; config key `locale` gives the system's language, even when Pocket Sync's "Always use English (US)" is ticked (`demo_host` doesn't set it); one thread; `std::thread::sleep` works; no time or memory limits set | Pairing logs RomM's approval link and code before opening the page, then polls; no WebSockets; no free-space query |
| Install and update | The user adds the GitHub repo link; Pocket Sync fetches the latest release's `plugin.wasm` and `plugin.json` | Releases are the distribution channel ([section 17](#17-build-and-release)) |

### 3.3 RomM

The plugin needs **RomM 5.4.0 or newer** (5.4.0 added device-aware save records and Install on device). [Section 8](#8-romm-api-used) lists every call.

What RomM already provides, so the plugin doesn't have to:

- **Devices and pairing:** a device approves itself through RomM's web page and gets a token bound to it. Pairing again with the same identifier and user gives back the same device, while it exists. RomM has no server ID; a device ID that changes when the Pocket pairs again means a different server, or a device deleted in RomM.
- **Collections:** the game list can be filtered by a collection and can include each game's files.
- **File details:** each file has its name, size, CRC, MD5 and SHA-1, its category, and for zips the name, size and hashes of every file inside (`archive_members`).
- **Save sync:** slots, content hashes, a record of which save version each device last synced, a `409` when an upload would go over a version the device hasn't seen, and old versions kept per slot (up to 50 by default).
- **Install on device:** the web page queues a game for a device; the device claims it and reports the outcome.

## 4. System overview

```
 RomM web page / Argosy / RomP / Rommarch
   │  add games to the Pocket collection (later: "Install on device → Analogue Pocket")
   ▼
 ┌──────────────────────┐        HTTPS/HTTP (Extism, allowed_hosts "*")
 │      RomM server     │◄──────────────────────────────────────┐
 │  collections, games, │                                       │
 │  saves, installs     │                                       │
 └──────────────────────┘                                       │
                                     ┌──────────────────────────┴───────────┐
                                     │ Pocket Sync (on the computer)        │
                                     │  ┌────────────────────────────────┐  │
                                     │  │ RomM Sync (plugin.wasm)        │  │
                                     │  │  prompts ⇄ Pocket Sync window  │  │
                                     │  └───────┬───────────────┬────────┘  │
                                     │          │ WASI          │ WASI      │
                                     │     host/ (computer)   pocket/ (card)│
                                     └──────────┼───────────────┼───────────┘
                                                ▼               ▼
                                   temp downloads,            Assets/, Saves/,
                                   save backups               romm-sync/ (ID, server, manifest)
```

The card is the whole record: any computer that syncs the Pocket sees the same Pocket ID, server address, sign-in, RomM device ID, collection and manifest, so a second computer needs no setup. The costs:
- A computer that reaches the server at a different address (a LAN address at home, a VPN address elsewhere) has to change it, and the change applies to every computer.
- A lost card carries a working sign-in until it's revoked in RomM ([section 15](#15-security-and-privacy)).

## 5. User flows

### 5.1 First run with a Pocket

1. Pocket Sync is connected to the card; the user starts the plugin from the Plugins page.
2. **Check the card:** it must look like a Pocket card (`Cores/`, `Assets/`, `Platforms/`), or the run ends before anything is written.
3. **Pocket ID:** if `romm-sync/device.json` is missing, create it with a new random ID. If it's there but can't be read, the run ends and the file is left alone: a new ID would make RomM see a different Pocket.
4. **Server:** if the card has no `romm-sync/server.json`, ask for the address (`Text`) and save it there. It must start with `http://` or `https://` and name a server, with an optional port and path; the plugin checks this before saving, with the URL parser Extism uses, because Extism ends the call on an address it can't parse ([3.2](#32-the-plugin-host-pocket-sync), network failures). An answer that isn't an address, an empty one included, offers **Try again** and **Cancel**: on first setup Cancel ends the run, and when changing the address it goes back to Options. Then check `GET /api/heartbeat` answers and is 5.4 or newer.
5. **Pair:** start pairing with the Pocket ID and the name "Analogue Pocket", log the approval link and code, open RomM's approval page (`open_url`), and poll every 2 seconds until it's approved, denied or expired ([8.1](#81-pairing)). The approver can rename the device on that page. Save the token as `api_key` and RomM's device ID as `romm_device_id` in `server.json`. Check the granted permissions against the ones requested and say which features are off if any were left out. A denial, an expired code or a failed request offers Connect again.
6. **Check the server:** if `server.json` already had a RomM device ID and it differs from the one pairing returned, the address now points at a different RomM server ([13](#13-safety-rules), rule 8).
7. **Choose the Pocket collection:** offer **Use "Analogue Pocket"** (created if it doesn't exist) or the user's other regular collections as buttons. Smart and virtual collections are never listed. Save the choice as `collection` in `server.json`.
8. Offer **Sync now**.

Once `server.json` exists, a run starts with **Connect**, **Options** and **Exit**. Connect runs the heartbeat check (step 4), checks the saved sign-in by reading the Pocket's device (`GET /api/devices/{romm_device_id}`), pairs if there's no sign-in or RomM rejects it (steps 5 and 6), asks for a collection if there's none (step 7), and shows the menu. Options is described in [5.5](#55-options-and-settings).

The same Pocket on a second computer needs no setup: the card carries the address, sign-in and collection. If that computer reaches the server at a different address, the user changes it in Options, which changes it for every computer.

### 5.2 Sync now

1. **Preflight:** check the card looks like a Pocket card (`Cores/`, `Assets/`, `Platforms/`); load the card's files, including the address and sign-in in `server.json`; check the server; delete leftover `.part` files in the plugin's folders; empty `host/tmp/`.
2. **(M6) Claim install requests** and add the playable games to the Pocket collection ([section 11](#11-install-on-device)).
3. **Plan games:** fetch the Pocket collection's games with their files and compare them with the manifest, the card and the installed cores. This gives add, update, rename, adopt, remove, re-add and skip lists ([9.3](#93-planning)).
4. **Saves:** sync the saves of games in the manifest, including games about to be removed. Ask about each save conflict ([5.4](#54-save-conflicts)). A game to be removed whose conflict is skipped stays on the card for now.
5. **Confirm:** "Add 12 games (180 MB), update 1, remove 3. **Sync / Show details / Cancel**". **Show details** writes the lists to the log and asks again. Taking over existing files and renames are listed too.
6. **Transfer:** removals first (frees space), then renames, updates and additions, staged in `host/tmp/` ([9.4](#94-transfer-pipeline)).
7. **Saves for new games:** run the save decisions ([10.3](#103-decisions)) for games added in this run, asking about any conflict as in step 4. A game re-added because its file went missing keeps its save record and was handled in step 4.
8. **(M6) Report install requests:** `done`, `already_installed` or `failed` with a reason.
9. **Finish:** write the manifest, set `last_synced_at` in `device.json`, empty `host/tmp/`, log a summary, and return to the menu.

### 5.3 Removing games

Taking a game out of the Pocket collection removes it on the next sync, after the confirmation in step 5. Its save is synced one last time in step 4, then left on the card and ignored. If the game is added back later, that save is treated as never synced.

### 5.4 Save conflicts

A save conflict is asked only when both the Pocket's copy and RomM's copy changed since the last sync. One question per game, with the facts in the question:

```
Metroid Fusion (USA)
Pocket:  changed since the last sync (32 KB)
RomM:    changed since the last sync, 2026-11-17 21:40
[Keep Pocket's] [Use RomM's] [Skip for now]
[Keep Pocket's for all the rest] [Use RomM's for all the rest]
```

Whichever copy isn't chosen is kept ([10.5](#105-conflicts-and-keeping-the-other-copy)). **Skip for now** leaves both untouched and asks again next sync. A deletion on either side, or RomM going back to an older version, counts as a change and is described in the question.

### 5.5 Options and Settings

**Options**, on the first screen, works before connecting, so a run that ended on a bad address can still reach it:
- **Change server address:** asks for the new address, saves it in `server.json` (keeping the rest of the file, the sign-in included) and connects. A server that moved accepts the sign-in, so there's no pairing again. A different server rejects it, so Connect pairs, and the wrong-server check catches the change ([13](#13-safety-rules), rule 8). Editing `server.json` by hand works too; the plugin reads it again each time it's needed.
- **Unpair:** asks first, then checks the server (heartbeat), deletes the Pocket's device in RomM, and deletes `server.json`, so the next step asks for an address and pairs. From M2 the question also says how many games it removes ([13](#13-safety-rules), rule 5), and Unpair removes the games RomM Sync put on the Pocket and deletes `manifest.json`, whose device ID would otherwise trip rule 8 after pairing again. The Pocket keeps `device.json` (its ID) and its saves.
  - When RomM doesn't delete the device (it doesn't accept the sign-in, pairing didn't grant `devices.write`, or the device isn't the sign-in's), the plugin says why and how to clean up by hand (in RomM, and by deleting `server.json`), changes nothing on the Pocket, and goes back to Options.
  - When RomM deleted the device but `server.json` can't be deleted, the run stops and says to delete it by hand.
  - A Pocket with no address or no sign-in isn't paired, so there's nothing to unpair.
- **Pocket name** (later): the name RomM shows for this Pocket's device, "Analogue Pocket" until it's changed here or on RomM's approval page.

The menu's **Settings**, once connected, offers:
- **Change collection:** pick another regular collection. The next sync plans the difference, with the usual confirmation before removals.
- **Unpair:** the same as in Options.

## 6. Components

The plugin is one Rust crate with modules split along these lines. Logic that doesn't touch Extism (planning, manifest, mapping, save decisions) builds and tests on the normal host target too.

| Module | Responsibility |
|---|---|
| `lib` (entry) | `start` and `handle_response`; holds the run's state; turns errors into log lines and a menu; picks the language, fixed at `en-US` until Pocket Sync passes the one the user chose |
| `flow` | The state machine: which phase runs next and which question to ask ([section 12](#12-control-flow)) |
| `ui` | Building `Choice` and `Text` messages, naming questions, logging; every string looked up in the bundled `locales/<lang>/index.json` files (Pocket Sync's layout), each falling back from `de-AT` to `de` to English. Pocket Sync sends back the button's text as the answer, so answers are matched to the choice they stand for, never to English text |
| `pocket_sync` | What the plugin uses of Pocket Sync (the log, HTTP, `open_url`, waiting) behind a trait, so tests use a fake in place of Extism |
| `romm::client` | HTTP over Extism: base URL, bearer token, JSON, error mapping (401, 403, 404, 409, 429), multipart bodies, `Range` downloads |
| `romm::pairing` | Device pairing's calls: start it, and poll it once for the token and device ID; the permissions asked for. `flow` opens the approval page, waits between polls and writes the log, since that's text the user sees |
| `romm::api` | Typed calls: heartbeat, devices, collections, games, files, saves, installs |
| `card` | Checking the card, reading `core.json` and `data.json`, card paths, the plugin's card files (`device.json`, `server.json`) |
| `platforms` | RomM system ↔ Pocket platform ID mapping, checked against installed cores |
| `manifest` | Loading, saving (safely, with `.bak`) and querying the record of what the plugin put on the card |
| `plan` | Comparing the collection, manifest, card and cores into add / update / rename / adopt / remove / re-add / skip |
| `transfer` | Staging downloads in `host/tmp/`, unzipping, verifying hashes, copying to the card, `.part` then rename |
| `saves` | Decisions, upload, download, conflicts, backups |
| `installs` | (M6) Claiming, adding to the collection, reporting outcomes |
| `store` | The plugin's files in `host/`: backups, temp |

## 7. Data and storage

### 7.1 On the card

Everything lives in one folder at the card root, `romm-sync/`. The only secret is the sign-in in `server.json` ([section 15](#15-security-and-privacy)).

Every file has a `schema` number, so a later plugin can tell an older file. `device.json` and `server.json` are written to `<name>.part` and renamed into place, so a card pulled out mid-write never holds half a file under the real name. The manifest has its own procedure, below.

**`romm-sync/device.json`**: the Pocket's identity, written once, and when it last finished a sync.

```json
{
  "schema": 1,
  "pocket_id": "6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71",
  "last_synced_at": "2026-11-18T09:00:00Z"
}
```

- `pocket_id` is a random UUID. It's sent as RomM's `client_device_identifier`, so pairing this Pocket again with the same server and user gets the same RomM device, while it exists. Its first 8 hex digits name the Pocket slot ([10.1](#101-where-saves-live)). It must parse as a UUID, so a card edited by hand can't put a path into the file names it's used for.
- `last_synced_at` is `null` until the first sync finishes.
- Only `pocket_id` is required. Fields from a newer plugin are kept when this one writes the file.
- A `device.json` that can't be read, or doesn't match its format, is left alone and the run stops, because a new ID would make RomM see a different Pocket. The user restores it, or deletes it to sync as a new Pocket.

**`romm-sync/server.json`**: the RomM server this Pocket syncs with, its sign-in, and what it syncs.

```json
{
  "schema": 1,
  "host": "https://romm.example.com",
  "api_key": "rmm_…",
  "romm_device_id": "c1a2…",
  "collection": { "id": 12, "name": "Analogue Pocket" }
}
```

- `host` is the server's address, starting with `http://` or `https://`, possibly with a sub-path (`https://example.com/romm`). It's the only field required, so the file can be written by hand with just `{"host": "…"}`; the plugin fills in the rest.
- `api_key` is the client token from pairing. `romm_device_id` is RomM's ID for this Pocket's device, returned by pairing. It says which RomM server the manifest's IDs belong to: pairing with a different server gives a different device ID ([13](#13-safety-rules), rule 8).
- Connect checks the sign-in each time. When either field is missing, `api_key` holds a space or a line break (which can't be sent), or RomM rejects it (revoked, expired, or from another server), the Pocket pairs again.
- The permissions granted and the token's expiry aren't stored. RomM narrows a token's permissions to whatever its user's role allows at the time, so a saved list could be wrong, and Connect's check catches an expired token.
- `collection` is the Pocket collection.
- Unpair deletes the whole file ([5.5](#55-options-and-settings)); `device.json` stays.
- Changing the address (in Options, or by hand) keeps every other field, including fields the plugin doesn't know. The plugin reads the file again each time it needs it, so edits made while it waits are used.
- A `server.json` with no address in `host` (missing, empty or not text) asks for the address, and entering one keeps the file's other fields. One that can't be read stops the run, wherever it's read, and is left alone, since it may hold the sign-in.

**`romm-sync/manifest.json`**: every file the plugin put on the card, and the last-synced state of each save.

```json
{
  "schema": 1,
  "revision": 42,
  "romm_device_id": "c1a2…",
  "games": {
    "1234": {
      "name": "Metroid Fusion (USA)",
      "platform": "gba",
      "files": [
        {
          "path": "Assets/gba/common/Metroid Fusion (USA).gba",
          "size": 8388608,
          "md5": "…",
          "romm_file_id": 5678,
          "romm_file_name": "Metroid Fusion (USA).zip",
          "romm_file_md5": "…",
          "source": "zip-member"
        }
      ],
      "save": {
        "path": "Saves/gba/common/Metroid Fusion (USA).sav",
        "base_md5": "…",
        "romm_save_id": 991,
        "romm_updated_at": "2026-11-17T21:40:00Z",
        "romm_content_hash": "…"
      },
      "added_at": "2026-11-18T09:05:00Z"
    }
  }
}
```

- Keyed by RomM game ID; every lookup is by ID, never by name. `name` and `platform` are kept for logs, renames and recovery.
- `md5` is the file as written to the card; `romm_file_md5` is RomM's file, which differs when the game was unzipped. `romm_file_name` spots a rename in RomM.
- `save` is missing until the game has a synced save. If the save file is deleted on the Pocket, the record stays, so the plugin knows it was deleted rather than never made ([10.3](#103-decisions)). When the game is removed, the whole entry goes, save record included.
- `revision` goes up on every write, so a computer can tell whether a cached copy is current.
- Written to `manifest.json.tmp`, then the old file is renamed to `manifest.json.bak` and the new one renamed into place. On load, a missing or broken `manifest.json` falls back to `.bak`.
- Written every few games, right after every save upload, download or deletion, and at the end of a run. Writing after each save change means a cut-off run never mistakes the plugin's own upload for a change in RomM.

### 7.2 On the computer (`host/`)

Nothing in `host/` is needed to sync. It holds working files and backups only.

| Path | Holds | Notes |
|---|---|---|
| `host/tmp/` | Downloads in progress, unzipped files, staged saves | Emptied at the start and end of every run |
| `host/backups/saves/<pocket_id>/<platform>/<game>/<timestamp>.sav` | Every save the plugin replaced or lost in a conflict on the card | Newest 10 per game kept |
| `host/log/` | A copy of the last few runs' logs (optional) | For bug reports; no tokens in logs |

### 7.3 Rules

- The card holds everything any computer needs to carry on syncing, the address and the sign-in included.
- The sign-in is on the card, so a lost card carries a working sign-in until it's revoked in RomM ([section 15](#15-security-and-privacy)).
- The plugin only ever replaces, renames or deletes managed files: card files in the manifest that still match its recorded size and hash.

## 8. RomM API used

Permissions requested when pairing, covering every planned milestone so nobody has to pair again: `platforms.read`, `roms.read`, `collections.read`, `collections.write`, `devices.read`, `devices.write`, `assets.read`, `assets.write`, `firmware.read`.

| Purpose | Call | Permission |
|---|---|---|
| Server version and install setting | `GET /api/heartbeat` (`SYSTEM.VERSION`, `DEVICE_INSTALL.ENABLED`, `DEVICE_INSTALL.EXCLUDED_PLATFORM_SLUGS`) | none |
| Start pairing | `POST /api/auth/device/init` `{client_device_identifier, name, client, platform, client_version, requested_scopes}` → `device_code`, `user_code`, `verification_path`, `verification_path_complete`, `expires_in`, `interval` | none (rate-limited) |
| Finish pairing | `POST /api/auth/device/token` `{device_code}` → `access_token`, `device_id`, `scopes`, `expires_at` | none |
| Check the sign-in on Connect | `GET /api/devices/{device_id}` | `devices.read`; a `403` still means signed in |
| Unpair | `DELETE /api/devices/{device_id}` (only the device's own token); RomM deletes the device's tokens with it | `devices.write` |
| Systems | `GET /api/platforms` | `platforms.read` |
| List collections | `GET /api/collections` (regular only are offered) | `collections.read` |
| One collection (quick change check) | `GET /api/collections/{id}` → `rom_ids`, `updated_at` | `collections.read` |
| Create the Pocket collection | `POST /api/collections` | `collections.write` |
| Games in the collection | `GET /api/roms?collection_id=…&with_files=true&limit=…&offset=…`, optionally `&updated_after=…` | `roms.read` |
| One game | `GET /api/roms/{id}` | `roms.read` |
| Download a game file | `GET /api/roms/{id}/content/{file_name}?file_ids={file_id}` with `Range: bytes=…` | `roms.read` |
| List a game's saves | `GET /api/saves?rom_id=…&slot=<Pocket slot>&device_id=…` | `assets.read`, `devices.read` |
| Upload a save | `POST /api/saves?rom_id=…&slot=<Pocket slot>&emulator=analogue-pocket&device_id=…&overwrite=…&content_hash=…`, multipart field `saveFile` | `assets.write`, `devices.write` |
| Download a save | `GET /api/saves/{id}/content?device_id=…` (also records the device as synced) | `assets.read`, `devices.read` |
| Confirm the Pocket holds a save | `POST /api/saves/{id}/downloaded` `{device_id, content_hash}` | `devices.write` |
| (M6) Accept installs | `PUT /api/devices/{device_id}` `{"capabilities": {"remote_install": true}}` | `devices.write` |
| (M6) Add claimed games to the collection | `POST /api/collections/{id}/roms` `{rom_ids}` | `collections.write` |
| (M6) Claim install requests | `POST /api/devices/{device_id}/installs/claim` (only the device's own token) | `devices.write`, `roms.read` |
| (M6) Report an install request | `PUT /api/devices/{device_id}/installs/{request_id}` `{status, reason}` (only the device's own token) | `devices.write` |
| (Later) BIOS files | `GET /api/firmware?platform_id=…`, `GET /api/firmware/{id}/content/{file_name}` | `firmware.read` |

### 8.1 Pairing

- Init takes `client` `romm-sync`, `platform` `analogue-pocket` (the `emulator` label of the Pocket's saves) and the plugin's version as `client_version`. It returns `expires_in` 600 seconds and `interval` 5 seconds.
- The approval link is `verification_path_complete` joined onto the server address, sub-path included.
- The plugin polls every 2 seconds (`POLL_INTERVAL` in `plugin/src/romm/pairing.rs`), faster than `interval`. While waiting, RomM answers `slow_down` to a poll inside 5 seconds of the last, which the plugin treats as still waiting; the first poll after approval gets the token all the same. A `429` adds 5 seconds to the wait.
- The approver chooses which of the requested permissions to grant; a RomM user with the Viewer role can grant only the `.read` ones.
- Polling `/api/auth/device/token` answers:
  - **400 `authorization_pending`** while waiting;
  - **400 `slow_down`** when polled faster than the interval;
  - **400 `access_denied`** for 60 seconds after the user denies, then `expired_token`;
  - **400 `expired_token`** when the code has expired or is unknown;
  - **429** past 60 polls a minute from one address;
  - **200** with the token on approval. The token is handed out once; a later poll gets `expired_token`.
- Pairing again with the same Pocket ID and user returns the same device, renamed to whatever the approver chose, with a new token. Earlier tokens stay valid until revoked in RomM.
- RomM picks a device's ID itself. Deleting a device in RomM deletes its tokens too, so later requests get `401`; pairing again then makes a new device with a new ID.

### 8.2 Save rules that matter

- RomM matches saves by game, user and slot. The `emulator` label only sets the storage path and the "saved by" text.
- An upload with `device_id` and a slot, without `overwrite`, gets **409** when the slot's newest save isn't the version this device last synced.
- `overwrite=true` skips that check and RomM's same-content check.
- Old versions are pruned per user, game and slot: up to 50 by default (`MAX_SAVES_PER_SLOT`), or 10 when the upload asks for `autocleanup`. The plugin doesn't ask for `autocleanup`.

## 9. Game sync

### 9.1 Systems

A mapping table (data in the plugin, with tests) turns RomM's system into the Pocket's platform ID. At run time, only platform IDs that an installed core lists in `core.json` count. Anything else is skipped with a reason. Every single-file cartridge system joins v1 once its mapping is confirmed.

| RomM system | Pocket platform | Status |
|---|---|---|
| `gb` | `gb` | v1 |
| `gbc` | `gbc` | v1 |
| `gba` | `gba` | v1 |
| `nes`, `famicom` | `nes` | v1 |
| `snes`, `sfam` | `snes` | v1 |
| `genesis` | `genesis` | v1 |
| `sms` | `sms` | v1 |
| `gamegear` | `gg` | v1 |
| TurboGrafx-16 / PC Engine | `pce` | v1; RomM's system name to confirm |
| Neo Geo Pocket (Color), WonderSwan (Color), Lynx, Atari 2600/7800, ColecoVision, Intellivision and similar | to confirm | v1 as each mapping is confirmed |
| Arcade, Neo Geo, CD systems | per core | Not planned |

### 9.2 Choosing what to copy

For each game:

1. Take its files in the `game` category (or uncategorised), ignoring manuals and other documents.
2. Find the extensions the core accepts in the core's `data.json`.
3. If a file has an accepted extension, copy it as it is.
4. If it's a zip and the core doesn't take zips, look in `archive_members` for a file with an accepted extension. The plugin then knows the name, size and hashes it will unpack before downloading.
5. Use RomM's file name, with the extension's capitalisation changed to the core's (`.GBA` → `.gba`).
6. Two games that would land on the same path: keep the first and skip the second with a reason.
7. Nothing suitable: skip with a reason ("No .gba file in this game's zip").

### 9.3 Planning

Inputs: the Pocket collection's games (with files), the manifest, the card, the installed cores, and the save conflicts left unanswered in this run.

| Case | Result |
|---|---|
| In the collection, not in the manifest, nothing at the target path | **Add** |
| In the collection, not in the manifest, an identical file (size and MD5 match RomM's) at the target path | **Adopt**: record it as managed without copying |
| In the collection, not in the manifest, a different file at the target path | **Skip**, with a reason ("A different file is already at …") |
| In both, RomM's file hash differs from `romm_file_md5` | **Update**: replace the file on the card |
| In both, same hash, RomM's file name changed | **Rename** the game file and its save on the card, no copy. The save record's path changes too, even if the save file is gone |
| In the manifest, not in the collection, card file still matches | **Remove**, unless its save conflict was skipped this run (**Hold**: removed once the conflict is answered) |
| In the manifest, not in the collection, card file changed by hand | Drop it from the manifest and leave the file alone |
| In the manifest, card file missing (deleted on the Pocket or by hand) | **Re-add**, listed separately in the confirmation so the user notices |
| No core for its system, or no suitable file | **Skip**, with a reason |

Adopting reads the existing file back from the card once to hash it. That's acceptable for cartridge-sized files, and it only happens the first time.

For speed on later runs, `GET /api/collections/{id}` gives `rom_ids` and `updated_at`. If neither changed since the last run, only games with `updated_after` the last sync need fetching.

The plugin can't see the card's free space. The confirmation shows the total to download, and running out of space during the transfer stops the run cleanly ([section 13](#13-safety-rules)).

### 9.4 Transfer pipeline

For each game to add or update:

1. **Download** into `host/tmp/<rom_id>/`, in pieces of up to 32 MB with `Range`, appending to one file and logging progress. A piece that fails is retried a few times before the game is marked failed.
2. **Check** size and hash against RomM's file details.
3. **Unpack** if needed, and check the unpacked file against `archive_members`.
4. **Copy to the card** as `<name>.part`, in large sequential writes, then rename to the final name.
5. **Record** the file in the manifest (in memory; written every few games).
6. **Delete** the game's staged files from `host/tmp/`.

Rules:

- No downloading or unpacking on the card, and no hashing there except when adopting.
- No reading files back from the card to check them. They were verified in `host/` first, so the size after copying is enough.
- Removals run before additions to free space.

Plugins run on one thread, so downloading and copying take turns rather than overlapping.

## 10. Save sync

### 10.1 Where saves live

- **On the Pocket:** `Saves/<platform>/common/<rom stem>.sav`, written by whichever core plays the game. Only `.sav` files are synced. `.rtc` clock files (mincer_ray.GBA) stay on the card untouched.
- **In RomM:** the **Pocket slot**, `analogue-pocket-<first 8 hex digits of the Pocket ID>` (for example `analogue-pocket-6f1c5d0e`), labelled `analogue-pocket`, holding the file byte for byte under its card name.
- Each Pocket is its own RomM device with its own slot. Pockets never share saves with each other, and in v1 not with libretro clients either ([ADR 0001](adr/0001-pocket-saves-stay-separate.md)).

### 10.2 Records

- RomM records which version of the Pocket slot the Pocket last synced.
- The manifest keeps the same thing on the card (`base_md5`, `romm_save_id`, `romm_updated_at`, `romm_content_hash`), so decisions don't depend on the server alone and any computer can carry on.

### 10.3 Decisions

Each side is compared with the last-synced state in the manifest's save record.

**The Pocket's side:**
- **same:** the card file's MD5 equals `base_md5`;
- **changed:** it differs;
- **deleted:** there's a save record but no card file;
- **new:** a card file but no save record (never synced);
- **none:** neither.

**RomM's side** (the Pocket slot's newest version):
- **same:** its `id` and `content_hash` match the record;
- **newer:** it's newer than the recorded version;
- **older:** the recorded version is gone from the slot and the newest left is older, because someone deleted newer versions in RomM;
- **deleted:** there's a save record but the slot is empty;
- **new:** a save in the slot but no save record;
- **none:** neither.

| Pocket | RomM | Action |
|---|---|---|
| same | same | nothing |
| changed | same | upload |
| same | newer | back up the Pocket's copy, then download |
| same | deleted | back up the Pocket's copy, delete it on the card, drop the record |
| same | older | **ask** (save conflict) |
| changed | newer, older or deleted | **ask** (save conflict) |
| deleted | same | nothing: it stays deleted |
| deleted | newer or older | **ask** (save conflict) |
| deleted | deleted | drop the record |
| new | none | upload |
| none | new | download |
| new | new | **ask**, unless the contents are identical |
| none | none | nothing |

- RomM's copy only changes when someone uploads, restores or deletes a save there by hand. RomM going back to an older version is asked about rather than rolling the Pocket back.
- Identical contents are never a conflict. The plugin records the match and confirms it to RomM (`POST /api/saves/{id}/downloaded`), otherwise RomM answers the next upload with `409`.
- **A save deleted on the Pocket stays deleted.** Its record stays in the manifest with no card file. RomM's copy comes back in two ways: RomM's copy changes (asked, since both sides changed), or the game is removed and later added back ([10.6](#106-removed-games)). If the Pocket writes a new save for the game, that one is "changed" and is uploaded. A **Restore deleted saves** option is planned for later ([section 18](#18-milestones)).
- **A save deleted in RomM is deleted on the Pocket too,** after a backup to `host/backups/`, unless the Pocket's copy also changed, which makes it a save conflict.
- Saves are small, so hashing them on the card is acceptable.

### 10.4 Upload and download details

- **Upload** with `device_id`, the Pocket slot, `content_hash` and without `overwrite`. A `409` means RomM's copy changed after the check, so the game joins the conflict questions.
- **"Keep Pocket's"** in a conflict uploads with `overwrite=true`.
- **RomM quirk (also worked around in Rommarch):** uploading a save identical to an older version returns that older version. If the reply is older than the newest listed, upload again with `overwrite=true` so it becomes the newest.
- **Download** with `device_id`, staged in `host/tmp/`; the card's copy is backed up to `host/backups/` first, then RomM's is copied to the card as `.part` and renamed.
- **The Pocket must be off the card while it syncs.** Pocket Sync already requires the card to be in a reader, or the Pocket in USB mode.

### 10.5 Conflicts and keeping the other copy

- **"Keep Pocket's":** the Pocket's copy is uploaded with `overwrite=true`, and RomM's stays in the Pocket slot's version history. If the Pocket's side is a deletion, nothing is uploaded and the save stays deleted.
- **"Use RomM's":** the Pocket's copy, if there is one, is backed up to `host/backups/` on this computer and uploaded to `<Pocket slot>-conflict`, so it isn't stuck on one computer. Then RomM's copy is written to the card, or, if RomM's side is a deletion, the card file is deleted.
- **Skip for now:** nothing moves; the question comes back next sync. A game being removed stays on the card until it's answered.

### 10.6 Removed games

When a game leaves the Pocket collection, its save is synced one last time (step 4 of [5.2](#52-sync-now)). Then the game's manifest entry goes, and the `.sav` stays on the card, ignored. If the game comes back later, the leftover save is treated as never synced: uploaded if the Pocket slot is empty, asked about unless identical otherwise. A save deleted on the Pocket before the game was removed is forgotten with the record, so adding the game back brings RomM's copy down again.

### 10.7 Shared saves (later)

A later milestone lets a game carry on between the Pocket and libretro clients (Rommarch, RomP, RomM's web player), switched on one system at a time once its conversion is tested on a real Pocket. The [research notes](research/save-compatibility.md) have the per-system rules: most systems need only a rename or a resize; Genesis and the Game Boy clock games need a lossless conversion; Neo Geo Pocket is incompatible.

- A shared system's saves go in RomM's `autosave` slot as `<rom stem>.srm` (plus `.rtc` for clock games) in libretro layout, which is what Rommarch expects.
- The milestone chooses between converting in the plugin and adding Pocket formats to RomM's own save conversion (argosy-sigil, unreleased).
- Switching a system over needs no special handling: its first sync meets two never-synced saves and asks unless they're identical.

## 11. Install on device

Designed now, built in M6.

- **Prerequisites:** RomM 5.4+ with `DEVICE_INSTALL_ENABLED` on (the default). Pairing already grants `devices.write` and `roms.read`.
- **Offering installs:** the plugin marks the device as accepting installs (`remote_install`) once M6 ships. "Analogue Pocket" then appears in RomM's Install on device dialog.
- **Lifecycle:** `pending` → `taken` (claim) → `done`, `already_installed` or `failed` (reported by the plugin), or `cancelled` (by the user in RomM).
- **Claiming** happens at the start of Sync now and returns every request the device holds, including ones taken by a run that was cut off.
- **Each claimed request:**
  - **Can't be played on this Pocket** (no installed core for the system, or no suitable file): report `failed` with the reason (500 characters at most), and don't add the game to the collection.
  - **Playable:** add it to the Pocket collection, then plan it like any other addition. The collection stays the only list of what belongs on the Pocket, and removing a sent game means taking it out of the collection.
  - **Already on the Pocket** with matching files: report `already_installed`.
  - **On the card after the transfer:** report `done`.
  - **Temporary failure** (card full, download error): leave the request unreported. The next sync claims it again and reports `done` once the game lands. If the request expires first, nothing is lost: the game is already in the collection.
  - **Cancelled in RomM after the claim:** the report call fails; ignore it.
- **Expiry:** requests expire `DEVICE_INSTALL_REQUEST_TTL_DAYS` after they're queued or claimed (2 by default) and vanish without notice. The plugin only runs when the user starts it, so the README recommends `DEVICE_INSTALL_REQUEST_TTL_DAYS=0` (never). Once claimed, the game is safe in the collection.
- **Persistence:** requests live in RomM's Redis/Valkey. The built-in one snapshots to `/redis-data` hourly and on a clean stop, so requests survive restarts when that folder is a persistent volume; a crash can lose the last hour.
- **RomM forgets finished requests at once.** It never knows what's on the card; the manifest is the only record.
- **The Pocket always shows "Offline"** in RomM's dialog, because "Online" means holding a live connection. Requests still wait for it.

## 12. Control flow

Pocket Sync calls `start()` once, then `handle_response()` with each answer, and shows whatever question the plugin returns. A question can only be asked by returning from a call. So:

- The run's state (phase, plan, pending conflict list, claimed requests) lives in the plugin's memory between calls, held by `lib`. Pocket Sync keeps one instance per run, and Extism resets an instance's memory only after a call to `_start`, which a Rust `cdylib` doesn't export (it exports `_initialize`). Each run gets a new instance, so nothing carries over from the last run. `lib` also drops the state when the plugin returns `Exit`, and an answer that arrives with no run is an error, not a new run.
- Long phases (planning, transfer) run inside one call and report progress through `print_msg`.
- A phase that needs answers (confirmation, conflicts) returns a question and continues in the next `handle_response`.
- Questions are named after their phase and subject (`confirm-plan`, `save-conflict:1234`), so a stale or unexpected answer is recognised and ignored. The run remembers the question waiting; an answer to another question, or one that matches none of its buttons, gets the same question again.

```
start ──► Check card, Pocket ID ──► [no server.json] ──Text──► Connect
                                └─► [server.json] ──Choice──► Connect / Options / Exit
Options ──► Change server address ──Text──► Connect
        └─► Unpair ──Choice──► Heartbeat ──► Delete the device in RomM ──► Delete server.json ──Text──► Connect
Connect ──► Heartbeat ──► Sign-in check ──► [none, or rejected] Pair ──► [no collection] ──Choice──► Menu
Menu ──Sync now──► (M6 Claim) ──► Plan ──► Saves ──Choice per conflict──► Confirm ──Choice──►
       Transfer ──► New-game saves ──► (M6 Report) ──► Finish ──► Menu
Menu ──Settings──► change collection / Unpair
Menu ──Exit──► Exit
```

`Kill` (sent while waiting for an answer) saves the manifest if it changed and returns `Exit`. Stop during a long call cuts the plugin off with no warning, which [section 13](#13-safety-rules) covers.

## 13. Safety rules

1. **Never touch a file the plugin didn't write.** Only managed files are replaced, renamed or deleted.
2. **No half-written files on the card.** Every card write goes to `.part` and is renamed when complete. Leftover `.part` files in the plugin's folders are deleted at the start of a run.
3. **The manifest is replaced safely** (`.tmp`, `.bak`, rename) and written every few games and after every save change, so a cut-off run loses at most a few game entries and no save records.
4. **A cut-off run recovers.** A card file with the right name that's missing from the manifest, and whose size and hash match RomM's, is adopted rather than downloaded again. (M6) Claimed requests come back on the next claim.
5. **Ask before deleting games,** and say how many and which.
6. **Saves are never lost.** The copy not chosen in a conflict is kept ([10.5](#105-conflicts-and-keeping-the-other-copy)), every card save the plugin replaces or deletes is backed up in `host/backups/`, and saves of removed games stay on the card.
7. **Card full:** stop the transfer, delete the current `.part`, write the manifest, and say how much was left. (M6) Waiting install requests stay unreported.
8. **Wrong server:** if pairing returns a RomM device ID different from the one in `server.json`, or the manifest's `romm_device_id` differs from `server.json`'s, the address now points at a different RomM server. Don't sync until the user confirms. Confirming starts fresh with this server: `collection` is cleared, the manifest's game records are cleared (the files become unmanaged and are never deleted), and matching files are adopted again on the next plan.
9. **Tokens never appear in logs.** An error about one of the plugin's files says where the problem is (line, column and field) and never quotes the file.

## 14. Performance

- Card writes happen once per game, as large sequential writes. No downloading, unpacking or reading back on the card.
- Downloads are pieces of up to 32 MB (under Extism's 50 MB limit), appended to one file in `host/tmp/`.
- The manifest is written every few games and after each save change, not per file.
- Later syncs fetch only a changed collection and changed games.
- Saves are hashed on the card, since they're small. Games are hashed on the card only when adopted.

## 15. Security and privacy

- `allowed_hosts: ["*"]` lets the plugin reach any web address. Pocket Sync doesn't show this to users, so the README says so plainly, and the plugin only ever contacts the RomM address the user entered.
- **The sign-in is on the card,** as `api_key` in `server.json`. Anyone holding the card can use it, with the permissions granted at pairing (games, collections, saves and the device), until it's revoked in RomM. That's the price of a card that syncs from any computer with no setup. The README tells users to revoke a lost card's token.
- The token is bound to the Pocket's RomM device. Revoking that device's token in RomM's settings cuts the Pocket off on every computer.
- Changing the address keeps the sign-in, so the next Connect sends it to the new address. A mistyped address that reaches someone else's server would receive it.
- Each pairing adds a token, and earlier ones stay valid until revoked in RomM (a RomM limitation).
- Nothing personal goes into the repo: no addresses, user names or real tokens in code, tests or fixtures.

## 16. Testing

| Level | What | How |
|---|---|---|
| Unit | Mapping, choosing files, planning (including adopt, rename and hold), manifest load/save/recovery, save decisions, parsing RomM replies | `cargo test` on the normal host target; the logic modules don't depend on Extism |
| Plugin | The whole flow against a fake card and a fake RomM | pocket-plugin's `demo_host` runs `plugin.wasm` from the terminal, or Pocket Sync runs it, with `test-pocket/` as the card ([running-locally.md](running-locally.md)) |
| Fake RomM | Pairing (pending, slow down, denied, expired, approved), heartbeat, collections, the game list with `collection_id` and `with_files`, file downloads with `Range`, saves with slots and `409`, (M6) install requests | A small fake server in `tools/`, started from the one used for Rommarch, which already does pairing, collections, games and saves |
| Fake card | Several `Cores/*/core.json` and `data.json` samples, `Assets/`, `Saves/` | Fixtures in `tests/fixtures/card/` |
| Real | A real Pocket and a real RomM 5.4 server | By hand before each release, in Pocket Sync on macOS, Windows and Linux |

Cases worth a test each:
- a run cut off mid-download, mid-copy, and mid-manifest write;
- the card filling up;
- `409` on upload;
- an identical save upload getting an older version back;
- a game replaced in RomM, and a game renamed in RomM;
- an identical file already on the card (adopt) and a different one (skip);
- a file deleted on the card;
- a removed game whose save conflict is skipped;
- a save deleted on the Pocket: stays deleted over several syncs, is asked about when RomM's copy changes, follows a rename, is uploaded when the Pocket saves again, and comes back when the game is removed and added back;
- a save deleted in RomM, with and without changes on the Pocket;
- RomM's newest save version deleted, so RomM goes back to an older one;
- a run cut off right after a save upload;
- the server address changed in Options and by hand, keeping the rest of `server.json`;
- a `server.json` written by hand with only `host`, and one that can't be read;
- an address that refuses the connection: the run ends, and Options is still reachable on the next run;
- two Pockets on one computer;
- the same card paired with a different RomM server.

## 17. Build and release

- **Toolchain:** stable Rust, `rustup target add wasm32-wasip1`, `extism-pdk`. Release builds use `cargo build --release --target wasm32-wasip1`.
- **Pure Rust only:** every dependency has to build for `wasm32-wasip1`, e.g. a zip reader with a pure-Rust deflate, and an MD5 crate.
- **`plugin.json`:**

  ```json
  {
    "name": "RomM Sync",
    "description": "Sync games and saves between your RomM library and this Pocket",
    "allowed_hosts": ["*"],
    "logo_url": "https://raw.githubusercontent.com/<owner>/pocket-sync-romm-plugin/main/docs/logo.png"
  }
  ```

- **Release:**
  - A GitHub release with `plugin.wasm` and `plugin.json` attached.
  - Pocket Sync installs and updates from the latest release, and records its tag as the version.
  - The plugin's ID in Pocket Sync is `<owner>-pocket-sync-romm-plugin`, which also names its `host/` folder. Renaming the repo makes it a different plugin and strands every user's save backups in `host/`, so the name is fixed.
- **CI:**
  - `.github/workflows/ci.yml` runs format, Clippy for both targets, the unit tests and the `wasm32-wasip1` build on every pull request and every push to `main`;
  - `.github/workflows/add_to_release.yml` builds and attaches the release files when a release is published;
  - later: the `demo_host` flow tests.

## 18. Milestones

Each milestone is small, tested commits, with its tests passing before the next starts.

| # | Milestone | Done when |
|---|---|---|
| M0 | Skeleton | The plugin builds for `wasm32-wasip1`, runs in `demo_host`, shows a menu, logs, and keeps state between calls. Its text comes from bundled locale files with English as the fallback (the language is fixed at `en-US` for now), and answers are matched to the choice they stand for, not to English text |
| M1 | Server and pairing | Pocket ID, server address in `server.json` with Connect and Options, heartbeat check, pairing with polling, token in `server.json`, permission check, Unpair, wrong-server check |
| M2 | Games: add | Choose or create the Pocket collection; plan and copy GB, GBC and GBA games; manifest; staging; `.part` then rename |
| M3 | Games: update, rename, remove, adopt | Hash-based updates, renames, confirmed removals, adopting existing files, recovery after a cut-off run, `Range` downloads |
| M4 | Saves | Pocket slot, decisions, conflicts, backups, final sync for removed games |
| M5 | First release | Every mapped cartridge system, README, `plugin.json`, release, a real-Pocket test on all three operating systems |
| M6 | Install on device | Accept installs, claim, add to the collection, report outcomes |
| Later | Shared saves (per system), Restore deleted saves (Settings), BIOS from RomM's firmware, translations into other languages, save states, pupdate | |

## 19. Open questions

1. **GBA clock files:** v1 leaves `.rtc` files alone. Revisit with shared saves.
2. **Free space:** ask Pocket Sync's author for a host function that reports the card's free space?
3. **Install request expiry:** a README note for now; revisit if users lose requests.
4. **Shared saves:** convert in the plugin or through RomM's sigil; which Game Boy layout to write (gambatte, used by Rommarch and the web player, or mGBA, used by RomP); whether sharing is a per-Pocket setting; how conflicts with libretro clients work in `autosave`. Also confirm whether the Pocket's timestamp is UTC or local time.
5. **Mappings and cores:** RomM's system names for PC Engine and the later cartridge systems; core quirks such as PC Engine's MB128 save size.

## 20. References

- [GLOSSARY.md](../GLOSSARY.md), [ADR 0001](adr/0001-pocket-saves-stay-separate.md), [save compatibility research](research/save-compatibility.md), [RomM API research](research/romm-api.md)
- Pocket Sync: <https://github.com/neil-morrison44/pocket-sync> (plugin host in `src-tauri/src/commands/plugins.rs`; plugins since v6.0.0)
- Plugin interface and `demo_host`: <https://github.com/openfpga-library/pocket-plugin>
- Extism: <https://extism.org>
- openFPGA `data.json` (save slots): <https://www.analogue.co/developer/docs/core-definition-files/data-json>
- RomM: <https://github.com/rommapp/romm>, version 5.4.0 or newer. Source read for this design:
  - `backend/endpoints/device/auth.py` and `backend/utils/device_auth.py` (pairing)
  - `backend/endpoints/saves.py` (save sync, `409`, pruning)
  - `backend/endpoints/collections.py` and `backend/handler/database/rom_filters.py` (collections and the game list)
  - `backend/endpoints/device/install.py` and `backend/handler/device_install/` (Install on device)
  - `docker/init_scripts/init` (Valkey persistence)
- RomP (for comparison: pairing, installs, save sync): <https://github.com/RompEmu/RomP>
