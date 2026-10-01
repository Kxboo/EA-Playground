//! Host for the original minigame code running in the PowerPC VM (`gekko`).  This file holds the engine-side state the
//! hooks operate on and the boot sequence (policy, static constructors, singletons).
pub mod hooks;
pub mod vfs;
pub mod physics;
pub mod snapshot;
pub mod physhooks;
pub mod world;
use crate::gekko::Vm;

/// Engine state the hooks share.  Guest objects live in VM memory; this holds what only the host knows.
#[derive(Default)]
pub struct MgHost {
    pub log: Vec<String>,
    /// `MGID` -> minigame type: 0 DartShootout, 1 RcCars, 2 Tetherball, 3 Dodgeball, 4 Footie, 5 PaperAirplanes, 6 Wallball, 8 FreeThrow.
    pub minigame_type: i32,
    pub vfs: vfs::Vfs,
    pub fe_manager: u32,
    pub scene_options: u32,
    pub events: Vec<FeEvent>,
    /// Model draws of the current frame: (model name, world matrix).
    pub draws: Vec<(String, [f32; 16])>,
    /// Names of the placeholder models handed out by the asset manager.
    pub model_names: std::collections::HashMap<u32, String>,
    /// The camera whose position was set last.
    pub camera: u32,
    pub pads: [hooks::Pad; 4],
    pub phys: physics::Physics,
    pub pending_contacts: Vec<physics::Contact>,
    /// Names of null-serviced functions already reported.
    pub stubbed: std::collections::BTreeSet<String>,
}

