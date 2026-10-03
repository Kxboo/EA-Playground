//! Built-in ActionScript objects for the APT VM: Object/Function/Array/String/Number/Boolean/Math/Color/Key/
//! MovieClip, timers, and the game bridge (`CallGameFunc`, `PlaySoundById`, ...), which queues host calls.
use crate::apt_vm::{fmt_num,Kind,NativeFn,NodeRef,Obj,Vm,V,O,Interval};
use crate::apt_player::{Kind as NodeKind,NodeId};
use std::rc::Rc;

fn arg(a:&[V],i:usize)->V{a.get(i).cloned().unwrap_or(V::Undef)}
fn num(a:&[V],i:usize)->f64{arg(a,i).to_num()}

fn clip_of(vm:&Vm,this:&V)->Option<NodeId>{
    if let V::Obj(o)=this{if let Kind::Clip(r)=o.borrow().kind(){if vm.node_alive(*r){return Some(r.id)}}}
    None
}

pub fn install(vm:&mut Vm){
    let g=vm.global.clone();
    let op=vm.object_proto.clone();let fp=vm.function_proto.clone();let ap=vm.array_proto.clone();let sp=vm.string_proto.clone();
    let np=vm.number_proto.clone();let bp=vm.boolean_proto.clone();let cp=vm.clip_proto.clone();let colp=vm.color_proto.clone();

    // constructors
    let mk=|vm:&mut Vm,name:&str,f:NativeFn,proto:&O|->O{
        let c=Rc::new(std::cell::RefCell::new(Obj{proto:Some(vm.function_proto.clone()),kind:Some(Kind::Native(f)),..Default::default()}));
        c.borrow_mut().set_own("prototype",V::Obj(proto.clone()));
        proto.borrow_mut().set_own("constructor",V::Obj(c.clone()));
        vm.global.borrow_mut().set_own(name,V::Obj(c.clone()));
        c
    };
    let object_c=mk(vm,"Object",|vm,_t,a|{ if let V::Obj(_)=arg(a,0){return Ok(arg(a,0))} Ok(V::Obj(vm.new_plain())) },&op);
    mk(vm,"Function",|_vm,_t,_a|Ok(V::Undef),&fp);
    mk(vm,"Array",|vm,t,a|{
        let items=if a.len()==1{if let V::Num(n)=a[0]{vec![V::Undef;n.max(0.) as usize]}else{a.to_vec()}}else{a.to_vec()};
        if let V::Obj(o)=t{let mut b=o.borrow_mut();b.kind=Some(Kind::Array(items));b.proto=Some(vm.array_proto.clone());}
        Ok(t.clone())
    },&ap);
    mk(vm,"String",|vm,t,a|{
        let s=if a.is_empty(){"".into()}else{vm.to_str(&a[0])};
        if let V::Obj(o)=t{if o.borrow().kind.is_none(){o.borrow_mut().kind=Some(Kind::Str(s.clone()));return Ok(t.clone())}}
        Ok(V::Str(s))
    },&sp);
    mk(vm,"Number",|_vm,t,a|{
        let n=num(a,0);
        if let V::Obj(o)=t{if o.borrow().kind.is_none(){o.borrow_mut().kind=Some(Kind::Number(n));return Ok(t.clone())}}
        Ok(V::Num(n))
    },&np);
    mk(vm,"Boolean",|_vm,t,a|{
        let b=arg(a,0).to_bool();
        if let V::Obj(o)=t{if o.borrow().kind.is_none(){o.borrow_mut().kind=Some(Kind::Bool(b));return Ok(t.clone())}}
        Ok(V::Bool(b))
    },&bp);
    let mc=mk(vm,"MovieClip",|_vm,_t,_a|Ok(V::Undef),&cp);
    let _=mc;
    mk(vm,"Color",|vm,t,a|{
        if let (V::Obj(o),Some(n))=(t,clip_of(vm,&arg(a,0))){let generation=vm.player.gens.get(n).copied().unwrap_or(0);let mut b=o.borrow_mut();b.kind=Some(Kind::Color(NodeRef{id:n,generation}));b.proto=Some(vm.color_proto.clone());}
        Ok(V::Undef)
    },&colp);

    // Object
    vm.native(&object_c,"registerClass",|vm,_t,a|{
        let name=vm.to_str(&arg(a,0)).to_string();let c=arg(a,1);
        if matches!(c,V::Obj(_)){vm.classes.insert(name,c);}else{vm.classes.remove(&name);}
        Ok(V::Bool(true))
    });
    vm.native(&op,"hasOwnProperty",|vm,t,a|{let n=vm.to_str(&arg(a,0));Ok(V::Bool(match t{V::Obj(o)=>o.borrow().has_prop(&n),_=>false}))});
    vm.native(&op,"toString",|_vm,_t,_a|Ok(V::str("[object Object]")));
    vm.native(&op,"valueOf",|_vm,t,_a|Ok(t.clone()));
    vm.native(&op,"isPrototypeOf",|vm,t,a|{let c=vm.proto_of_value(&arg(a,0));Ok(V::Bool(match (c,t){(Some(p),V::Obj(o))=>Rc::ptr_eq(&p,o),_=>false}))});
    vm.native(&op,"FwPrint",|vm,_t,a|{let s=vm.to_str(&arg(a,0));vm.log.push(format!("FwPrint: {s}"));Ok(V::Undef)});
    vm.native(&op,"addProperty",|_vm,_t,_a|Ok(V::Bool(true)));
    vm.native(&op,"watch",|_vm,_t,_a|Ok(V::Bool(true)));
    vm.native(&op,"unwatch",|_vm,_t,_a|Ok(V::Bool(true)));

    // Function
    vm.native(&fp,"call",|vm,t,a|{let this=arg(a,0);let rest=a.get(1..).unwrap_or(&[]).to_vec();vm.call_function(t,this,rest,None)});
    vm.native(&fp,"apply",|vm,t,a|{
        let this=arg(a,0);
        let args=match arg(a,1){V::Obj(o)=>match o.borrow().kind(){Kind::Array(items)=>items.clone(),_=>vec![]},_=>vec![]};
        vm.call_function(t,this,args,None)
    });
    vm.native(&fp,"toString",|_vm,_t,_a|Ok(V::str("[type Function]")));

    // Array
    fn with_items<R>(t:&V,f:impl FnOnce(&mut Vec<V>)->R)->Option<R>{if let V::Obj(o)=t{if let Some(Kind::Array(items))=o.borrow_mut().kind.as_mut(){return Some(f(items))}}None}
    vm.native(&ap,"push",|_vm,t,a|Ok(V::Num(with_items(t,|v|{v.extend(a.iter().cloned());v.len()}).unwrap_or(0) as f64)));
    vm.native(&ap,"pop",|_vm,t,_a|Ok(with_items(t,|v|v.pop()).flatten().unwrap_or(V::Undef)));
    vm.native(&ap,"shift",|_vm,t,_a|Ok(with_items(t,|v|if v.is_empty(){V::Undef}else{v.remove(0)}).unwrap_or(V::Undef)));
    vm.native(&ap,"unshift",|_vm,t,a|Ok(V::Num(with_items(t,|v|{for (i,x) in a.iter().enumerate(){v.insert(i,x.clone());}v.len()}).unwrap_or(0) as f64)));
    vm.native(&ap,"reverse",|_vm,t,_a|{with_items(t,|v|v.reverse());Ok(t.clone())});
    vm.native(&ap,"concat",|vm,t,a|{
        let mut out=with_items(t,|v|v.clone()).unwrap_or_default();
        for x in a{match with_items(x,|v|v.clone()){Some(items)=>out.extend(items),None=>out.push(x.clone())}}
        Ok(V::Obj(vm.new_array(out)))
    });
    vm.native(&ap,"join",|vm,t,a|{
        let sep=if a.is_empty(){",".into()}else{vm.to_str(&a[0])};
        let items=with_items(t,|v|v.clone()).unwrap_or_default();
        let parts:Vec<String>=items.iter().map(|x|vm.to_str(x).to_string()).collect();
        Ok(V::str(&parts.join(&sep)))
    });
    vm.native(&ap,"toString",|vm,t,_a|{let items=with_items(t,|v|v.clone()).unwrap_or_default();let parts:Vec<String>=items.iter().map(|x|vm.to_str(x).to_string()).collect();Ok(V::str(&parts.join(",")))});
    vm.native(&ap,"slice",|vm,t,a|{
        let items=with_items(t,|v|v.clone()).unwrap_or_default();let n=items.len() as i64;
        let norm=|x:f64|{let x=if x.is_nan(){0}else{x as i64};if x<0{(n+x).max(0)}else{x.min(n)}};
        let s=norm(num(a,0));let e=if a.len()>1&&!matches!(a[1],V::Undef){norm(num(a,1))}else{n};
        Ok(V::Obj(vm.new_array(if s<e{items[s as usize..e as usize].to_vec()}else{vec![]})))
    });
    vm.native(&ap,"splice",|vm,t,a|{
        let n=with_items(t,|v|v.len()).unwrap_or(0) as i64;
        let s=num(a,0);let s=if s<0.{(n+s as i64).max(0)}else{(s as i64).min(n)} as usize;
        let cnt=if a.len()>1{(num(a,1).max(0.) as usize).min(n as usize-s)}else{n as usize-s};
        let ins:Vec<V>=a.get(2..).unwrap_or(&[]).to_vec();
        let removed=with_items(t,|v|v.splice(s..s+cnt,ins).collect::<Vec<_>>()).unwrap_or_default();
        Ok(V::Obj(vm.new_array(removed)))
    });
    vm.native(&ap,"sort",|vm,t,a|{
        let mut items=with_items(t,|v|v.clone()).unwrap_or_default();
        let cmp=arg(a,0);
        if matches!(cmp,V::Obj(_)){
            // insertion sort so a script comparator can be called without borrowing the array
            for i in 1..items.len(){let mut j=i;while j>0{let r=vm.call_function(&cmp,V::Undef,vec![items[j-1].clone(),items[j].clone()],None)?.to_num();if r>0.{items.swap(j-1,j);j-=1;}else{break}}}
        }else{
            let mut keyed:Vec<(String,V)>=items.iter().map(|x|(vm.to_str(x).to_string(),x.clone())).collect();
            keyed.sort_by(|x,y|x.0.cmp(&y.0));items=keyed.into_iter().map(|x|x.1).collect();
        }
        with_items(t,|v|*v=items);Ok(t.clone())
    });
    vm.native(&ap,"sortOn",|vm,t,a|{
        let key=vm.to_str(&arg(a,0)).to_string();
        let items=with_items(t,|v|v.clone()).unwrap_or_default();
        let mut keyed:Vec<(f64,V)>=items.iter().map(|x|(vm.get_member(x,&key).to_num(),x.clone())).collect();
        keyed.sort_by(|x,y|x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        with_items(t,|v|*v=keyed.into_iter().map(|x|x.1).collect());Ok(t.clone())
    });

    // String
    fn this_str(vm:&mut Vm,t:&V)->Vec<char>{vm.to_str(t).chars().collect()}
    vm.native(&sp,"charAt",|vm,t,a|{let c=this_str(vm,t);Ok(V::str(&c.get(num(a,0) as usize).map(|c|c.to_string()).unwrap_or_default()))});
    vm.native(&sp,"charCodeAt",|vm,t,a|{let c=this_str(vm,t);Ok(c.get(num(a,0) as usize).map(|c|V::Num(*c as u32 as f64)).unwrap_or(V::Num(f64::NAN)))});
    vm.native(&sp,"concat",|vm,t,a|{let mut s=vm.to_str(t).to_string();for x in a{s.push_str(&vm.to_str(x));}Ok(V::str(&s))});
    vm.native(&sp,"indexOf",|vm,t,a|{let s=vm.to_str(t);let n=vm.to_str(&arg(a,0));let from=num(a,1);let from=if from.is_nan(){0}else{from.max(0.) as usize};
        let chars:Vec<char>=s.chars().collect();let hay:String=chars.iter().skip(from).collect();
        Ok(V::Num(hay.find(&*n).map(|b|(hay[..b].chars().count()+from) as f64).unwrap_or(-1.)))});
    vm.native(&sp,"lastIndexOf",|vm,t,a|{let s=vm.to_str(t);let n=vm.to_str(&arg(a,0));Ok(V::Num(s.rfind(&*n).map(|b|s[..b].chars().count() as f64).unwrap_or(-1.)))});
    vm.native(&sp,"slice",|vm,t,a|{
        let c=this_str(vm,t);let n=c.len() as i64;
        let norm=|x:f64|{let x=if x.is_nan(){0}else{x as i64};if x<0{(n+x).max(0)}else{x.min(n)}};
        let s=norm(num(a,0));let e=if a.len()>1&&!matches!(a[1],V::Undef){norm(num(a,1))}else{n};
        Ok(V::str(&if s<e{c[s as usize..e as usize].iter().collect::<String>()}else{String::new()}))
    });
    vm.native(&sp,"substring",|vm,t,a|{
        let c=this_str(vm,t);let n=c.len() as i64;
        let cl=|x:f64|(if x.is_nan(){0}else{x as i64}).clamp(0,n);
        let mut s=cl(num(a,0));let mut e=if a.len()>1&&!matches!(a[1],V::Undef){cl(num(a,1))}else{n};
        if s>e{std::mem::swap(&mut s,&mut e);}
        Ok(V::str(&c[s as usize..e as usize].iter().collect::<String>()))
    });
    vm.native(&sp,"substr",|vm,t,a|{
        let c=this_str(vm,t);let n=c.len() as i64;
        let s=num(a,0);let s=if s<0.{(n+s as i64).max(0)}else{(s as i64).min(n)};
        let l=if a.len()>1&&!matches!(a[1],V::Undef){(num(a,1).max(0.) as i64).min(n-s)}else{n-s};
        Ok(V::str(&c[s as usize..(s+l) as usize].iter().collect::<String>()))
    });
    vm.native(&sp,"toLowerCase",|vm,t,_a|Ok(V::str(&vm.to_str(t).to_lowercase())));
    vm.native(&sp,"toUpperCase",|vm,t,_a|Ok(V::str(&vm.to_str(t).to_uppercase())));
    vm.native(&sp,"split",|vm,t,a|{
        let s=vm.to_str(t);
        let parts:Vec<V>=match arg(a,0){
            V::Undef=>vec![V::Str(s.clone())],
            d=>{let d=vm.to_str(&d);if d.is_empty(){s.chars().map(|c|V::str(&c.to_string())).collect()}else{s.split(&*d).map(V::str).collect()}}
        };
        let lim=if a.len()>1{num(a,1) as usize}else{usize::MAX};
        Ok(V::Obj(vm.new_array(parts.into_iter().take(lim).collect())))
    });
    vm.native(&sp,"toString",|vm,t,_a|Ok(V::Str(vm.to_str(t))));
    vm.native(&sp,"valueOf",|vm,t,_a|Ok(V::Str(vm.to_str(t))));
    if let Some(V::Obj(sc))=g.borrow().get_prop("String").cloned(){
        vm.native(&sc,"fromCharCode",|_vm,_t,a|Ok(V::str(&a.iter().filter_map(|x|char::from_u32(x.to_num() as u32)).collect::<String>())));
    }
    // Number / Boolean
    vm.native(&np,"toString",|vm,t,a|{
        let n=t.to_num();let radix=if a.is_empty(){10}else{num(a,0) as u32};
        if radix==10||radix<2||radix>36{return Ok(V::str(&fmt_num(n)))}
        let mut v=n as i64;let neg=v<0;if neg{v=-v}let mut digits=vec![];if v==0{digits.push('0')}
        while v>0{digits.push(std::char::from_digit((v%radix as i64) as u32,radix).unwrap());v/=radix as i64;}
        if neg{digits.push('-')}let _=vm;Ok(V::str(&digits.iter().rev().collect::<String>()))
    });
    vm.native(&np,"valueOf",|_vm,t,_a|Ok(V::Num(t.to_num())));
    vm.native(&bp,"toString",|_vm,t,_a|Ok(V::str(if t.to_bool(){"true"}else{"false"})));
    vm.native(&bp,"valueOf",|_vm,t,_a|Ok(V::Bool(t.to_bool())));
    if let Some(V::Obj(nc))=g.borrow().get_prop("Number").cloned(){
        for (k,v) in [("MAX_VALUE",f64::MAX),("MIN_VALUE",5e-324),("NaN",f64::NAN),("POSITIVE_INFINITY",f64::INFINITY),("NEGATIVE_INFINITY",f64::NEG_INFINITY)]{nc.borrow_mut().set_own(k,V::Num(v));}
    }

    // Math
    let math=vm.new_plain();
    macro_rules! m1{($($n:literal=>$f:expr),*)=>{$(vm.native(&math,$n,|_vm,_t,a|{let x=num(a,0);let f:fn(f64)->f64=$f;Ok(V::Num(f(x)))});)*}}
    m1!("abs"=>f64::abs,"acos"=>f64::acos,"asin"=>f64::asin,"atan"=>f64::atan,"ceil"=>f64::ceil,"cos"=>f64::cos,"exp"=>f64::exp,"floor"=>f64::floor,"log"=>f64::ln,"sin"=>f64::sin,"sqrt"=>f64::sqrt,"tan"=>f64::tan,"round"=>|x|(x+0.5).floor());
    vm.native(&math,"atan2",|_vm,_t,a|Ok(V::Num(num(a,0).atan2(num(a,1)))));
    vm.native(&math,"pow",|_vm,_t,a|Ok(V::Num(num(a,0).powf(num(a,1)))));
    vm.native(&math,"max",|_vm,_t,a|Ok(V::Num(a.iter().map(|x|x.to_num()).fold(f64::NEG_INFINITY,f64::max))));
    vm.native(&math,"min",|_vm,_t,a|Ok(V::Num(a.iter().map(|x|x.to_num()).fold(f64::INFINITY,f64::min))));
    vm.native(&math,"random",|vm,_t,_a|{vm.rng^=vm.rng<<13;vm.rng^=vm.rng>>7;vm.rng^=vm.rng<<17;Ok(V::Num((vm.rng>>11) as f64/(1u64<<53) as f64))});
    for (k,v) in [("PI",std::f64::consts::PI),("E",std::f64::consts::E),("LN2",std::f64::consts::LN_2),("LN10",std::f64::consts::LN_10),("LOG2E",std::f64::consts::LOG2_E),("LOG10E",std::f64::consts::LOG10_E),("SQRT2",std::f64::consts::SQRT_2),("SQRT1_2",std::f64::consts::FRAC_1_SQRT_2)]{math.borrow_mut().set_own(k,V::Num(v));}
    g.borrow_mut().set_own("Math",V::Obj(math));

    // global functions
    vm.native(&g,"isNaN",|_vm,_t,a|Ok(V::Bool(num(a,0).is_nan())));
    vm.native(&g,"boolean",|_vm,_t,a|Ok(V::Bool(arg(a,0).to_bool())));
    vm.native(&g,"parseInt",|vm,_t,a|{
        let s=vm.to_str(&arg(a,0));let radix=if a.len()>1{num(a,1) as u32}else{10};let t=s.trim();
        let (t,radix)=if radix==16||(a.len()<2&&(t.starts_with("0x")||t.starts_with("0X"))){(t.trim_start_matches("0x").trim_start_matches("0X"),16)}else{(t,if radix<2{10}else{radix})};
        let (neg,t)=match t.strip_prefix('-'){Some(r)=>(true,r),None=>(false,t.strip_prefix('+').unwrap_or(t))};
        let digits:String=t.chars().take_while(|c|c.is_digit(radix)).collect();
        if digits.is_empty(){return Ok(V::Num(f64::NAN))}
        let v=i64::from_str_radix(&digits,radix).unwrap_or(0) as f64;Ok(V::Num(if neg{-v}else{v}))
    });
    vm.native(&g,"parseFloat",|vm,_t,a|{
        let s=vm.to_str(&arg(a,0));let t=s.trim();
        let mut end=0;let mut seen_dot=false;let mut seen_e=false;
        for (i,c) in t.char_indices(){
            let ok=c.is_ascii_digit()||(c=='-'||c=='+')&&(i==0||t[..i].ends_with(['e','E']))||(c=='.'&&!seen_dot&&!seen_e)||((c=='e'||c=='E')&&!seen_e&&i>0);
            if !ok{break}
            if c=='.'{seen_dot=true}if c=='e'||c=='E'{seen_e=true}end=i+c.len_utf8();
        }
        Ok(V::Num(t[..end].parse::<f64>().unwrap_or(f64::NAN)))
    });
    vm.native(&g,"escape",|vm,_t,a|{let s=vm.to_str(&arg(a,0));let mut o=String::new();for b in s.bytes(){if b.is_ascii_alphanumeric()||b"@-_.*+/".contains(&b){o.push(b as char)}else{o.push_str(&format!("%{b:02X}"))}}Ok(V::str(&o))});
    vm.native(&g,"unescape",|vm,_t,a|{let s=vm.to_str(&arg(a,0));let b=s.as_bytes();let mut o=vec![];let mut i=0;while i<b.len(){if b[i]==b'%'&&i+2<b.len()+0&&i+2<=b.len()-1+0{if let Ok(v)=u8::from_str_radix(&s[i+1..i+3],16){o.push(v);i+=3;continue}}o.push(b[i]);i+=1;}Ok(V::str(&String::from_utf8_lossy(&o)))});
    vm.native(&g,"ASSetPropFlags",|vm,_t,a|{
        // Only the "hidden from enumeration" bit matters here.
        let flags=num(a,2) as u32;let set_mask=if a.len()>3{num(a,3) as u32}else{0};
        if let V::Obj(o)=arg(a,0){
            let names:Vec<String>=match arg(a,1){V::Str(s)=>s.split(',').map(String::from).collect(),V::Obj(l)=>match l.borrow().kind(){Kind::Array(items)=>items.iter().map(|x|x.to_num().to_string()).collect(),_=>vec![]},_=>o.borrow().order.iter().map(|k|k.to_string()).collect()};
            let names=if matches!(arg(a,1),V::Null|V::Undef){o.borrow().order.iter().map(|k|k.to_string()).collect()}else{names};
            let mut b=o.borrow_mut();
            for n in names{ if flags&1!=0{b.hidden.insert(crate::apt_vm::lkey(&n).as_ref().into());} if set_mask&1!=0&&flags&1==0{b.hidden.remove(&*crate::apt_vm::lkey(&n));} }
        }
        let _=vm;Ok(V::Undef)
    });
    vm.native(&g,"setInterval",|vm,t,a|{
        let (func,this,rest)=if let V::Str(name)=arg(a,1){ // (obj, "method", ms, args...)
            let o=arg(a,0);let f=vm.get_member(&o,&name);(f,o,a.get(3..).unwrap_or(&[]).to_vec())
        }else{(arg(a,0),t.clone(),a.get(2..).unwrap_or(&[]).to_vec())};
        let ms=if let V::Str(_)=arg(a,1){num(a,2)}else{num(a,1)};
        let id=vm.next_interval;vm.next_interval+=1;
        let due=vm.time_ms+ms.max(1.);
        vm.intervals.push(Interval{id,func,this,args:rest,every:ms,due});
        Ok(V::Num(id as f64))
    });
    vm.native(&g,"clearInterval",|vm,_t,a|{let id=num(a,0) as u32;vm.intervals.retain(|i|i.id!=id);Ok(V::Undef)});
    vm.native(&g,"trace",|vm,_t,a|{let s=vm.to_str(&arg(a,0));vm.log.push(s.to_string());Ok(V::Undef)});
    vm.native(&g,"getTimer",|vm,_t,_a|Ok(V::Num(vm.time_ms.floor())));
    // game bridge: queued for the host to execute after the script step
    vm.native(&g,"CallGameFunc",|vm,_t,a|{let n=vm.to_str(&arg(a,0)).to_string();vm.host_calls.push((n,a.get(1..).unwrap_or(&[]).to_vec()));Ok(V::Undef)});
    vm.native(&g,"PlaySoundById",|vm,_t,a|{vm.host_calls.push(("PlaySoundById".into(),a.to_vec()));Ok(V::Undef)});
    vm.native(&g,"FwPrint",|vm,_t,a|{let s=vm.to_str(&arg(a,0));vm.log.push(format!("FwPrint: {s}"));Ok(V::Undef)});
    vm.native(&g,"AipAptPrint",|vm,_t,a|{let s=vm.to_str(&arg(a,0));vm.log.push(s.to_string());Ok(V::Undef)});
    g.borrow_mut().set_own("_global",V::Obj(g.clone()));

    // LoadVars: `load("Func?params")` is a call into the game; its result variables land on the object.
    let lv=vm.new_plain();
    let lvc=mk(vm,"LoadVars",|vm,t,_a|{ if let V::Obj(o)=t{o.borrow_mut().proto=Some(vm.loadvars_proto.clone());} Ok(V::Undef) },&lv);
    let _=lvc;
    vm.loadvars_proto=lv.clone();
    vm.native(&lv,"load",|vm,t,a|{
        let url=vm.to_str(&arg(a,0)).to_string();
        let (name,params)=url.split_once('?').map(|(a,b)|(a.to_string(),b.to_string())).unwrap_or((url.clone(),String::new()));
        let pairs=crate::fe_host::game_call(vm,&name,&params);
        if vm.trace_print&&!pairs.is_empty(){let m=format!("game call {name} -> {}",pairs.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join("&"));vm.log.push(m);}
        for (k,v) in pairs{vm.set_member(t,&k,V::str(&v));}
        let h=vm.get_member(t,"onLoad");
        if matches!(h,V::Obj(_)){vm.call_function(&h,t.clone(),vec![V::Bool(true)],None)?;}
        Ok(V::Bool(true))
    });
    vm.native(&lv,"send",|_vm,_t,_a|Ok(V::Bool(true)));
    vm.native(&lv,"sendAndLoad",|_vm,_t,_a|Ok(V::Bool(true)));
    vm.native(&lv,"getBytesLoaded",|_vm,_t,_a|Ok(V::Num(1.)));
    vm.native(&lv,"getBytesTotal",|_vm,_t,_a|Ok(V::Num(1.)));

    // Key / Mouse / Selection / Stage stubs
    let key=vm.new_plain();
    for (k,v) in [("BACKSPACE",8.),("TAB",9.),("ENTER",13.),("SHIFT",16.),("CONTROL",17.),("ESCAPE",27.),("SPACE",32.),("LEFT",37.),("UP",38.),("RIGHT",39.),("DOWN",40.),("DELETEKEY",46.),("HOME",36.),("END",35.),("PGUP",33.),("PGDN",34.),("INSERT",45.),("CAPSLOCK",20.)]{key.borrow_mut().set_own(k,V::Num(v));}
    vm.native(&key,"isDown",|vm,_t,a|{let c=num(a,0) as u32;Ok(V::Bool(vm.focus_keys.iter().any(|(k,d)|*k==c&&*d)))});
    vm.native(&key,"isToggled",|_vm,_t,_a|Ok(V::Bool(false)));
    vm.native(&key,"getCode",|vm,_t,_a|Ok(V::Num(vm.cur_key.0 as f64)));
    vm.native(&key,"getAscii",|_vm,_t,_a|Ok(V::Num(0.)));
    vm.native(&key,"getController",|vm,_t,_a|Ok(V::Num(vm.cur_key.1 as f64)));
    vm.native(&key,"addListener",|vm,_t,a|{ if let V::Obj(_)=arg(a,0){vm.key_listeners.push(arg(a,0));} Ok(V::Undef)});
    vm.native(&key,"removeListener",|vm,_t,a|{vm.key_listeners.retain(|l|!Vm::strict_eq(l,&arg(a,0)));Ok(V::Bool(true))});
    g.borrow_mut().set_own("Key",V::Obj(key));
    let mouse=vm.new_plain();
    vm.native(&mouse,"addListener",|vm,_t,a|{ if let V::Obj(_)=arg(a,0){vm.mouse_listeners.push(arg(a,0));} Ok(V::Undef)});
    vm.native(&mouse,"removeListener",|vm,_t,a|{vm.mouse_listeners.retain(|l|!Vm::strict_eq(l,&arg(a,0)));Ok(V::Bool(true))});
    vm.native(&mouse,"show",|_vm,_t,_a|Ok(V::Undef));vm.native(&mouse,"hide",|_vm,_t,_a|Ok(V::Undef));
    g.borrow_mut().set_own("Mouse",V::Obj(mouse));
    let stage=vm.new_plain();
    stage.borrow_mut().set_own("width",V::Num(640.));stage.borrow_mut().set_own("height",V::Num(480.));
    vm.native(&stage,"addListener",|_vm,_t,_a|Ok(V::Undef));vm.native(&stage,"removeListener",|_vm,_t,_a|Ok(V::Undef));
    g.borrow_mut().set_own("Stage",V::Obj(stage));
    let sel=vm.new_plain();
    vm.native(&sel,"setFocus",|_vm,_t,_a|Ok(V::Undef));vm.native(&sel,"getFocus",|_vm,_t,_a|Ok(V::Null));
    g.borrow_mut().set_own("Selection",V::Obj(sel));

    // Color
    vm.native(&colp,"setRGB",|vm,t,a|{
        if let V::Obj(o)=t{if let Kind::Color(r)=o.borrow().kind(){let r=*r;if vm.node_alive(r){
            let c=num(a,0) as u32;let n=&mut vm.player.nodes[r.id];
            n.cx.mul=[0.,0.,0.,n.cx.mul[3]];n.cx.add=[((c>>16)&255) as f32/255.,((c>>8)&255) as f32/255.,(c&255) as f32/255.,0.];
        }}}
        Ok(V::Undef)
    });
    vm.native(&colp,"getRGB",|vm,t,_a|{
        if let V::Obj(o)=t{if let Kind::Color(r)=o.borrow().kind(){let r=*r;if vm.node_alive(r){let a=vm.player.nodes[r.id].cx.add;return Ok(V::Num((((a[0]*255.) as u32)<<16|((a[1]*255.) as u32)<<8|(a[2]*255.) as u32) as f64))}}}
        Ok(V::Num(0.))
    });
    vm.native(&colp,"setTransform",|vm,t,a|{
        let tr=arg(a,0);
        if let V::Obj(o)=t{if let Kind::Color(r)=o.borrow().kind(){let r=*r;if vm.node_alive(r){
            let g=|vm:&mut Vm,k:&str,d:f64|{let v=vm.get_member(&tr,k);if v.is_undef(){d}else{v.to_num()}};
            let cx=&mut vm.player.nodes[r.id].cx;let _=cx;
            let (ra,ga,ba,aa)=(g(vm,"ra",100.),g(vm,"ga",100.),g(vm,"ba",100.),g(vm,"aa",100.));
            let (rb,gb,bb,ab)=(g(vm,"rb",0.),g(vm,"gb",0.),g(vm,"bb",0.),g(vm,"ab",0.));
            let cx=&mut vm.player.nodes[r.id].cx;
            cx.mul=[(ra/100.) as f32,(ga/100.) as f32,(ba/100.) as f32,(aa/100.) as f32];cx.add=[(rb/255.) as f32,(gb/255.) as f32,(bb/255.) as f32,(ab/255.) as f32];
        }}}
        Ok(V::Undef)
    });
    vm.native(&colp,"getTransform",|vm,t,_a|{
        let o=vm.new_plain();
        if let V::Obj(c)=t{if let Kind::Color(r)=c.borrow().kind(){let r=*r;if vm.node_alive(r){
            let cx=vm.player.nodes[r.id].cx;
            for (i,k) in ["ra","ga","ba","aa"].iter().enumerate(){o.borrow_mut().set_own(k,V::Num((cx.mul[i]*100.) as f64));}
            for (i,k) in ["rb","gb","bb","ab"].iter().enumerate(){o.borrow_mut().set_own(k,V::Num((cx.add[i]*255.) as f64));}
        }}}
        Ok(V::Obj(o))
    });

    install_movieclip(vm,&cp);
}

fn install_movieclip(vm:&mut Vm,cp:&O){
    vm.native(cp,"gotoAndPlay",|vm,t,a|{goto(vm,t,a,true);Ok(V::Undef)});
    vm.native(cp,"gotoAndStop",|vm,t,a|{goto(vm,t,a,false);Ok(V::Undef)});
    vm.native(cp,"play",|vm,t,_a|{if let Some(n)=clip_of(vm,t){if let NodeKind::Sprite{playing,..}=&mut vm.player.nodes[n].kind{*playing=true;}}Ok(V::Undef)});
    vm.native(cp,"stop",|vm,t,_a|{if let Some(n)=clip_of(vm,t){if let NodeKind::Sprite{playing,..}=&mut vm.player.nodes[n].kind{*playing=false;}}Ok(V::Undef)});
    vm.native(cp,"nextFrame",|vm,t,_a|{if let Some(n)=clip_of(vm,t){if let NodeKind::Sprite{frame,frame_count,..}=&vm.player.nodes[n].kind{let f=(*frame+1).min(frame_count.saturating_sub(1));vm.player.goto_frame(n,f);if let NodeKind::Sprite{playing,..}=&mut vm.player.nodes[n].kind{*playing=false;}}}Ok(V::Undef)});
    vm.native(cp,"prevFrame",|vm,t,_a|{if let Some(n)=clip_of(vm,t){if let NodeKind::Sprite{frame,..}=&vm.player.nodes[n].kind{let f=frame.saturating_sub(1);vm.player.goto_frame(n,f);if let NodeKind::Sprite{playing,..}=&mut vm.player.nodes[n].kind{*playing=false;}}}Ok(V::Undef)});
    vm.native(cp,"getBytesLoaded",|_vm,_t,_a|Ok(V::Num(100.)));
    vm.native(cp,"getBytesTotal",|_vm,_t,_a|Ok(V::Num(100.)));
    vm.native(cp,"getDepth",|vm,t,_a|Ok(clip_of(vm,t).map(|n|V::Num(vm.player.nodes[n].depth as f64)).unwrap_or(V::Undef)));
    vm.native(cp,"getNextHighestDepth",|vm,t,_a|{
        let d=clip_of(vm,t).and_then(|n|if let NodeKind::Sprite{children,..}=&vm.player.nodes[n].kind{children.keys().next_back().map(|k|k+1)}else{None}).unwrap_or(0).max(0);
        Ok(V::Num(d as f64))
    });
    vm.native(cp,"removeMovieClip",|vm,t,_a|{if let Some(n)=clip_of(vm,t){vm.remove_clip(n);}Ok(V::Undef)});
    vm.native(cp,"unloadMovie",|vm,t,_a|{
        if let Some(n)=clip_of(vm,t){let kids:Vec<NodeId>=if let NodeKind::Sprite{children,..}=&vm.player.nodes[n].kind{children.values().copied().collect()}else{vec![]};for k in kids{vm.remove_clip(k);}}
        Ok(V::Undef)
    });
    vm.native(cp,"swapDepths",|vm,t,a|{
        let Some(n)=clip_of(vm,t) else{return Ok(V::Undef)};
        let Some(parent)=vm.player.nodes[n].parent else{return Ok(V::Undef)};
        let other=match arg(a,0){V::Num(d)=>{if let NodeKind::Sprite{children,..}=&vm.player.nodes[parent].kind{children.get(&(d as i32)).copied()}else{None}}x=>clip_of(vm,&x)};
        let my=vm.player.nodes[n].depth;
        let target_depth=match (arg(a,0),other){(V::Num(d),_)=>d as i32,(_,Some(o))=>vm.player.nodes[o].depth,_=>return Ok(V::Undef)};
        if let NodeKind::Sprite{children,..}=&mut vm.player.nodes[parent].kind{
            children.remove(&my);
            if let Some(o)=other{children.remove(&target_depth);children.insert(my,o);}
            children.insert(target_depth,n);
        }
        vm.player.nodes[n].depth=target_depth;
        if let Some(o)=other{vm.player.nodes[o].depth=my;}
        Ok(V::Undef)
    });
    vm.native(cp,"createEmptyMovieClip",|vm,t,a|{
        let Some(p)=clip_of(vm,t) else{return Ok(V::Undef)};
        let name=vm.to_str(&arg(a,0)).to_string();let depth=num(a,1) as i32;
        let movie=vm.player.nodes[p].movie.clone();
        let n=vm.player.spawn_empty(&movie,p,depth);
        vm.player.nodes[n].name=name;
        Ok(V::Obj(vm.clip_obj(n)))
    });
    vm.native(cp,"attachMovie",|vm,t,a|{
        let Some(p)=clip_of(vm,t) else{return Ok(V::Undef)};
        let link=vm.to_str(&arg(a,0)).to_string();let name=vm.to_str(&arg(a,1)).to_string();let depth=num(a,2) as i32;
        let Some((movie,cid))=vm.find_export(&link,p) else{vm.log.push(format!("attachMovie: no export {link}"));return Ok(V::Undef)};
        let Some(n)=vm.player.spawn_character(&movie,cid,p,depth,false) else{return Ok(V::Undef)};
        vm.player.nodes[n].name=name;
        let o=vm.clip_obj(n);
        if let V::Obj(init)=arg(a,3){let keys:Vec<Rc<str>>=init.borrow().order.clone();for k in keys{let v=init.borrow().get_prop(&k).cloned().unwrap_or(V::Undef);o.borrow_mut().set_own(&k,v);}}
        vm.process_pending();
        Ok(V::Obj(o))
    });
    vm.native(cp,"duplicateMovieClip",|vm,t,a|{
        let Some(n)=clip_of(vm,t) else{return Ok(V::Undef)};
        let name=vm.to_str(&arg(a,0)).to_string();
        Ok(vm.duplicate(n,&name,num(a,1) as i32).map(|c|V::Obj(vm.clip_obj(c))).unwrap_or(V::Undef))
    });
    vm.native(cp,"createTextField",|vm,t,a|{
        let Some(p)=clip_of(vm,t) else{return Ok(V::Undef)};
        let name=vm.to_str(&arg(a,0)).to_string();let depth=num(a,1) as i32;
        let movie=vm.player.nodes[p].movie.clone();
        let n=vm.player.spawn_empty_text(&movie,p,depth);
        vm.player.nodes[n].name=name;vm.player.nodes[n].xf.0[4]=num(a,2) as f32;vm.player.nodes[n].xf.0[5]=num(a,3) as f32;
        Ok(V::Obj(vm.clip_obj(n)))
    });
    vm.native(cp,"loadMovie",|vm,t,a|{
        let Some(n)=clip_of(vm,t) else{return Ok(V::Undef)};
        let path=vm.to_str(&arg(a,0)).to_string();
        vm.load_movie_into(n,&path);
        Ok(V::Undef)
    });
    vm.native(cp,"loadVariables",|_vm,_t,_a|Ok(V::Undef));
    vm.native(cp,"hitTest",|vm,t,a|{
        let Some(n)=clip_of(vm,t) else{return Ok(V::Bool(false))};
        let b=vm.world_bounds(n);
        if std::env::var("EAGL_APT_HIT").is_ok_and(|k|vm.player.nodes[n].name.contains(&k)){let nm=vm.player.nodes[n].name.clone();let args:Vec<String>=a.iter().map(|x|vm.to_str(x).to_string()).collect();let m=format!("hitTest {nm} bounds {b:?} args {}",args.join(","));vm.log.push(m);}
        let r=if a.len()>=2{let (x,y)=(num(a,0) as f32,num(a,1) as f32);b.map(|b|x>=b[0]&&x<=b[2]&&y>=b[1]&&y<=b[3]).unwrap_or(false)}
              else if let Some(o)=clip_of(vm,&arg(a,0)){let b2=vm.world_bounds(o);match (b,b2){(Some(p),Some(q))=>p[0]<=q[2]&&q[0]<=p[2]&&p[1]<=q[3]&&q[1]<=p[3],_=>false}}else{false};
        Ok(V::Bool(r))
    });
    vm.native(cp,"getBounds",|vm,t,_a|{
        let o=vm.new_plain();
        if let Some(n)=clip_of(vm,t){if let Some(b)=vm.world_bounds(n){for (k,v) in [("xMin",b[0]),("yMin",b[1]),("xMax",b[2]),("yMax",b[3])]{o.borrow_mut().set_own(k,V::Num(v as f64));}}}
        Ok(V::Obj(o))
    });
    vm.native(cp,"localToGlobal",|vm,t,a|{
        if let (Some(n),V::Obj(pt))=(clip_of(vm,t),arg(a,0)){
            let x=vm.get_member(&V::Obj(pt.clone()),"x").to_num() as f32;let y=vm.get_member(&V::Obj(pt.clone()),"y").to_num() as f32;
            let w=vm.world_xf(n);let (gx,gy)=w.apply(x,y);
            pt.borrow_mut().set_own("x",V::Num(gx as f64));pt.borrow_mut().set_own("y",V::Num(gy as f64));
        }
        Ok(V::Undef)
    });
    vm.native(cp,"globalToLocal",|vm,t,a|{
        if let (Some(n),V::Obj(pt))=(clip_of(vm,t),arg(a,0)){
            let x=vm.get_member(&V::Obj(pt.clone()),"x").to_num() as f32;let y=vm.get_member(&V::Obj(pt.clone()),"y").to_num() as f32;
            if let Some(inv)=vm.world_xf(n).inverse(){let (lx,ly)=inv.apply(x,y);pt.borrow_mut().set_own("x",V::Num(lx as f64));pt.borrow_mut().set_own("y",V::Num(ly as f64));}
        }
        Ok(V::Undef)
    });
    vm.native(cp,"startDrag",|_vm,_t,_a|Ok(V::Undef));
    vm.native(cp,"stopDrag",|_vm,_t,_a|Ok(V::Undef));
    vm.native(cp,"setMask",|_vm,_t,_a|Ok(V::Undef));
    vm.native(cp,"getTextFormat",|vm,_t,_a|Ok(V::Obj(vm.new_plain())));
    vm.native(cp,"setTextFormat",|_vm,_t,_a|Ok(V::Undef));
    vm.native(cp,"getNewTextFormat",|vm,_t,_a|Ok(V::Obj(vm.new_plain())));
    vm.native(cp,"removeTextField",|vm,t,_a|{if let Some(n)=clip_of(vm,t){vm.remove_clip(n);}Ok(V::Undef)});
}

fn goto(vm:&mut Vm,t:&V,a:&[V],play:bool){
    let Some(n)=clip_of(vm,t) else{return};
    let frame=match arg(a,0){
        V::Num(x)=>Some((x as i64-1).max(0) as usize),
        V::Str(s)=>{let s=s.to_string();vm.player.label_frame(n,&s).or_else(||s.parse::<f64>().ok().map(|x|(x as i64-1).max(0) as usize))}
        _=>None,
    };
    if std::env::var("EAGL_APT_GOTO").is_ok(){let l=vm.to_str(&arg(a,0)).to_string();let m=format!("goto{} node {n} {l} -> {frame:?}",if play{"AndPlay"}else{"AndStop"});vm.log.push(m);}
    if let Some(f)=frame{vm.player.goto_frame(n,f);}
    if let NodeKind::Sprite{playing,..}=&mut vm.player.nodes[n].kind{*playing=play;}
}
