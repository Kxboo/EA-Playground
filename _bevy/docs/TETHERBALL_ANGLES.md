# Tetherball angle helpers

`src/tetherball_angles.rs` ports the scalar `rmAngle` routines used by the
tetherball serve test from retail ELF `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

| Routine | Address | Behavior |
|---|---:|---|
| `rmAngle::Wrap` | `0x802cd4e8` | Repeated rounded f32 subtraction of `6.2831854820251465` while angle is at least one turn, then repeated addition while negative |
| `rmAngle(float)` | `0x803b2144` | Stores the input and calls `Wrap` |
| `rmAngle::IsBetween` | `0x802ecc6c` | Directed half-open angle interval test with a reverse-role switch |

`is_between(angle, first, second, false)` includes `first` and excludes
`second`, wrapping across zero when `second <= first`. With `reverse: true`,
the opposite interval is selected: for ascending endpoints it accepts values
below `first` or at least `second`; for descending endpoints it accepts
`[second, first)`. Equal endpoints accept every ordered value for either
orientation. Each compare follows PPC unordered-compare branches, so an angle
NaN yields false; a NaN endpoint follows the branch predicates directly rather
than being normalized or rejected up front.

`UpdateServe` constructs an `rmAngle` from ball angle at `0x80398278`, then calls
`IsBetween` at `0x803982a0` with the two stored bounds at `+0x23c` and `+0x240`.
Its final argument is true when the selected player role at `+0x21c` is zero.
The wrapper normalizes the constructed ball angle; `IsBetween` itself performs
no normalization on its three stored values.

The retail wrap loop leaves NaN unchanged, but positive/negative infinity and
sufficiently large finite magnitudes can loop forever because a one-turn f32
addition/subtraction no longer changes the value. The Rust helper preserves
that scalar loop behavior; callers must supply bounded angles. The oracle skips
nonterminating inputs while checking zero, signed zero, exact and neighboring
turn boundaries, negative values, multiple turns, and NaNs. Its interval set
also contains endpoint permutations, exceptional IEEE values, and deterministic
arbitrary-bit vectors.

Run `py -3.14 _bevy/tools/tetherball_angles_oracle.py --check` to execute both
original PPC routines and compare them with the golden vectors.

The frozen corpus contains 43 wrapping vectors and 1,994 interval vectors. The
ball and reset modules re-export/use this same wrapping implementation.
The oracle models comparison condition-register results; it does not claim
FPSCR exception flags or enabled floating-point traps.
