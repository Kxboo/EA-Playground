//! Engine-service hooks, grouped by subsystem.  Each hook reads its arguments from the guest registers (`vm.a(i)` /
//! `vm.fa(i)`) and returns like the original function would (`vm.ret`).
use super::MgHost;
use crate::gekko::Vm;

pub type R = Result<(), String>;
type V = Vm<MgHost>;

pub fn install(vm: &mut V) {
    memory(vm);
    world(vm);
    super::vfs::install(vm);
    assets(vm);
    front_end(vm);
    scene(vm);
    input(vm);
    drawing(vm);
    view(vm);
    fe_screens(vm);
    apt_calls(vm);
    pregame(vm);
}

fn bind(vm: &mut V, names: &[&str], f: fn(&mut MgHost, &mut V) -> R) {
    for n in names {
        if !vm.hook(n, f) {
            vm.log_missing_symbol(n);
        }
    }
}

// --- memory -----------------------------------------------------------------------------------------------------------

fn alloc_size(h: &mut MgHost, vm: &mut V) -> R {
    // power-of-two size classes so freed blocks can be reused (the games allocate and free every frame)
    let cap = vm.a(0).max(32).next_power_of_two();
    let p = match h.free_blocks.get_mut(&cap).and_then(|v| v.pop()) {
        Some(p) => {
            vm.st.mem.fill(p, cap as usize, 0);
            p
        }
        None => {
            let p = vm.alloc_zeroed(cap, 32);
            h.blocks.insert(p, cap);
            p
        }
    };
    vm.ret(p);
    Ok(())
}
fn mem_free(h: &mut MgHost, vm: &mut V) -> R {
    let p = vm.a(0);
    if let Some(&cap) = h.blocks.get(&p) {
        let list = h.free_blocks.entry(cap).or_default();
        if !list.contains(&p) {
            list.push(p);
        }
    }
    Ok(())
}
/// `MemMgr::Alloc(size, pool, type, name)`: size is the first argument.
fn mem_alloc(_h: &mut MgHost, vm: &mut V) -> R {
    alloc_size(_h, vm)
}
fn nothing(_h: &mut MgHost, _vm: &mut V) -> R {
    Ok(())
}

fn memory(vm: &mut V) {
    bind(
        vm,
        &[
            "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc",
            "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypeiiPCc",
            "__nw__FUl",
            "__nwa__FUl",
            "__nw__FUlPCcQ26MemMgr8PoolTypeQ26MemMgr9AllocType",
            "__nwa__FUlPCcQ26MemMgr8PoolTypeQ26MemMgr9AllocType",
            "malloc",
            "Alloc__Q24Csis6SystemFUl",
        ],
        mem_alloc,
    );
    bind(vm, &["Free__6MemMgrFPv", "__dl__FPv", "__dla__FPv", "free", "Free__Q24Csis6SystemFPv"], mem_free);
    bind(vm, &["MemFill__6MemMgrFPvUii"], nothing);
}

// --- world ------------------------------------------------------------------------------------------------------------

fn world(_vm: &mut V) {}

// --- null services ----------------------------------------------------------------------------------------------------

/// Unhooked rendering / audio service: logged once, returns 0.
fn stub(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.name_of(vm.st.cpu.pc);
    if h.stubbed.insert(name.clone()) {
        h.log.push(format!("stub {name}"));
    }
    // front-end handler calls are the HUD / screen interface: record them with their arguments
    if let Some(class) = super::class_of(&name) {
        if class.ends_with("Handlers") {
            let ints = [vm.a(1), vm.a(2), vm.a(3), vm.a(4)];
            let floats = [vm.fa(0), vm.fa(1)];
            let args = decode_args(vm, &name);
            h.events.push(super::FeEvent { name: name.clone(), ints, floats, args });
        }
    }
    // constructors return `this` and install the class's vtable so virtual calls reach the (stubbed or native) methods
    if let Some(rest) = name.strip_prefix("__ct__") {
        if let Some(class) = super::mangled_class(rest) {
            if let Some(vt) = vm.img.addr(&format!("__vt__{class}")) {
                let this = vm.a(0);
                if this != 0 {
                    vm.w32(this, vt);
                }
            }
        }
    } else {
        vm.ret(0);
    }
    vm.st.cpu.f[1] = 0.;
    Ok(())
}

/// The soft-stub hook (lets the progress map tell stubs from real host functions).
pub fn stub_fn() -> crate::gekko::HookFn<MgHost> {
    stub
}

pub fn install_stubs(vm: &mut V) {
    vm.hook_matching(super::is_soft_stub, stub);
}


// --- assets -----------------------------------------------------------------------------------------------------------

/// `AssetManager::ResolveModel(data, size, DynamicLoader** loader, Model** models, int* count)`: the renderer is not
/// emulated, so a model is an opaque placeholder object.
fn resolve_model(h: &mut MgHost, vm: &mut V) -> R {
    let (loader, models, count) = (vm.a(3), vm.a(4), vm.a(5));
    // the asset slot's name sits at +0xc of the 0xb8-byte entry that `models` (+0x90) points into
    let name = vm.st.mem.cstr(models - 0x90 + 0xc, 64);
    // RcCars' car files hold one model per paint job
    let n: u32 = if name.to_lowercase().starts_with("rc_") { 6 } else { 1 };
    if std::env::var("EAGL_DBG_MODELS").is_ok() {
        eprintln!("[model] ResolveModel {name} x{n}");
    }
    for k in 0..n {
        let dummy = vm.alloc_zeroed(0x200, 32);
        // +0x44 -> geometry record whose bounding box (+0x6c min, +0x7c max) some games read when drawing
        let geo = vm.alloc_zeroed(0x100, 32);
        vm.w32(dummy + 0x44, geo);
        vm.w32(dummy + 8, n); // number of sub-models the game loops over
        for (i, v) in [-0.5f32, 0., -1., 0.5, 0.5, 1.].iter().enumerate() {
            let o = if i < 3 { 0x6c + 4 * i as u32 } else { 0x7c + 4 * (i as u32 - 3) };
            vm.st.mem.wf32(geo + o, *v);
            vm.st.mem.wf32(dummy + o, *v);
        }
        h.model_names.insert(dummy, name.clone());
        vm.w32(models + 4 * k, dummy);
    }
    vm.w32(loader, 0);
    vm.w32(count, n);
    Ok(())
}

pub fn assets(vm: &mut V) {
    bind(vm, &["ResolveModel__12AssetManagerFPviPPQ24EAGL13DynamicLoaderPPQ24EAGL5ModelPi"], resolve_model);
}

// --- front end ---------------------------------------------------------------------------------------------------------

