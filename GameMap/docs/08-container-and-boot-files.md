# 08 — Container and boot files: `.dol`, `.elf`, `opening.bnr`, the 16:9 strap `.tpl`, `disc.ini`

Evidence tags: **[C]** confirmed, **[D]** data read from the files, **[I]** inferred, **[U]** unresolved. These are the files supplied alongside the executable; none of them is committed here.

## `playgroundz.dol` and `playgroundz.elf` **[C]**

| | `playgroundz.elf` | `playgroundz.dol` |
|---|---:|---:|
| size | 7,856,344 B | 5,204,096 B |
| SHA-256 | `5cef3efc…3e2c` (matches `FINDINGS.md`) | `e0c83ebb…e50b` |
| symbols | 55,133 symbol-table entries (19,166 functions, 26,613 objects, 1,669 file names) | none |
| debug info | `.debug` 89 KB (EA Exposure/Lua only), `.line` 26 KB | none |
| loadable sections | 13 `PT_LOAD` segments (incl. 3 zero-fill) | 10 sections in use (2 text + 8 data) |

**Every DOL section is byte-identical to the corresponding ELF section** (10 of 10 initialised sections compared), and both share entry `0x80006124` and BSS `0x804f0f00`, `0x10afc8` bytes (DOL header BSS length `0x116f7c` covers `.bss`+small-bss). So the DOL adds nothing; use the ELF. **[C]**
`FINDINGS.md`'s note that `.dol` "has no code decompilation" is now moot: the ELF's symbols make the code mappable, see [01](01-executable-and-boot.md).

## `disc.ini` **[C]**

```
region=us
productcode=RPXE
parentallock=1
authserver=1
```

The game reads it at boot (`SetBootOptionsFromIni`, `0x803aee08`): `region` (compared with `eu`/`us`), `productcode` (→ global `productCode`), `parentallock` (→ `atoi`). Missing file/keys → `RET000000` / 0. **`authserver` is not referenced anywhere in the executable**
(no string, no code), so it is for the disc/loader tooling. `RPXE` is the game ID's title code (`R`=Wii, `PX`=title, `E`=USA). See [01](01-executable-and-boot.md).

## `opening.bnr` (Wii channel banner) **[C]**

246,904 bytes. Layout:

| offset | content |
|---|---|
| `0x00` | 64 zero bytes (no BNR header) |
| `0x40` | `IMET` header (`0x600` B, version 3): uncompressed sizes **icon 83,648 / banner 429,152 / sound 105,336** (all three match the decompressed files below), then ten language title slots of which seven carry the text **"EA Playground"** (UTF-16BE, 84-byte slots starting `0x5c`), MD5 in the last 16 bytes |
| `0x600` | **U8 archive** `meta/`: `banner.bin` (122,052 B), `icon.bin` (17,764 B), `sound.bin` (105,368 B), each wrapped as `IMD5` (32-byte header) + `LZ77` (type `0x10`, LZ10) |

Decompressed: `banner.bin` (429,152 B) is a U8 with `arc/anim/banner.brlan`, `arc/blyt/banner.brlyt`, `arc/timg/banner_832x314.tpl` (427,200 B; RGBA8-sized) and `banner_regular_bottom.tpl`; `icon.bin` (83,648 B) is a U8 with `icon.brlan`, `icon.brlyt`,
`Background_170x96.tpl`, `DiscChannelLogo.tpl`; `sound.bin` (105,336 B) is a **BNS** stream (`BNS ` header, 22,050 Hz). The `brlyt/brlan` files are the Nintendo layout formats already inventoried in `FINDINGS.md` (`.brlyt/.brlan`). The executable never references `opening.bnr` (a `banner.bin` string exists in SDK data at `0x804847c8` but no game code uses it; the save-file banner is `/data/saveicons/banner.tpl`), so the banner is disc metadata for the Wii menu and not gameplay-relevant; it is useful only as a small, complete example of the layout/animation formats. **[D]**

## `strap_16_9_853x480_english.tpl` **[C]**

A standard GX **TPL**: magic `0x0020AF30`, 1 image, image table at the *end* of the file (`0x191440`, entry `{header @0x0c, palette 0}`), image header `{height 480, width 853, format 6 (RGBA8), data @0x40, wrap/filter fields 0}`;
pixel data `0x40..0x191440` = `856 × 480 × 4` bytes (width padded to the 4-pixel tile). Decoded (GX RGBA8: 4×4 tiles, 32-byte AR block then 32-byte GB block per tile) it is the **Nintendo wrist-strap reminder**
("Reminder: Securely fasten the wrist strap as shown … put the wrist strap through the connector hook"), 853×480 with a binary alpha channel.
- The executable **does not reference this file**: there is no `strap_16_9…`, `853`, `16_9` or `.tpl`-per-language string; its own boot screen is built from `data/boot/strapwarn_{wide,standard}_<language>.gsh` (see [01](01-executable-and-boot.md)). The TPL is
  therefore a disc-side/loader asset for the same reminder in 16:9, not part of the game's own boot path. **[C]** (absence of references) / **[I]** (purpose).
- It confirms `FINDINGS.md`'s decode of `strapwarn_standard_english.gsh` as this same reminder (the "safety screen") — the GSH version is 640×480 PAL8; this one is the 16:9 RGBA8 original artwork.
