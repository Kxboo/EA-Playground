//! EA SHPG (`.gsh`) texture archives and GX texture decoding - Rust port of Remaster/src/legacy/gsh_parser.py.
//!
//! Header: `SHPG`, u32 LE file size, u32 BE directory count, 4-byte version; directory of (4-byte short name, u32 BE offset).
//! Image record: u8 format, u24 BE block size (incl. the 16-byte header), u16 BE width/height/centers, 4 tail bytes.
//! Indexed images are followed by a palette record (0x31 RGB565, 0x32 RGB5A3, 0x33 two 32-byte aligned AR/GB planes);
//! a `70 00 00 00` record then holds the full name.  GX levels occupy whole tiles; only the top mip is decoded.
//! Evidence and format corrections: Remaster/research/FINDINGS.md.  Verified against the Python decoder for every image.

#[derive(Debug,Clone)]
pub struct Entry{
    pub index:usize,pub name:String,pub full_name:Option<String>,pub record_id:u8,pub width:usize,pub height:usize,
    pub img_offset:usize,pub img_end:usize,pub palette:Option<Vec<[u8;4]>>,
}
pub struct Gsh{pub entries:Vec<Entry>}

fn be16(d:&[u8],o:usize)->usize{u16::from_be_bytes([d[o],d[o+1]]) as usize}

/// (label, bits per pixel numerator, denominator, format) as in RECORD_FORMATS.
fn format_info(id:u8)->Option<(usize,usize,Fmt)>{Some(match id{20=>(16,8,Fmt::Rgb565),30=>(1,2,Fmt::Cmpr),25=>(1,1,Fmt::Pal8),24=>(4,8,Fmt::Pal4),22=>(32,8,Fmt::Rgba8),21=>(16,8,Fmt::Rgb5a3),_=>return None})}
#[derive(Clone,Copy,PartialEq,Debug)] enum Fmt{Rgb565,Cmpr,Pal8,Pal4,Rgba8,Rgb5a3}

pub(crate) fn rgb565(v:u16)->[u8;4]{
    let (r,g,b)=(((v>>11)&0x1f) as u8,((v>>5)&0x3f) as u8,(v&0x1f) as u8);
    [(r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2),255]
}
pub(crate) fn rgb5a3(v:u16)->[u8;4]{
    if v&0x8000!=0{
        let (r,g,b)=(((v>>10)&0x1f) as u8,((v>>5)&0x1f) as u8,(v&0x1f) as u8);
        [(r<<3)|(r>>2),(g<<3)|(g>>2),(b<<3)|(b>>2),255]
    }else{
        let (a,r,g,b)=(((v>>12)&7) as u8,((v>>8)&0xf) as u8,((v>>4)&0xf) as u8,(v&0xf) as u8);
        [(r<<4)|r,(g<<4)|g,(b<<4)|b,(a<<5)|(a<<2)|(a>>1)]
    }
}

fn read_palette(d:&[u8],off:usize)->Option<Vec<[u8;4]>>{
    if off+16>d.len(){return None}
    let hdr=&d[off..off+16];let id=hdr[0];
    if !matches!(id,0x31|0x32|0x33){return None}
    let size=((hdr[1] as usize)<<16)|((hdr[2] as usize)<<8)|hdr[3] as usize;
    let (w,h)=(be16(hdr,4),be16(hdr,6));let count=w*h;
    if count==0||count>256{return None}
    let pal_off=off+16;let stride=(count*2+31)&!31;
    let pal_size=if id==0x33{stride+count*2}else{count*2};
    if size.saturating_sub(16)<pal_size||pal_off+pal_size>d.len(){return None}
    let p=&d[pal_off..pal_off+pal_size];
    Some((0..count).map(|i|match id{
        0x33=>[p[2*i+1],p[stride+2*i],p[stride+2*i+1],p[2*i]],
        0x31=>rgb565(u16::from_be_bytes([p[2*i],p[2*i+1]])),
        _=>rgb5a3(u16::from_be_bytes([p[2*i],p[2*i+1]])),
    }).collect())
}

