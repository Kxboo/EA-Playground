//! The engine's native tween system (`AIP::AnimationAptExtObjClass`): the scripts register Move/Fade/Scale/Rotate
//! animations on clips and the movie's frame loop calls `DoAnimationLoop`, which advances every animation by one unit and
//! assigns integer-truncated results to the clip properties.  Easing functions are the executable's `*Eq` routines
//! (0x801123b0..0x80112874, single precision); a time within 0.01 of another counts as equal (`AIP::IsEqual`).
use crate::apt_vm::{Kind,NodeRef,Vm,V};
use crate::apt_player::NodeId;

#[derive(Clone)]
pub struct Anim{
    pub target:NodeRef,pub kind:u8,
    pub s:[f32;2],pub e:[f32;2],pub active:[bool;2],
    pub cur:f32,pub total:f32,pub motion:u8,pub p:f32,pub q:f32,
    pub scope:String,pub func:String,
    pub paused:bool,pub stopped:bool,pub completed:bool,
}
const PROPS:[[&str;2];4]=[["_x","_y"],["_alpha",""],["_width","_height"],["_rotation",""]];

fn is_equal(a:f32,b:f32)->bool{(a-b).abs()<=0.01}
fn is_less(a:f32,b:f32)->bool{!is_equal(a,b)&&a<b}

/// `AnimVars`: start, end, elapsed, duration, p, q.
pub fn ease(motion:u8,v:[f32;6])->f32{
    let (s,e,t,tt,p,q)=(v[0],v[1],v[2],v[3],v[4],v[5]);
    match motion{
        // 1 = Quadratic, 3 = Cubic, 4 = Quartic, 5 = Elastic, 6 = Spring, 7 = Bounce, 8 = Bounce2; 0 and the unset 2 = Linear
        1=>{
            if is_equal(tt,0.){return e}
            let u=t/tt;let f0=u*(s-e);let f2=u-2.0;f2.mul_add(f0,s)
        }
        3=>{
            if is_equal(tt,0.){return e}
            let u=t/tt;let f5=u*u;let f2=e-s;let f0=-(3.0f32.mul_add(u,-f5));let f2=u*f2;let f0=3.0+f0;f2.mul_add(f0,s)
        }
        4=>{
            if is_equal(tt,0.){return e}
            let u=t/tt;let f6=u*u;let f1=e-s;let f7=u*f6;let f3=-f1;let f1=-(4.0f32.mul_add(f6,-f7));let f3=f3*u;
            let f0=6.0f32.mul_add(u,f1);let f0=f0-4.0;f3.mul_add(f0,s)
        }
        6=>{
            if is_equal(tt,0.)||is_equal(t,tt){return e}
            let u=t/tt;let d=e-s;
            let f31=(2.0f64.powf((-10.0f32*u) as f64)) as f32;
            let f2=6.0f32*p;let f1=f2.mul_add(u,-0.5);let f1=std::f32::consts::PI*f1;
            let sn=(f1 as f64).sin() as f32;let f1=f31*sn;
            let f0=d.mul_add(f1,s);d+f0
        }
        7=>{
            let f30=-(2.0f32*q-tt);
            if is_equal(f30,0.){return e}
            let f28=f30*f30;let f29=t*t;let f31=e-s;
            let r=if is_less(t,f30){ s+(f31*f29)/f28 }else{
                let n=(t-f30) as i32;
                let rem=n-(n/4)*4;
                if (rem as u32)>1{ f31+s }else{
                    let f5=p/q;let f3=n as f32;let f2=f3-1.0;let f1=q-f2*0.5;
                    let f1=-(f5.mul_add(f1,-f31));
                    s+f1
                }
            };
            if t==tt{e}else{r}
        }
        8|5=>{
            if is_equal(tt,0.){return e}
            let f0=tt*tt;let f1=2.0*tt;let f3=p*tt;let f11=p/f1;let f9=e-s;
            let f4=t*t;let f1=tt*f0;
            let f3=-(0.5f32.mul_add(f3,-f9));
            let f0=f11*f4;let f10=t-tt;
            let f4=f3/f1;let f2=f10*f10;let f1=f2*f10;
            let f0=f4.mul_add(f1,f0);let f0=f3+f0;let r=s+f0;
            if motion==8&&r>e{ let d=r-e;e-d }else{r}
        }
        _=>{
            if is_equal(tt,0.){return e}
            t.mul_add((e-s)/tt,s)
        }
    }
}

