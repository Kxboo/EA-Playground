//! EA APT user-interface programs (`.apt` + `.const`), Wii big-endian "Apt Data:74" variant.
//!
//! Every layout below is read from the executable, not from other APT versions:
//! * `AptCharacterAnimation::Fixup` (0x8013e7f8): character list, per-type pointer fields, imports (16 bytes:
//!   movie, name, character, runtime handle), exports (8 bytes: name, character).  Characters start with
//!   `{type, 0x09876543}`; sprite/movie bodies (`AptMovie`) start at +8.
//! * `AptMovie::resolve` (0x8014b944): frames `{item count, item pointer list}` and frame-item pointer fields
//!   (action +4, label +4, place-object name +0x34 / clip actions +0x3c, init-action +8).
//! * `AptActionInterpreter::_parseStream` (0x80134064): operand size/alignment of every opcode and which operands
//!   are pointers; constant-file entry kinds (1 string, 3 undefined, 4 register, 5 boolean, 6 float, 7 integer,
//!   8 lookup).
//! * Interpreter dispatch table at 0x8047b58c: opcode names (`_FunctionAptAction<Name>`).
//! * `_FunctionAptActionPushWord`/`PushFloat`: unaligned immediates are big-endian.
use serde_json::{json,Value};

pub const SIGNATURE:u32=0x0987_6543;

fn be32(d:&[u8],o:usize)->Result<u32,String>{d.get(o..o+4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).ok_or_else(||format!("APT read past end at {o:#x}"))}
fn bef(d:&[u8],o:usize)->Result<f32,String>{Ok(f32::from_bits(be32(d,o)?))}
fn cstr(d:&[u8],o:usize)->Result<String,String>{
    let s=d.get(o..).ok_or_else(||format!("string at {o:#x} outside file"))?;
    let e=s.iter().position(|&c|c==0).ok_or_else(||format!("unterminated string at {o:#x}"))?;
    Ok(String::from_utf8_lossy(&s[..e]).into_owned())
}
fn opt_str(d:&[u8],o:u32)->Result<Value,String>{if o==0{Ok(Value::Null)}else{Ok(json!(cstr(d,o as usize)?))}}
fn rect(d:&[u8],o:usize)->Result<Value,String>{Ok(json!([bef(d,o)?,bef(d,o+4)?,bef(d,o+8)?,bef(d,o+12)?]))}

