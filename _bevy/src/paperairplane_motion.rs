//! Recovered PaperAirplane frame/input/startup helpers; physics is a host boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    pub target_count: i32,
    pub finished: bool,
    pub target_timer: i32,
    pub dashing: bool,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub max_horizontal_speed: f32,
    pub indicator_state: i32,
    pub indicator_timer: i32,
    pub controller: i32,
    pub audio_handles: [i32; 2],
    pub catch_up: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    ChaseMode { enabled: bool, milliseconds: i32 },
    CreateDashFx { position: [f32; 3] },
    DestroyDashFx { milliseconds: i32 },
    Audio { event: i32, handles: [i32; 2] },
    SetLinearVelocity([f32; 3]),
}
/// Synchronous services retain the original query/effect ordering. Implementors
/// may update host state while processing effects before subsequent queries.
pub trait DashServices {
    fn bind_target(&mut self, _index: usize) {}
    fn resolve_controller(&mut self, _controller: i32) {}
    fn dash_event(&mut self, controller: i32) -> bool;
    fn target_position(&mut self) -> [f32; 3];
    fn linear_velocity(&mut self) -> [f32; 3];
    fn emit(&mut self, state: &mut Motion, effect: Effect);
}
struct Snapshots {
    event: bool,
    target: [f32; 3],
    velocity: [f32; 3],
    effects: Vec<Effect>,
}
impl DashServices for Snapshots {
    fn dash_event(&mut self, _: i32) -> bool {
        self.event
    }
    fn target_position(&mut self) -> [f32; 3] {
        self.target
    }
    fn linear_velocity(&mut self) -> [f32; 3] {
        self.velocity
    }
    fn emit(&mut self, _: &mut Motion, effect: Effect) {
        self.effects.push(effect);
    }
}
impl Motion {
    /// Complete 0x80375fe0. The selected target and current rigid-body velocity
    /// are values returned by external owners, not a replacement physics model.
    /// Snapshots must match native sampling after the initial presentation calls.
    /// This pure projection assumes those calls do not mutate target/plane physics.
    /// A host with mutating/reentrant presentation services must split that phase
    /// and resample before applying the returned velocity; never use an old frame.
    pub fn update_dashing(
        &mut self,
        milliseconds: i32,
        dash_event: bool,
        target_position: [f32; 3],
        rigid_body_velocity: [f32; 3],
    ) -> Vec<Effect> {
        let mut services = Snapshots {
            event: dash_event,
            target: target_position,
            velocity: rigid_body_velocity,
            effects: Vec::new(),
        };
        self.update_dashing_with_services(milliseconds, &mut services);
        services.effects
    }
    pub fn update_dashing_with_services(
        &mut self,
        milliseconds: i32,
        services: &mut impl DashServices,
    ) {
        if self.target_count == 0 || self.finished {
            return;
        }
        services.resolve_controller(self.controller);
        if self.target_timer == 500 && !self.dashing {
            self.dashing = services.dash_event(self.controller);
            if self.dashing {
                services.emit(
                    self,
                    Effect::ChaseMode {
                        enabled: true,
                        milliseconds: 1000,
                    },
                );
                services.emit(
                    self,
                    Effect::CreateDashFx {
                        position: self.position,
                    },
                );
                services.emit(self, Effect::DestroyDashFx { milliseconds: 2500 });
                self.set_indicator_active(false);
                services.emit(
                    self,
                    Effect::Audio {
                        event: 55,
                        handles: self.audio_handles,
                    },
                );
            }
        }
        if self.position[0] < services.target_position()[0] {
            self.dashing = false;
        }
        if !self.dashing {
            return;
        }
        let target_position = services.target_position();
        let d = std::array::from_fn::<_, 3, _>(|i| target_position[i] - self.position[i]);
        let length = (d[2].mul_add(d[2], d[0].mul_add(d[0], d[1] * d[1])) as f64).sqrt() as f32;
        let inverse = 1.0 / length;
        // The extra native Mult call receives f1=1.0, preserving this vector.
        let acceleration = std::array::from_fn::<_, 3, _>(|i| d[i] * inverse);
        let rigid_body_velocity = services.linear_velocity();
        let scale = 0.08_f32 * milliseconds as f32;
        self.velocity = std::array::from_fn(|i| rigid_body_velocity[i] + acceleration[i] * scale);
        self.velocity[0] = native_clamp(
            self.velocity[0],
            -self.max_horizontal_speed,
            self.max_horizontal_speed,
        );
        self.velocity[1] = native_clamp(self.velocity[1], -2.5, 2.5);
        self.velocity[2] = native_clamp(self.velocity[2], -2.5, 2.5);
        services.emit(self, Effect::SetLinearVelocity(self.velocity));
    }
    /// Complete 0x80376bfc; threshold 10 and maximum multiplier 1.5.
    pub fn update_catch_up(&mut self, distance: f32) {
        self.catch_up = if distance > 10.0 {
            native_clamp(
                0.02_f32.mul_add(distance - 10.0, 1.0),
                f32::NEG_INFINITY,
                1.5,
            )
        } else {
            1.0
        };
    }
    /// Complete 0x8037846c. Unknown/out-of-range states remain unchanged.
    pub fn set_indicator_active(&mut self, active: bool) {
        if (0..2).contains(&self.indicator_state) && !active {
            self.indicator_state = 2;
            self.indicator_timer = 0;
        } else if (2..4).contains(&self.indicator_state) && active {
            self.indicator_state = 1;
            self.indicator_timer = 0;
        }
    }
}
fn native_clamp(value: f32, low: f32, high: f32) -> f32 {
    let upper = if high < value { high } else { value };
    if low > upper { low } else { upper }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub position: [f32; 3],
    pub finished: bool,
    pub controller: i32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TargetLock {
    pub selected: usize,
    pub timers: [i32; 3],
    pub particle: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub enum TargetEffect {
    StartSound {
        controller: i32,
        event: i32,
        volume: i32,
    },
    StopSound {
        controller: i32,
        event: i32,
    },
    GetParticle(u32),
    ParticlePosition([f32; 3]),
    ParticleScale(f32),
}
pub trait TargetServices {
    fn target(&mut self, index: usize) -> Target;
    fn loop_playing(&mut self, controller: i32, event: i32) -> bool;
    fn emit(&mut self, motion: &mut Motion, lock: &mut TargetLock, effect: TargetEffect);
}
/// Complete original three-box targeting predicate at 0x80376360.
/// The argument is target position minus this plane's position.
pub fn is_in_targeting_range(delta: [f32; 3]) -> bool {
    let x = [5.5_f32, 11.5, 20.0];
    let yz = [0.9_f32, 0.75, 0.6];
    (0..3).any(|i| {
        delta[0] <= -1.0
            && delta[0] >= -x[i]
            && delta[1] <= yz[i]
            && delta[1] >= -yz[i]
            && delta[2] <= yz[i]
            && delta[2] >= -yz[i]
    })
}
impl TargetLock {
    /// Complete 0x80375d54. Timers/index writes and service queries follow native
    /// order, including the current-frame comparison against target zero's timer.
    pub fn update(
        &mut self,
        motion: &mut Motion,
        milliseconds: i32,
        services: &mut impl TargetServices,
    ) {
        if motion.target_count == 0 || motion.finished {
            return;
        }
        assert!(
            (1..=3).contains(&motion.target_count),
            "original target array has three slots"
        );
        if !motion.dashing {
            self.selected = 0;
            for i in 0..motion.target_count as usize {
                let target = services.target(i);
                let delta = std::array::from_fn(|j| target.position[j] - motion.position[j]);
                if is_in_targeting_range(delta) && !target.finished {
                    self.timers[i] = self.timers[i].wrapping_add(milliseconds);
                    if self.timers[i] > 500 {
                        self.timers[i] = 500;
                    }
                } else {
                    self.timers[i] = self.timers[i].wrapping_sub(milliseconds);
                    if self.timers[i] < 0 {
                        self.timers[i] = 0;
                    }
                }
                if self.timers[i] > 0 && self.timers[i] > self.timers[self.selected] {
                    self.selected = i;
                }
            }
        }
        let controller = services.target(self.selected).controller;
        if self.timers[self.selected] == 500 {
            if !services.loop_playing(controller, 36) {
                let controller = services.target(self.selected).controller;
                services.emit(
                    motion,
                    self,
                    TargetEffect::StartSound {
                        controller,
                        event: 36,
                        volume: 4096,
                    },
                );
            }
            if !motion.dashing {
                motion.set_indicator_active(true);
            }
        } else {
            if services.loop_playing(controller, 36) {
                let controller = services.target(self.selected).controller;
                services.emit(
                    motion,
                    self,
                    TargetEffect::StopSound {
                        controller,
                        event: 36,
                    },
                );
            }
            motion.set_indicator_active(false);
        }
        services.emit(motion, self, TargetEffect::GetParticle(self.particle));
        let position = services.target(self.selected).position;
        services.emit(motion, self, TargetEffect::ParticlePosition(position));
        services.emit(
            motion,
            self,
            TargetEffect::ParticleScale(self.timers[self.selected] as f32 / 500.0),
        );
        motion.target_timer = self.timers[self.selected];
    }
}

pub type Matrix = [f32; 16];
/// Additional field ownership for the complete retail UpdateInput body.
#[derive(Clone, Debug, PartialEq)]
pub struct InputState {
    pub state: i32,                      // +4
    pub filtered_acceleration: [f32; 3], // +60
    pub force_mass: f32,                 // +70
    pub orientation: Matrix,             // +100
    pub respawn_requested: bool,         // +141
    pub powerup_kind: i32,               // +144
    pub recovery_ready: bool,            // +149
    pub recovery_remaining: i32,         // +14c
    pub recovery_duration: i32,          // +150
    pub horizontal_control: bool,        // +154
    pub horizontal_delay: i32,           // +158
    pub vertical_control: bool,          // +15c
    pub vertical_delay: i32,             // +160
    pub speed_factor: f32,               // +174
    pub respawn_target: [f32; 3],        // +1d0
    pub respawn_start: [f32; 3],         // +1e0
    pub respawn_elapsed: i32,            // +1f0
    pub angles: [f32; 2],                // +204/+208
    pub recovery_angles: [f32; 2],       // +20c/+210
    pub cruise_speed: f32,               // +230
    pub lateral_speed: f32,              // +23c
    pub upward_speed: f32,               // +240
    pub downward_speed: f32,             // +244
}
#[derive(Clone, Debug, PartialEq)]
pub enum InputEffect {
    SetMatrix(Matrix),
    SetLinearVelocity([f32; 3]),
    SetAngularVelocity([f32; 3]),
    ApplyForce { time: f32, force: [f32; 3] },
}
pub trait InputServices {
    fn select_controller(&mut self, controller: i32);
    fn accelerometers(&mut self) -> [f32; 3];
    fn linear_velocity(&mut self) -> [f32; 3];
    fn angular_velocity(&mut self) -> [f32; 3];
    fn position(&mut self) -> [f32; 3];
    fn matrix(&mut self) -> Matrix;
    fn x_direction(&mut self) -> [f32; 3];
    fn z_direction(&mut self) -> [f32; 3];
    fn mass(&mut self) -> f32;
    fn nearest_respawn(&mut self, position: [f32; 3]) -> [f32; 3];
    fn emit(&mut self, state: &mut InputState, motion: &mut Motion, effect: InputEffect);
}
fn angle_wrap(mut x: f32) -> f32 {
    let tau = f32::from_bits(0x40c90fdb);
    while x >= tau {
        x -= tau;
    }
    while x < 0.0 {
        x += tau;
    }
    x
}
/// Complete InterpolateAngle at 0x80375b94, including its trigonometric
/// direction selection and twice-wrapped result. libc atan2 is modeled in f64.
pub fn interpolate_angle(current: f32, target: f32, fraction: f32, maximum: f32) -> f32 {
    let (s, c) = crate::character_input::ea_sin_cos(target);
    let direction = angle_wrap((s as f64).atan2(c as f64) as f32);
    let plus = angle_wrap(direction - current) < f32::from_bits(0x40490fdb);
    let tau = f32::from_bits(0x40c90fdb);
    let distance = if plus {
        if target > current {
            target - current
        } else {
            tau - (current - target)
        }
    } else if target > current {
        tau - (target - current)
    } else {
        current - target
    };
    let distance = angle_wrap(distance);
    let step = fraction * distance;
    let step = if step > maximum { maximum } else { step };
    angle_wrap(angle_wrap(if plus {
        step + current
    } else {
        current - step
    }))
}
fn rotation_x(angle: f32) -> Matrix {
    let (s, c) = crate::character_input::ea_sin_cos(angle);
    [1., 0., 0., 0., 0., c, s, 0., 0., -s, c, 0., 0., 0., 0., 1.]
}
fn rotation_z(angle: f32) -> Matrix {
    let (s, c) = crate::character_input::ea_sin_cos(angle);
    [c, s, 0., 0., -s, c, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
}
fn lerp(a: [f32; 3], b: [f32; 3], fraction: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction)
}
/// Complete original course GetNearestRespawnPoint at 0x8037a144.
pub fn nearest_respawn_point(position: [f32; 3], points: &[[f32; 3]]) -> [f32; 3] {
    let mut result = [0., 2., 0.];
    let mut distance = -position[0];
    for point in points {
        if -point[0] < -position[0] {
            let candidate = -(position[0] - point[0]);
            if candidate < distance {
                distance = candidate;
                result = *point;
            }
        }
    }
    result
}
/// GetNearestCheckpoint (0x8037a060) has identical arithmetic, scanning the
/// course checkpoint array rather than the respawn-point array.
pub fn nearest_checkpoint_point(position: [f32; 3], points: &[[f32; 3]]) -> [f32; 3] {
    nearest_respawn_point(position, points)
}
impl InputState {
    fn steering_velocity(&self, motion: &mut Motion, desired: [f32; 3]) {
        if self.vertical_control {
            motion.velocity[0] = 0.003_f32.mul_add(desired[1], motion.velocity[0]);
            if desired[1].abs() > 0.0 {
                motion.velocity[1] =
                    0.10000002384185791_f32.mul_add(desired[1], 0.9_f32 * motion.velocity[1]);
            }
        }
        motion.velocity[2] =
            0.10000002384185791_f32.mul_add(desired[2], 0.9_f32 * motion.velocity[2]);
    }
    fn desired_angles(&self, motion: &Motion) -> [f32; 2] {
        let half_pi = f32::from_bits(0x3fc90fdb);
        let x = (half_pi * (motion.velocity[2] / self.lateral_speed)) * 0.5;
        let y = if motion.velocity[1] > self.downward_speed {
            self.downward_speed
        } else {
            motion.velocity[1]
        };
        let z = (half_pi * (-y / self.downward_speed)) * 0.25;
        [angle_wrap(x), angle_wrap(z)]
    }
    fn oriented_matrix(&self, position: [f32; 3], angles: [f32; 2]) -> Matrix {
        let m = crate::area_transform::multiply(&self.orientation, &rotation_x(angles[0]));
        let m = crate::area_transform::multiply(&m, &rotation_z(angles[1]));
        crate::area_transform::multiply(&m, &crate::area_transform::translation(position))
    }
    /// Complete original 0x80374fd8; synchronous engine services preserve the
    /// original fresh queries, call order and bugs rather than simulate Havok.
    pub fn update_input(
        &mut self,
        motion: &mut Motion,
        milliseconds: i32,
        services: &mut impl InputServices,
    ) {
        services.select_controller(motion.controller);
        motion.velocity = services.linear_velocity();
        if self.respawn_requested {
            self.respawn_requested = false;
            self.state = 6;
            self.powerup_kind = 0;
            self.recovery_ready = true;
            self.vertical_control = true;
            self.horizontal_control = true;
            self.recovery_remaining = 0;
            self.horizontal_delay = 0;
            self.vertical_delay = 0;
            self.respawn_target = services.nearest_respawn(motion.position);
            self.respawn_elapsed = 0;
            self.respawn_start = motion.position;
            let matrix = crate::area_transform::multiply(
                &self.orientation,
                &crate::area_transform::translation(motion.position),
            );
            services.emit(self, motion, InputEffect::SetMatrix(matrix));
            services.emit(self, motion, InputEffect::SetLinearVelocity([0.; 3]));
            services.emit(self, motion, InputEffect::SetAngularVelocity([0.; 3]));
        }
        match self.state {
            0 | 3 => {
                let mut acc = services.accelerometers();
                let factor = milliseconds as f32 / 33.0;
                acc = acc.map(|x| x * factor);
                if acc[0] > 0.0 && acc[2] < 0.0 {
                    acc[0] = 54.0;
                }
                if acc[0] < 0.0 && acc[2] < 0.0 {
                    acc[0] = -54.0;
                }
                let prior = self.filtered_acceleration.map(|x| x * (1.0 - factor));
                self.filtered_acceleration = std::array::from_fn(|i| acc[i] + prior[i]);
                let y = -self.filtered_acceleration[1] * 0.015625;
                let z = -self.filtered_acceleration[0] * 0.015625;
                let desired = [
                    0.,
                    y * if y > 0. {
                        self.upward_speed
                    } else {
                        self.downward_speed
                    },
                    z * self.lateral_speed,
                ];
                self.steering_velocity(motion, desired);
                self.angles = self.desired_angles(motion);
                let position = services.position();
                let matrix = self.oriented_matrix(position, self.angles);
                services.emit(self, motion, InputEffect::SetMatrix(matrix));
                if !motion.dashing {
                    let speed = -motion.velocity[0];
                    if speed < self.cruise_speed - 0.05 {
                        motion.velocity[0] =
                            0.0055_f32.mul_add(-(milliseconds as f32), motion.velocity[0]);
                    } else if speed > 0.05 + self.cruise_speed {
                        motion.velocity[0] =
                            -0.002_f32.mul_add(-(milliseconds as f32), -motion.velocity[0]);
                    }
                }
                services.emit(
                    self,
                    motion,
                    InputEffect::SetLinearVelocity(motion.velocity),
                );
                services.emit(self, motion, InputEffect::SetAngularVelocity([0.; 3]));
            }
            4 => {
                services.emit(
                    self,
                    motion,
                    InputEffect::SetLinearVelocity(motion.velocity),
                );
                let mass = services.mass();
                services.emit(
                    self,
                    motion,
                    InputEffect::ApplyForce {
                        time: milliseconds as f32 / 1000.,
                        force: [0., -2.0 * mass, 0.],
                    },
                );
            }
            5 => {
                if self.recovery_ready {
                    self.recovery_ready = false;
                    self.recovery_remaining = 400;
                    self.recovery_duration = 400;
                    self.state = 2;
                }
            }
            6 => {
                self.respawn_elapsed = self.respawn_elapsed.wrapping_add(milliseconds);
                self.speed_factor = 0.3;
                let mut fraction = self.respawn_elapsed as f32 / 800.;
                if fraction > 1. {
                    self.state = 0;
                    fraction = 1.;
                    let matrix = crate::area_transform::multiply(
                        &self.orientation,
                        &crate::area_transform::translation(motion.position),
                    );
                    motion.velocity[0] = -self.cruise_speed;
                    services.emit(self, motion, InputEffect::SetMatrix(matrix));
                    services.emit(
                        self,
                        motion,
                        InputEffect::SetLinearVelocity(motion.velocity),
                    );
                }
                motion.position = lerp(self.respawn_start, self.respawn_target, fraction);
            }
            2 => {
                let fraction = (self.recovery_duration as f32 - self.recovery_remaining as f32)
                    / self.recovery_duration as f32;
                let fraction = if fraction > 1. { 1. } else { fraction };
                let angular = services.angular_velocity();
                let _unused = lerp(angular, [0.; 3], fraction);
                // The executable passes the zero source, not the interpolated result.
                services.emit(self, motion, InputEffect::SetAngularVelocity([0.; 3]));
                self.steering_velocity(motion, [0.; 3]);
                let xdir = services.x_direction();
                let zdir = services.z_direction();
                let elapsed = self.recovery_duration.wrapping_sub(self.recovery_remaining);
                if elapsed == 0 || elapsed == milliseconds {
                    self.recovery_angles = [
                        angle_wrap(
                            f32::from_bits(0x3fc90fdb)
                                + (zdir[2] as f64).atan2(zdir[1] as f64) as f32,
                        ),
                        angle_wrap(
                            f32::from_bits(0x40490fdb)
                                + (xdir[1] as f64).atan2(xdir[0] as f64) as f32,
                        ),
                    ];
                }
                let desired = self.desired_angles(motion);
                let angles = std::array::from_fn(|i| {
                    interpolate_angle(
                        self.recovery_angles[i],
                        desired[i],
                        fraction,
                        f32::from_bits(0x40c90fdb),
                    )
                });
                if (angle_wrap(angle_wrap(angles[0] - desired[0])).abs() < 0.3
                    && angle_wrap(angle_wrap(angles[1] - desired[1])).abs() < 0.4)
                    || fraction == 1.
                {
                    self.recovery_duration = 0;
                    self.recovery_ready = true;
                    self.state = 0;
                }
                self.angles = angles.map(angle_wrap);
                let position = services.position();
                let matrix = self.oriented_matrix(position, angles);
                let _unused = services.matrix();
                services.emit(self, motion, InputEffect::SetMatrix(matrix));
            }
            _ => {}
        }
        if motion.position[1] > 3. {
            let scale = -0.02_f32 * (1.0 - (3.5 - motion.position[1]) / 0.5);
            let force = [
                0.0 * self.force_mass,
                scale * self.force_mass,
                0.0 * self.force_mass,
            ];
            services.emit(
                self,
                motion,
                InputEffect::ApplyForce {
                    time: milliseconds as f32,
                    force,
                },
            );
        }
    }
}

/// ELF table at 0x80441f14: six float properties and one integer property.
pub const PLANE_PROPERTIES: [[u32; 7]; 5] = [
    [
        0x40d00000, 0x41180000, 0x40800000, 0x3fa00000, 0x3fd33333, 0x3fd33333, 3,
    ],
    [
        0x40d00000, 0x41200000, 0x40800000, 0x400ccccd, 0x4019999a, 0x4019999a, 3,
    ],
    [
        0x40d00000, 0x41300000, 0x40800000, 0x400ccccd, 0x4019999a, 0x4019999a, 3,
    ],
    [
        0x40e80000, 0x41400000, 0x40a00000, 0x400ccccd, 0x4019999a, 0x4019999a, 3,
    ],
    [
        0x41080000, 0x41500000, 0x40a00000, 0x40200000, 0x40266666, 0x40266666, 3,
    ],
];
#[derive(Clone, Debug)]
pub struct Holder {
    pub matrix: Matrix,
    pub position: [f32; 3],
    pub gender: i32, // native Character +1e8
}
/// Complete constructor's native byte writes. Unspecified bytes deliberately
/// retain caller contents (state, matrices and several tuning fields are absent).
pub fn construct_plane_image(image: &mut [u8; 0x284], bad_guid: u32) {
    fn word(image: &mut [u8; 0x284], offset: usize, value: u32) {
        image[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    word(image, 0, 0x804db86c);
    for offset in [
        8, 0x74, 0x140, 0x141, 0x148, 0x149, 0x154, 0x15c, 0x164, 0x1c4, 0x21c, 0x24c, 0x254, 0x26c,
    ] {
        image[offset] = 0;
    }
    for offset in [0x74, 0x149, 0x154, 0x15c] {
        image[offset] = 1;
    }
    for offset in [
        0xc, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x28, 0x2c, 0x34, 0x40, 0x44, 0x48, 0x50, 0x54, 0x58,
        0x60, 0x64, 0x68, 0x70, 0x78, 0x7c, 0x144, 0x14c, 0x158, 0x160, 0x168, 0x16c, 0x170, 0x17c,
        0x184, 0x188, 0x18c, 0x190, 0x194, 0x198, 0x19c, 0x1a0, 0x1b0, 0x1b4, 0x1c8, 0x204, 0x208,
        0x20c, 0x210, 0x218, 0x220, 0x224, 0x228, 0x22c, 0x250, 0x25c, 0x264, 0x268, 0x270, 0x274,
        0x278, 0x27c, 0x1a4, 0x1a8, 0x1ac, 0x1b8, 0x1bc, 0x1c0,
    ] {
        word(image, offset, 0);
    }
    for offset in [0x24, 0x180] {
        word(image, offset, u32::MAX);
    }
    for offset in [0x174, 0x178] {
        word(image, offset, 1_f32.to_bits());
    }
    for offset in [0x1f4, 0x1f8, 0x1fc, 0x200, 0x260] {
        word(image, offset, bad_guid);
    }
    word(image, 0x214, 1);
    word(image, 0x258, 3);
    word(image, 0x280, 100);
}
/// Additional retail fields used by the four frame children. Zero/default here
/// is a storage convenience, not a recovered constructor or launch policy.
#[derive(Clone, Debug, Default)]
pub struct FrameChildren {
    pub contacts: [u32; 8],         // +184..1a0
    pub dash_particle: u32,         // +1c8
    pub targets: [u32; 3],          // native +1a4 pointer roster
    pub slow_active: bool,          // +164
    pub slow_remaining: i32,        // +168
    pub elapsed: [i32; 2],          // +16c/+170
    pub smoothed_speed_factor: f32, // +178
    pub boost_available: i32,       // +214
    pub boost_remaining: i32,       // +218
    pub boost_count: i32,           // +228
    pub rumble_remaining: i32,      // +268
    pub boost_hud_remaining: i32,   // +278
    pub trail_particle: u32,        // +1f4
    pub holder_particle: u32,       // +1fc
    pub extra_particle: u32,        // +200
    pub ready_particle: u32,        // +260
}
#[derive(Clone, Debug, Default)]
pub struct StartupState {
    pub initialized: bool,
    pub cached_main: u32,
    pub cached_shadow: u32,
    pub shadow_entity: u32,
    pub model_id: i32,
    pub shadow_model_id: i32,
    pub texture_id: i32,
    pub physics_system: i32,
    pub rigid_body: u32,
    pub collision_listener: u32,
    pub camera: u32,
    pub course: u32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct StartupReply {
    pub handle: u32,
    pub asset_id: i32,
}
#[derive(Clone, Debug, PartialEq)]
pub enum StartupRequest {
    Particle {
        asset: String,
        position: [f32; 3],
    },
    Texture(String),
    Model(String),
    ModelTextures {
        model: u32,
        texture: u32,
    },
    Allocate {
        bytes: u32,
        tag: &'static str,
    },
    CachedModel(u32),
    CachedScale {
        cached: u32,
        model: u32,
    },
    ShadowEntity {
        handle: u32,
        model: u32,
    },
    RegisterShadow(u32),
    LoadPhysics {
        asset: String,
        matrix: Matrix,
        flags: u32,
        option: bool,
    },
    GenerateBody {
        system: i32,
        shape: u32,
        option: i32,
    },
    AddBody(u32),
    Quality {
        body: u32,
        quality: i32,
    },
    UserData {
        body: u32,
        kind: i32,
    },
    CollisionListener(u32),
    Azimuth {
        controller: i32,
        dare: u8,
    },
    Volume {
        controller: i32,
        dare: u8,
    },
}
/// Synchronous native asset/render/physics creation obligations. Handles remain
/// opaque; this port does not fabricate model geometry or a rigid-body solver.
pub trait StartupServices: FrameServices {
    fn startup(
        &mut self,
        startup: &mut StartupState,
        frame: &mut FrameState,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        request: StartupRequest,
    ) -> StartupReply;
}
#[derive(Clone, Debug)]
pub struct InitializeRequest {
    pub position: [f32; 3],
    pub controller: i32,
    pub plane_type: usize,
    pub camera: u32,
    pub course: u32,
    pub checkpoint_active: bool,
    pub boost_factor: f32,
}
#[derive(Clone, Debug)]
pub struct FrameState {
    pub checkpoint_duration_seconds: i32, // course +db8
    pub children: FrameChildren,
    pub grabbed: bool,
    pub holder: Option<Holder>,
    pub attachment: Option<Matrix>,
    pub plane_type: usize,
    pub third_speed: f32,
    pub integer_property: u32,
    pub boost_factor: f32,
    pub ready: bool,
    pub in_sprinkler: bool,
    pub crashed: bool,
    pub crash_phase: i32,
    pub crash_elapsed: i32,
    pub checkpoint_active: bool,
    pub checkpoint_remaining: i32,
    pub checkpoint_progress: f32,
    pub checkpoint_flash: bool,
    pub checkpoint_flash_remaining: i32,
    pub render_matrix: Matrix,
    pub physical_matrix: Matrix,
    pub area_radius: f32,
    pub curvature_disabled: bool,
    pub start_camera_position: [f32; 3],
    pub start_camera_target: [f32; 3],
    pub bad_particle: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStage {
    Particles,
    Counters,
    Sprinkler,
    ReadyIndicator,
}
#[derive(Clone, Debug, PartialEq)]
pub enum FrameEffect {
    ParticleViewportEnabled {
        particle: u32,
        controller: i32,
        enabled: bool,
    },
    GameAudio {
        event: i32,
        handles: [i32; 2],
    },
    StartGameAudio {
        event: i32,
        handles: [i32; 2],
    },
    ChaseMode {
        enabled: bool,
        milliseconds: i32,
    },
    Boost {
        controller: i32,
        value: i32,
    },
    Rumble {
        milliseconds: i32,
        strength: f32,
    },
    ParticleEnable {
        particle: u32,
        enabled: bool,
    },
    ParticleViewport {
        particle: u32,
        controller: i32,
    },
    ParticlePosition {
        particle: u32,
        position: [f32; 3],
    },
    ParticleScale {
        particle: u32,
        scale: f32,
    },
    HudAudio {
        event: i32,
        handles: [i32; 2],
    },
    WiiSound {
        controller: i32,
        event: i32,
        volume: i32,
    },
    DestroyParticle {
        particle: u32,
        milliseconds: i32,
    },
    DisplayPoints {
        controller: i32,
        points: i32,
    },
    Checkpoint(bool),
    Walls {
        controller: i32,
        position: [f32; 3],
    },
    CameraPosition([f32; 3]),
    CameraRoll(f32),
    CameraOffsets {
        position: [f32; 3],
        target: [f32; 3],
    },
    AltitudeAudio {
        event: i32,
        value: i32,
        parameters: [i32; 2],
    },
}
/// All four frame stages default to recovered bodies. Override `stage` only for
/// isolated orchestration comparisons. Particle operations must be synchronous;
/// lookup/create return the original GUID identity and existence contract.
pub trait FrameServices: DashServices + InputServices + TargetServices {
    fn stage(
        &mut self,
        frame: &mut FrameState,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        stage: FrameStage,
        milliseconds: i32,
    ) where
        Self: Sized,
    {
        frame.update_stage(input, motion, lock, stage, milliseconds, self);
    }
    fn particle_exists(&mut self, particle: u32) -> bool;
    fn create_particle(&mut self, asset: &'static str, position: [f32; 3]) -> u32;
    fn game_audio_active(&mut self, event: i32) -> bool;
    fn frame_emit(
        &mut self,
        frame: &mut FrameState,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        effect: FrameEffect,
    );
    fn ready_event(&mut self, action: i32) -> bool;
    fn nearest_checkpoint(&mut self, position: [f32; 3]) -> [f32; 3];
    fn dare_type(&mut self) -> u8;
}
impl FrameState {
    /// Complete Initialize dispatch, not a constructor/reset substitute.
    pub fn initialize(
        &mut self,
        startup: &mut StartupState,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        request: InitializeRequest,
        services: &mut impl StartupServices,
    ) {
        if startup.initialized {
            return;
        }
        motion.position = request.position;
        self.boost_factor = request.boost_factor;
        let asset = match request.plane_type {
            1 => "pa_plane_glider",
            2 => "pa_plane_stealth",
            3 => "pa_plane_red",
            4 => "pa_plane_fighter",
            _ => "pa_plane",
        };
        motion.controller = request.controller;
        startup.camera = request.camera;
        startup.course = request.course;
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Texture(format!("{asset}.gsh")),
        );
        startup.texture_id = reply.asset_id;
        let texture = reply.handle;
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Model(format!("{asset}.o")),
        );
        startup.model_id = reply.asset_id;
        let model = reply.handle;
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::ModelTextures { model, texture },
        );
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Model(format!("{asset}_shadow.o")),
        );
        startup.shadow_model_id = reply.asset_id;
        let shadow = reply.handle;
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Allocate {
                bytes: 0x4c,
                tag: "Ren::CachedModel",
            },
        );
        let handle = reply.handle;
        if handle != 0 {
            services.startup(
                startup,
                self,
                input,
                motion,
                lock,
                StartupRequest::CachedModel(handle),
            );
        }
        startup.cached_main = handle;
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Allocate {
                bytes: 0x4c,
                tag: "Ren::CachedModel",
            },
        );
        let handle = reply.handle;
        if handle != 0 {
            services.startup(
                startup,
                self,
                input,
                motion,
                lock,
                StartupRequest::CachedModel(handle),
            );
        }
        startup.cached_shadow = handle;
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::CachedScale {
                cached: startup.cached_main,
                model,
            },
        );
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::CachedScale {
                cached: startup.cached_shadow,
                model: shadow,
            },
        );
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Allocate {
                bytes: 0x4c,
                tag: "PaperAirplaneShadowRenderEntity",
            },
        );
        let handle = reply.handle;
        if handle != 0 {
            services.startup(
                startup,
                self,
                input,
                motion,
                lock,
                StartupRequest::ShadowEntity {
                    handle,
                    model: shadow,
                },
            );
        }
        startup.shadow_entity = handle;
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::RegisterShadow(startup.shadow_entity),
        );
        let matrix = crate::area_transform::translation(motion.position);
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::LoadPhysics {
                asset: asset.into(),
                matrix,
                flags: 0x16,
                option: false,
            },
        );
        startup.physics_system = reply.handle as i32;
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::GenerateBody {
                system: startup.physics_system,
                shape: 0,
                option: 0,
            },
        );
        startup.rigid_body = reply.handle;
        InputServices::emit(services, input, motion, InputEffect::SetMatrix(matrix));
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::AddBody(startup.rigid_body),
        );
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Quality {
                body: startup.rigid_body,
                quality: 4,
            },
        );
        services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::UserData {
                body: startup.rigid_body,
                kind: 15,
            },
        );
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Allocate {
                bytes: 0x14,
                tag: "Minigame::PaperAirplaneCollisionListener",
            },
        );
        let handle = reply.handle;
        if handle != 0 {
            services.startup(
                startup,
                self,
                input,
                motion,
                lock,
                StartupRequest::CollisionListener(handle),
            );
        }
        startup.collision_listener = handle;
        input.orientation = rotation_y(f32::from_bits(0x40490fdb));
        let matrix = crate::area_transform::multiply(
            &input.orientation,
            &crate::area_transform::translation(motion.position),
        );
        InputServices::emit(services, input, motion, InputEffect::SetMatrix(matrix));
        startup.initialized = true;
        input.recovery_ready = true;
        motion.velocity = [0.; 3];
        InputServices::emit(
            services,
            input,
            motion,
            InputEffect::SetLinearVelocity(motion.velocity),
        );
        input.force_mass = InputServices::mass(services);
        self.grabbed = true;
        self.crash_phase = 5;
        let mut position = motion.position;
        position[0] += -0.2;
        position[1] += -0.05;
        self.children.trail_particle = services.create_particle("pg_wind", position);
        let particle = self.children.trail_particle;
        assert!(services.particle_exists(particle), "native trail FX");
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle,
                enabled: false,
            },
        );
        lock.particle = services.create_particle("pg_pa_lockon", motion.position);
        let particle = lock.particle;
        assert!(services.particle_exists(particle), "native lock FX");
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleViewport {
                particle,
                controller: motion.controller,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleScale {
                particle,
                scale: 0.,
            },
        );
        let mut position = motion.position;
        position[1] += 0.4;
        position[2] += -0.05;
        // Native sprintf permits the controller-derived suffix; the asset API
        // accepts owned strings for this one dynamic particle name.
        self.children.holder_particle = services
            .startup(
                startup,
                self,
                input,
                motion,
                lock,
                StartupRequest::Particle {
                    asset: format!("pg_pa_indicator_{}", motion.controller.wrapping_add(1)),
                    position,
                },
            )
            .handle;
        let particle = self.children.holder_particle;
        assert!(services.particle_exists(particle), "native holder FX");
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleViewport {
                particle,
                controller: motion.controller,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleScale {
                particle,
                scale: 0.,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle,
                enabled: true,
            },
        );
        self.render_matrix = crate::area_transform::AreaTransform {
            radius: self.area_radius,
            disabled: self.curvature_disabled,
        }
        .model_matrix(motion.position);
        input.state = 1;
        self.in_sprinkler = false;
        self.children.boost_available = 1;
        self.children.boost_remaining = 0;
        self.checkpoint_active = request.checkpoint_active;
        self.checkpoint_progress = 0.;
        self.plane_type = request.plane_type;
        self.checkpoint_remaining = self.checkpoint_duration_seconds.wrapping_mul(1000);
        let p = PLANE_PROPERTIES[self.plane_type];
        input.cruise_speed = f32::from_bits(p[0]) * self.boost_factor;
        motion.max_horizontal_speed = f32::from_bits(p[1]) * self.boost_factor;
        self.third_speed = f32::from_bits(p[2]) * self.boost_factor;
        input.lateral_speed = f32::from_bits(p[3]);
        input.upward_speed = f32::from_bits(p[4]);
        input.downward_speed = f32::from_bits(p[5]);
        self.integer_property = p[6];
        self.children.boost_count = self.integer_property as i32;
        motion.catch_up = 1.;
        let dare = services.dare_type();
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Azimuth {
                controller: motion.controller,
                dare,
            },
        );
        motion.audio_handles[0] = reply.handle as i32;
        let reply = services.startup(
            startup,
            self,
            input,
            motion,
            lock,
            StartupRequest::Volume {
                controller: motion.controller,
                dare,
            },
        );
        motion.audio_handles[1] = reply.handle as i32;
    }
    /// Complete Reset body, including its pre-property-copy boost count read.
    pub fn reset(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        position: [f32; 3],
        services: &mut impl FrameServices,
    ) {
        if self.checkpoint_flash {
            services.frame_emit(self, input, motion, lock, FrameEffect::Checkpoint(false));
        }
        self.checkpoint_flash = false;
        motion.position = position;
        self.grabbed = true;
        self.holder = None;
        self.attachment = None;
        input.filtered_acceleration = [0.; 3];
        input.recovery_ready = true;
        input.recovery_remaining = 0;
        input.horizontal_control = true;
        input.horizontal_delay = 0;
        input.vertical_control = true;
        input.vertical_delay = 0;
        motion.velocity = [0.; 3];
        self.crashed = false;
        self.children.contacts = [0; 8];
        self.children.slow_active = false;
        self.children.slow_remaining = 0;
        self.crash_phase = 0;
        self.children.smoothed_speed_factor = 1.;
        input.speed_factor = 1.;
        input.powerup_kind = 0;
        input.respawn_requested = false;
        self.children.elapsed = [0; 2];
        motion.dashing = false;
        self.children.dash_particle = 0;
        lock.selected = 0;
        self.in_sprinkler = false;
        self.children.boost_available = 1;
        self.children.boost_remaining = 0;
        self.checkpoint_remaining = 0;
        self.checkpoint_progress = 0.;
        motion.finished = false;
        self.ready = false;
        motion.indicator_state = 3;
        motion.indicator_timer = 0;
        motion.catch_up = 0.;
        self.children.rumble_remaining = 0;
        self.checkpoint_flash_remaining = 0;
        self.crash_elapsed = 0;
        input.orientation = rotation_y(f32::from_bits(0x40490fdb));
        let matrix = crate::area_transform::multiply(
            &input.orientation,
            &crate::area_transform::translation(motion.position),
        );
        InputServices::emit(services, input, motion, InputEffect::SetMatrix(matrix));
        input.recovery_ready = true;
        motion.velocity = [0.; 3];
        InputServices::emit(
            services,
            input,
            motion,
            InputEffect::SetLinearVelocity(motion.velocity),
        );
        self.grabbed = true;
        self.crash_phase = 5;
        let trail = self.children.trail_particle;
        assert!(services.particle_exists(trail), "native mandatory trail FX");
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle: trail,
                enabled: false,
            },
        );
        if lock.particle == self.bad_particle {
            lock.particle = services.create_particle("pg_pa_lockon", motion.position);
        }
        let particle = lock.particle;
        assert!(services.particle_exists(particle), "native lock FX");
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleViewport {
                particle,
                controller: motion.controller,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleScale {
                particle,
                scale: 0.,
            },
        );
        let mut position = motion.position;
        position[1] += 0.4;
        position[2] += -0.05;
        let particle = self.children.holder_particle;
        assert!(
            services.particle_exists(particle),
            "native mandatory holder FX"
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticlePosition { particle, position },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleScale {
                particle,
                scale: 0.,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle,
                enabled: true,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleViewport {
                particle,
                controller: motion.controller,
            },
        );
        self.render_matrix = crate::area_transform::AreaTransform {
            radius: self.area_radius,
            disabled: self.curvature_disabled,
        }
        .model_matrix(motion.position);
        input.state = 1;
        self.children.boost_available = 1;
        self.in_sprinkler = false;
        self.children.boost_remaining = 0;
        self.checkpoint_progress = 0.;
        self.children.boost_count = self.integer_property as i32;
        self.checkpoint_remaining = self.checkpoint_duration_seconds.wrapping_mul(1000);
        let p = PLANE_PROPERTIES[self.plane_type];
        input.cruise_speed = f32::from_bits(p[0]) * self.boost_factor;
        motion.max_horizontal_speed = f32::from_bits(p[1]) * self.boost_factor;
        self.third_speed = f32::from_bits(p[2]) * self.boost_factor;
        input.lateral_speed = f32::from_bits(p[3]);
        input.upward_speed = f32::from_bits(p[4]);
        input.downward_speed = f32::from_bits(p[5]);
        self.integer_property = p[6];
        motion.finished = false;
        motion.catch_up = 1.;
        self.crashed = false;
        self.ready = false;
        motion.indicator_state = 3;
        motion.set_indicator_active(false);
    }
    /// Native Grab is unconditional: null holder remains a valid stored value.
    pub fn grab(
        &mut self,
        input: &mut InputState,
        holder: Option<Holder>,
        attachment: Option<Matrix>,
    ) {
        self.grabbed = true;
        self.holder = holder;
        self.attachment = attachment;
        input.state = 1;
    }
    /// Native AddTarget appends pointer identity only while count is below three.
    pub fn add_target(&mut self, motion: &mut Motion, target: u32) {
        if motion.target_count >= 3 {
            return;
        }
        self.children.targets[motion.target_count as usize] = target;
        motion.target_count = motion.target_count.wrapping_add(1);
    }
    /// Complete Throw 0x8037714c. Presence of the holder pointer is the gate,
    /// independent of grabbed/ready flags. Body setters execute synchronously.
    pub fn throw(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        services: &mut impl FrameServices,
    ) {
        if self.holder.is_none() {
            return;
        }
        input.state = 0;
        motion.velocity = [-input.cruise_speed, 0., 0.];
        InputServices::emit(
            services,
            input,
            motion,
            InputEffect::SetLinearVelocity(motion.velocity),
        );
        self.grabbed = false;
        let trail = self.children.trail_particle;
        assert!(services.particle_exists(trail), "native mandatory trail FX");
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle: trail,
                enabled: true,
            },
        );
        let holder = self.children.holder_particle;
        assert!(
            services.particle_exists(holder),
            "native mandatory holder FX"
        );
        for controller in 0..4 {
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::ParticleViewportEnabled {
                    particle: holder,
                    controller,
                    enabled: true,
                },
            );
        }
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleViewportEnabled {
                particle: holder,
                controller: motion.controller,
                enabled: false,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle: holder,
                enabled: true,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ChaseMode {
                enabled: true,
                milliseconds: 400,
            },
        );
        input.orientation = rotation_y(f32::from_bits(0x40490fdb));
        let matrix = crate::area_transform::multiply(
            &input.orientation,
            &crate::area_transform::translation(motion.position),
        );
        InputServices::emit(services, input, motion, InputEffect::SetMatrix(matrix));
        if !services.game_audio_active(54) {
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::StartGameAudio {
                    event: 54,
                    handles: [0, 100],
                },
            );
        }
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::GameAudio {
                event: 61,
                handles: motion.audio_handles,
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::WiiSound {
                controller: motion.controller,
                event: 28,
                volume: 4096,
            },
        );
        let event = if self
            .holder
            .as_ref()
            .expect("native holder pointer remains valid")
            .gender
            == 0
        {
            7
        } else {
            8
        };
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::GameAudio {
                event,
                handles: motion.audio_handles,
            },
        );
    }
    pub fn update_stage(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        stage: FrameStage,
        ms: i32,
        services: &mut impl FrameServices,
    ) {
        match stage {
            FrameStage::Counters => self.update_counters(input, motion, lock, ms, services),
            FrameStage::Sprinkler => self.update_sprinkler(input, motion, ms, services),
            FrameStage::Particles => self.update_particles(input, motion, lock, services),
            FrameStage::ReadyIndicator => {
                self.update_ready_indicator(input, motion, lock, ms, services)
            }
        }
    }
    /// Complete retail 0x803773c8, including signed timer arithmetic and the
    /// checkpoint warning/rumble order. The finished gate follows HUD expiry.
    pub fn update_counters(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        ms: i32,
        services: &mut impl FrameServices,
    ) {
        if self.checkpoint_flash {
            self.checkpoint_flash_remaining = self.checkpoint_flash_remaining.wrapping_sub(ms);
            if self.checkpoint_flash_remaining < 0 {
                services.frame_emit(self, input, motion, lock, FrameEffect::Checkpoint(false));
                self.checkpoint_flash = false;
            }
        }
        if self.children.boost_hud_remaining > 0 {
            self.children.boost_hud_remaining = self.children.boost_hud_remaining.wrapping_sub(ms);
            if self.children.boost_hud_remaining <= 0 {
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::Boost {
                        controller: motion.controller,
                        value: self.children.boost_count,
                    },
                );
            }
        }
        if motion.finished {
            return;
        }
        self.children.elapsed = self.children.elapsed.map(|v| v.wrapping_add(ms));
        if input.state == 4 {
            self.crash_elapsed = self.crash_elapsed.wrapping_add(ms);
            if self.crash_elapsed > 4000 {
                self.crashed = true;
            }
        }
        if self.checkpoint_active {
            let old_seconds = self.checkpoint_remaining / 1000;
            self.checkpoint_remaining = self.checkpoint_remaining.wrapping_sub(ms);
            let seconds = self.checkpoint_remaining / 1000;
            if old_seconds != seconds {
                let warning = self.checkpoint_remaining.wrapping_add(999) / 1000;
                let event = match warning {
                    1 => Some(71),
                    2 => Some(70),
                    3 => Some(69),
                    4 | 5 => {
                        self.children.rumble_remaining = 0;
                        Some(69)
                    }
                    _ => None,
                };
                if let Some(event) = event {
                    services.frame_emit(
                        self,
                        input,
                        motion,
                        lock,
                        FrameEffect::HudAudio {
                            event,
                            handles: motion.audio_handles,
                        },
                    );
                    services.frame_emit(
                        self,
                        input,
                        motion,
                        lock,
                        FrameEffect::WiiSound {
                            controller: motion.controller,
                            event: 33,
                            volume: 4096,
                        },
                    );
                }
            }
            if self.checkpoint_remaining < 3000 {
                self.children.rumble_remaining = self.children.rumble_remaining.wrapping_sub(ms);
                if self.children.rumble_remaining < 0 && self.checkpoint_remaining > 0 {
                    let strength = match seconds {
                        2 => 0.75,
                        3 => 0.5,
                        _ => 1.0,
                    };
                    DashServices::resolve_controller(services, motion.controller);
                    services.frame_emit(
                        self,
                        input,
                        motion,
                        lock,
                        FrameEffect::Rumble {
                            milliseconds: 200,
                            strength,
                        },
                    );
                    self.children.rumble_remaining = 500;
                }
            }
        }
        if self.children.boost_remaining > 0 {
            self.children.boost_remaining = self.children.boost_remaining.wrapping_sub(ms);
            if self.children.boost_remaining <= 0 {
                self.children.boost_remaining = 0;
                self.children.boost_available = 1;
            }
        }
        if input.recovery_remaining > 0 && !motion.finished {
            input.recovery_remaining = input.recovery_remaining.wrapping_sub(ms);
            if input.recovery_remaining <= 0 && !self.crashed {
                if -motion.velocity[0] < 2.5 {
                    motion.velocity = [-input.cruise_speed, input.upward_speed, 0.];
                    InputServices::emit(
                        services,
                        input,
                        motion,
                        InputEffect::SetLinearVelocity(motion.velocity),
                    );
                }
                input.recovery_ready = true;
            }
        }
        if input.horizontal_delay > 0 {
            input.horizontal_delay = input.horizontal_delay.wrapping_sub(ms);
            if input.horizontal_delay <= 0 {
                input.horizontal_control = true;
            }
        }
        if input.vertical_delay > 0 {
            input.vertical_delay = input.vertical_delay.wrapping_sub(ms);
            if input.vertical_delay <= 0 {
                input.vertical_control = true;
            }
        }
        if self.children.slow_remaining > 0 {
            self.children.slow_remaining = self.children.slow_remaining.wrapping_sub(ms);
            if self.children.slow_remaining <= 0 {
                self.children.slow_active = false;
                input.speed_factor = 1.;
                input.powerup_kind = 0;
            }
        }
        let current = self.children.smoothed_speed_factor;
        let target = input.speed_factor;
        if current < target {
            let next = 0.001_f32.mul_add(ms as f32, current);
            self.children.smoothed_speed_factor = if next > target { target } else { next };
        } else if current > target {
            let next = -(0.001_f32.mul_add(ms as f32, -current));
            self.children.smoothed_speed_factor = if next < target { target } else { next };
        }
    }
    /// Complete 0x80377c04. Fresh rigid velocity is sampled here, after input
    /// effects, and stored Motion velocity deliberately remains unchanged.
    pub fn update_sprinkler(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        ms: i32,
        services: &mut impl FrameServices,
    ) {
        let velocity = InputServices::linear_velocity(services);
        if -velocity[0] > 1.8 {
            InputServices::emit(
                services,
                input,
                motion,
                InputEffect::SetLinearVelocity([-1.8, velocity[1], velocity[2]]),
            );
            let force = [
                0.0 * input.force_mass,
                0.004_f32 * input.force_mass,
                0.0 * input.force_mass,
            ];
            InputServices::emit(
                services,
                input,
                motion,
                InputEffect::ApplyForce {
                    time: ms as f32,
                    force,
                },
            );
        }
    }
    /// Complete 0x80377d00. The native mandatory trail/holder FX pointers must
    /// exist; only the optional extra pointer has a native null recovery arm.
    pub fn update_particles(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        services: &mut impl FrameServices,
    ) {
        let trail = self.children.trail_particle;
        assert!(services.particle_exists(trail), "native mandatory trail FX");
        let mut position = motion.position;
        let emitting = input.state == 0;
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticleEnable {
                particle: trail,
                enabled: emitting,
            },
        );
        if emitting {
            position[0] += -0.2;
            position[1] += -0.05;
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::ParticlePosition {
                    particle: trail,
                    position,
                },
            );
        }
        let holder = self.children.holder_particle;
        assert!(
            services.particle_exists(holder),
            "native mandatory holder FX"
        );
        let mut holder_position = motion.position;
        holder_position[1] += 0.4;
        if self.grabbed {
            holder_position[2] += -0.05;
        }
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::ParticlePosition {
                particle: holder,
                position: holder_position,
            },
        );
        let extra = self.children.extra_particle;
        if extra != self.bad_particle {
            if services.particle_exists(extra) {
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::ParticleEnable {
                        particle: extra,
                        enabled: input.state == 0,
                    },
                );
                position[0] += -0.2;
                position[1] += -0.05;
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::ParticlePosition {
                        particle: extra,
                        position,
                    },
                );
            } else {
                self.children.extra_particle = self.bad_particle;
            }
        }
    }
    /// Complete indicator state machine and ordered native particle lifecycle.
    pub fn update_ready_indicator(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        ms: i32,
        services: &mut impl FrameServices,
    ) {
        motion.indicator_timer = motion.indicator_timer.wrapping_add(ms);
        let timer = motion.indicator_timer;
        let mut scale = 0.;
        let active = match motion.indicator_state {
            0 => {
                if timer < 500 {
                    scale = 0.2_f32 * ((500_i32.wrapping_sub(timer) as f32) / 500.);
                } else {
                    motion.indicator_state = 1;
                    motion.indicator_timer = 0;
                }
                true
            }
            1 => {
                if timer < 500 {
                    scale = 0.2_f32 * (timer as f32 / 500.);
                } else {
                    motion.indicator_state = 0;
                    motion.indicator_timer = 0;
                    scale = 0.2;
                }
                true
            }
            2 => {
                if timer < 150 {
                    scale = timer as f32 / 150.;
                } else {
                    motion.indicator_state = 3;
                    motion.indicator_timer = 0;
                    scale = 1.;
                }
                true
            }
            _ => false,
        };
        let mut particle = self.children.ready_particle;
        services.particle_exists(particle); // Native lookup occurs even for GUID_BAD.
        if active {
            let held = self.grabbed && self.holder.is_some();
            if particle == self.bad_particle {
                let position = if held {
                    self.holder.as_ref().unwrap().position
                } else {
                    motion.position
                };
                particle = services
                    .create_particle(if held { "pg_pa_ready" } else { "pg_pa_locka" }, position);
                self.children.ready_particle = particle;
                assert!(
                    services.particle_exists(particle),
                    "native created indicator FX"
                );
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::ParticleEnable {
                        particle,
                        enabled: false,
                    },
                );
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::ParticleViewport {
                        particle,
                        controller: motion.controller,
                    },
                );
                if !held {
                    services.frame_emit(
                        self,
                        input,
                        motion,
                        lock,
                        FrameEffect::HudAudio {
                            event: 35,
                            handles: [0, 100],
                        },
                    );
                }
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::WiiSound {
                        controller: motion.controller,
                        event: 1,
                        volume: 4096,
                    },
                );
            }
            let mut position = if held {
                self.holder.as_ref().unwrap().position
            } else {
                motion.position
            };
            if held {
                position[1] += 0.3;
            } else {
                position[0] -= 0.2;
                position[1] += 0.04;
            }
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::ParticleEnable {
                    particle,
                    enabled: true,
                },
            );
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::ParticlePosition { particle, position },
            );
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::ParticleScale { particle, scale },
            );
        } else if particle != self.bad_particle {
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::DestroyParticle {
                    particle,
                    milliseconds: 0,
                },
            );
            self.children.ready_particle = self.bad_particle;
        }
    }
    pub fn crash(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        services: &mut impl FrameServices,
    ) {
        if input.state == 4 {
            return;
        }
        self.crash_phase = 0;
        input.state = 4;
        self.crash_elapsed = 0;
        if motion.target_count > 0 {
            lock.timers[lock.selected] = 0;
            motion.target_timer = 0;
            if lock.particle != self.bad_particle {
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::DestroyParticle {
                        particle: lock.particle,
                        milliseconds: 0,
                    },
                );
                lock.particle = self.bad_particle;
            }
        }
    }
    pub fn set_ready(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        services: &mut impl FrameServices,
    ) {
        if self.ready || motion.indicator_state as u32 > 1 {
            return;
        }
        self.ready = true;
        motion.set_indicator_active(false);
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::HudAudio {
                event: 36,
                handles: [0, 100],
            },
        );
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::WiiSound {
                controller: motion.controller,
                event: 2,
                volume: 4096,
            },
        );
    }
    pub fn passed_checkpoint(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        services: &mut impl FrameServices,
    ) {
        if self.crashed {
            return;
        }
        if input.state == 4 {
            input.state = 0;
            self.crashed = false;
            self.crash_phase = 5;
        }
        services.frame_emit(
            self,
            input,
            motion,
            lock,
            FrameEffect::HudAudio {
                event: 66,
                handles: motion.audio_handles,
            },
        );
        if self.checkpoint_active {
            self.checkpoint_remaining = self.checkpoint_remaining.wrapping_add(3000);
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::HudAudio {
                    event: 68,
                    handles: motion.audio_handles,
                },
            );
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::WiiSound {
                    controller: motion.controller,
                    event: 32,
                    volume: 4096,
                },
            );
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::DisplayPoints {
                    controller: motion.controller,
                    points: 3,
                },
            );
            self.checkpoint_flash = true;
            self.checkpoint_flash_remaining = 800;
            services.frame_emit(self, input, motion, lock, FrameEffect::Checkpoint(true));
        }
    }
    /// Complete 0x803749c4 orchestration with recovered default stage bodies.
    /// Hosts supply synchronous controller, physics and presentation services.
    pub fn update(
        &mut self,
        input: &mut InputState,
        motion: &mut Motion,
        lock: &mut TargetLock,
        milliseconds: i32,
        services: &mut impl FrameServices,
    ) {
        services.stage(
            self,
            input,
            motion,
            lock,
            FrameStage::Particles,
            milliseconds,
        );
        InputServices::select_controller(services, motion.controller);
        let properties = PLANE_PROPERTIES[self.plane_type];
        input.cruise_speed = (f32::from_bits(properties[0]) * self.boost_factor) * motion.catch_up;
        motion.max_horizontal_speed =
            (f32::from_bits(properties[1]) * self.boost_factor) * motion.catch_up;
        self.third_speed = (f32::from_bits(properties[2]) * self.boost_factor) * motion.catch_up;
        input.lateral_speed = f32::from_bits(properties[3]);
        input.upward_speed = f32::from_bits(properties[4]);
        input.downward_speed = f32::from_bits(properties[5]);
        self.integer_property = properties[6];
        if !self.grabbed {
            services.stage(
                self,
                input,
                motion,
                lock,
                FrameStage::Counters,
                milliseconds,
            );
            if self.checkpoint_active && self.checkpoint_remaining < 0 {
                self.checkpoint_remaining = 0;
                if input.state != 4 {
                    services.frame_emit(
                        self,
                        input,
                        motion,
                        lock,
                        FrameEffect::HudAudio {
                            event: 72,
                            handles: motion.audio_handles,
                        },
                    );
                    services.frame_emit(
                        self,
                        input,
                        motion,
                        lock,
                        FrameEffect::WiiSound {
                            controller: motion.controller,
                            event: 33,
                            volume: 4096,
                        },
                    );
                }
                self.crash(input, motion, lock, services);
            }
            motion.target_timer = lock.timers[lock.selected];
            services.bind_target(lock.selected);
            motion.update_dashing_with_services(milliseconds, services);
            input.update_input(motion, milliseconds, services);
            lock.update(motion, milliseconds, services);
            if self.in_sprinkler {
                services.stage(
                    self,
                    input,
                    motion,
                    lock,
                    FrameStage::Sprinkler,
                    milliseconds,
                );
            }
            if self.checkpoint_active {
                let checkpoint = services.nearest_checkpoint(motion.position);
                let distance = -checkpoint[0];
                if distance > self.checkpoint_progress {
                    self.checkpoint_progress = distance;
                    if (checkpoint[2] < 0. && motion.position[2] < 0.)
                        || (checkpoint[2] > 0. && motion.position[2] > 0.)
                        || (checkpoint[2] < 0.1 && checkpoint[2] > -0.1)
                    {
                        self.passed_checkpoint(input, motion, lock, services);
                    }
                }
            }
            if input.state != 6 {
                self.physical_matrix = InputServices::matrix(services);
                motion.position = [
                    self.physical_matrix[12],
                    self.physical_matrix[13],
                    self.physical_matrix[14],
                ];
            }
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::Walls {
                    controller: motion.controller,
                    position: motion.position,
                },
            );
            if !motion.finished {
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::CameraPosition(motion.position),
                );
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::CameraRoll(input.angles[0]),
                );
            }
        } else {
            if let Some(holder) = &self.holder {
                let holder_matrix = holder.matrix;
                let orient = rotation_y(f32::from_bits(0x3fc90fdb));
                input.orientation = crate::area_transform::multiply(
                    &rotation_x(f32::from_bits(0x40490fdb)),
                    &orient,
                );
                let attachment = self
                    .attachment
                    .unwrap_or_else(|| crate::area_transform::translation([-0.2, 0.5, -0.2]));
                let mut local = crate::area_transform::multiply(&input.orientation, &attachment);
                local = crate::area_transform::multiply(&local, &holder_matrix);
                motion.position = [local[12], local[13], local[14]];
                let area = crate::area_transform::AreaTransform {
                    radius: self.area_radius,
                    disabled: self.curvature_disabled,
                }
                .model_matrix(motion.position);
                local[12..15].fill(0.);
                self.render_matrix = crate::area_transform::multiply(&local, &area);
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::CameraOffsets {
                        position: self.start_camera_position,
                        target: self.start_camera_target,
                    },
                );
                let mut position = self
                    .holder
                    .as_ref()
                    .expect("native holder pointer remains valid")
                    .position;
                position[1] += 0.4;
                services.frame_emit(
                    self,
                    input,
                    motion,
                    lock,
                    FrameEffect::CameraPosition(position),
                );
            }
            if services.ready_event(122) {
                self.set_ready(input, motion, lock, services);
            }
            InputServices::emit(
                services,
                input,
                motion,
                InputEffect::SetLinearVelocity([0.; 3]),
            );
            InputServices::emit(
                services,
                input,
                motion,
                InputEffect::SetAngularVelocity([0.; 3]),
            );
        }
        services.stage(
            self,
            input,
            motion,
            lock,
            FrameStage::ReadyIndicator,
            milliseconds,
        );
        if services.dare_type() == 1 {
            let value = 700.0_f32 * (motion.position[1] / 3.0);
            let value = if value.is_finite()
                && value as f64 >= -2147483648.
                && (value as f64) < 2147483648.
            {
                value.trunc() as i32
            } else {
                i32::MIN
            };
            services.frame_emit(
                self,
                input,
                motion,
                lock,
                FrameEffect::AltitudeAudio {
                    event: 54,
                    value,
                    parameters: [-1, -1],
                },
            );
        }
    }
}
fn rotation_y(angle: f32) -> Matrix {
    let (s, c) = crate::character_input::ea_sin_cos(angle);
    [c, 0., -s, 0., 0., 1., 0., 0., s, 0., c, 0., 0., 0., 0., 1.]
}
