# 03 — Input: controllers, events and the `controls*.csv` mapping

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF, **[I]** inferred, **[U]** unresolved.
Machine-readable: [`data/enums/`](../data/enums/) (`input_*.tsv`), [`data/functions/game-input.tsv`](../data/functions/game-input.tsv),
[`data/functions/engine-pad-drivers.tsv`](../data/functions/engine-pad-drivers.tsv). Generated page: [game-input](subsystems/game-input.md).

## Architecture

The game does **not** read buttons directly in gameplay code. It has an abstract layer:

```
Wii hardware ─▶ pad drivers (Core / DPD-Ex / FreeStyle / Classic / GC / Future PadModeHandlers, wiipad.cpp, EA "Conga")
             ─▶ Controller (controller.cpp)  ── per-frame Update(int ms) → UpdateInput(int)
                  ├─ loaded from one of 14 CSV files (ControlType) : rows of [action event, context state, transition,
                  │      button-event kind, modifiers, button]
                  └─ produces an *event state* per EActionEvent   →   Controller::GetEventState(EActionEvent)
gameplay code (minigames, camera, player) polls GetEventState / analog helpers with EVENT_* ids
```

- `Controller::Create(ControlType)` (`0x8032c520`) → `Initialize(ControlType, int)` (`0x8032c6c4`): loads `ControlTypeFilenames[type]` with `FILE_loadsize`, parses it with
  `cCSVParser`, and builds a table of **100-byte rows**. `ControlType` selects the CSV (values below). **[C]**
- `Controller::Update(int)` → `UpdateInput(int)` (`0x8032cb58`, 3,672 B) is the per-frame state machine; `GetInput(int, int&, int&, int&, int&)` (`0x80329cb4`, 1,444 B) reads a
  pad. Analog helpers: `GetControllerAnalogScale` (dead-zone), `…NoDeadZone`, `GetControllerDPadDirectionScale`; Wii extras: `GetCoreAccelerometers`,
  `GetLastCoreAccelerometers`, `GetFilteredValue(EWiiMoteAxis, int)`, pointer (DPD) helpers (`GetDPDMousePointRotationallyCorrected`,
  `…InScreenCoordsRotationallyCorrected`, `GetWorldVectorFromDPDRotationallyCorrected(ViewPort*, Camera*, int, rmVector3*)`), `GetMouseRotation`, `StartRumble(int, float)`,
  `EnableFrontEndInput(bool)`, `SetCurrentControllerState/PopState`. **[D]** (signatures); behaviour of `UpdateInput` is **[U]** beyond what is below.
- Reflection names registered in the constructor (Exposure/Lua): `Controller:mButtonHeldState`, `mTimeSinceLastButtonDown`, `mTimeBetweenLastTwoButtonDowns`, `mEventState`. **[D]**
  These are the per-button timers that implement tap / hold / double-press.
- `Controller::Update` receives the **uncapped** frame time in ms (see [02](02-state-machine-and-timing.md)), so timing thresholds are in real milliseconds. **[C]**
- Pad-mode classes (engine): `IPadModeHandler` and `CCorePadModeHandler` (Wii Remote), `CDPDExPadModeHandler` (pointer + expansion), `CFreeStylePadModeHandler`
  (Nunchuk), `CClassicPadModeHandler`, `CGCPadModeHandler` (GameCube), `CFuturePadModeHandler`; `WiiRumbleEffects`. `gEnableNunchuck` is a runtime flag. **[D]** Which
  handler each minigame uses is **[U]** — start at `PadModeHandler` vtables in [`engine-pad-drivers`](subsystems/engine-pad-drivers.md).
- `PGConga` wraps EA's *Conga* input service; `CONTROLLER_OFFSCREEN_X/Y` (pointer off-screen sentinels) and `EA::Conga::PrimaryControllerOutput::ConvertMousePointToScreenCoords`
  map the DPD pointer to screen space. **[D]**

## The CSV schema (recovered from the parser) **[C]**

