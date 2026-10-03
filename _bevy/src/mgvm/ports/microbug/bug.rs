//! Bug Hunt bugs: `MicroBug` (microbug.cpp) and `MicroBugManager` (microbugmanager.cpp), plus the two `EA::Math` vector
//! helpers that live in the same translation unit.
//!
//! Calls to other original functions go through `callf`, which makes the callee see the stack pointer the original
//! would have handed it (the port's notional frame is `r1 - frame size`), so pointers to the caller's locals and
//! stack-passed arguments (`Matrix44::Set`) land at the same guest addresses as in the original.
#![allow(unused)]
use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
pub type R = Result<(), String>;

// ---- callees (mangled symbols) ----
const AS_VEC3: &str = "__as__9rmVector3FRC9rmVector3";
const RANDOM: &str = "Random__Q22EA4MathFv";
const SQRT: &str = "sqrt";
const CREATE_PARTFX: &str = "CreatePartFx__13PartFxManagerFPCcRC9rmVector3";
const GET_PARTFX: &str = "GetPartFx__13PartFxManagerF4GUID";
const DESTROY_PARTFX: &str = "DestroyPartFx__13PartFxManagerF4GUIDi";
const ENABLE_EMITTERS: &str = "EnableEmitters__6PartFxFb";
const SET_POS: &str = "SetPos__6PartFxFRC9rmVector3";
const MEM_ALLOC: &str = "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc";
const MEM_FREE: &str = "Free__6MemMgrFPv";
const SHADOW_CT: &str = "__ct__26MicroBugShadowRenderEntityFPQ24EAGL5Model";
const ADD_ENTITY: &str = "AddEntity__Q23Ren5SceneFiPQ23Ren6Entity";
const REMOVE_ENTITY: &str = "RemoveEntity__Q23Ren5SceneFiPQ23Ren6Entity";
const CALC_MATRIX: &str = "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4";
const MATRIX_SET: &str = "Set__Q32EA4Math8Matrix44Fffffffffffffffff";
const RM_ADD: &str = "rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3";
const RM_RAND_RANGE: &str = "rmRandRange__Fii";
const NORMALIZE: &str = "Normalize__Q22EA4MathFRCQ32EA4Math7Vector3RQ32EA4Math7Vector3";
const MULT: &str = "Mult__Q22EA4MathFRCQ32EA4Math7Vector3fRQ32EA4Math7Vector3";
const GET_LOADED_MODEL: &str = "GetLoadedModel__12AssetManagerFPCcPii";
const CM_CT: &str = "__ct__Q23Ren11CachedModelFv";
const CM_DT: &str = "__dt__Q23Ren11CachedModelFv";
const CM_SET_SCALE: &str = "SetScaleMatrix__Q23Ren11CachedModelFv";
const GUID_CT: &str = "__ct__4GUIDFUsUs";
const BUG_DT: &str = "__dt__8MicroBugFv";
const BUG_INIT: &str = "Initialize__8MicroBugFPQ23Ren11CachedModel";
const BUG_UNINIT: &str = "UnInitialize__8MicroBugFv";
const BUG_UPDATE: &str = "Update__8MicroBugFi";
const BUG_DRAW: &str = "Draw__8MicroBugFv";
const MGR_REMOVE_ALL: &str = "RemoveAllBugs__15MicroBugManagerFv";
const MGR_ADD_BUG: &str = "AddBug__15MicroBugManagerFRC9rmVector3Q28MicroBug7BugType";

