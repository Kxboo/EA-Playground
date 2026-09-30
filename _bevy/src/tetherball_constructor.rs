//! MGTetherball constructor (0x80396410), including Minigame/World constructors.
//! Explicit big-endian storage preserves allocation bytes the native leaves alone.
use crate::tetherball_runtime::Runtime;
use crate::tetherball_startup::StartupState;

// WorldMan::StartMinigameFadeComplete allocates 0x450 at 0x803e1d94.
pub const GAME_BYTES: usize = 0x450;
#[derive(Clone, Copy, Debug)]
pub struct ConstructorInputs {
    pub scene: u32,
    /// Original four bytes at _SDA_BASE_-0x46e8. Bytes 1..3 are sign extended.
    pub base_tag: [u8; 4],
    pub invalid_game_fx: u32,
    /// ResetStats' decoded database results, an explicit synchronous dependency.
    pub score_weights: [i32; 3],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructorMetadata {
    pub world_words_004_01c: [u32; 2],
    pub scene_028: u32,
    pub tag_038: u32,
    pub ui_words_050_054: [u32; 2],
    pub result_068: u32,
    pub rules_0f8: u32,
    pub scene_2e8: u32,
    pub word_2f0: u32,
}
fn put(storage: &mut [u8; GAME_BYTES], offset: usize, value: u32) {
    storage[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}
fn tag(bytes: [u8; 4]) -> u32 {
    ((bytes[0] as u32) << 24)
        .wrapping_add((bytes[1] as i8 as i32 as u32) << 16)
        .wrapping_add((bytes[2] as i8 as i32 as u32) << 8)
        .wrapping_add(bytes[3] as i8 as i32 as u32)
}
/// Apply the complete original object stores, including the ResetStats stores
/// from supplied weights. No zero-filled allocation or game-ready default is
/// invented: callers provide the initial storage and complete session/startup.
pub fn construct(storage: &mut [u8; GAME_BYTES], input: ConstructorInputs) {
    // World::World (0x803e1564).
    put(storage, 0, 0x804ecf18);
    for offset in (4..=0x20).step_by(4) {
        put(storage, offset, 0);
    }
    storage[0x24] = 0;
    // Minigame::Minigame (0x803ab294), including MEM_fill(+78,0,80).
    put(storage, 0, 0x804dddc0);
    for offset in [0x2c, 0x30, 0x40, 0x44, 0x50, 0x54, 0x70, 0xf8, 0xfc] {
        put(storage, offset, 0);
    }
    put(storage, 0x38, tag(input.base_tag));
    for offset in [0x48, 0x5c] {
        put(storage, offset, u32::MAX);
    }
    for offset in [0x4c, 0x4d, 0x4e, 0x58] {
        storage[offset] = 0;
    }
    for offset in [0x60, 0x64, 0x68] {
        put(storage, offset, 2);
    }
    storage[0x78..0xf8].fill(0);
    put(storage, 0x28, input.scene);
    // MGTetherball::MGTetherball (0x80396410).
    put(storage, 0, 0x804dd1fc);
    for offset in [0x100, 0x204, 0x224, 0x234, 0x238, 0x440] {
        put(storage, offset, u32::MAX);
    }
    for offset in [
        0x104, 0x110, 0x114, 0x118, 0x184, 0x18c, 0x210, 0x214, 0x218, 0x21c, 0x220, 0x23c, 0x240,
        0x248, 0x24c, 0x250, 0x254, 0x258, 0x260, 0x264, 0x268, 0x26c, 0x270, 0x274, 0x278, 0x2e4,
        0x2ec, 0x2f0, 0x338, 0x33c, 0x344, 0x348, 0x34c, 0x350, 0x354, 0x358, 0x35c, 0x390, 0x394,
        0x418, 0x420,
    ] {
        put(storage, offset, 0);
    }
    for offset in [
        0x20c, 0x228, 0x229, 0x22a, 0x25c, 0x32c, 0x32d, 0x32e, 0x32f, 0x330, 0x41c, 0x41d, 0x424,
        0x444,
    ] {
        storage[offset] = 0;
    }
    put(storage, 0x334, input.invalid_game_fx);
    put(storage, 0x340, input.invalid_game_fx);
    put(storage, 0x430, 6);
    put(storage, 0x434, 2);
    put(storage, 0x438, 3);
    put(storage, 0x2e8, input.scene);
    storage[0x132..0x136].fill(0);
    for player in 0..2 {
        put(storage, 0x30c + player * 4, 1);
        put(storage, 0x314 + player * 4, 0);
        put(storage, 0x22c + player * 4, 2);
    }
    for offset in [0x42c, 0x42d, 0x42e] {
        storage[offset] = 0;
    }
    put(storage, 0x428, 1.0f32.to_bits());
    // ResetStats (0x8039cbd0) field writes; database read results stay inputs.
    storage[0x138..0x16c].fill(0);
    for (i, weight) in input.score_weights.into_iter().enumerate() {
        put(storage, 0x138 + i * 4, weight as u32);
    }
    put(storage, 0x2ec, 1);
    for slot in 0..8 {
        put(storage, 0x284 + slot * 12, 2);
        put(storage, 0x288 + slot * 12, 0);
        put(storage, 0x28c + slot * 12, 0);
    }
    put(storage, 0x43c, u32::MAX);
    storage[0x445] = 0;
}

/// Project constructor-owned fields onto existing Runtime owners. Host/actor/
/// ball state not stored by this constructor remains caller-owned. This is a
/// construction step, not a replacement for session configuration or Initialize.
pub fn construct_runtime_fields(
    runtime: &mut Runtime,
    state: &mut StartupState,
    input: ConstructorInputs,
) {
    let life = &mut runtime.life;
    let reset = &mut runtime.state.reset;
    state.world_services = [0; 6];
    state.base_flags_04c_04d = [0; 2];
    state.tag_02c = 0;
    state.tag_030 = 0;
    state.identities_078_0b8 = [[0; 2]; 2];
    for player in &mut life.players {
        player.player_flag = false;
    }
    life.paused = false;
    life.session_mode = 0;
    life.game_type = -1;
    reset.game_mode_044 = 0;
    reset.base_player_count_070 = 0;
    reset.ai_special_case_0c0 = 0;
    runtime.state.serve.pause_menu_open = false;
    runtime.state.serve.pause_block_count_0fc = 0;
    runtime.state.frontend.pregame_ready_058 = 0;
    runtime.state.frontend.postgame_choice_05c = -1;
    life.match_state.result = 2;
    life.match_state.final_result = 2;
    state.asset_handle_100 = u32::MAX;
    reset.ball_handle_104 = 0;
    state.ball_resources = None;
    reset.world_position_110 = [0.; 3];
    reset.pole_handle_184 = 0;
    life.current_distance = 0;
    life.match_state.round_winner = -1;
    life.match_state.match_over = false;
    life.player_count = 0;
    life.server = 0;
    life.receiver = 0;
    life.focus_player = 0;
    reset.receiver_220 = 0;
    reset.word_224 = -1;
    life.latches = [false; 2];
    reset.field_229 = false;
    runtime.state.gestures.hit_attempt_marker = false;
    runtime.state.rally.ai_hit_attempt_234 = -1;
    runtime.state.rally.ai_power_hit_type_238 = -1;
    runtime.state.serve.return_angles = [0.; 2];
    reset.start_angles_248 = [0.; 2];
    life.match_state.state_ms = 0;
    life.match_state.previous_state_ms = 0;
    life.match_state.elapsed_ms = 0;
    reset.field_25c = false;
    reset.counter_260 = 0;
    reset.counter_264 = 0;
    reset.counter_268 = 0;
    reset.timer_26c = 0.;
    reset.counter_270 = 0;
    runtime.state.hit.pending_zone_274 = 0;
    reset.game_marker_matrix_278 = 0;
    reset.pending_count_2e4 = 0;
    state.mode_2ec = 1;
    runtime.state.hit.field_32c = false;
    runtime.state.hit.field_32d = false;
    life.hud_ready = false;
    life.field_32f = false;
    life.serve_bubble_visible = false;
    reset.field_334_guid = input.invalid_game_fx;
    state.pole_glow_340 = input.invalid_game_fx;
    runtime.state.hit.indicator_scale_338 = 0.;
    runtime.state.hit.indicator_rate_33c = 0.;
    life.indicator_current = 0.;
    life.indicator_target = 0.;
    reset.round_tunables_34c_358 = [0.; 4];
    reset.field_35c = 0.;
    reset.camera_handle_390 = 0;
    reset.camera_heading_394 = 0.;
    life.round_number = 0;
    life.round_visible = false;
    life.scoreboard_visible = false;
    life.round_timer_ms = 0;
    runtime.state.frontend.field_424 = 0;
    runtime.state.rules.rotation_limit = 6;
    reset.rotation_limit_430 = 6;
    runtime.state.rules.wins_required = 2;
    life.total_rounds = 3;
    life.match_state.match_winner = -1;
    reset.field_444 = false;
    life.match_state.rotations = [0; 2];
    life.match_state.round_wins = [0; 2];
    life.mega_states = [1; 2];
    life.mega_values = [0; 2];
    life.action_states = [2; 2];
    runtime.state.hit.mega_ability_42e = false;
    runtime.state.serve.power_serve_enabled = false;
    runtime.state.hit.hit_multiplier_enabled_42d = false;
    runtime.state.hit.hit_multiplier_428 = 1.;
    life.statistics = [[0; 5]; 2];
    life.score_weights = input.score_weights;
    runtime.state.gestures.pending.clear();
    for slot in &mut reset.slots_284 {
        slot.kind = 2;
        slot.value = 0.;
        slot.counter = 0;
    }
    runtime.state.hit.power_hit_type_43c = -1;
    reset.alternate_server_445 = false;
    state.constructor_metadata = Some(ConstructorMetadata {
        world_words_004_01c: [0; 2],
        scene_028: input.scene,
        tag_038: tag(input.base_tag),
        ui_words_050_054: [0; 2],
        result_068: 2,
        rules_0f8: 0,
        scene_2e8: input.scene,
        word_2f0: 0,
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_constructor_memory() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_constructor_golden.json"
        ))
        .unwrap();
        assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
        for c in fixture["cases"].as_array().unwrap() {
            let mut storage: [u8; GAME_BYTES] =
                std::array::from_fn(|i| c["initial_bytes"][i].as_u64().unwrap() as u8);
            let input = ConstructorInputs {
                scene: c["input"]["scene"].as_u64().unwrap() as u32,
                base_tag: std::array::from_fn(|i| {
                    c["input"]["base_tag"][i].as_u64().unwrap() as u8
                }),
                invalid_game_fx: c["input"]["invalid_game_fx"].as_u64().unwrap() as u32,
                score_weights: std::array::from_fn(|i| {
                    c["input"]["score_weights"][i].as_i64().unwrap() as i32
                }),
            };
            construct(&mut storage, input);
            assert_eq!(serde_json::json!(storage.as_slice()), c["expected_bytes"]);
        }
    }
}
