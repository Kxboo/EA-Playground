//! Original TetherballHitCompulsion configuration, activation, expiry and think logic.
//!
//! The AI entity owns its ball handle and supplies the already-loaded tunable
//! angle arrays. This component stores only the compulsion object's own fields;
//! rally outputs are written through the existing RallyRuleState.

use crate::tetherball::{BallMotion, wrap_angle};
use crate::tetherball_angles::is_between;
use crate::tetherball_rally_rules::RallyRuleState;

const TETHERBALL_HIT_COMPULSION: &str = "TetherballHitCompulsion";
const ACTIVATION_SPEED: f32 = 0.83;
const MAX_DISTANCE: f32 = 0.15;
const HALF_TURN: f32 = f32::from_bits(0x40490fdb);

/// Four selected-difficulty tuning arrays, each indexed by player-distance
/// enum (0..=2), after the native signed-degree to radians conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitCompulsionTuning {
    pub return_pre: [f32; 3],
    pub return_post: [f32; 3],
    pub accelerate_pre: [f32; 3],
    pub accelerate_post: [f32; 3],
}

impl HitCompulsionTuning {
    /// Load the hit-only tuning arrays selected by the native dare/session
    /// collection and converted from signed Int16 degrees to radians.
    pub fn load(db: &crate::vlt::Database, session_mode: i32, dare: i32) -> Result<Self, String> {
        let rows = crate::tetherball_tuning::ai_hit_angles(db, session_mode, dare)?;
        Ok(Self {
            return_pre: rows[0],
            return_post: rows[1],
            accelerate_pre: rows[2],
            accelerate_post: rows[3],
        })
    }
}

/// Engine-facing boundaries used by the original compulsion.
///
/// AIRand is queried synchronously so activation preserves its number and
/// order of draws. The active-minigame query stands for WorldMan's native
/// GetTetherballMinigame + TBLL ID check.
pub trait HitCompulsionServices {
    fn random_range(&mut self, low: i32, high: i32) -> i32;
    fn is_tetherball_minigame(&mut self) -> bool;
}

/// Stateful projection of `TetherballHitCompulsion`'s relevant native words.
/// Option-valued fields were not initialized by the C++ constructor; keeping
/// them absent avoids inventing values for SlotPool memory that was not proven
/// to be cleared.
#[derive(Debug, Clone, PartialEq)]
pub struct TetherballHitCompulsion {
    /// Base Compulsion owner pointer (+0x04), retained as an opaque handle.
    pub entity_handle: u32,
    /// Base Compulsion active byte (+0x08).
    pub active: bool,
    /// Compulsion priority byte (+0x09), passed by the entity factory.
    pub priority: u8,
    /// AI entity's ball pointer copied to the compulsion's +0x84 binding.
    pub ball_handle_84: Option<u32>,
    /// Chosen hit category (+0x88), also copied to MGTetherball +0x238.
    pub hit_category_88: Option<i32>,
    /// Current hit-attempt result (+0x8c), also copied to MGTetherball +0x234.
    pub hit_attempt_8c: Option<i32>,
    /// Random target angle (+0x90), in wrapped radians.
    pub target_angle_90: f32,
    /// SetAttributes angle (+0x94), in wrapped radians.
    pub base_angle_94: f32,
    /// SetAttributes rate/scale (+0x98).
    pub rate_98: f32,
    /// SetAttributes acceleration-angle selector byte (+0x9c).
    pub acceleration_mode_9c: bool,
    /// SetAttributes distance index (+0xa0), valid values are 0..=2.
    pub player_distance_a0: u32,
    /// SetAttributes maximum allowed power category (+0xa4).
    pub power_type_a4: u32,
    /// The seven AI probability bytes at +0xa8..+0xae.
    pub difficulty_a8_ae: Option<[u8; 7]>,
    /// Selected-difficulty angle tables stored at +0xb0, +0xbc, +0xc8, +0xd4.
    pub angle_tables_b0_d4: Option<HitCompulsionTuning>,
    /// SetAttributes expiry angle (+0xe0), in wrapped radians.
    pub expiry_angle_e0: f32,
}