// ---- guest memory / SDA helpers ----
fn rd8(vm: &mut V, a: u32) -> u8 {
    vm.st.mem.r8(a)
}
fn wr8(vm: &mut V, a: u32, v: u8) {
    vm.st.mem.w8(a, v)
}
fn rd32(vm: &mut V, a: u32) -> u32 {
    vm.st.mem.r32(a)
}
fn wr32(vm: &mut V, a: u32, v: u32) {
    vm.st.mem.w32(a, v)
}
fn rdf(vm: &mut V, a: u32) -> f32 {
    vm.st.mem.rf32(a)
}
fn wrf(vm: &mut V, a: u32, v: f32) {
    vm.st.mem.wf32(a, v)
}
/// Address `off(r13)`.
fn sda(vm: &V, off: i32) -> u32 {
    vm.st.cpu.r[13].wrapping_add(off as u32)
}
/// `lwz rX, off(r13)`.
fn g13(vm: &mut V, off: i32) -> u32 {
    let a = sda(vm, off);
    vm.st.mem.r32(a)
}
/// `lfs fX, off(r2)` (small-data constant).
fn c2(vm: &mut V, off: i32) -> f32 {
    let a = vm.st.cpu.r[2].wrapping_add(off as u32);
    vm.st.mem.rf32(a)
}
/// The stack pointer of a function that did `stwu r1,-size(r1)`.
fn fsp(vm: &V, size: u32) -> u32 {
    vm.st.cpu.r[1].wrapping_sub(size)
}

// ---- single-precision arithmetic exactly as cpu.rs does it (compute in f64, round to f32) ----
fn fadds(a: f32, b: f32) -> f32 {
    (a as f64 + b as f64) as f32
}
fn fsubs(a: f32, b: f32) -> f32 {
    (a as f64 - b as f64) as f32
}
fn fmuls(a: f32, c: f32) -> f32 {
    (a as f64 * c as f64) as f32
}
fn fdivs(a: f32, b: f32) -> f32 {
    (a as f64 / b as f64) as f32
}
/// fmadds: a*c + b, fused.
fn fmadds(a: f32, c: f32, b: f32) -> f32 {
    (a as f64).mul_add(c as f64, b as f64) as f32
}
/// The 0x43300000 / xoris int-to-float idiom followed by `fsubs` of the magic constant.
fn itof(v: u32) -> f32 {
    (v as i32 as f64) as f32
}
/// `Random() % 100000 - 50000` converted to float (the divwu/mullw/subf + addis/addi + xoris sequence).
fn rand_f(r: u32) -> f32 {
    itof((r % 100000).wrapping_sub(0x10000).wrapping_add(0x3cb0))
}

/// Call the original `name` from a port whose own frame pointer is `sp` (see module doc).
fn callf(h: &mut MgHost, vm: &mut V, sp: u32, name: &str, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
    let r1 = vm.st.cpu.r[1];
    vm.st.cpu.r[1] = sp.wrapping_add(0x200);
    let r = vm.call_by_name(h, name, args, fargs);
    vm.st.cpu.r[1] = r1;
    r
}

/// Same for a call through a computed address (virtual call).
fn callf_addr(h: &mut MgHost, vm: &mut V, sp: u32, addr: u32, args: &[u32], fargs: &[f64]) -> Result<u32, String> {
    let r1 = vm.st.cpu.r[1];
    vm.st.cpu.r[1] = sp.wrapping_add(0x200);
    let r = vm.call(h, addr, args, fargs);
    vm.st.cpu.r[1] = r1;
    r
}

// =====================================================================================================================
// MicroBug
// =====================================================================================================================

/// MicroBug::MicroBug(MicroBug::BugType, int, const rmVector3&) @0x8032e718
pub fn microbug_ct(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let (this, ty, idx, pos) = (vm.a(0), vm.a(1), vm.a(2), vm.a(3));
    wr8(vm, this + 4, 0);
    let g = g13(vm, -0x2288);
    wr32(vm, this + 0x40, g);
    let c = c2(vm, -0x48c8);
    wrf(vm, this + 0x44, c);
    wr32(vm, this + 0x48, 0);
    wr32(vm, this + 0x4c, 0);
    wr32(vm, this, idx);
    wr32(vm, this + 8, ty);
    callf(h, vm, sp, AS_VEC3, &[this + 0x10, pos], &[])?;
    callf(h, vm, sp, AS_VEC3, &[this + 0x20, this + 0x10], &[])?;
    let y = rdf(vm, this + 0x24);
    let k = c2(vm, -0x48c4);
    wrf(vm, this + 0x24, fadds(y, k));
    vm.ret(this);
    vm.fret(y);
    Ok(())
}

