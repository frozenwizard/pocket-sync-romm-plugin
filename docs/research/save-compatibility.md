# Pocket and libretro save compatibility

**Status:** research notes, 2026-11-18. Everything here was read from core source code and docs. Nothing has been tested on a Pocket or in RetroArch yet. Feeds [ADR 0001](../adr/0001-pocket-saves-stay-separate.md) and Q31 of the design review.

## Summary

| System | Pocket → libretro | libretro → Pocket | Confidence |
|---|---|---|---|
| GB, GBC (no clock) | Rename. MBC2 for mGBA: pack nibbles | Rename; cut to the Pocket's size (required for 128 KB Japanese Crystal saves) | high to medium |
| GB, GBC with MBC3 clock (Pokémon Gold/Silver/Crystal) | Drop the 16-byte clock trailer and write a `.rtc` | Build the trailer from the `.rtc` | medium |
| GBA (mincer_ray.GBA) | Rename; the `.rtc` sidecar can be dropped | Rename; pad 512 B EEPROM saves to 8 KB | medium-high |
| GBA (Spiritualized.GBA) | Probably rename | Unknown | low |
| NES | Rename (cutting to the real size is cleaner) | Rename | medium-high |
| SNES | Rename | Rename | high |
| Genesis | Lossless conversion: expand | Lossless conversion: collapse | medium-high |
| Master System, Game Gear | Rename | Rename | high |
| PC Engine | Rename | Rename | high |
| WonderSwan | Drop the 12-byte clock; the clock is lost | Rename | medium |
| Neo Geo Pocket | Incompatible | Incompatible | low |
| Lynx | No saves on the Pocket | — | high |

Every system with Pocket saves except Neo Geo Pocket can move both ways without losing game progress. Most need only a rename or a resize. Genesis and the Game Boy clock need a real conversion. Two Game Boy facts are unconfirmed and matter for the clock: whether the Pocket's timestamp is UTC or local time, and the byte order of the clock trailer.

## Rules on both sides

