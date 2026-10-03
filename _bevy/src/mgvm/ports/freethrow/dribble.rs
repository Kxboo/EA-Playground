//! Free Throw / Dribbling / High Five (DribblingManager, FreeThrowBall, FreeThrowBallCourt, the collision listeners and
//! HighFiveManager), ported from the original PowerPC.
//!
//! Conventions used by every port in this file (as in `microbug::hunt`):
//!  * the original's stack frame is reproduced (same `sp`, same local offsets), so pointers to locals that are handed to
//!    other functions are the same addresses the original would pass;
//!  * every call the original makes is made here in the same order with the same arguments, from the same `sp`
//!    (see `gc`), so the callee's own frame and anything it passes on is identical too;
//!  * float arithmetic mirrors the interpreter (single precision ops round to f32, fmadds/fmsubs are fused).
#![allow(unused)]
use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
pub type R = Result<(), String>;

/// String table of the unit: `lis r31,-0x7fb2 ; addi r31,r31,-0x74b0`.
const STR: u32 = 0x804d_8b50;
/// The WorldMan singleton: `lis r3,-0x7fa1 ; addi r3,r3,-0x7ce0`.
const WORLD_MAN: u32 = 0x805e_8320;

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
/// `lfs f, off(r2)` (small data area 2 constants).
fn lfs2(vm: &mut V, off: i32) -> f64 {
    let a = vm.st.cpu.r[2].wrapping_add(off as u32);
    lfs(vm, a)
}
/// `lfd f, off(r2)`.
fn lfd2(vm: &mut V, off: i32) -> f64 {
    let a = vm.st.cpu.r[2].wrapping_add(off as u32);
    f64::from_bits(vm.st.mem.r64(a))
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
/// `lfs f, off(r13)`.
fn lfs13(vm: &mut V, off: i32) -> f64 {
    let a = ad13(vm, off);
    lfs(vm, a)
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
/// fmsubs: a*c - b, fused, then rounded to single.
fn fmsubs(a: f64, c: f64, b: f64) -> f64 {
    fs(a.mul_add(c, -b))
}
/// fnmsubs: -(a*c - b), fused, then rounded to single.
fn fnmsubs(a: f64, c: f64, b: f64) -> f64 {
    fs(-(a.mul_add(c, -b)))
}
/// fnmadds: -(a*c + b), fused, then rounded to single.
fn fnmadds(a: f64, c: f64, b: f64) -> f64 {
    fs(-(a.mul_add(c, b)))
}
/// The `xoris 0x8000 / 0x43300000 / lfd / fsubs magic` int-to-single idiom.
fn itof(v: u32) -> f64 {
    fs((v as i32) as f64)
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
const EQ: u8 = 2;

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
/// The float result (f1) of the last call.
fn f1(vm: &V) -> f64 {
    vm.st.cpu.f[1]
}
/// A virtual call: slot `slot` (byte offset) of the vtable at `obj + 0`, no adjustment of `this`.
fn vcall(h: &mut MgHost, vm: &mut V, sp: u32, obj: u32, slot: u32, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
    let vt = rd(vm, obj);
    let f = rd(vm, vt + slot);
    gca(h, vm, sp, f, args, fargs)
}
/// WorldMan::GetPlayerCharacter(0) (`lis r3,-0x7fa1 ; addi r3,r3,-0x7ce0 ; li r4,0`).
fn player(h: &mut MgHost, vm: &mut V, sp: u32) -> Result<u32, String> {
    cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])
}
/// Audio::Instance().
fn audio(h: &mut MgHost, vm: &mut V, sp: u32) -> Result<u32, String> {
    cl(h, vm, sp, "Instance__5AudioFv", &[])
}
/// Controller::Get(i).
fn ctl(h: &mut MgHost, vm: &mut V, sp: u32, i: u32) -> Result<u32, String> {
    cl(h, vm, sp, "Get__10ControllerFi", &[i])
}
/// `Vector3::Set(dst, x, y, z)` with the three float registers.
fn vset(h: &mut MgHost, vm: &mut V, sp: u32, dst: u32, x: f64, y: f64, z: f64) -> Result<u32, String> {
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[dst], &[x, y, z])
}
/// The PhysicsManager of the world: `WorldMan + 0x8c`, then `+0x20`.
fn phys_mgr(vm: &mut V) -> u32 {
    let a = rd(vm, WORLD_MAN + 0x8c);
    rd(vm, a + 0x20)
}

// =====================================================================================================================
// DribblingManager
// =====================================================================================================================

/// DribblingManager::DribblingManager() @0x80331628
pub fn dm_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x20);
    let c0 = lfs2(vm, -0x4828);
    wr(vm, t, u32::MAX);
    wr(vm, t + 0x20, 0);
    wr(vm, t + 0x24, 0);
    wr(vm, t + 0x28, 0);
    wr(vm, t + 0x2c, 0);
    wr(vm, t + 0x30, 0);
    wb(vm, t + 0x34, 0);
    wr(vm, t + 0x38, u32::MAX);
    wb(vm, t + 0x3c, 0);
    wr(vm, t + 0x40, u32::MAX);
    wr(vm, t + 0x44, u32::MAX);
    wr(vm, t + 0x48, u32::MAX);
    stfs(vm, t + 0x50, c0);
    stfs(vm, t + 0x54, c0);
    stfs(vm, t + 0x58, c0);
    for o in [0xe0, 0xe4, 0xe8, 0xec, 0xf0, 0xf4] {
        wr(vm, t + o, 0);
    }
    wb(vm, t + 0xf8, 0);
    wb(vm, t + 0xf9, 0);
    stfs(vm, t + 0xfc, c0);
    wr(vm, t + 0x100, 0);
    wr(vm, t + 0x104, 0);
    wb(vm, t + 0x108, 0);
    let g = lwz13(vm, -0x2250);
    wr(vm, t + 0x10c, g);
    wb(vm, t + 0x110, 1);
    wr(vm, t + 0x114, 0);
    wr(vm, t + 0x118, 0);
    wr(vm, t + 0x130, 0);
    // CString at sp+8
    cl(h, vm, sp, "__ct__7CStringFPCc", &[sp + 8, STR])?;
    let sfx = ad13(vm, -0x4a50);
    cl(h, vm, sp, "__apl__7CStringFPCc", &[sp + 8, sfx])?;
    let am = lwz13(vm, -0x1ca4);
    let cs = cl(h, vm, sp, "c_str__7CStringCFv", &[sp + 8])?;
    let tex = cl(h, vm, sp, "GetLoadedTexture__12AssetManagerFPCcPi", &[am, cs, t + 0x44])?;
    let am = lwz13(vm, -0x1ca4);
    let model = cl(h, vm, sp, "GetLoadedModel__12AssetManagerFPCcPii", &[am, STR + 0xb, t + 0x40, 0])?;
    cl(h, vm, sp, "SetTextures__Q24EAGL5ModelFPCc", &[model, tex])?;
    let am = lwz13(vm, -0x1ca4);
    let model2 = cl(h, vm, sp, "GetLoadedModel__12AssetManagerFPCcPii", &[am, STR + 0x18, t + 0x48, 0])?;
    // the two cached models
    let mut p = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0x4c, 0, 0, STR + 0x2c])?;
    if p != 0 {
        p = cl(h, vm, sp, "__ct__Q23Ren11CachedModelFv", &[p])?;
    }
    wr(vm, t + 0x24, p);
    wr(vm, p + 0x44, model);
    cl(h, vm, sp, "SetScaleMatrix__Q23Ren11CachedModelFv", &[p])?;
    let mut p = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0x4c, 0, 0, STR + 0x4a])?;
    if p != 0 {
        p = cl(h, vm, sp, "__ct__Q23Ren11CachedModelFv", &[p])?;
    }
    wr(vm, t + 0x28, p);
    wr(vm, p + 0x44, model2);
    cl(h, vm, sp, "SetScaleMatrix__Q23Ren11CachedModelFv", &[p])?;
    // the shadow entity
    let mut p = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0x50, 0, 0, STR + 0x6e])?;
    if p != 0 {
        p = cl(h, vm, sp, "__ct__31DribblingBallShadowRenderEntityFPQ24EAGL5Model", &[p, model2])?;
    }
    wr(vm, t + 0x2c, p);
    let holder = lwz13(vm, -0x1cfc);
    let scene = rd(vm, holder + 8);
    cl(h, vm, sp, "AddEntity__Q23Ren5SceneFiPQ23Ren6Entity", &[scene, 0, p])?;
    let a = rd(vm, WORLD_MAN + 0x88);
    let a = rd(vm, a + 0x18);
    let tar = rd(vm, a + 0xe8);
    let r = cl(h, vm, sp, "GetTar__10TarManagerFPCc", &[tar, STR + 0x99])?;
    wr(vm, t + 0x114, r);
    let r = cl(h, vm, sp, "GetTar__10TarManagerFPCc", &[tar, STR + 0xa6])?;
    wr(vm, t + 0x118, r);
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[t + 0x60])?;
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[t + 0xa0])?;
    cl(h, vm, sp, "__dt__7CStringFv", &[sp + 8, u32::MAX])?;
    vm.ret(t);
    Ok(())
}

/// DribblingManager::StartUp() @0x8033184c
pub fn dm_startup(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x50);
    cl(h, vm, sp, "GetBallMatrix__16DribblingManagerCFP9rmMatrix4", &[t, sp + 8])?;
    cl(h, vm, sp, "__as__9rmMatrix4FRC9rmMatrix4", &[t + 0x60, sp + 8])?;
    let (x, y, z) = (lfs(vm, t + 0x90), lfs(vm, t + 0x94), lfs(vm, t + 0x98));
    vset(h, vm, sp, t + 0x50, x, y, z)?;
    let c = player(h, vm, sp)?;
    let mv = rd(vm, c + 0x10);
    let sm = "SetMappedMovmentState__17CharacterMovementFQ219AnimationStateGraph10AnimStatesQ219AnimationStateGraph10AnimStates";
    cl(h, vm, sp, sm, &[mv, 0, 0x2a])?;
    cl(h, vm, sp, sm, &[mv, 1, 0x2c])?;
    cl(h, vm, sp, sm, &[mv, 5, 0x2c])?;
    let r0 = rd(vm, t + 0xec);
    wr(vm, t + 0xe4, 0);
    wr(vm, t + 0xe8, r0);
    wr(vm, t + 0xf0, 0);
    wb(vm, t + 0xf8, 0);
    wb(vm, t + 0xf9, 0);
    wr(vm, t + 0x104, 0);
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "LoadData__5AudioF9AUDIODATA", &[a, 0x12])?;
    wr(vm, t + 0xe4, 0);
    wr(vm, t + 0xe0, 2);
    cl(h, vm, sp, "StartBeatGame__16DribblingManagerFv", &[t])?;
    cl(h, vm, sp, "StopIdleParticle__16DribblingManagerFv", &[t])?;
    Ok(())
}

/// DribblingManager::ShutDown() @0x80331930
pub fn dm_shutdown(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x20);
    if rd(vm, t + 0xe0) == 0 {
        if rb(vm, t + 0x110) == 0 {
            cl(h, vm, sp, "StopIdleParticle__16DribblingManagerFv", &[t])?;
        }
        return Ok(());
    }
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "UnloadData__5AudioF9AUDIODATA", &[a, 0x12])?;
    wb(vm, t + 0xf8, 0);
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "Purge__Q32EA5Conga15CallbackManagerFv", &[cb])?;
    let pm = phys_mgr(vm);
    if pm != 0 {
        if rb(vm, t + 0x34) != 0 {
            let body = rd(vm, t + 0x30);
            cl(h, vm, sp, "RemovePhysicsRigidBodyFromWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
            wb(vm, t + 0x34, 0);
        }
        let body = rd(vm, t + 0x30);
        cl(h, vm, sp, "DestroyPhysicsRigidBody__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
        let loaded = rb(vm, t + 0x3c);
        wr(vm, t + 0x30, 0);
        if loaded != 0 {
            let sys = rd(vm, t + 0x38);
            cl(h, vm, sp, "UnloadPhysics__14PhysicsManagerFib", &[pm, sys, 0])?;
            wb(vm, t + 0x3c, 0);
            wr(vm, t + 0x38, u32::MAX);
        }
    }
    cl(h, vm, sp, "rmSetIdentity__FR9rmMatrix4", &[t + 0x60])?;
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x50, t + 0x10])?;
    let c = player(h, vm, sp)?;
    let anim = rd(vm, c + 0x18);
    cl(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi", &[anim, 0, 0, u32::MAX])?;
    let c = player(h, vm, sp)?;
    let mv = rd(vm, c + 0x10);
    cl(h, vm, sp, "ResetMappedMovmentStates__17CharacterMovementFv", &[mv])?;
    wr(vm, t + 0xe0, 0);
    wr(vm, t + 0xe4, 0);
    cl(h, vm, sp, "StopBeatGame__16DribblingManagerFv", &[t])?;
    if rb(vm, t + 0x110) != 0 {
        cl(h, vm, sp, "StartIdleParticle__16DribblingManagerFv", &[t])?;
    } else {
        cl(h, vm, sp, "StopIdleParticle__16DribblingManagerFv", &[t])?;
    }
    Ok(())
}

