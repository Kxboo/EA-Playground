//! The character animation state machine used by the minigame characters.
//!
//! Original: `AnimationStateGraph` (the `player.csv` / `player_female.csv` rows in `player_anims.viv`: asset list, looping,
//! random start, blend time, next state, reverse, frame-time, start/truncate time, events) and `AnimationState`
//! (`SetNextAnimState` 0x803C8B78, `SetNextAnim` 0x803C8318, `Update` 0x803C851C).  The arithmetic below follows those
//! bodies: time `t` (+0x48) advances by `dt * speed` (x30 when the row uses frame time), the clip length (+0x4c) is the
//! decoded duration minus the start and truncate offsets (+0x58/+0x5c), a looping state wraps by that length, a finished
//! non-looping state hands over to its next state (0xf7 = none), a state timer (+0x6c, the `int` argument in ms) forces
//! the next state (or idle) when it runs out, and the pose of the previous state is frozen when a state changes and
//! blended into the new one over the *outgoing* row's blend time.
//!
//! Not recovered (kept simple): `ProcessAnimEvents` only reports an event when its time is crossed; marker/prop bones and
//! the procedural handlers are not modelled.
use crate::anim::{Clip, CLIP_FPS};
use std::collections::HashMap;

const STATES: &str = include_str!("../../GameMap/data/enums/animation_states.tsv");
pub const NO_STATE: usize = 0xf7;

#[derive(Clone, Debug, Default)]
pub struct StateInfo {
    pub assets: Vec<String>,
    pub looping: bool,
    pub random_start: bool,
    pub blend: f32,
    pub next: Option<usize>,
    pub reverse: bool,
    pub frame_time: bool,
    pub start: f32,
    pub events: Vec<(String, f32)>,
    pub truncate: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub states: Vec<Option<StateInfo>>,
}

/// `AnimStates` enum token -> value (the table at 0x804eb124).
pub fn state_number(token: &str) -> Option<usize> {
    STATES.lines().filter(|l| !l.starts_with('#')).find_map(|l| {
        let mut p = l.split('\t');
        let (v, t) = (p.next()?, p.next()?);
        (t == token).then(|| v.parse().ok()).flatten()
    })
}

impl Graph {
    pub fn parse(csv: &str) -> Result<Graph, String> {
        let mut lines = csv.lines();
        let header: Vec<&str> = lines.next().ok_or("empty animation graph")?.split(',').collect();
        let col = |n: &str| header.iter().position(|h| h.trim() == n);
        let c = |n: &str| col(n).ok_or_else(|| format!("animation graph has no {n} column"));
        let (cs, ca, cl, cr, cb, cn, cv, cu, ct, ce1, ct1, ce2, ct2, ctr) = (
            c("ANIM_STATE")?, c("ANIM_ASSET")?, c("ANIM_LOOPING")?, c("RANDOM_START_TIME")?, c("ANIM_BLEND_TIME")?, c("ANIM_NEXT_STATE")?,
            c("ANIM_REVERSE")?, c("USE_FRAMETIME")?, c("START_TIME")?, c("EVENT_NAME1")?, c("EVENT_TIME1")?, c("EVENT_NAME2")?, c("EVENT_TIME2")?,
            c("TRUNCATE_TIME")?,
        );
        let mut graph = Graph::default();
        for line in lines {
            let f: Vec<&str> = line.trim_end().split(',').collect();
            let get = |i: usize| f.get(i).copied().unwrap_or("").trim();
            let Some(number) = state_number(get(cs)) else { continue };
            let num = |i: usize| get(i).parse::<f32>().unwrap_or(0.);
            let mut events = vec![];
            for (n, t) in [(ce1, ct1), (ce2, ct2)] {
                if !get(n).is_empty() {
                    events.push((get(n).to_string(), num(t)));
                }
            }
            let info = StateInfo {
                assets: get(ca).split('|').filter(|s| !s.is_empty()).map(str::to_string).collect(),
                looping: num(cl) != 0.,
                random_start: num(cr) != 0.,
                blend: num(cb),
                next: state_number(get(cn)),
                reverse: num(cv) != 0.,
                frame_time: num(cu) != 0.,
                start: num(ct),
                events,
                truncate: num(ctr),
            };
            if graph.states.len() <= number {
                graph.states.resize(number + 1, None);
            }
            graph.states[number] = Some(info);
        }
        Ok(graph)
    }

