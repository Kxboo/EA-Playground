//! rmMath vector helpers (rmvector3.cpp and friends): small leaf functions used everywhere in the game code.
//! Float semantics follow the interpreter (`gekko::cpu`): single-precision results are rounded through f32, `fmadds` is
//! fused (one rounding).

use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
type R = Result<(), String>;

fn single(x: f64) -> f64 {
    x as f32 as f64
}
fn lfs(vm: &mut V, a: u32) -> f64 {
    vm.st.mem.rf32(a) as f64
}
fn stfs(vm: &mut V, a: u32, v: f64) {
    vm.st.mem.wf32(a, v as f32);
}
fn ret_single(vm: &mut V, v: f64) {
    vm.st.cpu.f[1] = v;
    vm.st.cpu.ps1[1] = v;
}

/// rmAdd(const rmVector3&, const rmVector3&, rmVector3&) @0x802ce970
pub fn rm_add(_h: &mut MgHost, vm: &mut V) -> R {
    let (a, b, out) = (vm.a(0), vm.a(1), vm.a(2));
    let (x, y, z) = (single(lfs(vm, a) + lfs(vm, b)), single(lfs(vm, a + 4) + lfs(vm, b + 4)), single(lfs(vm, a + 8) + lfs(vm, b + 8)));
    stfs(vm, out, x);
    stfs(vm, out + 4, y);
    stfs(vm, out + 8, z);
    Ok(())
}

/// rmSub(const rmVector3&, const rmVector3&, rmVector3&) @0x802ccda8
pub fn rm_sub(_h: &mut MgHost, vm: &mut V) -> R {
    let (a, b, out) = (vm.a(0), vm.a(1), vm.a(2));
    let (x, y, z) = (single(lfs(vm, a) - lfs(vm, b)), single(lfs(vm, a + 4) - lfs(vm, b + 4)), single(lfs(vm, a + 8) - lfs(vm, b + 8)));
    stfs(vm, out, x);
    stfs(vm, out + 4, y);
    stfs(vm, out + 8, z);
    Ok(())
}

/// rmVector3::operator=(const rmVector3&) @0x802cd4cc (lfs/stfs, stores y, x, z in that order)
pub fn rm_vector3_assign(_h: &mut MgHost, vm: &mut V) -> R {
    let (dst, src) = (vm.a(0), vm.a(1));
    let (z, y, x) = (lfs(vm, src + 8), lfs(vm, src + 4), lfs(vm, src));
    stfs(vm, dst + 4, y);
    stfs(vm, dst, x);
    stfs(vm, dst + 8, z);
    Ok(())
}

/// EA::Math::Vector3::Set(float, float, float) @0x802e3a28
pub fn ea_vector3_set(_h: &mut MgHost, vm: &mut V) -> R {
    let (t, f) = (vm.a(0), vm.st.cpu.f);
    stfs(vm, t, f[1]);
    stfs(vm, t + 4, f[2]);
    stfs(vm, t + 8, f[3]);
    Ok(())
}

/// EA::Conga::Math::Coordinate3<float>::operator=(const Coordinate3<float>&) @0x8025eab4
pub fn conga_coordinate3_assign(_h: &mut MgHost, vm: &mut V) -> R {
    let (dst, src) = (vm.a(0), vm.a(1));
    let (x, y, z) = (lfs(vm, src), lfs(vm, src + 4), lfs(vm, src + 8));
    stfs(vm, dst, x);
    stfs(vm, dst + 4, y);
    stfs(vm, dst + 8, z);
    Ok(())
}

/// rmVector4::rmVector4(float, float, float, float) @0x80344260
pub fn rm_vector4_ctor(_h: &mut MgHost, vm: &mut V) -> R {
    let (t, f) = (vm.a(0), vm.st.cpu.f);
    stfs(vm, t, f[1]);
    stfs(vm, t + 4, f[2]);
    stfs(vm, t + 8, f[3]);
    stfs(vm, t + 12, f[4]);
    Ok(())
}

/// rmAngle::rmAngle() @0x803b2138: the angle starts at the constant at r2-0x3128
pub fn rm_angle_ctor(_h: &mut MgHost, vm: &mut V) -> R {
    let (t, r2) = (vm.a(0), vm.st.cpu.r[2]);
    let v = lfs(vm, r2.wrapping_sub(0x3128));
    stfs(vm, t, v);
    Ok(())
}

