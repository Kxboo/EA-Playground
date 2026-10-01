//! Playable Tetherball: one human (Alicia) against an AI opponent around the decoded pole.
//!
//! What is original: the pole, ball and character models/animations (decoded from the game files), the ball
//! motion arithmetic (`tetherball::BallMotion`, compared bit-for-bit with the PowerPC code), the speed, power,
//! mega, distance and rotations-to-win values and the AI's mistake chances (read from `db.vlt` through
//! `tetherball_tuning`).  What is provisional: the match flow here (hit windows, charge costs, rotation counting
//! and the AI's timing decisions) is a desktop-friendly composition, not a port of the full minigame state
//! machine.  The HUD states this.
use bevy::{animation::prelude::*,prelude::*,window::RequestRedraw,winit::{UpdateMode,WinitSettings}};
use bevy_egui::{egui,EguiContexts};
use serde_json::json;
use std::{f32::consts::{PI,TAU},sync::{Mutex,mpsc}};
use crate::{assets,bridge,character,menu::AppMode,model,sim_time::FramePolicy,tetherball::{BallMotion,Direction,Zone},tetherball_tuning::TetherballTuning,vlt};

/// Hit-speed ceiling per difficulty (`HitTetherball`'s table at 0x80442094; docs/TETHERBALL_HIT.md).
const SPEED_CAP:[f32;5]=[4.5,4.9,5.3,5.7,6.0];
const DIFFICULTY:[&str;4]=["Easy","Normal","Hard","Expert"];
const ORBIT_RADIUS:f32=0.7;
/// Ball height above the ground for zone 0 / zone 1 (Tetherball zone offsets 0.5 / 0.9).
const MAX_CHARGE:u32=5;
const POWER_COST:u32=2;

#[derive(Component)] pub struct TbEntity;
#[derive(Component)] struct TbBall;
#[derive(Component)] struct TbRope;
#[derive(Component)] struct TbCamera{yaw:f32}
#[derive(Component)] struct TbWindowDot{player:usize,order:usize}
#[derive(Component)] struct TbFigure{index:usize}
#[derive(Component)] struct TbModel{index:usize}

struct Loaded{pole:assets::BuiltModel,ball:assets::BuiltModel,player:character::CharacterData,kids:[Option<assets::BuiltModel>;2],tuning:[Result<TetherballTuning,String>;4]}
enum Msg{Loaded(Box<Loaded>),Failed(String),Log(String)}
#[derive(PartialEq,Clone,Debug)] enum Load{Loading,Ready,Failed(String)}
#[derive(PartialEq,Clone,Copy,Debug)] enum Phase{Ready,Rally,Over}

#[derive(Clone,Copy,Default)] struct Seat{charge:u32,cooldown:f32,swing:f32,hits:u32,misses:u32,power_hits:u32}
/// A pending AI swing: when the ball is within `trigger` radians (approaching) it swings; `result` is decided up front.
#[derive(Clone,Copy)] struct AiPlan{trigger:f32,outcome:AiOutcome,power:bool}
#[derive(Clone,Copy,PartialEq,Debug)] enum AiOutcome{Hit,TooFast,TooSlow,WrongHeight}

#[derive(Resource)]
pub struct Tb{
    rx:Mutex<mpsc::Receiver<Msg>>,load:Load,log:Vec<String>,started:std::time::Instant,
    data:Option<Box<Loaded>>,built:bool,
    ball:BallMotion,phase:Phase,difficulty:usize,seats:[Seat;2],
    /// Signed angle swept since the last reversal, and the whole rotations credited from it.
    swept:f32,wind:i32,rotations_to_win:u32,
    window:f32,distance:f32,pole_top:f32,
    message:String,winner:Option<usize>,rng:u64,plans:[Option<AiPlan>;2],latch:[bool;2],
    autoplay:bool,report:Vec<serde_json::Value>,t:f32,last_event:String,
    elapsed_match:f32,
    /// Front-end driven session: the HUD is the original `TetherballHud` APT screen and the match ends in `PostGameMP`.
    fe:Option<FeRun>,
    /// Test aid (`EAGL_TB_AUTO=1`): in a front-end session the human seat plays itself.
    auto_fe:bool,
}

/// Progress of a front-end launched match.
#[derive(Clone,Copy,PartialEq,Debug)] enum FeStage{OpenHud,WaitHud,Intro,Playing,Finish}
struct FeRun{stage:FeStage,since:f32,players:usize,last_charge:[u32;2]}

