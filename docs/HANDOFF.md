Tetherball activity asset preparation now loads all six original archive members through the existing Rust pipeline and installs an owned Bevy resource. Original-data lifecycle checks cover readiness, restart, missing resources, lost image handles and release while preserving unrelated assets. The ball has 192 visible triangles, the rope 60, and all three texture images resolve with zero warnings. Shadow geometry is retained; native volume composition and the live activity scene remain unimplemented. See _bevy/docs/TETHERBALL_ASSETS.md.

All generic tetherball startup stages are now removed. Live SetUpServer queries animation markers after state requests and composes native Grab; selection/camera rules share the existing reset port. Native checks include 64 additional live server transitions and the 24 complete enclosing cases. Production Bevy host/readiness, live restart/exit and playable outcomes remain required. See `_bevy/docs/TETHERBALL_STARTUP.md`.

Area selection and pregame entry now compose directly in tetherball startup. SetArea/OpenPregame stages are removed; the shared helper preserves signed count semantics and the native ready byte, and area conversion preserves raw abstract/physical distinctions. Standalone and enclosing native checks cover these paths. SetUpServer is the last generic startup stage, followed by the concrete Bevy host/readiness/interactive loop. See `_bevy/docs/TETHERBALL_STARTUP.md`.

AI startup now composes derived AI construction and tuning through the existing live owners; the Ai stage is removed. Native enclosing cases execute constructor/Initialize/collection selection and cover active-game absence plus enabled/disabled configuration. Existing corpus callers share the service-based initializer. Remaining stages are SetArea, OpenPregame and SetUpServer, followed by the concrete Bevy host. See `_bevy/docs/TETHERBALL_STARTUP.md`.

Tetherball startup now directly composes ball resource initialization. The Ball stage is removed; StartupState retains all cached/shadow handles and asset IDs for cleanup. Native enclosing cases cover all shadow null combinations and the late live pole-height read. Remaining startup stages are area, AI tuning, pregame and server setup; the Bevy host remains required. See `_bevy/docs/TETHERBALL_STARTUP.md`.

Enclosing tetherball startup now composes all three recovered character append helpers directly; the Character stage is removed. The 24 native enclosing cases cover absent, reused and mismatching first characters and compare ownership/count/controller effects and AI ball bindings. Cleanup and constructor fixtures have been regenerated; `_bevy/docs/TETHERBALL_STARTUP.md` records the remaining engine/stage dependencies.

Session setup now has a shared constructor/configure/startup entry in `_bevy/src/tetherball_session.rs`. Five original setters and the team copy are compared across 48 dirty native memory images; typed fields reuse existing owners. See `_bevy/docs/TETHERBALL_SESSION.md`. Child startup service composition and the interactive Bevy host remain the next dependencies.

# Developer handoff

## Latest continuation (2026-09-30)

Current working changes now include exact MGTetherball/Minigame/World constructor stores and an in-place Runtime projection, verified against 32 full 0x450-byte native images and 24 typed projections. The original allocation size is pinned to its caller instruction. Tunables +170/+180 are now independent of live count/cap +438/+430. See `_bevy/docs/TETHERBALL_CONSTRUCTOR.md`; the next factory dependency is native session setter/team-copy ordering, already traced but not yet composed.

Current working changes compose startup game logic, ball construction and base initialization, and add the enclosing derived teardown in `_bevy/src/tetherball_cleanup.rs`. The cleanup oracle runs original base/ball teardown and the ball destructor across 64 scenarios, including signed negative +210 counts. See `_bevy/docs/TETHERBALL_CLEANUP.md` for precise service boundaries. Interactive tetherball remains unconnected; constructor/session recovery, child startup composition and a Bevy host remain necessary. Keep the full reconstruction objective active; no gameplay-completion claim is implied by these fixture checks.

