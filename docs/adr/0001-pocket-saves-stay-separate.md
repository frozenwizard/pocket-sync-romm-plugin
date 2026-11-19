# Pocket saves stay separate until each system is tested

In v1, each Pocket's in-game saves go to RomM byte for byte, in a slot that belongs to that Pocket alone, and are not shared with RetroArch-based clients, RomM's web player or another Pocket. Sharing with libretro clients comes in a later milestone, switched on one system at a time once its conversion has been tested on a real Pocket. FPGA cores and libretro cores are different things, and every rule for moving saves between them so far was read from source code, not tested. Putting untested conversions into the save history other clients rely on would risk exactly the data the plugin promises never to lose.

The [research notes](../research/save-compatibility.md) found that most systems need only a rename or a resize. Genesis and the Game Boy clock games (Pokémon Gold/Silver/Crystal) need a lossless conversion. The WonderSwan clock is lost going to libretro, and Neo Geo Pocket saves are incompatible. Two facts the Game Boy clock conversion depends on are unconfirmed: whether the Pocket's timestamp is UTC or local time, and the byte order of its clock record.

## Considered options

- **Share from v1, converting in the plugin.** Rejected for v1: untested conversions into a shared history.
- **Keep saves separate for good.** Rejected: most systems convert cleanly, and playing the same game on the Pocket and in Rommarch is wanted eventually.
- **Wait for RomM to convert saves itself** (argosy-sigil, on RomM's master branch but unreleased, with no Pocket formats yet). Kept open: the shared-saves milestone chooses between converting in the plugin and adding Pocket formats to sigil.
- **One slot shared by all of a user's Pockets.** Rejected: each Pocket is its own device and doesn't share with other Pockets.

## Consequences

- In v1, a game started on the Pocket doesn't carry on in Rommarch, RomP or the web player, and the other way round.
- v1 has no save conversion code.
- RomM's copy of a Pocket save only changes when someone uploads or restores it by hand, so save conflicts are rare.
- When a system switches to sharing, its first sync meets two never-synced saves (the Pocket's and libretro's) and asks unless they're identical, so the switch needs no special handling.
