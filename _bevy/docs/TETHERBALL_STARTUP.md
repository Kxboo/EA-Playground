# Tetherball startup orchestration

`tetherball_startup.rs` ports the enclosing 3,276-byte `MGTetherball::Initialize` routine at `0x803966c4`. It operates on the existing `Runtime` owners. `StartupState` adds only previously unrepresented base/world pointers, tags, identity inputs, archive and alternate-pole handles, the pole-glow handle, startup mode word and inverse world matrix. It requires existing constructor/session state. [Constructor recovery](TETHERBALL_CONSTRUCTOR.md) now supplies exact stores and a Runtime projection; [Session setup](TETHERBALL_SESSION.md) now composes constructor, selected settings and startup; a concrete engine/Bevy host remains pending.

The routine selects the original school, stadium or forest pole placeables; controls visibility; constructs world and inverse transforms; calculates the two spawn positions; routes first-player and AI/additional-player setup; configures camera offsets, direction and transitions; loads the original VIV; initializes the ball using a ground query; installs return/start angles and AI direction/distance stores; registers all four Conga callbacks; creates pole glow; reads speed and signed-angle tables; configures audio and shadows; then opens pregame, enters state 1 and selects the server. Home-menu, sync-task and VSync ordering is preserved. Startup does not substitute a round reset.

Animation initialization, `SetPlayerDistance`, state-1 entry, `SetupShadowOptions`, `InitGameLogicState` the ball constructor and both base initialization bodies call their recovered Rust bodies directly. The ball radius setter is also direct. The corresponding native bodies execute unhooked in the oracle. Remaining stages—base initialization, area selection, character/AI setup, ball initialization, pregame and server selection—are explicit synchronous dependencies. Many already have standalone recovered ports, but their complete startup composition and the Bevy service implementation are not verified by this fixture.

Camera vectors at `0x805e3750` and `0x805e37b0` are external initialized inputs. Pole position, world origin and ground-derived ball anchor stay distinct. The native initializer forces heading zero for the three supported areas; matrix arithmetic still follows the original multiply and inverse rounding order, including signed zero. Game mode 0..3 maps directly to difficulty, and participant mode 3/6 selects the additional human/AI path. Unsupported areas, game modes or participant configurations have no established successful startup in this port and return errors. An error can follow earlier effects and mutations; the host must discard the partial match.

The 24-case PowerPC oracle spans three areas, four modes and both participant configurations, with varied forced-AI flags, character flags, animation sides, signed origins, ground heights and tuning. It executes the enclosing original instructions and the composed helpers, while supplying explicit writes for the remaining stage boundaries. Comparisons cover the complete game memory image (mapped owners plus preservation of other words), both transforms, all animation tables, camera/AI stores, ball radii, visibility, renderer/global flags, and ordered boundary calls. Static shadow defaults are initialized by the original module initializer. This is stronger than independent helper tests, but is not yet a complete original startup graph with every child routine unhooked.