/// `FEManager::GetInstance()`: an opaque object; its methods are null services.
fn fe_instance(h: &mut MgHost, vm: &mut V) -> R {
    if h.fe_manager == 0 {
        h.fe_manager = vm.alloc_zeroed(0x400, 32);
        // FEManager+0x50 is the Locale (`SetCurrentLanguage` builds it from data/locale/ENG_US.loc)
        let locale = vm.alloc_zeroed(0x20, 32);
        vm.call_by_name(h, "__ct__6LocaleF9eLocaleDb11eLanguageId", &[locale, 0, 0], &[])?;
        let fe = h.fe_manager;
        vm.w32(fe + 0x50, locale);
        vm.w32(fe + 0x30, 0);
    }
    vm.ret(h.fe_manager);
    Ok(())
}
/// `FEManager::ConvertUTF8TOUCS2(wchar* dst, const char* src, int max)`
fn fe_utf8_to_ucs2(_h: &mut MgHost, vm: &mut V) -> R {
    let (dst, src, max) = (vm.a(1), vm.a(2), vm.a(3) as usize);
    let s = vm.st.mem.cstr(src, max.max(1) * 4);
    let mut n = 0;
    for (i, c) in s.encode_utf16().take(max.saturating_sub(1)).enumerate() {
        vm.st.mem.w16(dst + 2 * i as u32, c);
        n += 1;
    }
    vm.st.mem.w16(dst + 2 * n as u32, 0);
    vm.ret(n as u32);
    Ok(())
}

pub fn front_end(vm: &mut V) {
    bind(vm, &["GetInstance__9FEManagerFv"], fe_instance);
    bind(vm, &["ConvertUTF8TOUCS2__9FEManagerFPwPCci"], fe_utf8_to_ucs2);
}

// --- scene -------------------------------------------------------------------------------------------------------------

/// `Ren::Scene::GetSceneOptions(int)`: a static options block (viewport 0,0,640,480; the renderer is not emulated).
fn scene_options(h: &mut MgHost, vm: &mut V) -> R {
    if h.scene_options == 0 {
        let p = vm.alloc_zeroed(0x40, 16);
        for (i, v) in [0.0f32, 0.0, 0.0, 640.0, 480.0, 0.0, 1.0, 1.0, 0.0].iter().enumerate() {
            vm.st.mem.wf32(p + 4 + 4 * i as u32, *v);
        }
        h.scene_options = p;
    }
    vm.ret(h.scene_options);
    Ok(())
}

/// `Ren::Scene::SetViewPort(this, int, const SceneOptions&)`: keep the options (later `GetSceneOptions` return them) and
/// note the field of view (+0x1c, degrees across a 4:3 screen; `CalcWidescreenFOV` widens it for 16:9).
fn set_viewport(h: &mut MgHost, vm: &mut V) -> R {
    let opts = vm.a(2);
    if opts == 0 {
        return Ok(());
    }
    if h.scene_options == 0 {
        h.scene_options = vm.alloc_zeroed(0x40, 16);
    }
    for k in 0..12 {
        let w = vm.r32(opts + 4 * k);
        vm.w32(h.scene_options + 4 * k, w);
    }
    let fov = vm.st.mem.rf32(opts + 0x1c);
    let near = vm.st.mem.rf32(opts + 0x14);
    if near > 0.01 && near < 10. {
        h.near = Some(near);
    }
    if std::env::var("EAGL_DBG_SCENE").is_ok() {
        let w: Vec<f32> = (0..12).map(|k| vm.st.mem.rf32(opts + 4 * k)).collect();
        eprintln!("[scene] SetViewPort options {w:?}");
    }
    if fov > 1. && fov < 170. {
        h.fov_h43 = Some(fov);
        // vertical field of view (radians) for the pointer ray
        h.fov = 2. * ((fov.to_radians() * 0.5).tan() * 0.75).atan();
    }
    Ok(())
}

pub fn scene(vm: &mut V) {
    bind(vm, &["GetSceneOptions__Q23Ren5SceneCFi"], scene_options);
    bind(vm, &["SetViewPort__Q23Ren5SceneFiRCQ23Ren12SceneOptions"], set_viewport);
}

// --- input ------------------------------------------------------------------------------------------------------------

/// One Wii Remote as the executable's `_WiiPadStatus` sees it (`WPADStatus` bits, accelerometer centred at 512).
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    pub active: bool,
    /// WPAD button bits: LEFT 0x1, RIGHT 0x2, DOWN 0x4, UP 0x8, PLUS 0x10, TWO 0x100, ONE 0x200, B 0x400, A 0x800, MINUS 0x1000, Z 0x2000, C 0x4000, HOME 0x8000.
    pub buttons: u16,
    pub acc: [i16; 3],
    /// Nunchuk stick (x, y; -128..127) when the Nunchuk is plugged in (the playground walks with it).
    pub stick: Option<[i8; 2]>,
}

impl Default for Pad {
    fn default() -> Self {
        Pad { active: false, buttons: 0, acc: [512, 512, 616], stick: None }
    }
}

pub const PAD_STATUS: u32 = 0x805f_017c;

/// Write the host pad states into the guest's `PAD` array (what `PAD_update` would have done).
pub fn write_pads(vm: &mut V, pads: &[Pad; 4]) {
    for (i, p) in pads.iter().enumerate() {
        let a = PAD_STATUS + 0x60 * i as u32;
        vm.w32(a, if p.active { 2 } else { 0 });
        vm.st.mem.w16(a + 4, p.buttons);
        // `WPADStatus` acceleration is signed and centred (-512..511, about 100 counts per g); `Pad::acc` keeps the
        // raw 10-bit form (rest 512, 512, 616)
        for k in 0..3 {
            vm.st.mem.w16(a + 6 + 2 * k as u32, (p.acc[k] - 512) as u16);
        }
        for k in 0..16 {
            vm.st.mem.w16(a + 0xc + 2 * k, 1023);
        }
        vm.st.mem.w8(a + 0x2c, 0);
        vm.st.mem.w8(a + 0x2d, 0);
        // `CFreeStylePadModeHandler::Sample`: mode 3, extension flag +0x2c, Nunchuk acceleration +0x2e, stick +0x34 / +0x35
        if let (true, Some(st)) = (p.active, p.stick) {
            vm.w32(a, 3);
            vm.st.mem.w8(a + 0x2c, 1);
            for (k, v) in [0i16, 0, 104].iter().enumerate() {
                vm.st.mem.w16(a + 0x2e + 2 * k as u32, *v as u16);
            }
            vm.st.mem.w8(a + 0x34, st[0] as u8);
            vm.st.mem.w8(a + 0x35, st[1] as u8);
        }
    }
}

fn pad_active(h: &mut MgHost, vm: &mut V) -> R {
    let port = vm.a(0) as usize;
    vm.ret(h.pads.get(port).map(|p| p.active).unwrap_or(false) as u32);
    Ok(())
}

pub fn input(vm: &mut V) {
    bind(vm, &["PAD_active"], pad_active);
    bind(vm, &["PAD_update"], nothing);
}


// --- drawing / cameras --------------------------------------------------------------------------------------------------