/// MicroBug::~MicroBug() @0x8032e798
pub fn microbug_dt(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let (this, flags) = (vm.a(0), vm.a(1));
    if this != 0 {
        callf(h, vm, sp, BUG_UNINIT, &[this], &[])?;
        wr32(vm, this, 0xffff_ffff);
        if (flags as i32) > 0 {
            callf(h, vm, sp, MEM_FREE, &[this], &[])?;
        }
    }
    vm.ret(this);
    Ok(())
}

/// MicroBug::Initialize(Ren::CachedModel*) @0x8032e7f4
pub fn microbug_init(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x40);
    let (this, model) = (vm.a(0), vm.a(1));
    if rd8(vm, this + 4) != 0 {
        return Ok(());
    }
    wr8(vm, this + 4, 1);
    let r1 = callf(h, vm, sp, RANDOM, &[], &[])?;
    let f31 = rand_f(r1);
    let r2 = callf(h, vm, sp, RANDOM, &[], &[])?;
    let kind = rd32(vm, this + 8);
    let c = c2(vm, -0x48c8);
    wrf(vm, this + 0x34, c);
    wrf(vm, this + 0x38, f31);
    wrf(vm, this + 0x30, rand_f(r2));
    let mgr = g13(vm, -0x1ed8);
    let name = rd32(vm, 0x804d_8978u32.wrapping_add(kind << 2));
    let guid = callf(h, vm, sp, CREATE_PARTFX, &[mgr, name, this + 0x20], &[])?;
    wr32(vm, this + 0x40, guid);
    wr32(vm, sp + 8, guid);
    let mgr = g13(vm, -0x1ed8);
    let fx = callf(h, vm, sp, GET_PARTFX, &[mgr, sp + 8], &[])?;
    callf(h, vm, sp, ENABLE_EMITTERS, &[fx, 1], &[])?;
    let ent_model = rd32(vm, model + 0x44);
    let pool = g13(vm, -0x43fc);
    let mut p = callf(h, vm, sp, MEM_ALLOC, &[0x4c, pool, 0, 0x804d_8994], &[])?;
    if p != 0 {
        p = callf(h, vm, sp, SHADOW_CT, &[p, ent_model], &[])?;
    }
    wr32(vm, this + 0x48, p);
    let holder = g13(vm, -0x1cfc);
    let scene = rd32(vm, holder + 8);
    callf(h, vm, sp, ADD_ENTITY, &[scene, 0, p], &[])?;
    Ok(())
}

/// MicroBug::UnInitialize() @0x8032e94c
pub fn microbug_uninit(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let this = vm.a(0);
    if rd8(vm, this + 4) != 0 {
        let guid = rd32(vm, this + 0x40);
        wr32(vm, sp + 8, guid);
        let mgr = g13(vm, -0x1ed8);
        callf(h, vm, sp, DESTROY_PARTFX, &[mgr, sp + 8, 0], &[])?;
        let none = g13(vm, -0x2288);
        let ent = rd32(vm, this + 0x48);
        wr32(vm, this + 0x40, none);
        let holder = g13(vm, -0x1cfc);
        let scene = rd32(vm, holder + 8);
        callf(h, vm, sp, REMOVE_ENTITY, &[scene, 0, ent], &[])?;
        let ent = rd32(vm, this + 0x48);
        vm.ret(ent);
        if ent != 0 {
            // virtual deleting destructor: vtable slot 2, flag 1
            let vt = rd32(vm, ent);
            let f = rd32(vm, vt + 8);
            callf_addr(h, vm, sp, f, &[ent, 1], &[])?;
            wr32(vm, this + 0x48, 0);
        }
        wr8(vm, this + 4, 0);
    }
    Ok(())
}

/// MicroBug::Draw() @0x8032e9e8
pub fn microbug_draw(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x70);
    let this = vm.a(0);
    if rd8(vm, this + 4) == 0 {
        return Ok(());
    }
    let a = rd32(vm, 0x805e_8320 + 0x8c);
    let area = rd32(vm, a + 8);
    callf(h, vm, sp, CALC_MATRIX, &[area, this + 0x20, sp + 0x28], &[])?;
    // the second half of the matrix is copied to the outgoing stack-argument area (8(r1)..)
    for i in 0..8u32 {
        let v = rdf(vm, sp + 0x48 + 4 * i);
        wrf(vm, sp + 8 + 4 * i, v);
    }
    let ent = rd32(vm, this + 0x48);
    let mut fa = [0f64; 8];
    for i in 0..8u32 {
        fa[i as usize] = rdf(vm, sp + 0x28 + 4 * i) as f64;
    }
    callf(h, vm, sp, MATRIX_SET, &[ent + 0xc], &fa)?;
    Ok(())
}