/// DribblingManager::StartBeatGame() @0x80331a8c
pub fn dm_start_beat_game(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x130);
    if rb(vm, t + 0x108) != 0 {
        return Ok(());
    }
    for i in 0..4 {
        let c = ctl(h, vm, sp, i)?;
        cl(h, vm, sp, "ReInitialize__10ControllerF11ControlType", &[c, 5])?;
    }
    wr(vm, t + 0xe0, 1);
    wr(vm, t + 0xe4, 0);
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "RegisterCallback__Q32EA5Conga15CallbackManagerFPCcPFPCQ32EA5Conga11CongaOutputPv_vPv", &[cb, 0x804d_8c03, 0x8033_27f4, t])?;
    cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[1])?;
    cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[1])?;
    let v = rd(vm, t + 0xe8);
    cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[v])?;
    cl(h, vm, sp, "Counter_SetVisible__16WorldHudHandlersFi", &[1])?;
    let v = rd(vm, t + 0xf4);
    cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, v])?;
    cl(h, vm, sp, "DribblingHud_SetVisible__16WorldHudHandlersFb", &[1])?;
    let c = ctl(h, vm, sp, 0)?;
    cl(h, vm, sp, "SetCurrentControllerState__10ControllerF16EControllerState", &[c, 0x1a])?;
    let cam = lwz13(vm, -0x1d94);
    let cam = rd(vm, cam);
    let info = cl(h, vm, sp, "GetCameraViewInfo__6CameraFb", &[cam, 0])?;
    cl(h, vm, sp, "__ct__14CameraViewInfoFRC14CameraViewInfo", &[sp + 0xc0, info])?;
    let cm = lwz13(vm, -0x1d94);
    cl(h, vm, sp, "ReInitialize__13CameraManagerF10CameraTypeUi", &[cm, 3, 0])?;
    let cm = lwz13(vm, -0x1d94);
    let fc = cl(h, vm, sp, "GetFrameCamera__13CameraManagerFUi", &[cm, 0])?;
    wr(vm, fc + 0x2ec, 0x8033_208c);
    wr(vm, fc + 0x2f0, t);
    let c = player(h, vm, sp)?;
    let (w0, w1, w2) = (rd(vm, c + 0x180), rd(vm, c + 0x184), rd(vm, c + 0x188));
    let c2 = player(h, vm, sp)?;
    wr(vm, sp + 0x50, w0);
    wr(vm, sp + 0x54, w1);
    wr(vm, sp + 0x58, w2);
    let (a0, a1, a2) = (rd(vm, c2 + 0x1a0), rd(vm, c2 + 0x1a4), rd(vm, c2 + 0x1a8));
    let k = lfs2(vm, -0x4820);
    wr(vm, sp + 0x40, a0);
    wr(vm, sp + 0x44, a1);
    wr(vm, sp + 0x48, a2);
    gc(h, vm, sp, "Mult__Q22EA4MathFRCQ32EA4Math7Vector3fRQ32EA4Math7Vector3", &[sp + 0x40, sp + 0x40], &[k])?;
    let f1v = lfs(vm, sp + 0x44);
    let f0v = lfs(vm, sp + 0x54);
    let junk = rd(vm, sp + 0x3c);
    let y = fadds(f1v, f0v);
    let kd = lfd2(vm, -0x4818);
    let p54 = lfs(vm, sp + 0x54);
    let (p40, p50) = (lfs(vm, sp + 0x40), lfs(vm, sp + 0x50));
    let y = y + kd;
    let f1n = p54 + kd;
    let (p48, p58) = (lfs(vm, sp + 0x48), lfs(vm, sp + 0x58));
    let x = fadds(p40, p50);
    let z = fadds(p48, p58);
    wr(vm, sp + 0x2c, junk);
    let y = fs(y);
    stfs(vm, sp + 0x30, x);
    let f3n = fs(f1n);
    stfs(vm, sp + 0x38, z);
    let (r5, r0) = (rd(vm, sp + 0x30), rd(vm, sp + 0x38));
    stfs(vm, sp + 0x34, y);
    let r4 = rd(vm, sp + 0x34);
    wr(vm, sp + 0x20, r5);
    wr(vm, sp + 0x24, r4);
    let (x1, y1) = (lfs(vm, sp + 0x20), lfs(vm, sp + 0x24));
    wr(vm, sp + 0x28, r0);
    stfs(vm, sp + 0x54, f3n);
    let z1 = lfs(vm, sp + 0x28);
    vset(h, vm, sp, fc + 0x330, x1, y1, z1)?;
    let r0 = rd(vm, sp + 0x50);
    let r5 = rd(vm, sp + 0x54);
    wr(vm, sp + 0x10, r0);
    let r4 = rd(vm, sp + 0x58);
    let r0 = rd(vm, sp + 0x5c);
    wr(vm, sp + 0x14, r5);
    let x2 = lfs(vm, sp + 0x10);
    wr(vm, sp + 0x18, r4);
    let y2 = lfs(vm, sp + 0x14);
    wr(vm, sp + 0x1c, r0);
    let z2 = lfs(vm, sp + 0x18);
    vset(h, vm, sp, fc + 0x340, x2, y2, z2)?;
    let k = lfs2(vm, -0x4810);
    stfs(vm, fc + 0x350, k);
    vm.st.mem.copy(sp + 0x60, sp + 0xc0, 0x60);
    let cam = lwz13(vm, -0x1d94);
    let cam = rd(vm, cam);
    cl(h, vm, sp, "StartCameraSysTransition__6CameraF14CameraViewInfoib", &[cam, sp + 0x60, 0x2ee, 0])?;
    Ok(())
}

/// DribblingManager::StopBeatGame() @0x80331d08
pub fn dm_stop_beat_game(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0xe0);
    if rb(vm, t + 0x108) == 0 {
        return Ok(());
    }
    wb(vm, t + 0x108, 0);
    cl(h, vm, sp, "DribblingHud_SetVisible__16WorldHudHandlersFb", &[0])?;
    cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[0])?;
    cl(h, vm, sp, "Counter_SetVisible__16WorldHudHandlersFi", &[0])?;
    cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[2])?;
    let c = ctl(h, vm, sp, 0)?;
    cl(h, vm, sp, "PopState__10ControllerFv", &[c])?;
    let cam = lwz13(vm, -0x1d94);
    let cam = rd(vm, cam);
    let info = cl(h, vm, sp, "GetCameraViewInfo__6CameraFb", &[cam, 0])?;
    cl(h, vm, sp, "__ct__14CameraViewInfoFRC14CameraViewInfo", &[sp + 0x70, info])?;
    let cm = lwz13(vm, -0x1d94);
    cl(h, vm, sp, "ReInitialize__13CameraManagerF10CameraTypeUi", &[cm, 0, 0])?;
    vm.st.mem.copy(sp + 0x10, sp + 0x70, 0x60);
    let cam = lwz13(vm, -0x1d94);
    let cam = rd(vm, cam);
    cl(h, vm, sp, "StartCameraSysTransition__6CameraF14CameraViewInfoib", &[cam, sp + 0x10, 0x2ee, 0])?;
    wr(vm, t + 0xe4, 0);
    for i in 0..4 {
        let c = ctl(h, vm, sp, i)?;
        cl(h, vm, sp, "ReInitialize__10ControllerF11ControlType", &[c, 0])?;
    }
    Ok(())
}

/// DribblingManager::RestartBeatGame() @0x80331e18
pub fn dm_restart_beat_game(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if rb(vm, t + 0x108) == 0 {
        return Ok(());
    }
    let r4 = rd(vm, t + 0xec);
    wr(vm, t + 0xe4, 0);
    wr(vm, t + 0xe8, r4);
    wr(vm, t + 0xf0, 0);
    wb(vm, t + 0xf8, 0);
    wb(vm, t + 0xf9, 0);
    wr(vm, t + 0x104, 0);
    cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[r4])?;
    let v = rd(vm, t + 0xf4);
    cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, v])?;
    let k = lfs2(vm, -0x4824);
    gc(h, vm, sp, "DribblingHud_SetValue__16WorldHudHandlersFf", &[], &[k])?;
    let c = player(h, vm, sp)?;
    let anim = rd(vm, c + 0x18);
    let k = lfs2(vm, -0x4828);
    gc(h, vm, sp, "SetStateTime__14AnimationStateFf", &[anim], &[k])?;
    Ok(())
}

/// DribblingManager::ThrowBall() @0x80331ea4
pub fn dm_throw_ball(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x50);
    let pm = phys_mgr(vm);
    if pm != 0 {
        if rb(vm, t + 0x3c) != 0 {
            let sys = rd(vm, t + 0x38);
            cl(h, vm, sp, "UnloadPhysics__14PhysicsManagerFib", &[pm, sys, 0])?;
            wr(vm, t + 0x38, u32::MAX);
            wb(vm, t + 0x3c, 0);
        }
        let sys = cl(h, vm, sp, "LoadPhysics__14PhysicsManagerFPCcPC9rmMatrix4Uib", &[pm, STR, 0, 2, 0])?;
        wr(vm, t + 0x38, sys);
        wb(vm, t + 0x3c, 1);
        let body = cl(h, vm, sp, "GenerateRigidBodyFromPhysicsSystem__14PhysicsManagerFiUii", &[pm, sys, 0, 0])?;
        wr(vm, t + 0x30, body);
        cl(h, vm, sp, "AddPhysicsRigidBodyToWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
        wb(vm, t + 0x34, 1);
        let body = rd(vm, t + 0x30);
        cl(h, vm, sp, "SetPos__16PhysicsRigidBodyFRC9rmVector3", &[body, t + 0x50])?;
    }
    let c = player(h, vm, sp)?;
    let ang = lfs(vm, c + 0x1b0);
    gc(h, vm, sp, "__ct__7rmAngleFf", &[sp + 0x10], &[ang])?;
    cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x20, sp + 0x10])?;
    let (x, y, z) = (lfs(vm, sp + 0x20), lfs(vm, sp + 0x24), lfs(vm, sp + 0x28));
    vset(h, vm, sp, sp + 0x30, x, y, z)?;
    cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[sp + 0x30, sp + 0x30])?;
    let k = lfs2(vm, -0x480c);
    gc(h, vm, sp, "Mult__Q22EA4MathFRCQ32EA4Math7Vector3fRQ32EA4Math7Vector3", &[sp + 0x30, sp + 0x30], &[k])?;
    let k = lfs2(vm, -0x4824);
    stfs(vm, sp + 0x34, k);
    let body = rd(vm, t + 0x30);
    cl(h, vm, sp, "SetLinearVelocity__16PhysicsRigidBodyFRC9rmVector3", &[body, sp + 0x30])?;
    let c = player(h, vm, sp)?;
    let mv = rd(vm, c + 0x10);
    cl(h, vm, sp, "ResetMappedMovmentStates__17CharacterMovementFv", &[mv])?;
    wr(vm, t + 0xe0, 4);
    wr(vm, t + 0xe4, 0);
    Ok(())
}

/// DribblingManager::CharacterThrowBall() @0x80332014
pub fn dm_character_throw_ball(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let c = player(h, vm, sp)?;
    wr(vm, c + 0x13c, 0x258);
    let c = player(h, vm, sp)?;
    let anim = rd(vm, c + 0x18);
    cl(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi", &[anim, 0x6f, 0, u32::MAX])?;
    wr(vm, t + 0xe0, 3);
    wr(vm, t + 0xe4, 0);
    Ok(())
}

/// DribblingManager::CameraFinishedTransitionCallback(void*) @0x8033208c
pub fn dm_camera_finished(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    if t == 0 {
        return Ok(());
    }
    let r0 = rd(vm, t + 0x100);
    wr(vm, t + 0xe0, 2);
    wr(vm, t + 0xe4, 0);
    wb(vm, t + 0x108, 1);
    wr(vm, t + 0x104, r0);
    Ok(())
}

/// DribblingManager::Update(int) @0x803320b8
pub fn dm_update(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = fsp(vm, 0x20);
    let state = rd(vm, t + 0xe0);
    if state == 0 {
        let v = itof(dt);
        let k4 = lfs2(vm, -0x4808);
        let k3 = lfs2(vm, -0x4804);
        let f2 = lfs(vm, t + 0xfc);
        let f0 = lfs(vm, t + 0x54);
        let lim = lfs(vm, t + 0x14);
        let f4 = fdivs(v, k4);
        let f2 = fnmsubs(k3, f4, f2);
        let f0 = fmadds(f2, f4, f0);
        stfs(vm, t + 0xfc, f2);
        stfs(vm, t + 0x54, f0);
        if fcmp(f0, lim) & LT == 0 {
            return Ok(());
        }
        let k = lfs2(vm, -0x4800);
        stfs(vm, t + 0x54, lim);
        stfs(vm, t + 0xfc, k);
        return Ok(());
    }
    let n = rd(vm, t + 0xe4).wrapping_add(dt);
    wr(vm, t + 0xe4, n);
    match state as i32 {
        1 | 2 => {
            cl(h, vm, sp, "UpdateDribbling__16DribblingManagerFi", &[t, dt])?;
        }
        3 => {
            cl(h, vm, sp, "UpdateThrow__16DribblingManagerFi", &[t, dt])?;
        }
        4 => {
            cl(h, vm, sp, "UpdateThrownBall__16DribblingManagerFi", &[t, dt])?;
        }
        _ => {}
    }
    wb(vm, t + 0xf8, 0);
    Ok(())
}

/// The ground contact check shared by UpdateDribbling and UpdateThrownBall: the ball closer to the ground than a
/// threshold plays a bounce sound once (`sfx_a` for ground types 11 and 12, `sfx_b` for the others).
fn ground_sfx(h: &mut MgHost, vm: &mut V, sp: u32, t: u32, pm: u32, sfx_a: u32, sfx_b: u32) -> R {
    let k = lfs2(vm, -0x47f0);
    gc(h, vm, sp, "GetGroundHeight__14PhysicsManagerCFPC9rmVector3f", &[pm, t + 0x50], &[k])?;
    let gh = f1(vm);
    let y = lfs(vm, t + 0x54);
    let d = fsubs(y, gh);
    gc(h, vm, sp, "rmfAbs__Ff", &[], &[d])?;
    let a = f1(vm);
    let thr = lfs2(vm, -0x47ec);
    if fcmp(a, thr) & LT == 0 {
        wb(vm, t + 0xf9, 0);
        return Ok(());
    }
    if rb(vm, t + 0xf9) != 0 {
        return Ok(());
    }
    let k = lfs2(vm, -0x47f0);
    let ty = gc(h, vm, sp, "GetGroundType__14PhysicsManagerCFPC9rmVector3f", &[pm, t + 0x50], &[k])?;
    let id = if ty.wrapping_sub(0xb) > 1 { sfx_b } else { sfx_a };
    let au = audio(h, vm, sp)?;
    cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[au, id, 0, 0x64])?;
    wb(vm, t + 0xf9, 1);
    Ok(())
}

