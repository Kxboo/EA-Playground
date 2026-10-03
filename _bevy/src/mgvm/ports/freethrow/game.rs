//! MGFreeThrow (mgfreethrow.cpp and its small neighbours in the same address range), ported from the original PowerPC.
//!
//! Conventions (same as `microbug::hunt`):
//!  * the original's stack frame is reproduced (same `sp`, same local offsets) so pointers to locals handed to other
//!    functions are the addresses the original passes;
//!  * every call the original makes is made here in the same order with the same arguments from the same `sp`;
//!  * float arithmetic mirrors the interpreter (single ops round to f32, fmadds/fnmsubs are fused).
#![allow(unused)]
use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
pub type R = Result<(), String>;

/// The WorldMan singleton: `lis r3,-0x7fa1 ; addi r3,r3,-0x7ce0`.
const WORLD_MAN: u32 = 0x805e_8320;
/// The state dispatch table of MGFreeThrow::Update: `lis r3,-0x7fb2 ; addi r3,r3,-0x71a0`.
const UPDATE_TABLE: u32 = 0x804d_8e60;

// ---------------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------------

fn rd(vm: &mut V, a: u32) -> u32 {
    vm.st.mem.r32(a)
}
fn wr(vm: &mut V, a: u32, v: u32) {
    vm.st.mem.w32(a, v)
}
fn rb8(vm: &mut V, a: u32) -> u8 {
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
/// fnmsubs: -(a*c - b), fused, then rounded to single.
fn fnmsubs(a: f64, c: f64, b: f64) -> f64 {
    fs(-(a.mul_add(c, -b)))
}
/// `fcmpo a,b ; cror eq,lt,eq ; beq`
fn le(a: f64, b: f64) -> bool {
    a < b || a == b
}
/// `fcmpo a,b ; cror eq,gt,eq ; beq`
fn ge(a: f64, b: f64) -> bool {
    a > b || a == b
}
/// fctiwz as the integer the following `stfd ; lwz 4(..)` reads (saturating, NaN gives i32::MIN).
fn fctiwz(x: f64) -> u32 {
    let t = x.trunc();
    (if t.is_nan() { i32::MIN } else { t.max(i32::MIN as f64).min(i32::MAX as f64) as i32 }) as u32
}

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
/// `rmVector3::rmVector3(float, float, float)` into `at`.
fn v3(h: &mut MgHost, vm: &mut V, sp: u32, at: u32, x: f64, y: f64, z: f64) -> Result<u32, String> {
    gc(h, vm, sp, "__ct__9rmVector3Ffff", &[at], &[x, y, z])
}
/// `rmVector3::rmVector3(c(x), c(y), c(z))` from three small-data-2 constants.
fn v3k(h: &mut MgHost, vm: &mut V, sp: u32, at: u32, kx: i32, ky: i32, kz: i32) -> Result<u32, String> {
    let (x, y, z) = (lfs2(vm, kx), lfs2(vm, ky), lfs2(vm, kz));
    v3(h, vm, sp, at, x, y, z)
}

// ---------------------------------------------------------------------------------------------------------------------
// static initializers
// ---------------------------------------------------------------------------------------------------------------------

/// Tail call `GUID::GUID(&GUID_BAD, 0xffff, 0)` of the static initializers that only construct a GUID.
fn sinit_guid(h: &mut MgHost, vm: &mut V, off: i32) -> R {
    let sp = fsp(vm, 0);
    let g = ad13(vm, off);
    cl(h, vm, sp, "__ct__4GUIDFUsUs", &[g, 0xffff, 0])?;
    Ok(())
}

/// __sinit_\freethrowball_cpp() @0x80334460
pub fn sinit_ball(h: &mut MgHost, vm: &mut V) -> R {
    sinit_guid(h, vm, -0x2238)
}
/// __sinit_\freethrowballcourt_cpp() @0x80334804
pub fn sinit_ball_court(h: &mut MgHost, vm: &mut V) -> R {
    sinit_guid(h, vm, -0x2230)
}
/// __sinit_\freethrowcharactercollisionlistener_cpp() @0x803348e0
pub fn sinit_character_listener(h: &mut MgHost, vm: &mut V) -> R {
    sinit_guid(h, vm, -0x2228)
}
/// __sinit_\freethrowcollisionlistener_cpp() @0x80334b28
pub fn sinit_listener(h: &mut MgHost, vm: &mut V) -> R {
    sinit_guid(h, vm, -0x2220)
}

/// GUID_BAD constructor, then Conga::Math::Pi = k0*atan(arg) and its multiples (k1, k2, k3) into the r13 globals at
/// `dst` (Pi, HalfPi, QuarterPi, TwoPi).  Returns the `sp` used for the calls.
fn sinit_trig(h: &mut MgHost, vm: &mut V, sp: u32, guid: i32, arg: i32, k: [i32; 4], dst: i32) -> R {
    let g = ad13(vm, guid);
    cl(h, vm, sp, "__ct__4GUIDFUsUs", &[g, 0xffff, 0])?;
    let a = lfs2(vm, arg);
    gc(h, vm, sp, "atan__3stdFf", &[], &[a])?;
    let atan = vm.st.cpu.f[1];
    let k0 = lfs2(vm, k[0]);
    let pi = fmuls(k0, atan);
    let (k1, k2, k3) = (lfs2(vm, k[1]), lfs2(vm, k[2]), lfs2(vm, k[3]));
    let half = fmuls(k1, pi);
    let quarter = fmuls(k2, pi);
    let two = fmuls(k3, pi);
    let d = ad13(vm, dst);
    stfs(vm, d, pi);
    stfs(vm, d + 4, half);
    stfs(vm, d + 8, quarter);
    stfs(vm, d + 0xc, two);
    Ok(())
}

/// __sinit_\dribblingmanager_cpp() @0x803330e0
pub fn sinit_dribbling(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    sinit_trig(h, vm, sp, -0x2250, -0x4824, [-0x47cc, -0x47e0, -0x47ec, -0x47c8], -0x224c)
}

/// __sinit_\highfivemanager_cpp() @0x80337d14
pub fn sinit_highfive(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    sinit_trig(h, vm, sp, -0x2200, -0x4648, [-0x4628, -0x4630, -0x4624, -0x4620], -0x21fc)
}

/// __sinit_\mgfreethrow_cpp() @0x803371d4
pub fn sinit_mgfreethrow(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let b = 0x805e_2a60u32;
    sinit_trig(h, vm, sp, -0x2218, -0x4728, [-0x4698, -0x4714, -0x46a0, -0x4724], -0x2214)?;
    v3k(h, vm, sp, b, -0x4694, -0x46bc, -0x4690)?;
    v3k(h, vm, sp, b + 0x10, -0x468c, -0x4730, -0x4688)?;
    v3k(h, vm, sp, b + 0x20, -0x4684, -0x4680, -0x467c)?;
    let bad = ad13(vm, -0x2218);
    cl(h, vm, sp, "__ct__4GUIDFRC4GUID", &[b + 0x30, bad])?;
    cl(h, vm, sp, "__ct__4GUIDFRC4GUID", &[b + 0x34, bad])?;
    cl(h, vm, sp, "__ct__4GUIDFRC4GUID", &[b + 0x38, bad])?;
    let z = lfs2(vm, -0x4730);
    let x = lfs2(vm, -0x4678);
    v3(h, vm, sp, b + 0x40, x, z, z)?;
    v3k(h, vm, sp, b + 0x50, -0x4674, -0x4670, -0x466c)?;
    v3k(h, vm, sp, b + 0x60, -0x4668, -0x4664, -0x4660)?;
    v3k(h, vm, sp, b + 0x70, -0x465c, -0x4658, -0x4654)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------------------------------------------------

/// CharacterState::SetDir(const rmVector3*) @0x80336498
pub fn set_dir(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let v = vm.a(1);
    let sp = dsp(vm, 0x30);
    cl(h, vm, sp, "Set__7rmAngleFPC9rmVector3", &[t + 0x80, v])?;
    cl(h, vm, sp, "AsDir__7rmAngleCFv", &[sp + 0x10, t + 0x80])?;
    let (f1, f2, f3) = (lfs(vm, sp + 0x10), lfs(vm, sp + 0x14), lfs(vm, sp + 0x18));
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[t + 0x70], &[f1, f2, f3])?;
    Ok(())
}

/// rmMatrix4RotationY(float, rmMatrix4&) @0x80334770
pub fn rm_matrix4_rotation_y(h: &mut MgHost, vm: &mut V) -> R {
    let m = vm.a(0);
    let angle = vm.st.cpu.f[1];
    let sp = fsp(vm, 0x40);
    gc(h, vm, sp, "fSinCos__Q22EA4MathFfRfRf", &[sp + 0x28, sp + 0x2c], &[angle])?;
    let s = lfs(vm, sp + 0x28);
    let c = lfs(vm, sp + 0x2c);
    let zero = lfs2(vm, -0x4758);
    let one = lfs2(vm, -0x474c);
    // the last eight floats go in the parameter area
    stfs(vm, sp + 8, s);
    stfs(vm, sp + 0xc, zero);
    stfs(vm, sp + 0x10, c);
    for k in 0..3 {
        stfs(vm, sp + 0x14 + 4 * k, zero);
    }
    stfs(vm, sp + 0x24, one);
    let (f1, f3) = (lfs(vm, sp + 0x2c), -lfs(vm, sp + 0x28));
    gc(h, vm, sp, "Set__Q22EA4MathFRQ32EA4Math8Matrix44ffffffffffffffff", &[m], &[f1, zero, f3, zero, zero, one, zero, zero])?;
    Ok(())
}

/// rmfMax(float, float) @0x80333b94
pub fn rmf_max(_h: &mut MgHost, vm: &mut V) -> R {
    let (a, b) = (vm.st.cpu.f[1], vm.st.cpu.f[2]);
    // fcmpo f1,f2 ; bgtlr: a > b keeps f1, otherwise f1 = f2
    if !(a > b) {
        vm.st.cpu.f[1] = b;
    }
    Ok(())
}

/// PhysicsUserData::~PhysicsUserData() @0x80333444
pub fn physics_user_data_dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flag = vm.a(1);
    let sp = fsp(vm, 0x10);
    if t != 0 && (flag as i32) > 0 {
        cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
    }
    vm.ret(t);
    Ok(())
}

/// MGFreeThrow::OnPauseContinue() @0x80335b04 (tail call: ClosePauseMenu)
pub fn on_pause_continue(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0);
    cl(h, vm, sp, "ClosePauseMenu__11MGFreeThrowFv", &[t])?;
    Ok(())
}

