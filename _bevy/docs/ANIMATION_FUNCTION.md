# Native compound animation timing

The existing converter sampled every clip at 30 FPS. The original player bank
contains 253 compound clips with FPS attribute ID 1: 250 at 30 FPS, two at 24
and one at 37. Bevy curves now use those source attributes through
`anim::Bank::compound_function`, decoded `Clip::native_timing` and
`character::animation_clip`. Other codecs retain their existing 30-FPS
convention; their runtime timing is not newly verified by this change.

| Original routine | Address | Rust projection |
|---|---|---|
| FnCompoundChannel::UseFPS | 0x803fecac | animation_function::CompoundFunction::set_use_fps |
| FnCompoundChannel::GetLength | 0x803ffaf8 | CompoundFunction::length |
| FnCompoundChannel::GetAttributes | 0x803ffb54 | Bank::compound_function relocated map +4 |
| FnAnim::GetAttribute(byte) | 0x803fcdf8 | optional block passed to byte lookup |
| AttributeBlock::GetAttribute(byte) | 0x803fce60 | attribute_byte payload byte |
| AttributeBlock::GetAttribute(pointer) | 0x803fc424 | inline payload extraction |
| AttributeBlock::FindAttribute | 0x803fc478 | attribute_byte binary search |

The attribute block has a big-endian u32 entry count followed by sorted eight-byte
records: u16 ID at +0 and an inline four-byte payload at +4. The byte getter uses
the first byte of that payload. This is not a relocated pointer for byte values.
Compound map +0xa supplies the u16 sample count. UseFPS always stores the enable
flag; when enabling with a zero FPS cache it looks up attribute 1. Missing
attributes leave the cache unchanged. Nonzero cached FPS avoids further lookup.
GetLength returns the sample count or single-precision count/FPS.

`animation_function_oracle.py` executes all seven original bodies with no hooks.
The fixture includes executable/function code hashes, archive bank hash, all 253
original compound records and 96 synthetic cache, missing-block, empty/missing-ID,
zero-FPS and integer-limit cases. Five enable/disable steps run per case. For
enabled zero-FPS cases, only flag/cache effects are verified: the local interpreter
does not implement native divide-by-zero/FPSCR behavior, so no length result is
invented. Function construction/defaults and function evaluation remain separate.

The Rust differential test compares flag/cache stores and every supported length
result by bits. The original-bank integration test pins the bank hash, compares
all 253 map lengths/rates, preserves container durations independently of decoded child counts and verifies the resulting
Bevy curve durations. The unusual source rates are S_DB_Catch_Slide (24),
S_SC_BumpIdle (37), and S_SC_New_KickStart_Mirrored (24). Tetherball's existing
41 clips still have 30 FPS; per-clip source timing is included in asset diagnostics.

The Bevy curves are an integration check, not proof of native interpolation,
quaternion normalization, root motion, skinning or marker transforms. Native
AnimationState trim/duration arithmetic remains in `animation_playback.rs` and
is distinct from the converter's last sample timestamp.

Annotated original instruction listings are in `evidence/animation_function/`.
Regenerate/check them and native fixtures with
`py -3.14 _bevy/tools/animation_function_oracle.py --check`; run compiled Rust
comparisons with
`cargo test --manifest-path _bevy/Cargo.toml --release --offline --locked animation_function`.
Both checks are registered in `tools/prove.py`.

The integration test exposed a pre-existing sparse delta timeline gap: e.g. S_AI_Frustration has container length 27 but the decoder emits 22 child keys. Native EvalSQT at 0x803ff53c passes FPS-scaled time to child evaluators; FnDeltaQFast uses the relocated +8 sparse table. The converter now preserves the verified container duration and reports mismatched timelines as caveats. It does not claim the existing delta sparse poses are corrected. Recovering and comparing those child evaluators remains the next dependency.

Validation on 2026-09-30: native fixture/listing regeneration and check passed (349 cases); the full offline locked release Rust suite passed 118 tests. This includes all 253 compound clips reaching Bevy curves with source rates and container durations. Existing playback evidence regeneration/check also passed.

The release executable built successfully and `--prepare-tetherball` exited 0 with `main_world_assets_ready: true`, 41 installed clip entries and per-clip FPS 30. Diagnostics report native-container/decoded-child timeline mismatches for 40 of those 41 clips. This establishes a specific live-animation blocker; readiness here means owned assets exist, not that sparse poses are correct.
