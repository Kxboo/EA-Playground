# 04 — Front end, menus and the APT ↔ native bridge

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF, **[I]** inferred, **[U]** unresolved.
Machine-readable: [`data/apt_handlers.tsv`](../data/apt_handlers.tsv) (132 bindings), [`data/fe_state_transitions.tsv`](../data/fe_state_transitions.tsv),
[`data/traces/`](../data/traces/), [`data/enums/fe_game_state_update_functions.tsv`](../data/enums/fe_game_state_update_functions.tsv).
Generated pages: [game-frontend](subsystems/game-frontend.md) (22 units, 618 functions).

## Architecture

```
APT movie (Flash-like: AptMovie / AptActionInterpreter, ActionScript bytecode)      ←  data/fe/*.apt + .const + .gsh
   │  fscommand("Name", args)      ─▶  AIP::FSCommandHandler   (DoJobFS)   native reacts to a menu event
   │  loadVariables("Name", ...)   ─▶  AIP::LoadVariablesHandler (DoJobLV) native fills variables the menu reads
   ▲  AptCallFunction("ReplaceScreen", "_root", "Title")   native calls a function on the movie's _root
FEManager (front-end state machine, screen stack, kid-select 3-D scene)  ⇄  *Handlers singletons (one pair per screen family)
```

- The UI is the **EA "APT"** runtime (`Apt*.cpp`, `aip.cpp`, `composer.cpp`, `decomposer.cpp`, ~289 KB of middleware). Menus are authored as APT movies; native code never draws menu widgets
  itself. `AIP` ("APT Interface Protocol", **[I]** name) is the command bridge: `AIP::CmdDecomposer` reads arguments sent by the movie (`GetIntByName`, `GetStringByName`,
  `GetIntArrayByName`), `AIP::CmdComposer` writes results back (`SetIntByName`, `SetStringByName`). **[C]**
- Each handler class registers its names with a sequential *job index* in its constructor (`AIP::RegisterFSHandler(name, this, index)`) and dispatches on that index in `DoJobFS/DoJobLV`
  (jump table or compare tree). All **132** registrations were extracted, and each `DoJob*` path was executed in a small emulator to find the native target (`tools/handlers.py`). **[C]**
- Screens are opened by name through the movie: `FEManager::OpenAptScreen(char*)`, `OpenAptOverlay`, `ReplaceAptScreen`, `CloseAptScreen/Overlay`, `ClearScreenStack`
  (all thin wrappers over `AptCallFunction("OpenScreen"|"OpenOverlay"|"ReplaceScreen"|"CloseScreen"|"CloseOverlay"|"ClearScreenStack", "_root", name)`). A screen stack lives in the movie. **[C]**
- `GlobalFSHandlers::ScreenReady` and `ScreenLeaving` are called by every screen movie on load/exit, and `GlobalHandlers::SetCursorVisibility(int,int)` shows/hides the pointer. **[C]**

## Two front-end state variables

`FEManager` has two states (fields `+0x3c` and `+0x40`):

**`eFrontendState`** (`SetState`, `+0x3c`) selects *which handler families exist*. `EnterState` creates them **[C]**:

| value | `EnterState` creates | role ([I]) |
|---:|---|---|
| 0 | `GlobalHandlers`, `LoadingScreenHandlers` | boot / loading |
| 1 | `GlobalHandlers`, `MainMenuHandlers`, `CreditScreenHandlers`, `ProfileHandlers`, `MultiPlayerHandlers`; sets each controller's context (`Controller::SetCurrentControllerState`) | interactive front end |
| 2 | (nothing) | transition |
| 3 | `EndTourney`, `PaperAirplane`, `PauseMenu`, `PreGame`, `PostGame`, `StickerBook`, `StickerStore`, `BossEndGame`, `ReportCard`, `Minigame`, `Conversation`, `WorldHud` handlers | in the 3-D world (HUD, pause, minigame UI) |
| 4 | `LoadingScreenHandlers` | loading overlay |

