//! Immediate-mode textured drawing of the VM-hosted minigames: the batches the game builds with `EAGL::DrawTextured`
//! (player indicators, target markers, aiming cursors, rings) are captured by `mgvm` (texture = shape of a `TarManager`
//! bank, model matrix, vertices with GX RGBA8 colour and UV) and drawn here as unlit, alpha-blended meshes.
use crate::{game, mg_session, mgvm};
use bevy::{asset::RenderAssetUsages, mesh::{Indices, PrimitiveTopology}, prelude::*, render::render_resource::{Extent3d, TextureDimension, TextureFormat}};
use std::collections::HashMap;

#[derive(Component)]
pub struct ImmBatch(usize);

#[derive(Default)]
pub struct ImmCache {
    banks: HashMap<String, Option<crate::gsh::Gsh>>,
    materials: HashMap<Option<(String, String, usize)>, Handle<StandardMaterial>>,
    pool: Vec<(Entity, Handle<Mesh>)>,
}

/// Triangle indices for a GX primitive of `n` vertices (0x80 quads, 0x90 triangles, 0x98 strip, 0xa0 fan).
pub fn triangles(prim: u32, n: usize) -> Vec<u32> {
    let n = n as u32;
    let mut out = vec![];
    match prim {
        0x80 => {
            for q in 0..n / 4 {
                let b = q * 4;
                out.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        0x98 => {
            for i in 2..n {
                if i % 2 == 0 {
                    out.extend_from_slice(&[i - 2, i - 1, i]);
                } else {
                    out.extend_from_slice(&[i - 1, i - 2, i]);
                }
            }
        }
        0xa0 => {
            for i in 2..n {
                out.extend_from_slice(&[0, i - 1, i]);
            }
        }
        _ => out.extend(0..n / 3 * 3),
    }
    out
}

fn material(cache: &mut ImmCache, tex: &Option<(String, String, usize)>, banks: &HashMap<String, std::sync::Arc<Vec<u8>>>, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    if let Some(m) = cache.materials.get(tex) {
        return m.clone();
    }
    let image = tex.as_ref().and_then(|(bank, shape, index)| {
        let data = banks.get(bank)?;
        let gsh = cache.banks.entry(bank.clone()).or_insert_with(|| crate::gsh::parse(data).ok()).as_ref()?;
        // by long name, else by position (the TarManager table follows the bank order; some banks' long names are not
        // attachments the parser knows)
        let e = gsh.entries.iter().find(|e| e.full_name.as_deref().is_some_and(|n| n.trim().eq_ignore_ascii_case(shape)) || e.name.eq_ignore_ascii_case(shape)).or_else(|| gsh.entries.get(*index))?;
        let (rgba, w, h) = crate::gsh::decode(e, data).ok()?;
        Some(images.add(Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())))
    });
    if tex.is_some() && image.is_none() && std::env::var("EAGL_MG_DEBUG").is_ok() {
        eprintln!("[mg] no texture for {tex:?}");
    }
    let m = materials.add(StandardMaterial { base_color_texture: image, unlit: true, alpha_mode: AlphaMode::Blend, cull_mode: None, depth_bias: 10., ..default() });
    cache.materials.insert(tex.clone(), m.clone());
    m
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    mut commands: Commands,
    snap: Option<Res<mg_session::MgSnapshot>>,
    game: Option<Res<game::Game>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: Local<ImmCache>,
    mut batches: Query<(&ImmBatch, &mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(snapshot) = snap.as_ref().and_then(|s| s.0.as_ref()) else {
        // session over: its entities (MgEntity) are gone, and so are the cached ones
        cache.pool.clear();
        return;
    };
    let radius = game.as_ref().map(|g| g.world_radius).unwrap_or(0.);
    let draws: &Vec<mgvm::ImmDraw> = &snapshot.imm;
    for (i, d) in draws.iter().enumerate() {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.verts.iter().map(|v| v.0).collect::<Vec<_>>());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 1., 0.]; d.verts.len()]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, d.verts.iter().map(|v| v.2).collect::<Vec<_>>());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, d.verts.iter().map(|v| v.1.map(|c| c as f32 / 255.)).collect::<Vec<_>>());
        mesh.insert_indices(Indices::U32(triangles(d.prim, d.verts.len())));
        let mat = material(&mut cache, &d.tex, &snapshot.banks, &mut images, &mut materials);
        let t = mg_session::mat_to_transform(radius, &d.model);
        if i < cache.pool.len() {
            let (_, h) = &cache.pool[i];
            meshes.insert(h.id(), mesh);
        } else {
            let h = meshes.add(mesh);
            let e = commands.spawn((mg_session::MgEntity, ImmBatch(i), Mesh3d(h.clone()), MeshMaterial3d(mat.clone()), t, Visibility::Inherited)).id();
            cache.pool.push((e, h));
        }
        for (b, mut tr, mut vis, mut m) in &mut batches {
            if b.0 == i {
                *tr = t;
                *vis = Visibility::Inherited;
                if m.0 != mat {
                    m.0 = mat.clone();
                }
            }
        }
    }
    for (b, _, mut vis, _) in &mut batches {
        if b.0 >= draws.len() {
            *vis = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    /// Decode every shape of a bank dumped by `EAGL_DUMP_BANKS` (set `EAGL_BANK` to its path).
    #[test]
    #[ignore]
    fn decode_dumped_bank() {
        let Ok(path) = std::env::var("EAGL_BANK") else { return };
        let d = std::fs::read(path).unwrap();
        let g = crate::gsh::parse(&d).unwrap();
        for e in &g.entries {
            println!("{} {:?} id {} {}x{} -> {:?}", e.name, e.full_name, e.record_id, e.width, e.height, crate::gsh::decode(e, &d).map(|r| r.0.len()));
        }
    }

    #[test]
    fn primitives_become_triangles() {
        assert_eq!(super::triangles(0x90, 6), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(super::triangles(0x80, 4), vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(super::triangles(0xa0, 4), vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(super::triangles(0x98, 4), vec![0, 1, 2, 2, 1, 3]);
    }
}