`Controller::Initialize` reads these columns per record, in this order, converting each with the executable's own string→enum functions:

| column | stored at row offset | converter | notes |
|---|---:|---|---|
| `ACTION_EVENT` | `+0x00` | `ConvertStringToActionEvent` | `EVENT_*` (188 values) |
| `CONTROLLER_STATE` | `+0x44` | `ConvertStringToControllerState` | input context `STATE_*` in which the row is active |
| `CONTROLLER_STATE_TRANSITION` | `+0x48` | `ConvertStringToControllerState` | context to switch to when the row fires |
| `CONTROLLER_EVENT` | `+0x4c` | `ConvertStringToControllerEvent` | kind of button event required (`BUTTON_*`) |
| `MOD1` | `+0x50` (required) / `+0x58` (forbidden) | `ConvertStringToButton` | a leading `~` stores the button as *forbidden* (must not be held) |
| `MOD2` | `+0x54` (required) / `+0x5c` (forbidden) | `ConvertStringToButton` | same rule |
| `BUTTON` | `+0x60` | `ConvertStringToButton` | the primary button |

Row stride is `0x64` (100) bytes and the rows start at `Controller + 0x26c`; the row count is kept in a separate field. Empty `MOD` columns leave the four modifier fields at their
initial value `0`, which is numerically `UP`; how `UpdateInput` distinguishes "no modifier" from `UP` is **[U]** (read the compare in `UpdateInput`). Bytes `+0x04..+0x43` of a row are not
written by the parser (runtime state, **[U]**). The `~` variants of every button token map to the *same* enum value as the plain token.

### Validated against real files **[C]**

Seven distinct real files were supplied after the schema was recovered (`controls.csv`, `controlsmg21`, `…bughunt`, `…dartshootout`, `…dribbling`, `…footie`, `…freethrow`;
246 rows — `freethrow` arrived twice, byte-identical). `tools/check_controls_csv.py` resolves every token with the enums above: **245 of 246 rows resolve completely; the one exception is deliberate** (below). This
confirms the column set, the enum values and the `~` convention. The files are game data and are **not** committed; run the checker on your own copies.

What the real data adds:

- **Columns are matched by header name**, not position (`cCSVParser::GetStringField` scans the header names, `0x802f2eb0`), and every field has **leading whitespace trimmed**
  (`cParser::TrimWhiteSpaceLeft` in `cCSVParser::GetString`, `0x802f2b3c`). So `EVENT_PAUSE_ON, STATE_FOOTIE` (leading space, in `controlsmgfootie.csv`) works in the original.
  Trailing whitespace is *not* trimmed. Files use CRLF and blank lines between groups. **[C]**
- **Unrecognised tokens silently become the converter's fallback value** (action event `190`, state `31`, button-event kind `9`) — the game never reports an error.
  The data exploits this: `EVENT_PLAYER_FACEDIRECTION-NOTUSED` (in `controls.csv`) is a *disabled row*, an event name that no longer parses. A faithful loader must keep such rows
  inert rather than rejecting them. **[C]**
- An **empty `CONTROLLER_STATE_TRANSITION`** goes through the same converter and becomes state `31`, which is therefore the "no transition" value. **[I]** (check where `UpdateInput` compares against 31).
- Modifier examples in real rows: `EVENT_FOOTIE_POWER` = `A` while `B` is held (`MOD1=B`); `EVENT_EXIT_MINIGAME` = `TWO` while `ONE` is held (`MOD1=ONE`) in most minigames;
  `EVENT_PAUSE_ON` = `PLUS` with `MOD2=~MINUS` (PLUS only when MINUS is *not* held); `EVENT_REPORTCARD` = `MINUS` with `MOD2=~PLUS`; the dart minigame's mega-shot and debug rows use `MOD2`.