/// Minigame::CheckDare() @0x803371c4
pub fn check_dare(_h: &mut MgHost, vm: &mut V) -> R {
    vm.ret(2);
    Ok(())
}

/// Minigame::IsMiniGame() @0x803371cc
pub fn is_mini_game(_h: &mut MgHost, vm: &mut V) -> R {
    vm.ret(1);
    Ok(())
}

/// MGFreeThrow::UpdateWaitForApocalypse(int) @0x80336688
pub fn update_wait_for_apocalypse(h: &mut MgHost, vm: &mut V) -> R {
    let sp = fsp(vm, 0x10);
    let world = rd(vm, WORLD_MAN + 0x88);
    let done = cl(h, vm, sp, "IsMinigameFadeEffectComplete__15PlaygroundWorldFv", &[world])?;
    vm.ret(if done as i32 != 0 { 2 } else { 1 });
    Ok(())
}

/// MGFreeThrow::UpdateHUD(int) @0x8033679c
pub fn update_hud(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let time = rd(vm, t + 0x3c0);
    let shown = rd(vm, t + 0x3c4);
    // max(time, 0) the way the compiler wrote it: time & ((-time & ~time) >> 31)
    let mask = (((time.wrapping_neg() & !time) as i32) >> 31) as u32;
    let v = time & mask;
    if (v as i32) < (shown as i32) {
        cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[v])?;
        wr(vm, t + 0x3c4, v);
    }
    Ok(())
}

/// MGFreeThrow::UpdateAimOffset(int) @0x80335b08
pub fn update_aim_offset(_h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let dir = lfs(vm, t + 0x460);
    let k0 = lfs2(vm, -0x4730);
    // (float)(int)dt through the 0x43300000 trick
    let magic = lfd2(vm, -0x4738);
    let as_double = f64::from_bits(0x4330_0000_0000_0000 | (dt ^ 0x8000_0000) as u64);
    let f3 = fsubs(as_double, magic);
    let rate = lfs(vm, t + 0x45c);
    let cur = lfs(vm, t + 0x458);
    if dir > k0 {
        let k = lfs2(vm, -0x4714);
        let v = fmadds(rate, f3, cur);
        stfs(vm, t + 0x458, v);
        if v > k {
            let c = lfs2(vm, -0x4710);
            stfs(vm, t + 0x460, c);
        }
    } else {
        let k = lfs2(vm, -0x470c);
        let v = fnmsubs(rate, f3, cur);
        stfs(vm, t + 0x458, v);
        if v < k {
            let c = lfs2(vm, -0x4728);
            stfs(vm, t + 0x460, c);
        }
    }
    Ok(())
}

/// MGFreeThrow::IsWithinGrabDistance(int) @0x80336dfc
pub fn is_within_grab_distance(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let idx = vm.a(1);
    let sp = fsp(vm, 0x10);
    let player = rd(vm, t + 0x190);
    let ball = rd(vm, t.wrapping_add(idx << 2) + 0x198);
    cl(h, vm, sp, "rmDistanceSquaredXZ__FRC9rmVector3RC9rmVector3", &[player + 0x180, ball + 0x50])?;
    let d = vm.st.cpu.f[1];
    let k = lfs2(vm, -0x46bc);
    vm.ret((d < k) as u32);
    Ok(())
}

/// MGFreeThrow::HasBallEnteredHoop(int) @0x80336978
pub fn has_ball_entered_hoop(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let idx = vm.a(1);
    let sp = dsp(vm, 0x70);
    let slot = t.wrapping_add(idx << 2);
    let ball = rd(vm, slot + 0x198);
    let mut hit = 0u32;
    if rb8(vm, ball + 0x24) == 0 {
        vm.ret(0);
        return Ok(());
    }
    let k = lfs2(vm, -0x46d0);
    let y = lfs(vm, ball + 0x54);
    if y < k {
        vm.ret(0);
        return Ok(());
    }
    let hoop = rd(vm, t + 0x220);
    let k = lfs2(vm, -0x4708);
    let top = fsubs(lfs(vm, hoop + 4), k);
    if le(y, top) {
        let prev = t.wrapping_add(idx << 4);
        let prev_y = lfs(vm, prev + 0x234);
        if ge(prev_y, top) {
            cl(h, vm, sp, "__mi__Q22EA4MathFRCQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[sp + 0x10, ball + 0x50, prev + 0x230])?;
            let (f1, f2, f3) = (lfs(vm, sp + 0x10), lfs(vm, sp + 0x14), lfs(vm, sp + 0x18));
            gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[sp + 0x30], &[f1, f2, f3])?;
            let f1 = lfs(vm, prev + 0x234);
            let dy = lfs(vm, sp + 0x34);
            let f5 = fsubs(top, f1);
            let dx = lfs(vm, sp + 0x30);
            let px = lfs(vm, prev + 0x230);
            let dz = lfs(vm, sp + 0x38);
            let f5 = fdivs(f5, dy);
            let pz = lfs(vm, prev + 0x238);
            let x = fmadds(f5, dx, px);
            let z = fmadds(f5, dz, pz);
            v3(h, vm, sp, sp + 0x20, x, top, z)?;
            let hoop = rd(vm, t + 0x220);
            cl(h, vm, sp, "rmDistanceSquaredXZ__FRC9rmVector3RC9rmVector3", &[hoop, sp + 0x20])?;
            let d = vm.st.cpu.f[1];
            let k = lfs2(vm, -0x46cc);
            hit = (d < k) as u32;
        }
    }
    let cur = rd(vm, slot + 0x198);
    cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t.wrapping_add(idx << 4) + 0x230, cur + 0x50])?;
    vm.ret(hit);
    Ok(())
}