    pub fn load(viv_dir: &str, female: bool) -> Result<Graph, String> {
        let name = if female { "player_female.csv" } else { "player.csv" };
        let (bytes, _) = crate::archive::read_virtual(&format!("{viv_dir}/player_anims.viv::{name}"))?;
        Graph::parse(&String::from_utf8_lossy(&bytes))
    }

    pub fn info(&self, state: usize) -> Option<&StateInfo> {
        self.states.get(state).and_then(Option::as_ref)
    }

    /// Every asset name any row of the given states can play.
    pub fn assets_of(&self, states: impl IntoIterator<Item = usize>) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for s in states {
            if let Some(i) = self.info(s) {
                for a in &i.assets {
                    if !out.contains(a) {
                        out.push(a.clone());
                    }
                }
            }
        }
        out
    }
}

/// Decoded clips by graph asset name (`S_<asset>` in `player_anims.anm`).
#[derive(Default)]
pub struct Library {
    pub clips: HashMap<String, Clip>,
}

impl Library {
    pub fn clip(&self, asset: &str) -> Option<&Clip> {
        self.clips.get(asset)
    }
    /// Seconds -> the clip's own length in the units the state uses.
    fn length(&self, asset: &str, frame_time: bool) -> f32 {
        let Some(c) = self.clip(asset) else { return 0. };
        let frames = c.sample_count.saturating_sub(1) as f64;
        if frame_time { frames as f32 } else { (frames / CLIP_FPS) as f32 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimEvent {
    pub name: String,
    pub state: usize,
}

/// One character's `AnimationState`.
#[derive(Clone, Debug)]
pub struct AnimState {
    pub state: usize,
    pub asset: Option<String>,
    /// +0x48
    pub time: f32,
    /// +0x4c, effective length
    pub length: f32,
    pub speed: f32,
    /// +0x58 / +0x5c
    pub start: f32,
    pub truncate: f32,
    /// +0x60 / +0x64 / +0x68
    pub blend_left: f32,
    pub blend_total: f32,
    pub blending: bool,
    /// +0x69: the first Update after a change does not advance time.
    pub skip: bool,
    /// +0x6c state timer, in the units of the state (negative = none).
    pub timer: f32,
    pub events: Vec<AnimEvent>,
}

impl Default for AnimState {
    fn default() -> Self {
        AnimState {
            state: 0,
            asset: None,
            time: 0.,
            length: 0.,
            speed: 1.,
            start: 0.,
            truncate: 0.,
            blend_left: 0.,
            blend_total: 0.,
            blending: false,
            skip: false,
            timer: -0.001,
            events: vec![],
        }
    }
}

impl AnimState {
    /// `SetNextAnimState(state, speed, force, blend_ms)`.  Returns whether the state changed.  The caller snapshots the
    /// current pose (`Pose` of the previous frame) when this returns true.
    pub fn set_state(
        &mut self,
        graph: &Graph,
        lib: &Library,
        state: usize,
        speed: f32,
        force: bool,
        timer_ms: i32,
        mut random: impl FnMut(i32, i32) -> i32,
    ) -> bool {
        if state == self.state && !force && self.asset.is_some() {
            return false;
        }
        self.speed = speed;
        let Some(info) = graph.info(state) else { return false };
        let blend = graph.info(self.state).map(|i| i.blend).unwrap_or(0.);
        self.blend_left = blend;
        self.blend_total = blend;
        self.blending = true;
        self.skip = true;
        self.timer = timer_ms as f32 / 1000.;
        self.state = state;
        // SetNextAnim
        let pick = if info.assets.len() > 1 { random(0, info.assets.len() as i32 - 1).clamp(0, info.assets.len() as i32 - 1) as usize } else { 0 };
        let asset = info.assets.get(pick).cloned();
        self.start = info.start;
        self.truncate = info.truncate;
        let mut length = asset.as_deref().map(|a| lib.length(a, info.frame_time)).unwrap_or(0.);
        if info.frame_time {
            length -= 1.;
            if self.timer > 0. {
                self.timer *= 30.;
            }
            if self.blending {
                self.blend_left *= 30.;
                self.blend_total *= 30.;
            }
        }
        length -= self.start + self.truncate;
        self.length = length;
        self.time = if info.random_start {
            let r = random(0, 1000) as f32 / 1000.;
            r * length + self.start
        } else if info.reverse {
            length + self.start
        } else {
            self.start
        };
        self.asset = asset;
        true
    }

    /// `AnimationState::Update(dt)`; returns whether the state changed through a hand-over.
    pub fn update(&mut self, graph: &Graph, lib: &Library, dt: f32, mut random: impl FnMut(i32, i32) -> i32) -> bool {
        if self.skip {
            self.skip = false;
            return false;
        }
        let Some(info) = graph.info(self.state).cloned() else { return false };
        let dt = if info.frame_time { dt * 30. } else { dt };
        let before = self.time;
        let step = dt * self.speed;
        self.time += if info.reverse { -step } else { step };
        for (name, at) in &info.events {
            let at = *at + self.start;
            let crossed = if info.reverse { before >= at && self.time < at } else { before < at && self.time >= at };
            if crossed || (before == self.time && at == before) {
                self.events.push(AnimEvent { name: name.clone(), state: self.state });
            }
        }
        let mut changed = false;
        let ended = if info.reverse { self.time <= self.start } else { self.time >= self.length + self.start };
        if ended {
            if info.looping || (self.timer > 0. && info.next.is_some()) {
                if info.reverse { self.time += self.length } else { self.time -= self.length }
            } else {
                self.time = if info.reverse { self.start } else { self.length + self.start };
                if let Some(next) = info.next {
                    changed |= self.set_state(graph, lib, next, 1., false, -1, &mut random);
                }
            }
        }
        if self.timer > 0. && !changed {
            self.timer -= dt;
            if self.timer <= 0. {
                let next = info.next.unwrap_or(0);
                changed |= self.set_state(graph, lib, next, 1., false, -1, &mut random);
            }
        }
        if self.blending {
            self.blend_left -= dt;
            if self.blend_left <= 0. {
                self.blending = false;
            }
        }
        changed
    }

    /// Weight of the *new* pose while blending (`1 - left/total`), 1 when not blending.
    pub fn blend_weight(&self) -> f32 {
        if self.blending && self.blend_total > 0. { (1. - self.blend_left / self.blend_total).clamp(0., 1.) } else { 1. }
    }

    /// Animation frame (30 Hz) to sample in the active clip.
    pub fn frame(&self, graph: &Graph) -> f32 {
        let frames = if graph.info(self.state).is_some_and(|i| i.frame_time) { self.time } else { self.time * CLIP_FPS as f32 };
        frames.max(0.)
    }

    /// `IsAnimationComplete` (0x80343190): the state has reached the end of its clip.
    pub fn complete(&self) -> bool {
        self.time >= self.length + self.start
    }
}

/// One bone's local transform.
#[derive(Clone, Copy, Debug)]
pub struct BoneXf {
    pub rot: [f32; 4],
    pub trans: [f32; 3],
    pub scale: [f32; 3],
}

fn nlerp(a: [f32; 4], b: [f32; 4], w: f32) -> [f32; 4] {
    let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    let sign = if dot < 0. { -1. } else { 1. };
    let mut q: [f32; 4] = std::array::from_fn(|i| a[i] * (1. - w) + b[i] * sign * w);
    let n = q.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-8);
    q.iter_mut().for_each(|x| *x /= n);
    q
}

pub fn blend_pose(a: &[BoneXf], b: &[BoneXf], w: f32) -> Vec<BoneXf> {
    a.iter()
        .zip(b)
        .map(|(x, y)| BoneXf {
            rot: nlerp(x.rot, y.rot, w),
            trans: std::array::from_fn(|i| x.trans[i] * (1. - w) + y.trans[i] * w),
            scale: std::array::from_fn(|i| x.scale[i] * (1. - w) + y.scale[i] * w),
        })
        .collect()
}

fn sample<T: Copy>(samples: &[Option<T>], frame: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    if samples.is_empty() {
        return None;
    }
    let last = samples.len() - 1;
    let f = frame.clamp(0., last as f32);
    let (i, w) = (f.floor() as usize, f - f.floor());
    let a = samples[i].or_else(|| samples.iter().flatten().next().copied())?;
    let b = samples[(i + 1).min(last)].unwrap_or(a);
    Some(lerp(a, b, w))
}

/// Pose of `clip` at `frame` over the skeleton's bind pose (`bind[i]` for bones the clip does not animate).
pub fn pose_of(clip: &Clip, bind: &[BoneXf], frame: f32) -> Vec<BoneXf> {
    let mut out = bind.to_vec();
    for (bone, s) in &clip.rot {
        if let (Some(x), Some(q)) = (out.get_mut(*bone), sample(s, frame, |a: [f64; 4], b: [f64; 4], w| {
            let q = nlerp(a.map(|v| v as f32), b.map(|v| v as f32), w);
            q.map(|v| v as f64)
        })) {
            x.rot = q.map(|v| v as f32);
        }
    }
    for (bone, s) in &clip.trans {
        if let (Some(x), Some(v)) = (out.get_mut(*bone), sample(s, frame, |a: [f64; 3], b: [f64; 3], w| std::array::from_fn(|i| a[i] * (1. - w as f64) + b[i] * w as f64))) {
            x.trans = v.map(|c| c as f32);
        }
    }
    for (bone, s) in &clip.scale {
        if let (Some(x), Some(v)) = (out.get_mut(*bone), sample(s, frame, |a: [f64; 3], b: [f64; 3], w| std::array::from_fn(|i| a[i] * (1. - w as f64) + b[i] * w as f64))) {
            x.scale = v.map(|c| c as f32);
        }
    }
    out
}

pub fn bind_pose(skeleton: &crate::skeleton::Skeleton) -> Vec<BoneXf> {
    skeleton
        .bones
        .iter()
        .map(|b| BoneXf {
            rot: [b.quat[0] as f32, b.quat[1] as f32, b.quat[2] as f32, b.quat[3] as f32],
            trans: [b.trans[0] as f32, b.trans[1] as f32, b.trans[2] as f32],
            scale: [b.scale[0] as f32, b.scale[1] as f32, b.scale[2] as f32],
        })
        .collect()
}

/// A character's animation: the state machine plus the frozen pose it blends from.
pub struct Animator {
    pub anim: AnimState,
    pub frozen: Option<Vec<BoneXf>>,
    pub pose: Vec<BoneXf>,
    pub female: bool,
}

impl Animator {
    pub fn new(bind: Vec<BoneXf>, female: bool) -> Self {
        Animator { anim: AnimState::default(), frozen: None, pose: bind, female }
    }
    /// `SetNextAnimState`; the pose shown last frame becomes the blend source.
    pub fn set(&mut self, graph: &Graph, lib: &Library, state: usize, speed: f32, force: bool, timer_ms: i32, random: impl FnMut(i32, i32) -> i32) -> bool {
        let changed = self.anim.set_state(graph, lib, state, speed, force, timer_ms, random);
        if changed {
            self.frozen = Some(self.pose.clone());
        }
        changed
    }
    /// Advance and re-pose.  Returns events fired this step.
    pub fn step(&mut self, graph: &Graph, lib: &Library, bind: &[BoneXf], dt: f32, mut random: impl FnMut(i32, i32) -> i32) -> Vec<AnimEvent> {
        if self.anim.update(graph, lib, dt, &mut random) {
            self.frozen = Some(self.pose.clone());
        }
        let target = match self.anim.asset.as_deref().and_then(|a| lib.clip(a)) {
            Some(c) => pose_of(c, bind, self.anim.frame(graph)),
            None => bind.to_vec(),
        };
        let w = self.anim.blend_weight();
        self.pose = match (&self.frozen, self.anim.blending) {
            (Some(from), true) => blend_pose(from, &target, w),
            _ => target,
        };
        std::mem::take(&mut self.anim.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data() -> Option<String> {
        let dir = crate::bridge::data_root().join("files").join("data").join("characters");
        dir.exists().then(|| dir.to_string_lossy().replace('\\', "/"))
    }

    #[test]
    fn tetherball_rows_follow_the_original_graph() {
        let Some(dir) = data() else { return };
        let g = Graph::load(&dir, false).unwrap();
        // ANIM_TB_LOWHITNORMALSTART (61) -> IDLE (62); END (63) -> ANIM_TB_IDLE (56)
        assert_eq!(g.info(61).unwrap().next, Some(62));
        assert!(g.info(62).unwrap().looping);
        assert_eq!(g.info(63).unwrap().next, Some(56));
        assert_eq!(g.info(66).unwrap().next, Some(57));
        assert_eq!(g.info(85).unwrap().next, Some(56));
        assert!((g.info(56).unwrap().blend - 0.15).abs() < 1e-6);
        // Every tetherball asset exists in the animation bank under S_<asset>.
        let female = Graph::load(&dir, true).unwrap();
        let skeleton = crate::skeleton::Skeleton::parse(&crate::archive::read_virtual(&format!("{dir}/player_anims.viv::player_skel.ske")).unwrap().0).unwrap();
        let bank = crate::anim::Bank::parse(crate::archive::read_virtual(&format!("{dir}/player_anims.viv::player_anims.anm")).unwrap().0).unwrap();
        eprintln!("tb assets: {}", g.assets_of(56..=96).len());
        for graph in [&g, &female] {
            for asset in graph.assets_of(56..=96) {
                let index = bank.names.iter().position(|n| *n == format!("S_{asset}"));
                let Some(index) = index else { panic!("clip S_{asset} missing") };
                bank.decode(index, &skeleton).unwrap();
            }
        }
    }

    #[test]
    fn chain_and_loop_semantics() {
        let g = Graph::parse(
            "ANIM_STATE,ANIM_ASSET,ANIM_LOOPING,RANDOM_START_TIME,ANIM_BLEND_TIME,ANIM_NEXT_STATE,ANIM_REVERSE,USE_FRAMETIME,START_TIME,EVENT_NAME1,EVENT_TIME1,EVENT_NAME2,EVENT_TIME2,TRUNCATE_TIME\n\
             ANIM_TB_IDLE,A,1,0,0.15,,0,0,0,,,,,0\n\
             ANIM_TB_LOWHITNORMALSTART,B,0,0,0.15,ANIM_TB_LOWHITNORMALIDLE,0,0,0,,,,,0\n\
             ANIM_TB_LOWHITNORMALIDLE,C,1,0,0.15,,0,0,0,,,,,0\n",
        )
        .unwrap();
        let mut lib = Library::default();
        for (n, frames) in [("A", 31usize), ("B", 31), ("C", 31)] {
            lib.clips.insert(n.into(), Clip { index: 0, name: n.into(), sample_count: frames, codec: "t", rot: Default::default(), trans: Default::default(), scale: Default::default(), caveats: vec![] });
        }
        let mut a = AnimState::default();
        assert!(a.set_state(&g, &lib, 56, 1., false, -1, |l, _| l));
        assert!(!a.set_state(&g, &lib, 56, 1., false, -1, |l, _| l), "same state without force is ignored");
        assert!(a.set_state(&g, &lib, 61, 1., false, -1, |l, _| l));
        assert!((a.length - 1.).abs() < 1e-5);
        a.update(&g, &lib, 0.5, |l, _| l); // first update after a change does not advance
        assert!(a.time.abs() < 1e-6);
        a.update(&g, &lib, 0.5, |l, _| l);
        assert_eq!(a.state, 61);
        a.update(&g, &lib, 0.6, |l, _| l);
        assert_eq!(a.state, 62, "finished non-looping state hands over to its next state");
        for _ in 0..5 {
            a.update(&g, &lib, 0.6, |l, _| l);
        }
        assert_eq!(a.state, 62, "looping state stays");
        assert!(a.time < 1.);
    }
}
