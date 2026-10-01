//! Decoders for the remaining formats of the corpus (dispatched by `corpus::decode` after the established ones):
//! LION effect trees, the external BIG directory, Wii disc/partition system files, the DOL/ELF executables, the
//! channel banner and the NW4R (Home Button menu / keyboard) formats.
use crate::corpus::{self,Ctx,Out};
use crate::{archive,audio,export,nw4r};
use serde_json::{json,Value};

fn be16(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_be_bytes([b[0],b[1]])).ok_or_else(||format!("read past end at {o:#x}"))}
fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
fn be64(d:&[u8],o:usize)->Result<u64,String>{d.get(o..o+8).map(|b|u64::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
fn fixed(d:&[u8],o:usize,n:usize)->Result<String,String>{let s=d.get(o..o+n).ok_or("field past end")?;Ok(String::from_utf8_lossy(&s[..s.iter().position(|&c|c==0).unwrap_or(n)]).into_owned())}
fn hex(b:&[u8])->String{b.iter().map(|x|format!("{x:02x}")).collect()}


// ---------------------------------------------------------------------------------------------------------------
// LION effects (`effects/*.lef`): text tree `<TAG name="x">` `{` `KEY = v[, v...]` `}`.
pub fn lef(d:&[u8])->Result<Value,String>{
    let text=std::str::from_utf8(d).map_err(|_|"LEF is not UTF-8 text")?;
    fn value(v:&str)->Value{
        let parts:Vec<&str>=v.split(',').map(str::trim).collect();
        let one=|s:&str|->Value{if let Ok(i)=s.parse::<i64>(){json!(i)}else if let Ok(f)=s.parse::<f64>(){json!(f)}else if s=="(NULL)"{Value::Null}else{json!(s)}};
        if parts.len()>1{Value::Array(parts.iter().map(|p|one(p)).collect())}else{one(v.trim())}
    }
    let mut stack:Vec<Value>=vec![json!({"children":[]})];let mut pending:Option<Value>=None;
    for (ln,line) in text.lines().enumerate(){
        let l=line.trim();if l.is_empty(){continue}
        if let Some(rest)=l.strip_prefix('<'){
            let rest=rest.strip_suffix('>').ok_or(format!("line {}: unterminated tag",ln+1))?;
            let (tag,attr)=rest.split_once(char::is_whitespace).unwrap_or((rest,""));
            let name=attr.split_once('=').map(|(_,v)|v.trim().trim_matches('"').to_string());
            pending=Some(json!({"tag":tag,"name":name,"fields":{},"children":[]}));
        }else if l=="{"{stack.push(pending.take().ok_or(format!("line {}: block without tag",ln+1))?)}
        else if l=="}"{
            let node=stack.pop().ok_or(format!("line {}: unbalanced }}",ln+1))?;
            stack.last_mut().ok_or(format!("line {}: unbalanced }}",ln+1))?["children"].as_array_mut().unwrap().push(node);
        }else if let Some((k,v))=l.split_once('='){
            let top=stack.last_mut().unwrap();if top.get("fields").is_none(){return Err(format!("line {}: field outside a block",ln+1))}
            top["fields"][k.trim()]=value(v);
        }else{return Err(format!("line {}: unrecognised {l:?}",ln+1))}
    }
    if stack.len()!=1||pending.is_some(){return Err("unbalanced LEF blocks".into())}
    Ok(stack.pop().unwrap()["children"].take())
}

// ---------------------------------------------------------------------------------------------------------------
// Wii disc and partition system files (layouts: the disc/partition headers read by the apploader and IOS).

fn disc_header(d:&[u8])->Result<Value,String>{
    if be32(d,0x18)?!=0x5d1c9ea3{return Err("Wii disc magic missing".into())}
    Ok(json!({"game_id":fixed(d,0,6)?,"disc":d[6],"version":d[7],"audio_streaming":d[8],"stream_buffer":d[9],"magic":"5d1c9ea3","title":fixed(d,0x20,0x40)?}))
}
fn boot_bin(d:&[u8])->Result<Value,String>{
    let mut v=disc_header(d)?;
    // Wii partition offsets are stored >> 2.
    v["debug_monitor_offset"]=json!(be32(d,0x400)?);v["debug_load_address"]=json!(format!("{:08x}",be32(d,0x404)?));
    v["dol_offset"]=json!((be32(d,0x420)? as u64)<<2);v["fst_offset"]=json!((be32(d,0x424)? as u64)<<2);
    v["fst_size"]=json!((be32(d,0x428)? as u64)<<2);v["fst_max_size"]=json!((be32(d,0x42c)? as u64)<<2);
    Ok(v)
}
fn bi2(d:&[u8])->Result<Value,String>{
    Ok(json!({"debug_monitor_size":be32(d,0)?,"simulated_memory_size":be32(d,4)?,"argument_offset":be32(d,8)?,"debug_flag":be32(d,0xc)?,"track_location":be32(d,0x10)?,
        "track_size":be32(d,0x14)?,"country_code":be32(d,0x18)?,"total_discs":be32(d,0x1c)?,"long_file_names":be32(d,0x20)?,"pad_spec":be32(d,0x24)?,"dol_limit":be32(d,0x28)?}))
}
fn region(d:&[u8])->Result<Value,String>{
    Ok(json!({"region":(["Japan","USA","Europe","?","Korea"].get(be32(d,0)? as usize).copied().unwrap_or("?")),"age_ratings":hex(d.get(0x10..0x20).ok_or("short region.bin")?)}))
}
fn fst(d:&[u8])->Result<Value,String>{
    let n=be32(d,8)? as usize;let names=n*12;
    let mut out=vec![];let mut dirs:Vec<(String,usize)>=vec![(String::new(),n)];
    for i in 1..n{
        while dirs.last().map(|x|i>=x.1).unwrap_or(false){dirs.pop();}
        let w=be32(d,i*12)?;let name=d.get(names+(w&0xffffff) as usize..).ok_or("FST name past end")?;
        let name=String::from_utf8_lossy(&name[..name.iter().position(|&c|c==0).ok_or("FST name")?]).into_owned();
        let path=format!("{}{}",dirs.last().map(|x|x.0.as_str()).unwrap_or(""),name);
        if w>>24==1{let end=be32(d,i*12+8)? as usize;out.push(json!({"dir":path,"parent":be32(d,i*12+4)?,"next":end}));dirs.push((format!("{path}/"),end))}
        else{out.push(json!({"file":path,"offset":(be32(d,i*12+4)? as u64)<<2,"size":be32(d,i*12+8)?}))}
    }
    Ok(json!({"entries":out}))
}
fn apploader(d:&[u8])->Result<Value,String>{
    Ok(json!({"date":fixed(d,0,16)?,"entry":format!("{:08x}",be32(d,0x10)?),"size":be32(d,0x14)?,"trailer_size":be32(d,0x18)?,"code_bytes":d.len().saturating_sub(0x20)}))
}
/// DOL: 7 text + 11 data sections (file offset, load address, size), BSS, entry point.
pub fn dol(d:&[u8])->Result<Value,String>{
    let sec=|i:usize|->Result<Value,String>{Ok(json!({"kind":if i<7{"text"}else{"data"},"index":if i<7{i}else{i-7},"offset":be32(d,4*i)?,"address":format!("{:08x}",be32(d,0x48+4*i)?),"size":be32(d,0x90+4*i)?}))};
    let sections:Vec<Value>=(0..18).map(sec).collect::<Result<Vec<_>,_>>()?.into_iter().filter(|s|s["size"]!=0).collect();
    for s in &sections{let (o,n)=(s["offset"].as_u64().unwrap() as usize,s["size"].as_u64().unwrap() as usize);if o+n>d.len(){return Err("DOL section outside file".into())}}
    Ok(json!({"sections":sections,"bss_address":format!("{:08x}",be32(d,0xd8)?),"bss_size":be32(d,0xdc)?,"entry":format!("{:08x}",be32(d,0xe0)?),"sha256":crate::sha256::hex(d)}))
}
/// Signed blobs: signature type -> (signature length, padding).
fn sig_len(t:u32)->Option<usize>{match t{0x10000=>Some(0x200+0x3c),0x10001=>Some(0x100+0x3c),0x10002=>Some(0x3c+0x40),_=>None}}
fn certs(d:&[u8])->Result<Value,String>{
    let mut o=0;let mut v=vec![];
    while o+4<=d.len(){
        let st=be32(d,o)?;let sl=sig_len(st).ok_or(format!("unknown signature type {st:#x} at {o:#x}"))?;
        let b=o+4+sl;let kt=be32(d,b+0x40)?;let klen=match kt{0=>0x238,1=>0x138,2=>0x78,_=>return Err(format!("unknown key type {kt}"))};
        v.push(json!({"signature_type":format!("{st:#x}"),"issuer":fixed(d,b,0x40)?,"key_type":kt,"name":fixed(d,b+0x44,0x40)?,"key_id":be32(d,b+0x84)?}));
        o=b+0x88+klen;
        if v.len()>16{break}
    }
    Ok(json!({"certificates":v}))
}
fn ticket(d:&[u8])->Result<Value,String>{
    let st=be32(d,0)?;let b=4+sig_len(st).ok_or("unknown ticket signature")?;
    Ok(json!({"signature_type":format!("{st:#x}"),"issuer":fixed(d,b,0x40)?,"title_key_encrypted":hex(d.get(b+0x7f..b+0x8f).ok_or("short")?),"ticket_id":format!("{:016x}",be64(d,b+0x90)?),
        "console_id":be32(d,b+0x98)?,"title_id":format!("{:016x}",be64(d,b+0x9c)?),"ticket_version":be16(d,b+0xa6)?,"common_key_index":d.get(b+0xb1).copied()}))
}
fn tmd(d:&[u8])->Result<Value,String>{
    let st=be32(d,0)?;let b=4+sig_len(st).ok_or("unknown TMD signature")?;
    let n=be16(d,b+0x9e)? as usize;
    let contents:Vec<Value>=(0..n).map(|i|{let c=b+0xa4+36*i;Ok(json!({"id":format!("{:08x}",be32(d,c)?),"index":be16(d,c+4)?,"type":be16(d,c+6)?,"size":be64(d,c+8)?,"sha1":hex(d.get(c+16..c+36).ok_or("short")?)}))}).collect::<Result<_,String>>()?;
    Ok(json!({"signature_type":format!("{st:#x}"),"issuer":fixed(d,b,0x40)?,"version":d[b+0x40],"ios":format!("{:016x}",be64(d,b+0x44)?),"title_id":format!("{:016x}",be64(d,b+0x4c)?),
        "title_type":be32(d,b+0x54)?,"group_id":be16(d,b+0x58)?,"region":be16(d,b+0x5c)?,"access_rights":be32(d,b+0x98)?,"title_version":be16(d,b+0x9c)?,"boot_index":be16(d,b+0xa0)?,"contents":contents}))
}
fn h3(d:&[u8])->Result<Value,String>{
    let hashes:Vec<String>=d.chunks_exact(20).take_while(|c|c.iter().any(|&b|b!=0)).map(hex).collect();
    Ok(json!({"entries":hashes.len(),"sha1":hashes}))
}

/// ELF32 big-endian: header, sections and the full symbol table (the retail disc ships the symbol-bearing ELF).
pub fn elf(d:&[u8])->Result<Value,String>{
    if d.get(..4)!=Some(b"\x7fELF"){return Err("not ELF".into())}
    let (shoff,shentsize,shnum,shstrndx)=(be32(d,0x20)? as usize,be16(d,0x2e)? as usize,be16(d,0x30)? as usize,be16(d,0x32)? as usize);
    let sh=|i:usize,f:usize|be32(d,shoff+i*shentsize+f);
    let strtab=|off:usize,at:usize|->String{d.get(off+at..).map(|s|String::from_utf8_lossy(&s[..s.iter().position(|&c|c==0).unwrap_or(0)]).into_owned()).unwrap_or_default()};
    let shstr=sh(shstrndx,0x10)? as usize;
    let mut sections=vec![];let mut symbols=vec![];
    for i in 0..shnum{
        let (name,ty,addr,off,size,link,entsize)=(sh(i,0)? as usize,sh(i,4)?,sh(i,0xc)?,sh(i,0x10)? as usize,sh(i,0x14)? as usize,sh(i,0x18)? as usize,sh(i,0x24)? as usize);
        sections.push(json!({"name":strtab(shstr,name),"type":ty,"address":format!("{addr:08x}"),"offset":off,"size":size}));
        if ty==2&&entsize==16{
            let so=sh(link,0x10)? as usize;
            for k in 0..size/16{
                let e=off+16*k;let (n,v,s,info,shndx)=(be32(d,e)? as usize,be32(d,e+4)?,be32(d,e+8)?,d[e+12],be16(d,e+14)?);
                if n==0{continue}
                symbols.push(json!({"name":strtab(so,n),"address":format!("{v:08x}"),"size":s,"type":(["notype","object","func","section","file"].get((info&15) as usize).copied().unwrap_or("?")),"bind":info>>4,"section":shndx}));
            }
        }
    }
    Ok(json!({"entry":format!("{:08x}",be32(d,0x18)?),"machine":be16(d,0x12)?,"sections":sections,"symbols":symbols,"sha256":crate::sha256::hex(d)}))
}

/// Full disassembly of every function symbol in the executable sections of an ELF, with call targets named.
pub fn elf_listing(d:&[u8],info:&Value)->Result<(String,usize,usize),String>{
    let (shoff,shentsize,shnum)=(be32(d,0x20)? as usize,be16(d,0x2e)? as usize,be16(d,0x30)? as usize);
    let mut exec=vec![];
    for i in 0..shnum{let b=shoff+i*shentsize;let (flags,addr,off,size)=(be32(d,b+8)?,be32(d,b+0xc)?,be32(d,b+0x10)? as usize,be32(d,b+0x14)? as usize);if flags&4!=0&&be32(d,b+4)?==1{exec.push((addr,off,size))}}
    let mut funcs:Vec<(u32,u32,String)>=info["symbols"].as_array().unwrap().iter().filter(|s|s["type"]=="func"&&s["size"].as_u64().unwrap_or(0)>0)
        .map(|s|(u32::from_str_radix(s["address"].as_str().unwrap(),16).unwrap(),s["size"].as_u64().unwrap() as u32,s["name"].as_str().unwrap().to_string())).collect();
    funcs.sort();funcs.dedup_by_key(|f|f.0);
    let by_addr:std::collections::HashMap<u32,&str>=funcs.iter().map(|f|(f.0,f.2.as_str())).collect();
    let names=|t:u32|by_addr.get(&t).map(|s|s.to_string());let note=|_:u32,_:u32|None;
    let mut out=String::new();let (mut n,mut unknown)=(0,0);
    for (a,size,name) in &funcs{
        let Some(&(sa,so,_))=exec.iter().find(|(sa,_,ss)|*a>=*sa&&a+size<=sa+*ss as u32) else{continue};
        let o=so+(a-sa) as usize;let code=d.get(o..o+*size as usize).ok_or("function outside file")?;
        let (text,u)=crate::ppc::listing(code,*a,&names,&note);
        out+=&format!("
{name}:  # {a:#010x} size {size:#x}
");out+=&text;n+=1;unknown+=u;
    }
    Ok((out,n,unknown))
}

// ---------------------------------------------------------------------------------------------------------------
// Channel banner: `opening.bnr` = 0x40 padding, IMET header (localised names, sizes, MD5), U8 archive `meta/`
// holding `banner.bin`, `icon.bin`, `sound.bin`, each `IMD5` (+ MD5) wrapping an LZ77-compressed U8 or a BNS.

/// Nintendo LZ77 (type 0x10 `LZ77` header variant as used by channel banners).
pub fn lz77(d:&[u8])->Result<Vec<u8>,String>{
    let d=if d.starts_with(b"LZ77"){&d[4..]}else{d};
    if d.first()!=Some(&0x10){return Err("not LZ77 type 0x10".into())}
    let n=(d[1] as usize)|((d[2] as usize)<<8)|((d[3] as usize)<<16);
    let mut out=Vec::with_capacity(n);let mut p=4;
    while out.len()<n{
        let flags=*d.get(p).ok_or("LZ77 truncated")?;p+=1;
        for bit in (0..8).rev(){
            if out.len()>=n{break}
            if flags>>bit&1==0{out.push(*d.get(p).ok_or("LZ77 truncated")?);p+=1}
            else{
                let (a,b)=(*d.get(p).ok_or("LZ77 truncated")? as usize,*d.get(p+1).ok_or("LZ77 truncated")? as usize);p+=2;
                let len=(a>>4)+3;let disp=(((a&15)<<8)|b)+1;
                if disp>out.len(){return Err("LZ77 back-reference before start".into())}
                for _ in 0..len{out.push(out[out.len()-disp])}
            }
        }
    }
    out.truncate(n);Ok(out)
}
fn imd5(d:&[u8])->Result<(&[u8],String),String>{
    if d.get(..4)!=Some(b"IMD5"){return Err("IMD5 header missing".into())}
    let n=be32(d,4)? as usize;Ok((d.get(0x20..0x20+n).ok_or("IMD5 size exceeds file")?,hex(&d[0x10..0x20])))
}
fn banner(ctx:&Ctx,src:&str,d:&[u8])->Result<Out,String>{
    let base=if d.get(0x40..0x44)==Some(b"IMET"){0x40}else if d.starts_with(b"IMET"){0}else{return Err("IMET header missing".into())};
    let langs=["japanese","english","german","french","spanish","italian","dutch","chinese_simplified","chinese_traditional","korean"];
    let names:serde_json::Map<String,Value>=langs.iter().enumerate().map(|(i,l)|{
        let o=base+0x1c+0x54*i;let units:Vec<u16>=(0..0x2a).map(|k|be16(d,o+2*k).unwrap_or(0)).take_while(|&u|u!=0).collect();(l.to_string(),json!(String::from_utf16_lossy(&units)))}).collect();
    let meta=json!({"imet_offset":base,"sizes":[be32(d,base+0x0c)?,be32(d,base+0x10)?,be32(d,base+0x14)?],"flag":be32(d,base+0x18)?,"names":names,"md5":hex(&d[base+0x5f0..base+0x600])});
    let u8o=(0..d.len().saturating_sub(4)).step_by(0x20).find(|&o|d[o..].starts_with(b"U\xaa8-")).ok_or("banner U8 archive not found")?;
    let arc_d=&d[u8o..];
    let entries=archive::u8_entries(arc_d)?;
    let mut o=Out::new("Wii channel banner").json("json",&meta);let mut parts=vec![];
    for e in &entries{
        let raw=&arc_d[e.offset..e.offset+e.size];let stem=corpus::safe(&e.name);
        let (payload,md5)=imd5(raw)?;
        if payload.starts_with(b"BNS "){let (info,pcm)=nw4r::bns(payload)?;o=o.file(&format!("{stem}.wav"),audio::to_wav(&pcm));parts.push(json!({"name":e.name,"md5":md5,"sound":info}));continue}
        let inner=if payload.starts_with(b"LZ77")||payload.first()==Some(&0x10){lz77(payload)?}else{payload.to_vec()};
        let members=archive::u8_entries(&inner)?;let mut list=vec![];
        for m in &members{
            let b=&inner[m.offset..m.offset+m.size];let sub=corpus::decode(ctx,&format!("{src}::{}::{}",e.name,m.name),&m.name,b);
            for (sfx,bytes) in sub.files{o=o.file(&format!("{stem}/{}.{sfx}",corpus::safe(&m.name)),bytes)}
            list.push(json!({"name":m.name,"size":m.size,"format":sub.format,"status":sub.status.name(),"detail":sub.detail}));
            if matches!(sub.status,corpus::Status::Failed|corpus::Status::Unsupported){return Err(format!("{}::{}: {}",e.name,m.name,sub.detail))}
        }
        parts.push(json!({"name":e.name,"md5":md5,"members":list}));
    }
    Ok(o.json("parts.json",&json!(parts)))
}

// ---------------------------------------------------------------------------------------------------------------

pub fn decode(ctx:&Ctx,src:&str,name:&str,ext:&str,d:&[u8])->Out{
    let lname=name.to_lowercase();let file=lname.rsplit(['/','\\']).next().unwrap_or(&lname).to_string();
    let js=|fmt:&str,r:Result<Value,String>|match r{Ok(v)=>Out::new(fmt).json("json",&v),Err(e)=>Out::failed(fmt,e)};
    match (ext,file.as_str()){
        ("lef",_)=>js("LION effect tree",lef(d)),
        ("bh",_)=>match archive::big_entries(d){Ok(es)=>Out::new("EA BIG external directory").json("json",&json!({"entries":es.iter().map(|e|json!({"name":e.name,"offset":e.offset,"size":e.size})).collect::<Vec<_>>()})),Err(e)=>Out::failed("EA BIG external directory",e)},
        ("bin","header.bin")=>js("Wii disc header",disc_header(d)),
        ("bin","region.bin")=>js("Wii region settings",region(d)),
        ("bin","boot.bin")=>js("Wii partition boot header",boot_bin(d)),
        ("bin","bi2.bin")=>js("Wii partition bi2",bi2(d)),
        ("bin","fst.bin")=>js("Wii file system table",fst(d)),
        ("bin","cert.bin")=>js("Wii certificate chain",certs(d)),
        ("bin","ticket.bin")=>js("Wii ticket",ticket(d)),
        ("bin","tmd.bin")=>js("Wii title metadata",tmd(d)),
        ("bin","h3.bin")=>js("Wii H3 hash table",h3(d)),
        ("bin","db.bin")=>Out::new("EA Attrib vault data").json("json",&json!({"decoded_with":"db.vlt","size":d.len()})),
        ("img","apploader.img")=>js("Wii apploader",apploader(d)),
        ("dol",_)=>js("Nintendo DOL executable",dol(d)),
        ("elf",_)=>match elf(d).and_then(|v|elf_listing(d,&v).map(|l|(v,l))){
            Ok((mut v,(text,n,unknown)))=>{v["disassembly"]=json!({"functions":n,"undecoded_words":unknown});let o=Out::new("ELF32 executable").json("json",&v).file("s",text.into_bytes());
                if unknown>0{o.partial(format!("{unknown} instruction words inside function symbols did not disassemble"))}else{o}}
            Err(e)=>Out::failed("ELF32 executable",e),
        },
        ("bnr",_)=>banner(ctx,src,d).unwrap_or_else(|e|Out::failed("Wii channel banner",e)),
        ("brlyt",_)=>js("NW4R layout",nw4r::brlyt(d)),
        ("brlan",_)=>js("NW4R layout animation",nw4r::brlan(d)),
        ("brfnt",_)=>match nw4r::brfnt(d){
            Ok((info,sheets))=>{let mut o=Out::new("NW4R font").json("json",&info);for (n,px,w,h) in sheets{match export::png(&px,w,h){Ok(p)=>o=o.file(&format!("png/{n}.png"),p),Err(e)=>return Out::failed("NW4R font",e)}}o}
            Err(e)=>Out::failed("NW4R font",e),
        },
        ("bwav",_)|("brwav",_) if d.starts_with(b"RWAV")=>match nw4r::rwav(d){Ok((info,pcm))=>Out::new("NW4R wave").json("json",&info).file("wav",audio::to_wav(&pcm)),Err(e)=>Out::failed("NW4R wave",e)},
        ("bwav",_)=>Out::new("Wii Remote speaker PCM (s16be, 6000 Hz)").file("wav",audio::to_wav(&nw4r::speaker_pcm(d))),
        ("brsar",_)=>match nw4r::rsar(d){
            Ok(r)=>{
                let mut o=Out::new("NW4R sound archive").json("json",&r.json);let mut bad=vec![];
                for (n,f,w) in &r.files{
                    match nw4r::blocks_json(f){Ok(v)=>o=o.json(&format!("{n}.json"),&v),Err(e)=>bad.push(format!("{n}: {e}"))}
                    match nw4r::wave_block(f,w){
                        Ok(ws)=>for (i,x) in ws.into_iter().enumerate(){match x{Ok((_,pcm))=>o=o.file(&format!("{n}/{i:03}.wav"),audio::to_wav(&pcm)),Err(e)=>bad.push(format!("{n} wave {i}: {e}"))}},
                        Err(e)=>bad.push(format!("{n}: {e}")),
                    }
                }
                if bad.is_empty(){o}else{o.partial(bad.join("; "))}
            }
            Err(e)=>Out::failed("NW4R sound archive",e),
        },
        _=>crate::formats2::decode(ctx,src,name,ext,d),
    }
}
