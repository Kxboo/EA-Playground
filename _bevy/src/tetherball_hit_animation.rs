//! Tetherball's native hit-animation gates and hit-range math.
//!
//! Animation IDs that are player/game-configured are supplied through
//! `HitAnimations`; Lifecycle, ResetState and BallMotion remain the owners of
//! the state already decoded by their respective modules.

use crate::tetherball::{BallMotion, Zone, wrap_angle};
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_reset::ResetState;
use crate::tetherball_serve::ServeServices;
use crate::tetherball_serve::ServeState;

const GRUNT_THRESHOLDS: [u32; 3] = [50, 75, 75];
const NORMAL_EVENT: i32 = 0x5c;
const REVERSE_EVENT: i32 = 0x5d;
const ACTION_SERVE_IDLE: i32 = 2;

/// One ready-animation selection and its current-animation suppression value.
/// The native game stores these as separate two-player arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyAnimation {
    pub state: [i32; 2],
    pub suppress_if_current: [i32; 2],
}

/// Native animation IDs stored in the MGTetherball per-player tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitAnimations {
    /// MGT+0x1a0 / +0x1a8, selected for power-hit type 2.
    pub ready_power: ReadyAnimation,
    /// MGT+0x1d0 / +0x1d8, selected for power-hit type 1.
    pub ready_reverse: ReadyAnimation,
    /// MGT+0x1b8 / +0x1c0, zone-zero ordinary ready animation.
    pub ready_zone_zero: ReadyAnimation,
    /// MGT+0x1e8 / +0x1f0, zone-one ordinary ready animation.
    pub ready_zone_one: ReadyAnimation,
    /// MGT+0x198 state, shared by both regular hit directions.
    pub hit_power: [i32; 2],
    /// MGT+0x1b0 state used by a zone-zero ordinary hit.
    pub hit_zone_zero: [i32; 2],
}

/// `InHitAnimation` (`0x8039a8cc`) is a native switch over the active graph
/// animation. These are precisely the IDs whose jump-table arms return true.
pub fn in_hit_animation(current_animation: i32) -> bool {
    matches!(
        current_animation,
        63 | 66 | 69 | 72 | 75 | 78 | 81 | 84 | 91 | 94
    )
}

/// Execute `StartReadyAnimation` (`0x8039a178`). Its engine call is synchronous
/// through `ServeServices::animation`; a matching suppression state does not
/// issue another animation request.
pub fn start_ready_animation(
    lifecycle: &Lifecycle,
    ball: &BallMotion,
    animations: &HitAnimations,
    player: usize,
    hit_type: i32,
    services: &mut impl ServeServices,
) {
    if lifecycle.players[player].current_animation == 1 {
        return;
    }

    // Mega hit type 3 is translated to direction-specific type 1 or 2 by zone.
    let kind = if hit_type == 3 && ball.zone == Zone::Zero {
        2
    } else if hit_type == 3 {
        1
    } else {
        hit_type
    };
    let selected = if ball.zone == Zone::Zero {
        match kind {
            2 => animations.ready_power,
            1 => animations.ready_reverse,
            _ => animations.ready_zone_zero,
        }
    } else {
        match kind {
            2 => animations.ready_power,
            1 => animations.ready_reverse,
            _ => animations.ready_zone_one,
        }
    };
    if lifecycle.players[player].current_animation != selected.suppress_if_current[player] {
        services.animation(player, selected.state[player], false, -1);
    }
}

