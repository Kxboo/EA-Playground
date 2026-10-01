//! Formats whose loaders were recovered from the executable: EA fonts, speech banks/events, keyboard dictionaries,
//! VP6 video.
use crate::corpus::{Ctx,Out};
use crate::{export,gsh};
use serde_json::{json,Value};

fn le32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
fn be16(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_be_bytes([b[0],b[1]])).ok_or_else(||format!("read past end at {o:#x}"))}
fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
fn u8at(d:&[u8],o:usize)->Result<u8,String>{d.get(o).copied().ok_or_else(||format!("read past end at {o:#x}"))}

/// EA `FntG` fonts (`fonts/*.gfn`).  `FONT` header: `FntG`, u32 LE file size, u16 +8, u16 glyph count (+0xa), u32 flags
/// (+0xc; bit 0x40000 = 16-byte glyphs with per-glyph kerning lists), line metrics bytes (+0x12/+0x13, summed for a line
/// feed by `NEWFONT_getrectw`), glyph table offset (+0x14), kerning table offset (+0x18), glyph-sheet SHAPE offset (+0x1c).
/// Glyph (`FONTINLINE_getcharacter` 0x8040f9fc / `NEWFONT_getrectw` 0x80155e3c): u16 code, u8 width, u8 height,
/// u16 sheet x, u16 sheet y, s8 advance, s8 x offset, s8 y offset, u8 kerning count, [u16 kerning index, s16 advance].
/// Kerning (`FONT_getkern` 0x804106bc): 4-byte pairs {u16 other char, s8 adjust, u8 this char (low byte)}.
/// The sheet is an EA SHAPE record read by `EAGLFont::createfont` (0x803ee0c0) through `SHAPE_clut`.
pub fn gfn(d:&[u8])->Result<(Value,Option<(Vec<u8>,usize,usize)>),String>{
    if d.get(..4)!=Some(b"FntG"){return Err("not an EA FntG font".into())}
    let size=le32(d,4)? as usize;if size!=d.len(){return Err(format!("header size {size} but file is {}",d.len()))}
    let (n,flags,gt,kt,st)=(be16(d,0xa)? as usize,be32(d,0xc)?,be32(d,0x14)? as usize,be32(d,0x18)? as usize,be32(d,0x1c)? as usize);
    let wide=flags&0x40000!=0;let rec=if wide{16}else{12};
    let mut glyphs=vec![];
    for i in 0..n{
        let g=gt+rec*i;
        let mut v=json!({"code":be16(d,g)?,"char":char::from_u32(be16(d,g)? as u32).map(|c|c.to_string()),"width":u8at(d,g+2)?,"height":u8at(d,g+3)?,"x":be16(d,g+4)?,"y":be16(d,g+6)?,
            "advance":u8at(d,g+8)? as i8,"x_offset":u8at(d,g+9)? as i8,"y_offset":u8at(d,g+10)? as i8,"kerning_count":u8at(d,g+11)?});
        if wide{v["kerning_index"]=json!(be16(d,g+12)?);v["advance"]=json!(be16(d,g+14)? as i16)}
        glyphs.push(v);
    }
    if glyphs.windows(2).any(|w|w[0]["code"].as_u64()>w[1]["code"].as_u64()){return Err("glyph table is not sorted by code (FONT_bsearch requires it)".into())}
    let kerning:Vec<Value>=if kt==0{vec![]}else{
        let m=be32(d,kt)? as usize;if m>65536{return Err("kerning count".into())}
        (0..m).map(|i|{let k=kt+4+4*i;Ok(json!([be16(d,k)?,u8at(d,k+2)? as i8,u8at(d,k+3)?]))}).collect::<Result<_,String>>()?
    };
    let mut header=serde_json::Map::new();
    header.insert("u16_08".into(),json!(be16(d,8)?));header.insert("flags".into(),json!(format!("{flags:08x}")));
    header.insert("line_metrics".into(),json!([u8at(d,0x12)?,u8at(d,0x13)?]));header.insert("words_20_2c".into(),json!((0..4).map(|k|be32(d,0x20+4*k).map(|x|format!("{x:08x}"))).collect::<Result<Vec<_>,_>>()?));
    let sheet=if st==0{None}else{let e=gsh::shape_at(d,st)?;Some(gsh::decode(&e,d)?)};
    Ok((json!({"header":header,"glyphs":glyphs,"kerning":kerning,"sheet_offset":st}),sheet))
}

