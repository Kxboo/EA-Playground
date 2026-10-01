//! LION particle effects (`effects/*.lef`) played with a CPU emitter simulation and drawn as camera-facing sprites.
//!
//! Original: the effect files (text trees of emitter descriptors: rates, lifetimes, velocity / size / rotation with
//! variance, four colour steps, alpha fades, blend mode, texture from `effects.gsh`); disabled descriptors are dropped as
//! `cLionParticleParser::DescriptorRemoveDisabled` does.  Approximate (written for this port, not a port of
//! `cParticleBehaviour`/`cParticleBucket`): the exact interpolation of the colour steps and fades, the random sequence,
//! emission timing within a frame and the sprite sizing against the original renderer.
use bevy::{asset::RenderAssetUsages, mesh::{Indices, PrimitiveTopology}, prelude::*, render::render_resource::{Extent3d, TextureDimension, TextureFormat}};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug, Default)]
pub struct Emitter {
    pub name: String,
    pub dynamic: bool,
    pub emitter_life: f32,
    pub infinite: bool,
    pub rate: f32,
    pub count_clamp: u32,
    pub life: [f32; 2],
    pub pos: [[f32; 3]; 2],
    pub vel: [[f32; 3]; 2],
    pub acc: [[f32; 3]; 2],
    pub size: [f32; 2],
    pub size_var: f32,
    pub size_vel: f32,
    pub rot: [f32; 2],
    pub rot_vel: [f32; 2],
    pub colours: Vec<([f32; 4], f32)>,
    pub fade_in: f32,
    pub fade_out: f32,
    pub additive: bool,
    pub texture: String,
    pub drag: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Effect {
    pub emitters: Vec<Emitter>,
}

fn nums(s: &str) -> Vec<f32> {
    s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect()
}

impl Effect {
    pub fn parse(text: &str) -> Effect {
        let mut emitters = vec![];
        for block in text.split("<DESCRIPTOR").skip(1) {
            let mut kv: HashMap<&str, &str> = HashMap::new();
            for line in block.lines() {
                if let Some((k, v)) = line.split_once(" = ") {
                    if k.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                        kv.entry(k).or_insert(v.trim());
                    }
                }
            }
            let f = |k: &str| kv.get(k).map(|v| nums(v)).unwrap_or_default();
            let f1 = |k: &str| f(k).first().copied().unwrap_or(0.);
            let v3 = |k: &str| -> [f32; 3] { let v = f(k); [v.first().copied().unwrap_or(0.), v.get(1).copied().unwrap_or(0.), v.get(2).copied().unwrap_or(0.)] };
            if f1("DISABLED_FLAG") != 0. || kv.get("SHAPE").is_some_and(|s| *s != "SPRITE") {
                continue;
            }
            let mut colours = vec![];
            for i in 0..4 {
                if f1(&format!("DO_COLOURSTEP{i}")) != 0. {
                    let c = f(&format!("COLOUR{i}"));
                    if c.len() == 4 {
                        colours.push(([c[0] / 255., c[1] / 255., c[2] / 255., c[3] / 255.], f1(&format!("COLOUR_TIME{i}"))));
                    }
                }
            }
            let life = f1("LIFE_BASE");
            let rot = v3("ROTXYZ_BASE");
            let rotv = v3("ROTXYZ_VARIANCE");
            let rot_vel = v3("ROTXYZ_VEL_BASE");
            let rot_vel_var = v3("ROTXYZ_VEL_VARIANCE");
            let size = v3("SIZEXYZ_BASE");
            emitters.push(Emitter {
                name: kv.get("NAME").unwrap_or(&"").to_string(),
                dynamic: f1("DYNAMIC_PLACEMENT_FLAG") != 0.,
                emitter_life: f1("EMITTER_LIFE_BASE"),
                infinite: f1("EMITTER_LIFE_INFINITE") != 0.,
                rate: f1("EMISSION_RATE_BASE"),
                count_clamp: f1("EMISSION_COUNT_CLAMP") as u32,
                life: [life, f1("LIFE_VARIANCE")],
                pos: [v3("POS_BASE"), v3("POS_VARIANCE")],
                vel: [v3("VEL_BASE"), v3("VEL_VARIANCE")],
                acc: [v3("ACC_BASE"), v3("ACC_VARIANCE")],
                size: [size[0], f1("SIZE_SCALE")],
                size_var: v3("SIZEXYZ_VARIANCE")[0],
                size_vel: v3("SIZEXYZ_VEL_BASE")[0],
                rot: [rot[2], rotv[2]],
                rot_vel: [rot_vel[2], rot_vel_var[2]],
                colours,
                fade_in: f1("ALPHA_FADEIN"),
                fade_out: f1("ALPHA_FADEOUT"),
                additive: kv.get("BLEND_MODE").is_some_and(|b| b.contains("ADD")),
                texture: kv.get("TEXTURE").unwrap_or(&"").to_string(),
                drag: if f1("DO_DRAG") != 0. { f1("DRAG_FACTOR") } else { 0. },
            });
        }
        Effect { emitters }
    }

