//! EA `FntG` bitmap fonts (`data/fonts/*.gfn`): 16-byte glyph records, a kerning table and one 256-wide I4 (Wii
//! GX 8x8-tiled, 4 bits/pixel colour-indexed) glyph atlas with a 16-entry palette in the trailer.  Layout was recovered from the files themselves:
//!   0x00 "FntG" · 0x08 u16 ? · 0x0a u16 glyph count · 0x14 glyph table offset · 0x18 kerning table offset ·
//!   0x1c texture header offset (pixels start 0x10 later) · 0x30 pointer to a texture info block whose u16 at +6 is
//!   the pixel byte count + 0x40.
//! Glyph record: u16 code, u8 w, u8 h, u16 x, u16 y (atlas rect), 2 bytes ?, i8 x-bearing, i8 y-offset,
//! u8 kerning-pair count, u16 first kerning index, u16 advance.
//! Kerning entry (4 bytes, entries start 4 bytes into the table): u16 following char, i8 adjustment, u8 first char.
use std::collections::HashMap;

#[derive(Clone,Copy,Debug)]
pub struct Glyph{pub w:u8,pub h:u8,pub x:u16,pub y:u16,pub xoff:i8,pub yoff:i8,pub kcount:u8,pub kstart:u16,pub adv:u16}

pub struct FntG{pub glyphs:HashMap<u16,Glyph>,pub kern:Vec<(u16,i8,u8)>,pub tex_w:u32,pub tex_h:u32,
    /// RGBA8 atlas: the CI4 indices looked up in the font's 16-entry palette.
    pub rgba:Vec<u8>}

fn u16be(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_be_bytes([b[0],b[1]])).ok_or_else(||"FntG: truncated".to_string())}
fn u32be(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes([b[0],b[1],b[2],b[3]])).ok_or_else(||"FntG: truncated".to_string())}

impl FntG{
    pub fn parse(d:&[u8])->Result<FntG,String>{
        if d.get(..4)!=Some(b"FntG"){return Err("not a FntG font".into())}
        let count=u16be(d,0xa)? as usize;
        let (gt,kt,th)=(u32be(d,0x14)? as usize,u32be(d,0x18)? as usize,u32be(d,0x1c)? as usize);
        let info=u32be(d,0x30)? as usize;
        let mut glyphs=HashMap::new();
        for i in 0..count{
            let o=gt+16*i;
            let g=Glyph{w:*d.get(o+2).ok_or("FntG: glyph table")?,h:d[o+3],x:u16be(d,o+4)?,y:u16be(d,o+6)?,xoff:d[o+9] as i8,yoff:d[o+10] as i8,kcount:d[o+11],kstart:u16be(d,o+12)?,adv:u16be(d,o+14)?};
            glyphs.insert(u16be(d,o)?,g);
        }
        let mut kern=vec![];
        let kend=info.min(d.len());
        let mut o=kt+4;
        while o+4<=kend{kern.push((u16be(d,o)?,d[o+2] as i8,d[o+3]));o+=4;}
        let bytes=(u32be(d,info+4)? as usize).saturating_sub(0x40);
        // The atlas is 256 or 512 wide (the glyph rects decide), 4 bits per pixel.
        let max_x=glyphs.values().map(|g:&Glyph|g.x as usize+g.w as usize).max().unwrap_or(0);
        let w=if max_x>256{512usize}else{256};
        let h=bytes*2/w;
        let pix=th+0x10;
        let data=d.get(pix..pix+bytes).ok_or("FntG: texture truncated")?;
        // 16-entry TLUT 0x40 bytes into the 192-byte trailer; word format selected by the u16 at 0x0e
        // (9: IA8 = alpha<<8|intensity, 15: RGB5A3).
        let tail=d.get(pix+bytes..).ok_or("FntG: palette truncated")?;
        let kind=u16be(d,0x0e)?;
        let mut pal=[[0u8;4];16];
        for (i,e) in pal.iter_mut().enumerate(){
            let wd=u16be(tail,0x40+2*i)?;
            *e=if kind==0x0f{
                if wd&0x8000!=0{[(((wd>>10)&31) as u32*255/31) as u8,(((wd>>5)&31) as u32*255/31) as u8,((wd&31) as u32*255/31) as u8,255]}
                else{[((wd>>8)&15) as u8*17,((wd>>4)&15) as u8*17,(wd&15) as u8*17,(((wd>>12)&7) as u32*255/7) as u8]}
            }else{let (a,i)=((wd>>8) as u8,(wd&255) as u8);[i,i,i,a]};
        }
        let mut rgba=vec![0u8;w*h*4];
        let mut p=0;
        for ty in (0..h).step_by(8){for tx in (0..w).step_by(8){for y in 0..8{for x in (0..8).step_by(2){
            let b=data.get(p).copied().unwrap_or(0);p+=1;
            for (k,idx) in [b>>4,b&15].into_iter().enumerate(){
                if ty+y>=h||tx+x+k>=w{continue}
                let o=((ty+y)*w+tx+x+k)*4;
                rgba[o..o+4].copy_from_slice(&pal[idx as usize]);
            }
        }}}}
        Ok(FntG{glyphs,kern,tex_w:w as u32,tex_h:h as u32,rgba})
    }