fn cstr(d:&[u8],o:usize)->Result<String,String>{let s=d.get(o..).ok_or_else(||format!("string at {o:#x} outside file"))?;Ok(String::from_utf8_lossy(&s[..s.iter().position(|&c|c==0).ok_or("unterminated string")?]).into_owned())}

/// Csis interface registries (`MOIR`: `audio/speech/events.csi`, `audio/aems/playground_aems.csi`).
/// `Csis::System::Subscribe` (0x80273360): three tables from +0x28 with counts u16 +0xa / +0xc / +0xe; the first two
/// hold 12-byte entries {u32, u32 name offset, u16 id, u16 runtime key}, the third 16-byte entries whose name offset
/// is at +8 (runtime key +0xe).  Name offsets are relative to the file.
pub fn csi(d:&[u8])->Result<Value,String>{
    if d.get(..4)!=Some(b"MOIR"){return Err("not a Csis registry".into())}
    let (a,b,c)=(be16(d,0xa)? as usize,be16(d,0xc)? as usize,be16(d,0xe)? as usize);
    let t12=|base:usize,n:usize|->Result<Vec<Value>,String>{(0..n).map(|i|{let e=base+12*i;Ok(json!({"word0":format!("{:08x}",be32(d,e)?),"name":cstr(d,be32(d,e+4)? as usize)?,"id":format!("{:04x}",be16(d,e+8)?)}))}).collect()};
    let (ta,tb)=(0x28,0x28+12*a);let tc=tb+12*b;
    let third:Vec<Value>=(0..c).map(|i|{let e=tc+16*i;Ok(json!({"words":[format!("{:08x}",be32(d,e)?),format!("{:08x}",be32(d,e+4)?)],"name":cstr(d,be32(d,e+8)? as usize)?,"id":format!("{:04x}",be16(d,e+12)?)}))}).collect::<Result<_,String>>()?;
    Ok(json!({"version":be16(d,4)?,"header":format!("{:04x}{:04x}",be16(d,6)?,be16(d,8)?),"interface":format!("{:04x}",be16(d,0x10)?),"table1":t12(ta,a)?,"table2":t12(tb,b)?,"table3":third}))
}

/// SPCH (EA speech) bank headers (`spchhdr.viv::*.hdr`, `SPCH_AddBank` 0x80291a44, `VOXBANKHDR`): u16 bank id
/// (the sort key with +4), u8 flags (bit 0x80 = cycle bits, `iSPCH_SetCycleBits`), u8 sample count, u32 +4, u8 +8,
/// u16 +0xa, then per sample {u16 start in 256-byte units within the bank's `spchdat.viv::<name>.dat`, u16 attribute}.
/// Each start is verified to land on an `SCHl` stream header in the data bank when that file is available.
pub fn spch_hdr(d:&[u8],dat:Option<&[u8]>)->Result<Value,String>{
    let n=u8at(d,3)? as usize;
    let samples:Vec<(u16,u16)>=(0..n).map(|i|Ok((be16(d,0x10+4*i)?,be16(d,0x12+4*i)?))).collect::<Result<_,String>>()?;
    if samples.windows(2).any(|w|w[0].0>=w[1].0){return Err("sample starts are not increasing".into())}
    let mut checked=Value::Null;
    if let Some(dat)=dat{
        for (i,&(o,_)) in samples.iter().enumerate(){if dat.get(o as usize*256..o as usize*256+4)!=Some(b"SCHl"){return Err(format!("sample {i} start {:#x} is not an SCHl stream in the data bank",o as usize*256))}}
        checked=json!(true);
    }
    Ok(json!({"bank_id":format!("{:04x}",be16(d,0)?),"flags":u8at(d,2)?,"sample_count":n,"word4":format!("{:08x}",be32(d,4)?),"byte8":u8at(d,8)?,"size_units":be16(d,0xa)?,
        "samples":samples.iter().map(|&(o,a)|json!({"offset":o as usize*256,"attribute":a})).collect::<Vec<_>>(),"trailer":d.get(0x10+4*n..).map(|t|t.iter().map(|b|format!("{b:02x}")).collect::<String>()),"stream_starts_verified":checked}))
}