    pub fn load(name: &str) -> Option<Effect> {
        let path = crate::bridge::data_root().join("files").join("data").join("effects").join(format!("{}.lef", name.to_lowercase()));
        let text = std::fs::read_to_string(path).ok()?;
        Some(Effect::parse(&text))
    }
}

struct Particle {
    pos: [f32; 3],
    vel: [f32; 3],
    acc: [f32; 3],
    age: f32,
    life: f32,
    size: f32,
    rot: f32,
    rot_vel: f32,
}

struct EmitterState {
    def: Emitter,
    particles: Vec<Particle>,
    age: f32,
    carry: f32,
    emitted: u32,
}

pub struct Instance {
    pub id: u32,
    pub name: String,
    pub origin: [f32; 3],
    emitters: Vec<EmitterState>,
    /// Emission stops (the effect was "disabled"); live particles finish.
    stopped: bool,
    /// Remove after this many seconds even if emitting.
    kill_in: Option<f32>,
}

#[derive(Resource, Default)]
pub struct FxWorld {
    cache: HashMap<String, Option<Arc<Effect>>>,
    pub instances: Vec<Instance>,
    rng: u64,
}

fn rnd(s: &mut u64) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    ((*s >> 40) as f32) / ((1u64 << 24) as f32)
}
fn spread(s: &mut u64, base: f32, var: f32) -> f32 {
    base + var * (rnd(s) * 2. - 1.)
}

