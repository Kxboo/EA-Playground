//! Runtime asset pipeline in Rust: model + texture banks -> CPU-side meshes/materials -> Bevy assets.
//!
//! Replaces the Python worker for models.  Rules (all from Remaster/src/{core,material_bindings,asset_links}.py and
//! FINDINGS.md):
//! * Texture banks are the `.gsh` files in the model's containment scopes (its folder/archive and every enclosing
//!   archive's folder).  A name resolves to an exact full-name match, else a case-insensitive one, else an unnamed
//!   entry whose short directory alias matches.  Several matches are accepted only if their pixels are identical.
//! * Alpha: opaque / mask (cut-out) / blend, classified from the decoded pixels.
//! * TAR properties 22/23 give the S/T wrap (0 clamp, 1 repeat, 2 mirror); non-power-of-two axes clamp
//!   (`RuntimeAllocTARConstructor` 0x803e8c48).
//! * Vertex-colour families are unlit; missing normals are computed from the triangles.
use crate::{archive,gsh,model};
use bevy::{asset::RenderAssetUsages,camera::visibility::RenderLayers,image::{ImageAddressMode,ImageSampler,ImageSamplerDescriptor},mesh::{Indices,PrimitiveTopology},prelude::*,render::render_resource::{Extent3d,TextureDimension,TextureFormat}};
use std::collections::{BTreeMap,HashMap};
use std::path::Path;

#[derive(Clone,Copy,PartialEq,Eq,Debug)] pub enum AlphaKind{Opaque,Mask,Blend}
#[derive(Clone,Copy,PartialEq,Eq,Debug,Hash)] pub enum Wrap{Clamp,Repeat,Mirror}

pub struct BuiltTexture{pub name:String,pub width:usize,pub height:usize,pub rgba:Vec<u8>,pub alpha:AlphaKind}
#[derive(Clone,PartialEq,Debug)]
pub struct BuiltMaterial{pub texture:Option<usize>,pub wrap:[Wrap;2],pub unlit:bool,pub alpha:AlphaKind,pub double_sided:bool,pub base_color:[f32;4]}
pub struct BuiltPrim{
    pub family:String,pub positions:Vec<[f32;3]>,pub normals:Vec<[f32;3]>,pub uvs:Vec<[f32;2]>,pub colors:Option<Vec<[f32;4]>>,
    pub indices:Vec<u32>,pub material:usize,pub joints:Option<Vec<[u16;4]>>,pub weights:Option<Vec<[f32;4]>>,
}
pub struct BuiltModel{pub prims:Vec<BuiltPrim>,pub materials:Vec<BuiltMaterial>,pub textures:Vec<BuiltTexture>,pub warnings:Vec<String>,pub bounds:[[f32;3];2]}

const COLOUR_FAMILIES:[&str;4]=["PlaygroundTexture","PlaygroundTextureBakedLight","PlaygroundTextureShadow","PlaygroundTextureBakedLightShadowDouble"];

pub fn classify_alpha(rgba:&[u8])->AlphaKind{
    let a:Vec<u8>=rgba.iter().skip(3).step_by(4).copied().collect();
    if a.is_empty()||*a.iter().min().unwrap()==255{return AlphaKind::Opaque}
    let total=a.len();let n0=a.iter().filter(|&&x|x==0).count();let nf=a.iter().filter(|&&x|x==255).count();let mid=total-n0-nf;
    if mid==0{return AlphaKind::Mask}
    if n0>0&&(mid as f64/total as f64)<0.15{return AlphaKind::Mask}
    AlphaKind::Blend
}

fn sibling_sources(source:&str,suffix:&str)->Vec<String>{
    if !source.contains("::"){
        let mut v:Vec<String>=Path::new(source).parent().and_then(|d|std::fs::read_dir(d).ok()).map(|rd|rd.filter_map(|e|e.ok()).map(|e|e.path()).filter(|p|p.is_file()&&p.extension().is_some_and(|x|format!(".{}",x.to_string_lossy()).eq_ignore_ascii_case(suffix))).map(|p|p.to_string_lossy().into_owned()).collect()).unwrap_or_default();
        v.sort();return v
    }
    let (parent,name)=source.rsplit_once("::").unwrap();
    let Ok((data,_))=archive::read_virtual(parent) else{return vec![]};
    let folder=|n:&str|{let n=n.replace('\\',"/");n.rsplit_once('/').map(|x|x.0.to_string()).unwrap_or_default()};
    let want=folder(name);
    match archive::entries(&data){
        Some(Ok(t))=>t.into_iter().filter(|e|e.name.to_lowercase().ends_with(suffix)&&folder(&e.name)==want).map(|e|format!("{parent}::{}",e.name)).collect(),
        _=>vec![],
    }
}