/// DribblingManager::UpdateDribbling(int) @0x803321a4
pub fn dm_update_dribbling(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = dsp(vm, 0x80);
    cl(h, vm, sp, "GetBallMatrix__16DribblingManagerCFP9rmMatrix4", &[t, sp + 0x20])?;
    cl(h, vm, sp, "__as__9rmMatrix4FRC9rmMatrix4", &[t + 0x60, sp + 0x20])?;
    let (x, y, z) = (lfs(vm, t + 0x90), lfs(vm, t + 0x94), lfs(vm, t + 0x98));
    vset(h, vm, sp, t + 0x50, x, y, z)?;
    let pm = phys_mgr(vm);
    ground_sfx(h, vm, sp, t, pm, 0x7a, 0x7b)?;
    if rb(vm, t + 0x108) == 0 {
        // not in the beat game: throw the ball after a while
        if rd(vm, t + 0xe4) > 0x7d0 && rb(vm, t + 0xf8) != 0 {
            wb(vm, t + 0xf8, 0);
            cl(h, vm, sp, "CharacterThrowBall__16DribblingManagerFv", &[t])?;
        }
        return Ok(());
    }
    let timer = (rd(vm, t + 0xe8) as i32).wrapping_sub(dt as i32);
    wr(vm, t + 0xe8, timer as u32);
    if timer < 0 {
        cl(h, vm, sp, "WinLose_SetVisible__16WorldHudHandlersFii", &[1, 1])?;
        cl(h, vm, sp, "ShutDown__16DribblingManagerFv", &[t])?;
    }
    let r100 = rd(vm, t + 0x100);
    let r104 = rd(vm, t + 0x104);
    let f1v = itof(r100);
    let f2v = itof(r104);
    let k1 = lfs2(vm, -0x47e8);
    let f0v = fmuls(k1, f1v);
    if fcmp(f2v, f0v) & (LT | EQ) != 0 {
        let c = player(h, vm, sp)?;
        // scratch copy of the character position into the frame (never read again)
        let (w1, w0, w2) = (rd(vm, c + 0x184), rd(vm, c + 0x180), rd(vm, c + 0x188));
        wr(vm, sp + 0x14, w1);
        let kk = lfs2(vm, -0x47e4);
        let v = fadds(lfs(vm, sp + 0x14), kk);
        wr(vm, sp + 0x10, w0);
        wr(vm, sp + 0x18, w2);
        stfs(vm, sp + 0x14, v);
        if std::env::var("DRIB_DBG").is_ok() {
            let an = rd(vm, c + 0x18);
            eprintln!("DBG UD sp={sp:08x} r7={:08x} anim={an:08x} r1={:08x}", vm.st.cpu.r[7], vm.st.cpu.r[1]);
        }
        cl(h, vm, sp, "DribblingHud_FlashHud__16WorldHudHandlersFv", &[])?;
        if rb(vm, t + 0xf8) != 0 {
            let n = rd(vm, t + 0xf0).wrapping_add(1);
            let r100 = rd(vm, t + 0x100);
            let max = rd(vm, t + 0xf4);
            wr(vm, t + 0x104, r100);
            wr(vm, t + 0xf0, n);
            cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[n, max])?;
            cl(h, vm, sp, "DribblingHud_ShowFeedback__16WorldHudHandlersF24DribblingHudFeedbackType", &[1])?;
            let n = rd(vm, t + 0xf0);
            let max = rd(vm, t + 0xf4);
            if n >= max {
                cl(h, vm, sp, "WinLose_SetVisible__16WorldHudHandlersFii", &[0, 1])?;
                let au = audio(h, vm, sp)?;
                cl(h, vm, sp, "PlaySFX__5AudioFi15AUDIOWIIMOTESFXi", &[au, 0, 0x39, 0x1000])?;
                let cp = lwz13(vm, -0x1e7c);
                let prof = cl(h, vm, sp, "GetProfile__16CharacterProfileFv", &[cp])?;
                let area = rd(vm, t);
                let flag = rb(vm, prof.wrapping_add(area).wrapping_add(0xce3));
                if flag == 0 {
                    let cp = lwz13(vm, -0x1e7c);
                    cl(h, vm, sp, "SetDribblingGameBeat__16CharacterProfileFQ25Enums8AreaTypeb", &[cp, area, 1])?;
                    let m = rd(vm, t + 0x20);
                    cl(h, vm, sp, "AddMarblesEvent__8WorldManFii", &[WORLD_MAN, 4, m])?;
                }
                cl(h, vm, sp, "StopBeatGame__16DribblingManagerFv", &[t])?;
            }
        } else {
            let v = (rd(vm, t + 0x104) as i32).wrapping_sub(dt as i32);
            wr(vm, t + 0x104, v as u32);
            if v <= 0 {
                let r100 = rd(vm, t + 0x100);
                wr(vm, t + 0x104, r100);
                cl(h, vm, sp, "DribblingHud_ShowFeedback__16WorldHudHandlersF24DribblingHudFeedbackType", &[0])?;
                let max = rd(vm, t + 0xf4);
                wr(vm, t + 0xf0, 0);
                cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, max])?;
            }
        }
        wb(vm, t + 0xf8, 0);
    } else {
        wr(vm, t + 0x104, r104.wrapping_sub(dt));
        wb(vm, t + 0xf8, 0);
    }
    let v = rd(vm, t + 0xe8);
    cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[v])?;
    let (r104, r100) = (rd(vm, t + 0x104), rd(vm, t + 0x100));
    let k1 = lfs2(vm, -0x47e8);
    let f3 = itof(r104);
    let k0 = lfs2(vm, -0x4828);
    let f2 = itof(r100);
    let mut f2 = fdivs(f3, f2);
    f2 = fsubs(f2, k1);
    if fcmp(f2, k0) & LT != 0 {
        let k = lfs2(vm, -0x4824);
        f2 = fadds(f2, k);
    }
    let k3 = lfs2(vm, -0x47e0);
    if fcmp(f2, k3) & (LT | EQ) != 0 {
        let v = fdivs(f2, k3);
        gc(h, vm, sp, "DribblingHud_SetValue__16WorldHudHandlersFf", &[], &[v])?;
    } else {
        let f2 = fsubs(f2, k3);
        let k1 = lfs2(vm, -0x4824);
        let k0 = lfs2(vm, -0x47dc);
        let f2 = fdivs(f2, k3);
        let f1v = fsubs(k1, f2);
        let f1v = fadds(k0, f1v);
        gc(h, vm, sp, "DribblingHud_SetValue__16WorldHudHandlersFf", &[], &[f1v])?;
    }
    let c = ctl(h, vm, sp, 0)?;
    let ev = cl(h, vm, sp, "GetEventState__10ControllerCF12EActionEvent", &[c, 0xaf])?;
    if rb(vm, ev) != 0 {
        let cm = rd(vm, WORLD_MAN + 0x8c);
        if rb(vm, cm + 0x24) == 0 {
            let open = rb(vm, t + 0x108) != 0 || rd(vm, t + 0xe0) == 1;
            if open {
                cl(h, vm, sp, "OpenPauseMenu__16DribblingManagerFv", &[t])?;
            }
        }
    }
    let cm = rd(vm, WORLD_MAN + 0x8c);
    if rb(vm, cm + 0x24) == 0 {
        let v = rd(vm, t + 0x130) as i32;
        if v > 0 {
            wr(vm, t + 0x130, v.wrapping_sub(dt as i32) as u32);
        }
    }
    Ok(())
}

/// DribblingManager::UpdateThrow(int) @0x803325a4
pub fn dm_update_throw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x50);
    cl(h, vm, sp, "GetBallMatrix__16DribblingManagerCFP9rmMatrix4", &[t, sp + 8])?;
    cl(h, vm, sp, "__as__9rmMatrix4FRC9rmMatrix4", &[t + 0x60, sp + 8])?;
    let (x, y, z) = (lfs(vm, t + 0x90), lfs(vm, t + 0x94), lfs(vm, t + 0x98));
    vset(h, vm, sp, t + 0x50, x, y, z)?;
    if rd(vm, t + 0xe4) > 0xdc {
        cl(h, vm, sp, "ThrowBall__16DribblingManagerFv", &[t])?;
    }
    Ok(())
}

/// DribblingManager::UpdateThrownBall(int) @0x80332608
pub fn dm_update_thrown_ball(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x70);
    let body = rd(vm, t + 0x30);
    cl(h, vm, sp, "GetPos__16PhysicsRigidBodyCFv", &[sp + 0x10, body])?;
    let (x, y, z) = (lfs(vm, sp + 0x10), lfs(vm, sp + 0x14), lfs(vm, sp + 0x18));
    vset(h, vm, sp, t + 0x50, x, y, z)?;
    let body = rd(vm, t + 0x30);
    cl(h, vm, sp, "GetMat__16PhysicsRigidBodyCFv", &[sp + 0x20, body])?;
    cl(h, vm, sp, "__as__9rmMatrix4FRC9rmMatrix4", &[t + 0x60, sp + 0x20])?;
    let pm = phys_mgr(vm);
    ground_sfx(h, vm, sp, t, pm, 0x79, 0x7c)?;
    if rd(vm, t + 0xe4) > 0xbb8 {
        cl(h, vm, sp, "ShutDown__16DribblingManagerFv", &[t])?;
    }
    Ok(())
}

/// DribblingManager::DribbleProcess(const EA::Conga::CongaOutput*) @0x80332734
pub fn dm_dribble_process(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let out = vm.a(1);
    let sp = fsp(vm, 0x30);
    let key = rd(vm, out + 8);
    let mut i: u32 = 0;
    loop {
        let hist = cl(h, vm, sp, "GetInstance__Q32EA5Conga12CongaHistoryFv", &[])?;
        let a = cl(h, vm, sp, "GetHistoryOutput__Q32EA5Conga12CongaHistoryFii", &[hist, key, i])?;
        if a == 0 {
            break;
        }
        let hist = cl(h, vm, sp, "GetInstance__Q32EA5Conga12CongaHistoryFv", &[])?;
        let b = cl(h, vm, sp, "GetHistoryOutput__Q32EA5Conga12CongaHistoryFii", &[hist, key, i.wrapping_add(1)])?;
        if b == 0 {
            break;
        }
        cl(h, vm, sp, "Magnitude__Q42EA5Conga4Math14Coordinate3<f>CFv", &[a + 0x6c])?;
        let m1 = f1(vm);
        cl(h, vm, sp, "Magnitude__Q42EA5Conga4Math14Coordinate3<f>CFv", &[b + 0x6c])?;
        let m2 = f1(vm);
        if fcmp(m2, m1) & LT != 0 {
            break;
        }
        i = i.wrapping_add(1);
        if (i as i32) >= 10 {
            break;
        }
    }
    if key == 0 {
        wb(vm, t + 0xf8, 1);
    }
    Ok(())
}

/// DribblingManager::DribbleCallback(const EA::Conga::CongaOutput*, void*) @0x803327f4 (tail-calls DribbleProcess)
pub fn dm_dribble_callback(h: &mut MgHost, vm: &mut V) -> R {
    let out = vm.a(0);
    let user = vm.a(1);
    let sp = vm.st.cpu.r[1];
    let addr = vm.img.addr("DribbleProcess__16DribblingManagerFPCQ32EA5Conga11CongaOutput").ok_or("no symbol DribbleProcess")?;
    gca(h, vm, sp, addr, &[user, out], &[])?;
    Ok(())
}

/// DribblingManager::Draw(Ren::SceneContext&) @0x80332804
pub fn dm_draw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if rb(vm, t + 0x110) == 0 {
        return Ok(());
    }
    let model = rd(vm, t + 0x24);
    if model != 0 {
        cl(h, vm, sp, "CalculateRenderMatrix__16DribblingManagerFv", &[t])?;
        let model = rd(vm, t + 0x24);
        cl(h, vm, sp, "Draw__Q23Ren11CachedModelFRC9rmMatrix4b", &[model, t + 0xa0, 0])?;
        let ent = rd(vm, t + 0x2c);
        cl(h, vm, sp, "__as__9rmMatrix4FRC9rmMatrix4", &[ent + 0xc, t + 0xa0])?;
        let ent = rd(vm, t + 0x2c);
        wb(vm, ent + 0x4c, 1);
    } else {
        let ent = rd(vm, t + 0x2c);
        wb(vm, ent + 0x4c, 0);
    }
    Ok(())
}

/// DribblingManager::GetBallMatrix(rmMatrix4*) const @0x80332884
pub fn dm_get_ball_matrix(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let out = vm.a(1);
    let sp = dsp(vm, 0x80);
    let c = player(h, vm, sp)?;
    let state = rd(vm, t + 0xe0);
    let anim = rd(vm, c + 0x18);
    if state != 3 {
        if rb(vm, anim + 0x44) == 0 {
            return Ok(());
        }
        cl(h, vm, sp, "GetCurrentBonePos__14AnimationStateFiR9rmMatrix4", &[anim, 0x43, sp + 0x20])?;
        let c = player(h, vm, sp)?;
        cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0x20, c + 0x20, out])?;
        return Ok(());
    }
    let n = rd(vm, anim + 0x1670);
    let c = player(h, vm, sp)?;
    let mut found = false;
    let mut i: u32 = 0;
    while (i as i32) < (n as i32) {
        let id = cl(h, vm, sp, "GetMarkerID__14AnimationStateCFi", &[anim, i])?;
        if id == 0xf {
            let m = cl(h, vm, sp, "GetMarkerMatrix__14AnimationStateCFi", &[anim, i])?;
            cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[m, c + 0x20, out])?;
            found = true;
        }
        i = i.wrapping_add(1);
    }
    if found {
        return Ok(());
    }
    let k5 = lfs2(vm, -0x47d8);
    let f1v = lfs(vm, c + 0x1a0);
    let f0v = lfs(vm, c + 0x180);
    let k4 = lfs2(vm, -0x47d4);
    let f2v = lfs(vm, c + 0x184);
    let x = fmadds(k5, f1v, f0v);
    let f3v = lfs(vm, c + 0x1a8);
    let f0v = lfs(vm, c + 0x188);
    let y = fadds(k4, f2v);
    let z = fmadds(k5, f3v, f0v);
    gc(h, vm, sp, "__ct__9rmVector3Ffff", &[sp + 0x10], &[x, y, z])?;
    let f4 = lfs(vm, sp + 0x18);
    let f3 = lfs(vm, sp + 0x14);
    let f2 = lfs(vm, sp + 0x10);
    let c0 = lfs2(vm, -0x4828);
    let c1 = lfs2(vm, -0x4824);
    stfs(vm, out + 4, c0);
    stfs(vm, out, c1);
    stfs(vm, out + 8, c0);
    stfs(vm, out + 0xc, c0);
    stfs(vm, out + 0x10, c0);
    stfs(vm, out + 0x14, c1);
    stfs(vm, out + 0x18, c0);
    stfs(vm, out + 0x1c, c0);
    stfs(vm, out + 0x20, c0);
    stfs(vm, out + 0x24, c0);
    stfs(vm, out + 0x28, c1);
    stfs(vm, out + 0x2c, c0);
    stfs(vm, out + 0x30, f2);
    stfs(vm, out + 0x34, f3);
    stfs(vm, out + 0x38, f4);
    stfs(vm, out + 0x3c, c1);
    Ok(())
}

/// DribblingManager::CalculateRenderMatrix() @0x80332a10
pub fn dm_calc_render_matrix(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x70);
    // the second half of the matrix goes to the outgoing stack-argument area (8(r1)..)
    for i in 0..8u32 {
        let v = lfs(vm, t + 0x80 + 4 * i);
        stfs(vm, sp + 8 + 4 * i, v);
    }
    let mut fa = [0f64; 8];
    for i in 0..8u32 {
        fa[i as usize] = lfs(vm, t + 0x60 + 4 * i);
    }
    gc(h, vm, sp, "Set__Q32EA4Math8Matrix44Fffffffffffffffff", &[sp + 0x28], &fa)?;
    let c0 = lfs2(vm, -0x4828);
    let c1 = lfs2(vm, -0x4824);
    stfs(vm, sp + 0x58, c0);
    stfs(vm, sp + 0x5c, c0);
    stfs(vm, sp + 0x60, c0);
    stfs(vm, sp + 0x64, c1);
    let area = rd(vm, WORLD_MAN + 0x8c);
    let area = rd(vm, area + 8);
    cl(h, vm, sp, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[area, t + 0x50, t + 0xa0])?;
    cl(h, vm, sp, "rmMult__FRC9rmMatrix4RC9rmMatrix4R9rmMatrix4", &[sp + 0x28, t + 0xa0, t + 0xa0])?;
    Ok(())
}

/// The collection names of the four areas (offsets into the string table), area -> name.
fn area_name(area: u32) -> Option<u32> {
    match area as i32 {
        2 => Some(STR + 0xd5),
        3 => Some(STR + 0x123),
        0 => Some(STR + 0x12d),
        1 => Some(STR + 0x139),
        _ => None,
    }
}

