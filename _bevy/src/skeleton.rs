//! EAGL `.ske` skeletons - Rust port of Remaster/src/legacy/eagl_skeleton.py.
//!
//! `.ske` / `.anm` files are little-endian ELF containers with big-endian payloads.  The `.ske` `.data` section
//! holds one `__Skeleton:::Root` header (bone count at +8) followed by 112-byte bone records
//! (scale[3], parent i32, local quaternion xyzw, local translation, cached world rotation rows, ...); bone names
//! come from `__Bone:::Root.<name>` symbols (index = symbol value / 16).  Bind-pose world transforms are composed
//! from the local quaternion/translation chain (the cached matrix in the file agrees, see the tests).
use std::collections::HashMap;

pub fn le32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
pub fn le16(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_le_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
pub fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
pub fn be16(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}
pub fn bef32(d:&[u8],o:usize)->Result<f64,String>{Ok(f32::from_bits(be32(d,o)?) as f64)}

pub struct Section{pub name:String,pub ty:u32,pub offset:usize,pub size:usize,pub entsize:usize,pub link:usize}

/// Little-endian ELF container (`.ske`, `.anm`): sections, symbols and the self-relocation map.
pub struct Container<'a>{pub data:&'a [u8],pub sections:Vec<Section>,pub data_start:usize,pub data_size:usize}

fn cstr(d:&[u8],o:usize)->Result<String,String>{let s=d.get(o..).ok_or("string start")?;let e=s.iter().position(|&c|c==0).ok_or("unterminated string")?;Ok(String::from_utf8_lossy(&s[..e]).into_owned())}

impl<'a> Container<'a>{
    pub fn parse(d:&'a [u8])->Result<Self,String>{
        if d.len()<52||&d[..4]!=b"\x7fELF"{return Err("Not an ELF file".into())}
        let shoff=le32(d,32)? as usize;let (shentsize,shnum,shstr)=(le16(d,46)? as usize,le16(d,48)? as usize,le16(d,50)? as usize);
        if shnum==0||shentsize<40||shstr>=shnum{return Err("Unsupported section table".into())}
        let mut raw=vec![];
        for i in 0..shnum{let o=shoff+i*shentsize;raw.push((le32(d,o)? as usize,le32(d,o+4)?,le32(d,o+16)? as usize,le32(d,o+20)? as usize,le32(d,o+24)? as usize,le32(d,o+36)? as usize));}
        let base=raw[shstr].2;
        let mut sections=vec![];
        for (ni,ty,offset,size,link,entsize) in raw{sections.push(Section{name:cstr(d,base+ni)?,ty,offset,size,entsize,link})}
        // `.data` section (the pseudo-ELF containers keep it at index 1)
        let ds=sections.iter().find(|s|s.name==".data").or_else(||sections.get(1)).ok_or("No .data section")?;
        let (data_start,data_size)=(ds.offset,ds.size);
        Ok(Self{data:d,sections,data_start,data_size})
    }
    pub fn section(&self,name:&str)->Option<&Section>{self.sections.iter().find(|s|s.name==name)}
    /// (name, value) of every symbol in `.symtab`.
    pub fn symbols(&self)->Result<Vec<(String,u32)>,String>{
        let (Some(sym),Some(strs))=(self.section(".symtab"),self.section(".strtab")) else{return Ok(vec![])};
        let step=if sym.entsize==0{16}else{sym.entsize};let mut out=vec![];
        for i in 0..sym.size/step{let o=sym.offset+i*step;out.push((cstr(self.data,strs.offset+le32(self.data,o)? as usize)?,le32(self.data,o+4)?));}
        Ok(out)
    }
    /// `{r_offset: pointed_to_offset}` (both relative to `.data`) for every R_MIPS_32 self relocation: the pointer is
    /// stored little-endian inside an otherwise big-endian payload.
    pub fn self_relocations(&self)->HashMap<usize,usize>{
        let mut out=HashMap::new();
        let Some(rel)=self.section(".rel.data") else{return out};
        for i in 0..rel.size/8{
            let o=rel.offset+i*8;let (Ok(off),Ok(info))=(le32(self.data,o),le32(self.data,o+4)) else{continue};
            if info&0xff!=2{continue}
            let Ok(v)=le32(self.data,self.data_start+off as usize) else{continue};
            if (v as usize)<self.data.len(){out.insert(off as usize,v as usize);}
        }
        out
    }
}