`feat/menu-and-game-proof` is merged into `main` alongside the executable GameMap research. The playable slice now uses `GameState::Update`'s variable integer-millisecond frame policy instead of an assumed fixed 60 Hz loop. `sim_time.rs` also ports Havok step scheduling without claiming Havok dynamics. `tools/timing_oracle.py --check` executes the original PowerPC routines for 160 frame and 252 physics cases; constants regenerate from the pinned ELF. The clock divisor address is absolute `0x800000fc` (corrected in GameMap), with the console's runtime value still uncaptured.

`multiplayer.rs` ports tournament points/wins/ranks as pure rules, verified against 210 original-code transitions and queries. This includes ordered tie handling and the original placement-slot bug; see [multiplayer evidence](../_bevy/docs/MULTIPLAYER.md). Playable minigames remain future work.

`controller.rs` ports button edges/timers, all eight event predicates, modifiers and ordered context transitions. The Bevy slice loads `controls.csv` through `control_bindings.rs` and dispatches after world update using uncapped input time. Oracles cover 303 controller frames and all 506 bindings in 14 original CSV files. See [controller evidence](../_bevy/docs/CONTROLLER.md). Wii device input, rumble and Apt frontend dispatch remain separate work. The original playground jump command is inert; the provisional Bevy impulse was removed after original-machine differential checks and confirmation that PhysicsDynamicCharacter hardcodes wantJump=false.

`tetherball.rs` ports Hit, rmAngle::Wrap, SpinDownPole and the numerical prefix of Update (2,416 transitions, eight angle cases, 128 scores). `tetherball_scene.rs` now completes original Update: grabbed attachment feedback into height/radius, ball/rope matrices, shadows and ordered particle requests. `area_transform.rs` ports the original AreaManager matrix and EA matrix arithmetic. A further 72 area matrices and 360 complete Update transitions compare every state/matrix/effect field by bits. See [scene evidence](../_bevy/docs/TETHERBALL_SCENE.md). This is not yet a playable minigame or complete particle renderer.

`tetherball_match.rs` retains its 900-case winner-decision proof. `tetherball_lifecycle.rs` now consumes those effects synchronously and ports complete ChangeGameState entry, distance/camera decisions, win-animation selection, round-end facing/results, and outer Update orchestration. Its 1,280 original calls compare state, float bits, ordered service requests and the full 0x118-byte post-game payload. UI, animation, AI takeover and delegated handlers remain explicit boundaries. See [lifecycle evidence](../_bevy/docs/TETHERBALL_LIFECYCLE.md). The old local `_bevy/logs/tetherball_match_extended_draft.rs` remains unverified; do not adopt it.

`tetherball_tuning.rs` reads the original inherited VLT values, including short child arrays returning zero rather than falling back to parent elements. The selector uses base +0x40 (the human-player count retained as `session_mode`), not tetherball character count +0x210. Speeds/angles always come from regular tunables; game/AI fields use the dare-selected collection. The oracle covers 75 selectors, 9 difficulty conversions, 40 tuning records, 3 multiplayer copies and 12 height cases. See [tuning evidence](../_bevy/docs/TETHERBALL_TUNING.md).

`tetherball_reset.rs` ports ResetRound, ResetMiniGame/ResetStats and SetUpServer bodies against 72 original calls. It updates the shared lifecycle and ball directly; ResetState owns additional fields and object handles. Exact snapshots cover scoring weights, camera/AI stores, marker-selected attachments, both FX sentinels and ordered effects. The new reset composition replaces supplied InitializePlayerAnimations values with the decoded body; InitializeAptHud requests do not set HUD readiness. See [reset evidence](../_bevy/docs/TETHERBALL_RESET.md). Preserve base count +0x70 separately from tetherball count +0x210, and initial pair +0x21c/+0x220 separately from active pair +0x214/+0x218.

`tetherball_gestures.rs` ports callback capture, ordered queue consumption and active-player hit-attempt sampling, with 112 original-code transitions. Its adapters use the shared lifecycle controller/action fields. `reset_pending_gestures` rewrites live records to kind 2/zero/controller 0 while preserving count and attempt marker, matching ResetRound's stores. The original attempt sampler ignores its nominal player argument and reads +0x214. Controller sampling still does not invoke a ball hit; preserve the handler's range, animation and charge gates. See [gesture evidence](../_bevy/docs/TETHERBALL_GESTURES.md).

