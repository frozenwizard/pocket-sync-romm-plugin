# RomM Sync

A Pocket Sync plugin that keeps an Analogue Pocket's games and saves in step with a RomM library.

## Language

### Devices

**Pocket**:
An Analogue Pocket and its SD card, treated as one thing. Its identity lives on the card, and RomM knows it as one device.
_Avoid_: card, SD card (as something separate from the Pocket)

**Computer**:
A machine running Pocket Sync with the Pocket's card attached. Any computer can sync the Pocket with no setup, because the RomM address and sign-in are on the card.
_Avoid_: host, desktop

### What goes on the Pocket

**Game**:
One entry in the RomM library, made of one or more files. RomM's API calls it a "rom".
_Avoid_: ROM, rom, title

**Pocket collection**:
The one regular RomM collection a Pocket mirrors: the games in it are the games that belong on the Pocket. Games sent to the Pocket with Install on device are added to it.
_Avoid_: home collection, followed collections, device collection

**Install request**:
RomM's queued ask to put one game on one device, made with Install on device.
_Avoid_: install, push, send

**Claim**:
A Pocket taking its waiting install requests from RomM, so no other run handles them.
_Avoid_: fetch, accept

### Records

**Manifest**:
The record on the Pocket of every file the plugin put there, and the last-synced state of each save.
_Avoid_: index, database, state file

**Managed file**:
A file on the Pocket that is in the manifest and still matches it. The only kind of file the plugin replaces or deletes.
_Avoid_: owned file, synced file

**Adopt**:
To record a file already on the Pocket, identical to RomM's copy, as a managed file without copying it again.
_Avoid_: take over, import

### Saves

**Pocket save**:
A game's in-game save exactly as the Pocket's core wrote it. It belongs to one Pocket and is never shared with another Pocket.
_Avoid_: srm, save state (save states are the Pocket's Memories, out of scope)

**Pocket slot**:
The RomM save slot that holds one Pocket's saves and nothing else, named after the Pocket's ID.
_Avoid_: autosave, save slot (unqualified)

**Save conflict**:
A Pocket save and RomM's copy of it that have both changed since that Pocket last synced it. A deletion, or RomM going back to an older version, counts as a change.
_Avoid_: collision, mismatch
