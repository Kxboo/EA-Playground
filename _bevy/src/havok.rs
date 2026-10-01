//! Havok 4.6 packfile (`.hkx`) reader and collision extraction - Rust reimplementation.
//!
//! * Class reflection (`havok_classes.json`) is generated from playgroundz.elf by `tools/extract_havok_classes.py`:
//!   each `hk*Class` initialiser (`__sinit_...`) calls `hkClass::hkClass(name, parent, size, ..., members, n, ...)`
//!   @0x801e25c8; the 20-byte `hkClassMember` records ({name,class,enum,type,subtype,cArraySize,flags,offset}) are read
//!   from .rodata.  Sizes agree with the executable's symbol sizes.
//! * Packfile: 0x40-byte header, 0x30-byte section headers, local/global/virtual fixups (big-endian).
//! * Collision: rigid bodies -> shapes (MOPP -> child, simple mesh, box, convex vertices via plane equations,
//!   convex transform/translate, transform, list).  The terrain/prop collision the original game used.
use serde_json::{json,Value};
use std::collections::{BTreeMap,HashMap};

#[derive(Debug,Clone)] struct Member{name:String,cls:Option<String>,ty:u8,sub:u8,csize:u16,off:u16}
#[derive(Debug,Clone)] struct Class{parent:Option<String>,size:u32,members:Vec<Member>}
pub struct ClassTable(HashMap<String,Class>);
impl ClassTable{
    pub fn embedded()->Self{Self::from_json(include_str!("havok_classes.json"))}
    pub fn from_json(s:&str)->Self{
        let v:Value=serde_json::from_str(s).expect("class table");let mut m=HashMap::new();
        for (name,c) in v.as_object().unwrap(){
            m.insert(name.clone(),Class{parent:c["parent"].as_str().map(String::from),size:c["size"].as_u64().unwrap() as u32,
                members:c["members"].as_array().unwrap().iter().map(|x|{let (ty,sub)=(x["t"].as_u64().unwrap() as u8,x["s"].as_u64().unwrap() as u8);
                    Member{name:x["n"].as_str().unwrap().into(),cls:x["c"].as_str().map(String::from),ty,sub:enum_storage(ty,sub,x["f"].as_u64().unwrap_or(0)),csize:x["a"].as_u64().unwrap() as u16,off:x["o"].as_u64().unwrap() as u16}}).collect()});
        }
        Self(m)
    }
    pub fn has(&self,name:&str)->bool{self.0.contains_key(name)}
    fn all_members(&self,name:&str)->Vec<&Member>{
        let Some(c)=self.0.get(name) else{return vec![]};
        let mut v=c.parent.as_deref().map(|p|self.all_members(p)).unwrap_or_default();v.extend(c.members.iter());v
    }
    pub fn len(&self)->usize{self.0.len()}
    /// Extend with the reflection a packfile carries in its own `__types__` section (`hkClass` objects with their
    /// `hkClassMember` arrays), for classes the executable does not reflect (e.g. `hkxScene` in the airplane files).
    /// Returns the names that were added.
    pub fn with_file_types(&self,pf:&Packfile)->(ClassTable,Vec<String>){
        let mut m=self.0.clone();let mut added=vec![];
        let name_of=|r:&Value|->Option<String>{let a=r["$ref"].as_array()?;let o=pf.decode_object(a[0].as_u64()? as usize,a[1].as_i64()? as i32,"hkClass",1);o["name"].as_str().map(String::from)};
        for (&(si,off),cn) in &pf.virt{
            if cn!="hkClass"{continue}
            let c=pf.decode_object(si,off,"hkClass",0);let Some(name)=c["name"].as_str() else{continue};
            if m.contains_key(name){continue}
            let members=pf.array_elements(&c["declaredMembers"]).iter().map(|x|Member{name:x["name"].as_str().unwrap_or("").into(),cls:name_of(&x["class"]),
                ty:x["type"].as_u64().unwrap_or(0) as u8,sub:enum_storage(x["type"].as_u64().unwrap_or(0) as u8,x["subtype"].as_u64().unwrap_or(0) as u8,x["flags"].as_u64().unwrap_or(0)),csize:x["cArraySize"].as_i64().unwrap_or(0) as u16,off:x["offset"].as_u64().unwrap_or(0) as u16}).collect();
            m.insert(name.to_string(),Class{parent:name_of(&c["parent"]),size:c["objectSize"].as_u64().unwrap_or(0) as u32,members});added.push(name.to_string());
        }
        (ClassTable(m),added)
    }
}