/// MGFreeThrow::GrabBall(int, const rmVector3*) @0x80336d14
pub fn grab_ball(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let idx = vm.a(1);
    let at = vm.a(2);
    let sp = dsp(vm, 0x30);
    let player = rd(vm, t + 0x190);
    let slot = t.wrapping_add(idx << 2);
    if at != 0 {
        let ball = rd(vm, slot + 0x198);
        cl(h, vm, sp, "Grab__13FreeThrowBallFPC9rmVector3", &[ball, at])?;
    } else {
        let f5 = lfs2(vm, -0x46d8);
        let (v1, v0) = (lfs(vm, player + 0x1a0), lfs(vm, player + 0x180));
        let f4 = lfs2(vm, -0x46d4);
        let v2 = lfs(vm, player + 0x184);
        let f1 = fmadds(f5, v1, v0);
        let (v3_, v4) = (lfs(vm, player + 0x1a8), lfs(vm, player + 0x188));
        let f2 = fadds(f4, v2);
        let f3 = fmadds(f5, v3_, v4);
        v3(h, vm, sp, sp + 0x10, f1, f2, f3)?;
        let ball = rd(vm, slot + 0x198);
        cl(h, vm, sp, "Grab__13FreeThrowBallFPC9rmVector3", &[ball, sp + 0x10])?;
    }
    wb(vm, player + 0x1c0, 1);
    let mv = rd(vm, player + 0x10);
    for (a, b) in [(0u32, 0x29u32), (1, 0x2b), (5, 0x2b)] {
        cl(h, vm, sp, "SetMappedMovmentState__17CharacterMovementFQ219AnimationStateGraph10AnimStatesQ219AnimationStateGraph10AnimStates", &[mv, a, b])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Update / Draw
// ---------------------------------------------------------------------------------------------------------------------

/// MGFreeThrow::Update(int) @0x803356fc
pub fn update(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = fsp(vm, 0x20);
    let mut result = 1u32;
    if rb8(vm, t + 0x24) == 0 {
        let prev = rd(vm, t + 0x3c8);
        wr(vm, t + 0x3cc, prev);
        wr(vm, t + 0x3c8, prev.wrapping_add(dt));
        let v = rd(vm, t + 0x214);
        wr(vm, t + 0x214, v.wrapping_add(dt));
        for i in 0..0x19u32 {
            let ball = rd(vm, t + 0x198 + 4 * i);
            cl(h, vm, sp, "Update__13FreeThrowBallFi", &[ball, dt])?;
        }
        let state = rd(vm, t + 0x34);
        if state <= 9 {
            let target = rd(vm, UPDATE_TABLE + state * 4);
            match target {
                0x8033_5790 => {
                    cl(h, vm, sp, "ChangeGameState__11MGFreeThrowF13MinigameState", &[t, 3])?;
                }
                0x8033_57a0 => result = cl(h, vm, sp, "UpdateIntro__11MGFreeThrowFi", &[t, dt])?,
                0x8033_57b4 => result = cl(h, vm, sp, "UpdateGame__11MGFreeThrowFi", &[t, dt])?,
                0x8033_57c8 => result = cl(h, vm, sp, "UpdateOutro__11MGFreeThrowFi", &[t, dt])?,
                0x8033_57dc => result = cl(h, vm, sp, "UpdateWaitForApocalypse__11MGFreeThrowFi", &[t, dt])?,
                0x8033_57ec => {}
                other => return Err(format!("MGFreeThrow::Update: unknown state target {other:#x}")),
            }
        }
        cl(h, vm, sp, "Update__8MinigameFi", &[t, dt])?;
    }
    wr(vm, t + 0x438, 0);
    vm.ret(result);
    Ok(())
}

/// MGFreeThrow::Draw(Ren::SceneContext&) @0x8033581c
pub fn draw(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x70);
    for i in 0..0x19u32 {
        let ball = rd(vm, t + 0x198 + 4 * i);
        cl(h, vm, sp, "Draw__13FreeThrowBallFv", &[ball])?;
    }
    if rb8(vm, t + 0x464) != 0 && rd(vm, t + 0x34) == 6 {
        let player = rd(vm, t + 0x190);
        let hoop = rd(vm, t + 0x220);
        let (pz, hz, px, hx) = (lfs(vm, player + 0x188), lfs(vm, hoop + 8), lfs(vm, player + 0x180), lfs(vm, hoop));
        let f1 = fsubs(hz, pz);
        let f2 = lfs2(vm, -0x4718);
        let f3 = fsubs(px, hx);
        v3(h, vm, sp, sp + 0x50, f1, f2, f3)?;
        cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[sp + 0x50, sp + 0x50])?;
        let aim = lfs(vm, t + 0x458);
        let (nx, ny, nz) = (lfs(vm, sp + 0x50), lfs(vm, sp + 0x54), lfs(vm, sp + 0x58));
        let (nx, ny, nz) = (fmuls(nx, aim), fmuls(ny, aim), fmuls(nz, aim));
        let (hx, hy, hz) = (lfs(vm, hoop), lfs(vm, hoop + 4), lfs(vm, hoop + 8));
        let (x, y, z) = (fadds(hx, nx), fadds(hy, ny), fadds(hz, nz));
        gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[sp + 0x40], &[x, y, z])?;
        let k = lfs2(vm, -0x4718);
        let hy = lfs(vm, hoop + 4);
        stfs(vm, sp + 0x44, fadds(k, hy));
        cl(h, vm, sp, "DrawCursor__11MGFreeThrowCFPC9rmVector3PC9rmVector3", &[t, sp + 0x40, sp + 0x50])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// shared sequences
// ---------------------------------------------------------------------------------------------------------------------

const SET_NEXT_ANIM: &str = "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesbi";
const GRAB_BALL: &str = "GrabBall__11MGFreeThrowFiPC9rmVector3";
const RM_MULT: &str = "rmMult__FRC9rmVector3RC9rmMatrix4R9rmVector3";
/// The off-screen ball position (kFreeThrowOffScreenBallPos): `lis r3,-0x7fa2 ; addi r3,r3,0x2ad0`.
const OFF_SCREEN_POS: u32 = 0x805e_2ad0;
/// The table of free throw positions (kFreeThrowPosition, 16 bytes each): `lis r6,-0x7fa2 ; addi r6,r6,0x2a60`.
const POSITIONS: u32 = 0x805e_2a60;
/// kFreeThrowPointsToWin: `lis r4,-0x7fb2 ; addi r4,r4,-0x71e8`.
const POINTS_TO_WIN: u32 = 0x804d_8e18;
const VTABLE: u32 = 0x804d_8ea8;

/// `Character::mAnimationState` of the player: `lwz r3,0x190(this) ; lwz r3,0x18(r3)`.
fn player_anim(vm: &mut V, t: u32) -> u32 {
    let p = rd(vm, t + 0x190);
    rd(vm, p + 0x18)
}

/// The ball `n` of the court: `lwz r3,0x198(this + n*4)`.
fn ball(vm: &mut V, t: u32, n: u32) -> u32 {
    rd(vm, t.wrapping_add(n << 2) + 0x198)
}

/// Grab the ball whose index is at `this + idx_off`, at the player's hand marker when there is one (`this+0x43c`).
fn grab_current(h: &mut MgHost, vm: &mut V, sp: u32, t: u32, idx_off: u32, buf: u32) -> R {
    let marker = rd(vm, t + 0x43c);
    if marker as i32 != -1 {
        let anim = player_anim(vm, t);
        cl(h, vm, sp, "GetMarkerPosition__14AnimationStateCFi", &[buf, anim, marker])?;
        let idx = rd(vm, t + idx_off);
        cl(h, vm, sp, GRAB_BALL, &[t, idx, buf])?;
    } else {
        let idx = rd(vm, t + idx_off);
        cl(h, vm, sp, GRAB_BALL, &[t, idx, 0])?;
    }
    Ok(())
}

/// Count the baskets of all balls: `HasBallEnteredHoop` and `ProcessBasket` for each of the 25 balls.
fn process_baskets(h: &mut MgHost, vm: &mut V, sp: u32, t: u32) -> R {
    for i in 0..0x19u32 {
        if cl(h, vm, sp, "HasBallEnteredHoop__11MGFreeThrowFi", &[t, i])? != 0 {
            cl(h, vm, sp, "ProcessBasket__11MGFreeThrowFi", &[t, i])?;
        }
    }
    Ok(())
}

/// A copy of a table entry (3 words of kFreeThrowPosition[this+0x3c]) to `dst`.
fn copy_position(vm: &mut V, t: u32, dst: u32) {
    let k = rd(vm, t + 0x3c) << 4;
    let p = POSITIONS.wrapping_add(k);
    let (a, b, c) = (rd(vm, p), rd(vm, p + 4), rd(vm, p + 8));
    wr(vm, dst, a);
    wr(vm, dst + 4, b);
    wr(vm, dst + 8, c);
}

// ---------------------------------------------------------------------------------------------------------------------
// construction / destruction
// ---------------------------------------------------------------------------------------------------------------------

/// MGFreeThrow::MGFreeThrow(Ren::Scene*) @0x80334b40
pub fn ctor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let scene = vm.a(1);
    let sp = fsp(vm, 0x20);
    cl(h, vm, sp, "__ct__8MinigameFPQ23Ren5Scene", &[t, scene])?;
    let f0 = lfs2(vm, -0x4730);
    for o in [0x100, 0x104, 0x108] {
        stfs(vm, t + o, f0);
    }
    wr(vm, t, VTABLE);
    for o in [0x190u32, 0x1fc, 0x200] {
        wr(vm, t + o, 0);
    }
    wb(vm, t + 0x194, 0);
    for o in [0x204, 0x208, 0x20c] {
        stfs(vm, t + o, f0);
    }
    wb(vm, t + 0x210, 0);
    for o in [0x214u32, 0x21c, 0x220, 0x3c0, 0x3c4, 0x3c8, 0x3cc, 0x3d0, 0x3d4, 0x438] {
        wr(vm, t + o, 0);
    }
    wr(vm, t + 0x43c, u32::MAX);
    for o in [0x444u32, 0x44c, 0x450, 0x454] {
        wr(vm, t + o, 0);
    }
    for o in [0x458, 0x45c, 0x460] {
        stfs(vm, t + o, f0);
    }
    wb(vm, t + 0x464, 0);
    wr(vm, t + 0x468, 0);
    wb(vm, t + 0x46c, 0);
    wr(vm, t + 0x470, 0);
    wr(vm, t + 0x474, 0);
    for i in 0..0x19u32 {
        let f1 = lfs2(vm, -0x4730);
        wr(vm, t + 4 * i + 0x198, 0);
        gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[t + 16 * i + 0x230], &[f1, f1, f1])?;
    }
    vm.ret(t);
    Ok(())
}

/// MGFreeThrow::~MGFreeThrow() @0x80334c58
pub fn dtor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flag = vm.a(1);
    let sp = fsp(vm, 0x20);
    if t != 0 {
        wr(vm, t, VTABLE);
        let engine = lwz13(vm, -0x1d84);
        cl(h, vm, sp, "WaitForDrawsToComplete__Q23Ren6EngineFv", &[engine])?;
        let pc = cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])?;
        cl(h, vm, sp, "SwitchToLocalControl__9CharacterFv", &[pc])?;
        let pc = cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])?;
        wb(vm, pc + 0x1c0, 0);
        let pc = cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])?;
        let mv = rd(vm, pc + 0x10);
        cl(h, vm, sp, "ResetMappedMovmentStates__17CharacterMovementFv", &[mv])?;
        cl(h, vm, sp, "__dt__8MinigameFv", &[t, 0])?;
        if (flag as i32) > 0 {
            cl(h, vm, sp, "Free__6MemMgrFPv", &[t])?;
        }
    }
    vm.ret(t);
    Ok(())
}

/// MGFreeThrow::InitializePlayer(const rmVector3*, Character*) @0x80336c40
pub fn initialize_player(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let pos = vm.a(1);
    let chr = vm.a(2);
    let sp = dsp(vm, 0x50);
    let mv = rd(vm, chr + 0x10);
    cl(h, vm, sp, "StopMovement__17CharacterMovementFv", &[mv])?;
    let (f1, f2, f3) = (lfs(vm, pos), lfs(vm, pos + 4), lfs(vm, pos + 8));
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[chr + 0x180], &[f1, f2, f3])?;
    wb(vm, chr + 0x190, 1);
    wr(vm, t + 0x190, chr);
    wb(vm, t + 0x194, 0);
    copy_position(vm, t, sp + 0x30);
    cl(h, vm, sp, "__mi__Q22EA4MathFRCQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[sp + 0x10, sp + 0x30, pos])?;
    let (f1, f2, f3) = (lfs(vm, sp + 0x10), lfs(vm, sp + 0x14), lfs(vm, sp + 0x18));
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[sp + 0x20], &[f1, f2, f3])?;
    cl(h, vm, sp, "SetDir__14CharacterStateFPC9rmVector3", &[chr + 0x130, sp + 0x20])?;
    Ok(())
}

/// MGFreeThrow::UnInitialize() @0x80335558
pub fn uninitialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0xf0);
    for i in 0..4u32 {
        let c = cl(h, vm, sp, "Get__10ControllerFi", &[i])?;
        cl(h, vm, sp, "PopState__10ControllerFv", &[c])?;
    }
    cl(h, vm, sp, "UnInitialize__8MinigameFv", &[t])?;
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "Purge__Q32EA5Conga15CallbackManagerFv", &[cb])?;
    cl(h, vm, sp, "Unload__13FreeThrowBallFv", &[])?;
    for i in 0..0x19u32 {
        let slot = t + 4 * i + 0x198;
        let b = rd(vm, slot);
        if b != 0 {
            cl(h, vm, sp, "__dt__13FreeThrowBallFv", &[b, 1])?;
            wr(vm, slot, 0);
        }
        wr(vm, slot, 0);
    }
    let court = rd(vm, t + 0x220);
    cl(h, vm, sp, "UnInitialize__18FreeThrowBallCourtFv", &[court])?;
    let court = rd(vm, t + 0x220);
    if court != 0 {
        cl(h, vm, sp, "__dt__18FreeThrowBallCourtFv", &[court, 1])?;
        wr(vm, t + 0x220, 0);
    }
    wr(vm, t + 0x220, 0);
    let cm = lwz13(vm, -0x1d94);
    let cam = rd(vm, cm);
    let info = cl(h, vm, sp, "GetCameraViewInfo__6CameraFb", &[cam, 0])?;
    cl(h, vm, sp, "__ct__14CameraViewInfoFRC14CameraViewInfo", &[sp + 0x70, info])?;
    let cm = lwz13(vm, -0x1d94);
    cl(h, vm, sp, "ReInitialize__13CameraManagerF10CameraTypeUi", &[cm, 0, 0])?;
    // the view info is passed by value: copied to the parameter area
    for k in 0..24u32 {
        let w = rd(vm, sp + 0x70 + 4 * k);
        wr(vm, sp + 0x10 + 4 * k, w);
    }
    let cm = lwz13(vm, -0x1d94);
    let cam = rd(vm, cm);
    cl(h, vm, sp, "StartCameraSysTransition__6CameraF14CameraViewInfoib", &[cam, sp + 0x10, 0x12c, 0])?;
    let listener = rd(vm, t + 0x468);
    if listener != 0 {
        let vt = rd(vm, listener);
        let f = rd(vm, vt + 8);
        gca(h, vm, sp, f, &[listener, 1], &[])?;
        wr(vm, t + 0x468, 0);
    }
    cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[0])?;
    cl(h, vm, sp, "ShotMeter_SetVisible__16WorldHudHandlersFiii", &[0, 0, 0])?;
    cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[2])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "StopMusic__5AudioFv", &[a])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "UnloadData__5AudioF9AUDIODATA", &[a, 0x10])?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// game states