/// SPCH event database (`events.evt`, `SPCH_AddEventDB` 0x8029494c: byte 0 = 3, byte 1 = 0x15).  Header: u32 +4 name
/// table offset, u16 +0x10 event count, u16 table from +0x18 of record offsets in 4-byte units; 52-byte event records
/// (bank id at +0x1c, matching the `.hdr` banks); name entries {u32 name offset, u32 event id} (event ids are the
/// Csis interface 0x071e + the 16-bit ids of `events.csi`).  Other record words are kept as raw values.
pub fn spch_evt(d:&[u8])->Result<Value,String>{
    if u8at(d,0)?!=3||u8at(d,1)?!=0x15{return Err("not a version 3/0x15 SPCH event database".into())}
    let nt=be32(d,4)? as usize;let n=be16(d,0x10)? as usize;
    let names_base=nt+0x20*n;
    let mut events=vec![];
    for i in 0..n{
        let r=be16(d,0x18+2*i)? as usize*4;
        let words:Vec<String>=(0..26).map(|k|Ok(format!("{:04x}",be16(d,r+2*k)?))).collect::<Result<_,String>>()?;
        let e=nt+0x20*i;
        events.push(json!({"record_offset":r,"bank_id":format!("{:04x}",be16(d,r+0x1c)?),"name":cstr(d,names_base+be32(d,e)? as usize)?,"event_id":format!("{:08x}",be32(d,e+4)?),"record_words":words}));
    }
    Ok(json!({"version":3,"subversion":0x15,"header_words":(0..12).map(|k|be16(d,4+2*k).map(|x|format!("{x:04x}"))).collect::<Result<Vec<_>,_>>()?,"events":events}))
}

/// Shift-JIS -> Unicode through the Wii OS table (`OSUTF32toSJIS` pages, extracted by tools/extract_sjis_table.py).
pub fn sjis(b:&[u8])->String{
    static T:&[u8]=include_bytes!("sjis_table.bin");
    let look=|c:u16|->Option<char>{
        let (mut lo,mut hi)=(0usize,T.len()/4);
        while lo<hi{let m=(lo+hi)/2;let k=u16::from_be_bytes([T[4*m],T[4*m+1]]);if k==c{return char::from_u32(u16::from_be_bytes([T[4*m+2],T[4*m+3]]) as u32)}if k<c{lo=m+1}else{hi=m}}
        None
    };
    let mut out=String::new();let mut i=0;
    while i<b.len(){
        let c=b[i];
        if c<0x80{out.push(c as char);i+=1}
        else if (0xa1..=0xdf).contains(&c){out.push(look(c as u16).unwrap_or('\u{fffd}'));i+=1}
        else if i+1<b.len(){out.push(look(((c as u16)<<8)|b[i+1] as u16).unwrap_or('\u{fffd}'));i+=2}
        else{out.push('\u{fffd}');i+=1}
    }
    out
}

const UNUSED:&str="not loaded by this build: textinput WithAtok/WithZi are stubs and no code references zi.arc or the .atd files";