`character_input.rs` ports PhysicsDynamicCharacter::BuildCharacterInput with explicit support/proxy boundaries (182 exact cases, 24 ApplyVelocity calls). `character_movement.rs` now ports calculateMovement and Grounded/InAir state logic: 262 exact utility/context vectors under the documented frsqrte instruction model. The world persists prior XYZ velocity/state, supplies decoded support normals, skips nonpositive physics deltas and consumes original airborne gravity without a second accumulation. Collision/step/slide, support queries and desktop yaw remain host boundaries. See [movement evidence](../_bevy/docs/CHARACTER_MOVEMENT.md). Ground→Air and Air→Ground transitions intentionally defer destination arithmetic by one frame.

The game replaces fitted world drop with original `AreaManager::CalcRenderingModelMatrix` (0x803d78b8). Props/player use area * local-yaw in Bevy column convention, matching original PlaceableManager/Character composition. Static area geometry keeps its loaded coordinates; original static draws do not call this warp. Camera points use the same coordinate mapping with provisional host orbit framing.

`tetherball_serve.rs` adds the complete UpdateServe body and its SetActiveCharacter, rumble-wrapper and OpenPauseMenu behavior. The original oracle executes the nested ball predicates/Serve/Toss, rmAngle constructor/IsBetween, state entry/winner decisions and pause payload setup without gameplay stubs. Its 672 serve cases and 21 raw selector cases visit all 509 UpdateServe instructions and both outcomes of all 45 conditional branches, including 18 nested state-30 transitions. See [serve evidence](../_bevy/docs/TETHERBALL_SERVE.md). Shared Lifecycle/ResetState/GestureState/BallMotion remain authoritative; ServeState owns only additional fields. The reset snapshot's marker is a preserved shadow; the live gesture marker is authoritative during serving.

`tetherball_rally.rs` now reconstructs complete Return and Accelerate, composing the new hit, hit-animation and rally-rule modules with existing shared state. The combined oracle executes 2,086 original calls, covers every instruction of the two handlers plus HitTetherball/IncrementMultiplier/DrawHitIndicatorParticle (1,489 total), and both outcomes of all 117 conditional branches. Five Rust suites compare full state and ordered effects; helper rules have a separate 511-call corpus and animations/ranges have 1,663 vectors. The hit-window orientation is player != focus, the waiting player's mega gate is +42e, and IncrementMultiplier's explicit player selects the multiplier while +214 selects character feedback. The oracle handles engine tail calls as well as linked calls. See [rally evidence](../_bevy/docs/TETHERBALL_RALLY.md). Stationary indicator cases preserve positive/negative zero; unsupported 0/0 and arbitrary FPSCR/NaN behavior remain explicit boundaries.


`tetherball_ai.rs` now ports AI entity initialization, position/swing predicates and complete compulsion construction using live Lifecycle/ServeState/RallyRuleState owners. Its oracle covers 726 original calls, including 16 corpus-backed hit-table loads; the two compulsion modules separately recover movement, hit planning and attempt writes. `tetherball_ai_reset.rs` replaces ResetRound's AI initialization/store dependencies (24 complete native resets with original constructors/Initialize). `tetherball_animation_init.rs` initializes all 13 animation tables (48 native cases), and `tetherball_reset_runtime.rs` composes it into round and full resets (576 native calls). See [AI evidence](../_bevy/docs/TETHERBALL_AI.md), [move evidence](../_bevy/docs/TETHERBALL_AI_MOVE.md), [hit evidence](../_bevy/docs/TETHERBALL_AI_HIT.md), and [reset composition](../_bevy/docs/TETHERBALL_RESET_RUNTIME.md). Full AI scheduling and a playable minigame remain unimplemented.

