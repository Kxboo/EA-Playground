//! Recovered tetherball frontend handlers; UI and full reset remain synchronous services.
use crate::tetherball::BallMotion;
use crate::tetherball_lifecycle::{Lifecycle, Services};
use crate::tetherball_match::MatchRules;
use crate::tetherball_serve::ServeState;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrontendState {
    pub pregame_ready_058: u8,
    pub postgame_choice_05c: i32,
    pub field_424: u8,
}
pub trait FrontendServices: Services {
    fn clear_pregame_handlers(&mut self);
    fn clear_postgame_handlers(&mut self);
    fn start_fade_out(&mut self, ms: i32);
    fn fade_out_renders(&mut self, first: bool, second: bool);
    fn setup_minigame_handlers(&mut self, kind: i32);
    fn open_apt_screen(&mut self, name: &str);
    fn reset_minigame(&mut self, life: &mut Lifecycle, ball: &mut BallMotion);
    fn close_apt_overlay(&mut self);
    fn audio_unpause(&mut self);
    fn minigame_fade_complete(&mut self) -> bool;
    fn timer_visible(&mut self, visible: i32);
}
pub fn initialize_apt_hud(serve: &mut ServeState, s: &mut impl FrontendServices) {
    serve.frontend_flags = [true, true];
    s.setup_minigame_handlers(2);
    s.open_apt_screen("TetherballHud");
}
pub fn on_game_start_anim_complete(life: &mut Lifecycle) {
    life.field_32f = true;
}
pub fn on_hud_load_complete(life: &mut Lifecycle, s: &mut impl FrontendServices) {
    s.timer_visible(0);
    s.mega_visible(0, 0);
    s.mega_visible(1, 0);
    life.hud_ready = true;
}
pub fn on_pause_reset(
    life: &mut Lifecycle,
    serve: &mut ServeState,
    rules: MatchRules,
    ball: &mut BallMotion,
    s: &mut impl FrontendServices,
) {
    s.reset_minigame(life, ball);
    life.change_state(3, rules, ball, s);
    life.paused = false;
    s.close_apt_overlay();
    s.audio_unpause();
    serve.pause_menu_open = false;
    serve.pause_block_count_0fc = 1200;
}
pub fn update_pregame_instructions(
    life: &mut Lifecycle,
    front: &mut FrontendState,
    serve: &mut ServeState,
    _ms: i32,
    rules: MatchRules,
    ball: &mut BallMotion,
    s: &mut impl FrontendServices,
) -> u32 {
    if front.pregame_ready_058 != 0 {
        s.clear_pregame_handlers();
        s.close_screen();
        s.start_fade_out(400);
        s.fade_out_renders(false, true);
        front.pregame_ready_058 = 0;
        initialize_apt_hud(serve, s);
        life.change_state(3, rules, ball, s);
        front.field_424 = 1;
    }
    1
}
pub fn update_postgame(
    life: &mut Lifecycle,
    front: &mut FrontendState,
    _ms: i32,
    rules: MatchRules,
    ball: &mut BallMotion,
    s: &mut impl FrontendServices,
) -> u32 {
    match front.postgame_choice_05c {
        0 => {
            s.clear_postgame_handlers();
            s.close_screen();
            front.postgame_choice_05c = -1;
            s.reset_minigame(life, ball);
            life.change_state(3, rules, ball, s);
        }
        1 => {
            s.clear_postgame_handlers();
            s.close_screen();
            front.postgame_choice_05c = -1;
            life.change_state(9, rules, ball, s);
        }
        _ => {}
    }
    1
}
pub fn update_wait_for_apocalypse(_ms: i32, s: &mut impl FrontendServices) -> u32 {
    if s.minigame_fade_complete() { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tetherball_match::WinnerUi;
    use serde_json::{Value, json};
    struct Recorder {
        events: Vec<Value>,
        randoms: Vec<i32>,
        next: usize,
        fade: bool,
    }
    impl Services for Recorder {
        fn scoreboard(&mut self, a: [i32; 4]) {
            self.events
                .push(json!(["scoreboard", a[0], a[1], a[2], a[3]]));
        }
        fn round(&mut self, a: [i32; 3]) {
            self.events.push(json!(["round", a[0], a[1], a[2]]));
        }
        fn serve_bubble(&mut self, a: [i32; 3]) {
            self.events.push(json!(["serve_bubble", a[0], a[1], a[2]]));
        }
        fn mega_visible(&mut self, p: i32, v: i32) {
            self.events.push(json!(["mega_visible", p, v]));
        }
        fn mega_value(&mut self, p: i32, v: i32) {
            self.events.push(json!(["mega_value", p, v]));
        }
        fn winner_visible(&mut self, ui: WinnerUi, p: i32, v: bool) {
            self.events.push(json!([
                if ui == WinnerUi::WinLose {
                    "winner_single"
                } else {
                    "winner_multi"
                },
                p,
                i32::from(v)
            ]));
        }
        fn sound(&mut self, front: bool, s: i32, v: i32, volume: i32) {
            self.events.push(json!([
                if front {
                    "sound_frontend"
                } else {
                    "sound_backend"
                },
                s,
                v,
                volume
            ]));
        }
        fn controller_pop(&mut self, c: i32) {
            self.events.push(json!(["controller_pop", c]));
        }
        fn controller_set(&mut self, c: i32, s: i32) {
            self.events.push(json!(["controller_set", c, s]));
        }
        fn animation(&mut self, p: usize, s: i32, f: bool, b: i32) {
            self.events.push(json!(["animation", p, s, f, b]));
        }
        fn switch_to_ai(&mut self, p: usize) {
            self.events.push(json!(["switch_ai", p]));
        }
        fn camera_offset(&mut self, t: bool, p: [f32; 3], ms: u32) {
            self.events
                .push(json!(["camera", t, p.map(f32::to_bits), ms]));
        }
        fn wrap_particle(&mut self, p: usize, pos: [f32; 3], fade: i32) {
            self.events.push(json!([
                "particle_create",
                if p == 0 {
                    "pg_tetherball_point_blue"
                } else {
                    "pg_tetherball_point_red"
                },
                pos.map(f32::to_bits)
            ]));
            self.events
                .push(json!(["particle_destroy", 0xabcdef, fade]));
        }
        fn fade_in(&mut self, v: i32) {
            self.events.push(json!(["fade", v]));
        }
        fn clear_hud(&mut self) {
            self.events.push(json!(["clear_hud"]));
        }
        fn close_screen(&mut self) {
            self.events.push(json!(["close_screen"]));
        }
        fn reset_round(&mut self, _state: &mut Lifecycle, _ball: &mut BallMotion) {
            self.events.push(json!(["reset_round"]));
        }
        fn post_game(&mut self, kind: i32, words: &[u32; 70]) {
            self.events
                .push(json!(["post_game", kind, words.as_slice()]));
        }
        fn random_range(&mut self, low: i32, high: i32) -> i32 {
            let value = self.randoms[self.next] % (high - low + 1) + low;
            self.next += 1;
            self.events.push(json!(["random", low, high, value]));
            value
        }
    }

    impl FrontendServices for Recorder {
        fn clear_pregame_handlers(&mut self) {
            self.events.push(json!(["clear_pregame"]));
        }
        fn clear_postgame_handlers(&mut self) {
            self.events.push(json!(["clear_postgame"]));
        }
        fn start_fade_out(&mut self, ms: i32) {
            self.events.push(json!(["fade_out", ms]));
        }
        fn fade_out_renders(&mut self, a: bool, b: bool) {
            self.events
                .push(json!(["fade_renders", i32::from(a), i32::from(b)]));
        }
        fn setup_minigame_handlers(&mut self, k: i32) {
            self.events.push(json!(["setup", k]));
        }
        fn open_apt_screen(&mut self, n: &str) {
            self.events.push(json!(["open_screen", n]));
        }
        fn reset_minigame(&mut self, _: &mut Lifecycle, _: &mut BallMotion) {
            self.events.push(json!(["reset_minigame"]));
        }
        fn close_apt_overlay(&mut self) {
            self.events.push(json!(["close_overlay"]));
        }
        fn audio_unpause(&mut self) {
            self.events.push(json!(["audio_unpause"]));
        }
        fn minigame_fade_complete(&mut self) -> bool {
            self.events.push(json!(["fade_complete", self.fade]));
            self.fade
        }
        fn timer_visible(&mut self, v: i32) {
            self.events.push(json!(["timer_visible", v]));
        }
    }
    #[test]
    fn original_frontend_handlers() {
        let d: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_frontend_golden.json"
        ))
        .unwrap();
        assert_eq!(
            d["elf_sha256"],
            "5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c"
        );
        for c in d["cases"].as_array().unwrap() {
            let mut life: Lifecycle = serde_json::from_value(c["initial"].clone()).unwrap();
            let mut front: FrontendState = serde_json::from_value(c["front"].clone()).unwrap();
            let mut serve = ServeState {
                pause_block_count_0fc: c["front"]["pause_delay_ms_0fc"].as_i64().unwrap() as i32,
                pause_menu_open: c["front"]["pause_menu_flag_04e"].as_u64().unwrap() != 0,
                frontend_flags: [
                    c["front"]["frontend_flags"][0].as_u64().unwrap() != 0,
                    c["front"]["frontend_flags"][1].as_u64().unwrap() != 0,
                ],
                power_serve_enabled: false,
                return_angles: [0.; 2],
                power_animations: [0; 2],
                high_animations: [0; 2],
                voice_types: [0; 2],
                ai_waiting: [false; 2],
                forced_ai: [false; 2],
            };
            let preserved_serve = serve.clone();
            let rules: MatchRules = serde_json::from_value(c["rules"].clone()).unwrap();
            let mut ball = crate::tetherball::tests::ball(&c["ball"]);
            let mut s = Recorder {
                events: vec![],
                randoms: vec![],
                next: 0,
                fade: c["fade_complete"].as_bool().unwrap(),
            };
            let result = match c["operation"].as_str().unwrap() {
                "pregame" => Some(update_pregame_instructions(
                    &mut life, &mut front, &mut serve, 0, rules, &mut ball, &mut s,
                )),
                "postgame" => Some(update_postgame(
                    &mut life, &mut front, 0, rules, &mut ball, &mut s,
                )),
                "wait" => Some(update_wait_for_apocalypse(0, &mut s)),
                "start_complete" => {
                    on_game_start_anim_complete(&mut life);
                    None
                }
                "hud_complete" => {
                    on_hud_load_complete(&mut life, &mut s);
                    None
                }
                "pause_reset" => {
                    on_pause_reset(&mut life, &mut serve, rules, &mut ball, &mut s);
                    None
                }
                "hud_init" => {
                    initialize_apt_hud(&mut serve, &mut s);
                    None
                }
                _ => panic!(),
            };
            assert_eq!(json!(result), c["result"]);
            assert_eq!(serde_json::to_value(life).unwrap(), c["expected"]);
            let mut expected_front = c["expected_front"].clone();
            for key in [
                "pause_delay_ms_0fc",
                "pause_menu_flag_04e",
                "frontend_flags",
            ] {
                expected_front.as_object_mut().unwrap().remove(key);
            }
            assert_eq!(serde_json::to_value(front).unwrap(), expected_front);
            assert_eq!(
                serve.pause_block_count_0fc as i64,
                c["expected_front"]["pause_delay_ms_0fc"].as_i64().unwrap()
            );
            assert_eq!(
                serve.pause_menu_open,
                c["expected_front"]["pause_menu_flag_04e"].as_u64().unwrap() != 0
            );
            assert_eq!(
                serve.frontend_flags,
                c["expected_front"]["frontend_flags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() != 0)
                    .collect::<Vec<_>>()
                    .as_slice()
            );
            crate::tetherball::tests::assert_bits(
                &ball,
                &crate::tetherball::tests::ball(&c["expected_ball"]),
                0,
                0,
            );
            let mut expected_serve = preserved_serve;
            expected_serve.pause_block_count_0fc = serve.pause_block_count_0fc;
            expected_serve.pause_menu_open = serve.pause_menu_open;
            expected_serve.frontend_flags = serve.frontend_flags;
            assert_eq!(serve, expected_serve);
            assert_eq!(json!(s.events), c["effects"]);
        }
    }
}