#[derive(Debug,Clone)]
pub struct Bone{pub index:usize,pub name:String,pub scale:[f64;3],pub parent:i32,pub quat:[f64;4],pub trans:[f64;3],
    pub world_matrix:[f64;9],pub world_translation:[f64;3],pub cached_matrix:[f64;9]}

#[derive(Debug,Clone)]
pub struct Skeleton{pub flags:u32,pub bones:Vec<Bone>}

pub fn quat_to_matrix(q:[f64;4])->[f64;9]{
    let [x,y,z,w]=q;let (xx,yy,zz)=(x*x,y*y,z*z);let (xy,xz,yz)=(x*y,x*z,y*z);let (wx,wy,wz)=(w*x,w*y,w*z);
    [1.-2.*(yy+zz),2.*(xy-wz),2.*(xz+wy),  2.*(xy+wz),1.-2.*(xx+zz),2.*(yz-wx),  2.*(xz-wy),2.*(yz+wx),1.-2.*(xx+yy)]
}
pub fn mat3_mul(a:&[f64;9],b:&[f64;9])->[f64;9]{
    [a[0]*b[0]+a[1]*b[3]+a[2]*b[6],a[0]*b[1]+a[1]*b[4]+a[2]*b[7],a[0]*b[2]+a[1]*b[5]+a[2]*b[8],
     a[3]*b[0]+a[4]*b[3]+a[5]*b[6],a[3]*b[1]+a[4]*b[4]+a[5]*b[7],a[3]*b[2]+a[4]*b[5]+a[5]*b[8],
     a[6]*b[0]+a[7]*b[3]+a[8]*b[6],a[6]*b[1]+a[7]*b[4]+a[8]*b[7],a[6]*b[2]+a[7]*b[5]+a[8]*b[8]]
}
pub fn mat3_vec(m:&[f64;9],v:[f64;3])->[f64;3]{[m[0]*v[0]+m[1]*v[1]+m[2]*v[2],m[3]*v[0]+m[4]*v[1]+m[5]*v[2],m[6]*v[0]+m[7]*v[1]+m[8]*v[2]]}

const HEADER:usize=16;const RECORD:usize=112;

