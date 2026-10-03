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
* **World placeables** – the guest's `Placeable+0xa8` visibility changes since launch are mirrored onto the Bevy world entities of the same name (default-hidden placeables are spawned hidden; attributes are inherited from parent collections).
* **Presentation** – the viewport FOV (`SceneOptions+0x1c`, degrees across 4:3) drives the Bevy camera;
  `EAGL::DrawTextured` batches (indicators, cursors) are drawn by `mg_draw.rs` with textures from the `TarManager` banks;
  `PartFxManager` effects feed `fx.rs`; `Audio::PlaySFX` runs the original id switch and `Csis` instances play through
  `sfx::play_class`; `Audio::PlayMusic` picks `kMusicFilenames`.
* **Rendering state** – the snapshot camera is `CameraManager` slot 0 in its final pose (`GetPos/GetTarget(true)`); the
  Bevy camera takes the game's near plane (`SceneOptions+0x14`, default 1.0) and field of view; primitives cull back
  faces only when their `EAGL::GeoPrimState` sets property 50 (`SetCullEnable`; the default state does not cull);
  the guest renders flat (`gDisableCurvedWorld` set at boot) and the host bends by the world radius (250).
* **HUD queries** – `MinigameLVHandlers` answers (`GetNumberOfHuds`, `Counter_GetText`, `GetGameRules`,
  `Footie_IsSaveDare`, `PaperAirPlanes_GetCheckPoints`) are published from the guest every frame (`fe.mg_lv`).
* **World physics** – `PhysicsManager::InitializeSim(name)` loads `name`'s world: its `hkWorldCinfo` gravity and its
  bodies on layer 1 (`LoadPhysicsData(.., 1)`); the playground's (`playground.hkx`, the school area) and Paper
  Airplanes' (`pa_world`, no gravity).
* **Input** – `WPADStatus` acceleration is signed and centred (rest 0, 0, ~104); Conga gestures read changes, the games'
  tilt controls (RcCars lanes, Paper pitch / bank, Free Throw aim) read the values themselves.
* **Materials** – friction and restitution combine as Havok's geometric mean (square roots stored per collider,
  multiplied per pair).
* **Contacts** – a body listener gets `ContactAddedCallback` (its return value accepts the contact), then
  `ContactConfirmedCallback` and `ContactProcessCallback` (where e.g. Dart Shootout scores).
* **Lab** – `tools/mgbattery.sh` runs every game to PostGame and exit under AI (`EAGL_MG_ALLAI`); `EAGL_MG_FE="kids;teams"`
  uses the front-end launch; `EAGL_MG_POSTGAME`, `EAGL_MG_PAUSEBTN` press screen buttons; `EAGL_DBG_CT/CTWORDS`,
  `EAGL_DBG_WORDS`, `EAGL_DBG_EV`, `EAGL_MG_SOUNDS`, `EAGL_MG_FX`, `EAGL_DBG_IMM`, `EAGL_DBG_DART`, `EAGL_DBG_DRAW=<model>`
  inspect; `EAGL_MG_LEVEL` picks the level. In the app `EAGL_MG_HIDE=<model>`, `EAGL_FX_SKIP=<effect>` and
  `EAGL_MG_CAMBACK=<units>` (watch from behind the game camera) help find what is drawn where.

## Per-game status

| Game | Status |
| --- | --- |
| Dodgeball | Complete: FE quick play / multiplayer teams, 3v3 with AI (hits, catches, dodges), keyboard controls (move, A ready/catch, J throw, Q/E dodge), HUD + round banners, indicators, effects, sound + music, pause (resume/restart/quit), PostGame -> replay/done -> menus. |
| Tetherball | Complete: FE launch, AI serve/rally, J toss / K strike / L reverse strike (overhand strike needs the mega-hit ability flag `MGTetherball+0x424`), plain pole swapped in for the world's pole-with-ball (guest `Placeable+0xa8` mirrored onto the Bevy world), HUD, effects, sound, music, PostGame -> menus. |
| Wallball | Complete: FE launch, AI rallies, J toss / K backhand / L forehand (overhand serve is ability-gated, `MGWallball+0x1d3/+0x1d5`), power-ups and ball trail drawn, HUD hit counter / serve bubble, sound, music, PostGame -> menus. |
| Footie | Complete: FE launch, 2v2 AI (serves, juggles, shots, dive saves, goals via net phantoms), J serve / O juggle / I shot / Q,E dive, power-ring indicators (textures by bank index), HUD, sound, music, PostGame -> menus. |
| Dart Shootout | Complete: FE launch (course from the level argument), rail camera, mouse = remote pointer (reticle effect, blaster aims via `Controller::GetWorldVectorFromDPDRotationallyCorrected`), left button B fires, right button A reloads / raises the shield, pin-up and character targets score through `DSDartCollisionListener::ContactProcessCallback`, hostile darts stick to the screen plane and drain health, single-player HUD (`GetNumberOfHuds`), PostGame -> menus. |
| RcCars | Complete: FE launch, 4-car race with AI, the game's own chase camera (viewport 0 of `CameraManager`), Space/A accelerate, arrows tilt the remote to switch lanes (`atan2(x, z)` beyond 45 degrees), B power-ups, boost, track model drawn as the game's replacement environment (`gRenderWorld` 0 hides the playground terrain and props), lap / position / power-up HUD (`Counter_GetText`), `MGSFX_HUD_RC` sounds, engine / fuse loops (`Audio::StartSFX` / `UpdateSFX` / `StopSFX` run the manager's native switch; one looping voice per class, pitched by the instance's speed input - an approximation of the AEMS engine programs), PostGame -> menus. |
| Paper Airplanes | Complete: FE launch, hallway courses drawn with the game's own curvature (`SetCurvedWorldRadius` 75, area model hidden), zero-gravity Havok world from `pa_world` (`InitializeSim`), J throws, arrows pitch / bank (centred accelerometer), B boost, crashes and respawns, checkpoints / points / timer HUD, Times Up -> PostGame -> menus. |
| Free Throw | Complete as the playground microgame: in world play, standing at one of the three hoops (`kFreeThrowPosition`) shows the World HUD's press-A, A (Space) starts it with the game's first-person camera, J shoots with the swing strength as power, shot meter / counter / timer HUD, the world resumes afterwards. The aim sweeps left and right by itself (`UpdateAimOffset`); a throw released as it crosses the middle goes straight in, at all three hoops. |

