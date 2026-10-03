//! Physics for the emulated games: a rapier3d world fed by the original Havok packfiles (`data/physics/*.hkx`).
//! The guest never sees rapier; `PhysicsManager` / `PhysicsRigidBody` / character hooks call into this.
use crate::havok::{self, BodyInfo, Prim};
use rapier3d::prelude::*;
use std::collections::HashMap;

/// Row-vector 4x4 matrix as the engine stores it (`rmMatrix4`): rows are the x/y/z axes and the translation.
pub type Mat = [f32; 16];

pub fn mat_identity() -> Mat {
    [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
}

fn quat_from_rows(m: &Mat) -> Rotation {
    // basis vectors are the rows; as a column-vector rotation matrix they are the columns
    let (xx, yx, zx) = (m[0], m[4], m[8]);
    let (xy, yy, zy) = (m[1], m[5], m[9]);
    let (xz, yz, zz) = (m[2], m[6], m[10]);
    let tr = xx + yy + zz;
    let (x, y, z, w);
    if tr > 0. {
        let s = (tr + 1.).sqrt() * 2.;
        w = 0.25 * s;
        x = (yz - zy) / s;
        y = (zx - xz) / s;
        z = (xy - yx) / s;
    } else if xx > yy && xx > zz {
        let s = (1. + xx - yy - zz).sqrt() * 2.;
        w = (yz - zy) / s;
        x = 0.25 * s;
        y = (xy + yx) / s;
        z = (xz + zx) / s;
    } else if yy > zz {
        let s = (1. + yy - xx - zz).sqrt() * 2.;
        w = (zx - xz) / s;
        x = (xy + yx) / s;
        y = 0.25 * s;
        z = (yz + zy) / s;
    } else {
        let s = (1. + zz - xx - yy).sqrt() * 2.;
        w = (xy - yx) / s;
        x = (xz + zx) / s;
        y = (yz + zy) / s;
        z = 0.25 * s;
    }
    Rotation::from_xyzw(x, y, z, w).normalize()
}

fn mat_from_pose(rot: Rotation, t: Vector) -> Mat {
    let (x, y, z, w) = (rot.x, rot.y, rot.z, rot.w);
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    // column-vector rotation matrix R; engine rows are R's columns
    let r = [[1. - 2. * (yy + zz), 2. * (xy - wz), 2. * (xz + wy)], [2. * (xy + wz), 1. - 2. * (xx + zz), 2. * (yz - wx)], [2. * (xz - wy), 2. * (yz + wx), 1. - 2. * (xx + yy)]];
    [r[0][0], r[1][0], r[2][0], 0., r[0][1], r[1][1], r[2][1], 0., r[0][2], r[1][2], r[2][2], 0., t.x, t.y, t.z, 1.]
}

fn apply(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    [p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12], p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13], p[0] * m[2] + p[1] * m[6] + p[2] * m[10] + m[14]]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(pub u32);

pub struct DynBody {
    pub handle: RigidBodyHandle,
    pub system: usize,
    pub user_data: u32,
    pub listener: u32,
    pub in_world: bool,
}

pub struct PhantomBody {
    pub handle: RigidBodyHandle,
    pub listener: u32,
}

pub struct CharBody {
    pub handle: RigidBodyHandle,
    pub position: [f32; 3],
    pub destination: Option<[f32; 3]>,
    pub speed: f32,
    pub orientation: f32,
    pub listener: u32,
    pub guest_character: u32,
    /// Guest `PhysicsUserData` (type 4) describing this character to rigid-body listeners.
    pub user_data: u32,
    /// Half the capsule height: the body centre sits this far above the feet.
    pub mid: f32,
}

/// A `PhysicsVehicle`: RcCars computes the driving model itself (SetVel/SetDir every frame), so this only integrates,
/// keeps the car on the ground and answers pose queries.
#[derive(Clone, Debug)]
pub struct Vehicle {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    /// Forward axis (the car's local +X).
    pub dir: [f32; 3],
    pub angvel: [f32; 3],
    pub fixed: bool,
    pub grounded: bool,
}

pub const WHEEL_OFFSETS: [[f32; 3]; 4] = [[0.22, -0.06, 0.13], [0.22, -0.06, -0.13], [-0.22, -0.06, 0.13], [-0.22, -0.06, -0.13]];
pub const VEHICLE_REST: f32 = 0.12;

