# Tetherball game construction

## Native routines and storage

`tetherball_constructor::construct` recovers the object stores of `MGTetherball::MGTetherball(Ren::Scene*)` at `0x80396410`, including native `Minigame::Minigame` (`0x803ab294`) and `World::World` (`0x803e1564`). `WorldMan::StartMinigameFadeComplete` allocates `0x450` bytes at `0x803e1d94` before calling the constructor at `0x803e1dac`; this instruction is asserted by the oracle. The pinned ELF is `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

The explicit big-endian storage interface requires caller-supplied allocation contents. It preserves every byte the native constructor leaves untouched. There is no synthesized ready-to-play default. World service pointers and paused state clear; base flags, counters, results and the 128-byte participant region initialize; both scene-handle copies are retained; game timers/actions/multipliers, round state, eight gesture slots and game particle sentinels initialize. The prior live rotation cap is exactly 6, wins-required is 2, and live round count is 3.

The base +38 tag is not a simple big-endian concatenation. Bytes 1, 2 and 3 are sign extended, shifted and added with wrapping; fixtures explicitly cover high-bit bytes. Game particle null GUIDs are supplied from `0x80601f60`. ResetStats' decoded score weights remain an input dependency; its represented score/statistic stores are composed. Native constructor/base/world/MEM_fill/rmAngle bodies execute unhooked in the oracle; ResetStats database/helper work is a supplied boundary.

Verified preservation includes +34 state code, +3c level, tunables +16c..180, character/AI handles and spawn flags +120..131, many animation/angle tables, matrices and unrelated allocation bytes. The base participant fill covers +78..f7, including the two identity pairs and participant metadata; it does not extend into character handles. A new object still needs session setters, child startup composition and complete host services before any update.

## Runtime projection and ownership

`construct_runtime_fields` applies the constructor-owned stores to the existing Runtime and StartupState owners. It leaves external character/AI/ball data and fields absent from the constructor untouched. The live gesture queue is cleared, ResetState's eight backing slots receive kind 2/value 0/counter 0, and existing compatibility projections are synchronized. `ConstructorMetadata` adds only previously unrepresented world/scene/tag/UI/team-reference words; existing fields are not duplicated there.

Tunable round/rotation words +170/+180 now have their own `StartupState.tunables_170_180` owner. They remain distinct from live count/cap +438/+430: construction preserves the tunables while initializing the live values. Startup's recovered InitGameLogic composition updates the tunable owner from its selected input values. The previous startup fixture projection aliased these fields and could not represent that distinction before game logic ran.

Verified: 32 complete native 0x450-byte images use randomized allocation contents, varied scene handles, signed tag bytes, particle tokens and scoring weights. Rust compares every output byte. A further 24 native constructor cases project from existing startup snapshots with dirty counters/actions/HUD/multiplier fields; the Rust test compares every mapped owner and preserves unrelated ball motion. These tests prove constructor stores and the typed projection, not a complete Runtime factory or authentic interactive session.

Run from `_bevy`:

- `py -3.14 tools/tetherball_constructor_oracle.py --check`
- `cargo test --release --offline --locked tetherball_constructor::tests`
- `cargo test --release --offline --locked tetherball_startup::tests`

Both suites and the oracle are registered in `tools/prove.py`.

## Next dependency: session setup

The enclosing WorldMan creation path configures difficulty, level, dare type, teams, rule pointer and game tag before Initialize. The native setters are `SetDifficulty` (`0x803ab380`, +44), `SetLevel` (`0x803ab3e8`, +3c), `SetDareType` (`0x803ab420`, +48) and `SetRules` (`0x803ab428`, +f8). `SetUpTeams` (`0x803ab3f0`) copies team count to +70 and 128 participant bytes to +78..f7. These routines and the tetherball branch of `StartMinigameFadeComplete` have been inspected but are not yet composed in the Rust startup factory. Preserve this ordering: constructor alone cannot supply selected participant identities or session rules.
