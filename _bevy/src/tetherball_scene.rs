//! Remaining Tetherball::Update transform feedback and external render/effect calls.
use crate::area_transform::{self as math, AreaTransform, Matrix};
use crate::tetherball::{BallMotion, Direction};

#[derive(Clone, Copy, Debug)]
pub struct Attachment {
    pub world: Matrix,
    pub local: Option<Matrix>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trail {
    Normal,
    PlayerOne,
    PlayerTwo,
    Mega,
}
impl Trail {
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "pg_tetherball_trail",
            Self::PlayerOne => "pg_tetherball_trail_plr1",
            Self::PlayerTwo => "pg_tetherball_trail_plr2",
            Self::Mega => "pg_tetherball_trail_mega",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    struct Services {
        next: u32,
        events: Vec<Value>,
    }
    impl SceneServices for Services {
        fn create_trail(&mut self, kind: Trail, p: [f32; 3]) -> u32 {
            let id = self.next;
            self.next += 1;
            self.events
                .push(json!(["create", kind.name(), p.map(f32::to_bits), id]));
            id
        }
        fn destroy_trail(&mut self, id: u32, fade: i32) {
            self.events.push(json!(["destroy", id, fade]));
        }
        fn move_trail(&mut self, id: u32, p: [f32; 3]) {
            self.events.push(json!(["move", id, p.map(f32::to_bits)]));
        }
        fn shadow_matrix(&mut self, rope: bool, matrix: Matrix) {
            self.events
                .push(json!(["shadow", rope, matrix.map(f32::to_bits)]));
        }
    }
    fn float(v: &Value) -> f32 {
        f32::from_bits(v.as_u64().unwrap() as u32)
    }
    fn floats<const N: usize>(v: &Value) -> [f32; N] {
        std::array::from_fn(|i| float(&v[i]))
    }
    #[test]
    fn original_area_and_complete_ball_update() {
        let data: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_scene_golden.json"))
                .unwrap();
        assert_eq!(
            data["elf_sha256"].as_str().unwrap(),
            crate::recovered::ELF_SHA256
        );
        for (i, row) in data["areas"].as_array().unwrap().iter().enumerate() {
            let area = AreaTransform {
                radius: float(&row["radius"]),
                disabled: row["disabled"].as_bool().unwrap(),
            };
            let got = area.model_matrix(floats(&row["position"]));
            assert_eq!(
                got.map(f32::to_bits),
                floats::<16>(&row["matrix"]).map(f32::to_bits),
                "area {i}"
            );
        }
        for (i, case) in data["cases"].as_array().unwrap().iter().enumerate() {
            let mut motion = crate::tetherball::tests::ball(&case["initial"]);
            let mut scene = BallScene {
                anchor: floats(&case["anchor"]),
                position: [0.; 3],
                ball_matrix: math::IDENTITY,
                rope_matrix: math::IDENTITY,
                trails: std::array::from_fn(|i| case["trails"][i].as_u64().unwrap() as u32),
                null_trail: u32::MAX,
                ball_shadow: case["shadows"][0].as_bool().unwrap(),
                rope_shadow: case["shadows"][1].as_bool().unwrap(),
            };
            let attachment = Some(Attachment {
                world: floats(&case["world"]),
                local: if case["local"].is_null() {
                    None
                } else {
                    Some(floats(&case["local"]))
                },
            });
            let area = AreaTransform {
                radius: float(&case["area_radius"]),
                disabled: case["disabled"].as_bool().unwrap(),
            };
            let mut services = Services {
                next: 1000,
                events: vec![],
            };
            for (j, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                services.events.clear();
                scene.update(
                    &mut motion,
                    step["ms"].as_i64().unwrap() as i32,
                    attachment,
                    area,
                    &mut services,
                );
                crate::tetherball::tests::assert_bits(
                    &motion,
                    &crate::tetherball::tests::ball(&step["motion"]),
                    i,
                    j,
                );
                assert_eq!(
                    scene.position.map(f32::to_bits),
                    floats::<3>(&step["position"]).map(f32::to_bits),
                    "position {i}/{j}"
                );
                assert_eq!(
                    scene.ball_matrix.map(f32::to_bits),
                    floats::<16>(&step["ball_matrix"]).map(f32::to_bits),
                    "ball {i}/{j}"
                );
                assert_eq!(
                    scene.rope_matrix.map(f32::to_bits),
                    floats::<16>(&step["rope_matrix"]).map(f32::to_bits),
                    "rope {i}/{j}"
                );
                assert_eq!(json!(scene.trails), step["trails"], "trails {i}/{j}");
                assert_eq!(json!(services.events), step["effects"], "effects {i}/{j}");
            }
        }
    }
}

/// The original PartFxManager owns handles and effects; this port owns its requests.
pub trait SceneServices {
    fn create_trail(&mut self, kind: Trail, position: [f32; 3]) -> u32;
    fn destroy_trail(&mut self, handle: u32, fade_ms: i32);
    fn move_trail(&mut self, handle: u32, position: [f32; 3]);
    fn shadow_matrix(&mut self, rope: bool, matrix: Matrix);
}

#[derive(Clone, Debug)]
pub struct BallScene {
    pub anchor: [f32; 3],
    pub position: [f32; 3],
    pub ball_matrix: Matrix,
    pub rope_matrix: Matrix,
    /// Original +0x140/+0x144/+0x148: normal, player-coloured power, mega.
    pub trails: [u32; 3],
    /// The external GUID null sentinel; never invent a live particle handle.
    pub null_trail: u32,
    pub ball_shadow: bool,
    pub rope_shadow: bool,
}

impl BallScene {
    /// Execute the full Update state/transform/effect logic with attachment and
    /// renderer/particle objects supplied at their original external boundaries.
    /// A grabbed ball requires a valid attachment, just as the original pointer does.
    pub fn update(
        &mut self,
        motion: &mut BallMotion,
        ms: i32,
        attachment: Option<Attachment>,
        area: AreaTransform,
        services: &mut impl SceneServices,
    ) {
        motion.update_motion(ms);
        let (s, c) = crate::character_input::ea_sin_cos(motion.angle);
        self.position = [
            s.mul_add(motion.radius, self.anchor[0]),
            motion.height,
            c.mul_add(motion.radius, self.anchor[2]),
        ];
        let offset = std::array::from_fn(|i| self.position[i] - self.anchor[i]);
        let axis = math::normalize(math::cross(offset, [0., 1., 0.]));
        let tilt = (motion.radius as f64).atan2((self.anchor[1] - self.position[1]) as f64) as f32;
        let (s, c) = crate::character_input::ea_sin_cos(motion.secondary_angle);
        let spin = [c, 0., -s, 0., 0., 1., 0., 0., s, 0., c, 0., 0., 0., 0., 1.];
        let (s, c) = crate::character_input::ea_sin_cos(f32::from_bits(0x3fc90fdb));
        let quarter = [c, s, 0., 0., -s, c, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
        let tilt_matrix = math::axis_angle(axis, tilt);
        let rotation = math::multiply(&math::multiply(&quarter, &spin), &tilt_matrix);
        self.ball_matrix = math::multiply(&rotation, &area.model_matrix(self.position));
        if motion.grabbed {
            let attachment =
                attachment.expect("grabbed tetherball needs its original attachment matrix");
            let mut held = match attachment.local {
                Some(local) => {
                    math::multiply(&math::multiply(&tilt_matrix, &local), &attachment.world)
                }
                None => math::multiply(&math::translation([0., 0.5, 0.5]), &attachment.world),
            };
            self.position.copy_from_slice(&held[12..15]);
            motion.height = self.position[1];
            let x = self.position[0] - self.anchor[0];
            let z = self.position[2] - self.anchor[2];
            motion.radius = (x.mul_add(x, z * z) as f64).sqrt() as f32;
            held[12..15].fill(0.);
            self.ball_matrix = math::multiply(&held, &area.model_matrix(self.position));
        }
        self.rope_matrix = math::multiply(
            &math::multiply(&spin, &tilt_matrix),
            &area.model_matrix(self.position),
        );
        if self.ball_shadow {
            services.shadow_matrix(false, self.ball_matrix);
        }
        if self.rope_shadow {
            services.shadow_matrix(true, self.rope_matrix);
        }
        if motion.angular_velocity.abs() > motion.secondary_acceleration.abs() {
            self.destroy(0, services);
            self.destroy(1, services);
            self.ensure(2, Trail::Mega, services);
        } else if motion.angular_velocity.abs() > motion.acceleration.abs() {
            self.destroy(2, services);
            self.destroy(0, services);
            self.ensure(
                1,
                if motion.direction == Direction::Zero {
                    Trail::PlayerOne
                } else {
                    Trail::PlayerTwo
                },
                services,
            );
        } else if motion.angular_velocity.abs() > 0. {
            self.destroy(1, services);
            self.destroy(2, services);
            self.ensure(0, Trail::Normal, services);
        }
        // At zero speed, the original leaves existing trails untouched.
    }
    fn destroy(&mut self, index: usize, services: &mut impl SceneServices) {
        if self.trails[index] != self.null_trail {
            services.destroy_trail(self.trails[index], 1000);
            self.trails[index] = self.null_trail;
        }
    }
    fn ensure(&mut self, index: usize, kind: Trail, services: &mut impl SceneServices) {
        if self.trails[index] == self.null_trail {
            self.trails[index] = services.create_trail(kind, self.position);
        }
        services.move_trail(self.trails[index], self.position);
    }
}
