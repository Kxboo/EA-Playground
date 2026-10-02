//! Schema-driven decoder for EAGL `.o` models - Rust port of Remaster/src/model_unified.py.
//!
//! An `.o` model is an ELF relocatable object whose `.data` holds `__Model:::<name>` objects.  Each model lists
//! draw groups of shader primitives (`Model::Draw` 0x803e2c78; +0x9c group count, +0xcc list pointer).  A primitive is a
//! shader struct whose family is named by a relocation; the fields of every family (coordinates, normals, colours,
//! UVs, weights, textures) are (count, pointer) pairs at offsets recovered from the executable
//! (`shader_schemas.json`).  Its +4 field points to PCode (`ProcessPCode` 0x803ef24c) that declares the GX vertex
//! attributes and the display list.  Triangles come from GX strips/fans/lists/quads; the GX winding is reversed to
//! counter-clockwise.  Verified against the Python reference for all 836 corpus models.
use serde_json::Value;
use std::collections::{BTreeMap,HashMap};

fn rd32(d:&[u8],o:usize,le:bool)->Result<u32,String>{let b:[u8;4]=d.get(o..o+4).ok_or_else(||format!("read past end at {o:#x}"))?.try_into().unwrap();Ok(if le{u32::from_le_bytes(b)}else{u32::from_be_bytes(b)})}
fn rd16(d:&[u8],o:usize,le:bool)->Result<u16,String>{let b:[u8;2]=d.get(o..o+2).ok_or("read past end")?.try_into().unwrap();Ok(if le{u16::from_le_bytes(b)}else{u16::from_be_bytes(b)})}
fn be32(d:&[u8],o:usize)->Result<u32,String>{rd32(d,o,false)}
fn bef(d:&[u8],o:usize)->Result<f32,String>{Ok(f32::from_bits(be32(d,o)?))}
fn span(d:&[u8],o:usize,n:usize)->Result<&[u8],String>{d.get(o..o.checked_add(n).ok_or("span overflow")?).ok_or_else(||format!("span {o:#x}+{n:#x} outside {:#x} bytes",d.len()))}
fn cstr(d:&[u8],o:usize)->Result<String,String>{let s=d.get(o..).ok_or("string start")?;let e=s.iter().position(|&c|c==0).ok_or("unterminated string")?;Ok(String::from_utf8_lossy(&s[..e]).into_owned())}

struct Section{name:String,ty:u32,offset:usize,size:usize,link:usize,info:usize,entry_size:usize}
struct Symbol{name:String,value:u32,shndx:u16}
struct Reloc{offset:u32,symbol:usize,table:usize,target:usize}
struct Elf<'a>{data:&'a [u8],sections:Vec<Section>,symbols:HashMap<(usize,usize),Symbol>,relocs:Vec<Reloc>,le:bool}

impl<'a> Elf<'a>{
    fn parse(d:&'a [u8])->Result<Self,String>{
        if d.len()<52||&d[..4]!=b"\x7fELF"||d[4]!=1||!(d[5]==1||d[5]==2){return Err("Expected ELF32 with declared byte order".into())}
        let le=d[5]==1;
        let shoff=rd32(d,32,le)? as usize;let (shentsize,shnum,shstrndx)=(rd16(d,46,le)? as usize,rd16(d,48,le)? as usize,rd16(d,50,le)? as usize);
        if shnum==0||shentsize<40{return Err("Unsupported empty/extended ELF section table".into())}
        let mut secs=vec![];
        for i in 0..shnum{
            let o=shoff+i*shentsize;
            secs.push((rd32(d,o,le)? as usize,Section{name:String::new(),ty:rd32(d,o+4,le)?,offset:rd32(d,o+16,le)? as usize,size:rd32(d,o+20,le)? as usize,link:rd32(d,o+24,le)? as usize,info:rd32(d,o+28,le)? as usize,entry_size:rd32(d,o+36,le)? as usize}));
        }
        if shstrndx>=secs.len(){return Err("Invalid section-name table index".into())}
        let names=span(d,secs[shstrndx].1.offset,secs[shstrndx].1.size)?;
        let mut sections=vec![];
        for (ni,mut s) in secs{s.name=cstr(names,ni)?;sections.push(s);}
        let mut symbols=HashMap::new();let mut relocs=vec![];
        for (si,s) in sections.iter().enumerate(){
            if s.ty==2||s.ty==11{
                let strs=&sections[s.link];let strings=span(d,strs.offset,strs.size)?;
                let step=if s.entry_size==0{16}else{s.entry_size};
                for idx in 0..s.size/step{
                    let o=s.offset+idx*step;
                    symbols.insert((si,idx),Symbol{name:cstr(strings,rd32(d,o,le)? as usize)?,value:rd32(d,o+4,le)?,shndx:rd16(d,o+14,le)?});
                }
            }
        }
        for s in &sections{
            if s.ty==9||s.ty==4{
                let width=if s.ty==9{8}else{12};let step=if s.entry_size==0{width}else{s.entry_size};
                for idx in 0..s.size/step{
                    let o=s.offset+idx*step;let info=rd32(d,o+4,le)?;
                    relocs.push(Reloc{offset:rd32(d,o,le)?,symbol:(info>>8) as usize,table:s.link,target:s.info});
                }
            }
        }
        Ok(Self{data:d,sections,symbols,relocs,le})
    }
    fn section(&self,name:&str)->Option<(usize,&Section)>{self.sections.iter().enumerate().find(|(_,s)|s.name==name)}
}

