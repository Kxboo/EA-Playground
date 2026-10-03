//! MicroBugHuntManager (microbughuntmanager.cpp): the Bug Hunt minigame manager, ported from the original PowerPC.
//!
//! Conventions used by every port in this file:
//!  * the original's stack frame is reproduced (same `sp`, same local offsets), so pointers to locals that are handed to
//!    other functions are the same addresses the original would pass;
//!  * every call the original makes is made here in the same order with the same arguments, from the same `sp`
//!    (see `gc`), so the callee's own frame and anything it passes on is identical too;
//!  * float arithmetic mirrors the interpreter (single precision ops round to f32, fmadds/fmadd are fused).
#![allow(unused)]
use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
pub type R = Result<(), String>;

/// String table of the unit: `lis r31,-0x7fb2 ; addi r31,r31,-0x7640`.
const STR: u32 = 0x804d_89c0;
/// The WorldMan singleton: `lis r3,-0x7fa1 ; addi r3,r3,-0x7ce0`.
const WORLD_MAN: u32 = 0x805e_8320;
/// The global rmMatrix4 built by the static initializer: `lis r3,-0x7fa2 ; addi r3,r3,0x2a20`.
const G_MATRIX: u32 = 0x805e_2a20;
/// Swipe callbacks registered with the Conga callback manager.
const SWIPE_CB: u32 = 0x8033_0378;
const SWIPE_REV_CB: u32 = 0x8033_0388;

// ---------------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------------

fn rd(vm: &mut V, a: u32) -> u32 {
    vm.st.mem.r32(a)
}
fn wr(vm: &mut V, a: u32, v: u32) {
    vm.st.mem.w32(a, v)
}
fn rb(vm: &mut V, a: u32) -> u8 {
    vm.st.mem.r8(a)
}
fn wb(vm: &mut V, a: u32, v: u8) {
    vm.st.mem.w8(a, v)
}
/// lfs: a single from memory, widened to the register's f64.
fn lfs(vm: &mut V, a: u32) -> f64 {
    vm.st.mem.rf32(a) as f64
}
/// stfs: the register's f64 narrowed to a single in memory.
fn stfs(vm: &mut V, a: u32, v: f64) {
    vm.st.mem.wf32(a, v as f32)
}
fn lfd(vm: &mut V, a: u32) -> f64 {
    f64::from_bits(vm.st.mem.r64(a))
}
/// `lfs f, off(r2)` (small data area 2 constants).
fn lfs2(vm: &mut V, off: i32) -> f64 {
    let a = vm.st.cpu.r[2].wrapping_add(off as u32);
    lfs(vm, a)
}
/// `lfd f, off(r2)`.
fn lfd2(vm: &mut V, off: i32) -> f64 {
    let a = vm.st.cpu.r[2].wrapping_add(off as u32);
    lfd(vm, a)
}
/// `lwz r, off(r13)` (small data area global).
fn lwz13(vm: &mut V, off: i32) -> u32 {
    let a = vm.st.cpu.r[13].wrapping_add(off as u32);
    rd(vm, a)
}
/// `addi r, r13, off` (address of a small data area global).
fn ad13(vm: &V, off: i32) -> u32 {
    vm.st.cpu.r[13].wrapping_add(off as u32)
}

/// Single-precision rounding of the interpreter (`round_single`).
fn fs(x: f64) -> f64 {
    x as f32 as f64
}
fn fsubs(a: f64, b: f64) -> f64 {
    fs(a - b)
}
fn fadds(a: f64, b: f64) -> f64 {
    fs(a + b)
}
fn fmuls(a: f64, c: f64) -> f64 {
    fs(a * c)
}
fn fdivs(a: f64, b: f64) -> f64 {
    fs(a / b)
}
/// fmadds: a*c + b, fused, then rounded to single.
fn fmadds(a: f64, c: f64, b: f64) -> f64 {
    fs(a.mul_add(c, b))
}
/// fcmpo / fcmpu result as the 4-bit CR field of the interpreter: 8 = lt, 4 = gt, 2 = eq, 1 = unordered.
fn fcmp(a: f64, b: f64) -> u8 {
    if a.is_nan() || b.is_nan() {
        1
    } else if a < b {
        8
    } else if a > b {
        4
    } else {
        2
    }
}
const LT: u8 = 8;
const GT: u8 = 4;
/// the "ge" of `fcmpo; cror eq,gt,eq; bne`: gt or eq.
const GE_BITS: u8 = 6;

/// The `sp` of a function with a fixed frame (`stwu r1,-size(r1)`).
fn fsp(vm: &V, size: u32) -> u32 {
    vm.st.cpu.r[1].wrapping_sub(size)
}
/// The `sp` of a function with a 16-byte aligned dynamic frame (`clrlwi r11,r1,28 ; subfic r11,r11,-size ; stwux`).
fn dsp(vm: &V, size: u32) -> u32 {
    let r1 = vm.st.cpu.r[1];
    r1.wrapping_sub(size).wrapping_sub(r1 & 0xf)
}

/// Call the guest function at `addr` the way the original does from a frame whose stack pointer is `sp`: the callee
/// enters with `r1 == sp`, and r3 / f1 hold its result afterwards (r4 gets the callee's final r4 as a leftover).
fn gca(h: &mut MgHost, vm: &mut V, sp: u32, addr: u32, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
    let r1 = vm.st.cpu.r[1];
    // `Vm::call` puts the callee at (r1 - 0x200) & !0xf
    vm.st.cpu.r[1] = sp.wrapping_add(0x200);
    let r = vm.call(h, addr, args, fargs);
    vm.st.cpu.r[1] = r1;
    vm.st.cpu.r[4] = vm.ret_hi_lo;
    r
}
fn gc(h: &mut MgHost, vm: &mut V, sp: u32, name: &str, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
    let addr = vm.img.addr(name).ok_or_else(|| format!("no symbol {name}"))?;
    gca(h, vm, sp, addr, args, fargs)
}
/// `gc` without float arguments.
fn cl(h: &mut MgHost, vm: &mut V, sp: u32, name: &str, args: &[u32]) -> Result<u32, String> {
    gc(h, vm, sp, name, args, &[])
}

// ---------------------------------------------------------------------------------------------------------------------
// construction / destruction
// ---------------------------------------------------------------------------------------------------------------------

/// MicroBugHuntManager::MicroBugHuntManager() @0x8032efb0
pub fn ctor(_h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let none = lwz13(vm, -0x2278);
    let c0 = lfs2(vm, -0x489c);
    wr(vm, t, 0);
    wr(vm, t + 4, u32::MAX);
    wr(vm, t + 8, u32::MAX);
    wr(vm, t + 0xc, u32::MAX);
    wr(vm, t + 0x14, 0);
    wr(vm, t + 0x18, 0);
    wr(vm, t + 0x1c, none);
    wr(vm, t + 0x20, u32::MAX);
    wr(vm, t + 0x24, 0);
    wr(vm, t + 0x28, 0);
    wr(vm, t + 0x40, 0);
    stfs(vm, t + 0x44, c0);
    stfs(vm, t + 0x48, c0);
    stfs(vm, t + 0x4c, c0);
    wr(vm, t + 0x50, 0);
    stfs(vm, t + 0x80, c0);
    stfs(vm, t + 0x84, c0);
    stfs(vm, t + 0x88, c0);
    wr(vm, t + 0x8c, 2);
    wr(vm, t + 0x90, 0);
    wb(vm, t + 0x94, 0);
    wb(vm, t + 0x95, 0);
    wb(vm, t + 0x96, 0);
    wb(vm, t + 0x97, 0);
    wr(vm, t + 0x98, 3);
    wr(vm, t + 0x9c, 0);
    wr(vm, t + 0xa0, 0);
    wr(vm, t + 0xa4, 0);
    wr(vm, t + 0xa8, 0);
    wb(vm, t + 0xac, 0);
    wr(vm, t + 0xb0, 0);
    wr(vm, t + 0xb4, 0);
    wr(vm, t + 0xb8, 0);
    Ok(())
}

/// MicroBugHuntManager::~MicroBugHuntManager() @0x8032f050
pub fn dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flag = vm.a(1);
    let sp = fsp(vm, 0x10);
    if t != 0 {
        if rb(vm, t + 0x94) != 0 {
            cl(h, vm, sp, "UnInitialize__19MicroBugHuntManagerFv", &[t])?;
        }
        if (flag as i32) > 0 {
            // r4 is still the delete flag, or what UnInitialize left in it
            cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

/// MicroBugHuntManager::Initialize() @0x8032f0b0
pub fn initialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    cl(h, vm, sp, "LoadAssets__19MicroBugHuntManagerFv", &[t])?;
    let audio = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "LoadData__5AudioF9AUDIODATA", &[audio, 0x11])?;
    wb(vm, t + 0x94, 1);
    wb(vm, t + 0xac, 0);
    wr(vm, t + 0xb0, 0);
    vm.ret(1);
    Ok(())
}

