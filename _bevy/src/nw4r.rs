//! Nintendo NW4R binary formats used by the Home Button menu and keyboard (`home/*.arc`, `keyboard/*`):
//! `RLYT` layouts, `RLAN` layout animations, `RFNT` fonts, `RWAV` waves, `RSAR` sound archives (with their
//! `RWSD`/`RWAR`/`RBNK`/`RSEQ` members), and the channel's `BNS` banner sound.
//!
//! All are big-endian with the common header `magic, BOM 0xFEFF, version, file size, header size, section count`.
//! Material records are validated by construction: the flag-selected blocks must end exactly where the next material
//! begins (see `material`).  The runtime that reads these is the `nw4hbm` library in the executable
//! (`nw4hbm::lyt::*`, `nw4hbm::ut::ResFont`, `nw4hbm::snd::*`).
use crate::{audio,tpl};
use serde_json::{json,Value};

fn be16(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_be_bytes([b[0],b[1]])).ok_or_else(||format!("read past end at {o:#x}"))}
fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
fn bef(d:&[u8],o:usize)->Result<f32,String>{Ok(f32::from_bits(be32(d,o)?))}
fn u8at(d:&[u8],o:usize)->Result<u8,String>{d.get(o).copied().ok_or_else(||format!("read past end at {o:#x}"))}
fn fixed(d:&[u8],o:usize,n:usize)->Result<String,String>{let s=d.get(o..o+n).ok_or("name past end")?;Ok(String::from_utf8_lossy(&s[..s.iter().position(|&c|c==0).unwrap_or(n)]).into_owned())}
fn cstr(d:&[u8],o:usize)->Result<String,String>{let s=d.get(o..).ok_or("string past end")?;Ok(String::from_utf8_lossy(&s[..s.iter().position(|&c|c==0).ok_or("unterminated string")?]).into_owned())}
fn floats(d:&[u8],o:usize,n:usize)->Result<Vec<f32>,String>{(0..n).map(|i|bef(d,o+4*i)).collect()}
fn rgba(d:&[u8],o:usize)->Result<String,String>{Ok(format!("{:08x}",be32(d,o)?))}

pub struct Header{pub magic:[u8;4],pub version:u16,pub size:u32,pub header:usize,pub sections:usize}
pub fn header(d:&[u8])->Result<Header,String>{
    let magic:[u8;4]=d.get(..4).ok_or("short file")?.try_into().unwrap();
    if be16(d,4)?!=0xfeff{return Err("NW4R byte-order mark missing".into())}
    let size=be32(d,8)?;if size as usize!=d.len(){return Err(format!("header size {size} but file is {} bytes",d.len()))}
    Ok(Header{magic,version:be16(d,6)?,size,header:be16(d,12)? as usize,sections:be16(d,14)? as usize})
}
/// (magic, offset, size) of each block after the header.
fn blocks(d:&[u8],h:&Header)->Result<Vec<([u8;4],usize,usize)>,String>{
    let mut o=h.header;let mut v=vec![];
    for _ in 0..h.sections{let m:[u8;4]=d.get(o..o+4).ok_or("section header past end")?.try_into().unwrap();let s=be32(d,o+4)? as usize;if s<8||o+s>d.len(){return Err(format!("section {} size {s:#x} at {o:#x}",String::from_utf8_lossy(&m)))}v.push((m,o,s));o+=s}
    Ok(v)
}
fn name_table(d:&[u8],o:usize)->Result<Vec<String>,String>{
    let n=be16(d,o+8)? as usize;let base=o+12;
    (0..n).map(|i|cstr(d,base+be32(d,base+8*i)? as usize)).collect()
}

fn pane(d:&[u8],o:usize)->Result<Value,String>{
    Ok(json!({"flags":u8at(d,o+8)?,"origin":u8at(d,o+9)?,"alpha":u8at(d,o+10)?,"name":fixed(d,o+12,16)?,"user_data":fixed(d,o+28,8)?,
        "translate":floats(d,o+36,3)?,"rotate":floats(d,o+48,3)?,"scale":floats(d,o+60,2)?,"size":floats(d,o+68,2)?}))
}
fn texcoords(d:&[u8],o:usize,n:usize)->Result<Vec<Vec<f32>>,String>{(0..n).map(|i|floats(d,o+32*i,8)).collect()}

