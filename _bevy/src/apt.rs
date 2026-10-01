//! EA APT movie format (the Wii frontend's Flash-derived UI format): `.apt` structures + `.const` dictionary.
//!
//! Layout recovered from the data and the executable (`AptActionInterpreter::_parseStream`, 0x80134064):
//! every pointer is a file offset into the `.apt` blob; structures carry the 0x09876543 signature after their type word;
//! all values are big-endian.  The `.const` file stores the string/number dictionary and the offset of the movie record.
//! Streams (action byte code) are executed in place from `Apt::data`, so only their offsets are kept here.
use std::collections::HashMap;

pub const SIGNATURE:u32=0x0987_6543;

#[derive(Debug,Clone)]
pub enum Const{Str(String),Undef,Reg(u32),Bool(bool),Float(f32),Int(i32),Lookup(u32),Other(u32,u32)}

#[derive(Debug,Clone)]
pub struct ClipAction{pub flags:u32,pub key:u32,pub code:u32}

#[derive(Debug,Clone)]
pub struct Place{
    pub flags:u32,pub depth:i32,pub character:i32,
    /// m00,m01,m10,m11,tx,ty as stored (row-vector convention: x' = x*m00 + y*m10 + tx).
    pub matrix:[f32;6],
    /// 0xAARRGGBB colour-transform multiplier and additive term as stored.
    pub color:u32,pub add:u32,pub ratio:f32,pub name:Option<String>,pub clip_depth:i32,pub clip_actions:Vec<ClipAction>,
}
impl Place{
    pub const MOVE:u32=1;pub const HAS_CHARACTER:u32=2;pub const HAS_MATRIX:u32=4;pub const HAS_COLOR:u32=8;
    pub const HAS_RATIO:u32=0x10;pub const HAS_NAME:u32=0x20;pub const HAS_CLIP_DEPTH:u32=0x40;pub const HAS_CLIP_ACTIONS:u32=0x80;
}

#[derive(Debug,Clone)]
pub enum Item{
    /// Frame script: offset of the byte code.
    Action(u32),
    Label{name:String,flags:u32,frame:u32},
    Place(Place),
    Remove(i32),
    Background(u32),
    InitAction{sprite:u32,code:u32},
}

#[derive(Debug,Clone,Default)]
pub struct Frame{pub items:Vec<Item>}

#[derive(Debug,Clone)]
pub struct TextDef{pub bounds:[f32;4],pub font:u32,pub align:u32,pub color:u32,pub height:f32,pub read_only:bool,pub multiline:bool,pub word_wrap:bool,pub text:String,pub variable:String}

#[derive(Debug,Clone)]
pub enum Character{
    Shape{bounds:[f32;4],geometry:u32},
    Text(TextDef),
    Font{name:String},
    Sprite{frames:Vec<Frame>},
    Image{texture:u32},
    Movie,
}

#[derive(Debug,Clone)]
pub struct Import{pub movie:String,pub name:String,pub id:u32}
#[derive(Debug,Clone)]
pub struct Export{pub name:String,pub id:u32}

pub struct Apt{
    pub data:Vec<u8>,
    pub dict:Vec<Const>,
    pub width:f32,pub height:f32,
    pub frames:Vec<Frame>,
    pub characters:Vec<Option<Character>>,
    pub imports:Vec<Import>,
    pub exports:Vec<Export>,
    pub labels:HashMap<String,usize>,
    pub ms_per_frame:u32,
}

struct R<'a>{d:&'a [u8]}
impl<'a> R<'a>{
    fn u32(&self,o:usize)->Result<u32,String>{self.d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("apt: read past end at {o:#x}"))}
    fn i32(&self,o:usize)->Result<i32,String>{Ok(self.u32(o)? as i32)}
    fn f32(&self,o:usize)->Result<f32,String>{Ok(f32::from_bits(self.u32(o)?))}
    fn cstr(&self,o:usize)->Result<String,String>{
        if o==0{return Ok(String::new())}
        let s=self.d.get(o..).ok_or_else(||format!("apt: string offset {o:#x} out of range"))?;
        let e=s.iter().position(|&c|c==0).ok_or("apt: unterminated string")?;
        Ok(s[..e].iter().map(|&c|c as char).collect())
    }
}

