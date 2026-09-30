use super::*;
use serde_json::{Value, json};

const EXISTING: u32 = 0x7100_1000;
const SPAWNED: u32 = 0x7100_2000;
const PLAYER_INFO: u32 = 0x7100_3000;
const CONTROLLER: u32 = 0x7100_4000;
const AI: u32 = 0x7100_5000;

fn u32v(v: &Value) -> u32 {
    v.as_u64()
        .map(|x| x as u32)
        .unwrap_or_else(|| v.as_i64().unwrap() as u32)
}
fn i32v(v: &Value) -> i32 {
    u32v(v) as i32
}
fn f32v(v: &Value) -> f32 {
    f32::from_bits(u32v(v))
}
fn vec3(v: &Value) -> [f32; 3] {
    [f32v(&v[0]), f32v(&v[1]), f32v(&v[2])]
}
fn bits3(v: [f32; 3]) -> [u32; 3] {
    v.map(f32::to_bits)
}

#[derive(Default)]
struct Recorder {
    input: Value,
    events: Vec<Value>,
    ai_ball_store: Option<(u32, u32)>,
}

impl PlayerInitServices for Recorder {
    fn get_player_character(&mut self, player: i32) -> Option<ExistingCharacter> {
        self.events.push(json!(["get_player_character", player]));
        if self.input["existing"].as_bool().unwrap() {
            Some(ExistingCharacter {
                handle: EXISTING,
                identity_words: [
                    u32v(&self.input["existing_identity"][0]),
                    u32v(&self.input["existing_identity"][1]),
                ],
            })
        } else {
            None
        }
    }

    fn spawn_character(&mut self, call: SpawnCharacterCall) -> u32 {
        self.events.push(json!([
            "spawn_character",
            call.identity_words_r5_r6,
            bits3(call.position_r7),
            call.arg8,
            call.arg9,
            call.arg10,
            call.stack_words,
        ]));
        SPAWNED
    }

    fn set_character_state_position(&mut self, character: u32, position: [f32; 3]) {
        self.events.push(json!([
            "set_character_state_position",
            character,
            bits3(position)
        ]));
    }

    fn set_character_state_direction(&mut self, character: u32, direction: [f32; 3]) {
        self.events.push(json!([
            "set_character_state_direction",
            character,
            bits3(direction)
        ]));
    }

    fn allocate_ai_slot(&mut self) -> u32 {
        self.events.push(json!(["allocate_ai_slot"]));
        AI
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
        self.ai_ball_store = Some((entity, ball));
    }

    fn multiplayer_enable_byte(&mut self) -> u8 {
        self.input["multiplayer_enable"].as_u64().unwrap() as u8
    }

    fn setup_multiplayer_ability(&mut self) {
        self.events.push(json!(["setup_multiplayer_ability"]));
    }

    fn setup_single_player_ability(&mut self) {
        self.events.push(json!(["setup_single_player_ability"]));
    }

    fn player_info_handle(&mut self, _character: u32) -> u32 {
        PLAYER_INFO
    }
    fn controller_index(&mut self, _player_info: u32) -> i32 {
        i32v(&self.input["controller_index"])
    }

    fn controller_handle(&mut self, index: i32) -> u32 {
        self.events.push(json!(["controller_get", index]));
        CONTROLLER
    }

    fn set_controller_state(&mut self, controller: u32, state: i32) {
        self.events
            .push(json!(["set_controller_state", controller, state]));
    }
}

#[test]
fn matches_original_initialize_player_cases() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_player_init_golden.json"
    ))
    .unwrap();
    let lifecycle_fixture: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_lifecycle_golden.json"
    ))
    .unwrap();
    let base_lifecycle = lifecycle_fixture["cases"][0]["initial"].clone();

    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let input = &case["input"];
        let mut lifecycle: Lifecycle = serde_json::from_value(base_lifecycle.clone()).unwrap();
        lifecycle.player_count = u32v(&input["player_count"]) as usize;
        lifecycle.session_mode = i32v(&input["session_mode"]);

        let mut reset = ResetState::blank();
        reset.ball_handle_104 = u32v(&input["ball_handle"]);
        reset.player_handles_120 = [
            u32v(&input["initial_player_handles"][0]),
            u32v(&input["initial_player_handles"][1]),
        ];
        reset.current_ai_entities_128 = [
            u32v(&input["initial_ai_handles"][0]),
            u32v(&input["initial_ai_handles"][1]),
        ];
        let mut state = PlayerInitState::new([
            input["initial_flags"][0].as_u64().unwrap() as u8,
            input["initial_flags"][1].as_u64().unwrap() as u8,
        ]);
        let mut services = Recorder {
            input: input.clone(),
            ..Default::default()
        };

        initialize_player(
            &mut lifecycle,
            &mut reset,
            &mut state,
            PlayerInitInput {
                position: vec3(&input["position"]),
                heading: f32v(&input["heading"]),
                identity_words: [u32v(&input["identity"][0]), u32v(&input["identity"][1])],
            },
            &mut services,
        );

        assert_eq!(
            json!(services.events),
            case["events"],
            "service trace case {index}"
        );
        assert_eq!(
            lifecycle.player_count as u32,
            u32v(&case["player_count"]),
            "+0x210 case {index}"
        );
        assert_eq!(
            lifecycle.session_mode as u32,
            u32v(&case["session_mode"]),
            "+0x40 case {index}"
        );
        assert_eq!(
            reset.player_handles_120,
            [
                u32v(&case["player_handles"][0]),
                u32v(&case["player_handles"][1])
            ],
            "+0x120 case {index}"
        );
        assert_eq!(
            reset.current_ai_entities_128,
            [u32v(&case["ai_handles"][0]), u32v(&case["ai_handles"][1])],
            "+0x128 case {index}"
        );
        assert_eq!(
            state.player_init_flags_130,
            [
                case["flags_130"][0].as_u64().unwrap() as u8,
                case["flags_130"][1].as_u64().unwrap() as u8
            ],
            "+0x130 case {index}"
        );
        let expected_ai_store = if let Some(slot) = case["active_slot"].as_u64() {
            Some((
                u32v(&case["ai_handles"][slot as usize]),
                u32v(&case["ai_ball_handle_60"]),
            ))
        } else {
            None
        };
        assert_eq!(
            services.ai_ball_store, expected_ai_store,
            "AI +0x60 case {index}"
        );
    }
}