/// `nw4r::lyt::res::Material`: fixed part then flag-selected blocks in this order: texture maps (4), texture SRTs
/// (20), texture-coordinate generators (4), channel control (4), material colour (4), TEV swap table (4), indirect
/// SRTs (20), indirect stages (4), TEV stages (16), alpha compare (4), blend mode (4).  Returns (json, end offset).
fn material(d:&[u8],o:usize)->Result<(Value,usize),String>{
    let colors:Vec<Vec<i16>>=(0..3).map(|c|(0..4).map(|k|Ok(be16(d,o+20+8*c+2*k)? as i16)).collect::<Result<Vec<_>,String>>()).collect::<Result<_,_>>()?;
    let kcolors:Vec<String>=(0..4).map(|k|rgba(d,o+44+4*k)).collect::<Result<_,_>>()?;
    let f=be32(d,o+60)?;
    let (ntm,nsrt,ntg,swap,nisrt,nist,ntev,ac,bm,cc,mc)=((f&15) as usize,((f>>4)&15) as usize,((f>>8)&15) as usize,(f>>12)&1,((f>>13)&3) as usize,((f>>15)&7) as usize,((f>>18)&31) as usize,(f>>23)&1,(f>>24)&1,(f>>25)&1,(f>>27)&1);
    let mut p=o+64;let mut m=json!({"name":fixed(d,o,20)?,"tev_colors":colors,"konst_colors":kcolors,"flags":format!("{f:08x}")});
    m["texture_maps"]=json!((0..ntm).map(|i|Ok(json!({"texture":be16(d,p+4*i)?,"wrap_s":u8at(d,p+4*i+2)?,"wrap_t":u8at(d,p+4*i+3)?}))).collect::<Result<Vec<_>,String>>()?);p+=4*ntm;
    m["texture_srts"]=json!((0..nsrt).map(|i|floats(d,p+20*i,5)).collect::<Result<Vec<_>,_>>()?);p+=20*nsrt;
    m["texcoord_gens"]=json!((0..ntg).map(|i|Ok(format!("{:08x}",be32(d,p+4*i)?))).collect::<Result<Vec<_>,String>>()?);p+=4*ntg;
    if cc!=0{m["channel_control"]=json!(format!("{:08x}",be32(d,p)?));p+=4}
    if mc!=0{m["material_color"]=json!(rgba(d,p)?);p+=4}
    if swap!=0{m["tev_swap"]=json!(format!("{:08x}",be32(d,p)?));p+=4}
    m["indirect_srts"]=json!((0..nisrt).map(|i|floats(d,p+20*i,5)).collect::<Result<Vec<_>,_>>()?);p+=20*nisrt;
    m["indirect_stages"]=json!((0..nist).map(|i|Ok(format!("{:08x}",be32(d,p+4*i)?))).collect::<Result<Vec<_>,String>>()?);p+=4*nist;
    m["tev_stages"]=json!((0..ntev).map(|i|Ok(hex(d.get(p+16*i..p+16*i+16).ok_or("TEV stage past end")?))).collect::<Result<Vec<_>,String>>()?);p+=16*ntev;
    if ac!=0{m["alpha_compare"]=json!(format!("{:08x}",be32(d,p)?));p+=4}
    if bm!=0{m["blend_mode"]=json!(format!("{:08x}",be32(d,p)?));p+=4}
    Ok((m,p))
}
fn hex(b:&[u8])->String{b.iter().map(|x|format!("{x:02x}")).collect()}