impl Apt{
    pub fn parse(apt:&[u8],konst:&[u8])->Result<Apt,String>{
        if !apt.starts_with(b"Apt Data"){return Err("not an Apt Data file".into())}
        if !konst.starts_with(b"Apt constant file"){return Err("not an Apt constant file".into())}
        let c=R{d:konst};let r=R{d:apt};
        let movie=c.u32(0x14)? as usize;let count=c.u32(0x18)? as usize;let table=c.u32(0x1c)? as usize;
        let mut dict=Vec::with_capacity(count);
        for i in 0..count{
            let (t,v)=(c.u32(table+8*i)?,c.u32(table+8*i+4)?);
            dict.push(match t{1=>Const::Str(c.cstr(v as usize)?),3=>Const::Undef,4=>Const::Reg(v),5=>Const::Bool(v!=0),6=>Const::Float(f32::from_bits(v)),7=>Const::Int(v as i32),8=>Const::Lookup(v),_=>Const::Other(t,v)});
        }
        if r.u32(movie)?!=9||r.u32(movie+4)?!=SIGNATURE{return Err("apt: movie record has the wrong type/signature".into())}
        let frames=Self::frames(&r,r.u32(movie+8)? as usize,r.u32(movie+0xc)? as usize)?;
        let nchar=r.u32(movie+0x14)? as usize;let ctab=r.u32(movie+0x18)? as usize;
        let (width,height)=(r.u32(movie+0x1c)? as f32,r.u32(movie+0x20)? as f32);
        let ms_per_frame=r.u32(movie+0x24)?;
        let mut characters=Vec::with_capacity(nchar);
        for i in 0..nchar{
            let p=r.u32(ctab+4*i)? as usize;
            if p==0{characters.push(None);continue}
            if i==0{characters.push(Some(Character::Movie));continue}
            let ty=r.u32(p)?;
            if r.u32(p+4)?!=SIGNATURE{return Err(format!("apt: character {i} has no signature"))}
            characters.push(Some(match ty{
                1=>Character::Shape{bounds:[r.f32(p+8)?,r.f32(p+12)?,r.f32(p+16)?,r.f32(p+20)?],geometry:r.u32(p+24)?},
                2=>Character::Text(TextDef{bounds:[r.f32(p+8)?,r.f32(p+12)?,r.f32(p+16)?,r.f32(p+20)?],font:r.u32(p+24)?,align:r.u32(p+28)?,color:r.u32(p+32)?,height:r.f32(p+36)?,
                    read_only:r.u32(p+40)?!=0,multiline:r.u32(p+44)?!=0,word_wrap:r.u32(p+48)?!=0,text:r.cstr(r.u32(p+52)? as usize)?,variable:r.cstr(r.u32(p+56)? as usize)?}),
                3=>Character::Font{name:r.cstr(r.u32(p+8)? as usize)?},
                5=>Character::Sprite{frames:Self::frames(&r,r.u32(p+8)? as usize,r.u32(p+12)? as usize)?},
                7=>Character::Image{texture:r.u32(p+8)?},
                9=>Character::Movie,
                t=>return Err(format!("apt: character {i} has unsupported type {t}")),
            }));
        }
        let mut imports=vec![];
        let (ic,ip)=(r.u32(movie+0x28)? as usize,r.u32(movie+0x2c)? as usize);
        for i in 0..ic{imports.push(Import{movie:r.cstr(r.u32(ip+16*i)? as usize)?,name:r.cstr(r.u32(ip+16*i+4)? as usize)?,id:r.u32(ip+16*i+8)?});}
        let mut exports=vec![];
        let (ec,ep)=(r.u32(movie+0x30)? as usize,r.u32(movie+0x34)? as usize);
        for i in 0..ec{exports.push(Export{name:r.cstr(r.u32(ep+8*i)? as usize)?,id:r.u32(ep+8*i+4)?});}
        let mut labels=HashMap::new();
        for (n,f) in frames.iter().enumerate(){for it in &f.items{if let Item::Label{name,..}=it{labels.entry(name.to_lowercase()).or_insert(n);}}}
        Ok(Apt{data:apt.to_vec(),dict,width,height,frames,characters,imports,exports,labels,ms_per_frame})
    }