## Single-player world (2026-10-03)

Main-menu Single Player (and the `wp` / `gm99` script codes) starts `mgvm::launch` type 99 (`WORLD`): the playground
itself runs in the VM as one session, and `minigame_type` follows whatever the world starts (`WorldMan + 0x90` by vtable).

* **Intro** - `PlaygroundWorld + 0x44` state machine (fade 0xd -> pan camera 0xe -> 0xf -> area NIS 0xb -> Sticker King
  4/5 -> free roam 0); needs both `FadeToColourEffect`s (+0x8b0, +0xff0) ticked each frame.
* **Front end** - the game's AIP broker runs natively (`world::init_aip`; Broker, CmdComposer/Decomposer and handler
  registration exempt from stubs). `mgvm::aip_call(name, params)` = `Broker::LoadVariables` (reply `k=v&..`, escapes
  `%25 %26 %3D %2B`, arrays 0x7f) else `FSCommand`. In a world session every front-end call goes there first
  (`mg_session::guest_lv`, via a thread-local guest scope during the APT tick): conversations, info dialogues, pre/post
  game, HUD queries, sticker book, gauntlet select. Screen calls from the guest are deferred into the APT tick.
* **Talking** - facing a beacon kid (`NpcIndicator`, drawn from `world-misc.gsh`) and A (event 0xae) starts
  `CharacterProfile::StartInitialConversation`; dares start the minigame inside the session (pre-game, game, post-game,
  back to the world, `StickerBookGame` award).
* **Microgames** - Dribbling and Bug Hunt start from their press-A spots (World HUD loaded flag FEManager + 0x130).
* **Areas** - gates unlock from the profile's stickers (placeables are built before `InitializeCharacters`);
  `StartAreaTransition` runs the area title / NIS / intro conversation; Bug Hunt is placed in areas 2 and 3.
* **Sticker King** - store / gauntlet conversation; gauntlet -> `BossGameSelect` -> boss battle.
* **Rendering** - scenes with `gRenderWorld` off render unbent; immediate-mode batches bend at their centroid; prop
  entities pooled per model.

Testing aids: `EAGL_MG_TP="frame:x,z[,fx,fz]"`, `EAGL_MG_AREA="frame:gate"`, `EAGL_MG_STICKERS=N`,
`EAGL_MG_GAUNTLET=1`, `EAGL_MG_KID`, lab `EAGL_MG_CONV="text~n"`, `EAGL_MG_BOSS`, `EAGL_DBG_MICRO`, `EAGL_DBG_PLACE`;
app scripts `--apt-script file:PATH` with `@Screen+N~step` guards and `iPATH` inspections.

Open: post-game DONE clicks and placing an award sticker fail in the app because the front-end player computes wrong
global bounds for some animated clips (game logic verified in the lab: placing + `StickerBook_Exit` resumes the world
with 1/24); the sky dome looks plain blue towards the north of the school plaza; saving; bug sprites (`pg_bughunt_*`
effects) not yet checked on screen.

## Decompilation progress map

The decomp.dev-style map lives in GameMap: [`GameMap/site`](../../GameMap/site), published to GitHub Pages. See
[`GameMap/docs/11-progress-site.md`](../../GameMap/docs/11-progress-site.md). Its **runtime** measurement comes from this VM:

1. Record every hook the VM installs:

   ```bash
   cargo run --release --bin mglab -- classify scratch_progress/classify.tsv
   ```

   Each hook is classified as host, observe, stub or trap; everything else is native.
2. Run scenarios with `EAGL_PPC_COVER=scratch_progress/cov_<id>.tsv`. `boot_as` turns on per-function entry counting in
   gekko, and `mgvm::write_coverage` appends `addr\tentries` lines when the probe finishes.
3. Import both, then rebuild the site:

   ```bash
   python GameMap/tools/ingest_runtime.py --elf Remaster/reference/playgroundz.elf --classify scratch_progress/classify.tsv --cover 'scratch_progress/cov_*.tsv'
   python GameMap/tools/build_site.py
   ```

   `build_site.py` also scans `_bevy/src` for the Rust that ports, hooks or drives each function.
