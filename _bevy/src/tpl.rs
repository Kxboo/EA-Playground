//! Nintendo TPL texture banks (`.tpl`, magic 0x0020AF30) - Rust port of Remaster/src/formats.py `tpl`/`tpl_rgba`.
//! Directory of (image header, palette header) pairs; images use the GX texture formats (I4, I8, IA4, IA8, RGB565,
//! RGB5A3, RGBA8, C4, C8, C14X2, CMPR).  Base level only.
use crate::gsh::{decode_cmpr,detile_16,detile_rgba8,rgb565,rgb5a3};

#[derive(Debug,Clone)]
pub struct Entry{pub index:usize,pub width:usize,pub height:usize,pub format:u32,pub format_name:&'static str,pub offset:usize,pub size:usize,pub palette:Option<Palette>}
#[derive(Debug,Clone)]
pub struct Palette{pub count:usize,pub format:u32,pub offset:usize}

/// (name, tile width, tile height, bytes per tile)
fn info(fmt:u32)->Option<(&'static str,usize,usize,usize)>{Some(match fmt{0=>("I4",8,8,32),1=>("I8",8,4,32),2=>("IA4",8,4,32),3=>("IA8",4,4,32),4=>("RGB565",4,4,32),5=>("RGB5A3",4,4,32),6=>("RGBA8",4,4,64),8=>("C4",8,8,32),9=>("C8",8,4,32),10=>("C14X2",4,4,32),14=>("CMPR",8,8,32),_=>return None})}

fn be32(d:&[u8],o:usize)->Result<usize,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap()) as usize).ok_or_else(||format!("TPL read past end at {o:#x}"))}
fn be16(d:&[u8],o:usize)->Result<usize,String>{d.get(o..o+2).map(|b|u16::from_be_bytes(b.try_into().unwrap()) as usize).ok_or_else(||format!("TPL read past end at {o:#x}"))}
fn span(d:&[u8],o:usize,n:usize)->Result<&[u8],String>{d.get(o..o.checked_add(n).ok_or("span overflow")?).ok_or_else(||format!("TPL span {o:#x}+{n:#x} outside file"))}

pub fn parse(d:&[u8])->Result<Vec<Entry>,String>{
    if be32(d,0)?!=0x20af30{return Err("Not TPL".into())}
    let (count,table)=(be32(d,4)?,be32(d,8)?);
    span(d,table,count.checked_mul(8).ok_or("count overflow")?)?;
    let mut out=vec![];
    for i in 0..count{
        let (image,palette)=(be32(d,table+i*8)?,be32(d,table+i*8+4)?);
        let (h,w,fmt,off)=(be16(d,image)?,be16(d,image+2)?,be32(d,image+4)? as u32,be32(d,image+8)?);
        let (name,bw,bh,bs)=info(fmt).ok_or_else(||format!("Unknown GX texture format {fmt}"))?;
        let size=w.div_ceil(bw)*h.div_ceil(bh)*bs;
        span(d,off,size)?;
        let pal=if palette!=0{
            let (n,pfmt,poff)=(be16(d,palette)?,be32(d,palette+4)? as u32,be32(d,palette+8)?);
            span(d,poff,n*2)?;Some(Palette{count:n,format:pfmt,offset:poff})
        }else{None};
        out.push(Entry{index:i,width:w,height:h,format:fmt,format_name:name,offset:off,size,palette:pal});
    }
    Ok(out)
}