/// MicroBugHuntManager::UnInitialize() @0x8032f0fc
pub fn uninitialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x20);
    if rb(vm, t + 0x94) != 0 {
        cl(h, vm, sp, "FreeAssets__19MicroBugHuntManagerFv", &[t])?;
        let audio = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
        cl(h, vm, sp, "UnloadData__5AudioF9AUDIODATA", &[audio, 0x11])?;
        let fx = rd(vm, t + 0x1c);
        wb(vm, t + 0x94, 0);
        wr(vm, sp + 8, fx);
        let mgr = lwz13(vm, -0x1ed8);
        cl(h, vm, sp, "DestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0])?;
        let none = lwz13(vm, -0x2278);
        wr(vm, t + 0x1c, none);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// area / start / stop
// ---------------------------------------------------------------------------------------------------------------------

/// The Bug Hunt database collection name for an area (2 = nature, 3 = park), as an offset into the string table.
fn area_collection(area: u32) -> Option<u32> {
    match area as i32 {
        3 => Some(STR + 0x6c),
        2 => Some(STR + 0x27),
        _ => None,
    }
}

/// MicroBugHuntManager::ChangeArea(Enums::AreaType) @0x8032f168
pub fn change_area(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let area = vm.a(1);
    let sp = fsp(vm, 0x20);
    if rd(vm, t + 0x20) == area {
        return Ok(());
    }
    wr(vm, t + 0x20, area);
    let mut found = false;
    if let Some(name) = area_collection(area) {
        let db = lwz13(vm, -0x260c);
        let coll = cl(h, vm, sp, "GetCollection__11pgIDatabaseFPCcPCc", &[db, STR + 0x15, name])?;
        cl(h, vm, sp, "GetVector3FromArray__14pgDBCollectionFPCcUiR9rmVector3", &[coll, STR + 0x33, 0, t + 0x30])?;
        let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0x41, 0])?;
        wr(vm, t + 0x24, v);
        let n = cl(h, vm, sp, "GetArrayCount__14pgDBCollectionFPCc", &[coll, STR + 0x51])?;
        wr(vm, t + 0xa4, n);
        let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0x5f, 0])?;
        wr(vm, t + 0xa8, v);
        let db = lwz13(vm, -0x260c);
        cl(h, vm, sp, "DestroyCollection__11pgIDatabaseFP14pgDBCollection", &[db, coll])?;
        found = true;
    }
    if found {
        cl(h, vm, sp, "StartIdleParticle__19MicroBugHuntManagerFv", &[t])?;
    } else {
        cl(h, vm, sp, "StopIdleParticle__19MicroBugHuntManagerFv", &[t])?;
    }
    Ok(())
}

/// MicroBugHuntManager::StartUp(Character*) @0x8032f2c4
pub fn start_up(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let chr = vm.a(1);
    let sp = fsp(vm, 0x20);
    if rb(vm, t + 0x95) != 0 {
        return Ok(());
    }
    wr(vm, t, chr);
    let mv = rd(vm, chr + 0x10);
    cl(h, vm, sp, "StopMovement__17CharacterMovementFv", &[mv])?;
    let c = rd(vm, t);
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x60, c + 0x180])?;
    let c = rd(vm, t);
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x70, c + 0x1a0])?;
    let c = rd(vm, t);
    let anim = rd(vm, c + 0x18);
    let markers = rd(vm, anim + 0x1670);
    let mut i: u32 = 0;
    while (i as i32) < (markers as i32) {
        let id = cl(h, vm, sp, "GetMarkerID__14AnimationStateCFi", &[anim, i])?;
        if id == 0xf {
            wr(vm, t + 4, i);
        }
        i = i.wrapping_add(1);
    }
    // the manager of the bugs (0xd0 bytes)
    let mem = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0xd0, 0, 0, STR + 0x7a])?;
    let mgr = if mem != 0 { cl(h, vm, sp, "__ct__15MicroBugManagerFv", &[mem])? } else { mem };
    wr(vm, t + 0xa0, mgr);
    // LoadData / LoadBugs run with r30 = the string table base in the original
    let r30 = vm.st.cpu.r[30];
    vm.st.cpu.r[30] = STR;
    let r = cl(h, vm, sp, "LoadData__19MicroBugHuntManagerFv", &[t]).and_then(|_| cl(h, vm, sp, "LoadBugs__19MicroBugHuntManagerFv", &[t]));
    vm.st.cpu.r[30] = r30;
    r?;
    wb(vm, t + 0x96, 0);
    let c = rd(vm, t);
    wr(vm, t + 0x8c, 2);
    wb(vm, t + 0x97, 0);
    wr(vm, t + 0x90, 0);
    let mv = rd(vm, c + 0x10);
    cl(h, vm, sp, "SetMappedMovmentState__17CharacterMovementFQ219AnimationStateGraph10AnimStatesQ219AnimationStateGraph10AnimStates", &[mv, 0, 0x61])?;
    cl(h, vm, sp, "SetMappedMovmentState__17CharacterMovementFQ219AnimationStateGraph10AnimStatesQ219AnimationStateGraph10AnimStates", &[mv, 1, 0x62])?;
    cl(h, vm, sp, "SetMappedMovmentState__17CharacterMovementFQ219AnimationStateGraph10AnimStatesQ219AnimationStateGraph10AnimStates", &[mv, 5, 0x62])?;
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "RegisterCallback__Q32EA5Conga15CallbackManagerFPCcPFPCQ32EA5Conga11CongaOutputPv_vPv", &[cb, STR + 0x9f, SWIPE_CB, t])?;
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "RegisterCallback__Q32EA5Conga15CallbackManagerFPCcPFPCQ32EA5Conga11CongaOutputPv_vPv", &[cb, STR + 0xaf, SWIPE_REV_CB, t])?;
    let secs = rd(vm, t + 0x24);
    wr(vm, t + 0x28, secs.wrapping_mul(1000));
    cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[1])?;
    cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[1])?;
    cl(h, vm, sp, "Counter_SetVisible__16WorldHudHandlersFi", &[1])?;
    let ms = rd(vm, t + 0x28);
    cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[ms])?;
    let goal = rd(vm, t + 0xa8);
    cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, goal])?;
    wr(vm, t + 0x98, 0);
    wb(vm, t + 0x95, 1);
    wr(vm, t + 0x9c, 0);
    cl(h, vm, sp, "StopIdleParticle__19MicroBugHuntManagerFv", &[t])?;
    Ok(())
}

/// MicroBugHuntManager::ShutDown() @0x8032f470
pub fn shut_down(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if rb(vm, t + 0x95) != 0 {
        cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[0])?;
        cl(h, vm, sp, "Counter_SetVisible__16WorldHudHandlersFi", &[0])?;
        cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[2])?;
        let c = rd(vm, t);
        wr(vm, c + 0x13c, 0);
        let c = rd(vm, t);
        let anim = rd(vm, c + 0x18);
        cl(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi", &[anim, 0x61, 0, u32::MAX])?;
        let c = rd(vm, t);
        let mv = rd(vm, c + 0x10);
        cl(h, vm, sp, "ResetMappedMovmentStates__17CharacterMovementFv", &[mv])?;
        let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
        cl(h, vm, sp, "Purge__Q32EA5Conga15CallbackManagerFv", &[cb])?;
        let mgr = rd(vm, t + 0xa0);
        if mgr != 0 {
            cl(h, vm, sp, "__dt__15MicroBugManagerFv", &[mgr, 1])?;
            wr(vm, t + 0xa0, 0);
        }
        wr(vm, t, 0);
        wb(vm, t + 0x95, 0);
        cl(h, vm, sp, "StartIdleParticle__19MicroBugHuntManagerFv", &[t])?;
    }
    Ok(())
}

/// MicroBugHuntManager::Restart() @0x8032f528
pub fn restart(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x30);
    if rb(vm, t + 0x95) != 0 {
        let mgr = rd(vm, t + 0xa0);
        cl(h, vm, sp, "RemoveAllBugs__15MicroBugManagerFv", &[mgr])?;
        let mgr = rd(vm, t + 0xa0);
        cl(h, vm, sp, "UnInitialize__15MicroBugManagerFv", &[mgr])?;
        cl(h, vm, sp, "LoadBugs__19MicroBugHuntManagerFv", &[t])?;
        let c = rd(vm, t);
        let mv = rd(vm, c + 0x10);
        cl(h, vm, sp, "StopMovement__17CharacterMovementFv", &[mv])?;
        let c = rd(vm, t);
        cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[c + 0x180, t + 0x60])?;
        wb(vm, c + 0x190, 1);
        let c = rd(vm, t);
        cl(h, vm, sp, "Set__7rmAngleFPC9rmVector3", &[c + 0x1b0, t + 0x70])?;
        cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x10, c + 0x1b0])?;
        cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[c + 0x1a0, sp + 0x10])?;
        let c = rd(vm, t);
        wr(vm, c + 0x13c, 0);
        let c = rd(vm, t);
        let anim = rd(vm, c + 0x18);
        cl(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi", &[anim, 0x61, 0, u32::MAX])?;
        let secs = rd(vm, t + 0x24);
        wb(vm, t + 0x96, 0);
        wr(vm, t + 0x8c, 2);
        wb(vm, t + 0x97, 0);
        wr(vm, t + 0x90, 0);
        wr(vm, t + 0x28, secs.wrapping_mul(1000));
        cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[1])?;
        cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[1])?;
        cl(h, vm, sp, "Counter_SetVisible__16WorldHudHandlersFi", &[1])?;
        let ms = rd(vm, t + 0x28);
        cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[ms])?;
        let goal = rd(vm, t + 0xa8);
        cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, goal])?;
        wb(vm, t + 0x95, 1);
        wr(vm, t + 0x98, 0);
        wr(vm, t + 0x9c, 0);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// data / assets
