//! Original `MGTetherball::UpdateServe` and `SetActiveCharacter` decisions.
//!
//! `Lifecycle`, `ResetState`, `GestureState` and `BallMotion` hold all recovered
//! game-owned fields that already have a shared owner. `ServeState` contains
//! only the remaining scalar words/bytes read or written by this function.
//! Calls into controllers, audio, camera, particles, HUD and front-end handlers
//! are synchronous `ServeServices` boundaries; their decisions are never
//! replaced by guessed local results.

use crate::tetherball::{BallMotion, Direction};
use crate::tetherball_angles::{is_between, wrap_angle};
use crate::tetherball_gestures::GestureState;
use crate::tetherball_lifecycle::{Lifecycle, Services};
use crate::tetherball_match::MatchRules;
use crate::tetherball_reset::ResetState;

const SERVE_NORMAL: i32 = 5;
const SERVE_POWER: i32 = 6;
const ACTION_TOSS: i32 = 1;
const ACTION_SERVE_IDLE: i32 = 2;
const EVENT_HIGH_SERVE: i32 = 0x5c;
const EVENT_PAUSE: i32 = 0xaf;
const PARTICLE_FADE_MS: i32 = 2500;
const PAUSE_OVERLAY: &str = "PreGameInstructions";

/// GAME/global fields used by UpdateServe that have no existing shared owner.
/// The retail offsets are noted beside each field to keep the projection exact.
#[derive(Debug, Clone, PartialEq)]
pub struct ServeState {
    pub pause_block_count_0fc: i32,
    pub pause_menu_open: bool,
    pub power_serve_enabled: bool,
    pub return_angles: [f32; 2],
    pub power_animations: [i32; 2],
    pub high_animations: [i32; 2],
    pub voice_types: [i32; 2],
    /// AI entity byte +0x6c, indexed by character.
    pub ai_waiting: [bool; 2],
    /// Static controller-routing bytes at 0x80607e78..79, indexed by character.
    pub forced_ai: [bool; 2],
    /// Front-end manager flags +0x48/+0x49, written by OpenPauseMenu.
    pub frontend_flags: [bool; 2],
}

/// Per-call data whose storage belongs to the game/rendering host.
#[derive(Debug, Clone, PartialEq)]
pub struct ServeInputs {
    /// Signed frame delta passed to UpdateServe.
    pub milliseconds: i32,
    /// Tetherball object position +0x40, supplied for part-effect creation.
    pub ball_position: [f32; 3],
    /// GAME +0x2fc/+0x304 effect names, indexed [focus player][normal/power].
    pub fx_names: [[String; 2]; 2],
}

/// Synchronous services called by UpdateServe and its nested state-28 entry.
///
/// The inherited methods cover the lifecycle entry handlers. The additional
/// methods below correspond to concrete native engine calls in UpdateServe;
/// event/azimuth results are reads supplied at the point of use so query order
/// is retained in the caller's trace.
pub trait ServeServices: Services {
    fn azimuth(&mut self, player: usize) -> i32;
    fn event(&mut self, controller: i32, action: i32) -> bool;
    fn timer_visible(&mut self, visible: i32);
    fn wiimote_sound(&mut self, player: usize, sound: i32, flags: i32);
    fn camera_shake(&mut self, milliseconds: i32, strength: f32);
    fn rumble(&mut self, controller: i32, milliseconds: u32, strength: f32);
    fn serve_particle(&mut self, name: &str, position: [f32; 3], fade_ms: i32);
    fn pregame(&mut self, kind: i32, argument: i32, words: [u32; 4]);
    fn overlay(&mut self, name: &str);
    fn audio_pause(&mut self, mode: i32);
}

/// Exact scalar-store body of `SetActiveCharacter` (`0x8039a91c`) for raw
/// fields. It always stores the selector to +0x214. It writes +0x218 for
/// selector 0 or 1 and leaves that word untouched for other values.
pub fn set_active_character_fields(server_214: &mut i32, receiver_218: &mut i32, selected: i32) {
    *server_214 = selected;
    if selected == 1 {
        *receiver_218 = 0;
    } else if selected == 0 {
        *receiver_218 = 1;
    }
}

/// Lifecycle projection for the valid character domain used by UpdateServe.
/// Invalid raw selectors remain representable through
/// [`set_active_character_fields`], but cannot be indexed by `Lifecycle`.
pub fn set_active_character(lifecycle: &mut Lifecycle, selected: i32) {
    assert!(
        (0..=1).contains(&selected),
        "lifecycle player index must be 0 or 1"
    );
    let mut server = lifecycle.server as i32;
    let mut receiver = lifecycle.receiver as i32;
    set_active_character_fields(&mut server, &mut receiver, selected);
    lifecycle.server = server as usize;
    lifecycle.receiver = receiver as usize;
}

