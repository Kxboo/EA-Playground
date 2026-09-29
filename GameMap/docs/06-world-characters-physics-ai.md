# 06 — World, characters, physics, AI, cameras, animation and conversations

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF/DB, **[I]** inferred, **[U]** unresolved.
Every class and function below is listed with addresses in the generated pages under [`subsystems/`](subsystems/README.md); this document only says how the parts fit together.
Class sizes are from `data/classes.tsv`.

## The world (`engine-world`, 10 units / 223 functions)

- **`WorldMan`** (29 methods) is the game-wide façade: `Initialize/UnInitialize(WorldInitType)`, `Update(int dt)`, `GetPlayerCharacter(int)`, `StartMinigame`/`EndMinigame`, `GetActiveMinigame` and one `Get<Game>Minigame()` per
  minigame, `GetCurrentAreaAbstract/Physical`, `IsInMicrogame`, `HandleControllerDisconnect`. **[D]** Its instance is the global `gWorld` (`0x805e8320`). **[C]**
- **`PlaygroundWorld`** (64 methods) is the free-roam playground: areas are `Enums::AreaType` (abstract vs physical area mapping via `ConvertAreaAbstractToPhysical`), spawn/unspawn/clean/refill
  of an area, area physics load/unload, area transitions (`StartAreaTransition(from, to, int)`), **gates** (`PlaygroundGate`, `CheckForNewGateUnlocks`), **marbles** (collectibles: `SpawnMarbleIcons`, `SetupMarblesForArea`,
  `LoadMarbleInfoFromProfileAndDB`, `UpdateMarbles`, `AddMarblesEvent`), the in-world **microgames** (`StartBugHunt`, `StartFreeThrow`, `StartDribbling`, `UpdateMicroGame{BugHunt,FreeThrow,HighFive,Dribbling}`),
  cutscenes/"NIS" (`StartGauntletNIS`, `StartEndgameNIS`, `CheckForStickerKidNis`) and world HUD updates. **[D]**
- Supporting classes: `PlaceableManager`/`Placeable` (world props; DB class `placeables`), `SpawnManager`/`SpawnRegion` (DB `spawn_regions`), `AreaManager`, `AreaDrawManager`, `AccessibilityManager` (pop-up hints), `Marker` (`.mkr` files). **[D]**
- The world database classes (schema in [07](07-data-formats-and-hashes.md)): `area`, `area_transition`, `gates`, `placeables`, `spawn_regions`, `marbles`, `nis`, `main_menu_nis`, `character_info`, `effects`.

## Player progression and save data (`CharacterProfile`, 82 methods) **[D]**

The save-game/profile object: `SetProfileSlot`, `StartNewProfile`, `EraseProfile`, `LoadProfile(int, const ProfileData*)`, `ManualSave`, `RevertSaveFile`, plus everything the front end shows:
`GetGrade` (report card), `GetTotalStickerCount`, `GetTotalDaresComplete/GetTotalNumberDares`, `GetTotalPurchasedAbilities/AvailableAbilities`, marbles (`AddMarbles`, `RemoveMarbles`, `MarkMarbleAsPickedUp(AreaType, int)`,
`GetTotalHiddenMarbles`), `BoughtSticker(MinigameType, int, int)`, sticker book/store/cover open-close, `StartBossBattle(MinigameType)`, `OpenBossEndGameScreen`, `ShowFinalSticker`, `SetHighScore(MinigameType, int, name)`,
`SetAreaVisited/GetAreaVisited(AreaType)`, `GetSavedNPCState(u64)`, conversation-end callbacks (`ConversationEndWonStickerCallback`), `CheckForGateUnlocks`, `CheckForPopUpConversation`. The on-disc save format goes through `TRC`'s memory-card
code (`TRCStateSave/Load/Delete`, icon `data/saveicons/icon0.tpl`, banner `banner.tpl`). The byte layout of `ProfileData` is **[U]** — a real save file would let it be decoded quickly.
Progression is: minigames → stickers/marbles → gates unlock new areas → boss ("gauntlet") battles → end game. **[I]**

## Characters (`characters`, 14 units / 209 functions)

- `Character` (26 methods): created from a `CharacterAssetBundle` with a spawn region, skeleton, `AnimationState`, `TarManager`; `Update(int)`, scene add/remove, `SwitchToLocalControl()` / `SwitchToAIControl()`,
  head tracking, props (`LoadProp/UnloadProp/UpdateProp`), walk-surface effects, marble effect, celebration animation. **[D]**
