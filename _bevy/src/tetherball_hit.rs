//! Original tetherball hit effects, multiplier feedback, and hit indicator.
//!
//! The ball/state decisions execute here. Engine-owned particle, controller,
//! audio, camera and charge-meter effects are explicit synchronous services.
//! See `docs/TETHERBALL_HIT.md` for the state projection and oracle boundary.

use crate::tetherball::{BallMotion, Direction, Zone};
use crate::tetherball_angles::wrap_angle;
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_rally_rules::{RallyRuleState, increment_charge_meter};
use crate::tetherball_reset::ResetState;
use crate::tetherball_serve::ServeServices;

const PI_OVER_TWO: f32 = f32::from_bits(0x3fc90fdb);
const NORMAL_HIT_SPEED_CAPS: [f32; 5] = [4.5, 4.9, 5.3, 5.7, 6.0]; // 0x80442094
const X2_FX: &str = "pg_tetherball_x2";
const X3_FX: &str = "pg_tetherball_x3";
const X4_FX: &str = "pg_tetherball_x4";
const X5_FX: &str = "pg_tetherball_x5";
const POSITIVE_FX: &str = "pg_generic_positive";
const HIT_INDICATOR_FX: &str = "pg_tetherball_glow";
const HIT_INDICATOR_DESTROY_MS: i32 = 0;
const OVERLAY_FX_DESTROY_MS: i32 = 4000;
const HIT_FX_DESTROY_MS: i32 = 2500;
const MULTIPLIER_FX_DESTROY_MS: i32 = 2000;

/// Additional MGTetherball scalars required by hit, multiplier and indicator
/// paths. Offsets are kept in field names because Return/Accelerate share them.
#[derive(Debug, Clone, PartialEq)]
pub struct HitState {
    pub field_32c: bool,
    pub field_32d: bool,
    pub mega_ability_42e: bool,
    pub pending_zone_274: i32,
    pub power_hit_type_43c: i32,
    pub hit_multiplier_428: f32,
    pub hit_multiplier_enabled_42d: bool,
    pub indicator_scale_338: f32,
    pub indicator_rate_33c: f32,
    /// Four three-distance tables beginning at +0x360/+36c/+378/+384.
    pub indicator_angles_360_36c_378_384: [[f32; 3]; 4],
}

/// Host data read from the game object or initialized static storage.
#[derive(Debug, Clone, PartialEq)]
pub struct HitInputs {
    /// Tetherball object position +0x40.
    pub ball_position: [f32; 3],
    /// Per-player GAME +0x2fc (normal) and +0x304 (power) effect names.
    pub fx_names: [[String; 2]; 2],
    /// Caller-supplied vector corresponding to `kWallballOriginPosition`
    /// at 0x805e37d0; no initializer value is synthesized here.
    pub controller_fx_offset: [f32; 3],
}

/// Part-effect and controller-position access used by the original graph.
pub trait HitServices: ServeServices {
    fn create_part_fx(&mut self, name: &str, position: [f32; 3]) -> u32;
    fn disable_and_destroy_part_fx(&mut self, guid: u32, delay_ms: i32);
    fn get_part_fx(&mut self, guid: u32) -> u32;
    fn set_part_fx_position(&mut self, part_fx: u32, position: [f32; 3]);
    fn set_part_fx_scale(&mut self, part_fx: u32, scale: f32);
    /// Character world position at GAME+0x120[player] +0x180/+0x184/+0x188.
    fn character_position(&mut self, player: usize) -> Option<[f32; 3]>;
}