/// Zi eZiText Nintendo word lists (`keyboard/zi.arc::zi/*.znd`): u32 count, u32 offsets, UTF-16BE NUL-terminated words.
pub fn znd(d:&[u8])->Result<Value,String>{
    let n=be32(d,0)? as usize;if 4+4*n>d.len(){return Err("word table exceeds file".into())}
    let words:Vec<String>=(0..n).map(|i|{let o=be32(d,4+4*i)? as usize;let mut u=vec![];let mut p=o;while let Ok(c)=be16(d,p){if c==0{break}u.push(c);p+=2}Ok(String::from_utf16_lossy(&u))}).collect::<Result<_,String>>()?;
    Ok(json!({"words":words,"note":UNUSED}))
}
/// Zi eZiText compressed system lexicon (`*.zsd`): header words, the frequency-ordered alphabet (UTF-16BE) and the
/// packed lexicon bytes.  The lexicon coding belongs to Zi's engine, which this build does not contain.
pub fn zsd(d:&[u8])->Result<Value,String>{
    let words:Vec<String>=(0..0xc8/4).map(|k|be32(d,4*k).map(|x|format!("{x:08x}"))).collect::<Result<_,_>>()?;
    // +0xca: byte length of the alphabet block = n UTF-16BE characters followed by the same n characters as 8-bit codes.
    let len=be16(d,0xca)? as usize;if len%3!=0{return Err(format!("alphabet block length {len} is not 3 x characters"))}
    let n=len/3;let alpha:Vec<u16>=(0..n).map(|i|be16(d,0xcc+2*i)).collect::<Result<_,_>>()?;
    let bytes=d.get(0xcc+2*n..0xcc+3*n).ok_or("alphabet past end")?;
    // Latin-1 characters must agree between the two copies; others (e.g. U+0153 in the French lexicons) carry a code-page byte.
    if alpha.iter().zip(bytes).any(|(&u,&b)|u<0x100&&u!=b as u16){return Err("8-bit alphabet copy disagrees with the UTF-16 table".into())}
    let codepage:Vec<Value>=alpha.iter().zip(bytes).filter(|(u,_)|**u>=0x100).map(|(&u,&b)|json!({"char":String::from_utf16_lossy(&[u]),"byte":b})).collect();
    let p=0xcc+len;
    Ok(json!({"header_words":words,"alphabet":String::from_utf16_lossy(&alpha),"alphabet_code_page":codepage,"lexicon_offset":p,"lexicon_bytes":d.len().saturating_sub(p),"note":UNUSED}))
}
/// JustSystems ATOK dictionaries (`keyboard/*.atd`, magic `ATAD`): Shift-JIS title, copyright, build date, section
/// table (+0xb0) and the dictionary body.
pub fn atd(d:&[u8])->Result<Value,String>{
    if d.get(..4)!=Some(b"ATAD"){
        // ATOK system/user dictionaries: binary header (u16 magic 0x800d / 0xe00a) and section offset table.
        return Ok(json!({"magic":format!("{:04x}",be16(d,0)?),"header_words":(0..64).map(|k|be32(d,4*k).map(|x|format!("{x:08x}"))).collect::<Result<Vec<_>,_>>()?,"body_bytes":d.len(),"note":UNUSED}))
    }
    let z=|o:usize,n:usize|{let s=&d[o.min(d.len())..(o+n).min(d.len())];s[..s.iter().position(|&c|c==0).unwrap_or(s.len())].to_vec()};
    Ok(json!({"words":[format!("{:08x}",be32(d,4)?),format!("{:08x}",be32(d,8)?)],"title":sjis(&z(0xc,0x44)),"copyright":sjis(&z(0x54,0x40)),"date":sjis(&z(0x94,0x1c)),
        "sections":(0..6).map(|k|be32(d,0xb0+4*k)).collect::<Result<Vec<_>,_>>()?,"body_bytes":d.len().saturating_sub(0x100),"note":UNUSED}))
}

