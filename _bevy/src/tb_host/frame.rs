//! Per-frame driving of the recovered runtime: controllers, attachment matrix, AI compulsion scheduling (provisional
//! loop around the recovered evaluate / think / has_expired), character locomotion and animation.
use super::*;
use crate::tetherball_gestures::GestureCallback;
use bevy::math::{Mat4, Quat, Vec3};

fn to_mat(m: &Matrix) -> Mat4 {
    Mat4::from_cols_array(m)
}

impl TbHost {
    /// Index of the bone the served ball is attached to (marker 0x3f is not decoded; the right hand is used).
    pub fn hand_bone(&self) -> usize {
        let find = |n: &str| self.assets.skeleton.bones.iter().position(|b| b.name.eq_ignore_ascii_case(n));
        find("r_prop").or_else(|| find("r_hand")).unwrap_or(0)
    }

    /// World matrices of every bone of `chars[index]` in the engine's matrix layout.
    pub fn bone_matrices(&self, index: usize) -> Vec<Mat4> {
        let c = &self.chars[index];
        let root = to_mat(&c.world());
        let bones = &self.assets.skeleton.bones;
        let mut world: Vec<Mat4> = Vec::with_capacity(bones.len());
        for (i, b) in bones.iter().enumerate() {
            let x = c.animator.pose.get(i).copied().unwrap_or(BoneXf { rot: [0., 0., 0., 1.], trans: [0.; 3], scale: [1.; 3] });
            let local = Mat4::from_scale_rotation_translation(
                Vec3::from(x.scale),
                Quat::from_xyzw(x.rot[0], x.rot[1], x.rot[2], x.rot[3]).normalize(),
                Vec3::from(x.trans),
            );
            let parent = if b.parent < 0 { root } else { world[b.parent as usize] };
            world.push(parent * local);
        }
        world
    }

    fn attachment(&self, rt: &Runtime) -> Option<Attachment> {
        if !rt.ball.grabbed {
            return None;
        }
        let index = self.grab?;
        let hand = self.hand_bone();
        let m = self.bone_matrices(index).get(hand).copied()?;
        Some(Attachment { world: m.to_cols_array(), local: Some(matrix::IDENTITY) })
    }

    /// Eye jitter of a running camera shake (provisional amplitude: strength metres, decaying linearly).
    pub fn shake_offset(&mut self) -> [f32; 3] {
        if self.shake_ms <= 0 {
            return [0.; 3];
        }
        let k = self.shake_ms as f32 / self.shake.0 as f32 * self.shake.1;
        let mut r = |s: &mut u64| {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            ((*s >> 40) as f32 / (1u64 << 24) as f32) * 2. - 1.
        };
        let mut rng = self.rng;
        let o = [r(&mut rng) * k, r(&mut rng) * k, r(&mut rng) * k];
        self.rng = rng;
        o
    }

    /// Whether the ball is inside `player`'s narrow hit window (the predicate UpdateReturn / UpdateAccelerate use).
    pub fn in_swing_window(&self, rt: &Runtime, player: usize) -> bool {
        let accelerate = rt.life.match_state.state_code == 29;
        let row = if accelerate { 2 } else { 0 };
        let dist = rt.life.current_distance.clamp(0, 2) as usize;
        let t = &rt.state.hit.indicator_angles_360_36c_378_384;
        crate::tetherball_hit_animation::is_ball_in_hit_range(&rt.life, &rt.state.reset, &rt.ball, player, t[row][dist], t[row + 1][dist])
    }

    /// Conga callback entry (`RegularStrike`, `RegularStrikeReverse`, `OverhandStrike`, `ServeToss`).
    pub fn gesture(&mut self, rt: &mut Runtime, callback: GestureCallback, controller: i32) -> bool {
        rt.capture_gesture(callback, controller)
    }

    fn sync_animations(&mut self, rt: &mut Runtime) {
        for (p, c) in self.chars.iter().enumerate().take(2) {
            rt.life.players[p].current_animation = c.animator.anim.state as i32;
        }
    }

