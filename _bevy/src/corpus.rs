//! `--decode-all`: decode the complete game DATA tree with the native Rust decoders only.
//!
//! Every loose file is read, RefPack-decompressed and recursively expanded (BIG/VIV/U8).  Each record (loose file or
//! archive member) is dispatched to its format decoder and the results are written below `--out`:
//! `<virtual path with :: as />.<suffix>` (PNG images, binary glTF models/rigs/animations, WAV audio, OBJ collision,
//! JSON structures, UTF-8 text).  Identical contents are decoded once and later copies point at the first.
//! `report.json` / `REPORT.md` list every record with its status:
//! * `decoded`   - the decoder consumed the record and produced its outputs,
//! * `partial`   - decoded, but the decoder reports an explicit unresolved part (named in `detail`),
//! * `container` - an archive; its members are separate records,
//! * `failed`    - a decoder exists but rejected the record (error in `detail`),
//! * `unsupported` - no decoder for this format.
use crate::{anim,apt_dump,archive,assets,audio,conga,conversation,export,gsh,havok,locale,model,placement,sha256,skeleton::Skeleton,tpl,vlt,formats};
use serde_json::{json,Value};
use std::collections::{BTreeMap,HashMap};
use std::path::{Path,PathBuf};

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug)]
pub enum Status{Decoded,Partial,Container,Failed,Unsupported}
impl Status{pub fn name(self)->&'static str{match self{Status::Decoded=>"decoded",Status::Partial=>"partial",Status::Container=>"container",Status::Failed=>"failed",Status::Unsupported=>"unsupported"}}}

pub struct Out{pub format:String,pub status:Status,pub detail:String,pub files:Vec<(String,Vec<u8>)>}
impl Out{
    pub fn new(format:&str)->Self{Out{format:format.into(),status:Status::Decoded,detail:String::new(),files:vec![]}}
    pub fn json(mut self,suffix:&str,v:&Value)->Self{self.files.push((suffix.into(),serde_json::to_vec_pretty(v).unwrap()));self}
    pub fn file(mut self,suffix:&str,b:Vec<u8>)->Self{self.files.push((suffix.into(),b));self}
    pub fn partial(mut self,why:impl Into<String>)->Self{self.status=Status::Partial;self.detail=why.into();self}
    pub fn failed(format:&str,e:impl Into<String>)->Self{Out{format:format.into(),status:Status::Failed,detail:e.into(),files:vec![]}}
    pub fn unsupported(format:&str,why:&str)->Self{Out{format:format.into(),status:Status::Unsupported,detail:why.into(),files:vec![]}}
}

/// Records of the loose file currently being decoded (the file itself and all nested members).
pub struct Ctx<'a>{pub members:&'a HashMap<String,usize>,pub records:&'a [(String,String,Vec<u8>)],pub english:Option<&'a locale::Locale>,pub schemas:&'a model::Schemas,pub havok:&'a havok::ClassTable}
impl<'a> Ctx<'a>{
    /// A file beside `src`: in the same archive directory, or in the same folder on disk.
    pub fn sibling(&self,src:&str,name:&str)->Option<Vec<u8>>{
        match src.rsplit_once("::"){
            Some((parent,member))=>{
                let dir=member.rsplit_once(['/','\\']).map(|(d,_)|format!("{d}/")).unwrap_or_default();
                self.members.get(&format!("{parent}::{dir}{name}")).map(|&i|self.records[i].2.clone())
            }
            None=>std::fs::read(Path::new(src).with_file_name(name)).ok().and_then(|d|archive::decompress(&d).ok()),
        }
    }
    /// Names of the members in the same archive directory as `src` (empty for loose files).
    pub fn sibling_names(&self,src:&str)->Vec<String>{
        let Some((parent,member))=src.rsplit_once("::") else{
            return std::fs::read_dir(Path::new(src).parent().unwrap_or(Path::new("."))).map(|r|r.flatten().map(|e|e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
        };
        let dir=member.rsplit_once(['/','\\']).map(|(d,_)|format!("{d}/")).unwrap_or_default();
        let pre=format!("{parent}::{dir}");
        self.members.keys().filter_map(|k|k.strip_prefix(&pre)).filter(|r|!r.contains("::")&&!r.contains('/')).map(String::from).collect()
    }
}

fn ext_of(name:&str)->String{name.rsplit('/').next().unwrap_or(name).rsplit_once('.').map(|(_,e)|e.to_lowercase()).unwrap_or_default()}
fn stem_of(name:&str)->String{let n=name.rsplit(['/','\\']).next().unwrap_or(name);n.rsplit_once('.').map(|(s,_)|s.to_string()).unwrap_or(n.to_string())}
pub fn safe(s:&str)->String{
    let t:String=s.chars().map(|c|if "<>:\"|?*\\".contains(c)||(c as u32)<32{'_'}else{c}).collect();
    let t=t.trim_end_matches(['.',' ']).to_string();if t.is_empty(){"_".into()}else{t}
}

fn text(d:&[u8])->String{
    if d.starts_with(&[0xff,0xfe]){String::from_utf16_lossy(&d[2..].chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).collect::<Vec<_>>())}
    else{match std::str::from_utf8(d){Ok(s)=>s.to_string(),Err(_)=>d.iter().map(|&b|b as char).collect()}}
}

fn wav_stream(d:&[u8])->Result<audio::Pcm,String>{
    match audio::parse_header(d).map(|h|h.0.codec){Ok(0x0a)=>audio::decode_xa(d),Ok(0x04)=>audio::decode_utk(d),Ok(_)=>audio::decode(d,None),Err(e)=>Err(e)}
}

/// Decode one record.  `src` is its virtual path, `name` the member (or file) name.
pub fn decode(ctx:&Ctx,src:&str,name:&str,d:&[u8])->Out{
    let ext=ext_of(name);
    if let Some(Ok(entries))=archive::entries(d){
        if !name.to_lowercase().ends_with(".bh"){
            let kind=if d.starts_with(b"U\xaa8-"){"Nintendo U8 archive"}else{"EA BIG archive"};
            let mut o=Out::new(kind).json("json",&json!({"format":kind,"entries":entries.iter().map(|e|json!({"name":e.name,"offset":e.offset,"size":e.size})).collect::<Vec<_>>()}));
            o.status=Status::Container;o.detail=format!("{} members",entries.len());return o;
        }
    }
    match ext.as_str(){
        "gsh"=>{
            let g=match gsh::parse(d){Ok(g)=>g,Err(e)=>return Out::failed("EA GSH texture bank",e)};
            let mut o=Out::new("EA GSH texture bank");let mut bad=vec![];
            for e in &g.entries{
                let label=e.full_name.clone().unwrap_or_else(||e.name.clone());
                match gsh::decode(e,d).and_then(|(rgba,w,h)|export::png(&rgba,w,h)){Ok(p)=>o=o.file(&format!("png/{:03}_{}.png",e.index,safe(label.trim())),p),Err(err)=>bad.push(format!("{}: {err}",label))}
            }
            o=o.json("json",&json!({"entries":g.entries.iter().map(|e|json!({"index":e.index,"name":e.name,"full_name":e.full_name})).collect::<Vec<_>>()}));
            if bad.is_empty(){o}else{o.partial(format!("{} of {} images failed: {}",bad.len(),g.entries.len(),bad.join("; ")))}
        }
        "tpl"=>{
            let es=match tpl::parse(d){Ok(e)=>e,Err(e)=>return Out::failed("Nintendo TPL",e)};
            let mut o=Out::new("Nintendo TPL");let mut bad=vec![];
            for e in &es{match tpl::decode(d,e).and_then(|(rgba,w,h)|export::png(&rgba,w,h)){Ok(p)=>o=o.file(&format!("png/{:03}_{}.png",e.index,e.format_name),p),Err(err)=>bad.push(format!("{}: {err}",e.index))}}
            if bad.is_empty(){o}else{o.partial(bad.join("; "))}
        }
        "o"=>{
            let mf=match model::parse(d,ctx.schemas){Ok(m)=>m,Err(e)=>return Out::failed("EAGL model (.o)",e)};
            let info=json!({"models":mf.models.iter().map(|m|json!({"name":m.name,"bounds":m.bounds,"scale":m.scale,"center":m.center,"primitives":m.prims})).collect::<Vec<_>>(),
                "primitives":mf.prims.iter().map(|p|json!({"family":p.family,"vertices":p.verts.len(),"triangles":p.tris.len(),"textures":p.textures})).collect::<Vec<_>>()});
            let o=Out::new("EAGL model (.o)").json("json",&info);
            if mf.prims.is_empty(){return o}
            match assets::build(src,ctx.schemas).and_then(|b|export::glb_model(&b,&stem_of(name)).map(|g|(g,b.warnings))){
                Ok((g,w))=>{let o=o.file("glb",g);if w.is_empty(){o}else{o.partial(format!("material warnings: {}",w.join("; ")))}}
                Err(e)=>Out{status:Status::Failed,detail:e,..o},
            }
        }
        "ske"=>match Skeleton::parse(d){
            Ok(s)=>Out::new("EAGL skeleton").json("json",&json!({"flags":s.flags,"bones":s.bones.iter().map(|b|json!({"index":b.index,"name":b.name,"parent":b.parent,"rotation":b.quat,"translation":b.trans,"scale":b.scale,"world_translation":b.world_translation})).collect::<Vec<_>>()}))
                .file("glb",export::glb_rig(&s,&[],&stem_of(name))),
            Err(e)=>Out::failed("EAGL skeleton",e),
        },
        "anm"=>{
            let skes:Vec<String>=ctx.sibling_names(src).into_iter().filter(|n|n.to_lowercase().ends_with(".ske")).collect();
            let skel=match skes.as_slice(){[one]=>ctx.sibling(src,one).ok_or("sibling skeleton unreadable".to_string()).and_then(|b|Skeleton::parse(&b)),_=>Err(format!("{} candidate skeletons beside the bank",skes.len()))};
            let skel=match skel{Ok(s)=>s,Err(e)=>return Out::failed("EAGL animation bank",e)};
            let bank=match anim::Bank::parse(d.to_vec()){Ok(b)=>b,Err(e)=>return Out::failed("EAGL animation bank",e)};
            let (mut clips,mut bad)=(vec![],vec![]);
            for i in 0..bank.blocks.len(){match bank.decode(i,&skel){Ok(c)=>clips.push(c),Err(e)=>bad.push(json!({"index":i,"name":bank.names.get(i),"error":e}))}}
            let summary=json!({"skeleton":skes[0],"clips":clips.iter().map(|c|json!({"index":c.index,"name":c.name,"samples":c.sample_count,"codec":c.codec,"caveats":c.caveats})).collect::<Vec<_>>(),"failed":bad});
            let o=Out::new("EAGL animation bank").json("json",&summary).file("glb",export::glb_rig(&skel,&clips,&stem_of(name)));
            // Clip caveats record engine behaviour (sBoneMask gating, empty tracks, unwritten pose words); only clips
            // that fail to decode make the bank partial.
            if !bad.is_empty(){o.partial(format!("{} of {} clips undecoded",bad.len(),bank.blocks.len()))}else{o}
        }
        "hkx"=>{
            let pf0=match havok::Packfile::parse(d,ctx.havok){Ok(p)=>p,Err(e)=>return Out::failed("Havok packfile",e)};
            // Classes the executable does not reflect come from the file's own __types__ section when present.
            let (table,added)=ctx.havok.with_file_types(&pf0);
            let pf=match havok::Packfile::parse(d,&table){Ok(p)=>p,Err(e)=>return Out::failed("Havok packfile",e)};
            let col=havok::Collision::load(&pf);
            let groups:Vec<(String,Vec<[[f32;3];3]>)>=col.bodies.iter().map(|b|(b.name.clone(),b.tris.clone())).collect();
            let unref:Vec<&String>=pf.virt.values().filter(|c|!table.has(c)).collect();
            let mut dump=pf.dump();dump["classes_from_file_types"]=json!(added);
            let o=Out::new("Havok 4.6 packfile").json("json",&dump).file("obj",export::obj(&groups).into_bytes());
            if unref.is_empty(){o}else{o.partial(format!("classes without reflection data: {unref:?}"))}
        }
        "vlt"=>{
            let Some(bin)=ctx.sibling(src,"db.bin") else{return Out::failed("EA Attrib vault","db.bin not found beside db.vlt")};
            match vlt::Database::load(d,&bin,vlt::known_names()){Ok(db)=>Out::new("EA Attrib vault").json("json",&db.to_json()),Err(e)=>Out::failed("EA Attrib vault",e)}
        }
        "loc"=>{
            let idx=ctx.sibling(src,"string.idx");
            let Some(idx)=idx else{return Out::failed("EA localisation","string.idx not found")};
            match locale::Locale::parse(d,&idx){
                Ok(l)=>Out::new("EA localisation").json("json",&json!({"strings":l.strings,"index":l.index().iter().map(|&(h,i)|json!({"hash":format!("{h:08x}"),"string":i})).collect::<Vec<_>>()})),
                Err(e)=>Out::failed("EA localisation",e),
            }
        }
        "idx" if d.get(..4)==Some(&[0x20,0x04,0x05,0x19])=>{
            let n=crate::skeleton::be32(d,4).unwrap_or(0) as usize;
            let rows:Vec<Value>=(0..n).filter_map(|i|Some(json!({"hash":format!("{:08x}",crate::skeleton::be32(d,8+8*i).ok()?),"string":crate::skeleton::be32(d,12+8*i).ok()?}))).collect();
            if rows.len()!=n||8+8*n!=d.len(){return Out::failed("EA localisation index","table length mismatch")}
            Out::new("EA localisation index").json("json",&json!({"version":"20040519","entries":rows}))
        }
        "con"=>match conversation::parse(d){
            Ok(c)=>{
                let node=|n:&conversation::Node|json!({"text_id":format!("{:08x}",n.text_id),"text":ctx.english.and_then(|l|l.get_hash(n.text_id)),"value":n.value,"children":n.children});
                let mut o=Out::new("EA conversation").json("json",&json!({"flag":c.flag,"name":c.name,"roots":c.roots.iter().map(node).collect::<Vec<_>>(),"dialogs":c.dialogs.iter().map(node).collect::<Vec<_>>(),
                    "responses":c.responses.iter().map(|r|json!({"text_id":format!("{:08x}",r.text_id),"text":ctx.english.and_then(|l|l.get_hash(r.text_id)),"value":r.value,"action":r.action})).collect::<Vec<_>>()}));
                if let Some(l)=ctx.english{o=o.file("txt",c.outline(l).into_bytes())}
                if c.trailing!=0{o.partial(format!("{} trailing bytes",c.trailing))}else{o}
            }
            Err(e)=>Out::failed("EA conversation",e),
        },
        "mkr"=>match placement::parse_markers(d){Ok(m)=>Out::new("EA marker set").json("json",&json!(m.iter().map(|m|json!({"kind":m.kind,"a":m.a,"b":m.b,"matrix":m.matrix})).collect::<Vec<_>>())),Err(e)=>Out::failed("EA marker set",e)},
        "cpt"=>match placement::parse_checkpoints(d){Ok(c)=>Out::new("EA checkpoint set").json("json",&json!({"records":c.records,"lanes":c.lanes.iter().map(|l|json!({"index":l.index,"points":l.points})).collect::<Vec<_>>()})),Err(e)=>Out::failed("EA checkpoint set",e)},
        "gsm"=>match conga::parse(d){
            Ok(g)=>{
                fn tr(t:&conga::Transition)->Value{json!({"kind":t.kind,"name":t.name,"device":t.device,"floats":t.floats,"flags":t.flags,"children":t.children.iter().map(tr).collect::<Vec<_>>()})}
                let o=Out::new("EA Conga gesture machines").json("json",&json!({"flag":g.flag,"value":g.value,"machines":g.machines.iter().map(|m|json!({"name":m.name,"sequences":m.sequences})).collect::<Vec<_>>(),
                    "sequences":g.sequences.iter().map(|s|json!({"name":s.name,"transitions":s.transitions.iter().map(tr).collect::<Vec<_>>()})).collect::<Vec<_>>()}));
                // `EA::Conga::LoadFromFiles` (0x8025780c) serialises the machine and sequence managers and stops; the
                // rest of the fixed 8192-byte buffer is never read.  Keep it verbatim in the report.
                if g.trailing!=0{let t=&d[d.len()-g.trailing..];o.json("trailer.json",&json!({"bytes":g.trailing,"nonzero":t.iter().filter(|&&b|b!=0).count(),"hex":t.iter().take(64).map(|b|format!("{b:02x}")).collect::<String>(),"read_by_game":false}))}else{o}
            }
            Err(e)=>Out::failed("EA Conga gesture machines",e),
        },
        "asf"|"ast"|"dat" if d.starts_with(b"SCHl")=>{
            let list=audio::streams(d);let mut o=Out::new("EA SCHl audio stream");let mut bad=vec![];
            for (i,&(a,b)) in list.iter().enumerate(){match wav_stream(&d[a..b]){Ok(p)=>o=o.file(&format!("wav/{i:03}.wav"),audio::to_wav(&p)),Err(e)=>bad.push(format!("stream {i}: {e}"))}}
            if list.is_empty(){return Out::failed("EA SCHl audio stream","no streams")}
            if bad.is_empty(){o}else{o.partial(bad.join("; "))}
        }
        "bnk"|"abk"=>{
            let fmt=if ext=="abk"{"EA AEMS module bank"}else{"EA sound bank"};
            let bank=if ext=="abk"{match audio::abk_bank(d){Ok(b)=>b,Err(e)=>return Out::failed(fmt,e)}}else{Some(d)};
            let mut o=Out::new(fmt);let mut bad=vec![];let mut n=0;
            if let Some(b)=bank{
                match audio::parse_bank(b){Ok(s)=>n=s.len(),Err(e)=>return Out::failed(fmt,e)}
                for i in 0..n{match audio::decode_bank_sound(b,i){Ok(p)=>o=o.file(&format!("wav/{i:03}.wav"),audio::to_wav(&p)),Err(e)=>bad.push(format!("sound {i}: {e}"))}}
            }
            if ext=="abk"{
                let names:std::collections::BTreeMap<u16,String>=ctx.sibling(src,"playground_aems.csi").and_then(|c|crate::formats2::csi(&c).ok()).map(|v|{
                    ["table1","table2","table3"].iter().flat_map(|t|v[*t].as_array().cloned().unwrap_or_default()).filter_map(|e|Some((u16::from_str_radix(e["id"].as_str()?,16).ok()?,e["name"].as_str()?.to_string()))).collect()}).unwrap_or_default();
                match crate::aems::decode(d,&names){
                    Ok(b)=>{let unk=b.json["code"]["undecoded_words"].as_u64().unwrap_or(0);o=o.json("json",&b.json).file("s",b.listing.into_bytes());if unk>0{bad.push(format!("module code: {unk} words not disassembled"))}}
                    Err(e)=>bad.push(format!("module graph: {e}")),
                }
            }
            if bad.is_empty(){o}else{o.partial(format!("{} of {n} sounds / parts undecoded: {}",bad.len(),bad.join("; ")))}
        }
        "apt"=>{
            let c=ctx.sibling(src,&format!("{}.const",stem_of(name)));
            match apt_dump::parse(d,c.as_deref()){
                Ok(m)=>{let o=Out::new("EA APT UI program").json("json",&m.json);
                    if c.is_none(){o.partial("sibling .const missing: movie entry and constants inferred")}else if m.stats.unknown_opcodes>0{o.partial(format!("{} unknown opcodes",m.stats.unknown_opcodes))}else{o}}
                Err(e)=>Out::failed("EA APT UI program",e),
            }
        }
        "const" if d.starts_with(b"Apt constant file")=>match apt_dump::ConstFile::parse(d).and_then(|c|c.to_json()){Ok(v)=>Out::new("EA APT constants").json("json",&v),Err(e)=>Out::failed("EA APT constants",e)},
        "csv"|"txt"|"ini"|"bts"=>Out::new("text").file("txt",text(d).into_bytes()),
        _=>formats::decode(ctx,src,name,&ext,d),
    }
}

pub struct Record{pub source:String,pub ext:String,pub size:usize,pub sha256:String,pub format:String,pub status:Status,pub detail:String,pub outputs:Vec<String>,pub duplicate_of:Option<usize>}

fn loose_files(root:&Path,out:&mut Vec<PathBuf>){
    let Ok(rd)=std::fs::read_dir(root) else{return};
    let mut v:Vec<PathBuf>=rd.flatten().map(|e|e.path()).collect();v.sort();
    for p in v{if p.is_dir(){loose_files(&p,out)}else{out.push(p)}}
}

/// Decode everything below `data`, write outputs to `out`.  `only` restricts to extensions (records of other
/// extensions are still walked so archives expand, but are not decoded or written).
pub fn run(data:&Path,out:&Path,only:Option<Vec<String>>)->Result<Vec<Record>,String>{
    std::fs::create_dir_all(out).map_err(|e|e.to_string())?;
    let schemas=model::Schemas::embedded();let hk=havok::ClassTable::embedded();
    let loc_dir=data.join("files").join("data").join("locale");
    let english=std::fs::read(loc_dir.join("eng_us.loc")).ok().zip(std::fs::read(loc_dir.join("string.idx")).ok()).and_then(|(l,i)|locale::Locale::parse(&l,&i).ok());
    let mut files=vec![];loose_files(data,&mut files);
    let mut records:Vec<Record>=vec![];let mut seen:HashMap<(String,String),usize>=HashMap::new();
    let started=std::time::Instant::now();
    for (fi,path) in files.iter().enumerate(){
        let src=path.to_string_lossy().into_owned();
        let raw=std::fs::read(path).map_err(|e|format!("{src}: {e}"))?;
        let top=archive::decompress(&raw).unwrap_or(raw);
        let name=path.file_name().unwrap().to_string_lossy().into_owned();
        let mut recs:Vec<(String,String,Vec<u8>)>=vec![];
        archive::walk_bytes(&src,&top,&name,&mut |s:&str,b:&[u8]|{let n=s.rsplit("::").next().unwrap_or(s);let n=if s.contains("::"){n.to_string()}else{name.clone()};recs.push((s.to_string(),n,b.to_vec()))});
        let members:HashMap<String,usize>=recs.iter().enumerate().map(|(i,r)|(r.0.clone(),i)).collect();
        let ctx=Ctx{members:&members,records:&recs,english:english.as_ref(),schemas:&schemas,havok:&hk};
        for (vsrc,vname,bytes) in &recs{
            let ext=ext_of(vname);
            let hash=sha256::hex(bytes);
            let is_container=archive::entries(bytes).map(|r|r.is_ok()).unwrap_or(false)&&!vname.to_lowercase().ends_with(".bh");
            if let Some(o)=&only{if !o.contains(&ext)&&!is_container{continue}}
            if let Some(&first)=seen.get(&(hash.clone(),ext.clone())){
                let f=&records[first];
                records.push(Record{source:vsrc.clone(),ext,size:bytes.len(),sha256:hash,format:f.format.clone(),status:f.status,detail:f.detail.clone(),outputs:vec![],duplicate_of:Some(first)});continue
            }
            let res=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||decode(&ctx,vsrc,vname,bytes)));
            let o=res.unwrap_or_else(|p|Out::failed("panic",p.downcast_ref::<String>().cloned().or_else(||p.downcast_ref::<&str>().map(|s|s.to_string())).unwrap_or_default()));
            let rel=vsrc.strip_prefix(&*data.to_string_lossy()).unwrap_or(vsrc).trim_start_matches(['\\','/']).replace("::","/").replace('\\',"/");
            let base:PathBuf=rel.split('/').map(safe).collect();
            let mut outputs=vec![];
            for (suffix,b) in &o.files{
                let p=out.join(format!("{}.{}",base.to_string_lossy(),suffix));
                if let Some(parent)=p.parent(){std::fs::create_dir_all(parent).map_err(|e|format!("{}: {e}",parent.display()))?}
                std::fs::write(&p,b).map_err(|e|format!("{}: {e}",p.display()))?;
                outputs.push(p.strip_prefix(out).unwrap_or(&p).to_string_lossy().replace('\\',"/"));
            }
            seen.insert((hash.clone(),ext.clone()),records.len());
            records.push(Record{source:vsrc.clone(),ext,size:bytes.len(),sha256:hash,format:o.format,status:o.status,detail:o.detail,outputs,duplicate_of:None});
        }
        eprintln!("[{}/{}] {:.0}s {}",fi+1,files.len(),started.elapsed().as_secs_f64(),path.strip_prefix(data).unwrap_or(path).display());
    }
    write_report(data,out,&records)?;
    Ok(records)
}

