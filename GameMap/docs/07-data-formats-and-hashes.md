# 07 — Data formats, hashes and how the code reaches the data

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF/DB, **[I]** inferred, **[U]** unresolved.
Code: [`tools/hashes.py`](../tools/hashes.py), [`tools/vlt_probe.py`](../tools/vlt_probe.py), [`tools/check_controls_csv.py`](../tools/check_controls_csv.py).
Data: [`data/vlt_schema.tsv`](../data/vlt_schema.tsv), [`data/string_tables.tsv`](../data/string_tables.tsv), [asset name tables](asset-name-tables.md).

## 1. Two hashes, recovered and verified **[C]**

| use | function | algorithm |
|---|---|---|
| localisation keys (`string.idx`) | `ComputeHash(const char*)` `0x803ae608` | `h = 0xFFFFFFFF; for each *signed* byte c: h = h*33 + c` (mod 2³²) |
| attribute/VLT keys (`Attrib::StringToKey` = `StringHash64`) | `0x802d8dd4` → `hash64` `0x802d86b0` | Bob Jenkins **lookup8** `hash()` (24-byte blocks, `mix64` shifts 43/9/8/38/23/5/35/49/11/12/18/22, golden constant `0x9E3779B97F4A7C13`, length folded into `c`), **seed `0xABCDEF0011223344`**; the empty/null string hashes to `0` |

- Both are byte-exact against the original PowerPC code (`tools/verify_hashes.py` emulates the real functions: 309 strings including the 24- and 48-byte block edges). **[C]**
- The 64-bit hash was additionally **validated on real game data**: hashing the string pool of the supplied `db.bin` and the strings of the ELF resolves the keys inside `db.vlt` (below): 303 distinct non-empty strings from the pool alone,
  30 of the 32 exported class names, and the `Attrib::ClassLoadData`/`Attrib::CollectionLoadData` type keys. **[C]**
- FINDINGS.md listed the `.idx` and VLT key hashes as unknown; these are the answers. Reference implementations are in `tools/hashes.py` (`locale_hash`, `attrib_hash64`).

## 2. Localisation: `string.idx` + `<LANG>.loc` **[C]** (code) / **[U]** (files not inspected)

