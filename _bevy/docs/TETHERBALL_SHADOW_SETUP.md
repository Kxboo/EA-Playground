# Tetherball shadow startup options

`src/tetherball_shadow_setup.rs` ports `MGTetherball::SetupShadowOptions` at `0x8039cb30` (160 bytes) from the pinned `playgroundz.elf` (`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`). It reads the existing `ResetState.world_position_110`, builds the native shadow-view options, and makes one synchronous `ShadowManager::SetViewport` request with mode `1`.

The 13 float words read by `SetViewport`, in native order, are the caller’s world position, up vector `[0,1,0]`, viewport parameters `[2.5,2.5,0]`, color vector `[0.33,0,0.33]`, and scalar `0.62`. The final color values come from the actual `__sinit_shadowmanager_cpp` initializer at `0x803c63a8`, which initializes the BSS vector at `0x805f82a0`; they are not assumed-zero defaults. The port stores these exact f32 bit patterns. Padding between the Vector3 members is omitted because `SetViewport` reads only the listed members and the native constructor does not initialize that padding.

`ShadowSetupServices::set_shadow_viewport` represents the rendering singleton operation. The oracle executes the original BSS initializer, `ShadowViewOptions` constructor, vector constructors/assignments, and full `SetupShadowOptions` function; it captures the single `SetViewport` call and its consumed payload. The `SetViewport` engine implementation remains behind that service boundary.

`_bevy/tools/tetherball_shadow_setup_oracle.py` produces `_bevy/tests/data/tetherball_shadow_setup_golden.json`. Its 24 positions include zero, signed zero, ordinary and large finite coordinates, and deterministic varied inputs. The Rust adapter compares the viewport mode and all 13 consumed float words for every case. This helper does not write scalar or flag fields back to MGTetherball.