impl Vehicle {
    /// Body matrix: rows are forward, up, side axes (engine row-vector convention).
    pub fn mat(&self) -> Mat {
        let d = self.dir;
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
        let x = [d[0] / l, d[1] / l, d[2] / l];
        let mut y = [0., 1., 0.];
        // keep the up vector perpendicular to forward
        let dot = x[1];
        y = [y[0] - x[0] * dot, y[1] - x[1] * dot, y[2] - x[2] * dot];
        let yl = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt().max(1e-6);
        y = [y[0] / yl, y[1] / yl, y[2] / yl];
        let z = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
        [x[0], x[1], x[2], 0., y[0], y[1], y[2], 0., z[0], z[1], z[2], 0., self.pos[0], self.pos[1], self.pos[2], 1.]
    }
    pub fn wheel_pos(&self, i: usize) -> [f32; 3] {
        let m = self.mat();
        apply(&m, WHEEL_OFFSETS[i % 4])
    }
}

/// A contact that began (or ended) this step, for the guest listeners.
#[derive(Clone, Debug)]
pub struct Contact {
    pub a: u32,
    pub b: u32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub added: bool,
}

pub struct Physics {
    pub world: PhysicsWorld,
    pub gravity: [f32; 3],
    systems: Vec<Option<Vec<BodyInfo>>>,
    /// Static colliders added by each loaded system.
    system_bodies: Vec<Vec<RigidBodyHandle>>,
    /// Collision filter info each system was loaded with (`LoadPhysics`' filter argument).
    system_filters: Vec<u32>,
    pub bodies: HashMap<u32, DynBody>,
    pub chars: HashMap<u32, CharBody>,
    pub vehicles: HashMap<u32, Vehicle>,
    /// `PhysicsPhantom`s: overlap volumes (football goals, paper-airplane obstacles) reporting to a phantom listener.
    pub phantoms: HashMap<u32, PhantomBody>,
    by_handle: HashMap<RigidBodyHandle, u32>,
    prev_pairs: std::collections::HashSet<(u32, u32)>,
    pub log: Vec<String>,
}

fn tri_mesh(tris: &[[[f32; 3]; 3]]) -> Option<ColliderBuilder> {
    if tris.is_empty() {
        return None;
    }
    let mut verts = Vec::with_capacity(tris.len() * 3);
    let mut idx = Vec::with_capacity(tris.len());
    for (i, t) in tris.iter().enumerate() {
        for p in t {
            verts.push(Vector::new(p[0], p[1], p[2]));
        }
        idx.push([3 * i as u32, 3 * i as u32 + 1, 3 * i as u32 + 2]);
    }
    ColliderBuilder::trimesh(verts, idx).ok()
}

/// Havok collision layer of the characters (the `Character_*` walls of layer 18 only stop layer 3).
pub const CHARACTER_LAYER: u32 = 3;

/// The layer matrix `PhysicsManager::CreateCollisionFilter` (0x803b8d44) builds into the hkGroupFilter: row `layer` is the
/// set of layers it collides with.
pub fn layer_mask(layer: u32) -> u32 {
    let mut m = [u32::MAX; 32];
    let enable_bits = |m: &mut [u32; 32], a: u32, b: u32| {
        for i in 0..32 {
            if a >> i & 1 != 0 {
                m[i] |= b;
            }
            if b >> i & 1 != 0 {
                m[i] |= a;
            }
        }
    };
    let disable_bits = |m: &mut [u32; 32], a: u32, b: u32| {
        for i in 0..32 {
            if a >> i & 1 != 0 {
                m[i] &= !b;
            }
            if b >> i & 1 != 0 {
                m[i] &= !a;
            }
        }
    };
    enable_bits(&mut m, 0xffff_fffe, 0xffff_fffe);
    for a in [0x10000, 0x20000, 0x40000, 0x200000, 0x10, 0x80000, 0x20, 0x100000, 0x8000, 0x100, 0x400000] {
        disable_bits(&mut m, a, 0xffff_fffe);
    }
    for (a, b) in [(0x10, 1), (0x11, 2), (0x12, 3), (0x15, 6), (4, 5), (4, 1), (4, 2), (0x13, 4), (5, 4), (5, 1), (5, 2), (0x14, 5), (0x14, 1), (0x14, 2), (0xf, 2), (0xf, 1), (8, 8), (0x16, 2), (0x16, 1), (0x16, 0x16)] {
        enable_bits(&mut m, 1 << a, 1 << b);
    }
    disable_bits(&mut m, 1 << 6, 1 << 3);
    m[(layer & 0x1f) as usize]
}

pub fn layer_groups(layer: u32) -> InteractionGroups {
    InteractionGroups::new(Group::from_bits_retain(1 << (layer & 0x1f)), Group::from_bits_retain(layer_mask(layer)), InteractionTestMode::And)
}

/// Collider user data marking terrain meshes (the ground probe only looks at those).
const MESH_TAG: u128 = 1;