/// Execute `StartHitAnimation` (`0x80399f88`). Native body is void; callers
/// own the subsequent hit delay/timer stores. Animation, random, azimuth and
/// sound services are called in their original order.
pub fn start_hit_animation(
    lifecycle: &Lifecycle,
    ball: &BallMotion,
    serve: &ServeState,
    animations: &HitAnimations,
    player: usize,
    mut normal: bool,
    mut reverse: bool,
    mut hit_type: i32,
    services: &mut impl ServeServices,
) {
    if hit_type == 3 {
        if ball.zone == Zone::Zero {
            hit_type = 2;
            normal = true;
            reverse = false;
        } else {
            hit_type = 1;
            normal = false;
            reverse = true;
        }
    }

    let state = if normal {
        Some(match hit_type {
            2 => animations.hit_power[player],
            1 => serve.power_animations[player],
            _ => animations.hit_zone_zero[player],
        })
    } else if reverse {
        Some(match hit_type {
            2 => animations.hit_power[player],
            1 => serve.power_animations[player],
            _ => serve.high_animations[player],
        })
    } else {
        None
    };
    if let Some(state) = state {
        services.animation(player, state, true, -1);
    }

    let random_value = services.random_range(0, 0x63) as u32;
    let threshold = GRUNT_THRESHOLDS[lifecycle.current_distance as usize];
    if random_value < threshold {
        let azimuth = services.azimuth(player);
        let sound = if serve.voice_types[player] != 0 { 8 } else { 7 };
        services.sound(false, sound, azimuth, 100);
    }
}

/// Execute `CheckForWaitingCharacterSwing` (`0x80399db8`). Controller events
/// are queried in native order, including the repeated pair used to qualify a
/// simultaneous mega-hit. The receiver's action word is reset to 2 at exit.
pub fn check_for_waiting_character_swing(
    lifecycle: &mut Lifecycle,
    ball: &BallMotion,
    serve: &ServeState,
    animations: &HitAnimations,
    mega_ability: bool,
    services: &mut impl ServeServices,
) {
    let player = lifecycle.receiver;
    let current = lifecycle.players[player].current_animation;
    let Some(controller) = lifecycle.players[player].controller else {
        return;
    };
    if current == 1 || in_hit_animation(current) {
        return;
    }

    let mut hit_type = 0;
    if services.event(controller, NORMAL_EVENT) {
        hit_type = 1;
    }
    if services.event(controller, REVERSE_EVENT) {
        hit_type = 2;
    }
    let both_first_pair = services.event(controller, NORMAL_EVENT);
    if both_first_pair && services.event(controller, REVERSE_EVENT) && mega_ability {
        hit_type = 3;
    }

    if hit_type != 0 {
        start_ready_animation(lifecycle, ball, animations, player, hit_type, services);
    } else {
        services.animation(player, lifecycle.lose_animations[player], false, -1);
    }

    if lifecycle.action_states[player] == 0 {
        let state = match hit_type {
            2 => animations.hit_power[player],
            1 => serve.power_animations[player],
            _ => animations.hit_zone_zero[player],
        };
        services.animation(player, state, false, -1);
    }
    lifecycle.action_states[player] = ACTION_SERVE_IDLE;
}