/// MicroBug::Update(int) @0x8032eaa4
pub fn microbug_update(h: &mut MgHost, vm: &mut V) -> R {
    let r1 = vm.st.cpu.r[1];
    // clrlwi r11,r1,28 / subfic r11,r11,-0x90 / stwux r1,r1,r11
    let sp = r1.wrapping_sub(0x90).wrapping_sub(r1 & 0xf);
    let (this, dt) = (vm.a(0), vm.a(1));
    if rd8(vm, this + 4) == 0 {
        return Ok(());
    }
    let a = c2(vm, -0x48b8);
    let z = rdf(vm, this + 0x28);
    let tz = rdf(vm, this + 0x18);
    let x = rdf(vm, this + 0x20);
    let dz = fsubs(z, tz);
    let tx = rdf(vm, this + 0x10);
    let f31 = fdivs(itof(dt), a);
    let dx = fsubs(x, tx);
    let dz2 = fmuls(dz, dz);
    let s = fmadds(dx, dx, dz2);
    callf(h, vm, sp, SQRT, &[], &[s as f64])?;
    let dist = vm.st.cpu.f[1] as f32;
    let b = c2(vm, -0x48b4);
    let ga = sda(vm, -0x2280);
    let g = rdf(vm, ga);
    let thr = fmuls(b, g);
    if dist > thr {
        let f2 = rdf(vm, this + 0x10);
        let f5 = fsubs(g, thr);
        let f1 = rdf(vm, this + 0x20);
        let f3 = fsubs(dist, thr);
        let c = c2(vm, -0x48c8);
        let vx = fsubs(f2, f1);
        let f30 = fdivs(f3, f5);
        wrf(vm, sp + 0x40, vx);
        let (p, q) = (rdf(vm, this + 0x14), rdf(vm, this + 0x24));
        wrf(vm, sp + 0x44, fsubs(p, q));
        let (p, q) = (rdf(vm, this + 0x18), rdf(vm, this + 0x28));
        let vz = fsubs(p, q);
        wrf(vm, sp + 0x44, c);
        wrf(vm, sp + 0x48, vz);
        callf(h, vm, sp, NORMALIZE, &[sp + 0x40, sp + 0x40], &[])?;
        let k = fmuls(f30, f31);
        let k0 = c2(vm, -0x48b0);
        let k = fmuls(k0, k);
        callf(h, vm, sp, MULT, &[sp + 0x40, sp + 0x40], &[k as f64])?;
        callf(h, vm, sp, RM_ADD, &[sp + 0x40, this + 0x30, this + 0x30], &[])?;
        let c = c2(vm, -0x48c8);
        wrf(vm, this + 0x34, c);
        callf(h, vm, sp, NORMALIZE, &[this + 0x30, this + 0x30], &[])?;
    } else {
        let r = callf(h, vm, sp, RANDOM, &[], &[])?;
        let f30 = rand_f(r);
        let r = callf(h, vm, sp, RANDOM, &[], &[])?;
        let c = c2(vm, -0x48c8);
        wrf(vm, sp + 0x34, c);
        wrf(vm, sp + 0x38, f30);
        wrf(vm, sp + 0x30, rand_f(r));
        callf(h, vm, sp, NORMALIZE, &[sp + 0x30, sp + 0x30], &[])?;
        let k0 = c2(vm, -0x48b0);
        let k = fmuls(k0, f31);
        callf(h, vm, sp, MULT, &[sp + 0x30, sp + 0x30], &[k as f64])?;
        callf(h, vm, sp, RM_ADD, &[this + 0x30, sp + 0x30, this + 0x30], &[])?;
        let c = c2(vm, -0x48c8);
        wrf(vm, this + 0x34, c);
        callf(h, vm, sp, NORMALIZE, &[this + 0x30, this + 0x30], &[])?;
    }
    // .L_8032ec94: velocity (copied raw to a local) scaled by dt and added to the position
    let w0 = rd32(vm, this + 0x30);
    let w1 = rd32(vm, this + 0x34);
    let c = c2(vm, -0x48b4);
    wr32(vm, sp + 0x20, w0);
    let k = fmuls(c, f31);
    wr32(vm, sp + 0x24, w1);
    let w2 = rd32(vm, this + 0x38);
    wr32(vm, sp + 0x28, w2);
    callf(h, vm, sp, MULT, &[sp + 0x20, sp + 0x20], &[k as f64])?;
    callf(h, vm, sp, RM_ADD, &[this + 0x20, sp + 0x20, this + 0x20], &[])?;
    let f3 = rdf(vm, this + 0x44);
    let f2 = rdf(vm, this + 0x24);
    let k1 = c2(vm, -0x48c4);
    let f0 = rdf(vm, this + 0x14);
    let ny = fmadds(f3, f31, f2);
    let cnt = rd32(vm, this + 0x4c);
    let lim = fadds(k1, f0);
    let cnt = cnt.wrapping_add(dt);
    wrf(vm, this + 0x24, ny);
    wr32(vm, this + 0x4c, cnt);
    if ny <= lim {
        // hit the ground: reset the timer and pick a new bounce
        wr32(vm, this + 0x4c, 0);
        let r = callf(h, vm, sp, RM_RAND_RANGE, &[15, 150], &[])?;
        let f3 = itof(r);
        let f2 = c2(vm, -0x48ac);
        let f1 = c2(vm, -0x48c4);
        let f0 = rdf(vm, this + 0x14);
        let f0 = fadds(f1, f0);
        let f1 = fdivs(f3, f2);
        wrf(vm, this + 0x24, f0);
        wrf(vm, this + 0x44, f1);
    } else {
        let r = callf(h, vm, sp, RANDOM, &[], &[])?;
        if r % 75 == 0 {
            let cnt = rd32(vm, this + 0x4c) as i32;
            let max = g13(vm, -0x2284) as i32;
            if cnt < max {
                let r = callf(h, vm, sp, RM_RAND_RANGE, &[15, 150], &[])?;
                let f1 = itof(r);
                let f0 = c2(vm, -0x48ac);
                wrf(vm, this + 0x44, fdivs(f1, f0));
            }
        }
    }
    let f1 = rdf(vm, this + 0x44);
    let f0 = c2(vm, -0x48a8);
    if f1 < f0 {
        wrf(vm, this + 0x44, f0);
    } else {
        let k = c2(vm, -0x48a4);
        wrf(vm, this + 0x44, fmadds(k, f31, f1));
    }
    let guid = rd32(vm, this + 0x40);
    let mgr = g13(vm, -0x1ed8);
    wr32(vm, sp + 0x10, guid);
    let fx = callf(h, vm, sp, GET_PARTFX, &[mgr, sp + 0x10], &[])?;
    callf(h, vm, sp, SET_POS, &[fx, this + 0x20], &[])?;
    callf(h, vm, sp, ENABLE_EMITTERS, &[fx, 1], &[])?;
    Ok(())
}

