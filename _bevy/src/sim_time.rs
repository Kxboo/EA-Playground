//! GameState::Update (0x803acdc4) and PhysicsManager::Update (0x803b6f84).
//! Pure timing policy, separate from the host clock and the provisional character solver.
//! See tools/timing_oracle.py: expected values come from executing the PowerPC image.
use crate::recovered as k;

#[derive(Debug, Clone, Copy)]
pub struct FramePolicy {
    pub cap_ms: Option<u32>,
    pub fixed_ms: Option<u32>,
    pub paused: bool,
}

impl Default for FramePolicy {
    fn default() -> Self {
        Self {
            cap_ms: (k::FRAME_CAP_ENABLED != 0).then_some(k::FRAME_CAP_MS as u32),
            fixed_ms: None,
            paused: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStep {
    pub simulation_ms: u32,
    /// Controller::Update receives uncapped, truncated wall-clock milliseconds.
    pub input_ms: i32,
}

impl FrameStep {
    pub fn seconds(self) -> f32 { self.simulation_ms as f32 / 1000.0 }
}

impl FramePolicy {
    /// Truncate each frame independently, then cap, override, and pause, in that order.
    /// No fractional remainder or fixed-rate catch-up accumulator exists in the original.
    /// Host durations must be finite and nonnegative; invalid inputs yield a zero frame.
    pub fn step(self, frame_ms: f32) -> FrameStep {
        let frame_ms = if frame_ms.is_finite() && frame_ms >= 0.0 { frame_ms } else { 0.0 };
        let mut simulation_ms = frame_ms as u32;
        if let Some(cap) = self.cap_ms { simulation_ms = simulation_ms.min(cap); }
        if let Some(fixed) = self.fixed_ms { simulation_ms = fixed; }
        if self.paused { simulation_ms = 0; }
        FrameStep { simulation_ms, input_ms: frame_ms as i32 }
    }
}

/// The executable reads the CPU clock word at absolute address 0x800000fc,
/// NOT gpTrcCorehandlers+0xfc. The clock word's runtime value is not in the ELF.
/// This preserves the original 32-bit cycle wrap and single-precision operations.
pub fn milliseconds_from_ticks(tick: u32, previous_cycles: u32, clock_hz: u32) -> f32 {
    let cycles = tick.wrapping_mul(12).wrapping_sub(previous_cycles);
    (cycles as f32 / clock_hz as f32) * 1000.0
}

/// Havok step arguments, including the 200 ms hard cap, integer remainder
/// distribution, and the world's pause/missing-world gates. Does not simulate Havok.
pub fn physics_steps(dt_ms: i32, has_world: bool, paused: bool) -> Vec<f32> {
    if dt_ms <= 0 || !has_world || paused { return Vec::new(); }
    let dt = dt_ms.min(200);
    let n = (dt + k::PHYSICS_SLICE_CAP_MS - 1) / k::PHYSICS_SLICE_CAP_MS;
    (0..n).map(|i| {
        let slice = dt * (i + 1) / n - dt * i / n;
        (slice as f32 * k::PHYSICS_TIME_MULTIPLIER) / 1000.0
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_cap_override_pause_and_uncapped_input() {
        assert_eq!(k::FRAME_CAP_MS, 60);
        assert_eq!(k::FIXED_FRAME_MS, 16);
        let mut p = FramePolicy::default();
        assert_eq!(p.step(16.999), FrameStep { simulation_ms: 16, input_ms: 16 });
        assert_eq!(p.step(250.75), FrameStep { simulation_ms: 60, input_ms: 250 });
        p.fixed_ms = Some(80); // override happens after cap
        assert_eq!(p.step(5.0).simulation_ms, 80);
        p.paused = true;
        assert_eq!(p.step(250.75), FrameStep { simulation_ms: 0, input_ms: 250 });
    }

    #[test]
    fn frame_fraction_is_discarded_without_catch_up() {
        let p = FramePolicy::default();
        let total: u32 = (0..60).map(|_| p.step(1000.0 / 60.0).simulation_ms).sum();
        assert_eq!(total, 960);
        assert_eq!(p.step(1000.0).simulation_ms, 60);
        assert_eq!(p.step(0.5).simulation_ms, 0);
        assert_eq!(p.step(f32::NAN), FrameStep::default());
    }

    #[test]
    fn physics_substeps_distribute_remainder_and_obey_gates() {
        assert_eq!(physics_steps(61, true, false), vec![0.030, 0.031]);
        assert_eq!(physics_steps(121, true, false), vec![0.040, 0.040, 0.041]);
        assert_eq!(physics_steps(999, true, false), vec![0.050; 4]);
        assert!(physics_steps(-1, true, false).is_empty());
        assert!(physics_steps(60, false, false).is_empty());
        assert!(physics_steps(60, true, true).is_empty());
    }

    #[test]
    fn matches_original_powerpc_timing_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("../tests/data/timing_golden.json")).unwrap();
        assert_eq!(vectors["elf_sha256"].as_str(), Some(k::ELF_SHA256));
        for v in vectors["frames"].as_array().unwrap() {
            let u = |name: &str| v[name].as_u64().unwrap() as u32;
            let p = FramePolicy {
                cap_ms: v["cap_enabled"].as_bool().unwrap().then(|| u("cap_ms")),
                fixed_ms: v["fixed_enabled"].as_bool().unwrap().then(|| u("fixed_ms")),
                paused: v["paused"].as_bool().unwrap(),
            };
            let ms = milliseconds_from_ticks(u("tick"), u("previous_cycles"), u("clock_hz"));
            assert_eq!(ms.to_bits(), u("frame_ms_bits"), "clock conversion {v}");
            assert_eq!(p.step(ms), FrameStep { simulation_ms: u("simulation_ms"), input_ms: u("input_ms") as i32 }, "{v}");
        }
        for v in vectors["physics"].as_array().unwrap() {
            let got = physics_steps(v["dt_ms"].as_i64().unwrap() as i32, v["has_world"].as_bool().unwrap(), v["paused"].as_bool().unwrap());
            let expected: Vec<u32> = v["step_bits"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
            assert_eq!(got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), expected, "{v}");
        }
    }
}