/// EA VP6 movie: every video frame decoded (key frames and one frame per second written as PNG, or all frames when
/// `EAGL_VP6_ALL_FRAMES` is set), per-frame log, interleaved EA audio as WAV.
fn vp6_movie(d:&[u8])->Out{
    let fmt="EA VP6 movie";
    let m=match crate::vp6::movie(d){Ok(m)=>m,Err(e)=>return Out::failed(fmt,e)};
    let all=std::env::var_os("EAGL_VP6_ALL_FRAMES").is_some();
    let fps=if m.rate.1!=0{m.rate.0 as f64/m.rate.1 as f64}else{0.};
    let every=(fps.round() as usize).max(1);
    let mut dec=crate::vp6::Decoder::new();let mut o=Out::new(fmt);let mut frames=vec![];let mut audio=vec![];let mut n=0usize;
    for (tag,at,size) in &m.chunks{
        let body=&d[at+8..at+size];
        match tag{
            b"MV0K"|b"MV0F"=>{
                match dec.decode(body){
                    Ok(key)=>{
                        let cur=dec.current().unwrap();
                        frames.push(json!({"index":n,"key":key,"bytes":body.len(),"luma_sha256":crate::sha256::hex(&cur.p[0])}));
                        if all||key||n%every==0{let (dw,dh)=dec.display;match export::png(&cur.rgba(dw,dh),dw,dh){Ok(p)=>o=o.file(&format!("frames/{n:05}.png"),p),Err(e)=>return Out::failed(fmt,e)}}
                    }
                    Err(e)=>return Out::failed(fmt,format!("frame {n}: {e}")),
                }
                n+=1;
            }
            b"SCHl"|b"SCCl"|b"SCDl"|b"SCEl"=>audio.extend_from_slice(&d[*at..at+size]),
            b"MVhd"=>{}
            t=>return Out::failed(fmt,format!("unknown chunk {}",String::from_utf8_lossy(t))),
        }
    }
    let mut info=json!({"width":m.width,"height":m.height,"declared_frames":m.frames,"decoded_frames":n,"frame_rate":fps,"coded":[dec.w,dec.h],"display":[dec.display.0,dec.display.1],"frames":frames,"frames_written":if all{"all"}else{"key frames and one per second"}});
    let mut bad=vec![];
    for (i,&(a,b)) in crate::audio::streams(&audio).iter().enumerate(){
        let r=match crate::audio::parse_header(&audio[a..b]).map(|h|h.0.codec){Ok(0x0a)=>crate::audio::decode_xa(&audio[a..b]),Ok(0x04)=>crate::audio::decode_utk(&audio[a..b]),_=>crate::audio::decode(&audio[a..b],None)};
        match r{Ok(p)=>{info["audio"]=json!({"sample_rate":p.sample_rate,"channels":p.channels,"samples":p.samples.len()/p.channels.max(1)});o=o.file(&format!("audio{i}.wav"),crate::audio::to_wav(&p))}Err(e)=>bad.push(format!("audio stream {i}: {e}"))}
    }
    o=o.json("json",&info);
    if n as u32!=m.frames{bad.push(format!("{n} frames decoded, header declares {}",m.frames))}
    if bad.is_empty(){o}else{o.partial(bad.join("; "))}
}

pub fn decode(ctx:&Ctx,src:&str,name:&str,ext:&str,d:&[u8])->Out{
    match ext{
        "gfn"=>match gfn(d){
            Ok((v,sheet))=>{let mut o=Out::new("EA FntG font").json("json",&v);if let Some((px,w,h))=sheet{match export::png(&px,w,h){Ok(p)=>o=o.file("png",p),Err(e)=>return Out::failed("EA FntG font",e)}}o}
            Err(e)=>Out::failed("EA FntG font",e),
        },
        "csi"=>match csi(d){Ok(v)=>Out::new("Csis interface registry").json("json",&v),Err(e)=>Out::failed("Csis interface registry",e)},
        "evt"=>match spch_evt(d){Ok(v)=>Out::new("SPCH event database").json("json",&v),Err(e)=>Out::failed("SPCH event database",e)},
        "hdr"=>{
            // The sample data bank: DATA/files/data/audio/speech/spchdat.viv::<name>.dat
            let stem=name.rsplit_once('.').map(|(s,_)|s).unwrap_or(name);
            let dat=src.split("::").next().and_then(|outer|std::path::Path::new(outer).parent().map(|p|p.join("spchdat.viv"))).and_then(|p|crate::archive::read_virtual(&format!("{}::{stem}.dat",p.display())).ok()).map(|x|x.0);
            let _=ctx;
            match spch_hdr(d,dat.as_deref()){Ok(v)=>{let o=Out::new("SPCH speech bank header").json("json",&v);if dat.is_none(){o.partial("data bank not found; stream starts not verified")}else{o}}Err(e)=>Out::failed("SPCH speech bank header",e)}
        }
        "vp6"=>vp6_movie(d),
        "znd"=>match znd(d){Ok(v)=>Out::new("Zi eZiText word list").json("json",&v),Err(e)=>Out::failed("Zi eZiText word list",e)},
        "zsd"=>match zsd(d){Ok(v)=>Out::new("Zi eZiText system lexicon").json("json",&v).partial(format!("header and alphabet decoded; compressed lexicon is third-party Zi data ({UNUSED})")),Err(e)=>Out::failed("Zi eZiText system lexicon",e)},
        "atd"=>match atd(d){Ok(v)=>Out::new("ATOK dictionary").json("json",&v).partial(format!("header decoded; dictionary body is third-party JustSystems ATOK data ({UNUSED})")),Err(e)=>Out::failed("ATOK dictionary",e)},
        _=>Out::unsupported(&format!(".{ext}"),"no native decoder"),
    }
}
