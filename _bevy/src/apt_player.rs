//! APT display tree and timeline stepping (Flash semantics: PlaceObject/RemoveObject accumulate frame by frame,
//! a backwards jump rebuilds the timeline from frame 0, frame scripts are queued for the VM).
use crate::{apt::{Apt,Character,Item,Place},apt_geom::Geometry};
use std::{collections::BTreeMap,rc::Rc};

pub type NodeId=usize;

pub struct Movie{pub key:String,pub apt:Apt,pub geom:Geometry,pub links:std::cell::RefCell<std::collections::HashMap<u32,(Rc<Movie>,u32)>>}

#[derive(Clone,Copy,Debug)]
pub struct Xf(pub [f32;6]);
impl Xf{
    pub const ID:Xf=Xf([1.,0.,0.,1.,0.,0.]);
    /// self applied first, then `outer` (child local -> parent space).
    pub fn then(&self,outer:&Xf)->Xf{
        let a=&self.0;let b=&outer.0;
        Xf([a[0]*b[0]+a[1]*b[2],a[0]*b[1]+a[1]*b[3],a[2]*b[0]+a[3]*b[2],a[2]*b[1]+a[3]*b[3],a[4]*b[0]+a[5]*b[2]+b[4],a[4]*b[1]+a[5]*b[3]+b[5]])
    }
    pub fn apply(&self,x:f32,y:f32)->(f32,f32){(x*self.0[0]+y*self.0[2]+self.0[4],x*self.0[1]+y*self.0[3]+self.0[5])}
    pub fn inverse(&self)->Option<Xf>{
        let m=&self.0;let det=m[0]*m[3]-m[1]*m[2];if det.abs()<1e-9{return None}
        let (a,b,c,d)=(m[3]/det,-m[1]/det,-m[2]/det,m[0]/det);
        Some(Xf([a,b,c,d,-(m[4]*a+m[5]*c),-(m[4]*b+m[5]*d)]))
    }
}

#[derive(Clone,Copy,Debug)]
pub struct Cx{pub mul:[f32;4],pub add:[f32;4]}
impl Cx{
    pub const ID:Cx=Cx{mul:[1.;4],add:[0.;4]};
    pub fn then(&self,outer:&Cx)->Cx{
        let mut o=Cx::ID;
        for i in 0..4{o.mul[i]=self.mul[i]*outer.mul[i];o.add[i]=self.add[i]*outer.mul[i]+outer.add[i];}
        o
    }
}

pub enum Kind{
    Sprite{
        /// Character id in `movie` (0 for the movie's root timeline).
        char_id:u32,
        frame:usize,frame_count:usize,playing:bool,
        children:BTreeMap<i32,NodeId>,
    },
    Shape{id:u32},
    /// `bounds`: field rectangle overridden by script (`_width`/`_height` resize the field, they do not scale glyphs).
    Text{id:u32,text:String,variable:String,bounds:Option<[f32;4]>},
}

pub struct Node{
    pub kind:Kind,pub movie:Rc<Movie>,pub parent:Option<NodeId>,pub depth:i32,
    pub xf:Xf,pub cx:Cx,pub visible:bool,pub name:String,pub clip_depth:i32,
    /// Set for children created by the timeline (removed when it rebuilds); AS-created clips persist.
    pub timeline:bool,
    /// VM object attached to this node (index into the VM's object table), 0 = none yet.
    pub obj:usize,
    pub ratio:f32,
    /// The frame whose items were last applied, so a node never re-applies a frame it already ran.
    pub applied:Option<usize>,
    pub alive:bool,
    pub clip_actions:Vec<crate::apt::ClipAction>,pub actions_movie:Option<Rc<Movie>>,
}

/// Work the VM must perform after a timeline step.
#[derive(Debug,Clone)]
pub enum Pending{
    /// Frame script of `node` at byte offset `code` of its movie's apt data.
    FrameScript{node:NodeId,code:u32},
    /// `onClipEvent`-style handler (flags are SWF ClipEventFlags) to run when the clip is created.
    InitAction{movie_key:String,sprite:u32,code:u32},
    Constructed(NodeId),
}