impl Tb{
    pub fn new(autoplay:bool)->Self{Self::with_kids(autoplay,[None,None])}
    /// `kids`: roster asset names for the two seats (None = the default Alicia model).
    pub fn with_kids(autoplay:bool,kids:[Option<String>;2])->Self{
        let (tx,rx)=mpsc::channel();
        std::thread::spawn(move||loader(tx,kids));
        Self{rx:Mutex::new(rx),load:Load::Loading,log:vec!["Tetherball: reading original data files".into()],started:std::time::Instant::now(),
            data:None,built:false,ball:fresh_ball(&[1.,1.,1.,1.]),phase:Phase::Ready,difficulty:1,seats:[Seat::default();2],
            swept:0.,wind:0,rotations_to_win:3,window:0.6,distance:1.3,pole_top:2.,message:String::new(),winner:None,rng:0x9e3779b97f4a7c15,plans:[None;2],latch:[false;2],
            autoplay,report:vec![],t:0.,last_event:String::new(),elapsed_match:0.,fe:None,auto_fe:std::env::var("EAGL_TB_AUTO").is_ok()}
    }
    pub fn new_fe(players:usize,kids:[Option<String>;2])->Self{let mut t=Self::with_kids(false,kids);t.fe=Some(FeRun{stage:FeStage::OpenHud,since:0.,players,last_charge:[99,99]});t}
    fn rand(&mut self)->f32{
        self.rng^=self.rng<<13;self.rng^=self.rng>>7;self.rng^=self.rng<<17;
        ((self.rng>>40) as f32)/((1u64<<24) as f32)
    }
    fn tuning(&self)->Option<&TetherballTuning>{self.data.as_ref().and_then(|d|d.tuning[self.difficulty].as_ref().ok())}
}

fn fresh_ball(speeds:&[f32;4])->BallMotion{
    BallMotion{angle:0.,hit_angle:0.,secondary_angle:0.,target_velocity:0.,secondary_target_velocity:0.,spin_acceleration:0.,hit_direction:Direction::Zero,grabbed:false,
        vertical_velocity:0.,toss_time:0,angular_velocity:0.,acceleration:0.,secondary_acceleration:0.,secondary_velocity:0.,hit_type:0,direction:Direction::Zero,zone:Zone::Zero,
        radius:ORBIT_RADIUS,desired_radius:ORBIT_RADIUS,height:0.5,target_height:0.5,base_hit_speed:speeds[0],power_modifier:speeds[1],mega_modifier:speeds[2],pole_height:0.,
        tossed:false,spinning_up:false,spinning_down:false}
}

fn data_dir(rel:&str)->String{bridge::data_root().join("files").join("data").join(rel.replace('/',"\\")).to_string_lossy().into_owned()}

fn loader(tx:mpsc::Sender<Msg>,seat_kids:[Option<String>;2]){
    let schemas=model::Schemas::embedded();
    let run=||->Result<Loaded,String>{
        let pole=assets::build(&format!("{}::tetherball_pole.o",data_dir("placeables/tetherball_pole.viv")),&schemas)?;
        let mg=data_dir("minigames/tetherball/mgtetherball.viv");
        let ball=assets::build(&format!("{mg}::teatherball.o"),&schemas)?;
        let player=character::load(&data_dir("characters"),&schemas)?;
        let kids:[Option<assets::BuiltModel>;2]=std::array::from_fn(|i|seat_kids[i].as_ref().filter(|a|a.as_str()!="alicia").and_then(|a|character::load_kid_model(&data_dir("characters"),a,&schemas).ok()));
        let dir=bridge::data_root().join("files").join("data").join("db");
        let (v,b)=(std::fs::read(dir.join("db.vlt")).map_err(|e|format!("db.vlt: {e}"))?,std::fs::read(dir.join("db.bin")).map_err(|e|format!("db.bin: {e}"))?);
        let db=vlt::Database::load(&v,&b,vlt::known_names())?;
        // Single-player, no dare: the regular `mg_tetherball/tunables` collection.
        let tuning=std::array::from_fn(|i|TetherballTuning::load(&db,0,-1,i as i32));
        Ok(Loaded{pole,ball,player,kids,tuning})
    };
    let _=tx.send(match run(){Ok(l)=>Msg::Loaded(Box::new(l)),Err(e)=>Msg::Failed(e)});
}

pub fn teardown(commands:&mut Commands,entities:&Query<Entity,With<TbEntity>>){
    for e in entities{commands.entity(e).try_despawn();}
    commands.remove_resource::<Tb>();
}

fn in_mode(tb:Option<Res<Tb>>)->bool{tb.is_some()}
pub fn plugin(app:&mut App){
    app.add_systems(Update,(pump,build,input,simulate,present).chain().run_if(in_mode))
        .add_systems(bevy_egui::EguiPrimaryContextPass,hud.run_if(in_mode));
}

fn pump(mut tb:Option<ResMut<Tb>>,mut settings:ResMut<WinitSettings>,mut redraw:MessageWriter<RequestRedraw>){
    let Some(tb)=tb.as_mut() else{return};
    settings.focused_mode=UpdateMode::Continuous;settings.unfocused_mode=UpdateMode::Continuous;redraw.write(RequestRedraw);
    if tb.load!=Load::Loading||tb.data.is_some(){return}
    let msgs:Vec<Msg>=tb.rx.lock().unwrap().try_iter().collect();
    for m in msgs{match m{
        Msg::Log(l)=>tb.log.push(l),
        Msg::Failed(e)=>{tb.log.push(format!("FAILED: {e}"));tb.load=Load::Failed(e);}
        Msg::Loaded(d)=>{
            tb.log.push(format!("pole bounds {:?}; ball bounds {:?}",d.pole.bounds,d.ball.bounds));
            for (i,t) in d.tuning.iter().enumerate(){match t{
                Ok(t)=>tb.log.push(format!("{}: speeds {:?} rotations {} distance {} ai {:?} angles {:?}",DIFFICULTY[i],t.speeds,t.single_player[5],t.single_player[2],t.ai,t.angles[0])),
                Err(e)=>tb.log.push(format!("{}: tuning unavailable ({e})",DIFFICULTY[i])),
            }}
            tb.data=Some(d);
        }
    }}
}

