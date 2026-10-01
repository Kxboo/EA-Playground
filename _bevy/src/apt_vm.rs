//! ActionScript interpreter for APT movies.
//!
//! Instruction set: the Flash AVM1 opcodes plus EA's compact opcodes 0xA1-0xB8 (`AptActionInterpreter` jump table at
//! 0x8047b584, operand layouts from `_parseStream` at 0x80134064).  Operands that carry pointers/words are 4-byte aligned
//! after the opcode byte; byte/word/dword/float push operands are inline.  Branch offsets are relative to the end of the
//! operand.  Push/Define-dictionary operands are arrays of indices into the movie's `.const` table.
use crate::{apt::{Apt,Const},apt_player::{Cx,Kind as NodeKind,NodeId,Pending,Player,Xf,Movie}};
use std::{cell::RefCell,collections::{HashMap,HashSet},rc::Rc};

pub type O=Rc<RefCell<Obj>>;
#[derive(Clone)]
pub enum V{Undef,Null,Bool(bool),Num(f64),Str(Rc<str>),Obj(O)}

pub type NativeFn=fn(&mut Vm,&V,&[V])->Result<V,V>;

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
pub struct NodeRef{pub id:NodeId,pub generation:u32}

pub enum Kind{
    Plain,Array(Vec<V>),Func(Rc<Function>),Native(NativeFn),
    Clip(NodeRef),
    Color(NodeRef),
    Super{this:V,proto:Option<O>,ctor:V},
    Bool(bool),Number(f64),Str(Rc<str>),
}

#[derive(Default)]
pub struct Obj{pub props:HashMap<Rc<str>,V>,pub order:Vec<Rc<str>>,pub proto:Option<O>,pub kind:Option<Kind>,pub hidden:HashSet<Rc<str>>}
/// Property names are case-insensitive (the engine runs SWF-5 style semantics); keys are stored lowercased and the
/// original spelling is kept in `order` for enumeration.
pub fn lkey(name:&str)->std::borrow::Cow<'_,str>{
    if name.bytes().any(|b|b.is_ascii_uppercase()){std::borrow::Cow::Owned(name.to_ascii_lowercase())}else{std::borrow::Cow::Borrowed(name)}
}
impl Obj{
    pub fn kind(&self)->&Kind{self.kind.as_ref().unwrap_or(&Kind::Plain)}
    pub fn get_prop(&self,name:&str)->Option<&V>{self.props.get(&*lkey(name))}
    pub fn has_prop(&self,name:&str)->bool{self.props.contains_key(&*lkey(name))}
    pub fn remove_prop(&mut self,name:&str)->bool{
        let k=lkey(name).into_owned();
        let had=self.props.remove(k.as_str()).is_some();
        if had{self.order.retain(|o|o.to_ascii_lowercase()!=k);}
        had
    }
    pub fn set_own(&mut self,k:&str,v:V){
        let lk=lkey(k);
        if let Some(slot)=self.props.get_mut(&*lk){*slot=v;return}
        self.order.push(k.into());self.props.insert(lk.as_ref().into(),v);
    }
}

pub struct FuncProto{pub name:Rc<str>,pub params:Vec<(u32,Rc<str>)>,pub flags:u16,pub nregs:usize,pub body:usize,pub len:usize,pub v2:bool}
pub struct Function{pub proto:Rc<FuncProto>,pub scope:Rc<Scope>,pub pool:Rc<Vec<V>>,pub movie:Rc<Movie>}

pub struct Scope{pub obj:O,pub parent:Option<Rc<Scope>>,pub local:bool}

#[derive(Clone)]
pub enum I{
    Op(u8),
    Str(Rc<str>,u8),            // a1 push, a4 getvar, a5 getmember, a6 setvar, a7 setmember (second = opcode)
    Dict(u16,u8),               // a2/a3 push, ae getvar, af getmember, b0..b3 calls (second = opcode)
    Float(f64),
    Branch(u8,usize),
    Pool(Vec<u32>),Push(Vec<u32>),
    GotoFrame(u32),GotoFrame2(u32),GotoLabel(Rc<str>),SetTarget(Rc<str>),GetUrl(Rc<str>,Rc<str>),StoreReg(u32),
    With(usize),
    Func(Rc<FuncProto>),
    Try{catch_name:Rc<str>,catch_reg:Option<u8>,has_catch:bool,has_finally:bool,try_end:usize,catch_end:usize,fin_end:usize},
}

pub struct Block{pub instrs:Vec<I>,pub offs:Vec<usize>}

struct Rd<'a>{d:&'a [u8]}
impl Rd<'_>{
    fn u8(&self,o:usize)->Result<u8,String>{self.d.get(o).copied().ok_or_else(||format!("script read past end at {o:#x}"))}
    fn u16(&self,o:usize)->Result<u16,String>{Ok(((self.u8(o)? as u16)<<8)|self.u8(o+1)? as u16)}
    fn u32(&self,o:usize)->Result<u32,String>{Ok(((self.u16(o)? as u32)<<16)|self.u16(o+2)? as u32)}
    fn cstr(&self,o:usize)->Result<Rc<str>,String>{
        if o==0{return Ok("".into())}
        let s=self.d.get(o..).ok_or("script string out of range")?;
        let e=s.iter().position(|&c|c==0).ok_or("unterminated script string")?;
        Ok(s[..e].iter().map(|&c|c as char).collect::<String>().into())
    }
}

/// Decode `len` bytes (or up to the terminating End when `len` is None) of byte code starting at `start`.
pub fn decode(apt:&Apt,start:usize,len:Option<usize>)->Result<Block,String>{
    let r=Rd{d:&apt.data};
    let mut instrs:Vec<I>=vec![];let mut offs:HashMap<usize,usize>=HashMap::new();
    let mut fixups:Vec<(usize,usize)>=vec![]; // instr index, byte target
    let mut try_fix:Vec<(usize,usize,usize,usize)>=vec![];
    let mut with_fix:Vec<(usize,usize)>=vec![];
    let end=len.map(|l|start+l);
    let mut pos=start;
    let align=|p:usize|(p+3)&!3;
    loop{
        if let Some(e)=end{if pos>=e{break}}
        offs.insert(pos,instrs.len());
        let op=r.u8(pos)?;pos+=1;
        match op{
            0=>{instrs.push(I::Op(0));if end.is_none(){break}}
            0x81=>{pos=align(pos);instrs.push(I::GotoFrame(r.u32(pos)?));pos+=4}
            0x9f=>{pos=align(pos);instrs.push(I::GotoFrame2(r.u32(pos)?));pos+=4}
            0x8b=>{pos=align(pos);instrs.push(I::SetTarget(r.cstr(r.u32(pos)? as usize)?));pos+=4}
            0x8c=>{pos=align(pos);instrs.push(I::GotoLabel(r.cstr(r.u32(pos)? as usize)?));pos+=4}
            0x83=>{pos=align(pos);instrs.push(I::GetUrl(r.cstr(r.u32(pos)? as usize)?,r.cstr(r.u32(pos+4)? as usize)?));pos+=8}
            0x87=>{pos=align(pos);instrs.push(I::StoreReg(r.u32(pos)?));pos+=4}
            0x88|0x96=>{
                pos=align(pos);
                let (n,p)=(r.u32(pos)? as usize,r.u32(pos+4)? as usize);
                let mut v=Vec::with_capacity(n);
                for i in 0..n{v.push(r.u32(p+4*i)?);}
                instrs.push(if op==0x88{I::Pool(v)}else{I::Push(v)});pos+=8;
            }
            0x99|0x9d|0xb8=>{pos=align(pos);let off=r.u32(pos)? as i32;pos+=4;fixups.push((instrs.len(),(pos as i64+off as i64) as usize));instrs.push(I::Branch(op,0));}
            0x94=>{pos=align(pos);let size=r.u32(pos)? as usize;pos+=4;with_fix.push((instrs.len(),pos+size));instrs.push(I::With(0));}
            0x9b=>{
                pos=align(pos);
                let name=r.cstr(r.u32(pos)? as usize)?;let np=r.u32(pos+4)? as usize;let pp=r.u32(pos+8)? as usize;let size=r.u32(pos+12)? as usize;
                let mut params=vec![];
                for i in 0..np{params.push((0u32,r.cstr(r.u32(pp+4*i)? as usize)?));}
                instrs.push(I::Func(Rc::new(FuncProto{name,params,flags:0,nregs:0,body:pos+0x18,len:size,v2:false})));
                pos+=0x18+size;
            }
            0x8e=>{
                pos=align(pos);
                let name=r.cstr(r.u32(pos)? as usize)?;let np=r.u32(pos+4)? as usize;
                let nregs=r.u8(pos+8)? as usize;let flags=r.u16(pos+10)?;
                let pp=r.u32(pos+12)? as usize;let size=r.u32(pos+16)? as usize;
                let mut params=vec![];
                for i in 0..np{params.push((r.u32(pp+8*i)?,r.cstr(r.u32(pp+8*i+4)? as usize)?));}
                instrs.push(I::Func(Rc::new(FuncProto{name,params,flags,nregs,body:pos+0x1c,len:size,v2:true})));
                pos+=0x1c+size;
            }
            0x8f=>{
                pos=align(pos);
                let (ts,cs,fs)=(r.u32(pos)? as usize,r.u32(pos+4)? as usize,r.u32(pos+8)? as usize);
                let flags=r.u8(pos+12)?;let reg=r.u8(pos+15)?;
                let name=if flags&4==0{r.cstr(r.u32(pos+16)? as usize)?}else{"".into()};
                let body=pos+0x14;
                try_fix.push((instrs.len(),body+ts,body+ts+cs,body+ts+cs+fs));
                instrs.push(I::Try{catch_name:name,catch_reg:if flags&4!=0{Some(reg)}else{None},has_catch:flags&1!=0,has_finally:flags&2!=0,try_end:0,catch_end:0,fin_end:0});
                pos=body;
            }
            0xa1|0xa4|0xa5|0xa6|0xa7=>{pos=align(pos);instrs.push(I::Str(r.cstr(r.u32(pos)? as usize)?,op));pos+=4}
            0xa2|0xae|0xaf|0xb0|0xb1|0xb2|0xb3=>{instrs.push(I::Dict(r.u8(pos)? as u16,op));pos+=1}
            0xa3=>{instrs.push(I::Dict(r.u16(pos)?,op));pos+=2}
            0xb4=>{instrs.push(I::Float(f32::from_bits(r.u32(pos)?) as f64));pos+=4}
            0xb5=>{instrs.push(I::Float(r.u8(pos)? as i8 as f64));pos+=1}
            0xb6=>{instrs.push(I::Float(r.u16(pos)? as i16 as f64));pos+=2}
            0xb7=>{instrs.push(I::Float(r.u32(pos)? as i32 as f64));pos+=4}
            _=>instrs.push(I::Op(op)),
        }
    }
    offs.insert(pos,instrs.len());
    let target=|b:usize|->Result<usize,String>{offs.get(&b).copied().ok_or_else(||format!("branch to {b:#x} is not an instruction boundary"))};
    for (i,b) in fixups{if let I::Branch(_,t)=&mut instrs[i]{*t=target(b)?;}}
    for (i,e) in with_fix{if let I::With(t)=&mut instrs[i]{*t=target(e)?;}}
    for (i,a,b,c) in try_fix{if let I::Try{try_end,catch_end,fin_end,..}=&mut instrs[i]{*try_end=target(a)?;*catch_end=target(b)?;*fin_end=target(c)?;}}
    let mut ov=vec![0usize;instrs.len()+1];
    for (o,i) in &offs{if *i<ov.len(){ov[*i]=*o;}}
    Ok(Block{instrs,offs:ov})
}

pub struct Frame{
    pub movie:Rc<Movie>,pub block:Rc<Block>,pub stack:Vec<V>,pub regs:Vec<V>,pub scope:Rc<Scope>,pub this:V,
    pub target:Option<NodeId>,pub pool:Rc<Vec<V>>,pub locals:Option<O>,pub callee:Option<O>,pub super_v:V,pub pc:usize,
}
enum Flow{Next,Return(V)}

pub struct Interval{pub id:u32,pub func:V,pub this:V,pub args:Vec<V>,pub every:f64,pub due:f64}