/// `RLYT`: every section decoded; pane hierarchy rebuilt from the `pas1`/`pae1` markers, groups from `grs1`/`gre1`.
pub fn brlyt(d:&[u8])->Result<Value,String>{
    let h=header(d)?;if &h.magic!=b"RLYT"{return Err("not RLYT".into())}
    let mut out=json!({"version":h.version});let mut stack:Vec<Vec<Value>>=vec![vec![]];let mut gstack:Vec<Vec<Value>>=vec![vec![]];
    let mut unknown=vec![];
    for (m,o,s) in blocks(d,&h)?{
        match &m{
            b"lyt1"=>out["layout"]=json!({"centered":u8at(d,o+8)?,"size":floats(d,o+12,2)?}),
            b"txl1"=>out["textures"]=json!(name_table(d,o)?),
            b"fnl1"=>out["fonts"]=json!(name_table(d,o)?),
            b"mat1"=>{
                let n=be16(d,o+8)? as usize;let offs:Vec<usize>=(0..n).map(|i|Ok(o+be32(d,o+12+4*i)? as usize)).collect::<Result<_,String>>()?;
                let mut mats=vec![];
                for (i,&mo) in offs.iter().enumerate(){
                    let (mj,end)=material(d,mo)?;let next=offs.get(i+1).copied().unwrap_or(o+s);
                    if end!=next{return Err(format!("material {i} ends at {end:#x}, next begins at {next:#x}"))}
                    mats.push(mj);
                }
                out["materials"]=json!(mats);
            }
            b"pan1"|b"bnd1"|b"pic1"|b"txt1"|b"wnd1"=>{
                let mut p=pane(d,o)?;p["kind"]=json!(String::from_utf8_lossy(&m));let q=o+0x4c;
                match &m{
                    b"pic1"=>{p["vertex_colors"]=json!((0..4).map(|k|rgba(d,q+4*k)).collect::<Result<Vec<_>,_>>()?);p["material"]=json!(be16(d,q+16)?);p["texcoords"]=json!(texcoords(d,q+20,u8at(d,q+18)? as usize)?)}
                    b"txt1"=>{
                        let (buf,len,mat,font)=(be16(d,q)?,be16(d,q+2)? as usize,be16(d,q+4)?,be16(d,q+6)?);let so=o+be32(d,q+12)? as usize;
                        let units:Vec<u16>=(0..len/2).map(|i|be16(d,so+2*i)).collect::<Result<_,_>>()?;
                        p["text"]=json!(String::from_utf16_lossy(&units).trim_end_matches('\0'));
                        p["text_buffer_bytes"]=json!(buf);p["material"]=json!(mat);p["font"]=json!(font);p["text_position"]=json!(u8at(d,q+8)?);p["text_alignment"]=json!(u8at(d,q+9)?);
                        p["colors"]=json!([rgba(d,q+16)?,rgba(d,q+20)?]);p["font_size"]=json!(floats(d,q+24,2)?);p["char_space"]=json!(bef(d,q+32)?);p["line_space"]=json!(bef(d,q+36)?);
                    }
                    b"wnd1"=>{
                        let nf=u8at(d,q+16)? as usize;let co=o+be32(d,q+20)? as usize;let fo=o+be32(d,q+24)? as usize;
                        p["inflation"]=json!(floats(d,q,4)?);
                        p["content"]=json!({"vertex_colors":(0..4).map(|k|rgba(d,co+4*k)).collect::<Result<Vec<_>,_>>()?,"material":be16(d,co+16)?,"texcoords":texcoords(d,co+20,u8at(d,co+18)? as usize)?});
                        p["frames"]=json!((0..nf).map(|i|{let f=o+be32(d,fo+4*i)? as usize;Ok(json!({"material":be16(d,f)?,"flip":u8at(d,f+2)?}))}).collect::<Result<Vec<_>,String>>()?);
                    }
                    _=>{}
                }
                stack.last_mut().unwrap().push(p);
            }
            b"pas1"=>stack.push(vec![]),
            b"pae1"=>{let kids=stack.pop().ok_or("unbalanced pae1")?;let parent=stack.last_mut().ok_or("unbalanced pae1")?;
                match parent.last_mut(){Some(p)=>p["children"]=json!(kids),None=>return Err("pas1 without a parent pane".into())}}
            b"grp1"=>{let n=be16(d,o+24)? as usize;gstack.last_mut().unwrap().push(json!({"name":fixed(d,o+8,16)?,"panes":(0..n).map(|i|fixed(d,o+28+16*i,16)).collect::<Result<Vec<_>,_>>()?}))}
            b"grs1"=>gstack.push(vec![]),
            b"gre1"=>{let kids=gstack.pop().ok_or("unbalanced gre1")?;match gstack.last_mut().and_then(|g|g.last_mut()){Some(g)=>g["children"]=json!(kids),None=>return Err("grs1 without a parent group".into())}}
            _=>unknown.push(String::from_utf8_lossy(&m).into_owned()),
        }
    }
    if stack.len()!=1||gstack.len()!=1{return Err("unbalanced pane/group markers".into())}
    out["panes"]=json!(stack.pop().unwrap());out["groups"]=json!(gstack.pop().unwrap());
    if !unknown.is_empty(){return Err(format!("unknown RLYT sections {unknown:?}"))}
    Ok(out)
}

