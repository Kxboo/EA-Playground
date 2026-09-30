//! TetherballAIEntity initialization, geometric predicates and priority selection.
use crate::area_transform::Matrix;
use crate::tetherball_angles::{is_between, wrap_angle};

#[derive(Clone, Debug)]
pub struct AiEntity {
    pub handle: u32,
    pub player: usize,
    pub ball_handle: u32,
    pub angle: f32,
    pub direction_scale: f32,
    pub difficulty: [u8; 7],
    pub enabled: bool,
    pub heading: f32,
}

#[derive(Clone, Debug)]
pub struct AiGeometry {
    pub world: Matrix,
    pub inverse: Matrix,
    pub anchor: [f32; 3],
    pub character: [f32; 3],
    pub desired_radius: f32,
}

/// rmMult(Vector3, Matrix4), including single precision multiply-add ordering.
pub fn transform_point(v: [f32; 3], m: &Matrix) -> [f32; 3] {
    std::array::from_fn(|i| {
        1.0f32.mul_add(
            m[12 + i],
            v[2].mul_add(m[8 + i], v[1].mul_add(m[4 + i], v[0] * m[i])),
        )
    })
}

pub fn is_in_gameplay(state: u32) -> bool {
    matches!(state, 28 | 29)
}
pub fn is_swinging(animation: i32) -> bool {
    matches!(animation, 63 | 69 | 75 | 81 | 91)
}

pub fn is_in_position(state: u32, geometry: &AiGeometry, tolerance: f32) -> bool {
    if !is_in_gameplay(state) {
        return true;
    }
    let mut target = transform_point(geometry.character, &geometry.inverse);
    let anchor = transform_point(geometry.anchor, &geometry.inverse);
    target[0] = if anchor[0] > target[0] {
        (anchor[0] - geometry.desired_radius) - 0.55
    } else {
        0.55 + (anchor[0] + geometry.desired_radius)
    };
    let target = transform_point(target, &geometry.world);
    let dx = geometry.character[0] - target[0];
    let dz = geometry.character[2] - target[2];
    dx.mul_add(dx, dz * dz) < tolerance * tolerance
}

impl AiEntity {
    /// Typed constructor after allocation; caller supplies the otherwise
    /// uninitialized direction scale. Shared +70 charge is initialized by the
    /// composition adapter, and +6c/+74 remain owned by serve/lifecycle state.
    pub fn new(handle: u32, player: usize, direction_scale: f32) -> Self {
        assert!(player < 2);
        Self {
            handle,
            player,
            ball_handle: 0,
            angle: 0.,
            direction_scale,
            difficulty: [0; 7],
            enabled: false,
            heading: 0.,
        }
    }

    pub fn initialize(
        &mut self,
        game: Option<(i32, i32)>,
        enabled: bool,
        difficulty: u32,
        heading: f32,
        db: &crate::vlt::Database,
    ) -> Result<(), String> {
        if let Some((session, dare)) = game {
            // The native routine acquires a collection even when disabled.
            let name = crate::tetherball_tuning::collection_name(session, dare)
                .ok_or("native null tuning collection")?;
            if db.find_collection("mg_tetherball", name).is_none() {
                return Err("missing AI tuning collection".into());
            }
            self.enabled = enabled;
            if enabled {
                self.difficulty =
                    crate::tetherball_tuning::ai_difficulty(db, session, dare, difficulty)?;
            }
        }
        self.heading = wrap_angle(heading);
        Ok(())
    }