pub fn write_report(data:&Path,out:&Path,records:&[Record])->Result<(),String>{
    let mut by_ext:BTreeMap<String,BTreeMap<&'static str,usize>>=BTreeMap::new();
    let mut uniq:BTreeMap<String,BTreeMap<&'static str,usize>>=BTreeMap::new();
    for r in records{
        *by_ext.entry(r.ext.clone()).or_default().entry(r.status.name()).or_default()+=1;
        if r.duplicate_of.is_none(){*uniq.entry(r.ext.clone()).or_default().entry(r.status.name()).or_default()+=1}
    }
    let total=|m:&BTreeMap<String,BTreeMap<&'static str,usize>>,s:&str|m.values().map(|v|v.get(s).copied().unwrap_or(0)).sum::<usize>();
    const NAMES:[&str;5]=["decoded","partial","container","failed","unsupported"];
    let st_all:serde_json::Map<String,Value>=NAMES.iter().map(|s|(s.to_string(),json!(total(&by_ext,s)))).collect();
    let st_uniq:serde_json::Map<String,Value>=NAMES.iter().map(|s|(s.to_string(),json!(total(&uniq,s)))).collect();
    let summary=json!({"data":data,"records":records.len(),"distinct":records.iter().filter(|r|r.duplicate_of.is_none()).count(),
        "status":st_all,"distinct_status":st_uniq,
        "by_extension":by_ext,"distinct_by_extension":uniq});
    let recs:Vec<Value>=records.iter().map(|r|json!({"source":r.source,"ext":r.ext,"size":r.size,"sha256":r.sha256,"format":r.format,"status":r.status.name(),"detail":r.detail,"outputs":r.outputs,"duplicate_of":r.duplicate_of.map(|i|&records[i].source)})).collect();
    std::fs::write(out.join("report.json"),serde_json::to_vec_pretty(&json!({"summary":summary,"records":recs})).unwrap()).map_err(|e|e.to_string())?;
    let mut md=format!("# Native decode of the game data\n\nSource: `{}`  \nRecords: {} ({} distinct contents)\n\n| Extension | Distinct | Decoded | Partial | Container | Failed | Unsupported | Format |\n|---|---:|---:|---:|---:|---:|---:|---|\n",
        data.display(),records.len(),records.iter().filter(|r|r.duplicate_of.is_none()).count());
    for (e,m) in &uniq{
        let fmt=records.iter().find(|r|&r.ext==e&&r.duplicate_of.is_none()).map(|r|r.format.as_str()).unwrap_or("");
        let g=|s:&str|m.get(s).copied().unwrap_or(0);
        md+=&format!("| .{e} | {} | {} | {} | {} | {} | {} | {fmt} |\n",m.values().sum::<usize>(),g("decoded"),g("partial"),g("container"),g("failed"),g("unsupported"));
    }
    md+="\n## Records that are not fully decoded\n\n";
    for r in records.iter().filter(|r|r.duplicate_of.is_none()&&matches!(r.status,Status::Partial|Status::Failed|Status::Unsupported)){
        md+=&format!("- **{}** `{}`: {}\n",r.status.name(),r.source.strip_prefix(&*data.to_string_lossy()).unwrap_or(&r.source),r.detail.chars().take(300).collect::<String>());
    }
    std::fs::write(out.join("REPORT.md"),md).map_err(|e|e.to_string())
}
