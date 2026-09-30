# Tetherball serve and active-character selection

`src/tetherball_serve.rs` ports the complete `MGTetherball::UpdateServe`
routine and the scalar body of `SetActiveCharacter`. The source of truth is the
pinned executable `Remaster/reference/playgroundz.elf`, SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

| Routine | Address | Recovered behavior |
|---|---:|---|
| `UpdateServe` | `0x80397b74` | Full 509-instruction body, including its timer, human/AI serve selection, angle transition, and pause-event loop |
| `SetActiveCharacter` | `0x8039a91c` | Always writes the requested server; writes the receiver for selectors 0 or 1 and preserves it for other raw values |
| `StartRumble` | `0x8039c76c` | Reached after controller soft-lock recovery; its attached-controller check and `150 ms, 0.5` rumble request are represented at the service boundary |
| `OpenPauseMenu` | `0x803abac4` | Guarded pause state, four-word pre-game payload, front-end flags, overlay, sound, and audio pause |
| `rmAngle::IsBetween` | `0x802ecc6c` | Called through the shared angle port with the wrapped ball angle and unmodified stored endpoints |

## Shared state and inputs

The function mutates shared state directly. `Lifecycle` owns the active server
and receiver (+0x214/+0x218), focus player (+0x21c), controller/action state,
pause state, round timer, banners, ball elapsed time, and serve-bubble flag.
`ResetState` owns the distinct base-player count (+0x70), serve kind (+0x224),
serve counters (+0x260/+0x264/+0x270), +0x25c, and the normal/power speed
tunables (+0x34c/+0x354). `GestureState.hit_attempt_marker` is the authoritative
+0x229 byte. `BallMotion` owns the tossed flag and the exact Toss/Serve and
high/power-serve predicates. The reset state's similarly named +0x229 shadow
is not changed by this handler.

`ServeState` carries the remaining values: pause-block count (+0xfc), pause
menu flag (+0x4e), power-serve enable (+0x42c), the two raw return-angle
endpoints (+0x23c/+0x240), power/high animation IDs (+0x1c8/+0x1e0), per-player
voice type (+0x1e8), AI waiting bytes (+AI +0x6c), static forced-AI routing
bytes (`0x80607e78..79`), and front-end flags (+0x48/+0x49). The controller
iteration count comes from `Lifecycle.session_mode` (+0x40); it is separate from
the tetherball character count and the base-player count.

`ServeInputs` supplies the signed frame delta, ball world position (+0x40), and
the normal/power effect names stored at +0x2fc/+0x304 for each focus player.
`ServeServices` extends the lifecycle services with synchronous controller
event and audio-azimuth reads, timer and Wii sound calls, camera shake, rumble,
serve-particle requests, and pause front-end/audio requests. The inherited
services cover the banner, animation, serve-bubble, sound, controller, camera,
and nested state-entry calls. This keeps engine objects explicit while the
serve timing, ball predicates, state stores, and selection logic execute in
Rust.

## Ordering that affects behavior

Paused calls return `true` before touching the timers. Otherwise the round
timer is decremented with signed 32-bit wraparound; a negative result hides the
scoreboard and round banner in that order. A pending serve latch (canonical
byte value 1 in the shared bool projection) is processed only when its signed
delay counter is positive. Once that counter expires, a
tossed ball clears elapsed time, marks +0x25c, clears +0x270, and shows the
timer. Serve kind 5 emits its normal sound, serves at +0x34c, optionally plays
Wii sound 9, then creates and schedules the focus player's normal serve FX.
Kind 6 emits sounds 0x18, 0x17 and the character-specific voice sound (9 or
10), optionally plays Wii sound 10, serves at `+0x34c * +0x354`, shakes the
camera for 400 ms at 0.2, then emits the power FX. Both serve kinds preserve
the original call order. Controller recovery follows this common serve block
when the active player has a controller and its forced-AI byte is clear.

If the delayed request expires while the ball is not tossed, the serve bubble
is shown only when needed, then +0x228 and +0x260 are cleared. The tossed-ball
arm does not clear those two fields. This distinction matters because the
serve request remains pending through its actual serve.

