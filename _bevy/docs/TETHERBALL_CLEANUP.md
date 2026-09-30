# Tetherball teardown

## Recovered behavior and owners

`tetherball_cleanup::uninitialize` ports the enclosing 616-byte `MGTetherball::UnInitialize` at `0x803973d8` in the pinned ELF (`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`). It directly composes `Minigame::UnInitialize` (`0x803ab524`), `Tetherball::Uninitialize` (`0x8039d768`) and the represented ball destructor/free behavior (`0x8039d398`). The existing Runtime, StartupState, ball resources and scene remain authoritative.

The original clears +424, then despawns only character slots whose +130/+131 bytes are nonzero, clearing those +120/+124 handles. Both character loops use signed cmpw on +210; negative raw values skip the loops. Positive counts above two exceed the represented slot arrays and return an error. `StartupState.player_init` now retains the existing PlayerInitState owner for those bytes; they are not confused with the adjacent signed rotation bytes +132/+133.

Cleanup hides the active pole, reveals the alternate pole, pops controller 0, resets all four controllers through base cleanup, enables the global game byte, deletes the activity bigfile assets, cleans up the ball and releases its allocation. It does not clear the character count, AI handles or reused character pointers. A retained world player receives animation state 0, false, -1. Retained characters with a zero +12c byte switch to local control; their +120 activity flag clears regardless of that byte.

Game particles +334/+340 use the independent `0x80601f60` null GUID. Ball trails use `0x80601f78`. Each comparison and post-destruction store queries its original global token separately. Global callback purge always runs. A ready HUD hides the timer and both mega meters, clears its readiness byte, then clears handlers/closes the screen. Pole-indicator offset 0, unduck/stop music, unload audio kind 8, renderer +1c0=0 and area restoration follow in original order.

`tetherball_ball_init::uninitialize_ball` destroys three non-null trails, four non-null cached models, unconditionally removes the ball shadow from scene layer 0 (including null), then destroys both non-null shadow entities. Only the ball shadow is removed from the scene; initialization never registered the rope shadow. It clears cached/shadow pointers and +15c while preserving asset ids, motion, matrices and acceleration modifier. Rust drops BallResources ownership after the enclosing free, matching the live ball handle becoming zero.

## Evidence and validation

Verified: 64 native scenarios execute the enclosing original body, complete base/ball cleanup, the real ball destructor and RestoreArea's wrapper. Cases vary signed counts -1/0/1/2 independently of spawn flags, reused/spawned/null characters, both world-player identity outcomes, local-control suppression, HUD readiness, cached/shadow null combinations, active/null trails and independent game/ball null tokens. Rust compares ordered service effects, every mapped game word plus untouched word preservation, character activity flags, pole visibility, ball resource stores, trails, released ownership and unchanged ball motion bits. The oracle captures native memory after resource-release calls; its free hook retains bytes for inspection, not as a claim that freed objects remain usable.

Explicit synchronous service boundaries: character-manager despawn, character local control and animation internals, camera/controller internals, asset deletion, cached/shadow object destruction, renderer scene removal, particle manager, callback/UI/audio managers, pole-indicator rendering and PlaygroundWorld area restoration. Only the bounding routines and listed composed children execute unhooked. Fixture inputs are synthetic; no console recording or interactive Bevy teardown comparison is claimed.

A null live ball or missing BallResources corresponds to an unsupported/incomplete initialization state. The port returns an error before teardown instead of inventing a successful native null-ball path (the original calls ball Uninitialize before its pointer guard). After successful teardown the host must discard this session; calling this function again on released ownership returns an error. Scene asset readiness, actual ECS despawning, callback adapters, renderer and audio integration still require the Bevy host.

Run from `_bevy`:

- `py -3.14 tools/tetherball_cleanup_oracle.py --check`
- `cargo test --release --offline --locked tetherball_startup::tests`

The new oracle is registered in `tools/prove.py`; `original_complete_cleanup` is in the existing startup fixture suite, sharing its state projection and engine recorder.
