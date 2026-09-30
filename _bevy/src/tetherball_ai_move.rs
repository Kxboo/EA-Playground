//! Original `TetherballMoveCompulsion` state and movement decision.
//!
//! This is a typed projection of the native `Compulsion` object. Fields that
//! the constructors leave untouched remain `Option`s until `Think` stores
//! them, so the projection never invents values for slot-pool memory.

use crate::area_transform::Matrix;
use crate::tetherball_ai::transform_point;
use crate::tetherball_angles::wrap_angle;
use crate::tetherball_lifecycle::Lifecycle;

const VTABLE: u32 = 0x804d_cb7c;
const TETHERBALL_MOVE_COMPULSION_NAME: &str = "TetherballMoveCompulsion";
const POSITION_TOLERANCE: f32 = f32::from_bits(0x3a83_126f);
const OFFSET: f32 = f32::from_bits(0x3f0c_cccd); // 0.55f

/// World and character data read by the native `Think` routine. `anchor` and
/// `radius` are the bound move object's +0x60 vector and +0xb4 radius;
/// matrices and character position are MGTetherball +0x398/+0x3d8 and the
/// character +0x180 vector respectively.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveInputs {
    pub world: Matrix,
    pub inverse: Matrix,
    pub anchor: [f32; 3],
    pub radius: f32,
    pub character: [f32; 3],
}

/// Compute the native desired position in world space. The character is
/// transformed to tetherball-local coordinates, its X coordinate is placed
/// just beyond the anchor/radius, and the result is transformed back.
pub fn target_position(inputs: &MoveInputs) -> [f32; 3] {
    let mut local_character = transform_point(inputs.character, &inputs.inverse);
    let local_anchor = transform_point(inputs.anchor, &inputs.inverse);
    // Native `fcmpu; ble` takes the addition arm for finite equality. On an
    // unordered compare `ble` is false, so NaN follows the subtraction arm.
    local_character[0] = if !(local_anchor[0] <= local_character[0]) {
        (local_anchor[0] - inputs.radius) - OFFSET
    } else {
        OFFSET + (local_anchor[0] + inputs.radius)
    };
    transform_point(local_character, &inputs.world)
}

/// Synchronous queries made by native `HasExpired`. The first method covers
/// WorldMan lookup and the returned object's tetherball MGID check. The other
/// two mirror the AIEntity virtual calls and preserve their short-circuit
/// order; implementations should query live animation/position state.
pub trait MoveServices {
    fn tetherball_minigame_is_active(&mut self) -> bool;
    fn is_swinging(&mut self, ai_entity_handle: u32) -> bool;
    fn is_in_position(
        &mut self,
        ai_entity_handle: u32,
        inputs: &MoveInputs,
        tolerance: f32,
    ) -> bool;
}

/// Relevant base and derived fields of the retail `Compulsion` object.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveCompulsion {
    /// Base vtable pointer after the derived constructor.
    pub vtable: u32,
    /// AIEntity pointer at +0x04.
    pub ai_entity_handle_004: u32,
    /// Base active byte at +0x08.
    pub active_008: bool,
    /// Caller-supplied priority byte at +0x09.
    pub priority_009: u8,
    /// Base timer at +0x10; the base constructor initializes it to -1.
    pub timer_010: i32,
    /// Target vector at +0x20, left uninitialized by the constructors.
    pub target_position_020: Option<[f32; 3]>,
    /// Target rmAngle value at +0x30, initialized to zero by the base ctor.
    pub target_angle_030: f32,
    /// rmAngle validity byte at +0x34; the base constructor clears it.
    pub target_angle_valid_034: bool,
    /// Threshold at +0x40, not initialized until Think.
    pub threshold_040: Option<f32>,
    /// Word at +0x44, not initialized until Think.
    pub field_044: Option<i32>,
    /// Byte at +0x48, not initialized until Think.
    pub field_048: Option<bool>,
    /// rmAngle value at +0x60; base constructor initializes the float to zero.
    pub angle_060: f32,
    /// The byte at +0x64 is not initialized by the native constructor.
    pub angle_valid_064: Option<bool>,
    /// rmAngle value at +0x70; base constructor initializes the float to zero.
    pub angle_070: f32,
    /// The byte at +0x74 is not initialized by the native constructor.
    pub angle_valid_074: Option<bool>,
    /// Base byte at +0x80, cleared by the constructor and Think.
    pub flag_080: bool,
    /// Derived AIEntity pointer at +0x84.
    pub ai_entity_handle_084: u32,
    /// Bound move-object pointer at +0x88; native constructor leaves it alone.
    pub bound_move_object_088: Option<u32>,
    /// Derived byte at +0x8c. Native code tests for exactly 1.
    pub think_gate_08c: u8,
}