/// RGBA8 (row-major) of the entry's base level.
pub fn decode(d:&[u8],e:&Entry)->Result<(Vec<u8>,usize,usize),String>{
    let (w,h)=(e.width,e.height);let raw=span(d,e.offset,e.size)?;
    match e.format{
        14=>return Ok((decode_cmpr(raw,w,h),w,h)),
        6=>return Ok((detile_rgba8(raw,w,h),w,h)),
        5=>return Ok((detile_16(raw,w,h,rgb5a3),w,h)),
        4=>return Ok((detile_16(raw,w,h,rgb565),w,h)),
        _=>{}
    }
    let mut palette:Vec<[u8;4]>=vec![];
    if matches!(e.format,8|9|10){
        let p=e.palette.as_ref().ok_or("Indexed TPL has no palette")?;
        for i in 0..p.count{
            let v=be16(d,p.offset+i*2)? as u16;
            palette.push(match p.format{0=>[(v&255) as u8,(v&255) as u8,(v&255) as u8,(v>>8) as u8],1=>rgb565(v),2=>rgb5a3(v),_=>return Err("Unknown TPL palette format".into())});
        }
    }
    let (_,bw,bh,bs)=info(e.format).unwrap();
    let mut out=vec![0u8;w*h*4];let mut pos=0;
    for by in (0..h).step_by(bh){for bx in (0..w).step_by(bw){
        let tile=&raw[pos..pos+bs];pos+=bs;
        for y in 0..bh{for x in 0..bw{
            let index=y*bw+x;
            let v:usize=match e.format{0|8=>((tile[index/2]>>(if index%2==0{4}else{0}))&15) as usize,1|2|9=>tile[index] as usize,_=>u16::from_be_bytes([tile[index*2],tile[index*2+1]]) as usize};
            let color:[u8;4]=match e.format{
                0=>[(v*17) as u8,(v*17) as u8,(v*17) as u8,255],
                1=>[v as u8,v as u8,v as u8,255],
                2=>{let g=((v&15)*17) as u8;[g,g,g,((v>>4)*17) as u8]}
                3=>[(v&255) as u8,(v&255) as u8,(v&255) as u8,(v>>8) as u8],
                _=>{let idx=if e.format==10{v&0x3fff}else{v};*palette.get(idx).ok_or("TPL palette index out of bounds")?}
            };
            if bx+x<w&&by+y<h{let o=((by+y)*w+bx+x)*4;out[o..o+4].copy_from_slice(&color);}
        }}
    }}
    Ok((out,w,h))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn rejects_non_tpl(){assert!(parse(b"nope, not a tpl file").is_err())}
    /// Every image of every TPL in the corpus must decode to the pixels the Python reference produces.
    #[test] fn corpus_matches_python_decoder(){
        let cov=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Remaster/research/coverage.json");
        let Ok(txt)=std::fs::read_to_string(&cov) else{eprintln!("coverage.json absent; skipped");return};
        let j:serde_json::Value=serde_json::from_str(&txt).unwrap();let recs=j["records"].as_array().unwrap();
        if !std::path::Path::new(recs[0]["source"].as_str().unwrap()).exists(){eprintln!("DATA absent; skipped");return}
        let golden:serde_json::Value=serde_json::from_str(include_str!("../tests/data/tpl_golden.json")).unwrap();
        let mut seen=std::collections::HashSet::new();let (mut files,mut images,mut bad)=(0,0,vec![]);
        for r in recs{
            if r["extension"]!=".tpl"{continue}
            let h=r["sha256"].as_str().unwrap();if !seen.insert(h.to_string()){continue}
            let (data,_)=crate::archive::read_virtual(r["source"].as_str().unwrap()).unwrap();
            let want=&golden[h];files+=1;
            match parse(&data){
                Err(_)=>{if want["error"].is_null(){bad.push(format!("{}: parse error but reference parses",r["source"]))}}
                Ok(entries)=>{
                    let list=want["images"].as_array();
                    let Some(list)=list else{bad.push(format!("{}: parses but reference fails",r["source"]));continue};
                    if list.len()!=entries.len(){bad.push(format!("{}: {} entries vs {}",r["source"],entries.len(),list.len()));continue}
                    for (e,w) in entries.iter().zip(list){
                        images+=1;
                        let res=match decode(&data,e){Ok((rgba,ww,hh))=>format!("ok:{ww}x{hh}:{}",&crate::sha256::hex(&rgba)[..16]),Err(_)=>"err".into()};
                        if res!=w.as_str().unwrap(){bad.push(format!("{}: entry {} {} vs {}",r["source"],e.index,res,w))}
                    }
                }
            }
        }
        eprintln!("tpl corpus: {files} files, {images} images compared");
        assert!(bad.is_empty(),"{} mismatches, first: {:?}",bad.len(),&bad[..bad.len().min(4)]);
        assert!(images>=199);
    }
}