    /// `auxgraph.gfn`: the Wii button icons.  Same container with 12-byte glyph records (code u16, w, h, x u16, y u16, advance u8)
    /// and a 128x84 IA4 atlas (8x4 tiles, alpha in the high nibble).  The character `'A' + n` is the icon of button token `n`
    /// (see `apt_text::BUTTON_TOKENS`).
    pub fn parse_aux(d:&[u8])->Result<FntG,String>{
        if d.get(..4)!=Some(b"FntG"){return Err("not a FntG font".into())}
        let count=u16be(d,0xa)? as usize;
        let (gt,th)=(u32be(d,0x14)? as usize,u32be(d,0x1c)? as usize);
        let mut glyphs=HashMap::new();
        for i in 0..count{
            let o=gt+12*i;
            let r=d.get(o..o+12).ok_or("FntG: aux glyph table")?;
            let code=u16::from_be_bytes([r[0],r[1]]);
            // The table lists 'C' twice; the first record wins.
            glyphs.entry(code).or_insert(Glyph{w:r[2],h:r[3],x:u16::from_be_bytes([r[4],r[5]]),y:u16::from_be_bytes([r[6],r[7]]),xoff:0,yoff:0,kcount:0,kstart:0,adv:r[8] as u16});
        }
        let (w,h)=(128usize,84usize);
        let data=d.get(th+0x10..th+0x10+w*h).ok_or("FntG: aux texture truncated")?;
        let mut rgba=vec![0u8;w*h*4];let mut p=0;
        for ty in (0..h).step_by(4){for tx in (0..w).step_by(8){for y in 0..4{for x in 0..8{
            let b=data[p];p+=1;let (a,i)=((b>>4)*17,(b&15)*17);
            let o=((ty+y)*w+tx+x)*4;rgba[o..o+4].copy_from_slice(&[i,i,i,a]);
        }}}}
        Ok(FntG{glyphs,kern:vec![],tex_w:w as u32,tex_h:h as u32,rgba})
    }

    /// Append the button icons below this font's atlas and register them as characters `U+E000 + token`; icons are centred on
    /// a line `line_h` tall (as `DrawGraphic` centres them).
    pub fn with_buttons(&self,aux:&FntG,line_h:i32)->FntG{
        let w=self.tex_w.max(aux.tex_w) as usize;
        let mut rgba=vec![0u8;w*(self.tex_h+aux.tex_h) as usize*4];
        for y in 0..self.tex_h as usize{let n=self.tex_w as usize*4;rgba[y*w*4..y*w*4+n].copy_from_slice(&self.rgba[y*n..(y+1)*n]);}
        for y in 0..aux.tex_h as usize{let n=aux.tex_w as usize*4;let dst=(self.tex_h as usize+y)*w*4;rgba[dst..dst+n].copy_from_slice(&aux.rgba[y*n..(y+1)*n]);}
        let mut glyphs=self.glyphs.clone();
        for t in 0..15u16{
            if let Some(g)=aux.glyphs.get(&(0x41+t)){
                glyphs.insert(0xE000+t,Glyph{y:g.y+self.tex_h as u16,yoff:((line_h-g.h as i32)/2).clamp(-100,100) as i8,adv:g.w as u16+2,..*g});
            }
        }
        FntG{glyphs,kern:self.kern.clone(),tex_w:w as u32,tex_h:self.tex_h+aux.tex_h,rgba}
    }

    /// Pair adjustment between two characters.
    pub fn kerning(&self,a:u16,b:u16)->i32{
        let Some(g)=self.glyphs.get(&a) else{return 0};
        for i in 0..g.kcount as usize{
            if let Some(&(s,adj,f))=self.kern.get(g.kstart as usize+i){ if s==b&&f as u16==a&0xff{return adj as i32} }
        }
        0
    }
}

/// One row of `fe/FontTable.txt`: how a Flash font/size maps to a bitmap font and where its glyphs sit.
#[derive(Clone,Debug)]
pub struct FontEntry{pub flash:String,pub flash_size:f32,pub real:String,pub real_size:f32,pub xoff:f32,pub yoff:f32,pub xscale:f32,pub yscale:f32}

pub fn parse_font_table(text:&str)->Vec<FontEntry>{
    let mut out=vec![];
    for line in text.lines(){
        let line=line.trim();
        if line.is_empty()||line.starts_with("//"){continue}
        let c:Vec<&str>=line.split(',').map(|s|s.trim()).collect();
        if c.len()<8{continue}
        let f=|i:usize|c.get(i).and_then(|s|s.parse::<f32>().ok()).unwrap_or(0.);
        out.push(FontEntry{flash:c[0].to_string(),flash_size:f(1),real:c[2].to_string(),real_size:f(3),xoff:f(4),yoff:f(5),xscale:if f(6)==0.{1.}else{f(6)},yscale:if f(7)==0.{1.}else{f(7)}});
    }
    out
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn corpus_fonts_parse(){
        let dir=crate::bridge::data_root().join("files").join("data").join("fonts");
        if !dir.exists(){eprintln!("DATA absent; skipped");return}
        let mut n=0;
        for e in std::fs::read_dir(&dir).unwrap().flatten(){
            let p=e.path();if p.extension().is_none_or(|x|x!="gfn"){continue}
            let f=FntG::parse(&std::fs::read(&p).unwrap()).unwrap_or_else(|e|panic!("{}: {e}",p.display()));
            assert!(f.tex_h>0&&!f.glyphs.is_empty()&&(f.glyphs.len()>20||p.file_name().is_some_and(|n|n=="auxgraph.gfn")),"{}",p.display());
            let aux=p.file_name().is_some_and(|n|n=="auxgraph.gfn");
            if !aux{for g in f.glyphs.values(){assert!(g.x as u32+g.w as u32<=f.tex_w&&g.y as u32+g.h as u32<=f.tex_h,"{} glyph outside atlas",p.display());}}
            n+=1;
        }
        assert!(n>=14);
    }
}