/// `Ren::CachedModel::Draw(this, const rmMatrix4&, bool)`: remember what the game asked to draw.
fn cached_model_draw(h: &mut MgHost, vm: &mut V) -> R {
    let (this, m) = (vm.a(0), vm.a(1));
    if this == 0 || m == 0 {
        return Ok(());
    }
    let model = vm.r32(this + 0x44);
    // usually a CachedModel around a resolved model (+0x44); some tables (RcCars' paint jobs) hold the model itself
    let name = h.model_names.get(&model).or_else(|| h.model_names.get(&this)).cloned().unwrap_or_else(|| format!("model@{model:#x}"));
    let mut mat = [0f32; 16];
    for (i, v) in mat.iter_mut().enumerate() {
        *v = vm.st.mem.rf32(m + 4 * i as u32);
    }
    h.draws.push((name, mat));
    Ok(())
}
/// `Controller::GetWorldVectorFromDPDRotationallyCorrected(this, ViewPort*, Camera*, int, rmVector3* out)`: the direction
/// from the camera through the pointer; returns 0 when the remote is not pointing at the screen.
fn dpd_world_vector(h: &mut MgHost, vm: &mut V) -> R {
    let (cam, out) = (vm.a(2), vm.a(4));
    let Some(p) = h.pointer else {
        vm.ret(0);
        return Ok(());
    };
    let v = |vm: &mut V, o: u32| [vm.st.mem.rf32(cam + o), vm.st.mem.rf32(cam + o + 4), vm.st.mem.rf32(cam + o + 8)];
    // the final (transitioned, shaken) view the game aims from, `Camera::GetPos/GetTarget(true)`; the base pair before
    // the camera has updated
    let (mut pos, mut tgt) = (v(vm, 0x70), v(vm, 0x80));
    if pos == tgt {
        (pos, tgt) = (v(vm, 0x10), v(vm, 0x20));
    }
    if std::env::var("EAGL_DBG_DPD").is_ok() {
        eprintln!("dpd pos {pos:?} tgt {tgt:?} base {:?} {:?} p {p:?}", v(vm, 0x10), v(vm, 0x20));
    }
    let norm = |a: [f32; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-6);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let fwd = norm([tgt[0] - pos[0], tgt[1] - pos[1], tgt[2] - pos[2]]);
    let right = norm(cross(fwd, [0., 1., 0.]));
    let up = cross(right, fwd);
    let th = (h.fov * 0.5).tan();
    let (sx, sy) = (2. * p[0] * th * h.aspect, 2. * p[1] * th);
    let d = norm([fwd[0] + right[0] * sx + up[0] * sy, fwd[1] + right[1] * sx + up[1] * sy, fwd[2] + right[2] * sx + up[2] * sy]);
    for i in 0..3 {
        vm.st.mem.wf32(out + 4 * i as u32, d[i]);
    }
    vm.ret(1);
    Ok(())
}
fn camera_set_pos(h: &mut MgHost, vm: &mut V) -> R {
    h.camera = vm.a(0);
    Ok(())
}

fn dbg_reach(h: &mut MgHost, vm: &mut V) -> R {
    if h.log.len() < 400 {
        let p = vm.a(1);
        let (x, y, z) = (vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8));
        h.log.push(format!("IsBallReachable this={:x} pos=({x:.2},{y:.2},{z:.2}) i={}", vm.a(0), vm.a(2)));
    }
    Ok(())
}

fn dbg_move(h: &mut MgHost, vm: &mut V) -> R {
    let p = vm.a(1);
    if p != 0 {
        let (x, y, z) = (vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8));
        eprintln!("AddMove this={:x} dir=({x:.3},{y:.3},{z:.3}) f={}", vm.a(0), vm.fa(0));
    }
    Ok(())
}

fn dbg_getcoll(_h: &mut MgHost, vm: &mut V) -> R {
    let n = vm.st.mem.cstr(vm.a(1), 100);
    eprintln!("GetCollection {n} key {:x}{:08x}", vm.a(2), vm.a(3));
    Ok(())
}
fn dbg_getstring(_h: &mut MgHost, vm: &mut V) -> R {
    let n = vm.st.mem.cstr(vm.a(1), 100);
    eprintln!("GetString this={:x} {n}", vm.a(0));
    Ok(())
}

fn dbg_rccar(_h: &mut MgHost, vm: &mut V) -> R {
    let this = vm.a(0);
    let (t, v) = (vm.r32(this + 0x434), vm.r32(this + 0x438));
    let e = 0x805e3380 + t * 0x34;
    let ent: Vec<u32> = (0..13).map(|i| vm.r32(e + 4 * i)).collect();
    eprintln!("RcCar::Render type {t} variant {v} entry {ent:x?}");
    Ok(())
}

/// RcCars drives its chase camera through `SetBase`/`SetViewMatrix` instead of `SetPos`.
fn camera_set_base(h: &mut MgHost, vm: &mut V) -> R {
    if h.minigame_type == 1 {
        let v = |vm: &mut V, i: usize| {
            let p = vm.a(i);
            [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)]
        };
        let a = [v(vm, 1), v(vm, 2), v(vm, 3)];
        if std::env::var("EAGL_DBG_CAM").is_ok() {
            eprintln!("SetBase {:x} {:?}", vm.a(0), a);
        }
        if a[0] != [0.; 3] {
            h.camera = vm.a(0);
            h.view = Some(a);
        }
    }
    Ok(())
}

