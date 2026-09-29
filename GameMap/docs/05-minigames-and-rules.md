# 05 — Minigames, tournament rules and result scoring

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF/DB, **[I]** inferred, **[U]** unresolved.
Verified models: [`reference/multiplayer_mode.py`](../reference/multiplayer_mode.py) (`MultiplayerMode`, `post_game_awards`) — byte-exact against the original code
(`tools/verify_multiplayer.py`: 66,264 comparisons; `tools/verify_postgame.py`: 3,000 randomized results).
Per-game generated pages: [dodgeball](subsystems/game-mg-dodgeball.md) · [footie](subsystems/game-mg-footie.md) · [RC cars](subsystems/game-mg-rccars.md) ·
[tetherball](subsystems/game-mg-tetherball.md) · [wall ball](subsystems/game-mg-wallball.md) · [paper airplanes](subsystems/game-mg-paperairplanes.md) ·
[dart shootout](subsystems/game-mg-dartshootout.md) · [free throw / dribbling](subsystems/game-mg-freethrow.md) · [bug hunt](subsystems/game-mg-microbug.md).

## The catalogue

`Enums::MinigameType` is the index into `kMiniGameString` (a name table in the ELF) **[D]**; it is the value the result scoring switches on **[C]**:

| id | name | class | code (units, functions, bytes) | audio bank / music | controls CSV (`ControlType`) | database table(s) |
|---:|---|---|---|---|---|---|
| 0 | DartShootout (+ Quick Draw, boss encounters) | `MGDartShootout` | 21 / 426 / 82,380 | `MG_DartShootout.abk` / `dartshootout.asf` | `dartshootout`(3), `quickdraw`(8) | `mg_dartshootout` (160 rows), `minigames` |
| 1 | RCCar | `MGRcCars` (two scenes) | 18 / 415 / 109,684 | `MG_RC_Cars_OffRoad.abk`, `MG_RC_Cars_Touring.abk` / `rc_offroad.asf`, `rc_touring.asf` | `rccars`(9) | `mg_rccars` (47) |
| 2 | Tetherball | `MGTetherball` | 3 / 129 / 39,144 | `MG_Tetherball.abk` / `tetherball.asf` | `tetherball`(11) | `mg_tetherball` (6) |
| 3 | Dodgeball | `MGDodgeball` | 9 / 347 / 66,476 | `MG_Dodgeball.abk` / `dodgeball.asf` | `dodgeball`(4) | `mg_dodgeball` (7) |
| 4 | Footie | `MGFootie` | 10 / 398 / 73,332 | `MG_Footie.abk` / `footie.asf` | `footie`(6) | `mg_footie` (7) |
| 5 | PaperAirplanes | `MGPaperAirplanes` | 9 / 209 / 59,472 | `MG_PaperAirplanes.abk` / `paperairplanes.asf` | `paperairplanes`(7) | `mg_paperairplanes` (14) |
| 6 | WallBall | `MGWallball` | 8 / 250 / 50,840 | `MG_WallBall.abk` / `wallball.asf` | `wallball`(12) | `mg_wallball` (7) |
| — (microgames) | Free Throw / "21" | `MGFreeThrow` | 7 / 114 / 26,456 | `MW_FreeThrow.abk`, `MW_BBall_Dribble.abk` | `freethrow`(13), `mg21`(1), `dribbling`(5) | `microgame_freethrow`, `microgame_dribbling` |
| — (microgames) | Bug Hunt | `MicroBugHuntManager`, `MicroBug` | 3 / 61 / 12,048 | `MW_BugCatch.abk` | `bughunt`(2) | `microgame_bughunt` |
| — (microgames) | High Five | `HighFiveManager` | (in free-throw unit group) | — | — | `microgame_highfives` |

Row counts are the number of *collections* per class in the supplied `db.vlt` (see [07](07-data-formats-and-hashes.md)). Free Throw is constructed by the same `StartMinigameFadeComplete` switch as the seven types
but is not part of the seven-entry name table; it and the other microgames are launched from the world (`WorldMan::IsInMicrogame`, `microgames.viv`) **[D]/[I]**.
`GameState`'s `Playground` state hosts both the free-roam world and any running minigame.

## Lifecycle **[C]**

