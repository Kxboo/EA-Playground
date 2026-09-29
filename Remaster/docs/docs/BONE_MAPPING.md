# BONE_MAPPING.md

Two genuinely different bone-index conventions exist in this format.
Conflating them was an actual bug caught and fixed during development
(see `docs/CODEC_NOTES.md` and `docs/SESSION_NOTES/` for the full story) --
this document exists so it doesn't happen again.

## Convention A: flat u8 direct index

Used by: `FnDeltaQFast` (marker+0x0C), `FnDeltaSingleQ` (same, on its
primary marker), `FnStatelessQ` (map+0x0C).

```
bone_index = table[channel]     # table is a plain byte array
```

## Convention B: encoded u16, shared big table

Used by: `FnDeltaF3` (marker+0x04), `FnDeltaF1` (marker+0x04),
`FnStatelessF3` (map+0x0C).

```
raw = table[channel]            # u16
bone_index = (raw - 8) // 12
axis       = (raw - 8) % 12     # only meaningful for FnDeltaF1
```

The table itself, for the 12 whole-clip containers' `FnStatelessF3` tracks,
is a FIXED, clip-independent 68-entry array (`8, 20, 32, ... 812`) from
which each clip selects a `channel_count`-sized prefix for keyframe-animated
bones and an `extra_count`-sized remainder for static (bind-pose-held)
bones. This was discovered by dumping raw entries across multiple clips and
noticing the arithmetic progression -- not by reading it out of the ELF
directly (the *formula* was disassembly-confirmed via the pose-write
address math; the *fixed table contents* were an empirical corpus
observation).

## How to tell which convention applies

By codec, not by "does it look like a small integer." Bone index 2 is
valid under BOTH conventions and will silently produce a wrong-but-
plausible result if you use the wrong formula -- this is exactly what
happened when `anm_exporter.py`'s original `read_axis_channel_bones()`
used `entry // 3` for `FnDeltaF3`/`FnDeltaF1` (a leftover assumption never
actually range-checked). Always validate: every resolved bone index must
satisfy `0 <= bone < 68`, and ideally you should also check it against
`player_skel.ske`'s bone names for a sanity read (facial/prop bones for
the translation codecs' actual corpus; full-body coverage for the
rotation codecs).

## Cross-codec confirmation

For the 12 whole-clip containers, `FnStatelessQ` (Convention A) and
`FnStatelessF3` (Convention B) both resolve against the SAME 68-bone
skeleton, and their respective animated-bone sets are disjoint in role
(full-body rotation vs. face/prop/pelvis translation) but overlap
partially in bone identity (e.g. bone 0 / Root appears in both, as
expected for a bone that both rotates and translates).