/// Texture banks visible to a model: `.gsh` beside it, then beside each enclosing archive.
pub fn texture_sources(source:&str)->Vec<String>{
    let mut out:Vec<String>=vec![];let mut scope=source.to_string();
    loop{
        for s in sibling_sources(&scope,".gsh"){if !out.contains(&s){out.push(s)}}
        match scope.rsplit_once("::"){Some((p,_))=>scope=p.to_string(),None=>break}
    }
    out
}

struct Bank{data:Vec<u8>,gsh:gsh::Gsh}
fn load_banks(sources:&[String],warn:&mut Vec<String>)->Vec<Bank>{
    let mut out=vec![];
    for s in sources{
        match archive::read_virtual(s).and_then(|(d,_)|gsh::parse(&d).map(|g|Bank{data:d,gsh:g})){Ok(b)=>out.push(b),Err(e)=>warn.push(format!("Texture {s}: {e}"))}
    }
    out
}

/// Resolve a material texture name against the banks (exact, case-insensitive, unnamed alias); identical duplicates merge.
fn resolve(name:&str,banks:&[Bank],warn:&mut Vec<String>)->Option<(Vec<u8>,usize,usize)>{
    let n=name.trim();
    let mut hits:Vec<(&Bank,&gsh::Entry)>=vec![];
    for b in banks{for e in &b.gsh.entries{if e.full_name.as_deref().map(str::trim)==Some(n){hits.push((b,e))}}}
    if hits.is_empty(){for b in banks{for e in &b.gsh.entries{if e.full_name.as_deref().map(|f|f.trim().to_lowercase())==Some(n.to_lowercase()){hits.push((b,e))}}}}
    if hits.is_empty(){
        let short:String=name.chars().take(4).collect::<String>().to_lowercase();
        for b in banks{for e in &b.gsh.entries{if e.full_name.as_deref().map(|f|f.trim().is_empty()).unwrap_or(true)&&(e.name.trim().to_lowercase()==n.to_lowercase()||e.name.trim().to_lowercase()==short){hits.push((b,e))}}}
    }
    if hits.is_empty(){warn.push(format!("Material {name}: no texture matches in dependency banks; left untextured"));return None}
    let mut decoded:Vec<(Vec<u8>,usize,usize)>=vec![];
    for (b,e) in hits{
        match gsh::decode(e,&b.data){
            Ok((rgba,w,h))=>{if !decoded.iter().any(|(r,dw,dh)|*dw==w&&*dh==h&&*r==rgba){decoded.push((rgba,w,h))}}
            Err(err)=>{warn.push(format!("Material {name}: {err}"));return None}
        }
    }
    if decoded.len()!=1{warn.push(format!("Material {name}: {} different textures share this identifier; left untextured",decoded.len()));return None}
    decoded.pop()
}

fn wrap_modes(symbol:&str,w:usize,h:usize)->[Wrap;2]{
    let mut out=[Wrap::Repeat,Wrap::Repeat];
    for part in symbol.split(';'){
        let Some((k,v))=part.split_once('=') else{continue};
        let axis=match k{"22"=>0,"23"=>1,_=>continue};
        let val=if let Some(hex)=v.strip_prefix("0x"){i64::from_str_radix(hex,16).ok()}else{v.parse::<i64>().ok()}.unwrap_or(1);
        let size=[w,h][axis];
        out[axis]=if size&(size.wrapping_sub(1))!=0{Wrap::Clamp}else{match val{0=>Wrap::Clamp,2=>Wrap::Mirror,_=>Wrap::Repeat}};
    }
    out
}

