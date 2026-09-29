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
| World layer list | DataDerived | `worldfilelist.csv` (5 areas x `-`/`-alpha`/`-fade`/`-alphafade`), bounds from `world.csv`, input events from `controls.csv`. Two variants have empty draw lists and are skipped, matching the model audit. |
| World geometry, textures, Alicia, clips | AssetDerived | Existing decoders; see `HANDOFF.md`. |
| Terrain collision | Provisional | Walkable triangles from the display meshes (129,128 triangles); the original uses Havok data (`physics/*.hkx`, not decoded). |
| Jump, gravity, camera framing, spawn | Provisional | Chosen to make the slice playable. Camera distance/height use named ELF globals (`gHingeCameraDistance`, `gFrameCameraHeight`), but which camera the world uses is not established. |
| World shape | Open | `world-low-all.o` is a curved, whole-world level-of-detail mesh; no bend/curvature symbol exists in the executable. |
| AI, minigames, conversations, Havok physics, audio/video, APT menu scripts, save data | Not reconstructed | Formats are recognised but behaviour is not recovered. |

`--selftest` (11 in-game checks), `--flow-test` (six mode hops) and `tools/prove.py` (17 checks) are the repeatable proofs. None of them is a comparison against the running original game, so nothing is promoted to `GameplayCompared`.