/// Havok 4.x enum members record their storage size in `hkClassMember::flags` (ENUM_8 = 8, ENUM_16 = 16,
/// ENUM_32 = 32) rather than in the subtype; map it to the matching unsigned scalar type.
fn enum_storage(ty:u8,sub:u8,flags:u64)->u8{if ty!=T_ENUM||sub!=0{return sub}match flags&0x38{8=>T_UINT8,16=>T_UINT16,32=>T_UINT32,_=>T_UINT32}}

// hkClassMember::Type values (Havok 4.6)
const T_BOOL:u8=1;const T_CHAR:u8=2;const T_INT8:u8=3;const T_UINT8:u8=4;const T_INT16:u8=5;const T_UINT16:u8=6;const T_INT32:u8=7;const T_UINT32:u8=8;const T_INT64:u8=9;const T_UINT64:u8=10;const T_REAL:u8=11;
const T_VECTOR4:u8=12;const T_QUAT:u8=13;const T_MATRIX3:u8=14;const T_ROTATION:u8=15;const T_QSTRANSFORM:u8=16;const T_MATRIX4:u8=17;const T_TRANSFORM:u8=18;const T_ZERO:u8=19;
const T_POINTER:u8=20;const T_FUNCPTR:u8=21;const T_ARRAY:u8=22;const T_ENUM:u8=24;const T_STRUCT:u8=25;const T_SIMPLEARRAY:u8=26;const T_HOMOGENEOUS:u8=27;const T_CSTRING:u8=29;const T_ULONG:u8=30;const T_VOID:u8=0;const T_HALF:u8=32;

fn be32(b:&[u8],o:usize)->u32{u32::from_be_bytes(b[o..o+4].try_into().unwrap())}
fn bei32(b:&[u8],o:usize)->i32{be32(b,o) as i32}
fn scalar(ty:u8)->Option<usize>{match ty{T_BOOL|T_CHAR|T_INT8|T_UINT8=>Some(1),T_INT16|T_UINT16|T_HALF=>Some(2),T_INT32|T_UINT32|T_REAL|T_ULONG=>Some(4),T_INT64|T_UINT64=>Some(8),_=>None}}
fn floats(ty:u8)->Option<usize>{match ty{T_VECTOR4|T_QUAT=>Some(4),T_MATRIX3|T_ROTATION|T_QSTRANSFORM=>Some(12),T_MATRIX4|T_TRANSFORM=>Some(16),_=>None}}

pub struct Packfile<'a>{data:&'a [u8],classes:&'a ClassTable,starts:Vec<usize>,local:HashMap<(usize,i32),i32>,glob:HashMap<(usize,i32),(usize,i32)>,pub virt:BTreeMap<(usize,i32),String>}
type Ref=(usize,i32);