pub fn drawing(vm: &mut V) {
    bind(vm, &["GetWorldVectorFromDPDRotationallyCorrected__10ControllerFPQ24EAGL8ViewPortP6CameraiP9rmVector3"], dpd_world_vector);
    vm.observe("SetBase__6CameraFRC9rmVector3RC9rmVector3RC9rmVector3R9rmVector3R9rmVector3R9rmVector3", camera_set_base);
    if let Ok(ct) = std::env::var("EAGL_DBG_CT") {
        vm.observe(&ct, |h, vm| {
            let this = vm.a(0);
            if !h.dbg_objs.contains(&this) {
                h.dbg_objs.push(this);
            }
            Ok(())
        });
    }
    if std::env::var("EAGL_DBG_THROW").is_ok() {
        vm.observe("Throw__9DodgeballFP18DodgeballCharacterP18DodgeballCharacter19DodgeballThrowSpeed", |h, vm| {
            let (ball, target) = (vm.a(0), vm.a(2));
            if target != 0 {
                let body = vm.r32(ball + 0xc4);
                h.dbg_throws.retain(|t| t.0 != body);
                h.dbg_throws.push((body, target, 25));
            }
            Ok(())
        });
        vm.observe("ProcessBallMiss__18DodgeballCharacterFP9Dodgeball", |h, vm| {
            let (ch, ball) = (vm.a(0), vm.a(1));
            let body = vm.r32(ball + 0xc4);
            println!("    [miss] char {ch:#x} ball body {body:#x} at {:?}", h.phys.body_pos(body));
            Ok(())
        });
        vm.observe("ProcessBallCollision__18DodgeballCharacterFP9Dodgeball", |h, vm| {
            let (ch, ball) = (vm.a(0), vm.a(1));
            let body = vm.r32(ball + 0xc4);
            println!("    [coll] char {ch:#x} ball body {body:#x} at {:?} thrown {}", h.phys.body_pos(body), vm.st.mem.r8(ball + 0x61));
            Ok(())
        });
    }
    if std::env::var("EAGL_DBG_FT").is_ok() {
        vm.observe("HasBallEnteredHoop__11MGFreeThrowFi", |_h, vm| {
            let (this, i) = (vm.a(0), vm.a(1));
            let v = |vm: &mut V, p: u32| [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)];
            let court = vm.r32(this + 0x220);
            let ball = vm.r32(this + 0x198 + 4 * i);
            if ball != 0 && vm.st.mem.r8(ball + 0x24) != 0 {
                eprintln!("[ft] court {:?} ball {:?} prev {:?}", v(vm, court), v(vm, ball + 0x50), v(vm, this + 0x230 + 0x10 * i));
            }
            Ok(())
        });
    }
    if std::env::var("EAGL_DBG_RC").is_ok() {
        vm.observe("Render__5RcCarFRQ23Ren12SceneContext", dbg_rccar);
    }
    if std::env::var("EAGL_DBG_STR").is_ok() {
        vm.observe("GetString__14pgDBCollectionFPCc", dbg_getstring);
        vm.observe("GetCollection__11pgIDatabaseFPCcUx", dbg_getcoll);
    }
    if std::env::var("EAGL_DBG_MOVE").is_ok() {
        vm.observe("AddMoveInputEvent__18DodgeballCharacterFPC9rmVector3f", dbg_move);
    }
    if std::env::var("EAGL_DBG_REACH").is_ok() {
        vm.observe("IsBallReachable__14DodgeballCourtCFPC9rmVector3i", dbg_reach);
    }
    bind(vm, &["Draw__Q23Ren11CachedModelFRC9rmMatrix4b"], cached_model_draw);
    if !vm.observe("SetPos__6CameraFRC9rmVector3", camera_set_pos) {
        vm.log_missing_symbol("SetPos__6CameraFRC9rmVector3");
    }
}

// --- view / projection -------------------------------------------------------------------------------------------------

fn scene_viewport(h: &mut MgHost, vm: &mut V) -> R {
    if h.viewport == 0 {
        h.viewport = vm.alloc_zeroed(0x200, 32);
        h.vp_matrix = vm.alloc_zeroed(0x80, 32);
    }
    vm.ret(h.viewport);
    Ok(())
}

/// `EAGL::ViewPort::GetViewProjectionMatrix()`: view * projection of the active camera, in the engine's row-vector
/// layout (`[x y z 1] * M`).  Field of view and aspect are the Wii's widescreen defaults.
fn viewport_view_projection(h: &mut MgHost, vm: &mut V) -> R {
    if h.vp_matrix == 0 {
        h.viewport = vm.alloc_zeroed(0x200, 32);
        h.vp_matrix = vm.alloc_zeroed(0x80, 32);
    }
    let c = h.camera;
    let m = if c != 0 {
        let eye = read_vec(vm, c + 0x10);
        let target = read_vec(vm, c + 0x20);
        let up = read_vec(vm, c + 0x30);
        let view = glam_look_at(eye, target, up);
        let proj = glam_perspective(h.fov, h.aspect, 0.1, 1000.);
        mat_mul(&proj, &view)
    } else {
        [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
    };
    for (i, v) in m.iter().enumerate() {
        vm.st.mem.wf32(h.vp_matrix + 4 * i as u32, *v);
    }
    vm.ret(h.vp_matrix);
    Ok(())
}

fn read_vec(vm: &mut V, p: u32) -> [f32; 3] {
    [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)]
}

// Column-major 4x4 helpers (memory order == the row-vector matrix the engine expects).
fn mat_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    // column-vector product a * b, both column-major
    let mut r = [0f32; 16];
    for c in 0..4 {
        for row in 0..4 {
            for k in 0..4 {
                r[c * 4 + row] += a[k * 4 + row] * b[c * 4 + k];
            }
        }
    }
    r
}
fn glam_look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let norm = |a: [f32; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let f = norm(sub(target, eye));
    let s = norm(cross(f, up));
    let u = cross(s, f);
    // right-handed view matrix, column-major
    [s[0], u[0], -f[0], 0., s[1], u[1], -f[1], 0., s[2], u[2], -f[2], 0., -dot(s, eye), -dot(u, eye), dot(f, eye), 1.]
}
fn glam_perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1. / (fov_y / 2.).tan();
    [f / aspect, 0., 0., 0., 0., f, 0., 0., 0., 0., (far + near) / (near - far), -1., 0., 0., 2. * far * near / (near - far), 0.]
}

pub fn view(vm: &mut V) {
    bind(vm, &["GetViewPort__Q23Ren5SceneFi"], scene_viewport);
    bind(vm, &["GetViewProjectionMatrix__Q24EAGL8ViewPortFv"], viewport_view_projection);
}


/// Arguments of a `...Handlers` / `FEManager` call from its mangled signature (`Name__16MinigameHandlersFiif` = int, int,
/// float).  `this` is r3, integer arguments follow in r4.., floats in f1.., `char*` arguments are read as strings.
pub fn decode_args(vm: &mut V, name: &str) -> Vec<super::FeArg> {
    use super::FeArg;
    let Some(pos) = name.rfind('F') else { return vec![] };
    // the signature starts after the class token: "...Handlers" + 'F' ('CF' for const methods)
    let sig = &name[pos + 1..];
    let (mut gi, mut gf) = (1usize, 0usize);
    let mut out = vec![];
    let b = sig.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'i' | b'l' | b'b' | b'c' | b's' => {
                let v = vm.a(gi) as i32;
                out.push(FeArg::Int(if b[i] == b'b' { (v & 0xff) as i32 } else { v }));
                gi += 1;
                i += 1;
            }
            b'U' => {
                out.push(FeArg::Int(vm.a(gi) as i32));
                gi += 1;
                i += 2;
            }
            b'f' => {
                out.push(FeArg::Float(vm.fa(gf)));
                gf += 1;
                i += 1;
            }
            b'P' | b'R' => {
                // pointers: `PCc` / `Pc` are C strings, anything else is passed through as an address
                let rest = &sig[i..];
                let ptr = vm.a(gi);
                gi += 1;
                if rest.starts_with("PCc") || rest.starts_with("Pc") {
                    let s = vm.st.mem.cstr(ptr, 128);
                    out.push(FeArg::Str(s));
                } else {
                    out.push(FeArg::Int(ptr as i32));
                }
                // skip the pointee type tokens up to the next argument boundary (simple types only)
                i += 1;
                while i < b.len() && matches!(b[i], b'C' | b'P' | b'R') {
                    i += 1;
                }
                if i < b.len() && b[i].is_ascii_digit() {
                    let mut n = 0usize;
                    while i < b.len() && b[i].is_ascii_digit() {
                        n = n * 10 + (b[i] - b'0') as usize;
                        i += 1;
                    }
                    i += n;
                } else if i < b.len() && b[i] == b'Q' {
                    // qualified name: Q2 5Enums 5Foo
                    let parts = (b[i + 1] - b'0') as usize;
                    i += 2;
                    for _ in 0..parts {
                        let mut n = 0usize;
                        while i < b.len() && b[i].is_ascii_digit() {
                            n = n * 10 + (b[i] - b'0') as usize;
                            i += 1;
                        }
                        i += n;
                    }
                } else {
                    i += 1;
                }
            }
            b'Q' => {
                // enum / struct by value: pass as int
                let parts = (b[i + 1] - b'0') as usize;
                i += 2;
                for _ in 0..parts {
                    let mut n = 0usize;
                    while i < b.len() && b[i].is_ascii_digit() {
                        n = n * 10 + (b[i] - b'0') as usize;
                        i += 1;
                    }
                    i += n;
                }
                out.push(FeArg::Int(vm.a(gi) as i32));
                gi += 1;
            }
            b'v' => i += 1,
            c if c.is_ascii_digit() => {
                // class-typed argument (e.g. 15RCCarsHintTypes): enum -> int
                let mut n = 0usize;
                while i < b.len() && b[i].is_ascii_digit() {
                    n = n * 10 + (b[i] - b'0') as usize;
                    i += 1;
                }
                i += n;
                out.push(FeArg::Int(vm.a(gi) as i32));
                gi += 1;
            }
            _ => i += 1,
        }
    }
    out
}