/// `RLAN`: `pat1` (animation tag) and `pai1` (per pane/material curves: RLPA pane SRT, RLTS texture SRT,
/// RLVI visibility, RLVC vertex colour, RLMC material colour, RLTP texture pattern, RLIM indirect matrix).
pub fn brlan(d:&[u8])->Result<Value,String>{
    let h=header(d)?;if &h.magic!=b"RLAN"{return Err("not RLAN".into())}
    let mut out=json!({"version":h.version});
    for (m,o,_s) in blocks(d,&h)?{
        match &m{
            b"pai1"=>{
                let (frames,looped,nfiles,ncont,coff)=(be16(d,o+8)?,u8at(d,o+10)?,be16(d,o+12)? as usize,be16(d,o+14)? as usize,be32(d,o+16)? as usize);
                let files:Vec<String>=(0..nfiles).map(|i|cstr(d,o+20+be32(d,o+20+4*i)? as usize)).collect::<Result<_,_>>()?;
                let mut contents=vec![];
                for i in 0..ncont{
                    let c=o+be32(d,o+coff+4*i)? as usize;let n=u8at(d,c+20)? as usize;
                    let mut infos=vec![];
                    for k in 0..n{
                        let a=c+be32(d,c+24+4*k)? as usize;let kind=String::from_utf8_lossy(d.get(a..a+4).ok_or("anim info past end")?).into_owned();let nt=u8at(d,a+4)? as usize;
                        let mut targets=vec![];
                        for t in 0..nt{
                            let q=a+be32(d,a+8+4*t)? as usize;let (id,target,curve,nk,ko)=(u8at(d,q)?,u8at(d,q+1)?,u8at(d,q+2)?,be16(d,q+4)? as usize,be32(d,q+8)? as usize);
                            let keys:Vec<Value>=match curve{
                                1=>(0..nk).map(|j|Ok(json!([bef(d,q+ko+8*j)?,be16(d,q+ko+8*j+4)?]))).collect::<Result<_,String>>()?,
                                2=>(0..nk).map(|j|Ok(json!(floats(d,q+ko+12*j,3)?))).collect::<Result<_,String>>()?,
                                c=>return Err(format!("unknown curve type {c}")),
                            };
                            targets.push(json!({"id":id,"target":target,"curve":if curve==1{"step"}else{"hermite"},"keys":keys}));
                        }
                        infos.push(json!({"kind":kind,"targets":targets}));
                    }
                    contents.push(json!({"name":fixed(d,c,20)?,"type":u8at(d,c+21)?,"animations":infos}));
                }
                out["frames"]=json!(frames);out["loop"]=json!(looped!=0);out["files"]=json!(files);out["contents"]=json!(contents);
            }
            b"pat1"=>{
                let (order,ngroups,name_off,groups_off)=(be16(d,o+8)?,be16(d,o+10)? as usize,be32(d,o+12)? as usize,be32(d,o+16)? as usize);
                out["tag"]=json!({"order":order,"name":cstr(d,o+name_off)?,"groups":(0..ngroups).map(|i|fixed(d,o+groups_off+20*i,16)).collect::<Result<Vec<_>,_>>()?,
                    "start":be16(d,o+20)?,"end":be16(d,o+22)?,"child_binding":u8at(d,o+24)?});
            }
            _=>return Err(format!("unknown RLAN section {}",String::from_utf8_lossy(&m))),
        }
    }
    Ok(out)
}