fn collider_for(prim: &Prim, info: &BodyInfo, layer: u32) -> Option<ColliderBuilder> {
    let b = match prim {
        Prim::Sphere { c, r } => ColliderBuilder::ball(*r).translation(Vector::new(c[0], c[1], c[2])),
        Prim::Capsule { a, b, r } => ColliderBuilder::capsule_from_endpoints(Vector::new(a[0], a[1], a[2]), Vector::new(b[0], b[1], b[2]), *r),
        Prim::Hull(pts) => ColliderBuilder::convex_hull(&pts.iter().map(|p| Vector::new(p[0], p[1], p[2])).collect::<Vec<_>>())?,
        Prim::Mesh(tris) => tri_mesh(tris)?,
    };
    let tag = if matches!(prim, Prim::Mesh(_)) { MESH_TAG } else { 0 };
    Some(b.friction(info.friction).restitution(info.restitution).user_data(tag).collision_groups(layer_groups(layer)))
}

impl Physics {
    pub fn new() -> Self {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0., -9.81, 0.);
        Physics {
            world,
            gravity: [0., -9.81, 0.],
            systems: vec![],
            system_bodies: vec![],
            system_filters: vec![],
            bodies: HashMap::new(),
            chars: HashMap::new(),
            vehicles: HashMap::new(),
            phantoms: HashMap::new(),
            by_handle: HashMap::new(),
            prev_pairs: Default::default(),
            log: vec![],
        }
    }

    /// `PhysicsManager::LoadPhysics`: parse `data/physics/<name>.hkx`; with `add_to_world` its bodies exist immediately.
    /// `PhysicsManager::InitializeSim(name, matrix)`: the Havok world is built from `name`'s `hkWorldCinfo`; its gravity
    /// differs per game (Paper Airplanes' `pa_world` has none).
    pub fn initialize_sim(&mut self, name: &str) {
        let path = crate::bridge::data_root().join("files/data/physics").join(format!("{}.hkx", name.trim_end_matches(".hkx")));
        let Ok(bytes) = std::fs::read(&path) else { return };
        let ct = havok::ClassTable::embedded();
        let Ok(pf) = havok::Packfile::parse(&bytes, &ct) else { return };
        for (&(si, off), cn) in &pf.virt {
            if cn == "hkWorldCinfo" {
                let o = pf.decode_object(si, off, cn, 0);
                if let Some(g) = o["gravity"].as_array() {
                    let v = |i: usize| g.get(i).and_then(|x| x.as_f64()).unwrap_or(0.) as f32;
                    self.gravity = [v(0), v(1), v(2)];
                    self.world.gravity = Vector::new(v(0), v(1), v(2));
                    self.log.push(format!("InitializeSim {name}: gravity {:?}", self.gravity));
                }
            }
        }
        // the world's own static bodies: the school area of `playground` (its hoop, floor), Paper Airplanes' floor and walls
        self.load_system(name, None, true, 0);
    }

    pub fn load_system(&mut self, name: &str, matrix: Option<Mat>, add_to_world: bool, filter: u32) -> i32 {
        // layer 8 (camera collision) only ever meets itself; its bodies are not simulated here
        let camera_only = filter & 0x1f == 8;
        let path = crate::bridge::data_root().join("files/data/physics").join(format!("{}.hkx", name.trim_end_matches(".hkx")));
        let Ok(bytes) = std::fs::read(&path) else {
            self.log.push(format!("LoadPhysics: missing {}", path.display()));
            return -1;
        };
        let ct = havok::ClassTable::embedded();
        let Ok(pf) = havok::Packfile::parse(&bytes, &ct) else {
            self.log.push(format!("LoadPhysics: cannot parse {name}"));
            return -1;
        };
        let infos = havok::rigid_bodies(&pf);
        self.log.push(format!("LoadPhysics {name}: {} bodies, matrix {:?}, add {add_to_world}", infos.len(), matrix));
        if std::env::var("EAGL_PHYS_FILTER").is_ok() && (infos.len() < 40 || std::env::var("EAGL_PHYS_ALL").is_ok()) {
            for i in &infos {
                self.log.push(format!("    body {} filter {:#x} layer {} inv_mass {}", i.name, i.filter, i.filter & 0x1f, i.mass_inv));
            }
        }
        let id = self.systems.len();
        self.systems.push(Some(infos));
        self.system_bodies.push(vec![]);
        self.system_filters.push(filter);
        if add_to_world && !camera_only {
            let n = self.systems[id].as_ref().map(|v| v.len()).unwrap_or(0);
            for i in 0..n {
                let infos = self.systems[id].as_ref().unwrap();
                if infos[i].mass_inv == 0. {
                    let h = self.insert_body(id, i, matrix.as_ref(), None, filter);
                    self.system_bodies[id].push(h);
                }
            }
        }
        id as i32
    }

    pub fn unload_system(&mut self, id: usize) {
        if let Some(hs) = self.system_bodies.get_mut(id) {
            for h in hs.drain(..) {
                self.world.remove_body(h);
            }
        }
        if let Some(s) = self.systems.get_mut(id) {
            *s = None;
        }
    }

    fn insert_body(&mut self, system: usize, index: usize, matrix: Option<&Mat>, pose: Option<Mat>, filter: u32) -> RigidBodyHandle {
        let layer = if filter != 0 { filter } else { self.systems[system].as_ref().unwrap()[index].filter } & 0x1f;
        let info = self.systems[system].as_ref().unwrap()[index].clone();
        let local = {
            let r = info.rotation;
            // havok rotation is row-major with column vectors -> engine rows are its columns
            [r[0][0], r[1][0], r[2][0], 0., r[0][1], r[1][1], r[2][1], 0., r[0][2], r[1][2], r[2][2], 0., info.translation[0], info.translation[1], info.translation[2], 1.]
        };
        let m = pose.or_else(|| matrix.map(|w| mul_mat(&local, w))).unwrap_or(local);
        let rot = quat_from_rows(&m);
        let builder = if info.mass_inv == 0. { RigidBodyBuilder::fixed() } else { RigidBodyBuilder::dynamic() };
        let body = builder.pose(Pose::from_parts(Vector::new(m[12], m[13], m[14]), rot)).linear_damping(info.linear_damping).angular_damping(info.angular_damping).ccd_enabled(info.mass_inv != 0.);
        let h = self.world.insert_body(body);
        let mass = if info.mass_inv > 0. { 1. / info.mass_inv } else { 0. };
        let total_prims = info.prims.len().max(1) as f32;
        for prim in &info.prims {
            if let Some(mut c) = collider_for(prim, &info, layer) {
                if mass > 0. {
                    c = c.mass(mass / total_prims);
                }
                self.world.insert_collider(c, Some(h));
            }
        }
        h
    }

    /// `GenerateRigidBodyFromPhysicsSystem`: instantiate dynamic body `index` of a loaded system.
    pub fn generate_body(&mut self, guest: u32, system: usize, index: usize, filter: u32) -> bool {
        let count = self.systems.get(system).and_then(|s| s.as_ref()).map(|s| s.len()).unwrap_or(0);
        if count == 0 {
            return false;
        }
        let dynamic: Vec<usize> = self.systems[system].as_ref().unwrap().iter().enumerate().filter(|(_, b)| b.mass_inv > 0.).map(|(i, _)| i).collect();
        let pick = dynamic.get(index).copied().or_else(|| dynamic.first().copied()).unwrap_or(index.min(count - 1));
        let filter = if filter != 0 { filter } else { self.system_filters.get(system).copied().unwrap_or(0) };
        let h = self.insert_body(system, pick, None, None, filter);
        if std::env::var("EAGL_PHYS_FILTER").is_ok() {
            let i = &self.systems[system].as_ref().unwrap()[pick];
            self.log.push(format!("generate {guest:#x} {} mass_inv {} motion {}", i.name, i.mass_inv, i.motion));
        }
        self.by_handle.insert(h, guest);
        self.bodies.insert(guest, DynBody { handle: h, system, user_data: 0, listener: 0, in_world: true });
        true
    }

    /// `GeneratePhantomFromPhysicsSystem(system, listener, filter, index)`: the shape of body `index` as a sensor volume
    /// (placed later with `SetMat` / `SetPos`), on the given collision layer (0 = the body's own).
    pub fn add_phantom(&mut self, guest: u32, system: usize, index: usize, filter: u32, listener: u32) -> bool {
        let Some(infos) = self.systems.get(system).and_then(|s| s.as_ref()) else { return false };
        let Some(info) = infos.get(index.min(infos.len().saturating_sub(1))).cloned() else { return false };
        let layer = if filter != 0 { filter } else if info.filter != 0 { info.filter } else { self.system_filters.get(system).copied().unwrap_or(0) } & 0x1f;
        let h = self.world.insert_body(RigidBodyBuilder::kinematic_position_based());
        for prim in &info.prims {
            if let Some(c) = collider_for(prim, &info, layer) {
                self.world.insert_collider(c.sensor(true), Some(h));
            }
        }
        if std::env::var("EAGL_PHYS_FILTER").is_ok() {
            self.log.push(format!("phantom {guest:#x} from {} layer {layer} prims {:?}", info.name, info.prims.iter().map(|p| match p { Prim::Hull(v) => format!("hull {v:?}"), Prim::Mesh(t) => format!("mesh {}", t.len()), Prim::Sphere { c, r } => format!("sphere {c:?} {r}"), Prim::Capsule { .. } => "capsule".into() }).collect::<Vec<_>>()));
        }
        self.by_handle.insert(h, guest);
        self.phantoms.insert(guest, PhantomBody { handle: h, listener });
        true
    }

    pub fn set_phantom_mat(&mut self, guest: u32, m: &Mat) {
        if std::env::var("EAGL_PHYS_FILTER").is_ok() {
            self.log.push(format!("phantom {guest:#x} SetMat {m:?}"));
        }
        if let Some(p) = self.phantoms.get(&guest) {
            let rb = &mut self.world.bodies[p.handle];
            rb.set_translation(Vector::new(m[12], m[13], m[14]), true);
            rb.set_rotation(quat_from_rows(m), true);
        }
    }

    pub fn set_phantom_pos(&mut self, guest: u32, p: [f32; 3]) {
        if let Some(ph) = self.phantoms.get(&guest) {
            self.world.bodies[ph.handle].set_translation(Vector::new(p[0], p[1], p[2]), true);
        }
    }

    pub fn phantom_pos(&self, guest: u32) -> [f32; 3] {
        self.phantoms.get(&guest).map(|p| self.world.bodies[p.handle].translation()).map(|t| [t.x, t.y, t.z]).unwrap_or([0.; 3])
    }

    pub fn set_phantom_enabled(&mut self, guest: u32, on: bool) {
        if let Some(p) = self.phantoms.get(&guest) {
            self.world.bodies[p.handle].set_enabled(on);
        }
    }

    pub fn remove_phantom(&mut self, guest: u32) {
        if let Some(p) = self.phantoms.remove(&guest) {
            self.by_handle.remove(&p.handle);
            self.world.remove_body(p.handle);
        }
    }

    /// `Add/RemovePhysicsRigidBodyToWorld`: a removed body keeps its pose but is neither simulated nor collides.
    pub fn set_in_world(&mut self, guest: u32, on: bool) {
        if let Some(b) = self.bodies.get_mut(&guest) {
            b.in_world = on;
            let rb = &mut self.world.bodies[b.handle];
            rb.set_enabled(on);
            if on {
                rb.wake_up(true);
            }
        }
    }

    pub fn destroy_body(&mut self, guest: u32) {
        if let Some(b) = self.bodies.remove(&guest) {
            self.by_handle.remove(&b.handle);
            self.world.remove_body(b.handle);
        }
    }

    pub fn body_mat(&self, guest: u32) -> Option<Mat> {
        let b = self.bodies.get(&guest)?;
        let rb = &self.world.bodies[b.handle];
        Some(mat_from_pose(*rb.rotation(), rb.translation()))
    }

    pub fn set_body_mat(&mut self, guest: u32, m: &Mat) {
        if let Some(b) = self.bodies.get(&guest) {
            let rb = &mut self.world.bodies[b.handle];
            rb.set_translation(Vector::new(m[12], m[13], m[14]), true);
            rb.set_rotation(quat_from_rows(m), true);
        }
    }

    pub fn set_body_pos(&mut self, guest: u32, p: [f32; 3]) {
        if let Some(b) = self.bodies.get(&guest) {
            let rb = &mut self.world.bodies[b.handle];
            rb.set_translation(Vector::new(p[0], p[1], p[2]), true);
        }
    }

    pub fn body_pos(&self, guest: u32) -> [f32; 3] {
        self.bodies.get(&guest).map(|b| self.world.bodies[b.handle].translation()).map(|t| [t.x, t.y, t.z]).unwrap_or([0.; 3])
    }

    pub fn set_linvel(&mut self, guest: u32, v: [f32; 3]) {
        if let Some(b) = self.bodies.get(&guest) {
            self.world.bodies[b.handle].set_linvel(Vector::new(v[0], v[1], v[2]), true);
        }
    }
    pub fn linvel(&self, guest: u32) -> [f32; 3] {
        self.bodies.get(&guest).map(|b| self.world.bodies[b.handle].linvel()).map(|v| [v.x, v.y, v.z]).unwrap_or([0.; 3])
    }
    pub fn set_angvel(&mut self, guest: u32, v: [f32; 3]) {
        if let Some(b) = self.bodies.get(&guest) {
            self.world.bodies[b.handle].set_angvel(Vector::new(v[0], v[1], v[2]), true);
        }
    }
    pub fn angvel(&self, guest: u32) -> [f32; 3] {
        self.bodies.get(&guest).map(|b| self.world.bodies[b.handle].angvel()).map(|v| [v.x, v.y, v.z]).unwrap_or([0.; 3])
    }
    pub fn apply_force(&mut self, guest: u32, scale: f32, dir: [f32; 3]) {
        if let Some(b) = self.bodies.get(&guest) {
            let rb = &mut self.world.bodies[b.handle];
            rb.apply_impulse(Vector::new(dir[0] * scale, dir[1] * scale, dir[2] * scale), true);
        }
    }
    pub fn mass(&self, guest: u32) -> f32 {
        self.bodies.get(&guest).map(|b| self.world.bodies[b.handle].mass()).unwrap_or(1.)
    }

    /// Ray cast against the world: distance to the first hit.
    pub fn cast_ray(&self, from: [f32; 3], dir: [f32; 3], max: f32) -> Option<f32> {
        let ray = Ray::new(Vector::new(from[0], from[1], from[2]), Vector::new(dir[0], dir[1], dir[2]));
        self.world.cast_ray(&ray, max, true, QueryFilter::only_fixed()).map(|(_, t)| t)
    }

    /// Ray against the terrain meshes only (walls and props are separate colliders).
    pub fn cast_ground_ray(&self, from: [f32; 3], max: f32) -> Option<f32> {
        let ray = Ray::new(Vector::new(from[0], from[1], from[2]), Vector::new(0., -1., 0.));
        let is_mesh = |_h, c: &Collider| c.user_data == MESH_TAG;
        let filter = QueryFilter::only_fixed().predicate(&is_mesh);
        self.world.cast_ray(&ray, max, true, filter).map(|(_, t)| t)
    }

    /// Height of the ground below `pos` (searching `up` above and `down` below).
    pub fn ground_height(&self, pos: [f32; 3], up: f32, down: f32) -> Option<f32> {
        let from = [pos[0], pos[1] + up, pos[2]];
        self.cast_ground_ray(from, up + down).map(|t| from[1] - t)
    }

    // --- characters ---------------------------------------------------------------------------------------------------

    /// `PhysicsDynamicCharacter`: a capsule of the character's radius (`Character+0x140`) and height (`+0x144`) standing on
    /// `pos`, on the character layer (`CalculateFilterInfo(3)`).
    pub fn add_character(&mut self, guest: u32, character: u32, pos: [f32; 3], radius: f32, height: f32) {
        let radius = if radius > 0.05 && radius < 2. { radius } else { 0.3 };
        let height = if height > radius * 2. && height < 5. { height } else { 1.5 };
        let mid = height * 0.5;
        let body = RigidBodyBuilder::kinematic_position_based().translation(Vector::new(pos[0], pos[1] + mid, pos[2]));
        let h = self.world.insert_body(body);
        // a Havok character proxy does not push dynamic bodies around, it only reports them (ObjectInteractionCallback)
        self.world.insert_collider(ColliderBuilder::capsule_y((mid - radius).max(0.01), radius).sensor(true).collision_groups(layer_groups(CHARACTER_LAYER)), Some(h));
        self.by_handle.insert(h, guest);
        self.chars.insert(guest, CharBody { handle: h, position: pos, destination: None, speed: 0., orientation: 0., listener: 0, guest_character: character, user_data: 0, mid });
    }

    pub fn remove_character(&mut self, guest: u32) {
        if let Some(c) = self.chars.remove(&guest) {
            self.by_handle.remove(&c.handle);
            self.world.remove_body(c.handle);
        }
    }

    pub fn set_character_position(&mut self, guest: u32, p: [f32; 3]) {
        if let Some(c) = self.chars.get_mut(&guest) {
            c.position = p;
            c.destination = None;
        }
    }

    /// Move a character's capsule-ish probe horizontally by `delta`, sliding along static obstacles.
    pub fn slide(&self, pos: [f32; 3], delta: [f32; 2]) -> [f32; 3] {
        use rapier3d::parry::shape::Ball;
        let ball = Ball::new(0.28);
        let mut p = Vector::new(pos[0], pos[1] + 0.62, pos[2]);
        let mut rest = Vector::new(delta[0], 0., delta[1]);
        for _ in 0..3 {
            if rest.length() < 1e-5 {
                break;
            }
            let pose = Pose::from_parts(p, Rotation::IDENTITY);
            let mut opts = rapier3d::parry::query::ShapeCastOptions::with_max_time_of_impact(1.0);
            opts.stop_at_penetration = false;
            let filter = QueryFilter::only_fixed().groups(layer_groups(CHARACTER_LAYER));
            match self.world.cast_shape(&pose, rest, &ball, opts, filter) {
                Some((col, hit)) => {
                    if std::env::var("EAGL_PHYS_MOVE").is_ok() {
                        let c = &self.world.colliders[col];
                        eprintln!("[hit] toi {} n {:?} aabb {:?} parent {:?} fixed {}", hit.time_of_impact, hit.normal2, c.compute_aabb(), c.parent(), c.parent().map(|b| self.world.bodies[b].is_fixed()).unwrap_or(true));
                    }
                    let t = (hit.time_of_impact - 0.02).max(0.);
                    p += rest * t;
                    let n = hit.normal2;
                    let remaining = rest * (1. - t);
                    let into = remaining.dot(n);
                    rest = remaining - n * into;
                    rest.y = 0.;
                }
                None => {
                    p += rest;
                    break;
                }
            }
        }
        [p.x, pos[1], p.z]
    }

    // --- vehicles -----------------------------------------------------------------------------------------------------

    pub fn add_vehicle(&mut self, guest: u32, pos: [f32; 3]) {
        self.vehicles.insert(guest, Vehicle { pos, vel: [0.; 3], dir: [1., 0., 0.], angvel: [0.; 3], fixed: false, grounded: false });
    }

    pub fn step_vehicles(&mut self, guest: u32, ms: i32) {
        let dt = (ms.max(0) as f32 / 1000.).min(0.1);
        let Some(mut v) = self.vehicles.get(&guest).cloned() else { return };
        if !v.fixed && dt > 0. {
            // yaw by the angular velocity
            let a = v.angvel[1] * dt;
            if a != 0. {
                let (s, c) = a.sin_cos();
                v.dir = [v.dir[0] * c + v.dir[2] * s, v.dir[1], -v.dir[0] * s + v.dir[2] * c];
            }
            let delta = [v.vel[0] * dt, v.vel[2] * dt];
            let p = self.slide([v.pos[0], v.pos[1] - VEHICLE_REST, v.pos[2]], delta);
            v.pos = [p[0], v.pos[1], p[2]];
            v.pos[1] += v.vel[1] * dt;
            let ground = self.ground_height(v.pos, 0.5, 400.0).map(|g| g + VEHICLE_REST);
            match ground {
                Some(g) if v.pos[1] <= g + 0.02 => {
                    v.pos[1] = g;
                    v.grounded = true;
                    if v.vel[1] < 0. {
                        v.vel[1] = 0.;
                    }
                }
                _ => {
                    v.grounded = false;
                    v.vel[1] += self.gravity[1] * dt;
                }
            }
        }
        self.vehicles.insert(guest, v);
    }

    // --- stepping -----------------------------------------------------------------------------------------------------

    /// Advance the simulation by `ms`; returns the contacts that began since the previous step.
    pub fn step(&mut self, ms: i32) -> Vec<Contact> {
        let dt = (ms.max(0) as f32 / 1000.).min(0.1);
        if dt <= 0. {
            return vec![];
        }
        // characters walk toward their destination and stay on the ground
        let ids: Vec<u32> = self.chars.keys().copied().collect();
        for id in ids {
            let (pos, dest, speed, orient) = {
                let c = &self.chars[&id];
                (c.position, c.destination, c.speed, c.orientation)
            };
            let mut p = pos;
            let mut delta = [0f32; 2];
            if let Some(d) = dest {
                let (dx, dz) = (d[0] - p[0], d[2] - p[2]);
                let len = (dx * dx + dz * dz).sqrt();
                if len > 1e-4 && speed > 0. {
                    let step = (speed * dt).min(len);
                    delta = [dx / len * step, dz / len * step];
                }
            } else if speed > 0. {
                // no waypoint: walk along the heading (`rmAngle::AsDir` = (sin, cos))
                delta = [orient.sin() * speed * dt, orient.cos() * speed * dt];
            }
            if delta != [0.; 2] {
                p = self.slide(p, delta);
                if std::env::var("EAGL_PHYS_MOVE").is_ok() {
                    eprintln!("[move] {id:#x} {pos:?} delta {delta:?} -> {p:?} orient {orient} dest {dest:?}");
                }
            }
            if let Some(g) = self.ground_height(p, 1.0, 3.0) {
                p[1] = g;
            }
            let c = self.chars.get_mut(&id).unwrap();
            c.position = p;
            let mid = c.mid;
            let rb = &mut self.world.bodies[c.handle];
            rb.set_next_kinematic_translation(Vector::new(p[0], p[1] + mid, p[2]));
        }
        self.world.gravity = Vector::new(self.gravity[0], self.gravity[1], self.gravity[2]);
        self.world.integration_parameters.dt = dt;
        let before: Vec<(u32, Vector)> = self.bodies.iter().filter(|(_, b)| b.in_world).map(|(g, b)| (*g, self.world.bodies[b.handle].translation())).collect();
        self.world.step();
        if std::env::var("EAGL_DBG_BALL").is_ok() {
            for (g, b) in &self.bodies {
                let rb = &self.world.bodies[b.handle];
                let (t, v) = (rb.translation(), rb.linvel());
                if v.length() > 3. {
                    let near = self.chars.values().map(|c| { let dx = c.position[0] - t.x; let dz = c.position[2] - t.z; ((dx * dx + dz * dz).sqrt(), c.position[1]) }).fold((f32::MAX, 0.), |a, b| if b.0 < a.0 { b } else { a });
                    eprintln!("[ball] {g:#x} pos ({:.2},{:.2},{:.2}) v {:.1} near char xz {:.2} (ground {:.2})", t.x, t.y, t.z, v.length(), near.0, near.1);
                }
            }
        }
        // contacts between bodies the guest knows about
        let mut now = std::collections::HashSet::new();
        let mut out = vec![];
        for pair in self.world.narrow_phase.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            let (Some(c1), Some(c2)) = (self.world.colliders.get(pair.collider1), self.world.colliders.get(pair.collider2)) else { continue };
            let (Some(b1), Some(b2)) = (c1.parent(), c2.parent()) else { continue };
            // a body the guest does not know is the static world (guest id 0)
            let g1 = self.by_handle.get(&b1).copied().unwrap_or(0);
            let g2 = self.by_handle.get(&b2).copied().unwrap_or(0);
            if g1 == 0 && g2 == 0 {
                continue;
            }
            let key = (g1.min(g2), g1.max(g2));
            now.insert(key);
            if !self.prev_pairs.contains(&key) {
                let (point, normal) = pair.manifolds().first().map(|m| {
                    let p = m.points.first().map(|p| c1.position() * p.local_p1).unwrap_or_default();
                    ([p.x, p.y, p.z], [m.data.normal.x, m.data.normal.y, m.data.normal.z])
                }).unwrap_or(([0.; 3], [0., 1., 0.]));
                out.push(Contact { a: g1, b: g2, point, normal, added: true });
            }
        }
        // characters are sensors: their overlaps with bodies come from the intersection graph
        let mut overlaps = vec![];
        for (_, c1, _, c2, hit) in self.world.intersection_pairs() {
            if !hit {
                continue;
            }
            let (Some(b1), Some(b2)) = (c1.parent(), c2.parent()) else { continue };
            let (Some(&g1), Some(&g2)) = (self.by_handle.get(&b1), self.by_handle.get(&b2)) else { continue };
            // report the point on the body (the non-character / non-phantom side)
            let p = if self.chars.contains_key(&g1) || self.phantoms.contains_key(&g1) { c2.position().translation } else { c1.position().translation };
            overlaps.push((g1, g2, [p.x, p.y, p.z]));
        }
        // sensors get no CCD: sweep fast bodies against the character capsules so a thrown ball cannot step over one
        let char_bodies: std::collections::HashMap<RigidBodyHandle, u32> = self.chars.iter().map(|(g, c)| (c.handle, *g)).chain(self.phantoms.iter().filter(|(_, p)| self.world.bodies[p.handle].is_enabled()).map(|(g, p)| (p.handle, *g))).collect();
        for (g, p0) in before {
            let Some(b) = self.bodies.get(&g) else { continue };
            let rb = &self.world.bodies[b.handle];
            let delta = rb.translation() - p0;
            if delta.length() < 0.1 {
                continue;
            }
            let Some(col) = rb.colliders().first().and_then(|h| self.world.colliders.get(*h)) else { continue };
            let radius = col.shape().compute_local_bounding_sphere().radius();
            let groups = col.collision_groups();
            let is_char = |_h, c: &Collider| c.parent().is_some_and(|p| char_bodies.contains_key(&p));
            let filter = QueryFilter::default().groups(groups).predicate(&is_char);
            let mut opts = rapier3d::parry::query::ShapeCastOptions::with_max_time_of_impact(1.0);
            // a ball leaving the thrower's hand starts inside that capsule: only report what it runs into
            opts.stop_at_penetration = false;
            let ball = rapier3d::parry::shape::Ball::new(radius);
            if let Some((hit, _)) = self.world.cast_shape(&Pose::from_parts(p0, Rotation::IDENTITY), delta, &ball, opts, filter) {
                if let Some(&ch) = self.world.colliders[hit].parent().and_then(|p| char_bodies.get(&p)) {
                    let p = p0 + delta * 0.5;
                    overlaps.push((g, ch, [p.x, p.y, p.z]));
                }
            }
        }
        for (g1, g2, point) in overlaps {
            let key = (g1.min(g2), g1.max(g2));
            now.insert(key);
            if !self.prev_pairs.contains(&key) {
                out.push(Contact { a: g1, b: g2, point, normal: [0., 1., 0.], added: true });
            }
        }
        for k in &self.prev_pairs {
            if !now.contains(k) {
                out.push(Contact { a: k.0, b: k.1, point: [0.; 3], normal: [0., 1., 0.], added: false });
            }
        }
        self.prev_pairs = now;
        out
    }
}

fn mul_mat(a: &Mat, b: &Mat) -> Mat {
    let mut r = [0.; 16];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                r[i * 4 + j] += a[i * 4 + k] * b[k * 4 + j];
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_round_trip() {
        let rot = Rotation::from_xyzw(0.1, 0.5, -0.3, 0.8).normalize();
        let m = mat_from_pose(rot, Vector::new(1., 2., 3.));
        let back = quat_from_rows(&m);
        assert!((back.dot(rot).abs() - 1.).abs() < 1e-5);
        assert_eq!(apply(&mat_identity(), [1., 2., 3.]), [1., 2., 3.]);
    }
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}
