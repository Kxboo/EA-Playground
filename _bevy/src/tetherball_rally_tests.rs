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

impl crate::tetherball_scene::SceneServices for Host {
    fn create_trail(&mut self, kind: crate::tetherball_scene::Trail, p: [f32; 3]) -> u32 {
        self.create_part_fx(kind.name(), p)
    }
    fn destroy_trail(&mut self, id: u32, fade: i32) {
        self.disable_and_destroy_part_fx(id, fade);
    }
    fn move_trail(&mut self, id: u32, p: [f32; 3]) {
        let pointer = self.get_part_fx(id);
        self.set_part_fx_position(pointer, p);
    }
    fn shadow_matrix(&mut self, rope: bool, matrix: [f32; 16]) {
        self.trace
            .events
            .push(json!(["shadow", rope, matrix.map(f32::to_bits)]));
    }
}
impl crate::tetherball_lifecycle::IntroServices for Host {
    fn reset_scoreboard(&mut self) {
        self.trace.events.push(json!(["reset_scoreboard"]));
    }
}
impl crate::tetherball_frontend::FrontendServices for Host {
    fn clear_pregame_handlers(&mut self) {
        self.trace.events.push(json!(["clear_pregame"]));
    }
    fn clear_postgame_handlers(&mut self) {
        self.trace.events.push(json!(["clear_postgame"]));
    }
    fn start_fade_out(&mut self, ms: i32) {
        self.trace.events.push(json!(["fade_out", ms]));
    }
    fn fade_out_renders(&mut self, a: bool, b: bool) {
        self.trace
            .events
            .push(json!(["fade_renders", u32::from(a), u32::from(b)]));
    }
    fn setup_minigame_handlers(&mut self, kind: i32) {
        self.trace.events.push(json!(["setup", kind]));
    }
    fn open_apt_screen(&mut self, name: &str) {
        self.trace.events.push(json!(["open_screen", name]));
    }
    fn reset_minigame(&mut self, _: &mut Lifecycle, _: &mut crate::tetherball::BallMotion) {
        panic!("runtime owns reset");
    }
    fn close_apt_overlay(&mut self) {
        self.trace.events.push(json!(["close_overlay"]));
    }
    fn audio_unpause(&mut self) {
        self.trace.events.push(json!(["audio_unpause"]));
    }
    fn minigame_fade_complete(&mut self) -> bool {
        let value = self.input["fade_complete"].as_bool().unwrap();
        self.trace.events.push(json!(["fade_complete", value]));
        value
    }
    fn timer_visible(&mut self, v: i32) {
        crate::tetherball_serve::ServeServices::timer_visible(self, v);
    }
}
impl crate::tetherball_runtime::RuntimeHost for Host {
    fn pole_indicator(&mut self, offset: f32) {
        self.trace
            .events
            .push(json!(["indicator", offset.to_bits()]));
    }
    fn world_update(&mut self, ms: i32) {
        self.trace.events.push(json!(["world_update", ms]));
    }
    fn reset_inputs(
        &mut self,
        _: &Lifecycle,
        _: &ResetState,
        _: bool,
    ) -> crate::tetherball_reset::ResetInputs {
        let c = &self.input["reset_case"];
        let f = &self.input["reset_fixture"];
        crate::tetherball_reset::tests::seed(c, f).3
    }
    fn reset_effect(&mut self, effect: crate::tetherball_reset::ResetEffect) {
        let handles = self.input["reset_fixture"]["handles"].clone();
        let trace = crate::tetherball_reset::tests::effect_trace(
            vec![effect],
            &mut self.input["reset_aux"],
            &handles,
        );
        self.trace.events.extend(trace);
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
    compare_fixture(
        command_filter,
        include_str!("../tests/data/tetherball_rally_golden.json"),
    );
}
#[test]
fn original_complete_runtime_frames() {
    compare_fixture(
        "frame",
        include_str!("../tests/data/tetherball_runtime_golden.json"),
    );
}
fn compare_fixture(command_filter: &str, text: &str) {
    let fixture: Value = serde_json::from_str(text).unwrap();
    let db = if command_filter == "frame" {
        let path = crate::bridge::data_root().join("files/data/db");
        Some(
            crate::vlt::Database::load(
                &std::fs::read(path.join("db.vlt")).unwrap(),
                &std::fs::read(path.join("db.bin")).unwrap(),
                crate::vlt::known_names(),
            )
            .unwrap(),
        )
    } else {
        None
    };
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
        let mut animations = HitAnimations {
            ready_power: ready(0x1a0),
            ready_reverse: ready(0x1d0),
            ready_zone_zero: ready(0x1b8),
            ready_zone_one: ready(0x1e8),
            hit_power: pair(0x198),
            hit_zone_zero: pair(0x1b0),
        };
        let mut input = HitInputs {
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
            "frame" => {
                use crate::area_transform::AreaTransform;
                use crate::tetherball_runtime::{FrameInputs, Runtime, RuntimeState};
                use crate::tetherball_scene::{Attachment, BallScene};
                let scene = &c["scene"];
                gestures.pending = c["queue"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|q| crate::tetherball_gestures::GestureRecord {
                        kind: integer(&q["kind"]),
                        auxiliary: float(&q["auxiliary"]),
                        controller_id: integer(&q["controller_id"]),
                    })
                    .collect();
                reset.receiver_220 = integer(&c["intro"]["initial_receiver"]);
                reset.field_444 = c["intro"]["scoreboard_needs_reset"].as_bool().unwrap();
                let mut runtime = Runtime {
                    life,
                    ball,
                    state: RuntimeState {
                        reset,
                        serve: aux,
                        gestures,
                        rally,
                        hit,
                        animations,
                        rules,
                        scene: BallScene {
                            anchor: floats(&scene["anchor"]),
                            position: floats(&scene["position"]),
                            ball_matrix: floats(&scene["ball_matrix"]),
                            rope_matrix: floats(&scene["rope_matrix"]),
                            trails: std::array::from_fn(|i| word(&scene["trails"][i])),
                            null_trail: u32::MAX,
                            ball_shadow: scene["shadows"][0].as_bool().unwrap(),
                            rope_shadow: scene["shadows"][1].as_bool().unwrap(),
                        },
                        ai: [None, None],
                        frontend: serde_json::from_value(c["front"].clone()).unwrap(),
                        fx_names: input.fx_names.clone(),
                    },
                };
                let frame = FrameInputs {
                    milliseconds: integer(&c["milliseconds"]),
                    world_paused: c["world_paused"].as_bool().unwrap(),
                    area: AreaTransform {
                        radius: float(&scene["area_radius"]),
                        disabled: scene["disabled"].as_bool().unwrap(),
                    },
                    attachment: Some(Attachment {
                        world: floats(&scene["world"]),
                        local: if scene["local"].is_null() {
                            None
                        } else {
                            Some(floats(&scene["local"]))
                        },
                    }),
                    controller_fx_offset: input.controller_fx_offset,
                };
                assert_eq!(
                    runtime
                        .update(&frame, db.as_ref().unwrap(), &mut host)
                        .unwrap(),
                    Some(word(&c["returned"])),
                    "{label} native frame result"
                );
                let Runtime {
                    life: new_life,
                    ball: new_ball,
                    state,
                } = runtime;
                life = new_life;
                ball = new_ball;
                reset = state.reset;
                aux = state.serve;
                gestures = state.gestures;
                rally = state.rally;
                hit = state.hit;
                animations = state.animations;
                input.ball_position = state.scene.position;
                assert_eq!(
                    json!({"position":state.scene.position.map(f32::to_bits),
                    "ball_matrix":state.scene.ball_matrix.map(f32::to_bits),
                    "rope_matrix":state.scene.rope_matrix.map(f32::to_bits), "trails":state.scene.trails}),
                    c["expected_scene"],
                    "{label} complete scene"
                );
                assert_eq!(
                    serde_json::to_value(&state.frontend).unwrap(),
                    c["expected_front"],
                    "{label} frontend"
                );
                assert_eq!(
                    json!({"initial_receiver":reset.receiver_220,"field_25c":reset.field_25c,
                    "scoreboard_needs_reset":reset.field_444}),
                    c["expected_intro"],
                    "{label} intro shared owners"
                );
                assert_eq!(
                    gestures.pending.len(),
                    word(&c["expected_queue_count"]) as usize,
                    "{label} queue"
                );
            }
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
        if command == "frame" {
            actual["ball_position"] = json!(input.ball_position.map(f32::to_bits));
        }
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
        for (offset, values) in [
            (0x198, animations.hit_power),
            (0x1a0, animations.ready_power.state),
            (0x1a8, animations.ready_power.suppress_if_current),
            (0x1b0, animations.hit_zone_zero),
            (0x1b8, animations.ready_zone_zero.state),
            (0x1c0, animations.ready_zone_zero.suppress_if_current),
            (0x1d0, animations.ready_reverse.state),
            (0x1d8, animations.ready_reverse.suppress_if_current),
            (0x1e8, animations.ready_zone_one.state),
            (0x1f0, animations.ready_zone_one.suppress_if_current),
        ] {
            for (player, value) in values.into_iter().enumerate() {
                actual_r["words"][format!("{:#x}", offset + player * 4)] = json!(value as u32);
            }
        }
        assert_eq!(actual_r, c["expected_rally"], "{label} rally fields");
        assert_eq!(
            host.trace.events,
            c["effects"].as_array().unwrap().clone(),
            "{label} ordered effects"
        );
    }
}