- Kinds used across the samples (rows counted with the duplicate): `BUTTON_DOWN` 133, `BUTTON_PRESSED` 110, `BUTTON_HOLDPRESSED` 18 (debug-menu auto-repeat), `BUTTON_SECOND` 8, `BUTTON_DOUBLEDOWN` 8, `BUTTON_UP` 2.
  (`BUTTON_TAP`/`BUTTON_HOLD` appear only in files not yet seen, e.g. RC Cars.) Conventions visible in the data: `DOWN` = the frame the button goes down; `PRESSED` = held (analog
  and movement); `DOUBLEDOWN` opens the debug menu (`ONE`); `SECOND` on `MINUS` resets the free camera. **[I]** for the exact per-frame semantics.
- The shared blocks (free camera, debug menu) are copy-pasted into every minigame file with identical rows; the free camera enters via `STATE_ANY → STATE_FREECAM` on `C` and leaves with
  `STATE_FREECAM → STATE_RETURN`; the debug menu likewise on double-pressing `ONE`. `STATE_RETURN` (27) pops back to the previous context (`Controller::PopState`). **[C]/[I]**

**To decode the CSVs with certainty**, use the enums below (they are recovered from the code, so they are exact; the string-table order is *not* the enum order — e.g. `EVENT_21_AIM` is 26).

### `ControlType` → file **[D]**

`controls.csv`=0, `controlsmg21.csv`=1, `controlsmgbughunt.csv`=2, `controlsmgdartshootout.csv`=3, `controlsmgdodgeball.csv`=4, `controlsmgdribbling.csv`=5, `controlsmgfootie.csv`=6, `controlsmgpaperairplanes.csv`=7, `controlsmgquickdraw.csv`=8, `controlsmgrccars.csv`=9, `controlsmgtemplate.csv`=10, `controlsmgtetherball.csv`=11, `controlsmgwallball.csv`=12, `controlsmgfreethrow.csv`=13

### Input contexts (`EControllerState`) **[C]**

`STATE_FE`=0, `STATE_WANDER`=1, `STATE_MINIGAME`=2, `STATE_COMBAT`=3, `STATE_RANGED`=4, `STATE_CAMERA`=5, `STATE_FREECAM`=6, `STATE_PAUSE`=7, `STATE_DEBUGMENU`=8, `STATE_CONVERSATION`=9, `STATE_POITUNING`=10, `STATE_21_WAITING`=11, `STATE_21_FREEBALL`=12, `STATE_21_INTERFERENCE`=13, `STATE_21_SHOOTING`=14, `STATE_QD_HOLSTERED`=15, `STATE_QD_SHOOTING`=16, `STATE_TB_SERVING`=17, `STATE_TB_HITTING`=18, `STATE_TB_WAITING`=19, `STATE_DODGEBALL`=20, `STATE_FOOTIE`=21, `STATE_PA`=22, `STATE_WALLBALL`=23, `STATE_FREETHROW`=24, `STATE_DRIBBLING`=26, `STATE_RETURN`=27, `STATE_ANY`=28, `STATE_MULTIPLAYER`=29

`STATE_ANY` (28) is the wildcard; the parser returns `31` for an unknown token. Value 25 is never produced by the parser. `STATE_MINIGAME` (2) is the generic minigame context; the
per-minigame contexts (`21_*`, `QD_*`, `TB_*`, `DODGEBALL`, `FOOTIE`, `PA`, `WALLBALL`, `FREETHROW`, `DRIBBLING`) exist alongside it.

### Buttons (`ConvertStringToButton`) **[C]**

`UP`=0, `DOWN`=1, `LEFT`=2, `RIGHT`=3, `A`=4, `B`=5, `C`=6, `Z`=7, `ONE`=8, `TWO`=9, `HOME`=10, `PLUS`=11, `MINUS`=12, `ANALOG`=13  (plus a `~`-prefixed twin of each, same value)

`A B C Z ONE TWO HOME PLUS MINUS` are Wii Remote/Nunchuk (`C`, `Z` = Nunchuk); `ANALOG` is the stick.