/// DribblingManager::ChangeArea(Enums::AreaType) @0x80332ae4
pub fn dm_change_area(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let area = vm.a(1);
    let sp = fsp(vm, 0x20);
    if rd(vm, t) == area {
        cl(h, vm, sp, "StartIdleParticle__16DribblingManagerFv", &[t])?;
        return Ok(());
    }
    wr(vm, t, area);
    if let Some(name) = area_name(area) {
        let db = lwz13(vm, -0x260c);
        let coll = cl(h, vm, sp, "GetCollection__11pgIDatabaseFPCcPCc", &[db, STR + 0xc1, name])?;
        cl(h, vm, sp, "GetVector3FromArray__14pgDBCollectionFPCcUiR9rmVector3", &[coll, STR + 0xe1, 0, t + 0x10])?;
        let gi = "GetInt32FromArray__14pgDBCollectionFPCcUi";
        let v = cl(h, vm, sp, gi, &[coll, STR + 0xef, 0])?;
        wr(vm, t + 0x20, v);
        let v = cl(h, vm, sp, gi, &[coll, STR + 0xfb, 0])?;
        wr(vm, t + 0x100, v);
        let v = cl(h, vm, sp, gi, &[coll, STR + 0x106, 0])?;
        wr(vm, t + 0xec, v);
        let v = cl(h, vm, sp, gi, &[coll, STR + 0x111, 0])?;
        wr(vm, t + 0xf4, v);
        let db = lwz13(vm, -0x260c);
        cl(h, vm, sp, "DestroyCollection__11pgIDatabaseFP14pgDBCollection", &[db, coll])?;
    }
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x50, t + 0x10])?;
    if rb(vm, t + 0x110) != 0 {
        cl(h, vm, sp, "StartIdleParticle__16DribblingManagerFv", &[t])?;
    }
    Ok(())
}

/// DribblingManager::StartIdleParticle() @0x80332d88
pub fn dm_start_idle_particle(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x40);
    let g = rd(vm, t + 0x10c);
    let mgr = lwz13(vm, -0x1ed8);
    wr(vm, sp + 0x14, g);
    let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x14])?;
    let (r4, r0) = (rd(vm, t + 0x10), rd(vm, t + 0x14));
    let k = lfs2(vm, -0x47d0);
    wr(vm, sp + 0x24, r0);
    let y = lfs(vm, sp + 0x24);
    wr(vm, sp + 0x20, r4);
    let y = fadds(y, k);
    let r0 = rd(vm, t + 0x18);
    wr(vm, sp + 0x28, r0);
    stfs(vm, sp + 0x24, y);
    if fx != 0 {
        cl(h, vm, sp, "SetPos__6PartFxFRC9rmVector3", &[fx, sp + 0x20])?;
    } else {
        let mgr = lwz13(vm, -0x1ed8);
        let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8c96, sp + 0x20])?;
        wr(vm, t + 0x10c, g);
        wr(vm, sp + 0x10, g);
        let mgr = lwz13(vm, -0x1ed8);
        cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x10])?;
    }
    Ok(())
}

/// DribblingManager::StopIdleParticle() @0x80332e38
pub fn dm_stop_idle_particle(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x20);
    let g = rd(vm, t + 0x10c);
    let mgr = lwz13(vm, -0x1ed8);
    wr(vm, sp + 8, g);
    cl(h, vm, sp, "DestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0])?;
    let none = lwz13(vm, -0x2250);
    wr(vm, t + 0x10c, none);
    Ok(())
}

/// DribblingManager::OpenPauseMenu() @0x80332e80
pub fn dm_open_pause_menu(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if (rd(vm, t + 0x130) as i32) > 0 {
        return Ok(());
    }
    let cm = rd(vm, WORLD_MAN + 0x8c);
    wb(vm, cm + 0x24, 1);
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "PlaySFX__5AudioF17AUDIOAEMSFEHUDSFXii", &[a, 0xc, 0, 0x64])?;
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "Pause__5AudioFQ25Audio9PAUSEMODE", &[a, 2])?;
    let c = ctl(h, vm, sp, 0)?;
    cl(h, vm, sp, "SetCurrentControllerState__10ControllerF16EControllerState", &[c, 0])?;
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    wb(vm, fe + 0x48, 1);
    wb(vm, fe + 0x49, 1);
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    cl(h, vm, sp, "OpenAptOverlay__9FEManagerFPc", &[fe, 0x804d_8ca3])?;
    Ok(())
}

/// DribblingManager::ClosePauseMenu() @0x80332f10
pub fn dm_close_pause_menu(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let cm = rd(vm, WORLD_MAN + 0x8c);
    wb(vm, cm + 0x24, 0);
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "UnPause__5AudioFv", &[a])?;
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    cl(h, vm, sp, "CloseAptOverlay__9FEManagerFv", &[fe])?;
    let c = ctl(h, vm, sp, 0)?;
    cl(h, vm, sp, "PopState__10ControllerFv", &[c])?;
    wr(vm, t + 0x130, 0x4b0);
    Ok(())
}

/// DribblingManager::OnPauseMenuLoaded() @0x80332f70
pub fn dm_on_pause_menu_loaded(h: &mut MgHost, vm: &mut V) -> R {
    Ok(())
}

/// DribblingManager::OnPauseContinue() @0x80332f74 (tail-calls ClosePauseMenu)
pub fn dm_on_pause_continue(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = vm.st.cpu.r[1];
    cl(h, vm, sp, "ClosePauseMenu__16DribblingManagerFv", &[t])?;
    Ok(())
}

/// DribblingManager::OnPauseQuit() @0x80332f78
pub fn dm_on_pause_quit(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    cl(h, vm, sp, "ClosePauseMenu__16DribblingManagerFv", &[t])?;
    cl(h, vm, sp, "ShutDown__16DribblingManagerFv", &[t])?;
    Ok(())
}

/// DribblingManager::OnPauseReset() @0x80332fac
pub fn dm_on_pause_reset(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    cl(h, vm, sp, "ClosePauseMenu__16DribblingManagerFv", &[t])?;
    cl(h, vm, sp, "RestartBeatGame__16DribblingManagerFv", &[t])?;
    Ok(())
}

// =====================================================================================================================
// FreeThrowBall
// =====================================================================================================================

/// The name pool of the FreeThrowBall unit: `lis r29,-0x7fb2 ; addi r29,r29,-0x7340`.
const STR_BALL: u32 = 0x804d_8cc0;
const ALLOC: &str = "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc";

/// FreeThrowBall::FreeThrowBall(const rmVector3*, float) @0x8033314c
pub fn ball_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let pos = vm.a(1);
    let fa = vm.st.cpu.f[1];
    let sp = fsp(vm, 0x40);
    let c0 = lfs2(vm, -0x47c0);
    let (w0, w1, w2) = (rd(vm, pos), rd(vm, pos + 4), rd(vm, pos + 8));
    let (f2, f3) = (lfs(vm, pos + 4), lfs(vm, pos + 8));
    wr(vm, t, 0);
    wr(vm, t + 4, 0);
    wb(vm, t + 8, 0);
    wr(vm, t + 0xc, 0);
    wr(vm, t + 0x10, 0);
    wr(vm, t + 0x14, 0);
    wr(vm, t + 0x18, u32::MAX);
    wr(vm, t + 0x1c, u32::MAX);
    wr(vm, t + 0x20, u32::MAX);
    wb(vm, t + 0x24, 0);
    wb(vm, t + 0x25, 0);
    stfs(vm, t + 0x28, c0);
    stfs(vm, t + 0x2c, c0);
    stfs(vm, t + 0x30, fa);
    wr(vm, t + 0x40, w0);
    wr(vm, t + 0x44, w1);
    wr(vm, t + 0x48, w2);
    wr(vm, t + 0x50, w0);
    wr(vm, t + 0x54, w1);
    wr(vm, t + 0x58, w2);
    stfs(vm, t + 0x60, c0);
    stfs(vm, t + 0x64, c0);
    stfs(vm, t + 0x68, c0);
    wb(vm, t + 0x70, 0);
    let none = lwz13(vm, -0x2238);
    stfs(vm, t + 0x30, fa);
    let x = lfs(vm, pos);
    wr(vm, t + 0xb4, none);
    wb(vm, t + 0xb8, 0);
    wb(vm, t + 0x24, 1);
    wb(vm, t + 0x25, 0);
    vset(h, vm, sp, t + 0x50, x, f2, f3)?;
    let c0 = lfs2(vm, -0x47c0);
    stfs(vm, t + 0x60, c0);
    stfs(vm, t + 0x64, c0);
    stfs(vm, t + 0x68, c0);
    stfs(vm, t + 0x28, c0);
    stfs(vm, t + 0x2c, c0);
    let pm = phys_mgr(vm);
    if pm != 0 {
        let sys = lwz13(vm, -0x4a3c);
        let body = cl(h, vm, sp, "GenerateRigidBodyFromPhysicsSystem__14PhysicsManagerFiUii", &[pm, sys, 0, 0])?;
        wr(vm, t, body);
        cl(h, vm, sp, "AddPhysicsRigidBodyToWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
        wb(vm, t + 8, 1);
        let body = rd(vm, t);
        cl(h, vm, sp, "SetPos__16PhysicsRigidBodyFRC9rmVector3", &[body, t + 0x50])?;
        let body = rd(vm, t);
        cl(h, vm, sp, "SetQualityType__16PhysicsRigidBodyF18PhysicsQualityType", &[body, 4])?;
        cl(h, vm, sp, "__ct__15PhysicsUserDataFQ215PhysicsUserData4TypePv", &[sp + 0x10, 0xb, t])?;
        let body = rd(vm, t);
        cl(h, vm, sp, "SetUserData__16PhysicsRigidBodyFPC15PhysicsUserData", &[body, sp + 0x10])?;
        let pool = lwz13(vm, -0x43fc);
        let mut p = cl(h, vm, sp, ALLOC, &[0x18, pool, 0, 0x804d_8d21])?;
        if p != 0 {
            p = cl(h, vm, sp, "__ct__30FreeThrowBallCollisionListenerFP13FreeThrowBall", &[p, t])?;
        }
        wr(vm, t + 4, p);
    }
    let name = lwz13(vm, -0x4a48);
    cl(h, vm, sp, "__ct__7CStringFPCc", &[sp + 0xc, name])?;
    let sfx = ad13(vm, -0x4a44);
    cl(h, vm, sp, "__apl__7CStringFPCc", &[sp + 0xc, sfx])?;
    let am = lwz13(vm, -0x1ca4);
    let cs = cl(h, vm, sp, "c_str__7CStringCFv", &[sp + 0xc])?;
    let tex = cl(h, vm, sp, "GetLoadedTexture__12AssetManagerFPCcPi", &[am, cs, t + 0x1c])?;
    let am = lwz13(vm, -0x1ca4);
    let model = cl(h, vm, sp, "GetLoadedModel__12AssetManagerFPCcPii", &[am, STR_BALL + 0xb, t + 0x18, 0])?;
    cl(h, vm, sp, "SetTextures__Q24EAGL5ModelFPCc", &[model, tex])?;
    let am = lwz13(vm, -0x1ca4);
    let model2 = cl(h, vm, sp, "GetLoadedModel__12AssetManagerFPCcPii", &[am, STR_BALL + 0x18, t + 0x20, 0])?;
    for (slot, m) in [(0xc, model), (0x10, model2)] {
        let pool = lwz13(vm, -0x43fc);
        let mut p = cl(h, vm, sp, ALLOC, &[0x4c, pool, 0, 0x804c_c724])?;
        if p != 0 {
            p = cl(h, vm, sp, "__ct__Q23Ren11CachedModelFv", &[p])?;
        }
        wr(vm, t + slot, p);
        wr(vm, p + 0x44, m);
        cl(h, vm, sp, "SetScaleMatrix__Q23Ren11CachedModelFv", &[p])?;
    }
    let pool = lwz13(vm, -0x43fc);
    let mut p = cl(h, vm, sp, ALLOC, &[0x4c, pool, 0, 0x804d_8d4b])?;
    if p != 0 {
        p = cl(h, vm, sp, "__ct__31FreeThrowBallShadowRenderEntityFPQ24EAGL5Model", &[p, model2])?;
    }
    wr(vm, t + 0x14, p);
    let holder = lwz13(vm, -0x1cfc);
    let scene = rd(vm, holder + 8);
    cl(h, vm, sp, "AddEntity__Q23Ren5SceneFiPQ23Ren6Entity", &[scene, 0, p])?;
    let guid = rd(vm, t + 0xb4);
    let none = lwz13(vm, -0x2238);
    if guid == none {
        let mgr = lwz13(vm, -0x1ed8);
        let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, STR_BALL + 0x2c, t + 0x50])?;
        wr(vm, t + 0xb4, g);
    }
    let g = rd(vm, t + 0xb4);
    wr(vm, sp + 8, g);
    let mgr = lwz13(vm, -0x1ed8);
    let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 8])?;
    cl(h, vm, sp, "SetPos__6PartFxFRC9rmVector3", &[fx, t + 0x50])?;
    cl(h, vm, sp, "__dt__7CStringFv", &[sp + 0xc, u32::MAX])?;
    vm.ret(t);
    Ok(())
}

