//! The playable character built entirely from the original data with the Rust decoders: `alicia.o` (hardware-skinned
//! model), `player_skel.ske` (68-bone rig) and clips from `player_anims.anm`.  No glTF is generated: the bone hierarchy
//! becomes joint entities (bind pose = local quaternion/translation/scale from the `.ske`), the inverse bind matrices
//! come from the composed world bind pose, and every decoded clip becomes a Bevy `AnimationClip` sampled at 30 Hz.
use bevy::{animation::{animated_field,prelude::*,AnimatedBy,AnimationTargetId},mesh::skinning::{SkinnedMesh,SkinnedMeshInverseBindposes},prelude::*};
use crate::{anim::{self,Bank,Clip,CLIP_FPS},archive,assets,skeleton::Skeleton};

/// Clips the game plays (bank indices found by name in `player_anims.anm`; `Bank::decode` names are checked on load).
pub const CLIPS:[(&str,usize);3]=[("S_idle",250),("S_walk",263),("S_run",253)];

pub struct CharacterData{pub model:assets::BuiltModel,pub skeleton:Skeleton,pub clips:Vec<Clip>,pub source:String}

/// CPU-side decode of everything the player needs (runs on the loader thread).
pub fn load(viv_dir:&str,schemas:&crate::model::Schemas)->Result<CharacterData,String>{
    let model_src=format!("{viv_dir}/models/characters.viv::alicia.viv::alicia.o");
    let anim_viv=format!("{viv_dir}/player_anims.viv");
    let model=assets::build(&model_src,schemas)?;
    let skeleton=Skeleton::parse(&archive::read_virtual(&format!("{anim_viv}::player_skel.ske"))?.0)?;
    let bank=Bank::parse(archive::read_virtual(&format!("{anim_viv}::player_anims.anm"))?.0)?;
    let mut clips=vec![];
    for (name,index) in CLIPS{
        let c=bank.decode(index,&skeleton)?;
        if c.name!=name{return Err(format!("clip {index} is {:?}, expected {name}",c.name))}
        clips.push(c);
    }
    let max_joint=model.prims.iter().filter_map(|p|p.joints.as_ref()).flatten().flat_map(|j|j[..3].iter().copied()).max().unwrap_or(0) as usize;
    if max_joint>=skeleton.bones.len(){return Err(format!("model references joint {max_joint} but the skeleton has {} bones",skeleton.bones.len()))}
    Ok(CharacterData{model,skeleton,clips,source:model_src})
}

/// One decoded clip -> Bevy animation: rotation/translation/scale curves per animated bone (`i / 30` s), unit
/// quaternions with continuous sign.
pub fn animation_clip(clip:&Clip)->Result<AnimationClip,String>{
    let mut out=AnimationClip::default();
    // Bevy needs two keyframes per curve; a one-sample clip holds its pose.
    let pad=|n:usize|(0..n.max(2)).map(move|i|(i,i.min(n.saturating_sub(1))));
    for (bone,samples) in &clip.rot{
        let q=anim::normalized(samples)?;
        let curve=AnimatableKeyframeCurve::new(pad(q.len()).map(|(t,i)|(pad_time(t),Quat::from_xyzw(q[i][0],q[i][1],q[i][2],q[i][3])))).map_err(|e|format!("{e:?}"))?;
        out.add_curve_to_target(target(*bone),AnimatableCurve::new(animated_field!(Transform::rotation),curve));
    }
    for (bone,samples) in &clip.trans{
        let v:Vec<Vec3>=samples.iter().map(|s|s.map(|v|Vec3::new(v[0] as f32,v[1] as f32,v[2] as f32)).unwrap_or(Vec3::ZERO)).collect();
        let curve=AnimatableKeyframeCurve::new(pad(v.len()).map(|(t,i)|(pad_time(t),v[i]))).map_err(|e|format!("{e:?}"))?;
        out.add_curve_to_target(target(*bone),AnimatableCurve::new(animated_field!(Transform::translation),curve));
    }
    for (bone,samples) in &clip.scale{
        let v:Vec<Vec3>=samples.iter().map(|s|s.map(|v|Vec3::new(v[0] as f32,v[1] as f32,v[2] as f32)).unwrap_or(Vec3::ONE)).collect();
        let curve=AnimatableKeyframeCurve::new(pad(v.len()).map(|(t,i)|(pad_time(t),v[i]))).map_err(|e|format!("{e:?}"))?;
        out.add_curve_to_target(target(*bone),AnimatableCurve::new(animated_field!(Transform::scale),curve));
    }
    Ok(out)
}
fn pad_time(i:usize)->f32{(i as f64/CLIP_FPS) as f32}
fn joint_name(bone:usize)->Name{Name::new(format!("bone{bone}"))}
fn target(bone:usize)->AnimationTargetId{AnimationTargetId::from_name(&joint_name(bone))}

