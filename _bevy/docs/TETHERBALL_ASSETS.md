# Tetherball activity asset preparation

`tetherball_assets::Decoded::load` reads the original `files/data/minigames/tetherball/mgtetherball.viv` through the existing Rust archive, model, texture and material pipeline. Names are taken from `MGTetherball::Initialize` (`0x803966c4`) and `Tetherball::Initialize` (`0x8039d3d8`): `teatherball.o`, `teatherball_rope.o`, their two `.gsh` banks and their two `_shadow.o` models. The spelling is original. No extracted assets are committed.

Preparation decodes every texture-bank image, requires nonempty visible geometry with no material-resolution warnings, and retains decoded shadow geometry. All CPU work completes before modifying Bevy's asset stores. `install` creates an activity-owned `Prepared` resource using the existing `assets::upload`; `ready` checks its mesh, material and image handles in the main World. This check does not establish renderer/GPU readiness, scene readiness or gameplay readiness.

`release` removes only the activity's meshes, materials, images and resource. Reinstallation releases the previous bundle. The scene owner must remove activity entities before replacing or releasing the bundle. A failed CPU load leaves the previously prepared bundle intact. Missing asset stores fail before mutation.

Run `cargo test --release --offline --locked tetherball_assets -- --nocapture` to exercise original-archive loading, main-world readiness, repeated installation, failed input, release and preservation of unrelated assets. `EAGL-Workbench --prepare-tetherball [--data <DATA>]` uses the same preparation API without opening a window and reports decoded geometry and texture counts.

The 2026-09-30 release test run passed all 114 tests. The original archive produced two ball primitives (192 triangles), one rope primitive (60 triangles), two 128×128 ball textures and one 128×64 rope texture, with no material warnings. Both shadow models decoded (192 and 60 triangles). Removing an owned image made readiness false; restart and repeated release returned asset counts to the baseline, preserving an unrelated material. Missing stores failed before installation.

The release executable was also built and run with `--prepare-tetherball`: it returned `main_world_assets_ready: true` and exit 0. A missing `--data` directory returned the input-path diagnostic and exit 1. For PowerShell scripts, wait for this Windows GUI-subsystem executable with `Start-Process -WindowStyle Hidden -Wait -PassThru` and inspect its ExitCode.

This is a dependency for the live activity host, not a playable scene. Original shadow-volume composition is unimplemented and explicitly reported; the shadows are retained rather than drawn with substitute materials. Character selection/spawning, marker poses, pole data, animation callbacks, cameras, controls and frontend still need host adapters. Normal entry and developer entry will share this preparation boundary with the recovered session entry and cleanup.