/// FreeThrowBall::~FreeThrowBall() @0x80333484
pub fn ball_dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flags = vm.a(1);
    let sp = fsp(vm, 0x20);
    if t != 0 {
        let pm = phys_mgr(vm);
        if pm != 0 {
            let l = rd(vm, t + 4);
            if l != 0 {
                vcall(h, vm, sp, l, 8, &[l, 1], &[])?;
                wr(vm, t + 4, 0);
            }
            if rb(vm, t + 8) != 0 {
                let body = rd(vm, t);
                cl(h, vm, sp, "RemovePhysicsRigidBodyFromWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
            }
            let body = rd(vm, t);
            cl(h, vm, sp, "DestroyPhysicsRigidBody__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
            wr(vm, t, 0);
        }
        let m = rd(vm, t + 0xc);
        if m != 0 {
            cl(h, vm, sp, "__dt__Q23Ren11CachedModelFv", &[m, 1])?;
            wr(vm, t + 0xc, 0);
        }
        let m = rd(vm, t + 0x10);
        wr(vm, t + 0xc, 0);
        if m != 0 {
            cl(h, vm, sp, "__dt__Q23Ren11CachedModelFv", &[m, 1])?;
            wr(vm, t + 0x10, 0);
        }
        let ent = rd(vm, t + 0x14);
        wr(vm, t + 0x10, 0);
        let holder = lwz13(vm, -0x1cfc);
        let scene = rd(vm, holder + 8);
        cl(h, vm, sp, "RemoveEntity__Q23Ren5SceneFiPQ23Ren6Entity", &[scene, 0, ent])?;
        let ent = rd(vm, t + 0x14);
        if ent != 0 {
            vcall(h, vm, sp, ent, 8, &[ent, 1], &[])?;
            wr(vm, t + 0x14, 0);
        }
        let guid = rd(vm, t + 0xb4);
        let none = lwz13(vm, -0x2238);
        if guid != none {
            let g = rd(vm, t + 0xb4);
            wr(vm, sp + 8, g);
            let mgr = lwz13(vm, -0x1ed8);
            cl(h, vm, sp, "DisableAndDestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0])?;
            let none = lwz13(vm, -0x2238);
            wr(vm, t + 0xb4, none);
        }
        if (flags as i32) > 0 {
            cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

/// FreeThrowBall::Pass(const rmVector3*, unsigned int) @0x80333ba4
pub fn ball_pass(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let pos = vm.a(1);
    let frames = vm.a(2);
    let sp = dsp(vm, 0x80);
    let (w0, w1, w2) = (rd(vm, pos), rd(vm, pos + 4), rd(vm, pos + 8));
    wr(vm, sp + 0x30, w0);
    wr(vm, sp + 0x34, w1);
    wr(vm, sp + 0x38, w2);
    cl(h, vm, sp, "rmDistanceXZ__FRC9rmVector3RC9rmVector3", &[sp + 0x30, t + 0x50])?;
    let d = f1(vm);
    let mut f31 = lfs2(vm, -0x4798);
    if fcmp(f31, d) & GT == 0 {
        f31 = d;
    }
    let k = lfs2(vm, -0x4794);
    let y = lfs(vm, sp + 0x34);
    let ty = lfs(vm, t + 0x54);
    let f2 = fadds(k, y);
    let _f1 = fsubs(f2, ty);
    let kd = lfd2(vm, -0x4768);
    gc(h, vm, sp, "sqrt", &[], &[kd])?;
    let f30 = fs(f1(vm));
    wr(vm, sp + 0x44, frames);
    wr(vm, sp + 0x40, 0x4330_0000);
    let magic = lfd2(vm, -0x4778);
    let k = lfs2(vm, -0x4760);
    let fr = f64::from_bits(0x4330_0000_0000_0000 | frames as u64);
    let fr = fsubs(fr, magic);
    let f0 = fdivs(fr, k);
    let f31 = fdivs(f31, f0);
    cl(h, vm, sp, "__mi__Q22EA4MathFRCQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[sp + 0x14, sp + 0x30, t + 0x50])?;
    let (x, y, z) = (lfs(vm, sp + 0x14), lfs(vm, sp + 0x18), lfs(vm, sp + 0x1c));
    vset(h, vm, sp, sp + 0x20, x, y, z)?;
    cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[sp + 0x20, sp + 0x20])?;
    let (x, z) = (lfs(vm, sp + 0x20), lfs(vm, sp + 0x28));
    let vx = fmuls(f31, x);
    let vz = fmuls(f31, z);
    stfs(vm, t + 0x64, f30);
    stfs(vm, t + 0x60, vx);
    stfs(vm, t + 0x68, vz);
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x40, t + 0x50])?;
    wb(vm, t + 0x24, 1);
    let body = rd(vm, t);
    wb(vm, t + 0x25, 0);
    cl(h, vm, sp, "SetPos__16PhysicsRigidBodyFRC9rmVector3", &[body, t + 0x50])?;
    if rb(vm, t + 8) != 0 {
        let body = rd(vm, t);
        let pm = phys_mgr(vm);
        cl(h, vm, sp, "RemovePhysicsRigidBodyFromWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
        wb(vm, t + 8, 0);
    }
    let guid = rd(vm, t + 0xb4);
    let none = lwz13(vm, -0x2238);
    if guid == none {
        let mgr = lwz13(vm, -0x1ed8);
        let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8cec, t + 0x50])?;
        wr(vm, t + 0xb4, g);
    }
    let g = rd(vm, t + 0xb4);
    wr(vm, sp + 0x10, g);
    let mgr = lwz13(vm, -0x1ed8);
    let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x10])?;
    cl(h, vm, sp, "SetPos__6PartFxFRC9rmVector3", &[fx, t + 0x50])?;
    wb(vm, t + 0xb8, 1);
    wb(vm, t + 0x70, 1);
    Ok(())
}

/// FreeThrowBall::Grab(const rmVector3*) @0x80333d80
pub fn ball_grab(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let pos = vm.a(1);
    let sp = fsp(vm, 0x20);
    let c0 = lfs2(vm, -0x47c0);
    let (x, y, z) = (lfs(vm, pos), lfs(vm, pos + 4), lfs(vm, pos + 8));
    stfs(vm, t + 0x60, c0);
    stfs(vm, t + 0x64, c0);
    stfs(vm, t + 0x68, c0);
    vset(h, vm, sp, t + 0x50, x, y, z)?;
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x40, t + 0x50])?;
    let body = rd(vm, t);
    wb(vm, t + 0x24, 0);
    wb(vm, t + 0x25, 0);
    cl(h, vm, sp, "SetPos__16PhysicsRigidBodyFRC9rmVector3", &[body, t + 0x50])?;
    if rb(vm, t + 8) != 0 {
        let body = rd(vm, t);
        let pm = phys_mgr(vm);
        cl(h, vm, sp, "RemovePhysicsRigidBodyFromWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
        wb(vm, t + 8, 0);
    }
    let guid = rd(vm, t + 0xb4);
    let none = lwz13(vm, -0x2238);
    if guid != none {
        let g = rd(vm, t + 0xb4);
        wr(vm, sp + 8, g);
        let mgr = lwz13(vm, -0x1ed8);
        cl(h, vm, sp, "DisableAndDestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0])?;
        let none = lwz13(vm, -0x2238);
        wr(vm, t + 0xb4, none);
    }
    wb(vm, t + 0x70, 0);
    Ok(())
}

/// The inlined `rmMult(a, b, out)`: out = a x b with the paired-single sequence of the original
/// (`ps_muls0 / ps_madds1 / ps_madds0 / ps_madds1`, each step rounded to single, product and sum not fused).
fn ps_mat_mult(vm: &mut V, a: u32, b: u32, out: u32) {
    let mut av = [0f64; 16];
    let mut bv = [0f64; 16];
    for i in 0..16u32 {
        av[i as usize] = lfs(vm, a + 4 * i);
        bv[i as usize] = lfs(vm, b + 4 * i);
    }
    let mut res = [0f64; 16];
    for i in 0..4 {
        for j in 0..4 {
            let mut v = fs(bv[j] * av[i * 4]);
            v = fs(bv[4 + j] * av[i * 4 + 1] + v);
            v = fs(bv[8 + j] * av[i * 4 + 2] + v);
            v = fs(bv[12 + j] * av[i * 4 + 3] + v);
            res[i * 4 + j] = v;
        }
    }
    for i in 0..16u32 {
        stfs(vm, out + 4 * i, res[i as usize]);
    }
}

/// FreeThrowBall::Update(int) @0x80333e58
pub fn ball_update(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = dsp(vm, 0x190);
    let c0 = lfs2(vm, -0x47c0);
    let k = lfs2(vm, -0x4760);
    let f31 = fdivs(itof(dt), k);
    let k2 = lfs2(vm, -0x475c);
    gc(h, vm, sp, "__ct__9rmVector3Ffff", &[sp + 0x90], &[c0, k2, c0])?;
    if rb(vm, t + 0x70) != 0 {
        gc(h, vm, sp, "__ml__Q22EA4MathFRCQ32EA4Math7Vector3f", &[sp + 0x6c, t + 0x60], &[f31])?;
        cl(h, vm, sp, "__apl__Q22EA4MathFRQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[t + 0x50, sp + 0x6c])?;
        gc(h, vm, sp, "__ml__Q22EA4MathFRCQ32EA4Math7Vector3f", &[sp + 0x60, sp + 0x90], &[f31])?;
        cl(h, vm, sp, "__apl__Q22EA4MathFRQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[t + 0x60, sp + 0x60])?;
        let area = rd(vm, WORLD_MAN + 0x8c);
        let area = rd(vm, area + 8);
        cl(h, vm, sp, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[area, t + 0x50, t + 0x74])?;
    } else {
        let body = rd(vm, t);
        cl(h, vm, sp, "GetPos__16PhysicsRigidBodyCFv", &[sp + 0x50, body])?;
        let (x, y, z) = (lfs(vm, sp + 0x50), lfs(vm, sp + 0x54), lfs(vm, sp + 0x58));
        vset(h, vm, sp, t + 0x50, x, y, z)?;
        let body = rd(vm, t);
        cl(h, vm, sp, "GetLinearVelocity__16PhysicsRigidBodyCFv", &[sp + 0x40, body])?;
        let (x, y, z) = (lfs(vm, sp + 0x40), lfs(vm, sp + 0x44), lfs(vm, sp + 0x48));
        vset(h, vm, sp, t + 0x60, x, y, z)?;
        let body = rd(vm, t);
        cl(h, vm, sp, "GetMat__16PhysicsRigidBodyCFv", &[sp + 0x120, body])?;
        let k3 = lfs2(vm, -0x47bc);
        let (f2, f1v, f0) = (lfs(vm, sp + 0x150), lfs(vm, sp + 0x154), lfs(vm, sp + 0x158));
        stfs(vm, sp + 0x15c, k3);
        stfs(vm, sp + 0x80, f2);
        stfs(vm, sp + 0x84, f1v);
        stfs(vm, sp + 0x88, f0);
        let area = rd(vm, WORLD_MAN + 0x8c);
        let area = rd(vm, area + 8);
        cl(h, vm, sp, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[area, sp + 0x80, sp + 0xe0])?;
        let c0 = lfs2(vm, -0x47c0);
        stfs(vm, sp + 0x150, c0);
        stfs(vm, sp + 0x154, c0);
        stfs(vm, sp + 0x158, c0);
        ps_mat_mult(vm, sp + 0x120, sp + 0xe0, t + 0x74);
    }
    let ent = rd(vm, t + 0x14);
    if ent != 0 {
        for i in 0..8u32 {
            let v = lfs(vm, t + 0x94 + 4 * i);
            stfs(vm, sp + 8 + 4 * i, v);
        }
        let mut fa = [0f64; 8];
        for i in 0..8u32 {
            fa[i as usize] = lfs(vm, t + 0x74 + 4 * i);
        }
        gc(h, vm, sp, "Set__Q32EA4Math8Matrix44Fffffffffffffffff", &[ent + 0xc], &fa)?;
    }
    let guid = rd(vm, t + 0xb4);
    let none = lwz13(vm, -0x2238);
    if guid != none {
        let g = rd(vm, t + 0xb4);
        wr(vm, sp + 0x30, g);
        let mgr = lwz13(vm, -0x1ed8);
        let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x30])?;
        cl(h, vm, sp, "SetPos__6PartFxFRC9rmVector3", &[fx, t + 0x50])?;
    }
    Ok(())
}

/// FreeThrowBall::Draw() @0x803342bc (tail-calls CachedModel::Draw)
pub fn ball_draw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    if rb(vm, t + 0xb8) == 0 {
        return Ok(());
    }
    let model = rd(vm, t + 0xc);
    let sp = vm.st.cpu.r[1];
    cl(h, vm, sp, "Draw__Q23Ren11CachedModelFRC9rmMatrix4b", &[model, t + 0x74, 0])?;
    Ok(())
}

/// FreeThrowBall::Load() @0x803342e0
pub fn ball_load(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let loaded = lwz13_b(vm, -0x2234);
    if loaded != 0 {
        cl(h, vm, sp, "Unload__13FreeThrowBallFv", &[])?;
    }
    let pm = phys_mgr(vm);
    if pm != 0 {
        let name = lwz13(vm, -0x4a48);
        let sys = cl(h, vm, sp, "LoadPhysics__14PhysicsManagerFPCcPC9rmMatrix4Uib", &[pm, name, 0, 2, 0])?;
        let a = ad13(vm, -0x4a3c);
        wr(vm, a, sys);
    }
    let a = ad13(vm, -0x2234);
    wb(vm, a, 1);
    Ok(())
}

/// `lbz r, off(r13)`.
fn lwz13_b(vm: &mut V, off: i32) -> u8 {
    let a = ad13(vm, off);
    rb(vm, a)
}

/// FreeThrowBall::Unload() @0x80334344
pub fn ball_unload(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    if lwz13_b(vm, -0x2234) != 0 {
        let pm = phys_mgr(vm);
        if pm != 0 {
            let sys = lwz13(vm, -0x4a3c);
            cl(h, vm, sp, "UnloadPhysics__14PhysicsManagerFib", &[pm, sys, 0])?;
        }
        let a = ad13(vm, -0x4a3c);
        wr(vm, a, u32::MAX);
        let a = ad13(vm, -0x2234);
        wb(vm, a, 0);
    }
    Ok(())
}

/// FreeThrowBall::InitializeThrow(float, float) @0x80335ce4
pub fn ball_initialize_throw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let (a, b) = (vm.st.cpu.f[1], vm.st.cpu.f[2]);
    stfs(vm, t + 0x28, a);
    wb(vm, t + 0x25, 1);
    stfs(vm, t + 0x2c, b);
    Ok(())
}

// =====================================================================================================================
// FreeThrowBallCourt
// =====================================================================================================================

/// FreeThrowBallCourt::FreeThrowBallCourt() @0x80334478
pub fn court_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let c = lfs2(vm, -0x4758);
    gc(h, vm, sp, "__ct__9rmVector3Ffff", &[t], &[c, c, c])?;
    let c = lfs2(vm, -0x4758);
    wb(vm, t + 0x160, 0);
    stfs(vm, t + 0x10, c);
    stfs(vm, t + 0x14, c);
    stfs(vm, t + 0x18, c);
    vm.ret(t);
    Ok(())
}

