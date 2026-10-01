//! AEMS module banks (`audio/aems/*.abk`, magic `ABKC`): EA's sound-event module graph.  The graph is shipped as
//! compiled PowerPC routines (one per class) plus fixup tables; the runtime patches them in place and never
//! interprets a data graph.  Layout from `SNDAEMS_addmodulebank` (0x802764dc) and `SNDAEMSI_resolvemodulebank`
//! (0x8027620c), all offsets relative to the bank start:
//! * +0x0a u16 class count, +0x14 total size, +0x18 module-code/data size, +0x1c class definitions,
//!   +0x20/+0x24 embedded `BNKb` sample bank offset/size (`SNDbankadd`), +0x28/+0x2c a second bank (unused here).
//! * +0x30 function fixups `{count, offsets}`: at each offset the u16 pair (+0, +4) is a `lis`/`ori` immediate pair
//!   holding an index into the AEMS function table (0x804b9f30); the runtime replaces it with that function's address.
//! * +0x34 pointer fixups `{count, offsets}`: the u32 at each offset is made absolute.
//! * +0x38 Csis bindings `{count, 12-byte entries}`: +4 handle offset, +8 interface-id offset (u16 interface, u16 id),
//!   +0xc kind (0 global variable, 1 class, otherwise function).
//! * Class definition: +0x24 u8 input count, +0x27 u8 output count, +0x28 code offset, +0x2c data offset,
//!   +0x3c `inputs+outputs` u32 slot offsets inside the data (each receives the bank pointer); the next class follows.
use crate::ppc;
use serde_json::{json,Value};
use std::collections::BTreeMap;

fn be16(d:&[u8],o:usize)->Result<u16,String>{d.get(o..o+2).map(|b|u16::from_be_bytes([b[0],b[1]])).ok_or_else(||format!("read past end at {o:#x}"))}
fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("read past end at {o:#x}"))}

/// The executable's AEMS module function table (0x804b9f30), by index.
pub const FUNCTIONS:[&str;52]=["UpdateClassDestructor","UpdateClassData","UpdateGlobalVariable","updatecreate","updatedestroy","UpdateCallFunction",
    "updatecounter","updaterandom","updaterandomshuffle","updaterandomweighted","updaterangetrig","updatedelaytrig","updatestategen","updatemerge",
    "updateenvelope","updatetable","updatedelayline","updatemux","updatedemux","updatemin","updatemax","updatescale","updateadd","updatesubtract",
    "updatemultiply","updatedivide","updatemodulo","updateplayer","updateoscillator","updateramp","updateaddmax","updatesubtractmin","updatemultiplymax",
    "updatemin2","updatemax2","updatescale2","updateadd2","UpdateFunction","UpdateControlClass","UpdateSetGlobalVariable","streampitchmult",
    "streamtimemult","streamvol","streamazimuth","streamstub","streamfxwet0","streamlowpass","streamhighpass","streamdrylevel","streamstub","streamstub","streamstub"];

pub struct Bank{pub json:Value,pub listing:String}