// ---------------------------------------------------------------------------------------------------------------------

/// The database collection chosen by the current area; `prev` is what the original's r30 holds when no area matches
/// (the register is never initialised on that path).
fn area_db_collection(h: &mut MgHost, vm: &mut V, sp: u32, t: u32, prev: u32) -> Result<u32, String> {
    let area = rd(vm, t + 0x20);
    match area_collection(area) {
        Some(name) => {
            let db = lwz13(vm, -0x260c);
            cl(h, vm, sp, "GetCollection__11pgIDatabaseFPCcPCc", &[db, STR + 0x15, name])
        }
        None => Ok(prev),
    }
}

/// MicroBugHuntManager::LoadData() @0x8032f654
pub fn load_data(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let r30 = vm.st.cpu.r[30];
    let sp = fsp(vm, 0x20);
    let coll = area_db_collection(h, vm, sp, t, r30)?;
    let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0x41, 0])?;
    wr(vm, t + 0x24, v);
    let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0xc6, 0])?;
    wr(vm, t + 0x40, v);
    cl(h, vm, sp, "GetFloatFromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0xd6, 0])?;
    let f1 = vm.st.cpu.f[1];
    stfs(vm, t + 0x44, f1);
    let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0xe4, 0])?;
    wr(vm, t + 0x50, v);
    cl(h, vm, sp, "GetFloatFromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0xf0, 0])?;
    let f1 = vm.st.cpu.f[1];
    stfs(vm, t + 0x54, f1);
    let id = rd(vm, t + 0x40);
    let f1 = lfs(vm, t + 0x44);
    gc(h, vm, sp, "SetBugData__8MicroBugFif", &[id], &[f1])?;
    let db = lwz13(vm, -0x260c);
    cl(h, vm, sp, "DestroyCollection__11pgIDatabaseFP14pgDBCollection", &[db, coll])?;
    Ok(())
}

/// MicroBugHuntManager::LoadBugs() @0x8032f750
pub fn load_bugs(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let r30 = vm.st.cpu.r[30];
    let sp = dsp(vm, 0x30);
    let coll = area_db_collection(h, vm, sp, t, r30)?;
    let n = cl(h, vm, sp, "GetArrayCount__14pgDBCollectionFPCc", &[coll, STR + 0x51])?;
    wr(vm, t + 0xa4, n);
    let mut i: u32 = 0;
    loop {
        let count = rd(vm, t + 0xa4);
        if !((i as i32) < (count as i32)) {
            break;
        }
        cl(h, vm, sp, "GetVector3FromArray__14pgDBCollectionFPCcUiR9rmVector3", &[coll, STR + 0x51, i, sp + 0x10])?;
        let mgr = rd(vm, t + 0xa0);
        cl(h, vm, sp, "AddBug__15MicroBugManagerFRC9rmVector3", &[mgr, sp + 0x10])?;
        i = i.wrapping_add(1);
    }
    let db = lwz13(vm, -0x260c);
    cl(h, vm, sp, "DestroyCollection__11pgIDatabaseFP14pgDBCollection", &[db, coll])?;
    let mgr = rd(vm, t + 0xa0);
    cl(h, vm, sp, "Initialize__15MicroBugManagerFv", &[mgr])?;
    Ok(())
}

/// MicroBugHuntManager::LoadAssets() @0x8032f83c
pub fn load_assets(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x20);
    let s1 = ad13(vm, -0x4a6c);
    cl(h, vm, sp, "__ct__7CStringFPCc", &[sp + 8, s1])?;
    let s2 = ad13(vm, -0x4a68);
    cl(h, vm, sp, "__apl__7CStringFPCc", &[sp + 8, s2])?;
    let am = lwz13(vm, -0x1ca4);
    let cstr = cl(h, vm, sp, "c_str__7CStringCFv", &[sp + 8])?;
    let tex = cl(h, vm, sp, "GetLoadedTexture__12AssetManagerFPCcPi", &[am, cstr, t + 8])?;
    let am = lwz13(vm, -0x1ca4);
    let name = ad13(vm, -0x4a63);
    let model = cl(h, vm, sp, "GetLoadedModel__12AssetManagerFPCcPii", &[am, name, t + 0xc, 0])?;
    cl(h, vm, sp, "SetTextures__Q24EAGL5ModelFPCc", &[model, tex])?;
    let am = lwz13(vm, -0x1ca4);
    let shadow_model = cl(h, vm, sp, "GetLoadedModel__12AssetManagerFPCcPii", &[am, name, t + 0x10, 0])?;
    // the net shadow render entity (0x50 bytes)
    let pool = lwz13(vm, -0x43fc);
    let mem = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0x50, pool, 0, 0x804d_8af1])?;
    let ent = if mem != 0 { cl(h, vm, sp, "__ct__29MicroBugNetShadowRenderEntityFPQ24EAGL5Model", &[mem, shadow_model])? } else { mem };
    wr(vm, t + 0x18, ent);
    let scene = lwz13(vm, -0x1cfc);
    let scene = rd(vm, scene + 8);
    cl(h, vm, sp, "AddEntity__Q23Ren5SceneFiPQ23Ren6Entity", &[scene, 0, ent])?;
    // the cached model (0x4c bytes)
    let pool = lwz13(vm, -0x43fc);
    let mem = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0x4c, pool, 0, 0x804c_c724])?;
    let cm = if mem != 0 { cl(h, vm, sp, "__ct__Q23Ren11CachedModelFv", &[mem])? } else { mem };
    wr(vm, t + 0x14, cm);
    wr(vm, cm + 0x44, model);
    let r = cl(h, vm, sp, "SetScaleMatrix__Q23Ren11CachedModelFv", &[cm])?;
    cl(h, vm, sp, "LoadAssets__15MicroBugManagerFv", &[r])?;
    cl(h, vm, sp, "__dt__7CStringFv", &[sp + 8, u32::MAX])?;
    Ok(())
}