```
menu (PreGame)  ── FE2MP ──▶ WorldMan::StartMinigame(MGID, level, MiniGameDifficultyLevel, const Teams&, const int* rules | MiniGameDareType)
  StartMinigame(key: u64, MiniGameDareType) : reads a row of DB class `minigames` (ints for id/level/difficulty, ai_opponents / ai_teammates arrays, refs) and builds `Teams`
  → PlaygroundWorld::StartMinigameFadeInEffect, Audio::UnloadData/StopMusic/StopAmbience
WorldMan::StartMinigameFadeComplete (1,812 B):  GetMinigameType(MGID); save the player's conversation position/rotation; UnSpawnCurrentArea/CleanCurrentArea;
  allocate the subclass by type (0 DartShootout, 1 RcCars, 2 Tetherball, 3 Dodgeball, 4 Footie, 5 PaperAirplanes, 6 Wallball, FreeThrow) → `Minigame::SetUpTeams(teams)` → AddEntity to the scene
per frame:  WorldMan::Update → Minigame::Update(int dt) (+ virtual overrides) ; HUD calls into APT via MinigameHandlers
end:  Minigame::OpenPostGameScreen(type, PostGameInfo*)  [results → MultiplayerMode]  →  "PostGame" APT screen  →  PostGame_OnDone / OnReplay
WorldMan::EndMinigame(bool):  fade out, remove the minigame entity, MinigameHandlers::ClearMinigameHandlers, SetNewState(MP2FE), respawn the area,
  restore music/ambience, restore the player's position/rotation, CharacterProfile::EndMiniGame(SinglePlayerGameResult), Character::SwitchToLocalControl
```

`FE2MP` (`GameState`) is what calls `StartMinigame`; `MP2FE` calls `MultiplayerMode::EndMultiplayerGame`. **[C]**

### The `Minigame` base class (`minigame.cpp`) **[C]**

Virtual interface (7 slots, `data/vtables.tsv`): `~Minigame`, `UnInitialize`, `HandleControllerDisconnect` (base = a bare `blr` at `0x803e1694`), `Initialize(const World*)`, `OnPauseContinue`,
`SetDareType(MiniGameDareType)`, `GetArrayIndexFromDiffLevel(MiniGameDifficultyLevel)`. Every minigame overrides slots 0, 1 and 3; several reuse the base for the rest (e.g. Footie, Tetherball, Wallball and FreeThrow
use the base `HandleControllerDisconnect`; only Tetherball, PaperAirplanes and DartShootout override `SetDareType`).
Non-virtual base API: `SetDifficulty`, `SetLevel`, `SetUpTeams`, `SetRules(const int*)`, `Update(int)`, `Draw`, `OpenPreGameScreen/ClosePreGameScreen`, `OpenPostGameScreen/ClosePostGameScreen`, `OpenPauseMenu/ClosePauseMenu`,
`OnReplay/OnDone/OnPlay/OnPauseMenuLoaded/OnPauseQuit/OnPauseReset`, `BuildPlacementList1vs1 / FFA / TeamVsTeam`, `SetArea/RestoreArea`. `Teams` is a 0x88-byte value type (first word = number of players). **[D]**

## Tournament state: `MultiplayerMode` — **verified** **[C]**

A singleton (`0x128` bytes, global pointer at `0x8060204c`) that keeps points, wins and ranks across rounds. Full layout, quirks and a byte-exact Python model are in `reference/multiplayer_mode.py`:

- `StartFreePlay()` resets wins; `StartPointSeries(rounds)` also resets points, sets the point-series flag and `rounds_left`.
- `AddRoundResults(p0,p1,p2,p3)` adds points (players 2 and 3 only when the argument ≠ −1), remembers the round's points, **re-ranks** (rank = number of players *strictly* ahead), breaks ties using the previous rank
  (the previously better-ranked player keeps the better rank; mutual ties stay tied), counts the round and decrements `rounds_left` in a point series.
- `AddWinResults(w1,w2)` credits up to two winners the same way (wins + win rank), records `last_winners` and counts the round.
- Queries: `GetPlayerRank` (points rank in a point series, else win rank), `GetPlayerNumByRank`, `GetPointTotal`, `GetWinTotal`, `WonLastGame`, `GetPlayerPointsInThisMatch`, `GetNumRoundsLeft`.
- Verified quirks: `SetupMultiplayerGame` skips the second word of the `Teams` copy; `SetLastPlacement(a,b,c,d)` writes both `c` and `d` to the same slot (`d` wins if ≠ −1) and never writes the fourth slot.

