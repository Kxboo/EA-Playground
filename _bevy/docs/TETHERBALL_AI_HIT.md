# Tetherball AI hit compulsion

`tetherball_ai_hit.rs` ports the hit compulsion attached to an AI entity. It
models the native constructor, `SetAttributes`, `SetDifficultyVariables`,
`Activate`, `HasExpired`, `Think`, `IsInterruptible`, `GetName`, and
`Deactivate` routines from `playgroundz.elf`.

`HitCompulsionTuning::load` selects the same session/dare collection as the
native tuning selector and reads the four hit-angle arrays as signed 16-bit
degrees converted to radians. The arrays are passed into `activate`; the
entity factory copies the native ball binding and difficulty snapshot into
the compulsion. Waiting, distance and charge are sampled from the shared
state owners when the entity constructs it.
`HitCompulsionServices` exposes only the synchronous AIRand draws and the
active-tetherball query. Those calls retain native order, and `think` writes
the existing `RallyRuleState` words only when the active minigame is tetherball.

The constructor initializes the fields the executable writes. Fields at
`+0x84`, `+0x88`, `+0x8c`, `+0xa8..+0xae`, and `+0xb0..+0xdc` are not initialized
there, so the typed representation keeps them as `Option` until the entity
factory, setter, or activation binds them. This avoids assuming the allocator
clears native SlotPool storage.

`HasExpired` tests the wrapped ball angle against the expiry angle and its
half-turn endpoint. Its reverse-interval flag follows the native positive-rate
comparison (`rate > 0`). `Think` wraps the ball angle, compares the absolute
difference to the selected target with a strict `< 0.15f` cutoff, and records
`-1` outside that window. The original elapsed-time argument is unused.

The oracle at `tools/tetherball_ai_hit_oracle.py` executes the original PPC
function bodies. Only VLT storage and AIRand are serviced by the harness;
`GetTunablesCollectionName`, the angle helpers, minigame lookup, and the
compulsion functions run from the pinned ELF. The checked fixture contains
1,865 snapshots, including all activation combinations in the generator,
expiry intervals, the exact Think cutoff and adjacent f32 values, and positive
and negative multi-turn inputs. Rust tests in the module compare the decoded
state and effect order against those snapshots.

This module covers the hit compulsion itself. AI entity evaluation, movement
compulsion, scheduling, and the complete game remain in their owning modules.