impl TetherballHitCompulsion {
    /// Native `__ct__23TetherballHitCompulsionFP8AIEntityUc` (`0x80395c18`).
    pub fn new(entity_handle: u32, priority: u8) -> Self {
        Self {
            entity_handle,
            active: false,
            priority,
            ball_handle_84: None,
            hit_category_88: None,
            hit_attempt_8c: None,
            target_angle_90: 0.0,
            base_angle_94: wrap_angle(0.0),
            rate_98: 1.0,
            acceleration_mode_9c: false,
            player_distance_a0: 2,
            power_type_a4: 0,
            difficulty_a8_ae: None,
            angle_tables_b0_d4: None,
            expiry_angle_e0: wrap_angle(0.0),
        }
    }

    /// Bind the AI entity's existing ball handle to native field +0x84.
    pub fn bind_ball_handle(&mut self, ball_handle: u32) {
        self.ball_handle_84 = Some(ball_handle);
    }

    /// Native `SetAttributes` (`0x80395ce4`); the two rmAngle arguments are
    /// copied and wrapped without changing the supplied scalar values.
    pub fn set_attributes(
        &mut self,
        angle: f32,
        rate: f32,
        acceleration_mode: bool,
        player_distance: u32,
        power_type: u32,
        expiry_angle: f32,
    ) {
        self.base_angle_94 = wrap_angle(angle);
        self.rate_98 = rate;
        self.acceleration_mode_9c = acceleration_mode;
        self.player_distance_a0 = player_distance;
        self.power_type_a4 = power_type;
        self.expiry_angle_e0 = wrap_angle(expiry_angle);
    }

    /// Native `SetDifficultyVariables` (`0x80395d64`), seven byte stores.
    pub fn set_difficulty_variables(&mut self, values: [u8; 7]) {
        self.difficulty_a8_ae = Some(values);
    }

    /// Execute `Activate` (`0x80395d84`). `tunables` is the selected VLT
    /// collection's four signed-degree arrays after the original loader has
    /// converted them to radians. A live tetherball minigame and valid distance
    /// index are preconditions, matching the native call domain.
    pub fn activate(
        &mut self,
        ball: &BallMotion,
        tunables: HitCompulsionTuning,
        services: &mut impl HitCompulsionServices,
    ) {
        self.active = true;
        self.angle_tables_b0_d4 = Some(tunables);
        let difficulty = self
            .difficulty_a8_ae
            .expect("native hit compulsion requires SetDifficultyVariables before Activate");

        let distance = self.player_distance_a0 as usize;
        assert!(
            distance < 3,
            "native hit compulsion distance index must be 0..=2"
        );
        let (pre, post) = if self.acceleration_mode_9c {
            (
                tunables.accelerate_pre[distance],
                tunables.accelerate_post[distance],
            )
        } else {
            (
                tunables.return_pre[distance],
                tunables.return_post[distance],
            )
        };

        // Native code multiplies each selected angle by 0.83f, then performs
        // one fused multiply-add with the configured rate and base angle.
        let pre_delta = ACTIVATION_SPEED * pre;
        let post_delta = ACTIVATION_SPEED * post;
        let mut low = (self.rate_98.mul_add(-pre_delta, self.base_angle_94) * 1000.0) as i32;
        let mut high = (self.rate_98.mul_add(post_delta, self.base_angle_94) * 1000.0) as i32;
        if self.rate_98 < 0.0 {
            std::mem::swap(&mut low, &mut high);
        }

        let bonus = match ball.hit_type {
            1 | 3 => difficulty[5],
            7 => difficulty[6],
            _ => 0,
        } as i32;

        // Too-fast and too-slow outcomes shift the angular sampling window.
        if services.random_range(0, 99) < difficulty[0] as i32 + bonus {
            high = low.wrapping_sub(150);
            low = low.wrapping_sub(800);
        } else if services.random_range(0, 99) < difficulty[1] as i32 + bonus {
            low = high.wrapping_add(150);
            high = high.wrapping_add(800);
        }

        let sample = if low < high {
            services.random_range(low, high)
        } else {
            services.random_range(high, low)
        };
        self.target_angle_90 = wrap_angle(sample as f32 / 1000.0);

        let is_zone_zero = ball.zone == crate::tetherball::Zone::Zero;
        let mut category = if is_zone_zero { 0 } else { 1 };
        if services.random_range(0, 99) < difficulty[4] as i32 && self.power_type_a4 == 5 {
            category = 7;
        } else if is_zone_zero {
            if services.random_range(0, 99) < difficulty[2] as i32 {
                category = if services.random_range(0, 99) < difficulty[3] as i32
                    && self.power_type_a4 >= 1
                {
                    3
                } else {
                    2
                };
            } else {
                category = if services.random_range(0, 99) < difficulty[3] as i32
                    && self.power_type_a4 >= 1
                {
                    1
                } else {
                    0
                };
            }
        } else {
            if services.random_range(0, 99) < difficulty[2] as i32 {
                category = if services.random_range(0, 99) < difficulty[3] as i32
                    && self.power_type_a4 > 1
                {
                    1
                } else {
                    0
                };
            } else {
                category = if services.random_range(0, 99) < difficulty[3] as i32
                    && self.power_type_a4 > 1
                {
                    3
                } else {
                    2
                };
            }
        }
        self.hit_category_88 = Some(category);
    }