    fn frames(r:&R,count:usize,ptr:usize)->Result<Vec<Frame>,String>{
        let mut out=Vec::with_capacity(count);
        for i in 0..count{
            let (n,ip)=(r.u32(ptr+8*i)? as usize,r.u32(ptr+8*i+4)? as usize);
            let mut items=Vec::with_capacity(n);
            for k in 0..n{
                let p=r.u32(ip+4*k)? as usize;
                items.push(match r.u32(p)?{
                    1=>Item::Action(r.u32(p+4)?),
                    2=>Item::Label{name:r.cstr(r.u32(p+4)? as usize)?,flags:r.u32(p+8)?,frame:r.u32(p+12)?},
                    3=>{
                        let flags=r.u32(p+4)?;
                        let mut clip_actions=vec![];
                        let ca=r.u32(p+0x3c)? as usize;
                        if flags&Place::HAS_CLIP_ACTIONS!=0&&ca!=0{
                            let (cn,cp)=(r.u32(ca)? as usize,r.u32(ca+4)? as usize);
                            for j in 0..cn{clip_actions.push(ClipAction{flags:r.u32(cp+12*j)?,key:r.u32(cp+12*j+4)?,code:r.u32(cp+12*j+8)?});}
                        }
                        Item::Place(Place{flags,depth:r.i32(p+8)?,character:r.i32(p+12)?,
                            matrix:[r.f32(p+16)?,r.f32(p+20)?,r.f32(p+24)?,r.f32(p+28)?,r.f32(p+32)?,r.f32(p+36)?],
                            color:r.u32(p+40)?,add:r.u32(p+44)?,ratio:r.f32(p+48)?,
                            name:if flags&Place::HAS_NAME!=0{Some(r.cstr(r.u32(p+52)? as usize)?)}else{None},
                            clip_depth:r.i32(p+56)?,clip_actions})
                    }
                    4=>Item::Remove(r.i32(p+4)?),
                    5=>Item::Background(r.u32(p+4)?),
                    8=>Item::InitAction{sprite:r.u32(p+4)?,code:r.u32(p+8)?},
                    t=>return Err(format!("apt: unsupported frame item type {t} at {p:#x}")),
                });
            }
            out.push(Frame{items});
        }
        Ok(out)
    }

    pub fn string(&self,i:usize)->Option<&str>{match self.dict.get(i){Some(Const::Str(s))=>Some(s),_=>None}}
    pub fn character(&self,id:u32)->Option<&Character>{self.characters.get(id as usize).and_then(|c|c.as_ref())}
}

#[cfg(test)]
mod tests{
    use super::*;
    /// Every frontend movie in the corpus must parse into frames/characters without error.
    #[test]
    fn corpus_parses(){
        let root=crate::bridge::data_root().join("files").join("data").join("fe");
        if !root.exists(){eprintln!("DATA absent; skipped");return}
        let mut files=vec![];
        fn walk(d:&std::path::Path,out:&mut Vec<std::path::PathBuf>){for e in std::fs::read_dir(d).unwrap().flatten(){let p=e.path();if p.is_dir(){walk(&p,out)}else if p.extension().is_some_and(|x|x=="big"){out.push(p)}}}
        walk(&root,&mut files);
        let (mut ok,mut bad)=(0,vec![]);
        for f in files{
            let data=std::fs::read(&f).unwrap();
            let Some(Ok(entries))=crate::archive::entries(&data) else{continue};
            let find=|ext:&str|entries.iter().find(|e|e.name.to_lowercase().ends_with(ext));
            let (Some(a),Some(c))=(find(".apt"),find(".const")) else{continue};
            match Apt::parse(&data[a.offset..a.offset+a.size],&data[c.offset..c.offset+c.size]){Ok(_)=>ok+=1,Err(e)=>bad.push(format!("{}: {e}",f.display()))}
        }
        eprintln!("apt corpus: {ok} parsed, {} failed",bad.len());
        assert!(bad.is_empty(),"{bad:#?}");
        assert!(ok>600);
    }
}
