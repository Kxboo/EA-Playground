//! Complete original InitializePlayerAnimations (0x8039be84).
use crate::tetherball_hit_animation::HitAnimations;
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_reset::ResetState;
use crate::tetherball_serve::ServeState;

/// Initializes the live prefix of thirteen per-player animation tables.
/// Player count 0 preserves all tables; counts >2 exceed this safe projection.
/// Win-animation choices (+1f8) are not written by this routine.
pub fn initialize_player_animations(
    lifecycle: &mut Lifecycle,
    reset: &ResetState,
    serve: &mut ServeState,
    hit: &mut HitAnimations,
) {
    assert!(
        lifecycle.player_count <= 2,
        "native two-player animation table domain"
    );
    for p in 0..lifecycle.player_count {
        let special = lifecycle.players[p].player_flag;
        let side = reset.server_side_flags[2 + p];
        let (idle, ready, suppress, strike) = match (special, side) {
            (true, true) => (88, 92, 93, 94),
            (true, false) => (87, 89, 90, 91),
            (false, true) => (57, 70, 71, 72),
            (false, false) => (56, 67, 68, 69),
        };
        lifecycle.lose_animations[p] = idle;
        hit.ready_power.state[p] = ready;
        hit.ready_power.suppress_if_current[p] = suppress;
        hit.hit_power[p] = strike;
        let shift = if side { 3 } else { 0 };
        hit.ready_zone_zero.state[p] = 61 + shift;
        hit.ready_zone_zero.suppress_if_current[p] = 62 + shift;
        hit.hit_zone_zero[p] = 63 + shift;
        hit.ready_reverse.state[p] = 79 + shift;
        hit.ready_reverse.suppress_if_current[p] = 80 + shift;
        serve.power_animations[p] = 81 + shift;
        hit.ready_zone_one.state[p] = 73 + shift;
        hit.ready_zone_one.suppress_if_current[p] = 74 + shift;
        serve.high_animations[p] = 75 + shift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tetherball_hit_animation::ReadyAnimation;
    use serde_json::{Value, json};
    fn pair(v: &Value, key: &str) -> [i32; 2] {
        std::array::from_fn(|i| v[key][i].as_i64().unwrap() as i32)
    }
    fn flags(v: &Value, key: &str) -> [bool; 2] {
        std::array::from_fn(|i| v[key][i].as_bool().unwrap())
    }
    fn ready(w: &Value, a: &str, b: &str) -> ReadyAnimation {
        ReadyAnimation {
            state: pair(w, a),
            suppress_if_current: pair(w, b),
        }
    }
    #[test]
    fn original_animation_initialization_full_snapshots() {
        let d: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_animation_init_golden.json"
        ))
        .unwrap();
        assert_eq!(d["elf_sha256"], crate::recovered::ELF_SHA256);
        assert_eq!(d["cases"].as_array().unwrap().len(), 48);
        for c in d["cases"].as_array().unwrap() {
            let mut l: Lifecycle = serde_json::from_value(c["initial"].clone()).unwrap();
            let mut r = ResetState::blank();
            r.server_side_flags = std::array::from_fn(|i| c["side_flags"][i].as_bool().unwrap());
            let original_reset = r.clone();
            let a = &c["aux"];
            let mut s = ServeState {
                pause_block_count_0fc: a["pause_block_count_0fc"].as_i64().unwrap() as i32,
                pause_menu_open: a["pause_menu_open"].as_bool().unwrap(),
                power_serve_enabled: a["power_serve_enabled"].as_bool().unwrap(),
                return_angles: std::array::from_fn(|i| {
                    f32::from_bits(a["return_angles"][i].as_u64().unwrap() as u32)
                }),
                power_animations: pair(a, "power_animations"),
                high_animations: pair(a, "high_animations"),
                voice_types: pair(a, "voice_types"),
                ai_waiting: flags(a, "ai_waiting"),
                forced_ai: flags(a, "forced_ai"),
                frontend_flags: std::array::from_fn(|i| {
                    a["frontend_flags"][i].as_u64().unwrap() != 0
                }),
            };
            let w = &c["words"];
            let mut h = HitAnimations {
                ready_power: ready(w, "0x1a0", "0x1a8"),
                ready_reverse: ready(w, "0x1d0", "0x1d8"),
                ready_zone_zero: ready(w, "0x1b8", "0x1c0"),
                ready_zone_one: ready(w, "0x1e8", "0x1f0"),
                hit_power: pair(w, "0x198"),
                hit_zone_zero: pair(w, "0x1b0"),
            };
            initialize_player_animations(&mut l, &r, &mut s, &mut h);
            let label = c["label"].as_str().unwrap();
            assert_eq!(
                serde_json::to_value(&l).unwrap(),
                c["expected"],
                "{label}: lifecycle"
            );
            assert_eq!(r, original_reset, "{label}: reset must be preserved");
            let mut actual_aux = a.clone();
            actual_aux["power_animations"] = json!(s.power_animations);
            actual_aux["high_animations"] = json!(s.high_animations);
            assert_eq!(actual_aux, c["expected_aux"], "{label}: serve tables");
            // Every field of the supplied ServeState, not only the changed tables.
            assert_eq!(
                s.pause_block_count_0fc,
                a["pause_block_count_0fc"].as_i64().unwrap() as i32
            );
            assert_eq!(s.pause_menu_open, a["pause_menu_open"].as_bool().unwrap());
            assert_eq!(
                s.power_serve_enabled,
                a["power_serve_enabled"].as_bool().unwrap()
            );
            assert_eq!(
                s.return_angles.map(f32::to_bits),
                std::array::from_fn(|i| a["return_angles"][i].as_u64().unwrap() as u32)
            );
            assert_eq!(s.voice_types, pair(a, "voice_types"));
            assert_eq!(s.ai_waiting, flags(a, "ai_waiting"));
            assert_eq!(s.forced_ai, flags(a, "forced_ai"));
            assert_eq!(
                s.frontend_flags,
                std::array::from_fn(|i| a["frontend_flags"][i].as_u64().unwrap() != 0)
            );
            let actual = json!({"0x198":h.hit_power,"0x1a0":h.ready_power.state,"0x1a8":h.ready_power.suppress_if_current,
                "0x1b0":h.hit_zone_zero,"0x1b8":h.ready_zone_zero.state,"0x1c0":h.ready_zone_zero.suppress_if_current,
                "0x1d0":h.ready_reverse.state,"0x1d8":h.ready_reverse.suppress_if_current,
                "0x1e8":h.ready_zone_one.state,"0x1f0":h.ready_zone_one.suppress_if_current});
            assert_eq!(actual, c["expected_words"], "{label}: hit tables");
            assert_eq!(
                c["ball"], c["expected_ball"],
                "{label}: native ball unchanged"
            );
            assert_eq!(c["effects"], json!([]), "{label}: no engine effects");
        }
    }
}