pub struct Player{
    pub gens:Vec<u32>,
    pub nodes:Vec<Node>,
    pub free:Vec<NodeId>,
    pub pending:Vec<Pending>,
    pub init_done:std::collections::HashSet<(String,u32)>,
    pub background:u32,
}

fn mat_of(p:&Place)->Xf{Xf(p.matrix)}
fn color_of(p:&Place)->Cx{
    let argb=|c:u32|[((c>>16)&255) as f32/255.,((c>>8)&255) as f32/255.,(c&255) as f32/255.,((c>>24)&255) as f32/255.];
    Cx{mul:argb(p.color),add:argb(p.add)}
}

impl Player{
    pub fn new()->Player{Player{gens:vec![],nodes:vec![],free:vec![],pending:vec![],init_done:Default::default(),background:0}}

    /// Every allocation gets a fresh generation so script handles to a destroyed (and recycled) node go stale.
    fn alloc(&mut self,n:Node)->NodeId{
        let i=if let Some(i)=self.free.pop(){self.nodes[i]=n;i}else{self.nodes.push(n);self.nodes.len()-1};
        if self.gens.len()<=i{self.gens.resize(i+1,0);}
        self.gens[i]+=1;
        i
    }

    /// Create the root clip of a movie (its main timeline) and run frame 0's placements.
    pub fn spawn_root(&mut self,movie:Rc<Movie>,parent:Option<NodeId>,depth:i32,name:&str)->NodeId{
        let n=movie.apt.frames.len();
        let id=self.alloc(Node{kind:Kind::Sprite{char_id:0,frame:0,frame_count:n,playing:true,children:BTreeMap::new()},movie,parent,depth,xf:Xf::ID,cx:Cx::ID,visible:true,name:name.to_string(),clip_depth:-1,timeline:false,obj:0,ratio:0.,applied:None,alive:true,clip_actions:vec![],actions_movie:None});
        if let Some(p)=parent{self.attach(p,depth,id);}
        self.pending.push(Pending::Constructed(id));
        self.enter_frame(id,0,true);
        id
    }

    fn attach(&mut self,parent:NodeId,depth:i32,child:NodeId){
        let old=if let Kind::Sprite{children,..}=&mut self.nodes[parent].kind{children.insert(depth,child)}else{None};
        if let Some(o)=old{if o!=child{self.destroy(o);}}
    }

    pub fn spawn_empty(&mut self,movie:&Rc<Movie>,parent:NodeId,depth:i32)->NodeId{
        let id=self.alloc(Node{kind:Kind::Sprite{char_id:u32::MAX,frame:0,frame_count:0,playing:false,children:BTreeMap::new()},movie:movie.clone(),parent:Some(parent),depth,xf:Xf::ID,cx:Cx::ID,visible:true,name:String::new(),clip_depth:-1,timeline:false,obj:0,ratio:0.,applied:None,alive:true,clip_actions:vec![],actions_movie:None});
        self.attach(parent,depth,id);
        self.pending.push(Pending::Constructed(id));
        id
    }
    pub fn spawn_empty_text(&mut self,movie:&Rc<Movie>,parent:NodeId,depth:i32)->NodeId{
        let id=self.alloc(Node{kind:Kind::Text{id:u32::MAX,text:String::new(),variable:String::new(),bounds:None},movie:movie.clone(),parent:Some(parent),depth,xf:Xf::ID,cx:Cx::ID,visible:true,name:String::new(),clip_depth:-1,timeline:false,obj:0,ratio:0.,applied:None,alive:true,clip_actions:vec![],actions_movie:None});
        self.attach(parent,depth,id);
        id
    }
    /// Replace a clip's contents with another movie's main timeline (`loadMovie`).
    pub fn load_into(&mut self,id:NodeId,movie:Rc<Movie>){
        let kids:Vec<NodeId>=if let Kind::Sprite{children,..}=&self.nodes[id].kind{children.values().copied().collect()}else{vec![]};
        for k in kids{self.destroy(k);}
        let n=movie.apt.frames.len();
        self.nodes[id].movie=movie;
        self.nodes[id].kind=Kind::Sprite{char_id:0,frame:0,frame_count:n,playing:true,children:BTreeMap::new()};
        self.nodes[id].applied=None;
        self.pending.push(Pending::Constructed(id));
        self.enter_frame(id,0,true);
    }

