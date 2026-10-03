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
    /// The whole single-player game runs (`launch` type 99): `minigame_type` follows the minigame the world started.
    pub world_mode: bool,
    /// The player's kid, set in the profile before the world creates its characters.
    pub player_kid: u32,
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
    pub viewport: u32,
    pub vp_matrix: u32,
    /// Pointer (Wii Remote IR) position in -0.5..0.5 with +y up, when the remote is aimed at the screen.
    pub pointer: Option<[f32; 2]>,
    /// Vertical field of view (radians) of the minigame's viewport.
    pub fov: f32,
    /// The viewport's own field of view (`SceneOptions` +0x1c): degrees across a 4:3 screen.
    pub fov_h43: Option<f32>,
    pub aspect: f32,
    pub pads: [hooks::Pad; 4],
    pub phys: physics::Physics,
    pub pending_contacts: Vec<physics::Contact>,
    /// Names of null-serviced functions already reported.
    pub stubbed: std::collections::BTreeSet<String>,
    /// Scratch record handed out for every `PartFx` (particles are not simulated; the game only pokes flags in it).
    pub dummy_partfx: u32,
    /// Guest heap blocks by start address -> capacity, and freed ones by capacity.
    pub blocks: std::collections::HashMap<u32, u32>,
    pub free_blocks: std::collections::HashMap<u32, Vec<u32>>,
    /// RcCars: (position, target, up) the chase camera last set.
    pub view: Option<[[f32; 3]; 3]>,
    /// Sound effects the game played this frame: (Csis class, variant), e.g. ("MGSFX_Grunts_Male", 3).
    pub sounds: Vec<(String, usize)>,
    /// Answer for `AIP::CmdDecomposer::GetIntArrayByName` when the host calls a front-end handler itself.
    pub int_array: Vec<i32>,
    /// `TarManager`s initialised so far: (object, texture bank path).
    pub tar_managers: Vec<(u32, String)>,
    /// `EAGL::TAR` -> (bank path, shape name, index), filled from the managers' tables (bank order).
    pub tars: std::collections::HashMap<u32, (String, String, usize)>,
    /// Texture banks behind those managers, copied out of guest memory (they may live inside in-place-loaded archives).
    pub tar_banks: std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>,
    /// Open `EAGL::DrawTextured` objects.
    pub draw_textured: std::collections::HashMap<u32, DrawTexturedState>,
    /// Batches finished this frame.
    pub imm: Vec<ImmDraw>,
    /// Placeable visibility flags when the minigame started (the first snapshot).
    pub placeables_at_launch: Vec<(String, bool)>,
    /// Particle effects requested since the last frame.
    pub fx: Vec<FxEvent>,
    /// GUID -> stand-in `PartFx` object (and back), so `GetPartFx(guid)->SetPos(..)` reaches the right effect.
    pub partfx_objs: std::collections::HashMap<u32, u32>,
    pub partfx_ids: std::collections::HashMap<u32, u32>,
    pub next_fx: u32,
    /// Music the game asked for since the last frame: `Some("")` = stop, `Some("world")` = the area's world music,
    /// otherwise a file of `audio/music` (`kMusicFilenames`).
    pub music: Option<String>,
    /// Stand-in `AuAEMSManager` (+4 = enabled) handed to the original `PlaySFX` switch.
    pub aems_mgr: u32,
    /// `g<Class>Handle__4Csis` address -> class name.
    pub csis_handles: std::collections::HashMap<u32, String>,
    /// Debug: thrown balls being followed (physics body, target DodgeballCharacter, frames left).
    /// `AreaManager::SetCurvedWorldRadius` while the minigame runs (Paper Airplanes bends its course with 75).
    pub curved_radius: Option<f32>,
    /// `WorldMan::SetDrawCurrentAreaModel` (false: the minigame draws its own environment).
    pub hide_area_model: bool,
    /// Near clip plane the game set with `Ren::Scene::SetViewPort` (none: the default `SceneOptions`, 1.0).
    pub near: Option<f32>,
    /// The previous frame's model draws (development logs).
    pub last_draws: Vec<(String, [f32; 16])>,
    /// Looping Csis instances alive (wrapper object -> class): RcCars engines, the firecracker fuse.
    pub loops: std::collections::HashMap<u32, String>,
    /// Frame counter for periodic development logs.
    pub dbg_frames: u64,
    pub dbg_throws: Vec<(u32, u32, i32)>,
    /// Debug: objects whose constructor `EAGL_DBG_CT` names (their `this`), dumped with `EAGL_DBG_CTWORDS`.
    pub dbg_objs: Vec<u32>,
    /// Reused `Ren::SceneContext` handed to `Draw`.
    pub scene_ctx: u32,
    /// `aip_call`'s guest scratch: the query string (0x2000 bytes) and the `AIP::CmdComposer` (0x100).
    pub aip_scratch: u32,
}

/// Particle-effect requests of a frame (`PartFxManager` / `PartFx`), keyed by the GUID the game holds.
#[derive(Clone, Debug)]
pub enum FxEvent {
    Spawn { id: u32, name: String, pos: [f32; 3] },
    Move { id: u32, pos: [f32; 3] },
    /// Stop emitting; the effect is removed after `delay` seconds (or when its particles die).
    Stop { id: u32, delay: f32 },
}

/// One `EAGL::DrawTextured` batch (Begin .. End) the game drew this frame: indicators, cursors, rings.
#[derive(Clone, Debug, Default)]
pub struct ImmDraw {
    /// (texture bank path, shape name, shape index in the bank) of the `EAGL::TAR` set with `SetTexture`.
    pub tex: Option<(String, String, usize)>,
    /// `SetModelMatrix` (engine row-vector convention, translation at 12..14).
    pub model: [f32; 16],
    /// GX primitive: 0x80 quads, 0x90 triangles, 0x98 strip, 0xa0 fan.
    pub prim: u32,
    /// Position, RGBA, texture coordinate (as passed; see `uv_scale`).
    pub verts: Vec<([f32; 3], [u8; 4], [f32; 2])>,
}

#[derive(Default)]
pub struct DrawTexturedState {
    pub tex: u32,
    pub model: [f32; 16],
    pub prim: u32,
    pub verts: Vec<([f32; 3], [u8; 4], [f32; 2])>,
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
];

/// The front end's handler broker runs natively (`aip_call`): `AIP::Broker`, its trees, the command composer /
/// decomposer and the registration / text-conversion functions; the rest of AIP (the Apt player) stays a service.
fn is_aip_broker(name: &str) -> bool {
    ["Q23AIP6Broker", "Q23AIP11CmdComposer", "Q23AIP13CmdDecomposer", "BinaryTree", "_unnamed_broker_cpp_"].iter().any(|c| name.contains(c))
        || ["RegisterLVHandler__3AIPF", "UnregisterLVHandler__3AIPF", "RegisterFSHandler__3AIPF", "UnregisterFSHandler__3AIPF", "ConvertUCS2TOUTF8__3AIPF", "ConvertUTF8TOUCS2__3AIPF", "HexDigitToInt__3AIPF", "AptAllocHelper__3AIPF"].iter().any(|p| name.starts_with(p))
}