/// EA::Math::Normalize(const Vector3&, Vector3&) @0x8032ee24
pub fn math_normalize(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let (src, dst) = (vm.a(0), vm.a(1));
    let y = rdf(vm, src + 4);
    let y2 = fmuls(y, y);
    let x = rdf(vm, src);
    let z = rdf(vm, src + 8);
    let s = fmadds(x, x, y2);
    let s = fmadds(z, z, s);
    callf(h, vm, sp, SQRT, &[], &[s as f64])?;
    let len = vm.st.cpu.f[1] as f32;
    let one = c2(vm, -0x48c4);
    let x = rdf(vm, src);
    let y = rdf(vm, src + 4);
    let inv = fdivs(one, len);
    let z = rdf(vm, src + 8);
    let rx = fmuls(x, inv);
    let ry = fmuls(y, inv);
    let rz = fmuls(z, inv);
    wrf(vm, dst, rx);
    wrf(vm, dst + 4, ry);
    wrf(vm, dst + 8, rz);
    vm.fret(ry);
    Ok(())
}

/// EA::Math::Mult(const Vector3&, float, Vector3&) @0x8032eea4
pub fn math_mult(h: &mut MgHost, vm: &mut V) -> R {
    let (v, dst) = (vm.a(0), vm.a(1));
    let s = vm.st.cpu.f[1];
    let x = rdf(vm, v);
    let y = rdf(vm, v + 4);
    let z = rdf(vm, v + 8);
    wrf(vm, dst, (x as f64 * s) as f32);
    wrf(vm, dst + 4, (y as f64 * s) as f32);
    wrf(vm, dst + 8, (z as f64 * s) as f32);
    Ok(())
}

