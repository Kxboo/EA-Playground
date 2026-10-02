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
        x = (zy - yz) / s;
        y = (xz - zx) / s;
        z = (yx - xy) / s;
    } else if xx > yy && xx > zz {
        let s = (1. + xx - yy - zz).sqrt() * 2.;
        w = (zy - yz) / s;
        x = 0.25 * s;
        y = (xy + yx) / s;
        z = (xz + zx) / s;
    } else if yy > zz {
        let s = (1. + yy - xx - zz).sqrt() * 2.;
        w = (xz - zx) / s;
        x = (xy + yx) / s;
        y = 0.25 * s;
        z = (yz + zy) / s;
    } else {
        let s = (1. + zz - xx - yy).sqrt() * 2.;
        w = (yx - xy) / s;
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

pub struct CharBody {
    pub handle: RigidBodyHandle,
    pub position: [f32; 3],
    pub destination: Option<[f32; 3]>,
    pub speed: f32,
    pub orientation: f32,
    pub listener: u32,
    pub guest_character: u32,
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
    pub bodies: HashMap<u32, DynBody>,
    pub chars: HashMap<u32, CharBody>,
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

fn collider_for(prim: &Prim, info: &BodyInfo) -> Option<ColliderBuilder> {
    let b = match prim {
        Prim::Sphere { c, r } => ColliderBuilder::ball(*r).translation(Vector::new(c[0], c[1], c[2])),
        Prim::Capsule { a, b, r } => ColliderBuilder::capsule_from_endpoints(Vector::new(a[0], a[1], a[2]), Vector::new(b[0], b[1], b[2]), *r),
        Prim::Hull(pts) => ColliderBuilder::convex_hull(&pts.iter().map(|p| Vector::new(p[0], p[1], p[2])).collect::<Vec<_>>())?,
        Prim::Mesh(tris) => tri_mesh(tris)?,
    };
    let group = if matches!(prim, Prim::Mesh(_)) { Group::GROUP_2 } else { Group::GROUP_1 };
    Some(b.friction(info.friction).restitution(info.restitution).collision_groups(InteractionGroups::new(group, Group::ALL, InteractionTestMode::And)))
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
            bodies: HashMap::new(),
            chars: HashMap::new(),
            by_handle: HashMap::new(),
            prev_pairs: Default::default(),
            log: vec![],
        }
    }

    /// `PhysicsManager::LoadPhysics`: parse `data/physics/<name>.hkx`; with `add_to_world` its bodies exist immediately.
    pub fn load_system(&mut self, name: &str, matrix: Option<Mat>, add_to_world: bool, camera_only: bool) -> i32 {
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
        let id = self.systems.len();
        self.systems.push(Some(infos));
        self.system_bodies.push(vec![]);
        if add_to_world && !camera_only {
            let n = self.systems[id].as_ref().map(|v| v.len()).unwrap_or(0);
            for i in 0..n {
                let infos = self.systems[id].as_ref().unwrap();
                if infos[i].mass_inv == 0. {
                    let h = self.insert_body(id, i, matrix.as_ref(), None);
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

    fn insert_body(&mut self, system: usize, index: usize, matrix: Option<&Mat>, pose: Option<Mat>) -> RigidBodyHandle {
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
            if let Some(mut c) = collider_for(prim, &info) {
                if mass > 0. {
                    c = c.mass(mass / total_prims);
                }
                self.world.insert_collider(c, Some(h));
            }
        }
        h
    }

    /// `GenerateRigidBodyFromPhysicsSystem`: instantiate dynamic body `index` of a loaded system.
    pub fn generate_body(&mut self, guest: u32, system: usize, index: usize) -> bool {
        let count = self.systems.get(system).and_then(|s| s.as_ref()).map(|s| s.len()).unwrap_or(0);
        if count == 0 {
            return false;
        }
        let dynamic: Vec<usize> = self.systems[system].as_ref().unwrap().iter().enumerate().filter(|(_, b)| b.mass_inv > 0.).map(|(i, _)| i).collect();
        let pick = dynamic.get(index).copied().or_else(|| dynamic.first().copied()).unwrap_or(index.min(count - 1));
        let h = self.insert_body(system, pick, None, None);
        self.by_handle.insert(h, guest);
        self.bodies.insert(guest, DynBody { handle: h, system, user_data: 0, listener: 0, in_world: true });
        true
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
        let filter = QueryFilter::only_fixed().groups(InteractionGroups::new(Group::ALL, Group::GROUP_2, InteractionTestMode::And));
        self.world.cast_ray(&ray, max, true, filter).map(|(_, t)| t)
    }

    /// Height of the ground below `pos` (searching `up` above and `down` below).
    pub fn ground_height(&self, pos: [f32; 3], up: f32, down: f32) -> Option<f32> {
        let from = [pos[0], pos[1] + up, pos[2]];
        self.cast_ground_ray(from, up + down).map(|t| from[1] - t)
    }

    // --- characters ---------------------------------------------------------------------------------------------------

    pub fn add_character(&mut self, guest: u32, character: u32, pos: [f32; 3]) {
        let body = RigidBodyBuilder::kinematic_position_based().translation(Vector::new(pos[0], pos[1] + 0.75, pos[2]));
        let h = self.world.insert_body(body);
        self.world.insert_collider(ColliderBuilder::capsule_y(0.45, 0.3), Some(h));
        self.by_handle.insert(h, guest);
        self.chars.insert(guest, CharBody { handle: h, position: pos, destination: None, speed: 0., orientation: 0., listener: 0, guest_character: character });
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
    fn slide(&self, pos: [f32; 3], delta: [f32; 2]) -> [f32; 3] {
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
            let filter = QueryFilter::only_fixed();
            match self.world.cast_shape(&pose, rest, &ball, opts, filter) {
                Some((_, hit)) => {
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
            }
            if let Some(g) = self.ground_height(p, 1.0, 3.0) {
                p[1] = g;
            }
            let c = self.chars.get_mut(&id).unwrap();
            c.position = p;
            let rb = &mut self.world.bodies[c.handle];
            rb.set_next_kinematic_translation(Vector::new(p[0], p[1] + 0.75, p[2]));
        }
        self.world.gravity = Vector::new(self.gravity[0], self.gravity[1], self.gravity[2]);
        self.world.integration_parameters.dt = dt;
        self.world.step();
        // contacts between bodies the guest knows about
        let mut now = std::collections::HashSet::new();
        let mut out = vec![];
        for pair in self.world.narrow_phase.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            let (Some(c1), Some(c2)) = (self.world.colliders.get(pair.collider1), self.world.colliders.get(pair.collider2)) else { continue };
            let (Some(b1), Some(b2)) = (c1.parent(), c2.parent()) else { continue };
            let (Some(&g1), Some(&g2)) = (self.by_handle.get(&b1), self.by_handle.get(&b2)) else { continue };
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