    pub fn spawn_character(&mut self,movie:&Rc<Movie>,char_id:u32,parent:NodeId,depth:i32,timeline:bool)->Option<NodeId>{
        // Imported characters live in another movie.
        let (movie,char_id)=if movie.apt.character(char_id).is_none(){
            let link=movie.links.borrow().get(&char_id).cloned();
            match link{Some((m,id))=>(m,id),None=>return None}
        }else{(movie.clone(),char_id)};
        let movie=&movie;
        let kind=match movie.apt.character(char_id)?{
            Character::Shape{..}=>Kind::Shape{id:char_id},
            Character::Sprite{frames}=>Kind::Sprite{char_id,frame:0,frame_count:frames.len(),playing:true,children:BTreeMap::new()},
            Character::Text(t)=>Kind::Text{id:char_id,text:t.text.clone(),variable:t.variable.clone(),bounds:None},
            Character::Movie=>Kind::Sprite{char_id:0,frame:0,frame_count:movie.apt.frames.len(),playing:true,children:BTreeMap::new()},
            _=>return None,
        };
        let is_sprite=matches!(kind,Kind::Sprite{..});
        let id=self.alloc(Node{kind,movie:movie.clone(),parent:Some(parent),depth,xf:Xf::ID,cx:Cx::ID,visible:true,name:String::new(),clip_depth:-1,timeline,obj:0,ratio:0.,applied:None,alive:true,clip_actions:vec![],actions_movie:None});
        self.attach(parent,depth,id);
        Some(id)
        .inspect(|&i|{if is_sprite{self.pending.push(Pending::Constructed(i));self.enter_frame(i,0,true);}})
    }

    pub fn destroy(&mut self,id:NodeId){
        if !self.nodes[id].alive{return}
        let kids:Vec<NodeId>=if let Kind::Sprite{children,..}=&self.nodes[id].kind{children.values().copied().collect()}else{vec![]};
        for k in kids{self.destroy(k);}
        self.nodes[id].alive=false;
        if let Kind::Sprite{children,..}=&mut self.nodes[id].kind{children.clear();}
        if let Some(p)=self.nodes[id].parent{
            let d=self.nodes[id].depth;
            if let Kind::Sprite{children,..}=&mut self.nodes[p].kind{if children.get(&d)==Some(&id){children.remove(&d);}}
        }
        self.free.push(id);
    }

    /// Items of frame `f` of node `id` (cloned so the tree can be mutated while applying them).
    fn frame_items(&self,id:NodeId,f:usize)->Vec<Item>{
        let n=&self.nodes[id];
        let Kind::Sprite{char_id,..}=&n.kind else{return vec![]};
        let frames=if *char_id==0{&n.movie.apt.frames}else{match n.movie.apt.character(*char_id){Some(Character::Sprite{frames})=>frames,_=>return vec![]}};
        frames.get(f).map(|fr|fr.items.clone()).unwrap_or_default()
    }

    /// Apply the placements of one frame; queue scripts when `scripts` (only on arrival at the target frame).
    fn apply_frame(&mut self,id:NodeId,f:usize,scripts:bool,reuse:bool){
        let items=self.frame_items(id,f);
        let movie=self.nodes[id].movie.clone();
        for it in items{
            match it{
                Item::Place(p)=>self.place(id,&movie,&p,reuse),
                Item::Remove(d)=>{
                    let c=if let Kind::Sprite{children,..}=&self.nodes[id].kind{children.get(&d).copied()}else{None};
                    if let Some(c)=c{self.destroy(c);}
                }
                Item::Action(code)=>if scripts{self.pending.push(Pending::FrameScript{node:id,code});}
                Item::InitAction{sprite,code}=>{
                    if self.init_done.insert((movie.key.clone(),sprite)){self.pending.push(Pending::InitAction{movie_key:movie.key.clone(),sprite,code});}
                }
                Item::Background(c)=>{self.background=c;}
                Item::Label{..}=>{}
            }
        }
    }