impl<'a> Packfile<'a>{
    pub fn parse(data:&'a [u8],classes:&'a ClassTable)->Result<Self,String>{
        if data.len()<0x40||be32(data,0)!=0x57e0e057||be32(data,4)!=0x10c0c010{return Err("not a Havok packfile".into())}
        let nsec=bei32(data,20) as usize;let mut secs=vec![];
        for i in 0..nsec{let o=0x40+i*0x30;let v:Vec<usize>=(0..7).map(|k|be32(data,o+20+4*k) as usize).collect();secs.push(v);}
        let starts:Vec<usize>=secs.iter().map(|s|s[0]).collect();
        let (mut local,mut glob,mut virt_raw)=(HashMap::new(),HashMap::new(),vec![]);
        for (si,s) in secs.iter().enumerate(){
            let b=s[0];
            let mut o=b+s[1];while o+8<=data.len()&&o<b+s[2]{let (src,dst)=(bei32(data,o),bei32(data,o+4));if src==-1{break}local.insert((si,src),dst);o+=8;}
            let mut o=b+s[2];while o+12<=data.len()&&o<b+s[3]{let (src,ts,dst)=(bei32(data,o),bei32(data,o+4),bei32(data,o+8));if src==-1{break}glob.insert((si,src),(ts as usize,dst));o+=12;}
            let mut o=b+s[3];while o+12<=data.len()&&o<b+s[4]{let (src,ts,no)=(bei32(data,o),bei32(data,o+4),bei32(data,o+8));if src==-1{break}virt_raw.push((si,src,ts as usize,no));o+=12;}
        }
        let mut virt=BTreeMap::new();
        for (si,src,ts,no) in virt_raw{
            let a=starts[ts]+no as usize;let e=data[a..].iter().position(|&c|c==0).ok_or("class name")?;
            virt.insert((si,src),String::from_utf8_lossy(&data[a..a+e]).into_owned());
        }
        Ok(Self{data,classes,starts,local,glob,virt})
    }
    fn abs(&self,si:usize,off:i32)->usize{self.starts[si]+off as usize}
    fn read(&self,si:usize,off:i32,n:usize)->&[u8]{let a=self.abs(si,off);&self.data[a..a+n]}
    fn ptr(&self,si:usize,off:i32)->Option<Ref>{self.local.get(&(si,off)).map(|&d|(si,d)).or_else(||self.glob.get(&(si,off)).copied())}
    fn width(&self,ty:u8,m:&Member)->usize{
        if let Some(n)=scalar(ty){return n}
        if let Some(n)=floats(ty){return if ty==T_VECTOR4||ty==T_QUAT{16}else{4*n}}
        match ty{T_POINTER|T_CSTRING|T_FUNCPTR=>4,T_STRUCT=>m.cls.as_ref().and_then(|c|self.classes.0.get(c)).map(|c|c.size as usize).unwrap_or(0),T_ENUM=>scalar(m.sub).unwrap_or(4),T_ARRAY|T_SIMPLEARRAY|T_HOMOGENEOUS=>12,_=>4}
    }
    pub fn decode_object(&self,si:usize,off:i32,cls:&str,depth:u32)->Value{
        let mut o=serde_json::Map::new();
        for m in self.classes.all_members(cls){o.insert(m.name.clone(),self.decode_member(si,off+m.off as i32,m,depth));}
        Value::Object(o)
    }
    fn decode_member(&self,si:usize,off:i32,m:&Member,depth:u32)->Value{
        if m.csize>0&&m.ty!=T_ARRAY&&m.ty!=23{let w=self.width(m.ty,m) as i32;return Value::Array((0..m.csize as i32).map(|i|self.decode_type(si,off+i*w,m.ty,m,depth)).collect())}
        self.decode_type(si,off,m.ty,m,depth)
    }
    fn decode_type(&self,si:usize,o:i32,ty:u8,m:&Member,depth:u32)->Value{
        let b=|n:usize|self.read(si,o,n);
        if scalar(ty).is_some(){return match ty{
            T_BOOL=>json!(b(1)[0]!=0),T_CHAR|T_INT8=>json!(b(1)[0] as i8),T_UINT8=>json!(b(1)[0]),T_INT16=>json!(i16::from_be_bytes(b(2).try_into().unwrap())),T_UINT16|T_HALF=>json!(u16::from_be_bytes(b(2).try_into().unwrap())),
            T_INT32=>json!(bei32(b(4),0)),T_UINT32|T_ULONG=>json!(be32(b(4),0)),T_INT64=>json!(i64::from_be_bytes(b(8).try_into().unwrap())),T_UINT64=>json!(u64::from_be_bytes(b(8).try_into().unwrap())),
            _=>json!(f32::from_bits(be32(b(4),0)) as f64)}}
        if let Some(n)=floats(ty){return Value::Array((0..n).map(|i|json!(f32::from_bits(be32(b(4*n),4*i)) as f64)).collect())}
        match ty{
            T_ENUM=>self.decode_type(si,o,m.sub,m,depth),
            T_ZERO|T_VOID=>Value::Null,
            T_POINTER=>match self.ptr(si,o){Some(r)=>json!({"$ref":[r.0,r.1],"class":self.virt.get(&r)}),None=>Value::Null},
            T_CSTRING=>match self.ptr(si,o){Some(r)=>{let a=self.abs(r.0,r.1);let e=self.data[a..].iter().position(|&c|c==0).unwrap_or(0);json!(String::from_utf8_lossy(&self.data[a..a+e]))},None=>Value::Null},
            T_STRUCT=>match (&m.cls,depth<6){(Some(c),true)=>self.decode_object(si,o,c,depth+1),_=>Value::Null},
            T_ARRAY|T_SIMPLEARRAY|T_HOMOGENEOUS=>{
                let n=bei32(b(8),4);let data=self.ptr(si,o);
                json!({"$array":true,"count":n,"data":data.map(|r|json!([r.0,r.1])),"elem":m.sub,"elem_class":m.cls})
            }
            _=>Value::Null,
        }
    }
    pub fn array_elements(&self,arr:&Value)->Vec<Value>{
        let Some(d)=arr["data"].as_array() else{return vec![]};
        let (si,off)=(d[0].as_u64().unwrap() as usize,d[1].as_i64().unwrap() as i32);let et=arr["elem"].as_u64().unwrap() as u8;
        let m=Member{name:String::new(),cls:arr["elem_class"].as_str().map(String::from),ty:et,sub:0,csize:0,off:0};
        let w=self.width(et,&m) as i32;let n=arr["count"].as_i64().unwrap_or(0).max(0) as i32;
        (0..n).map(|i|self.decode_type(si,off+i*w,et,&m,1)).collect()
    }
    pub fn class_of(&self,r:Ref)->Option<&String>{self.virt.get(&r)}
    /// Every virtual-class object in the packfile, decoded by its reflected class, with arrays expanded (`$elements`).
    pub fn dump(&self)->Value{
        fn expand(pf:&Packfile,v:&mut Value,depth:u32){
            match v{
                Value::Object(m)=>{
                    if m.get("$array")==Some(&json!(true))&&depth<4{let els=pf.array_elements(&Value::Object(m.clone()));m.insert("$elements".into(),Value::Array(els));}
                    for (_,x) in m.iter_mut(){expand(pf,x,depth+1)}
                }
                Value::Array(a)=>for x in a{expand(pf,x,depth+1)},
                _=>{}
            }
        }
        let objs:Vec<Value>=self.virt.iter().map(|(&(si,off),cn)|{
            let mut o=if self.classes.0.contains_key(cn){self.decode_object(si,off,cn,0)}else{json!({"$unreflected":true})};
            expand(self,&mut o,0);json!({"ref":[si,off],"class":cn,"object":o})
        }).collect();
        json!({"sections":self.starts.len(),"objects":objs})
    }
}

