//! Boot of the engine objects the minigames expect: the Playground world with its managers, the player characters and
//! the Ren scene globals.
use super::{MgHost, MgVm};

pub const WORLD_MAN: u32 = 0x805e_8320;
/// `Ren::Scene* s3dScene` and `Ren::FullScreenEffectsManager::sFSEffectInstance` globals.
pub const S3D_SCENE: u32 = 0x8060_1ffc;
pub const FS_EFFECTS: u32 = 0x8060_2194;

/// Run the executable's static constructors (`.ctors`).  Services that are not available yet are skipped (logged).
pub fn run_static_constructors(vm: &mut MgVm, host: &mut MgHost) {
    let (start, end) = (0x8041cee0u32, 0x8041cee0u32 + 0x874);
    vm.soft_traps = true;
    let mut a = start;
    while a < end {
        let f = vm.st.mem.r32(a);
        a += 4;
        if f != 0 {
            if let Err(e) = vm.call(host, f, &[], &[]) {
                host.log.push(format!("static constructor {}: {e}", vm.name_of(f)));
            }
        }
    }
    vm.soft_traps = false;
}

/// Every `Ren::*` singleton pointer (`sXInstance__Q23Ren..`) gets an opaque zeroed object: the renderer is not emulated,
/// the game only passes these around.
fn stub_ren_singletons(vm: &mut MgVm) {
    let list: Vec<(u32, u32)> = vm.img.symbols.iter().filter(|s| s.name.contains("Instance__Q23Ren") && !s.is_func && s.size == 4).map(|s| (s.addr, s.size)).collect();
    for (addr, _) in list {
        if vm.st.mem.r32(addr) == 0 {
            let o = vm.alloc_zeroed(0x1000, 32);
            vm.w32(addr, o);
        }
    }
}

pub fn boot(vm: &mut MgVm, host: &mut MgHost) -> Result<(), String> {
    run_static_constructors(vm, host);
    stub_ren_singletons(vm);
    // opaque engine objects the game only passes around
    let scene = vm.alloc_zeroed(0x2000, 32);
    vm.w32(S3D_SCENE, scene);
    let fx = vm.alloc_zeroed(0x400, 32);
    vm.w32(FS_EFFECTS, fx);
    vm.call_by_name(host, "InitPool__7CStringFv", &[], &[])?;
    // the allocator callbacks `Ren::Engine::Initialize` installs for EAGL
    vm.call_by_name(host, "SetNewOverride__Q24EAGL6DeviceFPFUlPCc_PvPFUlPCc_Pv", &[0x803b_eff8, 0], &[])?;
    vm.call_by_name(host, "SetDeleteOverride__Q24EAGL6DeviceFPFPvUl_v", &[0x803b_f010], &[])?;
    vm.call_by_name(host, "InitInternal__Q28EAGLAnim11InitializerFUib", &[0x8_0000, 1], &[])?;
    vm.call_by_name(host, "SHAPE_setmemcallbacks", &[0x803b_f014, 0x803b_f030, 0x803b_f05c], &[])?;
    vm.call_by_name(host, "Create__4pgIOFv", &[], &[])?;
    vm.call_by_name(host, "Create__12AssetManagerFv", &[], &[])?;
    vm.call_by_name(host, "Create__11pgIDatabaseFv", &[], &[])?;
    vm.call_by_name(host, "InitialiseDatabase__11pgIDatabaseFv", &[], &[])?;
    // the nine MGIDs are database keys: rebuild them now that the database exists
    vm.call_by_name(host, r"__sinit_\conversationmanager_cpp", &[], &[])?;
    // GameState::Init
    let csvs = vm.alloc_cstr("data/csvs.viv");
    let pgio = vm.r32(0x8060_22ac);
    vm.call_by_name(host, "AddBigFile__4pgIOFPCc", &[pgio, csvs], &[])?;
    vm.call_by_name(host, "Create__10ControllerF11ControlType", &[0], &[])?;
    vm.call_by_name(host, "Create__7PGCongaFv", &[], &[])?;
    vm.call_by_name(host, "Create__16CharacterProfileFv", &[], &[])?;
    // the kid the player picked (index into the character list)
    let profile = vm.r32(0x8060_2064);
    let data = vm.r32(profile + 4);
    vm.w32(data + 0x10, 0);
    vm.call_by_name(host, "Create__15MultiplayerModeFv", &[], &[])?;
    // WorldMan::Initialize(scene), then (characters)
    vm.call_by_name(host, "Create__11AncientEvilFv", &[], &[])?;
    vm.call_by_name(host, "Initialize__8WorldManFQ28WorldMan13WorldInitType", &[WORLD_MAN, 0], &[])?;
    vm.call_by_name(host, "Create__13CameraManagerFv", &[], &[])?;
    let camera_manager = vm.r32(0x8060_214c);
    vm.call_by_name(host, "Initialize__13CameraManagerF10CameraTypeUi", &[camera_manager, 4, 4], &[])?;
    vm.call_by_name(host, "ReInitialize__13CameraManagerF10CameraTypeUi", &[camera_manager, 2, 0], &[])?;
    vm.call_by_name(host, "Initialize__8WorldManFQ28WorldMan13WorldInitType", &[WORLD_MAN, 1], &[])?;
    Ok(())
}

/// Database keys of the eight selectable kids (`character_select/character.characterlist`).
pub fn kid_keys(vm: &mut MgVm, host: &mut MgHost) -> Result<Vec<u64>, String> {
    let db = vm.r32(0x8060_18d4);
    let mut key = |vm: &mut MgVm, host: &mut MgHost, name: &str| -> Result<u64, String> {
        let s = vm.alloc_cstr(name);
        let hi = vm.call_by_name(host, "GetKey__11pgIDatabaseFPCc", &[db, s], &[])?;
        Ok(((hi as u64) << 32) | vm.ret_hi_lo as u64)
    };
    let (k1, k2) = (key(vm, host, "character_select")?, key(vm, host, "character")?);
    let coll = vm.call_by_name(host, "GetCollection__11pgIDatabaseFUxUx", &[db, 0, (k1 >> 32) as u32, k1 as u32, (k2 >> 32) as u32, k2 as u32], &[])?;
    let list = vm.alloc_cstr("characterlist");
    let mut out = vec![];
    for i in 0..8 {
        let s = vm.call_by_name(host, "GetArrayString__14pgDBCollectionFPCci", &[coll, list, i], &[])?;
        let name = vm.st.mem.cstr(s, 64);
        out.push(key(vm, host, &name)?);
    }
    Ok(out)
}

pub const MINIGAME_CLASSES: [(i32, &str); 8] = [(0, "MGDartShootout"), (1, "MGRcCars"), (2, "MGTetherball"), (3, "MGDodgeball"), (4, "MGFootie"), (5, "MGPaperAirplanes"), (6, "MGWallball"), (8, "MGFreeThrow")];

/// `Minigame::Draw(SceneContext&)` of the running game: fills `host.draws`.
pub fn draw_minigame(vm: &mut MgVm, host: &mut MgHost, ty: i32) -> Result<(), String> {
    let Some((_, class)) = MINIGAME_CLASSES.iter().find(|(t, _)| *t == ty) else { return Ok(()) };
    let mg = vm.r32(WORLD_MAN + 0x90);
    if mg == 0 {
        return Ok(());
    }
    let name = format!("Draw__{}{}FRQ23Ren12SceneContext", class.len(), class);
    let ctx = vm.alloc_zeroed(0x100, 16);
    vm.call_by_name(host, &name, &[mg, ctx], &[]).map(|_| ())
}
