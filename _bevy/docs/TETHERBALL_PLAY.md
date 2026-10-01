# Playable Tetherball

`EAGL-Workbench.exe` -> **Play - Tetherball** (or `--mode tetherball`). One human (Alicia) against an AI opponent
around the decoded `tetherball_pole.o`, with the decoded `teatherball.o` ball.

| Key | Action |
|---|---|
| Space | Serve / swing / rematch |
| Shift+Space | Power hit (costs 2 charge) |
| Ctrl+Space | Mega hit (costs 5 charge) |
| 1-4 | Difficulty (Easy..Expert), between rallies |
| Q/E or arrows | Rotate the view |
| R / Esc | Restart / back to menu |

Swing while the ball is inside the green window in front of you: a hit reverses the ball so it winds your way.
A full turn the other side fails to answer moves the shared winding count toward you; the first to reach the
tuned number of net turns (6) wins. A swing outside the window is a miss and slows the ball 5%.

## Evidence

* Original: models, textures, character rig/animation, `BallMotion` (serve, hit, miss, motion update; compared with the
  PowerPC code in `tetherball.rs`), per-difficulty hit speed / power / mega modifiers, rotations-to-win, the hit-angle
  delta used as the swing window, and the AI's too-fast / too-slow / wrong-height / power chances (all read from
  `db.vlt` through `tetherball_tuning`), plus the per-difficulty speed cap from `HitTetherball`.
* Provisional: charge costs, the winding-count rule, AI timing, the camera, the rope drawn as a straight cylinder, and the
  fixed stance distance (the stored `distance_from_pole` is 0 in the corpus). The full recovered minigame state
  machine (`tetherball_runtime.rs` etc.) is not yet driven by a Bevy host; this mode is a playable composition of its
  verified parts.

`--tb-autotest` plays a full match with a perfect-timing bot against the AI and writes `docs/tetherball-autotest.json`.
