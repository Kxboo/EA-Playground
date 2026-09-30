# Tetherball transforms and world rendering coordinates

`area_transform.rs` and `tetherball_scene.rs` complete the state, matrix and
effect-request logic of `Tetherball::Update` (`0x8039d904`, 2,628 bytes). They use
the same pinned retail ELF as the other ports: SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
The earlier numerical prefix remains in `tetherball.rs`; `BallScene::update`
calls it before performing the rest of the original routine.

## Ball update and service boundaries

The original order is preserved:

1. Advance the motion prefix; derive the ball position from angle, radius,
   height and anchor.
2. Compute the tilt axis from `(position - anchor) cross up`, normalize it,
   and use `atan2(radius, anchor.y - position.y)` for tilt. Build secondary-spin,
   quarter-turn and tilt matrices in the original multiplication order.
3. For a grabbed ball, obtain the owner world matrix and optional local
   attachment matrix. The local path uses `tilt * local * owner`; the other
   path uses a `(0, 0.5, 0.5)` translation times owner. Feed its translation back
   into ball position, height and planar radius, then remove the translation
   before applying the area rendering matrix. **Only grabbed balls execute
   this height/radius feedback.**
4. Produce ball and rope rendering matrices and optional shadow updates.
5. Select the normal, player-coloured power or mega trail by comparing absolute
   angular velocity with the absolute acceleration thresholds. Destroy obsolete
   trails with a 1,000 ms fade; create missing trails and move the selected trail.
   At zero speed, existing trails are left untouched. An existing power trail
   retains its colour when direction changes until it is recreated.

`SceneServices` represents the original particle-manager and shadow-object
calls. The supplied null handle and returned live handles remain external
values. This port does not render the original particle graph or reconstruct
asset/owner lifetimes. Grabbed updates require a valid attachment, matching the
original valid-pointer domain. These routines are available to the future
minigame integration; a complete tetherball match is not yet playable.

Matrices retain the executable's row-vector memory convention, with translation
at indices 12–14. `rmMult` preserves paired-single multiply/add order;
`Matrix44FromAxisAngle` (`0x8041c74c`) uses the recovered EA trigonometric
polynomial. Original scalar normalization uses the executable's libc square
root path here, separate from the Havok reciprocal-square-root estimate.

## Original AreaManager transformation

`AreaManager::CalcRenderingModelMatrix` (`0x803d78b8`, 524 bytes) replaces the
earlier fitted vertical drop. With radius R, flat position `(x, y, z)` and
`a = pi/2 - x/R`, `b = pi/2 - z/R`, the routine computes:

```
radial = (sin(b)*cos(a), sin(b)*sin(a), cos(b))
rendered = radial*(R+y) - (0,R,0)
normal = (-radial.x, radial.y, -radial.z)
side = normalize(normal cross (0,0,1))
forward = normalize(side cross normal)
```

The implementation preserves the original rounded/fused operations and basis
layout. The original global disable flag selects plain translation. The game
adapter uses that path if the radius has not loaded; regular world rendering
uses the radius from `world.csv` (250).

The original PlaceableManager draw path extracts flat translation, calls this
routine at `0x803d9b80`, then multiplies the translation-free local transform by
the area matrix at `0x803d9b9c`. Character update calls it at `0x802e8ac0` and
multiplies yaw by the area matrix at `0x802e8ba4`; its separate flat physics
matrix is built at `0x802e8c08`. The Bevy game follows those composition orders
for props and the player. Its host orbit camera also uses these transformed
coordinates; the camera framing algorithm remains provisional. World-layer
vertices retain their existing decoded coordinates: `AreaDrawManager::Draw`
(0x803d6834) reaches `DrawRegularModel` (0x803d6940), which calls EAGL Model::Draw
at 0x803d69b4 with the stored model matrix, without an AreaManager warp.

## Differential evidence

Run `py -3.14 tools/tetherball_scene_oracle.py --check` to regenerate and compare
the fixture in memory. It executes all of the original Update, area-matrix,
matrix multiplication, axis-angle, vector and libc math code. The interpreter
adds the actual paired-single matrix load/store/multiply/add instructions;
it does not replace matrix functions with host implementations.

Only five external service functions are hooked: particle CreatePartFx
(`0x802f78a4`), DisableAndDestroy (`0x802f7abc`), GetPartFx (`0x802f7bf4`),
PartFx::SetPos (`0x802f6c34`), and shadow SetModelMatrix (`0x8039e354`). Their
ordered arguments and handle results are captured.

The fixture contains **72 area matrices and 360 complete Update transitions**.
It covers the curved origin, original player spawn, flat/curved modes, three
radii, random finite positions, both attachment paths, arbitrary finite affine
owner transforms, shadows, all trail levels, existing/missing handles, zero
velocity, and deltas 0–399 ms. Rust tests compare all motion fields, position,
both 16-float matrices, handles and ordered service calls with zero tolerance.

This is evidence for the captured finite, nondegenerate input domain under the
interpreter. Host f64 sin/cos/atan2/sqrt agree with executed original libc math
for these inputs; exhaustive floating-point/hardware equality and console
visual matching are not claimed. Full match state entry, gestures, AI, tuning
selection, animation callbacks and effect rendering remain separate work.