    /// Execute `HasExpired` (`0x80396214`): tests the ball angle against the
    /// stored expiry angle and its half-turn endpoint. The native reverse flag
    /// is the signed comparison `rate_98 > 0.0`.
    pub fn has_expired(&self, ball: &BallMotion) -> bool {
        is_between(
            wrap_angle(ball.angle),
            self.expiry_angle_e0,
            wrap_angle(self.expiry_angle_e0 + HALF_TURN),
            self.rate_98 > 0.0,
        )
    }

    /// Execute `Think` (`0x80396288`). The elapsed-time argument is accepted
    /// by native code but not read. When the wrapped angle difference is
    /// strictly less than 0.15 radians, the current category becomes the hit attempt;
    /// otherwise the attempt is -1. The original writes both words to the
    /// shared MGTetherball object only when the world reports the TBLL minigame.
    pub fn think(
        &mut self,
        ball: &BallMotion,
        _milliseconds: i32,
        rally: &mut RallyRuleState,
        services: &mut impl HitCompulsionServices,
    ) {
        let difference = (wrap_angle(ball.angle) - self.target_angle_90).abs();
        let attempt = if difference < MAX_DISTANCE {
            self.hit_category_88
                .expect("native hit compulsion must be activated before Think")
        } else {
            -1
        };
        self.hit_attempt_8c = Some(attempt);
        if services.is_tetherball_minigame() {
            rally.ai_hit_attempt_234 = attempt;
            rally.ai_power_hit_type_238 = self
                .hit_category_88
                .expect("native hit compulsion must be activated before Think");
        }
    }

    /// Native trivial virtuals: interruptible, fixed name, and one active byte.
    pub fn is_interruptible(&self) -> bool {
        true
    }

    pub fn name(&self) -> &'static str {
        TETHERBALL_HIT_COMPULSION
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn f32bits(value: &Value) -> f32 {
        f32::from_bits(value.as_u64().unwrap() as u32)
    }

    fn i32v(value: &Value) -> i32 {
        value.as_i64().unwrap() as i32
    }

    fn attrs(value: &Value, compulsion: &mut TetherballHitCompulsion) {
        compulsion.set_attributes(
            f32bits(&value["angle_bits"]),
            f32bits(&value["rate_bits"]),
            value["acceleration_mode"].as_bool().unwrap(),
            value["distance"].as_u64().unwrap() as u32,
            value["power_type"].as_u64().unwrap() as u32,
            f32bits(&value["expiry_angle_bits"]),
        );
    }

    fn ball(angle: f32, hit_type: i32, zone: i32) -> BallMotion {
        BallMotion {
            angle,
            hit_angle: 0.0,
            secondary_angle: 0.0,
            target_velocity: 0.0,
            secondary_target_velocity: 0.0,
            spin_acceleration: 0.0,
            hit_direction: crate::tetherball::Direction::Zero,
            grabbed: false,
            vertical_velocity: 0.0,
            toss_time: 0,
            angular_velocity: 0.0,
            acceleration: 0.0,
            secondary_acceleration: 0.0,
            secondary_velocity: 0.0,
            hit_type,
            direction: crate::tetherball::Direction::Zero,
            zone: if zone == 0 {
                crate::tetherball::Zone::Zero
            } else {
                crate::tetherball::Zone::One
            },
            radius: 1.0,
            desired_radius: 1.0,
            height: 0.0,
            target_height: 0.0,
            base_hit_speed: 0.0,
            power_modifier: 0.0,
            mega_modifier: 0.0,
            pole_height: 0.0,
            tossed: false,
            spinning_up: false,
            spinning_down: false,
        }
    }

