# Tetherball hit, multiplier, and indicator

`src/tetherball_hit.rs` ports `HitTetherball`, `IncrementMultiplier`, and
`DrawHitIndicatorParticle` from the pinned original executable
`Remaster/reference/playgroundz.elf`, SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

| Routine | Address | Recovered body |
|---|---:|---|
| `HitTetherball` | `0x8039a2c0` | Complete hit-speed adjustment, ball hit/zone updates, charge and statistics stores, sound/controller feedback, particle requests, and effect cleanup |
| `IncrementMultiplier` | `0x8039c220` | Complete unsigned multiplier increment, stage-effect selection, and attached-controller feedback |
| `DrawHitIndicatorParticle` | `0x8039c538` | Complete persistent particle creation, scale progression, position updates, and destruction path |

## State and call contract

These methods mutate their shared owners directly. `Lifecycle` owns the active
server, per-player controllers, hit statistics, current distance and match
state. `BallMotion` owns the ball's hit response, angle, angular velocity and
zone. `ResetState` owns the hit kind (+0x224), direction (+0x268), speed
(+0x26c), indicator particle GUID (+0x334), invalid GUID sentinel, and hit
tunables. `RallyRuleState` owns the AI charge mirrors; the charge and multiplier
words remain in `Lifecycle`.
`HitState` contains the remaining values used by this graph: pending zone
(+0x274), overlay latch (+0x32c), power-hit type (+0x43c), the optional hit
speed multiplier (+0x428/+0x42d), indicator scale/rate (+0x338/+0x33c), and
the four distance-angle tables (+0x360..+0x384). It also carries +0x32d and
+0x42e for callers in the shared rally graph.

`hit_tetherball` consumes a hit kind, speed, direction and pending zone that
have already been selected by the original attempt and zone-selection stages.
This recovered call graph reaches it with zone 0 or 1; the Rust entry asserts
that domain instead of inventing behavior for zone 2. The speed-cap table at
`0x80442094` has entries `[4.5, 4.9, 5.3, 5.7, 6.0]` for game modes 0 through
4, so modes outside that range are outside this array-backed projection.

`increment_multiplier` takes the original routine's explicit player argument
for the +0x30c multiplier word. Its later attached-controller test instead
uses the active server (+0x214), matching the two distinct indices in the
native body.

`HitInputs` supplies the ball's world position, the two particle-name fields
for each character (+0x2fc/+0x304), and a caller-provided controller-effect
offset corresponding to the static vector at `0x805e37d0`. The host supplies
that offset explicitly; this module does not assume or synthesize the BSS
initializer's result.

## Preserved order and decisions

`HitTetherball` first caps speed only when the pre-cap speed exceeds the
mode-specific limit. On this capped path, hit types 1 and 3 apply +0x350 and
type 7 applies +0x354. The +0x42d/+0x428 multiplier check follows that branch,
so it applies whether or not speed was capped. The function wraps the ball
angle, calls the shared ball hit implementation, adjusts camera height, sets
the zone, adds 0.15 to +0x35c, and clears +0x270.

Hit kinds 0 and 2 update the charge meter and request their kind-specific
backend sound. Kinds 1 and 3 request two backend sounds; kind 7 also requests
a 400 ms camera shake at 0.2. Kinds 1 and 3 increment the active player's
power-hit statistic, while kind 7 increments the mega-hit statistic. When the
active player has a controller, each recognized kind also requests its Wii
sound and stores +0x43c. Hit particles are created from the selected player's
normal or power particle name and are scheduled for destruction after 2500 ms.

After the hit-kind branch, a set +0x32c latch creates the positive feedback
particle and power-hit particle at character world position plus the initialized
origin offset, schedules their 4000/2500 ms destruction, requests front-end
sound 30, and clears the latch. That latch is cleared even when the active
player has no controller. Finally, the routine destroys the selected hit
particle only when its GUID differs from the global invalid sentinel.

`IncrementMultiplier` increments the supplied player's +0x30c value with
unsigned arithmetic until it reaches 5. Values 2 through 5 request the
corresponding `pg_tetherball_x2` through `pg_tetherball_x5` stage particle at
the ball position, with a 2000 ms destruction delay. If the resulting unsigned
value exceeds 1 and the active server has a controller, the routine adds the
origin offset to that controller's position, creates the positive and
active-server-specific power-hit particles, schedules their destruction, and
requests front-end sound 30. Values above 5 skip stage-particle creation but
still take the controller-feedback path when its guard passes.

`DrawHitIndicatorParticle` advances +0x338 by frame delta times +0x33c using
the original fused multiply-add and caps the result at 1. If the persistent
GUID is invalid, it creates `pg_tetherball_glow` at the ball position and
computes a new rate. The calculation takes the absolute value of ball angular
velocity (+0x88), subtracts the selected table entry from pi/2, divides by
that magnitude, then applies the 1000 and reciprocal operations in native
single-precision order. State 28 selects the +0x360 table; other states select
+0x384. Current distance indexes the three-entry table. Each draw then looks
up the effect and sets its position and scale. A false draw flag schedules an
existing effect for destruction with delay zero and restores the invalid
sentinel.

## Service boundary and evidence

`HitServices` extends the existing `ServeServices`. Character world positions,
audio azimuth, Wii sounds, camera shake, particle creation/lookup/position/
scale/destruction, and charge-meter effects are synchronous calls at the
points where the original body performs them. The Rust module preserves their
order and arguments; the host remains responsible for the engine objects and
their actual rendering or playback.

`tools/tetherball_rally_oracle.py --check` executes the original PPC hit,
multiplier, and indicator bodies inside the complete Rally graph. Its golden
corpus contains 320 direct hit cases, 272 multiplier cases, and 244 indicator
cases. The emulator reaches all 387 instructions of `HitTetherball`, all 125
of `IncrementMultiplier`, and all 105 of `DrawHitIndicatorParticle`, including
both outcomes for each recorded conditional branch. `src/tetherball_rally_tests.rs`
compares full lifecycle and ball snapshots, reset/rally fields, and ordered
service traces against that corpus. The combined corpus also exercises
`Return` and `Accelerate` with the original hit-selection and zone helpers.

Boolean fields are canonical Rust projections of native truth tests, and the
array-backed player, mode, distance and hit-zone inputs are restricted to the
observed game domains above. Differential agreement covers the recorded
deterministic PPC cases; it does not emulate PartFx rendering, device hardware,
or unobserved pointer states.