pub struct Vm{
    pub trace_calls:Option<(u64,u64)>,pub tick_count:u64,pub trace_anim:bool,pub trace_print:bool,pub extern_obj:O,pub pointer:[(f32,f32);4],pub loadvars_proto:O,pub anims:Vec<crate::apt_anim::Anim>,
    pub player:Player,pub movies:HashMap<String,Rc<Movie>>,
    pub global:O,pub object_proto:O,pub function_proto:O,pub array_proto:O,pub string_proto:O,pub number_proto:O,pub boolean_proto:O,pub clip_proto:O,pub color_proto:O,
    pub clip_objs:HashMap<NodeId,(u32,O)>,
    pub classes:HashMap<String,V>,
    pub blocks:HashMap<(String,usize,usize),Rc<Block>>,
    pub intervals:Vec<Interval>,pub next_interval:u32,pub time_ms:f64,
    pub root:Option<NodeId>,
    pub log:Vec<String>,
    pub host_calls:Vec<(String,Vec<V>)>,
    pub loader:Option<fn(&str)->Result<Movie,String>>,
    pub depth:usize,
    pub rng:u64,
    pub focus_keys:Vec<(u32,bool)>,
    pub load_requests:Vec<(String,NodeRef)>,pub constructed:HashSet<(NodeId,u32)>,pub cur_key:(i32,i32),pub hover:Option<NodeRef>,pub pressed:Option<NodeRef>,
    pub key_listeners:Vec<V>,pub mouse_listeners:Vec<V>,pub seen_warn:HashSet<String>,
    pub fe:crate::fe_host::Fe,pub locale:Option<Rc<crate::locale::Locale>>,pub locale_trc:Option<Rc<crate::locale::Locale>>,
}

fn new_obj(proto:Option<O>,kind:Option<Kind>)->O{Rc::new(RefCell::new(Obj{proto,kind,..Default::default()}))}

impl V{
    pub fn str(s:&str)->V{V::Str(s.into())}
    pub fn is_undef(&self)->bool{matches!(self,V::Undef)}
    pub fn to_bool(&self)->bool{match self{V::Undef|V::Null=>false,V::Bool(b)=>*b,V::Num(n)=>*n!=0.&&!n.is_nan(),V::Str(s)=>{ s.parse::<f64>().map(|n|n!=0.).unwrap_or(false)},V::Obj(_)=>true}}
    pub fn to_num(&self)->f64{
        match self{
            V::Undef=>f64::NAN,V::Null=>0.,V::Bool(b)=>*b as i32 as f64,V::Num(n)=>*n,
            V::Str(s)=>{let t=s.trim();if t.is_empty(){0.}else if let Some(h)=t.strip_prefix("0x"){i64::from_str_radix(h,16).map(|x|x as f64).unwrap_or(f64::NAN)}else{t.parse::<f64>().unwrap_or(f64::NAN)}}
            V::Obj(o)=>match o.borrow().kind(){Kind::Number(n)=>*n,Kind::Bool(b)=>*b as i32 as f64,Kind::Str(s)=>V::Str(s.clone()).to_num(),_=>f64::NAN},
        }
    }
    pub fn obj(&self)->Option<&O>{if let V::Obj(o)=self{Some(o)}else{None}}
}
pub fn fmt_num(n:f64)->String{
    if n.is_nan(){return "NaN".into()}
    if n.is_infinite(){return if n>0.{"Infinity".into()}else{"-Infinity".into()}}
    if n==n.trunc()&&n.abs()<1e15{return format!("{}",n as i64)}
    let s=format!("{}",n);
    if s.contains('e'){s}else{s}
}

impl Vm{
    pub fn new()->Vm{
        let object_proto=new_obj(None,None);
        let mk=|o:&O|new_obj(Some(o.clone()),None);
        let function_proto=mk(&object_proto);let array_proto=mk(&object_proto);let string_proto=mk(&object_proto);let number_proto=mk(&object_proto);
        let boolean_proto=mk(&object_proto);let clip_proto=mk(&object_proto);let color_proto=mk(&object_proto);
        let global=mk(&object_proto);
        let mut vm=Vm{trace_calls:std::env::args().skip_while(|a|a!="--apt-trace-calls").nth(1).and_then(|s|s.split_once('-').and_then(|(a,b)|Some((a.parse().ok()?,b.parse().ok()?)))),tick_count:0,trace_print:std::env::args().any(|a|a=="--apt-fwprint"),trace_anim:std::env::args().any(|a|a=="--apt-trace-anim"),extern_obj:new_obj(None,None),pointer:[(747.,0.);4],loadvars_proto:new_obj(None,None),anims:vec![],player:Player::new(),movies:HashMap::new(),global,object_proto,function_proto,array_proto,string_proto,number_proto,boolean_proto,clip_proto,color_proto,
            clip_objs:HashMap::new(),classes:HashMap::new(),blocks:HashMap::new(),intervals:vec![],next_interval:1,time_ms:0.,root:None,log:vec![],host_calls:vec![],loader:None,depth:0,rng:0x2545F4914F6CDD1D,focus_keys:vec![],load_requests:vec![],constructed:HashSet::new(),cur_key:(0,0),hover:None,pressed:None,key_listeners:vec![],mouse_listeners:vec![],seen_warn:HashSet::new(),fe:crate::fe_host::Fe{first_screen:"Title".into(),..Default::default()},locale:None,locale_trc:None};
        crate::apt_lib::install(&mut vm);
        crate::apt_anim::install(&mut vm);
        vm
    }

    // ---------- objects ----------
    pub fn new_plain(&self)->O{new_obj(Some(self.object_proto.clone()),None)}
    pub fn new_array(&self,items:Vec<V>)->O{new_obj(Some(self.array_proto.clone()),Some(Kind::Array(items)))}
    pub fn new_native(&self,f:NativeFn)->V{V::Obj(new_obj(Some(self.function_proto.clone()),Some(Kind::Native(f))))}
    pub fn native(&mut self,target:&O,name:&str,f:NativeFn){let v=self.new_native(f);target.borrow_mut().set_own(name,v);}

    fn gen_of(&mut self,id:NodeId)->u32{if self.player.gens.len()<=id{self.player.gens.resize(id+1,0);}self.player.gens[id]}
    pub fn value_node_pub(&self,v:&V)->Option<NodeId>{ if let V::Obj(o)=v{ if let Kind::Clip(r)=o.borrow().kind(){ if self.node_alive(*r){return Some(r.id)} } } None }
    pub fn node_alive(&self,r:NodeRef)->bool{self.player.nodes.get(r.id).is_some_and(|n|n.alive)&&self.player.gens.get(r.id).copied().unwrap_or(0)==r.generation}

    /// Script object for a display node (created on first use).
    pub fn clip_obj(&mut self,id:NodeId)->O{
        let generation=self.gen_of(id);
        if let Some((g,o))=self.clip_objs.get(&id){if *g==generation{return o.clone()}}
        let proto=self.proto_for_node(id);
        let o=new_obj(Some(proto),Some(Kind::Clip(NodeRef{id,generation})));
        self.clip_objs.insert(id,(generation,o.clone()));
        o
    }
    fn proto_for_node(&self,id:NodeId)->O{
        let n=&self.player.nodes[id];
        if let Some(name)=self.export_name(&n.movie,self.player.char_of(id)){
            if let Some(V::Obj(c))=self.classes.get(&name){
                if let Some(V::Obj(p))=c.borrow().get_prop("prototype"){return p.clone()}
            }
        }
        self.clip_proto.clone()
    }
    pub fn export_name(&self,movie:&Rc<Movie>,char_id:u32)->Option<String>{
        movie.apt.exports.iter().find(|e|e.id==char_id).map(|e|e.name.clone())
    }
    /// Called when the player creates a sprite: attach the registered class and run its constructor.
    fn construct_node(&mut self,id:NodeId)->Result<(),V>{
        if !self.player.nodes[id].alive{return Ok(())}
        let generation=self.gen_of(id);
        if !self.constructed.insert((id,generation)){return Ok(())}
        let n=&self.player.nodes[id];
        let cls=self.export_name(&n.movie,self.player.char_of(id)).and_then(|name|self.classes.get(&name).cloned());
        if self.trace_print{ let nm=self.export_name(&self.player.nodes[id].movie,self.player.char_of(id)); let m=format!("construct node {id} {} char={} export={:?} class={}",self.clip_path(id),self.player.char_of(id),nm,cls.is_some());self.log.push(m); }
        let o=self.clip_obj(id);
        self.run_clip_actions(id,0x200);
        // `construct` clip actions (the Flash component parameters) run before the registered class's constructor.
        self.run_clip_actions(id,0x40000);
        if let Some(c)=cls{
            let proto=c.obj().and_then(|c|c.borrow().get_prop("prototype").cloned());
            if let Some(V::Obj(p))=&proto{o.borrow_mut().proto=Some(p.clone());}
            let home=proto.as_ref().and_then(|p|p.obj().cloned());
            self.call_function(&c,V::Obj(o),vec![],home)?;
        }
        self.run_clip_actions(id,0x1);
        Ok(())
    }

    // ---------- values ----------
    pub fn to_str(&mut self,v:&V)->Rc<str>{
        match v{
            V::Undef=>"undefined".into(),V::Null=>"null".into(),V::Bool(b)=>if *b{"true".into()}else{"false".into()},V::Num(n)=>fmt_num(*n).into(),V::Str(s)=>s.clone(),
            V::Obj(o)=>{
                let k=o.borrow();
                match k.kind(){
                    Kind::Str(s)=>return s.clone(),Kind::Number(n)=>return fmt_num(*n).into(),Kind::Bool(b)=>return b.to_string().into(),
                    Kind::Array(items)=>{let items=items.clone();drop(k);let mut parts=vec![];for i in &items{parts.push(self.to_str(i).to_string());}return parts.join(",").into()}
                    Kind::Func(_)|Kind::Native(_)=>return "[type Function]".into(),
                    Kind::Clip(r)=>{let r=*r;drop(k);return self.clip_path(r.id).into()}
                    _=>{}
                }
                drop(k);
                let ts=self.get_member(v,"toString");
                if matches!(ts,V::Obj(_)){
                    if let Ok(r)=self.call_function(&ts,v.clone(),vec![],None){if !matches!(r,V::Obj(_)){return self.to_str(&r)}}
                }
                "[object Object]".into()
            }
        }
    }
    pub fn to_prim_num(&mut self,v:&V)->f64{match v{V::Obj(o) if matches!(o.borrow().kind(),Kind::Plain|Kind::Clip(_))=>{let s=self.to_str(v);V::Str(s).to_num()},_=>v.to_num()}}

    pub fn clip_path(&self,id:NodeId)->String{
        let mut parts=vec![];let mut cur=Some(id);
        while let Some(c)=cur{
            let n=&self.player.nodes[c];
            if n.parent.is_none(){parts.push("_level0".to_string());break}
            parts.push(if n.name.is_empty(){format!("instance{c}")}else{n.name.clone()});
            cur=n.parent;
        }
        parts.reverse();parts.join(".")
    }

    pub fn warn(&mut self,msg:String){ if self.seen_warn.insert(msg.clone()){self.log.push(msg);} }

    /// Localised UI string for a `$KEY` text (`AIP::AllocateStringLocalized` -> `FEManager::GetLocalizedString`): a leading `$`
    /// is dropped and the key looked up; `$$` escapes a literal `$`.  A key missing from the tables is shown as the bare key
    /// (the executable `swprintf`s `%s` of it), so labels the shipped tables lack read e.g. `B_Boys`.
    pub fn locale_string(&self,key:&str)->String{
        if let Some(rest)=key.strip_prefix("$$"){return format!("${rest}")}
        let k=key.strip_prefix('$').unwrap_or(key);
        for l in [&self.locale,&self.locale_trc].into_iter().flatten(){
            if let Some(s)=l.get(k){return s.to_string()}
        }
        k.to_string()
    }