/// `RFNT`: font information, glyph sheets (decoded to RGBA through the GX texture decoder), width and code maps.
pub fn brfnt(d:&[u8])->Result<(Value,Vec<(String,Vec<u8>,usize,usize)>),String>{
    let h=header(d)?;if &h.magic!=b"RFNT"{return Err(format!("not RFNT ({})",String::from_utf8_lossy(&h.magic)))}
    let mut info=json!({"version":h.version});let mut sheets=vec![];let mut widths=vec![];let mut maps=vec![];
    for (m,o,_s) in blocks(d,&h)?{
        match &m{
            b"FINF"=>info["info"]=json!({"font_type":u8at(d,o+8)?,"line_feed":u8at(d,o+9)? as i8,"alternate_char":be16(d,o+10)?,
                "default_width":[u8at(d,o+12)? as i8,u8at(d,o+13)?,u8at(d,o+14)? as i8],"encoding":u8at(d,o+15)?,"height":u8at(d,o+28)?,"width":u8at(d,o+29)?,"ascent":u8at(d,o+30)?}),
            b"TGLP"=>{
                let (cw,ch,base,maxw,ssize,n,fmt,row,line,sw,sh,img)=(u8at(d,o+8)?,u8at(d,o+9)?,u8at(d,o+10)? as i8,u8at(d,o+11)?,be32(d,o+12)? as usize,be16(d,o+16)? as usize,be16(d,o+18)? as u32,be16(d,o+20)?,be16(d,o+22)?,be16(d,o+24)? as usize,be16(d,o+26)? as usize,be32(d,o+28)? as usize);
                info["glyphs"]=json!({"cell_width":cw,"cell_height":ch,"baseline":base,"max_char_width":maxw,"sheet_size":ssize,"sheets":n,"format":fmt,"columns":row,"rows":line,"sheet_width":sw,"sheet_height":sh});
                for i in 0..n{
                    let raw=d.get(img+i*ssize..img+(i+1)*ssize).ok_or("glyph sheet past end")?;
                    let e=tpl::Entry{index:i,width:sw,height:sh,format:fmt,format_name:"",offset:0,size:ssize,palette:None};
                    let (px,w,hh)=tpl::decode(raw,&e)?;sheets.push((format!("sheet{i:02}"),px,w,hh));
                }
            }
            b"CWDH"=>{let (a,b)=(be16(d,o+8)? as usize,be16(d,o+10)? as usize);widths.push(json!({"first":a,"last":b,"widths":(a..=b).map(|i|{let q=o+16+3*(i-a);Ok(json!([u8at(d,q)? as i8,u8at(d,q+1)?,u8at(d,q+2)? as i8]))}).collect::<Result<Vec<_>,String>>()?}))}
            b"CMAP"=>{
                let (a,b,method)=(be16(d,o+8)? as u32,be16(d,o+10)? as u32,be16(d,o+12)?);let q=o+20;
                let map:Value=match method{
                    0=>json!({"direct_offset":be16(d,q)?}),
                    1=>json!((a..=b).map(|c|be16(d,q+2*(c-a) as usize)).collect::<Result<Vec<_>,_>>()?),
                    2=>{let n=be16(d,q)? as usize;json!((0..n).map(|i|Ok([be16(d,q+2+4*i)?,be16(d,q+4+4*i)?])).collect::<Result<Vec<_>,String>>()?)}
                    m=>return Err(format!("unknown CMAP method {m}")),
                };
                maps.push(json!({"first":a,"last":b,"method":method,"map":map}));
            }
            _=>return Err(format!("unknown RFNT section {}",String::from_utf8_lossy(&m))),
        }
    }
    info["widths"]=json!(widths);info["code_maps"]=json!(maps);
    Ok((info,sheets))
}

/// DSP-ADPCM or PCM channel data -> interleaved i16.
fn wave_channels(fmt:u8,chans:&[(&[u8],Option<&[u8]>)],samples:usize)->Result<Vec<i16>,String>{
    let per:Vec<Vec<i16>>=chans.iter().map(|(data,coefs)|match fmt{
        0=>Ok(data.iter().take(samples).map(|&b|((b as i8 as i16)<<8)).collect()),
        1=>Ok(data.chunks_exact(2).take(samples).map(|c|i16::from_be_bytes([c[0],c[1]])).collect()),
        2=>audio::decode_dsp(data,coefs.ok_or("ADPCM channel without coefficients")?,samples),
        f=>Err(format!("wave format {f}")),
    }).collect::<Result<_,_>>()?;
    let n=per.iter().map(|c|c.len()).min().unwrap_or(0);
    Ok((0..n).flat_map(|i|per.iter().map(move|c|c[i])).collect())
}
fn nibbles_to_samples(n:u32)->usize{(n as usize/16)*14+((n as usize%16).saturating_sub(2))}

