# 09 — Suggested reconstruction order, prerequisites and open questions

Evidence tags: **[C]** confirmed, **[D]** data, **[I]** inferred, **[U]** unresolved. This follows the repository policy in [`_bevy/docs/RECONSTRUCTION.md`](../../_bevy/docs/RECONSTRUCTION.md):
recover tick rate and input first, no plausible replacement rules labelled 1:1, collision from decoded data, compare against the original before promoting anything.

## What is ready to port now (evidence: emulation-verified or read-and-cross-checked)

| item | source of truth | notes |
|---|---|---|
| `GameState` machine and transition graph | [02](02-state-machine-and-timing.md), `data/state_transitions.tsv` | 8 states, 16 call sites; trivial to port; drives everything else |
| time step: integer ms, cap 60, fixed 16 debug, uncapped time for input | [02](02-state-machine-and-timing.md) | divisor source `[U]` (assume 729 MHz until the store is found) |
| physics step: clamp 200 ms, ≤ 60 ms slices, `n = ceil(dt/60)`, integer slice distribution | [02](02-state-machine-and-timing.md) | independent of Havok |
| hashes for `.idx` and VLT keys | `tools/hashes.py` (verified) | unlocks text and the whole database |
| `MultiplayerMode` + post-game awards | `reference/multiplayer_mode.py` (verified) | port 1:1 |
| control-event/state/button enums and CSV schema | [03](03-input.md), `data/enums/` | validated on 7 real files |
| APT ↔ native binding names (132) and the front-end state machine | [04](04-frontend-and-menus.md) | contract between menus and code |

## Proposed order

1. **`eagl-game` crate (no rendering)** — modules `time`, `state`, `input` (CSV → event table → `EventState` polling), `attrib` (VLT reader using `attrib_hash64`), `locale`, `mp` (tournament scoring), `apt_bridge` (FS/LV handler traits registered by the exact names).
   Everything above is decoded; write unit tests from the verified models and keep `tools/verify_*.py` alongside to re-check against the ELF.
2. **Input**: decode the 14 `controls*.csv` (7 remaining files to send: dodgeball, paperairplanes, quickdraw, rccars, tetherball, wallball, template) and reproduce `Controller::UpdateInput` (`0x8032cb58`, 3.6 KB) against `gControllerDoubleDownThreshhold` (500), `gTapPressThreshhold` (180),
   `gTapHoldPressThreshhold` (120). Verify by emulating `UpdateInput` with scripted pad states (the harness in `tools/emu.py` already runs functions on the original image; `UpdateInput` only needs a `Controller` object and pad data).
3. **Database values**: extend `vlt_probe.py` from schema to values (collection field layout and `PtrN`; both are small, regular structures) and expose each `mg_*` table as typed Rust structs. This removes every magic number from the minigames.
   Also read `string.idx` + `.loc` (text for all screens).
4. **Front-end slice** (Title → Profile → Kid select → Main menu → Multiplayer setup → launch): the state machine and handler contract are fully known; the screens need either APT playback (open in `FINDINGS.md`) or Bevy UI stand-ins driven by the same
   handler calls. Selectable-kid scene = `character_select` DB rows + `SelectableCharacter`.
5. **First minigame: Tetherball.** Smallest complete game: `mgtetherball.cpp` + `tetherball.cpp` + `aitetherball.cpp` (3 units, 129 functions, 39 KB), 6 database rows (`mg_tetherball`: `ball_basehitspeed`, `ball_acceleratemodifier`, `ball_powermodifier`, `ball_megamodifier`,
   `hit_returnangle…`, `accuracy/powerhit/megahit_points`, `game_duration`, AI mess-up factors), input context `STATE_TB_*` with `EVENT_TETHERBALL_*`, animation states `ANIM_TB_*` (41), 1v1 scoring already verified. Then **Wall Ball** (1v1, same scoring), then Dodgeball/Footie (team scoring).
6. **World slice** (one character, one small area): prerequisites are locomotion values from `LocalCharacterControl::Update`, `PhysicsCharacter`/Havok character-proxy parameters, `.hkx` collision, the `ANIM_*` state graph, cameras (`gFrameCamera*`, `gOrbitCamera*`). This is the largest unknown; do it after a minigame has proven the loop.
7. **RC Cars** (needs Havok Vehicle behaviour + `rccarvehicleinfo` + track data), **Dart Shootout** (largest: targets/waves from `mg_dartshootout`, 160 rows), **Paper Airplanes**, microgames and the sticker/marble progression (`CharacterProfile`).

## Verification strategy (how to earn "compared")

- **Pure functions** — emulate the original code and diff (done for hashes, scoring, ranking; extend to `UpdateInput`, `BuildPlacementList*`, `GetArrayIndexFromDiffLevel`, camera math, AI decision functions given a fixed RNG).
  The RNG is `EA::Math::SeedRandom(OSGetTick())`: for reproducible AI comparisons the seed must be forced (emulate `SeedRandom` with a constant).
- **Whole-game behaviour** — Dolphin input recordings (`.dtm`) of menu navigation and one minigame round, with frame-stepped state dumps for `sCurrentGameState`, `FEManager` state (`+0x3c/+0x40`), `MultiplayerMode`, positions. The globals' addresses are in `data/globals.tsv`.
- Keep `[C]/[I]/[U]` tags on every ported rule and cite the ELF address; promote `[I]` only through one of the two methods above.

## Open questions (all `[U]`, each with a starting point)

| # | question | where to look |
|---|---|---|
| 1 | source of the clock divisor at `gpTrcCorehandlers+0xfc` | `TRCCoreHandlers::Create` (`0x802523e4`), `TRC_InitInfo` |
| 2 | exact `BUTTON_TAP/HOLD/SECOND/HOLDPRESSED` timing and how "no modifier" (0) is distinguished from `UP` | `Controller::UpdateInput` |
| 3 | which pad-mode handler each minigame/menu selects | `CorePadModeHandler` … vtables, `EnableFrontEndInput`, `gEnableNunchuck` |
| 4 | per-field value layout of VLT collections; `PtrN` fix-ups; enum member tables in `db.bin` | `Attrib::Vault/Collection` code + the supplied `db.vlt` |
| 5 | `ProfileData` save layout | `CharacterProfile::LoadProfile/ManualSave`, `TRCStateSave`; a real save file |
| 6 | APT screen list (`data/fe/`) and screen → handler mapping | directory listing + one `.apt` |
| 7 | animation state metadata (transitions, events, mapped movement states) | `AnimationStateGraph`, `AnimationState` |
| 8 | Lua scripts used by POIs (`script_on*` fields) and where they live | Exposure/Lua glue, `points_of_interest` values |
| 9 | player locomotion speeds/jump, camera follow rules | `LocalCharacterControl`, `CharacterMovement`, `CameraManager` |
| 10 | AI compulsion behaviour per game (`…Compulsion::Think`) | generated subsystem pages |
| 11 | `MinigameType` ↔ `MGID` mapping and free-throw/bug-hunt/high-five launch conditions | `WorldMan::GetMinigameType(MGID)` (`0x803e26e0`) |
| 12 | the two unnamed DB classes (hashes `22808541710a6d7a`, `8dc7ad63193d47f2`) | hash candidate names from the data files and Lua |
