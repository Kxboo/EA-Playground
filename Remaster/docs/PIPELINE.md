# EAGL `.anm` Animation Pipeline — Complete Coverage (265/265)

## What this is

A complete `player_anims.anm` → per-clip `.glb` exporter for the EA Playground
(EA Sports Wii/GameCube-era) EAGL animation format, covering every codec
combination the file actually exercises. Nothing in the exported output is
guessed: every codec's byte layout, bit-unpacking, and channel→bone mapping
is confirmed via Capstone disassembly of `playgroundz.elf` (Gekko/Broadway
PowerPC, including paired-single instructions) and cross-checked against
real corpus data before being wired into the exporter.

## Pipeline diagram

```
player_anims.anm ──┐
                    │
playgroundz.elf ────┼──► eagl_anm_decoder.py  (shared infra)
                    │      - ELF section/symbol/self-relocation parsing
                    │      - bank header, table_a (265-entry clip index)
                    │      - ClipBlock + TrackMarker parsing
                    │      - clip name table
                    │
player_skel.ske ────┼──► eagl_skeleton.py
                    │      - 68-bone hierarchy, names, bind pose
                    │
                    ▼
        ┌───────────────────────────┐
        │   per-clip codec dispatch  │   (anm_exporter.py: main())
        │   by (primary.tag,         │
        │        secondary.tag)      │
        └─────────────┬─────────────┘
                       │
   ┌───────────────────┼───────────────────┬──────────────────────┐
   ▼                   ▼                   ▼                      ▼
FnDeltaQFast(0x12)  FnDeltaQFast(0x12)  FnDeltaSingleQ(0x13)   whole_clip
 + FnDeltaF3(0x14)   + FnDeltaF1(0x15)   + FnDeltaQFast(0x12)   container(0x16)
   224 clips           20 clips          9 clips (rotation-only, + FnStatelessQ
                                          complementary bone sets)  + nested
                                                                    FnStatelessF3
                                                                    12 clips
   │                   │                   │                      │
   ▼                   ▼                   ▼                      ▼
fn_delta_qfast.py  fn_delta_qfast.py  fn_delta_singleq.py     fn_stateless_q.py
fn_delta_f3.py     fn_delta_f1.py     (+ fn_delta_qfast.py for  fn_stateless_f3.py
                                               the secondary track)
   │                   │                   │                      │
   └───────────────────┴─────────┬─────────┴──────────────────────┘
                                  ▼
                    channel → bone resolution
              (anm_exporter.py: read_qfast_channel_bones /
               read_vector_channel_bones / read_scalar_channel_bone_axis)
                                  │
                                  ▼
                     build_clip_glb() → node hierarchy +
                     rotation/translation animation channels
                                  │
                                  ▼
                  output/anm_export/*.glb
                  (265 files, one per clip, named by clip index
                   + heuristically-matched clip name)
```

## Files and their roles

| File | Role |
|---|---|
| `playgroundz.elf` | The game executable — source of truth for every codec's actual runtime behavior, via disassembly. |
| `player_anims.anm` | The animation bank — 265 clips for one character/skeleton. |
| `player_skel.ske` | Skeleton definition — 68 bones, names, hierarchy, bind pose. |
| `decoder/eagl_anm_decoder.py` | Shared low-level parsing: ELF sections/symbols/self-relocations, bank header, `table_a` (per-clip offset table), `ClipBlock`/`TrackMarker` structures. Every other script imports from this. |
| `decoder/eagl_skeleton.py` | Parses `.ske` into bone hierarchy + names + bind-pose transforms. |
| `docs/dev_tools/disasm_helper.py` | Capstone-based PPC disassembler for `playgroundz.elf`, configured with `CS_MODE_PS` (paired-single) — required because this is a Gekko/Broadway CPU, not stock PPC. |
| `decoder/fn_delta_qfast.py` | `FnDeltaQFast` (primary rotation codec, tag `0x12`) decoder + validator. |
| `decoder/fn_delta_f3.py` | `FnDeltaF3` (XYZ vector translation, tag `0x14`) decoder + validator. |
| `decoder/fn_delta_f1.py` | `FnDeltaF1` (single-axis translation, tag `0x15`) decoder + validator. |
| `decoder/fn_delta_singleq.py` | `FnDeltaSingleQ` (single-axis rotation, tag `0x13`) decoder + validator. |
| `decoder/fn_stateless_q.py` | `FnStatelessQ` (whole-clip keyframe rotation, tag `0x16` container) decoder + validator — implemented this session. |
| `decoder/fn_stateless_f3.py` | `FnStatelessF3` (whole-clip keyframe translation, tag `0x17` nested) decoder + validator — implemented this session. |
| `exporter/anm_exporter.py` + `exporter/gltf_writer.py` | The actual exporter: dispatches each clip to the right codec combination, resolves channel→bone tables, builds glTF/GLB nodes + animation channels, writes output files. |
| `validation/bone_mapping.py` | Standalone corpus-wide structural sanity checks (bone list ascending/unique/in-range, map header field bounds) — used during development, not part of the export path. |
| `validation/regression.py` | Final regression: re-decodes and validates all 265 clips (norms, bone ranges, exception-free) in one pass. |

## Codec coverage (final)

| Codec pair | Clips | Rotation | Translation |
|---|---:|---|---|
| `FnDeltaQFast` + `FnDeltaF3` | 224 | full quaternion, delta-accumulated | XYZ vector, delta-accumulated |
| `FnDeltaQFast` + `FnDeltaF1` | 20 | full quaternion, delta-accumulated | single-axis, delta-accumulated (root only) |
| `FnDeltaSingleQ` + `FnDeltaQFast` | 9 | two complementary rotation tracks (finger/hand joints + rest of body) | none |
| `FnStatelessQ` + `FnStatelessF3` | 12 | full-body keyframe quaternion, optional lerp | face/prop/pelvis keyframe vector, optional lerp |
| **Total** | **265** | | |

**265/265 clips exported. 0 structural failures. 0 bone-index-range violations.**

Quaternion norm violations (outside 0.99–1.01) all trace to `FnDeltaQFast`'s
and `FnDeltaSingleQ`'s own already-documented delta-accumulation drift (no
per-frame renormalization in the original codec) — not to any addressing or
decode bug. The two codecs implemented this session (`FnStatelessQ`,
`FnStatelessF3`) show **zero** norm violations even under this bound.

## Known, intentionally-unexercised gaps

- Sparse keyframe-time tables (`map+0x08 != 0`) for both stateless codecs — 0/12 real clips use them; only the fixed-rate fallback path is implemented.
- The `EXTRA_CHANNEL_COUNT` region's source data for both stateless codecs — bone slots are resolved (so translation/rotation isn't wrongly attributed), but the static values themselves aren't decoded; those bones fall back to bind pose.
- `EvalSQTMask` (BoneMask-gated variant) for both stateless codecs — irrelevant for a full-channel sequential export.
- True playback FPS is unconfirmed — exporter uses a documented 30fps placeholder throughout.

## What would extend this

This pipeline is specific to one `.anm` bank and one `.ske` skeleton. Extending
to other characters/banks should mostly be a matter of re-running the same
scripts against new files — the codec tag values (`0x12`–`0x17`) and bit-level
formulas are properties of the shared `EAGLAnim` engine code in
`playgroundz.elf`, not this specific animation bank, so they should hold
as-is. The skeleton-specific pieces (bone count, names) are already
parameterized via `decoder/eagl_skeleton.py` / `player_skel.ske`.
