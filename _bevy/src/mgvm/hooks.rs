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
}

fn bind(vm: &mut V, names: &[&str], f: fn(&mut MgHost, &mut V) -> R) {
    for n in names {
        if !vm.hook(n, f) {
            vm.log_missing_symbol(n);
        }
    }
}

// --- memory -----------------------------------------------------------------------------------------------------------

fn alloc_size(_h: &mut MgHost, vm: &mut V) -> R {
    let size = vm.a(0);
    let p = vm.alloc_zeroed(size.max(4), 32);
    vm.ret(p);
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
    bind(vm, &["Free__6MemMgrFPv", "__dl__FPv", "__dla__FPv", "free", "MemFill__6MemMgrFPvUii"], nothing);
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
            h.events.push(super::FeEvent { name: name.clone(), ints, floats });
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
fn resolve_model(_h: &mut MgHost, vm: &mut V) -> R {
    let (loader, models, count) = (vm.a(3), vm.a(4), vm.a(5));
    let dummy = vm.alloc_zeroed(0x200, 32);
    // the asset slot's name sits at +0xc of the 0xb8-byte entry that `models` (+0x90) points into
    let name = vm.st.mem.cstr(models - 0x90 + 0xc, 64);
    _h.model_names.insert(dummy, name);
    vm.w32(loader, 0);
    vm.w32(models, dummy);
    vm.w32(count, 1);
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

pub fn drawing(vm: &mut V) {
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