fn read_name(d:&[u8],off:usize)->Option<String>{
    if off+4>d.len()||d[off..off+4]!=[0x70,0,0,0]{return None}
    let start=off+4;let stop=(start+64).min(d.len());
    let end=d[start..stop].iter().position(|&c|c==0).map(|p|start+p).unwrap_or(stop);
    let raw=&d[start..end];
    if raw.is_empty()||!raw.iter().all(|&b|(32..127).contains(&b)){return None}
    Some(raw.iter().map(|&b|b as char).collect())
}

pub fn parse(d:&[u8])->Result<Gsh,String>{
    if d.len()<16||&d[..4]!=b"SHPG"{return Err("not a SHPG file".into())}
    let count=u32::from_be_bytes(d[8..12].try_into().unwrap()) as usize;
    if count>(d.len()-16)/8{return Err("GSH object table exceeds file".into())}
    let mut entries=vec![];
    for i in 0..count{
        let o=16+i*8;
        let short:String=d[o..o+4].iter().take_while(|&&c|c!=0).map(|&c|c as char).collect();
        let eo=u32::from_be_bytes(d[o+4..o+8].try_into().unwrap()) as usize;
        if eo+16>d.len(){continue}
        let eh=&d[eo..eo+16];
        let size=((eh[1] as usize)<<16)|((eh[2] as usize)<<8)|eh[3] as usize;
        if size<16||eo+size>d.len(){return Err(format!("GSH entry {i} block length is outside file"))}
        entries.push(Entry{index:i,name:short,full_name:None,record_id:eh[0],width:be16(eh,4),height:be16(eh,6),img_offset:eo+16,img_end:eo+size,palette:None});
        let _=eo;
    }
    for e in &mut entries{
        if matches!(e.record_id,24|25){e.palette=read_palette(d,e.img_end);}
        let mut name_off=e.img_end;
        if matches!(e.record_id,24|25)&&e.palette.is_some()&&e.img_end+16<=d.len(){
            let h=&d[e.img_end..e.img_end+16];let s=((h[1] as usize)<<16)|((h[2] as usize)<<8)|h[3] as usize;
            name_off=e.img_end+16+s.saturating_sub(16);
        }
        e.full_name=read_name(d,name_off);
    }
    entries.sort_by_key(|e|e.img_offset);
    Ok(Gsh{entries})
}

/// Byte length of the top mip level: whole GX tiles at this bit depth (must fit inside the record).
fn level0_len(e:&Entry,fmt:(usize,usize,Fmt))->Option<usize>{
    if e.width==0||e.height==0{return None}
    let (num,den,_)=fmt;let bits=num*8/den;
    let (tw,th)=if bits==4{(8,8)}else if bits==8{(8,4)}else{(4,4)};
    let len=e.width.div_ceil(tw)*e.height.div_ceil(th)*tw*th*num/den;
    (len<=e.img_end-e.img_offset).then_some(len)
}

/// Decode the entry's top mip to RGBA8 (row-major), returning (rgba, width, height).
pub fn decode(e:&Entry,d:&[u8])->Result<(Vec<u8>,usize,usize),String>{
    let info=format_info(e.record_id).ok_or_else(||format!("unsupported format for decode: 0x{:02x}",e.record_id))?;
    let len=level0_len(e,info).ok_or("no decodable mip level for this entry")?;
    let blob=d.get(e.img_offset..e.img_offset+len).ok_or("Truncated GX texture level")?;
    let (w,h)=(e.width,e.height);
    let rgba=match info.2{
        Fmt::Rgba8=>detile_rgba8(blob,w,h),
        Fmt::Rgb5a3=>detile_16(blob,w,h,rgb5a3),
        Fmt::Rgb565=>detile_16(blob,w,h,rgb565),
        Fmt::Cmpr=>decode_cmpr(blob,w,h),
        Fmt::Pal8=>{let p=e.palette.as_ref().ok_or("Missing palette; refusing grayscale placeholder as decoded texture")?;gather(&detile_index(blob,w,h,8,4,false),p)?}
        Fmt::Pal4=>{let p=e.palette.as_ref().ok_or("Missing palette; refusing grayscale placeholder as decoded texture")?;gather(&detile_index(blob,w,h,8,8,true),p)?}
    };
    Ok((rgba,w,h))
}

