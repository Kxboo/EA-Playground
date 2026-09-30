//! Original MGTetherball state entry, distance/camera decisions and win-animation gates.
//! UI, controller, animation, random-number and effect objects are explicit services.
use crate::tetherball::{BallMotion, Zone};
use crate::tetherball_match::{MatchEffect, MatchRules, MatchState, WinnerUi};
use serde::{Deserialize, Serialize};

mod float_bits {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(v.to_bits())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        Ok(f32::from_bits(u32::deserialize(d)?))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};
    pub(crate) struct Recorder {
        pub(crate) events: Vec<Value>,
        pub(crate) randoms: Vec<i32>,
        pub(crate) next: usize,
    }
    impl IntroServices for Recorder {
        fn reset_scoreboard(&mut self) {
            self.events.push(json!(["reset_scoreboard"]));
        }
    }
    impl FrameServices for Recorder {
        fn process_gestures(&mut self, _state: &mut Lifecycle, _ball: &mut BallMotion) {
            self.events.push(json!(["gestures"]));
        }
        fn update_ball(&mut self, _state: &mut Lifecycle, _ball: &mut BallMotion, ms: i32) {
            self.events.push(json!(["ball_update", ms]));
        }
        fn update_handler(
            &mut self,
            state: &mut Lifecycle,
            _ball: &mut BallMotion,
            code: u32,
            ms: i32,
        ) -> u32 {
            self.events.push(json!(["handler", code, ms]));
            // Explicit callback-boundary return value, not a model of the handler.
            state.paused = code == 9;
            if code == 9 { 2 } else { code % 2 }
        }
        fn pole_indicator(&mut self, offset: f32) {
            self.events.push(json!(["indicator", offset.to_bits()]));
        }
        fn base_update(&mut self, ms: i32) {
            self.events.push(json!(["base_update", ms]));
        }
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
    #[test]
    fn original_complete_entry_and_animation_gates() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_lifecycle_golden.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["elf_sha256"].as_str(),
            Some(crate::recovered::ELF_SHA256)
        );
        for (i, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut state: Lifecycle = serde_json::from_value(c["initial"].clone()).unwrap();
            let rules: MatchRules = serde_json::from_value(c["rules"].clone()).unwrap();
            let mut ball = crate::tetherball::tests::ball(&c["ball"]);
            let mut recorder = Recorder {
                events: vec![],
                randoms: serde_json::from_value(c["randoms"].clone()).unwrap(),
                next: 0,
            };
            let arg = c["command"][1].as_i64().unwrap();
            let mut intro: IntroState = serde_json::from_value(c["intro"].clone()).unwrap();
            let result = match c["command"][0].as_str().unwrap() {
                "intro" => Some(u32::from(state.update_intro(
                    &mut intro,
                    rules,
                    &mut ball,
                    &mut recorder,
                ))),
                "change" => {
                    state.change_state(arg as u32, rules, &mut ball, &mut recorder);
                    None
                }
                "resetting" => Some(u32::from(state.update_resetting(
                    rules,
                    &mut ball,
                    &mut recorder,
                ))),
                "win_animations" => {
                    state.initialize_win_animations(&mut recorder);
                    None
                }
                "celebrations" => Some(u32::from(state.celebrations_finished())),
                "distance" => {
                    state.update_player_distance(&mut ball, &mut recorder);
                    None
                }
                "camera" => {
                    state.adjust_camera_height(arg as i32, &mut recorder);
                    None
                }
                "round_end" => Some(u32::from(state.update_round_end(
                    rules,
                    &mut ball,
                    &mut recorder,
                ))),
                "update" => state.update(arg as i32, &mut ball, &mut recorder),
                other => panic!("unknown command {other}"),
            };
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                c["expected"],
                "state case {i}"
            );
            crate::tetherball::tests::assert_bits(
                &ball,
                &crate::tetherball::tests::ball(&c["expected_ball"]),
                i,
                0,
            );
            assert_eq!(json!(recorder.events), c["effects"], "effects case {i}");
            assert_eq!(
                serde_json::to_value(intro).unwrap(),
                c["expected_intro"],
                "intro case {i}"
            );
            if let Some(result) = result {
                assert_eq!(
                    result,
                    c["returned"].as_u64().unwrap() as u32,
                    "return case {i}"
                );
            }
        }
    }
}
mod vector_bits {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[f32; 3], s: S) -> Result<S::Ok, S::Error> {
        v.map(f32::to_bits).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; 3], D::Error> {
        Ok(<[u32; 3]>::deserialize(d)?.map(f32::from_bits))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub controller: Option<i32>,
    pub special_win_animation: i32,
    pub current_animation: i32,
    /// Original base-player record byte +0x84 (stride 0x40).
    pub player_flag: bool,
    #[serde(with = "float_bits")]
    pub facing: f32,
    #[serde(with = "vector_bits")]
    pub direction: [f32; 3],
    #[serde(with = "float_bits")]
    pub movement_speed: f32,
    pub ai_distance: i32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lifecycle {
    pub match_state: MatchState,
    pub paused: bool,
    /// Base +0x40 word: human-player helpers increment it. Existing rule ports
    /// use 1 for single-player and >1 for multiplayer; preserve the raw word.
    pub session_mode: i32,
    pub game_type: i32,
    pub variant: i32,
    /// Tetherball character count at +0x210, distinct from base +0x70.
    pub player_count: usize,
    /// Active pair at +0x214/+0x218; original serve pair is +0x21c/+0x220.
    pub server: usize,
    pub receiver: usize,
    /// Initial server at +0x21c, also used by winner-facing decisions.
    pub focus_player: i32,
    pub players: [Player; 2],
    pub round_number: i32,
    pub total_rounds: i32,
    pub scoreboard_visible: bool,
    pub round_visible: bool,
    pub round_timer_ms: i32,
    pub latches: [bool; 2],
    pub action_states: [i32; 2],
    pub mega_values: [i32; 2],
    pub mega_states: [i32; 2],
    pub mega_enabled: [bool; 2],
    pub field_32f: bool,
    pub serve_bubble_visible: bool,
    pub hud_ready: bool,
    /// attempts, successful hits, power hits, mega hits, accuracy percent.
    pub statistics: [[i32; 5]; 2],
    /// accuracy, power and mega score weights, already difficulty-selected.
    pub score_weights: [i32; 3],
    /// External byte dereferenced through 0x8060204c on a single-player win.
    pub postgame_win_flag: bool,
    pub distance_mode: i32,
    pub current_distance: i32,
    #[serde(with = "vector_bits")]
    pub pole_position: [f32; 3],
    #[serde(with = "float_bits")]
    pub indicator_target: f32,
    #[serde(with = "float_bits")]
    pub indicator_current: f32,
    pub win_animations: [i32; 2],
    pub lose_animations: [i32; 2],
    pub winner_turns: [bool; 2],
    #[serde(with = "float_bits")]
    pub winner_base_angle: f32,
    /// Runtime singleton after original minigame tag check; not inferred from variant.
    pub active_tetherball_variant: Option<i32>,
}

/// Auxiliary fields used by UpdateIntro, outside the shared round state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntroState {
    /// Initial receiver at +0x220, distinct from the active pair at +0x214/+0x218.
    pub initial_receiver: usize,
    /// Opaque +0x25c byte, cleared on every call.
    pub field_25c: bool,
    /// +0x444, set by ResetMiniGame and consumed after HUD readiness.
    pub scoreboard_needs_reset: bool,
}
pub trait IntroServices: Services {
    fn reset_scoreboard(&mut self);
}
pub trait Services {
    fn scoreboard(&mut self, args: [i32; 4]);
    fn round(&mut self, args: [i32; 3]);
    fn serve_bubble(&mut self, args: [i32; 3]);
    fn mega_visible(&mut self, player: i32, visible: i32);
    fn mega_value(&mut self, player: i32, value: i32);
    fn winner_visible(&mut self, ui: WinnerUi, player: i32, visible: bool);
    fn sound(&mut self, frontend: bool, sound: i32, variant: i32, volume: i32);
    fn controller_pop(&mut self, controller: i32);
    fn controller_set(&mut self, controller: i32, state: i32);
    fn animation(&mut self, player: usize, state: i32, force: bool, blend: i32);
    fn switch_to_ai(&mut self, player: usize);
    fn camera_offset(&mut self, target: bool, offset: [f32; 3], ms: u32);
    fn wrap_particle(&mut self, player: usize, position: [f32; 3], fade_ms: i32);
    fn fade_in(&mut self, value: i32);
    fn random_range(&mut self, low: i32, high: i32) -> i32;
    fn clear_hud(&mut self);
    fn close_screen(&mut self);
    /// Full reset is a separately recovered dependency; implementations must
    /// apply it synchronously before the state-3 transition that follows.
    fn reset_round(&mut self, state: &mut Lifecycle, ball: &mut BallMotion);
    /// Exact zero-filled 0x118-byte PostGameInfo payload, in big-endian u32 words.
    fn post_game(&mut self, kind: i32, words: &[u32; 70]);
}
pub const WIN_ANIMATIONS: [i32; 5] = [85, 225, 226, 227, 228];

