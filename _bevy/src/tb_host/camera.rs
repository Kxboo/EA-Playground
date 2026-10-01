//! `BehindTheBackCamera` (the follow camera MGTetherball drives), ported from the executable:
//! `Update` 0x803bcae4, `SoftTransition` 0x803bd14c, `SetDesired{Position,Target}Offset` 0x803bcf10 / 0x803bd000,
//! `SetDesiredRotationAboutTarget` 0x803bd0b0, `SetDir` 0x803bcd5c and the one-line setters at 0x80397390.
//!
//! Field names carry the object offsets.  Only the Y component of the position / target offsets is animated and used by
//! `Update`; the camera sits `back - distance` behind `start` along the rotation angle and looks at the point `distance`
//! in front of it (`distance` = 2.2 from `CameraScalars`, `back` = 4.5 / 4.75 / 5.0 by player distance).
use crate::tetherball_angles::wrap_angle;

#[derive(Clone, Debug)]
pub struct Camera {
    /// +0x330 / +0x340 (current offsets), +0x350 / +0x360 (desired).
    pub position_offset: [f32; 3],
    pub target_offset: [f32; 3],
    desired_position_offset: [f32; 3],
    desired_target_offset: [f32; 3],
    /// +0x370/+0x374, +0x378/+0x37c: velocity / acceleration of the Y transitions.
    position_motion: [f32; 2],
    target_motion: [f32; 2],
    /// +0x380 current rotation, +0x394 desired, +0x384/+0x388 motion, +0x3d2 transition active.
    pub rotation: f32,
    desired_rotation: f32,
    rotation_motion: [f32; 2],
    rotation_active: bool,
    /// +0x39c orbit distance, +0x3a0 swing amplitude (unused while the swing flag +0x38c is clear).
    pub distance: f32,
    pub amplitude: f32,
    /// +0x3a4 backwards offset, +0x3b0 desired, +0x3a8/+0x3ac motion.
    pub back: f32,
    desired_back: f32,
    back_motion: [f32; 2],
    /// +0x3c0 start position.
    pub start: [f32; 3],
}

impl Default for Camera {
    /// `__ct__19BehindTheBackCamera` (0x803bc9c0).
    fn default() -> Self {
        Camera {
            position_offset: [0., 0.55, 0.],
            target_offset: [0., 0.55, 0.],
            desired_position_offset: [0., 0.55, 0.],
            desired_target_offset: [0., 0.55, 0.],
            position_motion: [0.; 2],
            target_motion: [0.; 2],
            rotation: 0.,
            desired_rotation: 0.,
            rotation_motion: [0.; 2],
            rotation_active: false,
            distance: 0.,
            amplitude: 0.4,
            back: 1.4,
            desired_back: 1.4,
            back_motion: [0.; 2],
            start: [0.; 3],
        }
    }
}

/// `SoftTransition(cur, desired, velocity, acceleration, dt)`.
fn soft(cur: &mut f32, desired: f32, motion: &mut [f32; 2], dt: i32) {
    let dt = dt as f32;
    if *cur < desired {
        *cur = motion[0].mul_add(dt, *cur);
        motion[0] = motion[1].mul_add(dt, motion[0]);
        if motion[0] < 1e-4 {
            motion[0] = 1e-4;
        }
        if *cur > desired {
            *cur = desired;
            *motion = [0.; 2];
        }
    } else if *cur > desired {
        *cur = (-motion[0]).mul_add(dt, *cur);
        motion[0] = motion[1].mul_add(dt, motion[0]);
        if motion[0] < 1e-4 {
            motion[0] = 1e-4;
        }
        if *cur < desired {
            *cur = desired;
            *motion = [0.; 2];
        }
    }
}

fn motion_for(distance: f32, ms: u32) -> [f32; 2] {
    let t = ms as f32;
    [distance / t, (-0.5 * distance) / (ms.wrapping_mul(ms)) as f32]
}

impl Camera {
    pub fn set_position_offset(&mut self, v: [f32; 3]) {
        self.position_offset = v;
    }
    pub fn set_target_offset(&mut self, v: [f32; 3]) {
        self.target_offset = v;
    }
    pub fn set_backwards(&mut self, v: f32) {
        self.back = v;
        self.desired_back = v;
    }
    pub fn set_start(&mut self, v: [f32; 3]) {
        self.start = v;
    }
    pub fn set_scalars(&mut self, distance: f32, amplitude: f32) {
        self.distance = distance;
        self.amplitude = amplitude;
    }
    pub fn set_desired_position_offset(&mut self, v: [f32; 3], ms: u32) {
        if self.desired_position_offset != v {
            self.desired_position_offset = v;
            let d = ((v[0] - self.position_offset[0]).powi(2) + (v[1] - self.position_offset[1]).powi(2) + (v[2] - self.position_offset[2]).powi(2)).sqrt();
            self.position_motion = motion_for(d, ms);
        }
    }
    pub fn set_desired_target_offset(&mut self, v: [f32; 3], ms: u32) {
        if self.desired_target_offset != v {
            self.desired_target_offset = v;
            let d = ((v[0] - self.target_offset[0]).powi(2) + (v[1] - self.target_offset[1]).powi(2) + (v[2] - self.target_offset[2]).powi(2)).sqrt();
            self.target_motion = motion_for(d, ms);
        }
    }
    pub fn set_desired_rotation(&mut self, angle: f32, ms: u32) {
        self.rotation_active = true;
        if self.desired_rotation != angle {
            self.desired_rotation = angle;
            self.rotation_motion = motion_for((angle - self.rotation).abs(), ms);
        }
    }
    /// `SetDir`: `rmAngle(direction)` = wrapped `atan2(x, z)`.
    pub fn set_dir(&mut self, d: [f32; 3]) {
        let a = wrap_angle((d[0] as f64).atan2(d[2] as f64) as f32);
        self.rotation = a;
        self.desired_rotation = a;
    }
    /// `Update(ms)`; returns (eye, look-at target).
    pub fn update(&mut self, ms: i32) -> ([f32; 3], [f32; 3]) {
        if self.rotation_active {
            soft(&mut self.rotation, self.desired_rotation, &mut self.rotation_motion, ms);
            if self.rotation_motion[0] == 0. {
                self.rotation_active = false;
            }
        }
        let theta = self.rotation;
        soft(&mut self.position_offset[1], self.desired_position_offset[1], &mut self.position_motion, ms);
        soft(&mut self.target_offset[1], self.desired_target_offset[1], &mut self.target_motion, ms);
        soft(&mut self.back, self.desired_back, &mut self.back_motion, ms);
        let (s, c) = (theta.sin(), theta.cos());
        let target = [self.start[0] + self.distance * s, self.start[1] + self.target_offset[1], self.start[2] + self.distance * c];
        let eye = [target[0] - self.back * s, self.start[1] + self.position_offset[1], target[2] - self.back * c];
        (eye, target)
    }
}
