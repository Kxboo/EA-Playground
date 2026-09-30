//! Player locomotion recovered from `LocalCharacterControl::Update` (0x802eeb28, 2604 bytes)
//! in playgroundz.elf.  Deterministic and independent of rendering (see docs/RECONSTRUCTION.md).
//!
//! Recovered structure (disassembly: `py tools/re_functions.py Update__21LocalCharacterControl`):
//! * input events are 0x14-byte records; type 0 = analog stick, type 1 = digital (d-pad),
//!   type 3 = face-direction only.
//! * dead zone `STICK_DEAD_ZONE` on either axis (0x802ef1fc / 0x802eec2c path).
//! * speed = |(|x|,|y|)| * `CharacterState::maxSpeed` (state +0x18, default `STATE_MAX_SPEED`),
//!   passed to `CharacterMovement::SetMoveSpeed`.
//! * facing = pi + cameraYaw + stickAngle, written by `CharacterState::SetRot`; the stick angle is
//!   snapped when the character has been idle longer than `STOP_TIME_BEFORE_ABSOLUTE_MOVEMENT_MS`,
//!   otherwise stepped toward the target by `TURN_RATE`/`TURN_RATE_FAST` (analog) or
//!   `TURN_RATE_DIGITAL` (d-pad).
//! * d-pad axes ramp toward the pressed direction by `dt / DIGITAL_TRANSITION_TIME_MS` per update.
//!
//! NOT recovered (kept out of this module on purpose): the update rate the turn steps assume
//! (they are not scaled by dt in the binary; `TICK_HZ` is an assumption) and the exact
//! `ratan` angle convention.  Tests below pin the recovered numbers.
use crate::recovered as k;

/// Assumed simulation rate.  The binary applies turn steps once per call; the call rate is unresolved.
pub const TICK_HZ: f32 = 60.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputKind { Analog, Digital }

#[derive(Debug, Clone, Copy, Default)]
pub struct Locomotion {
    /// Smoothed d-pad axes (`LocalCharacterControl` +0xac/+0xb0).
    pub smoothed: [f32; 2],
    /// Milliseconds since the character last received a facing update (+0xb4).
    pub idle_ms: i32,
    /// Stick angle relative to the camera (+0xb8), radians.
    pub stick_angle: f32,
    /// Character facing (`CharacterState` +0x80), radians.
    pub rot: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// Value passed to `SetMoveSpeed`.
    pub speed: f32,
    /// Movement angle relative to camera (`stick_angle`), when moving.
    pub move_angle: Option<f32>,
}

fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    let mut a = a % t;
    if a > std::f32::consts::PI { a -= t } else if a < -std::f32::consts::PI { a += t }
    a
}

impl Locomotion {
    /// One `LocalCharacterControl::Update` call for one input event.
    pub fn update(&mut self, dt_ms: i32, kind: InputKind, x: f32, y: f32, camera_yaw_rad: f32, max_speed: f32) -> Step {
        let (ax, ay, turn_cap) = match kind {
            InputKind::Analog => (x, y, None),
            InputKind::Digital => {
                // Ramp each smoothed axis toward the pressed direction.
                let step = dt_ms as f32 / k::DIGITAL_TRANSITION_TIME_MS as f32;
                for (s, t) in self.smoothed.iter_mut().zip([x, y]) {
                    let d = t - *s;
                    *s = if d.abs() <= step { t } else { *s + step * d.signum() };
                }
                (self.smoothed[0], self.smoothed[1], Some(k::TURN_RATE_DIGITAL))
            }
        };
        if ax.abs() <= k::STICK_DEAD_ZONE && ay.abs() <= k::STICK_DEAD_ZONE {
            self.idle_ms += dt_ms;
            return Step { speed: 0., move_angle: None };
        }
        let speed = ax.abs().hypot(ay.abs()) * max_speed;
        let target = ax.atan2(ay);
        let a = if self.idle_ms > k::STOP_TIME_BEFORE_ABSOLUTE_MOVEMENT_MS {
            target
        } else {
            let current = wrap(self.rot - camera_yaw_rad - k::PI);
            let diff = wrap(target - current);
            let cap = turn_cap.unwrap_or(if diff.abs() > std::f32::consts::FRAC_PI_2 { k::TURN_RATE_FAST } else { k::TURN_RATE });
            wrap(current + diff.clamp(-cap, cap))
        };
        self.stick_angle = a;
        self.rot = wrap(k::PI + camera_yaw_rad + a);
        self.idle_ms = 0;
        Step { speed, move_angle: Some(a) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_zone_and_idle_time() {
        let mut l = Locomotion::default();
        let s = l.update(16, InputKind::Analog, 0.05, -0.1, 0., k::STATE_MAX_SPEED);
        assert_eq!(s.speed, 0.);
        assert_eq!(l.idle_ms, 16);
    }

    #[test]
    fn full_stick_reaches_state_max_speed() {
        let mut l = Locomotion::default();
        let s = l.update(16, InputKind::Analog, 0., 1., 0., k::STATE_MAX_SPEED);
        assert_eq!(s.speed, k::STATE_MAX_SPEED);
        assert_eq!(k::STATE_MAX_SPEED, 5.0);
    }

    #[test]
    fn snap_after_stop_time_then_step_limited() {
        let mut l = Locomotion::default();
        l.idle_ms = k::STOP_TIME_BEFORE_ABSOLUTE_MOVEMENT_MS + 1;
        let s = l.update(16, InputKind::Analog, 1., 0., 0., 5.);
        assert!((s.move_angle.unwrap() - std::f32::consts::FRAC_PI_2).abs() < 1e-6, "idle character snaps");
        // Moving continuously: reversing is limited by the fast turn step.
        let s = l.update(16, InputKind::Analog, -1., 0., 0., 5.);
        let moved = wrap(s.move_angle.unwrap() - std::f32::consts::FRAC_PI_2).abs();
        assert!((moved - k::TURN_RATE_FAST).abs() < 1e-5, "turn step {moved}");
    }

    #[test]
    fn digital_ramps_over_transition_time() {
        let mut l = Locomotion::default();
        let mut ms = 0;
        while l.smoothed[1] < 1. { l.update(10, InputKind::Digital, 0., 1., 0., 5.); ms += 10; assert!(ms < 1000) }
        assert!((ms - k::DIGITAL_TRANSITION_TIME_MS).abs() <= 10, "ramp took {ms} ms");
    }

    #[test]
    fn digital_turn_is_limited_to_recovered_step() {
        let mut l = Locomotion::default();
        l.smoothed = [1., 0.];
        l.update(16, InputKind::Digital, 1., 0., 0., 5.);
        l.idle_ms = 0;
        let a0 = l.stick_angle;
        l.smoothed = [-1., 0.];
        let s = l.update(16, InputKind::Digital, -1., 0., 0., 5.);
        assert!(wrap(s.move_angle.unwrap() - a0).abs() <= k::TURN_RATE_DIGITAL + 1e-5);
    }
}