impl MoveCompulsion {
    /// Execute `__ct__24TetherballMoveCompulsionFP8AIEntityUc` at 0x80395848.
    pub fn new(ai_entity_handle: u32, priority: u8) -> Self {
        Self {
            vtable: VTABLE,
            ai_entity_handle_004: ai_entity_handle,
            active_008: false,
            priority_009: priority,
            timer_010: -1,
            target_position_020: None,
            target_angle_030: 0.0,
            target_angle_valid_034: false,
            threshold_040: None,
            field_044: None,
            field_048: None,
            angle_060: 0.0,
            angle_valid_064: None,
            angle_070: 0.0,
            angle_valid_074: None,
            flag_080: false,
            ai_entity_handle_084: ai_entity_handle,
            bound_move_object_088: None,
            think_gate_08c: 0,
        }
    }

    /// Record the bound move object that EvaluateCompulsions stores at +0x88.
    pub fn bind_move_object(&mut self, handle: u32) {
        self.bound_move_object_088 = Some(handle);
    }

    /// Native `Activate` at 0x803963ac.
    pub fn activate(&mut self) {
        self.active_008 = true;
    }

    /// Native `Deactivate` at 0x803963a0.
    pub fn deactivate(&mut self) {
        self.active_008 = false;
    }

    /// Native `IsInterruptible` at 0x8039638c.
    pub const fn is_interruptible(&self) -> bool {
        true
    }