// --- FEManager screens --------------------------------------------------------------------------------------------------

fn fe_screen_call(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.name_of(vm.st.cpu.pc);
    let arg = if name.contains("FPc") { vm.st.mem.cstr(vm.a(1), 64) } else { String::new() };
    let call = name.split("__").next().unwrap_or("").to_string();
    // the front end's `ScreenReady("WorldHud")` -> `FEManager::SetWorldHudLoaded` (+0x130), which the world's "press A"
    // microgames (Bug Hunt, Dribbling) wait for; here the HUD counts as loaded once opened
    if call == "OpenAptScreen" && arg == "WorldHud" {
        let fe = vm.a(0);
        vm.st.mem.w8(fe + 0x130, 1);
    }
    h.events.push(super::FeEvent { name: format!("FEManager::{call}"), ints: [0; 4], floats: [0.; 2], args: vec![super::FeArg::Str(arg)] });
    Ok(())
}

pub fn fe_screens(vm: &mut V) {
    bind(
        vm,
        &[
            "OpenAptScreen__9FEManagerFPc",
            "CloseAptScreen__9FEManagerFv",
            "OpenAptOverlay__9FEManagerFPc",
            "CloseAptOverlay__9FEManagerFv",
            "ReplaceAptScreen__9FEManagerFPc",
            "ClearScreenStack__9FEManagerFv",
        ],
        fe_screen_call,
    );
}

/// `AptCallFunction(const char* function, char* result, const char* path, int count, ...)`: the executable's call into the
/// running front-end movie.  The variadic arguments are C strings (printf-formatted numbers or text).
fn apt_call_function(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.st.mem.cstr(vm.a(0), 96);
    let path = vm.st.mem.cstr(vm.a(2), 96);
    let count = vm.a(3).min(12) as usize;
    let sp = vm.st.cpu.r[1];
    let mut args = vec![];
    for k in 0..count {
        let p = if k < 4 { vm.a(4 + k) } else { vm.r32(sp + 8 + 4 * (k as u32 - 4)) };
        args.push(super::FeArg::Str(vm.st.mem.cstr(p, 256)));
    }
    h.events.push(super::FeEvent { name: format!("Apt::{name}"), ints: [0; 4], floats: [0.; 2], args });
    let _ = path;
    vm.ret(0);
    Ok(())
}

pub fn apt_calls(vm: &mut V) {
    bind(vm, &["AptCallFunction__FPCcPcPCcie"], apt_call_function);
}

/// `PreGameHandlers::SetupPreGameHandlers(this, MinigameType, int, PreGameInfo*)`: note the info words for the
/// front end's `PreGame_*` queries, then run the original.
fn setup_pregame(h: &mut MgHost, vm: &mut V) -> R {
    let (ty, flag, info) = (vm.a(1), vm.a(2), vm.a(3));
    let mut args = vec![super::FeArg::Int(ty as i32), super::FeArg::Int(flag as i32)];
    for k in 0..4 {
        args.push(super::FeArg::Int(vm.r32(info + 4 * k) as i32));
    }
    h.events.push(super::FeEvent { name: "PreGameInfo".into(), ints: [0; 4], floats: [0.; 2], args });
    Ok(())
}

/// `PostGameHandlers::SetupPostGameHandlers(this, MinigameType, PostGameInfo*)`: hand the 70 info words (0x118 bytes) to
/// the front end's post-game port, then run the original.
fn setup_postgame(h: &mut MgHost, vm: &mut V) -> R {
    let (ty, info) = (vm.a(1), vm.a(2));
    let mut args = vec![super::FeArg::Int(ty as i32)];
    for k in 0..70 {
        args.push(super::FeArg::Int(vm.r32(info + 4 * k) as i32));
    }
    h.events.push(super::FeEvent { name: "PostGameInfo".into(), ints: [0; 4], floats: [0.; 2], args });
    Ok(())
}

/// `GameState::UpdateHomeMenuIcon`: the loading-screen render task EndMinigame queues around its teardown; nothing to draw.
fn home_menu_icon(_h: &mut MgHost, vm: &mut V) -> R {
    vm.ret(0);
    Ok(())
}

// --- audio ------------------------------------------------------------------------------------------------------------