/// Exact `IsBallInHitRange` (`0x80399cb0`) angle construction and test. The
/// two float arguments are supplied by the caller; the native routine scales
/// each by the selected player's signed +0x27c value and uses the +0x248 angle
/// as its base. The original wraps both constructed endpoints and the ball
/// angle before calling `IsBetween`.
pub fn is_ball_in_hit_range(
    lifecycle: &Lifecycle,
    reset: &ResetState,
    ball: &BallMotion,
    player: usize,
    half_width: f32,
    center_offset: f32,
) -> bool {
    let direction_scale = reset.ai_initial_values_27c[player] as f32;
    let base_angle = reset.start_angles_248[player];
    let center = wrap_angle(base_angle + center_offset * direction_scale);
    let lower = wrap_angle(base_angle - half_width * direction_scale);
    let current = wrap_angle(ball.angle);
    crate::tetherball_angles::is_between(
        current,
        lower,
        center,
        lifecycle.focus_player != player as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tetherball_serve::tests::{Host, floats, integer};
    use serde_json::{Value, json};

    fn table(value: &Value) -> HitAnimations {
        let pair = |name: &str| std::array::from_fn(|i| integer(&value[name][i]));
        let ready = |state: &str, suppress: &str| ReadyAnimation {
            state: pair(state),
            suppress_if_current: pair(suppress),
        };
        HitAnimations {
            ready_power: ready("ready_power_state", "ready_power_suppress"),
            ready_reverse: ready("ready_reverse_state", "ready_reverse_suppress"),
            ready_zone_zero: ready("ready_zone_zero_state", "ready_zone_zero_suppress"),
            ready_zone_one: ready("ready_zone_one_state", "ready_zone_one_suppress"),
            hit_power: pair("hit_power"),
            hit_zone_zero: pair("hit_zone_zero"),
        }
    }

    fn serve(value: &Value) -> ServeState {
        ServeState {
            pause_block_count_0fc: 0,
            pause_menu_open: false,
            power_serve_enabled: false,
            return_angles: [0.0; 2],
            power_animations: std::array::from_fn(|i| integer(&value["power_animations"][i])),
            high_animations: std::array::from_fn(|i| integer(&value["high_animations"][i])),
            voice_types: std::array::from_fn(|i| integer(&value["voice_types"][i])),
            ai_waiting: [false; 2],
            forced_ai: [false; 2],
            frontend_flags: [false; 2],
        }
    }

    #[test]
    fn original_hit_animation_and_range_vectors() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_hit_animation_golden.json"
        ))
        .unwrap();
        assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1663);
        for (index, case) in cases.iter().enumerate() {
            let label = case["label"].as_str().unwrap();
            let mut lifecycle: Lifecycle = serde_json::from_value(case["initial"].clone()).unwrap();
            let ball = crate::tetherball::tests::ball(&case["ball"]);
            let animations = table(&case["animations"]);
            let serving = serve(&case["serve"]);
            let mut host = Host {
                trace: crate::tetherball_lifecycle::tests::Recorder {
                    events: vec![],
                    randoms: vec![integer(&case["random_raw"])],
                    next: 0,
                },
                input: case["input"].clone(),
            };
            let player = case["player"].as_u64().unwrap_or(0) as usize;
            let returned = match case["kind"].as_str().unwrap() {
                "in_hit_animation" => Some(in_hit_animation(
                    lifecycle.players[player].current_animation,
                )),
                "start_ready_animation" => {
                    start_ready_animation(
                        &lifecycle,
                        &ball,
                        &animations,
                        player,
                        integer(&case["hit_type"]),
                        &mut host,
                    );
                    None
                }
                "start_hit_animation" => {
                    start_hit_animation(
                        &lifecycle,
                        &ball,
                        &serving,
                        &animations,
                        player,
                        case["normal"].as_bool().unwrap(),
                        case["reverse"].as_bool().unwrap(),
                        integer(&case["hit_type"]),
                        &mut host,
                    );
                    None
                }
                "check_for_waiting_character_swing" => {
                    check_for_waiting_character_swing(
                        &mut lifecycle,
                        &ball,
                        &serving,
                        &animations,
                        case["mega_ability"].as_bool().unwrap(),
                        &mut host,
                    );
                    None
                }
                "is_ball_in_hit_range" => {
                    let mut reset = ResetState::blank();
                    reset.start_angles_248 = floats(&case["reset"]["start_angles"]);
                    reset.ai_initial_values_27c =
                        std::array::from_fn(|i| integer(&case["reset"]["scale"][i]));
                    Some(is_ball_in_hit_range(
                        &lifecycle,
                        &reset,
                        &ball,
                        player,
                        case["half_width"].as_f64().unwrap() as f32,
                        case["center_offset"].as_f64().unwrap() as f32,
                    ))
                }
                kind => panic!("unexpected case kind {kind}"),
            };
            if let Some(value) = returned {
                assert_eq!(value, case["expected"]["returned"] == 1, "{label} return");
            }
            assert_eq!(
                lifecycle.action_states,
                std::array::from_fn(|i| integer(&case["expected"]["action_states"][i])),
                "{label} action state"
            );
            assert_eq!(
                host.trace.events,
                case["expected"]["effects"].as_array().unwrap().clone(),
                "case {index} {label} ordered effects"
            );
        }
    }
}