fn restart(tb:&mut Tb){
    let (speeds,rot,dist,angles)=match tb.tuning(){
        Some(t)=>(t.speeds,t.single_player[5].max(1),t.single_player[2],t.angles),
        None=>([4.5,1.2,1.5,1.],3,0,[[0.;3];4]),
    };
    tb.rotations_to_win=rot;
    // `distance_from_pole` is stored as an integer; use it when it lands in a playable range, otherwise a fixed stance.
    let d=dist as f32;
    tb.distance=if (0.8..=3.0).contains(&d){d}else if (80.0..=300.0).contains(&d){d/100.}else{1.3};
    // Return-angle delta (pre-hit lead) bounds the swing window; fall back to a fixed one if the data is out of range.
    let w=angles[0][1];
    tb.window=if (0.25..=1.4).contains(&w){w}else{0.6};
    tb.ball=fresh_ball(&speeds);
    tb.ball.pole_height=0.;tb.ball.height=0.5;tb.ball.target_height=0.5;
    tb.ball.angle=0.5;tb.swept=0.;tb.wind=0;tb.plans=[None;2];tb.latch=[false;2];tb.winner=None;
    tb.seats=[Seat::default();2];tb.phase=Phase::Ready;tb.elapsed_match=0.;
    tb.message="Press SPACE to serve".into();
}

fn build(mut commands:Commands,mut tb:Option<ResMut<Tb>>,mut meshes:ResMut<Assets<Mesh>>,mut materials:ResMut<Assets<StandardMaterial>>,mut images:ResMut<Assets<Image>>,mut clips:ResMut<Assets<AnimationClip>>,
    mut ibp:ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,mut graphs:ResMut<Assets<AnimationGraph>>){
    let Some(tb)=tb.as_mut() else{return};let tb=&mut **tb;
    if tb.built||tb.load!=Load::Loading{return}
    restart(tb);
    let Some(data)=tb.data.take() else{return};
    let data_ref=&*data;
    // Ground and orbit ring.
    commands.spawn((TbEntity,Mesh3d(meshes.add(Cylinder::new(40.,0.1))),MeshMaterial3d(materials.add(StandardMaterial{base_color:Color::srgb(0.30,0.55,0.25),perceptual_roughness:1.,..default()})),Transform::from_xyz(0.,-0.05,0.)));
    commands.spawn((TbEntity,Mesh3d(meshes.add(Annulus::new(tb.distance-0.03,tb.distance+0.03))),MeshMaterial3d(materials.add(StandardMaterial{base_color:Color::srgb(0.9,0.85,0.6),unlit:true,..default()})),Transform::from_xyz(0.,0.01,0.).with_rotation(Quat::from_rotation_x(-PI/2.))));
    // Pole.
    let pole=assets::upload(&data_ref.pole,&mut meshes,&mut materials,&mut images,false);
    assets::spawn(&mut commands,&pole,(TbEntity,Transform::IDENTITY),None);
    tb.pole_top=data_ref.pole.bounds[1][1].max(1.5);
    // Ball.
    let ball=assets::upload(&data_ref.ball,&mut meshes,&mut materials,&mut images,false);
    assets::spawn(&mut commands,&ball,(TbEntity,TbBall,Transform::from_xyz(0.,0.5,ORBIT_RADIUS)),None);
    // Rope (a thin cylinder from the pole top to the ball; the decoded rope model is a fixed-pose mesh).
    commands.spawn((TbEntity,TbRope,Mesh3d(meshes.add(Cylinder::new(0.012,1.))),MeshMaterial3d(materials.add(StandardMaterial{base_color:Color::srgb(0.85,0.8,0.65),perceptual_roughness:1.,..default()})),Transform::IDENTITY));
    // Hit-window markers for the human seat.
    let dot=meshes.add(Sphere::new(0.035));
    for order in 0..9{
        commands.spawn((TbEntity,TbWindowDot{player:0,order},Mesh3d(dot.clone()),MeshMaterial3d(materials.add(StandardMaterial{base_color:Color::srgb(0.2,0.9,0.4),unlit:true,..default()})),Transform::IDENTITY));
    }
    // Characters (the same decoded Alicia rig for both seats).
    let default_up=assets::upload_skinned(&data_ref.player.model,&mut meshes,&mut materials,&mut images);
    for index in 0..2{
        let up_own=data_ref.kids[index].as_ref().map(|m|assets::upload_skinned(m,&mut meshes,&mut materials,&mut images));
        let up=up_own.as_ref().unwrap_or(&default_up);
        let handles:Vec<Handle<AnimationClip>>=data_ref.player.clips.iter().filter_map(|c|character::animation_clip(c).ok().map(|a|clips.add(a))).collect();
        let mut graph=AnimationGraph::new();
        let nodes:Vec<AnimationNodeIndex>=handles.iter().map(|c|graph.add_clip(c.clone(),1.,graph.root)).collect();
        let graph=graphs.add(graph);
        let yaw=if index==0{PI}else{0.};
        let z=if index==0{tb.distance}else{-tb.distance};
        let figure=commands.spawn((TbEntity,TbFigure{index},Transform::from_xyz(0.,0.,z).with_rotation(Quat::from_rotation_y(yaw)),Visibility::default())).id();
        let model=commands.spawn((TbEntity,TbModel{index},ChildOf(figure),Transform::default(),Visibility::default())).id();
        let mut ap=AnimationPlayer::default();
        for (i,n) in nodes.iter().enumerate(){let a=ap.play(*n);a.repeat();a.set_weight(if i==0{1.}else{0.});}
        commands.entity(model).insert((ap,AnimationGraphHandle(graph)));
        character::spawn_rig(&mut commands,&mut ibp,&data_ref.player.skeleton,up,model,model);
    }
    commands.spawn((TbEntity,TbCamera{yaw:0.75},crate::apt_view::LetterboxCamera,Camera3d::default(),Camera{order:0,clear_color:ClearColorConfig::Custom(Color::srgb(0.45,0.68,0.92)),..default()},Transform::from_xyz(0.,2.,5.).looking_at(Vec3::new(0.,0.8,0.),Vec3::Y)));
    commands.spawn((TbEntity,DirectionalLight{illuminance:9000.,shadow_maps_enabled:false,..default()},Transform::from_rotation(Quat::from_euler(EulerRot::XYZ,-0.9,-0.5,0.))));
    tb.data=Some(data);
    tb.built=true;tb.load=Load::Ready;
    tb.log.push(format!("ready: distance {:.2}, window {:.2} rad, rotations to win {}, pole top {:.2}",tb.distance,tb.window,tb.rotations_to_win,tb.pole_top));
}