Run `py -3.14 tools/tetherball_startup_oracle.py --check` and `cargo test --release --offline --locked tetherball_startup::tests` from `_bevy`. The pinned ELF SHA-256 is `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

Next: recover/apply native constructor and base/session state, replace the remaining stage boundaries with composed ports and an original full-graph fixture, then implement interactive Bevy startup and engine services. The existing exploration view does not launch this minigame yet.

## Game-logic startup composition (2026-09-30)

Verified: startup now invokes `tetherball_initialize::init_game_logic_state` (original `0x8039ceac`) on the live Lifecycle, ResetState and MatchRules owners. `StartupServices::game_logic_inputs` supplies only decoded tunables and score weights; hosts can no longer replace the helper with arbitrary game-memory writes. Inputs must preserve the constructor's prior rotation cap before the recovered signed-byte handicap division. This query boundary is not a reentrant callback and must not mutate game owners.

The 24 enclosing native cases now execute InitGameLogicState unhooked, covering both single/multiplayer tunable branches, signed handicaps, three areas and four difficulties. Only its VLT tunable helpers and ResetStats remain supplied dependencies, recorded in original call order. The fixture compares the added tuning, rotation-byte, winner, statistic and indicator stores alongside the existing startup projections. Test inputs are synthetic dependency values, not recovered production configuration. Constructor/base/session recovery and the interactive Bevy host remain outstanding.

## Ball constructor composition (2026-09-30)

Verified: `tetherball_ball_init::construct_ball` applies the represented stores of `Tetherball::Tetherball` at `0x8039d264`. Startup calls it directly after successful ball allocation and retains `BallResources` in StartupState; the arbitrary ConstructBall stage is removed. The null-trail token is a host input read from original global `0x80601f78`, not assumed to be all ones.

Motion floats, position/anchor, flags, resource handles and asset-id sentinels are initialized. Hit type, hit direction, current direction, zone, and ball/rope matrices remain caller/allocation state because the constructor leaves them untouched. Native vtable and unused attachment bookkeeping stores are captured by the oracle but have no corresponding ECS owner here. This is an in-place constructor projection, not a complete production Runtime factory; hosts still need valid startup/session inputs.

The 32 standalone cases execute the original body and rmAngle constructors with randomized preexisting memory and four null-trail tokens. Rust comparisons cover every represented motion field by bits, preserved matrices, scene projections and all resource fields. The 24 enclosing startup cases also execute the constructor natively and compare full represented ball motion and trail stores after initialization. Tunable, asset, character and other documented host boundaries remain supplied; no interactive fidelity is claimed.

Run `py -3.14 tools/tetherball_ball_constructor_oracle.py --check`. This oracle is registered in `tools/prove.py`; its Rust test runs under `tetherball_ball_init::tests`.

## Base initialization and cleanup composition (2026-09-30)

Verified: startup now composes `Minigame::Initialize(World*)` (`0x803ab430`) and its `Minigame::Initialize()` virtual target (`0x803ab470`) directly through `initialize_base`. The actual MGTetherball vtable at `0x804dd1fc` has `0x803ab470` at +10; the oracle asserts this target. The arbitrary BaseInitialize stage is removed. World service handles are queried and copied before camera effects. Camera view-info is requested with false, the camera-manager singleton is reloaded, then camera type +2c/player 0 is reinitialized. All four controllers are queried in index order and reinitialized with control type +30. Only then is +4d set to 1 and the shared pause-menu flag +4e cleared.

`uninitialize_base` recovers `Minigame::UnInitialize` (`0x803ab524`): all four controllers return to control type 0, then +4e and +4d clear. StartupState owns the previously absent +4c/+4d bytes and preserved +4f padding; ServeState remains the sole +4e owner. The enclosing derived teardown is now composed in [TETHERBALL_CLEANUP.md](TETHERBALL_CLEANUP.md); concrete engine adapters are still required for a complete interactive activity exit.

All 24 enclosing native startup cases now execute both base bodies and a subsequent native UnInitialize, with nonzero neighboring bytes and varied preexisting +4d/+4e values. Tests compare ordered engine calls, initialized state, cleanup stores and preservation of every projected game word. Camera and controller engine internals remain synchronous service boundaries. These checks verify base cleanup composition, not complete Bevy scene re-entry or resource destruction.


StartupState now retains the existing `PlayerInitState` spawn-ownership bytes for derived cleanup. Character startup composition must populate that owner through the recovered player/AI/additional-player helpers; the recovered character helpers now populate that owner directly.

Constructor metadata and the independent +170/+180 tunable owner are documented in [TETHERBALL_CONSTRUCTOR.md](TETHERBALL_CONSTRUCTOR.md).


## Character initialization composition (2026-09-30)

Verified: the enclosing startup now calls InitializePlayer (0x8039ba00), InitializeAI (0x8039bbec), and InitializeAdditionalPlayer (0x8039bd20) through `initialize_character`. The generic Character stage is removed. `StartupServices::player_services` exposes only the synchronous character/AI allocation, binding, ability and controller engine operations required by these existing ports. Player handles, AI handles, ownership flags, player count and session mode are written by the recovered helpers to their existing owners.

The 24 enclosing native scenarios execute all three helper bodies without boundary game writes. First-player cases rotate through absent world character, matching identity/reuse, and mismatching identity/spawn. All three areas, four difficulties and both second-participant configurations remain covered. Rust compares the complete projected game words, ordered engine effects, and AI ball-pointer stores. In particular, a reused first character gets ownership flag zero while newly spawned actors get one; additional players increment session mode and AI characters do not. These flags are the same owners consumed by the derived teardown.

The cleanup oracle now restores startup hooks before each initialization and installs cleanup hooks only afterward, so cleanup's world-player query cannot mask startup reuse. Constructor projection fixtures regenerate from the resulting startup state. Existing standalone helper comparisons remain independent of these enclosing checks.

Character spawning, AI allocation/constructor engine internals and ability setup remain services; AI tuning initialization, ball asset initialization, area selection, pregame and server setup remain explicit stages. No live scene, readiness or gameplay completion is implied. The next dependency is composing these remaining recovered startup stages into a concrete host.


## Ball resource initialization composition (2026-09-30)

Verified: startup directly calls `initialize_from_placeable`, composing Tetherball::Initialize (0x8039d3d8). The generic Ball stage is removed. `StartupServices::ball_services` exposes the existing asset/pool/database engine adapter. The original four cached handles, two shadow handles, six asset IDs, acceleration modifier and +15c flag now stay in StartupState's BallResources owner for derived teardown. Ball motion and scene projections are applied by the recovered helper.

The late placeable +f4 read occurs after asset construction and tuning collection release. The existing standalone initializer still accepts an explicit height snapshot; the enclosing entry queries the live placeable through BallInitServices at the original read point. Six enclosing cases supply a synthetic engine-side height mutation on collection release; these values are dependency test inputs, not recovered gameplay constants. They distinguish the live late read from an early snapshot.

All 24 enclosing PowerPC cases now execute ball initialization unhooked, including both null/non-null shadow outcomes independently. Comparison includes ordered texture/model/pool/cached/shadow/scene effects, complete motion fields by bits, trail preservation, resource handles/IDs, flag, modifier and shared scene shadow presence. The native routine registers only the ball shadow in scene layer zero, including a null pointer; the rope shadow is constructed but is not registered here. Resource engine internals remain explicit boundaries. Cleanup and constructor projections regenerate from these composed startup states.

Remaining startup stages are area selection, AI tuning initialization, pregame and server setup. Interactive assets and rendering require a concrete Bevy host; this composition does not establish playable completion.


## AI construction and tuning composition (2026-09-30)

Verified: enclosing startup no longer delegates the Ai stage. `initialize_startup_ai` projects the newly allocated TetherballAIEntity constructor (0x80395290) into the existing AiEntity owner, binds its ball handle and resets the shared +70 charge owner, then calls `initialize_with_services` for Initialize (0x80395354). The +68 direction scale is constructor-preserved allocation state supplied through an explicit read, not a recovered zero default. This projection assumes the documented synchronous, nonreentrant append services; it is applied immediately after character append, before AI tuning or subsequent startup effects.

WorldMan's GetTetherballMinigame lookup supplies a read-only optional context using the referenced minigame's live session/dare fields. Presence is not inferred from area or session count. With no active game, Initialize preserves enable/difficulty and only wraps heading. Otherwise the class key is read before selecting the collection name; the collection is acquired even when disabled, enable is stored afterward, seven unsigned bytes are read/stored only when enabled, then the collection is released before heading is wrapped. Difficulty is the raw unsigned array index. Errors retain preceding stores, matching the port's partial-initialization policy rather than rolling back.

AiInitServices restricts the host to database keys, collection lifetime and byte-array reads. Existing corpus callers of AiEntity::initialize now use the same implementation through a decoded Database adapter, preserving inherited/out-of-range byte semantics. A missing collection, invalid selector or decoding error is reported explicitly.

The 24 enclosing scenarios execute the derived AI constructor, Initialize and the collection selector natively. The base AIEntity constructor remains an engine boundary. Cases include absent/present active game, enabled/disabled AI, dare groups, both second-participant configurations, constructor-preserved scale inputs and varied seven-byte tuning results. Comparisons cover ordered queries, enable/difficulty/heading/charge stores, ball bindings and the existing complete startup projections. Synthetic dependency results remain test inputs, not production tuning. The independent corpus-backed AI and reset/runtime tests exercise the shared Database adapter.

The formerly generic SetArea, OpenPregame and SetUpServer stages are now composed below. A concrete Bevy engine host and live scene readiness are still required before an interactive tetherball completion claim. [Activity asset preparation](TETHERBALL_ASSETS.md) supplies the first main-world asset ownership boundary.


## Area selection and pregame entry composition (2026-09-30)

Verified: startup replaces SetArea and OpenPregame stages with explicit native entry behavior. `minigame_entry::physical_area` ports PlaygroundWorld::ConvertAreaAbstractToPhysical (0x803de608): abstract 4/5/6 map to physical 0/1/2, abstract 7/8 map to 3, and all other signed values pass through. Minigame::SetArea (0x803abefc) delegates to PlaygroundWorld::SetMinigameArea (0x803e0258), which stores physical area at the active world's AreaManager +2c before PlaceableManager::SetCurrentArea receives the original abstract value. Startup's MinigameArea effect conveys those ordered engine operations without exposing mutable gameplay owners to an arbitrary stage.

`minigame_entry::open_pregame` ports Minigame::OpenPreGameScreen (0x803ab580), including PreGameInfo construction (0x803ab634). It sets the existing ServeState frontend flag owner true/true, supplies [0,0,unsigned(player_count > 1),raw dare type] to SetupPreGameHandlers with the selected minigame kind/mode zero, opens the original PreGameInstructions screen, then enables fade rendering with true/false. The count comparison is signed. Native entry never clears minigame +58; the prior test boundary that supplied that write is removed. Fresh construction owns its initialization.

The standalone oracle covers 21 area conversions and 24 pregame entries with signed limits, varied counts/kinds/dares and dirty full game allocations. All game bytes remain unchanged by native pregame entry. Rust compares instruction words, flags and ordered effects. The 24 enclosing startup cases also execute area forwarding/conversion, pregame entry and the PreGameInfo constructor unhooked. They compare the AreaManager physical word, frontend flags, and ready-byte preservation along with the existing full projections. FEManager/PreGameHandlers, PlaceableManager and fade-render internals remain engine services, not reconstructed Apt implementation or verified live presentation.

Run `py -3.14 tools/minigame_entry_oracle.py --check`; this oracle and `minigame_entry::tests` are registered in tools/prove.py. The last generic startup stage is SetUpServer. A concrete Bevy host, readiness gating and interactive entry/restart/exit still remain to be completed.


## Live server setup composition (2026-09-30)

Verified: the last generic startup stage, SetUpServer, is removed. `tetherball_server::setup_server` runs SetUpServer (0x8039b1b0) directly against the live Runtime owners and synchronous ServerServices. Server choice and camera arithmetic share recovered helpers with the existing snapshot reset port rather than duplicating those rules. The enclosing startup trait no longer permits arbitrary helper writes to Runtime/StartupState.

The RNG is consumed first on every call with the native [0,1] result domain. Signed base count one forces player zero; counts greater than one alternate using +445; zero/negative counts use the random result. Active server/receiver/focus fields are stored before camera lookup. The target is the transformed local +/-0.5 X offset with no separate world-position addition. Camera direction is then set before side flags and AI start values are stored. Player animations are initialized using their recovered body before requesting server state 58 and the receiver's updated animation.

Marker count is captured once after both animation requests and compared as signed. Only marker ID 63 triggers a matrix lookup; the last match wins, and no match preserves the previous +278 handle. Stationary (+0 or -0 angular velocity) balls have their start angle wrapped and execute the represented Grab stores (0x8039e5a8). Moving and unordered/NaN velocities skip Grab. Grab sets character/matrix ownership, desired radius and flags, clears both angular velocities, then processes three trails in order, reloading the global null token for each comparison and after each destruction. Legacy ResetState trail mirrors are not extra live owners; the existing reset boundary refreshes them from BallScene.

The 24 enclosing native cases execute SetUpServer and Grab without hooks on either gameplay body. A further 64 native live server transitions cover signed count limits, both alternation values, random consumption, no/one/multiple matching markers, negative marker counts, cached-marker preservation, zero/moving/NaN velocity and all trail-null combinations. The supplied animation engine hook updates graph count when SetNextAnimState returns; this synthetic query dependency ensures the live port reads marker metadata after the state requests, without claiming native animation internals are reconstructed. Tests compare projected game words, complete ball motion by bits, attachment handles, trails, camera target and ordered engine effects.

The original snapshot ResetRound/ResetMiniGame ports remain available and share selection/camera arithmetic; their complete live Bevy reset adapter still needs validation against actual animation/scene resources. All generic enclosing startup stages are now composed. World/character/controller, renderer, database and frontend engine adapters, real readiness, playable entry/restart/exit and the full activity outcome remain unfinished. No game-completion claim follows from the native helper comparisons.

Run `py -3.14 tools/tetherball_server_oracle.py --check`. The oracle is registered in tools/prove.py and its Rust comparison runs under tetherball_startup::tests.