/// Services that are rendering / audio / effects only: when nothing hooks them they do nothing and return 0 (logged).
pub fn is_soft_stub(name: &str) -> bool {
    if NATIVE_EXCEPTIONS.contains(&name) || is_csis_setter(name) || is_aip_broker(name) {
        return false;
    }
    if class_of(name).is_none() && ["DC", "IC", "L2", "LC", "OS", "DVD", "VI", "GX", "PAD", "WPAD", "KPAD", "AX", "AI", "EXI", "SI", "IPC", "NAND", "CARD", "WENC", "TRC"].iter().any(|p| name.starts_with(p) && name[p.len()..].chars().next().map_or(false, |c| c.is_ascii_uppercase() || c == '_')) {
        return true;
    }
    match class_of(name) {
        Some(c) => {
            let q = qualified(c);
q.starts_with("Physics") || q.ends_with("RenderEntity") || q.starts_with("Ren::") || q.starts_with("AIP::") || q.starts_with("nw4") || q.starts_with("EAGL::Model") || q.starts_with("EAGL::Draw") || q.starts_with("EAGL::Device") || q.starts_with("EAGL::RenderContext") || q.starts_with("EAGL::Geo") || q.starts_with("EAGLInternal::RenderContext") || q.starts_with("Csis::") || ["AIP", "AreaManager", "PhysicsManager", "PhysicsRigidBody", "PhysicsDynamicCharacter", "PhysicsStaticCharacter", "Audio", "AuAEMSManager", "AuCharacterSoundObject", "AuEnvironmentManager", "AuHelpers", "PartFxManager", "PartFx", "FEManager", "TRC"].contains(&q.as_str())
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
const NATIVE_EXCEPTIONS: &[&str] = &[
    // game AI's random numbers; the "AI" prefix rule above is meant for the SDK audio interface (`AIInit`, ...)
    "AIRand__Fii",
    // plain data records the game fills for physics listeners ({vtable, type, value, pointer, material})
    "__ct__15PhysicsUserDataFv", "__ct__15PhysicsUserDataFQ215PhysicsUserData4TypePv", "__dt__15PhysicsUserDataFv", "GetMaterialFromID__15PhysicsUserDataF4MGID",
    // the sound-id -> Csis class switches; `Csis::Class::CreateInstance` is hooked to hear what they pick
    "PlaySFX__13AuAEMSManagerF14AUDIOAEMSBESFXii", "PlaySFX__13AuAEMSManagerF17AUDIOAEMSFEHUDSFXii",
    // looping sounds (engines, fuse): their instances are tracked through `Csis::Class::CreateInstance` and the class dtor
    "StartSFX__13AuAEMSManagerF14AUDIOAEMSBESFXii", "UpdateSFX__13AuAEMSManagerF14AUDIOAEMSBESFXiii", "StopSFX__13AuAEMSManagerF14AUDIOAEMSBESFX",
    "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", "CalcRenderingPosUp__11AreaManagerFRC9rmVector3R9rmVector3R9rmVector3", "PAD_getdataptr", "SetNewOverride__Q24EAGL6DeviceFPFUlPCc_PvPFUlPCc_Pv", "SetDeleteOverride__Q24EAGL6DeviceFPFPvUl_v",
    // the world's fade-to-colour effect is a timer the minigame exit waits on (`IsMinigameFadeEffectComplete`)
    "__ct__Q23Ren18FadeToColourEffectFv", "SetFadeColour__Q23Ren18FadeToColourEffectFRC9rmVector3", "StartFadeIn__Q23Ren18FadeToColourEffectFi",
    "StartFadeOut__Q23Ren18FadeToColourEffectFi", "Update__Q23Ren18FadeToColourEffectFi", "IsFinished__Q23Ren18FadeToColourEffectFv",
    "Start__Q23Ren11LinearBlendFffi", "GetCurrentValue__Q23Ren11LinearBlendFv",
    // the World HUD's loaded flag (FEManager + 0x130)
    "WaitForWorldHud__9FEManagerFv", "ClearWaitForWorldHud__9FEManagerFv", "SetWorldHudLoaded__9FEManagerFv", "IsWorldHudLoaded__9FEManagerCFv",
    "IsWorldHudLoadedAfterDiscEject__9FEManagerCFv"];

/// The Csis instance setters only clamp and store a field (`SetSpeed`, `SetAzimuth`, ...); looping sounds are read back.
fn is_csis_setter(name: &str) -> bool {
    name.starts_with("Set") && name.contains("__Q24Csis")
}

pub fn runs_natively(name: &str) -> bool {
    if NATIVE_EXCEPTIONS.contains(&name) {
        return true;
    }
    if name.starts_with("__sinit_") {
        return true;
    }
    if is_csis_setter(name) {
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
    /// Arguments decoded from the mangled signature (ints / floats in declaration order, C strings read through).
    pub args: Vec<FeArg>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FeArg {
    Int(i32),
    Float(f32),
    Str(String),
}

pub type MgVm = Vm<MgHost>;

pub fn boot() -> Result<(MgVm, MgHost), String> {
    boot_as(std::env::var("EAGL_MG_KID").ok().and_then(|v| v.parse().ok()).unwrap_or(0))
}

/// `boot` with the player's kid (index into the character list, as the profile stores it).
pub fn boot_as(kid: u32) -> Result<(MgVm, MgHost), String> {
    let mut vm = MgVm::load()?;
    // EAGL_PPC_COVER=path: count entries into every function, boot included (written by `write_coverage`)
    if std::env::var("EAGL_PPC_COVER").is_ok() {
        vm.enable_coverage();
    }
    vm.trap_unless(runs_natively);
    hooks::install(&mut vm);
    physhooks::install(&mut vm);
    hooks::install_stubs(&mut vm);
    let mut host = MgHost { fov: 0.8, aspect: 16. / 9., player_kid: kid, ..MgHost::default() };
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
        // 2 v 2
        4 => {
            out.push((8, 0, 1));
            out.push((0x18, 2, 6));
            out.push((0x48, 1, if humans > 1 { 2 } else { 6 }));
            out.push((0x58, 3, 6));
        }
        // four racers
        1 => {
            out.push((8, 0, 1));
            out.push((0x48, 1, if humans > 1 { 2 } else { 6 }));
            out.push((0x18, 2, 6));
            out.push((0x28, 3, 6));
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
    host.placeables_at_launch = snapshot::placeable_flags(vm, host);
    if ty == WORLD {
        // no minigame: the playground itself, player 1 on the Wii Remote; the world's own conversations start the games
        host.world_mode = true;
        host.pads[0].active = true;
        return Ok(());
    }
    let wm = world::WORLD_MAN;
    let ids = vm.img.addr("MinigameIDs").ok_or("MinigameIDs")?;
    let mgid = vm.r32(ids + 4 * ty as u32);
    let mg_id = vm.alloc_zeroed(8, 8);
    vm.w32(mg_id, mgid);
    let teams = vm.alloc_zeroed(0x100, 8);
    let kids = world::kid_keys(vm, host)?;
    host.kid_keys = kids.clone();
    // EAGL_MG_ALLAI: computer players only (testing aid).  Tetherball keeps its normal human seat and lets the game's own
    // debug override (`gTetherballAIOverride`) play it, since its exit expects the playground character to stay the player.
    let ai_override = std::env::var("EAGL_MG_ALLAI").is_ok() && ty == 2;
    let all_ai = std::env::var("EAGL_MG_ALLAI").is_ok() && !ai_override;
    // Teams +0: number of human players (games hand the first that many characters to controllers)
    vm.w32(teams, if all_ai { 0 } else { humans.max(1) as u32 });
    for (off, slot, control) in team_layout(ty, humans) {
        let control = if all_ai { 6 } else { control };
        // kid 0 is the playground player's own character, which keeps local control: all-AI runs skip it
        let key = kids[(slot + all_ai as usize) % kids.len()];
        vm.w32(teams + off, (key >> 32) as u32);
        vm.w32(teams + off + 4, key as u32);
        vm.w32(teams + off + 8, control);
    }
    for i in 0..humans.min(4) {
        host.pads[i].active = true;
    }
    if ai_override {
        if let Some(a) = vm.img.addr("gTetherballAIOverride") {
            vm.st.mem.w8(a, 1);
            vm.st.mem.w8(a + 1, 1);
        }
    }
    // StartMinigame(MGID, level, difficulty, teams, dare)
    let level = std::env::var("EAGL_MG_LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    vm.call_by_name(host, "StartMinigame__8WorldManF4MGIDiQ25Enums23MiniGameDifficultyLevelRC5TeamsPCi", &[wm, mg_id, level, 1, teams, 0], &[])?;
    vm.call_by_name(host, "StartMinigameFadeComplete__8WorldManFv", &[wm], &[])?;
    Ok(())
}

/// What the front end chose for a game it launches: the kids (indices into `character_select/characterlist`, one per
/// human player), the team-select answer for team games and the five rule values.
#[derive(Clone, Debug, Default)]
pub struct FeLaunch {
    pub ty: i32,
    pub avatars: Vec<i32>,
    pub teams: Vec<i32>,
    pub rules: Option<[i32; 5]>,
}

/// The front end's own sMinigameType (`MultiPlayerFSHandlers`, SDA -0x4d9c; another unit has a static of the same name).
const FE_MINIGAME_TYPE: u32 = 0x805f_f144;

/// Launch from the front end the way the original does: the multiplayer handlers build the Teams from the player count
/// and avatars (`LaunchNonTeamMiniGame`, or `SetTeams` with `aiTeams` for team games: humans get controls 2..5, computer
/// kids fill the rest at random), `MultiplayerMode::SetupMultiplayerGame` / `SetRules` store them and
/// `GameState::STATEFN_UPDATE_FE2MP` starts the game with `StartMinigame(MP+4, 0, 0, MP+8, MP+0xec)`.
pub fn launch_fe(vm: &mut MgVm, host: &mut MgHost, l: &FeLaunch) -> Result<(), String> {
    host.minigame_type = l.ty;
    host.placeables_at_launch = snapshot::placeable_flags(vm, host);
    host.kid_keys = world::kid_keys(vm, host)?;
    // the original comes here from the front end with no playground loaded; our boot spawned the area's kids, whose
    // asset bundles would fill CharacterManager's 16 slots
    let pw = vm.r32(world::WORLD_MAN + 0x88);
    if pw != 0 {
        vm.call_by_name(host, "UnSpawnCurrentArea__15PlaygroundWorldFv", &[pw], &[])?;
    }
    let n = l.avatars.len().clamp(1, 4);
    let info = vm.img.addr("sMultiPlayerTeamInfo").ok_or("sMultiPlayerTeamInfo")?;
    let avatars = vm.img.addr("sPlayerAvatarIds").ok_or("sPlayerAvatarIds")?;
    vm.w32(info, n as u32);
    for i in 0..4u32 {
        let a = l.avatars.get(i as usize).copied().unwrap_or(i as i32);
        vm.w32(avatars + 4 * i, a as u32);
    }
    vm.w32(FE_MINIGAME_TYPE, l.ty as u32);
    if l.teams.is_empty() {
        vm.call_by_name(host, "LaunchNonTeamMiniGame__21MultiPlayerFSHandlersFv", &[], &[])?;
    } else {
        // `GetIntArrayByName("aiTeams")` is answered from `host.int_array` (see hooks)
        host.int_array = l.teams.clone();
        let decomposer = vm.alloc_zeroed(0x20, 8);
        vm.call_by_name(host, "SetTeams__21MultiPlayerFSHandlersFRCQ23AIP13CmdDecomposer", &[0, decomposer], &[])?;
    }
    // SetTeams closes the team-select screen itself; the front end has already moved on
    host.events.retain(|e| e.name != "FEManager::CloseAptScreen");
    let mp = vm.r32(vm.img.addr("mInstance__15MultiplayerMode").ok_or("MultiplayerMode")?);
    // the game's rules: the front end's, or the database defaults (`MultiPlayerFSHandlers::RetrieveDefaultRules` fills
    // sDefaultRules[5] for sMinigameType) - all-zero rules leave e.g. Dodgeball with two-kid teams and no balls
    let rules = match l.rules {
        Some(r) => r,
        None => {
            vm.call_by_name(host, "RetrieveDefaultRules__21MultiPlayerFSHandlersFv", &[], &[])?;
            let d = vm.img.addr("sDefaultRules").ok_or("sDefaultRules")?;
            std::array::from_fn(|i| vm.r32(d + 4 * i as u32) as i32)
        }
    };
    let p = vm.alloc_zeroed(0x20, 8);
    for (i, v) in rules.iter().enumerate() {
        vm.w32(p + 4 * i as u32, *v as u32);
    }
    vm.call_by_name(host, "SetRules__15MultiplayerModeFPCi", &[mp, p], &[])?;
    if std::env::var("EAGL_MG_LOG").is_ok() {
        eprintln!("rules {rules:?}");
    }
    for i in 0..4 {
        host.pads[i].active = i < n;
    }
    if std::env::var("EAGL_MG_LOG").is_ok() {
        let words: Vec<String> = (0..0x22).map(|i| format!("{:x}", vm.r32(mp + 8 + 4 * i))).collect();
        eprintln!("MultiplayerMode teams: {}", words.join(" "));
        eprintln!("kid keys {:x?}", host.kid_keys);
    }
    let mgid = vm.alloc_zeroed(8, 8);
    let id = vm.r32(mp + 4);
    vm.w32(mgid, id);
    vm.call_by_name(host, "StartMinigame__8WorldManF4MGIDiQ25Enums23MiniGameDifficultyLevelRC5TeamsPCi", &[world::WORLD_MAN, mgid, 0, 0, mp + 8, mp + 0xec], &[])?;
    vm.st.mem.w8(mp + 1, 0);
    vm.call_by_name(host, "StartMinigameFadeComplete__8WorldManFv", &[world::WORLD_MAN], &[])?;
    Ok(())
}

/// Call a per-game callback (`OnHudLoadComplete`, `OnGameStartAnimComplete`, `OnPauseContinue`, `OnPauseReset`, ...) of the running
/// minigame; games without that method are skipped.
pub fn game_callback(vm: &mut MgVm, host: &mut MgHost, name: &str) -> Result<bool, String> {
    let Some((_, class)) = world::MINIGAME_CLASSES.iter().find(|(t, _)| *t == host.minigame_type) else { return Ok(false) };
    let sym = format!("{name}__{}{}Fv", class.len(), class);
    let mg = vm.r32(world::WORLD_MAN + 0x90);
    if mg == 0 || vm.img.addr(&sym).is_none() {
        return Ok(false);
    }
    vm.call_by_name(host, &sym, &[mg], &[])?;
    Ok(true)
}

/// One game frame: pads -> guest, `WorldMan::Update(ms)`, then the minigame's draw pass.
/// `launch` type for the playground itself (world play, single player).
pub const WORLD: i32 = 99;

/// The type of the minigame the world is running (`WorldMan + 0x90`, by its vtable), or `WORLD`.
pub fn running_minigame(vm: &mut MgVm) -> i32 {
    let mg = vm.r32(world::WORLD_MAN + 0x90);
    if mg == 0 {
        return WORLD;
    }
    let vt = vm.r32(mg);
    world::MINIGAME_CLASSES.iter().find(|(_, c)| vm.img.addr(&format!("__vt__{}{}", c.len(), c)) == Some(vt)).map_or(WORLD, |(t, _)| *t)
}

pub fn frame(vm: &mut MgVm, host: &mut MgHost, ms: i32) -> Result<(), String> {
    // the guest renders flat and the host bends (`gDisableCurvedWorld`, set at boot); RcCars / Paper Airplanes clear it on exit
    vm.st.mem.w8(world::CURVED_WORLD_OFF, 1);
    if host.world_mode {
        let ty = running_minigame(vm);
        if ty != host.minigame_type {
            host.log.push(format!("world: minigame {} -> {ty}", host.minigame_type));
            if ty != WORLD {
                host.placeables_at_launch = snapshot::placeable_flags(vm, host);
            }
            host.minigame_type = ty;
        }
    }
    // GameState::Update -> STATEFN_UPDATE_Playground order: pads, controllers, conga, AI, world, cameras
    hooks::write_pads(vm, &host.pads);
    if std::env::var("EAGL_DBG_DRAWS").is_ok() {
        host.dbg_frames += 1;
        if host.dbg_frames % std::env::var("EAGL_DBG_DRAWS_EVERY").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(30) == 0 {
            let k = std::env::var("EAGL_DBG_DRAWS").unwrap_or_default();
            let cam = snapshot::snapshot(vm, host).camera.map(|c| c.0);
            let d: Vec<(String, [f32; 3])> = host.last_draws.iter().filter(|d| d.0.contains(&k)).map(|d| (d.0.clone(), [d.1[12], d.1[13], d.1[14]])).collect();
            let c = vm.call_by_name(host, "Get__10ControllerFi", &[0], &[]).unwrap_or(0);
            let acc = if c != 0 { [vm.st.mem.rf32(c + 0x20), vm.st.mem.rf32(c + 0x24), vm.st.mem.rf32(c + 0x28)] } else { [0.; 3] };
            eprintln!("[draws] {} camera {cam:?} acc {acc:?} {d:?}", host.dbg_frames);
        }
    }
    if std::env::var("EAGL_DBG_RCCAM").is_ok() {
        host.dbg_frames += 1;
        let mg = vm.r32(snapshot::WORLD_MAN + 0x90);
        if mg != 0 && host.dbg_frames % 60 == 0 {
            let snap = snapshot::snapshot(vm, host);
            let car = snap.draws.iter().find(|d| d.0.contains("body")).map(|d| [d.1[12], d.1[13], d.1[14]]);
            eprintln!("[rccam] {} camera {:?} first body {car:?}", host.dbg_frames, snap.camera);
        }
    }
    for i in 0..4u32 {
        if host.pads[i as usize].active {
            let c = vm.call_by_name(host, "Get__10ControllerFi", &[i], &[])?;
            if c != 0 {
                vm.call_by_name(host, "Update__10ControllerFi", &[c, ms as u32], &[])?;
            }
        }
    }
    // the world's own buttons: sticker book (+, event 0xaf), report card (-, 0xb1), camera keys (GameState::Update order)
    if host.world_mode && host.minigame_type == WORLD {
        vm.call_by_name(host, "HandleActions__9GameStateFi", &[ms as u32], &[])?;
    }
    let ae = vm.r32(0x8060_12ac);
    if ae != 0 {
        vm.call_by_name(host, "Update__11AncientEvilFi", &[ae, ms as u32], &[])?;
    }
    // the renderer's FullScreenEffectsManager (an opaque stand-in here) would advance the playground's fades:
    // the intro / area fade (PlaygroundWorld + 0x8b0) and the minigame fade (+ 0xff0)
    let pw = vm.r32(world::WORLD_MAN + 0x88);
    if pw != 0 {
        for fade in [0x8b0, 0xff0] {
            vm.call_by_name(host, "Update__Q23Ren18FadeToColourEffectFi", &[pw + fade, ms as u32], &[])?;
        }
    }
    vm.call_by_name(host, "Update__8WorldManFi", &[world::WORLD_MAN, ms as u32], &[])?;
    physhooks::dispatch_contacts(host, vm)?;
    let cm = vm.r32(0x8060_214c);
    if cm != 0 {
        vm.call_by_name(host, "Update__13CameraManagerFi", &[cm, ms as u32], &[])?;
    }
    host.draws.clear();
    host.imm.clear();
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
    // EAGL_MG_FE="avatars[;teams]" (e.g. "0,3;1,2"): launch through the front end's multiplayer handlers instead
    let launched = match std::env::var("EAGL_MG_FE") {
        Ok(spec) => {
            let mut parts = spec.split(';');
            let list = |s: Option<&str>| s.unwrap_or("").split(',').filter_map(|v| v.trim().parse().ok()).collect::<Vec<i32>>();
            let fe = FeLaunch { ty, avatars: list(parts.next()), teams: list(parts.next()), rules: None };
            launch_fe(&mut vm, &mut host, &fe)
        }
        // type 99: no minigame - the playground itself (world play), walking with the Nunchuk (`EAGL_MG_STICK="from-to:x,y;.."`)
        Err(_) if ty == WORLD => {
            host.pads[0].stick = std::env::var("EAGL_MG_STICK").ok().map(|_| [0, 0]);
            launch(&mut vm, &mut host, ty, 1)
        }
        Err(_) => launch(&mut vm, &mut host, ty, 1),
    };
    if let Err(e) = launched {
        println!("launch failed: {e}");
        return;
    }
    if std::env::var("EAGL_MG_FE").is_ok() {
        let mp = vm.r32(vm.img.addr("mInstance__15MultiplayerMode").unwrap_or(0));
        let words: Vec<String> = (0..0x24).map(|i| format!("{:x}", vm.r32(mp + 8 + 4 * i))).collect();
        println!("MultiplayerMode teams: {}", words.join(" "));
    }
    println!("launched {ty}");
    if std::env::var("EAGL_DBG_RCTAB").is_ok() {
        for t in 0..6u32 {
            let e = 0x805e3380 + t * 0x34;
            let ent: Vec<u32> = (0..13).map(|i| vm.r32(e + 4 * i)).collect();
            println!("rccar table {t}: {ent:x?}");
        }
    }
    if std::env::var("EAGL_MG_SELFTEST").is_ok() {
        for _ in 0..8 {
            let r = vm.call_by_name(&mut host, "AIRand__Fii", &[100, 750], &[]);
            let j = vm.call_by_name(&mut host, "ChooseRandomJuggleTiming__14FootieAIEntityCFv", &[vm.heap], &[]);
            println!("AIRand(100,750) = {:?} juggle timing {:?}", r.map(|v| v as i32), j);
        }
        if let Some(a) = vm.img.addr("EAGLMalloc__12EAGLInternal") {
            let f = vm.r32(a);
            let r1 = vm.call(&mut host, f, &[0x5c, 0], &[]);
            let r2 = vm.call(&mut host, f, &[0x5c, 0], &[]);
            println!("EAGLMalloc -> {} : {:x?} {:x?}", vm.name_of(f), r1, r2);
        }
        for x in [4.0f64, 2.0, 0.25, 1.5, 100.0, 0.0001] {
            let r = vm.call_by_name(&mut host, "sqrt", &[], &[x]);
            println!("sqrt({x}) = {:?} f1={}", r, vm.st.cpu.f[1]);
        }
    }
    let frames: i32 = std::env::var("EAGL_MG_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let mut mg = vm.r32(world::WORLD_MAN + 0x90);
    // EAGL_MG_POSTGAME=replay|done: press that post-game button 30 frames after the screen opens
    let mut postgame_at: Option<i32> = None;
    let mut conv_at: Option<i32> = None;
    let mut popup_at: Option<i32> = None;
    let mut play_at: Option<i32> = None;
    let mut sticker_at: Option<i32> = None;
    let mut boss_at: Option<i32> = None;
    for f in 0..frames {
        let now = vm.r32(world::WORLD_MAN + 0x90);
        if now != mg {
            println!("  [{f}] minigame object {mg:#x} -> {now:#x}");
            mg = now;
        }
        if postgame_at == Some(f) {
            let sym = if std::env::var("EAGL_MG_POSTGAME").as_deref() == Ok("replay") { "OnReplay__8MinigameFv" } else { "OnDone__8MinigameFv" };
            println!("  [{f}] -> {sym}: {:?}", vm.call_by_name(&mut host, sym, &[mg], &[]));
        }
        if f == 10 && mg != 0 {
            let r = vm.call_by_name(&mut host, "OnPlay__8MinigameFv", &[mg], &[]);
            println!("OnPlay {r:?}");
        }
        let mut bits = 0u16;
        for part in std::env::var("EAGL_MG_PADS").unwrap_or_default().split(',') {
            // "from-to:hexbits"
            if let Some((range, b)) = part.split_once(':') {
                if let Some((a, z)) = range.split_once('-') {
                    if let (Ok(a), Ok(z), Ok(b)) = (a.parse::<i32>(), z.parse::<i32>(), u16::from_str_radix(b.trim_start_matches("0x"), 16)) {
                        if f >= a && f < z {
                            bits |= b;
                        }
                    }
                }
            }
        }
        host.pads[0].buttons = bits;
        host.pointer = std::env::var("EAGL_MG_POINTER").ok().and_then(|v| { let mut it = v.split(',').filter_map(|t| t.parse::<f32>().ok()); Some([it.next()?, it.next()?]) });
        host.pads[0].acc = [512, 512, 616];
        for part in std::env::var("EAGL_MG_ACC").unwrap_or_default().split(';') {
            // "from-to:x,y,z" (raw WPAD accelerometer values)
            if let Some((range, v)) = part.split_once(':') {
                if let Some((a, z)) = range.split_once('-') {
                    if let (Ok(a), Ok(z)) = (a.parse::<i32>(), z.parse::<i32>()) {
                        let v: Vec<i16> = v.split(',').filter_map(|t| t.parse().ok()).collect();
                        if f >= a && f < z && v.len() == 3 {
                            host.pads[0].acc = [v[0], v[1], v[2]];
                        }
                    }
                }
            }
        }
        if host.pads[0].stick.is_some() {
            let mut st = [0i8; 2];
            for part in std::env::var("EAGL_MG_STICK").unwrap_or_default().split(';') {
                if let Some((range, v)) = part.split_once(':') {
                    if let Some((a, z)) = range.split_once('-') {
                        if let (Ok(a), Ok(z)) = (a.parse::<i32>(), z.parse::<i32>()) {
                            let v: Vec<i8> = v.split(',').filter_map(|t| t.parse().ok()).collect();
                            if f >= a && f < z && v.len() == 2 {
                                st = [v[0], v[1]];
                            }
                        }
                    }
                }
            }
            host.pads[0].stick = Some(st);
            if std::env::var("EAGL_DBG_WORLD").is_ok() && f % 50 == 0 {
                let wm = vm.r32(0x805e_83ac);
                let cm = if wm != 0 { vm.r32(wm + 0x18) } else { 0 };
                let c = vm.call_by_name(&mut host, "Get__10ControllerFi", &[0], &[]).unwrap_or(0);
                println!("  world f{f}: wm {wm:#x} paused {} control {:#x} stick {:#x},{:#x} state {}", if wm != 0 { vm.st.mem.r8(wm + 0x24) } else { 0 }, if cm != 0 { vm.r32(cm + 0xc0) } else { 0 }, vm.st.mem.r8(c + 0x250), vm.st.mem.r8(c + 0x251), vm.call_by_name(&mut host, "GetCurrentControllerState__10ControllerCFv", &[c], &[]).unwrap_or(0));
            }
        }
        if std::env::var("EAGL_DBG_CTL").is_ok() && f % 50 == 0 {
            let c = vm.call_by_name(&mut host, "Get__10ControllerFi", &[0], &[]).unwrap_or(0);
            let sp = vm.r32(c + 0x244);
            let rows = vm.r32(c + 0xca6c);
            let st: Vec<u32> = (0..4).map(|i| vm.r32(c + 0x144 + 4 * i)).collect();
            let rows_state: Vec<(u32, u32, u32)> = (0..rows.min(400)).map(|i| { let r = c + 100 * i; (vm.r32(r + 0x2b0), vm.r32(r + 0x2cc), vm.r32(r + 0x26c)) }).take(60).collect();
            let pw = vm.r32(world::WORLD_MAN + 0x88);
            if pw != 0 {
                println!("  pw {pw:#x} hide_area {} gRenderWorld {} state {:#x} fade {} / {} paused {} sp-state {}", host.hide_area_model, vm.st.mem.r8(0x805f_fb80), vm.r32(pw + 0x44), vm.r32(pw + 0x8b0 + 0x1c), vm.r32(pw + 0x8b0 + 0x20), vm.st.mem.r8(pw + 0x24), { let a = vm.img.addr("mInstance__16CharacterProfile").unwrap_or(0); let cp = vm.r32(a); vm.r32(cp + 0x54) });
            }
            println!("  ctl f{f}: sp {sp} stack {st:x?} rows {rows} held {:#x} port {} state31 rows {rows_state:x?}", vm.r32(c + 0x264), vm.r32(c + 0x254));
        }
        // EAGL_DBG_EV="6c,63": controller 0 action events (hex ids) whenever one is active
        if let Ok(spec) = std::env::var("EAGL_DBG_EV") {
            let c = vm.call_by_name(&mut host, "Get__10ControllerFi", &[0], &[]).unwrap_or(0);
            let tbl = vm.r32(c + 0x268);
            for id in spec.split(',').filter_map(|v| u32::from_str_radix(v, 16).ok()) {
                if vm.st.mem.r8(tbl + 8 * id) != 0 {
                    println!("  [{f}] event {id:#x} active");
                }
            }
        }
        if f >= 1100 && f < 1105 {
            let c = vm.call_by_name(&mut host, "Get__10ControllerFi", &[0], &[]).unwrap_or(0);
            let tbl = vm.r32(c + 0x268);
            let ev = |vm: &mut MgVm, id: u32| (vm.r32(tbl + 8 * id), vm.r32(tbl + 8 * id + 4));
            let st = vm.call_by_name(&mut host, "GetCurrentControllerState__10ControllerCFv", &[c], &[]).unwrap_or(0);
            println!("  f{f}: ctrl {c:#x} state {st} ev62 {:?} ev63 {:?} ev66 {:?}", ev(&mut vm, 0x62), ev(&mut vm, 0x63), ev(&mut vm, 0x66));
        }
        if f % 300 == 299 && std::env::var("EAGL_DBG_CAMOBJ").is_ok() && host.camera != 0 {
            let c = host.camera;
            let fl: Vec<String> = (0..24).map(|i| format!("{:.2}", vm.st.mem.rf32(c + 0x8 + 4 * i))).collect();
            println!("  cam {:x}: {}", c, fl.join(" "));
        }
        if f % 30 == 29 && std::env::var("EAGL_DBG_HEAP").is_ok() {
            println!("  heap {:#x} / {:#x}", vm.heap, vm.heap_end);
        }
        let period: i32 = std::env::var("EAGL_DBG_PERIOD").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
        let from: i32 = std::env::var("EAGL_DBG_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        if f >= from && f % period == period - 1 {
                if let Ok(spec) = std::env::var("EAGL_DBG_CTWORDS") {
                    for &obj in &host.dbg_objs.clone() {
                        let mut out = vec![];
                        for path in spec.split(',') {
                            let mut v = obj;
                            for o in path.split('/') {
                                let o = u32::from_str_radix(o, 16).unwrap_or(0);
                                v = if v != 0 { vm.r32(v + o) } else { 0 };
                            }
                            out.push(format!("{path}={v:#x}"));
                        }
                        println!("  o{f} {obj:#x}: {}", out.join(" "));
                    }
                }
        }
        if f % 100 == 99 {
            // EAGL_DBG_WORDS="250,214,120/18/54": minigame fields (hex offsets; a/b/c follows pointers)
            if let Ok(spec) = std::env::var("EAGL_DBG_WORDS") {
                let mut out = vec![];
                for path in spec.split(',') {
                    let mut a = mg;
                    let mut v = 0;
                    for (i, o) in path.split('/').enumerate() {
                        let o = u32::from_str_radix(o, 16).unwrap_or(0);
                        if i > 0 {
                            a = v;
                        }
                        v = if a != 0 { vm.r32(a + o) } else { 0 };
                    }
                    out.push(format!("{path}={v:#x}"));
                }
                println!("  w{f}: {}", out.join(" "));
            }
            let sn = snapshot::snapshot(&mut vm, &mut host);
            println!("  t{f}: p0 {:?} anim {}", sn.chars.first().map(|c| c.pos), sn.chars.first().map(|c| c.anim_state).unwrap_or(0));
            if std::env::var("EAGL_DBG_SYNC").is_ok() {
                for c in &sn.chars {
                    let pc = host.phys.chars.values().find(|p| p.guest_character == c.ptr).map(|p| p.position);
                    println!("     char {:#x} guest {:?} phys {:?}", c.ptr, c.pos, pc);
                }
            }
        }
        if let Err(e) = frame(&mut vm, &mut host, 33) {
            println!("frame {f}: {e}");
            break;
        }
        let mut follow = std::mem::take(&mut host.dbg_throws);
        for (body, target, left) in follow.iter_mut() {
            let bp = host.phys.body_pos(*body);
            let tp = vm.call_by_name(&mut host, "GetPos__18DodgeballCharacterCFv", &[*target], &[]).unwrap_or(0);
            let t = [vm.st.mem.rf32(tp), vm.st.mem.rf32(tp + 4), vm.st.mem.rf32(tp + 8)];
            let d = ((bp[0] - t[0]).powi(2) + (bp[2] - t[2]).powi(2)).sqrt();
            let _ = vm.call_by_name(&mut host, "GetPersonalSpace__18DodgeballCharacterCFv", &[*target], &[]);
            let ps = vm.st.cpu.f[1];
            print!("  ps {ps:.2}");
            println!("  [{f}] ball {body:#x} at ({:.2},{:.2},{:.2}) target {target:#x} at ({:.2},{:.2},{:.2}) dxz {d:.2}", bp[0], bp[1], bp[2], t[0], t[1], t[2]);
            *left -= 1;
        }
        follow.retain(|x| x.2 > 0);
        host.dbg_throws.extend(follow);
        if let Some(m) = host.music.take() {
            println!("  [{f}] music {m:?}");
        }
        for e in std::mem::take(&mut host.fx) {
            if std::env::var("EAGL_MG_FX").is_ok() {
                println!("  [{f}] fx {e:?}");
            }
        }
        let sounds = std::mem::take(&mut host.sounds);
        if std::env::var("EAGL_MG_SOUNDS").is_ok() {
            for (c, v) in &sounds {
                println!("  [{f}] sound {c} {v}");
            }
        }
        // EAGL_MG_TP="frame:x,z": put the world's player there (`CharacterState` position + teleport flag, as
        // `ConversationManager::ExitConversation` does); the frame before, list the kids with a visible beacon
        if let Some((at, xz)) = std::env::var("EAGL_MG_TP").ok().and_then(|v| v.split_once(':').map(|(a, b)| (a.to_string(), b.to_string()))) {
            let at: i32 = at.parse().unwrap_or(-1);
            if f == at - 1 {
                let snap = snapshot::snapshot(&mut vm, &mut host);
                for c in &snap.chars {
                    let ind = vm.r32(c.ptr + 0x228);
                    let beacon = ind != 0 && vm.st.mem.r8(ind + 0x14) != 0;
                    let w: Vec<u32> = if ind != 0 { (0..8).map(|k| vm.r32(ind + 4 * k)).collect() } else { vec![] };
                    println!("  [{f}] char {:#x} key {:x} pos {:?} angle {} beacon {beacon} ind {w:x?}", c.ptr, c.key, c.pos, c.angle);
                }
            }
            if f == at {
                let v: Vec<f32> = xz.split(',').filter_map(|t| t.parse().ok()).collect();
                debug_teleport(&mut vm, &mut host, &v);
            }
        }
        if boss_at == Some(f) {
            boss_at = None;
            let game = std::env::var("EAGL_MG_BOSS").unwrap_or_else(|_| "3".into());
            println!("  [{f}] EndGame_OnLoad -> {:?}", aip_call(&mut vm, &mut host, "EndGame_OnLoad", ""));
            println!("  [{f}] EndGame_OnStickerSelect({game}) -> {:?}", aip_call(&mut vm, &mut host, "EndGame_OnStickerSelect", &format!("iMinigame={game}")));
        }
        if sticker_at == Some(f) {
            sticker_at = None;
            for (n, p) in [("StickerBook_RewardLoad", ""), ("StickerBook_RewardGetStickers", ""), ("StickerBook_LayoutLoad", "iMinigameType=1"),
                ("StickerBook_LayoutSave", "iNumStickers=1&aiStickerID=0&aiStickerPositionX=300&aiStickerPositionY=200&aiStickerOrientation=0&aiStickerTypes=0&iMinigameType=1"),
                ("StickerBook_Exit", "iSelected=0")] {
                let r = aip_call(&mut vm, &mut host, n, p);
                println!("  [{f}] {n} -> {}", format!("{r:?}").chars().take(200).collect::<String>());
            }
        }
        if play_at == Some(f) {
            play_at = None;
            println!("  [{f}] PreGame_OnPlay {:?}", aip_call(&mut vm, &mut host, "PreGame_OnPlay", ""));
        }
        // EAGL_DBG_MICRO: where the world's "press A" microgames start (Bug Hunt manager + 0x30, Dribbling + 0x10)
        if std::env::var("EAGL_DBG_MICRO").is_ok() && f % 100 == 0 {
            let pw = vm.r32(world::WORLD_MAN + 0x88);
            if pw != 0 {
                let (bh, dr) = (vm.r32(pw + 0xfb4), vm.r32(pw + 0xfbc));
                let v = |vm: &mut MgVm, a: u32| [vm.st.mem.rf32(a), vm.st.mem.rf32(a + 4), vm.st.mem.rf32(a + 8)];
                println!("  [{f}] bug hunt at {:?} active {} | dribbling at {:?} running {}", v(&mut vm, bh + 0x30), vm.st.mem.r8(bh + 0x95), v(&mut vm, dr + 0x10), vm.r32(dr + 0xe0));
            }
        }
        // EAGL_MG_AREA="frame:gate": go through that playground gate (PlaygroundWorld::StartAreaTransition), listing the
        // gates (from area, target area, unlocked) the frame before
        if let Some((at, gate)) = std::env::var("EAGL_MG_AREA").ok().and_then(|v| v.split_once(':').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<u32>().ok()?)))) {
            let pw = vm.r32(world::WORLD_MAN + 0x88);
            if pw != 0 && f == at - 1 {
                println!("  [{f}] current area {} stickers {}", vm.r32(pw + 0x5c), { let a = vm.img.addr("mInstance__16CharacterProfile").unwrap_or(0); let cp = vm.r32(a); vm.call_by_name(&mut host, "GetTotalStickerCount__16CharacterProfileFb", &[cp, 1], &[]).unwrap_or(0) });
                for i in 0..12u32 {
                    let g = pw + 0x70 + 0xb0 * i;
                    let to = vm.call_by_name(&mut host, "GetTargetArea__14PlaygroundGateFv", &[g], &[]).unwrap_or(0) as i32;
                    let words: Vec<u32> = (0..8).map(|k| vm.r32(g + 4 * k)).collect();
                    println!("  [{f}] gate {i}: from {} to {to} words {words:x?}", vm.r32(g) as i32);
                }
            }
            if f == at {
                println!("  [{f}] area transition: {:?}", debug_gate(&mut vm, &mut host, gate));
            }
        }
        // stand in for the World HUD's info dialogue: show it, then press its button
        if popup_at == Some(f) {
            popup_at = None;
            let text = aip_call(&mut vm, &mut host, "InfoDialogue_GetText", "");
            println!("  [{f}] info dialogue {text:?}");
            println!("  [{f}] close {:?}", aip_call(&mut vm, &mut host, "InfoDialogue_OnButtonClick", ""));
        }
        // stand in for the conversation screen: show each node, pick response `EAGL_MG_CONV` (default 0)
        if conv_at == Some(f) {
            conv_at = None;
            // through the game's own handler broker, as the conversation screen asks
            let q = |vm: &mut MgVm, host: &mut MgHost, n: &str, p: &[(String, String)]| {
                let params: Vec<String> = p.iter().map(|(k, v)| format!("{k}={v}")).collect();
                aip_call(vm, host, n, &params.join("&")).unwrap_or_default()
            };
            let name = q(&mut vm, &mut host, "Conversation_GetName", &[]);
            let text = q(&mut vm, &mut host, "Conversation_GetDialogueText", &[]);
            let resp = q(&mut vm, &mut host, "Conversation_GetResponses", &[]);
            println!("  [{f}] conversation {name:?} {text:?} {resp:?}");
            // EAGL_MG_CONV="text~n,..": pick response n at a node whose dialogue contains text (default 0)
            let said = text.first().map(|(_, t)| t.clone()).unwrap_or_default();
            let pick = std::env::var("EAGL_MG_CONV").unwrap_or_default().split(',').filter_map(|r| r.split_once('~')).find(|(t, _)| said.contains(t)).and_then(|(_, n)| n.trim().parse().ok()).unwrap_or(0u32);
            let next = q(&mut vm, &mut host, "Conversation_OnPlayerSelect", &[("iIndexSelected".to_string(), pick.to_string())]);
            println!("  [{f}] conversation pick {pick} -> {next:?}");
            if next.first().is_some_and(|(_, v)| v == "1") {
                conv_at = Some(f + 20);
            }
        }
        let evs: Vec<FeEvent> = host.events.drain(..).collect();
        for e in &evs {
            println!("  [{f}] {} {:?}", e.name, e.args);
            // stand in for the front end: the HUD "loads" at once and the start animation plays instantly
            let cb = match (e.name.as_str(), e.args.first()) {
                ("FEManager::OpenAptScreen" | "FEManager::ReplaceAptScreen", Some(FeArg::Str(n))) if n.ends_with("Hud") && n != "WorldHud" => Some("OnHudLoadComplete"),
                ("Apt::GameStartAnim_Play", _) => Some("OnGameStartAnimComplete"),
                _ => None,
            };
            if e.name == "FEManager::ReplaceAptScreen" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "SelectPlane") {
                let mg = vm.r32(world::WORLD_MAN + 0x90);
                println!("  [{f}] -> OnPlaneSelected: {:?}", vm.call_by_name(&mut host, "OnPlaneSelected__16MGPaperAirplanesFi", &[mg, 0], &[]));
            }
            if e.name == "FEManager::OpenAptScreen" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "PostGame") && std::env::var("EAGL_MG_POSTGAME").is_ok() {
                postgame_at = Some(f + 30);
            }
            if e.name == "FEManager::OpenAptOverlay" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "Conversation") {
                conv_at = Some(f + 20);
            }
            if e.name == "Apt::InfoDialogue_SetVisible" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "1") {
                popup_at = Some(f + 20);
            }
            // the sticker award: place the first reward sticker and go back to the world (StickerBookGame's own calls)
            if host.world_mode && e.name == "FEManager::OpenAptScreen" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "StickerBookGame") {
                sticker_at = Some(f + 20);
            }
            // the gauntlet's game select: pick the boss game `EAGL_MG_BOSS` (minigame type, default 3 Dodgeball)
            if host.world_mode && e.name == "FEManager::OpenAptScreen" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "BossGameSelect") {
                boss_at = Some(f + 20);
            }
            // the world's own pre-game screen: PLAY through the game's handlers
            if host.world_mode && e.name == "FEManager::OpenAptScreen" && matches!(e.args.first(), Some(FeArg::Str(n)) if n == "PreGameInstructions") {
                play_at = Some(f + 20);
            }
            if let Some(cb) = cb {
                println!("  [{f}] -> {cb}: {:?}", game_callback(&mut vm, &mut host, cb));
            }
        }
    }
    write_coverage(&vm);
    let snap = snapshot::snapshot(&mut vm, &mut host);
    println!("placeables changed: {:?}", snap.placeables);
    println!("snapshot: {} chars, camera {:?}", snap.chars.len(), snap.camera);
    for c in snap.chars.iter().take(8) {
        println!("  char key {:x} pos {:?} angle {} anim {} bones {}", c.key, c.pos, c.angle, c.anim_state, c.pose.len());
        if std::env::var("EAGL_DBG_POSE").is_ok() {
            for (i, b) in c.pose.iter().enumerate().take(24) {
                println!("     bone {i} q {:?}", b.0);
            }
        }
    }
    if std::env::var("EAGL_DBG_IMM").is_ok() {
        println!("  tar managers {:?}", host.tar_managers);
        for (m, _) in host.tar_managers.clone() {
            println!("    mgr {m:#x} count {} first {:#x} {}", vm.r32(m + 0x4200), vm.r32(m), vm.st.mem.cstr(m + 4, 64));
        }
        let am = vm.r32(vm.img.addr("sAssetManagerInstance__12AssetManager").unwrap_or(0));
        for (_, path) in host.tar_managers.clone() {
            let name = vm.alloc_cstr(&path);
            let d = vm.call_by_name(&mut host, "CheckForTexture__12AssetManagerFPCcPiPi", &[am, name, 0, 0], &[]).unwrap_or(0);
            let w: Vec<String> = (0..6).map(|i| format!("{:08x}", vm.r32(d + 4 * i))).collect();
            println!("    asset {path}: data {d:#x} {}", w.join(" "));
        }
        for (k, v) in &host.tar_banks {
            println!("    bank {k}: {} bytes {:?}", v.len(), &v[..8]);
            if let Ok(dir) = std::env::var("EAGL_DUMP_BANKS") {
                let _ = std::fs::write(std::path::Path::new(&dir).join(k.replace('/', "_")), v.as_slice());
            }
        }
        for (_, path) in &host.tar_managers {
            println!("    vfs {path}: {:?}", host.vfs.read(path).map(|d| (d.len(), d[..16.min(d.len())].to_vec())));
        }
        println!("  draw_textured {:?}", host.draw_textured.iter().map(|(k, v)| (*k, v.tex)).collect::<Vec<_>>());
        for d in &snap.imm {
            println!("  imm {:?} prim {:#x} verts {} first {:?} model t {:?}", d.tex, d.prim, d.verts.len(), d.verts.first(), &d.model[12..15]);
        }
    }
    if let Some(a) = vm.img.addr("sCameraManagerInstance__13CameraManager") {
        let cm = vm.r32(a);
        let cam = if cm != 0 { vm.r32(cm) } else { 0 };
        if cam != 0 {
            let v = |vm: &mut Vm<MgHost>, o: u32| [vm.st.mem.rf32(cam + o), vm.st.mem.rf32(cam + o + 4), vm.st.mem.rf32(cam + o + 8)];
            println!("guest camera {cam:#x} (host {:#x}) pos {:?} target {:?} base {:?} {:?}", host.camera, v(&mut vm, 0x70), v(&mut vm, 0x80), v(&mut vm, 0x10), v(&mut vm, 0x20));
        }
    }
    println!("loops {:?}", snap.loops);
    if let Some(a) = vm.img.addr("gDisableCurvedWorld") {
        println!("gDisableCurvedWorld {}", vm.st.mem.r8(a));
        if let Some(r) = vm.img.addr("gRenderWorld") {
            println!("gRenderWorld {}", vm.st.mem.r8(r));
        }
    }
    if std::env::var("EAGL_DBG_RCMODELS").is_ok() {
        // RcCar's CachedModel table (13 per vehicle type: bodies by paint, wheels, ...)
        for i in 0..39u32 {
            let cm = vm.r32(0x805e_3394 + 4 * i);
            let m = if cm != 0 { vm.r32(cm + 0x44) } else { 0 };
            println!("rc slot {i}: cached {cm:#x} model {m:#x} {:?}", host.model_names.get(&m));
        }
    }
    if let Ok(k) = std::env::var("EAGL_DBG_DRAW") {
        for (n, m) in snap.draws.iter().filter(|d| d.0.contains(&k)) {
            println!("draw {n} {m:?}");
        }
    }
    println!("draws {:?}", snap.draws.iter().map(|d| (d.0.clone(), [d.1[12], d.1[13], d.1[14]])).collect::<Vec<_>>());
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