/// `csi_names`: 16-bit id -> name from the sibling Csis registry (`playground_aems.csi`), used to label bindings.
pub fn decode(d:&[u8],csi_names:&BTreeMap<u16,String>)->Result<Bank,String>{
    if d.get(..4)!=Some(b"ABKC"){return Err("not an ABKC module bank".into())}
    let (ncls,total,module,defs,bank,bank_size,ff,pf,cf)=(be16(d,0xa)? as usize,be32(d,0x14)? as usize,be32(d,0x18)? as usize,be32(d,0x1c)? as usize,be32(d,0x20)? as usize,be32(d,0x24)? as usize,be32(d,0x30)? as usize,be32(d,0x34)? as usize,be32(d,0x38)? as usize);
    if total!=d.len(){return Err(format!("header size {total:#x} but file is {:#x}",d.len()))}
    // Classes
    let mut classes=vec![];let mut p=defs;
    for i in 0..ncls{
        let (ni,no)=(d.get(p+0x24).copied().ok_or("class past end")? as usize,d.get(p+0x27).copied().ok_or("class past end")? as usize);
        let slots:Vec<u32>=(0..ni+no).map(|k|be32(d,p+0x3c+4*k)).collect::<Result<_,_>>()?;
        classes.push(json!({"index":i,"offset":p,"id":format!("{:08x}",be32(d,p)?),"inputs":ni,"outputs":no,"flags":[d[p+0x25],d[p+0x26]],"code":be32(d,p+0x28)?,"data":be32(d,p+0x2c)?,
            "words_30":[be32(d,p+0x30)?,be32(d,p+0x34)?,be32(d,p+0x38)?],"slots":slots}));
        p+=0x3c+4*(ni+no);
    }
    // Fixups
    let n=be32(d,ff)? as usize;if n>100_000{return Err("function fixup count".into())}
    let mut fn_at:BTreeMap<u32,String>=BTreeMap::new();let mut funcs=vec![];
    for k in 0..n{
        let o=be32(d,ff+4+4*k)? as usize;let idx=((be16(d,o)? as u32)<<16)|be16(d,o+4)? as u32;
        let name=FUNCTIONS.get(idx as usize).ok_or_else(||format!("function index {idx} outside the 52-entry table"))?;
        fn_at.insert(o as u32&!3,format!("AEMS function #{idx} {name} (high half)"));fn_at.insert((o as u32+4)&!3,format!("AEMS function #{idx} {name} (low half)"));
        funcs.push(json!({"offset":o,"index":idx,"function":name}));
    }
    let n=be32(d,pf)? as usize;if n>100_000{return Err("pointer fixup count".into())}
    let mut ptr_at:BTreeMap<u32,u32>=BTreeMap::new();
    let ptrs:Vec<Value>=(0..n).map(|k|{let o=be32(d,pf+4+4*k)?;let v=be32(d,o as usize)?;ptr_at.insert(o,v);Ok(json!({"offset":o,"target":v}))}).collect::<Result<_,String>>()?;
    let n=be32(d,cf)? as usize;if n>100_000{return Err("Csis binding count".into())}
    let binds:Vec<Value>=(0..n).map(|k|{let e=cf+4+12*k;let (h,i,kind)=(be32(d,e)?,be32(d,e+4)? as usize,d.get(e+8).copied().ok_or("binding past end")?);
        let (iface,id)=(be16(d,i)?,be16(d,i+2)?);
        Ok(json!({"handle":h,"interface":format!("{iface:04x}"),"id":format!("{id:04x}"),"name":csi_names.get(&id),"kind":match kind{0=>"global_variable",1=>"class",_=>"function"}}))}).collect::<Result<_,String>>()?;
    // Code: from the lowest class code offset up to the lowest data offset.
    let code_starts:Vec<usize>=classes.iter().map(|c|c["code"].as_u64().unwrap() as usize).collect();
    let data_start=classes.iter().map(|c|c["data"].as_u64().unwrap() as usize).min().unwrap_or(module);
    let lo=code_starts.iter().copied().min().unwrap_or(data_start);
    if lo>data_start||data_start>module{return Err(format!("code {lo:#x}..{data_start:#x} outside module area {module:#x}"))}
    let names=|t:u32|->Option<String>{code_starts.iter().position(|&s|s as u32==t).map(|i|format!("class {i} entry"))};
    let note=|a:u32,_w:u32|->Option<String>{fn_at.get(&a).cloned().or_else(||ptr_at.get(&a).map(|v|format!("pointer fixup -> {v:#x}")))};
    // Zero words between the last routine and the data area are alignment padding, not code.
    let mut end=data_start;while end>lo&&be32(d,end-4)?==0{end-=4}
    let (text,unknown)=ppc::listing(&d[lo..end],lo as u32,&names,&note);
    let mut listing=String::new();
    for (i,s) in code_starts.iter().enumerate(){listing+=&format!("; class {i} code at {s:#x}\n")}
    listing+=&text;
    Ok(Bank{json:json!({"version":format!("{:02x}{:02x}{:02x}{:02x}",d[4],d[5],d[6],d[7]),"u16_08":be16(d,8)?,"size":total,"module_size":module,"sample_bank":{"offset":bank,"size":bank_size},
        "classes":classes,"code":{"start":lo,"end":end,"instructions":(end-lo)/4,"padding_words":(data_start-end)/4,"undecoded_words":unknown},"data":{"start":data_start,"end":module},
        "function_fixups":funcs,"pointer_fixups":ptrs,"csis_bindings":binds}),listing})
}

pub fn parse(d:&[u8])->Result<Value,String>{decode(d,&BTreeMap::new()).map(|b|b.json)}
