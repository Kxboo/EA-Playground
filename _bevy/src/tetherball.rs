//! Original Tetherball serve and motion arithmetic; independent of rendering and input.
//! Tunable fields are supplied by the caller, not invented balance values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction { Zero, One }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone { Zero, One }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallMotion {
    pub vertical_velocity: f32,
    pub toss_time: i32,
    pub angular_velocity: f32,
    pub acceleration: f32,
    pub secondary_acceleration: f32,
    pub secondary_velocity: f32,
    pub hit_type: i32,
    pub direction: Direction,
    pub zone: Zone,
    pub radius: f32,
    pub desired_radius: f32,
    pub height: f32,
    pub target_height: f32,
    pub scale: f32,
    pub drag: f32,
    pub secondary_drag: f32,
    pub pole_height: f32,
    pub tossed: bool,
    pub spinning_up: bool,
    pub spinning_down: bool,
}
impl BallMotion {
    pub fn set_angular_velocity(&mut self, velocity: f32) {
        self.angular_velocity = velocity; self.secondary_velocity = velocity;
    }
    pub fn set_radius(&mut self, radius: f32) {
        self.radius = radius; self.desired_radius = radius;
    }
    /// Original rescales current velocity/acceleration by current/desired radius,
    /// retaining the current radius until its separate update loop interpolates it.
    pub fn set_desired_radius(&mut self, radius: f32) {
        let factor = self.radius / radius;
        self.desired_radius = radius;
        self.angular_velocity *= factor;
        self.acceleration *= factor;
        self.secondary_acceleration *= factor;
    }
    pub fn toss(&mut self) {
        self.tossed = true; self.vertical_velocity = 3.6; self.toss_time = 400;
    }
    pub fn serve(&mut self, direction: Direction, speed: f32) {
        self.spinning_down = false; self.spinning_up = false; self.direction = direction;
        let velocity = speed / self.desired_radius;
        self.angular_velocity = velocity;
        self.secondary_velocity = 1.5 * velocity;
        self.acceleration = (self.scale * (self.drag - 0.1)) / self.desired_radius;
        self.secondary_acceleration = (self.scale * (self.secondary_drag - 0.1)) / self.radius;
        if self.vertical_velocity.abs() < 0.5 { self.angular_velocity *= 1.5; }
        if direction == Direction::Zero {
            self.angular_velocity = -self.angular_velocity;
            self.secondary_velocity = -self.secondary_velocity;
        }
        // Serve sets the zone field directly; it does not update target_height.
        self.zone = Zone::Zero; self.tossed = false;
    }
    /// Original unsigned milliseconds conversion and single-precision multiply-add.
    pub fn can_power_serve(&self, milliseconds: u32) -> bool {
        let seconds = milliseconds as f32 / 1000.0;
        (-4.807_f32).mul_add(seconds, self.vertical_velocity).abs() < 0.5
    }
    pub fn can_high_serve(&self, milliseconds: u32) -> bool {
        let seconds = milliseconds as f32 / 1000.0;
        let quadratic = -2.4035_f32 * seconds;
        let linear = self.vertical_velocity.mul_add(seconds, self.height);
        quadratic.mul_add(seconds, linear) > 0.7
    }
    pub fn miss(&mut self) {
        self.hit_type = 4; self.damp_velocity();
    }
    fn damp_velocity(&mut self) {
        self.angular_velocity = -(0.05_f32.mul_add(self.angular_velocity, -self.angular_velocity));
        self.secondary_velocity = -(0.05_f32.mul_add(self.secondary_velocity, -self.secondary_velocity));
    }
    pub fn set_zone(&mut self, zone: Zone) {
        self.zone = zone; self.update_target_height();
    }
    fn update_target_height(&mut self) {
        self.target_height = self.pole_height + match self.zone { Zone::Zero => 0.5, Zone::One => 0.9 };
    }
    /// Only zone 1 changes to 0; every call still damps velocity by five percent.
    pub fn drop_one_zone(&mut self) {
        if self.zone == Zone::One { self.zone = Zone::Zero; }
        self.damp_velocity(); self.update_target_height();
    }
}