fn wrap_pi(a:f32)->f32{let mut a=a%TAU;if a>PI{a-=TAU}else if a< -PI{a+=TAU}a}
/// Seat angles on the orbit circle (0 = +Z, the human; PI = the opponent).
fn seat_angle(i:usize)->f32{if i==0{0.}else{PI}}
/// Each player winds the ball in their own direction: seat 0 sends it toward decreasing angles.
fn seat_direction(i:usize)->Direction{if i==0{Direction::Zero}else{Direction::One}}
fn owner(d:Direction)->usize{if d==Direction::Zero{0}else{1}}
fn speed_cap(d:usize)->f32{SPEED_CAP[d.min(4)]}

#[derive(Clone,Copy,PartialEq)] enum HitKind{Normal,Power,Mega}

fn input(mut tb:Option<ResMut<Tb>>,keys:Res<ButtonInput<KeyCode>>,mut mode:ResMut<AppMode>,mut fe:Option<NonSendMut<crate::apt_view::AptViewNs>>){
    let Some(tb)=tb.as_mut() else{return};let tb=&mut **tb;
    if tb.fe.is_none()&&keys.just_pressed(KeyCode::Escape){*mode=AppMode::Menu;return}
    if tb.load!=Load::Ready{return}
    if let Some(f)=&tb.fe{ if f.stage!=FeStage::Playing{return} }
    if tb.fe.is_some(){
        if let Some(v)=fe.as_mut(){
            let vm=&mut v.0.vm;
            if !vm.fe.paused&&(keys.just_pressed(KeyCode::Escape)||keys.just_pressed(KeyCode::KeyP)||keys.just_pressed(KeyCode::Equal)||std::mem::take(&mut vm.fe.script_pause)){
                vm.fe.paused=true;vm.fe.pause_req=None;
                vm.fe.todo.push(("OpenOverlay".into(),vec![crate::apt_vm::V::Str("PauseMenu".into())]));
                return
            }
            if vm.fe.paused{return}
        }
    }
    for (i,k) in [KeyCode::Digit1,KeyCode::Digit2,KeyCode::Digit3,KeyCode::Digit4].into_iter().enumerate(){
        if tb.fe.is_none()&&keys.just_pressed(k)&&tb.phase!=Phase::Rally{tb.difficulty=i;restart(tb);}
    }
    if tb.fe.is_none()&&keys.just_pressed(KeyCode::KeyR){restart(tb);}
    let space=keys.just_pressed(KeyCode::Space)||keys.just_pressed(KeyCode::Enter);
    let kind=if keys.pressed(KeyCode::ControlLeft)||keys.pressed(KeyCode::ControlRight){HitKind::Mega}else if keys.pressed(KeyCode::ShiftLeft)||keys.pressed(KeyCode::ShiftRight){HitKind::Power}else{HitKind::Normal};
    if tb.autoplay{return}
    if space{
        match tb.phase{
            Phase::Ready=>serve(tb),
            Phase::Over=>{ if tb.fe.is_none(){restart(tb)} }
            Phase::Rally=>{swing(tb,0,kind);}
        }
    }
}

fn serve(tb:&mut Tb){
    let speed=tb.ball.base_hit_speed.min(speed_cap(tb.difficulty));
    tb.ball.angle=0.35;
    tb.ball.serve(seat_direction(0),speed);
    // Keep the serve in the normal speed band the hit code would use.
    tb.phase=Phase::Rally;tb.swept=0.;tb.plans=[None;2];tb.latch=[false;2];tb.message="Rally! Swing as the ball reaches you".into();
    tb.last_event="serve".into();
}