/// Decode a model into CPU-side buffers.  `source` is a (virtual) path, e.g. `...\world.big::world-low-all.o`.
pub fn build(source:&str,schemas:&model::Schemas)->Result<BuiltModel,String>{build_with(source,schemas,&[])}
/// `extra_banks`: additional texture banks (virtual paths) searched after the model's own sibling banks.
pub fn build_with(source:&str,schemas:&model::Schemas,extra_banks:&[String])->Result<BuiltModel,String>{
    let (data,_)=archive::read_virtual(source)?;
    let mf=model::parse(&data,schemas)?;
    let mut warnings=vec![];
    let mut sources=texture_sources(source);for e in extra_banks{if !sources.contains(e){sources.push(e.clone())}}
    let banks=load_banks(&sources,&mut warnings);
    let mut textures:Vec<BuiltTexture>=vec![];let mut tex_index:HashMap<String,Option<usize>>=HashMap::new();
    let mut materials:Vec<BuiltMaterial>=vec![];let mut prims=vec![];
    let (mut lo,mut hi)=([f32::MAX;3],[f32::MIN;3]);
    for p in &mf.prims{
        // Shadow-volume shaders carry no texture: the original composites them at render time, so they are not drawn here.
        if p.family.contains("Shadow")&&p.textures.is_empty(){continue}
        let apt=p.family.ends_with("Apt");
        let tname=p.textures.get("Texture").cloned();
        let tex=match &tname{
            Some(n)=>*tex_index.entry(n.clone()).or_insert_with(||resolve(n,&banks,&mut warnings).map(|(rgba,w,h)|{
                let alpha=classify_alpha(&rgba);textures.push(BuiltTexture{name:n.clone(),width:w,height:h,rgba,alpha});textures.len()-1})),
            None=>None,
        };
        let (tw,th)=tex.map(|i|(textures[i].width,textures[i].height)).unwrap_or((1,1));
        let wrap=match (tex,p.texture_symbols.get("Texture")){(Some(_),Some(sym))=>wrap_modes(sym,tw,th),_=>[Wrap::Repeat,Wrap::Repeat]};
        let has_colors=p.verts.iter().all(|v|v.clr.is_some())&&!p.verts.is_empty()&&(COLOUR_FAMILIES.contains(&p.family.as_str())||p.family=="PlaygroundTexture_HWSkin");
        let unlit=has_colors||apt;
        let alpha=tex.map(|i|textures[i].alpha).unwrap_or(AlphaKind::Opaque);
        let base=if apt{p.color.map(|c|[c[0] as f32/255.,c[1] as f32/255.,c[2] as f32/255.,c[3] as f32/255.]).unwrap_or([1.;4])}else{[1.;4]};
        let (alpha,double_sided)=if apt{(if base[3]<1.{AlphaKind::Blend}else{alpha},true)}else if alpha==AlphaKind::Mask{(alpha,true)}else{(alpha,false)};
        let mat=BuiltMaterial{texture:tex,wrap,unlit,alpha,double_sided,base_color:base};
        let mi=materials.iter().position(|m|*m==mat).unwrap_or_else(||{materials.push(mat);materials.len()-1});
        let flip=|y:f64|if apt{-y}else{y};
        let positions:Vec<[f32;3]>=p.verts.iter().map(|v|[v.pos[0] as f32,flip(v.pos[1]) as f32,v.pos[2] as f32]).collect();
        for q in &positions{for i in 0..3{lo[i]=lo[i].min(q[i]);hi[i]=hi[i].max(q[i]);}}
        let mut indices:Vec<u32>=p.tris.iter().flat_map(|t|if apt{[t[2],t[1],t[0]]}else{*t}).collect();
        if indices.is_empty(){continue}
        let uvs:Vec<[f32;2]>=p.verts.iter().map(|v|match v.uv{Some(u)=>if apt&&tex.is_some(){[(u[0]/tw as f64) as f32,(u[1]/th as f64) as f32]}else{[u[0] as f32,u[1] as f32]},None=>[0.,0.]}).collect();
        // Normals: stored (S16 Q14) when present, otherwise area-weighted from triangles, shared per position index.
        let normals:Vec<[f32;3]>=if p.verts.iter().all(|v|v.nrm.is_some())&&!p.verts.is_empty(){p.verts.iter().map(|v|v.nrm.unwrap()).collect()}else{
            let mut acc:HashMap<u32,[f64;3]>=HashMap::new();
            for t in &p.tris{
                let (a,b,c)=(p.verts[t[0] as usize].pos,p.verts[t[1] as usize].pos,p.verts[t[2] as usize].pos);
                let (u,v)=([b[0]-a[0],b[1]-a[1],b[2]-a[2]],[c[0]-a[0],c[1]-a[1],c[2]-a[2]]);
                let cr=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]];
                for &i in t{let e=acc.entry(p.verts[i as usize].pos_index).or_insert([0.;3]);for k in 0..3{e[k]+=cr[k]}}
            }
            p.verts.iter().map(|v|{let a=acc.get(&v.pos_index).copied().unwrap_or([0.,1.,0.]);let l=(a[0]*a[0]+a[1]*a[1]+a[2]*a[2]).sqrt();let l=if l==0.{1.}else{l};let n=[(a[0]/l) as f32,(a[1]/l) as f32,(a[2]/l) as f32];if apt{[n[0],-n[1],n[2]]}else{n}}).collect()};
        let colors=has_colors.then(||p.verts.iter().map(|v|{let c=v.clr.unwrap();[c[0] as f32/255.,c[1] as f32/255.,c[2] as f32/255.,c[3] as f32/255.]}).collect());
        let (joints,weights)=if p.verts.iter().all(|v|v.weight.is_some())&&!p.verts.is_empty(){
            (Some(p.verts.iter().map(|v|{let b=v.weight.unwrap().1;[b[0] as u16,b[1] as u16,b[2] as u16,0]}).collect()),Some(p.verts.iter().map(|v|{
                // Weights are renormalised to sum 1 (all-zero -> first bone only), as the reference exporter does.
                let w=v.weight.unwrap().0;let t=w[0]+w[1]+w[2];if t>0.{[w[0]/t,w[1]/t,w[2]/t,0.]}else{[1.,0.,0.,0.]}}).collect()))
        }else{(None,None)};
        let _=&mut indices;
        prims.push(BuiltPrim{family:p.family.clone(),positions,normals,uvs,colors,indices,material:mi,joints,weights});
    }
    let bounds=if prims.is_empty(){[[0.;3];2]}else{[lo,hi]};
    Ok(BuiltModel{prims,materials,textures,warnings,bounds})
}

