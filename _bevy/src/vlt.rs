//! EA "Attrib" vault databases (`db.vlt` + `db.bin`) - Rust reimplementation of the Wii executable's loader.
//!
//! Recovered from playgroundz.elf (disassemble with `py tools/re_functions.py <symbol>`):
//! * `Attrib::hash64` @0x802d86b0: lookup8 variant, a = b = seed, c = golden ratio; verified against the
//!   original machine code by `tools/ppc_emu.py` (vectors in `tests/data/hash64_vectors.json`).
//! * `Attrib::Vault::Vault` @0x802d9800: big-endian chunks `[tag][u32 size]` - Vers, DepN, StrN, DatN, ExpN, PtrN.
//! * `Attrib::Vault::Initialize` @0x802d9d00: PtrN relocation records (kind 0 end, 1 null, 2 select base block,
//!   3 pointer = block base + b, 4 cross-vault export reference).
//! * Export policies @0x802d7778 (database), 0x802d73cc (class), 0x802d71d4 + `Collection::Collection`
//!   @0x802d45bc (collection): field offsets used below.
//! Value storage (checked against real nodes): flag 0x40 inline, 0x02 array (8-byte header), otherwise pointer.
use serde_json::{json,Value};
use std::collections::HashMap;

pub const SEED:u64=0xABCD_EF00_1122_3344;
const GOLD:u64=0x9e37_79b9_7f4a_7c13;

fn mix(a:&mut u64,b:&mut u64,c:&mut u64){
    *a=a.wrapping_sub(*b).wrapping_sub(*c);*a^=*c>>43;*b=b.wrapping_sub(*c).wrapping_sub(*a);*b^=*a<<9;*c=c.wrapping_sub(*a).wrapping_sub(*b);*c^=*b>>8;
    *a=a.wrapping_sub(*b).wrapping_sub(*c);*a^=*c>>38;*b=b.wrapping_sub(*c).wrapping_sub(*a);*b^=*a<<23;*c=c.wrapping_sub(*a).wrapping_sub(*b);*c^=*b>>5;
    *a=a.wrapping_sub(*b).wrapping_sub(*c);*a^=*c>>35;*b=b.wrapping_sub(*c).wrapping_sub(*a);*b^=*a<<49;*c=c.wrapping_sub(*a).wrapping_sub(*b);*c^=*b>>11;
    *a=a.wrapping_sub(*b).wrapping_sub(*c);*a^=*c>>12;*b=b.wrapping_sub(*c).wrapping_sub(*a);*b^=*a<<18;*c=c.wrapping_sub(*a).wrapping_sub(*b);*c^=*b>>22;
}

/// `Attrib::hash64(key, len, level)`.
pub fn hash64(key:&[u8],level:u64)->u64{
    let (mut a,mut b,mut c)=(level,level,GOLD);
    let mut k=key;
    while k.len()>=24{
        a=a.wrapping_add(u64::from_le_bytes(k[0..8].try_into().unwrap()));b=b.wrapping_add(u64::from_le_bytes(k[8..16].try_into().unwrap()));c=c.wrapping_add(u64::from_le_bytes(k[16..24].try_into().unwrap()));
        mix(&mut a,&mut b,&mut c);k=&k[24..];
    }
    c=c.wrapping_add(key.len() as u64);
    for (i,&byte) in k.iter().enumerate().rev(){
        let v=byte as u64;
        if i>=16{c=c.wrapping_add(v<<(8*(i-15)))}else if i>=8{b=b.wrapping_add(v<<(8*(i-8)))}else{a=a.wrapping_add(v<<(8*i))}
    }
    mix(&mut a,&mut b,&mut c);c
}
/// `Attrib::StringHash64` (0x802d8dd4): empty string hashes to 0.
pub fn string_hash64(s:&str)->u64{if s.is_empty(){0}else{hash64(s.as_bytes(),SEED)}}

fn be32(b:&[u8],o:usize)->u32{u32::from_be_bytes(b[o..o+4].try_into().unwrap())}
fn be16(b:&[u8],o:usize)->u16{u16::from_be_bytes(b[o..o+2].try_into().unwrap())}
fn be64(b:&[u8],o:usize)->u64{u64::from_be_bytes(b[o..o+8].try_into().unwrap())}

