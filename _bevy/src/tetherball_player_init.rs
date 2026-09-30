//! Original `MGTetherball::InitializePlayer` startup helper.
//!
//! The PowerPC function owns the reuse/spawn branch, +0x120/+0x128/+0x130
//! writes, +0x210/+0x40 increments, and ordered setup calls. Engine-owned
//! character, controller, pool and ability operations are synchronous services.
use crate::tetherball_angles::wrap_angle;
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_reset::ResetState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExistingCharacter {
    pub handle: u32,
    pub identity_words: [u32; 2],
}

/// Register and stack arguments consumed by the native SpawnCharacter call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnCharacterCall {
    pub identity_words_r5_r6: [u32; 2],
    pub position_r7: [f32; 3],
    pub arg8: u32,
    pub arg9: u32,
    pub arg10: u32,
    pub stack_words: [i32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerInitInput {
    pub position: [f32; 3],
    pub heading: f32,
    pub identity_words: [u32; 2],
}

/// Explicit synchronous boundaries called by InitializePlayer. Their ordering
/// mirrors the original call sites; game-owned scalar stores remain in Rust.
pub trait PlayerInitServices {
    fn get_player_character(&mut self, player: i32) -> Option<ExistingCharacter>;
    fn spawn_character(&mut self, call: SpawnCharacterCall) -> u32;
    fn set_character_state_position(&mut self, character: u32, position: [f32; 3]);
    fn set_character_state_direction(&mut self, character: u32, direction: [f32; 3]);

    fn allocate_ai_slot(&mut self) -> u32;
    fn construct_tetherball_ai(&mut self, entity: u32, character: u32);
    fn add_ai_entity(&mut self, entity: u32);
    fn set_character_ai_entity(&mut self, character: u32, entity: u32);
    fn set_ai_ball_handle(&mut self, entity: u32, ball: u32);

    fn multiplayer_enable_byte(&mut self) -> u8;
    fn setup_multiplayer_ability(&mut self);
    fn setup_single_player_ability(&mut self);
    fn player_info_handle(&mut self, character: u32) -> u32;
    fn controller_index(&mut self, player_info: u32) -> i32;
    fn controller_handle(&mut self, index: i32) -> u32;
    fn set_controller_state(&mut self, controller: u32, state: i32);
}

/// MGTetherball's byte at +0x130. `InitializePlayer` writes only the slot it
/// appends: zero when an existing player character is reused, one when spawned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerInitState {
    pub player_init_flags_130: [u8; 2],
}

impl PlayerInitState {
    pub const fn new(player_init_flags_130: [u8; 2]) -> Self {
        Self {
            player_init_flags_130,
        }
    }
}

/// Execute the complete body of `InitializePlayer` at 0x8039ba00.
///
/// The native `player_count >= 2` guard returns without side effects. Character
/// state, pooling, ability setup and controller operations are applied synchronously
/// through `services`; offsets in MGTetherball and its two shared handle arrays
/// are updated directly.
pub fn initialize_player(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    state: &mut PlayerInitState,
    input: PlayerInitInput,
    services: &mut impl PlayerInitServices,
) {
    let slot = lifecycle.player_count;
    if slot >= 2 {
        return;
    }

    // Native calls WorldMan::GetPlayerCharacter(0) once to test null, a second
    // time to compare both identity words, and a third time only on a match.
    let first = services.get_player_character(0);
    let reuse = if first.is_some() {
        services
            .get_player_character(0)
            .expect("WorldMan character disappeared during native identity comparison")
            .identity_words
            == input.identity_words
    } else {
        false
    };
    let character = if reuse {
        services
            .get_player_character(0)
            .expect("WorldMan character disappeared during native reuse lookup")
            .handle
    } else {
        services.spawn_character(SpawnCharacterCall {
            identity_words_r5_r6: input.identity_words,
            position_r7: input.position,
            arg8: 0,
            arg9: 0,
            arg10: 1,
            stack_words: [0, -1],
        })
    };

    services.set_character_state_position(character, input.position);
    let (sin, cos) = crate::character_input::ea_sin_cos(wrap_angle(input.heading));
    services.set_character_state_direction(character, [sin, 0.0, cos]);

    // Native performs one pool allocation, conditionally invokes the AI ctor,
    // then unconditionally registers/binds the returned pointer and writes the
    // ball handle at AIEntity+0x60.
    let entity = services.allocate_ai_slot();
    if entity != 0 {
        services.construct_tetherball_ai(entity, character);
    }
    services.add_ai_entity(entity);
    services.set_character_ai_entity(character, entity);
    services.set_ai_ball_handle(entity, reset.ball_handle_104);

    // Preserve native store order and byte values: this flag distinguishes a
    // reused world character (0) from a newly spawned character (1).
    reset.current_ai_entities_128[slot] = entity;
    reset.player_handles_120[slot] = character;
    state.player_init_flags_130[slot] = u8::from(!reuse);

    if services.multiplayer_enable_byte() != 0 {
        services.setup_multiplayer_ability();
    } else {
        services.setup_single_player_ability();
    }

    let player_info = services.player_info_handle(character);
    let controller_index = services.controller_index(player_info);
    let controller = services.controller_handle(controller_index);
    services.set_controller_state(controller, 0x12);

    // Both native signed additions wrap in 32 bits.
    lifecycle.player_count = lifecycle.player_count.wrapping_add(1);
    lifecycle.session_mode = lifecycle.session_mode.wrapping_add(1);
}

#[cfg(test)]
#[path = "tetherball_player_init_tests.rs"]
mod tests;