impl FxWorld {
    pub fn spawn(&mut self, id: u32, name: &str, pos: [f32; 3]) {
        let effect = self.cache.entry(name.to_lowercase()).or_insert_with(|| Effect::load(name).map(Arc::new)).clone();
        let Some(effect) = effect else { return };
        let emitters = effect.emitters.iter().map(|e| EmitterState { def: e.clone(), particles: vec![], age: 0., carry: 0., emitted: 0 }).collect();
        self.instances.push(Instance { id, name: name.to_string(), origin: pos, emitters, stopped: false, kill_in: None });
    }
    pub fn move_to(&mut self, id: u32, pos: [f32; 3]) {
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == id) {
            i.origin = pos;
        }
    }
    /// `DisableAndDestroyPartFx(guid, delay)`: stop emitting now, drop the instance after the delay.
    pub fn stop(&mut self, id: u32, delay: f32) {
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == id) {
            i.stopped = true;
            i.kill_in = Some(delay.max(0.));
        }
    }
    pub fn update(&mut self, dt: f32) {
        if self.rng == 0 {
            self.rng = 0x2545_f491_4f6c_dd1d;
        }
        let mut rng = self.rng;
        for inst in &mut self.instances {
            if let Some(k) = &mut inst.kill_in {
                *k -= dt;
            }
            for em in &mut inst.emitters {
                let d = &em.def;
                em.age += dt;
                let emitting = !inst.stopped && (d.infinite || em.age <= d.emitter_life.max(0.0001));
                if emitting && d.rate > 0. {
                    em.carry += d.rate * dt;
                    while em.carry >= 1. {
                        em.carry -= 1.;
                        if d.count_clamp > 0 && em.emitted >= d.count_clamp {
                            em.carry = 0.;
                            break;
                        }
                        em.emitted += 1;
                        let p = |s: &mut u64, base: [f32; 3], var: [f32; 3]| [spread(s, base[0], var[0]), spread(s, base[1], var[1]), spread(s, base[2], var[2])];
                        let off = p(&mut rng, d.pos[0], d.pos[1]);
                        let life = spread(&mut rng, d.life[0], d.life[1]).max(0.01);
                        em.particles.push(Particle {
                            pos: if d.dynamic { [inst.origin[0] + off[0], inst.origin[1] + off[1], inst.origin[2] + off[2]] } else { off },
                            vel: p(&mut rng, d.vel[0], d.vel[1]),
                            acc: p(&mut rng, d.acc[0], d.acc[1]),
                            age: rnd(&mut rng) * dt,
                            life,
                            size: spread(&mut rng, d.size[0], d.size_var),
                            rot: spread(&mut rng, d.rot[0], d.rot[1]),
                            rot_vel: spread(&mut rng, d.rot_vel[0], d.rot_vel[1]),
                        });
                    }
                }
                let drag = d.drag;
                em.particles.retain_mut(|p| {
                    p.age += dt;
                    if p.age >= p.life {
                        return false;
                    }
                    for i in 0..3 {
                        p.vel[i] += p.acc[i] * dt;
                        if drag > 0. {
                            p.vel[i] *= (1. - drag * dt).max(0.);
                        }
                        p.pos[i] += p.vel[i] * dt;
                    }
                    p.rot += p.rot_vel * dt;
                    true
                });
            }
        }
        self.rng = rng;
        self.instances.retain(|i| {
            let alive = i.emitters.iter().any(|e| !e.particles.is_empty()) || (!i.stopped && i.emitters.iter().any(|e| e.def.infinite || e.age <= e.def.emitter_life));
            alive && i.kill_in.is_none_or(|k| k > -2.0)
        });
    }

    /// Sprites ready to draw: world position, size, rotation, RGBA, grouped by (texture, additive).
    fn sprites(&self) -> HashMap<(String, bool), Vec<([f32; 3], f32, f32, [f32; 4])>> {
        let mut out: HashMap<(String, bool), Vec<_>> = HashMap::new();
        for inst in &self.instances {
            for em in &inst.emitters {
                let d = &em.def;
                for p in &em.particles {
                    let u = (p.age / p.life).clamp(0., 1.);
                    let mut c = [1., 1., 1., 1.];
                    if !d.colours.is_empty() {
                        let steps = &d.colours;
                        c = steps[0].0;
                        for w in steps.windows(2) {
                            let (a, b) = (w[0], w[1]);
                            if u >= a.1 && u <= b.1 && b.1 > a.1 {
                                let k = (u - a.1) / (b.1 - a.1);
                                c = std::array::from_fn(|i| a.0[i] * (1. - k) + b.0[i] * k);
                            } else if u > b.1 {
                                c = b.0;
                            }
                        }
                    }
                    if d.fade_in > 0. && u < d.fade_in {
                        c[3] *= u / d.fade_in;
                    }
                    if d.fade_out > 0. && u > 1. - d.fade_out {
                        c[3] *= (1. - u) / d.fade_out;
                    }
                    let size = (p.size + d.size_vel * p.age).max(0.);
                    let pos = if d.dynamic { p.pos } else { [inst.origin[0] + p.pos[0], inst.origin[1] + p.pos[1], inst.origin[2] + p.pos[2]] };
                    out.entry((d.texture.to_lowercase(), d.additive)).or_default().push((pos, size, p.rot, c));
                }
            }
        }
        out
    }
}

/// Textures of `effects.gsh` by lower-cased name.
#[derive(Default)]
pub struct FxTextures {
    gsh: Option<(Vec<u8>, crate::gsh::Gsh)>,
    images: HashMap<String, Option<Handle<Image>>>,
}

impl FxTextures {
    fn image(&mut self, name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(h) = self.images.get(name) {
            return h.clone();
        }
        if self.gsh.is_none() {
            let path = crate::bridge::data_root().join("files").join("data").join("effects").join("effects.gsh");
            if let Ok(d) = std::fs::read(path) {
                if let Ok(g) = crate::gsh::parse(&d) {
                    self.gsh = Some((d, g));
                }
            }
        }
        let handle = self.gsh.as_ref().and_then(|(d, g)| {
            let e = g.entries.iter().find(|e| e.full_name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)) || e.name.eq_ignore_ascii_case(name))?;
            let (rgba, w, h) = crate::gsh::decode(e, d).ok()?;
            let img = Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
            Some(images.add(img))
        });
        self.images.insert(name.to_string(), handle.clone());
        handle
    }
}

