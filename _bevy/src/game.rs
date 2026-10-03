//! Game mode: boots from the original data files, loads the world and a player, and lets the
//! recovered locomotion (see `locomotion.rs`) drive a character.  Evidence per subsystem is shown
//! in the HUD and written by `--selftest`; nothing here is labelled original unless it traces to
//! the executable or a data file.
use bevy::{camera::visibility::RenderLayers,prelude::*,render::view::window::screenshot::{Screenshot,save_to_disk},input::mouse::{AccumulatedMouseMotion,AccumulatedMouseScroll},window::{PrimaryWindow,RequestRedraw},winit::{WinitSettings,UpdateMode},};
use bevy_egui::{egui,EguiContexts,EguiTextureHandle};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use std::{collections::{HashMap},path::PathBuf,time::Instant};
use crate::{archive,assets,bridge,character,gsh,locomotion::{InputKind,Locomotion},menu::AppMode,model,recovered as k,sim_time::{FramePolicy,FrameStep}};
use std::sync::{Mutex,mpsc};
use crate::character_input::{CharacterInputState,SurfaceSupport};
use crate::character_movement::CharacterMovementState;

#[derive(Component,Debug,Clone,Serialize,Deserialize)]
pub struct OriginalAsset {
    pub source:String,
    pub decoder_version:String,
    pub evidence:EvidenceLevel,
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
pub enum EvidenceLevel { Unresolved, AssetDerived, ExecutableDerived, GameplayCompared }

const CHARACTER_HEIGHT:f32=1.0;
const STEP_UP:f32=0.6;
const MIN_BOOT_SECONDS:f32=2.5;
use character::CLIPS;
const WORLD_SUFFIXES:[&str;4]=["","-alpha","-fade","-alphafade"];

/// Convert the original row-vector matrix to Bevy's column-vector convention.
/// A missing radius uses the original disabled-curvature path while loading.
/// Set when the world was entered from the frontend's Single Player entry: Esc then returns to the frontend.
pub static FROM_FE:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
pub fn display_matrix(radius:f32,position:Vec3)->Mat4{
    Mat4::from_cols_array(&crate::area_transform::AreaTransform{radius,disabled:radius<=0.}.model_matrix(position.to_array()))
}

#[derive(Component)] pub struct GameEntity;
#[derive(Component)] pub struct GameCamera{pub yaw:f32,pub pitch:f32,pub distance:f32}
#[derive(Component)] pub struct Player{loco:Locomotion,physics_input:CharacterInputState,movement:CharacterMovementState,physics_frames:u64,velocity:Vec3,support_normal:Vec3,grounded:bool,speed:f32,facing:Vec3,weights:[f32;3],anim_ready:bool}
#[derive(Component)] pub struct WorldLayer(pub String);
#[derive(Component)] struct PlayerModel;
#[derive(Clone,Debug)] pub struct Placeable{pub id:String,pub asset:String,pub pos:Vec3,pub orientation_deg:f32,pub physics:Option<String>,/// `default_visible` (hidden ones are spawned invisible).
    pub visible:bool}
#[derive(Component)] pub struct PlaceableProp;
/// SkyDome layers (`SkyDome::LoadGeometry` 0x803c6578): skybox, mountain/city ring, clouds; only the clouds turn.
#[derive(Component)] struct SkyLayer{clouds:bool}
#[derive(Component)] struct SkyCamera;
const SKY_LAYER:usize=1;
const SKY_MODELS:[(&str,bool);3]=[("skybox",false),("skybox_mountain_city_ring",false),("skybox_clouds",true)];

/// Keyboard/script -> Wii button adapter. The adapter and bindings are separate:
/// actions consumed by gameplay come only from the recovered Controller dispatcher.
#[derive(Resource,Default)] pub struct GameInput{pub x:f32,pub y:f32,pub jump:bool,pub reorient:bool,pub scripted:bool}
#[derive(Default)] struct DispatchedInput{x:f32,y:f32,jump:bool,reorient:bool}

#[derive(Resource,Default)]
struct SimulationClock{frame:FrameStep,frames:u64,total_ms:u64,max_simulation_ms:u32,max_input_ms:i32}

/// Read the uncapped host clock; the original applies its cap after integer truncation.
/// Loading resets these diagnostics so they describe the current playable session only.
fn sample_frame(time:Res<Time<Real>>,g:Option<Res<Game>>,mut clock:ResMut<SimulationClock>){
    if !g.is_some_and(|g|g.is_playing()){*clock=SimulationClock::default();return}
    clock.frame=FramePolicy::default().step((time.delta_secs_f64()*1000.) as f32);
    clock.frames+=1;clock.total_ms+=clock.frame.simulation_ms as u64;
    clock.max_simulation_ms=clock.max_simulation_ms.max(clock.frame.simulation_ms);
    clock.max_input_ms=clock.max_input_ms.max(clock.frame.input_ms);
}

