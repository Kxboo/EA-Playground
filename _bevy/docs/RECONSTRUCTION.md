# Asset viewer to game reconstruction

The native viewer is the first executable in this Bevy workspace. It establishes a real rendering path for recovered assets, including skinned model animation. `OriginalAsset` stores source, decoder version and evidence level on the Bevy scene root. The current evidence level is AssetDerived.

There is no reconstructed game loop yet. Asset names, animation names and CSV references cannot establish the original behavior by themselves. Avoid implementing plausible replacement rules and labelling them 1:1.

For each future gameplay subsystem, retain:

1. The original executable function/address or data-table references and their hashes.
2. The decoded inputs, state, update order and outputs, with unresolved fields marked.
3. A deterministic Rust implementation that can run independently of rendering.
4. A recorded original-game input sequence and matching state/visual observations.
5. Comparisons of timing, movement, animation transitions and events before promoting it to GameplayCompared.

Suggested first vertical slice: one character in a small verified environment, with original input mapping, locomotion state transitions and animation selection. Recover simulation tick rate and coordinates before physics. Collision must come from decoded collision data; display triangles are insufficient evidence. Add minigame rules only after their state machines and constants are recovered.

All 836 distinct model files are now accounted for: 828 geometry exports and eight empty draw lists. Individual frontend geometry/UVs and the previously failing model families are decoded. The next priorities are APT timeline/mask composition, exact Wii shader/material behavior, animation timing/static channels, and Havok/VLT object graphs. Material identity, vertex colours, normals and wrapping have executable evidence; remaining shader effects still need reconstruction. The shared JSON/GLB boundary allows Python decoders to be replaced incrementally without rewriting the Bevy viewer or future game systems. See the current [developer handoff](../../docs/HANDOFF.md).


## Evidence table for the game slice

| Subsystem | Status | Evidence |
| --- | --- | --- |
| Executable identity | ExecutableDerived | `playgroundz.elf` (SHA-256 `5cef3efc...3e2c`) is byte-identical to the retail `playgroundz.dol`/`sys/main.dol` image (5,203,700 bytes compared by `tools/prove.py`). It carries about 19,000 named functions. |
| Player locomotion | ExecutableDerived | `LocalCharacterControl::Update` (0x802eeb28): dead zone, speed = stick magnitude x `CharacterState` max speed (5.0), camera-relative facing, idle snap after 100 ms, d-pad ramp of 150 ms, turn steps `gTurnRate`/`gTurnRateFast`/`gTurnRateDigital`. Implemented in `src/locomotion.rs`; numbers pinned by unit tests and regenerated from the ELF by `tools/extract_constants.py`. |
| Turn-step update rate | Unresolved | The binary applies turn steps once per call and does not scale them by dt; the call rate is assumed to be 60 Hz (`TICK_HZ`). |
| Analog stick path | Implemented, not wired | The gamepad feature is not enabled in this build, so only the keyboard (d-pad style) path runs. |
| Player start | DataDerived | `character_info/player` in `db/db.vlt` (start_location 12, 0, -53; start_direction 0, 0, 1). The database is the game's Attrib vault, decoded with a loader layout recovered from `Attrib::Vault` (0x802d9800) and verified by `hash64` golden vectors from the original machine code. 75 types, 32 classes, 908 collections. |
| Physics world | ExecutableDerived + DataDerived | Havok 4.6 class reflection (244 classes) recovered from the executable; `physics/*.hkx` decoded to rigid bodies and shapes (881 bodies, 21,207 triangles over 4 areas); gravity 9.81 from `hkWorldCinfo`. |
| Sky | ExecutableDerived + AssetDerived | `SkyDome` class (skydome.cpp): `LoadGeometry` 0x803c6578 loads `skybox.o`, `skybox_mountain_city_ring.o`, `skybox_clouds.o` from `placeables/*.viv`; `Draw` 0x803c6724 draws them in that order at the sky origin (`Initialise` 0x803c64d0, translation 0; `SetPos` is only called by the paper-airplane minigame), rotating only the clouds; `Update` 0x803c6694 advances the cloud angle by 0.012 rad/s. Drawn behind the world by a dedicated camera. The original GX render state (z-mode value 0xFE) is not reproduced. |
| Placeables | DataDerived (not yet spawned) | The `placeables` database class holds exact positions/orientations for the swingset, teeter-totter, gates, tetherball poles and RC tracks (25 collections). Not yet placed in the game slice. |
| Walk/run selection | ExecutableDerived | `CharacterMovement::Update` (0x802ed784): idle at speed 0, walk while speed <= `CharacterState`+0x1c (2.5) + 0.001, run above. |
| World layer list | DataDerived | `worldfilelist.csv` (5 areas x `-`/`-alpha`/`-fade`/`-alphafade`), bounds from `world.csv`, input events from `controls.csv`. Two variants have empty draw lists and are skipped, matching the model audit. |
| World geometry, textures, Alicia, clips | AssetDerived | Existing decoders; see `HANDOFF.md`. |
| Terrain collision | DataDerived | The original Havok collision meshes (above), used for standing (upward faces) and blocking (steep faces). Character-vs-world resolution is my own simple step/slide logic, not the original Havok character proxy. |
| Jump impulse, camera framing | Provisional | Jump speed is chosen to make the slice playable (no jump parameter was found in the executable or database). Camera distance/height use named ELF globals (`gHingeCameraDistance`, `gFrameCameraHeight`), but which camera the world uses is not established. |
| World curvature | DataDerived (fit) | The decoded display meshes are baked onto a sphere: display height = flat height - (R - sqrt(R^2 - r^2)) with R = 250 = `world.csv` RADIUS (fit against the collision data, 90th-percentile error 0.17 m). Collision and gameplay coordinates are flat, so the game applies the same drop to the player and camera when drawing. Whether the original applies it at runtime or the artists baked it is not established. |
| AI, minigames, conversations, Havok physics, audio/video, APT menu scripts, save data | Not reconstructed | Formats are recognised but behaviour is not recovered. |

