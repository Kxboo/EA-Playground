# 02 — Top-level state machine, frame order and time step

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF, **[I]** inferred, **[U]** unresolved.
Machine-readable companions: [`data/state_transitions.tsv`](../data/state_transitions.tsv), [`data/globals.tsv`](../data/globals.tsv),
[`data/traces/`](../data/traces/).

## The eight `GameState` states

`GameState::Update()` (unit `GameState.cpp`, `0x803acdc4`) dispatches through a jump table at `0x804e885c`. The enum value is
the table index; the name is the `STATEFN_UPDATE_*` function it reaches. **[C]**

| value | state | function | role |
|---:|---|---|---|
| 0 | `PG2FE` | `0x803ad364` | leave the 3-D playground world, return to the front end |
| 1 | `FE2PG` | `0x803ada28` | enter the playground world from the front end |
| 2 | `Boot2FE` | `0x803adb20` | one-off: build world/physics/camera after the strap screen, enter the front end |
| 3 | `FE2MP` | `0x803ad834` | start a minigame ("MP" = minigame/multiplayer session) from the front end |
| 4 | `MP2FE` | `0x803ad910` | end a minigame, return to the front end |
| 5 | `FrontEnd` | `0x803adf70` | menus (title, profile, kid select, main menu, multiplayer setup, …) |
| 6 | `Playground` | `0x803adc18` | the 3-D playground **and** minigames (a minigame runs inside this state) |
| 7 | `BootFlow` | `0x803ad02c` | intro movie(s), boot check, then first menu |

`GameState::SetNewState(state, label)` (`0x803acfb8`) stores the state in `sCurrentGameState` (`0x805ff80c`, initially `-1`) and, on entering
`FrontEnd` (5) or `Playground` (6), calls `Audio::SetAudioMode(0)` / `SetAudioMode(1)` respectively. **[C]** The second argument is a debug label string.

`Update()` first runs pending `SYNCTASK`s, then returns **the state function's result** (`0` ends `MainThread`'s loop: `while (GameState::Update()) {}`).
If the byte at `gpTrcCorehandlers + 0xe0` is non-zero it returns `1` immediately, **skipping the frame** (a Wii pause/HOME-menu style path; the meaning of the
flag is **[I]**). With state `-1` (before `BootSequence`) it does nothing and returns `1`. After the state function it updates the `Controller`
(or `PAD_update` while the HOME menu is active). **[C]**

### Transition graph (every `SetNewState` call site)

```mermaid
stateDiagram-v2
    [*] --> BootFlow : BootSequence
    BootFlow --> Boot2FE : STATEFN_UPDATE_BootFlow
    Boot2FE --> FrontEnd : STATEFN_UPDATE_Boot2FE
    FrontEnd --> FE2PG : FEManager::EnterFEGameState
    FE2PG --> Playground
    FrontEnd --> FE2MP : MultiPlayerFSHandlers::SetTeams / LaunchNonTeamMiniGame
    FE2MP --> Playground : WorldMan::StartMinigame
    Playground --> MP2FE : WorldMan::EndMinigame / TRC::ControllerDisconnectOptionsResult
    MP2FE --> FrontEnd
    Playground --> PG2FE : StickerBookCoverOnQuit / StickerBookHandlers_SaveDoneCallBack
    PG2FE --> FrontEnd
    FrontEnd --> Playground : LoadingScreenLVHandlers::LoadingGetStartOrBack
```

The list above is exhaustive: it is the complete set of 16 call sites of `SetNewState` (`data/state_transitions.tsv`). **[C]**

### What each transition state does (call order read from the code) **[C]**

