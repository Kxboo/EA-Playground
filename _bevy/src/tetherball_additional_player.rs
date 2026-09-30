//! Original `MGTetherball::InitializeAdditionalPlayer` startup helper.
//!
//! The function appends one newly spawned character, binds its tetherball AI,
//! applies the local controller state and then updates the shared counters.
use crate::tetherball_angles::wrap_angle;
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_player_init::{
    PlayerInitInput, PlayerInitServices, PlayerInitState, SpawnCharacterCall,
};
use crate::tetherball_reset::ResetState;

/// Execute `InitializeAdditionalPlayer` at 0x8039bd20.
///
/// The native +0x210 guard leaves all state and services untouched once two
/// characters are present. For valid nonnegative counts below two, this always
/// spawns a new character and records +0x130[slot] = 1.
pub fn initialize_additional_player(
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

    // r4 is the ABI alignment word for the u64 identity parameter and is not a
    // semantic SpawnCharacter argument. The native function does pass +0x40
    // as stack word zero; retain it as a signed raw word.
    let character = services.spawn_character(SpawnCharacterCall {
        identity_words_r5_r6: input.identity_words,
        position_r7: input.position,
        arg8: 0,
        arg9: 0,
        arg10: 1,
        stack_words: [lifecycle.session_mode, -1],
    });

    services.set_character_state_position(character, input.position);
    let (sin, cos) = crate::character_input::ea_sin_cos(wrap_angle(input.heading));
    services.set_character_state_direction(character, [sin, 0.0, cos]);

    // The native body conditionally calls the AI constructor for a non-null
    // pool slot, then registers/binds the slot and stores ball +0x104 at AI+0x60.
    let entity = services.allocate_ai_slot();
    if entity != 0 {
        services.construct_tetherball_ai(entity, character);
    }
    services.add_ai_entity(entity);
    services.set_character_ai_entity(character, entity);
    services.set_ai_ball_handle(entity, reset.ball_handle_104);

    reset.current_ai_entities_128[slot] = entity;
    reset.player_handles_120[slot] = character;
    state.player_init_flags_130[slot] = 1;

    // +0x124 -> PlayerInfo +0xa8 -> Controller::Get; no ability setup occurs
    // on this path.
    let player_info = services.player_info_handle(character);
    let controller_index = services.controller_index(player_info);
    let controller = services.controller_handle(controller_index);
    services.set_controller_state(controller, 0x12);

    lifecycle.player_count = lifecycle.player_count.wrapping_add(1);
    lifecycle.session_mode = lifecycle.session_mode.wrapping_add(1);
}

#[cfg(test)]
#[path = "tetherball_additional_player_tests.rs"]
mod tests;