/// Execute `HitTetherball` (`0x8039a2c0`) through its game-owned stores and
/// original ball hit. The caller must have already selected +0x224 and +0x274
/// using the original attempt/zone stages.
pub fn hit_tetherball(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    rally: &mut RallyRuleState,
    ball: &mut BallMotion,
    hit: &mut HitState,
    inputs: &HitInputs,
    services: &mut impl HitServices,
) {
    let mode = usize::try_from(reset.game_mode_044)
        .ok()
        .filter(|&mode| mode < NORMAL_HIT_SPEED_CAPS.len())
        .expect("native hit speed table game-mode index must be 0..5");
    let cap = NORMAL_HIT_SPEED_CAPS[mode];

    // The retail branch applies the cap and type-specific modifier only when
    // the unmodified speed exceeds its mode cap. The +0x42d multiplier check
    // follows that branch and therefore applies independently of the cap.
    if !(reset.timer_26c <= cap) {
        reset.timer_26c = cap;
        match reset.word_224 {
            1 | 3 => reset.timer_26c *= reset.round_tunables_34c_358[1],
            7 => reset.timer_26c *= reset.round_tunables_34c_358[2],
            _ => {}
        }
    }
    if hit.hit_multiplier_enabled_42d {
        reset.timer_26c *= hit.hit_multiplier_428;
    }

    let angle = wrap_angle(ball.angle);
    let direction = match reset.counter_268 {
        0 => Direction::Zero,
        1 => Direction::One,
        value => panic!("native hit direction outside 0/1 domain: {value}"),
    };
    ball.hit(direction, reset.timer_26c, angle, reset.word_224);

    assert!(
        matches!(hit.pending_zone_274, 0 | 1),
        "native HitTetherball zone must be 0 or 1"
    );
    lifecycle.adjust_camera_height(hit.pending_zone_274, services);
    ball.set_zone(if hit.pending_zone_274 == 0 {
        Zone::Zero
    } else {
        Zone::One
    });

    // +0x35c uses a single fadds store. Counter +0x270 is reset for this hit.
    reset.field_35c += 0.15;
    reset.counter_270 = 0;

    let player = lifecycle.server;
    let mut hit_guid = reset.invalid_guid;
    match reset.word_224 {
        0 => {
            increment_charge_meter(lifecycle, rally, player, services);
            play_backend(services, player, 0x11);
            if lifecycle.players[player].controller.is_some() {
                services.wiimote_sound(player, 6, 0x1000);
                hit.power_hit_type_43c = 0;
            }
            hit_guid = services.create_part_fx(&inputs.fx_names[player][0], inputs.ball_position);
        }
        1 => {
            play_backend(services, player, 0x12);
            play_backend(services, player, 0x17);
            if lifecycle.players[player].controller.is_some() {
                services.wiimote_sound(player, 8, 0x1000);
                hit.power_hit_type_43c = 1;
            }
            hit_guid = services.create_part_fx(&inputs.fx_names[player][1], inputs.ball_position);
            lifecycle.statistics[player][2] = lifecycle.statistics[player][2].wrapping_add(1);
        }
        2 => {
            increment_charge_meter(lifecycle, rally, player, services);
            play_backend(services, player, 0x13);
            if lifecycle.players[player].controller.is_some() {
                services.wiimote_sound(player, 7, 0x1000);
                hit.power_hit_type_43c = 2;
            }
            hit_guid = services.create_part_fx(&inputs.fx_names[player][0], inputs.ball_position);
        }
        3 => {
            play_backend(services, player, 0x14);
            play_backend(services, player, 0x17);
            if lifecycle.players[player].controller.is_some() {
                services.wiimote_sound(player, 8, 0x1000);
                hit.power_hit_type_43c = 3;
            }
            hit_guid = services.create_part_fx(&inputs.fx_names[player][1], inputs.ball_position);
            lifecycle.statistics[player][2] = lifecycle.statistics[player][2].wrapping_add(1);
        }
        7 => {
            services.camera_shake(400, 0.2);
            play_backend(services, player, 0x14);
            play_backend(services, player, 0x17);
            if lifecycle.players[player].controller.is_some() {
                services.wiimote_sound(player, 10, 0x1000);
                hit.power_hit_type_43c = 7;
            }
            hit_guid = services.create_part_fx(&inputs.fx_names[player][1], inputs.ball_position);
            lifecycle.statistics[player][3] = lifecycle.statistics[player][3].wrapping_add(1);
        }
        _ => {}
    }

    if hit.field_32c {
        if lifecycle.players[player].controller.is_some() {
            let position = controller_effect_position(player, inputs, services);
            let guid = services.create_part_fx(POSITIVE_FX, position);
            services.disable_and_destroy_part_fx(guid, OVERLAY_FX_DESTROY_MS);
            let guid = services.create_part_fx(&inputs.fx_names[player][1], position);
            services.disable_and_destroy_part_fx(guid, HIT_FX_DESTROY_MS);
            services.sound(true, 0x1e, 0, 100);
        }
        hit.field_32c = false;
    }

    // r30 begins at the global invalid GUID and receives a hit-FX GUID in known hit branches.
    // Retail compares it with the global invalid GUID and schedules any other
    // value only when it differs from that sentinel.
    if hit_guid != reset.invalid_guid {
        services.disable_and_destroy_part_fx(hit_guid, HIT_FX_DESTROY_MS);
    }
}