/// `Audio::PlaySFX(this, AUDIOAEMSBESFX | AUDIOAEMSFEHUDSFX id, azimuth, volume)`: run the original id switch of
/// `AuAEMSManager::PlaySFX` against an enabled stand-in manager; the class it instantiates is caught below.
/// The `AuAEMSManager` the native sound switches run on (enabled flag +4; loop instances at +0x2e4..+0x300).
fn aems_manager(h: &mut MgHost, vm: &mut V) -> u32 {
    if h.aems_mgr == 0 {
        h.aems_mgr = vm.alloc_zeroed(0x400, 16);
        vm.st.mem.w8(h.aems_mgr + 4, 1);
    }
    h.aems_mgr
}
/// `Audio::StartSFX / UpdateSFX / StopSFX(this, AUDIOAEMSBESFX id, ..)`: looping sounds through the manager's native switch.
fn audio_loop_sfx(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.name_of(vm.st.cpu.pc);
    let mgr = aems_manager(h, vm);
    let (a1, a2, a3, a4) = (vm.a(1), vm.a(2), vm.a(3), vm.a(4));
    if name.starts_with("StartSFX") {
        vm.call_by_name(h, "StartSFX__13AuAEMSManagerF14AUDIOAEMSBESFXii", &[mgr, a1, a2, a3], &[])?;
    } else if name.starts_with("UpdateSFX") {
        vm.call_by_name(h, "UpdateSFX__13AuAEMSManagerF14AUDIOAEMSBESFXiii", &[mgr, a1, a2, a3, a4], &[])?;
    } else {
        vm.call_by_name(h, "StopSFX__13AuAEMSManagerF14AUDIOAEMSBESFX", &[mgr, a1], &[])?;
    }
    Ok(())
}
fn audio_play_sfx(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.name_of(vm.st.cpu.pc);
    aems_manager(h, vm);
    let target = if name.contains("AUDIOAEMSBESFX") { "PlaySFX__13AuAEMSManagerF14AUDIOAEMSBESFXii" } else { "PlaySFX__13AuAEMSManagerF17AUDIOAEMSFEHUDSFXii" };
    let (mgr, id, az, vol) = (h.aems_mgr, vm.a(1), vm.a(2), vm.a(3));
    vm.call_by_name(h, target, &[mgr, id, az, vol], &[])?;
    Ok(())
}

/// `Csis::Class::CreateInstance(ClassHandle*, void* inputs, Class** out)`: a sound starts; the handle's symbol names the
/// class and the first input is the variant (an index into the class's sample table).
fn csis_create_instance(h: &mut MgHost, vm: &mut V) -> R {
    let (handle, inputs, out) = (vm.a(0), vm.a(1), vm.a(2));
    if h.csis_handles.is_empty() {
        for sym in &vm.img.symbols {
            if let Some(c) = sym.name.strip_prefix('g').and_then(|n| n.strip_suffix("Handle__4Csis")) {
                h.csis_handles.insert(sym.addr, c.to_string());
            }
        }
    }
    if let Some(class) = h.csis_handles.get(&handle).cloned() {
        if is_loop_class(&class) {
            // the wrapper object (`out`) carries the inputs the game updates every frame; its destructor ends the loop
            h.loops.insert(out, class);
        } else {
            let variant = if inputs != 0 { vm.r32(inputs) as i32 } else { 0 };
            h.sounds.push((class, variant.max(0) as usize));
        }
    }
    if out != 0 {
        vm.w32(out, 0);
    }
    vm.ret(0);
    Ok(())
}

/// Csis classes that loop until their instance is destroyed (`AuAEMSManager::StartSFX` / `StopSFX`).
pub fn is_loop_class(class: &str) -> bool {
    class.contains("Engine") || class.ends_with("_LP")
}

/// `Csis::<loop class>::~<class>(this, flags)`: the loop stops.
fn csis_loop_dt(h: &mut MgHost, vm: &mut V) -> R {
    h.loops.remove(&vm.a(0));
    Ok(())
}

/// `AIP::CmdDecomposer::GetIntArrayByName(this, name, int* out, count)` for handlers the host calls directly.
fn int_array_by_name(h: &mut MgHost, vm: &mut V) -> R {
    let (out, count) = (vm.a(2), vm.a(3));
    for i in 0..count.min(16) {
        let v = h.int_array.get(i as usize).copied().unwrap_or(0);
        vm.w32(out + 4 * i, v as u32);
    }
    vm.ret(1);
    Ok(())
}

/// Renderer visibility queries (`Ren::FrustumTest::IsBoundingBoxInView`, `AreaManager::IsPointVisible`): Bevy culls on
/// its own, so everything counts as on screen.  Characters only evaluate their animation pose while visible
/// (`Character::Update` -> `Character+0x215` -> `AnimationState::Update`).
fn visible(_h: &mut MgHost, vm: &mut V) -> R {
    vm.ret(1);
    Ok(())
}

/// `Csis::<Class>::<Class>(this, int variant, ...)`: sound classes the game constructs directly (most `PlaySFX` ids do);
/// the mangled name gives the class.  Behaves like the null constructor otherwise (vtable, returns `this`).
fn csis_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.name_of(vm.st.cpu.pc);
    let this = vm.a(0);
    if let Some(class) = name.strip_prefix("__ct__").and_then(super::mangled_class) {
        // `Q24Csis23MGSFX_Dodgeball_Actions` -> `MGSFX_Dodgeball_Actions`
        let short = class.trim_start_matches("Q24Csis").trim_start_matches(|c: char| c.is_ascii_digit()).to_string();
        if let Some(vt) = vm.img.addr(&format!("__vt__{class}")) {
            if this != 0 {
                vm.w32(this, vt);
            }
        }
        // `...Fii`: the first int input is the variant
        if name.contains("Fi") {
            let variant = vm.a(1) as i32;
            h.sounds.push((short, variant.max(0) as usize));
        }
    }
    vm.ret(this);
    Ok(())
}

/// `Audio::PlayMusic(this, AUDIOMUSICTYPES)`: `AuMusicManager::Update` streams `kMusicFilenames[type]`, or for type 1
/// the current area's world track.
fn play_music(h: &mut MgHost, vm: &mut V) -> R {
    let ty = vm.a(1);
    let name = if ty == 1 {
        "world".to_string()
    } else {
        let table = vm.img.addr("kMusicFilenames").unwrap_or(0);
        if table == 0 || ty >= 10 {
            return Ok(());
        }
        let p = vm.r32(table + 4 * ty);
        vm.st.mem.cstr(p, 64)
    };
    h.music = Some(name);
    Ok(())
}

// --- immediate-mode textured drawing ------------------------------------------------------------------------------

/// `TarManager::Initialize(this, CString& bank)`: remember the bank; its table (count +0x4200, 0x84-byte entries
/// {TAR*, name}) is read lazily once the original has filled it.
fn tar_manager_init(h: &mut MgHost, vm: &mut V) -> R {
    let (this, cstr) = (vm.a(0), vm.a(1));
    let p = vm.call_by_name(h, "c_str__7CStringCFv", &[cstr], &[])?;
    let path = vm.st.mem.cstr(p, 256);
    h.tar_managers.retain(|t| t.0 != this);
    h.tar_managers.push((this, path));
    Ok(())
}

