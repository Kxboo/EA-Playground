# CODEC_NOTES.md

Concise per-codec reference. For the full derivation history (including
dead ends and corrections), see `docs/SESSION_NOTES/`.

All codecs share:
- A "magic double" int-to-float trick: `bits = (0x43300000 << 32) | raw;
  float(reinterpret(bits) - 2^52)`, used to turn quantized integers into
  floats without a divide.
- A per-bone `0x30`-byte pose record in the output buffer: quaternion at
  `+0x10`, translation at `+0x20` (confirmed across `FnDeltaQFast`,
  `FnDeltaSingleQ`, `FnStatelessQ`, `FnDeltaF3`, `FnStatelessF3`, `FnDeltaF1`).

## Rotation codecs

### FnDeltaQFast (tag 0x12)
Block-based delta-accumulation quaternion codec. Header at `marker+0x04`
(sample_count u16), `+0x06` (channel_count u8), `+0x10` (shift u8). Each
block: one 6-byte reference quaternion per channel, then `(2^shift - 1)`
delta samples (3 bytes each) accumulated forward. Bone table: `marker+0x0C`,
u8 direct index.

### FnDeltaSingleQ (tag 0x13)
Same block/delta shape as QFast, single-axis Euler angle instead of a full
quaternion delta, converted via `sin/cos` half-angle composition
(`Euler -> quaternion`) only at bind-pose init; per-frame accumulation stays
in angle space, composed via Hamilton product with a cached bind quaternion.
**Its "secondary" track is NOT translation** — in all 9 real clips it's a
second, complementary `FnDeltaQFast` track covering the rest of the
skeleton's bones (0 bone overlap between the two tracks, confirmed
corpus-wide).

### FnStatelessQ (tag 0x16, whole-clip container)
Keyframe-indexed (not block-delta) quaternion codec. `map` = the container's
own `ClipBlock` header. `map+0x14`=keyframe_count, `+0x16`=channel_count,
`+0x18`=record table (8 bytes/record = 4×u16, one per x/y/z/w, each via the
shared bit trick, no lerp scale/bias). Supports linear interpolation between
two adjacent keyframes when the query time falls between them (confirmed via
fresh disassembly this project cycle — an earlier internal write-up
incorrectly claimed "no lerp"; that claim only held for the exact-keyframe/
last-frame edge case it happened to test). Bone table: `map+0x0C`, u8 direct
index — same simple convention as QFast, NOT the encoded scheme the
translation codecs use.

## Translation codecs

### FnDeltaF3 (tag 0x14) / FnDeltaF1 (tag 0x15)
Same block/delta shape as QFast, decoding a 3-component (F3) or 1-component
(F1, one axis per channel) vector instead of a quaternion. Per-channel basis
record (E/B/W0/W1 floats/u16s) computes ref_base/ref_scale/delta_base/
delta_scale; delta region accumulates forward.

### FnStatelessF3 (tag 0x17, nested inside the whole-clip container)
Keyframe-indexed, optionally-lerped 3-component vector codec, structurally
parallel to FnStatelessQ but with its own per-channel basis table
(0x20 bytes/channel, only the X/Y/Z scale fields at `+0x10/0x14/0x18` used
in the per-frame path) and NO bias term (pure `raw_trick * scale`).

## The shared bone/axis table encoding (F3, StatelessF3, F1 ONLY)

These three translation codecs share one big fixed, clip-independent table
whose raw u16 entries encode BOTH a bone index and a component slot:

```
raw_entry = 8 + bone_index * 12 + axis_offset
bone_index = (raw_entry - 8) // 12
axis       = (raw_entry - 8) % 12      # 0/1/2, meaningful for F1 only
```

This is NOT a generic "entry // 3" scheme — an earlier version of the
exporter used that formula and it was wrong (never actually range-checked;
produced bone indices past 200 on a 68-bone skeleton). The correct formula
above was derived by dumping raw table entries and finding the arithmetic
progression directly, then confirming `raw_entry * 4 == bone*0x30 + 0x20`
(the translation pose-record slot) across 1217/1218 real corpus entries.

`FnDeltaQFast`/`FnDeltaSingleQ`/`FnStatelessQ`'s own bone tables are
DIFFERENT — flat u8 arrays, direct index, no formula. Do not conflate the
two conventions.

## Which bones actually get translation

Across the full corpus, only 22 of 68 bones ever receive `FnDeltaF3`/
`FnStatelessF3` translation, and they're the same set for both codecs
(root, several facial bones — eyebrows/mouth/cheeks — plus `l_prop`/
`r_prop` weapon-attachment bones, `pelvis`, `spine1-3`). Arms and legs are
rotation-only. `FnDeltaF1` translates root only (single-axis root motion).