/// Attempt a hit for `seat`. Returns whether the ball was struck.
fn swing(tb:&mut Tb,seat:usize,kind:HitKind)->bool{
    let s=tb.seats[seat];
    if s.cooldown>0.{return false}
    let kind=match kind{
        HitKind::Mega if s.charge>=MAX_CHARGE=>HitKind::Mega,
        HitKind::Power|HitKind::Mega if s.charge>=POWER_COST=>HitKind::Power,
        _=>HitKind::Normal,
    };
    let d=wrap_pi(tb.ball.angle-seat_angle(seat));
    tb.seats[seat].swing=0.35;
    if d.abs()>tb.window{
        tb.ball.miss();
        tb.seats[seat].misses+=1;tb.seats[seat].cooldown=0.45;
        tb.last_event=format!("seat {seat} missed");
        return false
    }
    let base=tb.ball.base_hit_speed;
    let (mult,cost,hit_type)=match kind{HitKind::Normal=>(1.,0,0),HitKind::Power=>(tb.ball.power_modifier,POWER_COST,1),HitKind::Mega=>(tb.ball.mega_modifier,MAX_CHARGE,3)};
    let speed=(base*mult).min(speed_cap(tb.difficulty)*mult.max(1.));
    let dir=seat_direction(seat);
    let reversed=tb.ball.direction!=dir;
    let angle=tb.ball.angle;
    tb.ball.hit(dir,speed,angle,hit_type);
    if reversed{tb.swept=0.;}
    let s=&mut tb.seats[seat];
    s.charge=(s.charge.saturating_sub(cost)+1).min(MAX_CHARGE);s.hits+=1;s.cooldown=0.25;
    if kind!=HitKind::Normal{s.power_hits+=1;}
    tb.last_event=format!("seat {seat} hit {}",match kind{HitKind::Normal=>"normal",HitKind::Power=>"power",HitKind::Mega=>"mega"});
    true
}

fn plan_ai(tb:&mut Tb,seat:usize)->AiPlan{
    let ai=tb.tuning().map(|t|t.ai).unwrap_or([10,10,10,20,0,0,0]);
    let pct=|v:u8|v as f32/100.;
    let r=tb.rand();
    let (fast,slow,wrong)=(pct(ai[0]),pct(ai[1]),pct(ai[2]));
    let outcome=if r<wrong{AiOutcome::WrongHeight}else if r<wrong+fast{AiOutcome::TooFast}else if r<wrong+fast+slow{AiOutcome::TooSlow}else{AiOutcome::Hit};
    let w=tb.window;
    let trigger=match outcome{AiOutcome::TooFast=>w*1.7,AiOutcome::TooSlow=>w*0.1,_=>w*(0.35+0.4*tb.rand())};
    let power=tb.rand()<pct(ai[3]).max(0.15)&&tb.seats[seat].charge>=POWER_COST;
    AiPlan{trigger,outcome,power}
}

/// Simulated opponent: choose a plan when the ball is heading at it, swing when the plan's trigger distance is reached.
fn ai_step(tb:&mut Tb,seat:usize){
    let dir=seat_direction(seat);
    // The ball is only a threat when it winds the *other* way.
    if tb.ball.direction==dir{tb.plans[seat]=None;tb.latch[seat]=false;return}
    let d=wrap_pi(tb.ball.angle-seat_angle(seat));
    let approaching=d*tb.ball.angular_velocity<0.;
    // One decision per pass: forget it once the ball has gone by (or is still far on the other side).
    if !approaching||d.abs()>PI*0.55{tb.plans[seat]=None;tb.latch[seat]=false;return}
    if tb.latch[seat]{return}
    if tb.plans[seat].is_none(){let p=plan_ai(tb,seat);tb.plans[seat]=Some(p);}
    let Some(p)=tb.plans[seat] else{return};
    if d.abs()<=p.trigger{
        tb.plans[seat]=None;tb.latch[seat]=true;
        match p.outcome{
            AiOutcome::Hit=>{swing(tb,seat,if p.power{HitKind::Power}else{HitKind::Normal});}
            // Mistimed or wrong-height swings are misses (the ball is not struck).
            _=>{tb.ball.miss();tb.seats[seat].misses+=1;tb.seats[seat].cooldown=0.45;tb.seats[seat].swing=0.35;tb.last_event=format!("seat {seat} {:?}",p.outcome);}
        }
    }
}