    /// Advance the world by `ms`: runs the recovered frame, then AI, locomotion and animation.
    pub fn frame(&mut self, rt: &mut Runtime, ms: i32, held: [u32; 2]) -> Result<Option<u32>, String> {
        self.time_ms += ms.max(0) as u64;
        if self.fade_left_ms > 0 {
            self.fade_left_ms -= ms;
        }
        for (i, c) in self.controllers.iter_mut().enumerate() {
            c.update(held[i], ms);
        }
        self.sync_animations(rt);
        // The accuracy percentage divides hits by attempts; the original's zero-attempt case is an undefined `divw`, so a player
        // who never faced the ball is given one attempt (0 %) once the last round ends.
        if rt.life.match_state.state_code == 30 {
            for p in 0..rt.life.player_count.min(2) {
                rt.life.statistics[p][0] = rt.life.statistics[p][0].max(1);
            }
        }
        let attachment = self.attachment(rt);
        let db = self.db.clone();
        let input = FrameInputs {
            milliseconds: ms,
            world_paused: false,
            area: self.area,
            attachment,
            controller_fx_offset: [0.2, 0.9, -0.15],
        };
        let result = rt.update(&input, &db, self)?;
        for p in 0..self.chars.len().min(2) {
            let d = rt.life.players[p].direction;
            if d != self.applied_dir[p] {
                self.applied_dir[p] = d;
                self.chars[p].dir = d;
            }
        }
        self.ball_matrix = rt.state.scene.ball_matrix;
        self.rope_matrix = rt.state.scene.rope_matrix;
        if !rt.life.paused {
            self.ai_step(rt, ms)?;
        }
        self.move_and_animate(rt, ms);
        self.camera_step(ms);
        self.sync_animations(rt);
        Ok(result)
    }

    fn camera_step(&mut self, ms: i32) {
        self.eye_target = self.camera.update(ms);
        if self.shake_ms > 0 {
            self.shake_ms -= ms;
        }
    }

    fn geometry(&self, rt: &Runtime, p: usize) -> AiGeometry {
        AiGeometry {
            world: rt.state.reset.world_matrix_398,
            inverse: self.inverse_world,
            anchor: rt.state.scene.anchor,
            character: self.chars[p].pos,
            desired_radius: rt.ball.desired_radius,
        }
    }

    fn move_inputs(&self, rt: &Runtime, p: usize) -> MoveInputs {
        MoveInputs {
            world: rt.state.reset.world_matrix_398,
            inverse: self.inverse_world,
            anchor: rt.state.scene.anchor,
            radius: rt.ball.desired_radius,
            character: self.chars[p].pos,
        }
    }

    /// `AIEntity::Think(ms, depth 2)` (0x802cc6b0): drop an expired compulsion, evaluate a replacement against the current
    /// priority (`CalculateCurrentPriority` 0x802cc93c; these compulsions are interruptible), activate it, then think.
    fn ai_step(&mut self, rt: &mut Runtime, ms: i32) -> Result<(), String> {
        let tuning = match self.hit_tuning {
            Some(t) => t,
            None => {
                let t = HitCompulsionTuning::load(&self.db, rt.life.session_mode, rt.life.game_type)?;
                self.hit_tuning = Some(t);
                t
            }
        };
        for p in 0..self.chars.len().min(2) {
            let Some(ai) = rt.state.ai[p].clone() else { continue };
            let inputs = self.move_inputs(rt, p);
            // 1. HasExpired -> SetCompulsion(null)
            let expired = match &self.active[p] {
                Some(Active::Move(m)) => ai.move_has_expired(m, &rt.life, &inputs, true),
                Some(Active::Hit(h)) => h.has_expired(&rt.ball),
                None => false,
            };
            if expired {
                match self.active[p].take() {
                    Some(Active::Move(mut m)) => m.deactivate(),
                    Some(Active::Hit(mut h)) => h.deactivate(),
                    None => {}
                }
                self.chars[p].target = None;
                self.chars[p].face = None;
            }
            // 2-3. EvaluateCompulsions(priority) -> SetCompulsion(new) -> Activate
            let priority = match &self.active[p] {
                Some(Active::Move(m)) => m.priority_009,
                Some(Active::Hit(h)) => h.priority,
                None => 0,
            };
            let geometry = self.geometry(rt, p);
            if let Some(new) = ai.evaluate(Some(&rt.life), priority, &geometry, &rt.ball, &rt.state.serve, &rt.state.rally) {
                if let Some(Active::Move(mut m)) = self.active[p].take() {
                    m.deactivate();
                }
                match new {
                    Compulsion::Move(mut m) => {
                        m.activate();
                        self.active[p] = Some(Active::Move(m));
                    }
                    Compulsion::Hit(mut h) => {
                        h.activate(&rt.ball, tuning, self);
                        self.active[p] = Some(Active::Hit(h));
                    }
                }
            }
            // 4. Compulsion::Think(ms): a true result publishes the movement request (+0x1c) for the character.
            match self.active[p].take() {
                Some(Active::Move(mut m)) => {
                    if m.think(ms, &inputs) {
                        self.chars[p].target = m.target_position_020;
                    }
                    // The edict's facing (`ProcessMovement` -> `SetFacingAngle`, flag at +0x28 set by `Think`).
                    self.chars[p].face = (m.target_angle_valid_034 && m.field_048 == Some(true)).then_some(m.target_angle_030);
                    self.active[p] = Some(Active::Move(m));
                }
                Some(Active::Hit(mut h)) => {
                    h.think(&rt.ball, ms, &mut rt.state.rally, self);
                    self.active[p] = Some(Active::Hit(h));
                }
                None => {}
            }
        }
        Ok(())
    }

