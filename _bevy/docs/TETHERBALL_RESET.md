# Tetherball round reset and server selection

`src/tetherball_reset.rs` ports the scalar, ball, and server-selection logic
inside `MGTetherball::ResetRound` at `0x803994f4`, `ResetMiniGame` at
`0x80399460`, and `SetUpServer` at `0x8039b1b0` from the pinned
`playgroundz.elf` (`5cef3efc…a3e2c`). It updates `Lifecycle`, `BallMotion`, and
a typed `ResetState`, while preserving calls into dependencies whose bodies
remain outside this port.
`ResetState` contains only game-object values without an existing Lifecycle
field; lifecycle offsets such as rotations, winner, active server pair,
indicators, HUD flags, score values, and AI distances are updated in place.

## Call order and recovered stores

`ResetMiniGame` first invokes `ResetStats`. It zeros the three score weights,
asks the game object for its difficulty, and reads `accuracy_points`,
`powerhit_points`, and `megahit_points` only when that index is below the
database array count. It then clears both five-word statistic rows and round
wins, resets the match winner and round number, clears the alternating-server
flag, and calls `ResetRound`. After that call it sets `+0x444`, hides the
scoreboard only when its flag and HUD-ready flag are both true, then clears the
scoreboard flag.

`ResetRound` hides the timer only when `+0x32e` says the HUD is ready. The
serve-bubble hide is nested in that branch and runs only when `+0x330` is set.
It destroys the round FX handle if it differs from the game-FX sentinel. HUD
initialization is requested when `+0x32e` is false; the native HUD initializer
does not itself set that flag. The function clears
`+0x25c`, sets `+0x224` and the round winner to `-1`, and rewrites all eight
gesture records at `+0x284..+0x2e0` to kind `2`, value `0`, counter `0`. It
preserves both the live-record count at `+0x2e4` and marker byte `+0x229`.

Round counters and elapsed time are cleared. Rotations become the signed-byte
pair `[-initial_rotation, initial_rotation]`; latches clear, action states
become `[2, 2]`, megameter values become `[0, 0]`, and megameter states become
`[1, 1]`. Indicator current becomes zero and target becomes the first rotation
divided by the rotation limit. The pole texture matrix remains identity because
the native call passes a zero indicator offset.

The initial server's start angle is wrapped before the first `Grab`. A distance
change to `2` sets each character's movement speed and AI distance, switches
the controls to AI, and rescales the ball's desired radius. If distance is
already `2`, the original setter returns without those writes; the later
`SetRadius(1.1)` still runs. Camera configuration uses the executed
`__sinit_mgtetherball` globals (`[0, 1.075, 0]` position offset and zero target
offset), distance-indexed backwards offset `5.0`, desired rotation
`camera_heading + 5.47`, and the world-matrix transform of local `[0.5, 0, 0]`
plus world position. The camera's direct height and zero fields are set to
`2.2` and `0`.

Game mode `0..=3` selects the four VLT tunable rows. Other values are rejected
by the typed boundary because the native routine leaves its difficulty
register undefined there. Character separation uses the current-distance
table; after the reset's distance setter, its index is always `2` and the
separation is `1.65`, independent of difficulty. The function creates
two AI objects and binds each to the ball, calls player one's AI initializer
with its enabled flag set when `+0xc0 == 6` or the global AI byte is set, then writes target angles, initial
values, and distance. It places the characters at world position plus or minus
that separation, switches both controls to AI again, clears serve flags, reads
the four tunables, and calls `SetUpServer`.

`SetUpServer` always requests a random value in `[0, 1]`. A one-character base
player count selects player zero; counts above one alternate between zero and
one using `+0x445`; all other counts use the supplied random result. It updates
the active pair (`+0x214/+0x218`), focus/server (`+0x21c`), initial receiver
(`+0x220`), server side flags, and AI initial values. It gets the behind-the-
back camera, transforms local `+0.5` or `-0.5` by the world matrix without
adding world position, and sets camera direction from the wrapped heading plus
`5.47` (and `pi` for server one). The animation initializer synchronously
supplies the new `+0x190/+0x194` values before the receiver's next animation is
selected. Marker IDs are read from the selected server's animation; marker
`63` supplies game matrix `+0x278`. When the ball's angular velocity is zero,
the function wraps the new server's start angle and performs the final `Grab`
with that matrix.

## Service boundary

`ResetEffect` retains the order of calls across engine and gameplay
dependencies. The caller supplies synchronous dependency results through
`ResetInputs` and applies the returned requests in order. `InitializeAptHud`
is an unported HUD dependency and does not synthesize a write to `+0x32e`.
`InitializePlayerAnimations` is an unported gameplay helper: its supplied
`+0x190/+0x194` results update `Lifecycle.lose_animations` before the receiver
animation is selected, and marker IDs/matrices remain supplied inputs. Camera
manager lookup and setters, particle destruction, AI object allocation/binding/
initialization, VLT reads, animation state changes, random output, and control
switches remain explicit call boundaries. The Rust code performs the recovered
server choice, game-object scalar stores, transforms, ball operations, tunable
selection, and the conditions around these calls; it does not claim to port the
dependencies' internals or replace missing objects with defaults. The game FX
sentinel (`0x80601f60`) and ball trail FX sentinel (`0x80601f78`) are separate
inputs.

The reset oracle executes the pinned PPC functions and their unhooked matrix,
ball, angle, distance, and server logic for 72 round/minigame/server cases. Its
paired-single emulation models `ps_merge00`, `ps_mul`, and fused `ps_madd` for
finite `f32` inputs; this does not claim hardware FPSCR or exception behavior.
The instruction model follows Dolphin's paired-single interpreter
([`Interpreter_Paired.cpp`](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/Core/PowerPC/Interpreter/Interpreter_Paired.cpp)).
The Rust fixture test compares complete lifecycle state, ball bits, auxiliary
stores, camera/AI snapshots, and ordered service traces against
`tests/data/tetherball_reset_golden.json`.
