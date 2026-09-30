//! Original rally selectors, charge accounting and third-return ball drop.
use crate::tetherball::{BallMotion, Zone};
use crate::tetherball_gestures::{GestureState, HitAttempt};
use crate::tetherball_lifecycle::{Lifecycle, Services};
use crate::tetherball_reset::ResetState;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RallyRuleState {
    pub ai_hit_attempt_234: i32,
    pub ai_power_hit_type_238: i32,
    /// AI entity +0x70; synchronized even when a charge request fails.
    pub ai_charge: [u32; 2],
}

pub fn get_ai_power_hit_type(state: &RallyRuleState) -> i32 {
    match state.ai_power_hit_type_238 {
        1 => 2,
        3 => 1,
        7 => 3,
        _ => 0,
    }
}
/// The original does not initialize the caller's output references.
pub fn get_ai_hit_attempt(
    _lifecycle: &Lifecycle,
    gestures: &mut GestureState,
    state: &RallyRuleState,
    out: &mut HitAttempt,
) {
    match state.ai_hit_attempt_234 {
        -1 => out.power_type = get_ai_power_hit_type(state),
        0 => {
            out.strike = true;
            out.power_type = 0;
            gestures.hit_attempt_marker = true;
        }
        1 => {
            out.strike = true;
            out.power_type = 2;
            gestures.hit_attempt_marker = true;
        }
        2 => {
            out.reverse_strike = true;
            out.power_type = 0;
            gestures.hit_attempt_marker = true;
        }
        3 => {
            out.reverse_strike = true;
            out.power_type = 1;
            gestures.hit_attempt_marker = true;
        }
        7 => {
            out.strike = true;
            out.reverse_strike = true;
            out.power_type = 3;
            gestures.hit_attempt_marker = true;
        }
        _ => {}
    }
}
/// Table domain is hit types 0..3 and zones 0..1; type 7 uses external RNG.
/// Other raw indices read adjacent native memory and are not modeled as rules.
pub fn calculate_tetherball_zone(
    reset: &mut ResetState,
    ball: &BallMotion,
    hit_type: i32,
    services: &mut impl Services,
) -> i32 {
    let zone = if hit_type == 7 {
        i32::from(services.random_range(0, 1) == 0)
    } else {
        assert!((0..4).contains(&hit_type), "native zone table hit index");
        let row = match ball.zone {
            Zone::Zero => 0,
            Zone::One => 1,
        };
        [[0, 1, 2, 2], [2, 2, 1, 0]][row][hit_type as usize]
    };
    reset.word_224 = if zone == 2 { 4 } else { hit_type };
    zone
}
pub fn consume_charge(
    lifecycle: &mut Lifecycle,
    state: &mut RallyRuleState,
    player: usize,
    amount: u32,
    services: &mut impl Services,
) -> bool {
    let charge = lifecycle.mega_values[player] as u32;
    let success = charge >= amount;
    if success {
        lifecycle.mega_values[player] = charge.wrapping_sub(amount) as i32;
        services.mega_value(player as i32, lifecycle.mega_values[player]);
    }
    state.ai_charge[player] = lifecycle.mega_values[player] as u32;
    success
}
pub fn increment_charge_meter(
    lifecycle: &mut Lifecycle,
    state: &mut RallyRuleState,
    player: usize,
    services: &mut impl Services,
) {
    if (lifecycle.mega_values[player] as u32) < 5 {
        lifecycle.mega_values[player] =
            lifecycle.mega_values[player].wrapping_add(lifecycle.mega_states[player]);
        services.mega_value(player as i32, lifecycle.mega_values[player]);
        if (lifecycle.mega_values[player] as u32) >= 5 {
            services.sound(true, 34, 0, 100);
        }
    }
    if (lifecycle.mega_values[player] as u32) > 5 {
        lifecycle.mega_values[player] = 5;
        services.mega_value(player as i32, 5);
    }
    state.ai_charge[player] = lifecycle.mega_values[player] as u32;
}
pub fn check_for_ball_drop(
    lifecycle: &Lifecycle,
    reset: &mut ResetState,
    ball: &mut BallMotion,
    services: &mut impl Services,
) {
    reset.counter_270 = reset.counter_270.wrapping_add(1);
    if reset.counter_270 == 3 {
        ball.drop_one_zone();
        lifecycle.adjust_camera_height(
            match ball.zone {
                Zone::Zero => 0,
                Zone::One => 1,
            },
            services,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tetherball_match::WinnerUi;
    use serde_json::{Value, json};
    struct Recorder {
        effects: Vec<Value>,
        random: i32,
    }
    impl Services for Recorder {
        fn mega_value(&mut self, p: i32, v: i32) {
            self.effects.push(json!(["mega_value", p, v]));
        }
        fn sound(&mut self, front: bool, s: i32, v: i32, n: i32) {
            self.effects.push(json!([
                if front {
                    "sound_frontend"
                } else {
                    "sound_backend"
                },
                s,
                v,
                n
            ]));
        }
        fn camera_offset(&mut self, t: bool, v: [f32; 3], ms: u32) {
            self.effects
                .push(json!(["camera", t, v.map(f32::to_bits), ms]));
        }
        fn random_range(&mut self, lo: i32, hi: i32) -> i32 {
            self.effects.push(json!(["random", lo, hi, self.random]));
            self.random
        }
        fn scoreboard(&mut self, _: [i32; 4]) {
            panic!("unexpected service")
        }
        fn round(&mut self, _: [i32; 3]) {
            panic!("unexpected service")
        }
        fn serve_bubble(&mut self, _: [i32; 3]) {
            panic!("unexpected service")
        }
        fn mega_visible(&mut self, _: i32, _: i32) {
            panic!("unexpected service")
        }
        fn winner_visible(&mut self, _: WinnerUi, _: i32, _: bool) {
            panic!("unexpected service")
        }
        fn controller_pop(&mut self, _: i32) {
            panic!("unexpected service")
        }
        fn controller_set(&mut self, _: i32, _: i32) {
            panic!("unexpected service")
        }
        fn animation(&mut self, _: usize, _: i32, _: bool, _: i32) {
            panic!("unexpected service")
        }
        fn switch_to_ai(&mut self, _: usize) {
            panic!("unexpected service")
        }
        fn wrap_particle(&mut self, _: usize, _: [f32; 3], _: i32) {
            panic!("unexpected service")
        }
        fn fade_in(&mut self, _: i32) {
            panic!("unexpected service")
        }
        fn clear_hud(&mut self) {
            panic!("unexpected service")
        }
        fn close_screen(&mut self) {
            panic!("unexpected service")
        }
        fn reset_round(&mut self, _: &mut Lifecycle, _: &mut BallMotion) {
            panic!("unexpected service")
        }
        fn post_game(&mut self, _: i32, _: &[u32; 70]) {
            panic!("unexpected service")
        }
    }
    #[test]
    fn original_rally_rules_full_snapshots() {
        let data: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_rally_rules_golden.json"
        ))
        .unwrap();
        assert_eq!(data["elf_sha256"], crate::recovered::ELF_SHA256);
        assert_eq!(data["cases"].as_array().unwrap().len(), 511);
        for c in data["cases"].as_array().unwrap() {
            let mut lifecycle: Lifecycle = serde_json::from_value(c["initial"].clone()).unwrap();
            let mut state: RallyRuleState = serde_json::from_value(c["state"].clone()).unwrap();
            let mut ball = crate::tetherball::tests::ball(&c["ball"]);
            let mut reset = ResetState::blank();
            reset.word_224 = c["word_224"].as_i64().unwrap() as i32;
            reset.counter_270 = c["counter_270"].as_u64().unwrap() as u32;
            let mut gestures = GestureState {
                hit_attempt_marker: c["hit_attempt_marker"].as_bool().unwrap(),
                pending: vec![],
            };
            let mut attempt = HitAttempt {
                strike: c["attempt"]["strike"].as_bool().unwrap(),
                reverse_strike: c["attempt"]["reverse_strike"].as_bool().unwrap(),
                power_type: c["attempt"]["power_type"].as_i64().unwrap() as i32,
            };
            let mut recorder = Recorder {
                effects: vec![],
                random: c["randoms"][0].as_i64().unwrap() as i32,
            };
            let player = c["player"].as_u64().unwrap() as usize;
            let argument = c["argument"].as_u64().unwrap() as u32;
            let returned = match c["operation"].as_str().unwrap() {
                "attempt" => {
                    get_ai_hit_attempt(&lifecycle, &mut gestures, &state, &mut attempt);
                    None
                }
                "power" => Some(get_ai_power_hit_type(&state) as u32),
                "zone" => Some(calculate_tetherball_zone(
                    &mut reset,
                    &ball,
                    argument as i32,
                    &mut recorder,
                ) as u32),
                "consume" => Some(u32::from(consume_charge(
                    &mut lifecycle,
                    &mut state,
                    player,
                    argument,
                    &mut recorder,
                ))),
                "increment" => {
                    increment_charge_meter(&mut lifecycle, &mut state, player, &mut recorder);
                    None
                }
                "drop" => {
                    check_for_ball_drop(&lifecycle, &mut reset, &mut ball, &mut recorder);
                    None
                }
                _ => panic!("invalid operation"),
            };
            assert_eq!(
                json!(gestures.hit_attempt_marker),
                c["expected_hit_attempt_marker"]
            );
            let label = c["label"].as_str().unwrap();
            assert_eq!(
                serde_json::to_value(&lifecycle).unwrap(),
                c["expected"],
                "{label}: lifecycle"
            );
            crate::tetherball::tests::assert_bits(
                &ball,
                &crate::tetherball::tests::ball(&c["expected_ball"]),
                0,
                0,
            );
            assert_eq!(
                serde_json::to_value(&state).unwrap(),
                c["expected_state"],
                "{label}: rule state"
            );
            assert_eq!(
                json!({"strike":attempt.strike,"reverse_strike":attempt.reverse_strike,"power_type":attempt.power_type}),
                c["expected_attempt"],
                "{label}: outparams"
            );
            assert_eq!(
                json!(reset.word_224),
                c["expected_word_224"],
                "{label}: hit type"
            );
            assert_eq!(
                json!(reset.counter_270),
                c["expected_counter_270"],
                "{label}: drop count"
            );
            assert_eq!(json!(returned), c["returned"], "{label}: return");
            assert_eq!(json!(recorder.effects), c["effects"], "{label}: effects");
        }
    }
}