/// FreeThrowBallCourt::~FreeThrowBallCourt() @0x803344cc
pub fn court_dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flags = vm.a(1);
    let sp = fsp(vm, 0x10);
    if t != 0 {
        wb(vm, t + 0x160, 0);
        if (flags as i32) > 0 {
            cl(h, vm, sp, "__dl__FPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

/// FreeThrowBallCourt::Initialize(const rmVector3*, const rmVector3*) @0x80334514
pub fn court_initialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let r31 = vm.a(1);
    let p = vm.a(2);
    let sp = dsp(vm, 0x100);
    let (x, y, z) = (lfs(vm, p), lfs(vm, p + 4), lfs(vm, p + 8));
    vset(h, vm, sp, t, x, y, z)?;
    cl(h, vm, sp, "__mi__Q22EA4MathFRCQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[sp + 0x60, t, r31])?;
    let (x, y, z) = (lfs(vm, sp + 0x60), lfs(vm, sp + 0x64), lfs(vm, sp + 0x68));
    vset(h, vm, sp, sp + 0x90, x, y, z)?;
    cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[sp + 0x90, sp + 0x90])?;
    let k = lfs13(vm, -0x4a38);
    let f31 = -k;
    cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[sp + 0x90, sp + 0x80])?;
    let (f2, f1v, f0) = (lfs(vm, sp + 0x80), lfs(vm, sp + 0x84), lfs(vm, sp + 0x88));
    let f5 = fmuls(f2, f31);
    let f4 = fmuls(f1v, f31);
    let f3 = fmuls(f0, f31);
    stfs(vm, sp + 0x80, f5);
    stfs(vm, sp + 0x84, f4);
    stfs(vm, sp + 0x88, f3);
    let (f2, f1v, f0) = (lfs(vm, t), lfs(vm, t + 4), lfs(vm, t + 8));
    let f2 = fadds(f2, f5);
    let f1v = fadds(f1v, f4);
    let f0 = fadds(f0, f3);
    stfs(vm, sp + 0x34, f2);
    stfs(vm, sp + 0x38, f1v);
    stfs(vm, sp + 0x3c, f0);
    let (a, b, c) = (rd(vm, sp + 0x34), rd(vm, sp + 0x38), rd(vm, sp + 0x3c));
    wr(vm, sp + 0x40, a);
    wr(vm, sp + 0x44, b);
    wr(vm, sp + 0x48, c);
    let (x, y, z) = (lfs(vm, sp + 0x40), lfs(vm, sp + 0x44), lfs(vm, sp + 0x48));
    vset(h, vm, sp, sp + 0x50, x, y, z)?;
    let (x, y, z) = (lfs(vm, sp + 0x50), lfs(vm, sp + 0x54), lfs(vm, sp + 0x58));
    vset(h, vm, sp, t + 0x10, x, y, z)?;
    let k = lfs2(vm, -0x4754);
    let pm = phys_mgr(vm);
    gc(h, vm, sp, "GetGroundHeight__14PhysicsManagerCFPC9rmVector3f", &[pm, t + 0x10], &[k])?;
    let gh = f1(vm);
    stfs(vm, t + 0x14, gh);
    cl(h, vm, sp, "__ct__7rmAngleFRC9rmVector3", &[sp + 0x30, sp + 0x90])?;
    let ang = lfs(vm, sp + 0x30);
    gc(h, vm, sp, "rmMatrix4RotationY__FfR9rmMatrix4", &[sp + 0xa0], &[ang])?;
    let k3 = lfs2(vm, -0x4750);
    let (x, y, z) = (lfs(vm, r31), lfs(vm, r31 + 4), lfs(vm, r31 + 8));
    let (vx, vy, vz) = (fmuls(k3, x), fmuls(k3, y), fmuls(k3, z));
    gc(h, vm, sp, "__ct__9rmVector3Ffff", &[sp + 0x70], &[vx, vy, vz])?;
    // Matrix44::Set(this+0x20, translation = scaled r31)
    let c = lfs2(vm, -0x4758);
    let d = lfs2(vm, -0x474c);
    stfs(vm, sp + 8, c);
    stfs(vm, sp + 0xc, c);
    stfs(vm, sp + 0x10, d);
    stfs(vm, sp + 0x14, c);
    let v = lfs(vm, sp + 0x70);
    stfs(vm, sp + 0x18, v);
    let v = lfs(vm, sp + 0x74);
    stfs(vm, sp + 0x1c, v);
    let v = lfs(vm, sp + 0x78);
    stfs(vm, sp + 0x20, v);
    stfs(vm, sp + 0x24, d);
    let set = "Set__Q22EA4MathFRQ32EA4Math8Matrix44ffffffffffffffff";
    gc(h, vm, sp, set, &[t + 0x20], &[d, c, c, c, c, d, c, c])?;
    let ang = lfs(vm, sp + 0x30);
    gc(h, vm, sp, "rmMatrix4RotationY__FfR9rmMatrix4", &[t + 0x60], &[-ang])?;
    let c = lfs2(vm, -0x4758);
    let d = lfs2(vm, -0x474c);
    stfs(vm, sp + 8, c);
    let x = lfs(vm, r31);
    stfs(vm, sp + 0xc, c);
    let y = lfs(vm, r31 + 4);
    stfs(vm, sp + 0x10, d);
    let z = lfs(vm, r31 + 8);
    stfs(vm, sp + 0x14, c);
    stfs(vm, sp + 0x18, x);
    stfs(vm, sp + 0x1c, y);
    stfs(vm, sp + 0x20, z);
    stfs(vm, sp + 0x24, d);
    gc(h, vm, sp, set, &[t + 0xa0], &[d, c, c, c, c, d, c, c])?;
    let ang = lfs(vm, sp + 0x30);
    gc(h, vm, sp, "rmMatrix4RotationY__FfR9rmMatrix4", &[t + 0xe0], &[ang])?;
    wb(vm, t + 0x160, 1);
    Ok(())
}

/// FreeThrowBallCourt::UnInitialize() @0x803347f8
pub fn court_uninitialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    wb(vm, t + 0x160, 0);
    Ok(())
}

fn lt(a: f64, b: f64) -> bool {
    fcmp(a, b) & LT != 0
}
fn gt(a: f64, b: f64) -> bool {
    fcmp(a, b) & GT != 0
}
/// `fcmpo ; cror eq,gt,eq ; bne` taken when this is false.
fn ge(a: f64, b: f64) -> bool {
    fcmp(a, b) & (GT | EQ) != 0
}

/// FreeThrowBall::Throw(const rmVector3*) @0x803335f8
pub fn ball_throw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let pos = vm.a(1);
    let sp = dsp(vm, 0xf0);
    let k1 = lfs2(vm, -0x47bc);
    let v28 = lfs(vm, t + 0x28);
    // f1 = min(k1, [0x28])
    let f1v = if lt(k1, v28) { k1 } else { v28 };
    let k2 = lfs2(vm, -0x47b8);
    // f31 = max(k2, f1)
    let mut f31 = if gt(k2, f1v) { k2 } else { f1v };
    let c0 = lfs2(vm, -0x47c0);
    gc(h, vm, sp, "__ct__9rmVector3Ffff", &[sp + 0x80], &[c0, c0, c0])?;
    let mut f30 = lfs2(vm, -0x47c0);
    let v2c = lfs(vm, t + 0x2c);
    let mut f29 = f30;
    let a = if ge(v2c, f30) { v2c } else { -v2c };
    let k3 = lfs2(vm, -0x47b4);
    let rn = "rmNormalizeXZ__FRC9rmVector3fR9rmVector3";
    if gt(a, k3) {
        let (pz, tz, tx, px) = (lfs(vm, pos + 8), lfs(vm, t + 0x58), lfs(vm, t + 0x50), lfs(vm, pos));
        let z = fsubs(pz, tz);
        let c = lfs2(vm, -0x47c0);
        let x = fsubs(tx, px);
        gc(h, vm, sp, "__ct__9rmVector3Ffff", &[sp + 0x70], &[z, c, x])?;
        let s = lfs(vm, t + 0x2c);
        gc(h, vm, sp, rn, &[sp + 0x70, sp + 0x80], &[s])?;
    } else {
        let c = lfs2(vm, -0x47c0);
        let b = if ge(f31, c) { f31 } else { -f31 };
        let k3 = lfs2(vm, -0x47b4);
        if gt(b, k3) {
            let (pz, tz, tx, px) = (lfs(vm, pos + 8), lfs(vm, t + 0x58), lfs(vm, t + 0x50), lfs(vm, pos));
            let z = fsubs(pz, tz);
            let c = lfs2(vm, -0x47c0);
            let x = fsubs(tx, px);
            gc(h, vm, sp, "__ct__9rmVector3Ffff", &[sp + 0x60], &[z, c, x])?;
            let r = cl(h, vm, sp, "Random__Q22EA4MathFv", &[])?;
            let magic = lfd2(vm, -0x4778);
            let k4 = lfs2(vm, -0x47b0);
            let rem = r.wrapping_sub((r / 100).wrapping_mul(100));
            let fr = fsubs(f64::from_bits(0x4330_0000_0000_0000 | rem as u64), magic);
            let f28 = fmuls(k4, fr);
            let r = cl(h, vm, sp, "Random__Q22EA4MathFv", &[])?;
            let k5 = lfs2(vm, -0x47ac);
            let sign: u32 = if r & 1 != 0 { u32::MAX } else { 1 };
            let f2 = fadds(k5, f28);
            let magic = lfd2(vm, -0x4770);
            let fs_ = fsubs(f64::from_bits(0x4330_0000_0000_0000 | (sign ^ 0x8000_0000) as u64), magic);
            let s = fmuls(f2, fs_);
            gc(h, vm, sp, rn, &[sp + 0x60, sp + 0x80], &[s])?;
        }
    }
    let c = lfs2(vm, -0x47c0);
    if lt(f31, c) {
        let f2 = -f31;
        let k6 = lfs2(vm, -0x47a8);
        let f2 = if lt(f2, k6) { f2 } else { k6 };
        let k6 = lfs2(vm, -0x47a8);
        let k7 = lfs2(vm, -0x47a4);
        let q = fdivs(f2, k6);
        f29 = lfs2(vm, -0x47c0);
        f30 = fmuls(k7, q);
    } else if gt(f31, c) {
        let k8 = lfs2(vm, -0x47a0);
        if !lt(f31, k8) {
            f31 = k8;
        }
        let k8 = lfs2(vm, -0x47a0);
        let k9 = lfs2(vm, -0x479c);
        let q = fdivs(f31, k8);
        f29 = lfs2(vm, -0x47c0);
        f30 = fmuls(k9, q);
    }
    // target = pos + A, with the height raised by f29
    let (p0, a0) = (lfs(vm, pos), lfs(vm, sp + 0x80));
    let p1 = lfs(vm, pos + 4);
    let f4 = fadds(p0, a0);
    let a1 = lfs(vm, sp + 0x84);
    let p2 = lfs(vm, pos + 8);
    let a2 = lfs(vm, sp + 0x88);
    let f2 = fadds(p1, a1);
    stfs(vm, sp + 0x14, f4);
    let f0 = fadds(p2, a2);
    stfs(vm, sp + 0x18, f2);
    stfs(vm, sp + 0x1c, f0);
    let (r5, r4, r0) = (rd(vm, sp + 0x14), rd(vm, sp + 0x18), rd(vm, sp + 0x1c));
    wr(vm, sp + 0x2c, r5);
    wr(vm, sp + 0x30, r4);
    wr(vm, sp + 0x34, r0);
    let (x, y, z) = (lfs(vm, sp + 0x2c), lfs(vm, sp + 0x30), lfs(vm, sp + 0x34));
    vset(h, vm, sp, sp + 0x50, x, y, z)?;
    let y = lfs(vm, sp + 0x54);
    let y = fadds(y, f29);
    stfs(vm, sp + 0x54, y);
    cl(h, vm, sp, "rmDistanceXZ__FRC9rmVector3RC9rmVector3", &[sp + 0x50, t + 0x50])?;
    let d = f1(vm);
    let k10 = lfs2(vm, -0x4798);
    let f0 = if gt(k10, d) { k10 } else { d };
    let k11 = lfs2(vm, -0x4794);
    let f29 = fadds(f30, f0);
    let ty = lfs(vm, sp + 0x54);
    let by = lfs(vm, t + 0x54);
    let f1v = fadds(k11, ty);
    let k10 = lfs2(vm, -0x4798);
    let f0 = fsubs(f1v, by);
    let f30 = if gt(k10, f0) { k10 } else { f0 };
    let k12 = lfs2(vm, -0x4790);
    let k13 = lfs2(vm, -0x478c);
    let f1v = fsubs(f29, k12);
    let k1 = lfs2(vm, -0x47bc);
    let f0 = fmuls(f1v, k13);
    let f2 = if lt(k1, f0) { k1 } else { f0 };
    let c0 = lfs2(vm, -0x47c0);
    let f4 = if gt(c0, f2) { c0 } else { f2 };
    let ty = lfs(vm, sp + 0x54);
    let by = lfs(vm, t + 0x54);
    let k13 = lfs2(vm, -0x478c);
    let k14 = lfs2(vm, -0x4788);
    let f1v = fsubs(ty, by);
    let f2 = fmadds(k13, f4, k14);
    gc(h, vm, sp, "rmfMax__Fff", &[], &[f1v, f2])?;
    let m = f1(vm);
    let k15 = lfs2(vm, -0x4784);
    let arg = fmuls(k15, m);
    gc(h, vm, sp, "sqrt", &[], &[arg])?;
    let f31 = fs(f1(vm));
    let k15 = lfs2(vm, -0x4784);
    let f0 = fmuls(k15, f30);
    let f28 = -f31;
    let arg = fmsubs(f28, f28, f0);
    gc(h, vm, sp, "sqrt", &[], &[arg])?;
    let f2 = fs(f1(vm));
    let k16 = lfs2(vm, -0x4780);
    let f1v = -f28;
    let f1v = fadds(f1v, f2);
    let f0 = fdivs(f1v, k16);
    let f28 = fdivs(f29, f0);
    cl(h, vm, sp, "__mi__Q22EA4MathFRCQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[sp + 0x20, sp + 0x50, t + 0x50])?;
    let (x, y, z) = (lfs(vm, sp + 0x20), lfs(vm, sp + 0x24), lfs(vm, sp + 0x28));
    vset(h, vm, sp, sp + 0x40, x, y, z)?;
    cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[sp + 0x40, sp + 0x40])?;
    let (x, z) = (lfs(vm, sp + 0x40), lfs(vm, sp + 0x48));
    let vx = fmuls(f28, x);
    let vz = fmuls(f28, z);
    stfs(vm, t + 0x64, f31);
    stfs(vm, t + 0x60, vx);
    stfs(vm, t + 0x68, vz);
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 0x40, t + 0x50])?;
    let inw = rb(vm, t + 8);
    wb(vm, t + 0x24, 1);
    wb(vm, t + 0x25, 0);
    if inw == 0 {
        let body = rd(vm, t);
        let pm = phys_mgr(vm);
        cl(h, vm, sp, "AddPhysicsRigidBodyToWorld__14PhysicsManagerFP16PhysicsRigidBody", &[pm, body])?;
        wb(vm, t + 8, 1);
    }
    let body = rd(vm, t);
    cl(h, vm, sp, "SetLinearVelocity__16PhysicsRigidBodyFRC9rmVector3", &[body, t + 0x60])?;
    let guid = rd(vm, t + 0xb4);
    let none = lwz13(vm, -0x2238);
    if guid == none {
        let mgr = lwz13(vm, -0x1ed8);
        let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8cec, t + 0x50])?;
        wr(vm, t + 0xb4, g);
    }
    let g = rd(vm, t + 0xb4);
    wr(vm, sp + 0x10, g);
    let mgr = lwz13(vm, -0x1ed8);
    let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0x10])?;
    cl(h, vm, sp, "SetPos__6PartFxFRC9rmVector3", &[fx, t + 0x50])?;
    wb(vm, t + 0xb8, 1);
    wb(vm, t + 0x70, 0);
    let sfx = "PlaySFX__5AudioF14AUDIOAEMSBESFXii";
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, sfx, &[a, 0x7d, 0, 0x64])?;
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, sfx, &[a, 0x7e, 0, 0x64])?;
    let cm = rd(vm, WORLD_MAN + 0x8c);
    let cmgr = rd(vm, cm + 0x18);
    let chr = if cmgr != 0 { cl(h, vm, sp, "GetPlayerCharacter__16CharacterManagerFi", &[cmgr, 0])? } else { 0 };
    let flag = rd(vm, chr.wrapping_add(0x1e8));
    let a = audio(h, vm, sp)?;
    if flag == 0 {
        cl(h, vm, sp, sfx, &[a, 7, 0, 0x64])?;
    } else {
        cl(h, vm, sp, sfx, &[a, 8, 0, 0x64])?;
    }
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "PlaySFX__5AudioFi15AUDIOWIIMOTESFXi", &[a, 0, 0x3a, 0x1000])?;
    Ok(())
}