/// MicroBugHuntManager::FreeAssets() @0x8032f95c
pub fn free_assets(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    cl(h, vm, sp, "UnloadAssets__15MicroBugManagerFv", &[t])?;
    let cm = rd(vm, t + 0x14);
    if cm != 0 {
        cl(h, vm, sp, "__dt__Q23Ren11CachedModelFv", &[cm, 1])?;
        wr(vm, t + 0x14, 0);
    }
    let scene = lwz13(vm, -0x1cfc);
    let ent = rd(vm, t + 0x18);
    let scene = rd(vm, scene + 8);
    cl(h, vm, sp, "RemoveEntity__Q23Ren5SceneFiPQ23Ren6Entity", &[scene, 0, ent])?;
    let ent = rd(vm, t + 0x18);
    if ent != 0 {
        // virtual destructor (vtable slot 2) with the delete flag
        let vt = rd(vm, ent);
        let f = rd(vm, vt + 8);
        gca(h, vm, sp, f, &[ent, 1], &[])?;
        wr(vm, t + 0x18, 0);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// per-frame update / draw
// ---------------------------------------------------------------------------------------------------------------------

/// The end of Update shared by the playing and the not-yet-started paths: the end-screen timer at +0x98 / +0x9c.
fn update_tail(vm: &mut V, t: u32, dt: u32) {
    let state = rd(vm, t + 0x98) as i32;
    if state >= 3 || state < 0 {
        return;
    }
    let v = rd(vm, t + 0x9c) as i32;
    if v > 0x7d0 {
        wr(vm, t + 0x98, 3);
    }
    let v = rd(vm, t + 0x9c);
    wr(vm, t + 0x9c, v.wrapping_add(dt));
}

/// MicroBugHuntManager::Update(int) @0x8032f9e4
pub fn update(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = dsp(vm, 0x60);
    if rb(vm, t + 0xac) != 0 {
        return Ok(());
    }
    if rb(vm, t + 0x95) == 0 {
        // not started yet: the idle animation of the character drifts with the frame time
        let magic = lfd2(vm, -0x4890);
        let f4 = lfs2(vm, -0x4898);
        let f3 = lfs2(vm, -0x4894);
        let as_double = f64::from_bits(0x4330_0000_0000_0000 | (dt ^ 0x8000_0000) as u64);
        let f5 = fsubs(as_double, magic);
        let f2 = lfs(vm, t + 0x48);
        let f1 = lfs2(vm, -0x48a0);
        let f0 = lfs(vm, t + 0x4c);
        let f4 = fdivs(f5, f4);
        let f2 = fmadds(f3, f4, f2);
        let f0 = fmadds(f1, f4, f0);
        stfs(vm, t + 0x48, f2);
        stfs(vm, t + 0x4c, f0);
        update_tail(vm, t, dt);
        return Ok(());
    }
    let elapsed = rd(vm, t + 0x90);
    let countdown = rd(vm, t + 0xb0);
    wr(vm, t + 0x90, elapsed.wrapping_add(dt));
    if (countdown as i32) > 0 {
        let left = countdown.wrapping_sub(1);
        wr(vm, t + 0xb0, left);
        if left == 0 {
            cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[1])?;
            cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[1])?;
            cl(h, vm, sp, "Counter_SetVisible__16WorldHudHandlersFi", &[1])?;
            let ms = rd(vm, t + 0x28);
            cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[ms])?;
            let mgr = rd(vm, t + 0xa0);
            let total = rd(vm, t + 0xa4);
            let caught = rd(vm, mgr + 0xcc);
            let goal = rd(vm, t + 0xa8);
            cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[total.wrapping_sub(caught), goal])?;
        }
    }
    let mgr = rd(vm, t + 0xa0);
    let goal = rd(vm, t + 0xa8);
    let total = rd(vm, t + 0xa4);
    let caught = rd(vm, mgr + 0xcc);
    let needed = total.wrapping_sub(goal);
    if (caught as i32) <= (needed as i32) {
        // enough bugs: the player wins
        cl(h, vm, sp, "WinLose_SetVisible__16WorldHudHandlersFii", &[0, 1])?;
        let area = rd(vm, t + 0x20);
        wr(vm, t + 0x98, 1);
        wr(vm, t + 0x9c, 0);
        match area as i32 {
            3 => {
                let p = lwz13(vm, -0x1e7c);
                let prof = cl(h, vm, sp, "GetProfile__16CharacterProfileFv", &[p])?;
                if rb(vm, prof + 0xcba) == 0 {
                    let marbles = rd(vm, t + 0x50);
                    cl(h, vm, sp, "AddMarblesEvent__8WorldManFii", &[WORLD_MAN, 1, marbles])?;
                    let p = lwz13(vm, -0x1e7c);
                    cl(h, vm, sp, "SetBugHuntBeatPark__16CharacterProfileFb", &[p, 1])?;
                }
            }
            2 => {
                let p = lwz13(vm, -0x1e7c);
                let prof = cl(h, vm, sp, "GetProfile__16CharacterProfileFv", &[p])?;
                if rb(vm, prof + 0xcb9) == 0 {
                    let marbles = rd(vm, t + 0x50);
                    cl(h, vm, sp, "AddMarblesEvent__8WorldManFii", &[WORLD_MAN, 1, marbles])?;
                    let p = lwz13(vm, -0x1e7c);
                    cl(h, vm, sp, "SetBugHuntBeatNature__16CharacterProfileFb", &[p, 1])?;
                }
            }
            _ => {}
        }
        cl(h, vm, sp, "ShutDown__19MicroBugHuntManagerFv", &[t])?;
        return Ok(());
    }
    if (rd(vm, t + 0x28) as i32) <= 0 {
        // out of time: the player loses
        cl(h, vm, sp, "WinLose_SetVisible__16WorldHudHandlersFii", &[1, 1])?;
        wr(vm, t + 0x98, 2);
        wr(vm, t + 0x9c, 0);
        cl(h, vm, sp, "ShutDown__19MicroBugHuntManagerFv", &[t])?;
        return Ok(());
    }
    if rb(vm, t + 0x96) != 0 {
        // a swing is under way
        let swing = rd(vm, t + 0x90) as i32;
        if swing > 0x50 && swing < 0xbe {
            wb(vm, t + 0x97, 1);
        }
        if rb(vm, t + 0x97) != 0 && (dt as i32) > 0 {
            cl(h, vm, sp, "__ct__7rmAngleFv", &[sp + 0x30])?;
            cl(h, vm, sp, "__ct__7rmAngleFv", &[sp + 0x34])?;
            cl(h, vm, sp, "UpdateSwingInfo__19MicroBugHuntManagerFiR21MicroBugHuntSwingInfo", &[t, dt, sp + 0x20])?;
            let a1 = rd(vm, sp + 0x34);
            let a0 = rd(vm, sp + 0x30);
            wr(vm, sp + 0x10, a1);
            let f1 = lfs(vm, sp + 0x38);
            wr(vm, sp + 0x14, a0);
            let f2 = lfs(vm, sp + 0x3c);
            let hit = gc(h, vm, sp, "DoSwingHitDetection__19MicroBugHuntManagerFRC9rmVector37rmAngle7rmAngleff", &[t, sp + 0x20, sp + 0x14, sp + 0x10], &[f1, f2])?;
            if hit != 0 {
                let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
                cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x77, 0, 0x64])?;
                let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
                cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x78, 0, 0x64])?;
                if (rd(vm, t + 0xb4) as i32) <= 0 {
                    let r = cl(h, vm, sp, "rmRandRange__Fii", &[0, 0x63])?;
                    if (r as i32) < 0x3c {
                        let pc = cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])?;
                        let v = rd(vm, pc + 0x1e8);
                        let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
                        let sfx = if v == 0 { 0xd } else { 0xf };
                        cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, sfx, 0, 0x64])?;
                    }
                    wr(vm, t + 0xb4, 0x7d0);
                }
                let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
                cl(h, vm, sp, "PlaySFX__5AudioFi15AUDIOWIIMOTESFXi", &[a, 0, 5, 0x1000])?;
            }
        }
        if rd(vm, t + 0x8c) == 1 {
            // the character's controller-model object (+0x20c) is told the swing speed: virtual call, slot 5
            let c = rd(vm, t);
            let obj = rd(vm, c + 0x20c);
            let f1 = lfs(vm, c + 0x148);
            let vt = rd(vm, obj);
            let f = rd(vm, vt + 0x14);
            gca(h, vm, sp, f, &[obj], &[f1])?;
        }
        let kind = rd(vm, t + 0x8c);
        let swing = rd(vm, t + 0x90) as i32;
        if (kind == 1 || kind == 0) && swing > 0x258 {
            // the swing is over
            let c = rd(vm, t);
            wb(vm, t + 0x96, 0);
            wr(vm, t + 0x90, 0);
            wr(vm, c + 0x13c, 0);
            let c = rd(vm, t);
            let anim = rd(vm, c + 0x18);
            cl(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi", &[anim, 0x61, 0, u32::MAX])?;
        }
    }
    let mgr = rd(vm, t + 0xa0);
    cl(h, vm, sp, "Update__15MicroBugManagerFi", &[mgr, dt])?;
    let ctl = cl(h, vm, sp, "Get__10ControllerFi", &[0])?;
    let ev = cl(h, vm, sp, "GetEventState__10ControllerCF12EActionEvent", &[ctl, 0xaf])?;
    if rb(vm, ev) != 0 {
        let o = rd(vm, WORLD_MAN + 0x8c);
        if rb(vm, o + 0x24) == 0 {
            cl(h, vm, sp, "OpenPauseMenu__19MicroBugHuntManagerFv", &[t])?;
        }
    }
    let ms = rd(vm, t + 0x28);
    let cool = rd(vm, t + 0xb4);
    wr(vm, t + 0x28, ms.wrapping_sub(dt));
    wr(vm, t + 0xb4, cool.wrapping_sub(dt));
    let o = rd(vm, WORLD_MAN + 0x8c);
    if rb(vm, o + 0x24) == 0 {
        let v = rd(vm, t + 0xb8);
        if (v as i32) > 0 {
            wr(vm, t + 0xb8, v.wrapping_sub(dt));
        }
    }
    update_tail(vm, t, dt);
    Ok(())
}

