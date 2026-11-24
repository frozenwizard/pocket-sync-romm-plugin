# Security

## Reporting a problem

Report security problems privately through GitHub: on this repo's **Security** tab, choose **Report a vulnerability**, or go straight to <https://github.com/frozenwizard/pocket-sync-romm-plugin/security/advisories/new>. Please don't open a public issue for them.

Include what you found, how to reproduce it, and which version of RomM Sync, Pocket Sync and RomM you used. RomM Sync is maintained in spare time, so replies are best effort, but every report is read.

## What counts

RomM Sync runs inside Pocket Sync, reads and writes the Pocket's SD card, and talks to the user's RomM server. Problems in the plugin itself are in scope, for example:

- the RomM sign-in (`api_key` in `romm-sync/server.json`) reaching a log, a server other than the one the user entered, or anywhere else it shouldn't;
- the plugin changing or deleting files on the card that it didn't write, or losing a save ([Architecture §13](docs/Architecture.md#13-safety-rules));
- the plugin contacting any address other than the RomM server the user entered.

Problems in RomM, Pocket Sync or Extism belong with those projects.

## Known trade-offs

These are deliberate, and described in [Architecture §15](docs/Architecture.md#15-security-and-privacy):

- The sign-in is kept on the card, so any computer can sync the Pocket with no setup. Anyone holding the card can use it until it's revoked in RomM's settings.
- `plugin.json` allows requests to any address (`"allowed_hosts": ["*"]`), because every user's RomM server is at a different address.

## Supported versions

Only the latest release gets fixes.
