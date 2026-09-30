//! Full snapshots from original ResetRound/ResetMiniGame/SetUpServer calls.
use super::*;
use serde_json::{Value, json};

fn word(v: &Value) -> u32 {
    v.as_u64()
        .map(|x| x as u32)
        .unwrap_or_else(|| v.as_i64().unwrap() as u32)
}
fn integer(v: &Value) -> i32 {
    word(v) as i32
}
fn float(v: &Value) -> f32 {
    f32::from_bits(word(v))
}
fn floats<const N: usize>(v: &Value) -> [f32; N] {
    std::array::from_fn(|i| float(&v[i]))
}
fn words<const N: usize>(v: &Value) -> [u32; N] {
    std::array::from_fn(|i| word(&v[i]))
}

/// Engine calls are compared separately from explicit modeled engine-memory
/// writes. Neither is dropped: direct writes update the independently seeded
/// auxiliary snapshot and are asserted against the original memory capture.
pub(crate) fn effect_trace(effects: Vec<ResetEffect>, aux: &mut Value, handles: &Value) -> Vec<Value> {
    let mut trace = Vec::new();
    for e in effects {
        match e {
            ResetEffect::TimerVisible { index, visible } => {
                assert!(!visible);
                trace.push(json!(["timer_visible", index]));
            }
            ResetEffect::ServeBubbleVisible { args } => {
                trace.push(json!(["serve_bubble", args[0], args[1], args[2]]))
            }
            ResetEffect::DestroyPartFx { guid, fade } => {
                trace.push(json!(["particle_destroy", guid, fade]))
            }
            ResetEffect::InitializeHud => trace.push(json!(["initialize_hud"])),
            ResetEffect::PoleTextureMatrix {
                placeable,
                matrix_bits,
            } => trace.push(json!(["pole_matrix", placeable, 0, matrix_bits])),
            ResetEffect::GrabBall {
                ball,
                character,
                matrix,
            } => {
                assert_eq!(ball, word(&handles["ball"]));
                aux["ball_owner_074"] = json!(character);
                aux["ball_matrix_078"] = json!(matrix);
            }
            ResetEffect::SwitchToAi { player } => trace.push(json!(["switch_ai", player])),
            ResetEffect::CameraPositionOffset {
                camera,
                offset_bits,
            } => trace.push(json!(["camera_position", camera, offset_bits])),
            ResetEffect::CameraDesiredPositionOffset {
                camera,
                offset_bits,
                ms,
            } => trace.push(json!(["camera_desired_position", camera, offset_bits, ms])),
            ResetEffect::CameraTargetOffset {
                camera,
                offset_bits,
            } => trace.push(json!(["camera_target", camera, offset_bits])),
            ResetEffect::CameraDesiredTargetOffset {
                camera,
                offset_bits,
                ms,
            } => trace.push(json!(["camera_desired_target", camera, offset_bits, ms])),
            ResetEffect::CameraBackwardsOffset {
                camera,
                offset_bits,
            } => trace.push(json!(["camera_backwards", camera, offset_bits])),
            ResetEffect::CameraDistance {
                camera,
                distance_bits,
                target_height_bits,
            } => {
                assert_eq!(camera, word(&handles["camera"]));
                aux["camera_direct"]["height"] = json!(distance_bits);
                aux["camera_direct"]["zero"] = json!(target_height_bits);
            }
            ResetEffect::CameraDesiredRotation {
                camera,
                angle_bits,
                ms,
            } => trace.push(json!(["camera_rotation", camera, angle_bits, ms])),
            ResetEffect::CameraStartPosition {
                camera,
                position_bits,
            } => trace.push(json!(["camera_start", camera, position_bits])),
            ResetEffect::CameraDirection {
                camera,
                direction_bits,
            } => trace.push(json!(["camera_direction", camera, direction_bits])),
            ResetEffect::CreateAiEntity {
                player,
                character,
                entity,
            } => {
                trace.push(json!(["create_ai", player, character, entity]));
                aux["ai_entities"][player] =
                    json!({"handle":entity,"ball":0,"angle":0,"value":0,"distance":0});
            }
            ResetEffect::BindAiEntity {
                player,
                character,
                entity,
                ball,
            } => {
                assert_eq!(character, word(&handles["players"][player]));
                trace.push(json!(["bind_ai", player, entity]));
                aux["ai_entities"][player]["ball"] = json!(ball);
            }
            ResetEffect::InitializeAi {
                entity,
                enabled,
                difficulty,
                angle_bits,
            } => trace.push(json!([
                "initialize_ai",
                entity,
                enabled,
                difficulty,
                angle_bits
            ])),
            ResetEffect::AiAngle { entity, angle_bits } => {
                let i = aux["ai_entities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|a| word(&a["handle"]) == entity)
                    .unwrap();
                aux["ai_entities"][i]["angle"] = json!(angle_bits);
            }
            ResetEffect::AiStartValue { entity, value_bits } => {
                let i = aux["ai_entities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|a| word(&a["handle"]) == entity)
                    .unwrap();
                aux["ai_entities"][i]["value"] = json!(value_bits);
            }
            ResetEffect::AiDistance { entity, distance } => {
                let i = aux["ai_entities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|a| word(&a["handle"]) == entity)
                    .unwrap();
                aux["ai_entities"][i]["distance"] = json!(distance);
            }
            ResetEffect::CharacterPosition {
                player,
                character,
                position_bits,
            } => {
                assert_eq!(character, word(&handles["players"][player]));
                trace.push(json!(["character_position", player, position_bits]));
            }
            ResetEffect::DatabaseCollection { outer, inner } => {
                trace.push(json!(["collection", outer, inner]))
            }
            ResetEffect::DatabaseArrayCount { name, count } => {
                trace.push(json!(["db_count", name, count]))
            }
            ResetEffect::DifficultyLookup { mode, result } => {
                trace.push(json!(["difficulty", mode, result]))
            }
            ResetEffect::DatabaseUInt { name, index, value } => {
                trace.push(json!(["db_uint", name, index, value]))
            }
            ResetEffect::DatabaseFloat {
                name,
                index,
                value_bits,
            } => trace.push(json!(["db_float", name, index, value_bits])),
            ResetEffect::DestroyDatabaseCollection => trace.push(json!(["destroy_collection"])),
            ResetEffect::RandomRange { low, high, value } => {
                trace.push(json!(["random", low, high, value]))
            }
            ResetEffect::CameraLookup { index, camera } => {
                trace.push(json!(["camera_lookup", index, camera]))
            }
            ResetEffect::CameraTargetPosition {
                camera,
                position_bits,
            } => {
                assert_eq!(camera, word(&handles["camera"]));
                aux["camera_direct"]["target"] = json!(position_bits);
            }
            ResetEffect::InitializePlayerAnimations => trace.push(json!(["initialize_animations"])),
            ResetEffect::AnimationNextState {
                player,
                state,
                force,
                blend,
            } => trace.push(json!(["animation", player, state, force, blend])),
            ResetEffect::AnimationMarkerId { player, index, id } => {
                trace.push(json!(["marker_id", player, index, id]))
            }
            ResetEffect::AnimationMarkerMatrix {
                player,
                index,
                handle,
            } => trace.push(json!(["marker_matrix", player, index, handle])),
            ResetEffect::ScoreboardVisible { args } => {
                trace.push(json!(["scoreboard", args[0], args[1], args[2], args[3]]))
            }
        }
    }
    trace
}

pub(crate) fn seed(c: &Value, f: &Value) -> (Lifecycle, ResetState, BallMotion, ResetInputs) {
    let lifecycle = serde_json::from_value(c["initial"].clone()).unwrap();
    let a = &c["aux"];
    let i = &c["inputs"];
    let h = &f["handles"];
    let mut s = ResetState::blank();
    s.game_mode_044 = integer(&a["game_mode_044"]);
    s.base_player_count_070 = integer(&a["base_player_count_070"]);
    s.initial_rotation_178 = integer(&a["initial_rotation_178"]);
    s.rotation_limit_430 = integer(&a["rotation_limit_430"]);
    s.ai_special_case_0c0 = integer(&a["ai_special_case_0c0"]);
    s.word_224 = integer(&a["word_224"]);
    s.camera_heading_394 = float(&a["camera_heading_394"]);
    s.timer_26c = float(&a["timer_26c"]);
    s.field_35c = float(&a["field_35c"]);
    s.world_position_110 = floats(&a["world_position_110"]);
    s.world_matrix_398 = floats(&a["world_matrix_398"]);
    s.start_angles_248 = floats(&a["start_angles_248"]);
    s.ai_initial_values_27c = std::array::from_fn(|n| integer(&a["ai_initial_values_27c"][n]));
    s.round_tunables_34c_358 = floats(&a["round_tunables_34c_358"]);
    s.field_25c = a["field_25c"].as_bool().unwrap();
    s.field_229 = a["field_229"].as_bool().unwrap();
    s.field_444 = a["field_444"].as_bool().unwrap();
    s.alternate_server_445 = a["alternate_server_445"].as_bool().unwrap();
    s.counter_260 = word(&a["counter_260"]);
    s.counter_264 = word(&a["counter_264"]);
    s.counter_268 = word(&a["counter_268"]);
    s.counter_270 = word(&a["counter_270"]);
    s.pending_count_2e4 = word(&a["pending_count_2e4"]);
    s.field_334_guid = word(&a["field_334_guid"]);
    s.receiver_220 = integer(&a["receiver_220"]);
    s.server_side_flags =
        std::array::from_fn(|n| a["side_flags_244_245_32a_32b"][n].as_bool().unwrap());
    s.slots_284 = std::array::from_fn(|n| RoundSlot {
        kind: word(&a["slots_284"][n][0]),
        value: float(&a["slots_284"][n][1]),
        counter: word(&a["slots_284"][n][2]),
    });
    s.current_ball_owner_074 = word(&a["ball_owner_074"]);
    s.current_ball_matrix_078 = word(&a["ball_matrix_078"]);
    s.game_marker_matrix_278 = word(&a["game_marker_matrix_278"]);
    s.ball_fx_140 = words(&a["ball_fx_140"]);
    s.current_ai_entities_128 = std::array::from_fn(|n| word(&a["ai_entities"][n]["handle"]));
    s.camera_handle_390 = word(&h["camera"]);
    s.pole_handle_184 = word(&h["pole"]);
    s.player_handles_120 = words(&h["players"]);
    s.ball_handle_104 = word(&h["ball"]);
    s.invalid_guid = word(&i["invalid_game_guid"]);
    s.ball_invalid_guid = word(&i["invalid_ball_guid"]);
    let input = ResetInputs {
        round_tuning: std::array::from_fn(|n| RoundTuning {
            base_hit_speed: float(&i["tuning"]["ball_basehitspeed"][n]),
            accelerate_modifier: float(&i["tuning"]["ball_acceleratemodifier"][n]),
            power_modifier: float(&i["tuning"]["ball_powermodifier"][n]),
            mega_modifier: float(&i["tuning"]["ball_megamodifier"][n]),
        }),
        scoring_difficulty: word(&i["scoring_difficulty"]) as usize,
        scoring_array_count: word(&i["scoring_count"]) as usize,
        scoring_weights: std::array::from_fn(|n| ScoreWeights {
            accuracy_points: integer(&i["scoring"]["accuracy_points"][n]),
            power_hit_points: integer(&i["scoring"]["powerhit_points"][n]),
            mega_hit_points: integer(&i["scoring"]["megahit_points"][n]),
        }),
        camera_position_offset: floats(&f["camera_globals"]["position"]),
        camera_target_offset: floats(&f["camera_globals"]["target"]),
        ai_global_enable: i["ai_enabled"].as_bool().unwrap(),
        initialized_animation_states: std::array::from_fn(|n| {
            integer(&i["initialized_animations"][n])
        }),
        created_ai_entities: words(&h["new_ai"]),
        random_server_result: integer(&i["random_server"]),
        camera_lookup_result: word(&h["camera"]),
        markers: std::array::from_fn(|n| {
            i["markers"][n]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| Marker {
                    id: integer(&m["id"]),
                    matrix_handle: word(&m["matrix"]),
                })
                .collect()
        }),
    };
    (
        lifecycle,
        s,
        crate::tetherball::tests::ball(&c["ball"]),
        input,
    )
}

pub(crate) fn snapshot(s: &ResetState, mut aux: Value) -> Value {
    for (n, v) in [
        ("game_mode_044", s.game_mode_044),
        ("base_player_count_070", s.base_player_count_070),
        ("initial_rotation_178", s.initial_rotation_178),
        ("rotation_limit_430", s.rotation_limit_430),
        ("ai_special_case_0c0", s.ai_special_case_0c0),
        ("word_224", s.word_224),
        ("receiver_220", s.receiver_220),
        ("counter_260", s.counter_260 as i32),
        ("counter_264", s.counter_264 as i32),
        ("counter_268", s.counter_268 as i32),
        ("counter_270", s.counter_270 as i32),
        ("pending_count_2e4", s.pending_count_2e4 as i32),
    ] {
        aux[n] = json!(v)
    }
    aux["field_334_guid"] = json!(s.field_334_guid);
    aux["game_marker_matrix_278"] = json!(s.game_marker_matrix_278);
    for (n, v) in [
        ("camera_heading_394", s.camera_heading_394),
        ("timer_26c", s.timer_26c),
        ("field_35c", s.field_35c),
    ] {
        aux[n] = json!(v.to_bits())
    }
    for (n, v) in [
        ("field_25c", s.field_25c),
        ("field_229", s.field_229),
        ("field_444", s.field_444),
        ("alternate_server_445", s.alternate_server_445),
    ] {
        aux[n] = json!(v)
    }
    aux["world_position_110"] = json!(s.world_position_110.map(f32::to_bits));
    aux["world_matrix_398"] = json!(s.world_matrix_398.map(f32::to_bits));
    aux["start_angles_248"] = json!(s.start_angles_248.map(f32::to_bits));
    aux["ai_initial_values_27c"] = json!(s.ai_initial_values_27c);
    aux["round_tunables_34c_358"] = json!(s.round_tunables_34c_358.map(f32::to_bits));
    aux["side_flags_244_245_32a_32b"] = json!(s.server_side_flags);
    aux["slots_284"] = json!(s.slots_284.map(|x| [x.kind, x.value.to_bits(), x.counter]));
    aux["ball_owner_074"] = json!(s.current_ball_owner_074);
    aux["ball_matrix_078"] = json!(s.current_ball_matrix_078);
    aux["ball_fx_140"] = json!(s.ball_fx_140);
    for n in 0..2 {
        aux["ai_entities"][n]["handle"] = json!(s.current_ai_entities_128[n]);
    }
    aux
}

#[test]
fn original_round_minigame_and_server_full_snapshots() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/tetherball_reset_golden.json")).unwrap();
    assert_eq!(
        fixture["elf_sha256"].as_str(),
        Some(crate::recovered::ELF_SHA256)
    );
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 72);
    for (n, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let (mut lifecycle, mut state, mut ball, inputs) = seed(c, &fixture);
        let effects = match c["operation"].as_str().unwrap() {
            "round" => reset_round(&mut lifecycle, &mut state, &mut ball, &inputs),
            "minigame" => reset_minigame(&mut lifecycle, &mut state, &mut ball, &inputs),
            "server" => setup_server(&mut lifecycle, &mut state, &mut ball, &inputs),
            other => panic!("unexpected operation {other}"),
        };
        let mut aux = c["aux"].clone();
        let trace = effect_trace(effects, &mut aux, &fixture["handles"]);
        assert_eq!(
            serde_json::to_value(&lifecycle).unwrap(),
            c["expected"],
            "Lifecycle case {n} {}",
            c["operation"]
        );
        crate::tetherball::tests::assert_bits(
            &ball,
            &crate::tetherball::tests::ball(&c["expected_ball"]),
            n,
            0,
        );
        assert_eq!(
            snapshot(&state, aux),
            c["expected_aux"],
            "auxiliary snapshot case {n} {}",
            c["operation"]
        );
        assert_eq!(
            json!(trace),
            c["effects"],
            "ordered engine trace case {n} {}",
            c["operation"]
        );
    }
}