`tetherball_runtime.rs` now composes the full nine-state frame dispatcher with live gestures, complete ball scene updates, serve/rally logic, shared frontend state and reset/AI/animation adapters. All 180 original complete-frame comparisons pass, including fresh particle positions, pause ordering, integer exit status 2, and the base pause-counter decrement controlled by the separate World pause byte. `tetherball_frontend.rs` adds seven frontend handlers/callbacks with 112 native cases. Reset animation composition covers 576 round/full resets. See [runtime evidence](../_bevy/docs/TETHERBALL_RUNTIME.md) and [frontend evidence](../_bevy/docs/TETHERBALL_FRONTEND.md). The runtime requires decoded startup and host services; it is not connected to the interactive Bevy game yet.

The runtime reset integration adds 24 combined original calls: live gesture/trail/rule owners override deliberately stale legacy reset snapshots; native AI/animation/HUD initialization and engine effect order agree. `tetherball_initialize.rs` recovers the bounded InitGameLogicState routine, including the signed-byte handicap divided by the **prior** rotation cap. `tetherball_player_init.rs` recovers InitializePlayer reuse/spawn and ordered setup; the new `tetherball_ai_init.rs` and `tetherball_additional_player.rs` recover the other two spawning paths against native fixtures. The enclosing top-level Initialize body is now ported with remaining stage boundaries documented below. Base +0x40 is incremented by human-player initialization and retains the existing `session_mode` field name; +0x210 counts the tetherball characters. See [logic initialization](../_bevy/docs/TETHERBALL_INITIALIZE.md) and [player initialization](../_bevy/docs/TETHERBALL_PLAYER_INIT.md).

`tetherball_ball_init.rs` recovers the complete 912-byte ball Initialize: original asset names, four cached models, two shadow entities, ordered regular tuning reads with raw difficulty, wrapped heading and initial height. Its 48 original-machine cases compare all represented motion/scene fields and resource/effect order. See [ball startup evidence](../_bevy/docs/TETHERBALL_BALL_INITIALIZE.md). It accepts existing constructor state; it does not invent a game-ready default.

Fresh top-level startup audit: MGTetherball's constructor sets the prior rotation cap to 6 before InitGameLogicState divides by it. Supported world variants set heading to zero, derive world origin from the selected pole placeable, and query ground height with range 50 before adding 2.2 for the independent ball anchor. The top-level initializer ends with OpenPreGameScreen(kind 2), ChangeGameState(1), then SetUpServer; substituting Runtime::reset changes its call graph. The enclosing world/asset orchestration now has a native oracle; full constructor/base/session and child-helper composition remain necessary before wiring authentic interactive startup.

`tetherball_startup.rs` ports the complete enclosing Initialize body across 24 native cases. It directly composes player-animation initialization, distance selection, state-1 entry and the new shadow setup port. Other existing helper bodies are explicit stage boundaries, so this is not yet proof of the full composed startup graph or an interactive minigame. `tetherball_shadow_setup.rs` recovers all 13 consumed option floats and mode 1; its 24 native cases execute the original static initializer. See [startup evidence](../_bevy/docs/TETHERBALL_STARTUP.md). No game-constructor implementation was completed before subagents were stopped; do not infer one from their planning messages.

Next bounded gameplay work: finish constructor/base/session and child-helper startup composition, connect the recovered tetherball runtime into a Bevy host, and replace AI scheduling/movement-application boundaries. Intro/resetting and all other frame handlers are recovered; avoid re-decoding them. The host still needs animation/particle/device adapters and startup/asset plumbing. Full Havok proxy integration remains unported. The user has now instructed **no more subagents**. All three helpers were interrupted; continue without spawning or resuming agents. The user asked to stop updating the progress page; leave GameMap/site and its progress manifest alone.

`corpus.rs` adds `--decode-all`, a native Rust decode of all 5,267 records (3,685 distinct) in about 85 s. It adds decoders for APT, NW4R, AEMS (with a PowerPC disassembler checked against Capstone with 0 disagreements), EA fonts, SPCH, Csis, Wii system files and VP6 video with its EA-XA audio (GSTR headers default to codec 0x0a per `SNDI_patchtohdrgen`). The pass also corrects animation, Havok and GSH palette handling. Only the unlinked ATOK/Zi dictionaries stay partial. See [native decode](../_bevy/docs/NATIVE_DECODE.md).

