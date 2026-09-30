//! Native `MGTetherball::InitializeAI` startup helper (0x8039bbec).
//!
//! This is the non-player-controlled character path: unlike InitializePlayer
//! it always spawns, does not query/reuse the current world character, and
//! appends one character/AI pair. Engine operations reuse the existing
//! `PlayerInitServices` boundary; shared handles/count/flags live in their
//! existing Lifecycle, ResetState and PlayerInitState owners.
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_player_init::{
    PlayerInitInput, PlayerInitServices, PlayerInitState, SpawnCharacterCall,
};
use crate::tetherball_reset::ResetState;

/// Execute `InitializeAI` for its memory-safe native player-count domain.
/// Counts 2 or greater return before making a host call or changing state.
/// Negative raw counts are outside the typed Lifecycle domain.
///
/// The retail allocation-failure branch is not a successful typed path: after
/// skipping the constructor, native code still stores at `entity + 0x60` and
/// can dereference null. A host receiving entity handle zero from
/// `allocate_ai_slot` must preserve that fault at `set_ai_ball_handle`.
pub fn initialize_ai(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    player_init: &mut PlayerInitState,
    input: PlayerInitInput,
    services: &mut impl PlayerInitServices,
) {
    let slot = lifecycle.player_count;
    if slot >= 2 {
        return;
    }

    let character = services.spawn_character(SpawnCharacterCall {
        identity_words_r5_r6: input.identity_words,
        position_r7: input.position,
        arg8: 0,
        arg9: 0,
        arg10: 0,
        stack_words: [0, -1],
    });

    let entity = services.allocate_ai_slot();
    if entity != 0 {
        services.construct_tetherball_ai(entity, character);
    }
    services.add_ai_entity(entity);
    services.set_character_ai_entity(character, entity);
    services.set_ai_ball_handle(entity, reset.ball_handle_104);
    reset.current_ai_entities_128[slot] = entity;

    let (sin, cos) =
        crate::character_input::ea_sin_cos(crate::tetherball_angles::wrap_angle(input.heading));
    services.set_character_state_direction(character, [sin, 0.0, cos]);

    reset.player_handles_120[slot] = character;
    player_init.player_init_flags_130[slot] = 1;
    lifecycle.player_count = lifecycle.player_count.wrapping_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tetherball_player_init::ExistingCharacter;
    use serde_json::{Value, json};

    const CHARACTER: u32 = 0x7100_2000;
    const AI: u32 = 0x7100_5000;

    fn uint(v: &Value) -> u32 {
        v.as_u64()
            .map(|x| x as u32)
            .unwrap_or_else(|| v.as_i64().unwrap() as u32)
    }
    fn float(v: &Value) -> f32 {
        f32::from_bits(uint(v))
    }
    fn vec3(v: &Value) -> [f32; 3] {
        [float(&v[0]), float(&v[1]), float(&v[2])]
    }
    fn bits(v: [f32; 3]) -> [u32; 3] {
        v.map(f32::to_bits)
    }

    #[derive(Default)]
    struct Recorder {
        input: Value,
        events: Vec<Value>,
        ball_write: Option<(u32, u32)>,
    }
    impl PlayerInitServices for Recorder {
        fn get_player_character(&mut self, _: i32) -> Option<ExistingCharacter> {
            panic!("InitializeAI never performs world-player reuse")
        }
        fn spawn_character(&mut self, call: SpawnCharacterCall) -> u32 {
            self.events.push(json!([
                "spawn_character",
                call.identity_words_r5_r6[0],
                call.identity_words_r5_r6[1],
                bits(call.position_r7),
                call.arg8,
                call.arg9,
                call.arg10,
                call.stack_words
            ]));
            CHARACTER
        }
        fn set_character_state_position(&mut self, _: u32, _: [f32; 3]) {
            panic!("InitializeAI does not set position after spawn")
        }
        fn set_character_state_direction(&mut self, character: u32, direction: [f32; 3]) {
            self.events.push(json!([
                "set_character_state_direction",
                character,
                bits(direction)
            ]));
        }
        fn allocate_ai_slot(&mut self) -> u32 {
            self.events.push(json!(["allocate_ai_slot"]));
            uint(&self.input["ai_entity_handle"])
        }
        fn construct_tetherball_ai(&mut self, entity: u32, character: u32) {
            self.events
                .push(json!(["construct_tetherball_ai", entity, character]));
        }
        fn add_ai_entity(&mut self, entity: u32) {
            self.events.push(json!(["add_ai_entity", entity]));
        }
        fn set_character_ai_entity(&mut self, character: u32, entity: u32) {
            self.events
                .push(json!(["set_character_ai_entity", character, entity]));
        }
        fn set_ai_ball_handle(&mut self, entity: u32, ball: u32) {
            self.ball_write = Some((entity, ball));
        }
        fn multiplayer_enable_byte(&mut self) -> u8 {
            panic!("InitializeAI does not configure abilities")
        }
        fn setup_multiplayer_ability(&mut self) {
            panic!("InitializeAI does not configure abilities")
        }
        fn setup_single_player_ability(&mut self) {
            panic!("InitializeAI does not configure abilities")
        }
        fn player_info_handle(&mut self, _: u32) -> u32 {
            panic!("InitializeAI does not query controller")
        }
        fn controller_index(&mut self, _: u32) -> i32 {
            panic!("InitializeAI does not query controller")
        }
        fn controller_handle(&mut self, _: i32) -> u32 {
            panic!("InitializeAI does not query controller")
        }
        fn set_controller_state(&mut self, _: u32, _: i32) {
            panic!("InitializeAI does not set controller state")
        }
    }

    #[test]
    fn original_initialize_ai_stores_and_ordered_calls() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_ai_init_golden.json"))
                .unwrap();
        assert_eq!(
            fixture["elf_sha256"].as_str(),
            Some(crate::recovered::ELF_SHA256)
        );
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 21);
        let life_fixture: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_lifecycle_golden.json"
        ))
        .unwrap();
        let seed = &life_fixture["cases"][0]["initial"];
        for (i, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let v = &c["input"];
            let mut life: Lifecycle = serde_json::from_value(seed.clone()).unwrap();
            life.player_count = uint(&v["player_count"]) as usize;
            life.session_mode = uint(&v["session_mode"]) as i32;
            let mut reset = ResetState::blank();
            reset.ball_handle_104 = uint(&v["ball_handle"]);
            reset.player_handles_120 = std::array::from_fn(|n| uint(&v["player_handles"][n]));
            reset.current_ai_entities_128 = std::array::from_fn(|n| uint(&v["ai_handles"][n]));
            let mut state =
                PlayerInitState::new(std::array::from_fn(|n| uint(&v["flags_130"][n]) as u8));
            let mut host = Recorder {
                input: v.clone(),
                ..Default::default()
            };
            initialize_ai(
                &mut life,
                &mut reset,
                &mut state,
                PlayerInitInput {
                    position: vec3(&v["position_bits"]),
                    heading: float(&v["heading_bits"]),
                    identity_words: [uint(&v["identity_words"][0]), uint(&v["identity_words"][1])],
                },
                &mut host,
            );

            let mut expected_events = c["events"].clone();
            for event in expected_events.as_array_mut().unwrap() {
                if event[0] == "set_ai_control_entity" {
                    assert_eq!(
                        uint(&event[1]),
                        uint(&v["control_handle"]),
                        "case {i}: native control pointer"
                    );
                    // The shared service API identifies its Character owner;
                    // the fixture retains the raw control pointer above.
                    event[0] = json!("set_character_ai_entity");
                    event[1] = json!(CHARACTER);
                }
            }
            assert_eq!(
                json!(host.events),
                expected_events,
                "case {i}: ordered service calls"
            );
            assert_eq!(
                life.player_count,
                uint(&c["player_count"]) as usize,
                "case {i}: +210"
            );
            assert_eq!(
                life.session_mode,
                uint(&v["session_mode"]) as i32,
                "case {i}: untouched +40"
            );
            assert_eq!(
                json!(reset.player_handles_120),
                c["player_handles"],
                "case {i}: +120"
            );
            assert_eq!(
                json!(reset.current_ai_entities_128),
                c["ai_handles"],
                "case {i}: +128"
            );
            assert_eq!(
                json!(state.player_init_flags_130),
                c["flags_130"],
                "case {i}: +130"
            );
            let expected_ball_write = if uint(&v["player_count"]) < 2 {
                Some((uint(&v["ai_entity_handle"]), uint(&c["ai_ball_handle_60"])))
            } else {
                None
            };
            assert_eq!(host.ball_write, expected_ball_write, "case {i}: entity +60");
        }
    }
}
