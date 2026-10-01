//! Host for the original minigame code running in the PowerPC VM (`gekko`).  This file holds the engine-side state the
//! hooks operate on and the boot sequence (policy, static constructors, singletons).
pub mod hooks;
pub mod vfs;
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
            q.starts_with("Physics") || q.ends_with("RenderEntity") || q.starts_with("Ren::") || q.starts_with("nw4") || q.starts_with("EAGL::Model") || q.starts_with("EAGL::Device") || q.starts_with("EAGL::RenderContext") || q.starts_with("EAGL::Geo") || q.starts_with("EAGLInternal::RenderContext") || q.starts_with("Csis::") || ["AreaManager", "PhysicsManager", "PhysicsRigidBody", "PhysicsDynamicCharacter", "PhysicsStaticCharacter", "Audio", "AuAEMSManager", "AuCharacterSoundObject", "AuEnvironmentManager", "AuHelpers", "PartFxManager", "PartFx", "FEManager", "WorldHudHandlers", "MinigameHandlers"].contains(&q.as_str())
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

pub type MgVm = Vm<MgHost>;

pub fn boot() -> Result<(MgVm, MgHost), String> {
    let mut vm = MgVm::load()?;
    vm.trap_unless(runs_natively);
    hooks::install(&mut vm);
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
    let r = vm.call_by_name(&mut host, "StartMinigameFadeComplete__8WorldManFv", &[0x805e8320], &[]);
    println!("RESULT {r:?}");
    if r.is_ok() {
        let frames = std::env::var("EAGL_MG_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
        for f in 0..frames {
            let r = vm.call_by_name(&mut host, "Update__8WorldManFi", &[0x805e8320, 33], &[]);
            if let Err(e) = r {
                println!("frame {f}: {e}");
                break;
            }
        }
    }
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
