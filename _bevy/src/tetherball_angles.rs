//! Original rmAngle wrapping and directed interval tests.

const TAU: f32 = f32::from_bits(0x40c90fdb);

/// Match `rmAngle::Wrap` (`0x802cd4e8`) with the original rounded, repeated
/// single-precision additions and subtractions. As in the executable, NaN is
/// left unchanged. Infinity and sufficiently large finite magnitudes can loop
/// forever when subtracting/adding one turn no longer changes the f32 value.
pub fn wrap_angle(mut angle: f32) -> f32 {
    while angle >= TAU {
        angle -= TAU;
    }
    while angle < 0.0 {
        angle += TAU;
    }
    angle
}

/// Match `rmAngle::IsBetween` (`0x802ecc6c`). `first` and `second` are the
/// stored endpoints; `reverse` selects the opposite directed interval used by
/// UpdateServe when player role +0x21c is zero. Endpoints are half-open for
/// unequal values. Equal endpoints match every ordered angle, as the PPC
/// branch sequence dictates. NaN comparisons follow unordered PPC compares.
pub fn is_between(angle: f32, first: f32, second: f32, reverse: bool) -> bool {
    if reverse {
        if !(first > second) {
            angle < first || angle >= second
        } else {
            angle < first && angle >= second
        }
    } else if !(second > first) {
        angle < second || angle >= first
    } else {
        angle < second && angle >= first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn bits(value: &Value) -> f32 {
        f32::from_bits(value.as_u64().unwrap() as u32)
    }

    #[test]
    fn original_powerpc_angle_vectors() {
        let root: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_angles_golden.json"))
                .unwrap();
        assert_eq!(
            root["elf_sha256"].as_str(),
            Some(crate::recovered::ELF_SHA256)
        );
        for case in root["wrap"].as_array().unwrap() {
            assert_eq!(
                wrap_angle(bits(&case["input_bits"])).to_bits(),
                case["result_bits"].as_u64().unwrap() as u32,
                "{case}"
            );
        }
        for case in root["between"].as_array().unwrap() {
            assert_eq!(
                is_between(
                    bits(&case["angle_bits"]),
                    bits(&case["first_bits"]),
                    bits(&case["second_bits"]),
                    case["reverse"].as_bool().unwrap()
                ),
                case["result"].as_bool().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn directed_boundaries_and_nan_follow_retail_branches() {
        let tau = TAU;
        assert_eq!(wrap_angle(tau), 0.0);
        assert_eq!(wrap_angle(f32::from_bits(0x7fc12345)).to_bits(), 0x7fc12345);
        assert!(is_between(1.0, 1.0, 2.0, false));
        assert!(!is_between(2.0, 1.0, 2.0, false));
        assert!(is_between(2.0, 1.0, 2.0, true));
        assert!(is_between(0.0, 1.0, 1.0, false));
        assert!(is_between(0.0, 1.0, 1.0, true));
        assert!(!is_between(f32::NAN, 1.0, 2.0, false));
    }
}