fn simulate(mut tb:Option<ResMut<Tb>>,time:Res<Time<Real>>,mut exit:MessageWriter<AppExit>,mut fe:Option<NonSendMut<crate::apt_view::AptViewNs>>,mut commands:Commands,entities:Query<Entity,With<TbEntity>>){
    let Some(tb)=tb.as_mut() else{return};let tb=&mut **tb;
    if tb.load!=Load::Ready{return}
    if tb.fe.is_some(){
        if let Some(v)=fe.as_mut(){
            if v.0.vm.fe.paused{
                use crate::fe_host::PauseReq;
                match v.0.vm.fe.pause_req.take(){
                    Some(PauseReq::Resume)=>{v.0.vm.fe.paused=false;v.0.vm.fe.todo.push(("CloseOverlay".into(),vec![]));}
                    Some(PauseReq::Restart)=>{
                        v.0.vm.fe.paused=false;v.0.vm.fe.todo.push(("CloseOverlay".into(),vec![]));
                        restart(tb);
                        if let Some(f)=tb.fe.as_mut(){f.stage=FeStage::Intro;f.since=0.;f.last_charge=[99,99];}
                        v.0.vm.fe.start_anim_done=false;v.0.vm.fe.serve_bubble=false;
                        v.0.vm.call_exposed("ServeBubble_SetVisible",vec![crate::apt_vm::V::Num(0.),crate::apt_vm::V::Num(380.),crate::apt_vm::V::Num(120.)]);
                        v.0.vm.call_exposed("GameStartAnim_Reset",vec![]);
                        v.0.vm.call_exposed("GameStartAnim_Play",vec![]);
                    }
                    Some(PauseReq::Quit)=>{
                        v.0.vm.fe.paused=false;v.0.vm.fe.mp.active=false;
                        v.0.vm.fe.todo.push(("CloseOverlay".into(),vec![]));
                        v.0.vm.fe.todo.push(("ReplaceScreen".into(),vec![crate::apt_vm::V::Str("MainMenu".into())]));
                        for e in &entities{commands.entity(e).try_despawn();}
                        commands.remove_resource::<Tb>();return
                    }
                    None=>{}
                }
                return
            }
            if fe_drive(tb,&mut v.0.vm,time.delta_secs()){ for e in &entities{commands.entity(e).try_despawn();} commands.remove_resource::<Tb>(); return }
        }
    }
    let step=FramePolicy::default().step((time.delta_secs_f64()*1000.) as f32);
    let ms=step.simulation_ms as i32;let dt=step.seconds();
    tb.t+=dt;
    for s in &mut tb.seats{s.cooldown=(s.cooldown-dt).max(0.);s.swing=(s.swing-dt).max(0.);}
    if (tb.autoplay||(tb.auto_fe&&tb.fe.as_ref().is_some_and(|f|f.stage==FeStage::Playing)))&&tb.phase==Phase::Ready&&tb.t>1.{serve(tb);}
    if tb.autoplay&&tb.phase==Phase::Over{finish_autotest(tb,&mut exit);return}
    if tb.phase!=Phase::Rally{
        // The ball idles in its serve position.
        tb.ball.update_motion(ms);return
    }
    tb.elapsed_match+=dt;
    let before=tb.ball.angle;
    tb.ball.update_motion(ms);
    let moved=wrap_pi(tb.ball.angle-before);
    tb.swept+=moved;
    // A full turn the opponent failed to answer moves the shared winding count toward the ball's owner.
    if tb.swept.abs()>=TAU{
        let w=owner(tb.ball.direction);
        tb.swept-=TAU*tb.swept.signum();
        tb.wind+=if w==0{1}else{-1};
        tb.last_event=format!("rotation for seat {w}; wind {}",tb.wind);
        if tb.wind.unsigned_abs()>=tb.rotations_to_win{
            tb.winner=Some(w);tb.phase=Phase::Over;
            tb.message=if w==0{"You win! Press SPACE for a rematch".into()}else{"The opponent wins. Press SPACE for a rematch".into()};
            tb.last_event=format!("seat {w} wins");
            return
        }
    }
    ai_step(tb,1);
    if tb.autoplay||tb.auto_fe{
        // The autotest stands in for a human who always reacts in time (power hits when charged).
        let d=wrap_pi(tb.ball.angle-seat_angle(0));
        if tb.ball.direction!=seat_direction(0)&&d*tb.ball.angular_velocity<0.&&d.abs()<=tb.window*0.5{swing(tb,0,HitKind::Power);}
    }
    if tb.elapsed_match>120.&&tb.autoplay{tb.phase=Phase::Over;tb.message="timeout".into();}
}

fn finish_autotest(tb:&mut Tb,exit:&mut MessageWriter<AppExit>){
    let _=std::fs::create_dir_all("docs");
    let rep=json!({
        "completed":tb.winner.is_some(),"winner":tb.winner,"difficulty":DIFFICULTY[tb.difficulty],"rotations_to_win":tb.rotations_to_win,
        "match_seconds":tb.elapsed_match,"seats":(0..2).map(|i|json!({"hits":tb.seats[i].hits,"misses":tb.seats[i].misses,"power_hits":tb.seats[i].power_hits,"charge":tb.seats[i].charge})).collect::<Vec<_>>(),
        "ball":{"angle":tb.ball.angle,"angular_velocity":tb.ball.angular_velocity,"radius":tb.ball.radius},"window":tb.window,"distance":tb.distance,"log":tb.log,
    });
    let _=std::fs::write("docs/tetherball-autotest.json",serde_json::to_string_pretty(&rep).unwrap());
    exit.write(AppExit::Success);
}

