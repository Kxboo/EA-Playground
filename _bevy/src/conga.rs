//! EA Conga gesture state machines (`conga/conga.gsm`): the Wii-remote motion recogniser's data.
//!
//! Layout from `EA::Conga::LoadFromFiles` (0x8025780c) and the `Serialize` methods (big-endian, raw `memcpy`):
//!   u8 flag; u32 value; `MachineDefinitionManager`: u32 count, per machine u32 type (0) + `MachineDefinition`
//!   { u32 len, name, u32 n, n x { u32 len, sequence name, u32 len, alias } };
//!   `SequenceDefinitionManager`: u32 count, per sequence u32 type (1) + `SequenceDefinition`
//!   { u32 len, name, u32 total, transitions... } where `total` counts every transition including nested children
//!   (`SerializeChildren` 0x8025d3d0) and each transition is `u32 type` + its own fields.
//! Transition types (`FactoryInitialize` 0x8025712c) and their field order (the `Serialize` of each class):
//!   2 MagnitudePeak: u16 device, vec3, f32 x3, u8 x3, vec3          3 MagnitudeThreshold: u16, vec3, u8 x2, f32 x3, u8 x3, f32 x3
//!   4 AccelerometerAtRest: u16, u8 x3                               5 AccelerometerOrientation: u16, vec3, u8 x3, f32 x2
//!   6 AtRestResetDelay: u8, f32                                     7 DualDeviceTransition: child, child, u8, u8 (children inline)
//!   8 AtRestSetCriteria: u16, u8, f32, u32                          9 AccelerometerAtRestInPast: u16, f32, u8 x3, f32 x4, u8 x3, f32 x3
use crate::skeleton::be32;

#[derive(Debug,Clone)]
pub struct Transition{pub kind:u32,pub name:&'static str,pub device:Option<u16>,pub floats:Vec<f32>,pub flags:Vec<u8>,pub children:Vec<Transition>}
impl Transition{fn descendants(&self)->usize{self.children.iter().map(|c|1+c.descendants()).sum()}}
#[derive(Debug,Clone)] pub struct Machine{pub name:String,pub sequences:Vec<(String,String)>}
#[derive(Debug,Clone)] pub struct Sequence{pub name:String,pub transitions:Vec<Transition>}
#[derive(Debug,Clone)] pub struct Gsm{pub flag:u8,pub value:u32,pub machines:Vec<Machine>,pub sequences:Vec<Sequence>,pub trailing:usize}

struct Rd<'a>{d:&'a [u8],p:usize}
#[derive(Clone,Copy)] enum F{U8,U16,F32,U32}
impl<'a> Rd<'a>{
    fn take(&mut self,n:usize)->Result<&'a [u8],String>{let s=self.d.get(self.p..self.p+n).ok_or("gsm truncated")?;self.p+=n;Ok(s)}
    fn u8(&mut self)->Result<u8,String>{Ok(self.take(1)?[0])}
    fn u32(&mut self)->Result<u32,String>{be32(self.d,{let p=self.p;self.take(4)?;p})}
    fn string(&mut self)->Result<String,String>{let n=self.u32()? as usize;if n>0x3f{return Err(format!("name length {n} exceeds 63"))}Ok(String::from_utf8_lossy(self.take(n)?).into_owned())}
    fn f32(&mut self)->Result<f32,String>{Ok(f32::from_bits(self.u32()?))}
    fn fields(&mut self,layout:&[F])->Result<(Option<u16>,Vec<f32>,Vec<u8>),String>{
        let (mut dev,mut fl,mut fg)=(None,vec![],vec![]);
        for f in layout{match f{F::U16=>{let b=self.take(2)?;dev=Some(u16::from_be_bytes([b[0],b[1]]))}F::U8=>fg.push(self.u8()?),F::F32=>fl.push(self.f32()?),F::U32=>fl.push(self.u32()? as f32)}}
        Ok((dev,fl,fg))
    }
    fn transition(&mut self,kind:u32)->Result<Transition,String>{
        use F::*;
        let (name,layout):(&'static str,&[F])=match kind{
            2=>("MagnitudePeak",&[U16,F32,F32,F32,F32,F32,F32,U8,U8,U8,F32,F32,F32]),
            3=>("MagnitudeThreshold",&[U16,F32,F32,F32,U8,U8,F32,F32,F32,U8,U8,U8,F32,F32,F32]),
            4=>("AccelerometerAtRest",&[U16,U8,U8,U8]),
            5=>("AccelerometerOrientation",&[U16,F32,F32,F32,U8,U8,U8,F32,F32]),
            6=>("AtRestResetDelay",&[U8,F32]),
            8=>("AtRestSetCriteria",&[U16,U8,F32,U32]),
            9=>("AccelerometerAtRestInPast",&[U16,F32,U8,U8,U8,F32,F32,F32,F32,U8,U8,U8,F32,F32,F32]),
            7=>{
                let mut children=vec![];
                for _ in 0..2{let t=self.u32()?;if t!=u32::MAX{let c=self.transition(t)?;children.push(c)}}
                let flags=vec![self.u8()?,self.u8()?];
                return Ok(Transition{kind,name:"DualDeviceTransition",device:None,floats:vec![],flags,children})
            }
            k=>return Err(format!("unknown transition type {k}")),
        };
        let (device,floats,flags)=self.fields(layout)?;
        Ok(Transition{kind,name,device,floats,flags,children:vec![]})
    }
}