/// One shader family: field name -> (count offset, pointer offset, element size).
type Fields=HashMap<String,(usize,usize,usize)>;
pub struct Schemas(HashMap<String,Fields>);
impl Schemas{
    pub fn embedded()->Self{
        let v:Value=serde_json::from_str(include_str!("shader_schemas.json")).expect("shader schemas");
        let mut m=HashMap::new();
        for (fam,fields) in v["schemas"].as_object().unwrap(){
            m.insert(fam.clone(),fields.as_array().unwrap().iter().map(|f|(f["name"].as_str().unwrap().to_string(),(f["count_offset"].as_u64().unwrap() as usize,f["pointer_offset"].as_u64().unwrap() as usize,f["element_size"].as_u64().unwrap() as usize))).collect());
        }
        Self(m)
    }
}

/// Bounded reimplementation of `ProcessPCode` (0x803ef24c).
#[derive(Debug,Default)]
pub struct PCode{pub attributes:BTreeMap<u8,(bool,usize)>,pub fraction:Option<u8>,pub offset:usize,pub length:usize,pub array_offsets:BTreeMap<u8,u32>}
fn pcode(raw:&[u8],start:usize,local:&HashMap<usize,usize>)->Result<PCode,String>{
    let mut r=PCode::default();let mut have_list=false;let mut cur=start;
    for _ in 0..128{
        let op=*raw.get(cur).ok_or("Truncated PCode program")?;
        match op{
            0=>{if !have_list{return Err("PCode has no display list".into())}return Ok(r)}
            2=>{let slot=*raw.get(cur+1).ok_or("Truncated PCode")?;r.array_offsets.insert(slot,be32(raw,cur+2)?);cur+=6;}
            3|6=>{cur+=1;let mut n=0;loop{n+=1;if n>64{return Err("Unterminated PCode list".into())}let e=span(raw,cur,2)?;if e[0]==0xff{cur+=2;break}cur+=2;}}
            4=>{span(raw,cur,2)?;cur+=2}
            5=>{cur+=1;let mut n=0;loop{n+=1;if n>64{return Err("Unterminated PCode allocation list".into())}let e=span(raw,cur,3)?;cur+=3;if u16::from_be_bytes([e[0],e[1]])==0{break}}}
            7=>{span(raw,cur,9)?;if have_list{return Err("Multiple PCode display lists not supported".into())}
                r.offset=*local.get(&(cur+1)).ok_or("Missing PCode display-list relocation")?;r.length=be32(raw,cur+5)? as usize;have_list=true;cur+=9}
            8|9|10=>{let attr=*raw.get(cur+1).ok_or("Truncated PCode")?;if r.attributes.contains_key(&attr){return Err("Duplicate PCode attribute".into())}
                r.attributes.insert(attr,if op==10{(true,1)}else{(false,(op-7) as usize)});cur+=2}
            11=>{let f=*raw.get(cur+1).ok_or("Truncated PCode")?;if f>31{return Err("Invalid PCode position fraction".into())}r.fraction=Some(f);cur+=2}
            _=>return Err(format!("Unsupported PCode opcode {op}")),
        }
    }
    Err("Unterminated PCode program".into())
}

