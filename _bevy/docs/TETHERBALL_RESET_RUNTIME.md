# Round and minigame reset animation composition

`tetherball_reset_runtime::reset_round_with_animations` composes the recovered
ResetRound projection with the complete native InitializePlayerAnimations body.
Its arguments are the existing ResetRound arguments followed by mutable
ServeState and HitAnimations. It returns the ordered ResetEffect list with the
consumed InitializePlayerAnimations game-helper marker removed. The initializer
has no engine effects, so every other effect retains its native order.

SetUpServer writes its selected server, receiver and side bytes before calling
InitializePlayerAnimations (`0x8039be84`). It then requests animation 58 for the
server and reads the receiver's newly initialized +190 entry. The adapter
restores the pre-reset +190 array, applies the initializer at that marker while
composing the effect list, and replaces the second AnimationNextState's supplied
lookup result with the recovered array entry. Restoration matters for player
counts zero and one: unprocessed entries must retain their original values.
The legacy ResetInputs.initialized_animation_states field is ignored at this
specific initializer/receiver-lookup boundary.

This is a narrow pure-state/effect composition, not an engine callback replay.
The existing ResetRound projection computes its state before its returned
effects are dispatched. No subsequent reset scalar operation reads the other
animation tables, and neither player flags nor player count change across this
boundary, so the composition uses exactly the initializer's native inputs.
The adapter assumes the existing explicit service inputs; it does not support
arbitrary external callbacks mutating unrelated game fields between effects.
AI allocation/initialization, database, controller, camera and marker boundaries
remain supplied by the host. In particular, this adapter does not synchronize
unmodeled memory of newly allocated AI entities into ServeState shadows.

`reset_minigame_with_animations` applies the same composition to ResetMiniGame,
including its ResetStats and subsequent ResetRound call.

`tools/tetherball_reset_runtime_oracle.py --check` executes original ResetRound
at `0x803994f4` and removes the old hook for InitializePlayerAnimations so its
full original body executes. It retains only the existing reset oracle's other
explicit engine/database/AI boundaries. The ELF is pinned to SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
The 576 cases combine 24 round-reset and 24 minigame-reset scenarios with counts 0/1/2
and all four independent two-player base-flag combinations; native server
selection determines the side flags. Arbitrary initial table words expose
unprocessed-player preservation. The native trace contains no
initialize_animations service event because that helper executes internally.

The Rust test compares the full native Lifecycle snapshot, all BallMotion bits,
the complete reset auxiliary snapshot and ordered engine trace using the strict
existing reset test adapter. It additionally compares all ten HitAnimations
arrays and both native ServeState animation arrays. Other supplied ServeState
fields are asserted unchanged as a host-state contract; they are not claimed as
complete snapshots of newly allocated engine objects. Tests deliberately poison
the old supplied initializer output, proving that it no longer determines either
the initialized table or receiver animation. Safe player count and index domains
remain those of the recovered reset and animation initializer APIs.

Only test visibility was shared: `tetherball_reset::tests` and its seed,
effect_trace and snapshot helpers are pub(crate), avoiding duplicate fixture
mapping. Original reset behavior and its existing fixture remain unchanged.
This is instruction-emulation evidence for the pinned executable, not console
hardware validation or a complete runtime host.