// ---------------------------------------------------------------------------------------------------------------------

/// MGFreeThrow::ChangeGameState(MinigameState) @0x803367f8
pub fn change_game_state(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let state = vm.a(1);
    let sp = dsp(vm, 0x40);
    wr(vm, t + 0x34, state);
    wr(vm, t + 0x3c8, 0);
    wr(vm, t + 0x3cc, 0);
    match state as i32 {
        7 => wb(vm, t + 0x210, 1),
        3 => {
            let idx = rd(vm, t + 0x1fc);
            let b = ball(vm, t, idx);
            let zero = lfs2(vm, -0x4730);
            wb(vm, b + 0x25, 1);
            stfs(vm, b + 0x28, zero);
            stfs(vm, b + 0x2c, zero);
            let player = rd(vm, t + 0x190);
            let f5 = lfs2(vm, -0x46d8);
            let f4 = lfs2(vm, -0x46e8);
            let (a1, a0) = (lfs(vm, player + 0x1a0), lfs(vm, player + 0x180));
            let a2 = lfs(vm, player + 0x184);
            let f1 = fmadds(f5, a1, a0);
            let (a3, a4) = (lfs(vm, player + 0x1a8), lfs(vm, player + 0x188));
            let f2 = fadds(f4, a2);
            let f3 = fmadds(f5, a3, a4);
            v3(h, vm, sp, sp + 0x20, f1, f2, f3)?;
            cl(h, vm, sp, RM_MULT, &[OFF_SCREEN_POS, t + 0x110, sp + 0x10])?;
            let k = lfs2(vm, -0x46e8);
            let py = lfs(vm, player + 0x184);
            stfs(vm, sp + 0x14, fadds(k, py));
            let idx = rd(vm, t + 0x1fc);
            let b = ball(vm, t, idx);
            cl(h, vm, sp, "Grab__13FreeThrowBallFPC9rmVector3", &[b, sp + 0x10])?;
            let idx = rd(vm, t + 0x1fc);
            let b = ball(vm, t, idx);
            cl(h, vm, sp, "Pass__13FreeThrowBallFPC9rmVector3Ui", &[b, sp + 0x20, 0x1f4])?;
        }
        6 => {
            wb(vm, t + 0x210, 1);
            let c = cl(h, vm, sp, "Get__10ControllerFi", &[0])?;
            cl(h, vm, sp, "PopState__10ControllerFv", &[c])?;
            let c = cl(h, vm, sp, "Get__10ControllerFi", &[0])?;
            cl(h, vm, sp, "SetCurrentControllerState__10ControllerF16EControllerState", &[c, 0x18])?;
        }
        9 => {
            let world = rd(vm, WORLD_MAN + 0x88);
            cl(h, vm, sp, "StartMinigameFadeInEffect__15PlaygroundWorldFi", &[world, u32::MAX])?;
        }
        _ => {}
    }
    Ok(())
}

/// MGFreeThrow::UpdateIntro(int) @0x80335cf8
pub fn update_intro(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x50);
    let anim = player_anim(vm, t);
    cl(h, vm, sp, SET_NEXT_ANIM, &[anim, 0x29, 0, u32::MAX])?;
    let (now, limit) = (rd(vm, t + 0x3c8), rd(vm, t + 0x3d0));
    let go = if now > limit {
        true
    } else {
        let idx = rd(vm, t + 0x1fc);
        cl(h, vm, sp, "IsWithinGrabDistance__11MGFreeThrowCFi", &[t, idx])? != 0
    };
    if go {
        let idx = rd(vm, t + 0x1fc);
        let b = ball(vm, t, idx);
        let hoop = rd(vm, t + 0x220);
        cl(h, vm, sp, "rmDistanceSquaredXZ__FRC9rmVector3RC9rmVector3", &[b + 0x50, hoop])?;
        let d2 = vm.st.cpu.f[1];
        gc(h, vm, sp, "sqrt", &[], &[d2])?;
        let root = fs(vm.st.cpu.f[1]);
        let mut f2 = lfs2(vm, -0x4728);
        let f0 = lfs2(vm, -0x46fc);
        let f1 = fsubs(root, f2);
        let f0 = fdivs(f1, f0);
        if !(f2 < f0) {
            f2 = f0;
        }
        let mut f3 = lfs2(vm, -0x4730);
        if !(f3 > f2) {
            f3 = f2;
        }
        let mut f4 = lfs2(vm, -0x4718);
        let (k0, k2, k1) = (lfs2(vm, -0x46f0), lfs2(vm, -0x46f4), lfs2(vm, -0x46f8));
        let f0 = fmadds(k0, f3, f4);
        let f1 = fmadds(k2, f3, k1);
        stfs(vm, t + 0x204, f1);
        if !(f4 < f0) {
            f4 = f0;
        }
        let mut f1 = lfs2(vm, -0x46ec);
        if !(f1 > f4) {
            f1 = f4;
        }
        stfs(vm, t + 0x20c, f1);
        wr(vm, t + 0x438, 0);
        let zero = lfs2(vm, -0x4730);
        stfs(vm, t + 0x208, zero);
        grab_current(h, vm, sp, t, 0x1fc, sp + 0x10)?;
        let k2 = lfs2(vm, -0x4700);
        wr(vm, t + 0x3c0, 30000);
        let a = lfs(vm, t + 0x204);
        let k3 = lfs2(vm, -0x4704);
        wr(vm, t + 0x3c4, 30000);
        let p = fctiwz(fmadds(k2, a, k3));
        cl(h, vm, sp, "ShotMeter_SetVisible__16WorldHudHandlersFiii", &[1, 100, p])?;
        let (f3, f1) = (lfs(vm, t + 0x204), lfs(vm, t + 0x20c));
        let k2 = lfs2(vm, -0x4700);
        let f4 = fadds(f3, f1);
        let k3 = lfs2(vm, -0x4704);
        let f3 = fsubs(f3, f1);
        let hi = fctiwz(fmadds(k2, f4, k3));
        let lo = fctiwz(fmadds(k2, f3, k3));
        cl(h, vm, sp, "ShotMeter_SetLimit__16WorldHudHandlersFii", &[hi, lo])?;
        cl(h, vm, sp, "ChangeGameState__11MGFreeThrowF13MinigameState", &[t, 6])?;
        wr(vm, t + 0x448, 0);
        wr(vm, t + 0x444, 0);
    }
    vm.ret(1);
    Ok(())
}

/// MGFreeThrow::UpdateOutro(int) @0x803364f8
pub fn update_outro(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x30);
    process_baskets(h, vm, sp, t)?;
    let idx = rd(vm, t + 0x1fc);
    let b = ball(vm, t, idx);
    if rb8(vm, b + 0x24) == 0 {
        grab_current(h, vm, sp, t, 0x1fc, sp + 0x10)?;
    }
    let (now, limit) = (rd(vm, t + 0x3c8), rd(vm, t + 0x3d4));
    if now > limit {
        let area = rd(vm, t + 0x3c);
        wr(vm, t + 0x68, 2);
        let score = rb8(vm, t + 0x194) as u32;
        let need = rd(vm, POINTS_TO_WIN.wrapping_add(area << 2));
        if (score as i32) < (need as i32) {
            wr(vm, t + 0x64, 2);
        } else {
            wr(vm, t + 0x64, 1);
            let inst = lwz13(vm, -0x1e7c);
            let prof = cl(h, vm, sp, "GetProfile__16CharacterProfileFv", &[inst])?;
            if rb8(vm, prof.wrapping_add(area) + 0xce0) == 0 {
                let inst = lwz13(vm, -0x1e7c);
                cl(h, vm, sp, "SetFreeThrowGameBeat__16CharacterProfileFib", &[inst, area, 1])?;
                let marbles = rd(vm, t + 0x470);
                cl(h, vm, sp, "AddMarblesEvent__8WorldManFii", &[WORLD_MAN, 3, marbles])?;
            }
            let area = rd(vm, t + 0x3c);
            let score = rb8(vm, t + 0x194) as u32;
            let inst = lwz13(vm, -0x1e7c);
            let prof = cl(h, vm, sp, "GetProfile__16CharacterProfileFv", &[inst])?;
            let best = rd(vm, prof.wrapping_add(area << 2) + 0xcd4);
            if (score as i32) > (best as i32) {
                let inst = lwz13(vm, -0x1e7c);
                cl(h, vm, sp, "SetFreeThrowHighScore__16CharacterProfileFii", &[inst, area, score])?;
            }
        }
        cl(h, vm, sp, "ChangeGameState__11MGFreeThrowF13MinigameState", &[t, 9])?;
    }
    vm.ret(1);
    Ok(())
}