impl Skeleton{
    pub fn parse(data:&[u8])->Result<Self,String>{
        let c=Container::parse(data)?;
        if c.section(".data").is_none(){return Err("No .data section".into())}
        let mut names:HashMap<usize,String>=HashMap::new();let mut skel_off=None;
        for (n,v) in c.symbols()?{
            if n.starts_with("__Bone:::Root."){names.insert(v as usize/0x10,n.rsplit('.').next().unwrap_or("").to_string());}
            else if n.starts_with("__Skeleton:::Root"){skel_off=Some(v as usize);}
        }
        let skel_off=skel_off.ok_or("Skeleton symbol missing")?;
        let base=c.data_start+skel_off;
        let flags=be32(data,base)?;let mut count=be32(data,base+8)? as usize;
        if count==0{count=names.len()}
        let table=skel_off+HEADER;
        if count==0||c.data_start+table+count*RECORD>c.data_start+c.data_size{return Err("Declared bone table does not fit the .data section".into())}
        let mut bones=vec![];
        for i in 0..count{
            let b=c.data_start+table+i*RECORD;let f=|k:usize|bef32(data,b+k*4);
            let parent=be32(data,b+12)? as i32;
            let mut cached=[0.;9];let rows=[(0usize,12usize),(3,16),(6,20)];
            for (dst,src) in rows{for k in 0..3{cached[dst+k]=f(src+k)?;}}
            bones.push(Bone{index:i,name:names.get(&i).cloned().unwrap_or_else(||format!("bone_{i}")),scale:[f(0)?,f(1)?,f(2)?],parent,quat:[f(4)?,f(5)?,f(6)?,f(7)?],trans:[f(8)?,f(9)?,f(10)?],
                world_matrix:[0.;9],world_translation:[0.;3],cached_matrix:cached});
        }
        for b in &bones{
            if b.parent< -1||b.parent>=count as i32{return Err(format!("Bone {}: invalid parent {}",b.index,b.parent))}
            let mut seen=std::collections::HashSet::new();let mut p=b.index as i32;
            while p!=-1{if !seen.insert(p){return Err(format!("Cyclic skeleton at bone {}",b.index))}p=bones[p as usize].parent;}
        }
        // Compose bind-pose world transforms (parents may come after children, so resolve recursively).
        let mut done=vec![false;count];
        fn resolve(i:usize,bones:&mut Vec<Bone>,done:&mut Vec<bool>){
            if done[i]{return}
            let local=quat_to_matrix(bones[i].quat);
            if bones[i].parent<0{bones[i].world_matrix=local;bones[i].world_translation=bones[i].trans;}
            else{
                let p=bones[i].parent as usize;resolve(p,bones,done);
                let (pm,pt)=(bones[p].world_matrix,bones[p].world_translation);let r=mat3_vec(&pm,bones[i].trans);
                bones[i].world_matrix=mat3_mul(&pm,&local);bones[i].world_translation=[pt[0]+r[0],pt[1]+r[1],pt[2]+r[2]];
            }
            done[i]=true;
        }
        for i in 0..count{resolve(i,&mut bones,&mut done);}
        Ok(Self{flags,bones})
    }
    pub fn is_player(&self)->bool{self.bones.len()==68&&self.bones.iter().any(|b|b.name=="l_SideCheek")}
    pub fn children(&self,i:usize)->Vec<usize>{self.bones.iter().filter(|b|b.parent==i as i32).map(|b|b.index).collect()}
}

#[cfg(test)]
mod tests{
    use super::*;
    fn golden()->Option<serde_json::Value>{
        let p=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/anim_golden.json");
        serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
    }
    pub fn ske_source(bank:&str)->String{
        let (viv,_)=bank.split_once("::").unwrap();
        let inner=if viv.contains("player"){"player_skel.ske".to_string()}else{format!("{}_skel.ske",std::path::Path::new(viv).file_stem().unwrap().to_string_lossy())};
        format!("{}::{}",crate::bridge::data_root().join("files").join("data").join(viv.replace('/',"\\")).to_string_lossy(),inner)
    }
    #[test]
    fn matches_python_reference_and_cached_matrices(){
        let Some(g)=golden() else{eprintln!("anim_golden.json absent; skipped");return};
        let mut checked=0;
        for (bank,entry) in g.as_object().unwrap(){
            let Ok((data,_))=crate::archive::read_virtual(&ske_source(bank)) else{eprintln!("DATA absent; skipped");return};
            let skel=Skeleton::parse(&data).unwrap();
            let want=entry["bones"].as_array().unwrap();
            assert_eq!(skel.bones.len(),want.len(),"{bank}: bone count");
            for (b,w) in skel.bones.iter().zip(want){
                assert_eq!(b.name,w[1].as_str().unwrap());assert_eq!(b.parent as i64,w[2].as_i64().unwrap());
                let close=|got:&[f64],exp:&serde_json::Value|got.iter().zip(exp.as_array().unwrap()).all(|(a,e)|(a-e.as_f64().unwrap()).abs()<1e-9);
                assert!(close(&b.scale,&w[3])&&close(&b.quat,&w[4])&&close(&b.trans,&w[5])&&close(&b.world_translation,&w[6])&&close(&b.world_matrix,&w[7]),"{bank}: bone {} differs",b.name);
                // The file's own cached world rotation must agree with the composed chain (proves the parent/quaternion layout).
                let d:f64=b.world_matrix.iter().zip(b.cached_matrix.iter()).map(|(a,c)|(a-c).abs()).sum();
                assert!(d<0.01,"{bank}: bone {} cached matrix differs by {d}",b.name);
                checked+=1;
            }
        }
        eprintln!("skeletons: {checked} bones match the Python reference");
    }
}
