# Composed tetherball runtime

`src/tetherball_runtime.rs` provides one state owner and frame entry point for
the recovered game. The caller supplies initialized state, original data and
engine services; there is no fixture-backed startup or guessed game default.
It is not yet connected to the interactive Bevy world.

`Runtime::update` calls the original outer Update port at `0x80397640`. The
unpaused path consumes the live gesture queue, advances the complete ball scene,
then chooses a handler using the current state. All nine native states are
connected: pregame (1), intro (3), postgame (8), exit fade (9), resetting (26),
serve (27), return (28), accelerate (29), and round end (30). Timers, nested
state transitions, pause checks and pole-indicator interpolation retain the
native ordering. Hit/serve particles use the position produced by this frame's
ball update rather than a previous frame's copied input.

The native outer function saves the handler's full return register in r29 and
returns it unchanged at `0x8039781c`. Its Rust return is now `Option<u32>`:
the exit fade returns 2 when finished, not a boolean. Undispatched states retain
the existing `None` representation for an unset native return register.

The base `Minigame::Update` at `0x803ab4f0` is also composed. If the separate
World singleton's pause byte is clear and +0xfc is positive, it subtracts the
signed frame delta with wrapping word arithmetic and no clamp. World::Update
remains a host call. The world pause input is distinct from Lifecycle.paused;
the singleton pointers are +0x8c and +0x90 respectively.

Intro projects +0x220/+0x25c/+0x444 from ResetState and writes its changes back.
Frontend and serving share the pause counter/menu flag and frontend flags in
ServeState. Round/postgame resets compose the native reset, animation initializer
and AI initializer before the following state transition. Live scene trail
handles are synchronized into the legacy reset projection; the gesture queue's
length and attempt marker are preserved while its live records are reset.
InitializeAptHud changes the real frontend flags and emits its ordered requests;
it does not fabricate the asynchronous HUD-ready callback.

Reset effects retain the existing batch contract: their explicit external
results are supplied up front and engine dispatch must not reenter the game.
Database failure is reported and makes that incomplete match unusable; updates
are not transactional. AI scheduling, character movement application, animation
playback, particles, Apt rendering and platform/device adapters remain host work.

`tools/tetherball_runtime_oracle.py --check` runs 180 complete frames in the
pinned original executable. It executes ProcessGestures, full Tetherball::Update,
all nine handlers and Minigame::Update from their original instructions. Engine
services, World::Update and the pole-render request are recorded boundaries.
Cases cross pause state, world pause, frame deltas, live queued callbacks,
attachment feedback, shadows/trails, cooldown thresholds and exit statuses.
Full lifecycle, ball bits, scene matrices/positions/trails, frontend/intro fields,
queue count, rally fields and ordered effects are compared by the Rust runtime.
These frames exclude reset entry, which has separate composition fixtures.

`tools/tetherball_runtime_reset_oracle.py --check` adds 24 combined round/full
resets. Original player-animation and HUD initialization run together with AI
construction/initialization and corpus-backed difficulty reads. The Rust test
poisons the legacy reset's trail handles, pending count/marker and rotation cap,
then verifies the live owners override them. It compares reset state, ball bits,
animation tables, AI charge/configuration, HUD flags and ordered engine effects.
Live queues of length 0 through 6 retain their count and marker while all records
become kind 2/zero/controller 0. The existing AI oracle adapter supplies freshly
constructed allocation storage; engine allocation/binding remains external.

The executable SHA-256 is
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
This is instruction-emulation evidence, not a running-console gameplay comparison.
