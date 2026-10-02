//! Hooks for `PhysicsManager`, `PhysicsRigidBody`, the character physics classes and the collision listeners.
use super::{hooks::R, physics::Mat, MgHost};
use crate::gekko::Vm;

type V = Vm<MgHost>;

fn bind(vm: &mut V, names: &[&str], f: fn(&mut MgHost, &mut V) -> R) {
    for n in names {
        if !vm.hook(n, f) {
            vm.log_missing_symbol(n);
        }
    }
}

fn read_mat(vm: &mut V, p: u32) -> Mat {
    let mut m = [0f32; 16];
    for (i, v) in m.iter_mut().enumerate() {
        *v = vm.st.mem.rf32(p + 4 * i as u32);
    }
    m
}
fn write_mat(vm: &mut V, p: u32, m: &Mat) {
    for (i, v) in m.iter().enumerate() {
        vm.st.mem.wf32(p + 4 * i as u32, *v);
    }
}
fn read_v3(vm: &mut V, p: u32) -> [f32; 3] {
    [vm.st.mem.rf32(p), vm.st.mem.rf32(p + 4), vm.st.mem.rf32(p + 8)]
}
fn write_v3(vm: &mut V, p: u32, v: [f32; 3]) {
    for i in 0..3 {
        vm.st.mem.wf32(p + 4 * i as u32, v[i]);
    }
}

// --- PhysicsManager ---------------------------------------------------------------------------------------------------

