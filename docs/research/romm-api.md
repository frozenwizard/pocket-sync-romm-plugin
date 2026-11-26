# RomM API

**Status:** research notes, 2026-11-19. Read from RomM's source (`rommapp/romm` at `c9d06c8`, 2026-11-19) and RomP's client (`RompEmu/RomP` at `1f16682`, 2026-11-15). Nothing here was called against a live server. Covers every endpoint RomP calls and every endpoint [Architecture.md §8](../Architecture.md#8-romm-api-used) plans for the plugin. RomM has 292 routes in all; the rest are in the OpenAPI spec.

## Summary

- **OpenAPI:** yes. Every RomM server serves its spec at `/openapi.json`, with Swagger UI at `/api/docs` and ReDoc at `/api/redoc`. RomP doesn't use it; its client is written by hand.
- **Auth:** a bearer client token (`rmm_…`) from device pairing. Each endpoint needs one or more permissions (scopes).
- **Shapes:** JSON in and out, except uploads (multipart, with the metadata in the query string, not the form) and file content (raw bytes).
- **RomP** calls 46 routes plus one Socket.IO channel. The plugin needs 20 of those routes: 14 in v1, 4 more for M6 and 2 later.
- **What clients get wrong:** `/api/roms` returns heavy extras unless told not to; `updated_after` is strict and its `+00:00` must be escaped; collections take multipart forms and `PUT` replaces the whole membership; save calls with a `device_id` need `devices.*` as well as `assets.*`. Details in [Gotchas](#gotchas).

## OpenAPI

RomM is a FastAPI app (`backend/main.py:118-124`) that keeps FastAPI's default spec URL and moves the docs pages under `/api`:

| URL | What |
|---|---|
| `<server>/openapi.json` | The OpenAPI 3 spec as JSON. The Docker image's nginx forwards it to the backend (`docker/nginx/templates/default.conf.template:115`). |
| `<server>/api/docs` | Swagger UI: every route, its parameters and schemas, with "Try it out". |
| `<server>/api/redoc` | The same spec as ReDoc. |

- **Without a server:** `backend/tools/dump_openapi.py out.json` writes the spec from RomM's source. Importing the app needs the database settings in the environment but never connects.
- **Readable copy in RomM's repo:** the web frontend generates TypeScript types from `/openapi.json` with `openapi-typescript-codegen` (`frontend/package.json:31`) and commits them in `frontend/src/__generated__/models/`, one file per schema (`SaveSchema.ts`, `DeviceAuthTokenResponse.ts`, …). That's the quickest way to read a schema on GitHub. Spot-checked against the Python models; they match.
- **Socket payloads:** the spec also includes the Socket.IO payload models (`backend/utils/openapi.py`), but not the socket protocol itself.
- **Version drift:** the spec is generated from the running version, so it always matches the server you fetch it from. Endpoints added in 5.4 (notifications, installs, save confirmations) are missing from older servers' specs.

**For the plugin:** OpenAPI generators for Rust (openapi-generator, progenitor) emit `reqwest` clients, which don't run inside Extism. Like RomP, write small `serde` structs that hold only the fields the plugin reads, with `#[serde(default)]` on anything optional, so fields newer RomM versions add don't break decoding. A spec dumped from the RomM version the plugin targets is a useful reference to diff on upgrades and to keep the fake server honest.

## Conventions

- **Base URL:** the server address, possibly under a sub-path (`https://host/romm/`). Keep a trailing `/` and join relative paths onto it, so `api/heartbeat` becomes `https://host/romm/api/heartbeat`. RomP tries `https://` then `http://` when the user types no scheme (`client.rs:27-51`).
- **Auth header:** `Authorization: Bearer rmm_…`. `heartbeat`, `auth/device/init` and `auth/device/token` need none. When the server sets `DISABLE_DOWNLOAD_ENDPOINT_AUTH`, `GET /api/roms/{id}`, its content and firmware content need none either; sending the token anyway is harmless.
- **Errors:** JSON `{"detail": "…"}`.
  - **401:** no token, or one RomM doesn't accept: unknown, revoked, expired, or its user disabled (`raise_auth_error` in `decorators/auth.py`, `resolve_client_token` in `handler/auth/hybrid_auth.py`). Every cause gets the same `{"detail": "Not authenticated"}`. Deleting a device deletes its tokens (`client_tokens.device_id` has `ondelete="CASCADE"`, `models/client_token.py`), so a deleted device's token gets a 401, never the endpoint's 404.
  - **403:** the token lacks the permission (`{"detail": "Forbidden"}`, `decorators/auth.py:97`).
  - **404:** not found, or the endpoint is newer than the server. A token bound to one device gets a 404 for any other device (`require_token_device` in `utils/auth.py`).
  - **409:** save conflict.
  - **422:** bad parameters; `detail` is then a list of `{loc, msg, type}`.
  - **429:** rate limited.
- **Times:** ISO 8601 with an offset. RomM writes them in UTC (`2026-09-25T09:38:20+00:00`, `responses/base.py:8-11`). Times sent as query parameters need a timezone too.
- **Paging:** `limit` (default 50, up to 10,000) and `offset` (`responses/base.py:27-31`). Of the endpoints here, only `/api/roms` wraps its page in an object; the other lists are plain JSON arrays.
- **Query values:** booleans are `true`/`false`. A list repeats the key (`statuses=finished&statuses=retired`), except `file_ids`, which is comma-separated.

## Endpoints

"Plugin" means planned in [Architecture.md §8](../Architecture.md#8-romm-api-used): ✓ for v1, M6 for Install on device, later for after v1. "+ `devices.*`" means the call also needs that permission when it passes `device_id`.

### Connection and pairing

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/heartbeat` | none | ✓ | ✓ |
| `POST /api/auth/device/init` | none | ✓ | ✓ |
| `POST /api/auth/device/token` | none | ✓ | ✓ |
| `GET /api/users/me` | `me.read` | ✓ | |
| `GET /api/users/{id}/avatar` | `assets.read` | ✓ | |
| `POST /api/users/{id}/ra/refresh` | `me.write` | ✓ | |

### Library

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/platforms` | `platforms.read` | ✓ | ✓ |
| `GET /api/roms` | `roms.read` | ✓ | ✓ |
| `GET /api/roms/identifiers` | `roms.read` | ✓ | |
| `GET /api/roms/{id}` | `roms.read` | ✓ | ✓ |
| `GET /api/roms/{id}/content/{file_name}` | `roms.read` | ✓ | ✓ |
| `GET /api/roms/{id}/similar` | `roms.read` | ✓ | |
| `GET /api/recommendations` | `roms.read` | ✓ | |
| `PUT /api/roms/{id}/props` | `roms.user.write` | ✓ | |
| `GET /api/firmware` | `firmware.read` | ✓ | later |
| `GET /api/firmware/{id}/content/{file_name}` | `firmware.read` | ✓ | later |

### Collections

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/collections` | `collections.read` | ✓ | ✓ |
| `GET /api/collections/smart` | `collections.read` | ✓ | |
| `GET /api/collections/virtual?type=…` | `collections.read` | ✓ | |
| `GET /api/collections/{id}` | `collections.read` | ✓ | ✓ |
| `POST /api/collections` | `collections.write` | ✓ | ✓ |
| `PUT /api/collections/{id}` | `collections.write` | ✓ | |
| `DELETE /api/collections/{id}` | `collections.write` | ✓ | |
| `POST /api/collections/{id}/roms` | `collections.write` | ✓ | M6 |
| `DELETE /api/collections/{id}/roms` | `collections.write` | ✓ | |

### Saves, states and screenshots

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/saves` | `assets.read` + `devices.read` | ✓ | ✓ |
| `POST /api/saves` | `assets.write` + `devices.write` | ✓ | ✓ |
| `GET /api/saves/{id}/content` | `assets.read` + `devices.read` | ✓ | ✓ |
| `POST /api/saves/{id}/downloaded` | `devices.write` | ✓ | ✓ |
| `GET /api/states` | `assets.read` | ✓ | |
| `POST /api/states` | `assets.write` | ✓ | |
| `GET /api/states/{id}/content` | `assets.read` | ✓ | |
| `POST /api/states/delete` | `assets.write` | ✓ | |
| `POST /api/screenshots` | `assets.write` | ✓ | |
| `GET /api/screenshots/{id}/content` | `assets.read` | ✓ | |

### Play tracking

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/play-sessions` | `roms.user.read` | ✓ | |
| `POST /api/play-sessions` | `roms.user.write` | ✓ | |
| `POST /api/activity/heartbeat` | `roms.user.write` | ✓ | |
| `DELETE /api/activity/heartbeat` | `roms.user.write` | ✓ | |

### Devices and Install on device

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/devices/{device_id}` | `devices.read` | | ✓ |
| `DELETE /api/devices/{device_id}` | `devices.write` | | ✓ |
| `PUT /api/devices/{device_id}` | `devices.write` | ✓ | M6 |
| `POST /api/devices/{device_id}/installs/claim` | `devices.write`, `roms.read` | ✓ | M6 |
| `PUT /api/devices/{device_id}/installs/{request_id}` | `devices.write` | ✓ | M6 |
| Socket.IO `/devices` channel | `devices.read`, device token | ✓ | |

### Notifications (RomM 5.4)

| Call | Permission | RomP | Plugin |
|---|---|---|---|
| `GET /api/notifications` | `me.read` | ✓ | |
| `POST /api/notifications/read` | `me.write` | ✓ | |
| `DELETE /api/notifications/{id}` | `me.write` | ✓ | |
| `DELETE /api/notifications` | `me.write` | ✓ | |

## Requests and responses

Schema names are the ones in the spec and in `frontend/src/__generated__/models/`. Responses list the fields RomP or the plugin use; the schema has the rest.

### Heartbeat

`GET /api/heartbeat` → `HeartbeatResponse`:

```json
{
  "SYSTEM": {"VERSION": "5.4.0", "GIT_BRANCH": null, "SHOW_SETUP_WIZARD": false},
  "DEVICE_INSTALL": {"ENABLED": true, "EXCLUDED_PLATFORM_SLUGS": []},
  "SAVE_SYNC": {}, "METADATA_SOURCES": {}, "FILESYSTEM": {}, "EMULATION": {}, "FRONTEND": {},
  "OIDC": {}, "NOTIFICATIONS": {}, "CONVERTO": {}, "ROM_PATCHER": {}, "TASKS": {}
}
```

(The contents of the sections shown as `{}` are left out here.)

- **Version:** RomP takes the part of `SYSTEM.VERSION` before the first `.` as the major version and needs 5 or more; it also accepts `"development"` (`client.rs:53-69`). The plugin needs 5.4.
- **Older servers:** `DEVICE_INSTALL` is missing before 5.4, so read it as optional.
- **Server clock:** the response's HTTP `Date` header is the server's clock. RomP reads it to set its sync cursor (`client.rs:199-206`).

### Pairing

`POST /api/auth/device/init`, JSON:

```json
{
  "client_device_identifier": "6f1c5d0e…",
  "name": "RomP on studio",
  "client": "Romp",
  "platform": "macos",
  "client_version": "0.22.0",
  "requested_scopes": ["me.read", "me.write", "platforms.read", "roms.read", "roms.user.read", "roms.user.write", "firmware.read", "assets.read", "assets.write", "devices.read", "devices.write", "collections.read", "collections.write"]
}
```

That's RomP's request (`client.rs:208-224`, `pairing.rs:4-18`). The plugin asks for fewer permissions ([Architecture.md §8](../Architecture.md#8-romm-api-used)).

→ `DeviceAuthInitResponse`:

```json
{
  "device_code": "<64 chars>",
  "user_code": "FDF64KC5",
  "verification_path": "/pair/device",
  "verification_path_complete": "/pair/device?user_code=FDF64KC5",
  "expires_in": 600,
  "interval": 5
}
```

- **Approval link:** the paths are relative; join them onto the base URL to make the link to open, or to show as a QR code.
- **Older servers:** a 404 means a RomM older than 5.0, with no device sign-in.
- **Same device every time:** `client_device_identifier` is the client's own stable ID. Pairing again with the same one, approved by the same user, gets the same RomM device back (`get_device_by_client_identifier` in `device/auth.py`, `approve`). Otherwise RomM makes a new device and picks its ID (`uuid4`); a client can't choose it.

`POST /api/auth/device/token`, JSON `{"device_code": "…"}`. Poll it every `interval` seconds:

| Status | Body | Meaning |
|---|---|---|
| 400 | `{"detail": "authorization_pending"}` | Not approved yet |
| 400 | `{"detail": "slow_down"}` | Polling too fast: add 5 s to the interval (RomP, `pairing.rs:45-48`) |
| 400 | `{"detail": "access_denied"}` | The user denied it |
| 400 | `{"detail": "expired_token"}` | The code expired, is unknown, or was already used |
| 429 | `{"detail": "…"}` | Rate limited; RomP treats it like `slow_down` |
| 200 | `DeviceAuthTokenResponse` | Approved |

```json
{"id": 12, "access_token": "rmm_…", "device_id": "c1a2…", "scopes": ["roms.read", "…"], "expires_at": null}
```

- **Device ID:** `device_id` is RomM's ID for the device, the one save and install calls take. RomP fetches the same value from `GET /api/users/me` as `current_device_id`, which needs `me.read`.
- **Granted permissions:** `scopes` lists what was actually granted, which can be fewer than requested. RomP saves it and turns features off when a permission is missing (`app/save_sync.rs:63-95`).
  - The approver can grant only what was requested and what their own role holds (`device/auth.py`, `approve`); a Viewer's role holds only the `.read` permissions (`handler/auth/constants.py`).
  - On every request, a token's permissions are narrowed again to its user's current role (`effective_scopes`, `handler/auth/hybrid_auth.py`), so the list from pairing can go stale.
- **Expiry:** the approver can give the token an expiry (`expires_in` on approve), returned as `expires_at`. An expired token is treated as unknown.
- **The token is handed out once.** The next poll gets `expired_token`.
- **`slow_down` only while waiting:** RomM records each poll's time and answers `slow_down` to one inside `interval` of the last, but only while the pairing is pending (`device/auth.py`, `token`). The first poll after approval gets the token however soon it comes.

### User

`GET /api/users/me` → `UserSchema`. RomP reads `id`, `username`, `current_device_id`, `avatar_path` (empty string when there's no picture), `ra_username` and `ra_progression`.

`GET /api/users/{id}/avatar` → image bytes. `POST /api/users/{id}/ra/refresh`, JSON `{"incremental": true}` → empty; it asks RomM to fetch the user's latest RetroAchievements progress.

### Platforms

`GET /api/platforms` (optional `updated_after`) → array of `PlatformSchema`: `id`, `slug`, `fs_slug`, `name`, `display_name`, `custom_name`, `rom_count`, `category`, `abbreviation`, `alternative_names`, `firmware_count`, and others.

### Game list

`GET /api/roms` → `CustomLimitOffsetPage[SimpleRomSchema]`. Its parameters (`roms/__init__.py:710-795`, filters in `handler/database/rom_filters.py:118`):

| Parameter | Default | Notes |
|---|---|---|
| `limit`, `offset` | 50, 0 | `limit` up to 10,000; RomP uses 500 |
| `order_by`, `order_dir` | `""`, `asc` | Use `order_by=id` for stable paging |
| `updated_after` | none | Strictly after: `Rom.updated_at > updated_after` (`roms_handler.py:1672`) |
| `with_char_index` | **true** | A letter → position index for the web gallery |
| `with_filter_values` | **true** | Every genre, company, region… for the filter menus |
| `with_rom_id_index` | **true** | Every matching ROM ID in order; scans the whole result set |
| `with_total` | true | The count; free when `with_rom_id_index` is on |
| `with_provider_metadata` | **true** | When false, the per-provider metadata blobs (`igdb_metadata`, `ss_metadata`, …) come back `null` |
| `with_files` | false | Include each game's `files` |
| Filters | none | `platform_ids`, `collection_id`, `smart_collection_id`, `virtual_collection_id`, `search_term`, `last_played`, `statuses`, `favorite`, `group_by_meta_id`, `genres`, `regions`, … |

RomP's library page (`client.rs:273-292`):

```
GET /api/roms?offset=500&limit=500&order_by=id&order_dir=asc&with_char_index=false&with_filter_values=false&with_rom_id_index=false&updated_after=2026-09-25T09%3A38%3A20%2B00%3A00
```

The plugin's collection page, from §8 plus the flags above:

```
GET /api/roms?collection_id=12&with_files=true&with_provider_metadata=false&with_char_index=false&with_filter_values=false&with_rom_id_index=false&order_by=id&order_dir=asc&limit=200&offset=0
```

Response:

```json
{
  "items": [ /* SimpleRomSchema */ ],
  "total": 4596,
  "limit": 500,
  "offset": 500,
  "char_index": {},
  "rom_id_index": [],
  "filter_values": {}
}
```

`total` is `null` when both `with_rom_id_index` and `with_total` are false. The last page is the first one shorter than `limit`.

**`SimpleRomSchema`** has about 90 fields. The useful ones:

- **Identity:** `id`, `platform_id`, `platform_slug`, `platform_fs_slug`, `platform_display_name`, `name` (may be `null`; RomP falls back to the file name without its extension), `slug`.
- **File:** `fs_name`, `fs_name_no_tags`, `fs_name_no_ext`, `fs_extension`, `fs_path`, `fs_size_bytes`, `has_simple_single_file`, `has_nested_single_file`, `has_multiple_files`, `crc_hash`, `md5_hash`, `sha1_hash`.
- **Files:** `files`, an array of `RomFileSchema`, filled only with `with_files=true`.
- **Changes:** `created_at`, `updated_at`.
- **Artwork:** `path_cover_small`, `path_cover_large`, `merged_screenshots`. These are server paths under `/assets/romm/resources/…`; fetch them relative to the base URL.
- **The user's own data:** `rom_user` (`last_played`, `now_playing`, `backlogged`, `status`, `is_main_sibling`, `rating`, …).
- **Versions of the same game:** `sibling_roms` (`id`, `name`, `is_main_sibling`).
- **Metadata:** `metadatum` (genres, companies, release date, rating), and provider IDs (`igdb_id`, `ss_id`, `ra_id`, …).

**`RomFileSchema`:** `id`, `rom_id`, `file_name`, `file_path`, `file_size_bytes`, `full_path`, `is_top_level`, `crc_hash`, `md5_hash`, `sha1_hash`, `ra_hash`, `archive_members` (`[{name, size, crc_hash, md5_hash, sha1_hash}]` for archives, else `null`), and `category` (`game`, `dlc`, `hack`, `manual`, `patch`, `update`, `translation`, …).

### One game

- **`GET /api/roms/{id}`** → `DetailedRomSchema`. It has every `SimpleRomSchema` field, with `files` always filled, plus `user_saves`, `user_states`, `user_screenshots`, `user_collections`, `all_user_*` and `download_formats`.
- **`GET /api/roms/identifiers`** → `[4596, 4597, …]`: every ROM ID the user can see. RomP compares it with its local copy to find games deleted on the server.
- **`GET /api/roms/{id}/similar?limit=12`** (`limit` 1–50) → `[{rom: SimpleRomSchema, score, reasons: [{facet, value}]}]`.
- **`GET /api/recommendations?limit=24`** → the same items, each with `seed_rom_id` and `seed_rom_name` (the game the pick came from).
- **`PUT /api/roms/{id}/props`**, JSON `RomUserData` with every field optional: `now_playing`, `backlogged`, `status` (`incomplete`, `finished`, `completed_100`, `retired`, `never_playing`), `is_main_sibling`, `hidden`, `rating`, `difficulty`, `completion`, `pinned_media`. The query string can add `update_last_played=true` or `remove_last_played=true`. Returns `RomUserSchema`.
  - RomP sends `{}` with `update_last_played=true` to mark a game played.
  - To change play status it sends all three fields, with `"status": null` to clear one.

### Game files

`GET /api/roms/{id}/content/{file_name}`. Query:
- `file_ids`: comma-separated file IDs.
- `purpose`: `download` (the default) or `play`; it only changes how RomM's audit log records the fetch.
- `formats`: not used by RomP.

`{file_name}` is the name the download is served under. RomP pushes it as a single escaped path segment, so `#`, `?` and `/` in names survive (`client.rs:615-631`):

```
GET /api/roms/4596/content/Tir%20Na%20Nog.tzx?file_ids=9811&purpose=download
Range: bytes=1048576-
```

What comes back depends on how many files are selected (`roms/__init__.py:1829-1855`):

- **One file**, which includes any request with a single ID in `file_ids`: nginx serves the file itself, so `Range` works and the answer is `206`. A range past the end gets `416`.
- **Several files, with a `Range` header:** a cached ZIP, which can be resumed.
- **Several files, without `Range`:** a ZIP streamed by nginx's mod_zip, which can't be resumed.

RomP downloads one file at a time with its own `file_ids`, resuming with `Range`. It checks the result against the file's `sha1_hash`, or against `file_size_bytes` when there's no hash or the file is an archive (`download.rs:90-185`).

### Firmware

`GET /api/firmware?platform_id=…` → array of `FirmwareSchema`: `id`, `platform_id`, `file_name`, `file_size_bytes`, `crc_hash`, `md5_hash`, `sha1_hash`, `is_verified`. The content is at `GET /api/firmware/{id}/content/{file_name}`.

### Collections

- **`GET /api/collections`** (optional `updated_after`) → array of `CollectionSchema`:
  - `id`, `name`, `description`, `rom_ids`, `rom_count`;
  - `is_public`, `is_favorite`, `is_smart`, `is_virtual`;
  - `user_id`, `owner_username`;
  - `created_at`, `updated_at`;
  - `path_cover_small`, `path_cover_large`.
- **`GET /api/collections/smart`** → `SmartCollectionSchema` (adds `filter_criteria`, `filter_summary`). **`GET /api/collections/virtual?type=…`** → `VirtualCollectionSchema`, whose `id` is a string, not a number.
- **`GET /api/collections/{id}`** → one `CollectionSchema`. The plugin uses its `rom_ids` and `updated_at` as a quick check for changes.
- **`POST /api/collections`** takes a **multipart form**, not JSON:
  - Query string: `is_public` and `is_favorite`.
  - Form fields: `name` (plus optional `description`, `url_cover`, and an `artwork` file).
  - Returns the new `CollectionSchema`.

  ```
  POST /api/collections?is_public=false&is_favorite=false
  Content-Type: multipart/form-data; boundary=…

  --…
  Content-Disposition: form-data; name="name"

  Analogue Pocket
  --…--
  ```

- **`PUT /api/collections/{id}`** is also a multipart form, and `rom_ids` is **required**: a JSON array as text, such as `[1,2,3]`. It replaces the collection's games, so renaming means sending the current `rom_ids` back with the new `name`. RomP reads the collection first for this (`client.rs:567-581`).
- **`POST /api/collections/{id}/roms`** and **`DELETE /api/collections/{id}/roms`** take JSON `{"rom_ids": [7]}` and return the updated `CollectionSchema`. Use these to add or remove games without replacing the list.
- **`DELETE /api/collections/{id}`** → empty.

### Saves

The save rules (slots, `409`, pruning) are in [Architecture.md §8.2](../Architecture.md#82-save-rules-that-matter). This section is the wire format.

**`GET /api/saves?rom_id=5&slot=autosave&device_id=…`** (also `rom_ids`, `platform_id`) → array of `SaveSchema`:

```json
{
  "id": 1, "rom_id": 5, "user_id": 1,
  "file_name": "Zelda.srm", "file_size_bytes": 8192,
  "slot": "autosave", "emulator": "snes9x", "content_hash": "3f2a…",
  "created_at": "2026-09-26T10:00:00+00:00", "updated_at": "2026-09-26T10:00:00+00:00",
  "download_path": "…", "screenshot": null, "origin_device_id": "c1a2…",
  "device_syncs": [{"device_id": "c1a2…", "device_name": "Analogue Pocket", "last_synced_at": "…", "is_untracked": false, "is_current": true}]
}
```

Several emulators' saves can share a slot. RomP picks its own by `emulator` and takes the newest by `updated_at` (`saves.rs:114-118, 534-543`).

**`POST /api/saves`**: all the metadata goes in the **query string**, and the file goes in multipart field `saveFile` (`saves.py:259-300`):

| Query parameter | Notes |
|---|---|
| `rom_id` | Required |
| `slot` | Up to 255 characters |
| `emulator`, `core` | Up to 50 characters each |
| `emulator_version`, `core_version` | Up to 100 characters each |
| `device_id` | Needs `devices.write` too; turns on the conflict check |
| `content_hash` | RomM 5.4 records it as the version this device holds |
| `overwrite` | Default `false`; `true` skips the conflict and same-content checks |
| `session_id` | Optional; RomP never sends it outside tests |
| `autocleanup`, `autocleanup_limit` | Default `false`, 10 |

```
POST /api/saves?rom_id=5&slot=autosave&emulator=snes9x&device_id=dev&overwrite=false&content_hash=3f2a…
Content-Type: multipart/form-data; boundary=…

--…
Content-Disposition: form-data; name="saveFile"; filename="Zelda.srm"

<bytes>
--…--
```

An optional `screenshotFile` part can go alongside. The response is the new `SaveSchema`, or **409** `{"detail": "…"}` when the slot's newest save isn't the one this device last synced.

**`content_hash`** is the lowercase hex MD5 of the file. For a ZIP, RomP hashes it the way it says RomM does: one `name:md5` line per file inside the ZIP, sorted, joined with `\n`, then MD5 of that (`saves.rs:16-41`).

**`GET /api/saves/{id}/content?device_id=…`** → the save's bytes. Other parameters:
- `optimistic`: default `true`. With a `device_id`, RomM records the device as holding this version as soon as it downloads (`saves.py:660-673`). RomP sends `optimistic=true`, which changes nothing.
- `session_id`.
- `core`, `option`, `profile`, `companion`: RomM's save conversion, unused by RomP.

**`POST /api/saves/{id}/downloaded`**, JSON `{"device_id": "…", "content_hash": "…"}` → `SaveSchema`. This tells RomM the device holds this exact version. It exists from RomM 5.4; RomP only logs a failure.

### States and screenshots

- **`GET /api/states?rom_id=5`** → array of `StateSchema`: `id`, `rom_id`, `file_name`, `emulator`, `core`, `updated_at`, and `screenshot` (`{id, …}` or `null`).
- **`POST /api/states`** works like a save upload:
  - Query string: `rom_id`, `emulator`, `core`, `device_id`.
  - Multipart: `stateFile`, plus an optional `screenshotFile`.
  - Returns `StateSchema`.
- **`GET /api/states/{id}/content`** → bytes.
- **`POST /api/states/delete`**, JSON `{"states": [3, 4]}` → the deleted IDs.
- **`POST /api/screenshots?rom_id=7`**, multipart `screenshotFile` (RomP sends `image/png`) → `ScreenshotSchema`.
- **`GET /api/screenshots/{id}/content`** → bytes.

### Play tracking

**`POST /api/play-sessions`**, JSON:

```json
{"device_id": "c1a2…", "sessions": [{"rom_id": 7, "start_time": "2026-09-30T10:00:00+00:00", "end_time": "2026-09-30T10:30:00+00:00", "duration_ms": 1800000}]}
```

→

```json
{"results": [{"index": 0, "status": "created", "id": 1}], "created_count": 1, "skipped_count": 0}
```

A session's `status` is `created`, `duplicate` or `error`. A session may also carry a `save_slot`.

- **`GET /api/play-sessions?rom_id=7&limit=100&offset=0`** (also `device_id`, `start_after`, `end_before`) → a plain array of `PlaySessionSchema`. Page through it until a page comes back shorter than `limit`.
- **`POST /api/activity/heartbeat`**, JSON `{"rom_id": 7, "device_id": "…"}` → `ActivityEntrySchema`. This marks the game as being played now on that device.
- **`DELETE /api/activity/heartbeat?device_id=…`** clears it.

### Devices and Install on device

- **`DELETE /api/devices/{device_id}`** → 204, no body. It deletes the device, its tokens, its save-sync records and its waiting install requests (`device/__init__.py`, `delete_device`). The permission check comes first (403 without `devices.write`), then a token bound to another device gets a 404.
- **`PUT /api/devices/{device_id}`**, JSON `DeviceUpdatePayload` with every field optional. RomP sends only `{"capabilities": {"remote_install": true}}` to offer the device in RomM's "Install on device", and `false` to withdraw it. Returns `DeviceSchema`. RomP only does this when the heartbeat's `DEVICE_INSTALL.ENABLED` is true.
- **`POST /api/devices/{device_id}/installs/claim`** (no body) → array of `InstallRequestSchema`. It takes every request waiting for the device and returns everything the device holds, oldest first. Only the device's own token can claim.

  ```json
  [{"id": "r1", "user_id": 1, "device_id": "c1a2…", "rom_id": 7, "file_ids": [3, 4], "status": "taken", "reason": null, "created_at": "…", "updated_at": "…"}]
  ```

  An empty `file_ids` means the whole game.

- **`PUT /api/devices/{device_id}/installs/{request_id}`**, JSON `{"status": "done", "reason": null}` → `InstallRequestSchema`.
  - The statuses are `pending`, `taken`, `done`, `already_installed`, `failed` and `cancelled`; a client reports `done`, `already_installed` or `failed`.
  - `reason` is at most 500 characters (`responses/device/install.py:18`).

### Device socket

RomP hears about installs the moment they're queued, over Socket.IO 5 on Engine.IO 4, as text frames on a WebSocket (`romm/device_socket.rs`, `romm/socketio.rs`):

- **URL:** `wss://<base>/ws/socket.io/?EIO=4&transport=websocket` (`ws://` for `http://`).
- **Joining:** the server opens with `0{"sid":…,"pingInterval":25000,"pingTimeout":20000}`. The client joins the channel with `40/devices,{"token":"rmm_…"}`.
- **Accepted:** `40/devices,{"sid":…}`. **Refused:** `44/devices,{"message":…}`; reconnecting won't help.
  - Only a token bound to a device, holding `devices.read`, is let in (`sockets/devices.py:105-120`).
- **Keep-alive:** the server sends `2` (ping); the client answers `3` (pong). Silence for `pingInterval + pingTimeout` means the link is gone.
- **Events:**
  - `42/devices,["install:queued",{"id":"r1","rom_id":7}]`
  - `42/devices,["install:cancelled",{"id":"r1","rom_id":7}]`
  - Both are sent to that device's room only (`sockets/devices.py:176-197`). RomP answers a connect or a queued event by claiming (above).

The plugin can't hold a socket open between runs, so it claims at the start of each sync instead ([Architecture.md §11](../Architecture.md#11-install-on-device)).

### Notifications

These exist from RomM 5.4 on; older servers answer `404`, which RomP treats as "none".

- **`GET /api/notifications`** → array of `NotificationSchema`, newest first: `id`, `kind`, `level`, `title`, `body`, `link`, `icon`, `data`, `actor`, `read_at`, `created_at`. A known `kind` comes with no `title`; the client words it.
- **`POST /api/notifications/read`**, JSON `{"ids": [3, 4]}` (`null` means all) → empty. RomP sends at most 200 IDs per call.
- **`DELETE /api/notifications/{id}`** dismisses one; **`DELETE /api/notifications`** dismisses all.

## Gotchas

1. **`/api/roms` is heavy by default.** `with_char_index`, `with_filter_values` and `with_rom_id_index` default to true, and the ID index scans the whole result set on every page. Turn all three off, and `with_provider_metadata` too, unless the client needs them. This matters for the plugin: Extism caps a response at 50 MB.
2. **`updated_after` is strictly after,** and the client's clock may disagree with the server's. RomP stores the server's `Date` header minus 60 seconds as the next cursor (`sync.rs:8-12, 160-195`).
3. **Escape `+` in times.** In a query string an unescaped `+` reads as a space, so `+00:00` has to be sent as `%2B00%3A00`. `reqwest`'s `.query()` does this; a URL built by hand must too, or can send `Z` instead.
4. **Collections take multipart forms,** with flags in the query string. `PUT /api/collections/{id}` replaces the game list and requires `rom_ids` as JSON text. Use `POST`/`DELETE …/roms` to add or remove games.
5. **Uploads put metadata in the query string,** not in the form. Only the file parts (`saveFile`, `stateFile`, `screenshotFile`) are multipart.
6. **`device_id` costs permissions:** on save calls it also needs `devices.read` (list, download) or `devices.write` (upload) (`saves.py:296, 546, 904`).
7. **File names go in the path,** so escape them as one segment.
8. **Defaults RomP restates:** `optimistic=true` on save downloads and `purpose=download` on game files are already the defaults.
9. **Length limits:** `slot` 255, `emulator` 50, install `reason` 500.
10. **Endpoints differ by version:** 5.4 added notifications, installs, `content_hash` on uploads and `POST /api/saves/{id}/downloaded`. On older servers check the heartbeat version, or treat `404` as "not supported".

## Compared with Architecture.md §8

- **The game list call** (`GET /api/roms?collection_id=…&with_files=true…`) should add `with_char_index=false&with_filter_values=false&with_rom_id_index=false&with_provider_metadata=false&order_by=id&order_dir=asc` (gotcha 1).
- **§9.3's "games with `updated_after` the last sync"** should take "the last sync" from the server's clock, and send the time escaped or with `Z` (gotchas 2 and 3).
- **Everything else in the §8 table matches RomM's source,** including the extra `devices.*` permissions on save calls and `device_id` in the token response.

## Sources

- **RomM**, `rommapp/romm` at `c9d06c8`:
  - routes in `backend/endpoints/` (`device/auth.py`, `device/install.py`, `roms/__init__.py`, `saves.py`, `collections.py`, `sockets/devices.py`, …);
  - schemas in `backend/endpoints/responses/` and `frontend/src/__generated__/models/`;
  - app setup in `backend/main.py`, `docker/nginx/templates/default.conf.template`.
- **RomP**, `RompEmu/RomP` at `1f16682`, in `crates/romp-app/src/`: `romm/client.rs` (every call, with wiremock tests), `romm/types.rs`, `romm/pairing.rs`, `romm/device_socket.rs`, `romm/socketio.rs`, `sync.rs`, `download.rs`, `saves.rs`. GPL-3.0-or-later: read for behaviour, not copied.
- RomM's route list was pulled from source with Python's `ast` module, without running RomM.