/// Opcode names from the interpreter's dispatch table (index = opcode).
pub fn opcode_name(op:u8)->Option<&'static str>{
    Some(match op{
        0x00=>"End",0x04=>"NextFrame",0x05=>"PrevFrame",0x06=>"Play",0x07=>"Stop",0x08=>"ToggleQuality",0x09=>"StopSounds",
        0x0a=>"Add",0x0b=>"Subtract",0x0c=>"Multiply",0x0d=>"Divide",0x0e=>"Equals",0x0f=>"LessThan",0x10=>"And",0x11=>"Or",
        0x12=>"Not",0x13=>"StringEquals",0x14=>"StringLength",0x15=>"SubString",0x17=>"Pop",0x18=>"ToInteger",
        0x1c=>"GetVariable",0x1d=>"SetVariable",0x20=>"SetTarget2",0x21=>"StringAdd",0x22=>"GetProperty",0x23=>"SetProperty",
        0x24=>"CloneSprite",0x25=>"RemoveSprite",0x26=>"Trace",0x27=>"StartDragMovie",0x28=>"StopDragMovie",
        0x29=>"StringLessThan",0x2a=>"Throw",0x2b=>"CastOp",0x2c=>"ImplementsOp",0x30=>"Random",0x31=>"MBLength",
        0x32=>"CharToAscii",0x33=>"AsciiToChar",0x34=>"GetTimer",0x35=>"MBSubString",0x36=>"MBCharToAscii",0x37=>"MBAsciiToChar",
        0x3a=>"Delete",0x3b=>"Delete2",0x3c=>"DefineLocal",0x3d=>"CallFunction",0x3e=>"Return",0x3f=>"Modulo",0x40=>"NewObject",
        0x41=>"DefineLocal2",0x42=>"InitArray",0x43=>"InitObject",0x44=>"TypeOf",0x45=>"TargetPath",0x46=>"Enumerate",
        0x47=>"Add2",0x48=>"LessThan2",0x49=>"Equals2",0x4a=>"ToNumber",0x4b=>"ToString",0x4c=>"PushDuplicate",0x4d=>"StackSwap",
        0x4e=>"GetMember",0x4f=>"SetMember",0x50=>"Increment",0x51=>"Decrement",0x52=>"CallMethod",0x53=>"NewMethod",
        0x54=>"InstanceOf",0x55=>"Enumerate2",0x56=>"PushThis",0x58=>"PushGlobal",0x59=>"Push0",0x5a=>"Push1",
        0x5b=>"CallFuncAndPop",0x5c=>"CallFuncSetVar",0x5d=>"CallMethodPop",0x5e=>"CallMethodSetVar",0x60=>"BitAnd",
        0x61=>"BitOr",0x62=>"BitXor",0x63=>"BitLShift",0x64=>"BitRShift",0x65=>"BitURShift",0x66=>"StrictEquals",0x67=>"Greater",
        0x69=>"Extends",0x70=>"PushThisVariable",0x71=>"PushGlobalVariable",0x72=>"PushZeroSetVar",0x73=>"PushTrue",
        0x74=>"PushFalse",0x75=>"PushNULL",0x76=>"PushUndefined",0x81=>"GotoFrame",0x83=>"GetUrl",0x87=>"StoreRegister",
        0x88=>"DefineDictionary",0x8a=>"WaitForFrame",0x8b=>"SetTarget",0x8c=>"GotoLabel",0x8e=>"DefineFunction2",0x8f=>"Try",
        0x94=>"With",0x96=>"Push",0x99=>"BranchAlways",0x9a=>"GetUrl2",0x9b=>"DefineFunction",0x9d=>"BranchIfTrue",
        0x9e=>"CallFrame",0x9f=>"GotoFrame2",0xa1=>"PushString",0xa2=>"PushStringDictByte",0xa3=>"PushStringDictWord",
        0xa4=>"PushStringGetVar",0xa5=>"PushStringGetMember",0xa6=>"PushStringSetVar",0xa7=>"PushStringSetMember",
        0xae=>"StringDictByteGetVar",0xaf=>"StringDictByteGetMember",0xb0=>"DictCallFuncPop",0xb1=>"DictCallFuncSetVar",
        0xb2=>"DictCallMethodPop",0xb3=>"DictCallMethodSetVar",0xb4=>"PushFloat",0xb5=>"PushByte",0xb6=>"PushWord",
        0xb7=>"PushDWord",0xb8=>"BranchIfFalse",
        _=>return None,
    })
}

/// `.const` file: `"Apt constant file\x1a\0\0"`, movie character offset (+0x14), entry count (+0x18), entry offset (+0x1c).
pub struct ConstFile{pub movie:u32,pub entries:Vec<(u32,u32)>,data:Vec<u8>}
impl ConstFile{
    pub fn parse(d:&[u8])->Result<Self,String>{
        if d.get(..17)!=Some(b"Apt constant file"){return Err("missing 'Apt constant file' header".into())}
        let movie=be32(d,0x14)?;let n=be32(d,0x18)? as usize;let at=be32(d,0x1c)? as usize;
        if n>d.len()/8{return Err(format!("constant count {n} exceeds file"))}
        let entries=(0..n).map(|i|Ok((be32(d,at+8*i)?,be32(d,at+8*i+4)?))).collect::<Result<Vec<_>,String>>()?;
        Ok(ConstFile{movie,entries,data:d.to_vec()})
    }
    /// Decoded constant as `_parseStream` constructs it.
    pub fn value(&self,i:usize)->Result<Value,String>{
        let (k,v)=*self.entries.get(i).ok_or_else(||format!("constant index {i} out of range ({})",self.entries.len()))?;
        Ok(match k{
            1=>json!({"string":cstr(&self.data,v as usize)?}),
            3=>json!({"undefined":null}),
            4=>json!({"register":v}),
            5=>json!({"boolean":v!=0}),
            6=>json!({"float":f32::from_bits(v)}),
            7=>json!({"integer":v as i32}),
            8=>json!({"lookup":v}),
            // Kinds 0 and 2 are never materialised by the loader (no case in _parseStream); keep them raw.
            _=>json!({"kind":k,"raw":v}),
        })
    }
    pub fn to_json(&self)->Result<Value,String>{Ok(json!({"movie_offset":self.movie,"entries":(0..self.entries.len()).map(|i|self.value(i)).collect::<Result<Vec<_>,_>>()?}))}
}

