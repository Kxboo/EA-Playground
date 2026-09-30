# Tetherball lifecycle, state entry and results

`tetherball_lifecycle.rs` composes the existing winner decisions and ball
arithmetic with the retail match's state-entry logic. Evidence uses the pinned
ELF SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

| Routine | Address | Native coverage |
|---|---|---|
| ChangeGameState | `0x8039a94c` | Complete dispatch and all entry bodies; nested winner transitions execute synchronously |
| UpdateIntro | `0x803979e4` | Initial-pair animation, strict unsigned time/HUD gate, ordered scoreboard reset/show and transition |
| UpdateResetting | `0x80397ae4` | Strict first-round wait, state entry, ordered meter resets |
| UpdatePlayerDistance / SetPlayerDistance | `0x8039aeb8` / `0x8039c6dc` | Rotation thresholds, fixed-distance modes, character/AI fields, radius rescaling |
| InitializePlayerWinAnimations | `0x8039c07c` | Every selection and facing flag; random draws supplied at the engine boundary |
| CelebrationAnimsAreFinished | `0x8039d0bc` | All current-animation/special-animation comparisons |
| AdjustCameraHeight | `0x8039b504` | Exact distance/zone offsets and ordered camera requests |
| UpdateRoundEnd | `0x80399108` | Facing, celebration/time gates, next-round/reset call, full score/placement/result payload |
| Update | `0x80397640` | Complete clock, dispatch and indicator orchestration; delegated handlers remain explicit |
| Tetherball::SpinUpPole | `0x8039e8c0` | Radius choice, rescaling, target height and signed angular velocities |

## State ordering

`UpdateIntro` uses the initial server/receiver at +0x21c/+0x220, distinct
from the active pair at +0x214/+0x218. It clears +0x25c every call, requests
server animation 58 only from current state 0, and always requests the receiver
animation from +0x190. Time must be strictly greater than 1000 (unsigned) and
the HUD ready before scoreboard reset/show and transition to state 26.
The separate `IntroState` retains +0x220, +0x25c and +0x444 without conflating
them with active-player or UI flags.

State entry always stores the new code and clears both state timers. Only 9 and
26–30 have additional entry bodies. Entry 27 configures the serving player's
controller, animation, round banner and meters. Entry 28 hides round/score UI,
updates distance and checks winners. Entry 29 increments the server's signed-byte
rotation counter and decrements the receiver's, emits the point particle,
updates the indicator and distance, then checks winners. Winner checks can
immediately enter state 30 before the outer call returns.

State 30 preserves the distinct endurance-dare behavior (game types 6–8 when
player 1 did not win). Otherwise the ball spins up the pole. The SpinUpPole
radius is .25 only when the runtime singleton passes the tetherball tag check
and has variant 2; absent/other context uses .18. Radius rescaling occurs before
the sign-dependent velocity is set to ±12. Target height is pole height +1.5.
The singleton result is an explicit `Option<i32>` input, not guessed from the
current match's variant field.

Distance mode zero maps absolute signed server rotations 0–1, 2–3 and 4–5 to
distances 2, 1 and 0; values at least 6 leave distance unchanged. Modes 1–3 force
distances 0–2. An actual distance change sets both character speeds to 1.2,
updates their AI distance fields, requests AI control and rescales the ball
toward radii [.7, .9, 1.1]. The full movement/AI engines remain dependencies.

The exact outer Update jump table matters: state **3 runs UpdateIntro**, while
state **26 runs UpdateResetting**. State timers advance even while paused.
Gestures and ball Update run only if initially unpaused; state dispatch still
runs while paused. Elapsed match time advances only in unpaused states 27–29.
After the handler, pause is read again before the indicator/base update. The
indicator moves by .01 per call, independent of the supplied delta. A state
without a dispatched handler leaves the original return register unset;
the Rust API returns `None` for that domain.

## Animation and round-end behavior

Nonfinal rounds select animation 225. A final match with the original player
record flag set selects 95. Other final-match players may select their special
animation if present and a [0,99] draw is below 50; otherwise a [0,4] draw indexes
the original table [85,225,226,227,228]. Draw ordering and the animation-85 facing
exception are retained. Celebration completion compares current state against
all five entries and the player's special animation, including a supplied -1.

The winner turn lasts through the exact 250/4750/5000 ms boundaries, wraps the
resulting angle and writes both facing and the EA polynomial direction vector.
Round progression requires time strictly greater than 5000 and finished
celebrations. An unfinished match resets the ball zone, increments the round,
calls ResetRound synchronously, enters state 3 and hides winner UI.

A finished match clears the HUD, closes the screen and enters state 8. It
constructs the exact zero-filled 0x118-byte PostGameInfo payload, including
wrapping integer accuracy/score arithmetic, original 1-vs-1 placement ordering,
single-player round wins and the context-dependent result/display fields.
The API exposes its big-endian words so unknown padding/fields are not guessed.
Valid scoring inputs require nonzero attempt counts, matching the meaningful
domain of the original signed divide; invalid memory indices are not fabricated.

## Boundaries and differential proof

`Services` supplies UI, controller, animation, camera, particles, AI takeover and
engine random draws. The point effects are `pg_tetherball_point_blue` for player
0 and `pg_tetherball_point_red` for player 1, followed by 2000 ms destruction.
`FrameServices` exposes gesture processing, ball update, state handlers, pole
indicator rendering and base-minigame update. These service calls preserve
orchestration; they do not make unported handlers playable automatically.
ResetRound is a synchronous dependency so its separately recovered state can
be mapped into the lifecycle before the next transition.

The companion [reset port](TETHERBALL_RESET.md) updates the shared lifecycle
and ball while retaining its additional object fields. Its ordered engine
requests must be applied at that boundary. Carry its +0x220/+0x25c/+0x444 fields
into `IntroState` for the following state-3 handler. The [gesture port](TETHERBALL_GESTURES.md)
uses shared controller/action fields; resetting its live records preserves the
pending count and +0x229 marker. The [tuning loader](TETHERBALL_TUNING.md) supplies
corpus-derived values for these operations. These components still require the
remaining state handlers and engine adapters to form a playable Bevy minigame.

`tools/tetherball_lifecycle_oracle.py --check` regenerates **1,280 original calls**:
state entries (including invalid dispatch codes), intro, resetting, win selection,
celebration completion, distance changes, camera zones, round-end progression
and outer Update ordering. The Rust test compares all represented state and
ball fields, every float by bits, ordered service arguments, result payloads
and defined return values with zero tolerance.

Original ChangeGameState, nested winner functions, ball operations, animation
selection, celebration checks, score calculation, placement construction,
angle/vector helpers and camera decisions execute unhooked. The original
`__sinit_mgtetherball` (`0x8039d134`) initializes camera globals; zero-filled BSS
is not substituted for those constants. RoundEnd's ResetRound call is recorded
as the explicit dependency. Outer-Update-only cases substitute recorded
handlers, gestures, ball update, indicator draw and base update to verify their
ordering; separate cases execute the actual recovered entry/round-end bodies.
The handler boundary also changes pause to test the post-handler reread.

The tested domain has 0–2 players, valid player indices and finite, nonzero ball
radii. This is original-code equivalence under the documented PPC interpreter,
not a running-console visual comparison or a completed playable minigame.
