//! ResetRound composition with native player-animation initialization.
use crate::tetherball::BallMotion;
use crate::tetherball_animation_init::initialize_player_animations;
use crate::tetherball_hit_animation::HitAnimations;
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_reset::{ResetEffect, ResetInputs, ResetState};
use crate::tetherball_serve::ServeState;

/// Replace only ResetRound's supplied animation initializer with its decoded
/// body. Other engine/database/AI responses remain explicit ResetInputs.
/// Returned engine effects preserve their native order and omit the consumed
/// game-helper marker, which emits no engine effects of its own.
pub fn reset_round_with_animations(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    ball: &mut BallMotion,
    input: &ResetInputs,
    serve: &mut ServeState,
    animations: &mut HitAnimations,
) -> Vec<ResetEffect> {
    let original_lose = lifecycle.lose_animations;
    let effects = crate::tetherball_reset::reset_round(lifecycle, reset, ball, input);
    // The existing projection supplies +190/+194 externally. Preserve the
    // pre-call table instead, so count 0/1 retain unprocessed native entries.
    lifecycle.lose_animations = original_lose;
    let mut initialized = false;
    let mut next_animation = 0;
    let mut result = Vec::with_capacity(effects.len());
    for effect in effects {
        match effect {
            ResetEffect::InitializePlayerAnimations => {
                assert!(
                    !initialized,
                    "one native initializer boundary per ResetRound"
                );
                initialize_player_animations(lifecycle, reset, serve, animations);
                initialized = true;
                next_animation = 2;
            }
            ResetEffect::AnimationNextState {
                player,
                mut state,
                force,
                blend,
            } if next_animation > 0 => {
                // SetUpServer requests server 58, then the receiver's newly
                // initialized +190 array entry. Keep that first request intact.
                if next_animation == 1 {
                    assert_eq!(player, lifecycle.receiver);
                    state = lifecycle.lose_animations[player];
                }
                next_animation -= 1;
                result.push(ResetEffect::AnimationNextState {
                    player,
                    state,
                    force,
                    blend,
                });
            }
            effect => result.push(effect),
        }
    }
    assert!(
        initialized && next_animation == 0,
        "complete native SetUpServer animation boundary"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tetherball_hit_animation::ReadyAnimation;
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
    fn original_reset_round_with_native_animation_initialization() {
        let f: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_reset_runtime_golden.json"
        ))
        .unwrap();
        assert_eq!(f["elf_sha256"], crate::recovered::ELF_SHA256);
        assert_eq!(f["cases"].as_array().unwrap().len(), 288);
        for (i, c) in f["cases"].as_array().unwrap().iter().enumerate() {
            let (mut life, mut reset, mut ball, mut input) =
                crate::tetherball_reset::tests::seed(c, &f);
            // Deliberately poisonous host result proves the old initializer
            // dependency no longer drives either table or receiver request.
            input.initialized_animation_states = [-1234567, 7654321];
            let v = &c["initial_serve"];
            let mut serve = ServeState {
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
            let mut animations = HitAnimations {
                ready_power: ready(w, "0x1a0", "0x1a8"),
                ready_reverse: ready(w, "0x1d0", "0x1d8"),
                ready_zone_zero: ready(w, "0x1b8", "0x1c0"),
                ready_zone_one: ready(w, "0x1e8", "0x1f0"),
                hit_power: pair(w, "0x198"),
                hit_zone_zero: pair(w, "0x1b0"),
            };
            let effects = reset_round_with_animations(
                &mut life,
                &mut reset,
                &mut ball,
                &input,
                &mut serve,
                &mut animations,
            );
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
            let mut aux = c["aux"].clone();
            let trace =
                crate::tetherball_reset::tests::effect_trace(effects, &mut aux, &f["handles"]);
            assert_eq!(
                json!(trace),
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
}
