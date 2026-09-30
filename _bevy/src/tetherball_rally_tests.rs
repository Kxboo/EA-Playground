//! Full-state and ordered-effect comparison against the original rally graph.
use super::*;
use crate::tetherball_lifecycle::tests::Recorder;
use crate::tetherball_serve::tests::{Host, float, floats, integer, word};
use serde_json::{Value, json};

impl HitServices for Host {
    fn create_part_fx(&mut self, name: &str, position: [f32; 3]) -> u32 {
        self.trace
            .events
            .push(json!(["particle_create", name, position.map(f32::to_bits)]));
        0xabcdef
    }
    fn disable_and_destroy_part_fx(&mut self, guid: u32, delay_ms: i32) {
        self.trace
            .events
            .push(json!(["particle_destroy", guid, delay_ms]));
    }
    fn get_part_fx(&mut self, guid: u32) -> u32 {
        self.trace
            .events
            .push(json!(["particle_lookup", guid, 0x71600000]));
        0x71600000
    }
    fn set_part_fx_position(&mut self, pointer: u32, position: [f32; 3]) {
        self.trace.events.push(json!([
            "particle_position",
            pointer,
            position.map(f32::to_bits)
        ]));
    }
    fn set_part_fx_scale(&mut self, pointer: u32, scale: f32) {
        self.trace
            .events
            .push(json!(["particle_scale", pointer, scale.to_bits()]));
    }
    fn character_position(&mut self, player: usize) -> Option<[f32; 3]> {
        Some(floats(&self.input["positions"][player]))
    }
}