/// MicroBugHuntManager::UpdateSwingInfo(int, MicroBugHuntSwingInfo&) @0x8032fe64
pub fn update_swing_info(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let out = vm.a(2);
    let sp = fsp(vm, 0x30);
    let c = rd(vm, t);
    cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x10, c + 0x1a0])?;
    // yaw
    let f5 = lfs(vm, sp + 0x10);
    wr(vm, sp + 0x18, 0x4330_0000);
    let f4 = lfd2(vm, -0x4890);
    stfs(vm, sp + 0xc, f5);
    let f2 = lfs2(vm, -0x4898);
    let r0 = rd(vm, t + 0x90);
    let f1 = lfs(vm, t + 0x80);
    let r4 = r0.wrapping_sub(dt);
    let f0 = lfs(vm, t + 0x84);
    let r0 = r4.wrapping_sub(0x50) ^ 0x8000_0000;
    wr(vm, sp + 0x1c, r0);
    let f3 = lfd(vm, sp + 0x18);
    let f3 = fsubs(f3, f4);
    let f2 = fdivs(f3, f2);
    let f0 = fmadds(f1, f2, f0);
    let f0 = fadds(f5, f0);
    stfs(vm, sp + 0xc, f0);
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[sp + 0xc])?;
    let r0 = rd(vm, t + 0x90);
    if (r0 as i32) >= 0xbe {
        wr(vm, t + 0x90, 0xbe);
        wb(vm, t + 0x97, 0);
    }
    // pitch
    let f5 = lfs(vm, sp + 0x10);
    wr(vm, sp + 0x18, 0x4330_0000);
    let f4 = lfd2(vm, -0x4890);
    stfs(vm, sp + 8, f5);
    let f2 = lfs2(vm, -0x4898);
    let r4 = rd(vm, t + 0x90);
    let f1 = lfs(vm, t + 0x80);
    let r0 = r4.wrapping_sub(0x50);
    let f0 = lfs(vm, t + 0x84);
    let r0 = r0 ^ 0x8000_0000;
    wr(vm, sp + 0x1c, r0);
    let f3 = lfd(vm, sp + 0x18);
    let f3 = fsubs(f3, f4);
    let f2 = fdivs(f3, f2);
    let f0 = fmadds(f1, f2, f0);
    let f0 = fadds(f5, f0);
    stfs(vm, sp + 8, f0);
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[sp + 8])?;
    let c = rd(vm, t);
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[out, c + 0x180])?;
    let f0 = lfs(vm, sp + 0xc);
    stfs(vm, out + 0x10, f0);
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[out + 0x10])?;
    let f0 = lfs(vm, sp + 8);
    stfs(vm, out + 0x14, f0);
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[out + 0x14])?;
    let f1 = lfs2(vm, -0x4888);
    let f0 = lfs2(vm, -0x4884);
    stfs(vm, out + 0x18, f1);
    stfs(vm, out + 0x1c, f0);
    Ok(())
}

/// `Math::Matrix44::Set` through the engine's 16-float entry: the matrix reference, f1..f8, then eight singles in the
/// caller's parameter area at `sp + 8`.
const MATRIX44_SET: &str = "Set__Q22EA4MathFRQ32EA4Math8Matrix44ffffffffffffffff";

/// MicroBugHuntManager::Draw(Ren::SceneContext&) @0x8032ffac
pub fn draw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let ctx = vm.a(1);
    let sp = dsp(vm, 0x230);
    if rb(vm, t + 0x95) != 0 {
        // playing: refresh the HUD and draw the character's net
        let mgr = rd(vm, t + 0xa0);
        let total = rd(vm, t + 0xa4);
        let caught = rd(vm, mgr + 0xcc);
        let goal = rd(vm, t + 0xa8);
        cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[total.wrapping_sub(caught), goal])?;
        let ms = rd(vm, t + 0x28);
        cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[ms])?;
        let marker = rd(vm, t + 4);
        let c = rd(vm, t);
        let anim = rd(vm, c + 0x18);
        if marker != u32::MAX {
            let mm = cl(h, vm, sp, "GetMarkerMatrix__14AnimationStateCFi", &[anim, marker])?;
            cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[G_MATRIX, mm, sp + 0x1e0])?;
            let c = rd(vm, t);
            cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0x1e0, c + 0x20, sp + 0x1e0])?;
            let f2 = lfs(vm, sp + 0x210);
            let f1 = lfs(vm, sp + 0x214);
            let f0 = lfs(vm, sp + 0x218);
            stfs(vm, sp + 0x50, f2);
            stfs(vm, sp + 0x54, f1);
            stfs(vm, sp + 0x58, f0);
            let o = rd(vm, WORLD_MAN + 0x8c);
            let area = rd(vm, o + 8);
            cl(h, vm, sp, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[area, sp + 0x50, sp + 0x1a0])?;
            let z = lfs2(vm, -0x489c);
            stfs(vm, sp + 0x210, z);
            stfs(vm, sp + 0x214, z);
            stfs(vm, sp + 0x218, z);
            cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0x1e0, sp + 0x1a0, sp + 0x1e0])?;
            let cm = rd(vm, t + 0x14);
            cl(h, vm, sp, "Draw__Q23Ren11CachedModelFRC9rmMatrix4b", &[cm, sp + 0x1e0, 0])?;
            let ent = rd(vm, t + 0x18);
            cl(h, vm, sp, "SetModelMatrix__29MicroBugNetShadowRenderEntityFRC9rmMatrix4", &[ent, sp + 0x1e0])?;
            let ent = rd(vm, t + 0x18);
            wb(vm, ent + 0x4c, 1);
        }
        let mgr = rd(vm, t + 0xa0);
        if mgr != 0 {
            cl(h, vm, sp, "Draw__15MicroBugManagerFRQ23Ren12SceneContext", &[mgr, ctx])?;
        }
        return Ok(());
    }
    // not playing: the net is drawn in the area while the manager is idle in an area with bugs (areas 2 and 3)
    let area = rd(vm, t + 0x20);
    if area.wrapping_sub(2) > 1 {
        let ent = rd(vm, t + 0x18);
        wb(vm, ent + 0x4c, 0);
        return Ok(());
    }
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[sp + 0xa0])?;
    let o = rd(vm, WORLD_MAN + 0x8c);
    let am = rd(vm, o + 8);
    cl(h, vm, sp, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[am, t + 0x30, sp + 0x60])?;
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[sp + 0x160])?;
    // rotation about x
    let f1 = lfs2(vm, -0x4880);
    gc(h, vm, sp, "fSinCos__Q22EA4MathFfRfRf", &[sp + 0x3c, sp + 0x38], &[f1])?;
    let c0 = lfs2(vm, -0x489c);
    let c1 = lfs2(vm, -0x487c);
    stfs(vm, sp + 8, c0);
    stfs(vm, sp + 0xc, c0);
    stfs(vm, sp + 0x10, c1);
    stfs(vm, sp + 0x14, c0);
    stfs(vm, sp + 0x18, c0);
    stfs(vm, sp + 0x1c, c0);
    stfs(vm, sp + 0x20, c0);
    stfs(vm, sp + 0x24, c1);
    let a = lfs(vm, sp + 0x38);
    let b = lfs(vm, sp + 0x3c);
    gc(h, vm, sp, MATRIX44_SET, &[sp + 0x160], &[a, b, c0, c0, -b, a, c0, c0])?;
    // rotation about y
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[sp + 0x120])?;
    let f1 = lfs(vm, t + 0x48);
    gc(h, vm, sp, "fSinCos__Q22EA4MathFfRfRf", &[sp + 0x34, sp + 0x30], &[f1])?;
    let f0 = lfs(vm, sp + 0x34);
    let f2 = lfs2(vm, -0x489c);
    stfs(vm, sp + 8, f0);
    let f6 = lfs2(vm, -0x487c);
    stfs(vm, sp + 0xc, f2);
    let f0 = lfs(vm, sp + 0x30);
    stfs(vm, sp + 0x10, f0);
    stfs(vm, sp + 0x14, f2);
    stfs(vm, sp + 0x18, f2);
    stfs(vm, sp + 0x1c, f2);
    stfs(vm, sp + 0x20, f2);
    stfs(vm, sp + 0x24, f6);
    let f0 = lfs(vm, sp + 0x34);
    let f1 = lfs(vm, sp + 0x30);
    gc(h, vm, sp, MATRIX44_SET, &[sp + 0x120], &[f1, f2, -f0, f2, f2, f6, f2, f2])?;
    // squash and stretch along z
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[sp + 0xe0])?;
    let f1 = lfs2(vm, -0x4860);
    let f0 = lfs(vm, t + 0x4c);
    let f1 = fmuls(f1, f0);
    gc(h, vm, sp, "rmSin__Ff", &[], &[f1])?;
    let sin = vm.st.cpu.f[1];
    let d0 = lfd2(vm, -0x4868);
    let f2 = lfs2(vm, -0x489c);
    let f0 = d0 + sin;
    let f6 = lfd2(vm, -0x4878);
    stfs(vm, sp + 8, f2);
    let f1 = lfs2(vm, -0x487c);
    let f5 = f6 * f0;
    let d0 = lfd2(vm, -0x4870);
    stfs(vm, sp + 0xc, f2);
    stfs(vm, sp + 0x10, f1);
    let f0 = d0.mul_add(f5, f6);
    stfs(vm, sp + 0x14, f2);
    stfs(vm, sp + 0x18, f2);
    let f0 = fs(f0);
    stfs(vm, sp + 0x40, f2);
    stfs(vm, sp + 0x1c, f0);
    stfs(vm, sp + 0x20, f2);
    stfs(vm, sp + 0x44, f0);
    stfs(vm, sp + 0x48, f2);
    stfs(vm, sp + 0x24, f1);
    gc(h, vm, sp, MATRIX44_SET, &[sp + 0xe0], &[f1, f2, f2, f2, f2, f1, f2, f2])?;
    cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0xe0, sp + 0xa0, sp + 0xa0])?;
    cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0x120, sp + 0xa0, sp + 0xa0])?;
    cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0x160, sp + 0xa0, sp + 0xa0])?;
    cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0xa0, sp + 0x60, sp + 0x60])?;
    let cm = rd(vm, t + 0x14);
    cl(h, vm, sp, "Draw__Q23Ren11CachedModelFRC9rmMatrix4b", &[cm, sp + 0x60, 0])?;
    let ent = rd(vm, t + 0x18);
    cl(h, vm, sp, "SetModelMatrix__29MicroBugNetShadowRenderEntityFRC9rmMatrix4", &[ent, sp + 0x60])?;
    let ent = rd(vm, t + 0x18);
    wb(vm, ent + 0x4c, 1);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// swipe input