| state | ordered calls (Wii/TRC and rendering helpers omitted where noted) |
|---|---|
| `Boot2FE` | `TRC::DisableHomeMenu` → `SYNCTASK_add` → `Engine::SetVSync` → `InitPostStrapScreen` (PartFx manager) → reconfigure memory pools (`ShutdownPoolConfig`/`InitPoolConfig`) → `PhysicsMemory::Create` → `PhysicsManager::Create` → `AncientEvil::Create` (AI manager) → `FEManager::SetState` → `WorldMan::Initialize(WorldInitType)` → `CameraManager::Create/Initialize/ReInitialize` → `SetNewState(FrontEnd)` → `SYNCTASK_del` → `TRC::EnableHomeMenu` → `TRC::RegisterCoreController` |
| `FE2PG` | `DisableHomeMenu` → `FEManager::SetState` → `WorldMan::Initialize(WorldInitType)` → `CameraManager::ReInitialize` → `SetNewState(Playground)` → `FEManager::SetState` → `FEManager::StartFadeOutEffect` → `Audio::PlayMusic/EnterArea/StartAmbience/LoadData` → `Controller::DisconnectAllUnusedRemotes` → `EnableHomeMenu` |
| `PG2FE` | `DisableHomeMenu` → `FEManager::SetState/SetFEGameState/StartFadeOutEffect` → `WorldMan::UnInitialize(WorldInitType)` → `Audio::UnloadData/StopAmbience/LeaveArea/Update` → `FEManager::ResetPanCamera` → `SetNewState(FrontEnd)` |
| `FE2MP` | `DisableHomeMenu` → `FEManager::SetState` ×2 → **`WorldMan::StartMinigame(MGID, int, MiniGameDifficultyLevel, const Teams&, const int*)`** → `SetNewState(Playground)` → `Controller::DisconnectAllUnusedRemotes` |
| `MP2FE` | `DisableHomeMenu` → `FEManager::SetState` → `OpenAptScreen` → `SetFEGameState` → `OpenAptScreen` → `Audio::Update` → `FEManager::ResetPanCamera` → `SetNewState(FrontEnd)` → **`MultiplayerMode::EndMultiplayerGame`** |
| `BootFlow` | `DisableHomeMenu` → `FEManager::SetState/SetFEGameState` → `GlobalHandlers::SetCursorVisibility` → builds movie path strings (`CString +=` ×7) → `PGMoviePlayer::Play` ×2 → per frame: `IsFinished` / `Update` / `Controller::Get` (skip) / `Stop` → `SetNewState(Boot2FE)` → `FEManager::OpenAptScreen` → `TRC::StartBootCheck` → each frame `Engine::BeginFrame` → movie render → `Scene::Draw` → `StrapWarningScreen::Update` → `TRC::UpdateTRC` → `EndFrame` → `FEManager::Update` |

`GameState::Init` (`0x803abf40`) builds the three scenes, adds the main `.big` file (`pgIO::AddBigFile`), creates the `Controller`, world-light/light/shadow
managers, full-screen effects, sky dome, `FEManager`, `PGConga`, `CharacterProfile`, `MultiplayerMode`, the debug menu, and seeds the RNG with `OSGetTick()`
(`EA::Math::SeedRandom`). **[C]** `GameState::Shutdown` reverses it. **[C]**

`SetVSync(bool)` brackets every transition (once before, once after). The arguments are not statically visible in the call sequence; presumably off
during loading and on after. **[I]**

## Per-frame order inside the two long-running states **[C]**

**`FrontEnd`** (`0x803adf70`): TRC checks (`HomeMenuActive`, `ControllerDisconnected`) → movie player queries → `Audio::Update` → `GameState::HandleSelections` →
view-matrix setup → `FEManager::UpdateWorldForFE`, `PartFxManager::Update`, `FEManager::UpdateCamera`, `FEManager::Update` → `SkyDome::Update` → `WorldMan::Update` →
`CameraManager::Update` → **`AIP::Update` (APT/menu movie tick)** → background colour → `FullScreenEffectsManager::Update` → `Engine::BeginFrame` → cross-sell
movie handling → `ShadowManager/WorldLightManager/LightManager::Update` → `Scene::Draw` ×5 → `TRC::UpdateTRC` → `Engine::EndFrame`.

**`Playground`** (`0x803adc18`): TRC checks → `FEManager::Update` → `Audio::Update` → `PGConga::Update` (EA "Conga" input service) → **`GameState::HandleActions`** →
`AncientEvil::Update` (AI manager) → `SkyDome::Update` → **`WorldMan::Update`** → `PartFxManager::Update` → `pgIDatabase::Update` → `FullScreenEffectsManager::Update` →
`CameraManager::Update` → view matrices → `AIP::Update` (HUD movies) → `WorldMan::HandleControllerDisconnect` → optional screenshot → `Engine::BeginFrame` → `UpdateShadows` →
world/light updates → `Scene::Draw` ×5 → `TRC::UpdateTRC` → `Engine::EndFrame`.

`HandleActions` (`0x803ac378`, 2,028 B) dispatches world-level actions: free-camera control (tilt/up/down and camera transitions), opening the **sticker book cover**
and the **report card**, and the debug menu; it consults `WorldMan::IsInMicrogame()`. `HandleSelections` (`0x803acbd4`) does the debug-menu subset for the front end. **[C]**
(Gameplay input — movement, jumping, minigame controls — is *not* handled here; see [03](03-input.md).)

## Time step **[C]** unless noted

`GameState::Update` computes one integer-millisecond delta per frame:

```
cycles  = (TBL * 12) - nLastCycles                     ; Broadway time base ×12 ⇒ CPU cycles  [I: 729 MHz core clock]
frameMs = cycles / <word at absolute 0x800000fc> * 1000.0     ; float  (sfFrameMilliseconds)
dt      = (unsigned) frameMs                              ; truncated integer ms (sElapsedTimeFrame)
if (gFrameCapEnabled)  dt = min(dt, gCappedMillisecondsPerFrame)   ; 1 / 60 initial
if (gSimFixedTime)     dt = gSimFixedTimeAmt                        ; 16 initial   (debug option)
if (gSimPause)         dt = 0
sTotalElapsedTime += dt ; sElapsedSinceLastRender += dt
```

