//! Full original-code snapshots for UpdateServe and its nested gameplay graph.
use super::*;
use crate::tetherball_lifecycle::{Services, tests::Recorder};
use crate::tetherball_match::WinnerUi;
use serde_json::{Value, json};

struct Host {
    trace: Recorder,
    input: Value,
}

impl Services for Host {
    fn scoreboard(&mut self, v: [i32; 4]) {
        self.trace.scoreboard(v);
    }
    fn round(&mut self, v: [i32; 3]) {
        self.trace.round(v);
    }
    fn serve_bubble(&mut self, v: [i32; 3]) {
        self.trace.serve_bubble(v);
    }
    fn mega_visible(&mut self, p: i32, v: i32) {
        self.trace.mega_visible(p, v);
    }
    fn mega_value(&mut self, p: i32, v: i32) {
        self.trace.mega_value(p, v);
    }
    fn winner_visible(&mut self, ui: WinnerUi, p: i32, v: bool) {
        self.trace.winner_visible(ui, p, v);
    }
    fn sound(&mut self, f: bool, s: i32, v: i32, volume: i32) {
        self.trace.sound(f, s, v, volume);
    }
    fn controller_pop(&mut self, c: i32) {
        self.trace.controller_pop(c);
    }
    fn controller_set(&mut self, c: i32, s: i32) {
        self.trace.controller_set(c, s);
    }
    fn animation(&mut self, p: usize, s: i32, f: bool, b: i32) {
        self.trace.animation(p, s, f, b);
    }
    fn switch_to_ai(&mut self, p: usize) {
        self.trace.switch_to_ai(p);
    }
    fn camera_offset(&mut self, t: bool, p: [f32; 3], ms: u32) {
        self.trace.camera_offset(t, p, ms);
    }
    fn wrap_particle(&mut self, p: usize, pos: [f32; 3], fade: i32) {
        self.trace.wrap_particle(p, pos, fade);
    }
    fn fade_in(&mut self, v: i32) {
        self.trace.fade_in(v);
    }
    fn random_range(&mut self, l: i32, h: i32) -> i32 {
        self.trace.random_range(l, h)
    }
    fn clear_hud(&mut self) {
        self.trace.clear_hud();
    }
    fn close_screen(&mut self) {
        self.trace.close_screen();
    }
    fn reset_round(&mut self, s: &mut Lifecycle, b: &mut BallMotion) {
        self.trace.reset_round(s, b);
    }
    fn post_game(&mut self, kind: i32, words: &[u32; 70]) {
        self.trace.post_game(kind, words);
    }
}

fn integer(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}
fn word(v: &Value) -> u32 {
    v.as_u64()
        .map(|n| n as u32)
        .unwrap_or_else(|| integer(v) as u32)
}
fn float(v: &Value) -> f32 {
    f32::from_bits(word(v))
}
fn floats<const N: usize>(v: &Value) -> [f32; N] {
    std::array::from_fn(|i| float(&v[i]))
}

impl ServeServices for Host {
    fn azimuth(&mut self, player: usize) -> i32 {
        let value = integer(&self.input["azimuth"][player]);
        self.trace.events.push(json!(["azimuth", player, value]));
        value
    }
    fn event(&mut self, controller: i32, action: i32) -> bool {
        let value = self.input["events"][controller.to_string()][action.to_string()]
            .as_bool()
            .unwrap_or(false);
        self.trace
            .events
            .push(json!(["event", controller, action, value]));
        value
    }
    fn timer_visible(&mut self, v: i32) {
        self.trace.events.push(json!(["timer_visible", v]));
    }
    fn wiimote_sound(&mut self, p: usize, s: i32, v: i32) {
        self.trace.events.push(json!(["sound_wiimote", p, s, v]));
    }
    fn camera_shake(&mut self, ms: i32, strength: f32) {
        self.trace
            .events
            .push(json!(["shake", ms, strength.to_bits()]));
    }
    fn rumble(&mut self, c: i32, ms: u32, strength: f32) {
        self.trace
            .events
            .push(json!(["rumble", c, ms, strength.to_bits()]));
    }
    fn serve_particle(&mut self, name: &str, pos: [f32; 3], fade: i32) {
        self.trace
            .events
            .push(json!(["particle_create", name, pos.map(f32::to_bits)]));
        self.trace
            .events
            .push(json!(["particle_destroy", 0xabcdef, fade]));
    }
    fn pregame(&mut self, kind: i32, args: i32, words: [u32; 4]) {
        self.trace
            .events
            .push(json!(["pregame", kind, args, words]));
    }
    fn overlay(&mut self, name: &str) {
        self.trace.events.push(json!(["overlay", name]));
    }
    fn audio_pause(&mut self, mode: i32) {
        self.trace.events.push(json!(["audio_pause", mode]));
    }
}

#[test]
fn original_complete_serve_graph() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/tetherball_serve_golden.json")).unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    assert_eq!(fixture["instruction_count"], 509);
    assert_eq!(fixture["branch_outcomes"].as_array().unwrap().len(), 90);
    for c in fixture["selectors"].as_array().unwrap() {
        let mut server = integer(&c["initial"][0]);
        let mut receiver = integer(&c["initial"][1]);
        set_active_character_fields(&mut server, &mut receiver, integer(&c["selected"]));
        assert_eq!(json!([server, receiver]), c["expected"], "raw selector {c}");
    }
    for (index, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
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
        let reset_before = reset.clone();
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
        let input = ServeInputs {
            milliseconds: integer(&c["milliseconds"]),
            ball_position: floats(&a["ball_position"]),
            fx_names: std::array::from_fn(|p| {
                std::array::from_fn(|power| format!("serve_fx_{power}_{p}"))
            }),
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
        let returned = update_serve(
            &mut life,
            &mut reset,
            &mut gestures,
            &mut ball,
            &mut aux,
            rules,
            &input,
            &mut host,
        );
        assert_eq!(returned, c["returned"] == 1, "{label} return");
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
        let expected = &c["expected_aux"];
        let mut expected_reset = reset_before;
        expected_reset.word_224 = integer(&expected["word_224"]);
        expected_reset.counter_260 = word(&expected["counter_260"]);
        expected_reset.counter_264 = word(&expected["counter_264"]);
        expected_reset.counter_270 = word(&expected["counter_270"]);
        expected_reset.field_25c = expected["field_25c"].as_bool().unwrap();
        assert_eq!(reset, expected_reset, "{label} complete shared reset state");
        assert_eq!(
            host.trace.events,
            c["effects"].as_array().unwrap().clone(),
            "{label} ordered effects"
        );
    }
}