/// Virtual address space over the data blocks (block 0 = .vlt, block 1 = .bin).
struct Mem{blocks:Vec<Vec<u8>>}
impl Mem{
    const BASE:u32=0x1000_0000;
    fn base(i:usize)->u32{Self::BASE*(i as u32+1)}
    fn find(&self,a:u32)->Result<(usize,usize),String>{
        for (i,b) in self.blocks.iter().enumerate(){let s=Self::base(i);if a>=s&&((a-s) as usize)<b.len(){return Ok((i,(a-s) as usize))}}
        Err(format!("address {a:#x} outside blocks"))
    }
    fn read(&self,a:u32,n:usize)->Result<&[u8],String>{let (i,o)=self.find(a)?;self.blocks[i].get(o..o+n).ok_or_else(||format!("read past end at {a:#x}"))}
    fn u8(&self,a:u32)->Result<u8,String>{Ok(self.read(a,1)?[0])}
    fn u16(&self,a:u32)->Result<u16,String>{Ok(be16(self.read(a,2)?,0))}
    fn u32(&self,a:u32)->Result<u32,String>{Ok(be32(self.read(a,4)?,0))}
    fn u64(&self,a:u32)->Result<u64,String>{Ok(be64(self.read(a,8)?,0))}
    fn cstr(&self,a:u32)->Result<String,String>{
        let (i,o)=self.find(a)?;let b=&self.blocks[i];let e=b[o..].iter().position(|&c|c==0).ok_or("unterminated string")?;
        Ok(b[o..o+e].iter().map(|&c|c as char).collect())
    }
    fn w32(&mut self,a:u32,v:u32)->Result<(),String>{let (i,o)=self.find(a)?;self.blocks[i][o..o+4].copy_from_slice(&v.to_be_bytes());Ok(())}
}

#[derive(Debug,Clone)] pub struct TypeDesc{pub name:String,pub size:u32,pub key:u64}
#[derive(Debug,Clone)] pub struct Field{pub name_key:u64,pub type_name:String,pub offset:u16,pub size:u16,pub max_count:u16,pub flags:u8}
#[derive(Debug,Clone)] pub struct Class{pub key:u64,pub layout_size:u32,pub fields:Vec<Field>}
#[derive(Debug,Clone)] pub struct Attribute{pub name_key:u64,pub type_name:String,pub flags:u8,pub raw:u32}
#[derive(Debug,Clone)] pub struct Collection{pub key:u64,pub class_key:u64,pub parent_key:u64,pub attributes:Vec<Attribute>}

pub struct Database{pub vault_key:u64,pub relocations:usize,pub types:Vec<TypeDesc>,pub classes:Vec<Class>,pub collections:Vec<Collection>,mem:Mem,names:HashMap<u64,String>}

impl Database{
    /// Load a Wii vault pair.  `known_names` are identifiers to hash for readable output and lookups.
    pub fn load(vlt:&[u8],bin:&[u8],known_names:impl IntoIterator<Item=String>)->Result<Self,String>{
        // Chunk walk (Vault::Vault).
        let mut chunks:HashMap<[u8;4],(usize,usize)>=HashMap::new();let mut o=0;
        while o+8<=vlt.len(){let size=be32(vlt,o+4) as usize;if size<8{break}chunks.insert(vlt[o..o+4].try_into().unwrap(),(o,size));o+=size;}
        let chunk=|t:&[u8;4]|chunks.get(t).copied().ok_or_else(||format!("missing chunk {}",String::from_utf8_lossy(t)));
        let (vo,_)=chunk(b"Vers")?;let vault_key=be64(vlt,vo+8);
        let mut mem=Mem{blocks:vec![vlt.to_vec(),bin.to_vec()]};
        // Relocations (Vault::Initialize): entries of 16 bytes from PtrN+8.
        let (po,ps)=chunk(b"PtrN")?;let mut base=0u32;let mut relocs=0;
        let mut e=po+8;
        while e+16<=po+ps{
            let (dest,kind,block,_a,b)=(be32(vlt,e),be16(vlt,e+4),be16(vlt,e+6) as usize,be32(vlt,e+8),be32(vlt,e+12));
            match kind{0=>break,1=>mem.w32(base+dest,0)?,2=>base=Mem::base(block),3=>mem.w32(base+dest,Mem::base(block)+b)?,_=>return Err("cross-vault export reference is not supported".into())}
            relocs+=1;e+=16;
        }
        let names:HashMap<u64,String>=known_names.into_iter().map(|s|(string_hash64(&s),s)).collect();
        // Exports.
        let (eo,_)=chunk(b"ExpN")?;let n=be32(vlt,eo+12) as usize;
        let (class_t,coll_t)=(string_hash64("Attrib::ClassLoadData"),string_hash64("Attrib::CollectionLoadData"));
        let b0=Mem::base(0);
        let mut db=Database{vault_key,relocations:relocs,types:vec![],classes:vec![],collections:vec![],mem,names};
        let mut exports=vec![];
        for i in 0..n{let p=eo+16+i*24;exports.push((be64(vlt,p),be64(vlt,p+8),be32(vlt,p+16),be32(vlt,p+20)));}
        // Database record: type table.
        for &(_,ty,_,off) in &exports{
            if ty==class_t||ty==coll_t{continue}
            let p=b0+off;let n_types=db.mem.u32(p+8)? as usize;let mut s=db.mem.u32(p+12)?;
            for i in 0..n_types{let name=db.mem.cstr(s)?;let size=db.mem.u32(p+0x10+4*i as u32)?;s+=name.len() as u32+1;let key=string_hash64(&name);db.types.push(TypeDesc{name,size,key});}
        }
        let tname=|db:&Database,k:u64|db.types.iter().find(|t|t.key==k).map(|t|t.name.clone()).unwrap_or_else(||format!("{k:#x}"));
        for &(_,ty,_,off) in &exports{
            let p=b0+off;
            if ty==class_t{
                let (key,nfields,fptr,layout)=(db.mem.u64(p)?,db.mem.u32(p+0xc)?,db.mem.u32(p+0x10)?,db.mem.u32(p+0x14)?);
                let mut fields=vec![];
                for i in 0..nfields{let f=fptr+i*0x18;fields.push(Field{name_key:db.mem.u64(f)?,type_name:tname(&db,db.mem.u64(f+8)?),offset:db.mem.u16(f+0x10)?,size:db.mem.u16(f+0x12)?,max_count:db.mem.u16(f+0x14)?,flags:db.mem.u8(f+0x16)?});}
                db.classes.push(Class{key,layout_size:layout,fields});
            }else if ty==coll_t{
                let (key,cls,parent)=(db.mem.u64(p)?,db.mem.u64(p+8)?,db.mem.u64(p+0x10)?);
                let (n_attr,n_types)=(db.mem.u32(p+0x20)? as usize,db.mem.u16(p+0x26)? as usize);
                let mut tkeys=vec![];for i in 0..n_types{tkeys.push(db.mem.u64(p+0x30+8*i as u32)?);}
                let nodes=p+0x30+8*n_types as u32;let mut attrs=vec![];
                for i in 0..n_attr{
                    let q=nodes+16*i as u32;let ti=db.mem.u16(q+0xc)? as usize;
                    attrs.push(Attribute{name_key:db.mem.u64(q)?,type_name:tkeys.get(ti).map(|&k|tname(&db,k)).unwrap_or("?".into()),flags:db.mem.u8(q+0xe)?,raw:db.mem.u32(q+8)?});
                }
                db.collections.push(Collection{key,class_key:cls,parent_key:parent,attributes:attrs});
            }
        }
        Ok(db)
    }

