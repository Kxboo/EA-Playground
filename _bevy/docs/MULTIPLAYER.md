# Multiplayer tournament scoring

`src/multiplayer.rs` ports the renderer-independent scoring state in the original
`MultiplayerMode`; it does not implement playable minigames or post-game UI.

Evidence is the original `playgroundz.elf` instructions:

| Routine | Address |
|---|---|
| StartFreePlay | 0x803af1b0 |
| StartPointSeries | 0x803af220 |
| AddRoundResults | 0x803af29c |
| AddWinResults | 0x803af45c |
| SetLastPlacement | 0x803af60c |
| GetNumRoundsLeft / score, rank and last-result queries | 0x803af630–0x803af754 |

`tools/multiplayer_oracle.py` runs those instructions in `ppc_emu2.py` and writes
`tests/data/multiplayer_golden.json`. Run `python _bevy/tools/multiplayer_oracle.py --check`
from the repository root to regenerate in memory and compare. The oracle pins SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`, recorded in the fixtures and asserted against `recovered::ELF_SHA256`. No executable contents
are included in the fixtures. MEM_copy is hooked with an exact memory copy; compiler
register save/restore helpers are skipped for isolated routine calls. There are no
hooks for score, rank, counter, placement or query behavior.

Six sessions cover two, three and four players in free play and point series, with
210 transitions including random scores/winners, ties, skipped slots, duplicate
winners, resets, signed overflow and negative rounds. Rust tests compare every
scoring field after each transition and original point-total, win-total, last-points, rounds-left, rank and last-win queries.

Preserved behavior:

- Arithmetic wraps as signed 32-bit PowerPC arithmetic. Only AddRoundResults
  decrements rounds_left; both result submission methods increment results_count.
  Submitting both for one game therefore counts twice.
- First compute each rank as number of strictly higher totals. Then run the
  original ordered pair loop: equal current ranks compare previous ranks and
  increment one rank in place. This is not a stable sort. Equal previous ranks
  enter the else branch and increment the current player's rank; the prose in
  GameMap/docs/05-minigames-and-rules.md about mutual ties remaining tied is not
  the behavior of these instructions.
- Player slots zero and one always add scores, including -1. Slots two and three
  skip -1, retaining their previous match points. Storage exists for all four
  slots even when the active player count is smaller.
- Free play resets wins, win ranks, winners and placement but retains points,
  point ranks and last match points. Point-series restart also resets points and
  point ranks, but still retains last match points.
- A duplicated winner gains two wins. No winners still reranks and counts a result.
- Fourth placement argument overwrites slot two; slot three is never written.
- Rank lookup returns the first matching active player, or None (original -1).

Safe API boundaries: constructors accept 2–4 players; queries and winner inputs
reject inactive/out-of-range player indices before mutation. Original routines
use unchecked indexing. Optional winners represent the original -1 sentinel.
Placement values are raw records and may include -1. New state zero-initializes
score fields that the original constructor leaves untouched; start a mode before
submitting results. Teams copying, setup flags, rules storage and the minigame
post-game award switch are outside this port.