/// GPU-side handles for a decoded model; can be instantiated many times (props share them).
pub struct Uploaded{pub parts:Vec<(Handle<Mesh>,Handle<StandardMaterial>)>}

fn address(w:Wrap)->ImageAddressMode{match w{Wrap::Clamp=>ImageAddressMode::ClampToEdge,Wrap::Repeat=>ImageAddressMode::Repeat,Wrap::Mirror=>ImageAddressMode::MirrorRepeat}}

/// Turn CPU buffers into Bevy assets.  `force_unlit`/`force_double_sided` are used for the sky layers.
pub fn upload(b:&BuiltModel,meshes:&mut Assets<Mesh>,materials:&mut Assets<StandardMaterial>,images:&mut Assets<Image>,force_unlit:bool)->Uploaded{
    upload_with(b,meshes,materials,images,force_unlit,false)
}
/// Like `upload`, but keeps hardware-skin joint indices/weights on the meshes: only for models spawned with a
/// `SkinnedMesh` (the character rig).  Static instances of skinned models are drawn in bind pose without them.
pub fn upload_skinned(b:&BuiltModel,meshes:&mut Assets<Mesh>,materials:&mut Assets<StandardMaterial>,images:&mut Assets<Image>)->Uploaded{
    upload_with(b,meshes,materials,images,false,true)
}
fn upload_with(b:&BuiltModel,meshes:&mut Assets<Mesh>,materials:&mut Assets<StandardMaterial>,images:&mut Assets<Image>,force_unlit:bool,skinned:bool)->Uploaded{
    let mut image_handles:HashMap<(usize,Wrap,Wrap),Handle<Image>>=HashMap::new();
    let mut mat_handles:Vec<Handle<StandardMaterial>>=vec![];
    for m in &b.materials{
        let tex=m.texture.map(|ti|image_handles.entry((ti,m.wrap[0],m.wrap[1])).or_insert_with(||{
            let t=&b.textures[ti];
            let mut img=Image::new(Extent3d{width:t.width as u32,height:t.height as u32,depth_or_array_layers:1},TextureDimension::D2,t.rgba.clone(),TextureFormat::Rgba8UnormSrgb,RenderAssetUsages::default());
            img.sampler=ImageSampler::Descriptor(ImageSamplerDescriptor{address_mode_u:address(m.wrap[0]),address_mode_v:address(m.wrap[1]),..ImageSamplerDescriptor::linear()});
            images.add(img)}).clone());
        let dbl=m.double_sided||force_unlit;
        mat_handles.push(materials.add(StandardMaterial{
            base_color:Color::srgba(m.base_color[0],m.base_color[1],m.base_color[2],m.base_color[3]),base_color_texture:tex,
            unlit:m.unlit||force_unlit,double_sided:dbl,cull_mode:if dbl{None}else{Some(bevy::render::render_resource::Face::Back)},
            alpha_mode:match m.alpha{AlphaKind::Opaque=>AlphaMode::Opaque,AlphaKind::Mask=>AlphaMode::Mask(0.5),AlphaKind::Blend=>AlphaMode::Blend},
            perceptual_roughness:1.0,metallic:0.0,reflectance:0.0,..default()}));
    }
    let parts=b.prims.iter().map(|p|{
        let mut mesh=Mesh::new(PrimitiveTopology::TriangleList,RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION,p.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL,p.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0,p.uvs.clone());
        if let Some(c)=&p.colors{mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR,c.clone());}
        if let (true,Some(j),Some(w))=(skinned,&p.joints,&p.weights){
            mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX,bevy::mesh::VertexAttributeValues::Uint16x4(j.iter().map(|x|*x).collect()));
            mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT,w.clone());
        }
        mesh.insert_indices(Indices::U32(p.indices.clone()));
        (meshes.add(mesh),mat_handles[p.material].clone())
    }).collect();
    Uploaded{parts}
}

