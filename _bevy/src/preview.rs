//! Native (pure Rust) previews for the asset viewer: models, skeletons, animation banks and GSH texture banks are
//! decoded on a worker thread with the same decoders the game uses.  Anything else (or a file the Rust decoders
//! reject) returns `FALLBACK` and the viewer asks the Python inspection worker instead.
use crate::{anim::{Bank,Clip},archive,assets,gsh,model,skeleton::Skeleton};
use std::time::Instant;

pub const FALLBACK:&str="fallback";

pub struct Request{pub source:String,pub index:usize,pub skeleton:Option<String>,pub model:Option<String>,pub bank:Option<String>,pub textures:Vec<String>,pub with_model:bool,pub with_animation:bool}

pub struct Rig{pub skeleton:Skeleton,pub clip:Option<Clip>}
pub enum Output{
    Image{name:String,rgba:Vec<u8>,w:usize,h:usize,items:Vec<(usize,String)>},
    /// A model, optionally skinned onto a rig (bind pose, or playing `clip`).
    Model{name:String,built:assets::BuiltModel,rig:Option<Rig>,items:Vec<(usize,String)>,frontend:bool,ms:f64},
    /// Skeleton (+ clip) without geometry.
    Skeleton{name:String,rig:Rig,items:Vec<(usize,String)>,ms:f64},
}

fn ext(s:&str)->String{s.rsplit("::").next().unwrap_or(s).rsplit('.').next().unwrap_or("").to_lowercase()}
fn file_name(s:&str)->String{s.rsplit("::").next().unwrap_or(s).rsplit(['\\','/']).next().unwrap_or(s).to_string()}
fn load_skeleton(source:&str)->Result<Skeleton,String>{Skeleton::parse(&archive::read_virtual(source)?.0)}
fn load_bank(source:&str)->Result<Bank,String>{Bank::parse(archive::read_virtual(source)?.0)}
fn clip_items(b:&Bank)->Vec<(usize,String)>{b.names.iter().cloned().enumerate().collect()}
fn decode_clip(bank:&Bank,index:usize,skel:&Skeleton)->Result<Clip,String>{bank.decode(index,skel)}

pub fn run(r:&Request)->Result<Output,String>{
    let started=Instant::now();let ms=||started.elapsed().as_secs_f64()*1000.;
    let name=file_name(&r.source);
    match ext(&r.source).as_str(){
        "gsh"=>{
            let (d,_)=archive::read_virtual(&r.source)?;
            if d.get(..4)!=Some(b"SHPG"){return Err(FALLBACK.into())}
            let g=gsh::parse(&d)?;
            let e=g.entries.get(r.index).ok_or("texture index out of range")?;
            let (rgba,w,h)=gsh::decode(e,&d)?;
            if w*h>16_777_216{return Err("Image exceeds interactive pixel limit".into())}
            let items=g.entries.iter().map(|e|(e.index,e.full_name.clone().unwrap_or_else(||e.name.clone()))).collect();
            Ok(Output::Image{name:format!("{name} ({})",e.full_name.clone().unwrap_or_else(||e.name.clone())),rgba,w,h,items})
        }
        "ske"=>Ok(Output::Skeleton{name,rig:Rig{skeleton:load_skeleton(&r.source)?,clip:None},items:vec![],ms:ms()}),
        "anm"=>{
            let skel_src=r.skeleton.as_ref().ok_or("Choose the matching skeleton to preview this animation bank")?;
            let skeleton=load_skeleton(skel_src)?;let bank=load_bank(&r.source)?;
            let clip=decode_clip(&bank,r.index,&skeleton)?;let items=clip_items(&bank);
            match r.model.as_ref().filter(|_|r.with_model){
                Some(m)=>model_output(m,&r.textures,Some(skeleton),Some(clip),items,ms()),
                None=>Ok(Output::Skeleton{name:format!("{name} — {}",clip.name),rig:Rig{skeleton,clip:Some(clip)},items,ms:ms()}),
            }
        }
        "o"=>{
            let skeleton=match &r.skeleton{Some(s)=>Some(load_skeleton(s)?),None=>None};
            let (clip,items)=match (&r.bank,r.with_animation,&skeleton){
                (Some(b),true,Some(sk))=>{let bank=load_bank(b)?;(Some(decode_clip(&bank,r.index,sk)?),clip_items(&bank))}
                _=>(None,vec![]),
            };
            model_output(&r.source,&r.textures,skeleton,clip,items,ms())
        }
        _=>Err(FALLBACK.into()),
    }
}

fn model_output(source:&str,textures:&[String],skeleton:Option<Skeleton>,clip:Option<Clip>,items:Vec<(usize,String)>,ms:f64)->Result<Output,String>{
    let data=archive::read_virtual(source)?.0;
    let mf=model::parse(&data,&model::Schemas::embedded())?;
    // Frontend (menu) files hold several selectable shapes: keep those on the inspection path for now.
    if mf.models.len()>1&&mf.prims.iter().any(|p|p.family.ends_with("Apt")){return Err(FALLBACK.into())}
    let built=assets::build_with(source,&model::Schemas::embedded(),textures)?;
    if built.prims.is_empty(){return Err(FALLBACK.into())}
    let frontend=mf.prims.iter().any(|p|p.family.ends_with("Apt"));
    let rig=match skeleton{
        Some(sk)=>{
            let max=built.prims.iter().filter_map(|p|p.joints.as_ref()).flatten().flat_map(|j|j[..3].iter().copied()).max();
            match max{
                Some(m) if m as usize>=sk.bones.len()=>return Err("Selected skeleton has too few bones for this model; choose a matching skeleton".into()),
                Some(_)=>Some(Rig{skeleton:sk,clip}),
                None=>None,
            }
        }
        None=>None,
    };
    Ok(Output::Model{name:file_name(source),built,rig,items,frontend,ms})
}