    fn place(&mut self,parent:NodeId,movie:&Rc<Movie>,p:&Place,reuse:bool){
        let existing=if let Kind::Sprite{children,..}=&self.nodes[parent].kind{children.get(&p.depth).copied()}else{None};
        let mv=p.flags&Place::MOVE!=0;let has_char=p.flags&Place::HAS_CHARACTER!=0;
        let target=match (existing,mv,has_char){
            (Some(e),true,false)=>Some(e),
            (Some(e),_,true) if reuse&&self.nodes[e].timeline&&self.char_of(e)==p.character as u32=>Some(e),
            (_,_,true)=>{
                // A new character replaces whatever sits at the depth (a pure "move" with a character keeps the old name/matrix).
                let keep=existing.filter(|_|mv).map(|e|(self.nodes[e].xf,self.nodes[e].cx,self.nodes[e].name.clone()));
                let n=self.spawn_character(movie,p.character as u32,parent,p.depth,true);
                if let (Some(n),Some((xf,cx,name)))=(n,keep){self.nodes[n].xf=xf;self.nodes[n].cx=cx;self.nodes[n].name=name;}
                n
            }
            _=>None,
        };
        let Some(t)=target else{return};
        let n=&mut self.nodes[t];
        if p.flags&Place::HAS_MATRIX!=0{n.xf=mat_of(p);}
        if p.flags&Place::HAS_COLOR!=0{n.cx=color_of(p);}
        if p.flags&Place::HAS_RATIO!=0{n.ratio=p.ratio;}
        if let Some(name)=&p.name{n.name=name.clone();}
        if p.flags&Place::HAS_CLIP_DEPTH!=0{n.clip_depth=p.clip_depth;}
        if p.flags&Place::HAS_CLIP_ACTIONS!=0{n.clip_actions=p.clip_actions.clone();n.actions_movie=Some(movie.clone());}
    }

    pub fn char_of(&self,id:NodeId)->u32{
        match &self.nodes[id].kind{Kind::Sprite{char_id,..}=>*char_id,Kind::Shape{id}=>*id,Kind::Text{id,..}=>*id}
    }

    /// Flash gotoFrame: forward jumps apply the skipped placements; backward jumps (and the wrap from the last
    /// frame) drop timeline children that are not placed at the target and update the ones that are.
    /// Scripts run only for the target frame.
    pub fn enter_frame(&mut self,id:NodeId,target:usize,first:bool){
        let count=match &self.nodes[id].kind{Kind::Sprite{frame_count,..}=>*frame_count,_=>return};
        if count==0{return}
        let target=target.min(count-1);
        let applied=if first{None}else{self.nodes[id].applied};
        match applied{
            None=>{
                for f in 0..target{self.apply_frame(id,f,false,false);}
                self.apply_frame(id,target,true,false);
            }
            Some(a) if target>a=>{
                for f in a+1..target{self.apply_frame(id,f,false,false);}
                self.apply_frame(id,target,true,false);
            }
            // Jumping to the frame a clip is already on does not re-run its frame script (the screens rely on this:
            // frame 0 holds `stop()` and the label `animateIn`).
            Some(a) if target==a=>{}
            Some(_)=>{
                // Backward jump: which characters does the target frame show at each depth?
                let mut sim:std::collections::HashMap<i32,u32>=Default::default();
                for f in 0..=target{for it in self.frame_items(id,f){match it{
                    Item::Place(p)=>if p.flags&Place::HAS_CHARACTER!=0{sim.insert(p.depth,p.character as u32);},
                    Item::Remove(d)=>{sim.remove(&d);}
                    _=>{}
                }}}
                let kids:Vec<(i32,NodeId)>=if let Kind::Sprite{children,..}=&self.nodes[id].kind{children.iter().map(|(d,c)|(*d,*c)).collect()}else{vec![]};
                for (d,c) in kids{if self.nodes[c].timeline&&sim.get(&d)!=Some(&self.char_of(c)){self.destroy(c);}}
                for f in 0..target{self.apply_frame(id,f,false,true);}
                self.apply_frame(id,target,true,true);
            }
        }
        if let Kind::Sprite{frame,..}=&mut self.nodes[id].kind{*frame=target;}
        self.nodes[id].applied=Some(target);
    }

