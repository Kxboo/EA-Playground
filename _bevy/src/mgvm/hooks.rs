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
