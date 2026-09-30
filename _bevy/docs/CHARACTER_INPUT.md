# Dynamic character input preparation

`src/character_input.rs` ports `PhysicsDynamicCharacter::BuildCharacterInput` at
`0x803b67f4` (976 bytes) from `Remaster/reference/playgroundz.elf`, SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
It prepares character input and mutates the one-shot gravity/impulse state. It
does not implement `hkCharacterContext`, Havok proxy integration, or collision
support detection.

## API and original storage

`CharacterInputState::build(dt_ms, support, position, velocity)` takes the host's
collision support result and proxy snapshots. `SurfaceSupport.kind == 2` means
supported; other values remain unsupported. Normal and surface velocity are
copied without normalization. The state stores:

| Field | Object offset | Behavior |
|---|---:|---|
| gravity | `+20` | Base vector remains unchanged |
| speed / orientation | `+38` / `+3c` | Speed divided by 20 and clamped to [-1,1] |
| impulse | `+70` | Active only while signed timer > 0 |
| impulse decay | `+80` | Added to impulse after scaling by seconds |
| impulse timer | `+90` | Subtracts truncation of f32(1000 * seconds), clamps at zero |
| gravity override flag / vector | `+ac` / `+b0` | Consumed once by the builder |

The original also stores the initial impulse duration at `+94` in
`ApplyVelocity`; the builder never reads that field. The bounded Rust state
does not retain it. `apply_velocity(vector, duration_ms)` ports the producer at
`0x803b6768`: it stores the vector and timer, and sets decay to
`vector * f32(-1000 / f32(duration_ms))`. Positive and negative durations are
covered by original-machine vectors; duration zero and nonfinite input values
are outside this fixture's validated domain.

| hkCharacterInput field | Offset |
|---|---:|
| input_lr / input_ud / want_jump | `00` / `04` / `08` |
| up / forward | `10` / `20` |
| at_ladder / supported | `30` / `31` |
| surface normal / velocity | `40` / `50` |
| step info / position / proxy velocity / gravity | `60` / `70` / `80` / `90` |

Up is [0,1,0,0]. `want_jump` and `at_ladder` are always false. Gravity is
converted from rmVector3 with w=1. Position, velocity and support vectors retain
their supplied fourth component. Step info is copied by the builder. The Rust
adapter generates [0,0,f32(dt_ms)/1000,1/seconds], following the positive-delta
conversion in `PhysicsDynamicCharacter::Update` at `0x803b6454–6470`. The two
leading fields are explicit zero-filled host values. At zero delta the adapter
uses zero reciprocal for a deterministic direct-builder test; the original
outer Update skips all work when dt_ms <= 0 (`0x803b63a0–63a8`).

## Direction and original state speed

Forward comes from rotating +X around +Y by f32(orientation + pi/2), through
`hkQuaternion::setAxisAngle` (`0x8023c1e4`), `hkRotation::set`
(`0x8023c4ac`), and `hkVector4::setRotatedDir` (`0x8023e3c8`). Its approximate
formula is [-sin(orientation),0,-cos(orientation),0]; the Rust port keeps the
original quaternion and matrix operations instead of substituting that formula.
At orientation zero, captured x is a small positive rounding residue and z is
slightly greater than -1.

The dynamic-character constructor sets both OnGround and InAir speed to 20 and
gain to 1 (`0x803b6030–6044`, `0x803b608c–60a0`). OnGround update reads speed
at `0x801dcdb8`, multiplies LR and UD at `0x801dcdc0/801dcdc8`, and writes
UD*20 as movement-local X and LR*20 as Y at `0x801dcdd0/801dcdd4`.

`hkCharacterMovementUtil::calculateMovement` (`0x801dd11c`) first computes
side = normalize(forward × up) (`0x801dd154–220`). It then computes local X
as normalize(side × surface_normal) (`0x801dd224–2c4`), and local Y as
normalize(local X × surface_normal) (`0x801dd2c8–380`). For flat support normal
equal to up, the planar basis is **local X = -forward, local Y = -side**.
Thus positive speed at original orientation zero requests motion toward +Z.
These downstream signs are supported by disassembly; this builder oracle does
not execute the movement utility's normalization, acceleration limit, slope
handling, or state transitions.

A desktop adapter may explicitly map its facing direction to original yaw using
`atan2(facing.x, facing.z)`, then project `-forward * input_ud * 20 - side *
input_lr * 20` through its own flat-support solver. That is a documented host
coordinate/collision boundary, not a claim that desktop facing equals the
original LocalCharacterControl rotation or that Havok dynamics are ported.

## Impulses and precision

The active impulse's magnitude uses the full XYZ vector. Its horizontal angle is
atan2(X,Z), wrapped to [0,2pi), minus wrapped character orientation, then wrapped
again. Original `EA::Math::fSinCos` (`0x8041c400`) produces local sin/cos using
its polynomial and square root. The scaled local X subtracts from LR/20; local Z
adds to UD/20, with independent clamps. A vertical impulse therefore contributes
to magnitude even though no vertical component is injected into proxy velocity by
this builder. Decay runs after output construction, including on the last active
frame. Gravity overrides affect character input; the original outer Update still
passes base gravity to proxy integration at `0x803b6488–64ac`.

The Rust implementation uses explicit f32 operations and fused multiply-adds
where the original does. Original libc double sin/cos/atan2/sqrt results are
rounded to f32 at their recovered call sites. Host double math matches every
captured case bit for bit; this is evidence for the tested finite domain, not an
exhaustive cross-platform proof for all floating-point bit patterns.

## Original-machine validation

Run `py -3.14 tools/character_input_oracle.py --check` from `_bevy` to regenerate
182 deterministic cases in memory and compare the committed fixture without
rewriting it. The oracle executes the complete original builder and, in 24
cases, the original `ApplyVelocity` producer. It includes speed-clamp boundaries,
cardinal and random orientations, support kinds 0–3, gravity overrides, signed
timer gates, impulse directions, duration boundaries and deltas 0–1000 ms.

Only three external proxy boundaries are hooked: `checkSupport` (`0x801d9480`),
`getPosition` (`0x801db7ec`), and `getLinearVelocity` (`0x801db814`). Supplied
support data is the explicit collision-query boundary. No builder, vector,
quaternion, angle, polynomial, or libc math routine is hooked. The emulator
subclass adds required PPC instructions and raw fctiwz/stfd payload handling;
compiler paired-register restores are ignored because scalar lfd restores follow.
Zero-filled output storage makes padding comparisons deterministic.

`--check --rust-check` also compiles a standalone rustc harness, without Cargo,
and checks all 40 output words, impulse vector, decay vector and timer against
the captured PPC values with **zero tolerance**. Rust's module test performs the
same comparisons and verifies consumption of the gravity override.