fn tar_name(h: &mut MgHost, vm: &mut V, tar: u32) -> Result<Option<(String, String, usize)>, String> {
    if let Some(t) = h.tars.get(&tar) {
        return Ok(Some(t.clone()));
    }
    for (mgr, path) in h.tar_managers.clone() {
        if !h.tar_banks.contains_key(&path) {
            // `AssetManager::CheckForTexture(name, int* size, int* index)` -> the loaded bank
            let am = vm.r32(vm.img.addr("sAssetManagerInstance__12AssetManager").unwrap_or(0));
            let (name, size) = (vm.alloc_cstr(&path), vm.alloc_zeroed(8, 4));
            let data = vm.call_by_name(h, "CheckForTexture__12AssetManagerFPCcPiPi", &[am, name, size, 0], &[])?;
            let len = vm.r32(size).min(16 << 20) as usize;
            if data != 0 && len > 0 {
                let bytes: Vec<u8> = (0..len as u32).map(|i| vm.st.mem.r8(data + i)).collect();
                h.tar_banks.insert(path.clone(), std::sync::Arc::new(bytes));
            }
        }
        let n = vm.r32(mgr + 0x4200).min(128);
        for i in 0..n {
            let e = mgr + 0x84 * i;
            let t = vm.r32(e);
            let name = vm.st.mem.cstr(e + 4, 128);
            h.tars.insert(t, (path.clone(), name, i as usize));
        }
    }
    Ok(h.tars.get(&tar).cloned())
}

fn dt_set_texture(h: &mut MgHost, vm: &mut V) -> R {
    let (this, tar) = (vm.a(0), vm.a(1));
    h.draw_textured.entry(this).or_default().tex = tar;
    Ok(())
}

fn dt_set_model(h: &mut MgHost, vm: &mut V) -> R {
    let (this, m) = (vm.a(0), vm.a(1));
    let mut mat = [0f32; 16];
    for (i, v) in mat.iter_mut().enumerate() {
        *v = vm.st.mem.rf32(m + 4 * i as u32);
    }
    h.draw_textured.entry(this).or_default().model = mat;
    Ok(())
}

fn dt_begin(h: &mut MgHost, vm: &mut V) -> R {
    let (this, prim) = (vm.a(0), vm.a(1));
    let st = h.draw_textured.entry(this).or_default();
    st.prim = prim;
    st.verts.clear();
    Ok(())
}

/// `AddVertex(this, const COORD3&, const Colour& (GX RGBA8), const COORD2&)`
fn dt_add_vertex(h: &mut MgHost, vm: &mut V) -> R {
    let (this, p, c, t) = (vm.a(0), vm.a(1), vm.a(2), vm.a(3));
    let pos = [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)];
    let rgba = vm.r32(c).to_be_bytes();
    let uv = [vm.st.mem.rf32(t), vm.st.mem.rf32(t + 4)];
    h.draw_textured.entry(this).or_default().verts.push((pos, rgba, uv));
    Ok(())
}

fn dt_end(h: &mut MgHost, vm: &mut V) -> R {
    let this = vm.a(0);
    let Some(st) = h.draw_textured.get_mut(&this) else { return Ok(()) };
    let (tex, model, prim, verts) = (st.tex, st.model, st.prim, std::mem::take(&mut st.verts));
    let tex = if tex != 0 { tar_name(h, vm, tex)? } else { None };
    h.imm.push(super::ImmDraw { tex, model, prim, verts });
    Ok(())
}