fn present(tb:Option<Res<Tb>>,mut ball:Query<&mut Transform,(With<TbBall>,Without<TbRope>,Without<TbCamera>,Without<TbFigure>,Without<TbWindowDot>,Without<TbModel>)>,
    mut rope:Query<&mut Transform,(With<TbRope>,Without<TbBall>,Without<TbCamera>,Without<TbFigure>,Without<TbWindowDot>,Without<TbModel>)>,
    mut cam:Query<(&mut TbCamera,&mut Transform),(Without<TbBall>,Without<TbRope>,Without<TbFigure>,Without<TbWindowDot>,Without<TbModel>)>,
    mut dots:Query<(&TbWindowDot,&mut Transform,&MeshMaterial3d<StandardMaterial>),(Without<TbBall>,Without<TbRope>,Without<TbCamera>,Without<TbFigure>,Without<TbModel>)>,
    mut materials:ResMut<Assets<StandardMaterial>>,
    mut figures:Query<(&TbFigure,&mut Transform),(Without<TbBall>,Without<TbRope>,Without<TbCamera>,Without<TbWindowDot>,Without<TbModel>)>,
    mut models:Query<(&TbModel,&mut AnimationPlayer)>,
    keys:Res<ButtonInput<KeyCode>>,time:Res<Time>){
    let Some(tb)=tb else{return};if tb.load!=Load::Ready{return}
    let a=tb.ball.angle;
    let pos=Vec3::new(tb.ball.radius*a.sin(),tb.ball.height,tb.ball.radius*a.cos());
    for mut t in &mut ball{t.translation=pos;}
    let top=Vec3::new(0.,tb.pole_top,0.);
    for mut t in &mut rope{
        let v=pos-top;let len=v.length().max(0.01);
        *t=Transform{translation:top+v*0.5,rotation:Quat::from_rotation_arc(Vec3::Y,v/len),scale:Vec3::new(1.,len,1.)};
    }
    for (mut c,mut t) in &mut cam{
        let k=|k:KeyCode|keys.pressed(k) as i32 as f32;
        c.yaw+=(k(KeyCode::KeyQ)-k(KeyCode::KeyE)+k(KeyCode::ArrowLeft)-k(KeyCode::ArrowRight))*1.4*time.delta_secs();
        let eye=Vec3::new(c.yaw.sin()*(tb.distance+4.2),2.6,c.yaw.cos()*(tb.distance+4.2));
        *t=Transform::from_translation(eye).looking_at(Vec3::new(0.,1.1,0.),Vec3::Y);
    }
    let inside=wrap_pi(tb.ball.angle).abs()<=tb.window&&tb.phase==Phase::Rally;
    for (d,mut t,m) in &mut dots{
        let f=if tb.window>0.{d.order as f32/8.*2.-1.}else{0.};
        let ang=seat_angle(d.player)+f*tb.window;
        t.translation=Vec3::new(tb.distance*0.62*ang.sin(),0.04,tb.distance*0.62*ang.cos());
        if let Some(mut mat)=materials.get_mut(&m.0){mat.base_color=if inside{Color::srgb(1.,0.9,0.1)}else{Color::srgb(0.2,0.9,0.4)};}
    }
    for (f,mut t) in &mut figures{
        let swing=tb.seats[f.index].swing;
        let lean=(swing/0.35).clamp(0.,1.)*0.35;
        let yaw=if f.index==0{PI}else{0.};
        let z=if f.index==0{tb.distance}else{-tb.distance};
        // Face the ball while rallying so the swing reads as aimed at it.
        let facing=if tb.phase==Phase::Rally{let to=pos-Vec3::new(0.,0.,z);to.x.atan2(to.z)}else{yaw};
        t.translation=Vec3::new(0.,0.,z);
        t.rotation=Quat::from_rotation_y(facing)*Quat::from_rotation_x(lean);
    }
    for (m,mut ap) in &mut models{
        // Idle normally; blend the decoded run clip in while swinging so the motion reads on screen.
        let run=(tb.seats[m.index].swing/0.35).clamp(0.,1.);
        for (i,w) in [1.-run,0.,run].into_iter().enumerate(){if let Some(a)=ap.animation_mut(AnimationNodeIndex::new(i+1)){a.set_weight(w);}}
    }
}

