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
        ],
        mem_alloc,
    );
    bind(vm, &["Free__6MemMgrFPv", "__dl__FPv", "__dla__FPv", "free"], mem_free);
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
    let n: u32 = if name.starts_with("rc_") { 6 } else { 1 };
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

pub fn scene(vm: &mut V) {
    bind(vm, &["GetSceneOptions__Q23Ren5SceneCFi"], scene_options);
}

// --- input ------------------------------------------------------------------------------------------------------------

/// One Wii Remote as the executable's `_WiiPadStatus` sees it (`WPADStatus` bits, accelerometer centred at 512).
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    pub active: bool,
    /// WPAD button bits: LEFT 0x1, RIGHT 0x2, DOWN 0x4, UP 0x8, PLUS 0x10, TWO 0x100, ONE 0x200, B 0x400, A 0x800, MINUS 0x1000, Z 0x2000, C 0x4000, HOME 0x8000.
    pub buttons: u16,
    pub acc: [i16; 3],
}

impl Default for Pad {
    fn default() -> Self {
        Pad { active: false, buttons: 0, acc: [512, 512, 616] }
    }
}

pub const PAD_STATUS: u32 = 0x805f_017c;

/// Write the host pad states into the guest's `PAD` array (what `PAD_update` would have done).
pub fn write_pads(vm: &mut V, pads: &[Pad; 4]) {
    for (i, p) in pads.iter().enumerate() {
        let a = PAD_STATUS + 0x60 * i as u32;
        vm.w32(a, if p.active { 2 } else { 0 });
        vm.st.mem.w16(a + 4, p.buttons);
        for k in 0..3 {
            vm.st.mem.w16(a + 6 + 2 * k as u32, p.acc[k] as u16);
        }
        for k in 0..16 {
            vm.st.mem.w16(a + 0xc + 2 * k, 1023);
        }
        vm.st.mem.w8(a + 0x2c, 0);
        vm.st.mem.w8(a + 0x2d, 0);
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
    let model = vm.r32(this + 0x44);
    let name = h.model_names.get(&model).cloned().unwrap_or_else(|| format!("model@{model:#x}"));
    let mut mat = [0f32; 16];
    for (i, v) in mat.iter_mut().enumerate() {
        *v = vm.st.mem.rf32(m + 4 * i as u32);
    }
    h.draws.push((name, mat));
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
    vm.observe("SetBase__6CameraFRC9rmVector3RC9rmVector3RC9rmVector3R9rmVector3R9rmVector3R9rmVector3", camera_set_base);
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

pub fn pregame(vm: &mut V) {
    if !vm.observe("SetupPreGameHandlers__15PreGameHandlersFQ25Enums12MinigameTypeiP11PreGameInfo", setup_pregame) {
        vm.log_missing_symbol("SetupPreGameHandlers");
    }
}