/// Per-player counters at MGTetherball +0x14c/+0x150/+0x154 (stride 0x14).
/// UpdateRoundEnd computes accuracy_percent = successful_hits * 100 / attempts.
#[derive(Debug, Clone, Copy)]
pub struct ScoreStats { pub power_hits: i32, pub mega_hits: i32, pub accuracy_percent: i32 }
/// Difficulty-selected mg_tetherball/scoring fields at +0x138/+0x13c/+0x140.
#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights { pub accuracy_points: i32, pub power_hit_points: i32, pub mega_hit_points: i32 }
/// CalcScore ignores its difficulty argument; ResetStats already selected weights.
pub fn calc_score(stats: ScoreStats, weights: ScoreWeights) -> i32 {
    stats.power_hits.wrapping_mul(weights.power_hit_points)
        .wrapping_add(stats.mega_hits.wrapping_mul(weights.mega_hit_points)
        .wrapping_add(stats.accuracy_percent.wrapping_mul(weights.accuracy_points)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn number(v: &Value, key: &str) -> i32 { v[key].as_i64().unwrap() as i32 }
    fn float(v: &Value, key: &str) -> f32 { f32::from_bits(v[key].as_u64().unwrap() as u32) }
    fn ball(v: &Value) -> BallMotion {
        BallMotion {
            vertical_velocity:float(v,"vertical_velocity"), toss_time:number(v,"toss_time"),
            angular_velocity:float(v,"angular_velocity"), acceleration:float(v,"acceleration"),
            secondary_acceleration:float(v,"secondary_acceleration"), secondary_velocity:float(v,"secondary_velocity"),
            hit_type:number(v,"hit_type"), direction:if number(v,"direction")==0 {Direction::Zero} else {Direction::One},
            zone:if number(v,"zone")==0 {Zone::Zero} else {Zone::One}, radius:float(v,"radius"),
            desired_radius:float(v,"desired_radius"), height:float(v,"height"), target_height:float(v,"target_height"),
            scale:float(v,"scale"), drag:float(v,"drag"), secondary_drag:float(v,"secondary_drag"),
            pole_height:float(v,"pole_height"), tossed:v["tossed"].as_bool().unwrap(),
            spinning_up:v["spinning_up"].as_bool().unwrap(), spinning_down:v["spinning_down"].as_bool().unwrap(),
        }
    }
    #[test]
    fn original_powerpc_motion_and_scoring() {
        let data:Value=serde_json::from_str(include_str!("../tests/data/tetherball_golden.json")).unwrap();
        assert_eq!(data["elf_sha256"].as_str().unwrap(), crate::recovered::ELF_SHA256);
        for (case_index,case) in data["cases"].as_array().unwrap().iter().enumerate() {
            let mut state=ball(&case["initial"]);
            for (step_index,step) in case["steps"].as_array().unwrap().iter().enumerate() {
                let arg=step["args"][0].as_u64().unwrap_or(0) as u32;
                let farg=f32::from_bits(step["fargs"][0].as_u64().unwrap_or(0) as u32);
                match step["kind"].as_str().unwrap() {
                    "serve"=>state.serve(if arg==0 {Direction::Zero} else {Direction::One},farg),
                    "toss"=>state.toss(), "miss"=>state.miss(), "set_radius"=>state.set_radius(farg),
                    "desired_radius"=>state.set_desired_radius(farg), "angular"=>state.set_angular_velocity(farg),
                    "zone"=>state.set_zone(if arg==0 {Zone::Zero} else {Zone::One}), "drop"=>state.drop_one_zone(),
                    "power"=>assert_eq!(state.can_power_serve(arg),step["result"].as_bool().unwrap()),
                    "high"=>assert_eq!(state.can_high_serve(arg),step["result"].as_bool().unwrap()),
                    _=>unreachable!(),
                }
                let expected=ball(&step["expected"]);
                // Bit comparison also catches signed-zero and rounding changes.
                assert_bits(&state,&expected,case_index,step_index);
            }
        }
        for row in data["scores"].as_array().unwrap() {
            let stat:Vec<i32>=row["stats"].as_array().unwrap().iter().map(|x|x.as_i64().unwrap() as i32).collect();
            let weights:Vec<i32>=row["coefficients"].as_array().unwrap().iter().map(|x|x.as_i64().unwrap() as i32).collect();
            assert_eq!(calc_score(ScoreStats{power_hits:weights[0],mega_hits:weights[1],accuracy_percent:weights[2]},ScoreWeights{accuracy_points:stat[0],power_hit_points:stat[1],mega_hit_points:stat[2]}),row["result"].as_i64().unwrap() as i32);
        }
    }
    fn assert_bits(a:&BallMotion,b:&BallMotion,case:usize,step:usize) {
        macro_rules! check { ($($field:ident),*)=>{$(assert_eq!(a.$field.to_bits(),b.$field.to_bits(),"case={case} step={step} field={}",stringify!($field));)*}; }
        check!(vertical_velocity,angular_velocity,acceleration,secondary_acceleration,secondary_velocity,radius,desired_radius,height,target_height,scale,drag,secondary_drag,pole_height);
        assert_eq!((a.toss_time,a.hit_type,a.direction,a.zone,a.tossed,a.spinning_up,a.spinning_down),(b.toss_time,b.hit_type,b.direction,b.zone,b.tossed,b.spinning_up,b.spinning_down));
    }
}
