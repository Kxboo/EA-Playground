# Tetherball AI entity

`src/tetherball_ai.rs` ports `TetherballAIEntity::Initialize` (0x80395354),
`EvaluateCompulsions` (0x803954a8), `IsInPosition` (0x8039565c), `IsSwinging`
(0x80395808), and the gameplay-state predicate (0x80397950).

The pinned ELF SHA-256 is
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
Run `py tools/tetherball_ai_oracle.py --check` and
`cargo test --release --offline --locked tetherball_ai::tests`.

The fixture contains 64 matrix-vector products, 113 animation queries, 37 gameplay-state queries,
160 position predicates, 240 complete compulsion selections and 96 corpus-backed
initializations, plus 16 complete hit activations checking corpus-loaded angle
tables. The oracle executes original angle/matrix helpers, geometric
predicates, constructors and configuration stores. Only world lookup and pool
allocation are replaced during selection. Database services use the original
VLT corpus and original inherited lookup/accessor code via the tuning oracle.
Rust checks both player mappings against selected components, including bindings,
priority, angle bits, difficulty bytes, distance and live charge/waiting values.

## Recovered behavior

- Only states 28 and 29 pass IsInGamePlay.
- Only animations 63, 69, 75, 81 and 91 pass IsSwinging.
- Position uses the ball anchor (+60), desired radius (+b4), character position,
  inverse/world matrices and a 0.55 offset; comparison is strict squared XZ
  distance, with original single precision multiply-add order.
- Movement (priority 40) is selected before a hit (priority 50). The incoming
  priority must be strictly lower. Enabled gates hits, not movement.
- Hit selection uses the wrapped ball angle within the directed half-turn
  interval from AI heading; negative direction scale reverses that interval.
- Initialize preserves difficulty bytes when disabled, and both enabled and
  difficulty when no minigame exists. Heading is always copied and wrapped.
  Difficulty indexing is the raw unsigned argument, not a clamped game mode.

## State and boundaries

AI owns its handle, player link, ball handle, angle, scale, heading, enabled byte
and seven difficulty bytes. Waiting remains in ServeState, charge in
RallyRuleState, and distance/current animation in Lifecycle. Evaluate constructs
the actual recovered move/hit component from those owners.
`move_has_expired` composes movement expiry with the same live AI predicates;
the host supplies only whether the active tetherball minigame exists.

The typed constructor initializes the derived fields; its caller supplies the
direction scale left unwritten by the original constructor. The base AI engine,
allocation/scheduling/destruction, animation graph and movement application are
not reconstructed here. Direct position queries require a live minigame and
valid matrices/pointers; Evaluate handles absent minigames before this query.
Allocation failure is outside the original valid domain (native writes through
the returned null pointer). Inputs are finite ordinary game floats; no arbitrary
NaN/FPSCR or nonterminating angle wrapping claim is made.

This adds verified AI components and shared-state composition, not a playable
tetherball minigame or a claim of full AI-engine equivalence.

## Reset composition

`tetherball_ai_reset::apply_reset_ai` applies ResetRound's AI requests to two
recovered entities. It preserves allocation/controller-binding requests for
the host, initializes shared charge to zero as the native constructor does,
and consumes AI initialization/angle/scale/distance stores. Distance remains
owned by Lifecycle. The caller can chain it after `reset_round_with_animations`.

`tetherball_ai_reset_oracle.py` checks 24 complete ResetRound calls with the
original AI constructors and Initialize body. The UInt8 database service runs
the original accessor against the real corpus in a second emulator. Other
reset engine/database services retain the existing explicit reset inputs.
The allocation service supplies preconstructed objects with explicitly seeded
unwritten storage; this is a fixture input, not a claim that SlotPool clears
memory. A nonzero waiting-byte sentinel is checked for preservation across
construction and reset. Waiting (+6c) is not written by the constructor and must be supplied by
the host when attaching newly allocated storage to ServeState. The adapter
does not invent a value for it. Base AI scheduling/allocation remains external.