/// Spawn an instance: `root` bundle plus one child entity per primitive.
pub fn spawn(commands:&mut Commands,up:&Uploaded,root:impl Bundle,layers:Option<RenderLayers>)->Entity{
    let parent=commands.spawn((root,Visibility::default())).id();
    if let Some(l)=layers.clone(){commands.entity(parent).insert(l);}
    for (m,mat) in &up.parts{
        let mut e=commands.spawn((Mesh3d(m.clone()),MeshMaterial3d(mat.clone()),Transform::IDENTITY,ChildOf(parent)));
        if let Some(l)=layers.clone(){e.insert(l);}
    }
    parent
}

pub fn summary(m:&BuiltModel)->BTreeMap<String,usize>{
    let mut s=BTreeMap::new();
    s.insert("prims".into(),m.prims.len());s.insert("materials".into(),m.materials.len());s.insert("textures".into(),m.textures.len());
    s.insert("triangles".into(),m.prims.iter().map(|p|p.indices.len()/3).sum());s.insert("warnings".into(),m.warnings.len());s
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn alpha_classes(){
        assert_eq!(classify_alpha(&[1,2,3,255,4,5,6,255]),AlphaKind::Opaque);
        assert_eq!(classify_alpha(&[0,0,0,0,0,0,0,255]),AlphaKind::Mask);
        assert_eq!(classify_alpha(&[0,0,0,128,0,0,0,200]),AlphaKind::Blend);
    }
    #[test] fn wrap_rules(){
        let s="__EAGL::TAR:::RUNTIME_ALLOC::0=1041;1=x,4;22=1;23=2;;24=0";
        assert_eq!(wrap_modes(s,64,64),[Wrap::Repeat,Wrap::Mirror]);
        assert_eq!(wrap_modes(s,60,64),[Wrap::Clamp,Wrap::Mirror]); // NPOT S axis clamps
    }
    /// The world's textures must all resolve (the audit recorded 161 diffuse textures and zero warnings).
    #[test] fn world_builds_with_all_textures(){
        let src=crate::bridge::data_root().join("files/data/world/world.big").to_string_lossy().into_owned()+"::world-low-all.o";
        if !Path::new(&src.split("::").next().unwrap()).exists(){eprintln!("DATA absent; skipped");return}
        let m=build(&src,&model::Schemas::embedded()).unwrap();
        eprintln!("world-low-all: {:?} warnings {:?}",summary(&m),&m.warnings[..m.warnings.len().min(3)]);
        assert_eq!(m.textures.len(),161);assert!(m.warnings.is_empty(),"{:?}",m.warnings);
        assert_eq!(m.prims.iter().map(|p|p.indices.len()/3).sum::<usize>(),100576);
    }
}