/// MGFreeThrow::UpdateGame(int) @0x80335ee4
pub fn update_game(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let dt = vm.a(1);
    let sp = dsp(vm, 0xc0);
    let player = rd(vm, t + 0x190);
    let v = rd(vm, t + 0x448);
    wr(vm, t + 0x448, v.wrapping_add(dt));
    let time = rd(vm, t + 0x3c0);
    wr(vm, t + 0x3c0, time.wrapping_sub(dt));
    for k in 0..3u32 {
        let w = rd(vm, player + 0x180 + 4 * k);
        wr(vm, sp + 0xa0 + 4 * k, w);
    }
    copy_position(vm, t, sp + 0x90);
    cl(h, vm, sp, "__mi__Q22EA4MathFRCQ32EA4Math7Vector3RCQ32EA4Math7Vector3", &[sp + 0x10, sp + 0x90, sp + 0xa0])?;
    let (f1, f2, f3) = (lfs(vm, sp + 0x10), lfs(vm, sp + 0x14), lfs(vm, sp + 0x18));
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[sp + 0x80], &[f1, f2, f3])?;
    let p = rd(vm, t + 0x190);
    cl(h, vm, sp, "SetDir__14CharacterStateFPC9rmVector3", &[p + 0x130, sp + 0x80])?;
    cl(h, vm, sp, "UpdateHUD__11MGFreeThrowFi", &[t, dt])?;
    process_baskets(h, vm, sp, t)?;
    match rd(vm, t + 0x444) as i32 {
        0 => {
            if rb8(vm, t + 0x464) != 0 {
                cl(h, vm, sp, "UpdateAimOffset__11MGFreeThrowFi", &[t, dt])?;
            }
            let anim = player_anim(vm, t);
            cl(h, vm, sp, SET_NEXT_ANIM, &[anim, 0x29, 0, u32::MAX])?;
            grab_current(h, vm, sp, t, 0x1fc, sp + 0x70)?;
            let zero = lfs2(vm, -0x4730);
            let time = rd(vm, t + 0x3c0);
            wr(vm, t + 0x448, 0);
            stfs(vm, t + 0x208, zero);
            wr(vm, t + 0x444, 1);
            if (time as i32) < 0 {
                cl(h, vm, sp, "ChangeGameState__11MGFreeThrowF13MinigameState", &[t, 7])?;
            }
        }
        1 => {
            if rb8(vm, t + 0x464) != 0 {
                cl(h, vm, sp, "UpdateAimOffset__11MGFreeThrowFi", &[t, dt])?;
            }
            grab_current(h, vm, sp, t, 0x1fc, sp + 0x60)?;
            let f1 = lfs(vm, t + 0x208);
            let zero = lfs2(vm, -0x4730);
            if f1 > zero && (rd(vm, t + 0x214) as i32) > 0x3e8 {
                stfs(vm, t + 0x208, zero);
                wr(vm, t + 0x214, 0);
                let mut i = 0u32;
                while (i as i32) < (rd(vm, t + 0x438) as i32) {
                    let e = t + i * 0xc;
                    if rd(vm, e + 0x3e0) == 0 && rd(vm, e + 0x3d8) == 0 {
                        wr(vm, e + 0x3d8, 1);
                    }
                    i += 1;
                }
            }
            let f1 = lfs2(vm, -0x4730);
            let f0 = lfs(vm, t + 0x208);
            if f1 == f0 {
                let n = rd(vm, t + 0x438);
                if (n as i32) > 0 {
                    for i in 0..n {
                        let e = t + i * 0xc;
                        if rd(vm, e + 0x3e0) == 0 && rd(vm, e + 0x3d8) == 0 {
                            let f2 = lfs(vm, e + 0x3dc);
                            let c1 = lfs2(vm, -0x46e8);
                            let c2 = lfs2(vm, -0x46e4);
                            let a = fsubs(f2, c1);
                            let mut f2 = lfs2(vm, -0x46e0);
                            let b = fdivs(a, c2);
                            if !(f2 < b) {
                                f2 = b;
                            }
                            let mut f0 = lfs2(vm, -0x4730);
                            if !(f0 > f2) {
                                f0 = f2;
                            }
                            let f1 = lfs(vm, t + 0x204);
                            let cur = rd(vm, t + 0x1fc);
                            let f2 = fsubs(f0, f1);
                            let c4 = lfs2(vm, -0x46dc);
                            wr(vm, t + 0x214, 0);
                            let f0 = fdivs(f2, c4);
                            wr(vm, t + 0x200, cur);
                            wr(vm, t + 0x448, 0);
                            wr(vm, t + 0x444, 2);
                            let f0 = fadds(f1, f0);
                            let f1 = fsubs(f0, f1);
                            stfs(vm, t + 0x208, f0);
                            gc(h, vm, sp, "ShootBasketball__11MGFreeThrowFf", &[t], &[f1])?;
                            let n = rd(vm, t + 0x1fc).wrapping_add(1);
                            wr(vm, t + 0x1fc, n);
                            if n == 0x19 {
                                wr(vm, t + 0x1fc, 0);
                            }
                            let idx = rd(vm, t + 0x1fc);
                            let b = ball(vm, t, idx);
                            let zero = lfs2(vm, -0x4730);
                            let f1 = lfs2(vm, -0x46d4);
                            let f6 = lfs2(vm, -0x46d8);
                            wb(vm, b + 0x25, 1);
                            stfs(vm, b + 0x28, zero);
                            stfs(vm, b + 0x2c, zero);
                            let player = rd(vm, t + 0x190);
                            let y = lfs(vm, player + 0x184);
                            let f5 = lfs(vm, player + 0x1a0);
                            let f2 = fadds(f1, y);
                            let f4 = lfs(vm, player + 0x180);
                            let f3 = lfs(vm, player + 0x1a8);
                            let z = lfs(vm, player + 0x188);
                            let f1 = fmadds(f6, f5, f4);
                            let f3 = fmadds(f6, f3, z);
                            v3(h, vm, sp, sp + 0x50, f1, f2, f3)?;
                            cl(h, vm, sp, RM_MULT, &[OFF_SCREEN_POS, t + 0x110, sp + 0x40])?;
                            let idx = rd(vm, t + 0x1fc);
                            let b = ball(vm, t, idx);
                            cl(h, vm, sp, "Grab__13FreeThrowBallFPC9rmVector3", &[b, sp + 0x40])?;
                            let idx = rd(vm, t + 0x1fc);
                            let b = ball(vm, t, idx);
                            cl(h, vm, sp, "Pass__13FreeThrowBallFPC9rmVector3Ui", &[b, sp + 0x50, 0x1f4])?;
                            break;
                        }
                    }
                }
            }
            if (rd(vm, t + 0x3c0) as i32) < 0 {
                cl(h, vm, sp, "ChangeGameState__11MGFreeThrowF13MinigameState", &[t, 7])?;
            }
        }
        2 => {
            grab_current(h, vm, sp, t, 0x200, sp + 0x30)?;
            if (rd(vm, t + 0x448) as i32) > 0x190 {
                let n = rd(vm, t + 0x21c).wrapping_add(1);
                let idx = rd(vm, t + 0x200);
                let hoop = rd(vm, t + 0x220);
                wr(vm, t + 0x21c, n);
                let b = ball(vm, t, idx);
                cl(h, vm, sp, "Throw__13FreeThrowBallFPC9rmVector3", &[b, hoop])?;
                wr(vm, t + 0x448, 0);
                wr(vm, t + 0x444, 3);
            }
        }
        3 => {
            if rb8(vm, t + 0x464) != 0 {
                cl(h, vm, sp, "UpdateAimOffset__11MGFreeThrowFi", &[t, dt])?;
            }
            let idx = rd(vm, t + 0x1fc);
            if cl(h, vm, sp, "IsWithinGrabDistance__11MGFreeThrowCFi", &[t, idx])? != 0 {
                grab_current(h, vm, sp, t, 0x1fc, sp + 0x20)?;
                let zero = lfs2(vm, -0x4730);
                wr(vm, t + 0x448, 0);
                stfs(vm, t + 0x208, zero);
                wr(vm, t + 0x444, 1);
            }
            if (rd(vm, t + 0x448) as i32) > 0x5a {
                wr(vm, t + 0x448, 0);
                wr(vm, t + 0x444, 0);
            }
        }
        _ => {}
    }
    let obj = rd(vm, WORLD_MAN + 0x8c);
    if rb8(vm, obj + 0x24) == 0 {
        let v = rd(vm, t + 0x474);
        if (v as i32) > 0 {
            wr(vm, t + 0x474, v.wrapping_sub(dt));
        }
    }
    let c = cl(h, vm, sp, "Get__10ControllerFi", &[0])?;
    let ev = cl(h, vm, sp, "GetEventState__10ControllerCF12EActionEvent", &[c, 0xaf])?;
    if rb8(vm, ev) != 0 {
        cl(h, vm, sp, "OpenPauseMenu__11MGFreeThrowFv", &[t])?;
    }
    vm.ret(1);
    Ok(())
}

/// MGFreeThrow::ResetMiniGame() @0x8033598c
pub fn reset_mini_game(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x40);
    cl(h, vm, sp, RM_MULT, &[OFF_SCREEN_POS, t + 0x110, sp + 0x10])?;
    wr(vm, t + 0x1fc, 0);
    for i in 0..0x19u32 {
        let b = ball(vm, t, i);
        cl(h, vm, sp, "Grab__13FreeThrowBallFPC9rmVector3", &[b, sp + 0x10])?;
        let b = ball(vm, t, i);
        cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 16 * i + 0x230, b + 0x50])?;
    }
    wr(vm, t + 0x438, 0);
    let zero = lfs2(vm, -0x4730);
    for i in 0..8u32 {
        let e = t + i * 0xc;
        wr(vm, e + 0x3d8, 1);
        wr(vm, e + 0x3e0, 0);
        stfs(vm, e + 0x3dc, zero);
    }
    let area = rd(vm, t + 0x3c);
    wr(vm, t + 0x40, 0);
    wr(vm, t + 0x21c, 0);
    wr(vm, t + 0x3c0, 0);
    wb(vm, t + 0x194, 0);
    let need = rd(vm, POINTS_TO_WIN.wrapping_add(area << 2));
    cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, need])?;
    let f2 = lfs2(vm, -0x4730);
    let f1 = lfs2(vm, -0x471c);
    let f0 = lfs2(vm, -0x4728);
    stfs(vm, t + 0x204, f2);
    stfs(vm, t + 0x208, f2);
    stfs(vm, t + 0x20c, f2);
    stfs(vm, t + 0x45c, f1);
    stfs(vm, t + 0x460, f0);
    stfs(vm, t + 0x458, f2);
    wr(vm, t + 0x448, 0);
    wr(vm, t + 0x444, 0);
    Ok(())
}

/// MGFreeThrow::OnPauseReset() @0x80335ab8
pub fn on_pause_reset(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    let vt = rd(vm, t);
    let f = rd(vm, vt + 0x30);
    gca(h, vm, sp, f, &[t], &[])?;
    cl(h, vm, sp, "ChangeGameState__11MGFreeThrowF13MinigameState", &[t, 3])?;
    cl(h, vm, sp, "OnPauseReset__8MinigameFv", &[t])?;
    Ok(())
}

/// MGFreeThrow::ShootBasketball(float) @0x80335ba8
pub fn shoot_basketball(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let arg = vm.st.cpu.f[1];
    let sp = fsp(vm, 0x40);
    let mut f30 = arg;
    let zero = lfs2(vm, -0x4730);
    let f1 = if ge(arg, zero) { arg } else { -arg };
    let limit = lfs(vm, t + 0x20c);
    if f1 < limit {
        f30 = lfs2(vm, -0x4730);
    }
    let f31;
    if rb8(vm, t + 0x464) != 0 {
        let aim = lfs(vm, t + 0x458);
        let zero = lfs2(vm, -0x4730);
        let a = if ge(aim, zero) { aim } else { -aim };
        let k = lfs2(vm, -0x4708);
        f31 = if a < k { lfs2(vm, -0x4730) } else { aim };
    } else {
        f31 = lfs2(vm, -0x4730);
    }
    let player = rd(vm, t + 0x190);
    let anim = rd(vm, player + 0x18);
    let k = lfs2(vm, -0x4724);
    gc(h, vm, sp, "SetNextAnimState__14AnimationStateFQ219AnimationStateGraph10AnimStatesfbi", &[anim, 0x2d, 0, u32::MAX], &[k])?;
    let player = rd(vm, t + 0x190);
    wr(vm, player + 0x13c, 0x898);
    let f2 = lfs2(vm, -0x4700);
    let f0 = lfs2(vm, -0x4704);
    let power = lfs(vm, t + 0x208);
    let i = fctiwz(fmadds(f2, power, f0)) as i32;
    let p = if i < 0 { 0 } else if i > 100 { 100 } else { i };
    cl(h, vm, sp, "ShotMeter_SetPower__16WorldHudHandlersFi", &[p as u32])?;
    let idx = rd(vm, t + 0x1fc);
    let b = ball(vm, t, idx);
    gc(h, vm, sp, "InitializeThrow__13FreeThrowBallFff", &[b], &[f30, f31])?;
    Ok(())
}

