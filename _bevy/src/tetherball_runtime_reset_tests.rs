use super::*;
use crate::tetherball_hit_animation::HitAnimations;
use crate::tetherball_hit_animation::ReadyAnimation;
use crate::tetherball_lifecycle::tests::Recorder;
use crate::tetherball_serve::ServeState;
use crate::tetherball_serve::tests::Host;
use serde_json::{Value, json};
fn pair(v: &Value, k: &str) -> [i32; 2] {
    std::array::from_fn(|i| v[k][i].as_i64().unwrap() as i32)
}
fn flags(v: &Value, k: &str) -> [bool; 2] {
    std::array::from_fn(|i| v[k][i].as_bool().unwrap())
}
fn ready(w: &Value, a: &str, b: &str) -> ReadyAnimation {
    ReadyAnimation {
        state: pair(w, a),
        suppress_if_current: pair(w, b),
    }
}
#[test]
fn original_runtime_reset_graph() {
    let f: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_runtime_reset_golden.json"
    ))
    .unwrap();
    assert_eq!(f["elf_sha256"], crate::recovered::ELF_SHA256);
    assert_eq!(f["cases"].as_array().unwrap().len(), 24);
    for operation in ["round", "minigame"] {
        assert_eq!(
            f["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["operation"] == operation)
                .count(),
            12
        );
    }
    for (i, c) in f["cases"].as_array().unwrap().iter().enumerate() {
        let (life, mut reset, ball, _) = crate::tetherball_reset::tests::seed(c, &f);
        let v = &c["initial_serve"];
        let serve = ServeState {
            pause_block_count_0fc: v["pause_block_count_0fc"].as_i64().unwrap() as i32,
            pause_menu_open: v["pause_menu_open"].as_bool().unwrap(),
            power_serve_enabled: v["power_serve_enabled"].as_bool().unwrap(),
            return_angles: std::array::from_fn(|i| {
                f32::from_bits(v["return_angles"][i].as_u64().unwrap() as u32)
            }),
            power_animations: pair(v, "power_animations"),
            high_animations: pair(v, "high_animations"),
            voice_types: pair(v, "voice_types"),
            ai_waiting: flags(v, "ai_waiting"),
            forced_ai: flags(v, "forced_ai"),
            frontend_flags: flags(v, "frontend_flags"),
        };
        let w = &c["words"];
        let animations = HitAnimations {
            ready_power: ready(w, "0x1a0", "0x1a8"),
            ready_reverse: ready(w, "0x1d0", "0x1d8"),
            ready_zone_zero: ready(w, "0x1b8", "0x1c0"),
            ready_zone_one: ready(w, "0x1e8", "0x1f0"),
            hit_power: pair(w, "0x198"),
            hit_zone_zero: pair(w, "0x1b0"),
        };

        let rules = crate::tetherball_match::MatchRules {
            mode: 0,
            rotation_limit: reset.rotation_limit_430,
            wins_required: 2,
            time_limit_seconds: 30000,
        };
        reset.ball_fx_140 = [123; 3];
        reset.pending_count_2e4 = 99;
        reset.field_229 = !c["aux"]["field_229"].as_bool().unwrap();
        reset.rotation_limit_430 = 987;
        let gestures = crate::tetherball_gestures::GestureState {
            pending: c["live_queue"]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| crate::tetherball_gestures::GestureRecord {
                    kind: q["kind"].as_i64().unwrap() as i32,
                    auxiliary: f32::from_bits(q["auxiliary"].as_u64().unwrap() as u32),
                    controller_id: q["controller_id"].as_i64().unwrap() as i32,
                })
                .collect(),
            hit_attempt_marker: c["aux"]["field_229"].as_bool().unwrap(),
        };
        let mut runtime = Runtime {
            life,
            ball,
            state: RuntimeState {
                reset,
                serve,
                gestures,
                animations,
                rules,
                rally: crate::tetherball_rally_rules::RallyRuleState {
                    ai_hit_attempt_234: 17,
                    ai_power_hit_type_238: 23,
                    ai_charge: [41, 42],
                },
                hit: crate::tetherball_hit::HitState {
                    field_32c: false,
                    field_32d: false,
                    mega_ability_42e: false,
                    pending_zone_274: 0,
                    power_hit_type_43c: 0,
                    hit_multiplier_428: 1.,
                    hit_multiplier_enabled_42d: false,
                    indicator_scale_338: 0.,
                    indicator_rate_33c: 0.,
                    indicator_angles_360_36c_378_384: [[0.; 3]; 4],
                },
                scene: crate::tetherball_scene::BallScene {
                    anchor: [0.; 3],
                    position: [0.; 3],
                    ball_matrix: crate::area_transform::IDENTITY,
                    rope_matrix: crate::area_transform::IDENTITY,
                    trails: std::array::from_fn(|i| c["live_trails"][i].as_u64().unwrap() as u32),
                    null_trail: u32::MAX,
                    ball_shadow: false,
                    rope_shadow: false,
                },
                ai: [None, None],
                frontend: crate::tetherball_frontend::FrontendState {
                    pregame_ready_058: 2,
                    postgame_choice_05c: -1,
                    field_424: 1,
                },
                fx_names: std::array::from_fn(|_| std::array::from_fn(|_| String::new())),
            },
        };
        let mut reset_case = c.clone();
        reset_case["inputs"]["initialized_animations"] = json!([-1234567, 7654321]);
        let mut host = Host {
            trace: Recorder {
                events: vec![],
                randoms: vec![],
                next: 0,
            },
            input: json!({"reset_case":reset_case,"reset_fixture":f,"reset_aux":c["aux"]}),
        };
        let path = crate::bridge::data_root().join("files/data/db");
        let db = crate::vlt::Database::load(
            &std::fs::read(path.join("db.vlt")).unwrap(),
            &std::fs::read(path.join("db.bin")).unwrap(),
            crate::vlt::known_names(),
        )
        .unwrap();
        runtime
            .reset(c["operation"] == "minigame", &db, &mut host)
            .unwrap();
        assert_eq!(
            runtime.state.gestures.pending.len(),
            c["live_queue"].as_array().unwrap().len()
        );
        for q in &runtime.state.gestures.pending {
            assert_eq!((q.kind, q.auxiliary.to_bits(), q.controller_id), (2, 0, 0));
        }
        assert_eq!(
            runtime.state.gestures.hit_attempt_marker,
            c["aux"]["field_229"].as_bool().unwrap()
        );
        assert_eq!(
            (
                runtime.state.rally.ai_hit_attempt_234,
                runtime.state.rally.ai_power_hit_type_238
            ),
            (17, 23)
        );
        let mut actual_aux = host.input["reset_aux"].clone();
        for p in 0..2 {
            let ai = runtime.state.ai[p].as_ref().unwrap();
            let want = &c["ai"][p];
            assert_eq!(
                json!({"ball":ai.ball_handle,"angle":ai.angle.to_bits(),"scale":ai.direction_scale.to_bits(),"charge":runtime.state.rally.ai_charge[p],"difficulty":ai.difficulty,"enabled":ai.enabled,"heading":ai.heading.to_bits(),"waiting":runtime.state.serve.ai_waiting[p]}),
                *want
            );
            actual_aux["ai_entities"][p] = json!({"handle":ai.handle,"ball":ai.ball_handle,"angle":ai.angle.to_bits(),"value":ai.direction_scale.to_bits(),"distance":runtime.life.players[p].ai_distance});
        }
        assert_eq!(runtime.state.scene.trails, runtime.state.reset.ball_fx_140);
        let Runtime { life, ball, state } = runtime;
        let reset = state.reset;
        let serve = state.serve;
        let animations = state.animations;
        let label = c["label"].as_str().unwrap();
        assert_eq!(
            serde_json::to_value(&life).unwrap(),
            c["expected"],
            "{label}: lifecycle"
        );
        crate::tetherball::tests::assert_bits(
            &ball,
            &crate::tetherball::tests::ball(&c["expected_ball"]),
            i,
            0,
        );
        let aux = actual_aux;
        assert_eq!(
            json!(host.trace.events),
            c["effects"],
            "{label}: ordered engine effects"
        );
        assert_eq!(
            crate::tetherball_reset::tests::snapshot(&reset, aux),
            c["expected_aux"],
            "{label}: full auxiliary snapshot"
        );
        assert_eq!(
            json!({"pause_block_count_0fc":serve.pause_block_count_0fc,"pause_menu_open":serve.pause_menu_open,
                "power_serve_enabled":serve.power_serve_enabled,"return_angles":serve.return_angles.map(f32::to_bits),
                "power_animations":serve.power_animations,"high_animations":serve.high_animations,"voice_types":serve.voice_types,
                "ai_waiting":serve.ai_waiting,"forced_ai":serve.forced_ai,"frontend_flags":serve.frontend_flags}),
            c["expected_serve"],
            "{label}: supplied serve state"
        );
        assert_eq!(
            json!({"0x198":animations.hit_power,"0x1a0":animations.ready_power.state,"0x1a8":animations.ready_power.suppress_if_current,
                "0x1b0":animations.hit_zone_zero,"0x1b8":animations.ready_zone_zero.state,"0x1c0":animations.ready_zone_zero.suppress_if_current,
                "0x1d0":animations.ready_reverse.state,"0x1d8":animations.ready_reverse.suppress_if_current,
                "0x1e8":animations.ready_zone_one.state,"0x1f0":animations.ready_zone_one.suppress_if_current}),
            c["expected_words"],
            "{label}: hit tables"
        );
    }
}