/// Execute one complete original `UpdateServe` call.
///
/// Return value is always true for this function's recovered call domain. The
/// caller supplies the same `MatchRules` used by the lifecycle state-entry
/// handler; an angle crossing enters state 28 synchronously before returning.
pub fn update_serve(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    gestures: &mut GestureState,
    ball: &mut BallMotion,
    aux: &mut ServeState,
    rules: MatchRules,
    input: &ServeInputs,
    services: &mut impl ServeServices,
) -> bool {
    if lifecycle.paused {
        return true;
    }

    // +0x420 uses signed word arithmetic and is updated before all other gates.
    lifecycle.round_timer_ms = lifecycle.round_timer_ms.wrapping_sub(input.milliseconds);
    if lifecycle.round_timer_ms < 0 {
        if lifecycle.scoreboard_visible {
            services.scoreboard([0; 4]);
            lifecycle.scoreboard_visible = false;
        }
        if lifecycle.round_visible {
            services.round([lifecycle.round_number, lifecycle.total_rounds, 0]);
            lifecycle.round_visible = false;
        }
    }

    // Delayed service after a captured serve gesture (+0x228/+0x260).
    if lifecycle.latches[0] && (reset.counter_260 as i32) > 0 {
        reset.counter_260 = reset.counter_260.wrapping_sub(input.milliseconds as u32);
        if (reset.counter_260 as i32) <= 0 {
            if ball.tossed {
                lifecycle.match_state.elapsed_ms = 0;
                reset.field_25c = true;
                reset.counter_270 = 0;
                services.timer_visible(1);

                match reset.word_224 {
                    SERVE_NORMAL => {
                        let player = lifecycle.server;
                        let azimuth = services.azimuth(player);
                        services.sound(false, 0x16, azimuth, 100);
                        ball.serve(Direction::Zero, reset.round_tunables_34c_358[0]);
                        if lifecycle.players[player].controller.is_some() {
                            services.wiimote_sound(player, 9, 0x1000);
                        }
                        let focus = player_index(lifecycle.focus_player);
                        emit_particle(services, input, focus, 0, PARTICLE_FADE_MS);
                    }
                    SERVE_POWER => {
                        let player = lifecycle.server;
                        let azimuth = services.azimuth(player);
                        services.sound(false, 0x18, azimuth, 100);
                        let azimuth = services.azimuth(player);
                        services.sound(false, 0x17, azimuth, 100);
                        let voice_sound = if aux.voice_types[player] == 0 { 9 } else { 10 };
                        let azimuth = services.azimuth(player);
                        services.sound(false, voice_sound, azimuth, 100);
                        if lifecycle.players[player].controller.is_some() {
                            services.wiimote_sound(player, 10, 0x1000);
                        }
                        let speed =
                            reset.round_tunables_34c_358[0] * reset.round_tunables_34c_358[2];
                        ball.serve(Direction::Zero, speed);
                        services.camera_shake(400, 0.2);
                        let focus = player_index(lifecycle.focus_player);
                        emit_particle(services, input, focus, 1, PARTICLE_FADE_MS);
                    }
                    _ => {}
                }

                // Controller soft-lock recovery is suppressed by the global AI
                // routing byte even when this character still has a controller.
                let player = lifecycle.server;
                if let Some(controller) = lifecycle.players[player].controller {
                    if !aux.forced_ai[player] {
                        services.controller_pop(controller);
                        services.controller_set(controller, 18);
                        services.rumble(controller, 150, 0.5);
                    }
                }
            } else {
                if !lifecycle.serve_bubble_visible {
                    services.serve_bubble([1, -1, -1]);
                    services.sound(true, 28, 0, 100);
                    lifecycle.serve_bubble_visible = true;
                }
                lifecycle.latches[0] = false;
                reset.counter_260 = 0;
            }
        }
    }

    let player = lifecycle.server;
    let focus = player_index(lifecycle.focus_player);
    let animation = lifecycle.players[player].current_animation;
    if animation != 60
        && animation != aux.power_animations[player]
        && animation != aux.high_animations[player]
    {
        if lifecycle.players[player].controller.is_some() && !aux.forced_ai[player] {
            update_human_serve(lifecycle, reset, ball, aux, input, services, player);
        } else {
            update_ai_serve(lifecycle, reset, ball, aux, services, player);
        }
    }

    // The focus player's action byte is set even when its character is not the
    // active server. This precedes angle sampling and the pause-event loop.
    lifecycle.action_states[focus] = ACTION_SERVE_IDLE;
    let angle = wrap_angle(ball.angle);
    let reverse = lifecycle.focus_player == 0;
    if is_between(angle, aux.return_angles[0], aux.return_angles[1], reverse) {
        gestures.hit_attempt_marker = false;
        set_active_character(lifecycle, lifecycle.receiver as i32);
        aux.ai_waiting[lifecycle.server] = false;
        lifecycle.change_state(28, rules, ball, services);
    }
    update_pause_events(lifecycle, reset, aux, services);
    true
}