### Button-event kinds (`ConvertStringToControllerEvent`) **[C]**

`BUTTON_UP`=0, `BUTTON_DOWN`=1, `BUTTON_DOUBLEDOWN`=2, `BUTTON_PRESSED`=3, `BUTTON_SECOND`=4, `BUTTON_TAP`=5, `BUTTON_HOLD`=6, `BUTTON_HOLDPRESSED`=7

Their exact timing semantics belong to `UpdateInput` and are **[U]**; thresholds that exist as named tunables: `gControllerDoubleDownThreshhold = 500` ms,
`gTapPressThreshhold = 180` ms, `gTapHoldPressThreshhold = 120` ms, `gControllerRumbleMSPerInterval = 100` ms (initial values, **[D]**). The names suggest
`BUTTON_DOUBLEDOWN` = second press within 500 ms, `BUTTON_TAP` = release within the tap window, `BUTTON_HOLD` = held past a threshold — **[I]**, verify in `UpdateInput`
before porting.

### Action events (`EActionEvent`) **[C]** — 188 values in 26 families

- **PLAYER** (7): `PLAYER_MOVE`=0, `PLAYER_FACEDIRECTION`=1, `PLAYER_JUMP`=2, `PLAYER_MOVE_LEFT`=178, `PLAYER_MOVE_RIGHT`=179, `PLAYER_MOVE_FORWARD`=180, `PLAYER_MOVE_BACKWARD`=181
- **ENTER** (1): `ENTER_MINIGAME`=3
- **EXIT** (1): `EXIT_MINIGAME`=4
- **SELECT** (1): `SELECT_BUBBLE`=5
- **INTERACTIVE** (1): `INTERACTIVE_OBJECT_USE`=6
- **CAMERA** (7): `CAMERA_REORIENT`=7, `CAMERA_RESET`=8, `CAMERA_MOVE`=9, `CAMERA_ON`=10, `CAMERA_OFF`=11, `CAMERA_ZOOMIN`=12, `CAMERA_ZOOMOUT`=13
- **FREECAM** (12): `FREECAM_MOVE`=14, `FREECAM_TILTUP`=15, `FREECAM_TILTDOWN`=16, `FREECAM_TILTLEFT`=17, `FREECAM_TILTRIGHT`=18, `FREECAM_ON`=19, `FREECAM_OFF`=20, `FREECAM_UP`=21, `FREECAM_DOWN`=22, `FREECAM_ZOOMIN`=23, `FREECAM_ZOOMOUT`=24, `FREECAM_RESET`=25
- **21** (5): `21_AIM`=26, `21_SHOOT`=27, `21_CHARGE`=28, `21_CANCEL`=29, `21_INTERFERENCE`=30
- **QUICKDRAW** (2): `QUICKDRAW_SHOOT`=31, `QUICKDRAW_DODGE`=32
- **RCCARS** (56): `RCCARS_ACCELERATE`=33, `RCCARS_REVERSE`=34, `RCCARS_BOOST`=35, `RCCARS_POWERUP`=36, `RCCARS_TURN`=37, `RCCARS_SMOKESCREEN`=38, `RCCARS_A`=39, `RCCARS_A_DOUBLEDOWN`=40, `RCCARS_A_TAP`=41, `RCCARS_A_HOLD`=42, `RCCARS_A_PRESSED`=43, `RCCARS_B`=44, `RCCARS_B_DOUBLEDOWN`=45, `RCCARS_B_TAP`=46, `RCCARS_B_HOLD`=47, `RCCARS_B_PRESSED`=48, `RCCARS_1`=49, `RCCARS_1_DOUBLEDOWN`=50, `RCCARS_1_TAP`=51, `RCCARS_1_HOLD`=52, `RCCARS_1_PRESSED`=53, `RCCARS_2`=54, `RCCARS_2_DOUBLEDOWN`=55, `RCCARS_2_TAP`=56, `RCCARS_2_HOLD`=57, `RCCARS_2_PRESSED`=58, `RCCARS_Z`=59, `RCCARS_Z_DOUBLEDOWN`=60, `RCCARS_Z_TAP`=61, `RCCARS_Z_HOLD`=62, `RCCARS_Z_PRESSED`=63, `RCCARS_C`=64, `RCCARS_C_DOUBLEDOWN`=65, `RCCARS_C_TAP`=66, `RCCARS_C_HOLD`=67, `RCCARS_C_PRESSED`=68, `RCCARS_UP`=69, `RCCARS_UP_DOUBLEDOWN`=70, `RCCARS_UP_TAP`=71, `RCCARS_UP_HOLD`=72, `RCCARS_UP_PRESSED`=73, `RCCARS_DOWN`=74, `RCCARS_DOWN_DOUBLEDOWN`=75, `RCCARS_DOWN_TAP`=76, `RCCARS_DOWN_HOLD`=77, `RCCARS_DOWN_PRESSED`=78, `RCCARS_LEFT`=79, `RCCARS_LEFT_DOUBLEDOWN`=80, `RCCARS_LEFT_TAP`=81, `RCCARS_LEFT_HOLD`=82, `RCCARS_LEFT_PRESSED`=83, `RCCARS_RIGHT`=84, `RCCARS_RIGHT_DOUBLEDOWN`=85, `RCCARS_RIGHT_TAP`=86, `RCCARS_RIGHT_HOLD`=87, `RCCARS_RIGHT_PRESSED`=88
- **TETHERBALL** (6): `TETHERBALL_SERVE`=90, `TETHERBALL_HIT`=91, `TETHERBALL_POWER_A`=92, `TETHERBALL_POWER_B`=93, `TETHERBALL_HIGH_MOD`=94, `TETHERBALL_OVERHAND`=95
- **BUGHUNT** (2): `BUGHUNT_SWINGLEFT`=96, `BUGHUNT_SWINGRIGHT`=97
- **DODGEBALL** (11): `DODGEBALL_MOVE`=98, `DODGEBALL_DPADMOVE`=99, `DODGEBALL_MODIFIERA`=100, `DODGEBALL_MODIFIERB`=101, `DODGEBALL_BUTTONA`=102, `DODGEBALL_BUTTONB`=103, `DODGEBALL_TARGETUP`=104, `DODGEBALL_TARGETDOWN`=105, `DODGEBALL_TARGETLEFT`=106, `DODGEBALL_TARGETRIGHT`=107, `DODGEBALL_GETREADY`=108
- **FOOTIE** (7): `FOOTIE_DPADMOVE`=109, `FOOTIE_MODIFIERA`=110, `FOOTIE_MODIFIERB`=111, `FOOTIE_BUTTONA`=112, `FOOTIE_BUTTONB`=113, `FOOTIE_SWAP`=114, `FOOTIE_POWER`=115
- **PA** (10): `PA_UP`=116, `PA_DOWN`=117, `PA_LEFT`=118, `PA_RIGHT`=119, `PA_FORWARD`=120, `PA_BACKWARD`=121, `PA_RESPAWN`=122, `PA_DASH`=123, `PA_TILT`=124, `PA_RESET`=125
- **DARTSHOOTOUT** (13): `DARTSHOOTOUT_FIRE`=126, `DARTSHOOTOUT_FIREPRESSED`=127, `DARTSHOOTOUT_DEBUG_POINT_ENABLE`=128, `DARTSHOOTOUT_DEBUG_POINT_DISPLAY`=129, `DARTSHOOTOUT_DISABLE_BULLETTIME`=130, `DARTSHOOTOUT_ENABLE_BULLETTIME`=131, `DARTSHOOTOUT_ARM_MEGASHOT`=132, `DARTSHOOTOUT_CHARGE_MEGASHOT`=133, `DARTSHOOTOUT_ENABLE_SHIELD`=134, `DARTSHOOTOUT_RELOAD_UP`=135, `DARTSHOOTOUT_RELOAD_DOWN`=136, `DARTSHOOTOUT_RELOAD_LEFT`=137, `DARTSHOOTOUT_RELOAD_RIGHT`=138
- **WALLBALL** (6): `WALLBALL_DPADMOVE`=139, `WALLBALL_BUTTONA`=140, `WALLBALL_BUTTONB`=141, `WALLBALL_GETREADY`=142, `WALLBALL_MODIFIERA`=143, `WALLBALL_MODIFIERB`=144
- **FREETHROW** (4): `FREETHROW_A`=145, `FREETHROW_B`=146, `FREETHROW_MOD_A`=147, `FREETHROW_MOD_B`=148
- **DEBUGMENU** (10): `DEBUGMENU_ON`=149, `DEBUGMENU_OFF`=150, `DEBUGMENU_UP`=151, `DEBUGMENU_DOWN`=152, `DEBUGMENU_LEFT`=153, `DEBUGMENU_RIGHT`=154, `DEBUGMENU_BACK`=155, `DEBUGMENU_ACCEPT`=156, `DEBUGMENU_LEFTREPEAT`=157, `DEBUGMENU_RIGHTREPEAT`=158
- **POITUNING** (15): `POITUNING_MOVE`=159, `POITUNING_ROTATELEFT`=160, `POITUNING_ROTATERIGHT`=161, `POITUNING_ENABLEROTATION`=162, `POITUNING_DISABLEROTATION`=163, `POITUNING_FASTMODIFIER`=164, `POITUNING_EDIT`=165, `POITUNING_NEXT`=166, `POITUNING_PREV`=167, `POITUNING_NEXTREPEAT`=168, `POITUNING_PREVREPEAT`=169, `POITUNING_NEXTTAGPOINT`=170, `POITUNING_PREVTAGPOINT`=171, `POITUNING_NEXTTAGPOINTREPEAT`=172, `POITUNING_PREVTAGPOINTREPEAT`=173
- **CONVERSATION** (1): `CONVERSATION_START`=174
- **PAUSE** (2): `PAUSE_ON`=175, `PAUSE_OFF`=176
- **REPORTCARD** (1): `REPORTCARD`=177
- **SELECTKID** (2): `SELECTKID_SELECT`=182, `SELECTKID_CANCEL`=183
- **TEMP** (5): `TEMP_MULTIPLAYER_LEFT`=184, `TEMP_MULTIPLAYER_RIGHT`=185, `TEMP_MULTIPLAYER_UP`=186, `TEMP_MULTIPLAYER_DOWN`=187, `TEMP_MULTIPLAYER_START`=188