/// One static/dynamic rigid body's collision geometry in world space.
pub struct Body{pub name:String,pub shape:String,pub friction:f64,pub restitution:f64,pub mass_inv:f64,pub tris:Vec<[[f32;3];3]>}

type M4=[[f64;4];4]; // row-major, column vectors: p' = M * p
fn ident()->M4{[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]]}
fn mul(a:&M4,b:&M4)->M4{let mut r=[[0.;4];4];for i in 0..4{for j in 0..4{for k in 0..4{r[i][j]+=a[i][k]*b[k][j]}}}r}
/// hkTransform = basis vectors c0,c1,c2 then translation (each vec4).
fn mat_from(t:&Value)->M4{
    let f:Vec<f64>=t.as_array().map(|a|a.iter().map(|x|x.as_f64().unwrap_or(0.)).collect()).unwrap_or_else(||vec![0.;16]);
    let mut m=ident();for c in 0..3{for r in 0..3{m[r][c]=f[c*4+r]}}for r in 0..3{m[r][3]=f[12+r]}m
}
fn apply(m:&M4,p:[f64;3])->[f64;3]{[0,1,2].map(|r|m[r][0]*p[0]+m[r][1]*p[1]+m[r][2]*p[2]+m[r][3])}
fn tri_out(m:&M4,t:[[f64;3];3])->[[f32;3];3]{t.map(|p|{let q=apply(m,p);[q[0] as f32,q[1] as f32,q[2] as f32]})}
fn v3(v:&Value)->[f64;3]{let a=v.as_array().unwrap();[a[0].as_f64().unwrap(),a[1].as_f64().unwrap(),a[2].as_f64().unwrap()]}

