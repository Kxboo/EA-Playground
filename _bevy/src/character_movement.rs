//! Recovered grounded/airborne Havok velocity preparation, before host proxy collision integration.
use crate::character_input::CharacterInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementState {
    Grounded,
    InAir,
}
#[derive(Clone, Copy, Debug)]
pub struct MovementConfig {
    pub ground_speed: f32,
    pub ground_gain: f32,
    pub air_speed: f32,
    pub air_gain: f32,
    /// Original OnGround bytes +10, +11, +12; defaults recovered from constructor.
    pub ground_flags: [bool; 3],
}
impl Default for MovementConfig {
    fn default() -> Self {
        Self {
            ground_speed: 20.0,
            ground_gain: 1.0,
            air_speed: 20.0,
            air_gain: 1.0,
            ground_flags: [true, false, false],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct CharacterMovementState {
    pub state: MovementState,
    pub config: MovementConfig,
}
impl Default for CharacterMovementState {
    fn default() -> Self {
        Self {
            state: MovementState::Grounded,
            config: MovementConfig::default(),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct MovementOutput {
    pub velocity: [f32; 4],
    pub state: MovementState,
}
#[derive(Clone, Copy, Debug)]
pub struct MovementInput {
    pub gain: f32,
    pub forward: [f32; 4],
    pub up: [f32; 4],
    pub normal: [f32; 4],
    pub velocity: [f32; 4],
    pub desired: [f32; 4],
    pub max_acceleration: f32,
    pub surface_velocity: [f32; 4],
}

// Numeric Gekko estimate data reported by Dolphin tag 2506 (see docs for attribution).
// Independently expressed f32 input bit mapping; no Dolphin implementation is vendored.
const ESTIMATE_BASE: [u32; 32] = [
    0x1a7e800, 0x17cb800, 0x1552800, 0x130c000, 0x10f2000, 0x0eff000, 0x0d2e000, 0x0b7c000,
    0x09e5000, 0x0867000, 0x06ff000, 0x05ab800, 0x046a000, 0x0339800, 0x0218800, 0x0105800,
    0x3ffa000, 0x3c29000, 0x38aa000, 0x3572000, 0x3279000, 0x2fb7000, 0x2d26000, 0x2ac0000,
    0x2881000, 0x2665000, 0x2468000, 0x2287000, 0x20c1000, 0x1f12000, 0x1d79000, 0x1bf4000,
];
const ESTIMATE_DROP: [u32; 32] = [
    0x568, 0x4f3, 0x48d, 0x435, 0x3e7, 0x3a2, 0x365, 0x32e, 0x2fc, 0x2d0, 0x2a8, 0x283, 0x261,
    0x243, 0x226, 0x20b, 0x7a4, 0x700, 0x670, 0x5f2, 0x584, 0x524, 0x4cc, 0x47e, 0x43a, 0x3fa,
    0x3c2, 0x38e, 0x35e, 0x332, 0x30a, 0x2e6,
];
fn reciprocal_estimate(x: f32) -> f32 {
    if x == 0.0 {
        return f32::INFINITY.copysign(x);
    }
    if x < 0.0 || x.is_nan() {
        return f32::NAN;
    }
    if x.is_infinite() {
        return 0.0;
    }
    // All positive f32 values become normal f64 values, including f32 subnormals.
    let bits = (x as f64).to_bits();
    let exponent = ((bits >> 52) & 2047) as usize;
    let index = ((exponent & 1) << 15) | ((bits & ((1_u64 << 52) - 1)) >> 37) as usize;
    let fraction =
        (ESTIMATE_BASE[index >> 11] - ESTIMATE_DROP[index >> 11] * (index & 2047) as u32) as u64;
    f64::from_bits((((3068 - exponent) / 2) as u64) << 52 | fraction << 26) as f32
}
fn inv_length(square: f32) -> f32 {
    if square == 0.0 {
        return 0.0;
    }
    if square <= 0.0 {
        return f32::INFINITY;
    }
    let r = reciprocal_estimate(square);
    let product = square * r;
    let half = 0.5 * r;
    half * (-r.mul_add(product, -3.0))
}
fn length_square(v: [f32; 4]) -> f32 {
    v[2].mul_add(v[2], v[0].mul_add(v[0], v[1] * v[1]))
}
fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1]))
}
fn cross(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[1].mul_add(b[2], -(a[2] * b[1])),
        a[2].mul_add(b[0], -(a[0] * b[2])),
        a[0].mul_add(b[1], -(a[1] * b[0])),
        0.0,
    ]
}
fn scale(v: [f32; 4], s: f32) -> [f32; 4] {
    v.map(|x| x * s)
}
fn normalize(v: [f32; 4]) -> [f32; 4] {
    scale(v, inv_length(length_square(v)))
}
fn rotate(basis: [[f32; 4]; 3], v: [f32; 4]) -> [f32; 4] {
    [
        v[2].mul_add(basis[2][0], v[0].mul_add(basis[0][0], v[1] * basis[1][0])),
        v[2].mul_add(basis[2][1], v[0].mul_add(basis[0][1], v[1] * basis[1][1])),
        v[2].mul_add(basis[2][2], v[0].mul_add(basis[0][2], v[1] * basis[1][2])),
        0.0,
    ]
}
fn inverse_rotate(basis: [[f32; 4]; 3], v: [f32; 4]) -> [f32; 4] {
    [dot(v, basis[0]), dot(v, basis[1]), dot(v, basis[2]), 0.0]
}

/// Original utility preserves the supplied output when forward × up is degenerate.
pub fn calculate_movement(input: MovementInput, initial_output: [f32; 4]) -> [f32; 4] {
    let side = cross(input.forward, input.up);
    if length_square(side) < f32::EPSILON {
        return initial_output;
    }
    let side = normalize(side);
    let x = normalize(cross(side, input.normal));
    let y = normalize(cross(x, input.normal));
    let basis = [x, y, input.normal];
    let relative = std::array::from_fn(|i| input.velocity[i] - input.surface_velocity[i]);
    let current = inverse_rotate(basis, relative);
    let mut change: [f32; 4] = std::array::from_fn(|i| input.desired[i] - current[i]);
    let square = length_square(change);
    if input.gain * square > input.max_acceleration * input.max_acceleration {
        let reciprocal = inv_length(square);
        let maximum = input.max_acceleration / input.gain;
        change = scale(scale(change, reciprocal), maximum);
    }
    let next = std::array::from_fn(|i| input.gain.mul_add(change[i], current[i]));
    let world = rotate(basis, next);
    std::array::from_fn(|i| world[i] + input.surface_velocity[i])
}

impl CharacterMovementState {
    /// Valid for inputs emitted by BuildCharacterInput (jump/ladder flags false).
    pub fn update(&mut self, input: &CharacterInput) -> MovementOutput {
        assert!(
            !input.want_jump && !input.at_ladder,
            "Jumping/climbing states are outside this grounded/airborne port"
        );
        let mut velocity = input.velocity;
        match self.state {
            MovementState::Grounded if !input.supported => {
                if self.config.ground_flags[0] {
                    let vertical = dot(input.velocity, input.up);
                    velocity =
                        std::array::from_fn(|i| -vertical.mul_add(input.up[i], -velocity[i]));
                }
                self.state = MovementState::InAir;
            }
            MovementState::InAir if input.supported => {
                self.state = MovementState::Grounded;
            }
            state => {
                let ground = state == MovementState::Grounded;
                let speed = if ground {
                    self.config.ground_speed
                } else {
                    self.config.air_speed
                };
                velocity = calculate_movement(
                    MovementInput {
                        gain: if ground {
                            self.config.ground_gain
                        } else {
                            self.config.air_gain
                        },
                        forward: input.forward,
                        up: input.up,
                        normal: if ground {
                            input.surface_normal
                        } else {
                            input.up
                        },
                        velocity: input.velocity,
                        desired: [input.input_ud * speed, input.input_lr * speed, 0.0, 0.0],
                        max_acceleration: 100.0,
                        surface_velocity: input.surface_velocity,
                    },
                    velocity,
                );
                if !ground {
                    let new_vertical = -dot(velocity, input.up);
                    let old_vertical = dot(input.velocity, input.up);
                    let removed: [f32; 4] =
                        std::array::from_fn(|i| new_vertical.mul_add(input.up[i], velocity[i]));
                    let restored: [f32; 4] =
                        std::array::from_fn(|i| old_vertical.mul_add(input.up[i], removed[i]));
                    velocity = std::array::from_fn(|i| {
                        input.step_info[2].mul_add(input.gravity[i], restored[i])
                    });
                } else {
                    if self.config.ground_flags[1] {
                        let next_up = dot(velocity, input.up);
                        let change = next_up - dot(input.velocity, input.up);
                        let gravity_step = dot(input.gravity, input.up) * input.step_info[2];
                        if next_up < 0.0 && change < gravity_step {
                            let correction = gravity_step - change;
                            velocity = std::array::from_fn(|i| {
                                correction.mul_add(input.up[i], velocity[i])
                            });
                        }
                    }
                    if !self.config.ground_flags[2] {
                        velocity = std::array::from_fn(|i| velocity[i] - input.surface_velocity[i]);
                        if dot(velocity, input.up) > 0.001 {
                            let square = length_square(velocity);
                            let length = if square <= 0.0 {
                                0.0
                            } else {
                                square * inv_length(square)
                            };
                            let unit = scale(velocity, 1.0 / length);
                            let side = cross(input.surface_normal, unit);
                            let amount = length / dot(input.surface_normal, input.up);
                            velocity = scale(cross(side, input.up), amount);
                        }
                        velocity = std::array::from_fn(|i| velocity[i] + input.surface_velocity[i]);
                    }
                }
            }
        }
        MovementOutput {
            velocity,
            state: self.state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_machine_velocity_and_state_vectors() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/character_movement_golden.json"))
                .unwrap();
        assert_eq!(
            data["elf_sha256"].as_str(),
            Some("5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c")
        );
        fn scalar(v: &serde_json::Value, key: &str) -> f32 {
            v[key].as_f64().unwrap() as f32
        }
        fn vector(v: &serde_json::Value, key: &str) -> [f32; 4] {
            std::array::from_fn(|i| v[key][i].as_f64().unwrap() as f32)
        }
        for (index, v) in data["vectors"].as_array().unwrap().iter().enumerate() {
            let output = if v["kind"].as_str() == Some("utility") {
                calculate_movement(
                    MovementInput {
                        gain: scalar(v, "gain"),
                        forward: vector(v, "forward"),
                        up: vector(v, "up"),
                        normal: vector(v, "normal"),
                        velocity: vector(v, "velocity"),
                        desired: vector(v, "desired"),
                        max_acceleration: scalar(v, "max_acceleration"),
                        surface_velocity: vector(v, "surface_velocity"),
                    },
                    vector(v, "initial_output"),
                )
            } else {
                let mut state = CharacterMovementState {
                    state: if v["state"].as_u64() == Some(0) {
                        MovementState::Grounded
                    } else {
                        MovementState::InAir
                    },
                    config: MovementConfig {
                        ground_speed: scalar(v, "ground_speed"),
                        ground_gain: scalar(v, "ground_gain"),
                        air_speed: scalar(v, "air_speed"),
                        air_gain: scalar(v, "air_gain"),
                        ground_flags: std::array::from_fn(|i| {
                            v["ground_flags"][i].as_u64().unwrap() != 0
                        }),
                    },
                };
                let seconds = scalar(v, "seconds");
                let output = state.update(&CharacterInput {
                    input_lr: scalar(v, "lr"),
                    input_ud: scalar(v, "ud"),
                    want_jump: false,
                    at_ladder: false,
                    supported: v["supported"].as_bool().unwrap(),
                    up: vector(v, "up"),
                    forward: vector(v, "forward"),
                    surface_normal: vector(v, "normal"),
                    surface_velocity: vector(v, "surface_velocity"),
                    step_info: [
                        0.0,
                        0.0,
                        seconds,
                        if seconds == 0.0 { 0.0 } else { 1.0 / seconds },
                    ],
                    position: [0.0; 4],
                    velocity: vector(v, "velocity"),
                    gravity: vector(v, "gravity"),
                });
                assert_eq!(
                    output.state,
                    if v["next_state"].as_u64() == Some(0) {
                        MovementState::Grounded
                    } else {
                        MovementState::InAir
                    },
                    "state vector {index}"
                );
                output.velocity
            };
            let expected: [u32; 4] =
                std::array::from_fn(|i| v["output_bits"][i].as_u64().unwrap() as u32);
            assert_eq!(
                output.map(f32::to_bits),
                expected,
                "velocity vector {index}"
            );
        }
    }
}
