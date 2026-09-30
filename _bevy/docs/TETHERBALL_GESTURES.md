# Tetherball gesture input and hit attempts

`src/tetherball_gestures.rs` ports the queued Conga callback records and the
controller-event sampling stage from the retail executable with SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
The module produces ordered state changes; it does not turn a button press or
gesture into a ball hit.

| Original routine | Address | Ported behavior |
|---|---:|---|
| `RegularStrikeProcess` / callback | `0x8039c7c4` / `0x8039c884` | Pause and enable gates, selected-player controller match, queue type 0 |
| `RegularStrikeReverseProcess` / callback | `0x8039c8b0` / `0x8039c970` | Same gates, selected reverse player, queue type 0 |
| `OverhandStrikeProcess` / callback | `0x8039c99c` / `0x8039c9fc` | Pause and enable gates, queue type 0 |
| `ServeTossProcess` / callback | `0x8039ca28` / `0x8039ca88` | Pause and enable gates, queue type 1 |
| `ProcessGestures` | `0x8039b674` | Ordered queue-to-player action stores and pending-count clear |
| `GetPlayerHitAttempt` | `0x8039b76c` | Zone-selected attempt flag and power-hit event classification |

All four callback wrappers return while the global minigame pause byte is set.
Their process functions then check the callback-enable byte and require fewer
than seven pending 0x0c-byte records. Regular and reverse strikes also compare
the callback controller ID with the selected player's attached controller;
overhand and serve-toss do not. Records are appended in callback order with
zero auxiliary float. The process functions do not resolve a hit.

`ProcessGestures` walks queued records first and players second. Kind 0 stores
zero and kind 1 stores one in the matching players' action word. Unknown kinds
are ignored. Multiple matching players are all updated, and later records can
overwrite earlier values. The queue count is cleared after every invocation,
including an empty queue. The routine itself has no pause test; outer
`MGTetherball::Update` skips its call while paused.

`ResetRound` overwrites each live queue slot with kind 2, zero auxiliary data,
and controller ID 0, while preserving the pending count and attempt marker. The
`reset_pending_gestures` adapter mirrors those retained fields; the subsequent
queue consumer ignores the reset kind and clears the count.

`GetPlayerHitAttempt` is a separate stage. If the selected player's action word
is zero, it sets one seeded output byte according to whether the tetherball
zone word is zero and sets the minigame attempt marker. It then samples
controller events 0x5c and 0x5d in order: those set power types 1 and 2, while
both together select type 3 only when the mega ability byte is set. The output
bytes and power type are not cleared by the original routine, so callers supply
their initial values. The original routine ignores its nominal player argument
and reads active player +0x214; `get_lifecycle_hit_attempt` therefore takes that
active index explicitly instead of assuming it equals the server role.

In the original `UpdateReturn` path, this sampling follows waiting-swing and
ball-range checks and is followed by animation-state checks. `StartHitAnimation`
selects animation states; the later state/timer handler invokes
`HitTetherball`. Charge consumption, exact hit-range and angle tests, AI hit
selection, animation event timing, effect/audio dispatch, and the complete
`UpdateServe` / `UpdateReturn` handlers remain outside this module. Integration
must preserve those stages and gates before calling the existing ball `hit`
routine.

`tools/tetherball_gestures_oracle.py --check` executes the original callback,
queue-consumer, and hit-attempt PowerPC routines. It hooks only the controller
lookup and event-state service boundaries. The golden corpus includes accepted
calls for all four callbacks, controller mismatch and full-queue rejection,
duplicate controller IDs with the original ordered action-store trace, and all
32 combinations of zone, action, event pair, and ability state (112 transitions
in total). It compares
queue records, player action words, store ordering, attempt markers, seeded
outputs, and transitions against the ELF; this is bounded deterministic
evidence rather than formal verification over every pointer graph.
