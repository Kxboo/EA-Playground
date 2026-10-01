# Frontend: the original APT screens in Rust

The game now starts in the original EA Playground front end. Nothing in the menus is redrawn by hand: the original
`fe/*.big` APT movies (Flash-derived shapes, timelines and ActionScript bytecode) are parsed, their scripts are executed
by a Rust ActionScript VM, and the resulting display list is drawn with Bevy. The game side of the API (the functions the
scripts call through `CallGameFunc`) is implemented in Rust in `fe_host.rs`.

Run it with no arguments (`--mode menu` keeps the older developer menu, `--no-world` skips the 3-D backdrop,
`--mute` disables music).

## Modules

| Module | Role |
| --- | --- |
| `apt.rs` | APT/BIG parser: characters, frames, places, clip actions, bytecode. |
| `apt_player.rs` | Display list: timelines, goto semantics, placement, flattening to draw items. |
| `apt_vm.rs`, `apt_lib.rs`, `apt_anim.rs` | ActionScript 2 VM, built-in classes (MovieClip, TextField, LoadVars, Color, ...), `loadMovie`, input events, the AEO tween loop. |
| `fntg.rs`, `apt_text.rs` | Original `.gfn` bitmap fonts (`fonttable.txt` selects face and offsets) and text layout. |
| `apt_mat.rs`, `apt_ui.wgsl` | Material for movie shapes: texture x multiply colour + additive colour in gamma space. |
| `apt_view.rs` | Bevy host: pooled quads, letterboxed 16:9 camera, pointer/keyboard input, camera moves, music, scripted test input. |
| `fe_host.rs` | The game API: profile, kid select, multiplayer flow, pregame/postgame, pause, minigame HUD calls. |
| `kid_pick.rs` | The 3-D selectable-kid scene behind `SelectKid` and `PlayerSetup`. |
| `game.rs` (`Backdrop`) | The playground world shown behind the menus; camera poses from `main_menu_nis` / `character_select`. |
| `tetherball_play.rs` | The tetherball match started from the front end, with the original `TetherballHud` movie as its HUD. |

## What is the original and what is not

Original, executed as shipped: every movie, every script, all screen transitions, fonts, colours, animation timing and
button behaviour. Strings come from the shipped `locale/*.loc` through the recovered hash lookup.

Recovered from data: the roster of selectable kids (`character_select/character`, `bestiary/g_*`: model, gender, name key,
unlock links), the menu camera poses, the world, the kid models and animation clips.

Written for this port (not 1:1, listed so they are not mistaken for recovered behaviour):

* Kid scene: multiplayer slot 0 has no depth in the vault, so it uses its select-kid spot; the hover animation (`S_PickMe_01`)
  and the meaning of the Boys / Girls buttons (here: the other gender stays unpickable) are guesses.
* Nothing stands in for missing strings any more: the executable renders a `$KEY` text whose key is absent from the shipped tables as
  the bare key (`AIP::AllocateStringLocalized` -> `FEManager::GetLocalizedString` `swprintf`s `%s`), so `B_Boys`, `B_Girls`,
  `VK_ESC`, `VK_DONE`, `HT_Caps_On` and `HT_Caps_Off` read exactly like that in this data set. (`B_CreateProfile` / `B_EmptyProfile`
  do exist: "MAKE A NEW PROFILE" / "NOTHING TO ERASE".)
* Progress data (report card counts, sticker book contents, unlocked minigames): the port has no progression system, so
  the screens show empty values. Only Tetherball is playable from the front end.
* Save data: profiles persist in `saves/profiles.json`, not in the Wii save format.
* The Wii pointer is the mouse; the keyboard maps to the Wii button codes of `fw.datatypes.KeyCode`.
* Stereo UI sounds are played as their first channel (the EA-XA decoder is mono).

Recovered from the executable and now implemented: the sound table (`fe_sfx.rs`), the name keyboard step (`TRC::OpenKeyboard(7, ...)`
with `T_NameKid`, the kid's name as default), and the button-icon substitution (`'X button'` -> `auxgraph.gfn` glyph `'A' + n`).

## Screens and flows

Verified end to end by scripted runs: Title, Profile (create / select / erase), SelectKid with the 3-D kids, ConfirmKid, MainMenu with
its camera moves, Multi-Player (player count, player setup, game select, rules, pre-game, tetherball with the original HUD,
post-game, pause), Quick Play (game select, rules, instruction book, match, `PostGame`), Extras / Credits (text from the
locale keys `T_Credits_*`), and Single Player (the world behind the menus becomes playable; the original `WorldHud` movie draws the report-card (-) and sticker-book (+) icons, `-` / `+` open those screens, Esc / P opens the original pause overlay and Quit returns to the main menu). Rendered with
data but without gameplay behind them: ReportCard, sticker book cover, sticker store cover, end game, select plane.

## Known gaps

* No blend-mode data was found in the APT place records (only colour multiply/add), so everything draws as alpha; masks are limited to 48 triangles per mask layer and nested masks use
  the innermost one.
* Only Tetherball is playable from the menus. The other six minigames stay locked; the sticker book, sticker store, boss
  select, conversation and world HUD screens have no game logic behind them.
* The world played from Single Player is the earlier playable slice (provisional camera and collision, no NPC conversations, no POIs), so `PressA_SetVisible`, dialogue and the sticker/report data have nothing driving them.
* UI sounds are triggered by name; their volume, and the loops (`UI_Loop_Sfx`), are not modelled.

## Test aids

`--apt <movie> --apt-script "tick:code,..." --shot out.png --shot-at <secs> --apt-fwprint --apt-inspect <path;path|tree>`
drives and captures the front end headlessly. Script codes: a number is a Wii key code (+1000 releases), `mX_Y` moves the
pointer in movie pixels, `d`/`u` press/release the pointer button, `cPATH` points at the centre of a clip, `xPATH` hides a
clip, `oNAME` opens a screen, `pause` opens the pause menu. `run_flow.sh` wraps this, and the `flow_base*.txt` files hold
click paths that were verified end to end. Calls the scripts make are written to `docs/apt-log.txt`; calls the host does
not know show up there as `unhandled game call`.
