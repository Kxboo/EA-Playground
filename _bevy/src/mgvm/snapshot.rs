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
    /// Vertical field of view (radians) of the camera above.
    pub fov: f32,
    /// Near clip plane of the game's viewport (`SceneOptions+0x14`; the default options use 1.0).
    pub near: f32,
    /// `gRenderWorld`: false while a game draws its own environment instead of the playground (RcCars' track).
    pub render_world: bool,
    /// `EAGL::DrawTextured` batches of this frame and the texture banks they name.
    pub imm: Vec<super::ImmDraw>,
    pub banks: std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>,
    /// World placeables whose visibility (`Placeable+0xa8`) the minigame changed since launch: (name, visible now).
    pub placeables: Vec<(String, bool)>,
}

/// `PlaceableManager` (AreaManager +0x1b0): entries of 0x120 bytes at +8, count +0xc; name CString +4, visible flag +0xa8.
pub fn placeable_flags(vm: &mut MgVm, host: &mut MgHost) -> Vec<(String, bool)> {
    let world = vm.r32(WORLD_MAN + 0x88);
    let am = if world != 0 { vm.r32(world + 8) } else { 0 };
    if am == 0 {
        return vec![];
    }
    let pm = am + 0x1b0;
    let (base, n) = (vm.r32(pm + 8), vm.r32(pm + 0xc).min(4096));
    let mut out = vec![];
    for i in 0..n {
        let e = base + 0x120 * i;
        let p = vm.call_by_name(host, "c_str__7CStringCFv", &[e + 4], &[]).unwrap_or(0);
        out.push((vm.st.mem.cstr(p, 96), vm.st.mem.r8(e + 0xa8) != 0));
    }
    out
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
    out.imm = std::mem::take(&mut host.imm);
    out.fov = host.fov;
    out.near = host.near.unwrap_or(1.0);
    out.render_world = vm.img.addr("gRenderWorld").is_none_or(|a| vm.st.mem.r8(a) != 0);
    if !host.placeables_at_launch.is_empty() && vm.r32(WORLD_MAN + 0x90) != 0 {
        let now = placeable_flags(vm, host);
        out.placeables = now.into_iter().zip(host.placeables_at_launch.iter()).filter(|(a, b)| a.1 != b.1).map(|(a, _)| a).collect();
    }
    out.banks = host.tar_banks.clone();
    // viewport 0's camera (`CameraManager` slot 0, what the renderer draws with) in its final pose
    // (`Camera::GetPos/GetTarget(true)`, after transitions and shake); else the camera the game last placed
    let manager = vm.img.addr("sCameraManagerInstance__13CameraManager").map(|a| vm.r32(a)).unwrap_or(0);
    let c = if manager != 0 && vm.r32(manager) != 0 { vm.r32(manager) } else { host.camera };
    if c != 0 {
        let v = |vm: &mut MgVm, o: u32| [vm.st.mem.rf32(c + o), vm.st.mem.rf32(c + o + 4), vm.st.mem.rf32(c + o + 8)];
        let (eye, target) = (v(vm, 0x70), v(vm, 0x80));
        out.camera = if eye != target { Some((eye, target, [0., 1., 0.])) } else { Some((v(vm, 0x10), v(vm, 0x20), v(vm, 0x30))) };
    }
    out
}