`Locale::Locale(eLocaleDb, eLanguageId)` (`0x803ae660`) builds the path `data\locale\` + language name + `.loc` from a 24-byte-stride language table at `0x804422b8`
(`ENG_US`, `FRE_FR`, `GER_DE`, `SPA_ES`, `ITA_IT`; further `.loc` names exist as strings: `DUT_NL`, `POR_PT`, `RUS_RU`, `JPN_JP`, `KOR_KR`, `CHT_CN`, `SPA_MX`), loads it with `FILE_loadatz`, and calls
`LOCALE_create`. Once per run it loads `data\locale\string.idx`. `Locale::GetString(const char* id)` = `LOCALE_getstr(loc, index)` where `index` = a binary search (`bsearch`, unsigned compare on the first word) of the
**sorted array of 8-byte `{ hash, index }` records starting 8 bytes into `string.idx`** (`count` at word 1) for `ComputeHash(id)`; a missing key returns `-1`/null. A debug flag (byte at `0x80602034`) makes `GetString` return the *id itself* formatted with `swprintf`
instead of the text. **[C]** — the `.idx` layout matches FINDINGS.md ("big-endian key/index pairs"); confirming against the real file needs one `string.idx` and `ENG_US.loc`.

## 3. The attribute database (`db.vlt` + `db.bin`) — structure and schema

The game's balance/level/rules data is an EA **Attrib/VLT** database, opened by `pgIDatabase::InitialiseDatabase` (boot, [01](01-executable-and-boot.md)) through the runtime classes `Attrib::Vault`, `Database`, `Class`, `Collection`,
`ExportManager`, `HashMap` (`attribute-db` subsystem, 215 functions) and wrapped by `pgIDatabase` / `pgDBCollection` (typed getters: `GetInt32/UInt8/Float/String/Enum/Vector4/RefSpecKey/Array*`, `GetCollection(class, key)`, `GetRefCollectionFromArray`). **[D]**

Structure recovered from the real files (`tools/vlt_probe.py`) **[C]**:

- **`db.vlt`** (286,192 B) is a chunk file: `tag[4] size[4 BE, includes the 8-byte header]`: `Vers` (16 B) · `DepN` (dependencies: the names `db.vlt`, `db.bin`) · `StrN` (empty) · `DatN` (208,912 B: class and collection blocks) ·
  `ExpN` (22,608 B: export table) · `PtrN` (54,576 B: pointer fix-ups, **[U]** layout).
- **`ExpN`**: `count` (941) at chunk offset 12, then 24-byte big-endian records **`{ name_hash u64, type_hash u64, size u32, offset u32 }`**; `offset` is an *absolute file offset* and consecutive records tile `DatN` exactly.
  `type_hash` = hash of `Attrib::ClassLoadData` (32 records) or `Attrib::CollectionLoadData` (908); one record has another type (a database-level record). Records are ordered **class first, then that class's collections** — so the export table *is* the table of contents.
- **Class block** (40 B): `{ class_hash u64, u32 0x36 (54), u32 6, 16 zero bytes, u64 hash-of-"base" }` (the two counts are **[U]**). **Collection block**: `{ parent_hash u64, class_hash u64, u64 (0 for top-level rows, else a further link — **[U]**), counts,
  then per-field entries whose first word is the field-name hash, with type hashes (`EA::Reflection::UInt8/Bool/…`, `Enums::…`, `Attrib::RefSpec`, …) }`. The collection's *own* name hash is only in `ExpN` (the block starts with its parent's hash;
  top-level rows inherit from `base`). The exact per-field value layout and `PtrN` semantics are **[U]** (next task).
- **`db.bin`** (97,352 B): a `StrE` string pool (767 strings: the *values* of text/symbol fields, e.g. animation state names, POI names), then the **type vocabulary** (below), then a hashed binary section (enum member tables **[U]**).
  Resolved names: hash every candidate string with `attrib_hash64` and look the 8-byte big-endian value up in `ExpN`/`DatN` (`vlt_probe.py` does this with the bin strings + every string in the ELF).
- Collection *names* (the 900+ row keys such as a specific minigame level or NPC) are not stored anywhere in the files; they are recovered by hashing candidate names (the game code passes many as literals; game-data names like NPC ids must come from other files).

### Type vocabulary in `db.bin` **[D]**

Primitives (`EA::Reflection::` `Bool Char Int8/16/32/64 UInt8/16/32/64 Float Double Text Type Reference KeyValueAttrib`), `Attrib::` `Blob RefSpec Definition Key`, `Attrib::Types::` `Symbol Colour FloatColour ShaderColour Matrix Quaternion Vector2/3/4`,
`BitfieldValue`, `Table::Int/Float`, and the game enums/structs: `Enums::` `AccessibilityPopupType AbilityType MinigameType DSTargetType MGPA_ObstacleType MGPA_PowerupType MGPA_PowerupColor TetherBallDistanceFromPole TetherBallGameplayTypes
DodgeBallGameplayTypes MGDB_VersusSetupTypes PaperAirplanePlaneTypes PaperAirplaneModes MiniGameMultiplayerMode MGDS_ShieldTypes MGDS_DartTypes DSEncounterWeaponsType DSTargetCharacterEnterExitType MGPA_SpeedTypes MiniGameDifficultyLevel
MiniGameDareType StickerType AreaType BubbleType EffectName InteractiveObjectType AIWanderType PoIChainType PoIPositionType PoITarget PoIType CharacterSkeleton CharacterHeadTrackingType CharacterPersonality CharacterGender`; `Structs::` `PG_AccessibilityPopup PG_Ability PG_HighScore
MGDS_SpawnPoint MGDS_ScoreRecord MGDS_TargetDef MGPA_Obstacle MGPA_Powerup`.

### The schema (32 classes, 908 collections) — **[C]** names, counts and field names

Field names are those resolvable from strings (the ELF and the pool); a class may have more fields whose names never appear as text. Two classes are named only by hash (their names are not strings anywhere in the executable).

| class (table) | collections (rows) | fields resolved |
|---|---:|---|
| `points_of_interest_groups` | 39 | `area`, `required`, `pois`, `force_finish`, `allow_wait`, `min_characters` |
| `points_of_interest` | 267 | `chain_previous`, `duration_max`, `position_type`, `script_onactivate`, `critical`, `uninterruptible_use_min`, `position`, `interest_chance`, `target_character`, `action_animation`, `script_onactionend`, `script_ondeactivate`, `wait_min`, `active`, `wait_max`, `filter_spawn_table`, `duration_min`, `default_behaviour`, `filter_gender`, `recharge_min`, `chain_continue_chance`, `action_animation_chance`, `name`, `duration_infinite`, `boredom_min`, `boredom_max`, `chain_type`, `type`, `recharge_initial_max`, `recharge_max`, `script_onactionstart`, `position_num_tag_points`, `uninterruptible_wait_max`, `uninterruptible_wait_min`, `uninterruptible_use_max`, `expiry`, `interest_proximity`, `speed`, `area`, `recharge_initial_min`, `filter_character` |
| `abilities` | 8 | `Structs::PG_Ability`, `ability` |
| `spawn_regions` | 63 | `active_state`, `area`, `vertices`, `default_pois` |
| `microgame_highfives` | 1 | `marbles_won` |
| `mg_rccars` | 47 | `sprint_laptime_level2`, `sprint_laptime_level3`, `sprint_laptime_level4`, `sprint_laptime_level1`, `pre_go_boost_window`, `zap_upgrade`, `nitro_upgrade`, `top_speed_upgrade`, `boost_ability`, `post_go_boost_window`, `score_bump_points`, `score_pickup_points`, `score_attack_points`, `score_first_place_points`, `num_laps`, `powerups_on`, `difficulty`, `location`, `camera_half_posvertdist`, `camera_half_lookahead`, `camera_full_posvertdist`, `camera_full_poshorizdist`, `camera_half_poshorizdist`, `camera_full_targetvertdist`, `camera_quarter_posvertdist`, `camera_quarter_laneskew`, `camera_quarter_targetvertdist`, `camera_half_targetvertdist`, `camera_pos_heightweight`, `camera_full_laneskew`, `camera_target_heightweight`, `camera_full_lookahead`, `camera_quarter_lookahead`, `camera_quarter_poshorizdist`, `Markers`, `camera_half_laneskew`, `use_shield`, `collision_course`, `quick_boost`, `speed_up`, `use_powerups`, `push_back`, `change_lane`, `slow_down`, `bypass` |
| `microgame_freethrow` | 1 | `marbles_won` |
| `minigames` | 35 | `minigame_difficulty`, `marbles_won_game`, `Sticker1`, `minigame_level`, `minigame_id`, `ai_bosscharacterindex`, `Sticker3`, `Sticker2`, `minigame_dare_2`, `minigame_dare_1`, `marbles_won_dares`, `minigame_dare_3`, `ai_opponents`, `ai_teammates` |
| `animation_states` | 1 | — |
| `mg_wallball` | 7 | `ai_mega_hit_percentage`, `rallys_per_speed_level`, `ai_lob_hit_percentage`, `ai_hit_percentage`, `ball_speed_levels`, `character_speed_levels`, `ai_super_serve_percentage`, `ai_dive_hit_percentage`, `ai_power_hit_percentage`, `score_warp`, `score_superball`, `score_ingame_point`, `score_megahit`, `solo_easy_rally_count`, `normal_play_num_rounds`, `increased_powerup_min_spawn_time`, `powerdown_med_num_rounds`, `warp_challenge_num_rounds`, `solo_easy_rallys_per_speed_level_override`, `solo_med_rallys_per_speed_level_override`, `solo_hard_rallys_per_speed_level_override`, `powerdown_easy_num_rounds`, `normal_powerup_max_wallhits`, `warpspeed_challenge_num_rounds`, `speed_challenge_num_rounds`, `normal_powerup_min_wallhits`, `normal_powerup_min_spawn_time`, `solo_hard_rally_count`, `increased_powerup_min_wallhits`, `increased_powerup_max_wallhits`, `normal_powerup_max_spawn_time`, `increased_powerup_max_spawn_time`, `solo_med_rally_count`, `points_to_win`, `powerups_on`, `difficulty`, `location`, `megahit_on` |
| `mg_footie` | 7 | `shutout_easy_goals_to_win`, `normal_goals_to_win`, `shutout_med_goals_to_win`, `keeper_challenge_hard_saves`, `shutout_hard_goals_to_win`, `keeper_challenge_easy_saves`, `normal_max_vollys`, `keeper_challenge_med_saves`, `ai_block_attempt_percentage`, `ai_block_shot_attempt_percentage`, `ai_second_hit_juggle_percentage`, `ai_good_shot_percentage`, `ai_enable_block_shot`, `ai_save_strong_shot_percentage`, `ai_second_hit_shot_percentage`, `ai_save_too_early_percentage`, `ai_bad_juggle_percentage`, `ai_enable_mega_shot2`, `ai_enable_mega_shot1`, `ai_save_normal_shot_percentage`, `ai_first_hit_juggle_percentage`, `ai_slow_shot_percentage`, `ai_serve_min_wait_time`, `ai_bad_shot_percentage`, `ai_first_hit_shot_percentage`, `ai_save_mega_shot_percentage`, `ai_third_hit_shot_percentage`, `ai_save_wrong_direction_percentage`, `ai_third_hit_jump_juggle_percentage`, `ai_perfect_shot_percentage`, `ai_perfect_juggle_percentage`, `ai_first_hit_jump_juggle_percentage`, `ai_save_block_shot_percentage`, `ai_third_hit_juggle_percentage`, `ai_fast_shot_percentage`, `ai_second_hit_jump_juggle_percentage`, `ai_save_fall_for_jump_juggle_percentage`, `ai_good_juggle_percentage`, `ai_save_weak_shot_percentage`, `ai_normal_shot_percentage`, `ai_serve_max_wait_time`, `score_block`, `score_save`, `score_regular_goal`, `score_block_shot`, `score_winner_team_multiplier`, `score_mega_goal`, `headergoal_on`, `difficulty`, `num_passes`, `location`, `goals_to_win` |
| `22808541710a6d7a` | 11 | `pos` |
| `area` | 4 | `area_type`, `area_name` |
| `bestiary` | 101 | `conversation`, `speed_run`, `minigame`, `personal_space`, `name`, `partfx_loc`, `ai_head_tracking`, `asset_name`, `variation`, `celebration_anim`, `start_direction`, `partfx_bone`, `ai_static_anim_linked_placeable`, `scale`, `ai_static_anim`, `ai_wander_run_percentage`, `rccars_icon`, `ai_static`, `speed_skip`, `gender`, `has_stickerking_idle`, `ai_head_tracking_percentage`, `ai_wander_skip_percentage`, `personality`, `speed_walk`, `height`, `partfx`, `highfiveable`, `speed_sneak`, `ai_wander_walk_percentage`, `area_conversations` |
| `gates` | 6 | `landmark_pan_final_pause`, `landmark_pan_milliseconds`, `landmark_camera_initial_target`, `landmark_camera_final_position`, `landmark_camera_initial_position`, `unlockable`, `fade_color`, `landmark_camera_final_target`, `stickers_required`, `nis_camera_target`, `gate_right`, `nis_camera_position`, `gate_left`, `nis_particle_effect_location` |
| `main_menu_nis` | 6 | `camera_main_menu_target`, `camera_main_menu_position` |
| `8dc7ad63193d47f2` | 50 | `conversation`, `minigame_level`, `minigame_id` |
| `mg_dartshootout` | 160 | `big_dart_score_multiplier`, `EnterStartOffset`, `EnterEndOffset`, `TargetType`, `ActiveSpeed`, `Health`, `LeaveSpeed`, `ActiveEndOffset`, `Lifetime`, `LeaveEndOffset`, `CharacterEnterExitType`, `LifetimeVariance`, `TargetID`, `EnterSpeed`, `ActiveLoop`, `dare_points`, `dare_health`, `dare_time`, `rules_dart_type`, `difficulty`, `rules_shield_type`, `location`, `mega_shot_on`, `Structs::MGDS_ScoreRecord`, `points_shielded_dart`, `bullet_time_hit_score`, `points_megashot`, `target_scores`, `Structs::MGDS_TargetDef`, `Structs::MGDS_SpawnPoint`, `CameraTransitionDuration`, `TimeLimit`, `EncounterTimeout`, `EncounterPostFullAutoBonusChance`, `EncounterWeapons`, `EncounterCameraTarget`, `EncounterCameraPos`, `EncounterPostShowCleared`, `EncounterPostHealthBonusChance`, `TargetDefs`, `SpawnPoints` |
| `high_scores` | 1 | `Structs::PG_HighScore`, `score` |
| `microgame_dribbling` | 5 | `beat_speed`, `dribbles_required`, `marbles_won`, `ball_position`, `time_limit` |
| `placeables` | 25 | `has_animtriggeredaudio`, `has_collisions`, `audio_range`, `active_areas_enabled`, `is_fadeable`, `default_visible`, `asset_name`, `position`, `orientation`, `physics_name`, `active_areas` |
| `area_transition` | 12 | `stickerkid_exit_position`, `trigger_position`, `stickerkid_enter_position`, `area_start`, `warpto_direction`, `trigger_radius`, `area_end`, `warpto_position`, `gate`, `stickerkid_conversation_position` |
| `mg_dodgeball` | 7 | `difficulty`, `versus_setup`, `location`, `num_rounds`, `num_dodgeballs`, `game_play_type`, `num_characters_ai_team`, `ai_bobble_catch_percentage`, `ai_boost_attempt_percentage`, `ai_react_to_fake_percentage`, `ai_normal_catch_percentage`, `user_eliminated_on_catch`, `ai_mega_catch_percentage`, `game_play_win_value`, `dodgeball_max_inactive_time`, `ai_power_upgrade_throw_percentage`, `ai_normal_dodge_percentage`, `ai_power_dodge_percentage`, `user_can_catch`, `ai_power_upgrade_catch_percentage`, `number_balls`, `ai_power_catch_percentage`, `ai_bobble_catch_drop_percentage`, `ai_good_boost_percentage`, `ai_fake_detection_radius`, `ai_mega_block_percentage`, `ai_power_throw_percentage`, `ai_mega_throw_percentage`, `ai_fake_throw_percentage`, `ai_power_block_percentage`, `ai_throw_max_wait`, `balls_on_ai_side_always`, `ai_power_upgrade_dodge_percentage`, `ai_throw_min_wait`, `instant_opponent_revive`, `ai_target_percentage`, `ai_movement_multiplier`, `ai_power_upgrade_block_percentage`, `num_characters_player_team`, `ai_normal_block_percentage`, `ai_mega_dodge_percentage`, `revive_on_catch`, `hit_points`, `dodge_points`, `powerhit_points`, `block_points`, `catch_points` |
| `mg_tetherball` | 6 | `accuracy_points`, `powerhit_points`, `megahit_points`, `ball_acceleratemodifier`, `ball_basehitspeed`, `hit_accelanglepredelta`, `ball_megamodifier`, `game_duration`, `ai_megamessupfactor`, `ball_powermodifier`, `hit_returnanglepostdelta`, `hit_returnanglepredelta`, `ai_toofastchance`, `ai_powermessupfactor`, `ai_powerhitchance`, `ai_tooslowchance`, `ai_megahitchance`, `game_play_type`, `hit_accelanglepostdelta`, `handicap`, `single_player_rotations_to_win`, `ai_wrongheightchance`, `single_player_num_rounds`, `distance_from_pole`, `difficulty`, `rotations_to_win`, `location`, `num_rounds`, `megahit_on` |
| `mg_paperairplanes` | 14 | `rules_speed_type`, `difficulty`, `tornado_on`, `course_number`, `plane_type`, `mode`, `start_time`, `treshold_increment`, `respawn_points`, `bonus_treshold`, `red_points`, `max_duration`, `green_points`, `length`, `blue_points`, `Structs::MGPA_Powerup`, `Structs::MGPA_Obstacle`, `rings_red`, `rings_blue`, `checkpoints`, `obstacles`, `rings_green`, `dare_blue_rings`, `dare_red_rings_reverse`, `dare_green_rings`, `powerups`, `dare_points`, `dare_points_reverse`, `dare_blue_rings_reverse`, `start_time_reverse`, `dare_red_rings`, `dare_green_rings_reverse`, `rings` |
| `character_select` | 3 | `unlockLinks`, `characterlist`, `positions` |
| `microgame_bughunt` | 3 | `bug_size`, `marbles_won`, `bugs_for_win`, `icon_position`, `game_time_limit`, `max_flight_time`, `wander_radius`, `bug_positions` |
| `effects` | 8 | `pos`, `name` |
| `character_info` | 2 | `start_direction`, `start_location` |
| `nis` | 3 | `cam_start_pos`, `cam_pause_end`, `player_start_pos`, `cam_pause_start`, `player_start_dir`, `cam_end_pos`, `cam_end_target`, `cam_start_target`, `cam_pan_milliseconds`, `sticker_kid_end_pos`, `player_end_pos` |
| `marbles` | 4 | `value`, `positions` |
| `accessibility_screens` | 1 | `Structs::PG_AccessibilityPopup`, `popups` |

Reading the rows: the key facts are that *rules and tuning live here* (e.g. `mg_rccars` lap-time thresholds/upgrades/score points; `mg_dartshootout` wave/target definitions; `mg_footie` goals-to-win and AI percentages; `minigames` = the
ladder of dares/levels/opponents/stickers/marbles; `bestiary` = every NPC; `points_of_interest*` = ambient NPC behaviours with Lua callbacks; `gates`, `area_transition`, `placeables`, `spawn_regions` = the world). **[D]**

## 4. CSV files and the parser **[C]**

`cCSVParser` (`csvparser.cpp`, 10 methods): comma-separated, **header-driven** (`GetStringField(name)` / `GetIntField` / `GetFloatField` look columns up by header name), **left-trims** fields, CRLF, blank lines skipped, `NextRecord` per row. Used for the 14
`controls*.csv` ([03](03-input.md)), `world.csv`, `partfx.csv`, `home.csv`, `tpdata/LightConfig.csv`, `*_tanim.csv` (per-animation tables) and the animation/costume tables named in `FINDINGS.md`. **[D]** for the names; per-file schemas for the non-controls CSVs are **[U]** until samples arrive
(each is decodable the same way: find the `GetStringField("…")` names in the loading function).

## 5. How each data area is reached (file names the code builds) **[D]**

| area | paths / patterns in the executable |
|---|---|
| main archive | `pgIO::AddBigFile` (in `GameState::Init`); `%s%s.big`; world: `data/world/world.big`, `world.gsh`, `world-misc.gsh` |
| minigames | `data/minigames/dodgeball/mgdodgeball.viv`, `footie/mgfootie.viv`, `paperairplane/mgpaperairplane.viv`, `rccars/rccars.viv`, `rccars/tracks/rccartrack%2d.viv` (+ `_shadow.viv`), `tetherball/mgtetherball.viv`, `wallball/mgwallball.viv`; `data\microgames\microgames.viv` |
| characters | `data/characters/models/characters.viv`, `shadow.viv`, `data/characters/player_anims.viv`; skeleton `player_skel.ske`; clips `*_anims.anm` |
| props / sky | `data\worldprops\worldprops.viv`, `data\placeables\skybox*.viv` (`skybox.o`, `skybox_clouds.o`, `skybox_mountain_city_ring.o`) |
| effects | `data\effects\effects.gsh`, `partfx.csv`, `.lef` |
| conversations / UI | `data\conversation\conversation.viv`, `data\fe\FontTable.txt`, `data\fe\RealFonts.txt`, `data/fe/` movies, `data/csvs.viv` |
| audio | `data/audio/{AEMS,Banks,Music,Speech}/`; banks `FE_UI.abk`, `World_SFX.abk`, `MG_*.abk`, `MW_*.abk`; music `*.asf`; speech `spchdat.viv/.bh`, `spchhdr.viv`; `MG_PaperAirplanes.ast` |
| localisation | `data\locale\<LANG>.loc`, `data\locale\string.idx` |
| database | `db.vlt`, `db.bin` (via `pgIDatabase`) |
| conga / input | `data/conga/conga.gsm`, `controls*.csv` |
| physics / markers | `.hkx` (Havok), `.mkr` (`character.mkr`, `lanes.mkr`, `strip.mkr`), `.cpt` |
| boot / system | `data/boot/strapwarn_{wide,standard}_<lang>.gsh`, `disc.ini`, `/homeBtn*.arc`, `data/saveicons/icon0.tpl`, `banner.tpl` |

## 6. What to send next (highest value first)

`string.idx` + `ENG_US.loc` (confirms §2 and gives all text) · one small `.mkr` and `.cpt`/`.con`/`.atd` (parsers are in the binary) · `conversation.viv` contents · a profile save file (`ProfileData` layout) · the other CSVs
(`world.csv`, `partfx.csv`, `home.csv`, `LightConfig.csv`, a `*_tanim.csv`) · one menu `.apt` + `.const` (to bind screens to handlers) · Dolphin input recordings of the menus and one minigame (for the "compared" evidence level).