`FEManager::Update` only runs the per-state logic when `eFrontendState == 1`; it always ticks `AIP::Update(dt)` (except in state 3 when the world HUD isn't loaded) and resets the background colour. **[C]**

**`eFrontEndGameState`** (`SetFEGameState`, `+0x40`) is the menu flow. `EnterFEGameState` (`0x803264e4`, 3 KB) / `LeaveFEGameState` (`0x80326380`) do the entry/exit work and
`Update` (`0x80324390`) dispatches a per-frame function through a jump table (`0x804d6e2c`, index = state + 1). **[C]**

| value | per-frame update | what `EnterFEGameState` does (traced) |
|---:|---|---|
| −1 | (sentinel) | `UnspawnAllSelectableCharacters`, then **`GameState::SetNewState(FE2PG)`** — this is how the world is entered |
| 0 | `UpdateIntroBootFlow` | — |
| 1 | `UpdateIntro` | — |
| 2 | — | `Audio::PlayMusic`, `ReplaceScreen("Title")` |
| 3 | — | `ReplaceScreen("Profile")` (profile/save-slot screen) |
| 4 | `UpdateReturnToSelectKid` | drop kids to ground height, AI walk to positions (`SetAIControl`) |
| 5 | `UpdateSelectAKid` | load DB tables `character_select` / `character`, spawn the selectable kids, `PhysicsManager::DisableCharacterCharacterCollisions`, player-indicator `TarManager` |
| 9 | `UpdateKidSelected` | `Multiplayer_DisableInput`, un-init kids, kid walks to the confirm position |
| 10 | `UpdateKeyboard` | opens the Wii keyboard (`TRC::OpenKeyboard`) with `T_NameKid` |
| 11 | `UpdateConfirmKid` | `ReplaceScreen("ConfirmKid")` |
| 12 | `UpdateConfirmedKid` | `ClearScreenStack`, `CloseScreen`, `SelectableCharacter::Celebrate`, re-enable character collisions |
| 13 | — | `OpenAptScreen("MainMenu")`, `ClearScreenStack`, `PlayMusic`, unregister extra controllers, despawn selectable kids |
| 15 | `UpdateMPNumPlayers` | checks which Wii Remotes are connected (`TRC::IsCoreControllerConnected` ×4) |
| 16 | `UpdateMPSelectAKid` | DB tables `character_select`/`character`/`multi_player`, `characterlist`, `positions`; allocates extra character slots |
| 17 | `UpdateMPCharactersSelected` | shows next button / help text / cursor for multiplayer setup |
| 19 | — | `Audio::PlayMusic` |
| 20 | (wait for fade) | `FadeToColourEffect::StartFadeIn`, `Audio::StopMusic`; each frame `IsFadeEffectComplete()` → SFX → `Leave(-1)`, `Enter(-1)` (starts the world) |

Values 6, 7, 8, 14, 18 have no update/enter work in the traced paths; they are set by `GlobalFSHandlers::ScreenReady` (18, 19) and the profile handlers (6). Full traces:
[`data/traces/FEManager_EnterFEGameState.tsv`](../data/traces/FEManager_EnterFEGameState.tsv). The value names are inferred from the screens they open (**[I]**); the values themselves are **[C]**.

### Who sets `eFrontEndGameState` (all call sites) **[C]**

| value | caller | call site |
|---:|---|---|
| -1 | `MultiPlayerFSHandlers::SetTeams` | `8031ab1c` |
| -1 | `MultiPlayerFSHandlers::LaunchNonTeamMiniGame` | `8031aeac` |
| 0 | `GameState::STATEFN_UPDATE_BootFlow` | `803ad094` |
| 10 | `MultiPlayerFSHandlers::SetPlayerOnNext` | `8031a510` |
| 12 | `ProfileHandlers::TRCOperationCallback` | `80320404` |
| 13 | `GlobalFSHandlers::ScreenReady` | `80316aa4` |
| 13 | `MultiPlayerFSHandlers::DoJobFS` | `8031a288` |
| 13 | `ProfileHandlers::TRCOperationCallback` | `803204ac` |
| 13 | `GameState::STATEFN_UPDATE_PG2FE` | `803ad3bc` |
| 13 | `GameState::STATEFN_UPDATE_MP2FE` | `803ad994` |
| 13 | `TRC::ControllerDisconnectOptionsResult` | `803b3bc4` |
| 15 | `MultiPlayerLVHandlers::HowManyPlayersOnLoad` | `8031b728` |
| 16 | `MultiPlayerFSHandlers::SetNumberPlayers` | `8031a38c` |
| 16 | `MultiPlayerFSHandlers::SetPlayerOnBack` | `8031a458` |
| 16 | `MultiPlayerLVHandlers::CharacterSetupOnLoad` | `8031b800` |
| 18 | `GlobalFSHandlers::ScreenReady` | `80316af4` |
| 19 | `GlobalFSHandlers::ScreenReady` | `80316b18` |
| 20 | `MainMenuFSHandlers::MainMenu_OnSelect` | `803174a0` |
| 20 | `GameState::STATEFN_UPDATE_FrontEnd` | `803ae164` |
| 20 | `GameState::STATEFN_UPDATE_FrontEnd` | `803ae1bc` |
| 3 | `ProfileLVHandlers::ProfileOnLoad` | `80320ed4` |
| 4 | `MultiPlayerFSHandlers::SetPlayerOnBack` | `8031a414` |
| 4 | `ProfileFSHandlers::KidConfirmOnResponse` | `80320984` |
| 5 | `ProfileFSHandlers::ProfileOnSelect` | `803207a4` |
| 6 | `ProfileFSHandlers::DoJobFS` | `8032071c` |

`SetState(eFrontendState)` callers:

| value | caller |
|---:|---|
| 0 | `GameState::STATEFN_UPDATE_BootFlow` |
| 1 | `GameState::STATEFN_UPDATE_PG2FE` |
| 1 | `GameState::STATEFN_UPDATE_MP2FE` |
| 1 | `GameState::STATEFN_UPDATE_Boot2FE` |
| 2 | `GameState::STATEFN_UPDATE_FE2MP` |
| 2 | `GameState::STATEFN_UPDATE_FE2PG` |
| 3 | `GameState::STATEFN_UPDATE_FE2MP` |
| 3 | `GameState::STATEFN_UPDATE_FE2PG` |
| 4 | `GameState::STATEFN_UPDATE_PG2FE` |
| 4 | `GameState::STATEFN_UPDATE_MP2FE` |

Reading the two tables together gives the menu graph: `BootFlow`(0) → Title(2) → Profile(3) → SelectAKid(5) → KidSelected(9) → Keyboard(10) ↔ ConfirmKid(11) → ConfirmedKid(12) → MainMenu(13);
MainMenu → multiplayer setup 15 → 16 → 17 or → 20 (start world/minigame); `MultiPlayer_NumPlayersBack` returns to 13; `GameState` returns to 13 after `PG2FE`, `MP2FE` and a controller-disconnect quit. **[C]/[I]**

### The selectable-kid scene

The character-select screens are a real 3-D scene inside the front end: `SelectableCharacter` (`selectablecharacter.cpp`, 26 methods) owns a spawned `Character` with head tracking, highlight, select/unselect,
celebration (`Celebrate(SelectCharacterCelebrationType)`), an AI walk (`SetAIControl`), and target planes (`CalculateTargetPlane`). It is driven by the DB tables `character_select`, `character`, `multi_player`
(fields `characterlist`, `positions`, `unlockLinks`) and the `ANIM_SELECTKID_*` animation states; input comes from the `EVENT_SELECTKID_SELECT/CANCEL` and `EVENT_TEMP_MULTIPLAYER_*` events. **[D]**

## Handler bindings (menu → native and native → menu variables)

Complete list with native targets in [`data/apt_handlers.tsv`](../data/apt_handlers.tsv). Names are the exact strings the movies use. **[C]**

| handler class | bindings |
|---|---|
| `AIP::AIPHandler` | FS `StartAPTRender`, FS `StopAPTRender`, FS `SetAPTRenderCallback`, LV `GetBattery`, LV `GetLocalizedString`, LV `GetAPTRenderCallback` |
| `BossEndGameFSHandlers` | FS `EndGame_OnStickerSelect`, FS `EndGame_QuitResponse`, FS `EndGame_OnFinished` |
| `BossEndGameHandlers` | LV `EndGame_OnLoad` |
| `ConversationHandlers` | FS `ConversationOnButtonNext` |
| `ConversationLVHandlers` | LV `Conversation_GetName`, LV `Conversation_GetDialogueText`, LV `Conversation_GetResponses`, LV `Conversation_OnPlayerSelect` |
| `CreditScreenHandlers` | LV `ScrCreditsInit` |
| `EndTourneyHandlers` | FS `EndTourney_OnButtonClick`, LV `EndTourney_OnLoad` |
| `GlobalFSHandlers` | FS `PlayAEMSsfx`, FS `ActivateRumble`, FS `ScreenReady`, FS `ScreenLeaving`, FS `ReturnCursorVisibility` |
| `GlobalLVHandlers` | LV `GetStartScreenFromMain`, LV `GetLocale`, LV `GetLocaleFE`, LV `GetAspectRatio`, LV `PG_GetBuildType` |
| `LoadingScreenHandlers` | FS `LoadingStartPressed` |
| `LoadingScreenLVHandlers` | LV `LoadingGetBtnLabels`, LV `LoadingGetStartOrBack` |
| `MainMenuFSHandlers` | FS `MainMenu_OnSelect`, FS `Title_OnNextScreen` |
| `MinigameFSHandlers` | FS `Hud_LoadComplete`, FS `GameStartAnim_Complete` |
| `MinigameLVHandlers` | LV `GetNumberOfHuds`, LV `GetGameRules`, LV `PaperAirPlanes_GetCheckPoints`, LV `Footie_IsSaveDare`, LV `Counter_GetText` |
| `MultiPlayerFSHandlers` | FS `MultiPlayer_SetNumberPlayers`, FS `MultiPlayer_NumPlayersBack`, FS `MultiPlayer_SetPlayerAvatars`, FS `PlayerSetup_Exit`, FS `PlayerSetup_OnNext`, FS `MultiPlayer_SetGameStyle`, FS `MultiPlayer_SetNumRounds`, FS `RoundSelect_OnBack`, FS `MultiPlayer_SetMinigame`, FS `MultiPlayer_SetRulesType`, FS `MultiPlayer_SetGameRules`, FS `MultiPlayer_SetTeams`, FS `QuickPlay_SaveScores` |
| `MultiPlayerLVHandlers` | LV `MultiPlayer_HowManyPlayersOnLoad`, LV `MultiPlayer_SetUpOnLoad`, LV `MultiPlayer_SelectGameOnLoad`, LV `MultiPlayer_GameRulesOnLoad`, LV `MultiPlayer_GetGameRule`, LV `MultiPlayer_TeamSelectOnLoad` |
| `PaperAirplaneHandlers` | FS `SelectPlane_Exit`, LV `SelectPlane_OnLoad` |
| `PauseMenuFSHandlers` | FS `Pause_OnLoadComplete`, FS `Pause_OnKeepPlaying`, FS `Pause_OnRestart`, FS `Pause_OnQuit` |
| `PostGameFSHandlers` | FS `PostGame_OnReplay`, FS `PostGame_OnDone`, FS `MultiPlayer_PostGameOnSelect`, FS `PostGame_OnButtonClick` |
| `PostGameLVHandlers` | LV `PostGame_OnLoad`, LV `PostGame_GetSPInfo`, LV `PostGame_GetStats`, LV `PostGame_GetMPInfo`, LV `PostGame_IsLastTourneyGame`, LV `PostGame_IsNextGameLastTourneyGame`, LV `PostGame_GetPointsWon`, LV `PostGame_GetTotalPoints`, LV `PostGame_GetTeamSetup`, LV `PostGame_GetGamesWon`, LV `PostGame_GetTotalGamesWon` |
| `PreGameHandlers` | FS `PreGame_OnPlay` |
| `PreGameLVHandlers` | LV `PreGame_OnLoad`, LV `PreGame_GetSinglePlayerInfo`, LV `PreGame_GetMultiPlayerInfo`, LV `PreGame_GetDareText` |
| `ProfileFSHandlers` | FS `ProfileSelect_OnSelect`, FS `ProfileSelect_OnErase`, FS `SelectKid_OnFilterSelect`, FS `KidConfirm_OnResponse` |
| `ProfileLVHandlers` | LV `ProfileSelect_OnLoad`, LV `SelectKid_OnLoad`, LV `KidConfirm_GetDialogue` |
| `ReportCardHandlers` | FS `ReportCard_OnClose`, LV `ReportCard_OnLoad` |
| `StickerBookFSHandlers` | FS `StickerBook_RewardSelected`, FS `StickerBook_LayoutSave`, FS `StickerBook_LaunchMiniGame`, FS `StickerBook_Exit`, FS `StickerBookCover_OnWorld`, FS `StickerBookCover_OnQuit`, FS `StickerBookCover_OnMusic`, FS `StickerBookCover_OnSave`, FS `StickerBookCover_OnSelect` |
| `StickerBookLVHandlers` | LV `StickerBook_RewardLoad`, LV `StickerBook_RewardGetStickers`, LV `StickerBook_GetSticker`, LV `StickerBook_LayoutLoad`, LV `StickerBook_GetPlayerName`, LV `StickerBookCover_LoadLayout`, LV `StickerBookCover_IsGameDirty` |
| `StickerStoreFSHandlers` | FS `StickerStoreCover_OnExit`, FS `StickerStoreCover_OnGameSelect`, FS `StickerStorePage_OnClick`, FS `StickerStorePage_OnBuy`, FS `StickerStore_SetMinigame`, FS `PurchaseConfirm_Answer` |
| `StickerStoreLVHandlers` | LV `StickerStoreCover_OnLoad`, LV `StickerStorePage_OnLoad`, LV `PurchaseConfirm_OnLoad` |
| `TRCApt` | FS `TRCSetPopupOption`, FS `TRCSetKeyboardInfo`, FS `TRCCancelKeyboard`, FS `TRCNISClosed`, FS `TRCSave`, FS `TRCCancelSave`, FS `TRCLoad`, FS `TRCCancelLoad`, FS `TRCDelete`, FS `TRCCancelDelete` |
| `WorldHudFSHandlers` | FS `WorldHud_LoadComplete`, FS `InfoDialogue_OnButtonClick` |
| `WorldHudHandlers` | LV `InfoDialogue_GetText` |

Notable native targets: `MultiPlayer_SetGameRules/SetTeams/SetMinigame/SetNumRounds` feed `MultiplayerMode` (see [05](05-minigames-and-rules.md)); `PostGame_OnReplay/OnDone` call `Minigame::OnReplay/OnDone` on the active minigame;
`EndGame_OnStickerSelect` → `CharacterProfile::StartBossBattle(MinigameType)`; `StickerStore*` → `CharacterProfile` purchase/close; `TRCSave/Load/Delete` → `TRCApt::DoFS*` (Wii save-file UI);
`PlayAEMSsfx(nFEsfxID)` plays a front-end sound bank effect by name.

### HUD traffic (native → movie) — `MinigameHandlers`

The in-game HUD is also an APT movie. `MinigameHandlers` (`MinigameHandlers.cpp`, 89 functions) exposes setters the minigames call every frame: `Timer_SetValue/Visible`, `Scoreboard_*`, `Round_SetVisible`, `Position_*`,
`LapCounter_*`, `BoostMeter_SetValue`, `MegaMeter_*`, `HealthBar_/BossHealthBar_*`, `MiniMap_SetValue`, `RcCars_Hint/SetDareMode/PipOverlay*`, `PaperAirplanes_*`, `DartShootout_*`, `Footie` goal animation, `HitCounter/SavesCounter/BallMeter_Update`,
`GameStartAnim_Play/Reset`, `PauseCountdownAnim`, `WinLose_SetVisible`, `TeamWin_SetVisible`, … Each minigame's page under [`subsystems/`](subsystems/README.md) lists the calls it uses. **[D]**

## Reconstruction guidance

1. Model the APT bridge as a Rust trait pair (`FsHandler::do_job(index, &Args)`, `LvHandler::do_job(index, &Args, &mut Out)`) registered by the *exact names* above; then any APT/Flash-compatible renderer (or a re-authored Bevy UI) can drive the same
   native logic. The bindings table is the complete contract between menus and game code.
2. Recreate `eFrontendState`/`eFrontEndGameState` and the transition graph from the tables above before any visuals; the flow is small (≈20 states).
3. The APT timelines, shapes, masks and `.const` pools are the open decoding task listed in `FINDINGS.md`; nothing here depends on them except drawing.
4. Screens are addressed by *name* (`Title`, `Profile`, `ConfirmKid`, `MainMenu`, …); the movie files that carry those names live under `data/fe/`. **[U]** for the complete screen list — send a directory listing of `data/fe/` to close this.
