# Tetherball move compulsion

`src/tetherball_ai_move.rs` reconstructs the retail `TetherballMoveCompulsion`
constructor, expiry check, movement target calculation, and small virtual
methods from `Remaster/reference/playgroundz.elf`.

| Routine | Address | Recovered behavior |
|---|---:|---|
| `__ct__24TetherballMoveCompulsionFP8AIEntityUc` | `0x80395848` | Runs the base constructor, installs the derived vtable, stores the AIEntity pointer, and clears the derived Think gate. |
| `HasExpired` | `0x803958d8` | Checks the active tetherball minigame, gameplay state, swing animation, then AI position, in order. |
| `Think` | `0x80395990` | Computes the target point and wrapped facing angle, then writes the movement parameters. |
| `IsInterruptible` / `GetName` | `0x8039638c` / `0x80396394` | Returns true and the name `TetherballMoveCompulsion`. |
| `Deactivate` / `Activate` | `0x803963a0` / `0x803963ac` | Clears or sets the base active byte. |

The executable SHA-256 is
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
Run `py -3.14 tools/tetherball_ai_move_oracle.py --check` and
`cargo test --release --offline --locked tetherball_ai_move::tests` to verify
the original-execution fixture and typed Rust projection.

The golden fixture records three constructor byte snapshots, 96 complete
`Think` calls, and seven `HasExpired` calls. The Think cases cover the null
binding, the exact `+0x8c == 1` gate, another nonzero gate byte, both target-X
branches, and varied world transforms and positions. HasExpired cases cover
the missing-minigame, wrong-MGID, non-gameplay, swinging, in-position, and
not-in-position paths. The PPC runner executes the original constructors,
matrix helpers, AI predicates, angle constructor, and wrap routine. World
lookup is replaced only at the explicit `MoveServices` boundary. The MGID
comparison input is produced by the original constructor and equals `TBLL`
(`0x54424c4c`).

## State and input contract

`MoveCompulsion` preserves the native base fields it initializes. Fields the
constructor leaves unwritten are represented as `Option`s and stay absent
until `Think` stores them. `bind_move_object` supplies the pointer that
`EvaluateCompulsions` stores at `+0x88`; a caller must bind it before `Think`.
An explicit zero pointer exercises the original early return. The native gate
is byte-exact: only `+0x8c == 1` suppresses Think.

`MoveInputs` is a read-only snapshot of MGTetherball world and inverse matrices,
the bound movement anchor and desired radius, and the character's world
position. Think inverse-transforms anchor and character, offsets the character's
local X by the anchor radius and `0.55`, then transforms the target back to
world space. Its facing direction uses the anchor transformed through inverse
and world matrices minus the original character world position. `rmAngle`
computes `atan2(x, z)` in the native double-precision call, rounds with `frsp`,
and wraps the result as an `rmAngle`. Think ignores elapsed milliseconds and
stores timer zero, target position and angle, tolerance `0.001`, word `+0x44 =
0`, byte `+0x48 = 1`, clears `+0x80`, and sets the gate byte to one.

`HasExpired` asks `MoveServices` whether WorldMan returned the tetherball
minigame with its expected `TBLL` identifier. If present, states 28 and 29
continue; other states expire immediately. It then asks whether the bound
AIEntity is swinging, and checks its position with the original `0.001`
tolerance only when it is not swinging. These live AI/entity queries remain
explicit service inputs; the module does not substitute assumed results.
`AiEntity::move_has_expired` supplies those queries through the recovered
live predicates; its differential test exercises both player mappings.

Think's deterministic call domain has a live tetherball object and valid
geometry. In the retail body, a missing minigame skips copying the local matrix
variables, then continues using those stack values; that path has no stable
input/output contract and is not replaced with invented matrices here. The
angle oracle verifies finite ordinary game floats; it makes no claim about
FPSCR exception behavior or nonterminating angle-wrap inputs. A null check on a
stack-local angle object is unreachable in a valid invocation.