/// MicroBug::SetBugData(int, float) @0x8032eecc
pub fn microbug_set_bug_data(h: &mut MgHost, vm: &mut V) -> R {
    let (n, f) = (vm.a(0), vm.st.cpu.f[1] as f32);
    let a = sda(vm, -0x2284);
    wr32(vm, a, n);
    let a = sda(vm, -0x2280);
    wrf(vm, a, f);
    Ok(())
}

/// __sinit_\microbug_cpp() @0x8032ef98
pub fn sinit_microbug(h: &mut MgHost, vm: &mut V) -> R {
    let sp = vm.st.cpu.r[1]; // frameless: tail call into GUID::GUID(unsigned short, unsigned short)
    let this = sda(vm, -0x2288);
    callf(h, vm, sp, GUID_CT, &[this, 0xffff, 0], &[])?;
    Ok(())
}

// =====================================================================================================================
// MicroBugManager  (+4 .. +0xc8: 50 bug pointers, +0xcc: count, +0: initialized)
// =====================================================================================================================

/// MicroBugManager::MicroBugManager() @0x80331104
pub fn mgr_ct(h: &mut MgHost, vm: &mut V) -> R {
    let this = vm.a(0);
    wr8(vm, this, 0);
    wr32(vm, this + 0xcc, 0);
    for i in 0..50u32 {
        wr32(vm, this + 4 + 4 * i, 0);
    }
    Ok(())
}

/// MicroBugManager::~MicroBugManager() @0x80331130
pub fn mgr_dt(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let (this, flags) = (vm.a(0), vm.a(1));
    if this != 0 {
        callf(h, vm, sp, MGR_REMOVE_ALL, &[this], &[])?;
        if (flags as i32) > 0 {
            callf(h, vm, sp, MEM_FREE, &[this], &[])?;
        }
    }
    vm.ret(this);
    Ok(())
}

/// MicroBugManager::Initialize() @0x80331184
pub fn mgr_init(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let this = vm.a(0);
    let mut r3 = this;
    let mut i: u32 = 0;
    while (i as i32) < rd32(vm, this + 0xcc) as i32 {
        r3 = rd32(vm, this + 4 + 4 * i);
        if r3 != 0 {
            let model = g13(vm, -0x225c);
            r3 = callf(h, vm, sp, BUG_INIT, &[r3, model], &[])?;
        }
        i += 1;
    }
    wr8(vm, this, 1);
    vm.ret(r3);
    Ok(())
}

/// MicroBugManager::UnInitialize() @0x803311f4
pub fn mgr_uninit(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let this = vm.a(0);
    let mut r3 = this;
    let mut i: u32 = 0;
    while (i as i32) < rd32(vm, this + 0xcc) as i32 {
        r3 = rd32(vm, this + 4 + 4 * i);
        if r3 != 0 {
            r3 = callf(h, vm, sp, BUG_UNINIT, &[r3], &[])?;
        }
        i += 1;
    }
    wr8(vm, this, 0);
    vm.ret(r3);
    Ok(())
}

/// MicroBugManager::LoadAssets() @0x80331260
pub fn mgr_load_assets(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let am = g13(vm, -0x1ca4);
    let name = g13(vm, -0x4a58);
    let model = callf(h, vm, sp, GET_LOADED_MODEL, &[am, name, 0, 0], &[])?;
    let pool = g13(vm, -0x43fc);
    let mut p = callf(h, vm, sp, MEM_ALLOC, &[0x4c, pool, 0, 0x804c_c724], &[])?;
    if p != 0 {
        p = callf(h, vm, sp, CM_CT, &[p], &[])?;
    }
    let a = sda(vm, -0x225c);
    wr32(vm, a, p);
    wr32(vm, p.wrapping_add(0x44), model);
    callf(h, vm, sp, CM_SET_SCALE, &[p], &[])?;
    let a = sda(vm, -0x2258);
    wr8(vm, a, 1);
    Ok(())
}