    /// Advance every playing sprite below `id` by one frame. Clips present before the tick advance after their parent
    /// changed frame; clips the new frame creates start at their own frame 0 next tick.
    pub fn advance(&mut self,id:NodeId){
        if !self.nodes[id].alive{return}
        let (frame,count,playing,kids)=match &self.nodes[id].kind{
            Kind::Sprite{frame,frame_count,playing,children,..}=>(*frame,*frame_count,*playing,children.values().copied().collect::<Vec<_>>()),
            _=>return,
        };
        if playing&&count>1{self.enter_frame(id,if frame+1>=count{0}else{frame+1},false);}
        else if playing&&count==1{}
        for k in kids{if self.nodes[k].alive{self.advance(k);}}
    }

    pub fn goto_frame(&mut self,id:NodeId,target:usize){self.enter_frame(id,target,false);}

    pub fn label_frame(&self,id:NodeId,label:&str)->Option<usize>{
        let n=&self.nodes[id];
        let Kind::Sprite{char_id,..}=&n.kind else{return None};
        let frames=if *char_id==0{&n.movie.apt.frames}else{match n.movie.apt.character(*char_id){Some(Character::Sprite{frames})=>frames,_=>return None}};
        let want=label.to_lowercase();
        for (i,f) in frames.iter().enumerate(){for it in &f.items{if let Item::Label{name,..}=it{if name.to_lowercase()==want{return Some(i)}}}}
        None
    }

    /// Paint-order flattening: (node, world transform, world colour transform).
    pub fn flatten(&self,root:NodeId,out:&mut Vec<DrawItem>){
        self.flatten_into(root,&Xf::ID,&Cx::ID,&None,out);
    }
    fn flatten_into(&self,id:NodeId,xf:&Xf,cx:&Cx,mask:&Option<Mask>,out:&mut Vec<DrawItem>){
        let n=&self.nodes[id];
        if !n.alive||!n.visible{return}
        let wx=n.xf.then(xf);let wc=n.cx.then(cx);
        match &n.kind{
            Kind::Sprite{children,..}=>{
                // A child with a clip depth is a mask layer: it is not drawn, and it clips the siblings above it up to that depth.
                let mut active:Option<(i32,Mask)>=None;
                for (&depth,&c) in children{
                    if let Some((end,_))=&active{ if depth>*end{active=None;} }
                    let cn=&self.nodes[c];
                    if cn.alive&&cn.clip_depth>=0{
                        if cn.visible{
                            let mut shapes=vec![];
                            self.flatten_into(c,&wx,&wc,&None,&mut shapes);
                            let mut tris:Vec<[f32;6]>=vec![];
                            for d in &shapes{ if let DrawItem::Shape{movie,shape,xf,..}=d{
                                for prim in movie.geom.shapes.get(shape).into_iter().flatten(){
                                    for t in prim.indices.chunks_exact(3){
                                        let p:Vec<(f32,f32)>=t.iter().map(|&i|{let q=prim.positions[i as usize];xf.apply(q[0],q[1])}).collect();
                                        tris.push([p[0].0,p[0].1,p[1].0,p[1].1,p[2].0,p[2].1]);
                                    }
                                }
                            }}
                            tris.truncate(MASK_TRIS);
                            active=Some((cn.clip_depth,Rc::new(tris)));
                        }
                        continue
                    }
                    let m=active.as_ref().map(|(_,m)|m.clone()).or_else(||mask.clone());
                    self.flatten_into(c,&wx,&wc,&m,out);
                }
            }
            Kind::Shape{id:sid}=>out.push(DrawItem::Shape{movie:n.movie.clone(),shape:*sid,xf:wx,cx:wc,node:id,mask:mask.clone()}),
            Kind::Text{id:tid,text,bounds,..}=>out.push(DrawItem::Text{movie:n.movie.clone(),def:*tid,text:text.clone(),bounds:*bounds,xf:wx,cx:wc,node:id,mask:mask.clone()}),
        }
    }
}

/// Triangles (movie space) of a mask layer; at most `MASK_TRIS` are honoured.
pub type Mask=Rc<Vec<[f32;6]>>;
pub const MASK_TRIS:usize=48;

pub enum DrawItem{
    Shape{movie:Rc<Movie>,shape:u32,xf:Xf,cx:Cx,node:NodeId,mask:Option<Mask>},
    Text{movie:Rc<Movie>,def:u32,text:String,bounds:Option<[f32;4]>,xf:Xf,cx:Cx,node:NodeId,mask:Option<Mask>},
}