pub struct Stats{pub instructions:usize,pub streams:usize,pub unknown_opcodes:usize}

/// Disassemble one action stream (terminated by opcode 0) exactly as `_parseStream` walks it.
pub fn disassemble(d:&[u8],start:usize,c:Option<&ConstFile>,st:&mut Stats)->Result<Vec<Value>,String>{
    st.streams+=1;
    let mut p=start;let mut out=vec![];
    let al=|p:usize|(p+3)&!3;
    loop{
        let at=p;let op=*d.get(p).ok_or_else(||format!("action stream runs past end at {p:#x}"))?;p+=1;
        st.instructions+=1;
        let name=opcode_name(op).map(String::from).unwrap_or_else(||{st.unknown_opcodes+=1;format!("Unknown{op:#04x}")});
        let mut ins=json!({"at":at,"op":name});
        let consts=|idx:&[u32]|->Result<Value,String>{Ok(match c{Some(c)=>json!(idx.iter().map(|&i|c.value(i as usize)).collect::<Result<Vec<_>,_>>()?),None=>json!(idx)})};
        match op{
            0=>{out.push(ins);break}
            0xb0|0xa2|0xae|0xaf|0xb1|0xb2|0xb3=>{let v=*d.get(p).ok_or("operand past end")?;p+=1;ins["index"]=json!(v);if let Some(c)=c{if let Ok(x)=c.value(v as usize){ins["constant"]=x}}}
            0xb5=>{let v=*d.get(p).ok_or("operand past end")? as i8;p+=1;ins["value"]=json!(v)}
            0xa3=>{let v=u16::from_be_bytes(d.get(p..p+2).ok_or("operand past end")?.try_into().unwrap());p+=2;ins["index"]=json!(v);if let Some(c)=c{if let Ok(x)=c.value(v as usize){ins["constant"]=x}}}
            0xb6=>{let v=i16::from_be_bytes(d.get(p..p+2).ok_or("operand past end")?.try_into().unwrap());p+=2;ins["value"]=json!(v)}
            0xb4=>{ins["value"]=json!(bef(d,p)?);p+=4}
            0xb7=>{ins["value"]=json!(be32(d,p)? as i32);p+=4}
            0xa1|0xa4|0xa5|0xa6|0xa7|0x8b|0x8c=>{p=al(p);ins["string"]=opt_str(d,be32(d,p)?)?;p+=4}
            0x88|0x96=>{
                p=al(p);let n=be32(d,p)? as usize;let q=be32(d,p+4)? as usize;p+=8;
                if n>d.len()/4{return Err(format!("constant list count {n} too large at {at:#x}"))}
                let idx=(0..n).map(|i|be32(d,q+4*i)).collect::<Result<Vec<_>,_>>()?;
                ins["indices"]=json!(idx);ins["values"]=consts(&idx)?;
            }
            0x99|0x9d|0xb8=>{p=al(p);let off=be32(d,p)? as i32;p+=4;ins["offset"]=json!(off);ins["target"]=json!(p as i64+off as i64)}
            0x81|0x9f|0x87=>{p=al(p);ins["value"]=json!(be32(d,p)?);p+=4}
            0x94=>{p=al(p);ins["size"]=json!(be32(d,p)?);p+=4}
            0x83=>{p=al(p);ins["url"]=opt_str(d,be32(d,p)?)?;ins["target"]=opt_str(d,be32(d,p+4)?)?;p+=8}
            0x9b=>{
                p=al(p);let (name,n,args,size)=(be32(d,p)?,be32(d,p+4)? as usize,be32(d,p+8)? as usize,be32(d,p+12)?);p+=0x18;
                if n>256{return Err(format!("DefineFunction argument count {n} at {at:#x}"))}
                ins["name"]=opt_str(d,name)?;ins["body_size"]=json!(size);
                ins["args"]=json!((0..n).map(|i|opt_str(d,be32(d,args+4*i)?)).collect::<Result<Vec<_>,_>>()?);
            }
            0x8e=>{
                p=al(p);let (name,n,flags,args,size)=(be32(d,p)?,be32(d,p+4)? as usize,be32(d,p+8)?,be32(d,p+12)? as usize,be32(d,p+16)?);p+=0x1c;
                if n>256{return Err(format!("DefineFunction2 argument count {n} at {at:#x}"))}
                ins["name"]=opt_str(d,name)?;ins["flags"]=json!(flags);ins["body_size"]=json!(size);
                ins["args"]=json!((0..n).map(|i|Ok(json!({"register":be32(d,args+8*i)?,"name":opt_str(d,be32(d,args+8*i+4)?)?}))).collect::<Result<Vec<_>,String>>()?);
            }
            0x8f=>{
                p=al(p);let flags=*d.get(p+0xc).ok_or("Try operand past end")?;
                ins["try_size"]=json!(be32(d,p)?);ins["catch_size"]=json!(be32(d,p+4)?);ins["finally_size"]=json!(be32(d,p+8)?);ins["flags"]=json!(flags);
                // Flag bit 2 selects a catch register; otherwise +0x10 is the catch variable name.
                if flags&4!=0{ins["catch_register"]=json!(be32(d,p+0x10)?)}else{ins["catch_name"]=opt_str(d,be32(d,p+0x10)?)?}
                p+=0x14;
            }
            _=>{}
        }
        out.push(ins);
    }
    Ok(out)
}