#[derive(Component)]
pub struct FxSprites {
    key: (String, bool),
}

/// Rebuild one mesh per (texture, blend) group every frame.
#[allow(clippy::too_many_arguments)]
pub fn render(
    world: Res<FxWorld>,
    game: Option<Res<crate::game::Game>>,
    cam: Query<&GlobalTransform, With<crate::game::GameCamera>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut textures: Local<Option<FxTextures>>,
    mut live: Local<HashMap<(String, bool), (Entity, Handle<Mesh>)>>,
) {
    let textures = textures.get_or_insert_with(FxTextures::default);
    let radius = game.as_ref().map(|g| g.world_radius).unwrap_or(0.);
    let Ok(ct) = cam.single() else { return };
    let (right, up) = (ct.right().as_vec3(), ct.up().as_vec3());
    let groups = world.sprites();
    for (key, sprites) in &groups {
        let mut positions = vec![];
        let mut uvs = vec![];
        let mut colours = vec![];
        let mut indices = vec![];
        for (pos, size, rot, c) in sprites {
            let centre = crate::game::display_matrix(radius, Vec3::from(*pos)).w_axis.truncate();
            let (s, co) = rot.sin_cos();
            let (r, u) = (right * co + up * s, -right * s + up * co);
            let h = size * 0.5;
            let base = positions.len() as u32;
            for (dx, dy, uv) in [(-1., -1., [0., 1.]), (1., -1., [1., 1.]), (1., 1., [1., 0.]), (-1., 1., [0., 0.])] {
                let p = centre + r * (dx * h) + u * (dy * h);
                positions.push(p.to_array());
                uvs.push(uv);
                colours.push(*c);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; positions.len()]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        mesh.insert_indices(Indices::U32(indices));
        match live.get(key) {
            Some((_, handle)) => {
                meshes.insert(handle.id(), mesh);
            }
            None => {
                let image = textures.image(&key.0, &mut images);
                let mat = materials.add(StandardMaterial {
                    base_color_texture: image,
                    unlit: true,
                    alpha_mode: if key.1 { AlphaMode::Add } else { AlphaMode::Blend },
                    cull_mode: None,
                    ..default()
                });
                let handle = meshes.add(mesh);
                let e = commands
                    .spawn((FxSprites { key: key.clone() }, Mesh3d(handle.clone()), MeshMaterial3d(mat), Transform::IDENTITY, bevy::camera::visibility::NoFrustumCulling))
                    .id();
                live.insert(key.clone(), (e, handle));
            }
        }
    }
    // Hide groups with no sprites this frame.
    for (key, (_, handle)) in live.iter() {
        if !groups.contains_key(key) {
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
            mesh.insert_indices(Indices::U32(vec![]));
            meshes.insert(handle.id(), mesh);
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<FxWorld>().add_systems(Update, render);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tetherball_effects_parse_and_emit() {
        let Some(trail) = Effect::load("pg_tetherball_trail") else { return };
        // the ambient-glow descriptor is disabled in the file and dropped; the core-star descriptor remains
        assert_eq!(trail.emitters.len(), 1);
        let e = &trail.emitters[0];
        assert!(e.infinite && e.additive && e.dynamic);
        assert!((e.rate - 150.).abs() < 1e-3 && (e.life[0] - 0.32).abs() < 1e-3);
        assert_eq!(e.texture, "EF_BUBBLE_TETHERBALL");
        let mut w = FxWorld::default();
        w.spawn(1, "pg_tetherball_trail", [0.; 3]);
        for _ in 0..30 {
            w.update(1. / 60.);
        }
        let n: usize = w.instances[0].emitters.iter().map(|e| e.particles.len()).sum();
        assert!((10..=60).contains(&n), "{n} live particles");
        let impact = Effect::load("pg_tetherball_normimpact_plr1").unwrap();
        assert!(impact.emitters.iter().all(|e| e.count_clamp <= 30));
    }
}
