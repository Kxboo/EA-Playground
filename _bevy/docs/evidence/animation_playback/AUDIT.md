# Independent evidence review, 2026-09-30

A separate GPT-6.1-sol agent at low reasoning effort performed a read-only review
of the playback module, native oracle, Rust comparison and proof registration.
It independently checked the six playback addresses and their sizes against ELF
symbols: SetNextAnim 484 bytes, Update 1,152, ProcessAnimEvents 460,
SetNextAnimState 268, SetStateTime 80 and GetStateTime 64.

Its conclusion was credible finite differential evidence for scalar state and
ordered projected service traces, with no full-game or rendered-pose proof.
It also confirmed the documented UseFPS/GetLength addresses, while stressing
that those bodies are service hooks in this playback oracle.

The review identified missing durable function mappings and saved listings.
The generated README/index/listings now address that discoverability gap and
include code/source hashes, direct case IDs and the installed hook inventory.
The proof runner now checks that index and executes the playback Rust test.

Outstanding limits from the review:

- No instruction/branch coverage or executed-callee ledger for this corpus.
- Synthetic combinations share correlated flags; they are not exhaustive.
- Time getter cases use setter-produced state rather than an independent corpus.
- Engine pose/mask/marker/function-length bodies remain supplied boundaries.
  Silent mask construction hooks do not compare their own call order.
- Handler removal is exercised; arbitrary callback mutation, handler additions
  and marker registration during callbacks are not established.
- Rust safety errors outside valid native inputs are intentional deviations.
- This six-function index does not yet inventory every port in the repository.

Verification after adding the index: eight generated files checked successfully;
native regeneration matched all 256 fixture cases; compiled Rust
`animation_playback::tests::original_animation_state_calls` passed. The preceding
implementation milestone passed the full 116-test release suite. No production
Rust behavior changed during this evidence-organizing milestone.