/// Original Update orchestration. Handler implementations can call the native
/// lifecycle methods; remaining game/AI/animation handlers stay explicit.
pub trait FrameServices: Services {
    fn process_gestures(&mut self, state: &mut Lifecycle, ball: &mut BallMotion);
    fn update_ball(&mut self, state: &mut Lifecycle, ball: &mut BallMotion, ms: i32);
    fn update_handler(
        &mut self,
        state: &mut Lifecycle,
        ball: &mut BallMotion,
        code: u32,
        ms: i32,
    ) -> u32;
    fn pole_indicator(&mut self, offset: f32);
    fn base_update(&mut self, ms: i32);
}

impl Lifecycle {
    /// Invalid/non-dispatched states leave the original return register unset;
    /// return None instead of inventing a success/failure result for that domain.
    pub fn update(
        &mut self,
        ms: i32,
        ball: &mut BallMotion,
        s: &mut impl FrameServices,
    ) -> Option<u32> {
        self.match_state.previous_state_ms = self.match_state.state_ms;
        self.match_state.state_ms = self.match_state.state_ms.wrapping_add(ms as u32);
        if !self.paused {
            s.process_gestures(self, ball);
            s.update_ball(self, ball, ms);
        }
        let code = self.match_state.state_code;
        if !self.paused && matches!(code, 27 | 28 | 29) {
            self.match_state.elapsed_ms = self.match_state.elapsed_ms.wrapping_add(ms as u32);
        }
        let result = if matches!(code, 1 | 3 | 8 | 9 | 26 | 27 | 28 | 29 | 30) {
            Some(s.update_handler(self, ball, code, ms))
        } else {
            None
        };
        if !self.paused {
            if self.indicator_target < self.indicator_current {
                self.indicator_current -= 0.01;
                if self.indicator_current <= self.indicator_target {
                    self.indicator_current = self.indicator_target;
                }
            } else if self.indicator_target > self.indicator_current {
                self.indicator_current += 0.01;
                if self.indicator_current >= self.indicator_target {
                    self.indicator_current = self.indicator_target;
                }
            }
            s.pole_indicator(self.indicator_current);
            s.base_update(ms);
        }
        result
    }
    /// Complete ChangeGameState dispatch. Original accepted all u32 state codes;
    /// only 9 and 26..30 have entry handlers. Nested winner transitions run inline.
    pub fn change_state(
        &mut self,
        code: u32,
        rules: MatchRules,
        ball: &mut BallMotion,
        s: &mut impl Services,
    ) {
        assert!(self.player_count <= 2 && self.server < 2 && self.receiver < 2);
        self.match_state.state_code = code;
        self.match_state.state_ms = 0;
        self.match_state.previous_state_ms = 0;
        match code {
            9 => s.fade_in(-1),
            26 => self.latches = [false; 2],
            27 => {
                if self.scoreboard_visible {
                    s.scoreboard([0; 4]);
                    self.scoreboard_visible = false;
                }
                s.round([self.round_number, self.total_rounds, 1]);
                self.round_visible = true;
                self.round_timer_ms = 2000;
                s.sound(true, 0x26, 0, 100);
                if let Some(controller) = self.players[self.server].controller {
                    s.controller_pop(controller);
                    s.controller_set(controller, 17);
                }
                self.action_states = [2; 2];
                s.animation(self.server, 58, false, -1);
                s.serve_bubble([1, -1, -1]);
                s.mega_visible(0, 1);
                s.mega_visible(1, 1);
                s.mega_value(0, self.mega_values[0]);
                s.mega_value(1, self.mega_values[1]);
                s.sound(true, 0x1c, 0, 100);
                self.serve_bubble_visible = true;
            }
            28 => {
                if self.scoreboard_visible {
                    s.scoreboard([0; 4]);
                    self.scoreboard_visible = false;
                }
                if self.round_visible {
                    s.round([self.round_number, self.total_rounds, 0]);
                    self.round_visible = false;
                }
                self.update_player_distance(ball, s);
                self.check(rules, ball, s);
                self.latches = [false; 2];
            }
            29 => {
                self.latches = [false; 2];
                self.match_state.rotations[self.server] =
                    self.match_state.rotations[self.server].wrapping_add(1);
                self.match_state.rotations[self.receiver] =
                    self.match_state.rotations[self.receiver].wrapping_sub(1);
                let mut position = self.pole_position;
                position[1] += if self.variant == 2 { 1.07 } else { 0.9 };
                position[1] +=
                    (0.4 * (self.match_state.rotations[0] as f32)) / (rules.rotation_limit as f32);
                s.wrap_particle(self.server, position, 2000);
                self.indicator_target =
                    (self.match_state.rotations[0] as f32) / (rules.rotation_limit as f32);
                self.update_player_distance(ball, s);
                s.sound(false, 0x17, 0, 100);
                self.check(rules, ball, s);
            }
            30 => {
                if matches!(self.game_type, 6 | 7 | 8) && self.match_state.round_winner != 1 {
                    ball.set_desired_radius(if self.variant == 2 { 0.4 } else { 0.25 });
                    ball.set_angular_velocity(if ball.angular_velocity > 0. {
                        2.5
                    } else {
                        -2.5
                    });
                    ball.set_zone(Zone::Zero);
                } else {
                    ball.spin_up_pole(self.active_tetherball_variant);
                }
                s.camera_offset(true, [0.; 3], 600);
                self.initialize_win_animations(s);
                let winner = if self.match_state.round_winner == 0 {
                    0
                } else {
                    1
                };
                let loser = 1 - winner;
                s.scoreboard([
                    1,
                    1,
                    winner as i32,
                    self.match_state.round_wins[winner] as i32,
                ]);
                self.scoreboard_visible = true;
                s.animation(winner, self.win_animations[winner], false, -1);
                s.animation(loser, self.lose_animations[loser], false, -1);
                s.sound(true, 0x25, 0, 100);
                self.winner_base_angle = self.players[winner].facing;
            }
            _ => {}
        }
    }
    pub fn check(&mut self, rules: MatchRules, ball: &mut BallMotion, s: &mut impl Services) {
        for effect in self.match_state.check(rules, self.session_mode) {
            match effect {
                MatchEffect::WinnerVisible {
                    ui,
                    player,
                    visible,
                } => s.winner_visible(ui, player, visible),
                MatchEffect::ChangeState(code) => self.change_state(code, rules, ball, s),
            }
        }
    }
    pub fn update_resetting(
        &mut self,
        rules: MatchRules,
        ball: &mut BallMotion,
        s: &mut impl Services,
    ) -> bool {
        if self.round_number != 1 || self.match_state.state_ms > 1000 {
            self.change_state(27, rules, ball, s);
            self.mega_values = [0; 2];
            self.mega_states = [1; 2];
            self.mega_enabled = [true; 2];
            self.field_32f = false;
        }
        true
    }
    /// UpdateIntro at 0x803979e4. State time is compared unsigned, strictly >1000.
    pub fn update_intro(
        &mut self,
        intro: &mut IntroState,
        rules: MatchRules,
        ball: &mut BallMotion,
        s: &mut impl IntroServices,
    ) -> bool {
        intro.field_25c = false;
        let server = usize::try_from(self.focus_player).expect("valid initial server");
        assert!(server < 2 && intro.initial_receiver < 2);
        if self.players[server].current_animation == 0 {
            s.animation(server, 58, false, -1);
        }
        let receiver = intro.initial_receiver;
        s.animation(receiver, self.lose_animations[receiver], false, -1);
        if self.match_state.state_ms > 1000 && self.hud_ready {
            if intro.scoreboard_needs_reset {
                s.reset_scoreboard();
                intro.scoreboard_needs_reset = false;
            }
            if !self.scoreboard_visible {
                s.scoreboard([1, 0, 0, 0]);
                self.scoreboard_visible = true;
                s.sound(true, 0x25, 0, 100);
            }
            self.change_state(26, rules, ball, s);
        }
        true
    }
    pub fn update_player_distance(&mut self, ball: &mut BallMotion, s: &mut impl Services) {
        let distance = match self.distance_mode {
            0 => match (self.match_state.rotations[self.server] as i32).abs() {
                0..=1 => 2,
                2..=3 => 1,
                4..=5 => 0,
                _ => return,
            },
            1 => 0,
            2 => 1,
            3 => 2,
            _ => return,
        };
        self.set_player_distance(distance, ball, s);
    }
    pub fn set_player_distance(
        &mut self,
        distance: i32,
        ball: &mut BallMotion,
        s: &mut impl Services,
    ) {
        if self.current_distance == distance {
            return;
        }
        assert!((0..=2).contains(&distance));
        for p in &mut self.players {
            p.movement_speed = 1.2;
            p.ai_distance = distance;
        }
        s.switch_to_ai(0);
        s.switch_to_ai(1);
        ball.set_desired_radius([0.7, 0.9, 1.1][distance as usize]);
        self.current_distance = distance;
    }
    pub fn initialize_win_animations(&mut self, s: &mut impl Services) {
        assert!(self.player_count <= 2);
        for i in 0..self.player_count {
            if !self.match_state.match_over {
                self.win_animations[i] = 225;
                self.winner_turns[i] = self.focus_player == i as i32;
            } else if self.players[i].player_flag {
                self.win_animations[i] = 95;
                self.winner_turns[i] = self.focus_player == i as i32;
            } else {
                let special = self.players[i].special_win_animation;
                self.win_animations[i] = if special != -1 && s.random_range(0, 99) < 50 {
                    special
                } else {
                    WIN_ANIMATIONS[s.random_range(0, 4) as usize]
                };
                self.winner_turns[i] =
                    self.focus_player == i as i32 && self.win_animations[i] != 85;
            }
        }
    }
    pub fn celebrations_finished(&self) -> bool {
        self.players[..self.player_count].iter().all(|p| {
            !WIN_ANIMATIONS.contains(&p.current_animation)
                && p.current_animation != p.special_win_animation
        })
    }
    /// Both offsets transition over 600 ms. Invalid zones perform no camera calls.
    pub fn adjust_camera_height(&self, zone: i32, s: &mut impl Services) {
        if zone != 0 && zone != 1 {
            return;
        }
        assert!((0..=2).contains(&self.current_distance));
        let d = self.current_distance as usize;
        let mut y = [0.8, 0.9, 1.075][d];
        if zone == 1 {
            y += [-0.3, -0.3, -0.4][d];
        }
        s.camera_offset(true, [0., if zone == 1 { 0.6 } else { 0. }, 0.], 600);
        s.camera_offset(false, [0., y, 0.], 600);
    }
    pub fn update_round_end(
        &mut self,
        rules: MatchRules,
        ball: &mut BallMotion,
        s: &mut impl Services,
    ) -> bool {
        self.adjust_camera_height(0, s);
        let winner =
            usize::try_from(self.match_state.round_winner).expect("valid original round winner");
        assert!(winner < 2);
        if self.winner_turns[winner] {
            let ms = self.match_state.state_ms;
            let offset = if ms < 250 {
                -2.2 * (ms as f32 / 250.)
            } else if ms <= 4750 {
                -2.2
            } else if ms < 5000 {
                -2.2 * ((5000 - ms) as f32 / 250.)
            } else {
                0.
            };
            let angle = crate::tetherball::wrap_angle(offset + self.winner_base_angle);
            self.players[winner].facing = angle;
            let (sin, cos) = crate::character_input::ea_sin_cos(angle);
            self.players[winner].direction = [sin, 0., cos];
        }
        if self.match_state.state_ms <= 5000 || !self.celebrations_finished() {
            return true;
        }
        let ui = if self.session_mode == 1 {
            WinnerUi::WinLose
        } else {
            WinnerUi::MultiplayerWin
        };
        if !self.match_state.match_over {
            ball.set_zone(Zone::Zero);
            self.round_number = self.round_number.wrapping_add(1);
            s.reset_round(self, ball);
            self.change_state(3, rules, ball, s);
            s.winner_visible(ui, 0, false);
            return true;
        }
        s.winner_visible(ui, 0, false);
        self.hud_ready = false;
        s.clear_hud();
        s.close_screen();
        self.change_state(8, rules, ball, s);
        let mut info = [0_u32; 70];
        info[0] = self.session_mode as u32;
        info[1] = self.player_count as u32;
        info[0x3c / 4] = 4;
        info[0x54 / 4] = 3;
        for i in 0..self.player_count {
            let stats = &mut self.statistics[i];
            assert!(
                stats[0] != 0,
                "original accuracy division requires nonzero attempts"
            );
            stats[4] = stats[1].wrapping_mul(100).wrapping_div(stats[0]);
            let offset = (0x58 + i * 0x28) / 4;
            info[offset] = stats[4] as u32;
            info[offset + 1] = stats[2] as u32;
            info[offset + 2] = stats[3] as u32;
            info[0xf8 / 4 + i] = crate::tetherball::calc_score(
                crate::tetherball::ScoreStats {
                    power_hits: stats[2],
                    mega_hits: stats[3],
                    accuracy_percent: stats[4],
                },
                crate::tetherball::ScoreWeights {
                    accuracy_points: self.score_weights[0],
                    power_hit_points: self.score_weights[1],
                    mega_hit_points: self.score_weights[2],
                },
            ) as u32;
        }
        let match_winner =
            usize::try_from(self.match_state.match_winner).expect("valid original match winner");
        assert!(match_winner < 2);
        info[0x40 / 4 + match_winner] = 0;
        info[0x40 / 4 + (1 - match_winner)] = 1;
        if self.session_mode > 1 {
            info[0x3c / 4] = match_winner as u32;
            info[0x50 / 4] = 0x01000000;
        } else {
            info[0x108 / 4] = self.match_state.round_wins[0] as i32 as u32;
            info[0x10c / 4] = self.match_state.round_wins[1] as i32 as u32;
            if self.match_state.final_result == 1 {
                info[0x3c / 4] = 0;
                info[0x50 / 4] = u32::from(self.postgame_win_flag) << 24;
            } else {
                info[0x3c / 4] = 4;
                info[0x50 / 4] = 0x01000000;
            }
        }
        s.post_game(2, &info);
        true
    }
}