/// MicroBugManager::UnloadAssets() @0x803312d4
pub fn mgr_unload_assets(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let p = g13(vm, -0x225c);
    vm.ret(p);
    if p != 0 {
        callf(h, vm, sp, CM_DT, &[p, 1], &[])?;
        let a = sda(vm, -0x225c);
        wr32(vm, a, 0);
    }
    let a = sda(vm, -0x2258);
    wr8(vm, a, 0);
    Ok(())
}

/// MicroBugManager::AddBug(const rmVector3&) @0x80331314
pub fn mgr_add_bug(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let (this, pos) = (vm.a(0), vm.a(1));
    let ty = callf(h, vm, sp, RM_RAND_RANGE, &[0, 2], &[])?;
    callf(h, vm, sp, MGR_ADD_BUG, &[this, pos, ty], &[])?;
    Ok(())
}

/// MicroBugManager::AddBug(const rmVector3&, MicroBug::BugType) @0x80331364
pub fn mgr_add_bug_typed(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let (this, pos, ty) = (vm.a(0), vm.a(1), vm.a(2));
    let mut p = callf(h, vm, sp, MEM_ALLOC, &[0x50, 0, 0, 0x804d_8b2d], &[])?;
    if p != 0 {
        let n = rd32(vm, this + 0xcc);
        p = callf(h, vm, sp, "__ct__8MicroBugFQ28MicroBug7BugTypeiRC9rmVector3", &[p, ty, n, pos], &[])?;
    }
    let n = rd32(vm, this + 0xcc);
    wr32(vm, this.wrapping_add(n << 2).wrapping_add(4), p);
    if rd8(vm, this) != 0 {
        let n = rd32(vm, this + 0xcc);
        let model = g13(vm, -0x225c);
        let bug = rd32(vm, this.wrapping_add(n << 2).wrapping_add(4));
        callf(h, vm, sp, BUG_INIT, &[bug, model], &[])?;
    }
    let n = rd32(vm, this + 0xcc);
    wr32(vm, this + 0xcc, n.wrapping_add(1));
    vm.ret(n);
    Ok(())
}

/// MicroBugManager::RemoveBug(int) @0x8033140c
pub fn mgr_remove_bug(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let (this, idx) = (vm.a(0), vm.a(1));
    let slot = this.wrapping_add(idx << 2);
    let bug = rd32(vm, slot.wrapping_add(4));
    if bug != 0 {
        callf(h, vm, sp, BUG_DT, &[bug, 1], &[])?;
        wr32(vm, slot.wrapping_add(4), 0);
    }
    let n = rd32(vm, this + 0xcc);
    let last = n.wrapping_sub(1);
    if idx != last {
        // move the last bug into the freed slot and tell it its new index
        let lb = rd32(vm, this.wrapping_add(last << 2).wrapping_add(4));
        wr32(vm, slot.wrapping_add(4), lb);
        wr32(vm, lb, idx);
        let n = rd32(vm, this + 0xcc);
        let last = n.wrapping_sub(1);
        wr32(vm, this.wrapping_add(last << 2).wrapping_add(4), 0);
    }
    let n = rd32(vm, this + 0xcc);
    wr32(vm, this + 0xcc, n.wrapping_sub(1));
    vm.ret(n);
    Ok(())
}

/// MicroBugManager::RemoveAllBugs() @0x803314ac
pub fn mgr_remove_all(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let this = vm.a(0);
    let mut r3 = this;
    let mut i: u32 = 0;
    while (i as i32) < rd32(vm, this + 0xcc) as i32 {
        let slot = this + 4 * i;
        r3 = rd32(vm, slot + 4);
        if r3 != 0 {
            r3 = callf(h, vm, sp, BUG_DT, &[r3, 1], &[])?;
            wr32(vm, slot + 4, 0);
        }
        i += 1;
    }
    wr32(vm, this + 0xcc, 0);
    vm.ret(r3);
    Ok(())
}