fn hud(mut contexts:EguiContexts,tb:Option<ResMut<Tb>>,mut mode:ResMut<AppMode>)->Result{
    let Some(tb)=tb else{return Ok(())};
    if tb.fe.is_some(){return Ok(())}
    let ctx=contexts.ctx_mut()?;
    let accent=egui::Color32::from_rgb(104,200,170);
    match &tb.load{
        Load::Loading=>{egui::Area::new("tb_load".into()).anchor(egui::Align2::CENTER_CENTER,[0.,0.]).show(ctx,|ui|{ui.vertical_centered(|ui|{ui.heading("Loading Tetherball…");ui.spinner();for l in tb.log.iter().rev().take(8).rev(){ui.label(egui::RichText::new(l).small());}});});return Ok(())}
        Load::Failed(e)=>{egui::Area::new("tb_fail".into()).anchor(egui::Align2::CENTER_CENTER,[0.,0.]).show(ctx,|ui|{ui.heading("Tetherball could not load");ui.colored_label(egui::Color32::LIGHT_RED,e);if ui.button("< Menu").clicked(){*mode=AppMode::Menu;}});return Ok(())}
        Load::Ready=>{}
    }
    egui::Area::new("tb_top".into()).anchor(egui::Align2::LEFT_TOP,[12.,12.]).show(ctx,|ui|{
        egui::Frame::popup(ui.style()).show(ui,|ui|{
            ui.horizontal(|ui|{if ui.button("< Menu").clicked(){*mode=AppMode::Menu;}ui.heading("Tetherball");ui.label(egui::RichText::new(format!("difficulty: {}",DIFFICULTY[tb.difficulty])).color(accent));});
            let owner_=owner(tb.ball.direction);
            ui.label(format!("Winding now: {}   Win at {} net turns",if tb.ball.angular_velocity==0.{"-"}else if owner_==0{"you"}else{"opponent"},tb.rotations_to_win));
            ui.horizontal(|ui|{ui.label("Opponent");ui.add(egui::ProgressBar::new((tb.wind as f32/tb.rotations_to_win as f32*0.5+0.5).clamp(0.,1.)).desired_width(180.).text(format!("{:+} turns",tb.wind)));ui.label("You");});
            ui.label(format!("You: {} hits, {} misses   Opponent: {} hits, {} misses",tb.seats[0].hits,tb.seats[0].misses,tb.seats[1].hits,tb.seats[1].misses));
            ui.horizontal(|ui|{ui.label("Charge");ui.add(egui::ProgressBar::new(tb.seats[0].charge as f32/MAX_CHARGE as f32).desired_width(120.).text(format!("{}/{}",tb.seats[0].charge,MAX_CHARGE)));});
            ui.label(egui::RichText::new("SPACE swing   SHIFT+SPACE power (2 charge)   CTRL+SPACE mega (5)   1-4 difficulty (between rallies)   Q/E or arrows rotate view   R restart   Esc menu").small().color(egui::Color32::GRAY));
        });
    });
    egui::Area::new("tb_msg".into()).anchor(egui::Align2::CENTER_BOTTOM,[0.,-40.]).show(ctx,|ui|{
        let big=egui::RichText::new(&tb.message).size(24.).strong().color(egui::Color32::WHITE);
        egui::Frame::popup(ui.style()).show(ui,|ui|{ui.label(big);});
    });
    egui::Area::new("tb_note".into()).anchor(egui::Align2::RIGHT_BOTTOM,[-12.,-12.]).show(ctx,|ui|{
        ui.label(egui::RichText::new("Original: models, animation, ball motion arithmetic, speeds, rotations and AI mistake chances.\nProvisional: hit windows, charge costs, rotation counting and AI timing.").small().color(egui::Color32::GRAY));
    });
    Ok(())
}

/// Drive a front-end session: open the original HUD screen, play its start animation, mirror state into it and
/// hand the result to `PostGameMP`.  Returns true once the session is over (the caller removes the scene).
fn fe_drive(tb:&mut Tb,vm:&mut crate::apt_vm::Vm,dt:f32)->bool{
    use crate::apt_vm::V;
    let Some(fe)=tb.fe.as_mut() else{return false};
    fe.since+=dt;
    match fe.stage{
        FeStage::OpenHud=>{
            vm.fe.hud_loaded=false;vm.fe.start_anim_done=false;
            vm.call_exposed("OpenScreen",vec![V::Str("TetherballHud".into())]);
            fe.stage=FeStage::WaitHud;fe.since=0.;
        }
        FeStage::WaitHud=>{
            if vm.fe.hud_loaded{
                vm.call_exposed("GameStartAnim_Play",vec![]);
                for m in 0..2{vm.call_exposed("MegaMeter_SetVisible",vec![V::Num(m as f64),V::Num(1.)]);}
                fe.stage=FeStage::Intro;fe.since=0.;
            }
        }
        FeStage::Intro=>{
            if vm.fe.start_anim_done{
                fe.stage=FeStage::Playing;fe.since=0.;tb.message="serve".into();
                vm.call_exposed("ServeBubble_SetVisible",vec![V::Num(1.),V::Num(380.),V::Num(120.)]);
            }
        }
        FeStage::Playing=>{
            for seat in 0..2{
                let c=tb.seats[seat].charge;
                if fe.last_charge[seat]!=c{fe.last_charge[seat]=c;vm.call_exposed("MegaMeter_SetValue",vec![V::Num(seat as f64),V::Num(c as f64)]);}
            }
            if tb.phase==Phase::Rally&&fe.since>=0.{ if vm.fe.serve_bubble{vm.fe.serve_bubble=false;vm.call_exposed("ServeBubble_SetVisible",vec![V::Num(0.),V::Num(380.),V::Num(120.)]);} }
            if tb.phase==Phase::Ready&&!vm.fe.serve_bubble&&tb.winner.is_none()&&fe.since<0.0{ }
            if tb.phase==Phase::Over{
                fe.stage=FeStage::Finish;fe.since=0.;
                let human=tb.winner==Some(0);
                vm.call_exposed("WinLose_SetVisible",vec![V::Num(if human{0.}else{1.}),V::Num(1.)]);
                vm.fe.mp.results=Some(crate::fe_host::MpResult{winner:tb.winner.unwrap_or(0) as i32,scores:[tb.wind.max(0) as i32,(-tb.wind).max(0) as i32],hits:[tb.seats[0].hits as i32,tb.seats[1].hits as i32],power_hits:[tb.seats[0].power_hits as i32,tb.seats[1].power_hits as i32]});
            }
        }
        FeStage::Finish=>{
            if fe.since>4.{
                vm.fe.launch_done=true;
                {let next=if vm.fe.mp.quick{"PostGame"}else{"PostGameMP"};vm.fe.todo.push(("ReplaceScreen".into(),vec![V::Str(next.into())]));}
                return true
            }
        }
    }
    false
}