fn mat3_cols(m:&[f64;9])->Mat3{Mat3::from_cols(Vec3::new(m[0] as f32,m[3] as f32,m[6] as f32),Vec3::new(m[1] as f32,m[4] as f32,m[7] as f32),Vec3::new(m[2] as f32,m[5] as f32,m[8] as f32))}

/// Spawn the rig and the skinned meshes below `parent`; `player` is the entity that owns the `AnimationPlayer`.
/// Returns the joint entities (index = bone index).
pub fn spawn_rig(commands:&mut Commands,ibp:&mut Assets<SkinnedMeshInverseBindposes>,skeleton:&Skeleton,up:&assets::Uploaded,parent:Entity,player:Entity)->Vec<Entity>{
    let joints:Vec<Entity>=skeleton.bones.iter().map(|b|{
        let t=Transform{translation:Vec3::new(b.trans[0] as f32,b.trans[1] as f32,b.trans[2] as f32),rotation:Quat::from_xyzw(b.quat[0] as f32,b.quat[1] as f32,b.quat[2] as f32,b.quat[3] as f32).normalize(),scale:Vec3::new(b.scale[0] as f32,b.scale[1] as f32,b.scale[2] as f32)};
        commands.spawn((joint_name(b.index),t,Visibility::default(),target(b.index),AnimatedBy(player))).id()
    }).collect();
    for b in &skeleton.bones{
        let owner=if b.parent<0{parent}else{joints[b.parent as usize]};
        commands.entity(joints[b.index]).insert(ChildOf(owner));
    }
    let inverse:Vec<Mat4>=skeleton.bones.iter().map(|b|Mat4::from_mat3_translation(mat3_cols(&b.world_matrix),Vec3::new(b.world_translation[0] as f32,b.world_translation[1] as f32,b.world_translation[2] as f32)).inverse()).collect();
    let handle=ibp.add(SkinnedMeshInverseBindposes::from(inverse));
    for (mesh,material) in &up.parts{
        commands.spawn((Mesh3d(mesh.clone()),MeshMaterial3d(material.clone()),Transform::IDENTITY,ChildOf(parent),SkinnedMesh{inverse_bindposes:handle.clone(),joints:joints.clone()}));
    }
    joints
}

#[cfg(test)]
mod tests{
    use super::*;
    /// The full player decode (model + rig + clips) succeeds from the original files and the rig is consistent.
    #[test]
    fn player_decodes_from_original_files(){
        let dir=crate::bridge::data_root().join("files").join("data").join("characters");
        if !dir.exists(){eprintln!("DATA absent; skipped");return}
        let d=load(&dir.to_string_lossy().replace('\\',"/"),&crate::model::Schemas::embedded()).unwrap();
        assert_eq!(d.skeleton.bones.len(),68);assert!(d.skeleton.is_player());
        assert_eq!(d.clips.len(),3);
        let skinned=d.model.prims.iter().filter(|p|p.joints.is_some()).count();
        eprintln!("alicia: {} prims ({skinned} skinned), {} triangles; clips: {:?}",d.model.prims.len(),d.model.prims.iter().map(|p|p.indices.len()/3).sum::<usize>(),d.clips.iter().map(|c|(c.name.clone(),c.sample_count,c.rot.len(),c.trans.len())).collect::<Vec<_>>());
        assert!(skinned>0);
        for c in &d.clips{animation_clip(c).unwrap();}
    }
}