#[derive(Debug,Clone,Default)]
pub struct Vertex{pub pos:[f64;3],pub nrm:Option<[f32;3]>,pub clr:Option<[u8;4]>,pub uv:Option<[f64;2]>,pub weight:Option<([f32;3],[u8;3])>,pub pos_index:u32}
#[derive(Debug,Clone)]
pub struct Prim{pub anchor:usize,pub family:String,pub verts:Vec<Vertex>,pub tris:Vec<[u32;3]>,pub textures:BTreeMap<String,String>,pub texture_symbols:BTreeMap<String,String>,pub color:Option<[u8;4]>,
    /// Back-face culling from the primitive's `EAGL::GeoPrimState` (+0x44): property 50 (`SetCullEnable`); the default
    /// state (0x2e050000) leaves culling off.  `None` when the primitive names no state.
    pub cull:Option<bool>}

/// Cull enable of a `__EAGL::GeoPrimState:::RUNTIME_ALLOC::0=642;50=1;48=152;` symbol.
fn state_cull(sym:&str)->bool{
    sym.split(';').filter_map(|p|p.strip_prefix("50=")).any(|v|v.trim().parse::<i64>().map(|n|n!=0).unwrap_or(false))
}
#[derive(Debug,Clone)]
pub struct ModelInfo{pub name:String,pub offset:usize,pub bounds:[[f32;3];2],pub scale:[f32;3],pub center:[f32;3],pub prims:Vec<usize>}
pub struct ModelFile{pub models:Vec<ModelInfo>,pub prims:Vec<Prim>}

const APT:[&str;2]=["TextureApt","GouraudApt"];

/// GX draw command -> triangle slot triples over a vertex list of `n` entries (GX strip winding alternates).
fn faces_of(mode:u8,n:usize)->Vec<[usize;3]>{
    match mode{
        0x98=>(0..n.saturating_sub(2)).map(|i|if i%2==0{[i,i+1,i+2]}else{[i+1,i,i+2]}).collect(),
        0xa0=>(1..n.saturating_sub(1)).map(|i|[0,i,i+1]).collect(),
        0x90=>(0..n-n%3).step_by(3).map(|i|[i,i+1,i+2]).collect(),
        _=>(0..n-n%4).step_by(4).flat_map(|i|[[i,i+1,i+2],[i,i+2,i+3]]).collect(),
    }
}

fn read_texture_name(sym:&str)->String{
    // Symbols look like `__EAGL::TAR:::RUNTIME_ALLOC::0=1041;1=name,4;22=1;23=1;;24=0`.
    for part in sym.split(';'){if let Some(rest)=part.strip_prefix("1="){if let Some(end)=rest.find(','){return rest[..end].to_string()}}}
    sym.to_string()
}

pub fn parse(data:&[u8],schemas:&Schemas)->Result<ModelFile,String>{
    let elf=Elf::parse(data)?;
    let Some((di,ds))=elf.section(".data") else{return Ok(ModelFile{models:vec![],prims:vec![]})};
    let raw=span(data,ds.offset,ds.size)?;
    let mut local:HashMap<usize,usize>=HashMap::new();let mut external:HashMap<usize,String>=HashMap::new();
    for r in &elf.relocs{
        if r.target!=di{continue}
        let Some(sym)=elf.symbols.get(&(r.table,r.symbol)) else{continue};
        let off=r.offset as usize;
        if sym.shndx as usize==di{local.insert(off,rd32(raw,off,elf.le)? as usize+sym.value as usize);}
        else if sym.shndx==0{external.insert(off,sym.name.clone());}
    }
    if std::env::var("EAGL_MODEL_SYMS").is_ok(){let mut n:Vec<&String>=external.values().collect();n.sort();n.dedup();for s in n{if !s.contains("TAR:::"){eprintln!("[sym] {s}");}}}
    let mut model_syms:Vec<(&Symbol,usize)>=elf.symbols.iter().filter(|(_,s)|s.name.starts_with("__Model:::")).map(|((_,i),s)|(s,*i)).collect();
    model_syms.sort_by_key(|(s,i)|(s.value,*i));
    let mut prims:Vec<Prim>=vec![];let mut models=vec![];let mut seen:HashMap<usize,usize>=HashMap::new();
    for (sym,_) in model_syms{
        let m=sym.value as usize;let count=be32(raw,m+0x9c)? as usize;
        if count>65536{return Err("Invalid model geometry count".into())}
        let mut anchors=vec![];
        if count>0{
            let mut cursor=*local.get(&(m+0xcc)).ok_or("missing draw-list relocation")?+4;
            for _ in 0..count{
                let size=be32(raw,cursor)? as usize;cursor+=4;
                if size>65536{return Err("Invalid model primitive count".into())}
                for _ in 0..size{anchors.push(*local.get(&cursor).ok_or("missing primitive relocation")?);cursor+=4;}
            }
        }
        let mut idxs=vec![];
        for a in anchors{
            let pi=if let Some(&p)=seen.get(&a){p}else{
                let p=decode_primitive(raw,&local,&external,a,m,schemas)?;prims.push(p);seen.insert(a,prims.len()-1);prims.len()-1};
            idxs.push(pi);
        }
        let b=|o:usize|->Result<[f32;3],String>{Ok([bef(raw,m+o)?,bef(raw,m+o+4)?,bef(raw,m+o+8)?])};
        models.push(ModelInfo{name:sym.name.split_once(":::").map(|x|x.1.to_string()).unwrap_or_default(),offset:m,bounds:[b(0x6c)?,b(0x7c)?],scale:b(0x4c)?,center:b(0x5c)?,prims:idxs});
    }
    Ok(ModelFile{models,prims})
}