// ---------------------------------------------------------------------------------------------------------------------

/// MicroBugHuntManager::SwipeRegularCallback(const CongaOutput*, void*) @0x80330378
pub fn swipe_regular_callback(h: &mut MgHost, vm: &mut V) -> R {
    // tail call: SwipeRegularProcess(this = userdata, output)
    let out = vm.a(0);
    let this = vm.a(1);
    let sp = fsp(vm, 0);
    cl(h, vm, sp, "SwipeRegularProcess__19MicroBugHuntManagerFPCQ32EA5Conga11CongaOutput", &[this, out])?;
    Ok(())
}

/// MicroBugHuntManager::SwipeRegularReverseCallback(const CongaOutput*, void*) @0x80330388
pub fn swipe_regular_reverse_callback(h: &mut MgHost, vm: &mut V) -> R {
    let out = vm.a(0);
    let this = vm.a(1);
    let sp = fsp(vm, 0);
    cl(h, vm, sp, "SwipeRegularProcess__19MicroBugHuntManagerFPCQ32EA5Conga11CongaOutput", &[this, out])?;
    Ok(())
}

/// MicroBugHuntManager::SwipeRegularProcess(const CongaOutput*) @0x80330398
pub fn swipe_regular_process(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let out = vm.a(1);
    let sp = fsp(vm, 0x10);
    if rb(vm, t + 0x94) == 0 {
        return Ok(());
    }
    let c = rd(vm, t);
    if rb(vm, c + 0x12c) == 0 {
        return Ok(());
    }
    let talking = cl(h, vm, sp, "IsInConversation__14CharacterStateFv", &[c + 0x130])?;
    if talking != 0 {
        return Ok(());
    }
    let c = rd(vm, t);
    let device = rd(vm, out + 8);
    let owner = rd(vm, c + 0x124);
    if owner == 0 {
        return Ok(());
    }
    let id = rd(vm, owner + 0xa8);
    if device != id {
        return Ok(());
    }
    if rb(vm, t + 0x96) != 0 {
        return Ok(());
    }
    let p = rd(vm, c + 0x10);
    let f0 = lfs2(vm, -0x489c);
    let f1 = lfs(vm, p);
    let kind = if fcmp(f1, f0) == GT { 1 } else { 0 };
    cl(h, vm, sp, "StartSwing__19MicroBugHuntManagerFQ219MicroBugHuntManager9SwingType", &[t, kind])?;
    Ok(())
}

/// MicroBugHuntManager::StartSwing(SwingType) @0x80330454
pub fn start_swing(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let kind = vm.a(1);
    let sp = fsp(vm, 0x10);
    // swing 0 and 1 differ only in the animation state they start
    let anim_state = match kind as i32 {
        0 => Some(0x64),
        1 => Some(0x63),
        _ => None,
    };
    if let Some(state) = anim_state {
        let f3 = lfs2(vm, -0x485c);
        let f2 = lfs2(vm, -0x4858);
        wb(vm, t + 0x96, 1);
        let f1 = fsubs(f2, f3);
        let f0 = lfs2(vm, -0x4854);
        stfs(vm, t + 0x84, f3);
        let c = rd(vm, t);
        let f0 = fdivs(f1, f0);
        stfs(vm, t + 0x88, f2);
        wr(vm, t + 0x90, 0);
        stfs(vm, t + 0x80, f0);
        wr(vm, c + 0x13c, 0x258);
        let c = rd(vm, t);
        let anim = rd(vm, c + 0x18);
        cl(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi", &[anim, state, 0, u32::MAX])?;
        let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
        cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x76, 0, 0x64])?;
        let r = cl(h, vm, sp, "rmRandRange__Fii", &[0, 0x63])?;
        if (r as i32) < 0x28 {
            let pc = cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])?;
            let v = rd(vm, pc + 0x1e8);
            let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
            let sfx = if v == 0 { 7 } else { 8 };
            cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, sfx, 0, 0x64])?;
        }
        let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
        cl(h, vm, sp, "PlaySFX__5AudioFi15AUDIOWIIMOTESFXi", &[a, 0, 4, 0x1000])?;
    }
    wr(vm, t + 0x8c, kind);
    Ok(())
}