/// `RWAV` (also the members of `RWAR`): INFO block (format, loop, channels, rate, loop/end, channel table) + DATA.
pub fn rwav(d:&[u8])->Result<(Value,audio::Pcm),String>{
    let h=header(d)?;if &h.magic!=b"RWAV"{return Err("not RWAV".into())}
    let bl=blocks(d,&h)?;
    let (_,i,_)=*bl.iter().find(|b|&b.0==b"INFO").ok_or("RWAV without INFO")?;
    let (_,data_blk,_)=*bl.iter().find(|b|&b.0==b"DATA").ok_or("RWAV without DATA")?;
    let w=i+8;let (fmt,lp,ch,rate,ls,le,tbl)=(u8at(d,w)?,u8at(d,w+1)?,u8at(d,w+2)? as usize,be16(d,w+4)? as u32,be32(d,w+8)?,be32(d,w+12)?,be32(d,w+16)? as usize);
    let samples=if fmt==2{nibbles_to_samples(le)}else{le as usize};
    let mut chans=vec![];
    for c in 0..ch{
        let ci=w+be32(d,w+tbl+4*c)? as usize;let off=be32(d,ci)? as usize;let ai=be32(d,ci+4)? as usize;
        let data=d.get(data_blk+8+off..).ok_or("channel data past end")?;
        let coefs=if fmt==2{Some(d.get(w+ai..w+ai+32).ok_or("coefficients past end")?)}else{None};
        chans.push((data,coefs));
    }
    let pcm=wave_channels(fmt,&chans,samples)?;
    Ok((json!({"format":(["pcm8","pcm16","dsp-adpcm"].get(fmt as usize)),"loop":lp!=0,"channels":ch,"sample_rate":rate,"loop_start":ls,"loop_end":le,"samples":samples}),
        audio::Pcm{sample_rate:rate,channels:ch,samples:pcm,frames:0,stats:Default::default()}))
}

/// `BNS ` banner sound (IMD5-wrapped in `opening.bnr::meta/sound.bin`): INFO (codec, loop, channels, rate, loop start,
/// sample count, per-channel ADPCM info) + DATA.
pub fn bns(d:&[u8])->Result<(Value,audio::Pcm),String>{
    if d.get(..4)!=Some(b"BNS "){return Err("not BNS".into())}
    let io=be32(d,0x10)? as usize;let doff=be32(d,0x18)? as usize;
    if d.get(io..io+4)!=Some(b"INFO")||d.get(doff..doff+4)!=Some(b"DATA"){return Err("BNS blocks missing".into())}
    let w=io+8;let (codec,lp,ch,rate,ls,n)=(u8at(d,w)?,u8at(d,w+1)?,u8at(d,w+2)? as usize,be16(d,w+4)? as u32,be32(d,w+8)?,be32(d,w+12)? as usize);
    let tbl=w+be32(d,w+16)? as usize;let mut chans=vec![];
    for c in 0..ch{
        let ci=w+be32(d,tbl+4*c)? as usize;let off=be32(d,ci)? as usize;let ai=w+be32(d,ci+4)? as usize;
        chans.push((d.get(doff+8+off..).ok_or("BNS channel past end")?,Some(d.get(ai..ai+32).ok_or("BNS coefficients past end")?)));
    }
    if codec!=0{return Err(format!("BNS codec {codec}"))}
    let pcm=wave_channels(2,&chans,n)?;
    Ok((json!({"codec":"dsp-adpcm","loop":lp!=0,"channels":ch,"sample_rate":rate,"loop_start":ls,"samples":n}),audio::Pcm{sample_rate:rate,channels:ch,samples:pcm,frames:0,stats:Default::default()}))
}

/// Home Button menu Wii Remote speaker sounds (`speakerse.arc::*.bwav`): headerless mono signed 16-bit big-endian PCM.
/// `homebutton::RemoteSpk::UpdateSpeaker` (0x800ba24c) reads 40 samples (`lha`) per tick, scales them by volume/10 and
/// WENC-encodes them; `Start` (0x800ba630) sets the tick to `OSNanosecondsToTicks(6666667)`, i.e. 6000 samples/s.
pub fn speaker_pcm(d:&[u8])->audio::Pcm{
    audio::Pcm{sample_rate:6000,channels:1,samples:d.chunks_exact(2).map(|c|i16::from_be_bytes([c[0],c[1]])).collect(),frames:0,stats:Default::default()}
}

