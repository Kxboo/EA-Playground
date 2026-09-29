# FILE_FORMAT.md

## `.anm` (animation bank)

```
ELF-like container
 ├─ sections (via _read_sections)
 ├─ symbol table (via _read_symbols) -- used to locate the bank + name table
 ├─ self-relocation table (via _self_reloc_map) -- EVERY internal pointer
 │   in this format is a self-relocated offset, resolved through this map,
 │   not a raw file offset
 │
 ├─ bank header (_parse_bank_header)
 │   └─ field1: clip count (265 for player_anims.anm)
 │
 ├─ table_a: per-clip pointer table (_read_table_a), 265 entries
 │
 └─ per clip: ClipBlock (_parse_clip_block)
      ├─ +0x00: container tag (0x16 for whole-clip FnStatelessQ containers,
      │         else a regular tag from the primary marker)
      ├─ +0x04: bone list pointer (ascending u16 array, header-prefixed)
      ├─ +0x0C: primary TrackMarker pointer (rotation codec)
      └─ +0x10: secondary TrackMarker pointer (translation codec, OR a
                second rotation track for FnDeltaSingleQ clips, OR the
                nested FnStatelessF3 object for whole-clip containers)
```

### TrackMarker (a.k.a. "map" for delta-block codecs)

```
+0x00: 0x00 (pad)
+0x01: tag (0x12=QFast, 0x13=SingleQ, 0x14=F3, 0x15=F1, 0x16=StatelessQ
        container, 0x17=StatelessF3)
+0x02: family (u16)
+0x04 or +0x08/+0x0C: codec-specific fields -- SEE docs/CODEC_NOTES.md,
  offsets differ per codec and are NOT interchangeable despite superficial
  structural similarity.
```

For whole-clip containers (tag 0x16), the `ClipBlock` header itself doubles
as the `FnStatelessQ` object's own "map" (`this+0x0C` in the disassembled
class == the ClipBlock's own address) -- there's no separate allocation.

## `.ske` (skeleton)

```
16-byte header (NOT 32, an early-session misreading corrected once real
  bone counts were cross-checked)
per-bone table, 112 bytes/bone, 68 bones for player_skel.ske:
  name resolved via `__Bone:::Root.<name>` ELF symbols in playgroundz.elf
    (only meaningful when parsing alongside the matching ELF; the .ske
    file alone has no embedded name strings)
  local translation / rotation / scale
  parent index (-1 for root)
```

No `.rel.data` section in `.ske` files -- expected, since there's nothing
to self-relocate (no internal cross-references, unlike `.anm`).

## Shared pose-record convention (both file formats meet here)

Every rotation/translation codec writes into a per-bone 0x30-byte record in
the runtime pose buffer:
```
bone*0x30 + 0x10 : quaternion (4 floats, x/y/z/w)
bone*0x30 + 0x20 : translation (3 floats, x/y/z)
```
This is inferred from disassembly of every codec's `EvalSQT` output-write
instructions, not asserted from a single source -- it's the one structural
fact tying all six codecs together.