`audio.rs` now decodes all 471 embedded bank sounds: 160 EA-XA, 292 DSP-ADPCM and 19 EA Layer 3, including the 12 MPEG-2 sounds in `world_sfx.abk`. `mp3_lsf_oracle.py` checks all 512 LSF compression values and spectral scaling/reorder for MPEG-1/2 against original PowerPC. Complete console PCM equality remains unverified. See [bank evidence](../_bevy/docs/BANK_AUDIO.md).

Latest verified build: **110/110** proof checks, **22/22** rendered checks and **6/6** mode transitions. The packaged and release executables match SHA-256 `d051d99121981f14300dce078b1e221d47b7ed0a3febf712795d99e9bee62543`.

From `_bevy`, run `py -3.14 tools/prove.py --with-tests` for corpus checks plus all registered original-code oracles. After building, run `target/release/EAGL-Workbench.exe --selftest docs/selftest` and `--flow-test --mode menu` for rendered integration. Current boundaries are in [RECONSTRUCTION.md](../_bevy/docs/RECONSTRUCTION.md); the older material-import baseline below remains useful for asset provenance.

Baseline: material decoding pass, decoder **`native-5-materials-4`**, recorded 2026-09-29. Start with the root README, [setup](SETUP.md), and [current findings](../Remaster/research/FINDINGS.md). This is an initial source import into Git; earlier development history survives through notes and provenance, not earlier Git commits.

## Architecture and ownership

| Area | Entry points |
| --- | --- |
| Bevy viewer and material/animation UI | `_bevy/src/main.rs`, `viewer.rs` |
| Worker lifecycle, source/package selection, JSON requests | `_bevy/src/bridge.rs` |
| Future game asset metadata boundary | `_bevy/src/game.rs` |
| Persistent worker, catalog, cached previews | `_bevy/tools/decoder_bridge.py` |
| Shared decode/export integration | `Remaster/src/core.py` |
| Archive traversal, deep inspection and exports | `Remaster/src/research.py`, `archives.py`, `containers.py` |
| Exact material ownership and wrap/colour/normal handling | `Remaster/src/material_bindings.py` |
| Containment-based texture dependencies | `Remaster/src/asset_links.py` |
| ELF-derived model families | `frontend_models.py`, `hwskin_models.py`, `shadow_models.py`, `pcode.py` |
| Recovered model/image/skeleton/animation implementations | `Remaster/src/legacy/` |

The native viewer communicates with one persistent Python worker over JSON lines. The worker reads original files and writes derived GLB/PNG/report previews under `_bevy/assets/generated`. Cache keys include decoder version, request and input content. Increment `VERSION` after semantic decoding changes to invalidate stale previews. Keep both projects adjacent; avoid introducing separate UI-only parsers.

## Material fixes in this baseline

- Read shader-specific named texture fields using ELF-derived schemas instead of guessing from nearby relocation names.
- Bind each mesh through its PCode interval or exact source position/UV array identities when a legacy descriptor begins early.
- Treat textureless shadow shaders as intentionally textureless. Do not silence genuine missing/ambiguous bindings.
- Search containing archives and sibling banks, including shared world banks. Prefer exact full names; only use short aliases for entries without a full name.
- Collapse duplicate names only when decoded dimensions and pixel hashes agree. Preserve different-image conflicts in diagnostics.
- Preserve world RGBA8 vertex colours and their seams, authored unlit state, and TAR wrap modes. NPOT axes clamp according to executable evidence.
- Decode PlaygroundToonShade normals as S16 Q14; the old parser read these as bytes.
- Show binding counts, provenance and detailed warnings in the native viewer/headless reports.

The net fixture catches a real ownership error: its net mesh previously received the handle's material. The buggy catches the normal/lighting correction. The world catches shared banks, 161 texture bindings, vertex colours and wrap state.