fn movie_body(d:&[u8],m:usize,c:Option<&ConstFile>,st:&mut Stats)->Result<Value,String>{
    let nf=be32(d,m)? as usize;let fp=be32(d,m+4)? as usize;
    if nf>100_000{return Err(format!("frame count {nf} at {m:#x}"))}
    let mut frames=vec![];
    for f in 0..nf{
        let ni=be32(d,fp+8*f)? as usize;let ip=be32(d,fp+8*f+4)? as usize;
        if ni>100_000{return Err(format!("frame item count {ni}"))}
        let mut items=vec![];
        for i in 0..ni{
            let q=be32(d,ip+4*i)? as usize;if q==0{items.push(Value::Null);continue}
            let t=be32(d,q)?;
            items.push(match t{
                1=>json!({"kind":"Action","actions":disassemble(d,be32(d,q+4)? as usize,c,st)?}),
                2=>json!({"kind":"FrameLabel","label":opt_str(d,be32(d,q+4)?)?,"flags":be32(d,q+8)?,"frame":be32(d,q+12)?}),
                3=>{
                    let clip=be32(d,q+0x3c)? as usize;
                    let clip_actions=if clip==0{Value::Null}else{
                        let n=be32(d,clip)? as usize;let e=be32(d,clip+4)? as usize;
                        if n>4096{return Err(format!("clip action count {n}"))}
                        json!((0..n).map(|k|Ok(json!({"flags":be32(d,e+12*k)?,"key":be32(d,e+12*k+4)?,"actions":disassemble(d,be32(d,e+12*k+8)? as usize,c,st)?}))).collect::<Result<Vec<_>,String>>()?)
                    };
                    json!({"kind":"PlaceObject","flags":be32(d,q+4)?,"depth":be32(d,q+8)? as i32,"character":be32(d,q+12)? as i32,
                        "rotscale":[bef(d,q+0x10)?,bef(d,q+0x14)?,bef(d,q+0x18)?,bef(d,q+0x1c)?],"translate":[bef(d,q+0x20)?,bef(d,q+0x24)?],
                        "color":format!("{:08x}",be32(d,q+0x28)?),"unknown_2c":be32(d,q+0x2c)?,"ratio":bef(d,q+0x30)?,
                        "name":opt_str(d,be32(d,q+0x34)?)?,"clip_depth":be32(d,q+0x38)? as i32,"clip_actions":clip_actions})
                }
                4=>json!({"kind":"RemoveObject","depth":be32(d,q+4)? as i32}),
                5=>json!({"kind":"BackgroundColor","color":format!("{:08x}",be32(d,q+4)?)}),
                8=>json!({"kind":"InitAction","sprite":be32(d,q+4)?,"actions":disassemble(d,be32(d,q+8)? as usize,c,st)?}),
                // Types 6/7 carry no pointers in AptMovie::resolve; keep their first words raw.
                t=>json!({"kind":format!("Item{t}"),"raw":[be32(d,q+4).ok(),be32(d,q+8).ok()]}),
            });
        }
        frames.push(json!(items));
    }
    Ok(json!(frames))
}