/// The mangled-name class of a symbol: `Update__11MGDodgeballFi` -> `MGDodgeball`.
pub fn class_of(name: &str) -> Option<&str> {
    let bytes = name.as_bytes();
    let mut i = 0;
    while let Some(p) = name[i..].find("__") {
        let s = i + p + 2;
        // nested: `Q24EAGL5Model` = EAGL::Model
        if s + 1 < bytes.len() && bytes[s] == b'Q' && bytes[s + 1].is_ascii_digit() {
            let n = (bytes[s + 1] - b'0') as usize;
            let mut e = s + 2;
            let mut ok = true;
            for _ in 0..n {
                let ds = e;
                while e < bytes.len() && bytes[e].is_ascii_digit() {
                    e += 1;
                }
                match name[ds..e].parse::<usize>() {
                    Ok(len) if len > 0 && e + len <= name.len() => e += len,
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                return Some(&name[s..e]);
            }
        }
        let mut e = s;
        while e < bytes.len() && bytes[e].is_ascii_digit() {
            e += 1;
        }
        if e > s {
            if let Ok(len) = name[s..e].parse::<usize>() {
                if e + len <= name.len() && len > 0 {
                    return Some(&name[e..e + len]);
                }
            }
        }
        i = i + p + 2;
    }
    None
}

/// The mangled class token at the start of `rest` (`15NPCRenderEntityFP20..` -> `15NPCRenderEntity`).
pub fn mangled_class(rest: &str) -> Option<String> {
    let b = rest.as_bytes();
    let mut e = 0;
    let parts = if b.len() > 1 && b[0] == b'Q' && b[1].is_ascii_digit() {
        e = 2;
        (b[1] - b'0') as usize
    } else {
        1
    };
    for _ in 0..parts {
        let ds = e;
        while e < b.len() && b[e].is_ascii_digit() {
            e += 1;
        }
        let len: usize = rest[ds..e].parse().ok()?;
        if len == 0 || e + len > rest.len() {
            return None;
        }
        e += len;
    }
    Some(rest[..e].to_string())
}

/// Classes whose methods are engine services (hardware, rendering, audio, assets, front end).
const SERVICE_CLASSES: &[&str] = &[
    "PhysicsManager", "PhysicsRigidBody", "PhysicsCharacter", "PhysicsUserData", "PhysicsCharacterListener",
    "PhysicsRigidBodyListener", "Audio", "AuAEMSManager", "AuCharacterSoundObject", "AuEnvironmentManager", "AuHelpers", "FEManager",
    "MemMgr", "PartFxManager", "PartFx", "AreaManager",
    "WorldHudHandlers", "MinigameHandlers", "MinigameLVHandlers", "MinigameFSHandlers",
];

/// Services that are rendering / audio / effects only: when nothing hooks them they do nothing and return 0 (logged).
pub fn is_soft_stub(name: &str) -> bool {
    if NATIVE_EXCEPTIONS.contains(&name) {
        return false;
    }
    if class_of(name).is_none() && ["DC", "IC", "L2", "LC", "OS", "DVD", "VI", "GX", "PAD", "WPAD", "KPAD", "AX", "AI", "EXI", "SI", "IPC", "NAND", "CARD", "WENC", "TRC"].iter().any(|p| name.starts_with(p) && name[p.len()..].chars().next().map_or(false, |c| c.is_ascii_uppercase() || c == '_')) {
        return true;
    }
    match class_of(name) {
        Some(c) => {
            let q = qualified(c);
            (q.ends_with("Handlers") && !q.ends_with("LVHandlers") && !q.ends_with("FSHandlers")) || q.starts_with("Physics") || q.ends_with("RenderEntity") || q.starts_with("Ren::") || q.starts_with("nw4") || q.starts_with("EAGL::Model") || q.starts_with("EAGL::Draw") || q.starts_with("EAGL::Device") || q.starts_with("EAGL::RenderContext") || q.starts_with("EAGL::Geo") || q.starts_with("EAGLInternal::RenderContext") || q.starts_with("Csis::") || ["AreaManager", "PhysicsManager", "PhysicsRigidBody", "PhysicsDynamicCharacter", "PhysicsStaticCharacter", "Audio", "AuAEMSManager", "AuCharacterSoundObject", "AuEnvironmentManager", "AuHelpers", "PartFxManager", "PartFx", "FEManager", "WorldHudHandlers", "MinigameHandlers", "TRC"].contains(&q.as_str())
        }
        None => false,
    }
}
const SERVICE_PREFIXES: &[&str] = &["Ren::", "nw4r::", "nw4hbm::", "Apt", "Wii", "hkp", "hkWorld", "hkCharacterProxy", "hkRigid", "OS", "DVD", "GX", "VI", "AX", "PAD", "KPAD", "WPAD", "WENC", "Csis::WSFX", "Csis::MGSFX", "Csis::FESFX", "Csis::Class::", "Csis::System"];

/// `Q24Csis11ClassHandle` -> `Csis::ClassHandle`; plain names are returned as they are.
fn qualified(c: &str) -> String {
    let b = c.as_bytes();
    if b.len() > 2 && b[0] == b'Q' && b[1].is_ascii_digit() {
        let n = (b[1] - b'0') as usize;
        let mut i = 2;
        let mut parts = vec![];
        for _ in 0..n {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let Ok(len) = c[s..i].parse::<usize>() else { break };
            if i + len > c.len() {
                break;
            }
            parts.push(&c[i..i + len]);
            i += len;
        }
        return parts.join("::");
    }
    c.to_string()
}

/// Whether a function may run natively in the VM (game logic, math, containers) as opposed to being an engine service.
/// Service-class functions that are really game logic and run natively.
const NATIVE_EXCEPTIONS: &[&str] = &["SetNewOverride__Q24EAGL6DeviceFPFUlPCc_PvPFUlPCc_Pv", "SetDeleteOverride__Q24EAGL6DeviceFPFPvUl_v"];

pub fn runs_natively(name: &str) -> bool {
    if NATIVE_EXCEPTIONS.contains(&name) {
        return true;
    }
    if name.starts_with("__sinit_") {
        return true;
    }
    if is_soft_stub(name) {
        return false;
    }
    match class_of(name) {
        Some(c) => {
            let q = qualified(c);
            !(SERVICE_CLASSES.contains(&q.as_str()) || SERVICE_PREFIXES.iter().any(|p| q.starts_with(p)))
        }
        // free functions: C runtime helpers and math run natively; engine entry points are bound by name
        None => !(name.starts_with("GX") || name.starts_with("OS") || name.starts_with("DVD") || name.starts_with("VI") || name.starts_with("PAD")),
    }
}

/// A call into a front-end handler class (`MinigameHandlers`, `PreGameHandlers`, ...): HUD updates and screens.
#[derive(Clone, Debug)]
pub struct FeEvent {
    pub name: String,
    pub ints: [u32; 4],
    pub floats: [f32; 2],
}

pub type MgVm = Vm<MgHost>;

pub fn boot() -> Result<(MgVm, MgHost), String> {
    let mut vm = MgVm::load()?;
    vm.trap_unless(runs_natively);
    hooks::install(&mut vm);
    physhooks::install(&mut vm);
    hooks::install_stubs(&mut vm);
    let mut host = MgHost::default();
    if let Err(e) = world::boot(&mut vm, &mut host) {
        if std::env::var("EAGL_MG_LOG").is_ok() {
            for l in &host.log {
                eprintln!("  log: {l}");
            }
        }
        return Err(e);
    }
    Ok((vm, host))
}

/// Run `WorldMan::StartMinigameFadeComplete` for minigame `ty` and report where the machine stops (development aid).
pub fn probe(ty: i32) {
    let (mut vm, mut host) = match boot() {
        Ok(v) => v,
        Err(e) => return println!("boot failed: {e}"),
    };
    for m in &vm.missing_symbols {
        println!("missing symbol {m}");
    }
    host.minigame_type = ty;
    let wm = 0x805e8320u32;
    let ids = vm.img.addr("MinigameIDs").unwrap();
    let mgid = vm.r32(ids + 4 * ty as u32);
    println!("MinigameIDs[{ty}] = {mgid:#x}");
    // WorldMan::StartMinigame(MGID, players, difficulty, Teams, rules) then the fade-complete step that creates it
    let mg_id = vm.alloc_zeroed(8, 8);
    vm.w32(mg_id, mgid);
    let teams = vm.alloc_zeroed(0x100, 8);
    let kids_dbg;
    let kids = world::kid_keys(&mut vm, &mut host).unwrap_or_else(|e| { println!("kid_keys: {e}"); vec![] });
    println!("kids {kids:x?}");
    kids_dbg = kids.clone();
    if kids.len() >= 2 {
        vm.w32(teams, 1);
        let mut rec = |off: u32, key: u64, control: u32| {
            vm.w32(teams + off, (key >> 32) as u32);
            vm.w32(teams + off + 4, key as u32);
            vm.w32(teams + off + 8, control);
        };
        rec(8, kids[0], 1);
        rec(0x18, kids[2], 6);
        rec(0x28, kids[3], 6);
        rec(0x48, kids[1], 6);
        rec(0x58, kids[4], 6);
        rec(0x68, kids[5], 6);
    }
    let req = vm.call_by_name(&mut host, "StartMinigame__8WorldManF4MGIDiQ25Enums23MiniGameDifficultyLevelRC5TeamsPCi", &[wm, mg_id, 1, 1, teams, 0], &[]);
    println!("StartMinigame {req:?}");
    let r = vm.call_by_name(&mut host, "StartMinigameFadeComplete__8WorldManFv", &[wm], &[]);
    println!("RESULT {r:?}");
    if r.is_ok() {
        let wm = 0x805e8320u32;
        let (cur, mg) = (vm.r32(wm + 0x8c), vm.r32(wm + 0x90));
        let vt = |vm: &mut MgVm, p: u32| {
            let v = vm.r32(p);
            vm.name_of(v)
        };
        let (a, b) = (vt(&mut vm, cur), vt(&mut vm, mg));
        println!("world {cur:#x} (vtable {a})  minigame {mg:#x} (vtable {b})");
        let nteams = vm.r32(mg + 0x15c);
        println!("teams {nteams} counts {} {}", vm.r32(mg + 0x148), vm.r32(mg + 0x14c));
        for i in 0..8 {
            let c = vm.r32(mg + 0x11c + 4 * i);
            let m = if c != 0 { vm.r32(c + 0x88) } else { 0 };
            println!("  slot {i}: char {c:#x} meter {m:#x}");
        }
        let frames = std::env::var("EAGL_MG_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
        for f in 0..frames {
            if f == 10 {
                let r = vm.call_by_name(&mut host, "OnPlay__8MinigameFv", &[mg], &[]);
                println!("OnPlay {r:?}");
            }
            for e in host.events.drain(..) {
                println!("  [{f}] {} {:x?} {:?}", e.name, e.ints, e.floats);
            }
            if f % 100 == 0 {
                let c = vm.r32(mg + 0x11c);
                let ch = vm.r32(c + 0x4);
                let pos = [vm.st.mem.rf32(ch + 0x180), vm.st.mem.rf32(ch + 0x184), vm.st.mem.rf32(ch + 0x188)];
                println!("frame {f}: state {} player0 {:?}", vm.r32(mg + 0x34), pos);
            }
            host.pads[0].active = true;
            // tap A every 40 frames once the match is running
            host.pads[0].buttons = if f > 650 && (f / 8) % 5 == 0 { 0x0008 } else { 0 };
            hooks::write_pads(&mut vm, &host.pads);
            let r = vm.call_by_name(&mut host, "Update__8WorldManFi", &[0x805e8320, 33], &[]);
            if let Err(e) = r {
                println!("frame {f}: {e}");
                break;
            }
        }
    }
    if let Err(e) = world::draw_minigame(&mut vm, &mut host, ty) {
        println!("draw: {e}");
    }
    let snap = snapshot::snapshot(&mut vm, &mut host);
    println!("snapshot: {} chars, draws {:?}, camera {:?}", snap.chars.len(), snap.draws.iter().map(|d| d.0.clone()).collect::<Vec<_>>(), snap.camera);
    for c in &snap.chars {
        println!("  char key {:x} pos {:?} angle {} anim {} bones {}", c.key, c.pos, c.angle, c.anim_state, c.pose.len());
    }
    for l in &host.phys.log {
        println!("  phys: {l}");
    }
    {
        let mut y = 60.0f32;
        for _ in 0..12 {
            match host.phys.cast_ray([88.527, y, 3.034], [0., -1., 0.], 200.) {
                Some(t) => {
                    println!("  ground hit at y={}", y - t);
                    y = y - t - 0.01;
                }
                None => break,
            }
        }
    }
    for (h, c) in host.phys.world.colliders.iter() {
        let a = c.compute_aabb();
        if a.mins.x <= 88.527 && a.maxs.x >= 88.527 && a.mins.z <= 3.034 && a.maxs.z >= 3.034 {
            println!("  collider {:?} shape {:?} aabb {:?}..{:?}", h, c.shape().shape_type(), a.mins, a.maxs);
        }
    }
    {
        let mgp = vm.r32(0x805e8320 + 0x90);
        let dc = vm.r32(mgp + 0x11c);
        let c = vm.r32(dc + 4);
        let a = vm.r32(c + 0x18);
        let pose = vm.r32(a + 0x30);
        println!("char {c:#x} animstate {a:#x} state {} bones {} pose {pose:#x}", vm.r32(a + 0x54), vm.r32(a + 0x40) / 12);
        for b in 0..3 {
            let f: Vec<f32> = (0..12).map(|i| vm.st.mem.rf32(pose + 48 * b + 4 * i)).collect();
            println!("  pose bone {b}: {f:?}");
        }
        let skin = vm.r32(a + 0x1674);
        for b in 0..3 {
            let f: Vec<f32> = (0..16).map(|i| vm.st.mem.rf32(skin + 64 * b + 4 * i)).collect();
            println!("  skin bone {b}: {f:?}");
        }
    }
    {
        let mgp = vm.r32(0x805e8320 + 0x90);
        for slot in 0..8u32 {
            let dc = vm.r32(mgp + 0x11c + 4 * slot);
            let c = vm.r32(dc + 4);
            if c == 0 {
                continue;
            }
            for off in (0..0x400u32).step_by(4) {
                let hi = vm.r32(c + off);
                let lo = vm.r32(c + off + 4);
                let v = ((hi as u64) << 32) | lo as u64;
                if let Some(i) = kids_dbg.iter().position(|k| *k == v) {
                    println!("slot {slot} char {c:#x}: kid {i} key found at +{off:#x}");
                }
            }
        }
    }
    println!("physics chars {} bodies {} colliders {}", host.phys.chars.len(), host.phys.bodies.len(), host.phys.world.colliders.len());
    for l in host.log.iter().rev().take(40).rev() {
        println!("  log: {l}");
    }
}

#[cfg(test)]
mod tests {
    /// Run `WorldMan::StartMinigameFadeComplete` for `EAGL_MG` (default Dodgeball) and report where the machine stops.
    #[test]
    #[ignore]
    fn launch_probe() {
        let Ok((mut vm, mut host)) = super::boot() else { return };
        for m in &vm.missing_symbols {
            println!("missing symbol {m}");
        }
        host.minigame_type = std::env::var("EAGL_MG").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
        let r = vm.call_by_name(&mut host, "StartMinigameFadeComplete__8WorldManFv", &[0x805e8320], &[]);
        println!("RESULT {r:?}");
    }

    use super::*;

    #[test]
    fn class_names_come_from_the_mangling() {
        assert_eq!(class_of("Update__11MGDodgeballFi"), Some("MGDodgeball"));
        assert_eq!(class_of("__ct__11MGDodgeballFPQ23Ren5Scene"), Some("MGDodgeball"));
        assert_eq!(class_of("rmDistanceXZ__FRC9rmVector3RC9rmVector3"), None);
        assert!(runs_natively("rmDistanceXZ__FRC9rmVector3RC9rmVector3"));
        assert!(!runs_natively("GetPlayerCharacter__8WorldManFi"));
        assert!(runs_natively("Update__11MGDodgeballFi"));
    }

    /// Run the static constructors (`.ctors`) and report which ones need engine services.
    #[test]
    #[ignore]
    fn static_constructors() {
        let Ok((mut vm, mut host)) = boot() else { return };
        vm.soft_traps = true;
        let (start, end) = (0x8041cee0u32, 0x8041cee0u32 + 0x874);
        let mut a = start;
        let mut ok = 0;
        let mut out = vec![];
        while a < end {
            let f = vm.st.mem.r32(a);
            a += 4;
            if f == 0 {
                continue;
            }
            match vm.call(&mut host, f, &[], &[]) {
                Ok(_) => ok += 1,
                Err(e) => out.push(format!("{}: {e}", vm.name_of(f))),
            }
        }
        println!("static constructors ok {ok}, failed {}", out.len());
        for l in out {
            println!("  ERR {l}");
        }
        let mut m: Vec<_> = vm.missing.iter().collect();
        m.sort_by_key(|x| std::cmp::Reverse(*x.1));
        for (n, c) in m {
            println!("  MISSING {c} {n}");
        }
    }
}