pub struct Collision{pub bodies:Vec<Body>,pub skipped:BTreeMap<String,u32>}
impl Collision{
    pub fn load(pf:&Packfile)->Self{
        let mut c=Collision{bodies:vec![],skipped:BTreeMap::new()};
        for (&(si,off),cn) in &pf.virt{
            if cn!="hkRigidBody"{continue}
            let o=pf.decode_object(si,off,cn,0);
            let Some(r)=o["collidable"]["shape"]["$ref"].as_array() else{continue};
            let sref:Ref=(r[0].as_u64().unwrap() as usize,r[1].as_i64().unwrap() as i32);
            let m=mat_from(&o["motion"]["motionState"]["transform"]);
            let tris=c.shape_tris(pf,sref,&ident(),&m);
            if tris.is_empty(){continue}
            c.bodies.push(Body{name:o["name"].as_str().unwrap_or("").into(),shape:pf.class_of(sref).cloned().unwrap_or_default(),friction:o["material"]["friction"].as_f64().unwrap_or(0.),restitution:o["material"]["restitution"].as_f64().unwrap_or(0.),
                mass_inv:o["motion"]["inertiaAndMassInv"][3].as_f64().unwrap_or(0.),tris});
        }
        c
    }
    fn child(&mut self,_pf:&Packfile,o:&Value,key:&str)->Option<Ref>{
        let r=o[key]["childShape"]["$ref"].as_array()?;Some((r[0].as_u64()? as usize,r[1].as_i64()? as i32))
    }
    /// `local` is the shape-local transform accumulated so far; `world` the rigid body's transform.
    fn shape_tris(&mut self,pf:&Packfile,r:Ref,local:&M4,world:&M4)->Vec<[[f32;3];3]>{
        let Some(cn)=pf.class_of(r).cloned() else{return vec![]};
        let o=pf.decode_object(r.0,r.1,&cn,0);let m=mul(world,local);
        match cn.as_str(){
            "hkMoppBvTreeShape"=>match self.child(pf,&o,"child"){Some(c)=>self.shape_tris(pf,c,local,world),None=>vec![]},
            "hkSimpleMeshShape"=>{
                let v:Vec<[f64;3]>=pf.array_elements(&o["vertices"]).iter().map(v3).collect();
                pf.array_elements(&o["triangles"]).iter().filter_map(|t|{
                    let (a,b,c)=(t["a"].as_i64()? as usize,t["b"].as_i64()? as usize,t["c"].as_i64()? as usize);
                    Some(tri_out(&m,[*v.get(a)?,*v.get(b)?,*v.get(c)?]))}).collect()
            }
            "hkBoxShape"=>box_tris(v3(&o["halfExtents"])).into_iter().map(|t|tri_out(&m,t)).collect(),
            "hkConvexVerticesShape"=>convex_tris(pf,&o).into_iter().map(|t|tri_out(&m,t)).collect(),
            "hkConvexTransformShape"|"hkTransformShape"=>match self.child(pf,&o,"childShape"){Some(c)=>self.shape_tris(pf,c,&mul(local,&mat_from(&o["transform"])),world),None=>vec![]},
            "hkConvexTranslateShape"=>match self.child(pf,&o,"childShape"){Some(c)=>{let mut t=ident();let v=v3(&o["translation"]);for i in 0..3{t[i][3]=v[i]}self.shape_tris(pf,c,&mul(local,&t),world)},None=>vec![]},
            "hkListShape"=>{
                let mut out=vec![];
                for ci in pf.array_elements(&o["childInfo"]){
                    if let Some(rr)=ci["shape"]["$ref"].as_array(){out.extend(self.shape_tris(pf,(rr[0].as_u64().unwrap() as usize,rr[1].as_i64().unwrap() as i32),local,world));}
                }
                out
            }
            other=>{*self.skipped.entry(other.into()).or_insert(0)+=1;vec![]}
        }
    }
    pub fn triangles(&self)->impl Iterator<Item=&[[f32;3];3]>{self.bodies.iter().flat_map(|b|b.tris.iter())}
}