impl Vm{
    fn anim_f(&mut self,init:&V,name:&str,default:f32)->f32{
        let v=self.get_member(init,name);
        if v.is_undef(){default}else{v.to_num() as f32}
    }
    fn anim_str(&mut self,init:&V,name:&str)->String{
        let v=self.get_member(init,name);
        match v{
            V::Undef|V::Null=>String::new(),
            V::Obj(ref o)=>{
                let cid=if let Kind::Clip(r)=o.borrow().kind(){Some(r.id)}else{None};
                match cid{Some(id)=>self.clip_path(id),None=>String::new()}
            }
            other=>self.to_str(&other).to_string(),
        }
    }

    pub fn aeo_register(&mut self,target:&V,init:&V){
        let Some(tid)=self.value_node_pub(target) else{return};
        let kind=self.get_member(init,"m_animationType").to_num();let kind=if kind.is_nan(){0}else{kind as i32};
        let kind=kind.clamp(0,3) as u8;
        let motion=self.get_member(init,"m_motionType").to_num();let motion=if motion.is_nan(){0}else{motion as u8};
        let total=self.anim_f(init,"m_time",0.);
        let cur=if self.get_member(init,"curTime").is_undef(){0.}else{self.get_member(init,"curTime").to_num() as f32};
        let (mut s,mut e)=([0f32;2],[0f32;2]);let mut active=[true,false];
        let (mut p,mut q)=(0f32,0f32);
        match kind{
            0=>{
                s=[self.anim_f(init,"m_startX",0.),self.anim_f(init,"m_startY",0.)];
                e=[self.anim_f(init,"m_endX",0.),self.anim_f(init,"m_endY",0.)];
                active=[s[0]!=e[0],s[1]!=e[1]];
            }
            1=>{s[0]=self.anim_f(init,"m_startAlpha",100.);e[0]=self.anim_f(init,"m_endAlpha",100.);}
            2=>{
                s=[self.anim_f(init,"m_startWidth",0.),self.anim_f(init,"m_startHeight",0.)];
                e=[self.anim_f(init,"m_endWidth",0.),self.anim_f(init,"m_endHeight",0.)];
                active=[true,true];
            }
            _=>{s[0]=self.anim_f(init,"m_startRotation",0.);e[0]=self.anim_f(init,"m_endRotation",0.);}
        }
        match motion{
            5|8=>{p=self.anim_f(init,"m_overSlope",0.);}
            6=>{p=self.anim_f(init,"m_springFreq",0.);}
            7=>{p=self.anim_f(init,"m_bounceHeight",0.);q=self.anim_f(init,"m_numBounces",0.);}
            _=>{}
        }
        let scope=self.anim_str(init,"m_onFinishScope");let func=self.anim_str(init,"m_onFinishFunc");
        // The constructor positions the target at the start values immediately.
        let r=NodeRef{id:tid,generation:self.player.gens.get(tid).copied().unwrap_or(0)};
        let o=V::Obj(self.clip_obj(tid));
        match kind{
            0=>{ for i in 0..2{ self.set_member(&o,PROPS[0][i],V::Num(s[i] as f64)); } }
            2=>{ for i in 0..2{ self.set_member(&o,PROPS[2][i],V::Num(s[i] as f64)); } }
            k=>{ self.set_member(&o,PROPS[k as usize][0],V::Num(s[0] as f64)); }
        }
        if self.trace_anim{ let m=format!("AEO register kind={kind} target={} motion={motion} s={:?} e={:?} T={total} cur={cur} finish={scope}.{func}",self.clip_path(tid),s,e);self.log.push(m); }
        let was_empty=self.anims.is_empty();
        self.anims.push(Anim{target:r,kind,s,e,active,cur,total,motion,p,q,scope,func,paused:false,stopped:false,completed:false});
        if was_empty{ self.call_root("startAnimationLoop"); }
    }

    /// `AptCallFunction(name, ..., "_root")`: the engine reaches script through the functions screens exposed.
    pub fn call_root(&mut self,name:&str){ self.call_exposed(name,vec![]); }

    /// Resolve a scripted object path like `_level0.main.m_wm` or `_root.foo`.
    pub fn resolve_path_object(&mut self,path:&str)->V{
        if path.is_empty(){return V::Undef}
        let mut parts=path.split(['.','/']).filter(|p|!p.is_empty());
        let first=parts.next().unwrap_or("");
        let mut cur=match first{
            "_global"=>V::Obj(self.global.clone()),
            "_root"|"_level0"=>match self.root{Some(r)=>V::Obj(self.clip_obj(r)),None=>V::Undef},
            other=>{
                match self.root{
                    Some(r)=>{let o=V::Obj(self.clip_obj(r));let v=self.get_member(&o,other);if v.is_undef(){let g=V::Obj(self.global.clone());self.get_member(&g,other)}else{v}}
                    None=>V::Undef,
                }
            }
        };
        for p in parts{cur=self.get_member(&cur,p);}
        cur
    }

