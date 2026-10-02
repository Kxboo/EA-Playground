//! What the renderer needs from the guest after a frame: characters (position, heading, skeleton pose), model draws,
//! the active camera.
use super::{MgHost, MgVm};

pub const WORLD_MAN: u32 = 0x805e_8320;

#[derive(Clone, Debug)]
pub struct CharSnap {
    pub ptr: u32,
    /// Database key of the character (`CharacterState` +0).
    pub key: u64,
    pub pos: [f32; 3],
    /// `rmAngle` heading (atan2(x, z)).
    pub angle: f32,
    pub anim_state: i32,
    /// Local bone transforms: rotation xyzw, translation, scale.
    pub pose: Vec<([f32; 4], [f32; 3], [f32; 3])>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub chars: Vec<CharSnap>,
    pub draws: Vec<(String, [f32; 16])>,
    pub camera: Option<([f32; 3], [f32; 3], [f32; 3])>,
}

pub fn snapshot(vm: &mut MgVm, host: &mut MgHost) -> Snapshot {
    let mut out = Snapshot::default();
    let world = vm.r32(WORLD_MAN + 0x8c);
    let cm = if world != 0 { vm.r32(world + 0x18) } else { 0 };
    if cm != 0 {
        let count = vm.r32(cm + 0x74).min(64);
        for i in 0..count {
            let c = vm.r32(cm + 4 * i);
            if c == 0 {
                continue;
            }
            let anim = vm.r32(c + 0x18);
            let hi = vm.r32(c + 0x130) as u64;
            let lo = vm.r32(c + 0x134) as u64;
            let mut snap = CharSnap {
                ptr: c,
                key: (hi << 32) | lo,
                pos: [vm.st.mem.rf32(c + 0x180), vm.st.mem.rf32(c + 0x184), vm.st.mem.rf32(c + 0x188)],
                angle: vm.st.mem.rf32(c + 0x1b0),
                anim_state: 0,
                pose: vec![],
            };
            if anim != 0 {
                snap.anim_state = vm.r32(anim + 0x54) as i32;
                let bones = vm.r32(anim + 0x40) / 12;
                let pose = vm.r32(anim + 0x30);
                if pose != 0 && bones <= 128 {
                    for b in 0..bones {
                        let f: Vec<f32> = (0..11).map(|k| vm.st.mem.rf32(pose + 48 * b + 4 * k)).collect();
                        snap.pose.push(([f[4], f[5], f[6], f[7]], [f[8], f[9], f[10]], [f[0], f[1], f[2]]));
                    }
                }
            }
            out.chars.push(snap);
        }
    }
    out.draws = std::mem::take(&mut host.draws);
    if host.camera != 0 {
        let c = host.camera;
        let v = |vm: &mut MgVm, o: u32| [vm.st.mem.rf32(c + o), vm.st.mem.rf32(c + o + 4), vm.st.mem.rf32(c + o + 8)];
        out.camera = Some((v(vm, 0x10), v(vm, 0x20), v(vm, 0x30)));
    }
    if host.minigame_type == 1 {
        // RcCars: follow the first car's body (the player's) from behind; its local +X is forward
        if let Some((_, m)) = out.draws.iter().find(|(n, _)| n.contains("body") || n.starts_with("model@")) {
            let f = [m[0], 0., m[2]];
            let l = (f[0] * f[0] + f[2] * f[2]).sqrt().max(1e-4);
            let f = [f[0] / l, 0., f[2] / l];
            let p = [m[12], m[13], m[14]];
            let eye = [p[0] - f[0] * 3.2, p[1] + 1.5, p[2] - f[2] * 3.2];
            let tgt = [p[0] + f[0] * 4., p[1] + 0.3, p[2] + f[2] * 4.];
            out.camera = Some((eye, tgt, [0., 1., 0.]));
        }
    }
    out
}
