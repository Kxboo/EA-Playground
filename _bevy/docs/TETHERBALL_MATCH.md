# Tetherball match decisions

`src/tetherball_match.rs` ports the winner decisions in the retail executable
with SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
This is a pure state/effect API, not a playable minigame or complete state machine.
Callers provide the original rule values; the port invents no default rules.

| Routine | Address | Coverage |
|---|---|---|
| CheckIfGameIsOver | 0x8039af5c | Rule dispatch: best-of, time survive, otherwise no action |
| CheckIfGameIsOver_BestOfNRounds | 0x8039af7c | All winner, counter, result and UI-selection decisions |
| CheckIfGameIsOver_TimeSurvive | 0x8039b0c4 | All rotation-transfer, timeout, winner and counter decisions |
| ChangeGameState | 0x8039a988 | Common state-code and two timer stores only |

Best-of checks player 0 rotations first, then player 1, then a positive time
limit. Timeout awards player 1. Rotation and round-win counters are signed
bytes, and increments wrap at eight bits. The match ends on equality with the
required wins, not on greater-than-or-equal. An earlier successful result word
survives a later player-1 loss, while the separate final-result word updates.

Time-survival first transfers one player-0 rotation to player 1 when the former
reaches the limit. Timeout then takes precedence over player 1 reaching the
rotation limit, and awards player 0. It has no positive-time guard: a zero
limit expires immediately. Calling it again with the match-over flag set can
increment the winner's counter again. The eventual host must obey the original
state dispatch rather than repeatedly evaluate a finished match.

The API returns ordered winner-visibility and state-entry requests. It applies
the common state transition (state 30 and zeroed state timers), but leaves the
state-specific ball transforms, camera, win animations, APT UI, score placement,
round reset and post-game flow to their future ports. Match-over states supplied
to time-survival must have a valid winner index; Rust safely ignores an invalid
index where the original would access outside its player-counter array.

`tools/tetherball_match_oracle.py --check` executes the three complete original
decision routines and the common ChangeGameState stores on prepared memory.
Only the two external UI calls are recorded as effects. After the common stores,
the oracle branches to the original ChangeGameState epilogue, skipping its
state-specific model, animation and UI work. None of the decisions or arithmetic
under test is replaced with a Python model.

The golden fixture contains 300 prepared states and 900 transitions, including
simultaneous thresholds, exact timeout boundaries, negative/overflowing time
limits, signed-byte overflow, sticky result fields, unsupported rule modes,
and repeated calls after a result. Rust compares every represented field and
ordered effect after each transition. This is bounded deterministic evidence;
it does not establish whole-function equivalence for ChangeGameState.