fn box_tris(h:[f64;3])->Vec<[[f64;3];3]>{
    let mut v=vec![];for sx in [-1.,1.]{for sy in [-1.,1.]{for sz in [-1.,1.]{v.push([sx*h[0],sy*h[1],sz*h[2]])}}}
    let mut t=vec![];for (a,b,c,d) in [(0,1,3,2),(4,6,7,5),(0,4,5,1),(2,3,7,6),(0,2,6,4),(1,5,7,3)]{t.push([v[a],v[b],v[c]]);t.push([v[a],v[c],v[d]]);}t
}
fn sub(a:[f64;3],b:[f64;3])->[f64;3]{[a[0]-b[0],a[1]-b[1],a[2]-b[2]]}
fn dot(a:[f64;3],b:[f64;3])->f64{a[0]*b[0]+a[1]*b[1]+a[2]*b[2]}
fn cross(a:[f64;3],b:[f64;3])->[f64;3]{[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]}
fn norm(a:[f64;3])->[f64;3]{let l=dot(a,a).sqrt();[a[0]/l,a[1]/l,a[2]/l]}
/// Faces of a convex vertices shape rebuilt from its stored plane equations.
fn convex_tris(pf:&Packfile,o:&Value)->Vec<[[f64;3];3]>{
    let n=o["numVertices"].as_i64().unwrap_or(0).max(0) as usize;let mut pts=vec![];
    for f in pf.array_elements(&o["rotatedVertices"]){for i in 0..4{pts.push([f["x"][i].as_f64().unwrap(),f["y"][i].as_f64().unwrap(),f["z"][i].as_f64().unwrap()])}}
    pts.truncate(n);let mut tris=vec![];
    for pl in pf.array_elements(&o["planeEquations"]){
        let p=pl.as_array().unwrap();let (nrm,d)=([p[0].as_f64().unwrap(),p[1].as_f64().unwrap(),p[2].as_f64().unwrap()],p[3].as_f64().unwrap());
        let mut on:Vec<[f64;3]>=pts.iter().copied().filter(|q|(dot(nrm,*q)+d).abs()<1e-3).collect();
        if on.len()<3{continue}
        let c=[0,1,2].map(|i|on.iter().map(|q|q[i]).sum::<f64>()/on.len() as f64);
        let u=norm(if nrm[0].abs()<0.9{cross(nrm,[1.,0.,0.])}else{cross(nrm,[0.,1.,0.])});let w=cross(nrm,u);
        on.sort_by(|a,b|{let (da,db)=(sub(*a,c),sub(*b,c));dot(da,w).atan2(dot(da,u)).partial_cmp(&dot(db,w).atan2(dot(db,u))).unwrap()});
        for i in 1..on.len()-1{tris.push([on[0],on[i],on[i+1]]);}
    }
    tris
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn class_table_has_havok_classes(){
        let t=ClassTable::embedded();assert!(t.len()>=240);
        assert_eq!(t.0["hkxAttribute"].size,12);assert_eq!(t.0["hkxAttribute"].members.len(),2);
        assert!(t.all_members("hkRigidBody").len()>=20);assert_eq!(t.0["hkRigidBody"].size,512);
    }
    #[test]
    fn collision_matches_python_reference_when_data_present(){
        let dir=crate::bridge::data_root().join("files").join("data").join("physics");
        if !dir.join("playground.hkx").exists(){eprintln!("DATA not present; skipped");return}
        let golden:Value=serde_json::from_str(include_str!("../tests/data/collision_golden.json")).unwrap();
        let ct=ClassTable::embedded();
        for name in ["playground","playground_park","playground_nature","playground_stadium"]{
            let bytes=std::fs::read(dir.join(format!("{name}.hkx"))).unwrap();
            let pf=Packfile::parse(&bytes,&ct).unwrap();let c=Collision::load(&pf);
            let g=&golden[name];
            assert_eq!(c.bodies.len() as u64,g["bodies"].as_u64().unwrap(),"{name} bodies");
            let tris:Vec<_>=c.triangles().collect();assert_eq!(tris.len() as u64,g["triangles"].as_u64().unwrap(),"{name} triangles");
            let mut sum=[0f64;3];for t in &tris{for p in t.iter(){for i in 0..3{sum[i]+=p[i] as f64}}}
            for i in 0..3{let want=g["sum"][i].as_f64().unwrap();assert!((sum[i]-want).abs()<0.01*want.abs().max(1.)+1.0,"{name} sum[{i}] {} vs {want}",sum[i]);}
            assert!(c.skipped.is_empty(),"{name} skipped {:?}",c.skipped);
        }
    }
}