    pub fn select(
        &self,
        game_state: Option<u32>,
        priority: u8,
        animation: i32,
        geometry: &AiGeometry,
        ball_angle: f32,
    ) -> Selection {
        let Some(state) = game_state else {
            return Selection::None;
        };
        if !is_in_gameplay(state) {
            return Selection::None;
        }
        if priority < 40 && !is_in_position(state, geometry, 0.002) && !is_swinging(animation) {
            return Selection::Move;
        }
        if priority < 50
            && self.enabled
            && is_between(
                wrap_angle(ball_angle),
                self.heading,
                wrap_angle(self.heading + std::f32::consts::PI),
                self.direction_scale < 0.,
            )
        {
            return Selection::Hit;
        }
        Selection::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    None,
    Move,
    Hit,
}

#[derive(Debug)]
pub enum Compulsion {
    Move(crate::tetherball_ai_move::MoveCompulsion),
    Hit(crate::tetherball_ai_hit::TetherballHitCompulsion),
}

impl AiEntity {
    /// Compose MoveCompulsion's expiry checks with the recovered live AI
    /// predicates. Only the world/minigame presence result comes from the host.
    pub fn move_has_expired(
        &self,
        compulsion: &crate::tetherball_ai_move::MoveCompulsion,
        life: &crate::tetherball_lifecycle::Lifecycle,
        inputs: &crate::tetherball_ai_move::MoveInputs,
        minigame_present: bool,
    ) -> bool {
        struct Queries<'a> {
            entity: &'a AiEntity,
            life: &'a crate::tetherball_lifecycle::Lifecycle,
            present: bool,
        }
        impl crate::tetherball_ai_move::MoveServices for Queries<'_> {
            fn tetherball_minigame_is_active(&mut self) -> bool {
                self.present
            }
            fn is_swinging(&mut self, handle: u32) -> bool {
                assert_eq!(handle, self.entity.handle);
                is_swinging(self.life.players[self.entity.player].current_animation)
            }
            fn is_in_position(
                &mut self,
                handle: u32,
                inputs: &crate::tetherball_ai_move::MoveInputs,
                tolerance: f32,
            ) -> bool {
                assert_eq!(handle, self.entity.handle);
                is_in_position(
                    self.life.match_state.state_code,
                    &AiGeometry {
                        world: inputs.world,
                        inverse: inputs.inverse,
                        anchor: inputs.anchor,
                        character: inputs.character,
                        desired_radius: inputs.radius,
                    },
                    tolerance,
                )
            }
        }
        compulsion.has_expired(
            life,
            inputs,
            &mut Queries {
                entity: self,
                life,
                present: minigame_present,
            },
        )
    }

    /// Complete EvaluateCompulsions, constructing the decoded component and
    /// copying the live shared fields rather than retaining AI shadow copies.
    pub fn evaluate(
        &self,
        lifecycle: Option<&crate::tetherball_lifecycle::Lifecycle>,
        priority: u8,
        geometry: &AiGeometry,
        ball: &crate::tetherball::BallMotion,
        serve: &crate::tetherball_serve::ServeState,
        rally: &crate::tetherball_rally_rules::RallyRuleState,
    ) -> Option<Compulsion> {
        let life = lifecycle?;
        match self.select(
            Some(life.match_state.state_code),
            priority,
            life.players[self.player].current_animation,
            geometry,
            ball.angle,
        ) {
            Selection::None => None,
            Selection::Move => {
                let mut c = crate::tetherball_ai_move::MoveCompulsion::new(self.handle, 40);
                c.bind_move_object(self.ball_handle);
                Some(Compulsion::Move(c))
            }
            Selection::Hit => {
                let mut c = crate::tetherball_ai_hit::TetherballHitCompulsion::new(self.handle, 50);
                c.bind_ball_handle(self.ball_handle);
                c.set_attributes(
                    self.angle,
                    self.direction_scale,
                    serve.ai_waiting[self.player],
                    life.players[self.player].ai_distance as u32,
                    rally.ai_charge[self.player],
                    self.heading,
                );
                c.set_difficulty_variables(self.difficulty);
                Some(Compulsion::Hit(c))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn f(v: &Value) -> f32 {
        f32::from_bits(v.as_u64().unwrap() as u32)
    }
    fn floats<const N: usize>(v: &Value) -> [f32; N] {
        std::array::from_fn(|i| f(&v[i]))
    }
    fn geometry(c: &Value) -> AiGeometry {
        AiGeometry {
            world: floats(&c["world"]),
            inverse: floats(&c["inverse"]),
            anchor: floats(&c["anchor"]),
            character: floats(&c["character"]),
            desired_radius: f(&c["radius"]),
        }
    }
    #[test]
    fn original_entity_predicates_and_selection() {
        let d: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_ai_golden.json")).unwrap();
        assert_eq!(d["elf_sha256"], crate::recovered::ELF_SHA256);
        for c in d["transforms"].as_array().unwrap() {
            assert_eq!(
                transform_point(floats(&c["vector"]), &floats(&c["matrix"])).map(f32::to_bits),
                std::array::from_fn(|i| c["result"][i].as_u64().unwrap() as u32),
                "{c}"
            );
        }
        for c in d["swinging"].as_array().unwrap() {
            assert_eq!(
                is_swinging(c["animation"].as_i64().unwrap() as i32),
                c["result"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in d["gameplay"].as_array().unwrap() {
            assert_eq!(
                is_in_gameplay(c["state"].as_i64().unwrap() as u32),
                c["result"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in d["positions"].as_array().unwrap() {
            assert_eq!(
                is_in_position(
                    c["state"].as_u64().unwrap() as u32,
                    &geometry(c),
                    f(&c["tolerance"])
                ),
                c["result"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in d["evaluations"].as_array().unwrap() {
            let mut ai = AiEntity::new(0x71700000, 0, f(&c["scale"]));
            ai.enabled = c["enabled"].as_bool().unwrap();
            ai.heading = f(&c["heading"]);
            let selected = ai.select(
                c["present"]
                    .as_bool()
                    .unwrap()
                    .then_some(c["state"].as_u64().unwrap() as u32),
                c["priority"].as_u64().unwrap() as u8,
                c["animation"].as_i64().unwrap() as i32,
                &geometry(c),
                f(&c["ball_angle"]),
            );
            assert_eq!(
                selected,
                match c["kind"].as_str().unwrap() {
                    "none" => Selection::None,
                    "move" => Selection::Move,
                    "hit" => Selection::Hit,
                    _ => panic!("invalid native vtable"),
                },
                "{c}"
            );
        }
    }
    #[test]
    fn original_evaluate_constructs_live_compulsions() {
        let d: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_ai_golden.json")).unwrap();
        let seed: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_lifecycle_golden.json"
        ))
        .unwrap();
        let mut life: crate::tetherball_lifecycle::Lifecycle =
            serde_json::from_value(seed["cases"][1]["initial"].clone()).unwrap();
        let mut ball = crate::tetherball::tests::ball(&seed["cases"][1]["ball"]);
        let mut serve = crate::tetherball_serve::ServeState {
            pause_block_count_0fc: 0,
            pause_menu_open: false,
            power_serve_enabled: false,
            return_angles: [0.; 2],
            power_animations: [0; 2],
            high_animations: [0; 2],
            voice_types: [0; 2],
            ai_waiting: [false; 2],
            forced_ai: [false; 2],
            frontend_flags: [false; 2],
        };
        let mut rally = crate::tetherball_rally_rules::RallyRuleState {
            ai_hit_attempt_234: -1,
            ai_power_hit_type_238: 0,
            ai_charge: [0; 2],
        };
        for c in d["evaluations"].as_array().unwrap() {
            // Exercise both Rust player mappings against the same native entity.
            for player in 0..2 {
                let mut ai = AiEntity::new(0x71700000, player, f(&c["scale"]));
                ai.ball_handle = 0x71703000;
                ai.angle = f(&c["angle"]);
                ai.heading = f(&c["heading"]);
                ai.enabled = c["enabled"].as_bool().unwrap();
                ai.difficulty = std::array::from_fn(|i| c["difficulty"][i].as_u64().unwrap() as u8);
                life.match_state.state_code = c["state"].as_u64().unwrap() as u32;
                life.players[player].current_animation = c["animation"].as_i64().unwrap() as i32;
                life.players[player].ai_distance = c["distance"].as_i64().unwrap() as i32;
                serve.ai_waiting[player] = c["waiting"].as_bool().unwrap();
                rally.ai_charge[player] = c["charge"].as_u64().unwrap() as u32;
                ball.angle = f(&c["ball_angle"]);
                let got = ai.evaluate(
                    c["present"].as_bool().unwrap().then_some(&life),
                    c["priority"].as_u64().unwrap() as u8,
                    &geometry(c),
                    &ball,
                    &serve,
                    &rally,
                );
                if c["kind"] == "none" {
                    assert!(got.is_none());
                    continue;
                }
                let raw = c["compulsion_bytes"].as_str().unwrap();
                let word = |offset: usize| {
                    u32::from_str_radix(&raw[offset * 2..offset * 2 + 8], 16).unwrap()
                };
                let byte = |offset: usize| {
                    u8::from_str_radix(&raw[offset * 2..offset * 2 + 2], 16).unwrap()
                };
                match got.unwrap() {
                    Compulsion::Hit(h) => {
                        assert_eq!(c["kind"], "hit");
                        assert_eq!(h.entity_handle, word(4));
                        assert_eq!(h.priority, byte(9));
                        assert_eq!(h.ball_handle_84, Some(word(0x84)));
                        assert_eq!(h.base_angle_94.to_bits(), word(0x94));
                        assert_eq!(h.rate_98.to_bits(), word(0x98));
                        assert_eq!(h.acceleration_mode_9c, byte(0x9c) != 0);
                        assert_eq!(h.player_distance_a0, word(0xa0));
                        assert_eq!(h.power_type_a4, word(0xa4));
                        assert_eq!(h.expiry_angle_e0.to_bits(), word(0xe0));
                        assert_eq!(
                            h.difficulty_a8_ae,
                            Some(std::array::from_fn(|i| byte(0xa8 + i)))
                        );
                    }
                    Compulsion::Move(m) => {
                        assert_eq!(c["kind"], "move");
                        assert_eq!(m.ai_entity_handle_004, word(4));
                        assert_eq!(m.ai_entity_handle_084, word(0x84));
                        assert_eq!(m.priority_009, byte(9));
                        assert_eq!(m.bound_move_object_088, Some(word(0x88)));
                    }
                }
            }
        }
    }
    #[test]
    fn original_move_expiry_with_live_predicates() {
        let d: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_ai_move_golden.json"))
                .unwrap();
        assert_eq!(d["elf_sha256"], crate::recovered::ELF_SHA256);
        let seed: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_lifecycle_golden.json"
        ))
        .unwrap();
        let mut life: crate::tetherball_lifecycle::Lifecycle =
            serde_json::from_value(seed["cases"][1]["initial"].clone()).unwrap();
        for player in 0..2 {
            let ai = AiEntity::new(0x71700000, player, 1.);
            let comp = crate::tetherball_ai_move::MoveCompulsion::new(ai.handle, 40);
            for c in d["has_expired"].as_array().unwrap() {
                life.match_state.state_code = c["state"].as_u64().unwrap() as u32;
                life.players[player].current_animation = c["animation"].as_i64().unwrap() as i32;
                let inputs = crate::tetherball_ai_move::MoveInputs {
                    world: crate::area_transform::IDENTITY,
                    inverse: crate::area_transform::IDENTITY,
                    anchor: [0., 1., 0.],
                    radius: 0.,
                    character: [
                        if c["near"].as_bool().unwrap() {
                            0.55
                        } else {
                            -2.
                        },
                        1.,
                        0.,
                    ],
                };
                assert_eq!(
                    ai.move_has_expired(
                        &comp,
                        &life,
                        &inputs,
                        c["present"].as_bool().unwrap() && c["mgid"].as_bool().unwrap()
                    ),
                    c["result"].as_bool().unwrap(),
                    "{c}"
                );
            }
        }
    }
    #[test]
    fn original_initialization_with_corpus() {
        let d: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_ai_golden.json")).unwrap();
        let path = crate::bridge::data_root().join("files/data/db");
        let db = crate::vlt::Database::load(
            &std::fs::read(path.join("db.vlt")).unwrap(),
            &std::fs::read(path.join("db.bin")).unwrap(),
            crate::vlt::known_names(),
        )
        .unwrap();
        for c in d["hit_tables"].as_array().unwrap() {
            let t = crate::tetherball_ai_hit::HitCompulsionTuning::load(
                &db,
                c["session"].as_i64().unwrap() as i32,
                c["dare"].as_i64().unwrap() as i32,
            )
            .unwrap();
            assert_eq!(
                serde_json::json!(
                    [
                        t.return_pre,
                        t.return_post,
                        t.accelerate_pre,
                        t.accelerate_post
                    ]
                    .map(|row| row.map(f32::to_bits))
                ),
                c["angles"],
                "{c}"
            );
        }
        for c in d["initializations"].as_array().unwrap() {
            let mut ai = AiEntity::new(1, 0, 0.);
            ai.enabled = true;
            ai.difficulty = [11, 22, 33, 44, 55, 66, 77];
            ai.initialize(
                c["present"]
                    .as_bool()
                    .unwrap()
                    .then_some((1, c["dare"].as_i64().unwrap() as i32)),
                c["enabled"].as_bool().unwrap(),
                c["difficulty"].as_u64().unwrap() as u32,
                f(&c["angle"]),
                &db,
            )
            .unwrap();
            for i in 0..7 {
                assert_eq!(
                    ai.difficulty[i] as u64,
                    c["result"][i].as_u64().unwrap(),
                    "{c}"
                );
            }
            assert_eq!(ai.enabled, c["result"][7].as_u64().unwrap() != 0, "{c}");
            assert_eq!(
                ai.heading.to_bits() as u64,
                c["heading"].as_u64().unwrap(),
                "{c}"
            );
        }
    }
}
