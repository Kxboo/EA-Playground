//! FnCompoundChannel timing and AttributeBlock byte lookup from the pinned ELF.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompoundFunction {
    pub samples: u16,
    pub use_fps: bool,
    pub fps: u8,
}
/// FindAttribute (0x803fc478) / GetAttribute RUc (0x803fce60).
/// Entries contain an ID and the first byte of the inline four-byte payload.
pub fn attribute_byte(entries: &[(u16, u8)], id: u16) -> Option<u8> {
    let mut low = 0i32;
    let mut high = entries.len() as i32 - 1;
    while low <= high {
        let mid = (low + high) >> 1;
        let entry = entries[mid as usize];
        if id > entry.0 {
            low = mid + 1;
        } else if id < entry.0 {
            high = mid - 1;
        } else {
            return Some(entry.1);
        }
    }
    None
}
impl CompoundFunction {
    /// UseFPS (0x803fecac): enable flag always stores; only a zero FPS cache
    /// on enable queries attribute 1. Missing attributes leave the cache intact.
    pub fn set_use_fps(&mut self, enabled: bool, attributes: &[(u16, u8)]) {
        self.use_fps = enabled;
        if enabled && self.fps == 0 {
            if let Some(fps) = attribute_byte(attributes, 1) {
                self.fps = fps;
            }
        }
    }
    /// GetLength (0x803ffaf8). Zero FPS retains native floating-point division.
    pub fn length(&self) -> f32 {
        let count = self.samples as f32;
        if self.use_fps {
            count / self.fps as f32
        } else {
            count
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_bank_timing_reaches_bevy_curves() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/animation_function_golden.json"))
                .unwrap();
        let source = crate::bridge::data_root().join("files/data/characters/player_anims.viv");
        let (raw, _) =
            crate::archive::read_virtual(&format!("{}::player_anims.anm", source.display()))
                .unwrap();
        assert_eq!(crate::sha256::hex(&raw), fixture["bank_sha256"]);
        let bank = crate::anim::Bank::parse(raw).unwrap();
        let skel = crate::skeleton::Skeleton::parse(
            &crate::archive::read_virtual(&format!("{}::player_skel.ske", source.display()))
                .unwrap()
                .0,
        )
        .unwrap();
        let mut count = 0;
        let mut unusual = vec![];
        for case in fixture["cases"].as_array().unwrap() {
            let Some(index) = case["index"].as_u64() else {
                continue;
            };
            let function = bank.compound_function(index as usize).unwrap();
            assert_eq!(bank.names[index as usize], case["name"]);
            assert_eq!(function.samples as u64, case["initial"]["samples"]);
            assert_eq!(function.fps as u64, case["steps"][1]["fps"]);
            assert_eq!(
                function.length().to_bits() as u64,
                case["steps"][1]["length_bits"]
            );
            let clip = bank.decode(index as usize, &skel).unwrap();
            assert_eq!(clip.sample_rate(), function.fps as f64);
            let bevy = crate::character::animation_clip(&clip).unwrap();
            let expected = ((function.samples - 1) as f64 / function.fps as f64) as f32;
            assert_eq!(
                bevy.duration().to_bits(),
                expected.to_bits(),
                "{} curve time",
                clip.name
            );
            if function.fps != 30 {
                unusual.push((clip.name.clone(), function.fps));
            }
            count += 1;
        }
        assert_eq!(count, 253);
        assert_eq!(
            unusual,
            vec![
                ("S_DB_Catch_Slide".into(), 24),
                ("S_SC_BumpIdle".into(), 37),
                ("S_SC_New_KickStart_Mirrored".into(), 24)
            ]
        );
    }
    #[test]
    fn original_compound_timing_calls() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/animation_function_golden.json"))
                .unwrap();
        assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut function: CompoundFunction =
                serde_json::from_value(case["initial"].clone()).unwrap();
            let attributes: Vec<(u16, u8)> =
                serde_json::from_value(case["attributes"].clone()).unwrap();
            for step in case["steps"].as_array().unwrap() {
                function.set_use_fps(step["enabled"].as_bool().unwrap(), &attributes);
                assert_eq!(
                    function.fps as u64,
                    step["fps"].as_u64().unwrap(),
                    "case {index}"
                );
                assert_eq!(function.use_fps, step["enabled"].as_bool().unwrap());
                if let Some(bits) = step["length_bits"].as_u64() {
                    assert_eq!(function.length().to_bits() as u64, bits, "case {index}");
                }
            }
        }
    }
}