- **Pocket:** a save file larger than the data slot's `size_maximum` won't load; a smaller one loads. The size written back comes from the core's datatable, which every core below sets itself, so the core decides the file size ([data.json docs](https://www.analogue.co/developer/docs/core-definition-files/data-json)). Saves live at `Saves/<platform>/common/<rom stem>.sav`.
- **RetroArch:** it loads min(file size, core's save size) bytes and only warns when the file is larger (RetroArch `tasks/task_save.c:441-451`). A padded file loads; the next save comes out at the core's own size. It writes a `.rtc` only when the core reports a clock (`save.c:317, 669-681`).
- **Which Pocket core per system:** there are no download counts, so the most current, maintained core was picked.

## Which libretro cores matter

| System | Rommarch | RomM web player | RomP |
|---|---|---|---|
| GB, GBC | gambatte, then SameBoy | gambatte / mGBA | mGBA |
| GBA | mGBA | mGBA | mGBA |
| NES | Mesen, then FCEUmm | FCEUmm / Nestopia | Nestopia |
| SNES | Snes9x | Snes9x | Snes9x |
| Genesis, SMS, Game Gear | Genesis Plus GX | Genesis Plus GX | Genesis Plus GX |
| PC Engine | Beetle PCE Fast | Beetle PCE | Beetle PCE Fast |
| Neo Geo Pocket | Beetle NeoPop | Beetle NeoPop | Beetle NeoPop |
| WonderSwan | Beetle Cygne | Beetle Cygne | Beetle Cygne |
| Lynx | Handy | Handy | Beetle Lynx |

Sources: Rommarch `network/romm/romm_library.c:189-216`; RomM `frontend/src/utils/index.ts:433-486`; RomP `crates/romp-app/src/cores.rs:54-98`.

## Game Boy, Game Boy Color, Game Boy Advance

| Pair | Pocket → libretro | libretro → Pocket |
|---|---|---|
| budude2 GB/GBC → gambatte, no clock, 8/32/128 KB RAM | Rename (high) | Rename (high) |
| same, 2 KB RAM or MBC2 (Pocket 512 B / 2 KB, gambatte 8 KB) | Rename; RetroArch loads a short file partially (high) | Cut to the Pocket's size; unchanged probably works (medium) |
| same, 64 KB RAM (Japanese Crystal, MBC30) | Rename (high) | Must cut gambatte's 128 KB to 64 KB, or the tail is read as a clock trailer (medium) |
| budude2 MBC3 clock → gambatte | Drop the 16-byte trailer, write an 8-byte `.rtc` (medium) | Build the trailer from the `.rtc` (medium) |
| budude2 → mGBA, MBC2 | Pack 512 nibbles into 256 bytes (high) | Unpack (high) |
| budude2 → mGBA, MBC3 clock | Trailer to mGBA's 48-byte `.rtc` and back (medium) | Same (medium) |
| Spiritualized GB/GBC 1.3.0 → either | Probably rename (low, closed source) | Probably rename (low) |
| mincer_ray.GBA 1.0.1 → mGBA | Rename; the `.rtc` sidecar is dropped without real loss (medium-high) | Rename; pad 512 B EEPROM saves to 8 KB to be safe (medium) |
| Spiritualized.GBA 1.2.0 → mGBA | Unknown, probably rename (low) | Unknown (low) |

### Layouts

- **budude2.GB / budude2.GBC:** the file is the exact cartridge RAM size: 512 B for MBC2/MBC7/TAMA, then 2/8/32/64/128 KB (openfpga-GBC `cart.v:416-422`). Clock games (MBC3, HuC3) add 16 bytes (`save_handler.sv:103-146`, `mbc3.v:187-212`):
  - bytes 0–3: the Pocket's timestamp;
  - bytes 4–9: seconds, minutes, hours, days, carry and halt, bit-packed;
  - bytes 10–15: 0xFF.
  
  On load, the clock catches up by the Pocket's time minus the stored timestamp, if the Pocket's clock is valid (`rtc_loader.sv`). The save slot's `size_maximum` is 0x20050.
- **gambatte:** the `.srm` is cartridge RAM in 8 KB banks, so 2 KB and MBC2 saves become 8 KB and a 64 KB cartridge becomes 128 KB (`cartridge.cpp:914-933`). The `.rtc` is one little-endian 64-bit base time; the clock reads as now minus that base, and halt and carry aren't saved (`rtc.h:40-47, 81`; `rtc.cpp:41-63`). To write one: base = timestamp − elapsed (+512 days if carry). RomM's sigil does the same (`clock_gb.c:144-148`).
- **mGBA (GB):** 2 KB RAM becomes 8 KB; MBC2 is 256 bytes with the even cell in the low nibble (`mbc.c:331-346, 441-446`). Its `.rtc` is 48 bytes: five current registers, five latched registers, a 64-bit Unix time (`mbc.h:34-46`). Standalone mGBA ignores a trailer shorter than 44 bytes, so the Pocket's 16 bytes are skipped rather than misread (`mbc.c:654-656`).
- **mincer_ray.GBA:** writes 64 KB for every game except FLASH1M games, which get 128 KB (openfpga-GBA `core_top.sv:1076-1078, 1463`). The clock is a separate 16-byte `.rtc`: a 32-bit timestamp, then a 42-bit BCD date and time. With no valid record, the core starts from the Pocket's clock (`rtc_persistence.sv:205-216`). Versions 0.7.1 and older appended the 16 bytes to the `.sav`; 1.0.1 still reads that. It wraps the MiSTer GBA core, so the save layout should match MiSTer's, which save-file-converter treats as the same as emulators' (`GameboyAdvance.js:1-11`; not checked in the VHDL).
- **mGBA libretro (GBA):** reports 128 KB until the save type is detected, then the chip size (`mgba_libretro.c:2307-2316`), and ignores bytes past the chip size (`savedata.c:267-325`). It exposes no clock for GBA; the game sees the host's local time (`:2326-2340`, `gpio.c:300-326`).

### Clock games

- **Pokémon Gold/Silver/Crystal:** without conversion the clock is lost. gambatte with no `.rtc` starts from base time 0, which sets the carry flag, and Crystal then reports the clock as reset (pokecrystal `engine/rtc/rtc.asm:102-114`; inferred, not tested). On the Pocket, no trailer means the clock starts near zero: wrong time and day, no prompt (inferred).
- **Pokémon Ruby/Sapphire/Emerald:** both sides use real local time, so dropping the clock file is harmless (inferred).

## Consoles and other handhelds

| System (Pocket core → libretro core) | Pocket → libretro | libretro → Pocket | Confidence |
|---|---|---|---|
| NES (agg23.NES → FCEUmm, Mesen, Nestopia) | Rename. RetroArch reads the first N bytes (usually 8 KB); cutting to N is cleaner. | Rename; the Pocket pads back to 256 KB | medium-high |
| SNES (agg23.SNES → Snes9x) | Rename | Rename | high |
| Genesis (ericlewis.Genesis, drizzt.MegaDrive → Genesis Plus GX) | Lossless conversion: expand | Lossless conversion: collapse | medium-high |
| Master System, Game Gear (drizzt.SMS, drizzt.GG → Genesis Plus GX) | Rename (optionally trim trailing 0xFF) | Rename | high (SRAM games) |
| PC Engine (agg23.PC Engine → Beetle PCE Fast, Beetle PCE) | Rename | Rename | high |
| WonderSwan (agg23.WonderSwan → Beetle Cygne) | Drop the 12-byte clock trailer (RetroArch's truncation does this anyway); the clock is lost | Rename (no clock trailer) | medium |
| Neo Geo Pocket (janisc.NGPC → Beetle NeoPop) | Incompatible | Incompatible | low |
| Lynx (budude2.Lynx) | No saves on the Pocket | — | high |

### Notes

- **NES:** the Pocket writes a fixed 256 KB file for any battery game (openfpga-NES `core_top.v:486`, `size_maximum` 0x40000). Battery RAM at offset 0 is inferred from the MiSTer mapper layout and matches save-file-converter's `Nes.js:39-47`. FCEUmm, Mesen and Nestopia all expose the mapper's battery RAM as the save. Mesen also writes `.sav.chr` for boards with battery-backed CHR RAM.
- **SNES:** both sides size the save as 1024 << n from the cartridge header (openfpga-SNES `core_top.sv:614`, Snes9x `libretro.cpp:2362`), up to 128 KB. Snes9x keeps clock data in a separate `.rtc`.
- **Genesis:** the Pocket stores save RAM packed, one byte per address (`sram_addr = MBUS_A[16:1]`), as a fixed 64 KB file. Genesis Plus GX stores it at odd offsets with 0xFF between and trims trailing 0xFF (gpgx `libretro.c:3753-3771`).
  - Pocket → GPGX: strip trailing 0xFF, then `out[2i] = 0xFF`, `out[2i+1] = in[i]`.
  - GPGX → Pocket: `out[i] = in[2i+1]`, then pad to 64 KB with 0xFF.
  - EEPROM games aren't expanded on either side (inferred). ericlewis.Genesis only ships an STM95 EEPROM model, so I2C-EEPROM games may not save at all (inferred).
- **Master System, Game Gear:** both sides use 32 KB, linear, prefilled with 0xFF.
- **PC Engine:** both sides use a raw 2 KB backup RAM. With MB128 on, the core writes 128 KB but its `size_maximum` is 0x4000, so it may refuse its own file (inferred).
- **WonderSwan:** the Pocket file is the SRAM (or a 2 KB EEPROM area) plus 12 bytes of clock. Beetle Cygne uses the SRAM, or EEPROM at 128 B / 1 KB / 2 KB.
- **Neo Geo Pocket:** the Pocket uses its own "NGPCSAV4" container (cartridge CRC32, a dirty-block bitmap, run-length-packed flash blocks, 0xFE00 bytes) and rejects any other file. Beetle NeoPop writes its own `<base>.flash` block list. Converting would need the ROM; no converter exists.
- **Lynx:** the Pocket core has no save slot. Handy writes `<content>.eeprom` itself.

## What Rommarch needs from a save

- Rommarch uploads every `<base>.*` file in the save folder except states, `.png` and `.tmp`, unchanged, so both `.srm` and `.rtc` go up. Slot `autosave`, labelled with the core's name, 10 versions kept (Rommarch `network/romm/romm_saves.c:37-40, 535-558, 1099-1140`).
- On download it matches files by the suffix after the game's base name, ignores the emulator label, and takes the newest file per suffix (`romm_saves.c:668-700, 787-791`).
- It has no `.sav` ↔ `.srm` mapping and no conversion. A Pocket save only reaches RetroArch if it arrives in `autosave` as `<rom stem>.srm` (plus `<rom stem>.rtc` for clock games), already in libretro layout. A `.sav` would download as `.sav` and be ignored. NGP uses `.flash` and Lynx `.eeprom`.
- With RetroArch's SaveRAM compression turned on (off by default), `.srm` files are RZIP-compressed.

## RomM's own save conversion (unreleased)

- RomM's master branch, after 5.4.1, gained save conversion through [argosy-sigil](https://github.com/rommapp) in PR #5198 (2026-11-14): `GET /api/saves/{id}/content?core=` returns a save converted for that core, for example `.srm` plus `.rtc` for gambatte (`test_save_conversion.py:473-481`). GBA is treated as native.
- sigil (commit 85f88a8) has no Pocket or MiSTer formats. Its `docs/platforms/gb.md` agrees with the MBC2 and clock facts above.
- If sigil learned the Pocket's formats, RomM could hand each client its own layout and the plugin would need no conversion code. That depends on an upstream change and a RomM release.

## Converters and reports

- [save-file-converter](https://github.com/euan-forrester/save-file-converter) has no Pocket support. Its MiSTer converters cover GB/GBC (raw, no clock trailer handling), GBA (strips appended clock data, pads 512 B EEPROM to 8 KB), NES, SNES, Genesis, SMS, Game Gear, PC Engine and WonderSwan; none for NGP or Lynx. Its WonderSwan converter expects MiSTer's 512-byte clock sector, so it would reject Pocket files (12 bytes). Its NES converter pads to 32 KB, not 256 KB.
- Its [issue #281](https://github.com/euan-forrester/save-file-converter/issues/281) (open, 2024) says Pocket saves are "very similar to the MiSTer" except that Genesis EEPROM saves aren't padded.
- mincer_ray's README recommends save-file-converter and mGBA's Save Converter, and ships a web clock tool that treats the timestamp as UTC seconds.
- budude2 issues #78 (users confused by the clock trailer) and #28 (64 KB saves, fixed in 1.0.7).
- Pocket Sync's MiSTer save sync copies bytes unchanged (`src-tauri/mister_saves_sync/src/lib.rs:144-198`) for NES, SNES, Genesis, SMS, Game Gear, PC Engine and WonderSwan.
- No reports were found of people moving saves between the Pocket and RetroArch.

## Unknowns

- Whether the Pocket's timestamp is UTC or local time. Local time would shift converted Game Boy clocks by the timezone offset.
- Byte order of budude2's clock trailer (little-endian inferred).
- Closed or unchecked cores: Spiritualized GB/GBC/GBA, BitBird.GBA, K3V.GBA, jotego's NGP cores, frazilpop.Genesis (released 2026-11-16).
- How the Pocket firmware names files for mincer_ray's `.rtc` slot, and whether mincer accepts a raw 512 B EEPROM save.
- MBC7 (budude2 512 B vs mGBA 256 B; gambatte has no MBC7) and the HuC3 clock packing.
- NES boards with large work RAM (MMC5) or EEPROM (Bandai), and what fills the rest of the 256 KB file.
- SNES enhancement chips (SA-1, SuperFX, SPC7110): sizes and clock data.
- Genesis games with 16-bit save RAM or I2C EEPROM; Game Gear games with 93C46 EEPROM.
- WonderSwan EEPROM byte order.
- That RomM's web player writes the same layout as desktop RetroArch (inferred).