- Control is pluggable: `LocalCharacterControl` (player, reads the `Controller`), `AICharacterControl` (13 methods, driven by the AI layer). `CharacterState` (21) holds position/rotation, conversation and high-five state;
  `CharacterMovement` (10) does waypoint following (`SetWaypoints(..., AIWanderType)`), speed, facing and the mapped animation states; `CharacterManager` (32) spawns/despawns characters and slots. **[D]**
- Rendering: `NPCRenderEntity`, `CharacterRenderEntity`/`CharacterPropRenderEntity`, `ShadowRenderEntity`, `NpcIndicator` (the "talk to me" marker). Character data is DB class `bestiary` (101 rows: `asset_name`, `variation`, `conversation`,
  `minigame`, `personal_space`, `celebration_anim`, `ai_head_tracking`, `partfx_*`, `scale`) plus `character_select` / `character_info`. **[D]**
- **Locomotion values (walk/run speed, jump) have not been located yet.** `CharacterMovement::SetMoveSpeed(float)` takes the speed as an argument, so the numbers come from the callers or data (**[U]**; start at
  `LocalCharacterControl::Update` and `AICharacterControl`, and the `EVENT_PLAYER_MOVE*` handling).

## Physics glue (`physics-glue`, 12 units / 221 functions) — Havok 4.x **[D]**

- `PhysicsManager` (45 methods) owns the `hkWorld`: `CreatePhysicsWorld(hkPhysicsData*)`, `LoadPhysicsData(file, handle, matrix, flags)`, rigid-body/phantom/vehicle generation, `CastRay`, `GetGroundHeight`, `GetGroundType`,
  character-character collision toggles, collision filter (`hkGroupFilter`), `Update(int)` (step model in [02](02-state-machine-and-timing.md)). Physics assets are `.hkx` packfiles (`hkBinaryPackfileReader`); ground types are 4-char material tags
  (`asph barr bloc can_ rock arch tree bush wall kite conc wood gras leaf dirt`, table `mMaterialStrings`). **[D]**
- Characters use Havok's **character proxy** (`PhysicsCharacter`, `PhysicsDynamicCharacter`, `PhysicsStaticCharacter`, listeners); Havok's `hkCharacterState{OnGround,InAir,Jumping,Climbing}` classes are linked in. Vehicles use the **Havok Vehicle** kit
  (`PhysicsVehicle`, 34 methods; `hkVehicleDefault{Engine,Transmission,Steering,Brake,AnalogDriverInput,VelocityDamper}`, `hkVehicleRaycastWheelCollide`, `hkTyremarks`) — the RC cars. Vehicle tuning: `RCCarVehicleInfo` (14) and the `mg_rccars` table. **[D]/[I]**
- Because Havok 4.x is a middleware, a Rust port has two choices: reproduce its behaviour with a physics engine and tune to the recovered data, or port the specific solver paths. Collision must come from the `.hkx` shapes (display meshes are not evidence). **[I]**

## AI (`ai`, 11 units / 231 functions) **[D]**

`AncientEvil` (15 methods; the AI manager, updated every frame from `Playground`) creates `AIEntity` objects (`SimpleMovementAIEntity`, `MetaWorldStaticAIEntity`, game-specific ones such as `TetherballAIEntity`, `DartShootoutHostileAIEntity`) and
attaches **`Compulsion`** behaviours (`WanderCompulsion`, `MoveToPointCompulsion`, `StaticAnimCompulsion`, `ActionPoICompulsion`, plus per-minigame ones). Ambient NPC life is a **Point-of-Interest** system:
`PoI` (36) / `PoIGroup` / `PoIManager` (15: `AssignBestCharacterToPoI`, `ResolvePoIChains`) / `PoIInterpreter`; PoIs read their parameters from the DB class `points_of_interest` (267 rows: `position`, `duration_*`, `wait_min`, `interest_chance`,
`action_animation`, `chain_previous`, `critical`, `uninterruptible_use_min`, `target_character`, `script_onactivate/ondeactivate/onactionstart/onactionend`) and `points_of_interest_groups` (39 rows). The four `script_*` fields name **Lua** callbacks
(the Exposure/Lua runtime is linked in; see [01](01-executable-and-boot.md)). Think depth is throttled by an `AIThinkLoD`/`AIThinkDepth` level. **[D]/[I]**

## Cameras (`cameras`, 10 units / 145 functions) **[D]**

