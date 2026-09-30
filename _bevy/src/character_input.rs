//! Original PhysicsDynamicCharacter input preparation; collision queries are supplied by the host.
pub const CHARACTER_SPEED: f32 = 20.0;

#[derive(Clone, Copy, Debug)]
pub struct SurfaceSupport {
    pub kind: u32,
    pub normal: [f32; 4],
    pub velocity: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct CharacterInputState {
    pub speed: f32,
    pub orientation: f32,
    pub gravity: [f32; 3],
    pub gravity_override: Option<[f32; 3]>,
    pub impulse: [f32; 3],
    pub impulse_decay: [f32; 3],
    pub impulse_ms: i32,
}

#[derive(Clone, Debug)]
pub struct CharacterInput {
    pub input_lr: f32,
    pub input_ud: f32,
    pub want_jump: bool,
    pub up: [f32; 4],
    pub forward: [f32; 4],
    pub at_ladder: bool,
    pub supported: bool,
    pub surface_normal: [f32; 4],
    pub surface_velocity: [f32; 4],
    pub step_info: [f32; 4],
    pub position: [f32; 4],
    pub velocity: [f32; 4],
    pub gravity: [f32; 4],
}

// Ordered comparisons reproduce the original min/max sequence, including NaN handling.
fn clamp_input(x: f32) -> f32 {
    let upper = if 1.0 < x { 1.0 } else { x };
    if -1.0 > upper { -1.0 } else { upper }
}

fn wrap(mut x: f32) -> f32 {
    let tau = f32::from_bits(0x40c90fdb);
    while x >= tau {
        x -= tau;
    }
    while x < 0.0 {
        x += tau;
    }
    x
}

// EA::Math::fSinCos's original polynomial, rather than host f32 trigonometry.
fn ea_sin_cos(x: f32) -> (f32, f32) {
    let q = ((0.6366197466850281_f32 * x) + if x < 0.0 { -0.5 } else { 0.5 }) as i32;
    let r = -1.5707963705062866_f32.mul_add(q as f32, -x);
    let r2 = r * r;
    let p = 2.7557318844628753e-6_f32 * r2;
    let p = (-0.00019841270113829523_f32 + p) * r2;
    let p = (0.008333333767950535_f32 + p) * r2;
    let p = (-0.1666666716337204_f32 + p) * r2;
    let s = (1.0 + p) * r;
    let c = ((-s.mul_add(s, -1.0)) as f64).sqrt() as f32;
    match q & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

fn original_forward(orientation: f32) -> [f32; 4] {
    let half = 0.5 * (1.5707963705062866_f32 + orientation);
    let s = (half as f64).sin() as f32;
    let c = (half as f64).cos() as f32;
    let x = s * 0.0;
    let y = s;
    let z = s * 0.0;
    let tx = x + x;
    let ty = y + y;
    let tz = z + z;
    // Column zero of hkRotation::set, followed by setRotatedDir(+X).
    let m00 = 1.0 - (ty * y + tz * z);
    let m10 = ty * x + tz * c;
    let m20 = tz * x - ty * c;
    let m01 = ty * x - tz * c;
    let m11 = 1.0 - (tx * x + tz * z);
    let m21 = tz * y + tx * c;
    let m02 = tz * x + ty * c;
    let m12 = tz * y - tx * c;
    let m22 = 1.0 - (tx * x + ty * y);
    [
        0.0_f32.mul_add(m02, 1.0_f32.mul_add(m00, 0.0 * m01)),
        0.0_f32.mul_add(m12, 1.0_f32.mul_add(m10, 0.0 * m11)),
        0.0_f32.mul_add(m22, 1.0_f32.mul_add(m20, 0.0 * m21)),
        0.0,
    ]
}

impl CharacterInputState {
    /// Original ApplyVelocity (803b6768): subsequent frames decay this override linearly.
    pub fn apply_velocity(&mut self, velocity: [f32; 3], duration_ms: i32) {
        self.impulse = velocity;
        self.impulse_ms = duration_ms;
        let scale = -1000.0 / duration_ms as f32;
        self.impulse_decay = velocity.map(|x| x * scale);
    }

    pub fn build(
        &mut self,
        dt_ms: i32,
        support: SurfaceSupport,
        position: [f32; 4],
        velocity: [f32; 4],
    ) -> CharacterInput {
        let seconds = dt_ms as f32 / 1000.0;
        let gravity = self.gravity_override.take().unwrap_or(self.gravity);
        let mut out = CharacterInput {
            input_lr: 0.0,
            input_ud: clamp_input(self.speed / CHARACTER_SPEED),
            want_jump: false,
            up: [0.0, 1.0, 0.0, 0.0],
            forward: original_forward(self.orientation),
            at_ladder: false,
            supported: support.kind == 2,
            surface_normal: support.normal,
            surface_velocity: support.velocity,
            step_info: [
                0.0,
                0.0,
                seconds,
                if seconds == 0.0 { 0.0 } else { 1.0 / seconds },
            ],
            position,
            velocity,
            gravity: [gravity[0], gravity[1], gravity[2], 1.0],
        };
        if self.impulse_ms > 0 {
            let [x, y, z] = self.impulse;
            let length = (z.mul_add(z, x.mul_add(x, y * y)) as f64).sqrt() as f32;
            let angle = wrap((x as f64).atan2(z as f64) as f32);
            let local = wrap(angle - wrap(self.orientation));
            let (s, c) = ea_sin_cos(local);
            out.input_lr = clamp_input(out.input_lr - (s * length) / CHARACTER_SPEED);
            out.input_ud = clamp_input(out.input_ud + (c * length) / CHARACTER_SPEED);
            for i in 0..3 {
                self.impulse[i] += self.impulse_decay[i] * seconds;
            }
            self.impulse_ms = self
                .impulse_ms
                .wrapping_sub((1000.0 * seconds) as i32)
                .max(0);
        }
        out
    }
}

impl CharacterInput {
    /// Zero-filled hkCharacterInput layout used for differential checking (40 big-endian words).
    pub fn output_words(&self) -> [u32; 40] {
        let mut words = [0; 40];
        words[0] = self.input_lr.to_bits();
        words[1] = self.input_ud.to_bits();
        words[2] = (self.want_jump as u32) << 24;
        words[12] = ((self.at_ladder as u32) << 24) | ((self.supported as u32) << 16);
        for (offset, vector) in [
            (4, self.up),
            (8, self.forward),
            (16, self.surface_normal),
            (20, self.surface_velocity),
            (24, self.step_info),
            (28, self.position),
            (32, self.velocity),
            (36, self.gravity),
        ] {
            for i in 0..4 {
                words[offset + i] = vector[i].to_bits();
            }
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_machine_vectors() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/character_input_golden.json"))
                .unwrap();
        assert_eq!(
            data["elf_sha256"].as_str(),
            Some("5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c")
        );
        fn vector<const N: usize>(v: &serde_json::Value) -> [f32; N] {
            std::array::from_fn(|i| v[i].as_f64().unwrap() as f32)
        }
        for (index, v) in data["vectors"].as_array().unwrap().iter().enumerate() {
            let mut state = CharacterInputState {
                speed: v["speed"].as_f64().unwrap() as f32,
                orientation: v["orientation"].as_f64().unwrap() as f32,
                gravity: vector(&v["gravity"]),
                gravity_override: if v["gravity_override"].is_null() {
                    None
                } else {
                    Some(vector(&v["gravity_override"]))
                },
                impulse: vector(&v["impulse"]),
                impulse_decay: vector(&v["impulse_decay"]),
                impulse_ms: v["impulse_ms"].as_i64().unwrap() as i32,
            };
            if !v["apply_velocity"].is_null() {
                state.apply_velocity(
                    vector(&v["apply_velocity"]),
                    v["duration_ms"].as_i64().unwrap() as i32,
                );
            }
            let out = state.build(
                v["dt_ms"].as_i64().unwrap() as i32,
                SurfaceSupport {
                    kind: v["support_kind"].as_u64().unwrap() as u32,
                    normal: vector(&v["surface_normal"]),
                    velocity: vector(&v["surface_velocity"]),
                },
                vector(&v["position"]),
                vector(&v["velocity"]),
            );
            let expected: [u32; 40] =
                std::array::from_fn(|i| v["output_words"][i].as_u64().unwrap() as u32);
            assert_eq!(out.output_words(), expected, "output vector {index}");
            assert_eq!(
                state.impulse.map(f32::to_bits),
                std::array::from_fn(|i| v["next_impulse_bits"][i].as_u64().unwrap() as u32),
                "impulse vector {index}"
            );
            assert_eq!(
                state.impulse_ms,
                v["next_impulse_ms"].as_i64().unwrap() as i32,
                "timer vector {index}"
            );
            assert_eq!(
                state.impulse_decay.map(f32::to_bits),
                std::array::from_fn(|i| v["next_decay_bits"][i].as_u64().unwrap() as u32)
            );
            assert!(state.gravity_override.is_none());
        }
    }
}
