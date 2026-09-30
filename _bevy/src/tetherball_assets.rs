//! Activity-owned asset preparation for the synchronous tetherball engine host.
//! Names come from Initialize 0x803966c4 and Tetherball::Initialize 0x8039d3d8.
use crate::{animation_graph, archive, assets, gsh, model};
use bevy::prelude::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub const ARCHIVE: &str = "minigames/tetherball/mgtetherball.viv";
const VISIBLE: [&str; 2] = ["teatherball.o", "teatherball_rope.o"];
const SHADOWS: [&str; 2] = ["teatherball_shadow.o", "teatherball_rope_shadow.o"];
const BANKS: [&str; 2] = ["teatherball.gsh", "teatherball_rope.gsh"];

/// CPU preparation can run on the existing loader thread. No Bevy assets are
/// mutated until every required original member has decoded successfully.
pub struct Decoded {
    visible: BTreeMap<String, assets::BuiltModel>,
    shadows: BTreeMap<String, model::ModelFile>,
    player: animation_graph::PlayerGraph,
    clips: BTreeMap<usize, AnimationClip>,
    report: Value,
}
impl Decoded {
    pub fn load(data_root: &Path) -> Result<Self, String> {
        let archive = data_root.join("files/data").join(ARCHIVE);
        let source = archive.to_string_lossy();
        let schemas = model::Schemas::embedded();
        let mut banks = BTreeMap::new();
        for name in BANKS {
            let member = format!("{source}::{name}");
            let (bytes, _) = archive::read_virtual(&member)?;
            let bank = gsh::parse(&bytes)?;
            if bank.entries.is_empty() {
                return Err(format!("{member}: empty texture bank"));
            }
            let images = bank
                .entries
                .iter()
                .map(|entry| {
                    let (_, width, height) = gsh::decode(entry, &bytes)?;
                    Ok(json!({"width":width,"height":height}))
                })
                .collect::<Result<Vec<_>, String>>()?;
            banks.insert(name, images);
        }
        let mut visible = BTreeMap::new();
        let mut geometry = BTreeMap::new();
        for name in VISIBLE {
            let member = format!("{source}::{name}");
            let built = assets::build(&member, &schemas)?;
            if built.prims.is_empty() || !built.warnings.is_empty() {
                return Err(format!(
                    "{member}: incomplete render assets: {:?}",
                    built.warnings
                ));
            }
            geometry.insert(name, assets::summary(&built));
            visible.insert(name.to_string(), built);
        }
        // Keep the original shadow geometry. The StandardMaterial pipeline
        // cannot reproduce native volume composition, so don't draw substitutes.
        let mut shadows = BTreeMap::new();
        let mut shadow_geometry = BTreeMap::new();
        for name in SHADOWS {
            let member = format!("{source}::{name}");
            let (bytes, _) = archive::read_virtual(&member)?;
            let parsed = model::parse(&bytes, &schemas)?;
            if parsed.prims.is_empty() {
                return Err(format!("{member}: empty shadow geometry"));
            }
            shadow_geometry.insert(
                name,
                json!({
                    "primitives":parsed.prims.len(),
                    "triangles":parsed.prims.iter().map(|p|p.tris.len()).sum::<usize>(),
                }),
            );
            shadows.insert(name.to_string(), parsed);
        }
        // Tetherball uses the same states in both gender graphs; the female
        // overlay only replaces non-tetherball states. Keep the base graph here
        // until character selection supplies its actual gender to the host.
        let player = animation_graph::PlayerGraph::load(data_root, false)?;
        let decoded_clips = player.tetherball_clips()?;
        let mut clips = BTreeMap::new();
        let mut clip_report = BTreeMap::new();
        for (index, clip) in decoded_clips {
            clips.insert(index, crate::character::animation_clip(&clip)?);
            clip_report.insert(
                index,
                json!({"name":clip.name,"samples":clip.sample_count,"sample_rate":clip.sample_rate(),"native_timing":clip.native_timing,"caveats":clip.caveats}),
            );
        }
        Ok(Self {
            visible,
            shadows,
            player,
            clips,
            report: json!({
                "archive":source,"visible":geometry,"texture_banks":banks,
                "shadows":shadow_geometry,"shadow_composition":"unimplemented",
                "tetherball_animation_states":41,"clips":clip_report,
                "animation_sample_rate":crate::anim::CLIP_FPS,
                "animation_playback":"unimplemented",
            }),
        })
    }
}

/// Main-world asset readiness only. This is not renderer/GPU readiness or a
/// complete scene: characters, placeables, controllers and frontend follow.
#[derive(Resource)]
pub struct Prepared {
    pub visible: BTreeMap<String, assets::Uploaded>,
    pub shadows: BTreeMap<String, model::ModelFile>,
    pub animation_graph: animation_graph::Graph,
    pub skeleton: crate::skeleton::Skeleton,
    pub clips: BTreeMap<usize, Handle<AnimationClip>>,
    images: Vec<Handle<Image>>,
    pub report: Value,
}
impl Prepared {
    pub fn ready(&self, world: &World) -> bool {
        let (Some(meshes), Some(materials), Some(images), Some(clips)) = (
            world.get_resource::<Assets<Mesh>>(),
            world.get_resource::<Assets<StandardMaterial>>(),
            world.get_resource::<Assets<Image>>(),
            world.get_resource::<Assets<AnimationClip>>(),
        ) else {
            return false;
        };
        self.visible.len() == VISIBLE.len()
            && self.shadows.len() == SHADOWS.len()
            && self.visible.values().all(|model| {
                !model.parts.is_empty()
                    && model.parts.iter().all(|(mesh, material)| {
                        meshes.contains(mesh.id()) && materials.contains(material.id())
                    })
            })
            && self.images.iter().all(|image| images.contains(image.id()))
            && !self.clips.is_empty()
            && self.clips.values().all(|clip| clips.contains(clip.id()))
    }
}

