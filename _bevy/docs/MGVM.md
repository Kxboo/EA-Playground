# Running the original minigame code (PowerPC VM)

Instead of re-implementing every minigame by hand, `src/gekko/` interprets the pinned `playgroundz.elf`
(`Remaster/reference/playgroundz.elf`) and `src/mgvm/` hosts it: the game's own code (WorldMan, PlaygroundWorld,
CharacterManager, the Attrib database, AnimationState / EAGLAnim, AI, MGDodgeball, ...) executes natively on guest
objects in emulated memory, and only engine *services* are bound to Rust hooks.

* `gekko/cpu.rs` – 750CL interpreter (integer, FP, paired-single spills / merges, quantized loads).
* `gekko/mod.rs` – VM: symbol-addressed hooks, "trap unless native" policy, nested guest calls, bump allocator.
* `mgvm/mod.rs` – which classes are native / service / null-stub (`runs_natively`, `is_soft_stub`).
* `mgvm/vfs.rs` – `FILE_*` / `FILESYS_*` on the DATA tree plus mounted `.big` / `.viv` archives.
* `mgvm/hooks.rs` – allocator, assets (`ResolveModel`), front-end singleton + Locale, null services.
* `mgvm/world.rs` – boot: static constructors, `InitAllModules`/`GameState::Init`/`Boot2FE` subset, WorldMan.

Development loop (no renderer, seconds per iteration):

```
cargo build --profile lab --bin mglab
EAGL_MG_LOG=1 ./target/lab/mglab.exe probe 3      # 3 = Dodgeball; 0 Dart 1 RcCars 2 Tetherball 4 Footie 5 Paper 6 Wall 8 FreeThrow
```

`probe` boots the world, runs `WorldMan::StartMinigameFadeComplete` and a few `WorldMan::Update` frames; an unhandled
engine function stops the run with its name and a guest backtrace.

## Shared engine services (2026-10-02)

Found with a Ghidra decompilation of the executable (`D:/tools/decomp.c`, see the Claude memory note):

* **Physics** – Havok collision layers from `PhysicsManager::CreateCollisionFilter`; characters are layer-3 sensors with
  the original capsule (`Character+0x140` radius, `+0x144` height) plus a swept test for fast bodies (sensors get no CCD);
  held bodies leave the world (`RemovePhysicsRigidBodyFromWorld`); `PhysicsRigidBody` embeds its `PhysicsUserData` at +8;
  phantoms (`GeneratePhantomFromPhysicsSystem`, `OverlapAdded/RemovedCallback`) for football goals and paper obstacles.
* **Animation** – characters only evaluate poses while "on screen": visibility queries answer yes and
  `AnimationState::Update`'s on-screen argument is forced.
* **Front end** – launches go through the original `MultiPlayerFSHandlers` (`LaunchNonTeamMiniGame` / `SetTeams`,
  `RetrieveDefaultRules`) and `StartMinigame(MP+4, 0, 0, MP+8, MP+0xec)`; PostGame info -> `fe_postgame::setup`,
  Replay/Done -> `Minigame::OnReplay/OnDone`; pause buttons -> `OnPause*`; `EndMinigame` tears the session down.
* **Presentation** – the viewport FOV (`SceneOptions+0x1c`, degrees across 4:3) drives the Bevy camera;
  `EAGL::DrawTextured` batches (indicators, cursors) are drawn by `mg_draw.rs` with textures from the `TarManager` banks;
  `PartFxManager` effects feed `fx.rs`; `Audio::PlaySFX` runs the original id switch and `Csis` instances play through
  `sfx::play_class`; `Audio::PlayMusic` picks `kMusicFilenames`.
* **Lab** – `tools/mgbattery.sh` runs every game to PostGame and exit under AI (`EAGL_MG_ALLAI`); `EAGL_MG_FE="kids;teams"`
  uses the front-end launch; `EAGL_MG_POSTGAME`, `EAGL_MG_PAUSEBTN` press screen buttons; `EAGL_DBG_CT/CTWORDS`,
  `EAGL_DBG_WORDS`, `EAGL_DBG_EV`, `EAGL_MG_SOUNDS`, `EAGL_MG_FX`, `EAGL_DBG_IMM` inspect.

## Per-game status

| Game | Status |
| --- | --- |
| Dodgeball | Complete: FE quick play / multiplayer teams, 3v3 with AI (hits, catches, dodges), keyboard controls (move, A ready/catch, J throw, Q/E dodge), HUD + round banners, indicators, effects, sound + music, pause (resume/restart/quit), PostGame -> replay/done -> menus. |
| others | Run start -> PostGame -> exit in the lab under AI; per-game polish pending (one game at a time). |