Notes: `RCCARS` carries a *per-button* set (`A/B/1/2/Z/C/UP/DOWN/LEFT/RIGHT` × {plain, `DOUBLEDOWN`, `TAP`, `HOLD`, `PRESSED`}) plus the semantic events
(`ACCELERATE`, `REVERSE`, `BOOST`, `POWERUP`, `TURN`, `SMOKESCREEN`). `POITUNING`, `DEBUGMENU`, `FREECAM`, `TEMP_MULTIPLAYER_*` are developer/testing inputs. `21` is the
basketball "21" game (`controlsmg21.csv`), `QUICKDRAW`/`DARTSHOOTOUT` the dart minigames.

## Reconstruction guidance

1. Decode the 14 CSVs with the enums above; build the same `(context, event-kind, modifiers, button) → action-event` table in Rust. Keep the `STATE_*` context stack (`SetCurrentControllerState`,
   `PopState`, the transition column) — it is how menus, conversations, pause and minigames each get their own bindings.
2. Reproduce `UpdateInput` (`0x8032cb58`) from its disassembly against the timing globals; it is a single 3.6 KB function and is the only place tap/hold/double semantics live.
3. Map Wii Remote features (pointer, accelerometer, Nunchuk stick, rumble) to a `Controller` trait so keyboard/gamepad/mouse backends can feed the same event table.