/// MicroBugHuntManager::DoSwingHitDetection(const rmVector3&, rmAngle, rmAngle, float, float) @0x80330658
pub fn do_swing_hit_detection(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let pos = vm.a(1);
    let pa = vm.a(2);
    let pb = vm.a(3);
    let f25 = vm.st.cpu.f[1];
    let f26 = vm.st.cpu.f[2];
    let sp = dsp(vm, 0x2a0);
    let mut result: u32 = 0;
    // the sector's two edge angles, widened by the same constant
    cl(h, vm, sp, "__ct__7rmAngleFv", &[sp + 0x54])?;
    let f1 = lfs(vm, pa);
    let k = lfs2(vm, -0x4850);
    let f0 = fsubs(f1, k);
    stfs(vm, sp + 0x54, f0);
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[sp + 0x54])?;
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[sp + 0x54])?;
    let w = rd(vm, sp + 0x54);
    wr(vm, sp + 0x60, w);
    let f0 = lfs(vm, sp + 0x60);
    stfs(vm, sp + 0x6c, f0);
    cl(h, vm, sp, "__ct__7rmAngleFv", &[sp + 0x50])?;
    let f1 = lfs(vm, pb);
    let k = lfs2(vm, -0x4850);
    let f0 = fadds(k, f1);
    stfs(vm, sp + 0x50, f0);
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[sp + 0x50])?;
    cl(h, vm, sp, "Wrap__7rmAngleFv", &[sp + 0x50])?;
    let w = rd(vm, sp + 0x50);
    wr(vm, sp + 0x5c, w);
    let f0 = lfs(vm, sp + 0x5c);
    stfs(vm, sp + 0x68, f0);
    cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x200, sp + 0x6c])?;
    cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x1f0, sp + 0x68])?;
    let mgr = rd(vm, t + 0xa0);
    let f29 = lfs2(vm, -0x487c);
    let n = rd(vm, mgr + 0xcc);
    let f30 = lfs2(vm, -0x489c);
    let f31 = lfs2(vm, -0x484c);
    let mut i: u32 = 0;
    while (i as i32) < (n as i32) {
        let mgr = rd(vm, t + 0xa0);
        let bug = cl(h, vm, sp, "GetBug__15MicroBugManagerCFi", &[mgr, i])?;
        let f28 = lfs(vm, t + 0x54);
        let bx = rd(vm, bug + 0x20);
        let by = rd(vm, bug + 0x24);
        wr(vm, sp + 0x1e4, by);
        let f0 = lfs(vm, sp + 0x1e4);
        wr(vm, sp + 0x1e0, bx);
        let f0 = fsubs(f0, f29);
        let bz = rd(vm, bug + 0x28);
        wr(vm, sp + 0x1e8, bz);
        stfs(vm, sp + 0x1e4, f0);
        // the bug's position relative to the net, and the four corners of the swept sector
        cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, pos, sp + 0x1d0])?;
        cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x64, sp + 0x1d0])?;
        cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x130, pa])?;
        cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x120, pb])?;
        gc(h, vm, sp, "rmScale__FRC9rmVector3fR9rmVector3", &[sp + 0x200, sp + 0x110], &[f28])?;
        gc(h, vm, sp, "rmScale__FRC9rmVector3fR9rmVector3", &[sp + 0x1f0, sp + 0x100], &[f28])?;
        let mut f27 = fsubs(f25, f28);
        if fcmp(f27, f30) == LT {
            f27 = f30;
        }
        gc(h, vm, sp, "rmScale__FRC9rmVector3fR9rmVector3", &[sp + 0x130, sp + 0x140], &[f27])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[pos, sp + 0x140, sp + 0x1c0])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1c0, sp + 0x110, sp + 0x1b0])?;
        let far = fadds(f26, f28);
        gc(h, vm, sp, "rmScale__FRC9rmVector3fR9rmVector3", &[sp + 0x130, sp + 0x140], &[far])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[pos, sp + 0x140, sp + 0x1a0])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1a0, sp + 0x110, sp + 0x190])?;
        gc(h, vm, sp, "rmScale__FRC9rmVector3fR9rmVector3", &[sp + 0x120, sp + 0x140], &[f27])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[pos, sp + 0x140, sp + 0x180])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x180, sp + 0x100, sp + 0x170])?;
        let far = fadds(f26, f28);
        gc(h, vm, sp, "rmScale__FRC9rmVector3fR9rmVector3", &[sp + 0x120, sp + 0x140], &[far])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[pos, sp + 0x140, sp + 0x160])?;
        cl(h, vm, sp, "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x160, sp + 0x100, sp + 0x150])?;

        // is the bug inside the quad?  Two chains of angle tests; the first one that holds up completely is a hit.
        let mut hit = false;
        cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x1c0, sp + 0xe0])?;
        cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x48, sp + 0xe0])?;
        let f1 = lfs(vm, sp + 0x48);
        let f0 = lfs(vm, pa);
        let f1 = fsubs(f0, f1);
        gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x4c], &[f1])?;
        let f0 = lfs(vm, sp + 0x4c);
        let mut second_chain = true;
        if fcmp(f0, f31) == LT {
            cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x1b0, sp + 0xd0])?;
            cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x40, sp + 0xd0])?;
            let f1 = lfs(vm, sp + 0x40);
            let f0 = lfs(vm, pa);
            let f1 = fsubs(f1, f0);
            gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x44], &[f1])?;
            let f0 = lfs(vm, sp + 0x44);
            if fcmp(f0, f31) == LT {
                cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x1c0, sp + 0xc0])?;
                cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x38, sp + 0xc0])?;
                let f1 = lfs(vm, sp + 0x38);
                let f0 = lfs(vm, sp + 0x6c);
                let f1 = fsubs(f1, f0);
                gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x3c], &[f1])?;
                let f0 = lfs(vm, sp + 0x3c);
                if fcmp(f0, f31) == LT {
                    cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x1a0, sp + 0xb0])?;
                    cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x30, sp + 0xb0])?;
                    let f1 = lfs(vm, sp + 0x30);
                    let f0 = lfs(vm, sp + 0x6c);
                    let f1 = fsubs(f0, f1);
                    gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x34], &[f1])?;
                    let f0 = lfs(vm, sp + 0x34);
                    if fcmp(f0, f31) == LT {
                        hit = true;
                        second_chain = false;
                    }
                }
            }
        }
        if second_chain {
            cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x180, sp + 0xa0])?;
            cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x28, sp + 0xa0])?;
            let f1 = lfs(vm, sp + 0x28);
            let f0 = lfs(vm, pb);
            let f1 = fsubs(f1, f0);
            gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x2c], &[f1])?;
            let f0 = lfs(vm, sp + 0x2c);
            if fcmp(f0, f31) == LT {
                cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x170, sp + 0x90])?;
                cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x20, sp + 0x90])?;
                let f1 = lfs(vm, sp + 0x20);
                let f0 = lfs(vm, pb);
                let f1 = fsubs(f0, f1);
                gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x24], &[f1])?;
                let f0 = lfs(vm, sp + 0x24);
                if fcmp(f0, f31) == LT {
                    cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x180, sp + 0x80])?;
                    cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x18, sp + 0x80])?;
                    let f1 = lfs(vm, sp + 0x18);
                    let f0 = lfs(vm, sp + 0x68);
                    let f1 = fsubs(f0, f1);
                    gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x1c], &[f1])?;
                    let f0 = lfs(vm, sp + 0x1c);
                    if fcmp(f0, f31) == LT {
                        cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[sp + 0x1e0, sp + 0x160, sp + 0x70])?;
                        cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x10, sp + 0x70])?;
                        let f1 = lfs(vm, sp + 0x10);
                        let f0 = lfs(vm, sp + 0x68);
                        let f1 = fsubs(f1, f0);
                        gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x14], &[f1])?;
                        let f0 = lfs(vm, sp + 0x14);
                        if fcmp(f0, f31) == LT {
                            hit = true;
                        }
                    }
                }
            }
        }

        if !hit {
            // fall back to the polar test: the bug's bearing inside [pa, pb) (wrapping) and its distance inside the ring
            let f1 = lfs(vm, pa);
            let f2 = lfs(vm, pb);
            let bearing = lfs(vm, sp + 0x64);
            let in_sector = if fcmp(f2, f1) == GT {
                fcmp(bearing, f2) == LT && (fcmp(bearing, f1) & GE_BITS) != 0
            } else {
                fcmp(bearing, f2) == LT || (fcmp(bearing, f1) & GE_BITS) != 0
            };
            if in_sector {
                let f0 = lfs(vm, sp + 0x1d4);
                let f4 = fadds(f26, f28);
                let f1 = lfs(vm, sp + 0x1d0);
                let f3 = fmuls(f27, f27);
                let f2 = fmuls(f0, f0);
                let f0 = lfs(vm, sp + 0x1d8);
                let f4 = fmuls(f4, f4);
                let f1 = fmadds(f1, f1, f2);
                let f0 = fmadds(f0, f0, f1);
                if (fcmp(f0, f3) & GE_BITS) != 0 && fcmp(f0, f4) == LT {
                    hit = true;
                }
            }
        }

        if hit {
            // catch the bug: spawn the effect slightly above it, let it play, remove the bug
            let bx = rd(vm, sp + 0x1e0);
            let by = rd(vm, sp + 0x1e4);
            wr(vm, sp + 0xf4, by);
            let bz = rd(vm, sp + 0x1e8);
            let f1 = lfs(vm, sp + 0xf4);
            let k = lfs2(vm, -0x487c);
            wr(vm, sp + 0xf0, bx);
            let f0 = fadds(f1, k);
            let fxm = lwz13(vm, -0x1ed8);
            wr(vm, sp + 0xf8, bz);
            let name = lwz13(vm, -0x4a70);
            stfs(vm, sp + 0xf4, f0);
            let fx = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[fxm, name, sp + 0xf0])?;
            wr(vm, sp + 0x58, fx);
            let fxm = lwz13(vm, -0x1ed8);
            cl(h, vm, sp, "DestroyPartFx__13PartFxManagerF4GUIDi", &[fxm, sp + 0x58, 0x3e8])?;
            let mgr = rd(vm, t + 0xa0);
            cl(h, vm, sp, "RemoveBug__15MicroBugManagerFi", &[mgr, i])?;
            result = 1;
            break;
        }
        i = i.wrapping_add(1);
    }
    vm.ret(result);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// idle particle / pause menu
// ---------------------------------------------------------------------------------------------------------------------

/// MicroBugHuntManager::StartIdleParticle() @0x80330c78
pub fn start_idle_particle(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x40);
    let guid = rd(vm, t + 0x1c);
    let mgr = lwz13(vm, -0x1ed8);
    wr(vm, sp + 0x14, guid);
    let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x14])?;
    let px = rd(vm, t + 0x30);
    let py = rd(vm, t + 0x34);
    let k = lfs2(vm, -0x4888);
    wr(vm, sp + 0x24, py);
    let f1 = lfs(vm, sp + 0x24);
    wr(vm, sp + 0x20, px);
    let f0 = fadds(f1, k);
    let pz = rd(vm, t + 0x38);
    wr(vm, sp + 0x28, pz);
    stfs(vm, sp + 0x24, f0);
    if fx != 0 {
        cl(h, vm, sp, "SetPos__6PartFxFRC9rmVector3", &[fx, sp + 0x20])?;
    } else {
        let mgr = lwz13(vm, -0x1ed8);
        let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8ab9, sp + 0x20])?;
        wr(vm, t + 0x1c, g);
        wr(vm, sp + 0x10, g);
        let mgr = lwz13(vm, -0x1ed8);
        cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x10])?;
    }
    Ok(())
}

/// MicroBugHuntManager::StopIdleParticle() @0x80330d28
pub fn stop_idle_particle(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x20);
    let guid = rd(vm, t + 0x1c);
    let mgr = lwz13(vm, -0x1ed8);
    wr(vm, sp + 8, guid);
    cl(h, vm, sp, "DestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0])?;
    let none = lwz13(vm, -0x2278);
    wr(vm, t + 0x1c, none);
    Ok(())
}