/// `WAVE` block of an `RWSD`/`RBNK`: wave infos (the RWAV INFO body) whose sample data live in the sound archive
/// group's wave region at `data_location + channel data offset`.
pub fn wave_block(file:&[u8],waves:&[u8])->Result<Vec<Result<(Value,audio::Pcm),String>>,String>{
    let h=header(file)?;let bl=blocks(file,&h)?;
    let Some(&(_,wb,_))=bl.iter().find(|b|&b.0==b"WAVE") else{return Ok(vec![])};
    let base=wb+8;let n=be32(file,base)? as usize;
    Ok((0..n).map(|i|->Result<(Value,audio::Pcm),String>{
        let w=base+be32(file,base+4+8*i+4)? as usize;
        let (fmt,lp,ch,rate,ls,le,tbl,loc)=(u8at(file,w)?,u8at(file,w+1)?,u8at(file,w+2)? as usize,be16(file,w+4)? as u32,be32(file,w+8)?,be32(file,w+12)?,be32(file,w+16)? as usize,be32(file,w+20)? as usize);
        let samples=if fmt==2{nibbles_to_samples(le+1)}else{le as usize+1};
        let mut chans=vec![];
        for c in 0..ch{
            let ci=w+be32(file,w+tbl+4*c)? as usize;let off=be32(file,ci)? as usize;let ai=be32(file,ci+4)? as usize;
            chans.push((waves.get(loc+off..).ok_or("wave data past end")?,if fmt==2{Some(file.get(w+ai..w+ai+32).ok_or("coefficients past end")?)}else{None}));
        }
        let pcm=wave_channels(fmt,&chans,samples)?;
        Ok((json!({"index":i,"format":(["pcm8","pcm16","dsp-adpcm"].get(fmt as usize)),"loop":lp!=0,"channels":ch,"sample_rate":rate,"loop_start":ls,"loop_end":le,"data_location":loc,"samples":samples}),
            audio::Pcm{sample_rate:rate,channels:ch,samples:pcm,frames:0,stats:Default::default()}))
    }).collect())
}

/// `RWAR`: TABL of (offset,size) references into DATA, each an `RWAV`.
pub fn rwar(d:&[u8])->Result<Vec<Result<(Value,audio::Pcm),String>>,String>{
    let h=header(d)?;if &h.magic!=b"RWAR"{return Err("not RWAR".into())}
    let bl=blocks(d,&h)?;
    let (_,t,_)=*bl.iter().find(|b|&b.0==b"TABL").ok_or("RWAR without TABL")?;
    let (_,dat,_)=*bl.iter().find(|b|&b.0==b"DATA").ok_or("RWAR without DATA")?;
    let n=be32(d,t+8)? as usize;
    Ok((0..n).map(|i|{let (o,s)=(be32(d,t+12+12*i+4)? as usize,be32(d,t+12+12*i+8)? as usize);rwav(d.get(dat+o..dat+o+s).ok_or("RWAR entry past end")?)}).collect())
}