pub fn pregame(vm: &mut V) {
    vm.observe("Initialize__10TarManagerFR7CString", tar_manager_init);
    // world rendering the minigame changes (radius of the curved world, the area model on or off)
    vm.observe("SetCurvedWorldRadius__11AreaManagerFUi", |h, vm| {
        h.curved_radius = Some(vm.a(1) as f32);
        Ok(())
    });
    vm.observe("SetDrawCurrentAreaModel__8WorldManFb", |h, vm| {
        h.hide_area_model = vm.a(1) & 0xff == 0;
        Ok(())
    });
    // AreaManager is a null service: the placeable list (props, gate doors) is built here, before the characters (and the
    // gates a profile's stickers unlock) are set up, as the original area load has it ready by then
    vm.observe("InitializeCharacters__15PlaygroundWorldFv", |h, vm| {
        let areas = vm.r32(vm.a(0) + 8);
        if areas != 0 && vm.r32(areas + 0x1b0 + 0xc) == 0 {
            vm.call_by_name(h, "Initialize__16PlaceableManagerFv", &[areas + 0x1b0], &[])?;
        }
        Ok(())
    });
    if std::env::var("EAGL_DBG_PLACE").is_ok() {
        vm.observe("GetPlaceable__16PlaceableManagerFPCc", |h, vm| {
            let name = vm.st.mem.cstr(vm.a(1), 96);
            let pm = vm.a(0);
            let (base, n) = (vm.r32(pm + 8), vm.r32(pm + 0xc).min(4096));
            let mut gates = vec![];
            for i in 0..n {
                let p = vm.call_by_name(h, "c_str__7CStringCFv", &[base + 0x120 * i + 4], &[]).unwrap_or(0);
                let s = vm.st.mem.cstr(p, 96);
                if s.contains("gate") {
                    gates.push(s);
                }
            }
            eprintln!("[place] GetPlaceable({name}) of {n} placeables; gates {gates:?}");
            Ok(())
        });
    }
    if std::env::var("EAGL_DBG_RC").is_ok() {
        vm.observe("Render__5RcCarFRQ23Ren12SceneContext", |_h, vm| {
            let c = vm.a(0);
            let mg = vm.r32(super::snapshot::WORLD_MAN + 0x90);
            let player = if mg != 0 { vm.r32(mg + 0x12c) } else { 0 };
            let track = if mg != 0 { vm.r32(mg + 0x114) } else { 0 };
            let lanes = if track != 0 { vm.r32(track + 0x25370) } else { 0 };
            let f = |vm: &mut V, o: u32| vm.st.mem.rf32(c + o);
            if c == player && track != 0 && std::env::var("EAGL_DBG_RCLANES").is_ok() {
                let buf = vm.alloc_zeroed(0x80, 16);
                let mut pts = vec![];
                for lane in 0..lanes.min(8) {
                    for k in 0..7u32 {
                        let w = vm.r32(c + 0x370 + 4 * k);
                        vm.w32(buf + 4 * k, w);
                    }
                    vm.w32(buf, lane);
                    vm.call_by_name(_h, "GetLanePos__7RcTrackF9RcLanePosf", &[buf + 0x40, track, buf], &[0.])?;
                    pts.push([vm.st.mem.rf32(buf + 0x50), vm.st.mem.rf32(buf + 0x54), vm.st.mem.rf32(buf + 0x58)]);
                }
                eprintln!("[rc] lanes at player: {pts:?}");
            }
            eprintln!("[rc] car {c:#x} player {} lane {} of {lanes} lanepos {:?} pos {:?} target {:?}", c == player, vm.r32(c + 0x370) as i32, [f(vm, 0x374), f(vm, 0x378), f(vm, 0x37c)], [f(vm, 0), f(vm, 4), f(vm, 8)], [f(vm, 0x380), f(vm, 0x384), f(vm, 0x388)]);
            Ok(())
        });
        vm.observe("SwitchLanes__5RcCarFb", |_h, vm| {
            let mg = vm.r32(super::snapshot::WORLD_MAN + 0x90);
            let player = if mg != 0 { vm.r32(mg + 0x12c) } else { 0 };
            eprintln!("[rc] SwitchLanes car {:#x} player {} right {} lane {}", vm.a(0), vm.a(0) == player, vm.a(1), vm.r32(vm.a(0) + 0x370) as i32);
            Ok(())
        });
    }
    if std::env::var("EAGL_DBG_DART").is_ok() {
        vm.observe("StickDart__6DSDartF9rmVector39rmVector3", |h, vm| {
            let v = |vm: &mut V, p: u32| [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)];
            let (pos, off) = (v(vm, vm.a(1)), v(vm, vm.a(2)));
            let cam = if h.camera != 0 { v(vm, h.camera + 0x70) } else { [0.; 3] };
            eprintln!("[dart] StickDart {:#x} pos {pos:?} off {off:?} cam {cam:?}", vm.a(0));
            Ok(())
        });
        vm.observe("RenderOpaque__9DSDartGunFb", |_h, vm| {
            if vm.st.mem.r8(vm.a(0) + 0x11e) == 0 || vm.a(1) == 0 {
                return Ok(());
            }
            let this = vm.a(0);
            let v = |vm: &mut V, p: u32| [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)];
            let cm = vm.img.addr("sCameraManagerInstance__13CameraManager").map(|a| vm.r32(a)).unwrap_or(0);
            let cm = if cm != 0 { vm.r32(cm) } else { 0 };
            let consts: Vec<(String, f32)> = ["@37760", "@38155", "@38154", "@38011", "@38153", "@37725", "@37759"].iter().map(|n| (n.to_string(), vm.img.addr(n).map(|a| vm.st.mem.rf32(a)).unwrap_or(f32::NAN))).collect();
            let m50: Vec<f32> = (0..16).map(|i| vm.st.mem.rf32(this + 0x50 + 4 * i)).collect();
            eprintln!("[gun] m50 {m50:?}");
            let am = vm.r32(vm.img.addr("gWorld").unwrap_or(0) + 0x8c);
            let am = if am != 0 { vm.r32(am + 8) } else { 0 };
            let out = vm.alloc_zeroed(0x40, 16);
            let r = vm.call_by_name(_h, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[am, this + 0x20, out], &[]);
            let calc: Vec<f32> = (0..16).map(|i| vm.st.mem.rf32(out + 4 * i)).collect();
            eprintln!("[gun] areamgr {am:#x} {r:?} calc {calc:?}");
            eprintln!("[gun] {this:#x} +0 {:?} +10 {:?} +20 {:?} +30 {:?} y+ {} cm d0 {:?} e0 {:?} f0 {:?} {consts:?}", v(vm, this), v(vm, this + 0x10), v(vm, this + 0x20), v(vm, this + 0x30), vm.st.mem.rf32(this + 0x90), v(vm, cm + 0xd0), v(vm, cm + 0xe0), v(vm, cm + 0xf0));
            Ok(())
        });
        vm.observe("Render__6DSDartFv", |h, vm| {
            let this = vm.a(0);
            if vm.r32(this + 0x230) as i32 > 0 {
                let body = vm.r32(this + 0x38);
                let cam = if h.camera != 0 { [vm.st.mem.rf32(h.camera + 0x70), vm.st.mem.rf32(h.camera + 0x74), vm.st.mem.rf32(h.camera + 0x78)] } else { [0.; 3] };
                eprintln!("[dart] stuck {this:#x} body {:?} cam {cam:?} t {}", h.phys.body_pos(body), vm.r32(this + 0x230) as i32);
            }
            Ok(())
        });
    }
    if std::env::var("EAGL_DBG_IMM").is_ok() {
        vm.observe("GetTar__10TarManagerFPCc", |_h, vm| {
            let this = vm.a(0);
            eprintln!("[tar] GetTar this {this:#x} count {} name {}", vm.r32(this + 0x4200), vm.st.mem.cstr(vm.a(1), 64));
            Ok(())
        });
    }
    bind(vm, &["SetTexture__Q24EAGL12DrawTexturedFPQ24EAGL3TAR"], dt_set_texture);
    bind(vm, &["SetModelMatrix__Q24EAGL12DrawTexturedFRC7MATRIX4", "SetModelMatrix__Q24EAGL12DrawTexturedFR7MATRIX4"], dt_set_model);
    bind(vm, &["Begin__Q24EAGL12DrawTexturedFQ24EAGL13PrimitiveType"], dt_begin);
    bind(vm, &["AddVertex__Q24EAGL12DrawTexturedFRC6COORD3RCQ24EAGL6ColourRC6COORD2"], dt_add_vertex);
    bind(vm, &["End__Q24EAGL12DrawTexturedFv"], dt_end);
    bind(vm, &["PlayMusic__5AudioF15AUDIOMUSICTYPES"], play_music);
    bind(vm, &["StopMusic__5AudioFv"], |h, _vm| {
        h.music = Some(String::new());
        Ok(())
    });
    vm.hook_matching(|n| n.starts_with("__ct__Q24Csis") && ["MGSFX_", "WSFX_", "FESFX_"].iter().any(|p| n.contains(p)), csis_ctor);
    bind(vm, &["IsBoundingBoxInView__Q23Ren11FrustumTestFRCQ23Ren11BoundingBoxRC9rmMatrix4", "IsPointVisible__11AreaManagerFRC9rmVector3RC9rmVector3"], visible);
    // `AnimationState::Update(this, dt, bool on_screen, bool)`: the character's on-screen flag comes from its (absent)
    // render geometry, so always evaluate the pose
    vm.observe("Update__14AnimationStateFfbb", |_h, vm| {
        vm.st.cpu.r[4] = 1;
        Ok(())
    });
    bind(vm, &["GetIntArrayByName__Q23AIP13CmdDecomposerCFPCcPii"], int_array_by_name);
    bind(vm, &["PlaySFX__5AudioF14AUDIOAEMSBESFXii", "PlaySFX__5AudioF17AUDIOAEMSFEHUDSFXii"], audio_play_sfx);
    bind(vm, &["CreateInstance__Q24Csis5ClassFPQ24Csis11ClassHandlePvPPQ24Csis5Class"], csis_create_instance);
    bind(vm, &["StartSFX__5AudioF14AUDIOAEMSBESFXii", "UpdateSFX__5AudioF14AUDIOAEMSBESFXiii", "StopSFX__5AudioF14AUDIOAEMSBESFX"], audio_loop_sfx);
    vm.hook_matching(|n| n.starts_with("__dt__Q24Csis") && is_loop_class(n), csis_loop_dt);
    bind(vm, &["UpdateHomeMenuIcon__9GameStateFii"], home_menu_icon);
    if !vm.observe("SetupPostGameHandlers__16PostGameHandlersFQ25Enums12MinigameTypeP12PostGameInfo", setup_postgame) {
        vm.log_missing_symbol("SetupPostGameHandlers");
    }
    if !vm.observe("SetupPreGameHandlers__15PreGameHandlersFQ25Enums12MinigameTypeiP11PreGameInfo", setup_pregame) {
        vm.log_missing_symbol("SetupPreGameHandlers");
    }
}