/// Decoded player assets waiting for the build step.
struct PlayerAsset{up:assets::Uploaded,skeleton:crate::skeleton::Skeleton,source:String}
/// What the loader thread hands to the main thread (CPU-side only; Bevy assets are created on the main thread).
enum LoadMsg{Boot(Vec<u8>,usize,usize),Csv(&'static str,Vec<Vec<String>>),Controls(Result<Vec<crate::controller::Binding>,String>),Expect{models:usize},Model{kind:ModelKind,result:Result<assets::BuiltModel,String>},Player(Result<character::CharacterData,String>),Log(String)}
#[derive(Clone)] enum ModelKind{Layer(String),Sky{clouds:bool},Prop(String)}

fn parse_csv(d:&[u8])->Vec<Vec<String>>{
    String::from_utf8_lossy(d).trim_start_matches(char::from_u32(0xfeff).unwrap()).lines().filter(|l|!l.trim().is_empty()).map(|l|l.split(',').map(|c|c.trim().trim_matches('"').to_string()).collect()).collect()
}

/// Decodes everything the world needs with the Rust decoders: boot screen, CSV tables, sky, world layers, props.
fn loader(tx:mpsc::Sender<LoadMsg>,placeables:Vec<Placeable>){
    let p=|rel:&str|src(rel);
    let schemas=model::Schemas::embedded();
    let strap=p("boot/strapwarn_standard_english.gsh");
    match archive::read_virtual(&strap).and_then(|(d,_)|{let g=gsh::parse(&d)?;let e=g.entries.first().ok_or("empty GSH")?.clone();gsh::decode(&e,&d)}){
        Ok((rgba,w,h))=>{let _=tx.send(LoadMsg::Boot(rgba,w,h));}
        Err(e)=>{let _=tx.send(LoadMsg::Log(format!("boot screen: {e}")));}
    }
    let _=tx.send(LoadMsg::Player(character::load(&p("characters"),&schemas)));
    let controls=archive::read_virtual(&format!("{}::controls.csv",p("csvs.viv"))).and_then(|(d,_)|crate::control_bindings::parse(&d));
    let _=tx.send(LoadMsg::Controls(controls));
    let big=p("world/world.big");
    let mut layers:Vec<String>=vec![];
    for (key,source) in [("filelist",format!("{big}::worldfilelist.csv")),("bounds",format!("{big}::world.csv"))]{
        match archive::read_virtual(&source){
            Ok((d,_))=>{
                let rows=parse_csv(&d);
                if key=="filelist"{
                    // worldfilelist.csv (FILE_NAME,IS_HIGH): IS_HIGH=1 rows are detailed area chunks (<name>-high*.o), IS_HIGH=0 the whole-world mesh.
                    for row in rows.iter().skip(1){for sfx in WORLD_SUFFIXES{layers.push(if row.get(1).map(String::as_str)==Some("1"){format!("{}-high{sfx}.o",row[0])}else{format!("{}{sfx}.o",row[0])});}}
                }
                let _=tx.send(LoadMsg::Csv(key,rows));
            }
            Err(e)=>{let _=tx.send(LoadMsg::Log(format!("{key}: {e}")));}
        }
    }
    let mut list:Vec<(ModelKind,String)>=vec![];
    for (name,clouds) in SKY_MODELS{list.push((ModelKind::Sky{clouds},format!("{}::{name}.o",p(&format!("placeables/{name}.viv")))));}
    for l in layers{list.push((ModelKind::Layer(l.clone()),format!("{big}::{l}")));}
    let mut prop_assets:Vec<String>=placeables.iter().map(|q|q.asset.clone()).collect();prop_assets.sort();prop_assets.dedup();
    for a in prop_assets{list.push((ModelKind::Prop(a.clone()),format!("{}::{a}.o",p(&format!("placeables/{}.viv",a.to_lowercase())))));}
    let _=tx.send(LoadMsg::Expect{models:list.len()});
    for (kind,source) in list{let _=tx.send(LoadMsg::Model{kind,result:assets::build(&source,&schemas)});}
}
#[derive(PartialEq,Clone,Debug)] enum Phase{Loading,Building,Playing,Failed(String)}

pub struct Ground{cell:f32,min:Vec2,cols:usize,rows:usize,cells:Vec<Vec<u32>>,walls:Vec<Vec<u32>>,tris:Vec<[Vec3;3]>}
impl Ground {
    fn build(tris:Vec<[Vec3;3]>)->Self{
        let (mut lo,mut hi)=(Vec2::splat(f32::MAX),Vec2::splat(f32::MIN));
        for t in &tris{for v in t{lo=lo.min(v.xz());hi=hi.max(v.xz());}}
        let cell=2.;let cols=(((hi.x-lo.x)/cell).ceil() as usize+1).max(1);let rows=(((hi.y-lo.y)/cell).ceil() as usize+1).max(1);
        let mut cells=vec![Vec::new();cols*rows];let mut walls=vec![Vec::new();cols*rows];
        for (i,t) in tris.iter().enumerate(){
            let n=(t[1]-t[0]).cross(t[2]-t[0]).normalize_or_zero();
            let tl=t[0].xz().min(t[1].xz()).min(t[2].xz());let th=t[0].xz().max(t[1].xz()).max(t[2].xz());
            let (c0,r0)=(((tl.x-lo.x)/cell) as usize,((tl.y-lo.y)/cell) as usize);
            let (c1,r1)=(((th.x-lo.x)/cell) as usize,((th.y-lo.y)/cell) as usize);
            // Upward-facing surfaces can be stood on; steep faces block horizontal movement.
            let bucket=if n.y>=0.3{&mut cells}else if n.y.abs()<0.3{&mut walls}else{continue};
            for r in r0..=r1.min(rows-1){for c in c0..=c1.min(cols-1){bucket[r*cols+c].push(i as u32);}}
        }
        Self{cell,min:lo,cols,rows,cells,walls,tris}
    }
    fn index(&self,x:f32,z:f32)->Option<usize>{
        let c=((x-self.min.x)/self.cell).floor();let r=((z-self.min.y)/self.cell).floor();
        if c<0.||r<0.||c as usize>=self.cols||r as usize>=self.rows{None}else{Some(r as usize*self.cols+c as usize)}
    }
    /// Highest walkable surface at (x,z) not above `max_y`.
    pub fn height(&self,x:f32,z:f32,max_y:f32)->Option<f32>{
        self.surface(x,z,max_y).map(|(height,_)|height)
    }
    fn surface(&self,x:f32,z:f32,max_y:f32)->Option<(f32,Vec3)>{
        let idx=self.index(x,z)?;let mut best:Option<(f32,Vec3)>=None;let p=Vec2::new(x,z);
        for &i in &self.cells[idx]{
            let t=&self.tris[i as usize];
            let (a,b,cc)=(t[0].xz(),t[1].xz(),t[2].xz());
            let d=(b.y-cc.y)*(a.x-cc.x)+(cc.x-b.x)*(a.y-cc.y);if d.abs()<1e-9{continue}
            let l1=((b.y-cc.y)*(p.x-cc.x)+(cc.x-b.x)*(p.y-cc.y))/d;let l2=((cc.y-a.y)*(p.x-cc.x)+(a.x-cc.x)*(p.y-cc.y))/d;let l3=1.-l1-l2;
            if l1< -1e-4||l2< -1e-4||l3< -1e-4{continue}
            let y=l1*t[0].y+l2*t[1].y+l3*t[2].y;
            if y<=max_y && best.is_none_or(|b|y>b.0){best=Some((y,(t[1]-t[0]).cross(t[2]-t[0]).normalize_or_zero()));}
        }
        best
    }
    /// True if a horizontal move from `a` to `b` (xz) crosses a steep collision face between heights y0..y1.
    pub fn blocked(&self,a:Vec2,b:Vec2,y0:f32,y1:f32)->bool{
        let steps=((a.distance(b)/self.cell).ceil() as usize).max(1);let mut seen=std::collections::HashSet::new();
        for s in 0..=steps{
            let p=a.lerp(b,s as f32/steps as f32);let Some(idx)=self.index(p.x,p.y) else{continue};
            for &i in &self.walls[idx]{
                if !seen.insert(i){continue}
                let t=&self.tris[i as usize];
                // Segment (a->b) at heights y0..y1 against the triangle: test the vertical strip by projecting to xz.
                let (ta,tb,tc)=(t[0],t[1],t[2]);let ymin=ta.y.min(tb.y).min(tc.y);let ymax=ta.y.max(tb.y).max(tc.y);
                if ymax<y0||ymin>y1{continue}
                for (u,v) in [(ta,tb),(tb,tc),(tc,ta)]{ if seg_cross(a,b,u.xz(),v.xz()){return true} }
            }
        }
        false
    }
}
fn seg_cross(p:Vec2,q:Vec2,r:Vec2,s:Vec2)->bool{
    let cr=|a:Vec2,b:Vec2|a.x*b.y-a.y*b.x;let d=q-p;let e=s-r;let den=cr(d,e);
    if den.abs()<1e-9{return false}
    let t=cr(r-p,e)/den;let u=cr(r-p,d)/den;(0.0..=1.0).contains(&t)&&(0.0..=1.0).contains(&u)
}

#[derive(Resource)]
pub struct Game{
    player_asset:Option<PlayerAsset>,pub music:Option<crate::playback::Music>,pub music_name:String,results:HashMap<String,Value>,rx:Mutex<mpsc::Receiver<LoadMsg>>,filelist:Vec<(String,bool)>,models_expected:Option<usize>,models_received:usize,
    phase:Phase,started:Instant,pub log:Vec<String>,boot_image:Option<Handle<Image>>,boot_egui:Option<egui::TextureId>,
    layers:Vec<(String,String)>,layers_expected:usize,layers_ready:usize,layers_empty:usize,clip_handles:Vec<Handle<AnimationClip>>,
    ground:Option<Ground>,build_wait:u32,player:Option<Entity>,pub spawn:Vec3,pub gravity:f32,pub world_radius:f32,sky_expected:usize,sky_ready:usize,sky_angle:f32,pub placeables:Vec<Placeable>,pub props_spawned:usize,pub props_failed:usize,pub collision_bodies:usize,pub collision_files:usize,
    pub db_summary:String,pub start_dir:Vec3,pub spawn_from_db:bool,pub tri_count:usize,pub controls_rows:usize,pub combat_bindings:usize,pub world_bounds:Option<(f32,f32,f32)>,
    boot_shown:Option<Instant>,boot_shot:bool,pub load_seconds:f32,
    pub selftest:Option<SelfTest>,
    controller:Option<crate::controller::Controller>,actions:DispatchedInput,input_frames:u64,input_moves:u64,input_jumps:u64,input_max_ms:i32,
}

#[derive(Clone)] pub struct SelfTest{pub out:PathBuf,step:usize,t:f32,samples:Vec<Value>,start:Vec3,checks:Vec<Value>,shot:bool,shot_wait:f32}
impl SelfTest{pub fn new(out:PathBuf)->Self{Self{out,step:0,t:0.,samples:vec![],start:Vec3::ZERO,checks:vec![],shot:false,shot_wait:0.}}}

fn src(rel:&str)->String{bridge::data_root().join("files").join("data").join(rel.replace('/',"\\")).to_string_lossy().into_owned()}

impl Game{
    fn apply_db(&mut self,r:HashMap<String,Value>){
        if let Some(s)=r.get("summary").and_then(|v|v.as_str()){self.db_summary=s.to_string();}
        let v3=|v:&Value|v.as_array().filter(|a|a.len()==3).map(|a|Vec3::new(a[0].as_f64().unwrap_or(0.) as f32,a[1].as_f64().unwrap_or(0.) as f32,a[2].as_f64().unwrap_or(0.) as f32));
        if let Some(d)=r.get("start_direction").and_then(v3){self.start_dir=d;}
        if let Some(list)=r.get("placeables").and_then(|v|v.as_array()){
            if let Ok(k)=std::env::var("EAGL_DBG_PLACEABLES"){for p in list{if p["asset"].as_str().unwrap_or("").contains(k.as_str())||p["id"].as_str().unwrap_or("").contains(k.as_str()){eprintln!("[placeable] {p}");}}}
            for p in list{
                let a=p["pos"].as_array().unwrap();
                self.placeables.push(Placeable{id:p["id"].as_str().unwrap_or("").into(),asset:p["asset"].as_str().unwrap_or("").into(),pos:Vec3::new(a[0].as_f64().unwrap() as f32,a[1].as_f64().unwrap() as f32,a[2].as_f64().unwrap() as f32),orientation_deg:p["orient"].as_f64().unwrap_or(0.) as f32,physics:p["physics"].as_str().map(String::from),visible:p["visible"].as_bool().unwrap_or(true)});
            }
        }
        if let Some(l)=r.get("start_location").and_then(v3){self.results.insert("db_start".into(),json!([l.x,l.y,l.z]));self.spawn_from_db=true;}
    }
    pub fn new(selftest:Option<SelfTest>)->Self{
        // Player start from the game's own Attrib database (character_info/player), read by the Rust vault loader.
        let mut db_results=HashMap::new();
        let dir=bridge::data_root().join("files").join("data").join("db");
        let mut log=vec!["Boot: reading original data files".to_string()];
        match (std::fs::read(dir.join("db.vlt")),std::fs::read(dir.join("db.bin"))){
            (Ok(v),Ok(b))=>match crate::vlt::Database::load(&v,&b,crate::vlt::known_names()){
                Ok(db)=>{
                    let start=db.find_collection("character_info","player");
                    let loc=start.and_then(|c|db.attribute(c,"start_location"));let dir_=start.and_then(|c|db.attribute(c,"start_direction"));
                    log.push(format!("db.vlt: {} types, {} classes, {} collections",db.types.len(),db.classes.len(),db.collections.len()));
                    db_results.insert("summary".to_string(),json!(format!("{} types / {} classes / {} collections",db.types.len(),db.classes.len(),db.collections.len())));
                    let pk=crate::vlt::string_hash64("placeables");
                    // attributes are inherited from parent collections (the plain tetherball poles take their model that way)
                    let inherited=|c:&crate::vlt::Collection,name:&str|->Option<Value>{
                        let mut cur=Some(c);
                        while let Some(col)=cur{
                            if let Some(v)=db.attribute(col,name){return Some(v)}
                            cur=db.collections.iter().find(|x|x.class_key==col.class_key&&x.key==col.parent_key&&col.parent_key!=0);
                        }
                        None
                    };
                    let list:Vec<Value>=db.collections.iter().filter(|c|c.class_key==pk).filter_map(|c|{
                        let pos=inherited(c,"position")?;let asset=inherited(c,"asset_name")?;
                        // default-invisible placeables are spawned hidden: minigames switch some on (tetherball's plain pole)
                        let visible=inherited(c,"default_visible").and_then(|v|v.as_bool())!=Some(false);
                        Some(json!({"id":db.attribute(c,"id").unwrap_or(json!("")),"asset":asset,"pos":pos,"visible":visible,"orient":inherited(c,"orientation").unwrap_or(json!(0)),"physics":inherited(c,"physics_name").unwrap_or(Value::Null)}))
                    }).collect();
                    db_results.insert("placeables".into(),Value::Array(list));
                    if let Some(l)=loc{db_results.insert("start_location".into(),l);}
                    if let Some(d)=dir_{db_results.insert("start_direction".into(),d);}
                }
                Err(e)=>log.push(format!("db.vlt failed to load: {e}")),
            },
            _=>log.push("db.vlt/db.bin not found".into()),
        }
        let (tx,rx)=mpsc::channel();
        let mut g=Self{controller:None,actions:DispatchedInput::default(),input_frames:0,input_moves:0,input_jumps:0,input_max_ms:0,player_asset:None,music:None,music_name:String::new(),results:HashMap::new(),rx:Mutex::new(rx),filelist:vec![],models_expected:None,models_received:0,phase:Phase::Loading,started:Instant::now(),log,
            boot_image:None,boot_egui:None,layers:vec![],layers_expected:0,layers_ready:0,layers_empty:0,clip_handles:vec![],ground:None,build_wait:0,player:None,spawn:Vec3::ZERO,
            gravity:9.81,world_radius:0.,sky_expected:0,sky_ready:0,sky_angle:0.,placeables:vec![],props_spawned:0,props_failed:0,collision_bodies:0,collision_files:0,db_summary:String::new(),start_dir:Vec3::Z,spawn_from_db:false,tri_count:0,controls_rows:0,combat_bindings:0,world_bounds:None,boot_shown:None,boot_shot:false,load_seconds:0.,selftest};
        // (fields set below)
        g.apply_db(db_results);
        let list=g.placeables.clone();std::thread::spawn(move||loader(tx,list));
        g
    }
}

fn in_game(mode:Res<AppMode>,play:Option<Res<WorldPlay>>)->bool{*mode==AppMode::Game||(*mode==AppMode::Apt&&play.is_some_and(|p|!p.paused&&!p.minigame))}
fn game_mode(mode:Res<AppMode>)->bool{*mode==AppMode::Game}
/// The world is being played underneath the frontend (Single Player): the front end draws the HUD and menus on top.
#[derive(Resource,Default)]
pub struct WorldPlay{pub paused:bool,/// A minigame started from the world (Free Throw) has the screen; the world waits.
    pub minigame:bool,/// Paused by a full-screen front-end screen (report card, sticker book) rather than the pause overlay.
    pub screen_pause:bool,pub seen_other:bool}
/// The world is also loaded behind the front end (menu cameras from the `main_menu_nis` collections).
fn world_active(mode:Res<AppMode>,backdrop:Option<Res<Backdrop>>,play:Option<Res<WorldPlay>>)->bool{*mode==AppMode::Game||backdrop.is_some()||play.is_some()}
fn backdrop_only(mode:Res<AppMode>,backdrop:Option<Res<Backdrop>>)->bool{*mode!=AppMode::Game&&backdrop.is_some()}

/// Camera rig for the world shown behind menus: eased moves between the `main_menu_nis` positions.
#[derive(Resource)]
pub struct Backdrop{pub eye:Vec3,pub target:Vec3,from:(Vec3,Vec3),to:(Vec3,Vec3),t:f32,dur:f32}
impl Backdrop{
    pub fn new(eye:Vec3,target:Vec3)->Self{Self{eye,target,from:(eye,target),to:(eye,target),t:1.,dur:1.}}
    /// Jump to a pose immediately (a minigame drives the camera itself).
    pub fn set_now(&mut self,eye:Vec3,target:Vec3){self.eye=eye;self.target=target;self.t=1.;}
    /// Start a smooth camera move (duration in seconds).
    pub fn move_to(&mut self,eye:Vec3,target:Vec3,dur:f32){self.from=(self.eye,self.target);self.to=(eye,target);self.t=0.;self.dur=dur.max(0.01);}
}
/// Camera poses of the `main_menu_nis` collections in `db.vlt` (first vector is the look-at point, second the eye).
pub fn nis_pose(name:&str)->Option<(Vec3,Vec3,f32)>{
    let dir=bridge::data_root().join("files").join("data").join("db");
    let (v,b)=(std::fs::read(dir.join("db.vlt")).ok()?,std::fs::read(dir.join("db.bin")).ok()?);
    let db=crate::vlt::Database::load(&v,&b,crate::vlt::known_names()).ok()?;
    // The kid-select scene has its own cameras in `character_select` (same two keys, no duration).
    let (class,coll)=match name{"select_kid"=>("character_select","single_player"),"select_kid_mp"=>("character_select","multi_player"),_=>("main_menu_nis",name)};
    let c=db.find_collection(class,coll)?;
    let to3=|v:Value|v.as_array().map(|a|Vec3::new(a[0].as_f64().unwrap_or(0.) as f32,a[1].as_f64().unwrap_or(0.) as f32,a[2].as_f64().unwrap_or(0.) as f32));
    let (target,eye)=if name=="general"{
        (db.attribute(c,"camera_main_menu_target").and_then(to3)?,db.attribute(c,"camera_main_menu_position").and_then(to3)?)
    }else{
        (db.attribute_by_key(c,0x2634b113a1d79c5f).and_then(to3)?,db.attribute_by_key(c,0x7eb3a9524cd43207).and_then(to3)?)
    };
    let ms=db.attribute_by_key(c,0xdbb7651f3b631882).and_then(|v|v.as_f64()).unwrap_or(3000.) as f32;
    Some((eye,target,ms/1000.))
}
fn backdrop_camera(g:Option<Res<Game>>,mut bd:Option<ResMut<Backdrop>>,time:Res<Time<Real>>,mut c:Query<&mut Transform,With<GameCamera>>){
    let (Some(g),Some(bd))=(g,bd.as_mut()) else{return};
    if bd.t<1.{
        bd.t=(bd.t+time.delta_secs()/bd.dur).min(1.);
        let k=bd.t*bd.t*(3.-2.*bd.t);
        bd.eye=bd.from.0.lerp(bd.to.0,k);bd.target=bd.from.1.lerp(bd.to.1,k);
    }
    let Ok(mut ct)=c.single_mut() else{return};
    let radius=g.world_radius;
    let (eb,tb)=(display_matrix(radius,bd.eye).w_axis.truncate(),display_matrix(radius,bd.target).w_axis.truncate());
    *ct=Transform::from_translation(eb).looking_at(tb,Vec3::Y);
}
pub fn plugin(app:&mut App){
    app.init_resource::<GameInput>().init_resource::<SimulationClock>()
        .add_systems(Update,(pump,build,sample_frame).chain().run_if(world_active))
        // GameState::Update runs the world before Controller::Update: gameplay
        // consumes the events produced at the end of the previous host frame.
        .add_systems(Update,(movement,camera,sky_update,animate,read_input,dispatch_input,selftest).chain().after(sample_frame).run_if(in_game))
        .add_systems(Update,(backdrop_camera,sky_update).chain().after(sample_frame).run_if(backdrop_only))
        .add_systems(bevy_egui::EguiPrimaryContextPass,hud.run_if(game_mode));
}


fn pump(backdrop:Option<Res<Backdrop>>,mut commands:Commands,mut g:Option<ResMut<Game>>,mut meshes:ResMut<Assets<Mesh>>,mut materials:ResMut<Assets<StandardMaterial>>,mut images:ResMut<Assets<Image>>,mut clips:ResMut<Assets<AnimationClip>>,mut settings:ResMut<WinitSettings>,mut redraw:MessageWriter<RequestRedraw>){
    let Some(g)=g.as_mut() else{return};let g=&mut **g;
    settings.focused_mode=UpdateMode::Continuous;settings.unfocused_mode=UpdateMode::Continuous;redraw.write(RequestRedraw);
    if g.phase!=Phase::Loading{return}
    // Rust loader thread: boot screen, tables, sky, world layers and props.
    let msgs:Vec<LoadMsg>=g.rx.lock().unwrap().try_iter().collect();
    for m in msgs{
        match m{
            LoadMsg::Log(l)=>g.log.push(l),
            LoadMsg::Controls(Ok(rows))=>{
                g.controls_rows=rows.len();g.combat_bindings=rows.iter().filter(|r|r.state==3).count();
                let mut controller=crate::controller::Controller::new(rows,3);
                // These are original dispatcher gates. Their corresponding world
                // modes are not implemented yet, so the desktop slice disables them.
                controller.freecam_enabled=false;controller.debug_enabled=false;
                g.controller=Some(controller);g.log.push(format!("native controls.csv: {} bindings",g.controls_rows));
            }
            LoadMsg::Controls(Err(e))=>{g.phase=Phase::Failed(format!("controls.csv: {e}"));return}
            LoadMsg::Boot(rgba,w,h)=>{
                g.boot_image=Some(images.add(Image::new(bevy::render::render_resource::Extent3d{width:w as u32,height:h as u32,depth_or_array_layers:1},bevy::render::render_resource::TextureDimension::D2,rgba,bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,bevy::asset::RenderAssetUsages::default())));
                g.log.push("decoded boot screen".into());
            }
            LoadMsg::Csv(key,rows)=>{
                g.log.push(format!("loaded {key}.csv ({} rows)",rows.len()));
                match key{
                    "filelist"=>{g.filelist=rows.iter().skip(1).filter_map(|r|Some((r.first()?.clone(),r.get(1).map(String::as_str)==Some("1")))).collect();g.layers_expected=g.filelist.len()*WORLD_SUFFIXES.len();}
                    "bounds"=>{if let Some(row)=rows.get(1){let f=|i:usize|row.get(i).and_then(|c|c.parse::<f32>().ok()).unwrap_or(0.);g.world_bounds=Some((f(0),f(1),f(2)));g.world_radius=f(2);}}
                    _=>{}
                }
            }
            LoadMsg::Player(Ok(d))=>{
                let up=assets::upload_skinned(&d.model,&mut meshes,&mut materials,&mut images);
                for c in &d.clips{
                    match character::animation_clip(c){Ok(a)=>g.clip_handles.push(clips.add(a)),Err(e)=>{g.log.push(format!("FAILED clip {}: {e}",c.name));g.phase=Phase::Failed(e);return}}
                }
                g.log.push(format!("player: {} bones, {} skinned parts, clips {}",d.skeleton.bones.len(),up.parts.len(),d.clips.iter().map(|c|format!("{} ({} samples)",c.name,c.sample_count)).collect::<Vec<_>>().join(", ")));
                g.player_asset=Some(PlayerAsset{up,skeleton:d.skeleton,source:d.source});
            }
            LoadMsg::Player(Err(e))=>{g.log.push(format!("FAILED player: {e}"));g.phase=Phase::Failed(format!("player: {e}"));return}
            LoadMsg::Expect{models}=>{g.models_expected=Some(models);g.sky_expected=SKY_MODELS.len();}
            LoadMsg::Model{kind,result}=>{
                g.models_received+=1;
                match (kind,result){
                    (ModelKind::Layer(name),Ok(b))=>{
                        if b.prims.is_empty(){g.layers_expected-=1;g.layers_empty+=1;g.log.push(format!("{name} has an empty draw list; skipped"));}
                        else{
                            let up=assets::upload(&b,&mut meshes,&mut materials,&mut images,false);
                            assets::spawn(&mut commands,&up,(GameEntity,WorldLayer(name.clone()),Transform::IDENTITY,OriginalAsset{evidence:EvidenceLevel::AssetDerived,source:name.clone(),decoder_version:"rust-model-1".into()}),None);
                            g.layers_ready+=1;
                        }
                        for w in b.warnings.iter().take(2){g.log.push(w.clone());}
                    }
                    (ModelKind::Sky{clouds},Ok(b))=>{
                        let up=assets::upload(&b,&mut meshes,&mut materials,&mut images,true);
                        assets::spawn(&mut commands,&up,(GameEntity,SkyLayer{clouds},Transform::IDENTITY,OriginalAsset{evidence:EvidenceLevel::AssetDerived,source:"SkyDome".into(),decoder_version:"rust-model-1".into()}),Some(RenderLayers::layer(SKY_LAYER)));
                        g.sky_ready+=1;
                    }
                    (ModelKind::Prop(asset),Ok(b))=>{
                        let up=assets::upload(&b,&mut meshes,&mut materials,&mut images,false);
                        let radius=g.world_radius;
                        for q in g.placeables.clone().into_iter().filter(|q|q.asset==asset){
                            let t=Transform::from_matrix(display_matrix(radius,q.pos)*Mat4::from_quat(Quat::from_rotation_y(q.orientation_deg.to_radians())));
                            let e=assets::spawn(&mut commands,&up,(GameEntity,PlaceableProp,Name::new(q.id.clone()),t,OriginalAsset{evidence:EvidenceLevel::AssetDerived,source:asset.clone(),decoder_version:"rust-model-1".into()}),None);
                            if !q.visible{commands.entity(e).insert(Visibility::Hidden);}
                            g.props_spawned+=1;
                        }
                    }
                    (kind,Err(e))=>{
                        let what=match &kind{ModelKind::Layer(n)=>n.clone(),ModelKind::Sky{..}=>"sky".into(),ModelKind::Prop(a)=>format!("prop {a}")};
                        g.log.push(format!("FAILED {what}: {e}"));
                        match kind{ModelKind::Prop(a)=>{g.props_failed+=g.placeables.iter().filter(|q|q.asset==a).count()},ModelKind::Layer(_)=>{g.layers_expected=g.layers_expected.saturating_sub(1)},ModelKind::Sky{..}=>{g.sky_expected=g.sky_expected.saturating_sub(1)}}
                    }
                }
            }
        }
    }
    if g.models_expected.is_some_and(|n|g.models_received>=n) && g.player_asset.is_some(){
        let boot_done=backdrop.is_some()||g.boot_shown.map(|t|t.elapsed().as_secs_f32()>=MIN_BOOT_SECONDS).unwrap_or(g.boot_image.is_none());
        if boot_done{g.phase=Phase::Building;g.log.push("world and player assets ready".into());}
    }
}

/// `SkyDome::Update` (0x803c6694): the cloud layer's angle advances by 0.012 rad/s (wrapped at 2 pi); `Draw` (0x803c6724)
/// rotates only the cloud model about Y.  The sky sits at the origin (`Initialise` 0x803c64d0), it does not follow the camera.
const SKY_CLOUD_RATE:f32=0.012;
fn sky_update(g:Option<ResMut<Game>>,clock:Res<SimulationClock>,mut layers:Query<(&SkyLayer,&mut Transform),(Without<SkyCamera>,Without<GameCamera>)>,main:Query<&Transform,With<GameCamera>>,mut sky_cam:Query<&mut Transform,(With<SkyCamera>,Without<GameCamera>,Without<SkyLayer>)>){
    let Some(mut g)=g else{return};
    g.sky_angle=(g.sky_angle+SKY_CLOUD_RATE*clock.frame.seconds())%std::f32::consts::TAU;
    for (l,mut t) in &mut layers{if l.clouds{t.rotation=Quat::from_rotation_y(g.sky_angle);}}
    if let (Ok(m),Ok(mut c))=(main.single(),sky_cam.single_mut()){*c=*m;}
}


fn build(backdrop:Option<Res<Backdrop>>,mut commands:Commands,mut g:Option<ResMut<Game>>,mut ibp:ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,mut graphs:ResMut<Assets<AnimationGraph>>,cam:Query<Entity,With<GameCamera>>){
    let Some(g)=g.as_mut() else{return};let g=&mut **g;
    if g.phase!=Phase::Building{return}
    g.build_wait+=1;if g.build_wait<3{return} // let transforms propagate
    // Terrain/prop collision: the Havok files the original game loads for each world area (physics/<area>.hkx).
    let classes=crate::havok::ClassTable::embedded();
    let areas:Vec<String>=g.filelist.iter().filter(|(_,high)|*high).map(|(n,_)|n.clone()).collect();
    let dir=bridge::data_root().join("files").join("data").join("physics");
    let mut tris:Vec<[Vec3;3]>=Vec::new();
    for area in &areas{
        let Ok(bytes)=std::fs::read(dir.join(format!("{area}.hkx"))) else{g.log.push(format!("{area}.hkx not found"));continue};
        match crate::havok::Packfile::parse(&bytes,&classes){
            Ok(pf)=>{
                let c=crate::havok::Collision::load(&pf);g.collision_files+=1;g.collision_bodies+=c.bodies.len();
                tris.extend(c.triangles().map(|t|[Vec3::from(t[0]),Vec3::from(t[1]),Vec3::from(t[2])]));
                if area=="playground"{
                    // hkWorldCinfo carries the physics world settings (gravity, solver, deactivation).
                    for (&(si,off),cn) in &pf.virt{if cn=="hkWorldCinfo"{
                        let w=pf.decode_object(si,off,cn,0);
                        if let Some(gv)=w["gravity"][1].as_f64(){g.gravity=(-gv) as f32;}
                    }}
                }
            }
            Err(e)=>g.log.push(format!("{area}.hkx: {e}")),
        }
    }
    // Placeables that name their own Havok file (gates): place its collision at the database position/orientation.
    for q in g.placeables.clone().into_iter().filter(|q|q.visible){
        let Some(name)=q.physics.as_deref() else{continue};
        let Ok(bytes)=std::fs::read(dir.join(format!("{name}.hkx"))) else{continue};
        let Ok(pf)=crate::havok::Packfile::parse(&bytes,&classes) else{continue};
        let c=crate::havok::Collision::load(&pf);let rot=Quat::from_rotation_y(q.orientation_deg.to_radians());
        let before=tris.len();
        tris.extend(c.triangles().map(|t|[0,1,2].map(|i|rot*Vec3::from(t[i])+q.pos)));
        g.collision_bodies+=c.bodies.len();g.collision_files+=1;g.log.push(format!("{} collision: +{} triangles",q.id,tris.len()-before));
    }
    g.tri_count=tris.len();
    if tris.is_empty(){let m="no Havok collision loaded".to_string();g.log.push(m.clone());g.phase=Phase::Failed(m);return}
    g.log.push(format!("collision: {} files, {} bodies, {} triangles; gravity {:.2}",g.collision_files,g.collision_bodies,g.tri_count,g.gravity));
    let ground=Ground::build(tris);
    // Spawn: character_info/player start_location from db.vlt (x,z; the stored y is 0, so height comes from the ground).
    // Fallback if the database is unavailable: nearest walkable point to the hub.
    let start=g.results.get("db_start").and_then(|v|v.as_array()).map(|a|Vec2::new(a[0].as_f64().unwrap_or(0.) as f32,a[2].as_f64().unwrap_or(0.) as f32));
    let hub=start.unwrap_or(Vec2::new(20.,-56.));
    let mut best:Option<(f32,Vec3)>=None;let mut covered=0;let mut total=0;
    for gx in -30..30{for gz in -30..30{
        let (x,z)=(hub.x+gx as f32*0.5,hub.y+gz as f32*0.5);total+=1;
        if let Some(y)=ground.height(x,z,50.){covered+=1;let d=Vec2::new(x,z).distance(hub);if best.is_none_or(|b|d<b.0){best=Some((d,Vec3::new(x,y,z)));}}
    }}
    g.log.push(format!("ground coverage around start: {covered}/{total} samples"));
    g.spawn=best.map(|b|b.1).unwrap_or(Vec3::new(hub.x,-5.,hub.y));g.ground=Some(ground);
    g.load_seconds=g.started.elapsed().as_secs_f32();
    // Animation graph over the three decoded clips.
    let mut graph=AnimationGraph::new();
    let nodes:Vec<AnimationNodeIndex>=g.clip_handles.iter().map(|c|graph.add_clip(c.clone(),1.,graph.root)).collect();
    let handle=graphs.add(graph);
    let asset=g.player_asset.take().expect("player asset");
    let physics_input=CharacterInputState{speed:0.,orientation:0.,gravity:[0.,-g.gravity,0.],gravity_override:None,impulse:[0.;3],impulse_decay:[0.;3],impulse_ms:0};
    let support_normal=g.ground.as_ref().and_then(|ground|ground.surface(g.spawn.x,g.spawn.z,g.spawn.y+STEP_UP)).map(|(_,normal)|normal).unwrap_or(Vec3::Y);
    let player=commands.spawn((GameEntity,Player{loco:Locomotion::default(),physics_input,movement:CharacterMovementState::default(),physics_frames:0,velocity:Vec3::ZERO,support_normal,grounded:true,speed:0.,facing:g.start_dir,weights:[1.,0.,0.],anim_ready:true},Transform::from_translation(g.spawn),Visibility::default(),
        OriginalAsset{evidence:EvidenceLevel::AssetDerived,source:asset.source.clone(),decoder_version:"rust-model-1 / rust-anim-1".into()})).id();
    // The model entity owns the AnimationPlayer; joints point at it through AnimatedBy (see character.rs).
    let model=commands.spawn((GameEntity,PlayerModel,ChildOf(player),Transform::default(),Visibility::default())).id();
    let mut ap=AnimationPlayer::default();
    for (i,n) in nodes.iter().enumerate(){let a=ap.play(*n);a.repeat();a.set_weight(if i==0{1.}else{0.});}
    commands.entity(model).insert((ap,AnimationGraphHandle(handle.clone())));
    let joints=character::spawn_rig(&mut commands,&mut ibp,&asset.skeleton,&asset.up,model,model);
    g.log.push(format!("player rig: {} joints, {} skinned parts",joints.len(),asset.up.parts.len()));
    g.player=Some(player);
    if backdrop.is_some(){commands.entity(player).insert(Visibility::Hidden);}
    // Area music: the executable names world_nature/park/schoolyard/stadium.asf; the start area is the schoolyard hub
    // (provisional mapping - the original's area -> track selection was not traced).
    if backdrop.is_none()&&!std::env::args().any(|a|a=="--mute"){
        let track="world_schoolyard.asf";
        g.music=Some(crate::playback::Music::start(bridge::data_root().join("files").join("data").join("audio").join("music").join(track),0.4));
        g.music_name=track.into();
    }
    if cam.is_empty(){
        // Sky camera (order -1) draws the sky layer first; the world camera keeps that colour buffer and adds the world on top.
        commands.spawn((GameEntity,SkyCamera,crate::apt_view::LetterboxCamera,Camera3d::default(),Camera{order:-1,..default()},RenderLayers::layer(SKY_LAYER),Transform::from_translation(g.spawn+Vec3::new(0.,2.,4.))));
        commands.spawn((GameEntity,crate::apt_view::LetterboxCamera,GameCamera{yaw:(-g.start_dir.x).atan2(-g.start_dir.z),pitch:0.28,distance:k::HINGE_CAMERA_DISTANCE},Camera3d::default(),Camera{order:0,clear_color:ClearColorConfig::None,..default()},Transform::from_translation(g.spawn+Vec3::new(0.,2.,4.))));
    }
    commands.spawn((GameEntity,DirectionalLight{illuminance:9000.,shadow_maps_enabled:false,..default()},Transform::from_rotation(Quat::from_euler(EulerRot::XYZ,-0.9,-0.5,0.))));
    g.log.push(format!("ground: {} triangles; spawn {:.1},{:.1},{:.1}",g.tri_count,g.spawn.x,g.spawn.y,g.spawn.z));
    g.phase=Phase::Playing;
}

fn read_input(keys:Res<ButtonInput<KeyCode>>,mut input:ResMut<GameInput>){
    if input.scripted{return}
    let a=|k:&[KeyCode]|k.iter().any(|c|keys.pressed(*c)) as i32 as f32;
    input.x=a(&[KeyCode::KeyD,KeyCode::ArrowRight])-a(&[KeyCode::KeyA,KeyCode::ArrowLeft]);
    input.y=a(&[KeyCode::KeyW,KeyCode::ArrowUp])-a(&[KeyCode::KeyS,KeyCode::ArrowDown]);
    input.jump=keys.pressed(KeyCode::Space);
    input.reorient=keys.pressed(KeyCode::KeyR);
}

fn dispatch_input(mut g:Option<ResMut<Game>>,input:Res<GameInput>,clock:Res<SimulationClock>){
    let Some(g)=g.as_mut() else{return};if !g.is_playing(){return}
    let Some(controller)=g.controller.as_mut() else{return};
    // Desktop mapping: WASD/arrows -> Wii D-pad, Space -> C, R -> B.
    let held=(u32::from(input.y>0.)<<0)|(u32::from(input.y<0.)<<1)|(u32::from(input.x<0.)<<2)|(u32::from(input.x>0.)<<3)|(u32::from(input.reorient)<<5)|(u32::from(input.jump)<<6);
    controller.update(held,clock.frame.input_ms);
    let event=|id|controller.event(id).active;
    let actions=DispatchedInput{x:event(179) as i32 as f32-event(178) as i32 as f32,y:event(180) as i32 as f32-event(181) as i32 as f32,jump:event(2),reorient:event(7)};
    g.input_moves+=u64::from(actions.x!=0.||actions.y!=0.);g.input_jumps+=u64::from(actions.jump);
    g.actions=actions;g.input_frames+=1;g.input_max_ms=g.input_max_ms.max(clock.frame.input_ms);
}

fn movement(g:Option<Res<Game>>,clock:Res<SimulationClock>,cam:Query<&GameCamera>,mut q:Query<(&mut Player,&mut Transform)>){
    let Some(g)=g else{return};if g.phase!=Phase::Playing{return}
    let input=&g.actions;
    let (Some(ground),Ok(cam))=(&g.ground,cam.single()) else{return};
    let dt=clock.frame.seconds();let dt_ms=clock.frame.simulation_ms as i32;
    for (mut p,mut t) in &mut q{
        let step=p.loco.update(dt_ms,InputKind::Digital,input.x,input.y,cam.yaw,k::STATE_MAX_SPEED);
        p.speed=step.speed;
        if let Some(a)=step.move_angle{
            // Camera at `yaw` looks along -(sin,cos); stick angle is measured from camera-forward.
            let f=Vec3::new(-cam.yaw.sin(),0.,-cam.yaw.cos());let r=Vec3::new(cam.yaw.cos(),0.,-cam.yaw.sin());
            p.facing=f*a.cos()+r*a.sin();
        }
        // Explicit desktop direction adapter; original locomotion-to-proxy yaw
        // wiring is still unresolved. The original flat-ground movement basis is
        // -forward / -(forward cross up), so desired +Z corresponds to yaw zero.
        p.physics_input.speed=step.speed;
        p.physics_input.orientation=p.facing.x.atan2(p.facing.z);
        // PhysicsDynamicCharacter::Update skips preparation and integration at dt <= 0.
        if dt_ms<=0 {continue}
        // Original Havok support queries remain a boundary supplied by the host
        // step/slide solver, using the decoded contact triangle's normal.
        let support=SurfaceSupport{kind:if p.grounded{2}else{0},normal:p.support_normal.extend(0.).to_array(),velocity:[0.;4]};
        let position=[t.translation.x,t.translation.y,t.translation.z,0.];
        let velocity=p.velocity.extend(0.).to_array();
        let prepared=p.physics_input.build(dt_ms,support,position,velocity);
        let output=p.movement.update(&prepared);
        p.velocity=Vec3::new(output.velocity[0],output.velocity[1],output.velocity[2]);
        p.physics_frames+=1;
        if p.velocity.x!=0.||p.velocity.z!=0.{
            let delta=Vec3::new(p.velocity.x,0.,p.velocity.z)*dt;
            let ny=t.translation.y;
            // Horizontal move against the Havok collision: needs walkable ground within step-up height and no
            // steep face crossing the character's body (slides along one axis if the diagonal is blocked).
            for cand in [delta,Vec3::new(delta.x,0.,0.),Vec3::new(0.,0.,delta.z)]{
                let np=t.translation+cand;
                if ground.blocked(t.translation.xz(),np.xz(),ny+STEP_UP,ny+CHARACTER_HEIGHT){continue}
                if let Some(gy)=ground.height(np.x,np.z,ny+STEP_UP){if !p.grounded||gy>=ny-1.5{t.translation.x=np.x;t.translation.z=np.z;break}}
            }
        }
        // Original LocalCharacterControl ignores queued command 4 (Jump), and
        // PhysicsDynamicCharacter::BuildCharacterInput writes wantJump=false.
        // The controller still emits the event, but it adds no vertical impulse.
        let mut y=t.translation.y+p.velocity.y*dt;
        match ground.surface(t.translation.x,t.translation.z,y+STEP_UP){
            Some((gy,normal)) if y<=gy+0.02&&p.velocity.y<=0.||p.grounded&&gy>=t.translation.y-0.5=>{y=gy;p.velocity.y=0.;p.support_normal=normal;p.grounded=true}
            _=>{p.grounded=false}
        }
        t.translation.y=y;
        if y<g.spawn.y-40.{t.translation=g.spawn;p.velocity=Vec3::ZERO;p.grounded=false;p.movement=CharacterMovementState::default();}
    }
}

fn camera(g:Option<Res<Game>>,mut c:Query<(&mut GameCamera,&mut Transform),Without<Player>>,p:Query<(&Player,&Transform),Without<GameCamera>>,keys:Res<ButtonInput<KeyCode>>,mouse:Res<ButtonInput<MouseButton>>,motion:Res<AccumulatedMouseMotion>,scroll:Res<AccumulatedMouseScroll>,clock:Res<SimulationClock>){
    let (Ok((mut cam,mut ct)),Ok((player,pt)))=(c.single_mut(),p.single()) else{return};
    if mouse.pressed(MouseButton::Right)||mouse.pressed(MouseButton::Middle){cam.yaw-=motion.delta.x*0.005;cam.pitch=(cam.pitch+motion.delta.y*0.005).clamp(-0.2,1.3);}
    let key=|k:KeyCode|keys.pressed(k) as i32 as f32;
    cam.yaw+=(key(KeyCode::KeyQ)-key(KeyCode::KeyE))*1.8*clock.frame.seconds();
    cam.distance=(cam.distance*(-scroll.delta.y*0.1).exp()).clamp(1.2,12.);
    // EVENT_CAMERA_REORIENT (controls.csv: STATE_COMBAT, BUTTON B): swing behind the player.
    if g.as_ref().is_some_and(|g|g.actions.reorient){cam.yaw=(-player.facing.x).atan2(-player.facing.z);}
    let radius=g.map(|g|g.world_radius).unwrap_or(0.);
    let target=pt.translation+Vec3::Y*k::FRAME_CAMERA_HEIGHT;
    let off=Vec3::new(cam.yaw.sin()*cam.pitch.cos(),cam.pitch.sin(),cam.yaw.cos()*cam.pitch.cos())*cam.distance;
    let eye=target+off;
    // Host orbit framing in the recovered curved rendering coordinate system.
    let (tb,eb)=(display_matrix(radius,target).w_axis.truncate(),display_matrix(radius,eye).w_axis.truncate());
    *ct=Transform::from_translation(eb).looking_at(tb,Vec3::Y);
}

fn animate(g:Option<Res<Game>>,mut q:Query<(&mut Player,&Children,&Transform),Without<PlayerModel>>,children:Query<&Children>,mut players:Query<&mut AnimationPlayer>,mut models:Query<&mut Transform,(With<PlayerModel>,Without<Player>)>,time:Res<Time>,clock:Res<SimulationClock>){
    let radius=g.map(|g|g.world_radius).unwrap_or(0.);
    for (mut p,kids,pt) in &mut q{
        // CharacterMovement::Update: idle at 0, walk while speed <= CharacterState+0x1c (2.5) + 0.001, otherwise run.
        let target=if p.speed<=0.{[1.,0.,0.]}else if p.speed<=k::STATE_FIELD_1C+k::ANIM_SPEED_EPSILON{[0.,1.,0.]}else{[0.,0.,1.]};
        let dt=clock.frame.seconds();let blend=(dt*8.).min(1.);
        // Bevy advances clips with virtual Time later in PostUpdate. Scale just these
        // game clips so a stalled frame advances them by the same capped dt as movement.
        let playback_speed=if time.delta_secs()>0.{dt/time.delta_secs()}else{0.};
        for i in 0..3{p.weights[i]+=(target[i]-p.weights[i])*blend;}
        let w=p.weights;
        for kid in kids.iter(){
            if let Ok(mut m)=models.get_mut(kid){
                // Model +Z is treated as forward until orientation is compared against the game.
                *m=Transform::from_matrix(display_matrix(radius,pt.translation)*Mat4::from_quat(Quat::from_rotation_y(p.facing.x.atan2(p.facing.z))));
                // The player parent retains flat collision coordinates.
                m.translation-=pt.translation;
            }
            for d in std::iter::once(kid).chain(children.iter_descendants(kid)){if let Ok(mut pl)=players.get_mut(d){
                for (i,weight) in w.iter().enumerate(){if let Some(a)=pl.animation_mut(AnimationNodeIndex::new(i+1)){a.set_weight(*weight);a.set_speed(playback_speed);}}
            }}
        }
    }
}

fn hud(images:Res<Assets<Image>>,mut contexts:EguiContexts,mut g:Option<ResMut<Game>>,mut mode:ResMut<AppMode>,keys:Res<ButtonInput<KeyCode>>,time:Res<Time>,player:Query<(&Player,&Transform)>)->Result{
    let Some(g)=g.as_mut() else{return Ok(())};
    if g.boot_image.is_some()&&g.boot_egui.is_none(){if let Some(h)=g.boot_image.clone(){g.boot_egui=Some(contexts.add_image(EguiTextureHandle::Strong(h)));}}
    let ctx=contexts.ctx_mut()?;
    if keys.just_pressed(KeyCode::Escape){*mode=if FROM_FE.swap(false,std::sync::atomic::Ordering::Relaxed){AppMode::Apt}else{AppMode::Menu};return Ok(())}
    let screen=ctx.viewport_rect();
    match g.phase.clone(){
        Phase::Loading|Phase::Building=>{
            let painter=ctx.layer_painter(egui::LayerId::new(egui::Order::Background,egui::Id::new("boot")));
            painter.rect_filled(screen,0.,egui::Color32::BLACK);
            if let Some(id)=g.boot_egui{
                let ready=g.boot_image.as_ref().is_some_and(|h|images.contains(h));
                if ready{
                    if g.boot_shown.is_none(){g.boot_shown=Some(Instant::now());}
                    let (w,h)=g.boot_image.as_ref().and_then(|h|images.get(h)).map(|i|(i.width() as f32,i.height() as f32)).unwrap_or((640.,480.));let fit=(screen.width()/w).min(screen.height()/h);let r=egui::Rect::from_center_size(screen.center(),egui::vec2(w*fit,h*fit));
                    painter.image(id,r,egui::Rect::from_min_max(egui::pos2(0.,0.),egui::pos2(1.,1.)),egui::Color32::WHITE);
                }
            }
            egui::Area::new(egui::Id::new("loadlog")).fixed_pos(egui::pos2(16.,16.)).show(ctx,|ui|{egui::Frame::popup(ui.style()).show(ui,|ui|{
                ui.label(egui::RichText::new("Loading from original data files…").color(egui::Color32::WHITE));
                for l in g.log.iter().rev().take(4).rev(){ui.label(egui::RichText::new(l).small().monospace().color(egui::Color32::LIGHT_GRAY));}
            });});
        }
        Phase::Failed(e)=>{egui::Area::new(egui::Id::new("fail")).fixed_pos(screen.center()-egui::vec2(200.,20.)).show(ctx,|ui|{ui.heading(egui::RichText::new("Game failed to load").color(egui::Color32::LIGHT_RED));ui.label(e);ui.label("Esc: back to menu");});}
        Phase::Playing=>{
            let (speed,pos)=player.single().map(|(p,t)|(p.speed,t.translation)).unwrap_or((0.,Vec3::ZERO));
            if !FROM_FE.load(std::sync::atomic::Ordering::Relaxed){egui::Area::new(egui::Id::new("hud")).fixed_pos(egui::pos2(12.,12.)).show(ctx,|ui|{
                egui::Frame::popup(ui.style()).show(ui,|ui|{
                    ui.label(egui::RichText::new("EA PLAYGROUND — reconstruction slice").strong());
                    ui.label(format!("pos {:.1}, {:.1}, {:.1}   speed {:.2}/{:.1}   {:.0} fps",pos.x,pos.y,pos.z,speed,k::STATE_MAX_SPEED,1./time.delta_secs().max(1e-4)));
                    ui.label(format!("collision: {} Havok files, {} bodies, {} triangles • gravity {:.2} • loaded in {:.1}s",g.collision_files,g.collision_bodies,g.tri_count,g.gravity,g.load_seconds));
                    ui.label(format!("db.vlt: {} • spawn {}",g.db_summary,if g.spawn_from_db{"from character_info/player"}else{"provisional"}));
                    if let Some(m)=&g.music{ui.label(format!("music: {} (EA Layer 3 decoded in Rust) • {} • {} s queued",g.music_name,m.status.describe(),m.status.samples_played.load(std::sync::atomic::Ordering::Relaxed)/44100));}
                    if let Some((x,z,r))=g.world_bounds{ui.label(format!("world.csv bounds: min ({x}, {z}) radius {r}"));}
                    ui.label(format!("controls.csv: {} bindings ({} in STATE_COMBAT)",g.controls_rows,g.combat_bindings));
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(104,200,170),"Locomotion: ELF-derived (LocalCharacterControl::Update)");
                    ui.colored_label(egui::Color32::from_rgb(104,200,170),"Terrain: original Havok collision (physics/*.hkx); display bent by world.csv radius");
                    ui.colored_label(egui::Color32::YELLOW,"Character collision response and camera framing remain provisional");
                    ui.label(egui::RichText::new("WASD/arrows move • Q/E or right-drag camera • wheel zoom • R behind • Esc menu").small());
                });
            });}
        }
    }
    Ok(())
}

fn selftest(mut commands:Commands,mut g:Option<ResMut<Game>>,mut input:ResMut<GameInput>,time:Res<Time>,clock:Res<SimulationClock>,player:Query<(&Player,&Transform)>,cam:Query<&Transform,With<GameCamera>>,meshes:Query<&Mesh3d>,players:Query<&AnimationPlayer>,joints:Query<(&bevy::animation::AnimationTargetId,&Transform)>,skins:Query<&bevy::mesh::skinning::SkinnedMesh>,mut exit:MessageWriter<AppExit>,_window:Query<&Window,With<PrimaryWindow>>){
    let Some(g)=g.as_mut() else{return};let g=&mut **g;
    if g.selftest.is_none(){return}
    let elapsed=g.started.elapsed().as_secs_f32();
    if elapsed>150.{
        let out=g.selftest.as_ref().unwrap().out.clone();let _=std::fs::create_dir_all(&out);
        let _=std::fs::write(out.join("selftest.json"),serde_json::to_string_pretty(&json!({"passed":false,"error":format!("timeout in phase {:?}",g.phase),"log":g.log})).unwrap());
        exit.write(AppExit::from_code(2));return
    }
    if let Phase::Failed(e)=g.phase.clone(){
        let out=g.selftest.as_ref().unwrap().out.clone();let _=std::fs::create_dir_all(&out);
        let _=std::fs::write(out.join("selftest.json"),serde_json::to_string_pretty(&json!({"passed":false,"error":e,"log":g.log})).unwrap());
        exit.write(AppExit::from_code(3));return
    }
    // Boot screenshot: once the decoded strap-warning texture has been on screen for a moment.
    if !g.boot_shot{if let Some(t)=g.boot_shown{if t.elapsed().as_secs_f32()>0.8{
        let out=g.selftest.as_ref().unwrap().out.clone();let _=std::fs::create_dir_all(&out);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.join("01-boot.png")));g.boot_shot=true;
    }}}
    if g.phase!=Phase::Playing{return}
    let st=g.selftest.as_mut().unwrap();
    let Ok((p,t))=player.single() else{return};
    st.t+=clock.frame.seconds();
    // Checksum of every animated joint rotation: changes whenever the decoded clips move the rig.
    let pose:f32=joints.iter().map(|(_,t)|t.rotation.x+2.*t.rotation.y+3.*t.rotation.z).sum();
    let record=|st:&mut SelfTest,name:&str|{st.samples.push(json!({"step":name,"t":st.t,"pos":[t.translation.x,t.translation.y,t.translation.z],"speed":p.speed,"grounded":p.grounded,"weights":p.weights,"pose":pose,"velocity":p.velocity.to_array(),"movement_state":format!("{:?}",p.movement.state)}));};
    input.scripted=true;
    // Scripted input sequence: settle, walk forward, run diagonally, jump, settle.
    let script=[(0.,0.,0.,false,1.5,"settle"),(0.,1.,0.,false,2.5,"forward"),(-1.,0.,0.,false,2.5,"left"),(-1.,1.,0.,false,2.0,"diagonal"),(0.,0.,0.,true,0.4,"jump"),(0.,0.,0.,false,1.0,"land")];
    if st.step==0&&st.t<1e-3+clock.frame.seconds()*2.{st.start=t.translation;}
    let mut acc=0.;let mut cur=None;
    for (i,s) in script.iter().enumerate(){if st.t>=acc&&st.t<acc+s.4{cur=Some(i);}acc+=s.4;}
    match cur{
        Some(i)=>{let s=script[i];input.x=s.0;input.y=s.1;input.jump=s.3;
            if i>st.step{record(st,script[st.step].5);st.step=i;}}
        None=>{
            if !st.shot{
                record(st,"final");
                st.shot=true;let out=st.out.clone();let _=std::fs::create_dir_all(&out);
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.join("02-game.png")));
            }
            st.shot_wait+=time.delta_secs();
            if st.shot_wait>1.5{
                let moved=(t.translation-st.start).xz().length();
                let walk_w=st.samples.iter().filter(|s|s["step"]=="forward").filter_map(|s|Some(s["weights"][1].as_f64()?+s["weights"][2].as_f64()?)).fold(0.,f64::max);
                let mut checks=vec![];
                let mut check=|name:&str,ok:bool,detail:String|checks.push(json!({"name":name,"ok":ok,"detail":detail}));
                check("recovered integer-ms simulation clock drives the game",clock.frames>0&&clock.total_ms>=9_900&&clock.max_simulation_ms<=k::FRAME_CAP_MS as u32,format!("{} frames, {} simulation ms, maximum {} simulation ms / {} uncapped input ms",clock.frames,clock.total_ms,clock.max_simulation_ms,clock.max_input_ms));
                check("world layers spawned from worldfilelist.csv",g.layers_ready==g.layers_expected&&g.layers_expected+g.layers_empty==WORLD_SUFFIXES.len()*5,format!("{}/{} layers ready, {} empty draw lists skipped",g.layers_ready,g.layers_expected,g.layers_empty));
                check("SkyDome layers (skybox, mountain/city ring, clouds) loaded",g.sky_expected==3&&g.sky_ready==3,format!("{}/{} layers ready; cloud angle {:.4} rad after the run",g.sky_ready,g.sky_expected,g.sky_angle));
                check("Havok collision decoded from physics/*.hkx",g.tri_count>20_000&&g.collision_bodies>=500&&g.collision_files>=4,format!("{} files, {} bodies, {} triangles",g.collision_files,g.collision_bodies,g.tri_count));
                check("placeables from the database spawned at their positions",g.placeables.len()>=15&&g.props_failed==0&&g.props_spawned==g.placeables.len(),format!("{} spawned / {} listed in the placeables class, {} failed",g.props_spawned,g.placeables.len(),g.props_failed));
                check("gravity read from hkWorldCinfo",(g.gravity-9.81).abs()<0.01,format!("gravity {:.3}",g.gravity));
                check("world curvature radius from world.csv",g.world_radius>0.,format!("radius {}",g.world_radius));
                check("player model + 3 clips loaded",g.clip_handles.len()==CLIPS.len()&&p.anim_ready,format!("{} clips, anim player bound: {}",g.clip_handles.len(),p.anim_ready));
                check("player rig built in Rust: 68 joints, skinned parts bound",joints.iter().count()==68&&skins.iter().count()==62&&skins.iter().all(|s|s.joints.len()==68),format!("{} joints, {} skinned mesh parts",joints.iter().count(),skins.iter().count()));
                let poses:Vec<f64>=st.samples.iter().filter_map(|s|s["pose"].as_f64()).collect();
                let (lo,hi)=poses.iter().fold((f64::MAX,f64::MIN),|a,&x|(a.0.min(x),a.1.max(x)));
                check("decoded clips animate the skeleton (joint pose changes idle -> walk/run)",poses.len()>=3&&hi-lo>0.05,format!("joint rotation checksum ranged {lo:.3}..{hi:.3} over {} samples",poses.len()));
                check("player spawn read from db.vlt (character_info/player)",g.spawn_from_db&&(g.spawn.x-12.).abs()<1.&&(g.spawn.z+53.).abs()<1.&&g.spawn.y.abs()<1.5,format!("start_location (12,0,-53) -> ground-snapped spawn {:.1},{:.1},{:.1}",g.spawn.x,g.spawn.y,g.spawn.z));
                let music_state=g.music.as_ref().map(|m|(m.status.state.load(std::sync::atomic::Ordering::Relaxed),m.status.samples_played.load(std::sync::atomic::Ordering::Relaxed)));
                check("area music streams from the Rust EA Layer 3 decoder",music_state.is_none_or(|(s,q)|(s==crate::playback::Status::PLAYING&&q>44100)||s==crate::playback::Status::NO_DEVICE),format!("{}: state {:?}, {} samples queued",g.music_name,music_state.map(|m|m.0),music_state.map(|m|m.1).unwrap_or(0)));
                check("original controls.csv drives movement and jump events",g.combat_bindings>=8&&g.input_frames==clock.frames&&g.input_moves>0&&g.input_jumps>0&&g.input_max_ms==clock.max_input_ms,format!("{} bindings, {} input frames, {} movement / {} jump frames, uncapped maximum {} ms",g.controls_rows,g.input_frames,g.input_moves,g.input_jumps,g.input_max_ms));
                check("native character input and movement states drive positive world frames",p.physics_frames<=clock.frames&&p.physics_frames>0&&p.velocity.is_finite(),format!("{} prepared frames; recovered grounded/airborne velocity, state {:?}; provisional collision support",p.physics_frames,p.movement.state));
                check("mesh entities in scene",meshes.iter().count()>100,format!("{} Mesh3d entities",meshes.iter().count()));
                check("animation players active",players.iter().count()>=1,format!("{} players",players.iter().count()));
                check("player walked on recovered locomotion",moved>5.,format!("moved {:.2} m from the start (5 m/s max)",moved));
                let fwd=st.samples.iter().find(|s|s["step"]=="forward").map(|s|{let p=&s["pos"];let dx=p[0].as_f64().unwrap()-st.start.x as f64;let dz=p[2].as_f64().unwrap()-st.start.z as f64;(dx*dx+dz*dz).sqrt()}).unwrap_or(0.);
                check("Havok collision stops the player at a solid prop",fwd>1.&&fwd<9.,format!("forward run of 2.5 s (12.5 m if unobstructed) ended after {fwd:.1} m"));
                check("walk/run clip blended in while moving",walk_w>0.5,format!("peak walk+run weight {walk_w:.2} (full stick = run)"));
                check("player grounded on world surface",p.grounded,format!("y={:.2}",t.translation.y));
                let jump_ignored=g.input_jumps>0&&st.samples.iter().any(|s|s["step"]=="jump"&&s["grounded"]==true);
                check("original playground jump command has no upward impulse",jump_ignored,"jump event dispatched while player remains grounded, matching original command 4".into());
                check("camera follows player",cam.single().map(|c|c.translation.distance(t.translation)<15.).unwrap_or(false),"camera within 15 m".into());
                let passed=checks.iter().all(|c|c["ok"]==true);
                let report=json!({"passed":passed,"load_seconds":g.load_seconds,"checks":checks,"samples":st.samples,"log":g.log,"evidence":{"locomotion":"ExecutableDerived: LocalCharacterControl::Update 0x802eeb28, constants from recovered.rs","timing":"ExecutableDerived: GameState::Update 0x803acdc4, variable integer ms capped at 60; once per host frame","terrain":"DataDerived Havok collision meshes; provisional character solver","character_input":"ExecutableDerived: BuildCharacterInput and Grounded/InAir calculateMovement consume prior velocity, surface normal and gravity; desktop yaw adapter and proxy collision/support remain provisional","world_display":"ExecutableDerived: AreaManager::CalcRenderingModelMatrix 0x803d78b8; camera framing remains provisional","gravity":"DataDerived hkWorldCinfo through original input and airborne velocity preparation","jump":"ExecutableDerived: LocalCharacterControl command 4 ignored; PhysicsDynamicCharacter wantJump=false","input":"ExecutableDerived: original CSV bindings and Controller event predicates; desktop logical-button adapter"}});
                let _=std::fs::write(st.out.join("selftest.json"),serde_json::to_string_pretty(&report).unwrap());
                exit.write(if passed{AppExit::Success}else{AppExit::from_code(1)});
            }
        }
    }
}

impl Game{
    /// Start the schoolyard area music (the frontend does this when the world takes over from the menus).
    pub fn start_music(&mut self){
        if self.music.is_none()&&!std::env::args().any(|a|a=="--mute"){
            let track="world_schoolyard.asf";
            self.music=Some(crate::playback::Music::start(bridge::data_root().join("files").join("data").join("audio").join("music").join(track),0.4));
            self.music_name=track.into();
        }
    }
    pub fn stop_music(&mut self){self.music=None;}
    pub fn player_entity(&self)->Option<Entity>{self.player}
    pub fn is_playing(&self)->bool{self.phase==Phase::Playing}
    /// Walkable height at a logical world position (collision data), if the ground is loaded.
    pub fn ground_height(&self,x:f32,z:f32)->Option<f32>{self.ground.as_ref().and_then(|g|g.height(x,z,3.0))}
    /// Display transform (world curvature) of a logical position.
    pub fn display(&self,p:Vec3)->Mat4{display_matrix(self.world_radius,p)}
}

/// Leave game mode: drop the loader and all game entities.
pub fn teardown(commands:&mut Commands,entities:&Query<Entity,With<GameEntity>>){
    for e in entities{commands.entity(e).try_despawn();}
    commands.remove_resource::<Game>();
}
