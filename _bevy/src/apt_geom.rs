//! Geometry of an APT movie: the `.o` beside the `.apt` holds one `__Model:::<id>` per shape character.
//! Positions stay in movie space (pixels, y down); UVs are normalised against the resolved GSH texture.
use crate::{archive,assets::{self,AlphaKind,Wrap},model};
use std::collections::HashMap;

pub struct Tex{pub name:String,pub width:usize,pub height:usize,pub rgba:Vec<u8>,pub alpha:AlphaKind}
pub struct GeomPrim{pub positions:Vec<[f32;2]>,pub uvs:Vec<[f32;2]>,pub indices:Vec<u32>,pub texture:Option<usize>,pub wrap:[Wrap;2],pub color:[f32;4]}
#[derive(Default)]
pub struct Geometry{pub textures:Vec<Tex>,pub shapes:HashMap<u32,Vec<GeomPrim>>,pub warnings:Vec<String>}

/// `source` is the virtual path of the `.o` (e.g. `...\TetherballHud.big::TetherballHud.o`).
pub fn load(source:&str,schemas:&model::Schemas)->Result<Geometry,String>{
    let (data,_)=archive::read_virtual(source)?;
    let mf=model::parse(&data,schemas)?;
    let mut g=Geometry::default();
    let sources=assets::texture_sources(source);
    let banks=assets::load_banks(&sources,&mut g.warnings);
    let mut tex_index:HashMap<String,Option<usize>>=HashMap::new();
    for m in &mf.models{
        let Ok(id)=m.name.parse::<u32>() else{continue};
        let mut out=vec![];
        for &pi in &m.prims{
            let p=&mf.prims[pi];
            let tname=p.textures.get("Texture").cloned();
            let tex=match &tname{
                Some(n)=>*tex_index.entry(n.clone()).or_insert_with(||assets::resolve(n,&banks,&mut g.warnings).map(|(rgba,w,h)|{
                    let alpha=assets::classify_alpha(&rgba);g.textures.push(Tex{name:n.clone(),width:w,height:h,rgba,alpha});g.textures.len()-1})),
                None=>None,
            };
            let (tw,th)=tex.map(|i|(g.textures[i].width,g.textures[i].height)).unwrap_or((1,1));
            let wrap=match (tex,p.texture_symbols.get("Texture")){(Some(_),Some(sym))=>assets::wrap_modes(sym,tw,th),_=>[Wrap::Repeat,Wrap::Repeat]};
            let positions:Vec<[f32;2]>=p.verts.iter().map(|v|[v.pos[0] as f32,v.pos[1] as f32]).collect();
            let uvs:Vec<[f32;2]>=p.verts.iter().map(|v|match v.uv{Some(u)=>if tex.is_some(){[(u[0]/tw as f64) as f32,(u[1]/th as f64) as f32]}else{[u[0] as f32,u[1] as f32]},None=>[0.,0.]}).collect();
            let indices:Vec<u32>=p.tris.iter().flat_map(|t|[t[0],t[1],t[2]]).collect();
            if indices.is_empty(){continue}
            let color=p.color.map(|c|[c[0] as f32/255.,c[1] as f32/255.,c[2] as f32/255.,c[3] as f32/255.]).unwrap_or([1.;4]);
            out.push(GeomPrim{positions,uvs,indices,texture:tex,wrap,color});
        }
        g.shapes.insert(id,out);
    }
    Ok(g)
}