// =====================================================================================================================
// collision listeners
// =====================================================================================================================

/// FreeThrowBallCollisionListener::ContactConfirmedCallback(const PhysicsUserData*, unsigned int,
/// const PhysicsContactPoint*, float) @0x80334a00
pub fn ball_listener_contact(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let kind = vm.a(2);
    let cp = vm.a(3);
    let fa = vm.st.cpu.f[1];
    let sp = fsp(vm, 0x30);
    if kind != 1 {
        return Ok(());
    }
    let k = lfs2(vm, -0x4748);
    let f31 = if ge(fa, k) { fa } else { -fa };
    let k = lfs13(vm, -0x4a30);
    if gt(f31, k) {
        let g = rd(vm, t + 0x14);
        wr(vm, sp + 0xc, g);
        let mgr = lwz13(vm, -0x1ed8);
        let fx = cl(h, vm, sp, "GetPartFx__13PartFxManagerF4GUID", &[mgr, sp + 0xc])?;
        if fx == 0 {
            let mgr = lwz13(vm, -0x1ed8);
            let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8d98, cp])?;
            wr(vm, t + 0x14, g);
            wr(vm, sp + 8, g);
            let mgr = lwz13(vm, -0x1ed8);
            cl(h, vm, sp, "DisableAndDestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0xc8])?;
        }
    }
    let k = lfs2(vm, -0x4744);
    if gt(f31, k) {
        let az = cl(h, vm, sp, "GetAzimuth__5AudioFRC9rmVector3", &[cp])?;
        let a = audio(h, vm, sp)?;
        cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x82, az, 0x64])?;
        return Ok(());
    }
    let k = lfs2(vm, -0x4740);
    if gt(f31, k) {
        let az = cl(h, vm, sp, "GetAzimuth__5AudioFRC9rmVector3", &[cp])?;
        let a = audio(h, vm, sp)?;
        cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x81, az, 0x64])?;
    }
    Ok(())
}

/// FreeThrowCharacterCollisionListener::FreeThrowCharacterCollisionListener(Character*) @0x8033481c
pub fn chr_listener_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let chr = vm.a(1);
    let sp = fsp(vm, 0x10);
    let body = rd(vm, chr + 0x20c);
    cl(h, vm, sp, "__ct__24PhysicsCharacterListenerFP16PhysicsCharacter", &[t, body])?;
    wr(vm, t + 0x10, chr);
    wr(vm, t, 0x804d_8d70);
    wb(vm, t + 8, 0);
    vm.ret(t);
    Ok(())
}

/// FreeThrowCharacterCollisionListener::~FreeThrowCharacterCollisionListener() @0x80334874
pub fn chr_listener_dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flags = vm.a(1);
    let sp = fsp(vm, 0x10);
    if t != 0 {
        cl(h, vm, sp, "__dt__24PhysicsCharacterListenerFv", &[t, 0])?;
        if (flags as i32) > 0 {
            cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

/// FreeThrowCharacterCollisionListener::CharacterInteractionCallback(...) @0x803348cc
pub fn chr_listener_character_interaction(h: &mut MgHost, vm: &mut V) -> R {
    Ok(())
}

/// FreeThrowCharacterCollisionListener::ObjectInteractionCallback(...) @0x803348d0
pub fn chr_listener_object_interaction(h: &mut MgHost, vm: &mut V) -> R {
    Ok(())
}

/// FreeThrowCharacterCollisionListener::OverlapAddedCallback(const PhysicsUserData*) @0x803348d4
pub fn chr_listener_overlap_added(h: &mut MgHost, vm: &mut V) -> R {
    vm.ret(1);
    Ok(())
}

/// FreeThrowCharacterCollisionListener::OverlapRemovedCallback(const PhysicsUserData*, bool) @0x803348dc
pub fn chr_listener_overlap_removed(h: &mut MgHost, vm: &mut V) -> R {
    Ok(())
}

/// FreeThrowBallCollisionListener::FreeThrowBallCollisionListener(FreeThrowBall*) @0x803348f8
pub fn ball_listener_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let ball = vm.a(1);
    let sp = fsp(vm, 0x10);
    let body = rd(vm, ball);
    cl(h, vm, sp, "__ct__24PhysicsRigidBodyListenerFP16PhysicsRigidBody", &[t, body])?;
    wr(vm, t + 0x10, ball);
    wr(vm, t, 0x804d_8db0);
    let none = lwz13(vm, -0x2220);
    wr(vm, t + 0x14, none);
    wb(vm, t + 8, 0);
    wb(vm, t + 9, 1);
    wb(vm, t + 0xa, 0);
    wb(vm, t + 0xb, 0);
    vm.ret(t);
    Ok(())
}

/// FreeThrowBallCollisionListener::~FreeThrowBallCollisionListener() @0x80334968
pub fn ball_listener_dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flags = vm.a(1);
    let sp = fsp(vm, 0x20);
    if t != 0 {
        let g = rd(vm, t + 0x14);
        wr(vm, t, 0x804d_8db0);
        let none = lwz13(vm, -0x2220);
        if g != none {
            let g = rd(vm, t + 0x14);
            wr(vm, sp + 8, g);
            let mgr = lwz13(vm, -0x1ed8);
            cl(h, vm, sp, "DisableAndDestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 8, 0])?;
            let none = lwz13(vm, -0x2220);
            wr(vm, t + 0x14, none);
        }
        cl(h, vm, sp, "__dt__24PhysicsRigidBodyListenerFv", &[t, 0])?;
        if (flags as i32) > 0 {
            cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

// =====================================================================================================================
// HighFiveManager
// =====================================================================================================================

/// HighFiveManager::HighFiveManager() @0x80337310
pub fn hf_ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let c = lfs2(vm, -0x4650);
    wr(vm, t + 4, 0);
    stfs(vm, t + 8, c);
    wr(vm, t + 0xc, 0);
    wr(vm, t + 0x10, 0);
    wr(vm, t + 0x14, 0);
    wr(vm, t + 0x18, 0);
    wb(vm, t + 0x1c, 0);
    wb(vm, t + 0x1d, 0);
    wr(vm, t + 0x20, 1);
    vm.ret(t);
    Ok(())
}

/// HighFiveManager::~HighFiveManager() @0x80337344
pub fn hf_dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flags = vm.a(1);
    let sp = fsp(vm, 0x10);
    if t != 0 {
        if rb(vm, t) != 0 {
            cl(h, vm, sp, "ShutDown__15HighFiveManagerFv", &[t])?;
        }
        if (flags as i32) > 0 {
            cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

/// HighFiveManager::Initialize() @0x803373a4
pub fn hf_initialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let db = lwz13(vm, -0x260c);
    let name = ad13(vm, -0x4a20);
    let coll = cl(h, vm, sp, "GetCollection__11pgIDatabaseFPCcPCc", &[db, 0x804d_8f48, name])?;
    let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, 0x804d_8f5c, 0])?;
    wr(vm, t + 0x20, v);
    let db = lwz13(vm, -0x260c);
    cl(h, vm, sp, "DestroyCollection__11pgIDatabaseFP14pgDBCollection", &[db, coll])?;
    Ok(())
}

/// HighFiveManager::StartUp() @0x8033740c
pub fn hf_startup(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    wb(vm, t, 1);
    wb(vm, t + 0x1c, 0);
    wb(vm, t + 0x1d, 0);
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "RegisterCallback__Q32EA5Conga15CallbackManagerFPCcPFPCQ32EA5Conga11CongaOutputPv_vPv", &[cb, 0x804d_8f68, 0x8033_7bdc, t])?;
    cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 1])?;
    Ok(())
}

/// HighFiveManager::ShutDown() @0x80337470
pub fn hf_shutdown(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    wb(vm, t, 0);
    wr(vm, t + 4, 0);
    wr(vm, t + 0xc, 0);
    wr(vm, t + 0x10, 0);
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "Purge__Q32EA5Conga15CallbackManagerFv", &[cb])?;
    cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 0])?;
    Ok(())
}

/// HighFiveManager::Update(int) @0x803374c0
pub fn hf_update(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = fsp(vm, 0x10);
    if rb(vm, t) == 0 {
        return Ok(());
    }
    let state = rd(vm, t + 0x14);
    if state != 1 {
        let v = rd(vm, t + 0x18).wrapping_add(dt);
        wr(vm, t + 0x18, v);
    }
    let state = rd(vm, t + 0x14);
    match state as i32 {
        1 => {
            cl(h, vm, sp, "UpdateIdle__15HighFiveManagerFi", &[t, dt])?;
        }
        2 => {
            cl(h, vm, sp, "UpdateStartHighFive__15HighFiveManagerFi", &[t, dt])?;
        }
        3 => {
            cl(h, vm, sp, "UpdateExecuteParticleEffect__15HighFiveManagerFi", &[t, dt])?;
        }
        4 => {
            cl(h, vm, sp, "UpdateFinishHighFive__15HighFiveManagerFi", &[t, dt])?;
        }
        5 => {
            cl(h, vm, sp, "UpdateHighFiveHud__15HighFiveManagerFi", &[t, dt])?;
        }
        _ => {}
    }
    wb(vm, t + 0x1c, 0);
    Ok(())
}

/// fctiwz as the interpreter does it (saturating truncation).
fn fctiwz(x: f64) -> i32 {
    if x.is_nan() {
        i32::MIN
    } else {
        x.trunc().max(i32::MIN as f64).min(i32::MAX as f64) as i32
    }
}

/// HighFiveManager::UpdateIdle(int) @0x8033757c
pub fn hf_update_idle(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0xd0);
    if rb(vm, t + 0x1c) == 0 {
        return Ok(());
    }
    let k29 = lfs2(vm, -0x4648);
    let k31 = lfs2(vm, -0x4640);
    let k30 = lfs2(vm, -0x4644);
    let k27 = lfd2(vm, -0x4638);
    let k28 = lfs2(vm, -0x464c);
    let cm = rd(vm, WORLD_MAN + 0x8c);
    let cmgr = rd(vm, cm + 0x18);
    let count = rd(vm, cmgr + 0x74);
    let mut i: u32 = 1;
    while (i as i32) < (count as i32) {
        let cm = rd(vm, WORLD_MAN + 0x8c);
        let cmgr = rd(vm, cm + 0x18);
        let chr = cl(h, vm, sp, "GetCharacter__16CharacterManagerFi", &[cmgr, i])?;
        let next = |i: u32| i.wrapping_add(1);
        if chr == 0 {
            i = next(i);
            continue;
        }
        let r25 = chr + 0x130;
        let ok = cl(h, vm, sp, "IsHighFiveable__14CharacterStateCFv", &[r25])?;
        if ok == 0 {
            i = next(i);
            continue;
        }
        let pc = player(h, vm, sp)?;
        let r24 = pc + 0x130;
        let r22 = r25 + 0x50;
        let r23 = r24 + 0x50;
        cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[r22, r23, sp + 0x30])?;
        cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[r23, r22, sp + 0x20])?;
        let f0 = lfs(vm, sp + 0x34);
        let f1v = lfs(vm, sp + 0x30);
        let f2 = fmuls(f0, f0);
        let f0 = lfs(vm, sp + 0x38);
        let f1v = fmadds(f1v, f1v, f2);
        let d2 = fmadds(f0, f0, f1v);
        let n = fctiwz(d2);
        let dist = fsubs(f64::from_bits(0x4330_0000_0000_0000 | ((n as u32) ^ 0x8000_0000) as u64), k27);
        if fcmp(dist, k28) & LT == 0 {
            i = next(i);
            continue;
        }
        gc(h, vm, sp, "sqrt", &[], &[d2])?;
        let f0 = fs(f1(vm));
        let f4 = lfs(vm, sp + 0x34);
        let f5 = lfs(vm, sp + 0x30);
        let f2 = lfs(vm, r24 + 0x74);
        let f6 = fdivs(k29, f0);
        let f3 = lfs(vm, sp + 0x38);
        let f1v = lfs(vm, r24 + 0x70);
        let f0 = lfs(vm, r24 + 0x78);
        let f4 = fmuls(f4, f6);
        let f5 = fmuls(f5, f6);
        let f3 = fmuls(f3, f6);
        let f2 = fmuls(f4, f2);
        stfs(vm, sp + 0x14, f4);
        stfs(vm, sp + 0x10, f5);
        let f1v = fmadds(f5, f1v, f2);
        stfs(vm, sp + 0x18, f3);
        let f1v = fmadds(f3, f0, f1v);
        gc(h, vm, sp, "rmACos__Ff", &[], &[f1v])?;
        let a = f1(vm);
        if fcmp(a, k30) & LT == 0 {
            i = next(i);
            continue;
        }
        if fcmp(a, k31) & GT == 0 {
            i = next(i);
            continue;
        }
        wr(vm, t + 4, chr);
        let v = lfs(vm, chr + 0x1b0);
        stfs(vm, t + 8, v);
        let anim = rd(vm, chr + 0x18);
        let v = rd(vm, anim + 0x54);
        wr(vm, t + 0xc, v);
        cl(h, vm, sp, "FaceDesiredPos__14CharacterStateFRC9rmVector3", &[r25, r23])?;
        cl(h, vm, sp, "SetIsDoingHighFive__14CharacterStateFb", &[r25, 1])?;
        cl(h, vm, sp, "FaceDesiredPos__14CharacterStateFRC9rmVector3", &[r24, r22])?;
        cl(h, vm, sp, "SetIsDoingHighFive__14CharacterStateFb", &[r24, 1])?;
        let anim = rd(vm, pc + 0x18);
        let v = rd(vm, anim + 0x54);
        wr(vm, t + 0x10, v);
        cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 2])?;
        return Ok(());
    }
    Ok(())
}

/// HighFiveManager::ChangeState(HighFiveManager::HighFiveState) @0x80337bb0
pub fn hf_change_state(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let s = vm.a(1);
    wr(vm, t + 0x14, s);
    wr(vm, t + 0x18, 0);
    if s != 1 {
        return Ok(());
    }
    let c = lfs2(vm, -0x4650);
    wr(vm, t + 4, 0);
    stfs(vm, t + 8, c);
    wr(vm, t + 0xc, 0);
    wr(vm, t + 0x10, 0);
    Ok(())
}