    /// Native `GetName` at 0x80396394.
    pub const fn get_name(&self) -> &'static str {
        TETHERBALL_MOVE_COMPULSION_NAME
    }

    /// Native `HasExpired` at 0x803958d8. Dependency queries run only after
    /// the earlier guards pass, in the exact order used by the retail code.
    pub fn has_expired(
        &self,
        lifecycle: &Lifecycle,
        inputs: &MoveInputs,
        services: &mut impl MoveServices,
    ) -> bool {
        if !services.tetherball_minigame_is_active() {
            return true;
        }
        if !crate::tetherball_ai::is_in_gameplay(lifecycle.match_state.state_code) {
            return true;
        }
        if services.is_swinging(self.ai_entity_handle_084) {
            return true;
        }
        services.is_in_position(
            self.ai_entity_handle_084,
            inputs,
            POSITION_TOLERANCE,
        )
    }

    /// Native `Think` at 0x80395990. The elapsed-time argument is ignored.
    /// Matrix and position values are explicit caller inputs from the active
    /// tetherball/character objects. Returns false for the native null binding
    /// or exact gate-byte check; otherwise writes every scalar/vector store.
    pub fn think(&mut self, _milliseconds: i32, inputs: &MoveInputs) -> bool {
        let binding = self
            .bound_move_object_088
            .expect("native Think requires the move-object pointer written by EvaluateCompulsions");
        if binding == 0 || self.think_gate_08c == 1 {
            return false;
        }

        let target = target_position(inputs);
        let local_anchor = transform_point(inputs.anchor, &inputs.inverse);
        let world_anchor = transform_point(local_anchor, &inputs.world);
        let delta = [
            world_anchor[0] - inputs.character[0],
            world_anchor[1] - inputs.character[1],
            world_anchor[2] - inputs.character[2],
        ];
        // rmAngle(Vector3) calls atan2(x, z), rounds to f32, then Wraps.
        let angle = wrap_angle((delta[0] as f64).atan2(delta[2] as f64) as f32);

        self.timer_010 = 0;
        self.target_position_020 = Some(target);
        self.target_angle_030 = angle;
        self.target_angle_valid_034 = true;
        self.threshold_040 = Some(POSITION_TOLERANCE);
        self.field_044 = Some(0);
        self.field_048 = Some(true);
        self.flag_080 = false;
        self.think_gate_08c = 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn word(bytes: &str, offset: usize) -> u32 {
        u32::from_str_radix(&bytes[offset * 2..offset * 2 + 8], 16).unwrap()
    }

    fn byte(bytes: &str, offset: usize) -> u8 {
        u8::from_str_radix(&bytes[offset * 2..offset * 2 + 2], 16).unwrap()
    }

    fn inputs(c: &Value) -> MoveInputs {
        let floats = |key: &str, n: usize| -> Vec<f32> {
            (0..n)
                .map(|i| f32::from_bits(c[key][i].as_u64().unwrap() as u32))
                .collect()
        };
        let matrix = |key: &str| -> Matrix {
            floats(key, 16).try_into().unwrap()
        };
        MoveInputs {
            world: matrix("world"),
            inverse: matrix("inverse"),
            anchor: floats("anchor", 3).try_into().unwrap(),
            radius: f32::from_bits(c["radius"].as_u64().unwrap() as u32),
            character: floats("character", 3).try_into().unwrap(),
        }
    }

    #[test]
    fn constructor_think_and_small_methods_match_retail_bytes() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_ai_move_golden.json"
        ))
        .unwrap();
        assert_eq!(corpus["elf_sha256"].as_str(), Some(crate::recovered::ELF_SHA256));

        for c in corpus["constructor"].as_array().unwrap() {
            let mut m = MoveCompulsion::new(0x7170_0000, c["priority"].as_u64().unwrap() as u8);
            let bytes = c["bytes"].as_str().unwrap();
            assert_eq!(m.vtable, 0x804d_cb7c);
            assert_eq!(m.ai_entity_handle_004, word(bytes, 4));
            assert_eq!(m.active_008, byte(bytes, 8) != 0);
            assert_eq!(m.priority_009, byte(bytes, 9));
            assert_eq!(m.timer_010, word(bytes, 0x10) as i32);
            assert_eq!(m.target_angle_030.to_bits(), word(bytes, 0x30));
            assert_eq!(m.target_angle_valid_034, byte(bytes, 0x34) == 1);
            assert_eq!(m.angle_060.to_bits(), word(bytes, 0x60));
            assert_eq!(m.angle_070.to_bits(), word(bytes, 0x70));
            assert_eq!(m.flag_080, byte(bytes, 0x80) != 0);
            assert_eq!(m.ai_entity_handle_084, word(bytes, 0x84));
            assert_eq!(m.think_gate_08c, byte(bytes, 0x8c));

            m.activate();
            assert!(m.active_008);
            m.deactivate();
            assert!(!m.active_008);
            assert!(m.is_interruptible());
            assert_eq!(m.get_name(), "TetherballMoveCompulsion");
        }

        for c in corpus["think"].as_array().unwrap() {
            let mut m = MoveCompulsion::new(0x7170_0000, 40);
            m.bind_move_object(c["binding"].as_u64().unwrap() as u32);
            m.think_gate_08c = c["gate"].as_u64().unwrap() as u8;
            let got = m.think(c["ms"].as_i64().unwrap() as i32, &inputs(c));
            let expected = c["result"].as_bool().unwrap();
            assert_eq!(got, expected, "{c}");

            let bytes = c["bytes"].as_str().unwrap();
            assert_eq!(m.timer_010, word(bytes, 0x10) as i32, "{c}");
            assert_eq!(m.bound_move_object_088, Some(word(bytes, 0x88)), "{c}");
            assert_eq!(m.think_gate_08c, byte(bytes, 0x8c), "{c}");
            if expected {
                assert_eq!(m.target_position_020.unwrap().map(f32::to_bits), std::array::from_fn(|i| word(bytes, 0x20 + i * 4)), "{c}");
                assert_eq!(m.target_angle_030.to_bits(), word(bytes, 0x30), "{c}");
                assert_eq!(m.target_angle_valid_034, byte(bytes, 0x34) == 1, "{c}");
                assert_eq!(m.threshold_040.unwrap().to_bits(), word(bytes, 0x40), "{c}");
                assert_eq!(m.field_044.unwrap() as u32, word(bytes, 0x44), "{c}");
                assert_eq!(m.field_048, Some(byte(bytes, 0x48) == 1), "{c}");
                assert_eq!(m.flag_080, byte(bytes, 0x80) != 0, "{c}");
            } else {
                assert!(m.target_position_020.is_none(), "{c}");
                assert!(m.threshold_040.is_none(), "{c}");
                assert!(m.field_044.is_none(), "{c}");
                assert!(m.field_048.is_none(), "{c}");
            }
        }
    }

    struct ExpiryServices {
        active: bool,
        swinging: bool,
        in_position: bool,
        calls: Vec<&'static str>,
    }

    impl MoveServices for ExpiryServices {
        fn tetherball_minigame_is_active(&mut self) -> bool {
            self.calls.push("minigame");
            self.active
        }
        fn is_swinging(&mut self, _: u32) -> bool {
            self.calls.push("swinging");
            self.swinging
        }
        fn is_in_position(&mut self, _: u32, _: &MoveInputs, tolerance: f32) -> bool {
            assert_eq!(tolerance.to_bits(), POSITION_TOLERANCE.to_bits());
            self.calls.push("position");
            self.in_position
        }
    }

    #[test]
    fn has_expired_short_circuits_like_original() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_ai_move_golden.json"
        ))
        .unwrap();
        let lifecycle_corpus: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_lifecycle_golden.json"
        ))
        .unwrap();
        let mut lifecycle: Lifecycle = serde_json::from_value(
            lifecycle_corpus["cases"][0]["initial"].clone(),
        )
        .unwrap();
        let geometry = inputs(&corpus["think"][0]);

        for c in corpus["has_expired"].as_array().unwrap() {
            lifecycle.match_state.state_code = c["state"].as_u64().unwrap() as u32;
            let mut m = MoveCompulsion::new(0x7170_0000, 40);
            let mut services = ExpiryServices {
                active: c["present"].as_bool().unwrap() && c["mgid"].as_bool().unwrap(),
                swinging: matches!(c["animation"].as_i64().unwrap(), 63 | 69 | 75 | 81 | 91),
                in_position: c["near"].as_bool().unwrap(),
                calls: Vec::new(),
            };
            let got = m.has_expired(&lifecycle, &geometry, &mut services);
            assert_eq!(got, c["result"].as_bool().unwrap(), "{c}");
            let expected_calls: Vec<&str> = if !services.active
                || !crate::tetherball_ai::is_in_gameplay(lifecycle.match_state.state_code)
            {
                vec!["minigame"]
            } else if services.swinging {
                vec!["minigame", "swinging"]
            } else {
                vec!["minigame", "swinging", "position"]
            };
            assert_eq!(services.calls, expected_calls, "{c}");
        }
    }
}
