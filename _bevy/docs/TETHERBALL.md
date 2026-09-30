# Tetherball serve, hit and motion core

`src/tetherball.rs` implements an original-code slice of the Tetherball ball
state and score arithmetic. It is not a complete minigame. Callers supply geometry
and tunable fields; no missing database balance values are invented.

| Original routine | Address | Rust API |
|---|---|---|
| rmAngle::Wrap | 0x802cd4e8 | wrap_angle |
| Tetherball::Hit | 0x8039e3d8 | hit |
| Tetherball::Update numerical prefix | 0x8039d904–0x8039dc84 | update_motion |
| Tetherball::SpinDownPole | 0x8039e9bc | spin_down_pole |
| Tetherball::SetAngularVelocity | 0x8039e348 | set_angular_velocity |
| Tetherball::Miss | 0x8039e53c | miss |
| Tetherball::SetRadius | 0x8039e564 | set_radius |
| Tetherball::SetDesiredRadius | 0x8039e570 | set_desired_radius |
| Tetherball::Toss | 0x8039e690 | toss |
| Tetherball::Serve | 0x8039e6ac | serve |
| Tetherball::CheckForPowerServe | 0x8039e784 | can_power_serve |
| Tetherball::CheckForHighServe | 0x8039e7ec | can_high_serve |
| Tetherball::SetZone | 0x8039e848 | set_zone |
| Tetherball::DropOneZone | 0x8039e868 | drop_one_zone |
| MGTetherball::CalcScore | 0x8039cd10 | calc_score |