/// C8 (8x4 tiles) / C4 (8x8 tiles, two texels per byte, high nibble first) index un-tiling.
fn detile_index(b:&[u8],w:usize,h:usize,tw:usize,th:usize,nibbles:bool)->Vec<u8>{
    let (nx,ny)=(w.div_ceil(tw),h.div_ceil(th));let mut out=vec![0u8;w*h];let mut pos=0usize;
    for ty in 0..ny{for tx in 0..nx{for by in 0..th{
        if nibbles{
            for bx in (0..tw).step_by(2){
                let byte=b.get(pos).copied().unwrap_or(0);pos+=1;
                let (px0,px1,py)=(tx*tw+bx,tx*tw+bx+1,ty*th+by);
                if px0<w&&py<h{out[py*w+px0]=byte>>4}
                if px1<w&&py<h{out[py*w+px1]=byte&0xf}
            }
        }else{
            for bx in 0..tw{
                let v=b.get(pos).copied().unwrap_or(0);pos+=1;
                let (px,py)=(tx*tw+bx,ty*th+by);
                if px<w&&py<h{out[py*w+px]=v}
            }
        }
    }}}
    out
}

fn gather(idx:&[u8],pal:&[[u8;4]])->Result<Vec<u8>,String>{
    let mut out=Vec::with_capacity(idx.len()*4);
    for &i in idx{out.extend_from_slice(pal.get(i as usize).ok_or("Texture index buffer references a missing palette color")?);}
    Ok(out)
}

/// RGBA8: 4x4 tiles of 64 bytes = 32 bytes of (A,R) pairs then 32 bytes of (G,B) pairs.
pub(crate) fn detile_rgba8(b:&[u8],w:usize,h:usize)->Vec<u8>{
    let (nx,ny)=(w.div_ceil(4),h.div_ceil(4));let mut out=vec![0u8;w*h*4];
    for ty in 0..ny{for tx in 0..nx{
        let base=(ty*nx+tx)*64;
        for i in 0..16{
            let (px,py)=(tx*4+i%4,ty*4+i/4);if px>=w||py>=h{continue}
            let (a,r)=(b[base+2*i],b[base+2*i+1]);let (g,bl)=(b[base+32+2*i],b[base+32+2*i+1]);
            let o=(py*w+px)*4;out[o..o+4].copy_from_slice(&[r,g,bl,a]);
        }
    }}
    out
}

/// 16-bit formats: 4x4 tiles of 32 bytes, texels big-endian.
pub(crate) fn detile_16(b:&[u8],w:usize,h:usize,f:fn(u16)->[u8;4])->Vec<u8>{
    let (nx,ny)=(w.div_ceil(4),h.div_ceil(4));let mut out=vec![0u8;w*h*4];
    for ty in 0..ny{for tx in 0..nx{
        let base=(ty*nx+tx)*32;
        for i in 0..16{
            let (px,py)=(tx*4+i%4,ty*4+i/4);if px>=w||py>=h{continue}
            let v=u16::from_be_bytes([b[base+2*i],b[base+2*i+1]]);
            let o=(py*w+px)*4;out[o..o+4].copy_from_slice(&f(v));
        }
    }}
    out
}