    #[derive(Default)]
    struct FixtureServices {
        raw: Vec<i32>,
        index: usize,
        random_calls: Vec<(i32, i32, i32)>,
        tetherball: bool,
        minigame_queries: usize,
    }

    impl HitCompulsionServices for FixtureServices {
        fn random_range(&mut self, low: i32, high: i32) -> i32 {
            let raw = self.raw[self.index];
            self.index += 1;
            let (lo, hi) = (low.min(high), low.max(high));
            let value = (lo as i64 + raw as i64 % (hi as i64 - lo as i64 + 1)) as i32;
            self.random_calls.push((low, high, value));
            value
        }

        fn is_tetherball_minigame(&mut self) -> bool {
            self.minigame_queries += 1;
            self.tetherball
        }
    }

    fn hit_corpus() -> Value {
        let corpus: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_ai_hit_golden.json"))
                .unwrap();
        assert_eq!(corpus["elf_sha256"], crate::recovered::ELF_SHA256);
        corpus
    }

    fn tuning_from_bits(rows: &Value) -> HitCompulsionTuning {
        let row =
            |i: usize| std::array::from_fn(|j| f32::from_bits(rows[i][j].as_u64().unwrap() as u32));
        HitCompulsionTuning {
            return_pre: row(0),
            return_post: row(1),
            accelerate_pre: row(2),
            accelerate_post: row(3),
        }
    }