`CameraManager` (20 methods) selects among `FreeCamera` (debug), `FrameCamera`, `HingeCamera`, `OrbitCamera`, `SlerpPanCamera`, `TopDownCamera`, `FixedCamera`, `BehindTheBackCamera` (+ the RC chase cameras in `rcchasecamera.cpp`, minigame
cameras in each game unit). `Camera::StartCameraSysTransition(CameraViewInfo, int, bool)` blends between views. Tunables: [02](02-state-machine-and-timing.md) (`gFrameCamera*`, `gOrbitCamera*`, `gHingeCamera*`, `gRc*ChaseCamera*`).
Widescreen: `Ren::Scene::CalcWidescreenFOV(float)`, `gWideScreenDefine`, the `GetAspectRatio` LV handler. **[D]**

## Animation

- **State graph**: `AnimationStateGraph::AnimStates` has **246 states** (`ANIM_IDLE`=0 … `ANIM_EG_CELEB_05`=245), grouped by activity (`AI_*` ambient NPC, `BBALL`, `QDRAW`, `TB`, `BUGHUNT`, `DB`, `SC` footie, `DS`, `WB`, `PA`, `RC`, `SELECTKID`, `CONV`, `CELEB_*` per character, `SK` sticker kid, `EG` end game).
  The full list with values is [`data/enums/animation_states.tsv`](../data/enums/animation_states.tsv). Each state maps to an animation clip through `AnimationStateGraph`/`AnimationState`/`AnimBank` (the clip names are the ones already decoded in the repo, e.g. state 240
  `ANIM_SK_BAG_REACH` ↔ the `S_sk_bag_reach` fixture). `ConvertStringToState` parses state names; the per-state metadata (`AnimStateInfo`, `AnimEventInfo`, transitions, the "mapped movement states" used by `CharacterMovement`) is **[U]**.
- **Playback** is EA's animation library (`Skeleton`, `FnAnim*`, blenders `FnPoseBlender/FnRunBlender/FnTurnBlender`, `StatelessQ/F3`, delta-compressed channels) — middleware, ~20 KB; the repo's decoder already reads its clips.
  The *rate* is still assumed 30 fps in the workbench; the state graph's event/phase channels (`RawEventChannel`, `CsisEventChannel`, `PhaseChan`) are where sound/effect triggers and root-motion phase live (**[U]**).

## Conversations **[D]**

`ConversationManager` (19 methods) + `ConversationRoot/Dialog/Response/Node/Pool`, data from `data/conversation/conversation.viv`; text via `Locale`; UI through `ConversationLVHandlers` (`Conversation_GetName/GetDialogueText/GetResponses/OnPlayerSelect`) and the fixed response labels
`Yes`, `No`, `DONE`, `GAUNTLET`, `STORE`, `Next`. Characters expose `StartConversation(ConversationBranchType)`, `IsInConversation`, conversation position/rotation (saved and restored around minigames: `CharacterState::SaveConversationPositionAndRotation`).
Camera: `gConversationCameraDistance = 5.0`, `gConversationCameraTransitionTime = 0.6`. The conversation file format is **[U]**.

## Audio (`audio`, 13 units / 329 functions) **[D]**

Game layer over EA's AEMS/Csis sound system: `Audio` (50 methods: `PlayMusic(AUDIOMUSICTYPES)`, `PlaySFX`, `EnterArea/LeaveArea`, `StartAmbience`, `LoadData(AUDIODATA)`, `SetAudioMode`), `AuAEMSManager` (banks), `AuMusicManager` (streams `*.asf`), `AuSpeechManager`,
`AuWiimoteManager` (Wii-remote speaker), `AuEnvironmentManager`, per-object sound classes. Banks/music/speech directories are in the [asset tables](asset-name-tables.md). Audio containers (`.abk`, `.ast`, `.asf`, `spchdat.*`) are **[U]**.

## Effects (`effects`, 5 units / 730 functions — the largest game unit group) **[D]**

`worldeffectmanager.cpp` (48 KB) contains the whole **Lion** particle system (`cParticleDescriptor/Emitter/Behaviour/Bucket…`, geometry plug-ins Cube/Cylinder/Quad/Sphere, post-processing plug-ins Bloom, Bulge, FastTint, FlexibleTint, Gloom, MotionBlur) and `PartFxManager`/`PartFx`
(`partfx.csv`, `.lef` effect files). Reconstruction can substitute Bevy particles; the data (`.lef`) format is documented in `FINDINGS.md`.