#[test]
fn original_return_graph() {
    compare("return");
}
#[test]
fn original_accelerate_graph() {
    compare("accelerate");
}
#[test]
fn original_hit_graph() {
    compare("hit");
}
#[test]
fn original_multiplier_graph() {
    compare("multiplier");
}
#[test]
fn original_indicator_graph() {
    compare("indicator");
}
fn compare(command_filter: &str) {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/tetherball_rally_golden.json")).unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    for (index, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        if c["command"][0] != command_filter {
            continue;
        }
        let label = c["label"].as_str().unwrap();
        let mut life: Lifecycle = serde_json::from_value(c["initial"].clone()).unwrap();
        let mut ball = crate::tetherball::tests::ball(&c["ball"]);
        let a = &c["aux"];
        let mut reset = crate::tetherball_reset::ResetState::blank();
        reset.base_player_count_070 = integer(&a["base_player_count_070"]);
        reset.word_224 = integer(&a["word_224"]);
        reset.counter_260 = word(&a["counter_260"]);
        reset.counter_264 = word(&a["counter_264"]);
        reset.counter_270 = word(&a["counter_270"]);
        reset.field_25c = a["field_25c"].as_bool().unwrap();
        reset.round_tunables_34c_358 = floats(&a["round_tunables_34c_358"]);
        let mut gestures = crate::tetherball_gestures::GestureState {
            pending: vec![],
            hit_attempt_marker: a["field_229"].as_bool().unwrap(),
        };
        let mut aux = ServeState {
            pause_block_count_0fc: integer(&a["pause_block_count_0fc"]),
            pause_menu_open: a["pause_menu_open"].as_bool().unwrap(),
            power_serve_enabled: a["power_serve_enabled"].as_bool().unwrap(),
            return_angles: floats(&a["return_angles"]),
            power_animations: std::array::from_fn(|i| integer(&a["power_animations"][i])),
            high_animations: std::array::from_fn(|i| integer(&a["high_animations"][i])),
            voice_types: std::array::from_fn(|i| integer(&a["voice_types"][i])),
            ai_waiting: std::array::from_fn(|i| a["ai_waiting"][i].as_bool().unwrap()),
            forced_ai: std::array::from_fn(|i| a["forced_ai"][i].as_bool().unwrap()),
            frontend_flags: std::array::from_fn(|i| word(&a["frontend_flags"][i]) != 0),
        };
        let r = &c["rally"];
        let w = &r["words"];
        let b = &r["bytes"];
        let pair = |offset: u32| {
            std::array::from_fn(|i| integer(&w[format!("{:#x}", offset + i as u32 * 4)]))
        };
        let wf = |offset: u32| float(&w[format!("{offset:#x}")]);
        reset.game_mode_044 = integer(&w["0x44"]);
        reset.start_angles_248 = [wf(0x248), wf(0x24c)];
        reset.ai_initial_values_27c = pair(0x27c);
        reset.counter_268 = word(&w["0x268"]);
        reset.timer_26c = wf(0x26c);
        reset.field_334_guid = word(&w["0x334"]);
        reset.field_35c = wf(0x35c);
        reset.invalid_guid = word(&r["invalid_guid"]);
        let mut rally = RallyRuleState {
            ai_hit_attempt_234: integer(&w["0x234"]),
            ai_power_hit_type_238: integer(&w["0x238"]),
            ai_charge: std::array::from_fn(|i| word(&r["ai_charge"][i])),
        };
        let mut hit = HitState {
            field_32c: b["0x32c"].as_bool().unwrap(),
            field_32d: b["0x32d"].as_bool().unwrap(),
            mega_ability_42e: b["0x42e"].as_bool().unwrap(),
            hit_multiplier_enabled_42d: b["0x42d"].as_bool().unwrap(),
            pending_zone_274: integer(&w["0x274"]),
            power_hit_type_43c: integer(&w["0x43c"]),
            hit_multiplier_428: wf(0x428),
            indicator_scale_338: wf(0x338),
            indicator_rate_33c: wf(0x33c),
            indicator_angles_360_36c_378_384: std::array::from_fn(|i| {
                std::array::from_fn(|j| wf(0x360 + (i * 3 + j) as u32 * 4))
            }),
        };
        use crate::tetherball_hit_animation::ReadyAnimation;
        let ready = |o| ReadyAnimation {
            state: pair(o),
            suppress_if_current: pair(o + 8),
        };
        let animations = HitAnimations {
            ready_power: ready(0x1a0),
            ready_reverse: ready(0x1d0),
            ready_zone_zero: ready(0x1b8),
            ready_zone_one: ready(0x1e8),
            hit_power: pair(0x198),
            hit_zone_zero: pair(0x1b0),
        };
        let input = HitInputs {
            ball_position: floats(&a["ball_position"]),
            fx_names: std::array::from_fn(|p| {
                std::array::from_fn(|power| format!("serve_fx_{power}_{p}"))
            }),
            controller_fx_offset: r.get("controller_fx_offset").map(floats).unwrap_or([0.; 3]),
        };
        let rules = serde_json::from_value(c["rules"].clone()).unwrap();
        let mut host = Host {
            trace: Recorder {
                events: vec![],
                randoms: serde_json::from_value(c["randoms"].clone()).unwrap(),
                next: 0,
            },
            input: c["input"].clone(),
        };
        host.input["positions"] = r["positions"].clone();
        let command = c["command"][0].as_str().unwrap();
        match command {
            "return" | "accelerate" => {
                let result = update_rally(
                    if command == "return" {
                        RallyPhase::Return
                    } else {
                        RallyPhase::Accelerate
                    },
                    &mut life,
                    &mut reset,
                    &mut gestures,
                    &mut ball,
                    &mut aux,
                    &mut rally,
                    &mut hit,
                    &animations,
                    rules,
                    integer(&c["milliseconds"]),
                    &input,
                    &mut host,
                );
                assert_eq!(result, c["returned"] == 1, "{label} return");
            }
            "hit" => hit_tetherball(
                &mut life, &mut reset, &mut rally, &mut ball, &mut hit, &input, &mut host,
            ),
            "multiplier" => increment_multiplier(
                &mut life,
                word(&c["command"][1]) as usize,
                &input,
                &mut host,
            ),
            "indicator" => draw_hit_indicator_particle(
                &life,
                &mut reset,
                &ball,
                &mut hit,
                word(&c["milliseconds"]),
                c["command"][1] == 1,
                &input,
                &mut host,
            ),
            _ => panic!("unknown command"),
        }
        assert_eq!(
            serde_json::to_value(&life).unwrap(),
            c["expected"],
            "{label} lifecycle"
        );
        crate::tetherball::tests::assert_bits(
            &ball,
            &crate::tetherball::tests::ball(&c["expected_ball"]),
            index,
            0,
        );
        let mut actual = a.clone();
        for (name, value) in [
            ("base_player_count_070", json!(reset.base_player_count_070)),
            ("word_224", json!(reset.word_224)),
            ("counter_260", json!(reset.counter_260)),
            ("counter_264", json!(reset.counter_264)),
            ("counter_270", json!(reset.counter_270)),
            ("field_25c", json!(reset.field_25c)),
            ("field_229", json!(gestures.hit_attempt_marker)),
            (
                "round_tunables_34c_358",
                json!(reset.round_tunables_34c_358.map(f32::to_bits)),
            ),
            ("pause_block_count_0fc", json!(aux.pause_block_count_0fc)),
            ("pause_menu_open", json!(aux.pause_menu_open)),
            ("power_serve_enabled", json!(aux.power_serve_enabled)),
            ("return_angles", json!(aux.return_angles.map(f32::to_bits))),
            ("power_animations", json!(aux.power_animations)),
            ("high_animations", json!(aux.high_animations)),
            ("voice_types", json!(aux.voice_types)),
            ("ai_waiting", json!(aux.ai_waiting)),
            ("forced_ai", json!(aux.forced_ai)),
            ("frontend_flags", json!(aux.frontend_flags.map(u8::from))),
        ] {
            actual[name] = value;
        }
        assert_eq!(actual, c["expected_aux"], "{label} auxiliary state");
        let mut actual_r = r.clone();
        for (offset, value) in [
            (0x44, reset.game_mode_044 as u32),
            (0x234, rally.ai_hit_attempt_234 as u32),
            (0x238, rally.ai_power_hit_type_238 as u32),
            (0x248, reset.start_angles_248[0].to_bits()),
            (0x24c, reset.start_angles_248[1].to_bits()),
            (0x268, reset.counter_268),
            (0x26c, reset.timer_26c.to_bits()),
            (0x274, hit.pending_zone_274 as u32),
            (0x27c, reset.ai_initial_values_27c[0] as u32),
            (0x280, reset.ai_initial_values_27c[1] as u32),
            (0x334, reset.field_334_guid),
            (0x338, hit.indicator_scale_338.to_bits()),
            (0x33c, hit.indicator_rate_33c.to_bits()),
            (0x35c, reset.field_35c.to_bits()),
            (0x428, hit.hit_multiplier_428.to_bits()),
            (0x43c, hit.power_hit_type_43c as u32),
        ] {
            actual_r["words"][format!("{offset:#x}")] = json!(value);
        }
        for (offset, value) in [
            (0x32c, hit.field_32c),
            (0x32d, hit.field_32d),
            (0x42d, hit.hit_multiplier_enabled_42d),
            (0x42e, hit.mega_ability_42e),
        ] {
            actual_r["bytes"][format!("{offset:#x}")] = json!(value);
        }
        actual_r["ai_charge"] = json!(rally.ai_charge);
        assert_eq!(actual_r, c["expected_rally"], "{label} rally fields");
        assert_eq!(
            host.trace.events,
            c["effects"].as_array().unwrap().clone(),
            "{label} ordered effects"
        );
    }
}