/// Development aid (`EAGL_MG_TP`): put the world's player at `[x, z]` (`CharacterState` position + teleport flag, as
/// `ConversationManager::ExitConversation` does), with `[x, z, fx, fz]` also facing the point (fx, fz).
pub fn debug_teleport(vm: &mut MgVm, host: &mut MgHost, v: &[f32]) {
    let p = vm.call_by_name(host, "GetPlayerCharacter__8WorldManFi", &[world::WORLD_MAN, 0], &[]).unwrap_or(0);
    if p == 0 || v.len() < 2 {
        return;
    }
    vm.st.mem.wf32(p + 0x180, v[0]);
    vm.st.mem.wf32(p + 0x188, v[1]);
    vm.st.mem.w8(p + 0x190, 1);
    if v.len() == 4 {
        let a = (v[2] - v[0]).atan2(v[3] - v[1]);
        vm.call_by_name(host, "Set__7rmAngleFf", &[p + 0x1b0], &[a as f64]).ok();
        vm.call_by_name(host, "AsDir__7rmAngleCFv", &[p + 0x1a0, p + 0x1b0], &[]).ok();
    }
}

/// Ask the game's own front-end handlers (registered with its `AIP::Broker`): `name?params` goes to the LoadVariables
/// handler of that name (`Broker::LoadVariables`, the reply decoded from the composer's `name=value&..` text with its
/// `%25 %26 %3D %2B` escapes; arrays keep their 0x7f delimiters), else to the FS command handler (`Broker::FSCommand`).
/// `None`: no handler of that name.
pub fn aip_call(vm: &mut MgVm, host: &mut MgHost, name: &str, params: &str) -> Option<Vec<(String, String)>> {
    let broker = vm.img.addr("s_pBroker__3AIP").map(|a| vm.r32(a)).unwrap_or(0);
    if broker == 0 {
        return None;
    }
    if host.aip_scratch == 0 {
        host.aip_scratch = vm.alloc_zeroed(0x2100, 8);
    }
    let (q, composer) = (host.aip_scratch, host.aip_scratch + 0x2000);
    let text = if params.is_empty() { name.to_string() } else { format!("{name}?{params}") };
    let bytes = text.as_bytes();
    let n = bytes.len().min(0x1fff);
    for (i, b) in bytes[..n].iter().enumerate() {
        vm.st.mem.w8(q + i as u32, *b);
    }
    vm.st.mem.w8(q + n as u32, 0);
    vm.call_by_name(host, "__ct__Q23AIP11CmdComposerFii", &[composer, 0x1000, 0x200], &[]).ok()?;
    let reply = vm.call_by_name(host, "LoadVariables__Q23AIP6BrokerFPCcRQ23AIP11CmdComposer", &[broker, q, composer], &[]).ok();
    let out = match reply {
        Some(p) if p != 0 => {
            let raw: Vec<u8> = (0..0x10000).map(|i| vm.st.mem.r8(p + i)).take_while(|&b| b != 0).collect();
            Some(decode_lv(&raw))
        }
        _ => None,
    };
    vm.call_by_name(host, "__dt__Q23AIP11CmdComposerFv", &[composer, u32::MAX], &[]).ok();
    if out.is_some() {
        return out;
    }
    match vm.call_by_name(host, "FSCommand__Q23AIP6BrokerFPCc", &[broker, q], &[]) {
        Ok(found) if found != 0 => Some(vec![]),
        _ => None,
    }
}