    pub fn aeo_loop(&mut self){
        if self.trace_anim{ let m=format!("AEO loop n={} {:?}",self.anims.len(),self.anims.iter().map(|a|(a.kind,a.cur,a.total)).collect::<Vec<_>>());self.log.push(m); }
        let mut finished:Vec<Anim>=vec![];
        let mut keep:Vec<Anim>=vec![];
        let list=std::mem::take(&mut self.anims);
        for mut a in list{
            if self.aeo_process(&mut a){keep.push(a)}else{finished.push(a)}
        }
        let added=std::mem::take(&mut self.anims);
        self.anims=keep;self.anims.extend(added);
        for a in finished{
            if self.trace_anim{ let m=format!("AEO finished kind={} {}.{}",a.kind,a.scope,a.func);self.log.push(m); }
            if !a.func.is_empty(){
                let scope=self.resolve_path_object(&a.scope);
                let scope=if scope.is_undef(){ match self.root{Some(r)=>V::Obj(self.clip_obj(r)),None=>V::Undef} }else{scope};
                if let Err(e)=self.call_method(&scope,&a.func,vec![]){let s=self.to_str(&e);self.log.push(format!("animation onFinish {}: {s}",a.func));}
            }
        }
        if self.anims.is_empty(){ self.call_root("endAnimationLoop"); }
    }

    /// Returns true while the animation continues.
    fn aeo_process(&mut self,a:&mut Anim)->bool{
        if !self.node_alive(a.target){return false}
        if a.cur>=a.total{return false}
        if a.stopped{return false}
        let o=V::Obj(self.clip_obj(a.target.id));
        if a.completed{
            for i in 0..2{ let nm=PROPS[a.kind as usize][i]; if !nm.is_empty()&&(a.active[i]||a.kind!=0){ self.set_member(&o,nm,V::Num(a.e[i] as f64)); } }
            return false
        }
        if a.paused{return true}
        a.cur+=1.0;
        if a.cur<0.{return true}
        for i in 0..2{
            let nm=PROPS[a.kind as usize][i];
            if nm.is_empty()||(a.kind==0&&!a.active[i]){continue}
            let v=ease(a.motion,[a.s[i],a.e[i],a.cur,a.total,a.p,a.q]);
            self.set_member(&o,nm,V::Num((v as i32) as f64));
        }
        true
    }

    fn aeo_find(&mut self,target:&V,kind:&V,f:impl Fn(&mut Anim)){
        let Some(t)=self.value_node_pub(target) else{return};
        let k=kind.to_num() as i32;
        for a in self.anims.iter_mut(){ if a.target.id==t&&a.kind as i32==k{f(a);} }
    }
}

fn a0(a:&[V])->V{a.first().cloned().unwrap_or(V::Undef)}
fn a1(a:&[V])->V{a.get(1).cloned().unwrap_or(V::Undef)}

pub fn install(vm:&mut Vm){
    let aeo=vm.new_plain();
    vm.native(&aeo,"RegisterAnimation",|vm,_t,a|{vm.aeo_register(&a0(a),&a1(a));Ok(V::Undef)});
    vm.native(&aeo,"DoAnimationLoop",|vm,_t,_a|{vm.aeo_loop();Ok(V::Undef)});
    vm.native(&aeo,"PauseAnimation",|vm,_t,a|{vm.aeo_find(&a0(a),&a1(a),|x|x.paused=true);Ok(V::Undef)});
    vm.native(&aeo,"ResumeAnimation",|vm,_t,a|{vm.aeo_find(&a0(a),&a1(a),|x|x.paused=false);Ok(V::Undef)});
    vm.native(&aeo,"StopAnimation",|vm,_t,a|{vm.aeo_find(&a0(a),&a1(a),|x|x.stopped=true);Ok(V::Undef)});
    vm.native(&aeo,"CompleteAnimation",|vm,_t,a|{vm.aeo_find(&a0(a),&a1(a),|x|x.completed=true);Ok(V::Undef)});
    vm.native(&aeo,"RemoveAnimationObject",|vm,_t,a|{
        let Some(t)=vm.value_node_pub(&a0(a)) else{return Ok(V::Undef)};
        let k=a1(a).to_num() as i32;
        vm.anims.retain(|x|!(x.target.id==t&&x.kind as i32==k));Ok(V::Undef)
    });
    vm.native(&aeo,"APTAssert",|vm,_t,a|{let s=vm.to_str(&a0(a));vm.log.push(format!("APTAssert: {s}"));Ok(V::Undef)});
    vm.global.borrow_mut().set_own("AeoAnimation",V::Obj(aeo));
}