    /// `CharacterMovement::Update` (0x802ed784) for an AI edict: the character walks toward the edict's waypoint at the
    /// bestiary `speed_walk` (movement type 0 -> `CharacterState` +0x1c), stops when the waypoint is reached (distance
    /// below 0.001), keeps the edict's facing, and maps speed to animation (walk state 1 while moving; state 0 when it
    /// stops from one of the mapped movement states 1..5).  The Havok character proxy that does the actual stepping is
    /// replaced by exact stepping.
    fn move_and_animate(&mut self, rt: &mut Runtime, ms: i32) {
        let dt = ms.max(0) as f32 / 1000.;
        let speed = self.walk_speed();
        for p in 0..self.chars.len().min(2) {
            let c = &mut self.chars[p];
            let mut moving = false;
            if let Some(t) = c.target {
                let d = [t[0] - c.pos[0], 0., t[2] - c.pos[2]];
                let len = (d[0] * d[0] + d[2] * d[2]).sqrt();
                if len >= 0.001 {
                    let step = speed * dt;
                    if len <= step {
                        c.pos[0] = t[0];
                        c.pos[2] = t[2];
                    } else {
                        c.pos[0] += d[0] / len * step;
                        c.pos[2] += d[2] / len * step;
                    }
                    moving = true;
                }
            }
            if let Some(a) = c.face {
                c.dir = [a.sin(), 0., a.cos()];
            }
            let current = rt.life.players[p].current_animation;
            if moving {
                c.moving = true;
                if current != 1 {
                    self.set_animation(p, 1, false, -1);
                }
            } else {
                c.moving = false;
                if (1..=5).contains(&current) {
                    self.set_animation(p, 0, false, -1);
                }
            }
        }
        let assets = self.assets.clone();
        let mut rng = self.rng;
        for c in &mut self.chars {
            let graph = if c.female { &assets.female } else { &assets.male };
            let events = c.animator.step(graph, &assets.lib, &assets.bind, dt, |lo, hi| {
                let span = (hi - lo + 1).max(1) as u64;
                lo + (xorshift(&mut rng) % span) as i32
            });
            for e in events {
                // `PlayerCharacterAnimationEventHandler::HandleEvent` (0x802eb730): CHAR_FOOTSTEP and SFX_ANIM.
                self.out.push(Out::AnimEvent { female: c.female, state: e.state as i32, name: e.name });
            }
        }
        self.rng = rng;
    }
}

impl TbHost {
    /// `speed_walk` of the character's bestiary entry (`CharacterState::Initialize` stores it at +0x1c).
    fn walk_speed(&self) -> f32 {
        self.db
            .find_collection("bestiary", "base")
            .and_then(|c| self.db.attribute(c, "speed_walk"))
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(2.5)
    }
}