/// `a=1&b=x%26y` -> [(a, 1), (b, x&y)] (UTF-8 text).
fn decode_lv(raw: &[u8]) -> Vec<(String, String)> {
    let unescape = |s: &[u8]| {
        let mut o = Vec::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            if s[i] == b'%' && i + 2 < s.len() {
                if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&s[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    o.push(v);
                    i += 3;
                    continue;
                }
            }
            o.push(s[i]);
            i += 1;
        }
        String::from_utf8_lossy(&o).into_owned()
    };
    raw.split(|&b| b == b'&').filter(|p| !p.is_empty()).map(|p| match p.iter().position(|&b| b == b'=') {
        Some(e) => (unescape(&p[..e]), unescape(&p[e + 1..])),
        None => (unescape(p), String::new()),
    }).collect()
}

/// Development aid (`EAGL_MG_AREA="frame:gate"`): go through playground gate `gate` (`PlaygroundWorld::StartAreaTransition`
/// from its area to its target). Returns (from, to).
pub fn debug_gate(vm: &mut MgVm, host: &mut MgHost, gate: u32) -> Result<(u32, u32), String> {
    let pw = vm.r32(world::WORLD_MAN + 0x88);
    if pw == 0 || gate > 11 {
        return Err("no world / gate".into());
    }
    let g = pw + 0x70 + 0xb0 * gate;
    let from = vm.r32(g);
    let to = vm.call_by_name(host, "GetTargetArea__14PlaygroundGateFv", &[g], &[])?;
    vm.call_by_name(host, "StartAreaTransition__15PlaygroundWorldFQ25Enums8AreaTypeQ25Enums8AreaTypei", &[pw, from, to, gate], &[])?;
    Ok((from, to))
}

