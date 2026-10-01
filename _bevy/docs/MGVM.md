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