## Verification baseline

| Check | Recorded result | Evidence |
| --- | --- | --- |
| Source regression suite | 32 passed | Tests under `Remaster/tests`; `_bevy/docs/build-verification.json` |
| Full model export audit | 828 geometry exports, 8 empty, 0 failures | [model-verification.json](../Remaster/research/model-verification.json) |
| Material resolution | 3,480 bindings, 0 warning models (previously 80) | [material-summary.json](../Remaster/research/material-summary.json), [detailed provenance](../Remaster/research/material-verification.json) |
| Native packaged headless checks | 17 passed | [packaged-verification.json](../_bevy/docs/packaged-verification.json) |
| Standalone packaged CLI | 12 passed | [cli-verification.json](../Remaster/docs/cli-verification.json) |
| Final material renderer checks | 5 passed | [material-render-verification.json](../_bevy/docs/material-render-verification.json) |

The five final renderer fixtures are net, buggy, world, Alicia and frontend. Their existing PNGs and adjacent `.png.json` diagnostics are in [_bevy/docs/captures](../_bevy/docs/captures/). The root README embeds those actual captures and the earlier animated-character capture. Automated scene checks plus visual review establish useful previews, not game-exact pixels.

Reports retain original machine paths and asset hashes for provenance. Some older inventory summaries and `Remaster/docs/docs/SESSION_NOTES` predate fixes; use `FINDINGS.md`, current model/material reports and the final renderer report for current status. The full texture audit covers 3,574 images in 736 distinct GSH files; model/frontend coverage includes 619 frontend files and 2,361 named shapes.

## Executable evidence

Private input: `Remaster/reference/playgroundz.elf`, SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

- `RuntimeAllocTARConstructor` at `0x803e8c48`: wrap properties 22/23 and NPOT clamp behavior.
- `ModelRenderPlaygroundTexture` at `0x80016dcc`: RGBA8 attributes and disabled lighting.
- `ModelRenderPlaygroundToonShade`: S16 Q14 normal setup at `0x8001ba2c`.
- Research disassemblies: `material-state-disassembly.txt`, `material-shaders-disassembly.txt`, `vertex-color-shaders-disassembly.txt`; shader field schemas: `shader-schemas.json`.

`Remaster/research/elf_trace.py` regenerates symbol-aware disassembly using optional Capstone. The binary itself is not versioned. Keep inferred behavior explicitly distinguished from confirmed calls, fields and state.

## Next decoding priorities

1. Recover GX/TEV material state: exact alpha compare/blend, depth/culling, texture filtering/LOD, sphere mapping/specular and dynamic shadow composition. Preserve current identity/provenance tests while adding evidence-specific fixtures.
2. Recover APT timelines, transforms, masks and composition. Individual frontend shapes already export; the previous note about 14 failed models is obsolete.
3. Resolve sparse animation timing, static extra channels and original update rate. Current playback assumes 30 fps; 265 parsed player clips do not prove timing equivalence.
4. Decode Havok/VLT object graphs, collision and remaining recognized-but-uninterpreted formats. Display triangles alone are not collision evidence.
5. Continue the gameplay slice from its recovered input, timing, locomotion and pure minigame-rule ports. Complete state transitions and physics remain unfinished; see [RECONSTRUCTION.md](../_bevy/docs/RECONSTRUCTION.md).

## Working conventions

Read originals without modifying them. Keep unsupported fields and ambiguity visible. Use new export destinations instead of overwriting previous outputs. Validate a decoder correction against the smallest representative fixture, then the affected corpus. Use headless preview/inspect for diagnostics and a real Bevy render for visual changes. Refresh packaged workers before testing executables: the packaged worker takes precedence over source.

Do not commit original DATA/ELF, extracted models/textures, local references, dependency installations, build outputs or caches. The `.gitignore` permits the two maintained projects and repository docs, excluding the older extraction workspace. Screenshots are intentionally included at the user's request. Keep the original provenance records; no blanket license has been assigned to recovered code by this import.