/// `EAGL_PPC_COVER=path`: append `address	entries` for every function entered so far (decompilation progress map).
pub fn write_coverage(vm: &MgVm) {
    let Ok(path) = std::env::var("EAGL_PPC_COVER") else { return };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let mut v: Vec<(&u32, &u64)> = vm.covered.iter().collect();
        v.sort();
        for (a, n) in v {
            let _ = writeln!(f, "{a:08x}	{n}");
        }
    }
}

/// `mglab classify OUT`: what the VM does with every function of the executable once booted, one
/// `address	name	kind` line each: `host` (a Rust hook replaces it), `observe` (Rust runs first, then the original),
/// `stub` (does nothing, returns 0), `trap` (an unimplemented service), `native` (the original code runs).
pub fn classify(out: &str) -> Result<(), String> {
    let (vm, _host) = boot()?;
    let stub_fn = hooks::stub_fn();
    let table: std::collections::HashMap<u32, (String, &'static str)> = vm.hook_table().into_iter().map(|(a, n, k)| {
        let kind = match k {
            crate::gekko::HookKind::Host(f) if f as usize == stub_fn as usize => "stub",
            crate::gekko::HookKind::Host(_) => "host",
            crate::gekko::HookKind::Observe(_) => "observe",
            crate::gekko::HookKind::Trap => "trap",
        };
        (a, (n, kind))
    }).collect();
    let mut text = String::new();
    for &i in &vm.img.funcs {
        let s = &vm.img.symbols[i];
        let kind = table.get(&s.addr).map(|t| t.1).unwrap_or("native");
        text.push_str(&format!("{:08x}	{}	{kind}
", s.addr, s.name));
    }
    std::fs::write(out, text).map_err(|e| e.to_string())
}
