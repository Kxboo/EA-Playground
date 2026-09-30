# Tetherball player-animation initialization

`initialize_player_animations` ports the complete 504-byte
`MGTetherball::InitializePlayerAnimations` body at `0x8039be84` from
`Remaster/reference/playgroundz.elf`, SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
It makes no engine calls and performs only integer stores.

The API receives `&mut Lifecycle`, `&ResetState`, `&mut ServeState`, and
`&mut HitAnimations`. It creates no duplicate runtime storage. The two input
flags are base-minigame player byte `GAME+0x84+player*0x40`, already represented
by `Lifecycle.players[player].player_flag`, and byte `GAME+0x32a+player`,
represented by `ResetState.server_side_flags[2+player]`.

| Native table | Shared owner | Base flag false, side false / true | Base flag true, side false / true |
| --- | --- | --- | --- |
| +190 | Lifecycle.lose_animations | 56 / 57 | 87 / 88 |
| +1a0 | HitAnimations.ready_power.state | 67 / 70 | 89 / 92 |
| +1a8 | HitAnimations.ready_power.suppress_if_current | 68 / 71 | 90 / 93 |
| +198 | HitAnimations.hit_power | 69 / 72 | 91 / 94 |
| +1b8 | HitAnimations.ready_zone_zero.state | 61 / 64 | 61 / 64 |
| +1c0 | HitAnimations.ready_zone_zero.suppress_if_current | 62 / 65 | 62 / 65 |
| +1b0 | HitAnimations.hit_zone_zero | 63 / 66 | 63 / 66 |
| +1d0 | HitAnimations.ready_reverse.state | 79 / 82 | 79 / 82 |
| +1d8 | HitAnimations.ready_reverse.suppress_if_current | 80 / 83 | 80 / 83 |
| +1c8 | ServeState.power_animations | 81 / 84 | 81 / 84 |
| +1e8 | HitAnimations.ready_zone_one.state | 73 / 76 | 73 / 76 |
| +1f0 | HitAnimations.ready_zone_one.suppress_if_current | 74 / 77 | 74 / 77 |
| +1e0 | ServeState.high_animations | 75 / 78 | 75 / 78 |

These are thirteen two-player arrays with stride four, not animation-state
changes. The routine does not issue SetNextAnimState. It does not write the
separate win-animation array at +1f8, the flags, player count, ball, or any other
modeled field. The +190 array's existing name is retained; this initializer
selects its per-player IDs without interpreting their later gameplay use.

The original loop uses signed `GAME+0x210` player count. The shared Lifecycle
stores this as usize, so this safe API supports counts 0, 1, and 2. Count zero
preserves every array; count one preserves the second player's words. Native
negative counts also skip every store, but are outside this API representation.
Counts above two index outside the modeled player arrays and are rejected.
Native flag reads treat every nonzero byte as true; host bools supply canonical
0/1 values.

`tools/tetherball_animation_init_oracle.py --check` executes the full original
body for 48 calls: all three supported counts and all sixteen independent
base/side flag combinations across both players. Arbitrary signed 32-bit seeds
make unprocessed and unrelated table preservation observable. The fixture pins
the ELF hash and records all 126 instructions and both outcomes of all four
conditional branches. No initializer instruction or decision is hooked; inherited
engine service hooks are available but asserted never called.

The standalone Rust fixture test compares the complete Lifecycle snapshot,
every supplied ServeState field, all ten HitAnimations arrays, unchanged
ResetState, native ball preservation and empty engine-effect trace. This is
original-instruction emulation evidence for the pinned executable, not console
hardware validation. ResetRound integration is performed separately by the
calling host after it writes its server-side flags.