fn decode_primitive(raw:&[u8],local:&HashMap<usize,usize>,external:&HashMap<usize,String>,anchor:usize,model:usize,schemas:&Schemas)->Result<Prim,String>{
    let family=external.get(&anchor).ok_or_else(||format!("primitive {anchor:#x} has no shader family"))?.clone();
    let fields=schemas.0.get(&family).ok_or_else(||format!("shader family {family:?} has no schema"))?;
    let prog=pcode(raw,*local.get(&(anchor+4)).ok_or("missing PCode relocation")?,local)?;
    let array=|name:&str|->Result<Option<(usize,&[u8],usize)>,String>{
        let Some(&(co,po,es))=fields.get(name) else{return Ok(None)};
        let n=be32(raw,anchor+co)? as usize;
        if !(0<n&&n<=1_000_000){return Err(format!("{family}: bad {name} count {n}"))}
        let p=*local.get(&(anchor+po)).ok_or_else(||format!("{family}: {name} pointer missing"))?;
        Ok(Some((n,span(raw,p,n*es)?,es)))
    };
    let coords=array("Coordinates")?.ok_or("no Coordinates field")?;
    let is_apt=APT.contains(&family.as_str());let mut uv_apt:Vec<[f64;2]>=vec![];let mut color=None;
    let positions:Vec<[f64;3]>=if is_apt{
        let scale=[bef(raw,model+0x4c)?,bef(raw,model+0x50)?,bef(raw,model+0x54)?];let center=[bef(raw,model+0x5c)?,bef(raw,model+0x60)?,bef(raw,model+0x64)?];
        let p:Vec<[f64;3]>=coords.1.chunks_exact(6).map(|c|{let v=[i16::from_be_bytes([c[0],c[1]]),i16::from_be_bytes([c[2],c[3]]),i16::from_be_bytes([c[4],c[5]])];
            [0,1,2].map(|i|v[i] as f64/32768.0*scale[i] as f64+center[i] as f64)}).collect();
        if let Some(&(_,po,_))=fields.get("GeomName_TextureMatrix"){
            let mp=*local.get(&(anchor+po)).ok_or("APT texture matrix pointer missing")?;let mut mat=[0f32;16];for i in 0..16{mat[i]=bef(raw,mp+4*i)?;}
            uv_apt=p.iter().map(|q|[0,1].map(|j|(0..3).map(|k|q[k]*mat[k*4+j] as f64).sum::<f64>()+mat[12+j] as f64)).collect();
        }
        if let Some(&(_,po,_))=fields.get("MatDiffuseColour"){let cp=*local.get(&(anchor+po)).ok_or("APT colour pointer missing")?;color=Some(span(raw,cp,4)?.try_into().unwrap());}
        p
    }else if coords.2==12{
        coords.1.chunks_exact(12).map(|c|[0,1,2].map(|i|f32::from_bits(u32::from_be_bytes(c[4*i..4*i+4].try_into().unwrap())) as f64)).collect()
    }else if coords.2==6{
        let f=prog.fraction.ok_or("compressed coordinates without a position fraction")?;let div=(1u64<<f) as f64;
        coords.1.chunks_exact(6).map(|c|[0,1,2].map(|i|i16::from_be_bytes([c[2*i],c[2*i+1]]) as f64/div)).collect()
    }else{return Err(format!("unexpected coordinate size {}",coords.2))};
    let normals:Vec<[f32;3]>=match array("Normals")?{Some((_,b,_))=>b.chunks_exact(6).map(|c|[0,1,2].map(|i|i16::from_be_bytes([c[2*i],c[2*i+1]]) as f32/16384.0)).collect(),None=>vec![]};
    let colors:Vec<[u8;4]>=match array("Colours")?{Some((_,b,_))=>b.chunks_exact(4).map(|c|[c[0],c[1],c[2],c[3]]).collect(),None=>vec![]};
    let uvs:Vec<[f64;2]>=if is_apt{uv_apt}else{match array("UVs")?{Some((_,b,_))=>b.chunks_exact(8).map(|c|[f32::from_bits(u32::from_be_bytes(c[0..4].try_into().unwrap())) as f64,f32::from_bits(u32::from_be_bytes(c[4..8].try_into().unwrap())) as f64]).collect(),None=>vec![]}};
    let weights:Vec<([f32;3],[u8;3])>=match array("Weights")?{Some((_,b,_))=>b.chunks_exact(16).map(|w|([0,1,2].map(|i|f32::from_bits(u32::from_be_bytes(w[4*i..4*i+4].try_into().unwrap()))),[w[3],w[7],w[11]])).collect(),None=>vec![]};

    let stride:usize=prog.attributes.values().map(|&(_,w)|w).sum();
    let stream=span(raw,prog.offset,prog.length)?;
    let mut lookup:HashMap<Vec<u32>,u32>=HashMap::new();let mut verts:Vec<Vertex>=vec![];let mut tris:Vec<[u32;3]>=vec![];
    let mut cur=0usize;
    while cur<stream.len(){
        let op=stream[cur];cur+=1;if op==0{continue}
        let mode=op&0xf8;
        if !matches!(mode,0x80|0x90|0x98|0xa0){return Err(format!("unknown GX opcode {op:02x}"))}
        let n=u16::from_be_bytes(span(stream,cur,2)?.try_into().unwrap()) as usize;cur+=2;
        let blob=span(stream,cur,n*stride)?;cur+=n*stride;
        let mut idx=Vec::with_capacity(n);
        for j in 0..n{
            let mut off=j*stride;let mut key=Vec::with_capacity(prog.attributes.len()*2);let mut d:HashMap<u8,u32>=HashMap::new();
            for (&attr,&(_,w)) in &prog.attributes{let v=blob[off..off+w].iter().fold(0u32,|a,&b|(a<<8)|b as u32);off+=w;d.insert(attr,v);key.push(attr as u32);key.push(v);}
            if let Some(&i)=lookup.get(&key){idx.push(i);continue}
            for (a,len) in [(9u8,positions.len()),(10,normals.len()),(11,colors.len())]{if let Some(&v)=d.get(&a){if a==10&&normals.is_empty(){continue}if v as usize>=len{return Err(format!("attribute {a} index {v} out of bounds"))}}}
            let pi=*d.get(&9).ok_or("display list has no position attribute")?;
            let vert=Vertex{pos:positions[pi as usize],
                nrm:d.get(&10).and_then(|&i|normals.get(i as usize).copied()),clr:d.get(&11).map(|&i|colors[i as usize]),
                uv:if is_apt{uvs.get(pi as usize).copied()}else{d.get(&13).and_then(|&i|uvs.get(i as usize).copied())},
                weight:d.get(&0).and_then(|&s|weights.get((s/3) as usize).copied()),pos_index:pi};
            let id=verts.len() as u32;lookup.insert(key,id);verts.push(vert);idx.push(id);
        }
        for f in faces_of(mode,n){
            let (a,b,c)=(idx[f[0]],idx[f[1]],idx[f[2]]);
            let (pa,pb,pc)=(verts[a as usize].pos_index,verts[b as usize].pos_index,verts[c as usize].pos_index);
            if pa!=pb&&pb!=pc&&pa!=pc{tris.push([a,c,b])} // GX winding reversed to CCW
        }
    }
    let mut textures=BTreeMap::new();let mut texture_symbols=BTreeMap::new();
    for name in ["Texture","Texture1","Texture2","Texture3"]{
        if let Some(&(_,po,_))=fields.get(name){if let Some(sym)=external.get(&(anchor+po)){textures.insert(name.to_string(),read_texture_name(sym));texture_symbols.insert(name.to_string(),sym.clone());}}
    }
    let cull=external.get(&(anchor+0x44)).filter(|n|n.contains("GeoPrimState")).map(|n|state_cull(n));
    Ok(Prim{anchor,family,verts,tris,textures,texture_symbols,color,cull})
}