    pub fn type_of(&self,v:&V)->&'static str{
        match v{V::Undef=>"undefined",V::Null=>"null",V::Bool(_)=>"boolean",V::Num(_)=>"number",V::Str(_)=>"string",
            V::Obj(o)=>match o.borrow().kind(){Kind::Func(_)|Kind::Native(_)=>"function",Kind::Clip(r)=>{ if self.player.nodes.get(r.id).is_some_and(|n|matches!(n.kind,NodeKind::Sprite{..})){"movieclip"}else{"object"}},_=>"object"}}
    }

    pub fn strict_eq(a:&V,b:&V)->bool{
        match (a,b){(V::Undef,V::Undef)|(V::Null,V::Null)=>true,(V::Bool(x),V::Bool(y))=>x==y,(V::Num(x),V::Num(y))=>x==y,(V::Str(x),V::Str(y))=>x==y,(V::Obj(x),V::Obj(y))=>Rc::ptr_eq(x,y),_=>false}
    }
    pub fn loose_eq(&mut self,a:&V,b:&V)->bool{
        match (a,b){
            (V::Undef|V::Null,V::Undef|V::Null)=>true,
            (V::Undef|V::Null,_)|(_,V::Undef|V::Null)=>false,
            (V::Obj(x),V::Obj(y))=>Rc::ptr_eq(x,y),
            (V::Str(x),V::Str(y))=>x==y,
            (V::Obj(_),_)=>{let s=self.to_str(a);self.loose_eq(&V::Str(s),b)}
            (_,V::Obj(_))=>{let s=self.to_str(b);self.loose_eq(a,&V::Str(s))}
            _=>a.to_num()==b.to_num(),
        }
    }

    // ---------- property access ----------
    pub fn proto_of_value(&self,v:&V)->Option<O>{
        match v{V::Obj(o)=>o.borrow().proto.clone(),V::Str(_)=>Some(self.string_proto.clone()),V::Num(_)=>Some(self.number_proto.clone()),V::Bool(_)=>Some(self.boolean_proto.clone()),_=>None}
    }

    /// Plain property lookup along the prototype chain (own props first). Returns the holder for `super` binding.
    pub fn lookup(&self,o:&O,name:&str)->Option<(V,O)>{
        let mut cur=Some(o.clone());let mut guard=0;
        while let Some(c)=cur{
            if let Some(v)=c.borrow().get_prop(name){return Some((v.clone(),c.clone()))}
            if guard==0{ /* case-insensitive fallback only for builtin-looking names is handled by callers */ }
            guard+=1;if guard>64{break}
            cur=c.borrow().proto.clone();
        }
        None
    }

    pub fn get_member(&mut self,base:&V,name:&str)->V{self.get_member_h(base,name).0}
    /// Returns (value, holder object where it was found).
    pub fn get_member_h(&mut self,base:&V,name:&str)->(V,Option<O>){
        match base{
            V::Undef|V::Null=>(V::Undef,None),
            V::Str(s)=>{
                if name=="length"{return (V::Num(s.chars().count() as f64),None)}
                match self.lookup(&self.string_proto.clone(),name){Some((v,h))=>(v,Some(h)),None=>(V::Undef,None)}
            }
            V::Num(_)=>match self.lookup(&self.number_proto.clone(),name){Some((v,h))=>(v,Some(h)),None=>(V::Undef,None)},
            V::Bool(_)=>match self.lookup(&self.boolean_proto.clone(),name){Some((v,h))=>(v,Some(h)),None=>(V::Undef,None)},
            V::Obj(o)=>{
                // exotic kinds
                {
                    let b=o.borrow();
                    match b.kind(){
                        Kind::Array(items)=>{
                            if name=="length"{return (V::Num(items.len() as f64),None)}
                            if let Ok(i)=name.parse::<usize>(){return (items.get(i).cloned().unwrap_or(V::Undef),None)}
                        }
                        Kind::Str(s)=>{if name=="length"{return (V::Num(s.chars().count() as f64),None)}}
                        Kind::Super{this,proto,ctor}=>{
                            let _=this;
                            if name=="__constructor__"{return (ctor.clone(),None)}
                            if let Some(p)=proto{let p=p.clone();drop(b);return match self.lookup(&p,name){Some((v,h))=>(v,Some(h)),None=>(V::Undef,None)}}
                            return (V::Undef,None)
                        }
                        Kind::Clip(r)=>{let r=*r;drop(b);return self.clip_get(o,r,name)}
                        _=>{}
                    }
                }
                match self.lookup(o,name){Some((v,h))=>(v,Some(h)),None=>{
                    if name=="prototype"{
                        // Script functions get a lazily created prototype object.
                        let is_fn=matches!(o.borrow().kind(),Kind::Func(_));
                        if is_fn{let p=self.new_plain();p.borrow_mut().set_own("constructor",V::Obj(o.clone()));let v=V::Obj(p);o.borrow_mut().set_own("prototype",v.clone());return (v,Some(o.clone()))}
                    }
                    if name=="__proto__"{return (o.borrow().proto.clone().map(V::Obj).unwrap_or(V::Undef),None)}
                    (V::Undef,None)
                }}
            }
        }
    }

    pub fn set_member(&mut self,base:&V,name:&str,val:V){
        let V::Obj(o)=base else{return};
        if self.trace_print&&name.eq_ignore_ascii_case("fwprint")&&Rc::ptr_eq(o,&self.global){return}
        {
            let mut b=o.borrow_mut();
            match b.kind.as_mut(){
                Some(Kind::Array(items))=>{
                    if let Ok(i)=name.parse::<usize>(){if i>=items.len(){items.resize(i+1,V::Undef);}items[i]=val;return}
                    if name=="length"{let n=val.to_num().max(0.) as usize;items.resize(n,V::Undef);return}
                }
                Some(Kind::Clip(r))=>{let r=*r;drop(b);if self.clip_set(o,r,name,&val){return}o.borrow_mut().set_own(name,val);return}
                Some(Kind::Color(_))=>{}
                _=>{}
            }
            if name=="__proto__"{ if let V::Obj(p)=&val{b.proto=Some(p.clone());}return }
            b.set_own(name,val);
        }
    }

    // ---------- clip objects ----------
    pub fn find_child(&self,id:NodeId,name:&str)->Option<NodeId>{
        let NodeKind::Sprite{children,..}=&self.player.nodes[id].kind else{return None};
        let lname=name.to_lowercase();
        children.values().copied().find(|&c|{let n=&self.player.nodes[c];n.alive&&!n.name.is_empty()&&n.name.to_lowercase()==lname})
    }

    fn clip_get(&mut self,o:&O,r:NodeRef,name:&str)->(V,Option<O>){
        if !self.node_alive(r){return (V::Undef,None)}
        let id=r.id;
        if let Some(v)=self.clip_builtin(id,name){return (v,None)}
        if let Some(v)=o.borrow().get_prop(name){return (v.clone(),Some(o.clone()))}
        if let Some(c)=self.find_child(id,name){return (V::Obj(self.clip_obj(c)),None)}
        let proto=o.borrow().proto.clone();
        if let Some(p)=proto{if let Some((v,h))=self.lookup(&p,name){return (v,Some(h))}}
        // Engine extension objects (`AeoAnimation`, ...) are reachable as members of the root as well as of `_global`.
        if self.root==Some(id){ if let Some((v,h))=self.lookup(&self.global.clone(),name){return (v,Some(h))} }
        // `aaFoo` members are Hungarian "associative array" fields that the compiled classes use without ever
        // constructing them (e.g. Move.aaMotion); the engine hands out an empty object on first use.
        if name.len()>2&&name.starts_with("aa")&&name.as_bytes()[2].is_ascii_uppercase(){
            let n=V::Obj(self.new_plain());o.borrow_mut().set_own(name,n.clone());return (n,Some(o.clone()))
        }
        (V::Undef,None)
    }

    /// Laid-out size of a text field's current text (`TextField.textWidth/textHeight`).
    pub fn text_metrics(&self,id:NodeId)->Option<(f32,f32)>{
        let n=&self.player.nodes[id];
        let NodeKind::Text{id:def,text,..}=&n.kind else{return None};
        let (_m,_d,t,font)=crate::apt_text::resolve(&n.movie,*def)?;
        let shown=if text.starts_with('$')||text.starts_with("T_"){self.locale_string(text)}else{text.clone()};
        let face=crate::apt_text::face(&font,t.height)?;
        Some(crate::apt_text::metrics(&face,&shown,t.bounds,t.multiline,t.word_wrap,t.height))
    }

    pub fn clip_builtin(&mut self,id:NodeId,name:&str)->Option<V>{
        let n=&self.player.nodes[id];
        let lower=name.to_lowercase();
        Some(match lower.as_str(){
            "_x"=>V::Num(n.xf.0[4] as f64),"_y"=>V::Num(n.xf.0[5] as f64),
            "_xscale"=>V::Num((n.xf.0[0].hypot(n.xf.0[1])*100.) as f64),
            "_yscale"=>V::Num((n.xf.0[2].hypot(n.xf.0[3])*100.) as f64),
            "_alpha"=>V::Num((n.cx.mul[3]*100.) as f64),
            "_visible"=>V::Bool(n.visible),
            "_rotation"=>V::Num((n.xf.0[1].atan2(n.xf.0[0]).to_degrees()) as f64),
            "_name"=>V::str(&n.name),
            "_target"=>V::str(&format!("/{}",self.clip_path(id).replace("_level0.","").replace('.',"/"))),
            "_currentframe"=>match &n.kind{NodeKind::Sprite{frame,..}=>V::Num((*frame+1) as f64),_=>V::Num(1.)},
            "_totalframes"|"_framesloaded"=>match &n.kind{NodeKind::Sprite{frame_count,..}=>V::Num(*frame_count as f64),_=>V::Num(1.)},
            "_parent"=>match n.parent{Some(p)=>V::Obj(self.clip_obj(p)),None=>V::Undef},
            "_width"|"_height"=>{let (w,h)=self.node_size(id);V::Num(if lower=="_width"{w}else{h} as f64)}
            "_xmouse"|"_ymouse"=>V::Num(0.),
            "_url"=>V::str(""),
            "extern"=>V::Obj(self.extern_obj.clone()),
            "text"|"htmltext" if matches!(n.kind,NodeKind::Text{..})=>match &n.kind{NodeKind::Text{text,..}=>V::str(text),_=>V::Undef},
            "variable" if matches!(n.kind,NodeKind::Text{..})=>match &n.kind{NodeKind::Text{variable,..}=>V::str(variable),_=>V::Undef},
            "textwidth"|"textheight" if matches!(n.kind,NodeKind::Text{..})=>{
                let (w,h)=self.text_metrics(id).unwrap_or((0.,0.));
                V::Num(if lower=="textwidth"{w}else{h} as f64)
            }
            "_lockroot"|"enabled" if false=>V::Undef,
            _=>return None,
        })
    }

    fn clip_set(&mut self,_o:&O,r:NodeRef,name:&str,val:&V)->bool{
        if !self.node_alive(r){return true}
        let id=r.id;let lower=name.to_lowercase();
        let num=val.to_num();
        let n=&mut self.player.nodes[id];
        match lower.as_str(){
            "_x"=>{n.xf.0[4]=num as f32}
            "_y"=>{n.xf.0[5]=num as f32}
            "_xscale"=>{let s=n.xf.0[0].hypot(n.xf.0[1]);let ns=(num/100.) as f32;if s.abs()>1e-6{let k=ns/s;n.xf.0[0]*=k;n.xf.0[1]*=k;}else{n.xf.0[0]=ns;}}
            "_yscale"=>{let s=n.xf.0[2].hypot(n.xf.0[3]);let ns=(num/100.) as f32;if s.abs()>1e-6{let k=ns/s;n.xf.0[2]*=k;n.xf.0[3]*=k;}else{n.xf.0[3]=ns;}}
            "_alpha"=>{n.cx.mul[3]=(num/100.) as f32}
            "_visible"=>{n.visible=val.to_bool()&&!(val.to_num()==0.&&!matches!(val,V::Bool(_)))}
            "_rotation"=>{
                let sx=n.xf.0[0].hypot(n.xf.0[1]);let sy=n.xf.0[2].hypot(n.xf.0[3]);let a=(num as f32).to_radians();
                n.xf.0[0]=a.cos()*sx;n.xf.0[1]=a.sin()*sx;n.xf.0[2]=-a.sin()*sy;n.xf.0[3]=a.cos()*sy;
            }
            "_width"|"_height" if matches!(n.kind,NodeKind::Text{..})=>{
                // Resizing a text field changes its rectangle; the glyphs keep their size.
                let (movie,def)=match &n.kind{NodeKind::Text{id:d,..}=>(n.movie.clone(),*d),_=>unreachable!()};
                let base=match crate::apt_text::resolve(&movie,def){Some((_,_,t,_))=>t.bounds,None=>[0.;4]};
                if let NodeKind::Text{bounds,..}=&mut self.player.nodes[id].kind{
                    let mut b=bounds.unwrap_or(base);
                    if lower=="_width"{b[2]=b[0]+num as f32}else{b[3]=b[1]+num as f32}
                    *bounds=Some(b);
                }
            }
            "_width"|"_height"=>{
                let (w,h)=self.node_size(id);let n=&mut self.player.nodes[id];
                if lower=="_width"{if w>1e-6{let k=(num/w) as f32;n.xf.0[0]*=k;n.xf.0[1]*=k;}}else if h>1e-6{let k=(num/h) as f32;n.xf.0[2]*=k;n.xf.0[3]*=k;}
            }
            "_name"=>{let s=self.to_str(val).to_string();self.player.nodes[id].name=s;}
            "text"|"htmltext" if matches!(n.kind,NodeKind::Text{..})=>{let s=self.to_str(val).to_string();if let NodeKind::Text{text,..}=&mut self.player.nodes[id].kind{*text=s;}}
            _=>return false,
        }
        true
    }

    /// Axis-aligned size of a node's contents in its parent's space (shape bounds of all descendants).
    pub fn node_size(&self,id:NodeId)->(f64,f64){
        let mut lo=(f32::MAX,f32::MAX);let mut hi=(f32::MIN,f32::MIN);
        self.bounds(id,&Xf::ID,&mut lo,&mut hi);
        if lo.0>hi.0{(0.,0.)}else{((hi.0-lo.0) as f64,(hi.1-lo.1) as f64)}
    }
    pub fn bounds(&self,id:NodeId,xf:&Xf,lo:&mut (f32,f32),hi:&mut (f32,f32)){
        let n=&self.player.nodes[id];if !n.alive{return}
        let w=if id==usize::MAX{*xf}else{*xf};
        let m=n.xf.then(&w);
        match &n.kind{
            NodeKind::Sprite{children,..}=>for &c in children.values(){self.bounds(c,&m,lo,hi);},
            NodeKind::Shape{id:sid}=>{
                if let Some(crate::apt::Character::Shape{bounds,..})=n.movie.apt.character(*sid){
                    for (x,y) in [(bounds[0],bounds[1]),(bounds[2],bounds[1]),(bounds[0],bounds[3]),(bounds[2],bounds[3])]{
                        let (px,py)=m.apply(x,y);lo.0=lo.0.min(px);lo.1=lo.1.min(py);hi.0=hi.0.max(px);hi.1=hi.1.max(py);
                    }
                }
            }
            NodeKind::Text{id:tid,bounds:ovr,..}=>{
                if let Some(crate::apt::Character::Text(t))=n.movie.apt.character(*tid){
                    let tb=ovr.unwrap_or(t.bounds);
                    for (x,y) in [(tb[0],tb[1]),(tb[2],tb[1]),(tb[0],tb[3]),(tb[2],tb[3])]{
                        let (px,py)=m.apply(x,y);lo.0=lo.0.min(px);lo.1=lo.1.min(py);hi.0=hi.0.max(px);hi.1=hi.1.max(py);
                    }
                }
            }
        }
    }

    // ---------- blocks ----------
    pub fn block(&mut self,movie:&Rc<Movie>,start:usize,len:Option<usize>)->Result<Rc<Block>,V>{
        let key=(movie.key.clone(),start,len.unwrap_or(usize::MAX));
        if let Some(b)=self.blocks.get(&key){return Ok(b.clone())}
        let b=Rc::new(decode(&movie.apt,start,len).map_err(|e|V::str(&format!("decode error: {e}")))?);
        self.blocks.insert(key,b.clone());Ok(b)
    }

    fn const_value(&self,movie:&Movie,idx:u32,pool:&[V],regs:&[V])->V{
        match movie.apt.dict.get(idx as usize){
            Some(Const::Str(s))=>V::str(s),Some(Const::Undef)=>V::Undef,Some(Const::Reg(r))=>regs.get(*r as usize).cloned().unwrap_or(V::Undef),
            Some(Const::Bool(b))=>V::Bool(*b),Some(Const::Float(f))=>V::Num(*f as f64),Some(Const::Int(i))=>V::Num(*i as f64),
            Some(Const::Lookup(i))=>pool.get(*i as usize).cloned().unwrap_or(V::Undef),
            _=>V::Undef,
        }
    }

    // ---------- running code ----------
    /// Run a frame/init/clip-action stream with `this` = clip `target`.
    pub fn run_clip_code(&mut self,node:NodeId,movie:&Rc<Movie>,code:usize)->Result<(),V>{
        if !self.player.nodes[node].alive{return Ok(())}
        let block=self.block(movie,code,None)?;
        let this=V::Obj(self.clip_obj(node));
        let scope=Rc::new(Scope{obj:this.obj().unwrap().clone(),parent:None,local:false});
        let mut f=Frame{movie:movie.clone(),block,stack:vec![],regs:vec![V::Undef;4],scope,this,target:Some(node),pool:Rc::new(vec![]),locals:None,callee:None,super_v:V::Undef,pc:0};
        self.depth+=1;
        let r=self.exec(&mut f,0,usize::MAX);
        self.depth-=1;
        r.map(|_|())
    }

    pub fn call_function(&mut self,func:&V,this:V,args:Vec<V>,home:Option<O>)->Result<V,V>{
        let V::Obj(fo)=func else{return Ok(V::Undef)};
        if self.depth>200{return Err(V::str("stack overflow"))}
        enum K{Script(Rc<Function>),Native(NativeFn),Other}
        let k=match fo.borrow().kind(){Kind::Func(f)=>K::Script(f.clone()),Kind::Native(n)=>K::Native(*n),_=>K::Other};
        match k{
            K::Native(n)=>n(self,&this,&args),
            K::Other=>Ok(V::Undef),
            K::Script(f)=>{
                let p=f.proto.clone();
                let block=self.block(&f.movie,p.body,Some(p.len))?;
                let act=self.new_plain();
                let scope=Rc::new(Scope{obj:act.clone(),parent:Some(f.scope.clone()),local:true});
                let mut regs=vec![V::Undef;p.nregs.max(4)];
                let super_v=match &home{
                    Some(h)=>{let proto=h.borrow().proto.clone();let ctor=h.borrow().get_prop("__constructor__").cloned().unwrap_or(V::Undef);
                        V::Obj(new_obj(None,Some(Kind::Super{this:this.clone(),proto,ctor})))}
                    None=>V::Undef,
                };
                let args_obj=self.new_array(args.clone());
                {let mut a=args_obj.borrow_mut();a.set_own("callee",func.clone());}
                let mut next_reg=1usize;
                if p.v2{
                    let fl=p.flags;
                    let mut put=|v:V,regs:&mut Vec<V>,next:&mut usize|{if *next>=regs.len(){regs.resize(*next+1,V::Undef);}regs[*next]=v;*next+=1;};
                    if fl&0x01!=0{put(this.clone(),&mut regs,&mut next_reg);}
                    if fl&0x04!=0{put(V::Obj(args_obj.clone()),&mut regs,&mut next_reg);}
                    if fl&0x10!=0{put(super_v.clone(),&mut regs,&mut next_reg);}
                    if fl&0x40!=0{let r=self.root_value(&f.movie);put(r,&mut regs,&mut next_reg);}
                    if fl&0x80!=0{let v=self.get_member(&this,"_parent");put(v,&mut regs,&mut next_reg);}
                    if fl&0x100!=0{put(V::Obj(self.global.clone()),&mut regs,&mut next_reg);}
                }
                for (i,(reg,name))in p.params.iter().enumerate(){
                    let a=args.get(i).cloned().unwrap_or(V::Undef);
                    if *reg!=0{let r=*reg as usize;if r>=regs.len(){regs.resize(r+1,V::Undef);}regs[r]=a;}else{act.borrow_mut().set_own(name,a);}
                }
                if !p.v2||p.flags&0x08==0{ if !p.v2{act.borrow_mut().set_own("arguments",V::Obj(args_obj.clone()));} }
                let target=match &this{V::Obj(o)=>if let Kind::Clip(r)=o.borrow().kind(){Some(r.id)}else{None},_=>None};
                let mut fr=Frame{movie:f.movie.clone(),block,stack:vec![],regs,scope,this,target,pool:f.pool.clone(),locals:Some(act),callee:Some(fo.clone()),super_v,pc:0};
                self.depth+=1;
                let r=self.exec(&mut fr,0,usize::MAX);
                self.depth-=1;
                match r?{Flow::Return(v)=>Ok(v),Flow::Next=>Ok(V::Undef)}
            }
        }
    }

    pub fn root_value(&mut self,_m:&Rc<Movie>)->V{
        match self.root{Some(r)=>V::Obj(self.clip_obj(r)),None=>V::Undef}
    }

    pub fn call_method(&mut self,obj:&V,name:&str,args:Vec<V>)->Result<V,V>{
        let (f,home)=self.get_member_h(obj,name);
        if matches!(f,V::Obj(_)){self.call_function(&f,obj.clone(),args,home)}else{Ok(V::Undef)}
    }

    pub fn construct(&mut self,ctor:&V,args:Vec<V>)->Result<V,V>{
        let V::Obj(c)=ctor else{return Ok(V::Undef)};
        let proto=match self.get_member(ctor,"prototype"){V::Obj(p)=>p,_=>self.object_proto.clone()};
        let o=new_obj(Some(proto.clone()),None);
        let is_native=matches!(c.borrow().kind(),Kind::Native(_));
        let r=self.call_function(ctor,V::Obj(o.clone()),args,Some(proto))?;
        if is_native{if let V::Obj(_)=&r{return Ok(r)}}
        Ok(match r{V::Obj(ro) if !is_native=>V::Obj(ro),_=>V::Obj(o)})
    }

    // ---------- variables ----------
    pub fn get_variable(&mut self,f:&Frame,name:&str)->V{
        // scope chain
        let mut sc=Some(f.scope.clone());
        while let Some(s)=sc{
            let found=if s.local{s.obj.borrow().get_prop(name).cloned()}else{
                let o=V::Obj(s.obj.clone());
                let is_clip=matches!(s.obj.borrow().kind(),Kind::Clip(_));
                if is_clip{let v=self.get_member(&o,name);if v.is_undef(){None}else{Some(v)}}else{ self.lookup(&s.obj,name).map(|x|x.0) }
            };
            if let Some(v)=found{return v}
            sc=s.parent.clone();
        }
        match name{
            "this"=>return f.this.clone(),
            "_global"=>return V::Obj(self.global.clone()),
            "_root"|"_level0"=>return self.root_value(&f.movie),
            "undefined"=>return V::Undef,
            "NaN"=>return V::Num(f64::NAN),"Infinity"=>return V::Num(f64::INFINITY),
            "_parent"|"_x"|"_y"|"_alpha"|"_visible"|"_name"|"_width"|"_height"|"_currentframe"|"_totalframes"|"_xscale"|"_yscale"|"_rotation"=>{
                if let Some(t)=f.target{let o=V::Obj(self.clip_obj(t));return self.get_member(&o,name)}
            }
            _=>{}
        }
        if let Some((v,_))=self.lookup(&self.global.clone(),name){return v}
        // path form a.b.c
        if name.contains('.'){
            let mut parts=name.split('.');
            let mut cur=self.get_variable(f,parts.next().unwrap());
            for p in parts{cur=self.get_member(&cur,p);}
            return cur
        }
        V::Undef
    }

    pub fn set_variable(&mut self,f:&Frame,name:&str,val:V){
        let mut sc=Some(f.scope.clone());
        while let Some(s)=sc{
            let has=if s.local{s.obj.borrow().has_prop(name)}else{
                let o=V::Obj(s.obj.clone());
                let is_clip=matches!(s.obj.borrow().kind(),Kind::Clip(_));
                if is_clip{!self.get_member(&o,name).is_undef()||s.obj.borrow().has_prop(name)}else{self.lookup(&s.obj,name).is_some()}
            };
            if has{ if s.local{s.obj.borrow_mut().set_own(name,val)}else{self.set_member(&V::Obj(s.obj.clone()),name,val)};return }
            sc=s.parent.clone();
        }
        // not found: assign on the timeline clip (or the innermost non-local scope)
        let mut sc=Some(f.scope.clone());let mut last=None;
        while let Some(s)=sc{ if !s.local{last=Some(s.obj.clone());break} sc=s.parent.clone(); }
        match last{
            Some(o)=>self.set_member(&V::Obj(o),name,val),
            None=>{ if let Some(t)=f.target{let o=V::Obj(self.clip_obj(t));self.set_member(&o,name,val)} else {self.global.borrow_mut().set_own(name,val)} }
        }
    }

    pub fn resolve_target(&mut self,f:&Frame,v:&V)->Option<NodeId>{
        match v{
            V::Obj(o)=>if let Kind::Clip(r)=o.borrow().kind(){Some(r.id)}else{None},
            V::Str(s)=>{
                let path=s.trim();
                if path.is_empty(){return f.target}
                let (mut cur,rest)=if let Some(r)=path.strip_prefix('/'){(self.root?,r)}else{(f.target?,path)};
                for part in rest.split(['/','.']).filter(|p|!p.is_empty()){
                    match part{
                        ".."|"_parent"=>cur=self.player.nodes[cur].parent?,
                        "_root"|"_level0"=>cur=self.root?,
                        "this"=>{}
                        p=>cur=self.find_child(cur,p)?,
                    }
                }
                Some(cur)
            }
            _=>None,
        }
    }

    // ---------- the interpreter ----------
    fn pop(f:&mut Frame)->V{f.stack.pop().unwrap_or(V::Undef)}

    fn exec(&mut self,f:&mut Frame,from:usize,to:usize)->Result<Flow,V>{
        let block=f.block.clone();
        let end=to.min(block.instrs.len());
        let mut pc=from;
        while pc<end{
            f.pc=pc;let ins=&block.instrs[pc];pc+=1;
            match ins{
                I::Float(n)=>f.stack.push(V::Num(*n)),
                I::Str(s,op)=>{
                    let v=V::Str(s.clone());
                    match op{
                        0xa1=>f.stack.push(v),
                        0xa4=>{let r=self.get_variable(f,s);f.stack.push(r)}
                        0xa5=>{let o=Self::pop(f);let r=self.get_member(&o,s);f.stack.push(r)}
                        0xa6=>{let name=Self::pop(f);let n=self.to_str(&name);self.set_variable(f,&n,v)}
                        _=>{let name=Self::pop(f);let o=Self::pop(f);let n=self.to_str(&name);self.set_member(&o,&n,v)}
                    }
                }
                I::Dict(i,op)=>{
                    let v=f.pool.get(*i as usize).cloned().unwrap_or(V::Undef);
                    match op{
                        0xa2|0xa3=>f.stack.push(v),
                        0xae=>{let n=self.to_str(&v);let r=self.get_variable(f,&n);f.stack.push(r)}
                        0xaf=>{let o=Self::pop(f);let n=self.to_str(&v);let r=self.get_member(&o,&n);f.stack.push(r)}
                        0xb0|0xb1=>{f.stack.push(v);self.op_call_function(f)?;if *op==0xb0{Self::pop(f);}else{self.op_set_variable(f)}}
                        _=>{f.stack.push(v);self.op_call_method(f)?;if *op==0xb2{Self::pop(f);}else{self.op_set_variable(f)}}
                    }
                }
                I::Pool(items)=>{
                    let mut pool=vec![];
                    for &i in items{pool.push(self.const_value(&f.movie,i,&[],&f.regs));}
                    f.pool=Rc::new(pool);
                }
                I::Push(items)=>{
                    for &i in items{let v=self.const_value(&f.movie,i,&f.pool,&f.regs);f.stack.push(v);}
                }
                I::Branch(op,t)=>{
                    match op{
                        0x99=>{pc=*t;}
                        0x9d=>{let c=Self::pop(f);if c.to_bool(){pc=*t}}
                        _=>{let c=Self::pop(f);if !c.to_bool(){pc=*t}}
                    }
                }
                I::StoreReg(r)=>{let v=f.stack.last().cloned().unwrap_or(V::Undef);let r=*r as usize;if r>=f.regs.len(){f.regs.resize(r+1,V::Undef);}f.regs[r]=v}
                I::GotoFrame(n)=>{if let Some(t)=f.target{self.player.goto_frame(t,*n as usize);}}
                I::GotoFrame2(flags)=>{
                    let fv=Self::pop(f);
                    let (target,frame_v)=(f.target,fv);
                    let play=flags&1!=0;
                    self.goto_value(f,target,&frame_v,Some(play))?;
                }
                I::GotoLabel(l)=>{if let Some(t)=f.target{if let Some(fr)=self.player.label_frame(t,l){self.player.goto_frame(t,fr);}}}
                I::SetTarget(name)=>{
                    if name.is_empty(){f.target=match &f.this{V::Obj(o)=>if let Kind::Clip(r)=o.borrow().kind(){Some(r.id)}else{None},_=>None};}
                    else{let v=V::Str(name.clone());f.target=self.resolve_target(f,&v).or(f.target);}
                }
                I::GetUrl(url,target)=>{let t=V::Str(target.clone());self.get_url(f,url,&t,&t);}
                I::With(t)=>{
                    let o=Self::pop(f);
                    if let V::Obj(o)=o{
                        let saved=f.scope.clone();
                        f.scope=Rc::new(Scope{obj:o,parent:Some(saved.clone()),local:false});
                        let r=self.exec(f,pc,*t);
                        f.scope=saved;
                        match r?{Flow::Return(v)=>return Ok(Flow::Return(v)),Flow::Next=>{}}
                    }
                    pc=*t;
                }
                I::Func(p)=>{
                    let func=Rc::new(Function{proto:p.clone(),scope:f.scope.clone(),pool:f.pool.clone(),movie:f.movie.clone()});
                    let fo=new_obj(Some(self.function_proto.clone()),Some(Kind::Func(func)));
                    let v=V::Obj(fo);
                    if p.name.is_empty(){f.stack.push(v)}else{self.set_variable(f,&p.name.clone(),v)}
                }
                I::Try{catch_name,catch_reg,has_catch,has_finally,try_end,catch_end,fin_end}=>{
                    let body_start=pc;
                    let mut result=self.exec(f,body_start,*try_end);
                    if let (Err(e),true)=(&result,*has_catch){
                        let e=e.clone();
                        match catch_reg{Some(r)=>{let r=*r as usize;if r>=f.regs.len(){f.regs.resize(r+1,V::Undef);}f.regs[r]=e;}
                            None=>{ if let Some(a)=f.locals.clone(){a.borrow_mut().set_own(catch_name,e)}else{self.set_variable(f,catch_name,e)} }}
                        result=self.exec(f,*try_end,*catch_end);
                    }
                    if *has_finally{
                        let fr=self.exec(f,*catch_end,*fin_end);
                        if let Err(e)=fr{return Err(e)}
                        if let Ok(Flow::Return(v))=fr{return Ok(Flow::Return(v))}
                    }
                    match result?{Flow::Return(v)=>return Ok(Flow::Return(v)),Flow::Next=>{}}
                    pc=*fin_end;
                }
                I::Op(op)=>{
                    match self.op(f,*op)?{Some(Flow::Return(v))=>return Ok(Flow::Return(v)),_=>{}}
                    if *op==0{break}
                }
            }
        }
        Ok(Flow::Next)
    }

    fn goto_value(&mut self,f:&mut Frame,target:Option<NodeId>,fv:&V,play:Option<bool>)->Result<(),V>{
        let Some(t)=target else{return Ok(())};
        let frame=match fv{
            V::Str(s)=>{
                if let Some((path,l))=s.rsplit_once(':'){let _=path;self.player.label_frame(t,l)}
                else if let Some(fr)=self.player.label_frame(t,s){Some(fr)}
                else{s.parse::<f64>().ok().map(|n|(n as i64-1).max(0) as usize)}
            }
            V::Num(n)=>Some((*n as i64-1).max(0) as usize),
            _=>None,
        };
        let _=f;
        if let Some(fr)=frame{self.player.goto_frame(t,fr);}
        if let Some(p)=play{if let NodeKind::Sprite{playing,..}=&mut self.player.nodes[t].kind{*playing=p;}}
        Ok(())
    }

    /// GetURL / GetURL2: FSCommand strings go to the host, anything aimed at a clip loads that movie into it.
    fn get_url(&mut self,f:&mut Frame,url:&str,target_s:&V,target_v:&V){
        if let Some(cmd)=url.strip_prefix("FSCommand:"){
            let (name,params)=cmd.split_once('?').map(|(a,b)|(a.to_string(),b.to_string())).unwrap_or((cmd.to_string(),String::new()));
            crate::fe_host::game_call(self,&name,&params);return
        }
        let tv=if matches!(target_v,V::Obj(_)){target_v.clone()}else{target_s.clone()};
        if let Some(n)=self.resolve_target(f,&tv){ if !url.is_empty(){self.load_movie_into(n,url);return} }
        let t=self.to_str(target_s);
        self.host_calls.push(("geturl".into(),vec![V::str(url),V::Str(t)]));
    }

    fn op_set_variable(&mut self,f:&mut Frame){
        let val=Self::pop(f);let name=Self::pop(f);
        let n=self.to_str(&name);self.set_variable(f,&n,val);
    }

    fn pop_args(&mut self,f:&mut Frame)->Vec<V>{
        let n=Self::pop(f).to_num();let n=if n.is_nan()||n<0.{0}else{n as usize};
        let mut args=Vec::with_capacity(n);
        for _ in 0..n{args.push(Self::pop(f));}
        args
    }

    fn op_call_function(&mut self,f:&mut Frame)->Result<(),V>{
        let name=Self::pop(f);
        let args=self.pop_args(f);
        let fv=match &name{V::Str(s)=>{
            let v=self.get_variable(f,s);
            if v.is_undef()&&(s.contains('.')){ v } else {v}
        },_=>name.clone()};
        let this=match &fv{V::Obj(_)=>f.this.clone(),_=>V::Undef};
        // global functions get `this` = the global object-ish; script functions called plainly keep `this` undefined unless a timeline
        let this=if matches!(&name,V::Str(_)){ match f.target{Some(t)=>V::Obj(self.clip_obj(t)),None=>this} } else {this};
        let r=if matches!(fv,V::Obj(_)){self.call_function(&fv,this,args,None)?}else{
            if let V::Str(s)=&name{let m=format!("call of undefined function {s}");self.warn(m);}
            V::Undef};
        f.stack.push(r);
        Ok(())
    }

    fn op_call_method(&mut self,f:&mut Frame)->Result<(),V>{
        let name=Self::pop(f);let obj=Self::pop(f);
        let args=self.pop_args(f);
        let mname=match &name{V::Undef|V::Null=>String::new(),other=>self.to_str(other).to_string()};
        if let Some((a,b))=self.trace_calls{ if self.tick_count>=a&&self.tick_count<=b{
            let mut desc=vec![];for x in &args{desc.push(self.describe(x,0));}
            let od=self.describe(&obj,0);
            let m=format!("CALL {}.{}({})",od.chars().take(60).collect::<String>(),mname,desc.join(", ").chars().take(240).collect::<String>());self.log.push(m);
        }}
        let r=if mname.is_empty(){
            // calling the object itself (constructor / super(...) / function value)
            if let V::Obj(o)=&obj{
                let sup=if let Kind::Super{this,ctor,..}=o.borrow().kind(){Some((this.clone(),ctor.clone()))}else{None};
                if let Some((this,ctor))=sup{
                    // The superclass constructor runs with its own `super` bound to the next class up the chain.
                    let home=match self.get_member(&ctor,"prototype"){V::Obj(p)=>Some(p),_=>None};
                    self.call_function(&ctor,this,args,home)?
                } else { self.call_function(&obj,V::Undef,args,None)? }
            }else{V::Undef}
        }else{
            let (func,home)=self.get_member_h(&obj,&mname);
            let this=match &obj{V::Obj(o) => { if let Kind::Super{this,..}=o.borrow().kind(){this.clone()}else{obj.clone()} },_=>obj.clone()};
            if matches!(func,V::Obj(_)){self.call_function(&func,this,args,home)?}
            else if matches!(func,V::Null){V::Undef}else{ let d=match &obj{V::Obj(o)=>match o.borrow().kind(){Kind::Clip(r)=>self.clip_path(r.id),Kind::Func(_)=>"function".into(),_=>format!("object{{{}}}",o.borrow().order.iter().take(6).map(|k|k.to_string()).collect::<Vec<_>>().join(","))},v=>self.type_of(v).to_string()}; let m=format!("call of undefined method {mname:?} on {d} (found {}) at {}:{:x}",self.type_of(&func),f.movie.key,f.block.offs.get(f.pc).copied().unwrap_or(0));self.warn(m); V::Undef }
        };
        f.stack.push(r);
        Ok(())
    }

    fn op(&mut self,f:&mut Frame,op:u8)->Result<Option<Flow>,V>{
        match op{
            0=>{}
            0x04=>{if let Some(t)=f.target{if let NodeKind::Sprite{frame,frame_count,..}=&self.player.nodes[t].kind{let n=(*frame+1).min(frame_count.saturating_sub(1));self.player.goto_frame(t,n);}}}
            0x05=>{if let Some(t)=f.target{if let NodeKind::Sprite{frame,..}=&self.player.nodes[t].kind{let n=frame.saturating_sub(1);self.player.goto_frame(t,n);}}}
            0x06|0x07=>{if let Some(t)=f.target{if let NodeKind::Sprite{playing,..}=&mut self.player.nodes[t].kind{*playing=op==6;}}}
            0x08|0x09=>{}
            0x0a=>{let b=Self::pop(f).to_num();let a=Self::pop(f).to_num();f.stack.push(V::Num(a+b))}
            0x0b=>{let b=Self::pop(f).to_num();let a=Self::pop(f).to_num();f.stack.push(V::Num(a-b))}
            0x0c=>{let b=Self::pop(f).to_num();let a=Self::pop(f).to_num();f.stack.push(V::Num(a*b))}
            0x0d=>{let b=Self::pop(f).to_num();let a=Self::pop(f).to_num();f.stack.push(V::Num(a/b))}
            0x0e=>{let b=Self::pop(f).to_num();let a=Self::pop(f).to_num();f.stack.push(V::Bool(a==b))}
            0x0f|0x48=>{
                let b=Self::pop(f);let a=Self::pop(f);
                let r=match (&a,&b){(V::Str(x),V::Str(y))=>x<y,_=>{let (x,y)=(self.to_prim_num(&a),self.to_prim_num(&b));x<y}};
                f.stack.push(V::Bool(r))
            }
            0x67=>{
                let b=Self::pop(f);let a=Self::pop(f);
                let r=match (&a,&b){(V::Str(x),V::Str(y))=>x>y,_=>{let (x,y)=(self.to_prim_num(&a),self.to_prim_num(&b));x>y}};
                f.stack.push(V::Bool(r))
            }
            0x10=>{let b=Self::pop(f).to_bool();let a=Self::pop(f).to_bool();f.stack.push(V::Bool(a&&b))}
            0x11=>{let b=Self::pop(f).to_bool();let a=Self::pop(f).to_bool();f.stack.push(V::Bool(a||b))}
            0x12=>{let a=Self::pop(f).to_bool();f.stack.push(V::Bool(!a))}
            0x13=>{let b=Self::pop(f);let a=Self::pop(f);let (x,y)=(self.to_str(&a),self.to_str(&b));f.stack.push(V::Bool(x==y))}
            0x14=>{let a=Self::pop(f);let s=self.to_str(&a);f.stack.push(V::Num(s.chars().count() as f64))}
            0x15=>{
                let count=Self::pop(f).to_num();let idx=Self::pop(f).to_num();let s=Self::pop(f);let s=self.to_str(&s);
                let chars:Vec<char>=s.chars().collect();let start=((idx as i64-1).max(0) as usize).min(chars.len());let cnt=(count.max(0.) as usize).min(chars.len()-start);
                f.stack.push(V::str(&chars[start..start+cnt].iter().collect::<String>()))
            }
            0x17=>{Self::pop(f);}
            0x18=>{let a=Self::pop(f).to_num();f.stack.push(V::Num(if a.is_nan(){0.}else{a.trunc()}))}
            0x1c=>{let n=Self::pop(f);let n=self.to_str(&n);let r=self.get_variable(f,&n);f.stack.push(r)}
            0x1d=>self.op_set_variable(f),
            0x20=>{let t=Self::pop(f);f.target=self.resolve_target(f,&t).or(f.target)}
            0x21=>{let b=Self::pop(f);let a=Self::pop(f);let (x,y)=(self.to_str(&a),self.to_str(&b));f.stack.push(V::str(&format!("{x}{y}")))}
            0x22=>{
                let idx=Self::pop(f).to_num() as usize;let t=Self::pop(f);
                let names=["_x","_y","_xscale","_yscale","_currentframe","_totalframes","_alpha","_visible","_width","_height","_rotation","_target","_framesloaded","_name"];
                let r=match (self.resolve_target(f,&t),names.get(idx)){(Some(n),Some(nm))=>self.clip_builtin(n,nm).unwrap_or(V::Undef),_=>V::Undef};
                f.stack.push(r)
            }
            0x23=>{
                let val=Self::pop(f);let idx=Self::pop(f).to_num() as usize;let t=Self::pop(f);
                let names=["_x","_y","_xscale","_yscale","_currentframe","_totalframes","_alpha","_visible","_width","_height","_rotation","_target","_framesloaded","_name"];
                if let (Some(n),Some(nm))=(self.resolve_target(f,&t),names.get(idx)){let o=V::Obj(self.clip_obj(n));self.set_member(&o,nm,val);}
            }
            0x24=>{ // CloneSprite: [target, new name, depth]
                let depth=Self::pop(f).to_num() as i32;let name=Self::pop(f);let name=self.to_str(&name);let t=Self::pop(f);
                if let Some(n)=self.resolve_target(f,&t){self.duplicate(n,&name,depth);}
            }
            0x25=>{let t=Self::pop(f);if let Some(n)=self.resolve_target(f,&t){self.remove_clip(n);}}
            0x26=>{let m=Self::pop(f);let s=self.to_str(&m);self.log.push(s.to_string());}
            0x27|0x28=>{if op==0x27{ Self::pop(f);Self::pop(f);Self::pop(f);let c=Self::pop(f).to_bool();if c{Self::pop(f);Self::pop(f);Self::pop(f);Self::pop(f);} } }
            0x29=>{let b=Self::pop(f);let a=Self::pop(f);let (x,y)=(self.to_str(&a),self.to_str(&b));f.stack.push(V::Bool(x<y))}
            0x2a=>{let v=Self::pop(f);return Err(v)}
            0x2b=>{ // CastOp: [constructor, object] (the class is pushed first, then the value)
                let o=Self::pop(f);let c=Self::pop(f);
                let ok=self.instance_of(&o,&c);f.stack.push(if ok{o}else{V::Null})
            }
            0x2c=>{ // ImplementsOp: [..interfaces, count, class]
                let _class=Self::pop(f);let n=Self::pop(f).to_num() as usize;for _ in 0..n{Self::pop(f);}
            }
            0x30=>{let m=Self::pop(f).to_num();self.rng^=self.rng<<13;self.rng^=self.rng>>7;self.rng^=self.rng<<17;f.stack.push(V::Num(if m>=1.{(self.rng%(m as u64)) as f64}else{0.}))}
            0x31=>{let a=Self::pop(f);let s=self.to_str(&a);f.stack.push(V::Num(s.chars().count() as f64))}
            0x32|0x36=>{let a=Self::pop(f);let s=self.to_str(&a);f.stack.push(V::Num(s.chars().next().map(|c|c as u32 as f64).unwrap_or(0.)))}
            0x33|0x37=>{let a=Self::pop(f).to_num();f.stack.push(V::str(&char::from_u32(a as u32).map(|c|c.to_string()).unwrap_or_default()))}
            0x34=>{f.stack.push(V::Num(self.time_ms.floor()))}
            0x35=>{
                let count=Self::pop(f).to_num();let idx=Self::pop(f).to_num();let s=Self::pop(f);let s=self.to_str(&s);
                let chars:Vec<char>=s.chars().collect();let start=((idx as i64-1).max(0) as usize).min(chars.len());let cnt=(count.max(0.) as usize).min(chars.len()-start);
                f.stack.push(V::str(&chars[start..start+cnt].iter().collect::<String>()))
            }
            0x3a=>{let n=Self::pop(f);let o=Self::pop(f);let n=self.to_str(&n);
                let ok=if let V::Obj(ob)=&o{ob.borrow_mut().remove_prop(&n)}else{false};f.stack.push(V::Bool(ok))}
            0x3b=>{let n=Self::pop(f);let n=self.to_str(&n);
                let mut sc=Some(f.scope.clone());let mut ok=false;
                while let Some(s)=sc{ if s.obj.borrow_mut().remove_prop(&n){ok=true;break} sc=s.parent.clone(); }
                f.stack.push(V::Bool(ok))}
            0x3c=>{let v=Self::pop(f);let n=Self::pop(f);let n=self.to_str(&n);
                match f.locals.clone(){Some(a)=>a.borrow_mut().set_own(&n,v),None=>self.set_variable(f,&n,v)}}
            0x41=>{let n=Self::pop(f);let n=self.to_str(&n);
                match f.locals.clone(){Some(a)=>{if !a.borrow().has_prop(&n){a.borrow_mut().set_own(&n,V::Undef)}},None=>{}}}
            0x3d=>self.op_call_function(f)?,
            0x3e=>{let v=Self::pop(f);return Ok(Some(Flow::Return(v)))}
            0x3f=>{let b=Self::pop(f).to_num();let a=Self::pop(f).to_num();f.stack.push(V::Num(a%b))}
            0x40=>{ // NewObject: [args.., numargs, classname]
                let name=Self::pop(f);let args=self.pop_args(f);
                let n=self.to_str(&name);let ctor=self.get_variable(f,&n);
                let r=self.construct(&ctor,args)?;f.stack.push(r)
            }
            0x42=>{let args=self.pop_args(f);let a=self.new_array(args);f.stack.push(V::Obj(a))}
            0x43=>{
                let n=Self::pop(f).to_num() as usize;let o=self.new_plain();
                for _ in 0..n{let v=Self::pop(f);let k=Self::pop(f);let k=self.to_str(&k);o.borrow_mut().set_own(&k,v);}
                f.stack.push(V::Obj(o))
            }
            0x44=>{let v=Self::pop(f);f.stack.push(V::str(self.type_of(&v)))}
            0x45=>{let v=Self::pop(f);let r=match self.resolve_target(f,&v){Some(n)=>V::str(&self.clip_path(n)),None=>V::Undef};f.stack.push(r)}
            0x46=>{ // Enumerate (by variable name)
                let n=Self::pop(f);let n=self.to_str(&n);let v=self.get_variable(f,&n);f.stack.push(V::Null);self.push_keys(f,&v)
            }
            0x55=>{let v=Self::pop(f);f.stack.push(V::Null);self.push_keys(f,&v)}
            0x47=>{
                let b=Self::pop(f);let a=Self::pop(f);
                let strish=|v:&V|matches!(v,V::Str(_))||matches!(v,V::Obj(o) if !matches!(o.borrow().kind(),Kind::Number(_)|Kind::Bool(_)));
                let r=if strish(&a)||strish(&b){let (x,y)=(self.to_str(&a),self.to_str(&b));V::str(&format!("{x}{y}"))}else{V::Num(a.to_num()+b.to_num())};
                f.stack.push(r)
            }
            0x49=>{let b=Self::pop(f);let a=Self::pop(f);let r=self.loose_eq(&a,&b);f.stack.push(V::Bool(r))}
            0x4a=>{let a=Self::pop(f);let n=self.to_prim_num(&a);f.stack.push(V::Num(n))}
            0x4b=>{let a=Self::pop(f);let s=self.to_str(&a);f.stack.push(V::Str(s))}
            0x4c=>{let v=f.stack.last().cloned().unwrap_or(V::Undef);f.stack.push(v)}
            0x4d=>{let n=f.stack.len();if n>=2{f.stack.swap(n-1,n-2)}}
            0x4e=>{let n=Self::pop(f);let o=Self::pop(f);let n=self.to_str(&n);let r=self.get_member(&o,&n);f.stack.push(r)}
            0x4f=>{let v=Self::pop(f);let n=Self::pop(f);let o=Self::pop(f);let n=self.to_str(&n);self.set_member(&o,&n,v)}
            0x50=>{let a=Self::pop(f).to_num();f.stack.push(V::Num(a+1.))}
            0x51=>{let a=Self::pop(f).to_num();f.stack.push(V::Num(a-1.))}
            0x52=>self.op_call_method(f)?,
            0x53=>{ // NewMethod: [args.., numargs, object, methodname]
                let name=Self::pop(f);let obj=Self::pop(f);let args=self.pop_args(f);
                let n=self.to_str(&name);
                let ctor=if n.is_empty(){obj}else{self.get_member(&obj,&n)};
                let r=self.construct(&ctor,args)?;f.stack.push(r)
            }
            0x54=>{let c=Self::pop(f);let o=Self::pop(f);let r=self.instance_of(&o,&c);f.stack.push(V::Bool(r))}
            0x56|0x70=>f.stack.push(f.this.clone()),
            0x58|0x71=>f.stack.push(V::Obj(self.global.clone())),
            0x59|0x5a=>f.stack.push(V::Num((op-0x59) as f64)),
            0x5b=>{self.op_call_function(f)?;Self::pop(f);}
            0x5c=>{self.op_call_function(f)?;self.op_set_variable(f);}
            0x5d=>{self.op_call_method(f)?;Self::pop(f);}
            0x5e=>{self.op_call_method(f)?;self.op_set_variable(f);}
            0x60=>{let b=Self::pop(f).to_num() as i32;let a=Self::pop(f).to_num() as i32;f.stack.push(V::Num((a&b) as f64))}
            0x61=>{let b=Self::pop(f).to_num() as i32;let a=Self::pop(f).to_num() as i32;f.stack.push(V::Num((a|b) as f64))}
            0x62=>{let b=Self::pop(f).to_num() as i32;let a=Self::pop(f).to_num() as i32;f.stack.push(V::Num((a^b) as f64))}
            0x63=>{let b=Self::pop(f).to_num() as i32;let a=Self::pop(f).to_num() as i32;f.stack.push(V::Num(a.wrapping_shl(b as u32&31) as f64))}
            0x64=>{let b=Self::pop(f).to_num() as i32;let a=Self::pop(f).to_num() as i32;f.stack.push(V::Num((a>>(b&31)) as f64))}
            0x65=>{let b=Self::pop(f).to_num() as i32;let a=Self::pop(f).to_num() as u32;f.stack.push(V::Num((a>>(b&31)) as f64))}
            0x66=>{let b=Self::pop(f);let a=Self::pop(f);f.stack.push(V::Bool(Self::strict_eq(&a,&b)))}
            0x69=>{ // Extends: [subclass, superclass]
                let sup=Self::pop(f);let sub=Self::pop(f);
                let sp=self.get_member(&sup,"prototype");
                let proto=self.new_plain();
                if let V::Obj(sp)=&sp{proto.borrow_mut().proto=Some(sp.clone());}
                proto.borrow_mut().set_own("__constructor__",sup.clone());
                proto.borrow_mut().set_own("constructor",sup.clone());
                if let V::Obj(s)=&sub{s.borrow_mut().set_own("prototype",V::Obj(proto));}
            }
            0x72=>{f.stack.push(V::Num(0.));self.op_set_variable(f)}
            0x73=>f.stack.push(V::Bool(true)),0x74=>f.stack.push(V::Bool(false)),0x75=>f.stack.push(V::Null),0x76=>f.stack.push(V::Undef),
            0x9a=>{ // GetUrl2: [url, target]
                let target=Self::pop(f);let url=Self::pop(f);
                let (u,t)=(self.to_str(&url),self.to_str(&target));
                self.get_url(f,&u,&V::Str(t),&target);
            }
            0x9e=>{ // CallFrame
                let fv=Self::pop(f);let t=f.target;let _=(fv,t);
            }
            _=>{ let m=format!("unimplemented opcode {op:#04x}");self.warn(m); }
        }
        Ok(None)
    }

    fn push_keys(&mut self,f:&mut Frame,v:&V){
        if let V::Obj(o)=v{
            let builtin=[self.object_proto.clone(),self.function_proto.clone(),self.array_proto.clone(),self.string_proto.clone(),self.number_proto.clone(),self.boolean_proto.clone(),self.clip_proto.clone(),self.color_proto.clone()];
            let mut names:Vec<String>=vec![];
            let mut cur=Some(o.clone());
            while let Some(c)=cur{
                if builtin.iter().any(|b|Rc::ptr_eq(b,&c)){break}
                let b=c.borrow();
                if let Kind::Array(items)=b.kind(){for i in 0..items.len(){if !matches!(items[i],V::Undef){names.push(i.to_string());}}}
                if let Kind::Clip(r)=b.kind(){
                    if let NodeKind::Sprite{children,..}=&self.player.nodes[r.id].kind{for &ch in children.values(){let n=&self.player.nodes[ch];if !n.name.is_empty(){names.push(n.name.clone());}}}
                }
                for k in &b.order{
                    let lk=lkey(k);
                    if b.hidden.contains(&*lk)||&*lk=="constructor"||&*lk=="__constructor__"||&*lk=="__proto__"{continue}
                    if !names.iter().any(|n|n.eq_ignore_ascii_case(k)){names.push(k.to_string());}
                }
                let next=b.proto.clone();drop(b);cur=next;
            }
            for n in names{f.stack.push(V::str(&n));}
        }
    }

    pub fn instance_of(&mut self,o:&V,c:&V)->bool{
        let V::Obj(_)=o else{return false};
        let proto=match self.get_member(c,"prototype"){V::Obj(p)=>p,_=>return false};
        let mut cur=self.proto_of_value(o);let mut g=0;
        while let Some(p)=cur{ if Rc::ptr_eq(&p,&proto){return true} g+=1;if g>64{break} cur=p.borrow().proto.clone(); }
        false
    }

    // ---------- clip operations used by natives ----------
    pub fn remove_clip(&mut self,id:NodeId){
        self.player.destroy(id);
        self.clip_objs.remove(&id);
    }
    fn destroy_tree_gens(&mut self,id:NodeId){let _=id;}

    pub fn duplicate(&mut self,src:NodeId,name:&str,depth:i32)->Option<NodeId>{
        let parent=self.player.nodes[src].parent?;
        let movie=self.player.nodes[src].movie.clone();
        let cid=self.player.char_of(src);
        let n=self.player.spawn_character(&movie,cid,parent,depth,false)?;
        self.player.nodes[n].name=name.to_string();
        self.player.nodes[n].xf=self.player.nodes[src].xf;self.player.nodes[n].cx=self.player.nodes[src].cx;
        Some(n)
    }

    /// Debug helper: describe the value at a dotted path (`_global.fw.X.prototype`, `_root.main`, ...).
    pub fn dump_tree(&self,id:NodeId,depth:usize,out:&mut String){
        let n=&self.player.nodes[id];if !n.alive{return}
        let kind=match &n.kind{NodeKind::Sprite{frame,frame_count,playing,char_id,..}=>format!("sprite#{char_id} f{}/{}{}",frame,frame_count,if *playing{""}else{" stop"}),NodeKind::Shape{id}=>format!("shape#{id}"),NodeKind::Text{text,..}=>format!("text {text:?}")};
        out.push_str(&format!("{}{} [{}] {} x={:.1} y={:.1} a={:.2}{}{}
","  ".repeat(depth),if n.name.is_empty(){"-"}else{&n.name},n.movie.key,kind,n.xf.0[4],n.xf.0[5],n.cx.mul[3],if n.visible{""}else{" HIDDEN"},
            if n.cx.mul[..3]!=[1.;3]||n.cx.add!=[0.;4]{format!(" cx mul={:?} add={:?}",&n.cx.mul[..3],n.cx.add)}else{String::new()}));
        if let NodeKind::Sprite{children,..}=&n.kind{ if depth<9{ for &c in children.values(){ if !matches!(self.player.nodes[c].kind,NodeKind::Shape{..}) || depth<3 {self.dump_tree(c,depth+1,out);} } } }
    }
    pub fn inspect(&mut self,path:&str)->String{
        if path=="classes"{let mut k:Vec<String>=self.classes.keys().cloned().collect();k.sort();return k.join(", ")}
        if path=="calls"{return self.fe.calls.join(" | ")}
        if path=="tree"{let mut s=String::new();if let Some(r)=self.root{self.dump_tree(r,0,&mut s);}return s}
        let mut parts=path.split('.');
        let first=parts.next().unwrap_or("");
        let mut cur=match first{
            "_global"=>V::Obj(self.global.clone()),
            "_root"|"_level0"=>match self.root{Some(r)=>V::Obj(self.clip_obj(r)),None=>V::Undef},
            other=>{let g=V::Obj(self.global.clone());self.get_member(&g,other)}
        };
        for p in parts{cur=self.get_member(&cur,p);}
        self.describe(&cur,2)
    }
    pub fn describe(&mut self,v:&V,depth:usize)->String{
        match v{
            V::Obj(o)=>{
                let kind=match o.borrow().kind(){Kind::Plain=>"obj".to_string(),Kind::Array(a)=>format!("array[{}]",a.len()),Kind::Func(_)=>"function".into(),Kind::Native(_)=>"native".into(),Kind::Clip(r)=>format!("clip#{}",r.id),_=>"special".into()};
                let keys:Vec<Rc<str>>=o.borrow().order.clone();
                let mut out=format!("{kind}{{");
                for k in keys.iter().take(60){
                    let val=o.borrow().get_prop(k).cloned().unwrap_or(V::Undef);
                    let d=if depth>0&&matches!(val,V::Obj(_)){self.describe(&val,depth-1)}else{match &val{V::Obj(_)=>"[obj]".into(),V::Str(s)=>format!("{s:?}"),V::Num(n)=>fmt_num(*n),V::Bool(b)=>b.to_string(),V::Null=>"null".into(),V::Undef=>"undefined".into()}};
                    out.push_str(&format!("{k}:{d}, "));
                }
                if keys.len()>60{out.push_str("...");}
                let proto=o.borrow().proto.clone();
                if let Some(p)=proto{let pk:Vec<String>=p.borrow().order.iter().take(40).map(|k|k.to_string()).collect();out.push_str(&format!(" | proto[{}]",pk.join(",")));}
                out.push('}');out
            }
            V::Str(s)=>format!("{s:?}"),V::Num(n)=>fmt_num(*n),V::Bool(b)=>b.to_string(),V::Null=>"null".into(),V::Undef=>"undefined".into(),
        }
    }

    // ---------- clip events ----------
    /// Run the `onClipEvent` handlers (PlaceObject clip actions) of `node` for event `flag` (SWF ClipEventFlags).
    pub fn run_clip_actions(&mut self,node:NodeId,flag:u32){
        if !self.player.nodes.get(node).is_some_and(|n|n.alive){return}
        let acts:Vec<crate::apt::ClipAction>=self.player.nodes[node].clip_actions.iter().filter(|a|a.flags&flag!=0).cloned().collect();
        if acts.is_empty(){return}
        let Some(movie)=self.player.nodes[node].actions_movie.clone() else{return};
        for a in acts{
            if let Err(e)=self.run_clip_code(node,&movie,a.code as usize){let s=self.to_str(&e);self.log.push(format!("clip action {flag:#x}: {s}"));}
        }
    }
    fn for_each_clip(&self,id:NodeId,out:&mut Vec<NodeId>){
        let n=&self.player.nodes[id];if !n.alive{return}
        out.push(id);
        if let NodeKind::Sprite{children,..}=&n.kind{for &c in children.values(){self.for_each_clip(c,out);}}
    }
    /// A keyboard/pad event from the host: `Key.getCode()`/`getController()` report it to KeyDown/KeyUp handlers.
    pub fn key_event(&mut self,code:i32,controller:i32,down:bool){
        self.cur_key=(code,controller);
        self.focus_keys.retain(|k|k.0!=code as u32);
        if down{self.focus_keys.push((code as u32,true));}
        let mut all=vec![];if let Some(r)=self.root{self.for_each_clip(r,&mut all);}
        for id in all{ self.run_clip_actions(id,if down{0x40}else{0x80}); }
        let ls=self.key_listeners.clone();
        for l in ls{
            let f=self.get_member(&l,if down{"onKeyDown"}else{"onKeyUp"});
            if matches!(f,V::Obj(_)){ if let Err(e)=self.call_function(&f,l.clone(),vec![],None){let s=self.to_str(&e);self.log.push(format!("key listener: {s}"));} }
        }
        self.process_pending();
    }

    // ---------- host -> script calls ----------
    /// Call a function a screen exposed to the engine with `RegisterAsExposed` (case-insensitive name).
    pub fn call_exposed(&mut self,name:&str,args:Vec<V>)->Option<V>{
        // `GameCommManager.RegisterAsExposed` stores a bound function under the exposed name on `_root`
        // (and the descriptor in `m_exposedFunctions[depth]`); the engine calls it by name.
        let root=self.resolve_path_object("_root");
        let f=self.get_member(&root,name);
        if matches!(f,V::Obj(_)){
            return match self.call_function(&f,root,args,None){Ok(v)=>Some(v),Err(e)=>{let s=self.to_str(&e);self.log.push(format!("exposed {name}: {s}"));None}};
        }
        if self.trace_anim{self.log.push(format!("exposed {name}: not registered"));}
        // fall back to a method of the global controller
        let gc=self.resolve_path_object("_root.main.m_gc");
        let f=self.get_member(&gc,name);
        if matches!(f,V::Obj(_)){ return self.call_function(&f,gc,args,None).ok() }
        None
    }

    /// Engine -> frontend button event (`code` per fw.datatypes.KeyCode; releases are code + 1000).
    pub fn input_key(&mut self,code:i32,controller:i32){
        self.call_exposed("OnInputReceived",vec![V::Num(code as f64),V::Num(controller as f64)]);
        self.process_pending();
    }

    // ---------- pointer ----------
    fn handler_clip_at(&mut self,id:NodeId,x:f32,y:f32,best:&mut Option<NodeId>){
        let n=&self.player.nodes[id];if !n.alive||!n.visible{return}
        let kids:Vec<NodeId>=if let NodeKind::Sprite{children,..}=&n.kind{children.values().copied().collect()}else{vec![]};
        if matches!(n.kind,NodeKind::Sprite{..}){
            let o=self.clip_obj(id);let ov=V::Obj(o);
            let has=["onPress","onRelease","onRollOver","onRollOut","onReleaseOutside"].iter().any(|h|matches!(self.get_member(&ov,h),V::Obj(_)));
            if has{
                if let Some(b)=self.world_bounds(id){ if x>=b[0]&&x<=b[2]&&y>=b[1]&&y<=b[3]{*best=Some(id);} }
            }
        }
        for k in kids{self.handler_clip_at(k,x,y,best);}
    }
    fn fire_clip_event(&mut self,id:NodeId,name:&str){
        let o=V::Obj(self.clip_obj(id));
        let f=self.get_member(&o,name);
        if matches!(f,V::Obj(_)){ if let Err(e)=self.call_function(&f,o,vec![],None){let s=self.to_str(&e);self.log.push(format!("{name}: {s}"));} }
    }
    /// Publish the pointer state the way the engine does: `extern._xmouse/_ymouse` are per-controller arrays and a
    /// pointer that is not on screen reports `PointerController.OUT_OF_SCREEN_X/Y` (747, 0).
    pub fn sync_extern(&mut self){
        let xs:Vec<V>=self.pointer.iter().map(|p|V::Num(p.0 as f64)).collect();
        let ys:Vec<V>=self.pointer.iter().map(|p|V::Num(p.1 as f64)).collect();
        let (xa,ya)=(V::Obj(self.new_array(xs)),V::Obj(self.new_array(ys)));
        let mut e=self.extern_obj.borrow_mut();
        e.set_own("_xmouse",xa);e.set_own("_ymouse",ya);
        drop(e);
        if !self.extern_obj.borrow().has_prop("gWideScreenDefine"){
            let g=self.resolve_path_object("_global.GeneralUtils");
            let w=self.get_member(&g,"SCRSIZE_WIDE");
            if !w.is_undef(){self.extern_obj.borrow_mut().set_own("gWideScreenDefine",w);}
        }
    }

    pub fn pointer_move(&mut self,x:f32,y:f32){
        self.pointer[0]=(x,y);
        self.sync_extern();
        let mut best=None;
        if let Some(r)=self.root{self.handler_clip_at(r,x,y,&mut best);}
        let new=best.map(|id|NodeRef{id,generation:self.player.gens.get(id).copied().unwrap_or(0)});
        if new!=self.hover{
            if let Some(h)=self.hover{ if self.node_alive(h){self.fire_clip_event(h.id,"onRollOut");} }
            if let Some(n)=new{self.fire_clip_event(n.id,"onRollOver");}
            self.hover=new;
        }
        self.process_pending();
    }
    pub fn pointer_button(&mut self,down:bool){
        // A press on a UI element is not a click on the 3-D scene behind it.
        if down&&self.hover.is_none(){self.fe.kid_click=Some(self.pointer[0]);}
        if down{
            if let Some(h)=self.hover{ if self.node_alive(h){ self.pressed=Some(h);self.fire_clip_event(h.id,"onPress"); } }
        }else if let Some(p)=self.pressed.take(){
            if self.node_alive(p){ if Some(p)==self.hover{self.fire_clip_event(p.id,"onRelease");}else{self.fire_clip_event(p.id,"onReleaseOutside");} }
        }
        self.process_pending();
    }

    // ---------- movies ----------
    pub fn find_export(&self,name:&str,near:NodeId)->Option<(Rc<Movie>,u32)>{
        let m=&self.player.nodes[near].movie;
        if let Some(e)=m.apt.exports.iter().find(|e|e.name==name){return Some((m.clone(),e.id))}
        for m in self.movies.values(){if let Some(e)=m.apt.exports.iter().find(|e|e.name==name){return Some((m.clone(),e.id))}}
        None
    }
    pub fn load_movie_rec(&mut self,key:&str)->Result<Rc<Movie>,String>{
        let norm=norm_key(key);
        if let Some(m)=self.movies.get(&norm).cloned(){
            // The engine unloads movies nobody references; their init actions (class definitions) run again when
            // a screen that imports them is loaded.  The scripts are guarded, so replaying them is harmless.
            self.queue_inits(&m,&mut HashSet::new());
            return Ok(m)
        }
        let loader=self.loader.ok_or("no movie loader")?;
        let m=Rc::new(loader(key)?);
        self.movies.insert(m.key.clone(),m.clone());
        for imp in m.apt.imports.clone(){
            match self.load_movie_rec(&imp.movie){
                Ok(dep)=>match dep.apt.exports.iter().find(|e|e.name==imp.name){
                    Some(e)=>{m.links.borrow_mut().insert(imp.id,(dep.clone(),e.id));}
                    None=>self.log.push(format!("{}: import {} not exported by {}",m.key,imp.name,imp.movie)),
                },
                Err(e)=>self.log.push(format!("{}: import {} failed: {e}",m.key,imp.movie)),
            }
        }
        self.queue_inits(&m,&mut HashSet::new());
        Ok(m)
    }
    /// Queue the InitActions of `m` and (dependencies first) of everything it imports.
    fn queue_inits(&mut self,m:&Rc<Movie>,seen:&mut HashSet<String>){
        if !seen.insert(m.key.clone()){return}
        for imp in m.apt.imports.clone(){
            if let Some(dep)=self.movies.get(&norm_key(&imp.movie)).cloned(){self.queue_inits(&dep,seen);}
        }
        for fr in &m.apt.frames{for it in &fr.items{if let crate::apt::Item::InitAction{sprite,code}=it{
            let dup=self.player.pending.iter().any(|p|matches!(p,Pending::InitAction{movie_key,sprite:sp,..} if movie_key==&m.key&&sp==sprite));
            if !dup{self.player.pending.push(Pending::InitAction{movie_key:m.key.clone(),sprite:*sprite,code:*code});}
        }}}
    }
    /// loadMovie is asynchronous in the engine: the movie appears on a later frame, never inside the calling script.
    pub fn load_movie_into(&mut self,node:NodeId,path:&str){
        let r=NodeRef{id:node,generation:self.player.gens.get(node).copied().unwrap_or(0)};
        self.load_requests.push((path.to_string(),r));
    }
    fn service_loads(&mut self){
        let reqs=std::mem::take(&mut self.load_requests);
        for (path,r) in reqs{
            // The target may have been destroyed (and its slot reused) since loadMovie was called.
            if !self.node_alive(r){continue}
            let node=r.id;
            match self.load_movie_rec(&path){
                Ok(m)=>{self.log.push(format!("loadMovie {path} -> node {node}"));self.player.load_into(node,m)},
                Err(e)=>self.log.push(format!("loadMovie {path}: {e}")),
            }
        }
    }
    pub fn world_xf(&self,id:NodeId)->Xf{
        let mut xf=Xf::ID;let mut cur=Some(id);
        while let Some(c)=cur{xf=xf.then(&self.player.nodes[c].xf);cur=self.player.nodes[c].parent;}
        xf
    }
    pub fn world_bounds(&self,id:NodeId)->Option<[f32;4]>{
        let parent=self.player.nodes[id].parent;
        let pw=match parent{Some(p)=>self.world_xf(p),None=>Xf::ID};
        let mut lo=(f32::MAX,f32::MAX);let mut hi=(f32::MIN,f32::MIN);
        self.bounds(id,&pw,&mut lo,&mut hi);
        if lo.0>hi.0{None}else{Some([lo.0,lo.1,hi.0,hi.1])}
    }

    // ---------- driving ----------
    pub fn process_pending(&mut self){
        for _round in 0..64{
            if self.player.pending.is_empty(){break}
            let mut items=std::mem::take(&mut self.player.pending);
            // class definitions first, then constructors/frame scripts in tree order
            let mut order:Vec<Pending>=vec![];
            order.extend(items.iter().filter(|p|matches!(p,Pending::InitAction{..})).cloned());
            order.extend(items.drain(..).filter(|p|!matches!(p,Pending::InitAction{..})));
            for p in order{
                let r=match p{
                    Pending::InitAction{movie_key,code,sprite}=>{
                        let Some(m)=self.movies.get(&movie_key).cloned() else{continue};
                        let root=self.root.unwrap_or(0);let _=sprite;
                        // Init code runs with the root clip as `this`/timeline.
                        self.run_clip_code(root,&m,code as usize)
                    }
                    Pending::Constructed(n)=>self.construct_node(n),
                    Pending::FrameScript{node,code}=>{
                        if !self.player.nodes[node].alive{continue}
                        let m=self.player.nodes[node].movie.clone();
                        self.run_clip_code(node,&m,code as usize)
                    }
                };
                if let Err(e)=r{let s=self.to_str(&e);self.log.push(format!("script error: {s}"));}
            }
        }
    }

    /// One display frame: advance timelines, fire enterFrame handlers, run queued scripts and timers.
    pub fn tick(&mut self,frame_ms:f64){
        self.time_ms+=frame_ms;self.tick_count+=1;
        self.sync_extern();
        self.service_loads();
        self.process_pending();
        // Render-resumed callback requested by StartAPTRender.
        if let Some(n)=self.fe.render_cb_due{
            if n<=1{
                self.fe.render_cb_due=None;
                if let Some((name,scope))=self.fe.render_cb.clone(){
                    let s=self.resolve_path_object(&scope);
                    if let Err(e)=self.call_method(&s,&name,vec![]){let m=self.to_str(&e);self.log.push(format!("render callback {name}: {m}"));}
                }
            }else{self.fe.render_cb_due=Some(n-1);}
        }
        if !self.fe.later.is_empty(){
            let mut due=vec![];
            self.fe.later.retain_mut(|(n,name,args)|{ if *n<=1{due.push((name.clone(),args.clone()));false}else{*n-=1;true} });
            for (name,args) in due{self.fe.todo.push((name,args));}
        }
        if !self.fe.todo.is_empty(){
            let todo=std::mem::take(&mut self.fe.todo);
            for (name,args) in todo{ self.call_exposed(&name,args);self.process_pending(); }
        }
        if let Some(root)=self.root{
            self.player.advance(root);
            self.process_pending();
            self.fire_enter_frame(root);
            self.process_pending();
        }
        self.run_intervals();
        self.process_pending();
    }

    fn fire_enter_frame(&mut self,id:NodeId){
        if !self.player.nodes[id].alive{return}
        let kids:Vec<NodeId>=if let NodeKind::Sprite{children,..}=&self.player.nodes[id].kind{children.values().copied().collect()}else{return};
        self.run_clip_actions(id,0x2);
        let generation=self.gen_of(id);
        if let Some((g,o))=self.clip_objs.get(&id).cloned(){
            if g==generation{
                let h=o.borrow().get_prop("onEnterFrame").cloned();
                if let Some(h@V::Obj(_))=h{
                    if let Err(e)=self.call_function(&h,V::Obj(o.clone()),vec![],None){let s=self.to_str(&e);self.log.push(format!("onEnterFrame error: {s}"));}
                }
            }
        }
        for k in kids{self.fire_enter_frame(k);}
    }

    fn run_intervals(&mut self){
        let now=self.time_ms;
        let due:Vec<usize>=self.intervals.iter().enumerate().filter(|(_,i)|i.due<=now).map(|(k,_)|k).collect();
        for k in due{
            let (func,this,args,id)={let i=&mut self.intervals[k];i.due+=i.every.max(1.);(i.func.clone(),i.this.clone(),i.args.clone(),i.id)};
            let _=id;
            if let Err(e)=self.call_function(&func,this,args,None){let s=self.to_str(&e);self.log.push(format!("interval error: {s}"));}
        }
    }
}

pub fn norm_key(key:&str)->String{
    let mut rel=key.trim().replace('\\',"/");
    for ext in [".swf",".big",".apt"]{if rel.to_lowercase().ends_with(ext){rel.truncate(rel.len()-ext.len());}}
    rel.to_lowercase()
}

trait PlainStr{fn to_str_plain(&self)->String;}
impl PlainStr for V{fn to_str_plain(&self)->String{match self{V::Str(s)=>s.to_string(),V::Num(n)=>fmt_num(*n),_=>String::new()}}}