fn update_human_serve(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    ball: &mut BallMotion,
    aux: &mut ServeState,
    input: &ServeInputs,
    services: &mut impl ServeServices,
    player: usize,
) {
    if lifecycle.action_states[player] == ACTION_TOSS && !ball.tossed {
        services.animation(player, 59, false, -1);
        ball.toss();
        reset.counter_264 = 500;
        if lifecycle.serve_bubble_visible {
            hide_serve_bubble(services);
            lifecycle.serve_bubble_visible = false;
        }
    }

    if ball.tossed {
        // A positive pre-decrement counter suppresses the predicate even when
        // this frame carries it through zero. Nonpositive input tests now.
        if (reset.counter_264 as i32) > 0 {
            reset.counter_264 = reset.counter_264.wrapping_sub(input.milliseconds as u32);
        } else if ball.can_high_serve(110) && reset.word_224 != SERVE_NORMAL {
            lifecycle.latches[0] = true;
            reset.counter_260 = 110;
            let controller = lifecycle.players[player]
                .controller
                .expect("human serve path requires a controller");
            let high_event = services.event(controller, EVENT_HIGH_SERVE);
            reset.word_224 =
                if high_event && (aux.power_serve_enabled || lifecycle.postgame_win_flag) {
                    SERVE_POWER
                } else {
                    SERVE_NORMAL
                };
            services.animation(player, 81, false, -1);
        }
    } else {
        services.animation(player, 58, false, -1);
        if !lifecycle.serve_bubble_visible {
            show_serve_bubble(lifecycle, services);
        }
    }
    lifecycle.action_states[player] = ACTION_SERVE_IDLE;
}

fn update_ai_serve(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    ball: &mut BallMotion,
    aux: &mut ServeState,
    services: &mut impl ServeServices,
    player: usize,
) {
    if lifecycle.match_state.state_ms > 2000 && !ball.tossed {
        services.animation(player, 59, false, -1);
        ball.toss();
        if lifecycle.serve_bubble_visible {
            // Retail hides the UI but leaves GAME+0x330 unchanged in this path.
            hide_serve_bubble(services);
        }
    }
    if lifecycle.match_state.state_ms > 2800 {
        lifecycle.latches[0] = true;
        reset.counter_260 = 110;
        if ball.can_power_serve(110) {
            reset.word_224 = SERVE_POWER;
            services.animation(player, aux.power_animations[player], false, -1);
        } else if ball.can_high_serve(110) {
            reset.word_224 = SERVE_NORMAL;
            services.animation(player, aux.high_animations[player], false, -1);
        } else {
            reset.word_224 = SERVE_NORMAL;
            services.animation(player, 60, false, -1);
        }
    }
}

fn update_pause_events(
    lifecycle: &mut Lifecycle,
    reset: &ResetState,
    aux: &mut ServeState,
    services: &mut impl ServeServices,
) {
    for controller in 0..lifecycle.session_mode.max(0) {
        if services.event(controller, EVENT_PAUSE)
            && !aux.pause_menu_open
            && aux.pause_block_count_0fc <= 0
        {
            open_pause_menu(lifecycle, reset, aux, services, controller);
        }
    }
}

fn open_pause_menu(
    lifecycle: &mut Lifecycle,
    reset: &ResetState,
    aux: &mut ServeState,
    services: &mut impl ServeServices,
    controller: i32,
) {
    // `OpenPauseMenu` rechecks these guards before its first store.
    if aux.pause_menu_open || aux.pause_block_count_0fc > 0 {
        return;
    }
    aux.pause_menu_open = true;
    lifecycle.paused = true;
    let words = [
        1,
        controller as u32,
        u32::from(reset.base_player_count_070 > 1),
        lifecycle.game_type as u32,
    ];
    services.pregame(2, 0, words);
    aux.frontend_flags = [true; 2];
    services.overlay(PAUSE_OVERLAY);
    services.sound(true, 12, 0, 100);
    services.audio_pause(2);
}

fn show_serve_bubble(lifecycle: &mut Lifecycle, services: &mut impl Services) {
    lifecycle.serve_bubble_visible = true;
    services.serve_bubble([1, -1, -1]);
    services.sound(true, 28, 0, 100);
}

fn hide_serve_bubble(services: &mut impl Services) {
    services.serve_bubble([0, -1, -1]);
    services.sound(true, 29, 0, 100);
}

fn emit_particle(
    services: &mut impl ServeServices,
    input: &ServeInputs,
    focus: usize,
    kind: usize,
    fade_ms: i32,
) {
    services.serve_particle(&input.fx_names[focus][kind], input.ball_position, fade_ms);
}

fn player_index(player: i32) -> usize {
    usize::try_from(player)
        .ok()
        .filter(|&player| player < 2)
        .expect("MGTetherball character selector must be 0 or 1")
}

#[cfg(test)]
#[path = "tetherball_serve_tests.rs"]
mod tests;