`--selftest` (11 in-game checks), `--flow-test` (six mode hops) and `tools/prove.py` (17 checks) are the repeatable proofs. None of them is a comparison against the running original game, so nothing is promoted to `GameplayCompared`.


## Decoders reimplemented in Rust (with differential proofs)

| Component | Rust module | Verified against |
| --- | --- | --- |
| Archives (BIGF/BIG4/VIV, U8) and RefPack | `src/archive.rs` | SHA-256 of all 5,267 corpus records (`cargo test archive`) |
| GSH textures (SHPG, GX C4/C8/RGB565/RGB5A3/RGBA8/CMPR) | `src/gsh.rs` | Python decoder, pixel hash of all 3,574 images in 736 files |
| Model geometry (all shader families) | `src/model.rs` | Python reference `Remaster/src/model_unified.py`, vertex/triangle hash of all 12,451 primitives in 836 models |
| Materials, alpha, wrap, Bevy meshes | `src/assets.rs` | `world-low-all.o`: 162 materials, 161 textures, 0 warnings, 100,576 triangles (matches the recorded audit) |
| Attrib database (`db.vlt`/`db.bin`) | `src/vlt.rs` | `hash64` golden vectors from the original machine code; 908 collections / 7,683 attributes cross-checked against the Python loader |
| Havok 4.6 collision (`*.hkx`) | `src/havok.rs` | Python reference; identical bodies, triangles and vertex sums on four areas |
| Player locomotion | `src/locomotion.rs` | Constants generated from the ELF; unit tests |
| Skeletons (`.ske`) | `src/skeleton.rs` | Python reference: all 181 bones of 4 rigs (bind pose, world transforms); the file's own cached world rotations agree with the composed chain |
| Animation banks (`.anm`), 6 codecs + `sBoneMask` | `src/anim.rs` | Python reference: all 266 decodable clips (player 265 + RC car 1) match per bone and channel; the 2 prop banks the reference cannot decode fail in both.  `BONE_MASK` is read from the ELF by `tools/extract_constants.py` |
| Textures (`.tpl`) | `src/tpl.rs` | Python reference: all 199 distinct TPL files (199 images) match pixel for pixel |
| Localisation (`.loc`, `string.idx`) | `src/locale.rs` | Hash routine read from `TRCLocale::ComputeHash` (0x8024d500); 10 locale files decode; 74 keys taken from the executable's own strings resolve to sensible text |
| Conversations (`.con`) | `src/conversation.rs` | Loader layout read from `Conversation::Load`/`Root`/`Dialog`/`Response::Load`; 32 files parse with 0 trailing bytes; 376 of 429 node texts resolve in `eng_us.loc` (the rest are hashes with no string in this locale) |
| Marker sets (`.mkr`), RC checkpoint lanes (`.cpt`) | `src/placement.rs` | Layouts from `AssetManager::ProcessMarkerSet`/`ProcessCheckpointSet`; 49 + 10 files parse to exactly their length |
| Streamed audio (`.asf`, `.ast`): `SCHl` container + EA Layer 3 | `src/audio.rs`, `src/mp3.rs`, `src/mp3_tables.rs` (tables generated from the ELF by `tools/extract_audio_tables.py`) | Frame syntax read from `CEALayer3::GetSideInfo`/`DecodeHuffman`/`Decode`; the executable's own Huffman trees and band tables.  All 14 music tracks (2,674 s) decode with 409,582/409,582 channel-granules ending exactly on `part2_3_length`, sample counts equal the headers, output spectrum/seams behave like music (16 kHz encoder low-pass, harmonic bass partials, no seam at granule boundaries).  No reference decoder exists on this machine: proof is internal consistency, not sample equality |
| Playable character | `src/character.rs` | `alicia.o` (62 skinned parts, 2,877 triangles, same as the reference) + 68 joint entities + inverse bind poses + Bevy `AnimationClip`s; no glTF, no Python in game mode; self-test checks joint pose changes