`tools/tetherball_oracle.py` executes these original PowerPC instructions and
emits `tests/data/tetherball_golden.json`. It pins the original ELF SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`;
Rust tests assert the same hash against `recovered::ELF_SHA256`. Its local emulator
subclass adds floating comparisons, CR-or and indexed stack-store instruction semantics. It has no
behavior hooks, and executes the original rmfAbs helper too. No original image
contents are distributed. Run `python _bevy/tools/tetherball_oracle.py --check`
from the repository root to compare refreshed machine-execution results.

100 prepared ball states cover 2,416 motion/serve/hit/update transitions; eight
angle-wrap cases and 128 score cases
cover signed multiplication and overflow. Rust compares every represented f32
field by its bits after every transition. Cases include both directions/zones,
unequal current/desired radius, positive/negative velocities, signed zero,
strict serve thresholds, spin-return thresholds at ±6, the delayed
toss boundary at 399+1 milliseconds, and unsigned conversion up to UINT_MAX.
This is bounded deterministic equivalence, not all-input formal verification.
Finite motion inputs and nonzero finite radii are the validated domain; the API
follows original arithmetic and does not add tuning clamps.

Decoded behavior:

- Hit wraps the supplied angle and records hit type/direction. Reversals take the
  requested speed divided by current radius; same-direction hits preserve the
  larger magnitude of existing angular velocity and requested speed/radius.
  Secondary target velocity is 1.5 times the primary; direction zero negates both.
- update_motion follows original order: delayed toss/gravity, radius interpolation,
  wrapped primary/secondary angles, height interpolation, spin-return velocity
  and possible SpinDownPole. SpinDownPole requests radius 0.7, height pole+0.5,
  enables the spin-up flag and sets both velocities to ±6 according to current sign.
- The oracle starts at the original Update prologue, runs the original arithmetic
  and stops before instruction 0x8039dc84. It does not replace any math helper;
  original rmAngle addition and Wrap execute too. The full function later derives
  radius/height again from transformed ball coordinates at 0x8039dfd8–0x8039e024.
  The separate `tetherball_scene.rs` now completes that feedback (grabbed balls
  only), model/rope matrices and ordered effect requests; see [scene evidence](TETHERBALL_SCENE.md).
  `update_motion` alone remains an exact prefix. Original
  particle effects remain required for complete gameplay.
- Toss sets vertical velocity to 3.6, the timer to 400 and the tossed flag.
- Serve converts speed to angular velocity using desired radius. Its secondary
  velocity is 1.5 times that value. When absolute vertical velocity is below 0.5,
  only the primary velocity gets another 1.5 multiplier. Direction zero negates
  both velocities. It clears both spin flags/tossed and resets zone directly,
  leaving target height untouched.
- Serve acceleration uses base_hit_speed * (modifier - 0.1), divided by desired radius for
  the primary channel and current radius for the secondary channel. The modifier
  fields are now named power_modifier and mega_modifier, matching the database.
- Power-serve predicate predicts vertical velocity with -4.807 acceleration and
  requires absolute value strictly below 0.5. High-serve predicate predicts height
  using a -2.4035 quadratic term and requires height strictly above 0.7.
- Miss sets hit type to 4 and uses the original fused negative multiply-subtract
  to damp both velocities by five percent. Zone-drop also damps both, even when
  zone zero remains unchanged. Only zone one changes to zero. Zone target heights
  use original offsets 0.5 and 0.9 added to the supplied pole height.
- Desired-radius changes rescale primary velocity and both acceleration fields;
  secondary velocity remains unchanged. Current radius is not immediately changed.
- CalcScore ignores its difficulty argument. ResetStats (0x8039cbd0) already
  selects difficulty weights from mg_tetherball/scoring: accuracy_points at
  +0x138, powerhit_points at +0x13c, megahit_points at +0x140. Its second argument
  selects a player-counter row with 0x14-byte stride: power hits at +0x14c,
  mega hits at +0x150, accuracy percentage at +0x154. UpdateRoundEnd
  (0x803992c8–0x803992e4) computes the latter as successful_hits * 100 / attempts.
  The public API now reflects those meanings; original oracle field layouts and
  fixture numbers remain unchanged. Score calculation wraps at every i32 operation.

## Database tuning evidence

Values read through the existing VLT loader from DATA/files/data/db/db.vlt + db.bin;
arrays below retain their original database order. Tetherball::Initialize
(0x8039d668–0x8039d6ec) obtains mg_tetherball/tunables and selects its supplied
AITetherballDifficultyType index. MGTetherball::ResetStats loads mg_tetherball/scoring.

| Collection / attribute | Values |
|---|---|
| scoring / accuracy_points | 10, 15, 20, 25 |
| scoring / powerhit_points | 20, 25, 30, 35 |
| scoring / megahit_points | 50, 55, 60, 65 |
| tunables / ball_basehitspeed | 3.2, 3.5, 3.8, 3.8 (f32) |
| tunables / ball_acceleratemodifier | 1.1, 1.1, 1.1, 1.1 (f32) |
| tunables / ball_powermodifier | 1.2, 1.2, 1.3, 1.3 (f32) |
| tunables / ball_megamodifier | 1.4, 1.4, 1.4, 1.4 (f32) |
| tunables / single_player_rotations_to_win | 6, 6, 6, 6 |
| tunables / single_player_num_rounds | 3, 5, 5, 5 |
| tunables / game_duration | 0, 0, 0, 0 |

Additional collections dares_speed_rounds, dares_time and dares_endurance inherit
from tunables. GetTunablesCollectionName (0x8039ce38) chooses tunables for
multiplayer or dare -1, speed_rounds for dares 0–2, time for 3–5 and endurance for
6–8. This records data evidence; the current BallMotion API still accepts supplied
tuning and does not implement database selection or the MGTetherball state machine.

Direction and Zone enums restrict raw indexing to original valid values zero/one.
The complete Update state/matrix/effect-request port is in `tetherball_scene.rs`.
Collision geometry, gestures, AI, animation
callbacks, database tuning import and full game states remain outside this slice.
The separate tetherball_match module decodes winner predicates and emits ordered
UI/state-entry requests; it does not implement the complete match.
