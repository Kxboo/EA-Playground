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
    // render space == world space: the host applies the world's curvature when presenting
    vm.st.mem.w8(CURVED_WORLD_OFF, 1);
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
    // the front end's LoadVariables / FS command broker (the `AIP::Initialize` subset handler registration needs), so the
    // handler singletons below register natively and `aip_call` can ask them
    init_aip(vm, host)?;
    // front-end handler singletons the games talk to (FEManager::InitializeAip creates them)
    for name in ["Create__15PreGameHandlersFv", "Create__16MinigameHandlersFv", "Create__16WorldHudHandlersFv", "Create__16PostGameHandlersFv", "Create__17PauseMenuHandlersFv", "Create__21PaperAirplaneHandlersFv", "Create__18EndTourneyHandlersFv", "Create__19MultiPlayerHandlersFv", "Create__20ConversationHandlersFv", "Create__19StickerBookHandlersFv", "Create__20StickerStoreHandlersFv", "Create__18ReportCardHandlersFv", "Create__19BossEndGameHandlersFv"] {
        vm.call_by_name(host, name, &[], &[])?;
    }
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
    vm.w32(data + 0x10, host.player_kid);
    // EAGL_MG_STICKERS=N: N Golden Stickers already won (testing aid: area gates unlock by sticker count)
    if let Some(n) = std::env::var("EAGL_MG_STICKERS").ok().and_then(|v| v.parse::<u32>().ok()) {
        for i in 0..9u32 {
            vm.w32(data + 0x668 + 0x84 * i, n / 9 + (i < n % 9) as u32);
        }
    }
    vm.call_by_name(host, "Create__15MultiplayerModeFv", &[], &[])?;
    // WorldMan::Initialize(scene), then (characters)
    vm.call_by_name(host, "Create__11AncientEvilFv", &[], &[])?;
    vm.call_by_name(host, "Initialize__8WorldManFQ28WorldMan13WorldInitType", &[WORLD_MAN, 0], &[])?;
    vm.call_by_name(host, "Create__13CameraManagerFv", &[], &[])?;
    let camera_manager = vm.r32(0x8060_214c);
    vm.call_by_name(host, "Initialize__13CameraManagerF10CameraTypeUi", &[camera_manager, 4, 4], &[])?;
    vm.call_by_name(host, "ReInitialize__13CameraManagerF10CameraTypeUi", &[camera_manager, 2, 0], &[])?;
    vm.call_by_name(host, "Initialize__8WorldManFQ28WorldMan13WorldInitType", &[WORLD_MAN, 1], &[])?;
    // AreaManager is a null service, so build the placeable list (positions of the games' props) ourselves
    let world = vm.r32(WORLD_MAN + 0x88);
    let areas = vm.r32(world + 8);
    if areas != 0 && vm.r32(areas + 0x1b0 + 0xc) == 0 {
        vm.call_by_name(host, "Initialize__16PlaceableManagerFv", &[areas + 0x1b0], &[])?;
    }
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
    if host.world_mode && ty == super::WORLD {
        return draw_world(vm, host);
    }
    let Some((_, class)) = MINIGAME_CLASSES.iter().find(|(t, _)| *t == ty) else { return Ok(()) };
    let mg = vm.r32(WORLD_MAN + 0x90);
    if mg == 0 {
        return Ok(());
    }
    let name = format!("Draw__{}{}FRQ23Ren12SceneContext", class.len(), class);
    let ctx = scene_ctx(vm, host);
    vm.call_by_name(host, &name, &[mg, ctx], &[]).map(|_| ())
}

/// The playground's own draws while no minigame runs: `PlaygroundWorld::Draw` (Bug Hunt bugs, the dribbling course)
/// and every kid's beacon (`NpcIndicator::Draw`, `Character + 0x228`, immediate-mode quads).
fn draw_world(vm: &mut MgVm, host: &mut MgHost) -> Result<(), String> {
    let pw = vm.r32(WORLD_MAN + 0x88);
    if pw == 0 {
        return Ok(());
    }
    let ctx = scene_ctx(vm, host);
    vm.call_by_name(host, "Draw__15PlaygroundWorldFRQ23Ren12SceneContext", &[pw, ctx], &[])?;
    let cm = vm.r32(pw + 0x18);
    if cm != 0 {
        let count = vm.r32(cm + 0x74).min(64);
        for i in 0..count {
            let c = vm.r32(cm + 4 * i);
            let ind = if c != 0 { vm.r32(c + 0x228) } else { 0 };
            if ind != 0 {
                vm.call_by_name(host, "Draw__12NpcIndicatorFRQ23Ren12SceneContext", &[ind, ctx], &[])?;
            }
        }
    }
    Ok(())
}

/// The `Ren::SceneContext` handed to `Draw` functions (created once).
fn scene_ctx(vm: &mut MgVm, host: &mut MgHost) -> u32 {
    if host.scene_ctx == 0 {
        // views list (+0, with a frustum at +0x18 and a count at +0x2e8) the games cull against; the view index at +0x10 is -1 (all)
        let ctx = vm.alloc_zeroed(0x100, 16);
        let views = vm.alloc_zeroed(0x300, 16);
        let frustum = vm.alloc_zeroed(0x80, 16);
        vm.w32(views + 0x18, frustum);
        vm.w32(views + 0x2e8, 1);
        vm.w32(ctx, views);
        vm.w32(ctx + 0x10, u32::MAX);
        host.scene_ctx = ctx;
    }
    host.scene_ctx
}

/// `AIP::Initialize` with `FEManager::SetupInitStruct`'s values, minus the Apt player: allocator and debug callbacks,
/// composer buffer sizes, the 0x7f array delimiters and the handler broker.
fn init_aip(vm: &mut MgVm, host: &mut MgHost) -> Result<(), String> {
    let sym = |vm: &MgVm, n: &str| vm.img.addr(n).ok_or_else(|| format!("missing {n}"));
    let alloc = sym(vm, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc")?;
    let free = sym(vm, "Free__6MemMgrFPv")?;
    let quiet = sym(vm, "AptDbgShutup__3AIPFPCce")?;
    for (n, v) in [("g_pfnMemAlloc__3AIP", alloc), ("g_pfnMemFree__3AIP", free), ("g_pfnDebugPrint__3AIP", quiet), ("g_pfnAssert__3AIP", quiet), ("s_nComposerMainBufferBytes__3AIP", 0x1000), ("s_nComposerArrayBufferBytes__3AIP", 0x200)] {
        let a = sym(vm, n)?;
        vm.w32(a, v);
    }
    for n in ["g_nComposerArrayDelimiter__3AIP", "g_nDecomposerArrayDelimiter__3AIP"] {
        let a = sym(vm, n)?;
        vm.st.mem.w8(a, 0x7f);
    }
    let broker = vm.alloc_zeroed(0x10, 8);
    vm.call_by_name(host, "__ct__Q23AIP6BrokerFv", &[broker], &[])?;
    let a = sym(vm, "s_pBroker__3AIP")?;
    vm.w32(a, broker);
    let a = sym(vm, "s_isInitialized__3AIP")?;
    vm.st.mem.w8(a, 1);
    Ok(())
}

/// `gDisableCurvedWorld`.
pub const CURVED_WORLD_OFF: u32 = 0x8060_22ec;