The model decoder is schema-driven: every shader family is a struct of (count, pointer) fields whose offsets come from the executable, and PCode (`ProcessPCode`, complete opcode set from its jump table) gives the GX vertex attributes and display list.  This replaces the earlier layout-scoring heuristics and also fixes three defects those had: duplicated meshes (`playground-high`, `world-low-all`: 4,553 phantom triangles), a missed animated-rope primitive (`tetherball_pole`), and family-specific degenerate-triangle rules.

Still Python: TPL/APT/audio formats and the asset viewer's worker (its model/animation preview still goes through a glTF file produced by the Python exporter; game mode no longer does).

## Audio: what is and is not decoded

* Decoded (Rust, from the executable's own code and tables): EA Layer 3 (`SCHl` codec 0x17: 14 music tracks, ambience), EA-XA ADPCM (codec 0x0A: the other ambience streams and `.bnk` bank sounds) and EA MicroTalk speech (codec 0x04: the 8 `spchdat.viv` files, 32 kHz).
* MicroTalk (`src/utk.rs`, `audio::decode_utk`): `decodemut`/`filter`/`readsamples` ported instruction for instruction (single precision, fused multiply-adds) and cross-checked against the original PowerPC code run in `tools/ppc_emu2.py` (`tools/utk_oracle.py`).  Framing from `CMTBLKDec::Feed/Decode`: `SCDl` = `[u32 samples][u32 0][u8 header flag]` + chunks `[type 00|EE][byte-aligned 432-sample frame]`, EE chunks append `[u16 offset][u16 count][i16 x count]` splice samples.  The 15 header bits (reduced bandwidth, multipulse threshold 24, gains) are identical in all eight files.  Evidence: every block of every file decodes to exactly its declared sample count with type bytes 00/EE on every frame boundary and the block ending on its last frame; output is low-pass speech (first-difference/rms 0.2-0.63, white noise would be 1.41), rms ~3000, unclipped.  Bit-for-bit equality with the console's PCM is not checked (no reference audio on this machine).
* AEMS module banks (`aems/*.abk`, magic `ABKC`): header words 0x20/0x24 give an embedded `BNKb` sound bank (`AddModuleBank` 0x802764dc); 21 files, 17 with samples (the four `amb_*.abk` only hold the module graph).  Sound headers follow the bank table back to back (tags as in `SCHl`; tag 0x88 = data offset inside the bank, 0x8f = 33-byte codec entry).  452 sounds decode: 160 EA-XA and 292 codec 0x12 = GameCube/Wii DSP-ADPCM (8-byte frames of 14 samples, eight Q11 coefficient pairs from tag 0x8f; data spacing in the banks equals ceil(samples/14)*8 rounded to 32 bytes, 290/292 audible sounds are low-pass rather than noise-like).  Nineteen bank sounds use codec 0x17 (EA Layer 3) and are not wired up.
* Not decoded: the module-graph part of the `.abk` (the AEMS sound-event scripts: players, envelopes, random/table modules from `SNDAEMSI_update*`) and the codec-id to decoder mapping through `MIXI_initunpack16/mt/xa` (table at 0x805975e8); decoders are selected here by the header's codec tag.