/// Install only after CPU preparation succeeds. The caller must remove activity
/// entities before replacing/releasing assets; it owns that scene boundary.
pub fn install(world: &mut World, decoded: Decoded) -> Result<(), String> {
    if !world.contains_resource::<Assets<Mesh>>()
        || !world.contains_resource::<Assets<StandardMaterial>>()
        || !world.contains_resource::<Assets<Image>>()
        || !world.contains_resource::<Assets<AnimationClip>>()
    {
        return Err("tetherball preparation requires mesh/material/image/animation stores".into());
    }
    release(world);
    let mut visible = BTreeMap::new();
    let mut images = Vec::new();
    world.resource_scope(|world, mut mesh_store: Mut<Assets<Mesh>>| {
        world.resource_scope(|world, mut material_store: Mut<Assets<StandardMaterial>>| {
            let mut image_store = world.resource_mut::<Assets<Image>>();
            for (name, built) in decoded.visible {
                let uploaded = assets::upload(
                    &built,
                    &mut mesh_store,
                    &mut material_store,
                    &mut image_store,
                    false,
                );
                for (_, material) in &uploaded.parts {
                    if let Some(image) = material_store
                        .get(material.id())
                        .and_then(|m| m.base_color_texture.as_ref())
                    {
                        if !images.contains(image) {
                            images.push(image.clone());
                        }
                    }
                }
                visible.insert(name, uploaded);
            }
        });
    });
    let clips = decoded
        .clips
        .into_iter()
        .map(|(index, clip)| {
            (
                index,
                world.resource_mut::<Assets<AnimationClip>>().add(clip),
            )
        })
        .collect();
    world.insert_resource(Prepared {
        visible,
        shadows: decoded.shadows,
        images,
        animation_graph: decoded.player.graph,
        skeleton: decoded.player.skeleton,
        clips,
        report: decoded.report,
    });
    Ok(())
}

pub fn release(world: &mut World) {
    let Some(prepared) = world.remove_resource::<Prepared>() else {
        return;
    };
    for uploaded in prepared.visible.values() {
        for (mesh, material) in &uploaded.parts {
            if let Some(mut store) = world.get_resource_mut::<Assets<Mesh>>() {
                store.remove(mesh.id());
            }
            if let Some(mut store) = world.get_resource_mut::<Assets<StandardMaterial>>() {
                store.remove(material.id());
            }
        }
    }
    if let Some(mut store) = world.get_resource_mut::<Assets<Image>>() {
        for image in prepared.images {
            store.remove(image.id());
        }
    }
    if let Some(mut store) = world.get_resource_mut::<Assets<AnimationClip>>() {
        for clip in prepared.clips.into_values() {
            store.remove(clip.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_assets_prepare_restart_and_release() {
        let root = crate::bridge::data_root();
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<AnimationClip>>();
        // Unrelated assets must survive activity cleanup.
        let unrelated = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let counts = |w: &World| {
            (
                w.resource::<Assets<Mesh>>().len(),
                w.resource::<Assets<StandardMaterial>>().len(),
                w.resource::<Assets<Image>>().len(),
                w.resource::<Assets<AnimationClip>>().len(),
            )
        };
        let baseline = counts(&world);
        install(
            &mut world,
            Decoded::load(&root).expect("original tetherball archive required"),
        )
        .unwrap();
        assert!(world.resource::<Prepared>().ready(&world));
        assert_eq!(world.resource::<Prepared>().skeleton.bones.len(), 68);
        assert!(
            (56..=96).all(|id| world.resource::<Prepared>().animation_graph.states[id].is_some())
        );
        eprintln!("{}", world.resource::<Prepared>().report);
        let first = counts(&world);
        assert!(first.0 > 0 && first.2 > 0);
        assert!(Decoded::load(&root.join("missing-input")).is_err());
        assert!(world.resource::<Prepared>().ready(&world));
        install(&mut world, Decoded::load(&root).unwrap()).unwrap();
        assert_eq!(counts(&world), first);
        assert!(world.resource::<Prepared>().ready(&world));
        let image = world.resource::<Prepared>().images[0].id();
        world.resource_mut::<Assets<Image>>().remove(image);
        assert!(!world.resource::<Prepared>().ready(&world));
        release(&mut world);
        release(&mut world);
        assert!(!world.contains_resource::<Prepared>());
        assert_eq!(counts(&world), baseline);
        assert!(
            world
                .resource::<Assets<StandardMaterial>>()
                .contains(unrelated.id())
        );
        let mut missing_stores = World::new();
        assert!(install(&mut missing_stores, Decoded::load(&root).unwrap()).is_err());
        assert!(!missing_stores.contains_resource::<Prepared>());
    }
}