/// MicroBugHuntManager::OpenPauseMenu() @0x80330d70
pub fn open_pause_menu(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if (rd(vm, t + 0xb8) as i32) > 0 {
        return Ok(());
    }
    let o = rd(vm, WORLD_MAN + 0x8c);
    wb(vm, o + 0x24, 1);
    // Audio::Instance() is a static: it sees whatever r3 holds, here the object just written to
    vm.ret(o);
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "PlaySFX__5AudioF17AUDIOAEMSFEHUDSFXii", &[a, 0xc, 0, 0x64])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "Pause__5AudioFQ25Audio9PAUSEMODE", &[a, 2])?;
    let ctl = cl(h, vm, sp, "Get__10ControllerFi", &[0])?;
    cl(h, vm, sp, "SetCurrentControllerState__10ControllerF16EControllerState", &[ctl, 0])?;
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    wb(vm, fe + 0x48, 1);
    wb(vm, fe + 0x49, 1);
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    cl(h, vm, sp, "OpenAptOverlay__9FEManagerFPc", &[fe, 0x804d_8ac6])?;
    Ok(())
}

/// MicroBugHuntManager::ClosePauseMenu() @0x80330e00
pub fn close_pause_menu(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let o = rd(vm, WORLD_MAN + 0x8c);
    wb(vm, o + 0x24, 0);
    vm.ret(o);
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "UnPause__5AudioFv", &[a])?;
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    cl(h, vm, sp, "CloseAptOverlay__9FEManagerFv", &[fe])?;
    let ctl = cl(h, vm, sp, "Get__10ControllerFi", &[0])?;
    cl(h, vm, sp, "PopState__10ControllerFv", &[ctl])?;
    wr(vm, t + 0xb8, 0x4b0);
    Ok(())
}

/// MicroBugHuntManager::OnPauseMenuLoaded() @0x80330e60
pub fn on_pause_menu_loaded(_h: &mut MgHost, _vm: &mut V) -> R {
    Ok(())
}

/// MicroBugHuntManager::OnPauseContinue() @0x80330e64
pub fn on_pause_continue(h: &mut MgHost, vm: &mut V) -> R {
    // tail call: ClosePauseMenu()
    let t = vm.a(0);
    let sp = fsp(vm, 0);
    cl(h, vm, sp, "ClosePauseMenu__19MicroBugHuntManagerFv", &[t])?;
    Ok(())
}

/// MicroBugHuntManager::OnPauseQuit() @0x80330e68
pub fn on_pause_quit(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    cl(h, vm, sp, "ClosePauseMenu__19MicroBugHuntManagerFv", &[t])?;
    cl(h, vm, sp, "ShutDown__19MicroBugHuntManagerFv", &[t])?;
    Ok(())
}

/// MicroBugHuntManager::OnPauseReset() @0x80330e9c
pub fn on_pause_reset(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    cl(h, vm, sp, "ClosePauseMenu__19MicroBugHuntManagerFv", &[t])?;
    cl(h, vm, sp, "Restart__19MicroBugHuntManagerFv", &[t])?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// static initializer and the 16-float rmMatrix4 constructor
// ---------------------------------------------------------------------------------------------------------------------

/// __sinit_\microbughuntmanager_cpp() @0x80330fd0
pub fn sinit(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x30);
    let none = ad13(vm, -0x2278);
    cl(h, vm, sp, "__ct__4GUIDFUsUs", &[none, 0xffff, 0])?;
    let f1 = lfs2(vm, -0x487c);
    gc(h, vm, sp, "atan__3stdFf", &[], &[f1])?;
    let atan = vm.st.cpu.f[1];
    let f3 = lfs2(vm, -0x487c);
    let f2 = lfs2(vm, -0x489c);
    stfs(vm, sp + 8, f3);
    let f0 = lfs2(vm, -0x4848);
    stfs(vm, sp + 0xc, f2);
    let f12 = fmuls(f0, atan);
    let f0 = lfs2(vm, -0x4844);
    stfs(vm, sp + 0x10, f2);
    let f9 = lfs2(vm, -0x4840);
    stfs(vm, sp + 0x14, f2);
    let f6 = lfs2(vm, -0x4834);
    let f11 = fmuls(f0, f12);
    let f0 = lfs2(vm, -0x4830);
    let f10 = fmuls(f9, f12);
    stfs(vm, sp + 0x18, f6);
    let f6 = lfs2(vm, -0x483c);
    stfs(vm, sp + 0x1c, f0);
    let f9 = fmuls(f6, f12);
    let f0 = lfs2(vm, -0x482c);
    let g = ad13(vm, -0x2274);
    stfs(vm, g, f12);
    let f6 = lfs2(vm, -0x4838);
    stfs(vm, sp + 0x20, f0);
    stfs(vm, g + 4, f11);
    stfs(vm, g + 8, f10);
    stfs(vm, g + 0xc, f9);
    stfs(vm, sp + 0x24, f3);
    gc(h, vm, sp, "__ct__9rmMatrix4Fffffffffffffffff", &[G_MATRIX], &[f2, f2, f3, f2, f2, f6, f2, f2])?;
    Ok(())
}

/// rmMatrix4::rmMatrix4(16 floats) @0x80331094
pub fn rm_matrix4_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let r1 = vm.st.cpu.r[1];
    let sp = fsp(vm, 0x30);
    let fa = [
        vm.st.cpu.f[1],
        vm.st.cpu.f[2],
        vm.st.cpu.f[3],
        vm.st.cpu.f[4],
        vm.st.cpu.f[5],
        vm.st.cpu.f[6],
        vm.st.cpu.f[7],
        vm.st.cpu.f[8],
    ];
    // the last eight floats arrive in the caller's parameter area (r1 + 8..) and are passed on at sp + 8..
    for k in 0..8u32 {
        let v = lfs(vm, r1.wrapping_add(8 + 4 * k));
        stfs(vm, sp + 8 + 4 * k, v);
    }
    gc(h, vm, sp, "Set__Q32EA4Math8Matrix44Fffffffffffffffff", &[t], &fa)?;
    vm.ret(t);
    Ok(())
}

pub const PORTS: &[crate::mgvm::ports::Port] = &[
    ("__ct__19MicroBugHuntManagerFv", ctor),
    ("__dt__19MicroBugHuntManagerFv", dtor),
    ("Initialize__19MicroBugHuntManagerFv", initialize),
    ("UnInitialize__19MicroBugHuntManagerFv", uninitialize),
    ("ChangeArea__19MicroBugHuntManagerFQ25Enums8AreaType", change_area),
    ("StartUp__19MicroBugHuntManagerFP9Character", start_up),
    ("ShutDown__19MicroBugHuntManagerFv", shut_down),
    ("Restart__19MicroBugHuntManagerFv", restart),
    ("LoadData__19MicroBugHuntManagerFv", load_data),
    ("LoadBugs__19MicroBugHuntManagerFv", load_bugs),
    ("LoadAssets__19MicroBugHuntManagerFv", load_assets),
    ("FreeAssets__19MicroBugHuntManagerFv", free_assets),
    ("Update__19MicroBugHuntManagerFi", update),
    ("UpdateSwingInfo__19MicroBugHuntManagerFiR21MicroBugHuntSwingInfo", update_swing_info),
    ("Draw__19MicroBugHuntManagerFRQ23Ren12SceneContext", draw),
    ("SwipeRegularCallback__19MicroBugHuntManagerFPCQ32EA5Conga11CongaOutputPv", swipe_regular_callback),
    ("SwipeRegularReverseCallback__19MicroBugHuntManagerFPCQ32EA5Conga11CongaOutputPv", swipe_regular_reverse_callback),
    ("SwipeRegularProcess__19MicroBugHuntManagerFPCQ32EA5Conga11CongaOutput", swipe_regular_process),
    ("StartSwing__19MicroBugHuntManagerFQ219MicroBugHuntManager9SwingType", start_swing),
    ("DoSwingHitDetection__19MicroBugHuntManagerFRC9rmVector37rmAngle7rmAngleff", do_swing_hit_detection),
    ("StartIdleParticle__19MicroBugHuntManagerFv", start_idle_particle),
    ("StopIdleParticle__19MicroBugHuntManagerFv", stop_idle_particle),
    ("OpenPauseMenu__19MicroBugHuntManagerFv", open_pause_menu),
    ("ClosePauseMenu__19MicroBugHuntManagerFv", close_pause_menu),
    ("OnPauseMenuLoaded__19MicroBugHuntManagerFv", on_pause_menu_loaded),
    ("OnPauseContinue__19MicroBugHuntManagerFv", on_pause_continue),
    ("OnPauseQuit__19MicroBugHuntManagerFv", on_pause_quit),
    ("OnPauseReset__19MicroBugHuntManagerFv", on_pause_reset),
    ("__sinit_\\microbughuntmanager_cpp", sinit),
    ("__ct__9rmMatrix4Fffffffffffffffff", rm_matrix4_ctor),
];
