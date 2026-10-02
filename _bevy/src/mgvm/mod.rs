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
    pub kid_keys: Vec<u64>,
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

/// Team layout (record offsets in the `Teams` image) for a quick match: (offset, kid slot, control code).
/// Control 1 = controller 1 (human), 2..4 further humans, 6 = computer.
fn team_layout(ty: i32, humans: usize) -> Vec<(u32, usize, u32)> {
    let mut out = vec![];
    match ty {
        // 3 v 3
        3 => {
            out.push((8, 0, 1));
            out.push((0x18, 2, 6));
            out.push((0x28, 3, 6));
            out.push((0x48, 1, if humans > 1 { 2 } else { 6 }));
            out.push((0x58, 4, 6));
            out.push((0x68, 5, 6));
        }
        // 1 v 1 default
        _ => {
            out.push((8, 0, 1));
            out.push((0x48, 1, if humans > 1 { 2 } else { 6 }));
        }
    }
    out
}

/// Start minigame `ty` (0 Dart, 1 RcCars, 2 Tetherball, 3 Dodgeball, 4 Footie, 5 Paper, 6 Wall, 8 FreeThrow) with the
/// first `humans` pads active: `WorldMan::StartMinigame` followed by the fade-complete step that creates it.
pub fn launch(vm: &mut MgVm, host: &mut MgHost, ty: i32, humans: usize) -> Result<(), String> {
    host.minigame_type = ty;
    let wm = world::WORLD_MAN;
    let ids = vm.img.addr("MinigameIDs").ok_or("MinigameIDs")?;
    let mgid = vm.r32(ids + 4 * ty as u32);
    let mg_id = vm.alloc_zeroed(8, 8);
    vm.w32(mg_id, mgid);
    let teams = vm.alloc_zeroed(0x100, 8);
    let kids = world::kid_keys(vm, host)?;
    host.kid_keys = kids.clone();
    vm.w32(teams, 1);
    for (off, slot, control) in team_layout(ty, humans) {
        let key = kids[slot % kids.len()];
        vm.w32(teams + off, (key >> 32) as u32);
        vm.w32(teams + off + 4, key as u32);
        vm.w32(teams + off + 8, control);
    }
    for i in 0..humans.min(4) {
        host.pads[i].active = true;
    }
    vm.call_by_name(host, "StartMinigame__8WorldManF4MGIDiQ25Enums23MiniGameDifficultyLevelRC5TeamsPCi", &[wm, mg_id, 1, 1, teams, 0], &[])?;
    vm.call_by_name(host, "StartMinigameFadeComplete__8WorldManFv", &[wm], &[])?;
    Ok(())
}

/// One game frame: pads -> guest, `WorldMan::Update(ms)`, then the minigame's draw pass.
pub fn frame(vm: &mut MgVm, host: &mut MgHost, ms: i32) -> Result<(), String> {
    hooks::write_pads(vm, &host.pads);
    vm.call_by_name(host, "Update__8WorldManFi", &[world::WORLD_MAN, ms as u32], &[])?;
    host.draws.clear();
    world::draw_minigame(vm, host, host.minigame_type)
}

/// Launch minigame `ty`, run `EAGL_MG_FRAMES` frames and report events / the final snapshot (development aid).
pub fn probe(ty: i32) {
    let (mut vm, mut host) = match boot() {
        Ok(v) => v,
        Err(e) => return println!("boot failed: {e}"),
    };
    for m in &vm.missing_symbols {
        println!("missing symbol {m}");
    }
    if let Err(e) = launch(&mut vm, &mut host, ty, 1) {
        println!("launch failed: {e}");
        return;
    }
    println!("launched {ty}");
    let frames: i32 = std::env::var("EAGL_MG_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let mg = vm.r32(world::WORLD_MAN + 0x90);
    for f in 0..frames {
        if f == 10 {
            let r = vm.call_by_name(&mut host, "OnPlay__8MinigameFv", &[mg], &[]);
            println!("OnPlay {r:?}");
        }
        host.pads[0].buttons = if f > 650 && (f / 8) % 5 == 0 { 0x0008 } else { 0 };
        if let Err(e) = frame(&mut vm, &mut host, 33) {
            println!("frame {f}: {e}");
            break;
        }
        for e in host.events.drain(..) {
            println!("  [{f}] {} {:x?} {:?}", e.name, e.ints, e.floats);
        }
    }
    let snap = snapshot::snapshot(&mut vm, &mut host);
    println!("snapshot: {} chars, camera {:?}", snap.chars.len(), snap.camera);
    for c in snap.chars.iter().take(8) {
        println!("  char key {:x} pos {:?} angle {} anim {} bones {}", c.key, c.pos, c.angle, c.anim_state, c.pose.len());
    }
    println!("draws {:?}", snap.draws.iter().map(|d| d.0.clone()).collect::<Vec<_>>());
    println!("physics chars {} bodies {} colliders {}", host.phys.chars.len(), host.phys.bodies.len(), host.phys.world.colliders.len());
    for l in &host.phys.log {
        println!("  phys: {l}");
    }
    if std::env::var("EAGL_MG_LOG").is_ok() {
        for l in &host.log {
            println!("  log: {l}");
        }
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
