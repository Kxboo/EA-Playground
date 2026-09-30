//! Original Tetherball serve and motion arithmetic; independent of rendering and input.
//! Tunable fields are supplied by the caller, not invented balance values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction { Zero, One }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone { Zero, One }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallMotion {
    pub angle: f32,
    pub hit_angle: f32,
    pub secondary_angle: f32,
    pub target_velocity: f32,
    pub secondary_target_velocity: f32,
    pub spin_acceleration: f32,
    pub hit_direction: Direction,
    pub grabbed: bool,
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
    pub base_hit_speed: f32,
    pub power_modifier: f32,
    pub mega_modifier: f32,
    pub pole_height: f32,
    pub tossed: bool,
    pub spinning_up: bool,
    pub spinning_down: bool,
}
/// Original rmAngle::Wrap uses repeated rounded additions/subtractions, not remainder.
/// Finite bounded angles are required; original also fails to terminate on infinity.
pub fn wrap_angle(mut angle: f32) -> f32 {
    let tau = f32::from_bits(0x40c90fdb);
    while angle >= tau { angle -= tau; }
    while angle < 0.0 { angle += tau; }
    angle
}
impl BallMotion {
    pub fn hit(&mut self, direction: Direction, speed: f32, angle: f32, hit_type: i32) {
        self.hit_angle = wrap_angle(angle); self.hit_direction = direction;
        let requested = speed / self.radius;
        let velocity = if self.direction != direction || requested.abs() > self.angular_velocity.abs() {
            requested
        } else { self.angular_velocity.abs() };
        self.target_velocity = velocity; self.secondary_target_velocity = 1.5 * velocity;
        self.acceleration = (self.base_hit_speed * (self.power_modifier - 0.1)) / self.radius;
        self.secondary_acceleration = (self.base_hit_speed * (self.mega_modifier - 0.1)) / self.radius;
        self.hit_type = hit_type;
        if direction == Direction::Zero {
            self.target_velocity = -self.target_velocity;
            self.secondary_target_velocity = -self.secondary_target_velocity;
        }
        self.angular_velocity = self.target_velocity;
        self.secondary_velocity = self.secondary_target_velocity;
        self.direction = self.hit_direction;
    }
    /// Exact numerical prefix of Update, ending before model/rope transforms.
    /// The full original later derives radius/height from those transforms.
    pub fn update_motion(&mut self, milliseconds: i32) {
        let dt = milliseconds as f32 / 1000.0;
        if self.tossed {
            self.toss_time = self.toss_time.wrapping_sub(milliseconds);
            if self.toss_time <= 0 {
                let baseline = self.pole_height + 0.5;
                self.height = self.vertical_velocity.mul_add(dt, self.height);
                self.vertical_velocity = (-4.807_f32).mul_add(dt, self.vertical_velocity);
                self.grabbed = false;
                if self.height < baseline - 0.03 && self.vertical_velocity < 0.0 {
                    self.height = baseline; self.tossed = false; self.grabbed = true;
                }
            }
        }
        if self.desired_radius < self.radius && self.angular_velocity.abs() > 0.0 {
            let speed = if self.spinning_up {0.35_f32} else {0.6_f32};
            self.radius = -(speed.mul_add(dt, -self.radius));
            if self.radius < self.desired_radius {
                self.radius = self.desired_radius;
                if self.spinning_up && self.height == self.target_height { self.begin_spin_return(); }
            }
        } else if self.desired_radius > self.radius && self.angular_velocity.abs() > 0.0 {
            let speed = if self.spinning_up {0.15_f32} else {0.6_f32};
            self.radius = speed.mul_add(dt, self.radius);
            if self.radius > self.desired_radius { self.radius = self.desired_radius; }
        }
        self.angle = wrap_angle(self.angular_velocity * dt + self.angle);
        self.secondary_angle = wrap_angle(self.secondary_velocity * dt + self.secondary_angle);
        if self.height > self.target_height {
            let speed = if self.spinning_up {0.3_f32} else {0.9_f32};
            self.height = -(speed.mul_add(dt, -self.height));
            if self.height < self.target_height { self.height = self.target_height; }
        } else if self.height < self.target_height {
            self.height = 0.9_f32.mul_add(dt, self.height);
            if self.height > self.target_height {
                self.height = self.target_height;
                if self.spinning_up && self.radius == self.desired_radius { self.begin_spin_return(); }
            }
        }
        if self.spinning_down {
            let velocity = self.spin_acceleration.mul_add(dt, self.angular_velocity);
            self.set_angular_velocity(velocity);
            if (self.spin_acceleration < 0.0 && velocity < -6.0) || (self.spin_acceleration > 0.0 && velocity > 6.0) {
                self.spinning_down = false; self.spin_down_pole();
            }
        }
    }
    fn begin_spin_return(&mut self) {
        self.spinning_down = true; self.spinning_up = false;
        self.spin_acceleration = if self.angular_velocity > 0.0 {-8.0} else {8.0};
    }
    pub fn spin_down_pole(&mut self) {
        self.spinning_up = true; self.set_desired_radius(0.7);
        self.target_height = self.pole_height + 0.5;
        let velocity = if self.angular_velocity > 0.0 {6.0} else {-6.0};
        self.set_angular_velocity(velocity);
    }
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
        self.acceleration = (self.base_hit_speed * (self.power_modifier - 0.1)) / self.desired_radius;
        self.secondary_acceleration = (self.base_hit_speed * (self.mega_modifier - 0.1)) / self.radius;
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
pub(crate) mod tests {
    use super::*;
    use serde_json::Value;
    fn number(v: &Value, key: &str) -> i32 { v[key].as_i64().unwrap() as i32 }
    fn float(v: &Value, key: &str) -> f32 { f32::from_bits(v[key].as_u64().unwrap() as u32) }
    pub(crate) fn ball(v: &Value) -> BallMotion {
        BallMotion {
            angle:float(v,"angle"),hit_angle:float(v,"hit_angle"),secondary_angle:float(v,"secondary_angle"),
            target_velocity:float(v,"target_velocity"),secondary_target_velocity:float(v,"secondary_target_velocity"),
            spin_acceleration:float(v,"spin_acceleration"),grabbed:v["grabbed"].as_bool().unwrap(),
            hit_direction:if number(v,"hit_direction")==0 {Direction::Zero} else {Direction::One},
            vertical_velocity:float(v,"vertical_velocity"), toss_time:number(v,"toss_time"),
            angular_velocity:float(v,"angular_velocity"), acceleration:float(v,"acceleration"),
            secondary_acceleration:float(v,"secondary_acceleration"), secondary_velocity:float(v,"secondary_velocity"),
            hit_type:number(v,"hit_type"), direction:if number(v,"direction")==0 {Direction::Zero} else {Direction::One},
            zone:if number(v,"zone")==0 {Zone::Zero} else {Zone::One}, radius:float(v,"radius"),
            desired_radius:float(v,"desired_radius"), height:float(v,"height"), target_height:float(v,"target_height"),
            base_hit_speed:float(v,"base_hit_speed"), power_modifier:float(v,"power_modifier"), mega_modifier:float(v,"mega_modifier"),
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
                    "hit"=>state.hit(if arg==0 {Direction::Zero} else {Direction::One},farg,f32::from_bits(step["fargs"][1].as_u64().unwrap() as u32),step["args"][1].as_i64().unwrap() as i32),
                    "update"=>state.update_motion(arg as i32), "spin_down"=>state.spin_down_pole(),
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
        for angle in data["angles"].as_array().unwrap() {
            assert_eq!(wrap_angle(f32::from_bits(angle["input"].as_u64().unwrap() as u32)).to_bits(),angle["result"].as_u64().unwrap() as u32);
        }
        for row in data["scores"].as_array().unwrap() {
            let stat:Vec<i32>=row["stats"].as_array().unwrap().iter().map(|x|x.as_i64().unwrap() as i32).collect();
            let weights:Vec<i32>=row["coefficients"].as_array().unwrap().iter().map(|x|x.as_i64().unwrap() as i32).collect();
            assert_eq!(calc_score(ScoreStats{power_hits:weights[0],mega_hits:weights[1],accuracy_percent:weights[2]},ScoreWeights{accuracy_points:stat[0],power_hit_points:stat[1],mega_hit_points:stat[2]}),row["result"].as_i64().unwrap() as i32);
        }
    }
    pub(crate) fn assert_bits(a:&BallMotion,b:&BallMotion,case:usize,step:usize) {
        macro_rules! check { ($($field:ident),*)=>{$(assert_eq!(a.$field.to_bits(),b.$field.to_bits(),"case={case} step={step} field={}",stringify!($field));)*}; }
        check!(angle,hit_angle,secondary_angle,target_velocity,secondary_target_velocity,spin_acceleration,vertical_velocity,angular_velocity,acceleration,secondary_acceleration,secondary_velocity,radius,desired_radius,height,target_height,base_hit_speed,power_modifier,mega_modifier,pole_height);
        assert_eq!((a.hit_direction,a.grabbed),(b.hit_direction,b.grabbed));
        assert_eq!((a.toss_time,a.hit_type,a.direction,a.zone,a.tossed,a.spinning_up,a.spinning_down),(b.toss_time,b.hit_type,b.direction,b.zone,b.tossed,b.spinning_up,b.spinning_down));
    }
}
