# Playable Tetherball (recovered runtime in the real world)

Started from the front end (Quick Play, Multiplayer) the match is **the recovered `MGTetherball` frame graph**
(`tetherball_runtime::Runtime`: startup, pregame, intro, resetting, serve, return, accelerate, round end, post-game,
wait) hosted by `tb_host::TbHost` and shown by `tb_session`.  It is played at the pole placeable of the loaded world
(`tetherball_pole_school_w_ball` / `_stadium_` / `_forest_` positions from `db.vlt`), with the original `TetherballHud`,
`PostGame`/`PostGameMP`, `PreGameInstructions` (pause) and `GameRules` screens.  The old stand-alone composition
(`tetherball_play.rs`) remains only behind the developer menu entry.

| Piece | File | Status |
| --- | --- | --- |
| Game rules, AI hit planning, serve/rally/round logic, reset, startup | `tetherball_*.rs` | **original** (PowerPC-verified) |
| Animation state graph: `player.csv` rows, `SetNextAnimState` / `Update` arithmetic, blend from frozen pose | `tb_anim.rs` | **original** arithmetic; events only fire on time crossing |
| `BehindTheBackCamera::Update`, `SoftTransition`, `SetDir`, setters | `tb_host/camera.rs` | **original** |
| Controllers: `controls.csv` + `controlsmgtetherball.csv` through the verified `Controller` (state stack, events 0x5c/0x5d/0xaf) | `tb_host.rs` | **original** |
| Engine services (handles, db reads, part-fx bookkeeping, spawn/AI slots, markers) | `tb_host/services.rs` | host glue |
| AI compulsion scheduler loop, locomotion toward the move target, once-per-frame `current_animation` sync | `tb_host/frame.rs` | **provisional** (recovered `evaluate/think/has_expired` are called; the loop and walking are written) |
| Rules rows (`SetTetherballRule`), defaults from `mg_tetherball/default_rules` | `fe_host.rs` | **original** strings/options; all areas selectable (no visited-area data) |
| Keyboard -> Wii buttons / Conga gestures | `tb_session.rs` | provisional mapping |

Keys: Space swing / serve toss, X backhand, Z overhand, Left Shift = hold A (power), Left Ctrl = hold B, both = mega, Esc/P pause;
second player: Enter, `.`, `/`, Right Shift, Right Ctrl.  `EAGL_TB_AUTO=1` plays seat 0 with a bot, `EAGL_TB_AREA=0..2` picks the
area, `tbN` in `--apt-script` starts a match for N players, `EAGL_TB_DEBUG=1` writes `docs/tb-log.txt`.

## Known gaps

* Gameplay sounds (`Services::sound` ids of `mg_tetherball.abk`), voice lines and rumble are reported by the host but not played.
* Shadows are the casters flattened onto the ground (`present_shadows`); the original renders them top-down into a 2.5 x 2.5 texture
  with a colour vector we do not apply (flat black at alpha 0.62), and overlapping parts darken each other.
* Footsteps use the first surface sound: the ground type comes from a Havok ray cast (`Character::UpdateWalkSurfaceType`).
* The ball is attached to the right-hand bone; the original uses marker 0x3f (not decoded).  Character locomotion speed/facing is a guess.
* `current_animation` reaches the recovered logic one frame late (the modules read it as plain data).
* Single-player entry from the world (point of interest) and progression (visited areas, unlocked abilities) are not implemented.
* Wii pointer/Conga gesture recognition is replaced by key presses.
