//! Rebuild the retail MGTetherball shadow view options passed at startup.
use crate::tetherball_reset::ResetState;

const SHADOW_COLOR_VECTOR_BITS: [u32; 3] = [0x3ea8_f5c3, 0, 0x3ea8_f5c3];
const SHADOW_OPTION_SCALAR_BITS: u32 = 0x3f1e_b852;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowViewOptions {
    pub position: [f32; 3],
    pub up: [f32; 3],
    pub viewport_parameters: [f32; 3],
    pub color_vector: [f32; 3],
    pub scalar: f32,
}

impl ShadowViewOptions {
    /// The 13 float words that SetViewport actually reads from the native
    /// 0x44-byte options object; uninitialized inter-vector padding is omitted.
    pub fn native_words(self) -> [u32; 13] {
        [
            self.position[0].to_bits(),
            self.position[1].to_bits(),
            self.position[2].to_bits(),
            self.up[0].to_bits(),
            self.up[1].to_bits(),
            self.up[2].to_bits(),
            self.viewport_parameters[0].to_bits(),
            self.viewport_parameters[1].to_bits(),
            self.viewport_parameters[2].to_bits(),
            self.color_vector[0].to_bits(),
            self.color_vector[1].to_bits(),
            self.color_vector[2].to_bits(),
            self.scalar.to_bits(),
        ]
    }
}

/// Synchronous rendering boundary for `ShadowManager::SetViewport`.
pub trait ShadowSetupServices {
    fn set_shadow_viewport(&mut self, mode: i32, options: ShadowViewOptions);
}

/// Execute the game-owned body of `MGTetherball::SetupShadowOptions` at
/// 0x8039cb30. The engine singleton and final viewport mutation stay behind a
/// synchronous service call; option values and mode match the original call.
pub fn setup_shadow_options(reset: &ResetState, services: &mut impl ShadowSetupServices) {
    let options = ShadowViewOptions {
        position: reset.world_position_110,
        up: [0.0, 1.0, 0.0],
        viewport_parameters: [2.5, 2.5, 0.0],
        color_vector: SHADOW_COLOR_VECTOR_BITS.map(f32::from_bits),
        scalar: f32::from_bits(SHADOW_OPTION_SCALAR_BITS),
    };
    services.set_shadow_viewport(1, options);
}

#[cfg(test)]
#[path = "tetherball_shadow_setup_tests.rs"]
mod tests;