    #[test]
    fn original_constructor_setters_virtuals_and_predicates() {
        let corpus = hit_corpus();
        for case in corpus["cases"].as_array().unwrap() {
            let kind = case["kind"].as_str().unwrap();
            let expected = &case["expected"];
            let state = &expected["state"];
            let mut compulsion = TetherballHitCompulsion::new(
                state["owner"].as_u64().unwrap() as u32,
                case["priority"].as_u64().unwrap() as u8,
            );
            match kind {
                "construct" => {
                    assert!(!compulsion.active);
                    assert_eq!(
                        compulsion.priority,
                        state["priority"].as_u64().unwrap() as u8
                    );
                    assert_eq!(
                        compulsion.target_angle_90.to_bits(),
                        state["target_angle_bits"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.base_angle_94.to_bits(),
                        state["base_angle_bits"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.rate_98.to_bits(),
                        state["rate_bits"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.player_distance_a0,
                        state["player_distance"].as_u64().unwrap() as u32
                    );
                    assert_eq!(compulsion.ball_handle_84, None);
                    assert_eq!(compulsion.hit_category_88, None);
                    assert_eq!(compulsion.hit_attempt_8c, None);
                    assert_eq!(compulsion.difficulty_a8_ae, None);
                    assert_eq!(compulsion.angle_tables_b0_d4, None);
                }
                "set_attributes" => {
                    attrs(&case["attributes"], &mut compulsion);
                    assert_eq!(
                        compulsion.base_angle_94.to_bits(),
                        state["base_angle_bits"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.rate_98.to_bits(),
                        state["rate_bits"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.acceleration_mode_9c,
                        state["acceleration_mode"].as_bool().unwrap()
                    );
                    assert_eq!(
                        compulsion.player_distance_a0,
                        state["player_distance"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.power_type_a4,
                        state["power_type"].as_u64().unwrap() as u32
                    );
                    assert_eq!(
                        compulsion.expiry_angle_e0.to_bits(),
                        state["expiry_angle_bits"].as_u64().unwrap() as u32
                    );
                }
                "set_difficulty_variables" => {
                    let values =
                        std::array::from_fn(|i| case["difficulty"][i].as_u64().unwrap() as u8);
                    compulsion.set_difficulty_variables(values);
                    assert_eq!(
                        compulsion.difficulty_a8_ae,
                        Some(std::array::from_fn(
                            |i| state["difficulty"][i].as_u64().unwrap() as u8
                        ))
                    );
                }
                "has_expired" => {
                    attrs(&case["attributes"], &mut compulsion);
                    let b = ball(f32bits(&case["ball_angle_bits"]), 0, 0);
                    assert_eq!(
                        compulsion.has_expired(&b),
                        expected["result"].as_i64().unwrap() != 0,
                        "{case}"
                    );
                }
                "activate_base" => {
                    compulsion.active = false;
                    compulsion.active = true;
                    assert!(compulsion.active);
                }
                "deactivate" => {
                    compulsion.active = true;
                    compulsion.deactivate();
                    assert!(!compulsion.active);
                }
                "is_interruptible" => assert_eq!(
                    compulsion.is_interruptible(),
                    expected["result"].as_i64().unwrap() != 0
                ),
                "get_name" => assert_eq!(compulsion.name(), expected["result"].as_str().unwrap()),
                _ => {}
            }
        }
    }

    #[test]
    fn original_activate_random_order_outputs_and_think_gate() {
        let corpus = hit_corpus();
        for case in corpus["cases"].as_array().unwrap() {
            let kind = case["kind"].as_str().unwrap();
            let state = &case["expected"]["state"];
            if kind == "activate" {
                let mut compulsion = TetherballHitCompulsion::new(
                    0x71020000,
                    case["priority"].as_u64().unwrap() as u8,
                );
                compulsion.bind_ball_handle(0x71000000);
                attrs(&case["attributes"], &mut compulsion);
                compulsion.set_difficulty_variables(std::array::from_fn(|i| {
                    case["difficulty"][i].as_u64().unwrap() as u8
                }));
                let b = ball(
                    f32bits(&case["ball_angle_bits"]),
                    i32v(&case["ball_hit_type"]),
                    i32v(&case["ball_zone"]),
                );
                let mut services = FixtureServices {
                    raw: case["random_values"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_i64().unwrap() as i32)
                        .collect(),
                    ..Default::default()
                };
                compulsion.activate(&b, tuning_from_bits(&case["tuning_bits"]), &mut services);
                assert!(compulsion.active);
                assert_eq!(
                    compulsion.hit_category_88,
                    Some(i32v(&state["hit_category"])),
                    "{case}"
                );
                assert_eq!(
                    compulsion.target_angle_90.to_bits(),
                    state["target_angle_bits"].as_u64().unwrap() as u32,
                    "{case}"
                );
                assert_eq!(
                    compulsion.base_angle_94.to_bits(),
                    state["base_angle_bits"].as_u64().unwrap() as u32
                );
                assert_eq!(
                    compulsion.rate_98.to_bits(),
                    state["rate_bits"].as_u64().unwrap() as u32
                );
                assert_eq!(
                    compulsion.angle_tables_b0_d4,
                    Some(tuning_from_bits(&case["tuning_bits"]))
                );
                let expected_calls: Vec<_> = case["expected"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| e[0] == "random")
                    .map(|e| {
                        (
                            e[1].as_i64().unwrap() as i32,
                            e[2].as_i64().unwrap() as i32,
                            e[3].as_i64().unwrap() as i32,
                        )
                    })
                    .collect();
                assert_eq!(services.random_calls, expected_calls, "{case}");
            } else if kind == "think" {
                let mut compulsion = TetherballHitCompulsion::new(0x71020000, 50);
                compulsion.hit_category_88 = Some(i32v(&case["hit_category"]));
                compulsion.target_angle_90 = f32bits(&case["target_angle_bits"]);
                let b = ball(f32bits(&case["ball_angle_bits"]), 0, 0);
                let mut rally = RallyRuleState {
                    ai_hit_attempt_234: 0,
                    ai_power_hit_type_238: 0,
                    ai_charge: [0; 2],
                };
                let mut services = FixtureServices {
                    tetherball: case["tetherball_game"].as_bool().unwrap(),
                    ..Default::default()
                };
                compulsion.think(
                    &b,
                    case["milliseconds"].as_i64().unwrap_or(16) as i32,
                    &mut rally,
                    &mut services,
                );
                assert_eq!(
                    compulsion.hit_attempt_8c,
                    Some(i32v(&state["hit_attempt"])),
                    "{case}"
                );
                assert_eq!(
                    rally.ai_hit_attempt_234,
                    i32v(&case["expected"]["rally"][0]),
                    "{case}"
                );
                assert_eq!(
                    rally.ai_power_hit_type_238,
                    i32v(&case["expected"]["rally"][1]),
                    "{case}"
                );
                assert_eq!(services.minigame_queries, 1);
            }
        }
    }
}