fn character(d:&[u8],q:usize,c:Option<&ConstFile>,st:&mut Stats)->Result<Value,String>{
    let t=be32(d,q)?;let sig=be32(d,q+4)?;
    if sig!=SIGNATURE{return Err(format!("character at {q:#x}: signature {sig:#x}"))}
    Ok(match t{
        1=>json!({"type":"Shape","bounds":rect(d,q+8)?,"geometry":be32(d,q+0x18)?}),
        2=>json!({"type":"EditText","bounds":rect(d,q+8)?,"font":be32(d,q+0x18)?,"alignment":be32(d,q+0x1c)?,"color":format!("{:08x}",be32(d,q+0x20)?),
            "font_height":bef(d,q+0x24)?,"read_only":be32(d,q+0x28)?,"multiline":be32(d,q+0x2c)?,"word_wrap":be32(d,q+0x30)?,
            "text":opt_str(d,be32(d,q+0x34)?)?,"variable":opt_str(d,be32(d,q+0x38)?)?}),
        3=>{let n=be32(d,q+0xc)? as usize;let g=be32(d,q+0x10)? as usize;if n>65536{return Err("font glyph count".into())}
            json!({"type":"Font","name":opt_str(d,be32(d,q+8)?)?,"glyphs":(0..n).map(|i|be32(d,g+4*i)).collect::<Result<Vec<_>,_>>()?})}
        4=>{
            let (nt,nv,vp,tp,nr,rp,na,ap)=(be32(d,q+0x1c)? as usize,be32(d,q+0x20)? as usize,be32(d,q+0x24)? as usize,be32(d,q+0x28)? as usize,be32(d,q+0x2c)? as usize,be32(d,q+0x30)? as usize,be32(d,q+0x34)? as usize,be32(d,q+0x38)? as usize);
            if nv>65536||nt>65536||nr>4096||na>4096{return Err("button counts".into())}
            json!({"type":"Button","is_menu":be32(d,q+8)?,"bounds":rect(d,q+0xc)?,
                "vertices":(0..nv).map(|i|Ok([bef(d,vp+8*i)?,bef(d,vp+8*i+4)?])).collect::<Result<Vec<_>,String>>()?,
                "triangle_count":nt,"triangles_offset":tp,"record_count":nr,"records_offset":rp,
                "actions":(0..na).map(|i|Ok(json!({"flags":be32(d,ap+8*i)?,"actions":disassemble(d,be32(d,ap+8*i+4)? as usize,c,st)?}))).collect::<Result<Vec<_>,String>>()?})
        }
        5=>json!({"type":"Sprite","frames":movie_body(d,q+8,c,st)?}),
        6=>json!({"type":"Sound","raw":be32(d,q+8)?}),
        7=>json!({"type":"Image","texture":be32(d,q+8)?}),
        8=>json!({"type":"Morph","start":be32(d,q+8)?,"end":be32(d,q+12)?}),
        9=>{
            let body=movie_body(d,q+8,c,st)?;
            let (ni,ip,ne,ep)=(be32(d,q+0x28)? as usize,be32(d,q+0x2c)? as usize,be32(d,q+0x30)? as usize,be32(d,q+0x34)? as usize);
            if ni>4096||ne>4096{return Err("import/export counts".into())}
            json!({"type":"Movie","frames":body,"width":be32(d,q+0x1c)?,"height":be32(d,q+0x20)?,"frame_ms":be32(d,q+0x24)?,
                "imports":(0..ni).map(|i|Ok(json!({"movie":opt_str(d,be32(d,ip+16*i)?)?,"name":opt_str(d,be32(d,ip+16*i+4)?)?,"character":be32(d,ip+16*i+8)?}))).collect::<Result<Vec<_>,String>>()?,
                "exports":(0..ne).map(|i|Ok(json!({"name":opt_str(d,be32(d,ep+8*i)?)?,"character":be32(d,ep+8*i+4)?}))).collect::<Result<Vec<_>,String>>()?})
        }
        10=>{let n=be32(d,q+0x30)? as usize;if n>4096{return Err("static text records".into())}
            json!({"type":"StaticText","bounds":rect(d,q+8)?,"record_count":n,"records_offset":be32(d,q+0x34)?})}
        12=>json!({"type":"Video"}),
        t=>json!({"type":format!("Type{t}")}),
    })
}