/// Content hash used by the golden data: first 16 hex chars of SHA-256 over vertex attributes then triangles.
pub fn prim_hash(p:&Prim)->String{
    let mut b:Vec<u8>=vec![];
    for v in &p.verts{
        for x in v.pos{b.extend_from_slice(&(x as f32).to_be_bytes());}
        if let Some(n)=v.nrm{for x in n{b.extend_from_slice(&x.to_be_bytes());}}
        if let Some(c)=v.clr{b.extend_from_slice(&c);}
        if let Some(u)=v.uv{for x in u{b.extend_from_slice(&(x as f32).to_be_bytes());}}
    }
    for t in &p.tris{for i in t{b.extend_from_slice(&i.to_be_bytes());}}
    crate::sha256::hex(&b)[..16].to_string()
}

#[cfg(test)]
mod tests{
    use super::*;
    /// Every primitive of every corpus model must match the Python reference (counts, textures and content hash).
    #[test]
    fn corpus_matches_python_reference(){
        let cov=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Remaster/research/coverage.json");
        let Ok(txt)=std::fs::read_to_string(&cov) else{eprintln!("coverage.json absent; skipped");return};
        let j:Value=serde_json::from_str(&txt).unwrap();let recs=j["records"].as_array().unwrap();
        if !std::path::Path::new(recs[0]["source"].as_str().unwrap()).exists(){eprintln!("DATA absent; skipped");return}
        let golden:Value=serde_json::from_str(include_str!("../tests/data/model_golden.json")).unwrap();
        let schemas=Schemas::embedded();
        let mut seen=std::collections::HashSet::new();let (mut models,mut prims,mut bad)=(0,0,vec![]);
        for r in recs{
            if r["extension"]!=".o"{continue}
            let h=r["sha256"].as_str().unwrap();if !seen.insert(h.to_string()){continue}
            let (data,_)=crate::archive::read_virtual(r["source"].as_str().unwrap()).unwrap();
            let want=&golden[h];let name=r["source"].as_str().unwrap();
            let mf=match parse(&data,&schemas){Ok(m)=>m,Err(e)=>{bad.push(format!("{name}: {e}"));continue}};models+=1;
            let wp=want["prims"].as_array().unwrap();
            if mf.prims.len()!=wp.len(){bad.push(format!("{name}: {} prims vs {}",mf.prims.len(),wp.len()));continue}
            for (p,w) in mf.prims.iter().zip(wp){
                prims+=1;
                let tex:BTreeMap<String,String>=w[5].as_object().unwrap().iter().map(|(k,v)|(k.clone(),v.as_str().unwrap().to_string())).collect();
                let ok=format!("{:#x}",p.anchor)==w[0].as_str().unwrap()&&p.family==w[1].as_str().unwrap()&&p.verts.len() as u64==w[2].as_u64().unwrap()&&p.tris.len() as u64==w[3].as_u64().unwrap()&&prim_hash(p)==w[4].as_str().unwrap()&&p.textures==tex;
                if !ok{bad.push(format!("{name}: prim {:#x} {} v{}/{} t{}/{} hash {} vs {} tex {:?} vs {:?}",p.anchor,p.family,p.verts.len(),w[2],p.tris.len(),w[3],prim_hash(p),w[4],p.textures,tex));break}
            }
        }
        eprintln!("model corpus: {models} models, {prims} primitives compared");
        assert!(bad.is_empty(),"{} mismatches, first: {:?}",bad.len(),&bad[..bad.len().min(4)]);
        assert!(models>=836&&prims>=12000);
    }
    #[test] fn rejects_non_elf(){assert!(Elf::parse(b"not an elf file at all, not even close to one").is_err())}
}