/// MGFreeThrow::OpenPauseMenu() @0x803366c4
pub fn open_pause_menu(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    if rb8(vm, t + 0x4e) == 0 && (rd(vm, t + 0x474) as i32) <= 0 {
        wb(vm, t + 0x4e, 1);
        wb(vm, t + 0x24, 1);
        let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
        wb(vm, fe + 0x48, 1);
        wb(vm, fe + 0x49, 1);
        let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
        cl(h, vm, sp, "OpenAptOverlay__9FEManagerFPc", &[fe, 0x804d_8e88])?;
        let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
        cl(h, vm, sp, "PlaySFX__5AudioF17AUDIOAEMSFEHUDSFXii", &[a, 0xc, 0, 0x64])?;
        let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
        cl(h, vm, sp, "Pause__5AudioFQ25Audio9PAUSEMODE", &[a, 2])?;
    }
    Ok(())
}

/// MGFreeThrow::ClosePauseMenu() @0x80336748
pub fn close_pause_menu(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = fsp(vm, 0x10);
    wb(vm, t + 0x24, 0);
    let fe = cl(h, vm, sp, "GetInstance__9FEManagerFv", &[])?;
    cl(h, vm, sp, "CloseAptOverlay__9FEManagerFv", &[fe])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "UnPause__5AudioFv", &[a])?;
    wb(vm, t + 0x4e, 0);
    wr(vm, t + 0x474, 0x4b0);
    Ok(())
}

/// MGFreeThrow::ProcessBasket(int) @0x80336ad4
pub fn process_basket(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let sp = dsp(vm, 0x70);
    let score = rb8(vm, t + 0x194).wrapping_add(1);
    let streak_off = rb8(vm, t + 0x46c);
    wb(vm, t + 0x194, score);
    if streak_off == 0 {
        let f2 = lfs(vm, t + 0x20c);
        let f1 = lfs2(vm, -0x46c8);
        let f0 = lfs2(vm, -0x46c4);
        let f1 = fsubs(f2, f1);
        stfs(vm, t + 0x20c, f1);
        if f1 < f0 {
            stfs(vm, t + 0x20c, f0);
        }
    }
    let f1 = lfs(vm, t + 0x45c);
    let k = lfs2(vm, -0x46c0);
    let area = rd(vm, t + 0x3c);
    let f2 = fadds(f1, k);
    let f1 = lfs(vm, t + 0x204);
    let f0 = lfs(vm, t + 0x20c);
    stfs(vm, t + 0x45c, f2);
    let f31 = fadds(f1, f0);
    let f30 = fsubs(f1, f0);
    let points = rb8(vm, t + 0x194) as u32;
    let need = rd(vm, POINTS_TO_WIN.wrapping_add(area << 2));
    cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[points, need])?;
    let f2 = lfs2(vm, -0x4700);
    let f0 = lfs2(vm, -0x4704);
    let hi = fctiwz(fmadds(f2, f31, f0));
    let lo = fctiwz(fmadds(f2, f30, f0));
    cl(h, vm, sp, "ShotMeter_SetLimit__16WorldHudHandlersFii", &[hi, lo])?;
    let hoop = rd(vm, t + 0x220);
    for k in 0..3u32 {
        let w = rd(vm, hoop + 4 * k);
        wr(vm, sp + 0x20 + 4 * k, w);
    }
    let mgr = lwz13(vm, -0x1ed8);
    let g = cl(h, vm, sp, "CreatePartFx__13PartFxManagerFPCcRC9rmVector3", &[mgr, 0x804d_8e92, sp + 0x20])?;
    wr(vm, sp + 0x10, g);
    let mgr = lwz13(vm, -0x1ed8);
    cl(h, vm, sp, "DisableAndDestroyPartFx__13PartFxManagerF4GUIDi", &[mgr, sp + 0x10, 0x9c4])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x7f, 0, 0x64])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "PlaySFX__5AudioF14AUDIOAEMSBESFXii", &[a, 0x80, 0, 0x64])?;
    Ok(())
}

/// MGFreeThrow::FreeThrowTossProcess(const Conga::CongaOutput*) @0x803370a0
pub fn free_throw_toss_process(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let out = vm.a(1);
    let sp = fsp(vm, 0x40);
    let mut f30 = lfs2(vm, -0x4730);
    let key = rd(vm, out + 8);
    let mut n = 0u32;
    while n < 10 {
        let hist = cl(h, vm, sp, "GetInstance__Q32EA5Conga12CongaHistoryFv", &[])?;
        let a = cl(h, vm, sp, "GetHistoryOutput__Q32EA5Conga12CongaHistoryFii", &[hist, key, n])?;
        if a == 0 {
            break;
        }
        let hist = cl(h, vm, sp, "GetInstance__Q32EA5Conga12CongaHistoryFv", &[])?;
        let b = cl(h, vm, sp, "GetHistoryOutput__Q32EA5Conga12CongaHistoryFii", &[hist, key, n + 1])?;
        if b == 0 {
            break;
        }
        cl(h, vm, sp, "Magnitude__Q42EA5Conga4Math14Coordinate3<f>CFv", &[a + 0x6c])?;
        let first = vm.st.cpu.f[1];
        cl(h, vm, sp, "Magnitude__Q42EA5Conga4Math14Coordinate3<f>CFv", &[b + 0x6c])?;
        let second = vm.st.cpu.f[1];
        if second < first {
            let k = lfs2(vm, -0x469c);
            f30 = fdivs(first, k);
            break;
        }
        n += 1;
    }
    let cnt = rd(vm, t + 0x438);
    if (cnt as i32) < 7 {
        let e = t.wrapping_add(cnt.wrapping_mul(0xc));
        wr(vm, e + 0x3e0, key);
        let cnt = rd(vm, t + 0x438);
        let e = t.wrapping_add(cnt.wrapping_mul(0xc));
        stfs(vm, e + 0x3dc, f30);
        let cnt = rd(vm, t + 0x438);
        let e = t.wrapping_add(cnt.wrapping_mul(0xc));
        wr(vm, e + 0x3d8, 0);
        let cnt = rd(vm, t + 0x438);
        wr(vm, t + 0x438, cnt.wrapping_add(1));
    }
    Ok(())
}