Animation 60 and the active player's configured power/high serve animations
skip human/AI serve selection. A human toss requires action 1 and an untossed
ball; it selects animation 59, calls Toss, sets +0x264 to 500, and hides an
already visible bubble. While tossed, a positive signed +0x264 counter is
decremented and suppresses the high-serve predicate for that call, even if the
decrement reaches zero. A nonpositive starting value tests the original
110-ms high-serve predicate. If the serve kind is not already 5, controller
event 0x5c and the power-enable/global-win flags choose kind 5 or 6, then
animation 81 is requested. An untossed human selects animation 58, shows a
hidden serve bubble, and stores action 2.

The AI route is selected when there is no controller or the active character's
forced-AI byte is set. Its toss gate is strictly `state_ms > 2000`; its serve
selection gate is strictly `state_ms > 2800` using the unsigned state timer.
The toss does not write +0x264. At the later gate it sets +0x228/+0x260, then
checks power serve, high serve, and fallback in that order, selecting the
matching stored word and animation. Hiding the AI serve bubble leaves the
game's serve-bubble byte unchanged. This route does not write the active
character's action word; the common later store targets only the focus
character (+0x21c), so the active word changes only when active and focus
refer to the same character.

After serve selection, the focus character's action is set to 2. The ball angle
is wrapped with the original `rmAngle` repeated-f32 operation before
`IsBetween`; endpoints are passed as stored, and the reverse argument is true
when focus is player 0. A crossing clears the gesture hit-attempt marker,
selects the current receiver as the new active server, clears that character's
AI waiting byte, and synchronously enters state 28. The pause-event loop runs
after either angle outcome, including after that state entry. It queries event
0xaf for every controller index below +0x40, and opens the pause menu only
when the menu is closed and signed pause-block count is nonpositive.

The raw `SetActiveCharacter` helper retains invalid selector stores exactly:
it always replaces +0x214 and leaves +0x218 alone unless the selector is 0 or
1. The `Lifecycle` adapter accepts the valid player indices 0 and 1 required by
the recovered UpdateServe path; it does not permit an invalid index to enter
the shared array-backed state.

`OpenPauseMenu` repeats its guards and sets the pause/menu bytes before the
front-end calls. Its 16-byte `PreGameInfo` words are `[1, controller,
base_player_count > 1, game_type]`; the third word uses a signed comparison
against the distinct base count at +0x70. The function then requests
`SetupPreGameHandlers(2, 0, payload)`, sets front-end flags +0x48/+0x49,
opens `PreGameInstructions`, plays front-end sound 12, and pauses audio mode 2.

## Oracle evidence and limits

`tools/tetherball_serve_oracle.py --check` executes the original PPC body and
its unhooked ball, angle, state-entry, winner, SetActiveCharacter, StartRumble,
and OpenPauseMenu graph. The 672 UpdateServe calls execute all 509 instructions
and cover both outcomes of all 45 conditional branches (90 recorded branch
outcomes), including 18 nested state-30 transitions and explicit cases where
the active player differs from the focus player. The raw active-character
helper is checked with 21 cases spanning valid and invalid selectors. The
Rust snapshot adapter compares the complete lifecycle and ball, shared reset
fields, gesture marker, auxiliary bytes/words, return value, and ordered service
trace against the golden corpus.

`Lifecycle.latches` represents the native latch bytes as booleans. The serve
gate in PPC accepts exactly byte value 1; arbitrary noncanonical byte values
are outside this shared projection.

The controller event/azimuth values and engine outputs are fixture inputs or
service results. Rendering the effects, actual controller devices, animation
graphs, audio playback, camera behavior, and Apt front-end behavior remain host
responsibilities. These tests establish differential agreement for the
recorded deterministic corpus, not formal proof for every pointer graph or
hardware state.

This does not yet make the minigame playable: Return, Accelerate, their AI/hit
dependencies, and the Bevy host integration remain unfinished.
