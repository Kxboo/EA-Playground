# Function comparison index: AnimationState

Generated from the pinned compiled executable. Each assembly listing includes
the original instruction words, address, symbol and resolved call targets.
`index.json` pins each function's machine bytes and the comparison source files
by SHA-256. Rust is organized by subsystem, not original C++ translation unit.

| Compiled function | Original instructions | Rust implementation | Oracle entry / cases |
|---|---|---|---|
| 0x803c8318 `SetNextAnim` | [803c8318-SetNextAnim.txt](803c8318-SetNextAnim.txt) | [`select`](../../../src/animation_playback.rs#L78) | `select` / 52 |
| 0x803c851c `Update` | [803c851c-Update.txt](803c851c-Update.txt) | [`update`](../../../src/animation_playback.rs#L245) | `update` / 51 |
| 0x803c89a4 `ProcessAnimEvents` | [803c89a4-ProcessAnimEvents.txt](803c89a4-ProcessAnimEvents.txt) | [`events`](../../../src/animation_playback.rs#L188) | `events` / 51 |
| 0x803c8b78 `SetNextAnimState` | [803c8b78-SetNextAnimState.txt](803c8b78-SetNextAnimState.txt) | [`set_next`](../../../src/animation_playback.rs#L129) | `set_next` / 51 |
| 0x803c8c84 `SetStateTime` | [803c8c84-SetStateTime.txt](803c8c84-SetStateTime.txt) | [`set_state_time`](../../../src/animation_playback.rs#L169) | `time` / 51 |
| 0x803c8cd4 `GetStateTime` | [803c8cd4-GetStateTime.txt](803c8cd4-GetStateTime.txt) | [`state_time`](../../../src/animation_playback.rs#L178) | `time` / 51 |

The 256 cases are shared across these entries; the two time accessors run in the
same cases. Nested state/event calls also execute, but these counts do not measure
branch or instruction coverage. The listings are disassembly, not decompiled C++.

Follow a comparison through [oracle inputs and original-code calls](../../../tools/animation_playback_oracle.py),
[recorded native results](../../../tests/data/animation_playback_golden.json), and
[Rust comparisons](../../../src/animation_playback_tests.rs). Native fixture
regeneration reads the original ELF; the Rust test consumes those fixtures and
compares state float bits and ordered service requests.

Engine allocation, lengths, RNG, skeleton/pose/mask operations, marker matrices
and handler bodies are supplied boundaries. This does not verify rendered poses,
console execution, allocator defaults, arbitrary floating-point inputs or full
gameplay. Read [scope and recovered behavior](../../ANIMATION_PLAYBACK.md).

From the repository root:

```powershell
py -3.14 _bevy/tools/animation_playback_evidence.py --check
py -3.14 _bevy/tools/animation_playback_oracle.py --check
cargo test --manifest-path _bevy/Cargo.toml --release --offline --locked animation_playback
```

The first command validates listings and source hashes. The second regenerates
native results for comparison with the checked-in fixture. The third executes
the compiled Rust implementation. This index currently covers these six
functions; it is not a repository-wide inventory of ported functions.