/// MGFreeThrow::FreeThrowTossCallback(const Conga::CongaOutput*, void*) @0x803371b4 (tail call: swaps the arguments)
pub fn free_throw_toss_callback(h: &mut MgHost, vm: &mut V) -> R {
    let (out, t) = (vm.a(0), vm.a(1));
    let sp = fsp(vm, 0);
    cl(h, vm, sp, "FreeThrowTossProcess__11MGFreeThrowFPCQ32EA5Conga11CongaOutput", &[t, out])?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Initialize
// ---------------------------------------------------------------------------------------------------------------------

/// The 4x4 product of the inlined rmMatrix4 multiply (paired singles): `out[i][j] = sum_k x[i][k] * y[k][j]`,
/// accumulated in the order k = 0..3 with a single rounding per step.
fn mat_mult(vm: &mut V, x: u32, y: u32, out: u32) {
    let mut res = [0f32; 16];
    for i in 0..4u32 {
        for j in 0..4u32 {
            let xik = |vm: &mut V, k: u32| vm.st.mem.rf32(x + 16 * i + 4 * k) as f64;
            let ykj = |vm: &mut V, k: u32| vm.st.mem.rf32(y + 16 * k + 4 * j) as f64;
            let mut acc = ((ykj(vm, 0) * xik(vm, 0)) as f32) as f64;
            for k in 1..4u32 {
                acc = ((ykj(vm, k) * xik(vm, k) + acc) as f32) as f64;
            }
            res[(4 * i + j) as usize] = acc as f32;
        }
    }
    for (n, v) in res.iter().enumerate() {
        vm.st.mem.wf32(out + 4 * n as u32, *v);
    }
}

const STR: u32 = 0x804d_8dd0;

/// MGFreeThrow::Initialize(const World*) @0x80334d04
pub fn initialize(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let world = vm.a(1);
    let sp = dsp(vm, 0x240);
    let cm = lwz13(vm, -0x1d94);
    let cam = rd(vm, cm);
    let info = cl(h, vm, sp, "GetCameraViewInfo__6CameraFb", &[cam, 0])?;
    cl(h, vm, sp, "__ct__14CameraViewInfoFRC14CameraViewInfo", &[sp + 0x1c0, info])?;
    wr(vm, t + 0x30, 0xd);
    wr(vm, t + 0x2c, 2);
    cl(h, vm, sp, "Initialize__8MinigameFPC5World", &[t, world])?;
    cl(h, vm, sp, "__ct__7rmAngleFv", &[sp + 0x38])?;
    // the angle of the free throw position, wrapped into [0, 2 pi)
    let area = rd(vm, t + 0x3c);
    let k2pi = lfs2(vm, -0x472c);
    let mut f0 = lfs(vm, 0x8044_1900u32.wrapping_add(area << 2));
    stfs(vm, sp + 0x38, f0);
    loop {
        let f1 = fs(f0);
        if !ge(f1, k2pi) {
            break;
        }
        f0 = fsubs(fs(f0), k2pi);
        stfs(vm, sp + 0x38, f0);
    }
    let k2pi = lfs2(vm, -0x472c);
    let zero = lfs2(vm, -0x4730);
    loop {
        let f2 = fs(f0);
        if !(f2 < zero) {
            break;
        }
        f0 = fadds(fs(f0), k2pi);
        stfs(vm, sp + 0x38, f0);
    }
    let area = rd(vm, t + 0x3c);
    let p = POSITIONS.wrapping_add(area << 4);
    let (f1, f2, f3) = (lfs(vm, p), lfs(vm, p + 4), lfs(vm, p + 8));
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[sp + 0x90], &[f1, f2, f3])?;
    let angle = lfs(vm, sp + 0x38);
    gc(h, vm, sp, "fSinCos__Q22EA4MathFfRfRf", &[sp + 0x34, sp + 0x30], &[angle])?;
    // rotation about Y at sp+0x120
    let s = lfs(vm, sp + 0x34);
    let zero = lfs2(vm, -0x4730);
    let one = lfs2(vm, -0x4728);
    let c = lfs(vm, sp + 0x30);
    stfs(vm, sp + 8, s);
    stfs(vm, sp + 0xc, zero);
    stfs(vm, sp + 0x10, c);
    for k in 0..3u32 {
        stfs(vm, sp + 0x14 + 4 * k, zero);
    }
    stfs(vm, sp + 0x24, one);
    let (s, c) = (lfs(vm, sp + 0x34), lfs(vm, sp + 0x30));
    gc(h, vm, sp, "Set__Q22EA4MathFRQ32EA4Math8Matrix44ffffffffffffffff", &[sp + 0x120], &[c, zero, -s, zero, zero, one, zero, zero])?;
    // translation to the position at sp+0xe0
    let zero = lfs2(vm, -0x4730);
    let one = lfs2(vm, -0x4728);
    stfs(vm, sp + 8, zero);
    stfs(vm, sp + 0xc, zero);
    stfs(vm, sp + 0x10, one);
    stfs(vm, sp + 0x14, zero);
    for k in 0..3u32 {
        let v = lfs(vm, sp + 0x90 + 4 * k);
        stfs(vm, sp + 0x18 + 4 * k, v);
    }
    stfs(vm, sp + 0x24, one);
    gc(h, vm, sp, "Set__Q22EA4MathFRQ32EA4Math8Matrix44ffffffffffffffff", &[sp + 0xe0], &[one, zero, zero, zero, zero, one, zero, zero])?;
    mat_mult(vm, sp + 0x120, sp + 0xe0, t + 0x110);
    cl(h, vm, sp, "Matrix44InverseRotTrans__Q22EA4MathFRCQ32EA4Math8Matrix44RQ32EA4Math8Matrix44", &[t + 0x110, t + 0x150])?;
    let q = POSITIONS + 0x40;
    let (f1, f2, f3) = (lfs(vm, q), lfs(vm, q + 4), lfs(vm, q + 8));
    gc(h, vm, sp, "Set__Q32EA4Math7Vector3Ffff", &[t + 0x100], &[f1, f2, f3])?;
    cl(h, vm, sp, RM_MULT, &[t + 0x100, t + 0x110, sp + 0x80])?;
    // the court
    let mem = cl(h, vm, sp, "__nw__FUl", &[0x170])?;
    let court = if mem != 0 { cl(h, vm, sp, "__ct__18FreeThrowBallCourtFv", &[mem])? } else { mem };
    let area = rd(vm, t + 0x3c);
    wr(vm, t + 0x220, court);
    let p = POSITIONS.wrapping_add(area << 4);
    let (a, b, c) = (rd(vm, p), rd(vm, p + 4), rd(vm, p + 8));
    wr(vm, sp + 0x74, b);
    wr(vm, sp + 0x70, a);
    let y = lfs(vm, sp + 0x74);
    let k = lfs2(vm, -0x4724);
    wr(vm, sp + 0x78, c);
    stfs(vm, sp + 0x74, fadds(y, k));
    let court = rd(vm, t + 0x220);
    cl(h, vm, sp, "Initialize__18FreeThrowBallCourtFPC9rmVector3PC9rmVector3", &[court, sp + 0x80, sp + 0x70])?;
    cl(h, vm, sp, RM_MULT, &[OFF_SCREEN_POS, t + 0x110, sp + 0x60])?;
    cl(h, vm, sp, "Load__13FreeThrowBallFv", &[])?;
    wr(vm, t + 0x1fc, 0);
    for i in 0..0x19u32 {
        let pool = lwz13(vm, -0x43fc);
        let mem = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0xc0, pool, 0, 0x804d_8f2a])?;
        let mut b = mem;
        if mem != 0 {
            let k = lfs2(vm, -0x4720);
            b = gc(h, vm, sp, "__ct__13FreeThrowBallFPC9rmVector3f", &[mem, sp + 0x60], &[k])?;
        }
        wr(vm, t + 4 * i + 0x198, b);
        cl(h, vm, sp, "Grab__13FreeThrowBallFPC9rmVector3", &[b, sp + 0x60])?;
        let b = rd(vm, t + 4 * i + 0x198);
        cl(h, vm, sp, "__as__9rmVector3FRC9rmVector3", &[t + 16 * i + 0x230, b + 0x50])?;
    }
    wr(vm, t + 0x40, 0);
    wr(vm, t + 0x21c, 0);
    wr(vm, t + 0x438, 0);
    let zero = lfs2(vm, -0x4730);
    for i in 0..8u32 {
        let e = t + i * 0xc;
        wr(vm, e + 0x3d8, 1);
        wr(vm, e + 0x3e0, 0);
        stfs(vm, e + 0x3dc, zero);
    }
    let zero = lfs2(vm, -0x4730);
    let court = rd(vm, t + 0x220);
    let spawn = court + 0x10;
    wr(vm, t + 0x34, 0);
    wr(vm, t + 0x3c8, 0);
    wr(vm, t + 0x3cc, 0);
    wr(vm, t + 0x3d0, 0x1ea);
    wr(vm, t + 0x3d4, 0x1388);
    wb(vm, t + 0x210, 0);
    wr(vm, t + 0x214, 0);
    for o in [0x204, 0x208, 0x20c] {
        stfs(vm, t + o, zero);
    }
    let chr = cl(h, vm, sp, "GetPlayerCharacter__8WorldManFi", &[WORLD_MAN, 0])?;
    cl(h, vm, sp, "InitializePlayer__11MGFreeThrowFPC9rmVector3P9Character", &[t, spawn, chr])?;
    for i in 0..4u32 {
        let c = cl(h, vm, sp, "Get__10ControllerFi", &[i])?;
        cl(h, vm, sp, "SetCurrentControllerState__10ControllerF16EControllerState", &[c, 0x18])?;
    }
    // the fixed camera
    let cm = lwz13(vm, -0x1d94);
    let fixed = cl(h, vm, sp, "GetFixedCamera__13CameraManagerFUi", &[cm, 0])?;
    if fixed != 0 {
        cl(h, vm, sp, RM_MULT, &[POSITIONS + 0x60, t + 0x110, sp + 0x50])?;
        cl(h, vm, sp, RM_MULT, &[POSITIONS + 0x50, t + 0x110, sp + 0x40])?;
        cl(h, vm, sp, "SetPos__6CameraFRC9rmVector3", &[fixed, sp + 0x50])?;
        cl(h, vm, sp, "SetTarget__6CameraFRC9rmVector3", &[fixed, sp + 0x40])?;
        // the saved view info is passed by value: copied to the parameter area
        for k in 0..24u32 {
            let w = rd(vm, sp + 0x1c0 + 4 * k);
            wr(vm, sp + 0x160 + 4 * k, w);
        }
        let cm = lwz13(vm, -0x1d94);
        let cam = rd(vm, cm);
        cl(h, vm, sp, "StartCameraSysTransition__6CameraF14CameraViewInfoib", &[cam, sp + 0x160, 0x12c, 0])?;
    }
    // the marker of the ball hand
    let anim = player_anim(vm, t);
    let count = rd(vm, anim + 0x1670);
    let mut i = 0u32;
    while (i as i32) < (count as i32) {
        let anim = player_anim(vm, t);
        if cl(h, vm, sp, "GetMarkerID__14AnimationStateCFi", &[anim, i])? == 0xf {
            wr(vm, t + 0x43c, i);
            break;
        }
        i += 1;
    }
    let cb = cl(h, vm, sp, "GetInstance__Q32EA5Conga15CallbackManagerFv", &[])?;
    cl(h, vm, sp, "RegisterCallback__Q32EA5Conga15CallbackManagerFPCcPFPCQ32EA5Conga11CongaOutputPv_vPv", &[cb, STR + 0x54, 0x8033_71b4, t])?;
    wr(vm, t + 0x450, u32::MAX);
    let am = lwz13(vm, -0x1ca4);
    let tex = cl(h, vm, sp, "GetLoadedTexture__12AssetManagerFPCcPi", &[am, STR + 0x62, t + 0x450])?;
    wr(vm, t + 0x44c, tex);
    let alloc = lwz13(vm, -0x41a8);
    let mut obj = gca(h, vm, sp, alloc, &[0x5c, 0], &[])?;
    if obj != 0 {
        let tex = rd(vm, t + 0x44c);
        let off = rd(vm, tex + 0x14);
        obj = cl(h, vm, sp, "__ct__Q24EAGL3TARFPC5SHAPE", &[obj, tex.wrapping_add(off)])?;
    }
    let mode = rd(vm, t + 0x44) as i32;
    let k1 = lfs2(vm, -0x471c);
    let k0 = lfs2(vm, -0x4728);
    wr(vm, t + 0x454, obj);
    stfs(vm, t + 0x45c, k1);
    stfs(vm, t + 0x460, k0);
    match mode {
        0 => {
            wb(vm, t + 0x464, 0);
            wb(vm, t + 0x46c, 1);
        }
        1 => {
            wb(vm, t + 0x464, 1);
            wb(vm, t + 0x46c, 1);
        }
        2 => {
            wb(vm, t + 0x464, 1);
            wb(vm, t + 0x46c, 0);
        }
        _ => {}
    }
    let db = lwz13(vm, -0x260c);
    let sdata = ad13(vm, -0x4a28);
    let coll = cl(h, vm, sp, "GetCollection__11pgIDatabaseFPCcPCc", &[db, STR + 0x6d, sdata])?;
    let area = rd(vm, t + 0x3c);
    let v = cl(h, vm, sp, "GetInt32FromArray__14pgDBCollectionFPCcUi", &[coll, STR + 0x81, area])?;
    wr(vm, t + 0x470, v);
    let db = lwz13(vm, -0x260c);
    cl(h, vm, sp, "DestroyCollection__11pgIDatabaseFP14pgDBCollection", &[db, coll])?;
    let pool = lwz13(vm, -0x43fc);
    let mem = cl(h, vm, sp, "Alloc__6MemMgrFUlQ26MemMgr8PoolTypeQ26MemMgr9AllocTypePCc", &[0x14, pool, 0, 0x804d_8efc])?;
    let mut listener = mem;
    if mem != 0 {
        let chr = rd(vm, t + 0x190);
        listener = cl(h, vm, sp, "__ct__35FreeThrowCharacterCollisionListenerFP9Character", &[mem, chr])?;
    }
    wr(vm, t + 0x468, listener);
    wr(vm, t + 0x3c0, u32::MAX);
    cl(h, vm, sp, "WorldHud_SetMicroGame__16WorldHudHandlersFi", &[3])?;
    cl(h, vm, sp, "Timer_SetVisible__16WorldHudHandlersFi", &[1])?;
    cl(h, vm, sp, "Timer_SetValue__16WorldHudHandlersFi", &[0x1e])?;
    let area = rd(vm, t + 0x3c);
    let need = rd(vm, POINTS_TO_WIN.wrapping_add(area << 2));
    cl(h, vm, sp, "Counter_SetValue__16WorldHudHandlersFii", &[0, need])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "LoadData__5AudioF9AUDIODATA", &[a, 0x10])?;
    let a = cl(h, vm, sp, "Instance__5AudioFv", &[])?;
    cl(h, vm, sp, "PlayMusic__5AudioF15AUDIOMUSICTYPES", &[a, 1])?;
    let w = rd(vm, WORLD_MAN + 0x88);
    cl(h, vm, sp, "StartMinigameFadeOutEffect__15PlaygroundWorldFi", &[w, u32::MAX])?;
    let w = rd(vm, WORLD_MAN + 0x88);
    cl(h, vm, sp, "EnableFadeOutEffectRenders__15PlaygroundWorldFbb", &[w, 0, 1])?;
    Ok(())
}