    pub fn name(&self,key:u64)->String{self.names.get(&key).cloned().unwrap_or_else(||format!("{key:#018x}"))}

    /// Decode an attribute value into JSON (see module docs for the storage rules).
    pub fn value(&self,a:&Attribute)->Result<Value,String>{
        let size=self.types.iter().find(|t|t.name==a.type_name).map(|t|t.size).unwrap_or(4) as usize;
        let ty=a.type_name.as_str();
        let one=|p:u32|->Result<Value,String>{Ok(match ty{
            "EA::Reflection::Float"=>json!(f32::from_bits(self.mem.u32(p)?)),
            "EA::Reflection::Double"=>json!(f64::from_bits(self.mem.u64(p)?)),
            "EA::Reflection::UInt32"=>json!(self.mem.u32(p)?),
            "EA::Reflection::Int16"=>json!(self.mem.u16(p)? as i16),
            "EA::Reflection::UInt16"=>json!(self.mem.u16(p)?),
            "EA::Reflection::Int8"=>json!(self.mem.u8(p)? as i8),
            "EA::Reflection::UInt8"|"EA::Reflection::Char"=>json!(self.mem.u8(p)?),
            "EA::Reflection::Bool"=>json!(self.mem.u8(p)?!=0),
            "EA::Reflection::UInt64"|"Attrib::Key"=>json!(self.mem.u64(p)?),
            "EA::Reflection::Int64"=>json!(self.mem.u64(p)? as i64),
            "Attrib::Types::Vector2"=>json!((0..2).map(|i|self.mem.u32(p+4*i).map(f32::from_bits)).collect::<Result<Vec<_>,_>>()?),
            "Attrib::Types::Vector3"=>json!((0..3).map(|i|self.mem.u32(p+4*i).map(f32::from_bits)).collect::<Result<Vec<_>,_>>()?),
            "Attrib::Types::Vector4"=>json!((0..4).map(|i|self.mem.u32(p+4*i).map(f32::from_bits)).collect::<Result<Vec<_>,_>>()?),
            "Attrib::RefSpec"=>json!({"class_key":self.mem.u64(p)?,"collection_key":self.mem.u64(p+8)?,"attribute_key":self.mem.u64(p+16)?}),
            "EA::Reflection::Text"=>json!(self.mem.cstr(self.mem.u32(p)?)?),
            t if t=="EA::Reflection::Int32"||t.starts_with("Enums::")=>json!(self.mem.u32(p)? as i32),
            _=>json!(self.mem.read(p,size.min(64))?.iter().map(|b|format!("{b:02x}")).collect::<String>()),
        })};
        if a.flags&0x40!=0{
            // Inline: integers are left-justified in the 32-bit word; Text is an inline pointer.
            let word=a.raw.to_be_bytes();
            return Ok(match ty{
                "EA::Reflection::Text"=>json!(self.mem.cstr(a.raw)?),
                "EA::Reflection::Float"=>json!(f32::from_bits(a.raw)),
                "EA::Reflection::Bool"=>json!(word[0]!=0),
                "EA::Reflection::UInt8"|"EA::Reflection::Char"=>json!(word[0]),
                "EA::Reflection::Int8"=>json!(word[0] as i8),
                "EA::Reflection::UInt16"=>json!(u16::from_be_bytes([word[0],word[1]])),
                "EA::Reflection::Int16"=>json!(i16::from_be_bytes([word[0],word[1]])),
                "EA::Reflection::UInt32"=>json!(a.raw),
                _=>json!(a.raw as i32),
            })
        }
        if a.flags&0x02!=0{
            let (count,esz)=(self.mem.u16(a.raw)? as u32,self.mem.u16(a.raw+4)? as u32);
            let mut items=vec![];
            for i in 0..count{items.push(one(a.raw+8+i*esz)?);}
            return Ok(Value::Array(items))
        }
        one(a.raw)
    }