### Result scoring: `Minigame::OpenPostGameScreen` — **verified** **[C]**

The only code that writes results into `MultiplayerMode`. For a minigame with more than one player:

| type(s) | rule (points only in a point series; wins always) |
|---|---|
| 2 Tetherball, 6 WallBall (1v1) | winner **+50**; recorded as the win; `last_placement` = (0,1) or (1,0) |
| 3 Dodgeball, 4 Footie (team vs team) | **every member of the winning team +50** and credited a win (the first two members go to `AddWinResults`); `last_placement` from the info block |
| 0 DartShootout, 1 RCCar, 5 PaperAirplanes (free-for-all) | placement 0 → **+50** (and the win), 1 → **+25**, 2 → **+10**, others 0 |

It also sets `PostGameInfo+0x50 = 1` and `+0x114 = −1` when a multiplayer session is active, otherwise copies the minigame's field `+0x48`; then calls `PostGameHandlers::SetupPostGameHandlers` and opens the APT screen `"PostGame"`.
The `PostGame_*`/`PreGame_*` LV handlers read the tournament back through the accessors above. Single-player results go through `CharacterProfile::EndMiniGame(SinglePlayerGameResult)` (stickers, marbles, unlocks — **[U]**, decode next).

## Difficulty, dares and balance data

- `Enums::MiniGameDifficultyLevel`, `Enums::MiniGameDareType` (three dares per game: `minigame_dare_1..3` fields), `Enums::MiniGameMultiplayerMode` and the game-specific enums
  (`TetherBallGameplayTypes`, `DodgeBallGameplayTypes`, `MGDB_VersusSetupTypes`, `PaperAirplanePlaneTypes/Modes`, `MGDS_*`, `DSTarget*`, `MGPA_*`) exist as **database types**, not as code tables.
  Their member names are stored in `db.bin`'s type section (hashed) — see [07](07-data-formats-and-hashes.md) — so the numeric values are decodable from the data with the verified hash.
- Almost all balance/AI numbers are **not in the code**: the AI is parameterised through the attribute DB (`ai_power_throw_percentage`, `ai_react_to_fake_percentage`, `ai_hit_percentage`, …), and scoring/timing constants through
  `mg_rccars` (`sprint_laptime_level1..4`, `score_first_place_points`, upgrades), `mg_tetherball` (`ball_basehitspeed`, `powerhit_points`, `game_duration`), `mg_footie` (`normal_goals_to_win`, `keeper_challenge_*_saves`), `mg_wallball`
  (`ball_speed_levels`, `rallys_per_speed_level`), `mg_paperairplanes` (`bonus_treshold`, `max_duration`), `mg_dartshootout` (target waves: `Health`, `ActiveSpeed`, `Lifetime`, …). **[D]** Reading the *values* is the next data task.

## AI structure (all minigames) **[D]**

Each game pairs an `AI<Game>Entity` with `…Compulsion` classes (e.g. Dodgeball: `Throw`, `React`, `ReactToFake`, `Boost`, `Idle`), each with `HasExpired(AIThinkLoD)` and `Deactivate` virtuals — a prioritised "compulsion" AI driven by the
`AncientEvil` manager (`ai` subsystem). The tuning percentages above are read from the DB by the `…AIDifficultyInfo` structs. Behaviour of each compulsion is **[U]**; start at `DodgeballThrowCompulsion` / `FootieAIEntity` in the generated pages.

## Reconstruction guidance

1. Implement `MultiplayerMode` and `post_game_awards` first — they are done and verified; port them to Rust one-to-one (keep the quirks or document each deliberate deviation).
2. Implement the lifecycle and the `Minigame` trait exactly as above; make each game a module behind the 7-slot interface plus `Update(dt_ms)`.
3. Read the `mg_*` tables (see [07](07-data-formats-and-hashes.md)) rather than hard-coding constants.
4. Then decode one game end to end. The smallest with clear rules is **Tetherball** (3 units, 129 functions) — see [09](09-roadmap.md).