/// CMPR: big-endian DXT1 in 2x2 arrangements of 4x4 sub-blocks per 8x8 tile.
pub(crate) fn decode_cmpr(b:&[u8],w:usize,h:usize)->Vec<u8>{
    let (nx,ny)=(w.div_ceil(8),h.div_ceil(8));let mut out=vec![0u8;w*h*4];let mut pos=0usize;
    for ty in 0..ny{for tx in 0..nx{for sub in 0..4{
        let (sx,sy)=(tx*8+(sub%2)*4,ty*8+(sub/2)*4);
        let (c0,c1)=(u16::from_be_bytes([b[pos],b[pos+1]]),u16::from_be_bytes([b[pos+2],b[pos+3]]));
        let bits=u32::from_be_bytes(b[pos+4..pos+8].try_into().unwrap());pos+=8;
        let (a,bb)=(rgb565(c0),rgb565(c1));
        let mix=|x:u8,y:u8,wx:u32,wy:u32,div:u32|((wx*x as u32+wy*y as u32)/div) as u8;
        let pal=if c0>c1{[a,bb,[mix(a[0],bb[0],2,1,3),mix(a[1],bb[1],2,1,3),mix(a[2],bb[2],2,1,3),255],[mix(a[0],bb[0],1,2,3),mix(a[1],bb[1],1,2,3),mix(a[2],bb[2],1,2,3),255]]}
            else{[a,bb,[mix(a[0],bb[0],1,1,2),mix(a[1],bb[1],1,1,2),mix(a[2],bb[2],1,1,2),255],[0,0,0,0]]};
        for py in 0..4{for px in 0..4{
            let sel=((bits>>(30-2*(py*4+px)))&3) as usize;let (x,y)=(sx+px,sy+py);
            if x<w&&y<h{let o=(y*w+x)*4;out[o..o+4].copy_from_slice(&pal[sel]);}
        }}
    }}}
    out
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn colour_expansion(){
        assert_eq!(rgb565(0xFFFF),[255,255,255,255]);assert_eq!(rgb565(0xF800),[255,0,0,255]);
        assert_eq!(rgb5a3(0x8000|0x7C00),[255,0,0,255]);assert_eq!(rgb5a3(0x7FFF),[255,255,255,255]);assert_eq!(rgb5a3(0x0FFF)[3],0);
    }
    #[test] fn c8_tiling_places_texels(){
        // One 8x4 tile in an 8x4 image: identity order.
        let b:Vec<u8>=(0..32).collect();assert_eq!(detile_index(&b,8,4,8,4,false),b);
        // 16x4: second tile starts at x=8.
        let b:Vec<u8>=(0..64).collect();let o=detile_index(&b,16,4,8,4,false);assert_eq!(o[8],32);assert_eq!(o[16],8);
    }
    #[test] fn rejects_non_shpg(){assert!(parse(b"nope").is_err())}
    /// Every image of every GSH in the corpus must decode to the same pixels as the Python decoder.
    #[test] fn corpus_matches_python_decoder(){
        let cov=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Remaster/research/coverage.json");
        let Ok(txt)=std::fs::read_to_string(&cov) else{eprintln!("coverage.json absent; skipped");return};
        let j:serde_json::Value=serde_json::from_str(&txt).unwrap();let recs=j["records"].as_array().unwrap();
        if !std::path::Path::new(recs[0]["source"].as_str().unwrap()).exists(){eprintln!("DATA absent; skipped");return}
        let golden:serde_json::Value=serde_json::from_str(include_str!("../tests/data/gsh_golden.json")).unwrap();
        let mut seen=std::collections::HashSet::new();let (mut files,mut images,mut bad)=(0,0,vec![]);
        for r in recs{
            if r["extension"]!=".gsh"{continue}
            let h=r["sha256"].as_str().unwrap();if !seen.insert(h.to_string()){continue}
            let (data,_)=crate::archive::read_virtual(r["source"].as_str().unwrap()).unwrap();
            let want=golden[h].as_array().unwrap_or_else(||panic!("no golden for {h}"));
            let gsh=parse(&data).unwrap_or_else(|e|panic!("{}: {e}",r["source"]));files+=1;
            if gsh.entries.len()!=want.len(){bad.push(format!("{}: {} entries vs {}",r["source"],gsh.entries.len(),want.len()));continue}
            for (e,w) in gsh.entries.iter().zip(want){
                images+=1;
                let ok=w[0].as_u64().unwrap() as usize==e.index&&w[1].as_str().unwrap()==e.name&&w[2].as_str()==e.full_name.as_deref()&&w[3].as_u64().unwrap() as u8==e.record_id&&w[4].as_u64().unwrap() as usize==e.width&&w[5].as_u64().unwrap() as usize==e.height;
                if !ok{bad.push(format!("{}: entry {} metadata differs",r["source"],e.index));continue}
                let res=match decode(e,&data){Ok((rgba,_,_))=>format!("ok:{}",&crate::sha256::hex(&rgba)[..16]),Err(_)=>"err".into()};
                if res!=w[6].as_str().unwrap(){bad.push(format!("{}: entry {} {} vs {}",r["source"],e.index,res,w[6]))}
            }
        }
        eprintln!("gsh corpus: {files} files, {images} images compared");
        assert!(bad.is_empty(),"{} mismatches, first: {:?}",bad.len(),&bad[..bad.len().min(4)]);
        assert!(images>=3500);
    }
}