/// rmAngle::Wrap() @0x802cd4e8: subtract the full turn (r2-0x4f04) while >= it, add it while < the constant at r2-0x4f00
pub fn rm_angle_wrap(_h: &mut MgHost, vm: &mut V) -> R {
    let (t, r2) = (vm.a(0), vm.st.cpu.r[2]);
    let full = lfs(vm, r2.wrapping_sub(0x4f04));
    loop {
        let a = lfs(vm, t);
        if !(a >= full) {
            break;
        }
        stfs(vm, t, single(a - full));
    }
    let zero = lfs(vm, r2.wrapping_sub(0x4f00));
    loop {
        let a = lfs(vm, t);
        if !(a < zero) {
            break;
        }
        stfs(vm, t, single(a + full));
    }
    Ok(())
}

/// rmDot(const rmVector3&, const rmVector3&) @0x802ccddc: z*z' + (x*x' + y*y') with fused multiply-adds
pub fn rm_dot(_h: &mut MgHost, vm: &mut V) -> R {
    let (a, b) = (vm.a(0), vm.a(1));
    let yy = single(lfs(vm, a + 4) * lfs(vm, b + 4));
    let s = single(lfs(vm, a).mul_add(lfs(vm, b), yy));
    let (za, zb) = (lfs(vm, a + 8), lfs(vm, b + 8));
    ret_single(vm, single(za.mul_add(zb, s)));
    Ok(())
}

/// rmScale(const rmVector3&, float, rmVector3&) @0x802ce948
pub fn rm_scale(_h: &mut MgHost, vm: &mut V) -> R {
    let (v, out, s) = (vm.a(0), vm.a(1), vm.st.cpu.f[1]);
    let (x, y, z) = (single(lfs(vm, v) * s), single(lfs(vm, v + 4) * s), single(lfs(vm, v + 8) * s));
    stfs(vm, out, x);
    stfs(vm, out + 4, y);
    stfs(vm, out + 8, z);
    Ok(())
}

/// rmDistanceSquaredXZ(const rmVector3&, const rmVector3&) @0x802e7c9c: dx*dx + dz*dz (fmadds, one rounding)
pub fn rm_distance_squared_xz(_h: &mut MgHost, vm: &mut V) -> R {
    let (a, b) = (vm.a(0), vm.a(1));
    let dz = single(lfs(vm, a + 8) - lfs(vm, b + 8));
    let dx = single(lfs(vm, a) - lfs(vm, b));
    let zz = single(dz * dz);
    ret_single(vm, single(dx.mul_add(dx, zz)));
    Ok(())
}

/// rmDistanceSquared(const rmVector3&, const rmVector3&) @0x8035ceb4: dz*dz + (dx*dx + dy*dy)
pub fn rm_distance_squared(_h: &mut MgHost, vm: &mut V) -> R {
    let (a, b) = (vm.a(0), vm.a(1));
    let dy = single(lfs(vm, a + 4) - lfs(vm, b + 4));
    let dx = single(lfs(vm, a) - lfs(vm, b));
    let dz = single(lfs(vm, a + 8) - lfs(vm, b + 8));
    let yy = single(dy * dy);
    let s = single(dx.mul_add(dx, yy));
    ret_single(vm, single(dz.mul_add(dz, s)));
    Ok(())
}

pub const PORTS: &[crate::mgvm::ports::Port] = &[
    ("rmAdd__FRC9rmVector3RC9rmVector3R9rmVector3", rm_add),
    ("rmSub__FRC9rmVector3RC9rmVector3R9rmVector3", rm_sub),
    ("__as__9rmVector3FRC9rmVector3", rm_vector3_assign),
    ("rmScale__FRC9rmVector3fR9rmVector3", rm_scale),
    ("rmDistanceSquaredXZ__FRC9rmVector3RC9rmVector3", rm_distance_squared_xz),
    ("rmDistanceSquared__FRC9rmVector3RC9rmVector3", rm_distance_squared),
    ("Set__Q32EA4Math7Vector3Ffff", ea_vector3_set),
    ("__as__Q42EA5Conga4Math14Coordinate3<f>FRCQ42EA5Conga4Math14Coordinate3<f>", conga_coordinate3_assign),
    ("__ct__9rmVector4Fffff", rm_vector4_ctor),
    ("__ct__7rmAngleFv", rm_angle_ctor),
    ("Wrap__7rmAngleFv", rm_angle_wrap),
    ("rmDot__FRC9rmVector3RC9rmVector3", rm_dot),
];