/// `RSAR`: symbol strings, sound/bank/player/group tables, and every group file.  Group files are returned for the
/// caller to decode (RWSD/RBNK/RSEQ + their RWAR wave archives).
pub struct Rsar{pub json:Value,pub files:Vec<(String,Vec<u8>,Vec<u8>)>}
pub fn rsar(d:&[u8])->Result<Rsar,String>{
    let h=header(d)?;if &h.magic!=b"RSAR"{return Err("not RSAR".into())}
    let (symb,info,file)=(be32(d,0x10)? as usize,be32(d,0x18)? as usize,be32(d,0x20)? as usize);
    if d.get(symb..symb+4)!=Some(b"SYMB")||d.get(info..info+4)!=Some(b"INFO")||d.get(file..file+4)!=Some(b"FILE"){return Err("RSAR blocks missing".into())}
    // SYMB: string table offset (relative to block+8), then the string list.
    let sb=symb+8;let st=sb+be32(d,sb)? as usize;let nstr=be32(d,st)? as usize;
    let strings:Vec<String>=(0..nstr).map(|i|cstr(d,sb+be32(d,st+4+4*i)? as usize)).collect::<Result<_,_>>()?;
    let sym=|i:u32|->Value{if i==0xffff_ffff{Value::Null}else{json!(strings.get(i as usize))}};
    // INFO: six data references (u32 type-flag, u32 offset relative to block+8): sounds, banks, players, files, groups, sound-archive player info.
    let ib=info+8;let reft=|k:usize|->Result<usize,String>{Ok(ib+be32(d,ib+8*k+4)? as usize)};
    let table=|at:usize|->Result<Vec<usize>,String>{let n=be32(d,at)? as usize;(0..n).map(|i|Ok(ib+be32(d,at+8+8*i)? as usize)).collect()};
    let sounds:Vec<Value>=table(reft(0)?)?.into_iter().map(|s|Ok(json!({"name":sym(be32(d,s)?),"file":be32(d,s+4)?,"player":be32(d,s+8)?,"volume":u8at(d,s+0x14)?,"priority":u8at(d,s+0x15)?,"type":(["?","seq","strm","wave"].get(u8at(d,s+0x16)? as usize))}))).collect::<Result<_,String>>()?;
    let banks:Vec<Value>=table(reft(1)?)?.into_iter().map(|s|Ok(json!({"name":sym(be32(d,s)?),"id":be32(d,s+4)?,"file":be32(d,s+8)?}))).collect::<Result<_,String>>()?;
    let players:Vec<Value>=table(reft(2)?)?.into_iter().map(|s|Ok(json!({"name":sym(be32(d,s)?),"sounds":u8at(d,s+4)?,"heap":be32(d,s+8)?}))).collect::<Result<_,String>>()?;
    let files:Vec<Value>=table(reft(3)?)?.into_iter().map(|s|Ok(json!({"size":be32(d,s)?,"wave_size":be32(d,s+4)?,"entry":be32(d,s+8)? as i32}))).collect::<Result<_,String>>()?;
    // Groups: name, entry number, external path, offset/size of the group's file data and wave data in FILE, item table.
    let mut groups=vec![];let mut out_files=vec![];
    for (gi,g) in table(reft(4)?)?.into_iter().enumerate(){
        let (name,_entry,_ext,off,size,woff,wsize,items)=(be32(d,g)?,be32(d,g+4)?,be32(d,g+8)?,be32(d,g+0x10)? as usize,be32(d,g+0x14)? as usize,be32(d,g+0x18)? as usize,be32(d,g+0x1c)? as usize,be32(d,g+0x24)? as usize);
        let list=table(ib+items)?;
        let mut its=vec![];
        for (k,it) in list.into_iter().enumerate(){
            let (fid,fo,fs,wo,ws)=(be32(d,it)?,be32(d,it+4)? as usize,be32(d,it+8)? as usize,be32(d,it+12)? as usize,be32(d,it+16)? as usize);
            its.push(json!({"file":fid,"offset":fo,"size":fs,"wave_offset":wo,"wave_size":ws}));
            out_files.push((format!("group{gi:02}_item{k:02}_file{fid}"),d.get(off+fo..off+fo+fs).ok_or("group file past end")?.to_vec(),d.get(woff+wo..woff+wo+ws).ok_or("group wave data past end")?.to_vec()));
        }
        groups.push(json!({"name":sym(name),"offset":off,"size":size,"wave_offset":woff,"wave_size":wsize,"items":its}));
    }
    Ok(Rsar{json:json!({"version":h.version,"strings":strings,"sounds":sounds,"banks":banks,"players":players,"files":files,"groups":groups}),files:out_files})
}

/// Generic NW4R member (RWSD/RBNK/RSEQ): header and block list; these hold note/sequence/wave indices whose waves
/// live in the matching RWAR, which is decoded to audio separately.
pub fn blocks_json(d:&[u8])->Result<Value,String>{
    let h=header(d)?;
    Ok(json!({"magic":String::from_utf8_lossy(&h.magic),"version":h.version,"blocks":blocks(d,&h)?.iter().map(|(m,o,s)|json!({"magic":String::from_utf8_lossy(m),"offset":o,"size":s})).collect::<Vec<_>>()}))
}
