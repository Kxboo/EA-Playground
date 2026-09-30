# Controller event dispatch

`src/controller.rs` reconstructs the original digital button timers and CSV row
predicates. Feed `Controller::update(held_mask, input_ms)` the **uncapped** signed
Controller delta, not the capped simulation step. Buttons occupy bits 0 through 13
in the exact `GameMap/data/enums/input_buttons.tsv` order. This module consumes
already translated logical button bits; Wii hardware, analog normalization,
accelerometer filters, rumble and Apt frontend queue integration remain outside it.

Evidence comes from `playgroundz.elf`, SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`:

| Instructions | Behavior |
|---|---|
| Constructor `0x8032a330–0x8032a368` | 14 held timers start at zero; since-down and between-down arrays start at `0xffffffff` |
| GetInput `0x80329fdc–0x8032a098` | derive release/down from previous held mask; update all three timer arrays |
| UpdateInput `0x8032cb58–0x8032d23c` | clear 189 events, test rows in order, publish event values, push/pop context |
| PopState `0x8032dc20` | immediately pop if stack depth is greater than zero |
| SetCurrentControllerState `0x8032dc38` | push a context |

The row's bytes `+0x04..+0x43` are not used by these event predicates. Runtime
state lives separately: Controller `+0x258` points to since-down timers, `+0x25c`
to intervals between the last two downs, `+0x260` to held timers, and `+0x268` to
189 eight-byte event records (active byte and held-time word). CSV definitions
start at `+0x26c`, stride 100, and row count is `+0xca6c`.

On a down edge held time resets to zero, previous since-down time becomes the
between-down interval, and since-down resets to zero. On subsequent held frames
held time adds delta. On release it retains its previous value; the release
frame's delta does not add to it. Since-down adds delta on every non-down frame,
including idle frames. All timer arithmetic wraps as 32-bit words. Initial
`0xffffffff` therefore wraps after an idle update; the port preserves this
unusual initial-double-press behavior instead of inventing a validity flag.

Every kind requires all required modifiers held and all forbidden modifiers
unheld. **Modifier value zero is ignored**, so UP cannot be used as a modifier.
The primary UP button is valid. Thresholds below are the ELF's default tunables:

| Kind | Original predicate |
|---|---|
| UP (0) | release edge |
| DOWN (1) | down edge |
| DOUBLEDOWN (2) | down edge and between-down interval strictly less than 500 ms |
| PRESSED (3) | currently held |
| SECOND (4) | held time at least 1000 and signed wrapping `held_time - delta` less than 1000 |
| TAP (5) | release edge and held time strictly less than 180 ms |
| HOLD (6) | held time at least 180 and unsigned wrapping `held_time - delta` less than 180 |
| HOLDPRESSED (7) | held time at least 120, on every held frame |

HOLDPRESSED is not itself a periodic repeat scheduler. The signed/unsigned
comparison difference for SECOND and HOLD comes directly from `cmpwi` at
`0x8032cf3c` and `cmplw` at `0x8032d098`. Unknown kind 9 never fires.

Events clear each update. A firing row sets active and copies its primary
button's held timer. Multiple firing rows for one action overwrite the held
value in row order. A row is eligible in the frame's starting context or ANY
(28); an ANY row is suppressed when its transition equals the pending context.
Transitions are applied only after all rows have run, so a pending transition
does not activate rows for its destination in the current frame. Transition 31
means no change; RETURN (27) pops. The last firing transition wins. Debug menu
(8) and free camera (6) require pad zero and their respective enable flags.
The event still fires when its transition is rejected by those gates.

The safe API retains the root context on a RETURN underflow and refuses pushes
past the original 64-slot capacity. It returns an inactive event for unknown
indices and ignores unsafe rows with a primary button outside 0..13; the original
uses unchecked memory. Initial state is explicit (the original constructor fills
its context stack with COMBAT=3). The three threshold values are currently the
original defaults, not exposed as live tunables. The unused internal `+0x248`
deferred-pop flag is not modeled. `pop_state()` follows the actual immediate
PopState routine.

`tools/controller_oracle.py --check` verifies 303 original-machine frames across
all 14 logical buttons, all eight event kinds, threshold neighbors, release timer
retention, modifiers, signed/wrapping deltas, event reset, context ordering,
ANY debounce, RETURN, and debug/free-camera gates. The oracle replaces hardware
GetInput at its call boundary with the **original timer instruction slice** and
runs original UpdateInput through its completed context decision. A checked PC
jump at `0x8032d240` enters the original epilogue, excluding subsequent
rumble, shared frontend D-pad repeat globals, Apt queue processing, and the
external deferred-pop flag (zero throughout these fixtures). It does not hook or reproduce event/timer
arithmetic in Python. Golden fixtures store outputs, never executable contents.

## Native CSV loading and Bevy integration

`src/control_bindings.rs` loads all 14 `controls*.csv` variants. The separate
`tools/control_bindings_oracle.py --check` executes original `Controller::Initialize`
and `cCSVParser` on the private corpus and records 506 numeric bindings, plus four
synthetic cases. Only file I/O, CString storage/comparison, allocation, and compiler
register helpers are adapted. Original code performs tokenization, enum conversion,
and row construction. Input file hashes are pinned in the fixtures.

Fields match the first header with the exact name. Fields trim their left edge;
the entire line trims its right edge, so only the final field loses trailing
whitespace. The trim functions treat bytes outside printable ASCII as whitespace.
Quotes are literal, and lines starting with `//` and longer than two characters
are comments. Unknown event/state/kind/button tokens become 190/31/9/0. Required
and forbidden modifier slots preserve the original zero sentinel. The disabled
`EVENT_PLAYER_FACEDIRECTION-NOTUSED` row is retained with event 190. Unsafe original
buffer limits or missing headers return a Rust error.

The Bevy slice loads `controls.csv` into context COMBAT=3. Its desktop adapter
maps WASD/arrows to D-pad bits, Space to C, and R to B. Recovered events
178–181 drive movement, event 2 emits the original inert playground jump command, and event 7 triggers
camera reorient. Dispatch runs after world movement/animation, preserving the
original next-frame event consumption order, and uses uncapped input milliseconds.
The original free-camera/debug gates are disabled until those modes are ported.
The scripted rendered self-test also passes through this controller path.