    pub fn find_collection(&self,class:&str,name:&str)->Option<&Collection>{
        let (ck,nk)=(string_hash64(class),string_hash64(name));
        self.collections.iter().find(|c|c.class_key==ck&&c.key==nk)
    }
    pub fn attribute(&self,c:&Collection,name:&str)->Option<Value>{
        let k=string_hash64(name);
        c.attributes.iter().find(|a|a.name_key==k).and_then(|a|self.value(a).ok())
    }
    /// Whole database as JSON (for `--vlt-dump` and cross-checks against the Python decoder).
    pub fn to_json(&self)->Value{
        json!({
            "vault_key":self.vault_key,"relocations":self.relocations,
            "types":self.types.iter().map(|t|json!({"name":t.name,"size":t.size})).collect::<Vec<_>>(),
            "classes":self.classes.iter().map(|c|json!({"name":self.name(c.key),"key":c.key,"layout_size":c.layout_size,"fields":c.fields.iter().map(|f|json!({"name":self.name(f.name_key),"type":f.type_name,"offset":f.offset,"size":f.size,"max_count":f.max_count,"flags":f.flags})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "collections":self.collections.iter().map(|c|json!({"name":self.name(c.key),"key":c.key,"class":self.name(c.class_key),"parent":(c.parent_key!=0).then(||self.name(c.parent_key)),
                "attributes":c.attributes.iter().map(|a|json!({"name":self.name(a.name_key),"type":a.type_name,"value":self.value(a).unwrap_or(Value::Null)})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        })
    }
}

/// Identifiers recovered by hashing strings from the executable and game data (see tools/prove.py); used to label keys.
pub fn known_names()->Vec<String>{include_str!("vlt_names.txt").lines().map(String::from).collect()}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn hash64_matches_original_machine_code(){
        let v:Value=serde_json::from_str(include_str!("../tests/data/hash64_vectors.json")).unwrap();
        let list=v["vectors"].as_array().unwrap();assert!(list.len()>=80);
        for e in list{
            let data:Vec<u8>=(0..e["data"].as_str().unwrap().len()/2).map(|i|u8::from_str_radix(&e["data"].as_str().unwrap()[2*i..2*i+2],16).unwrap()).collect();
            let want=u64::from_str_radix(e["hash"].as_str().unwrap(),16).unwrap();
            assert_eq!(hash64(&data,SEED),want,"len {}",data.len());
        }
    }
    #[test]
    fn empty_string_hashes_to_zero(){assert_eq!(string_hash64(""),0)}
    #[test]
    fn real_database_decodes_when_data_present(){
        let dir=crate::bridge::data_root().join("files").join("data").join("db");
        let (Ok(v),Ok(b))=(std::fs::read(dir.join("db.vlt")),std::fs::read(dir.join("db.bin"))) else{eprintln!("DATA not present; skipped");return};
        let names=["character_info","player","start_location","start_direction","area","area_name","area_type"].map(String::from);
        let db=Database::load(&v,&b,names).unwrap();
        assert_eq!(db.types.len(),75);assert_eq!(db.classes.len(),32);assert!(db.collections.len()>=900);
        let p=db.find_collection("character_info","player").expect("player collection");
        assert_eq!(db.attribute(p,"start_location").unwrap(),json!([12.0,0.0,-53.0]));
        assert_eq!(db.attribute(p,"start_direction").unwrap(),json!([0.0,0.0,1.0]));
    }
}