pub fn parse(d:&[u8])->Result<Gsm,String>{
    let mut r=Rd{d,p:0};
    let flag=r.u8()?;let value=r.u32()?;
    let mut machines=vec![];
    for _ in 0..r.u32()?{
        let t=r.u32()?;if t!=0{return Err(format!("machine type {t}"))}
        let name=r.string()?;let n=r.u32()?;
        let mut seqs=vec![];for _ in 0..n{seqs.push((r.string()?,r.string()?))}
        machines.push(Machine{name,sequences:seqs});
    }
    let mut sequences=vec![];
    for _ in 0..r.u32()?{
        let t=r.u32()?;if t!=1{return Err(format!("sequence type {t}"))}
        let name=r.string()?;let total=r.u32()? as usize;
        let mut transitions=vec![];let mut i=0;
        while i<total{let k=r.u32()?;let tr=r.transition(k)?;i+=1+tr.descendants();transitions.push(tr)}
        if i!=total{return Err(format!("sequence {name}: {i} transitions read, {total} declared"))}
        sequences.push(Sequence{name,transitions});
    }
    Ok(Gsm{flag,value,machines,sequences,trailing:d.len()-r.p})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn rejects_truncated(){assert!(parse(&[1,0,0,0]).is_err())}
    /// The shipped gesture file parses (34 sequences, consistent transition counts) and every machine alias names a sequence.
    #[test] fn shipped_conga_file_decodes(){
        let cov=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Remaster/research/coverage.json");
        let Ok(txt)=std::fs::read_to_string(&cov) else{eprintln!("coverage.json absent; skipped");return};
        let j:serde_json::Value=serde_json::from_str(&txt).unwrap();
        let Some(r)=j["records"].as_array().unwrap().iter().find(|r|r["extension"]==".gsm") else{return};
        let Ok((d,_))=crate::archive::read_virtual(r["source"].as_str().unwrap()) else{eprintln!("DATA absent; skipped");return};
        let g=parse(&d).unwrap();
        // Bytes after the last sequence are never read by `LoadFromFiles` (leftovers of the authoring buffer).
        assert!(g.trailing<d.len()/2);
        let names:std::collections::HashSet<&str>=g.sequences.iter().map(|s|s.name.as_str()).collect();
        assert_eq!(g.machines.len(),1);
        for (a,_) in &g.machines[0].sequences{assert!(names.contains(a.as_str()),"machine references unknown sequence {a}")}
        let total:usize=g.sequences.iter().map(|s|s.transitions.len()).sum();
        eprintln!("conga: {} machine(s), {} sequences, {total} top-level transitions, {} unread trailing bytes",g.machines.len(),g.sequences.len(),g.trailing);
        assert_eq!(g.sequences.len(),34);
    }
}