/// HighFiveManager::UpdateStartHighFive(int) @0x803377d8
pub fn hf_update_start(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let c = player(h, vm, sp)?;
    wb(vm, c + 0x1fc, 0);
    cl(h, vm, sp, "PressA_SetVisible__16WorldHudHandlersFi", &[0])?;
    let c = player(h, vm, sp)?;
    let ctrl = rd(vm, c + 0x124);
    if ctrl != 0 {
        cl(h, vm, sp, "ClearHistory__21LocalCharacterControlFv", &[ctrl])?;
    }
    let cc = ctl(h, vm, sp, 0)?;
    cl(h, vm, sp, "SetCurrentControllerState__10ControllerF16EControllerState", &[cc, 0x19])?;
    let c = player(h, vm, sp)?;
    let anim = rd(vm, c + 0x18);
    let ns = "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi";
    cl(h, vm, sp, ns, &[anim, 0xb, 0, u32::MAX])?;
    let other = rd(vm, t + 4);
    let anim = rd(vm, other + 0x18);
    cl(h, vm, sp, ns, &[anim, 0xa, 0, u32::MAX])?;
    cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 3])?;
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x75, 0, 0x64])?;
    Ok(())
}

/// HighFiveManager::UpdateExecuteParticleEffect(int) @0x803378b0
pub fn hf_update_execute(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x50);
    if rd(vm, t + 0x18) <= 0x4b0 {
        return Ok(());
    }
    let pc = player(h, vm, sp)?;
    let other = rd(vm, t + 4);
    let r31 = pc + 0x180;
    cl(h, vm, sp, "rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", &[other + 0x180, r31, sp + 0x30])?;
    let f3 = lfs(vm, sp + 0x30);
    let k = lfs2(vm, -0x4630);
    let f0 = lfs(vm, sp + 0x34);
    let f4 = fmuls(f3, k);
    let f1v = lfs(vm, sp + 0x38);
    let f3 = fmuls(f0, k);
    let kk = lfs2(vm, -0x462c);
    let f2 = fmuls(f1v, k);
    let mgr = lwz13(vm, -0x1ed8);
    stfs(vm, sp + 0x20, f4);
    stfs(vm, sp + 0x24, f3);
    stfs(vm, sp + 0x28, f2);
    let f1v = lfs(vm, r31);
    let f1v = fadds(f1v, f4);
    stfs(vm, sp + 0x20, f1v);
    let f1v = lfs(vm, r31 + 4);
    let f1v = fadds(f1v, f3);
    stfs(vm, sp + 0x24, f1v);
    let f0 = fadds(f1v, kk);
    let f1v = lfs(vm, r31 + 8);
    let f1v = fadds(f1v, f2);
    stfs(vm, sp + 0x24, f0);
    stfs(vm, sp + 0x28, f1v);
    let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8f74, sp + 0x20])?;
    wr(vm, sp + 0x10, g);
    let mgr = lwz13(vm, -0x1ed8);
    cl(h, vm, sp, "DestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 0x10, 0x3e8])?;
    cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 4])?;
    let other = rd(vm, t + 4);
    let flag = rd(vm, other + 0x1e8);
    let sfx = "PlaySFX__5AudioF14AUDIOAEMSBESFXii";
    let a = audio(h, vm, sp)?;
    if flag == 0 {
        cl(h, vm, sp, sfx, &[a, 0xd, 0, 0x64])?;
    } else {
        cl(h, vm, sp, sfx, &[a, 0xf, 0, 0x64])?;
    }
    let a = audio(h, vm, sp)?;
    cl(h, vm, sp, "PlaySFX__5AudioFi15AUDIOWIIMOTESFXi", &[a, 0, 3, 0x1000])?;
    Ok(())
}

/// HighFiveManager::UpdateFinishHighFive(int) @0x80337a00
pub fn hf_update_finish(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x30);
    if rd(vm, t + 0x18) <= 0x7d0 {
        return Ok(());
    }
    let c = ctl(h, vm, sp, 0)?;
    cl(h, vm, sp, "PopState__10ControllerFv", &[c])?;
    let pc = player(h, vm, sp)?;
    cl(h, vm, sp, "SetIsDoingHighFive__14CharacterStateFb", &[pc + 0x130, 0])?;
    let pc = player(h, vm, sp)?;
    let anim = rd(vm, pc + 0x18);
    let st = rd(vm, t + 0x10);
    let ns = "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi";
    cl(h, vm, sp, ns, &[anim, st, 1, u32::MAX])?;
    let other = rd(vm, t + 4);
    let r30 = other + 0x130;
    cl(h, vm, sp, "SetIsDoingHighFive__14CharacterStateFb", &[r30, 0])?;
    let ang = lfs(vm, t + 8);
    gc(h, vm, sp, "Set__7rmAngleFf", &[r30 + 0x80], &[ang])?;
    cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x10, r30 + 0x80])?;
    let (f2, f1v, f0) = (lfs(vm, sp + 0x18), lfs(vm, sp + 0x14), lfs(vm, sp + 0x10));
    stfs(vm, r30 + 0x70, f0);
    stfs(vm, r30 + 0x74, f1v);
    stfs(vm, r30 + 0x78, f2);
    let other = rd(vm, t + 4);
    let st = rd(vm, t + 0xc);
    let anim = rd(vm, other + 0x18);
    cl(h, vm, sp, ns, &[anim, st, 1, u32::MAX])?;
    let ok = cl(h, vm, sp, "IsHighFiveable__14CharacterStateCFv", &[r30])?;
    if ok != 0 {
        let prof = lwz13(vm, -0x1e7c);
        let other = rd(vm, t + 4);
        let idx = cl(h, vm, sp, "GetNpcIndex__16CharacterProfileFP9Character", &[prof, other])?;
        let prof = lwz13(vm, -0x1e7c);
        let r = cl(h, vm, sp, "MarkHighFiveObtained__16CharacterProfileFi", &[prof, idx])?;
        wb(vm, t + 0x1d, r as u8);
        if r & 0xff != 0 {
            let m = rd(vm, t + 0x20);
            cl(h, vm, sp, "AddMarblesEvent__8WorldManFii", &[WORLD_MAN, 0, m])?;
        }
    }
    cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 5])?;
    let am = lwz13(vm, -0x1c0c);
    cl(h, vm, sp, "EvaluatePopup__20AccessibilityManagerFQ25Enums22AccessibilityPopupTypei", &[am, 6, 0])?;
    Ok(())
}

/// HighFiveManager::UpdateHighFiveHud(int) @0x80337b50
pub fn hf_update_hud(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if rb(vm, t + 0x1d) != 0 {
        if rd(vm, t + 0x18) > 0x2ee {
            wb(vm, t + 0x1d, 0);
        }
        return Ok(());
    }
    cl(h, vm, sp, "ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", &[t, 1])?;
    let c = player(h, vm, sp)?;
    wb(vm, c + 0x1fc, 1);
    Ok(())
}

/// HighFiveManager::HighFiveCallback(const EA::Conga::CongaOutput*, void*) @0x80337bdc
pub fn hf_callback(h: &mut MgHost, vm: &mut V) -> R {
    let out = vm.a(0);
    let user = vm.a(1);
    let sp = fsp(vm, 0x10);
    let cm = rd(vm, WORLD_MAN + 0x8c);
    if rb(vm, cm + 0x24) != 0 {
        return Ok(());
    }
    let pc = player(h, vm, sp)?;
    let conv = cl(h, vm, sp, "IsInConversation__14CharacterStateFv", &[pc + 0x130])?;
    if conv != 0 {
        return Ok(());
    }
    let am = lwz13(vm, -0x1c0c);
    if rb(vm, am + 4) != 0 {
        return Ok(());
    }
    cl(h, vm, sp, "ProcessHighFiveGesture__15HighFiveManagerFPCQ32EA5Conga11CongaOutput", &[user, out])?;
    Ok(())
}

/// HighFiveManager::ProcessHighFiveGesture(const EA::Conga::CongaOutput*) @0x80337c60
pub fn hf_process_gesture(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let out = vm.a(1);
    let sp = fsp(vm, 0x20);
    if rb(vm, t) == 0 {
        return Ok(());
    }
    let pc = player(h, vm, sp)?;
    if rb(vm, pc + 0x12c) == 0 {
        return Ok(());
    }
    let conv = cl(h, vm, sp, "IsInConversation__14CharacterStateFv", &[pc + 0x130])?;
    if conv != 0 {
        return Ok(());
    }
    let d = cl(h, vm, sp, "IsDoingHighFive__14CharacterStateCFv", &[pc + 0x130])?;
    if d != 0 {
        return Ok(());
    }
    let ctrl = rd(vm, pc + 0x124);
    let key = rd(vm, out + 8);
    if ctrl == 0 {
        return Ok(());
    }
    let want = rd(vm, ctrl + 0xa8);
    if key as i32 != want as i32 {
        return Ok(());
    }
    let d = cl(h, vm, sp, "IsDoingHighFive__14CharacterStateCFv", &[pc + 0x130])?;
    if d != 0 {
        return Ok(());
    }
    wb(vm, t + 0x1c, 1);
    Ok(())
}

// =====================================================================================================================
// registration
// =====================================================================================================================

pub const PORTS: &[crate::mgvm::ports::Port] = &[
    ("__ct__16DribblingManagerFv", dm_ctor),
    ("StartUp__16DribblingManagerFv", dm_startup),
    ("ShutDown__16DribblingManagerFv", dm_shutdown),
    ("StartBeatGame__16DribblingManagerFv", dm_start_beat_game),
    ("StopBeatGame__16DribblingManagerFv", dm_stop_beat_game),
    ("RestartBeatGame__16DribblingManagerFv", dm_restart_beat_game),
    ("ThrowBall__16DribblingManagerFv", dm_throw_ball),
    ("CharacterThrowBall__16DribblingManagerFv", dm_character_throw_ball),
    ("CameraFinishedTransitionCallback__16DribblingManagerFPv", dm_camera_finished),
    ("Update__16DribblingManagerFi", dm_update),
    ("UpdateDribbling__16DribblingManagerFi", dm_update_dribbling),
    ("UpdateThrow__16DribblingManagerFi", dm_update_throw),
    ("UpdateThrownBall__16DribblingManagerFi", dm_update_thrown_ball),
    ("DribbleProcess__16DribblingManagerFPCQ32EA5Conga11CongaOutput", dm_dribble_process),
    ("DribbleCallback__16DribblingManagerFPCQ32EA5Conga11CongaOutputPv", dm_dribble_callback),
    ("Draw__16DribblingManagerFRQ23Ren12SceneContext", dm_draw),
    ("GetBallMatrix__16DribblingManagerCFP9rmMatrix4", dm_get_ball_matrix),
    ("CalculateRenderMatrix__16DribblingManagerFv", dm_calc_render_matrix),
    ("ChangeArea__16DribblingManagerFQ25Enums8AreaType", dm_change_area),
    ("StartIdleParticle__16DribblingManagerFv", dm_start_idle_particle),
    ("StopIdleParticle__16DribblingManagerFv", dm_stop_idle_particle),
    ("OpenPauseMenu__16DribblingManagerFv", dm_open_pause_menu),
    ("ClosePauseMenu__16DribblingManagerFv", dm_close_pause_menu),
    ("OnPauseMenuLoaded__16DribblingManagerFv", dm_on_pause_menu_loaded),
    ("OnPauseContinue__16DribblingManagerFv", dm_on_pause_continue),
    ("OnPauseQuit__16DribblingManagerFv", dm_on_pause_quit),
    ("OnPauseReset__16DribblingManagerFv", dm_on_pause_reset),
    ("__ct__13FreeThrowBallFPC9rmVector3f", ball_ctor),
    ("__dt__13FreeThrowBallFv", ball_dtor),
    ("Pass__13FreeThrowBallFPC9rmVector3Ui", ball_pass),
    ("Grab__13FreeThrowBallFPC9rmVector3", ball_grab),
    ("Update__13FreeThrowBallFi", ball_update),
    ("Draw__13FreeThrowBallFv", ball_draw),
    ("Load__13FreeThrowBallFv", ball_load),
    ("Unload__13FreeThrowBallFv", ball_unload),
    ("InitializeThrow__13FreeThrowBallFff", ball_initialize_throw),
    ("__ct__18FreeThrowBallCourtFv", court_ctor),
    ("__dt__18FreeThrowBallCourtFv", court_dtor),
    ("Initialize__18FreeThrowBallCourtFPC9rmVector3PC9rmVector3", court_initialize),
    ("UnInitialize__18FreeThrowBallCourtFv", court_uninitialize),
    ("__ct__35FreeThrowCharacterCollisionListenerFP9Character", chr_listener_ctor),
    ("__dt__35FreeThrowCharacterCollisionListenerFv", chr_listener_dtor),
    ("CharacterInteractionCallback__35FreeThrowCharacterCollisionListenerFPC15PhysicsUserDataPC19PhysicsContactPoint", chr_listener_character_interaction),
    ("ObjectInteractionCallback__35FreeThrowCharacterCollisionListenerFPC15PhysicsUserDataPC19PhysicsContactPoint", chr_listener_object_interaction),
    ("OverlapAddedCallback__35FreeThrowCharacterCollisionListenerFPC15PhysicsUserData", chr_listener_overlap_added),
    ("OverlapRemovedCallback__35FreeThrowCharacterCollisionListenerFPC15PhysicsUserDatab", chr_listener_overlap_removed),
    ("__ct__30FreeThrowBallCollisionListenerFP13FreeThrowBall", ball_listener_ctor),
    ("__dt__30FreeThrowBallCollisionListenerFv", ball_listener_dtor),
    ("__ct__15HighFiveManagerFv", hf_ctor),
    ("__dt__15HighFiveManagerFv", hf_dtor),
    ("Initialize__15HighFiveManagerFv", hf_initialize),
    ("StartUp__15HighFiveManagerFv", hf_startup),
    ("ShutDown__15HighFiveManagerFv", hf_shutdown),
    ("Update__15HighFiveManagerFi", hf_update),
    ("UpdateIdle__15HighFiveManagerFi", hf_update_idle),
    ("ChangeState__15HighFiveManagerFQ215HighFiveManager13HighFiveState", hf_change_state),
    ("Throw__13FreeThrowBallFPC9rmVector3", ball_throw),
    ("ContactConfirmedCallback__30FreeThrowBallCollisionListenerFPC15PhysicsUserDataUiPC19PhysicsContactPointf", ball_listener_contact),
    ("UpdateStartHighFive__15HighFiveManagerFi", hf_update_start),
    ("UpdateExecuteParticleEffect__15HighFiveManagerFi", hf_update_execute),
    ("UpdateFinishHighFive__15HighFiveManagerFi", hf_update_finish),
    ("UpdateHighFiveHud__15HighFiveManagerFi", hf_update_hud),
    ("HighFiveCallback__15HighFiveManagerFPCQ32EA5Conga11CongaOutputPv", hf_callback),
    ("ProcessHighFiveGesture__15HighFiveManagerFPCQ32EA5Conga11CongaOutput", hf_process_gesture),
];