| global | address | initial value |
|---|---|---|
| `gFrameCapEnabled` | `0x805ff804` | `1` |
| `gCappedMillisecondsPerFrame` | `0x805ff800` | `60` |
| `gSimFixedTimeAmt` | `0x805ff808` | `16` |
| `gSimFixedTime`, `gSimPause` | `0x80602005`, `0x80602004` | runtime flags (`.sbss`) |

So the simulation is **variable-timestep, integer milliseconds, capped at 60 ms per frame** by default. The state function receives
`sElapsedSinceLastRender + dt` (that accumulator is reset to 0 at the end of every `Update`, so in practice `dt`) and passes it on to
`FEManager::Update`, `WorldMan::Update`, `AIP::Update` etc. `Controller::Update` is different: it receives the **uncapped** `sfFrameMilliseconds` truncated to int (`fctiwz`), so input timing (double-press, hold) is measured in real frame time even when the simulation is capped. The divisor is read from **absolute `0x800000fc`**: `lis r3,0x8000` at `0x803ace0c`, then `lwz r0,0xfc(r3)` at `0x803ace24`. Earlier notes incorrectly associated this with `gpTrcCorehandlers`. The runtime word is absent from the ELF, so its value remains **[U]**; 729 MHz is an inference, not a captured console value.

The Rust port in `_bevy/src/sim_time.rs` and `tools/timing_oracle.py` verify this conversion (including 32-bit cycle wrap), cap/fixed/pause order and uncapped controller time against execution of the original instructions. The Bevy slice now applies this policy once per host frame; matching the console's frame pacing still needs a recording.

### Physics step **[C]**

`PhysicsManager::Update(int dt_ms)` (`0x803b6f84`) drives the Havok world:

```
if (dt_ms > 0 && world && !paused(+5)):
    dt = min(dt_ms, 200)                                  ; hard clamp 200 ms
    n  = 1
    if (dt > gSimPhysicsSingleUpdateTimeCap)              ; 60 ms
        n = ceil(dt / (float)gSimPhysicsSingleUpdateTimeCap)
    for i in 0..n-1:
        slice = dt*(i+1)/n - dt*i/n                        ; integer division, distributes the remainder
        hkWorld::stepDeltaTime( slice * gSimPhysicsTimeMultiplier / 1000.0f )
```

`gSimPhysicsSingleUpdateTimeCap = 60`, `gSimPhysicsTimeMultiplier = 1.0f` (`0x805ff96c`, `0x805ff968`). Gravity is taken from the Havok world's construction info
(`PhysicsManager::GetGravity`); when no world exists it returns a hard-coded vector from `.sdata2`. The world itself is created from `.hkx` physics data
(`PhysicsManager::CreatePhysicsWorld(hkPhysicsData*)`, `LoadPhysicsData`), so world gravity/solver settings live in data, not code. **[C]/[U]**

## Tunable globals worth knowing (initial values from the ELF image) **[D]**

`data/globals.tsv` lists the ~1,400 globals referenced by game/engine code with their initial values. Examples that matter for a first faithful port:

| symbol | value | meaning ([I] for the reading) |
|---|---|---|
| `gControllerDoubleDownThreshhold` | `500` | double-press window, ms |
| `gTapPressThreshhold` / `gTapHoldPressThreshhold` | `180` / `120` | tap vs. hold timing, ms |
| `gControllerRumbleMSPerInterval` | `100` | rumble pulse interval, ms |
| `gConversationCameraDistance` / `…TransitionTime` | `5.0` / `0.6` | conversation camera |
| `gFrameCamera{Distance,Height,Angle}` | `2.9`, `1.1`, `-0.52` | frame camera (kid select) |
| `gOrbitCamera{Distance,Height,TargetX,TargetZ}` | `20`, `3.8`, `41.9`, `-83.8` | orbit camera |
| `gHingeCamera{Distance,Angle,TargetOffsetY}` | `3.6`, `10.5`, `-0.065` | hinge camera |
| `gRcCars…Camera*`, `gRcCars{LaneSwitchDuration,MinLaneAdvance,PowerupExplosionDuration,InterestCamDuration}` | see table | RC Cars chase cameras/timings (`300`, `0.2`, `3000`, `2000` ms) |
| `gDartShootoutMaxHittingStreak`, `gDartShootoutDamageWithDamageUpgrade` | `5`, `3` | Dart Shootout rules |
| `gShadow{Width,Height}__3Ren` | `512`, `448` | shadow-map size |
| `gShadowLightDistance` / `gShadowWorld{Length,Width,Offset}` | `12` / `6.67`, `8.33`, `-3.33` | shadow volume |

Class statics follow the naming `sName__Class`; they appear in the per-subsystem pages under *Globals and class statics referenced*.
