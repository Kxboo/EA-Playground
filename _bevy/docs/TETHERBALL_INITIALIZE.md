# Tetherball game-logic initialization

The original `MGTetherball::Initialize` is a 3,276-byte orchestration routine at `0x803966c4`; this pass does **not** claim to port that full function. It ports its first state-initialization stage, `InitGameLogicState` at `0x8039ceac`, before placeable lookup, spawning, camera/asset setup, tuning loads, audio and screen transitions.

`tetherball_initialize.rs` mutates the established `Lifecycle`, `ResetState` and `MatchRules` owners. It reproduces the signed-byte handicap stores, PPC `srawi`/`addze` round calculation, old-`+0x430` indicator ratio, rules and winner sentinels, statistics reset, and the helper call order. The native ratio is `signed_i8(-handicap) / old_rotation_limit`; it is stored in `Lifecycle.indicator_target` (+0x348). The selected round count is copied into `Lifecycle.total_rounds` (+0x438), while +0x170 is only read by the native initializer and used to derive wins required.

The nested `InitTunablesForSinglePlayer` / `InitTunablesForMultiPlayer` calls supply the values for the routine's field reads. `ResetStats` supplies its three score weights and clears per-player statistics. Those database operations are existing boundaries: the oracle hooks only those callees, then executes the complete `InitGameLogicState` body on the pinned PowerPC ELF. The source API accepts those results and emits the same ordered dependency trace; it does not invent tuning or database defaults.

The fixture contains 64 native executions spanning both helper paths, signed and byte-boundary tunable values, negative/odd round counts, prior rotation limits including zero, and preexisting sentinels. It records all relevant output words, indicator float bits, statistics and dependency order. The Rust test compares the complete shared lifecycle snapshot and the owned reset/rule stores against those native outputs.

Nonzero divided by zero follows the oracle's IEEE infinity model. Zero divided by zero is excluded: console FPSCR and default-NaN payload/sign behavior are not modeled or claimed by this fixture.

From `_bevy`, run `py -3.14 tools/tetherball_initialize_oracle.py --check`. The ELF SHA-256 is `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`. Full `MGTetherball::Initialize`, engine startup services, player/AI spawning, rendering setup and audio remain outside this bounded port.
