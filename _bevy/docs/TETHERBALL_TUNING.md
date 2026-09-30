# Tetherball tuning selection and initialization

The pinned `playgroundz.elf` SHA-256 is `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`. `tetherball_tuning.rs` reads the original `db.vlt` / `db.bin` through the existing Rust VLT loader. No tuning defaults were fitted to the desktop simulation.

`GetTunablesCollectionName` (0x8039ce38) compares the base word at +0x40 as a signed integer. This word is incremented by human-player initialization and retains the name `session_mode` in the existing projection; it is distinct from tetherball character count +0x210. Session mode >1 selects `tunables`. Otherwise dare +0x48 selects `tunables` for -1, `dares_speed_rounds` for 0..2, `dares_time` for 3..5, and `dares_endurance` for 6..8. Other values return a null pointer. The Rust selector preserves that result as `None`; loading an invalid selection returns an error.

`Minigame::GetArrayIndexFromDiffLevel` (0x803abea8) maps enum values 0..3 to indices 0..3 and all other values to 0. The main initializer separately selects AI/ball indices 0..3 at 0x80396cf0..0x80396d30 for the supported difficulty domain. Its invalid-enum path does not assign r29; this module does not claim that undefined lifecycle input has a recovered default.

## Object fields and sources

`InitTunablesForSinglePlayer` (0x8039cf90) selects the dare collection and writes the following six integer words. Game play type uses index 0; the other fields use the converted difficulty index.

| Attribute | MGTetherball offset |
|---|---|
| game_play_type | +0x16c |
| single_player_num_rounds | +0x170 |
| distance_from_pole | +0x174 |
| handicap | +0x178 |
| game_duration | +0x17c |
| single_player_rotations_to_win | +0x180 |

`InitTunablesForMultiPlayer` (0x8039d080) instead copies the supplied multiplayer parameter record: parameter +0 -> object +0x3c, +4 -> +0x44, +0x10 -> +0x170, +8 -> +0x180; +0x16c,+0x174,+0x178,+0x17c become zero. Parameter +0xc is unused. `multiplayer_fields` exposes this operation without inventing a host multiplayer parameter record. `TetherballTuning::single_player` remains the single-player initialization result even when the collection selector is requested with a multiplayer session mode.

MGTetherball initialization at 0x80397184..0x803972ec explicitly obtains **regular `mg_tetherball/tunables`**, independently of the dare selector. It reads base, acceleration, power and mega speed fields into +0x34c,+0x358,+0x350,+0x354. `Tetherball::Initialize` reads the same regular collection at 0x8039d668..0x8039d6fc into +0x14c,+0x158,+0x150,+0x154. The public `speeds` array orders these as base, power, mega, acceleration.

The MGTetherball angle loop at 0x8039721c..0x803972dc reads signed Int16 values for three hit zones, not the difficulty index. Each converts to f32 and multiplies by the executable's f32 degree-to-radian constant (`0x3c8efa35`). The four public arrays correspond to return-pre (+0x360), return-post (+0x36c), acceleration-pre (+0x378), and acceleration-post (+0x384). The corpus contains four entries but this loop reads only the first three.

`TetherballAIEntity::Initialize` (0x80395354) uses the dare-selected collection and, when enabled, writes seven UInt8 values at +0x78..+0x7e: fast, slow, wrong-height, power-hit, mega-hit, power-error, mega-error probabilities. Its disabled branch leaves these fields unchanged. The public tuning record provides the enabled reads, not a mutation of an existing disabled AI object. The player-owned AI entity is initialized separately at 0x80396d74 with index 1; opponent entities use the selected r29 index. The oracle's AI record represents enabled opponent tuning.

Ball initialization sets zone +0xac to zero before its height arithmetic. The executable's zone offsets at 0x80607040 are f32 `[0.5,0.9,0]`. At 0x8039d6fc..0x8039d738, pole height from Placeable +0xf4 is stored at ball +0x160, and pole height plus zone offset is stored at +0xb8 and +0xbc. Flags +0x164,+0x165 become zero, +0x15c becomes one. `ball_height` also exposes the three zone values for callers that already have a validated zone.

## Inheritance and bounds

The three dare collections inherit from regular `tunables`. `Collection::GetNode` (0x802d4c60) searches the child, then follows the runtime parent pointer at +0xc until a node is found. The Rust helper follows the actual corpus parent keys and rejects missing parents/cycles. It resolves the attribute before indexing, so a present short child array masks the parent's entire array.

Native `Attribute::GetElementPointer` (0x802f49e8), `GetInternalPointer` (0x802da4c8), and `Array::GetData` (0x802d4020) produce null for out-of-range indices. The UInt32/Int32/UInt8 accessors return zero on null, and Float returns +0.0. Thus difficulty 3 on the three-element dare overrides returns zero; it does not inherit the regular collection's fourth element. Every requested attribute in these real tuning records resolves along the collection chain. For the requested corpus arrays the stored count and capacity are equal; the native pointer accessor checks the capacity at array +2. The native secondary class-default lookup after a wholly missing attribute is outside this corpus-backed API's validated domain; no synthetic class defaults are supplied.

## Differential evidence and boundaries

Run `py -3.14 tools/tetherball_tuning_oracle.py --check` from `_bevy` (or use the repository-relative script path). It checks the committed fixture without writing. The fixture pins both the ELF identity and both database hashes. It captures 75 collection-selector cases, 9 difficulty-index cases, 40 records covering regular plus all nine dare IDs at each supported difficulty, 3 multiplayer parameter copies, and 12 height cases. The Rust fixture test compares the public API against the captured words, including exact f32 bits.

The oracle executes original selectors, complete single-player/multiplayer tunable initialization functions, enabled AI initialization, native numeric accessors and array bounds arithmetic, plus the exact tuning/angle/height instruction slices within the two larger initializers. Those slices start with the registers and integer-conversion stack words normally established by their preceding code. It does not execute asset allocation, player spawning, audio, camera setup or the entire lifecycle.

External boundaries are hash conversion, database collection handles, `Instance::Get` attribute materialization, array pointer extraction, world minigame lookup, and angle wrapping (the supplied AI angle is zero). Attribute materialization first executes original `Collection::GetNode` in a separate interpreter; only direct hash-table lookup is hooked to actual corpus nodes. Consequently parent traversal is machine-executed rather than Python expectations. The corpus relocation/value representation comes from the existing independently decoded VLT loader. No selector, parent traversal, index check, zero fallback, integer conversion, degree multiplication, field store or height addition is hooked. The auxiliary GetArrayCount call is stubbed: its return value is discarded by these accessors; the operative bounds check executes in Array::GetData. Compiler save/restore helpers follow the shared emulator convention. These are instruction-emulation differential results, not console hardware validation.
