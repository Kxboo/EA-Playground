# Tetherball hit animation routines

This module ports five native functions from the retail `playgroundz.elf`:
`InHitAnimation` (`0x8039a8cc`), `StartReadyAnimation` (`0x8039a178`),
`StartHitAnimation` (`0x80399f88`), `CheckForWaitingCharacterSwing`
(`0x80399db8`), and `IsBallInHitRange` (`0x80399cb0`). The oracle at
`tools/tetherball_hit_animation_oracle.py` executes each original PPC body and
its angle helpers. It replaces only controller, random, animation, azimuth,
and sound engine entrypoints with ordered service records.

`HitAnimations` contains the remaining configurable ready/hit state arrays:
the four ready-state/suppression pairs at `+0x1a0/+0x1a8`,
`+0x1d0/+0x1d8`, `+0x1b8/+0x1c0`, and `+0x1e8/+0x1f0`, plus hit states
`+0x198` and `+0x1b0`. The power/high hit states, voice types, ready fallback
states, current animation, controller, action states, current distance, and
ball angle/zone stay in their shared `ServeState`, `Lifecycle`, and `BallMotion`
owners. The temporary `mega_ability` argument represents the call's captured
`GAME+0x42e` byte; it is distinct from the lifecycle `mega_enabled` array.

`InHitAnimation` accepts exactly animation IDs 63, 66, 69, 72, 75, 78, 81,
84, 91, and 94. `StartReadyAnimation` returns without a service call for
current animation 1. Otherwise, a mega-hit type is translated to its
zone-specific ordinary power type; the chosen state is suppressed only when
the current animation equals that table's paired suppression value.

`StartHitAnimation` maps a mega hit to type 2 in zone zero or type 1 in zone
one, then selects an animation from the direction flags and per-player hit
tables. It still queries random range 0 through 99 when neither direction is
selected. The chance thresholds are the native `[50, 75, 75]` table indexed
by `Lifecycle.current_distance`. A successful roll reads the player's
azimuth and selects sound 7 or 8 from the configured voice-type pointer flag.
The native function is void; hit-delay counters are set by its caller.

`CheckForWaitingCharacterSwing` first checks animation 1, controller-pointer
presence, and the hit-animation set. It then reads normal event `0x5c`, reverse
event `0x5d`, and normal event `0x5c` again; only when that second normal read
is true does it read reverse again. Both second reads plus `GAME+0x42e` select
mega type 3. A nonzero action state suppresses only the fallback hit animation,
and the function stores action state 2 after processing. `IsBallInHitRange`
scales its half-width and center offset by the player's signed reset scale,
wraps the constructed angles and ball angle with native `rmAngle` code, and
passes `player != focus_player` as the reverse-interval flag.

The golden file contains 1,663 PPC-derived vectors: 111 predicate cases, 96
ready-animation cases, 480 hit-animation cases, 896 waiting-character cases,
and 80 hit-range cases. Its range inputs stay within the finite bounded domain
of the native repeated-subtraction angle wrapper; infinities and magnitudes so
large that subtracting one revolution no longer changes the float do not
terminate in that original wrapper and are not oracle inputs.

The Rust module test compares return values, action-state stores, and ordered
service effects to the golden vectors. These routines cover animation and
hit-range decisions; they do not implement the full tetherball game loop.