/// MicroBugManager::GetBug(int) const @0x80331524
pub fn mgr_get_bug(h: &mut MgHost, vm: &mut V) -> R {
    let (this, idx) = (vm.a(0), vm.a(1));
    let b = rd32(vm, this.wrapping_add(idx << 2).wrapping_add(4));
    vm.ret(b);
    Ok(())
}

/// MicroBugManager::Update(int) @0x80331534
pub fn mgr_update(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let (this, dt) = (vm.a(0), vm.a(1));
    if rd8(vm, this) == 0 {
        return Ok(());
    }
    let mut r3 = this;
    let mut i: u32 = 0;
    while (i as i32) < rd32(vm, this + 0xcc) as i32 {
        r3 = rd32(vm, this + 4 + 4 * i);
        if r3 != 0 {
            r3 = callf(h, vm, sp, BUG_UPDATE, &[r3, dt], &[])?;
        }
        i += 1;
    }
    vm.ret(r3);
    Ok(())
}

/// MicroBugManager::Draw(Ren::SceneContext&) @0x803315ac
pub fn mgr_draw(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x20);
    let this = vm.a(0);
    let mut r3 = this;
    let mut i: u32 = 0;
    while (i as i32) < rd32(vm, this + 0xcc) as i32 {
        r3 = rd32(vm, this + 4 + 4 * i);
        if r3 != 0 {
            r3 = callf(h, vm, sp, BUG_DRAW, &[r3], &[])?;
        }
        i += 1;
    }
    vm.ret(r3);
    Ok(())
}

/// __sinit_\microbugmanager_cpp() @0x80331610
pub fn sinit_microbugmanager(h: &mut MgHost, vm: &mut V) -> R {
    let sp = vm.st.cpu.r[1]; // frameless: tail call into GUID::GUID(unsigned short, unsigned short)
    let this = sda(vm, -0x2260);
    callf(h, vm, sp, GUID_CT, &[this, 0xffff, 0], &[])?;
    Ok(())
}

pub const PORTS: &[crate::mgvm::ports::Port] = &[
    ("__ct__8MicroBugFQ28MicroBug7BugTypeiRC9rmVector3", microbug_ct),
    ("__dt__8MicroBugFv", microbug_dt),
    ("Initialize__8MicroBugFPQ23Ren11CachedModel", microbug_init),
    ("UnInitialize__8MicroBugFv", microbug_uninit),
    ("Draw__8MicroBugFv", microbug_draw),
    ("Update__8MicroBugFi", microbug_update),
    ("Normalize__Q22EA4MathFRCQ32EA4Math7Vector3RQ32EA4Math7Vector3", math_normalize),
    ("Mult__Q22EA4MathFRCQ32EA4Math7Vector3fRQ32EA4Math7Vector3", math_mult),
    ("SetBugData__8MicroBugFif", microbug_set_bug_data),
    ("__sinit_\\microbug_cpp", sinit_microbug),
    ("__ct__15MicroBugManagerFv", mgr_ct),
    ("__dt__15MicroBugManagerFv", mgr_dt),
    ("Initialize__15MicroBugManagerFv", mgr_init),
    ("UnInitialize__15MicroBugManagerFv", mgr_uninit),
    ("LoadAssets__15MicroBugManagerFv", mgr_load_assets),
    ("UnloadAssets__15MicroBugManagerFv", mgr_unload_assets),
    ("AddBug__15MicroBugManagerFRC9rmVector3", mgr_add_bug),
    ("AddBug__15MicroBugManagerFRC9rmVector3Q28MicroBug7BugType", mgr_add_bug_typed),
    ("RemoveBug__15MicroBugManagerFi", mgr_remove_bug),
    ("RemoveAllBugs__15MicroBugManagerFv", mgr_remove_all),
    ("GetBug__15MicroBugManagerCFi", mgr_get_bug),
    ("Update__15MicroBugManagerFi", mgr_update),
    ("Draw__15MicroBugManagerFRQ23Ren12SceneContext", mgr_draw),
    ("__sinit_\\microbugmanager_cpp", sinit_microbugmanager),
];