pub struct Movie{pub json:Value,pub stats:Stats}

/// Decode a complete APT program. `constants` is the sibling `.const` file (strongly recommended: the movie entry
/// offset and all dictionary/push constants live there).
pub fn parse(d:&[u8],constants:Option<&[u8]>)->Result<Movie,String>{
    if d.get(..9)!=Some(b"Apt Data:"){return Err("missing 'Apt Data:' header".into())}
    let version=String::from_utf8_lossy(&d[9..d[9..].iter().position(|&b|b==0x1a).map(|p|p+9).unwrap_or(11)]).into_owned();
    let cf=match constants{Some(c)=>Some(ConstFile::parse(c)?),None=>None};
    let movie_at=match &cf{Some(c)=>c.movie as usize,None=>{
        // Without the .const, find the first movie character header.
        (16..d.len().saturating_sub(8)).step_by(4).find(|&o|be32(d,o).ok()==Some(9)&&be32(d,o+4).ok()==Some(SIGNATURE)).ok_or("no movie character")?
    }};
    if be32(d,movie_at)?!=9{return Err(format!("movie entry {movie_at:#x} is not a movie character"))}
    let mut st=Stats{instructions:0,streams:0,unknown_opcodes:0};
    let n=be32(d,movie_at+0x14)? as usize;let cp=be32(d,movie_at+0x18)? as usize;
    if n>100_000{return Err(format!("character count {n}"))}
    let mut chars=vec![];
    for i in 0..n{
        let q=be32(d,cp+4*i)? as usize;
        // Character 0 is the movie itself; decode it once as the root.
        chars.push(if q==0{Value::Null}else if q==movie_at{json!({"type":"Movie","root":true})}else{character(d,q,cf.as_ref(),&mut st).map_err(|e|format!("character {i}: {e}"))?});
    }
    let root=character(d,movie_at,cf.as_ref(),&mut st)?;
    Ok(Movie{json:json!({"version":version,"movie_offset":movie_at,"movie":root,"characters":chars,
        "constants":match &cf{Some(c)=>c.to_json()?,None=>Value::Null},
        "stats":{"action_streams":st.streams,"instructions":st.instructions,"unknown_opcodes":st.unknown_opcodes}}),stats:st})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn opcode_table_has_all_parser_operand_opcodes(){
        for op in [0x81u8,0x83,0x87,0x88,0x8b,0x8c,0x8e,0x8f,0x94,0x96,0x99,0x9b,0x9d,0x9f,0xa1,0xa2,0xa3,0xa4,0xa5,0xa6,0xa7,0xae,0xaf,0xb0,0xb1,0xb2,0xb3,0xb4,0xb5,0xb6,0xb7,0xb8]{
            assert!(opcode_name(op).is_some(),"{op:#x}");
        }
    }
    #[test]
    fn disassembles_immediates_and_alignment(){
        // PushByte -3, PushWord 0x1234, PushFloat 1.0, (align) BranchAlways +0, End
        let mut d=vec![0xb5,0xfd,0xb6,0x12,0x34,0xb4,0x3f,0x80,0,0,0x99,0,0,0,0,0,0,0,0,0];
        d.truncate(20);
        let mut st=Stats{instructions:0,streams:0,unknown_opcodes:0};
        let v=disassemble(&d,0,None,&mut st).unwrap();
        assert_eq!(v[0]["value"],-3);assert_eq!(v[1]["value"],0x1234);assert_eq!(v[2]["value"],1.0);
        assert_eq!(v[3]["op"],"BranchAlways");assert_eq!(v[3]["target"],16);assert_eq!(v[4]["op"],"End");
    }
}
