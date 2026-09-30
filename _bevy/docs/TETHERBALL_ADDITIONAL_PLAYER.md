# Tetherball additional-player initialization

`src/tetherball_additional_player.rs` ports the complete `MGTetherball::InitializeAdditionalPlayer` body at `0x8039bd20` from the pinned `playgroundz.elf` (`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`). It reuses `Lifecycle`, `ResetState`, `PlayerInitState`, and `PlayerInitServices`; it adds no shadow copies of their counters, handles, or startup flags.

For a nonnegative character count below two, the native helper always spawns a character. The SpawnCharacter request carries identity words in r5/r6, the input position in r7, zero in r8/r9, one in r10, and stack words `[session_mode_at_+0x40, -1]`. Its r4 value is an ABI alignment word for the 64-bit identity parameter and is not modeled as a semantic input. The function then applies CharacterState position and wrapped-angle direction, allocates and conditionally constructs the tetherball AI, registers and binds it, and stores the ball handle at AI `+0x60`.

The helper writes the new handles to the current `+0x210` slots at MGTetherball `+0x120/+0x128`, stores byte one at `+0x130[slot]`, then gets the character’s controller and sets controller state `0x12`. Only after that call does it increment `Lifecycle.player_count` (`+0x210`) and `Lifecycle.session_mode` (`+0x40`). This helper has no existing-character lookup, ability setup, animation selection, or world rebind.

Character, AI-pool/list, and controller work remains an ordered synchronous `PlayerInitServices` boundary. The PPC oracle hooks those object operations and executes the original function, including `rmAngle` construction/wrap and `AsDir`. The game-owned handle-array, flag, and counter stores are checked from the original memory image.

`_bevy/tools/tetherball_additional_player_oracle.py` generates `_bevy/tests/data/tetherball_additional_player_golden.json`. Seven cases cover counts zero and one with varied positions, multi-turn headings, session values including signed-word wraparound, and the count guard at two or more. The Rust adapter compares service order, counter/handle/flag stores, and the AI `+0x60` ball binding against the fixture.

As with the sibling player initializer, `Lifecycle.player_count` is unsigned and this port targets valid nonnegative game counts. The native guard is a signed compare, so negative/corrupt counts are outside this typed domain.
