//! Original UpdateReturn (80398368) and UpdateAccelerate (80398a84).
use crate::tetherball::{BallMotion, Direction};
use crate::tetherball_angles::{is_between, wrap_angle};
use crate::tetherball_gestures::{GestureState, HitAttempt, sample_player_hit_attempt};
use crate::tetherball_hit::{
    HitInputs, HitServices, HitState, draw_hit_indicator_particle, hit_tetherball,
    increment_multiplier,
};
use crate::tetherball_hit_animation::{
    HitAnimations, check_for_waiting_character_swing, in_hit_animation, is_ball_in_hit_range,
    start_hit_animation, start_ready_animation,
};
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_match::MatchRules;
use crate::tetherball_rally_rules::{
    RallyRuleState, calculate_tetherball_zone, check_for_ball_drop, consume_charge,
    get_ai_hit_attempt,
};
use crate::tetherball_reset::ResetState;
use crate::tetherball_serve::{ServeState, set_active_character, update_pause_events};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RallyPhase {
    Return,
    Accelerate,
}

/// Engine services are synchronous; all gameplay helpers execute their recovered bodies.
#[allow(clippy::too_many_arguments)]
pub fn update_rally(
    phase: RallyPhase,
    life: &mut Lifecycle,
    reset: &mut ResetState,
    gestures: &mut GestureState,
    ball: &mut BallMotion,
    serve: &mut ServeState,
    rally: &mut RallyRuleState,
    hit: &mut HitState,
    animations: &HitAnimations,
    rules: MatchRules,
    milliseconds: i32,
    input: &HitInputs,
    services: &mut impl HitServices,
) -> bool {
    if life.paused {
        return true;
    }
    let accelerate = phase == RallyPhase::Accelerate;
    let player = life.server;
    if life.latches[0] && (reset.counter_260 as i32) > 0 {
        reset.counter_260 = reset.counter_260.wrapping_sub(milliseconds as u32);
        if (reset.counter_260 as i32) <= 0 {
            if accelerate {
                increment_multiplier(life, player, input, services);
            }
            life.statistics[player][0] = life.statistics[player][0].wrapping_add(1);
            life.statistics[player][1] = life.statistics[player][1].wrapping_add(1);
            hit_tetherball(life, reset, rally, ball, hit, input, services);
        }
    } else if life.latches[1] && (reset.counter_260 as i32) > 0 {
        reset.counter_260 = reset.counter_260.wrapping_sub(milliseconds as u32);
        if (reset.counter_260 as i32) <= 0 {
            life.statistics[player][0] = life.statistics[player][0].wrapping_add(1);
            ball.miss();
            let azimuth = services.azimuth(player);
            services.sound(false, 0x15, azimuth, 100);
            if hit.field_32d {
                if life.players[player].controller.is_some() {
                    let position = services
                        .character_position(player)
                        .expect("present character position");
                    let position =
                        std::array::from_fn(|i| position[i] + input.controller_fx_offset[i]);
                    services.serve_particle("pg_generic_negative", position, 4000);
                    services.sound(true, 0x1f, 0, 100);
                }
                hit.field_32d = false;
            }
        }
    }
    check_for_waiting_character_swing(
        life,
        ball,
        serve,
        animations,
        hit.mega_ability_42e,
        services,
    );
    let distance = life.current_distance as usize;
    let table = &hit.indicator_angles_360_36c_378_384;
    let row = if accelerate { 2 } else { 0 };
    let narrow = is_ball_in_hit_range(
        life,
        reset,
        ball,
        player,
        table[row][distance],
        table[row + 1][distance],
    );
    let broad = if accelerate {
        is_ball_in_hit_range(
            life,
            reset,
            ball,
            player,
            table[2][distance],
            f32::from_bits(0x3fc90fdb),
        )
    } else {
        is_ball_in_hit_range(
            life,
            reset,
            ball,
            player,
            f32::from_bits(0x3fc90fdb),
            table[1][distance],
        )
    };
    let draw = (broad && !life.latches[0]) || (life.latches[0] && (reset.counter_260 as i32) > 0);
    draw_hit_indicator_particle(
        life,
        reset,
        ball,
        hit,
        milliseconds as u32,
        draw,
        input,
        services,
    );
    let mut attempt = HitAttempt::default();
    if !gestures.hit_attempt_marker {
        if let Some(controller) = life.players[player]
            .controller
            .filter(|_| !serve.forced_ai[player])
        {
            sample_player_hit_attempt(
                gestures,
                life.action_states[player],
                ball.zone as i32,
                hit.mega_ability_42e,
                &mut attempt,
                |action| services.event(controller, action),
            );
        } else {
            get_ai_hit_attempt(life, gestures, rally, &mut attempt);
        }
        if !in_hit_animation(life.players[player].current_animation) {
            start_ready_animation(life, ball, animations, player, attempt.power_type, services);
        }
    }
    life.action_states[player] = 2;
    if is_between(
        wrap_angle(ball.angle),
        serve.return_angles[0],
        serve.return_angles[1],
        player != 0,
    ) {
        if attempt.strike || attempt.reverse_strike {
            if attempt.power_type != 0 {
                let cost = if attempt.power_type == 3 { 5 } else { 1 };
                if !consume_charge(life, rally, player, cost, services) {
                    attempt.power_type = 0;
                } else if attempt.power_type == 3 {
                    hit.field_32d = true;
                    hit.field_32c = false;
                }
            }
            start_hit_animation(
                life,
                ball,
                serve,
                animations,
                player,
                attempt.strike,
                attempt.reverse_strike,
                attempt.power_type,
                services,
            );
            let delay = if accelerate { 160 } else { 110 };
            if narrow {
                reset.counter_260 = delay;
                let kind;
                if attempt.power_type == 3 {
                    reset.counter_260 = 110;
                    hit.field_32c = true;
                    hit.field_32d = false;
                    reset.timer_26c = reset.round_tunables_34c_358[2]
                        * (reset.round_tunables_34c_358[0] + reset.field_35c);
                    kind = 7;
                } else {
                    if attempt.strike && attempt.power_type == 2 {
                        let cap = 0.3f32 + reset.round_tunables_34c_358[0];
                        if reset.round_tunables_34c_358[0] + reset.field_35c > cap {
                            reset.round_tunables_34c_358[0] = cap;
                        }
                        reset.field_35c = 0.0;
                    }
                    let speed = reset.round_tunables_34c_358[0] + reset.field_35c;
                    (kind, reset.timer_26c) = match attempt.power_type {
                        2 => (1, reset.round_tunables_34c_358[1] * speed),
                        1 => (3, reset.round_tunables_34c_358[1] * speed),
                        _ => (if attempt.strike { 0 } else { 2 }, speed),
                    };
                }
                hit.pending_zone_274 = calculate_tetherball_zone(reset, ball, kind, services);
                if accelerate {
                    reset.timer_26c *= reset.round_tunables_34c_358[3];
                }
                if hit.pending_zone_274 == 2 {
                    life.latches[1] = true;
                    if !accelerate {
                        reset.word_224 = 4;
                    }
                } else {
                    if !accelerate {
                        life.latches[0] = true;
                    }
                    if let Some(controller) = life.players[player].controller {
                        services.rumble(
                            controller,
                            if attempt.power_type == 0 { 150 } else { 350 },
                            0.5,
                        );
                    }
                    life.latches[0] = true;
                    reset.counter_268 = if accelerate {
                        ball.direction as u32
                    } else {
                        u32::from(ball.direction != Direction::One)
                    };
                }
            } else {
                reset.counter_260 = delay;
                life.latches[1] = true;
            }
        }
    } else if accelerate {
        gestures.hit_attempt_marker = false;
        set_active_character(life, life.receiver as i32);
        serve.ai_waiting[life.server] = false;
        life.change_state(28, rules, ball, services);
    } else {
        set_active_character(life, life.receiver as i32);
        if life.latches[0] {
            gestures.hit_attempt_marker = false;
            life.change_state(28, rules, ball, services);
            serve.ai_waiting[life.server] = false;
            life.mega_states[life.server] = 1;
            life.mega_states[life.receiver] = 1;
        } else {
            if !life.latches[1] {
                life.statistics[life.receiver][0] =
                    life.statistics[life.receiver][0].wrapping_add(1);
            }
            gestures.hit_attempt_marker = false;
            check_for_ball_drop(life, reset, ball, services);
            life.change_state(29, rules, ball, services);
            serve.ai_waiting[life.server] = true;
            reset.word_224 = 4;
        }
    }
    update_pause_events(life, reset, serve, services);
    true
}

#[cfg(test)]
#[path = "tetherball_rally_tests.rs"]
mod tests;
