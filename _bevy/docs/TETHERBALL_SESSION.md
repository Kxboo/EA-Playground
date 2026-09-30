# Tetherball session entry

Verified against the pinned playgroundz.elf: WorldMan::StartMinigameFadeComplete (0x803e1b2c) dispatches tetherball setters in level, difficulty, dare, teams, rules order, then directly writes the game identifier before Initialize. The actual tetherball vtable resolves these setters to 0x803ab3e8, 0x803ab380, 0x8039cd48 and 0x803ab428; SetUpTeams is 0x803ab3f0.

`tetherball_session::configure_storage` preserves the complete native memory stores. Teams count goes to +70; 128 participant bytes go to +78..f7. The source reserved +4 word and destination +74 word remain untouched. Signed values and opaque rules/game identifiers are supplied explicitly. No game ID table is invented: GetMinigameType's table at 0x805a8ee0 is runtime initialized and is not file-backed ELF data.

`configure_runtime` updates the existing owners for selected settings, two character identities, the additional-player discriminator and participant flags. Flags use the established boolean representation (nonzero native bytes become true). Unrepresented team bytes remain caller-owned session input; this is not a claim that every multiplayer team field has a runtime port. Constructor metadata +f8 is correctly named rules_0f8.

`start_session` composes construction, session configuration and recovered enclosing initialization into one callable entry for developer and normal selection. It still requires explicit allocation state, decoded database inputs and a StartupServices host. Engine registration, fades, child initialization service composition, actual scene readiness and the interactive Bevy host are remaining dependencies. Errors retain the existing startup semantics; this wrapper does not add rollback.

The oracle executes the original five setters against 48 dirty complete 0x450-byte allocations with randomized records and reserved words. Rust compares every byte, then checks the typed session owners against these native outputs using the existing runtime fixture seeds. These comparisons establish setup stores, not playable completion.
