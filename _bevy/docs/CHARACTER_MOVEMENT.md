# Grounded and airborne character velocity

`src/character_movement.rs` ports these functions from the pinned playground ELF
SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`:

| Original function | Address | Size |
|---|---:|---:|
| hkCharacterMovementUtil::calculateMovement | `801dd11c` | 1280 |
| hkCharacterStateOnGround::update | `801dcbd4` | 1256 |
| hkCharacterStateInAir::update | `801dc614` | 572 |
| hkCharacterContext::update / setState | `801dc3a0` / `801dc30c` | 136 / 148 |

This is velocity preparation before `hkCharacterProxy::setLinearVelocity` and
`integrate`. Proxy collision/support queries, capsule constraints and position
integration remain a host boundary. The input builder is described separately
in `CHARACTER_INPUT.md`.

## Runtime API and recovered configuration

Persist `CharacterMovementState`, initialized with `Default` (Grounded), and
call `update(&CharacterInput)` with inputs from the native builder. The result
contains the next velocity vector and state. Feed the resulting proxy velocity
back through the next input; an integration that supplies only vertical velocity
would lose the recovered prior-velocity/gain/acceleration behavior. The host
collision solver may constrain this velocity before using it as the next proxy
snapshot. Call the state update only when simulation delta is positive: the
original outer dynamic-character Update skips nonpositive deltas at
`803b63a0–63a8`.

`MovementConfig` defaults to speed 20 and gain 1 for both Grounded and InAir,
as explicitly set by the dynamic-character constructor (`803b6030–6044`,
`803b608c–60a0`). Ground flags `[true,false,false]` come from the OnGround
constructor (`801dcbbc–801dcbc4`):

| Ground byte | Effect when true |
|---|---|
| `+10` | Remove prior velocity along up when departing support |
| `+11` | Limit downward change to projected gravity * delta while grounded |
| `+12` | Skip the default upward/slope speed correction |

Custom gains/speeds and all three flag paths have original-machine fixtures.
The port accepts only the Grounded/InAir domain: `want_jump` and `at_ladder`
must be false, as they always are in `BuildCharacterInput`. It asserts this
contract instead of silently implementing nonexistent Jumping/Climbing behavior.

## Utility arithmetic

The public `calculate_movement(MovementInput, initial_output)` also exposes the
standalone original utility. Its recovered input layout is:

| Utility input | Offset |
|---|---:|
| gain | `00` |
| forward / up / surface normal | `10` / `20` / `30` |
| prior velocity / desired local velocity | `40` / `50` |
| maximum-acceleration parameter / surface velocity | `60` / `70` |

First, side = normalize(forward × up). If its squared length is below
`1.1920928955078125e-7`, the utility returns without changing the supplied output
(`801dd1b8–801dd1bc`). Then local X = normalize(side × normal) and local Y =
normalize(local X × normal). Local Z is the supplied normal, without a further
normalization. Prior velocity minus surface velocity is rotated into this basis.
The desired-minus-current local vector is then calculated.

The exact limiter predicate is `f32(gain * squared_length(change)) >
f32(max_acceleration * max_acceleration)` (`801dd498–801dd4a4`). When taken,
change is normalized and scaled by `max_acceleration / gain`. Next local velocity
is the fused calculation `gain * change + current`, followed by world rotation
and addition of surface velocity. This parameter is not multiplied by dt in the
original utility. Both state updates supply 100 (`801dcd70`, `801dc6f0`).

On flat support, local X = -forward and local Y = -(forward × up). State code
puts UD*speed in local X and LR*speed in local Y (`801dcdc0–801dcdd4`,
`801dc740–801dc754`). This preserves the signs established by the input work;
the port also handles the original slope basis instead of using a flat projection.

## State arithmetic and transition latency

`hkCharacterContext::update` initially copies the input proxy velocity to output
(`801dc3d4–801dc3e0`) and invokes the current state. Its `setState` changes the
state and calls leave/enter callbacks; the Grounded/InAir callbacks are no-ops
(`801dc428`, `801dc42c`). It does not execute the destination update that frame.

* Grounded without support changes to InAir and returns. With the default +10
  flag it first removes the prior up-axis component (`801dcc9c–801dccf0`). No
  airborne gravity or horizontal update occurs until the next state update.
* InAir with support changes to Grounded and preserves prior velocity for that
  frame (`801dc63c–801dc670`).
* Supported Grounded runs the utility with the supplied surface normal. Its +11
  option can correct a downward velocity change. Unless +12 is true, it removes
  surface velocity, and when the remaining velocity has up projection greater
  than 0.001, applies the original cross-product correction scaled by movement
  length divided by normal·up (`801dcf20–801dd034`). It finally restores surface
  velocity. The exact order of rounded operations is retained.
* Unsupported InAir uses up as utility normal. It removes the utility result's
  up projection, restores the prior velocity's up projection, then adds
  `seconds * character_input.gravity` with fused operations
  (`801dc770–801dc818`). All four vector lanes are retained.

The default grounded path has no separate gravity accumulation. The airborne
path already adds input gravity, so a host must not add the same gravity again.
The outer original proxy `integrate` separately receives base gravity; that
collision integration is outside this port and should not be inferred from the
character-state arithmetic.

## Reciprocal-square-root instruction model and provenance

The original normalization uses `frsqrte`, rounds its estimate to f32, and runs
one Newton refinement with the original f32/fused sequence. Substituting host
sqrt/division changes results. The numerical estimate table is sourced from
[Dolphin tag 2506, Common/FloatUtils.cpp](https://github.com/dolphin-emu/dolphin/blob/2506/Source/Core/Common/FloatUtils.cpp),
specifically `frsqrte_expected`; the cited source carries Copyright 2018 Dolphin
Emulator Project and SPDX `GPL-2.0-or-later`. Only its numerical instruction-model
data is recorded here. No Dolphin C++ implementation is copied into the runtime;
the positive-f32 bit mapping and movement routines are independently expressed.

The oracle decodes the original instructions using that documented estimate
model. This proves Rust equivalence to original PPC execution **under the
instruction model**, not independent validation against a physical Wii. The
source revision is the release tag 2506, rather than moving master. The model's
positive finite inputs are covered by these cases; NaN/infinity payloads,
floating exception flags and an exhaustive hardware instruction sweep are not
claimed. The runtime preserves the tested operation order and uses explicit
fused multiply-adds.

## Differential fixture

`py -3.14 tools/character_movement_oracle.py --check` regenerates 262 deterministic
cases in memory and checks the committed JSON without writing it. The script
pins the original ELF hash. It executes complete utility/context/state functions,
including original state-manager lookup, state changes and leave/enter callbacks.
Prepared memory supplies the state objects/configuration and input snapshots.
There are **no movement, state, math-helper or proxy hooks** in this oracle.
The interpreter extension supplies frsqrte, fused single arithmetic and integer
register spills/restores; compiler paired restores use the following scalar lfd.

Cases include UD/LR directions, prior XYZ/W velocity, surface velocity, flat and
sloped/zero/inverted normals, rotated up, degenerate forwards, gains and speed
configuration, limiter branches, seconds 0–0.2, both states and both support gates.
Zero-seconds cases are direct arithmetic tests; they do not claim that the original
outer Update runs at zero delta.

`--check --rust-check` compiles a standalone rustc harness without Cargo and
compares all four output words and next state at zero tolerance. The module's
Rust test checks the same fixture and pinned ELF hash. No physical-console
measurement or full Havok proxy equivalence is implied.