fn load_physics(h: &mut MgHost, vm: &mut V) -> R {
    let name = vm.st.mem.cstr(vm.a(1), 128);
    let mp = vm.a(2);
    let matrix = if mp != 0 { Some(read_mat(vm, mp)) } else { None };
    let (flags, add) = (vm.a(3), vm.a(4) & 0xff != 0);
    // flag 8 = camera collision, which never blocks characters or balls
    let id = h.phys.load_system(&name, matrix, add, flags & 8 != 0);
    vm.ret(id as u32);
    Ok(())
}
fn unload_physics(h: &mut MgHost, vm: &mut V) -> R {
    h.phys.unload_system(vm.a(1) as usize);
    Ok(())
}
fn generate_body(h: &mut MgHost, vm: &mut V) -> R {
    let (system, index) = (vm.a(1) as usize, vm.a(3) as usize);
    let obj = vm.alloc_zeroed(0x60, 16);
    if let Some(vt) = vm.img.addr("__vt__16PhysicsRigidBody") {
        vm.w32(obj, vt);
    }
    if h.phys.generate_body(obj, system, index) {
        vm.ret(obj);
    } else {
        h.log.push(format!("GenerateRigidBodyFromPhysicsSystem: no body {index} in system {system}"));
        vm.ret(0);
    }
    Ok(())
}
fn destroy_body(h: &mut MgHost, vm: &mut V) -> R {
    h.phys.destroy_body(vm.a(1));
    Ok(())
}
fn nothing(_h: &mut MgHost, _vm: &mut V) -> R {
    Ok(())
}
/// `GetGravity(sret, this)`
fn get_gravity(h: &mut MgHost, vm: &mut V) -> R {
    let out = vm.a(0);
    write_v3(vm, out, h.phys.gravity);
    Ok(())
}
/// `GetGroundHeight(this, const rmVector3*, float)` -> float
fn get_ground_height(h: &mut MgHost, vm: &mut V) -> R {
    let p = read_v3(vm, vm.a(1));
    let reach = vm.fa(0).abs().max(1.);
    let g = h.phys.ground_height(p, reach.min(4.), reach + 5.);
    vm.fret(g.unwrap_or(p[1]));
    Ok(())
}
fn get_ground_type(_h: &mut MgHost, vm: &mut V) -> R {
    // ground material ids 11..15 select footstep sounds; the first material is used (collision materials are not decoded)
    vm.ret(11);
    Ok(())
}
/// `CastRay(this, from*, to*, mask, float* out)` -> hit
fn cast_ray(h: &mut MgHost, vm: &mut V) -> R {
    let (from, to) = (read_v3(vm, vm.a(1)), read_v3(vm, vm.a(2)));
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-6 {
        vm.ret(0);
        return Ok(());
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    match h.phys.cast_ray(from, dir, len) {
        Some(t) => {
            let out = vm.a(4);
            if out != 0 {
                vm.st.mem.wf32(out, t / len);
            }
            vm.ret(1);
        }
        None => vm.ret(0),
    }
    Ok(())
}
fn update_physics(h: &mut MgHost, vm: &mut V) -> R {
    let ms = vm.a(1) as i32;
    let contacts = h.phys.step(ms);
    h.pending_contacts.extend(contacts);
    Ok(())
}

// --- PhysicsRigidBody -------------------------------------------------------------------------------------------------

fn set_pos(h: &mut MgHost, vm: &mut V) -> R {
    let p = read_v3(vm, vm.a(1));
    h.phys.set_body_pos(vm.a(0), p);
    Ok(())
}
fn set_mat(h: &mut MgHost, vm: &mut V) -> R {
    let m = read_mat(vm, vm.a(1));
    h.phys.set_body_mat(vm.a(0), &m);
    Ok(())
}
/// `GetMat(sret, this)`
fn get_mat(h: &mut MgHost, vm: &mut V) -> R {
    let (out, this) = (vm.a(0), vm.a(1));
    let m = h.phys.body_mat(this).unwrap_or_else(super::physics::mat_identity);
    write_mat(vm, out, &m);
    Ok(())
}
fn get_pos(h: &mut MgHost, vm: &mut V) -> R {
    let (out, this) = (vm.a(0), vm.a(1));
    let p = h.phys.body_pos(this);
    write_v3(vm, out, p);
    Ok(())
}
fn get_linvel(h: &mut MgHost, vm: &mut V) -> R {
    let (out, this) = (vm.a(0), vm.a(1));
    let v = h.phys.linvel(this);
    write_v3(vm, out, v);
    Ok(())
}
fn get_angvel(h: &mut MgHost, vm: &mut V) -> R {
    let (out, this) = (vm.a(0), vm.a(1));
    let v = h.phys.angvel(this);
    write_v3(vm, out, v);
    Ok(())
}
fn set_linvel(h: &mut MgHost, vm: &mut V) -> R {
    let v = read_v3(vm, vm.a(1));
    h.phys.set_linvel(vm.a(0), v);
    Ok(())
}
fn set_angvel(h: &mut MgHost, vm: &mut V) -> R {
    let v = read_v3(vm, vm.a(1));
    h.phys.set_angvel(vm.a(0), v);
    Ok(())
}
/// `ApplyForce(this, float scale, const rmVector3& dir)`
fn apply_force(h: &mut MgHost, vm: &mut V) -> R {
    let (s, d) = (vm.fa(0), read_v3(vm, vm.a(1)));
    h.phys.apply_force(vm.a(0), s, d);
    Ok(())
}
fn get_mass(h: &mut MgHost, vm: &mut V) -> R {
    let m = h.phys.mass(vm.a(0));
    vm.fret(m);
    Ok(())
}
fn set_user_data(h: &mut MgHost, vm: &mut V) -> R {
    let (b, ud) = (vm.a(0), vm.a(1));
    if let Some(body) = h.phys.bodies.get_mut(&b) {
        body.user_data = ud;
    }
    Ok(())
}
/// `PhysicsRigidBodyListener(this, body)`: remember which listener belongs to the body.
fn rb_listener_ct(h: &mut MgHost, vm: &mut V) -> R {
    let (this, body) = (vm.a(0), vm.a(1));
    if let Some(vt) = vm.img.addr("__vt__24PhysicsRigidBodyListener") {
        vm.w32(this, vt);
    }
    vm.w32(this + 4, body);
    if let Some(b) = h.phys.bodies.get_mut(&body) {
        b.listener = this;
    }
    Ok(())
}

// --- characters -------------------------------------------------------------------------------------------------------

/// `PhysicsDynamicCharacter(this, Character*, const rmVector3* pos, PhysicsManager*)`
fn char_ct(h: &mut MgHost, vm: &mut V) -> R {
    let (this, character, pos) = (vm.a(0), vm.a(1), vm.a(2));
    let p = if pos != 0 { read_v3(vm, pos) } else { [0.; 3] };
    let vt = vm.img.addr("__vt__23PhysicsDynamicCharacter");
    if let Some(vt) = vt {
        vm.w32(this, vt);
    }
    h.phys.add_character(this, character, p);
    // rigid-body listeners reach the game character through `PhysicsUserData{type 4, ptr}` -> `+0x30` = Character*
    vm.w32(this + 0x30, character);
    let ud = vm.alloc_zeroed(0x10, 16);
    vm.w32(ud + 4, 4);
    vm.w32(ud + 0xc, this);
    if let Some(c) = h.phys.chars.get_mut(&this) {
        c.user_data = ud;
    }
    Ok(())
}
/// `PhysicsCharacterListener(this, PhysicsCharacter*)`: remember which listener belongs to the character.
fn char_listener_ct(h: &mut MgHost, vm: &mut V) -> R {
    let (this, ch) = (vm.a(0), vm.a(1));
    if let Some(vt) = vm.img.addr("__vt__24PhysicsCharacterListener") {
        vm.w32(this, vt);
    }
    vm.w32(this + 4, ch);
    vm.st.mem.w8(this + 8, 1);
    vm.w32(this + 0xc, 0);
    if let Some(c) = h.phys.chars.get_mut(&ch) {
        c.listener = this;
    }
    Ok(())
}

/// The virtual `<base>__...` method found in the vtable at `vt` (entries are `{s16 delta, s16 pad, ptr}`).
fn listener_method(vm: &mut V, vt: u32, base: &str) -> Option<u32> {
    let prefix = format!("{base}__");
    let words: Vec<u32> = (0..24u32).map(|i| vm.r32(vt + 4 * i)).collect();
    words.into_iter().find(|&w| w >= 0x8000_0000 && vm.img.func_at(w).is_some_and(|s| s.addr == w && s.name.starts_with(&prefix)))
}

/// Deliver the contacts that began during the last physics step to the guest listeners
/// (`ContactAddedCallback`/`ContactConfirmedCallback` on the body, `ObjectInteractionCallback` on the character).
pub fn dispatch_contacts(h: &mut MgHost, vm: &mut V) -> Result<(), String> {
    let contacts = std::mem::take(&mut h.pending_contacts);
    if std::env::var("EAGL_PHYS_TRACE").is_ok() {
        for c in contacts.iter().filter(|c| c.added) {
            eprintln!("[phys] contact {:#x} {:#x} bodies {:?} chars {:?}", c.a, c.b, h.phys.bodies.contains_key(&c.a), h.phys.chars.contains_key(&c.a));
        }
    }
    if contacts.iter().all(|c| !c.added) {
        return Ok(());
    }
    let cp = vm.alloc_zeroed(0x40, 16);
    for c in contacts.iter().filter(|c| c.added) {
        let (body, ch) = if h.phys.bodies.contains_key(&c.a) && h.phys.chars.contains_key(&c.b) {
            (c.a, c.b)
        } else if h.phys.bodies.contains_key(&c.b) && h.phys.chars.contains_key(&c.a) {
            (c.b, c.a)
        } else {
            continue;
        };
        let (b_ud, b_listener) = {
            let b = &h.phys.bodies[&body];
            (b.user_data, b.listener)
        };
        let (c_ud, c_listener) = {
            let c = &h.phys.chars[&ch];
            (c.user_data, c.listener)
        };
        for (i, v) in c.point.iter().chain(c.normal.iter()).enumerate() {
            vm.st.mem.wf32(cp + 4 * i as u32, *v);
        }
        if b_listener != 0 {
            let vt = vm.r32(b_listener);
            for slot_name in ["ContactAddedCallback", "ContactConfirmedCallback"] {
                if let Some(f) = listener_method(vm, vt, slot_name) {
                    vm.call(h, f, &[b_listener, c_ud, 3, cp], &[0.])?;
                }
            }
        }
        if c_listener != 0 {
            let vt = vm.r32(c_listener);
            if let Some(f) = listener_method(vm, vt, "ObjectInteractionCallback") {
                vm.call(h, f, &[c_listener, b_ud, cp], &[])?;
            }
        }
    }
    Ok(())
}
fn char_dt(h: &mut MgHost, vm: &mut V) -> R {
    h.phys.remove_character(vm.a(0));
    Ok(())
}
fn char_set_position(h: &mut MgHost, vm: &mut V) -> R {
    let p = read_v3(vm, vm.a(1));
    h.phys.set_character_position(vm.a(0), p);
    Ok(())
}
fn char_get_position(h: &mut MgHost, vm: &mut V) -> R {
    let (out, this) = (vm.a(0), vm.a(1));
    let p = h.phys.chars.get(&this).map(|c| c.position).unwrap_or([0.; 3]);
    write_v3(vm, out, p);
    Ok(())
}
fn char_set_destination(h: &mut MgHost, vm: &mut V) -> R {
    let p = read_v3(vm, vm.a(1));
    if std::env::var("EAGL_PHYS_TRACE").is_ok() {
        eprintln!("[phys] SetDestination {:#x} -> {p:?}", vm.a(0));
    }
    if let Some(c) = h.phys.chars.get_mut(&vm.a(0)) {
        c.destination = Some(p);
    }
    Ok(())
}
fn char_set_speed(h: &mut MgHost, vm: &mut V) -> R {
    let s = vm.fa(0);
    if std::env::var("EAGL_PHYS_TRACE").is_ok() && s != 0. {
        eprintln!("[phys] SetSpeed {:#x} = {s}", vm.a(0));
    }
    if let Some(c) = h.phys.chars.get_mut(&vm.a(0)) {
        c.speed = s;
        if s <= 0. {
            c.destination = None;
        }
    }
    Ok(())
}
fn char_set_orientation(h: &mut MgHost, vm: &mut V) -> R {
    let a = vm.fa(0);
    if let Some(c) = h.phys.chars.get_mut(&vm.a(0)) {
        c.orientation = a;
    }
    Ok(())
}

pub fn install(vm: &mut V) {
    bind(vm, &["LoadPhysics__14PhysicsManagerFPCcPC9rmMatrix4Uib"], load_physics);
    bind(vm, &["UnloadPhysics__14PhysicsManagerFib"], unload_physics);
    bind(vm, &["GenerateRigidBodyFromPhysicsSystem__14PhysicsManagerFiUii"], generate_body);
    bind(vm, &["DestroyPhysicsRigidBody__14PhysicsManagerFP16PhysicsRigidBody"], destroy_body);
    bind(
        vm,
        &["AddPhysicsRigidBodyToWorld__14PhysicsManagerFP16PhysicsRigidBody", "RemovePhysicsRigidBodyFromWorld__14PhysicsManagerFP16PhysicsRigidBody", "InitializeSim__14PhysicsManagerFPCcPC9rmMatrix4", "EnableCharacterCharacterCollisions__14PhysicsManagerFv", "DisableCharacterCharacterCollisions__14PhysicsManagerFv", "SetQualityType__16PhysicsRigidBodyF18PhysicsQualityType", "SetMotionType__16PhysicsRigidBodyF17PhysicsMotionType"],
        nothing,
    );
    bind(vm, &["GetGravity__14PhysicsManagerCFv"], get_gravity);
    bind(vm, &["GetGroundHeight__14PhysicsManagerCFPC9rmVector3f"], get_ground_height);
    bind(vm, &["GetGroundType__14PhysicsManagerCFPC9rmVector3f"], get_ground_type);
    bind(vm, &["CastRay__14PhysicsManagerCFPC9rmVector3PC9rmVector3UiPf"], cast_ray);
    bind(vm, &["Update__14PhysicsManagerFi"], update_physics);
    bind(vm, &["SetPos__16PhysicsRigidBodyFRC9rmVector3"], set_pos);
    bind(vm, &["SetMat__16PhysicsRigidBodyFRC9rmMatrix4"], set_mat);
    bind(vm, &["GetMat__16PhysicsRigidBodyCFv"], get_mat);
    bind(vm, &["GetPos__16PhysicsRigidBodyCFv"], get_pos);
    bind(vm, &["GetLinearVelocity__16PhysicsRigidBodyCFv"], get_linvel);
    bind(vm, &["GetAngularVelocity__16PhysicsRigidBodyCFv"], get_angvel);
    bind(vm, &["SetLinearVelocity__16PhysicsRigidBodyFRC9rmVector3"], set_linvel);
    bind(vm, &["SetAngularVelocity__16PhysicsRigidBodyFRC9rmVector3"], set_angvel);
    bind(vm, &["ApplyForce__16PhysicsRigidBodyFfRC9rmVector3"], apply_force);
    bind(vm, &["GetMass__16PhysicsRigidBodyFv"], get_mass);
    bind(vm, &["SetUserData__16PhysicsRigidBodyFPC15PhysicsUserData"], set_user_data);
    bind(vm, &["__ct__24PhysicsRigidBodyListenerFP16PhysicsRigidBody"], rb_listener_ct);
    bind(vm, &["__ct__24PhysicsCharacterListenerFP16PhysicsCharacter"], char_listener_ct);
    bind(vm, &["__ct__23PhysicsDynamicCharacterFP9CharacterPC9rmVector3P14PhysicsManager", "__ct__22PhysicsStaticCharacterFP9CharacterPC9rmVector3P14PhysicsManager"], char_ct);
    bind(vm, &["__dt__23PhysicsDynamicCharacterFv", "__dt__22PhysicsStaticCharacterFv"], char_dt);
    bind(vm, &["SetPosition__23PhysicsDynamicCharacterFPC9rmVector3", "SetPosition__22PhysicsStaticCharacterFPC9rmVector3"], char_set_position);
    bind(vm, &["GetPosition__23PhysicsDynamicCharacterCFv", "GetPosition__22PhysicsStaticCharacterCFv"], char_get_position);
    bind(vm, &["SetDestination__23PhysicsDynamicCharacterFPC9rmVector3", "SetDestination__22PhysicsStaticCharacterFPC9rmVector3"], char_set_destination);
    bind(vm, &["SetSpeed__23PhysicsDynamicCharacterFf", "SetSpeed__22PhysicsStaticCharacterFf"], char_set_speed);
    bind(vm, &["SetOrientation__23PhysicsDynamicCharacterFf", "SetOrientation__22PhysicsStaticCharacterFf"], char_set_orientation);
}