/// rmNormalizeXZ(const rmVector3&, float, rmVector3&) @0x80333b30
pub fn rm_normalize_xz_scaled(h: &mut MgHost, vm: &mut V) -> R {
    let v = vm.a(0);
    let out = vm.a(1);
    let scale = vm.st.cpu.f[1];
    let sp = fsp(vm, 0x20);
    cl(h, vm, sp, "rmNormalizeXZ__FRC9rmVector3R9rmVector3", &[v, out])?;
    let (x, y, z) = (lfs(vm, out), lfs(vm, out + 4), lfs(vm, out + 8));
    stfs(vm, out, fmuls(x, scale));
    stfs(vm, out + 4, fmuls(y, scale));
    stfs(vm, out + 8, fmuls(z, scale));
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// DrawCursor
// ---------------------------------------------------------------------------------------------------------------------

/// `lwz r, off(r2)` (a word of a small data area 2 constant).
fn lwz2(vm: &mut V, off: i32) -> u32 {
    let a = vm.st.cpu.r[2].wrapping_add(off as u32);
    rd(vm, a)
}

/// MGFreeThrow::DrawCursor(const rmVector3*, const rmVector3*) const @0x80336e44
pub fn draw_cursor(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let at = vm.a(1);
    let dir = vm.a(2);
    let sp = dsp(vm, 0x160);
    /// The static cursor geometry table: `lis r27,-0x7fbc ; addi r27,r27,0x1900`.
    const TBL: u32 = 0x8044_1900;
    let area = rd(vm, WORLD_MAN + 0x8c);
    let area_mgr = rd(vm, area + 8);
    cl(h, vm, sp, "CalcRenderingModelMatrix__11AreaManagerFRC9rmVector3R9rmMatrix4", &[area_mgr, at, sp + 0x78])?;
    let (bx, bz) = (lfs(vm, dir), lfs(vm, dir + 8));
    let k = lfs2(vm, -0x46a0);
    let f4 = fmuls(k, -bx);
    let f2 = fmuls(k, -bz);
    let f1 = fmuls(k, bx);
    let f0 = fmuls(k, bz);
    // the four corners (x, y, z): y from the table, x and z from the direction
    stfs(vm, sp + 0x6c, f4);
    let w = rd(vm, TBL + 0x10);
    wr(vm, sp + 0x70, w);
    stfs(vm, sp + 0x74, f2);
    stfs(vm, sp + 0x60, f4);
    let w = rd(vm, TBL + 0x1c);
    wr(vm, sp + 0x64, w);
    stfs(vm, sp + 0x68, f2);
    stfs(vm, sp + 0x54, f1);
    let w = rd(vm, TBL + 0x28);
    wr(vm, sp + 0x58, w);
    stfs(vm, sp + 0x5c, f0);
    stfs(vm, sp + 0x48, f1);
    let w = rd(vm, TBL + 0x34);
    wr(vm, sp + 0x4c, w);
    stfs(vm, sp + 0x50, f0);
    // the texture coordinates
    let uv = [(0x40u32, -0x46b8i32), (0x44, -0x46b4), (0x38, -0x46b0), (0x3c, -0x46ac), (0x28, -0x46a8), (0x2c, -0x46a4)];
    for (o, k) in uv {
        let w = lwz2(vm, k);
        wr(vm, sp + o, w);
    }
    wr(vm, sp + 0x30, 0);
    wr(vm, sp + 0x34, 0);
    let draw = sp + 0xb8;
    let state = sp + 0x20;
    cl(h, vm, sp, "__ct__Q24EAGL12DrawTexturedFv", &[draw])?;
    cl(h, vm, sp, "Init__Q24EAGL12DrawTexturedFUi", &[draw, 0x200])?;
    cl(h, vm, sp, "__ct__Q24EAGL12GeoPrimStateFv", &[state])?;
    cl(h, vm, sp, "SetAlphaBlendMode__Q24EAGL12GeoPrimStateFQ24EAGL14AlphaBlendMode", &[state, 1])?;
    cl(h, vm, sp, "SetTransparencyMethod__Q24EAGL12GeoPrimStateFQ24EAGL18TransparencyMethod", &[state, 1])?;
    cl(h, vm, sp, "SetState__Q24EAGL12DrawTexturedFRCQ24EAGL12GeoPrimState", &[draw, state])?;
    let tex = rd(vm, t + 0x454);
    cl(h, vm, sp, "SetTexture__Q24EAGL12DrawTexturedFPQ24EAGL3TAR", &[draw, tex])?;
    cl(h, vm, sp, "SetModelMatrix__Q24EAGL12DrawTexturedFR7MATRIX4", &[draw, sp + 0x78])?;
    cl(h, vm, sp, "Begin__Q24EAGL12DrawTexturedFQ24EAGL13PrimitiveType", &[draw, 0x90])?;
    // (coord, colour slot, texture coordinate) of the six vertices
    let verts = [(0x6cu32, 0x1cu32, 0x40u32), (0x60, 0x18, 0x38), (0x54, 0x14, 0x30), (0x54, 0x10, 0x30), (0x48, 0xc, 0x28), (0x6c, 8, 0x40)];
    for (c, col, uv) in verts {
        wr(vm, sp + col, u32::MAX);
        cl(h, vm, sp, "AddVertex__Q24EAGL12DrawTexturedFRC6COORD3RCQ24EAGL6ColourRC6COORD2", &[draw, sp + c, sp + col, sp + uv])?;
    }
    cl(h, vm, sp, "End__Q24EAGL12DrawTexturedFv", &[draw])?;
    cl(h, vm, sp, "__dt__Q24EAGL12GeoPrimStateFv", &[state, u32::MAX])?;
    cl(h, vm, sp, "__dt__Q24EAGL12DrawTexturedFv", &[draw, u32::MAX])?;
    Ok(())
}

pub const PORTS: &[crate::mgvm::ports::Port] = &[
    ("__sinit_\\dribblingmanager_cpp", sinit_dribbling),
    ("rmfMax__Fff", rmf_max),
    ("__sinit_\\freethrowball_cpp", sinit_ball),
    ("__sinit_\\freethrowballcourt_cpp", sinit_ball_court),
    ("__sinit_\\freethrowcharactercollisionlistener_cpp", sinit_character_listener),
    ("__sinit_\\freethrowcollisionlistener_cpp", sinit_listener),
    ("rmMatrix4RotationY__FfR9rmMatrix4", rm_matrix4_rotation_y),
    ("__dt__15PhysicsUserDataFv", physics_user_data_dtor),
    ("OnPauseContinue__11MGFreeThrowFv", on_pause_continue),
    ("UpdateAimOffset__11MGFreeThrowFi", update_aim_offset),
    ("Update__11MGFreeThrowFi", update),
    ("Draw__11MGFreeThrowFRQ23Ren12SceneContext", draw),
    ("SetDir__14CharacterStateFPC9rmVector3", set_dir),
    ("UpdateWaitForApocalypse__11MGFreeThrowFi", update_wait_for_apocalypse),
    ("UpdateHUD__11MGFreeThrowFi", update_hud),
    ("HasBallEnteredHoop__11MGFreeThrowFi", has_ball_entered_hoop),
    ("GrabBall__11MGFreeThrowFiPC9rmVector3", grab_ball),
    ("IsWithinGrabDistance__11MGFreeThrowCFi", is_within_grab_distance),
    ("CheckDare__8MinigameFv", check_dare),
    ("IsMiniGame__8MinigameFv", is_mini_game),
    ("__sinit_\\mgfreethrow_cpp", sinit_mgfreethrow),
    ("__sinit_\\highfivemanager_cpp", sinit_highfive),
    ("__ct__11MGFreeThrowFPQ23Ren5Scene", ctor),
    ("__dt__11MGFreeThrowFv", dtor),
    ("InitializePlayer__11MGFreeThrowFPC9rmVector3P9Character", initialize_player),
    ("UnInitialize__11MGFreeThrowFv", uninitialize),
    ("ChangeGameState__11MGFreeThrowF13MinigameState", change_game_state),
    ("UpdateIntro__11MGFreeThrowFi", update_intro),
    ("UpdateOutro__11MGFreeThrowFi", update_outro),
    ("UpdateGame__11MGFreeThrowFi", update_game),
    ("ResetMiniGame__11MGFreeThrowFv", reset_mini_game),
    ("OnPauseReset__11MGFreeThrowFv", on_pause_reset),
    ("ShootBasketball__11MGFreeThrowFf", shoot_basketball),
    ("OpenPauseMenu__11MGFreeThrowFv", open_pause_menu),
    ("ClosePauseMenu__11MGFreeThrowFv", close_pause_menu),
    ("ProcessBasket__11MGFreeThrowFi", process_basket),
    ("FreeThrowTossProcess__11MGFreeThrowFPCQ32EA5Conga11CongaOutput", free_throw_toss_process),
    ("FreeThrowTossCallback__11MGFreeThrowFPCQ32EA5Conga11CongaOutputPv", free_throw_toss_callback),
    ("Initialize__11MGFreeThrowFPC5World", initialize),
    ("rmNormalizeXZ__FRC9rmVector3fR9rmVector3", rm_normalize_xz_scaled),
    ("DrawCursor__11MGFreeThrowCFPC9rmVector3PC9rmVector3", draw_cursor),
];