/// Execute `IncrementMultiplier` (`0x8039c220`), including its stage pop-up
/// and attached-controller effects. +0x30c is Lifecycle.mega_states.
pub fn increment_multiplier(
    lifecycle: &mut Lifecycle,
    player: usize,
    inputs: &HitInputs,
    services: &mut impl HitServices,
) {
    let multiplier = lifecycle.mega_states[player] as u32;
    if multiplier < 5 {
        lifecycle.mega_states[player] = multiplier.wrapping_add(1) as i32;
    }
    let multiplier = lifecycle.mega_states[player];
    let stage_fx = match multiplier {
        2 => Some(X2_FX),
        3 => Some(X3_FX),
        4 => Some(X4_FX),
        5 => Some(X5_FX),
        _ => None,
    };
    if let Some(name) = stage_fx {
        let guid = services.create_part_fx(name, inputs.ball_position);
        services.disable_and_destroy_part_fx(guid, MULTIPLIER_FX_DESTROY_MS);
    }

    let active_player = lifecycle.server;
    if (multiplier as u32) > 1 && lifecycle.players[active_player].controller.is_some() {
        let position = controller_effect_position(active_player, inputs, services);
        let guid = services.create_part_fx(POSITIVE_FX, position);
        services.disable_and_destroy_part_fx(guid, OVERLAY_FX_DESTROY_MS);
        let guid = services.create_part_fx(&inputs.fx_names[active_player][1], position);
        services.disable_and_destroy_part_fx(guid, HIT_FX_DESTROY_MS);
        services.sound(true, 0x1e, 0, 100);
    }
}

/// Update or destroy the persistent `DrawHitIndicatorParticle` effect.
/// `delta_ms` is the unsigned native frame delta; `draw` is the original bool.
pub fn draw_hit_indicator_particle(
    lifecycle: &Lifecycle,
    reset: &mut ResetState,
    ball: &BallMotion,
    hit: &mut HitState,
    delta_ms: u32,
    draw: bool,
    inputs: &HitInputs,
    services: &mut impl HitServices,
) {
    if !draw {
        if reset.field_334_guid != reset.invalid_guid {
            services.disable_and_destroy_part_fx(reset.field_334_guid, HIT_INDICATOR_DESTROY_MS);
            reset.field_334_guid = reset.invalid_guid;
        }
        return;
    }

    hit.indicator_scale_338 =
        (delta_ms as f32).mul_add(hit.indicator_rate_33c, hit.indicator_scale_338);
    // Native ble leaves only ordered values <= 1 untouched; unordered values
    // fall through to the clamp store as well.
    if !(hit.indicator_scale_338 <= 1.0) {
        hit.indicator_scale_338 = 1.0;
    }

    if reset.field_334_guid == reset.invalid_guid {
        reset.field_334_guid = services.create_part_fx(HIT_INDICATOR_FX, inputs.ball_position);
        let angular_velocity = ball.angular_velocity;
        let height = if angular_velocity >= 0.0 {
            angular_velocity
        } else {
            -angular_velocity
        };
        let table = if lifecycle.match_state.state_code == 0x1c {
            &hit.indicator_angles_360_36c_378_384[0]
        } else {
            &hit.indicator_angles_360_36c_378_384[3]
        };
        let distance = usize::try_from(lifecycle.current_distance)
            .ok()
            .filter(|&distance| distance < 3)
            .expect("native hit-indicator distance index must be 0..3");
        let angle = PI_OVER_TWO - table[distance];
        let ratio = angle / height;
        let thousand_ratio = 1000.0 * ratio;
        hit.indicator_scale_338 = 0.0;
        hit.indicator_rate_33c = 1.0 / thousand_ratio;
    }

    let part_fx = services.get_part_fx(reset.field_334_guid);
    services.set_part_fx_position(part_fx, inputs.ball_position);
    services.set_part_fx_scale(part_fx, hit.indicator_scale_338);
}

fn play_backend(services: &mut impl ServeServices, player: usize, sound: i32) {
    let azimuth = services.azimuth(player);
    services.sound(false, sound, azimuth, 100);
}

fn controller_effect_position(
    player: usize,
    inputs: &HitInputs,
    services: &mut impl HitServices,
) -> [f32; 3] {
    let character_position = services
        .character_position(player)
        .expect("attached player's character position must be available");
    std::array::from_fn(|axis| character_position[axis] + inputs.controller_fx_offset[axis])
}
