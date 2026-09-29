//! Game mode: boots from the original data files, loads the world and a player, and lets the
//! recovered locomotion (see `locomotion.rs`) drive a character.  Evidence per subsystem is shown
//! in the HUD and written by `--selftest`; nothing here is labelled original unless it traces to
//! the executable or a data file.
use bevy::{prelude::*,render::view::window::screenshot::{Screenshot,save_to_disk},input::mouse::{AccumulatedMouseMotion,AccumulatedMouseScroll},window::{PrimaryWindow,RequestRedraw},winit::{WinitSettings,UpdateMode},world_serialization::WorldInstanceReady};
use bevy_egui::{egui,EguiContexts,EguiTextureHandle};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use std::{collections::{HashMap,VecDeque},path::PathBuf,time::Instant};
use crate::{bridge::{self,Bridge},locomotion::{InputKind,Locomotion,TICK_HZ},menu::AppMode,recovered as k};

#[derive(Component,Debug,Clone,Serialize,Deserialize)]
pub struct OriginalAsset {
    pub source:String,
    pub decoder_version:String,
    pub evidence:EvidenceLevel,
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
pub enum EvidenceLevel { Unresolved, AssetDerived, ExecutableDerived, GameplayCompared }

/// Not recovered from the executable: values below exist only to make the slice playable.
const PROVISIONAL_GRAVITY:f32=20.;
const PROVISIONAL_JUMP_SPEED:f32=5.5;
const STEP_UP:f32=0.6;
const MIN_BOOT_SECONDS:f32=2.5;
const CLIPS:[(&str,usize);3]=[("S_idle",250),("S_walk",263),("S_run",253)];
const WORLD_SUFFIXES:[&str;4]=["","-alpha","-fade","-alphafade"];

#[derive(Component)] pub struct GameEntity;
#[derive(Component)] pub struct GameCamera{pub yaw:f32,pub pitch:f32,pub distance:f32}
#[derive(Component)] pub struct Player{loco:Locomotion,vy:f32,grounded:bool,speed:f32,facing:Vec3,weights:[f32;3],anim_ready:bool}
#[derive(Component)] struct WorldLayer(String);
#[derive(Component)] struct PlayerModel;

#[derive(Resource,Default)] pub struct GameInput{pub x:f32,pub y:f32,pub jump:bool,pub scripted:bool}

struct Job{key:String,req:Value}
#[derive(PartialEq,Clone,Debug)] enum Phase{Loading,Building,Playing,Failed(String)}

pub struct Ground{cell:f32,min:Vec2,cols:usize,rows:usize,cells:Vec<Vec<u32>>,tris:Vec<[Vec3;3]>}
impl Ground {
    fn build(tris:Vec<[Vec3;3]>)->Self{
        let (mut lo,mut hi)=(Vec2::splat(f32::MAX),Vec2::splat(f32::MIN));
        for t in &tris{for v in t{lo=lo.min(v.xz());hi=hi.max(v.xz());}}
        let cell=2.;let cols=(((hi.x-lo.x)/cell).ceil() as usize+1).max(1);let rows=(((hi.y-lo.y)/cell).ceil() as usize+1).max(1);
        let mut cells=vec![Vec::new();cols*rows];
        for (i,t) in tris.iter().enumerate(){
            let a=t[1]-t[0];let b=t[2]-t[0];if a.cross(b).normalize_or_zero().y<0.3{continue} // walkable surfaces only
            let tl=t[0].xz().min(t[1].xz()).min(t[2].xz());let th=t[0].xz().max(t[1].xz()).max(t[2].xz());
            let (c0,r0)=(((tl.x-lo.x)/cell) as usize,((tl.y-lo.y)/cell) as usize);
            let (c1,r1)=(((th.x-lo.x)/cell) as usize,((th.y-lo.y)/cell) as usize);
            for r in r0..=r1.min(rows-1){for c in c0..=c1.min(cols-1){cells[r*cols+c].push(i as u32);}}
        }
        Self{cell,min:lo,cols,rows,cells,tris}
    }
    /// Highest walkable surface at (x,z) not above `max_y`.
    pub fn height(&self,x:f32,z:f32,max_y:f32)->Option<f32>{
        let c=((x-self.min.x)/self.cell).floor();let r=((z-self.min.y)/self.cell).floor();
        if c<0.||r<0.||c as usize>=self.cols||r as usize>=self.rows{return None}
        let mut best:Option<f32>=None;let p=Vec2::new(x,z);
        for &i in &self.cells[r as usize*self.cols+c as usize]{
            let t=&self.tris[i as usize];
            let (a,b,cc)=(t[0].xz(),t[1].xz(),t[2].xz());
            let d=(b.y-cc.y)*(a.x-cc.x)+(cc.x-b.x)*(a.y-cc.y);if d.abs()<1e-9{continue}
            let l1=((b.y-cc.y)*(p.x-cc.x)+(cc.x-b.x)*(p.y-cc.y))/d;let l2=((cc.y-a.y)*(p.x-cc.x)+(a.x-cc.x)*(p.y-cc.y))/d;let l3=1.-l1-l2;
            if l1< -1e-4||l2< -1e-4||l3< -1e-4{continue}
            let y=l1*t[0].y+l2*t[1].y+l3*t[2].y;
            if y<=max_y && best.is_none_or(|b|y>b){best=Some(y);}
        }
        best
    }
}

#[derive(Resource)]
pub struct Game{
    bridge:Bridge,jobs:VecDeque<Job>,pending:Option<(u64,String)>,results:HashMap<String,Value>,
    phase:Phase,started:Instant,pub log:Vec<String>,boot_image:Option<Handle<Image>>,boot_egui:Option<egui::TextureId>,
    layers:Vec<(String,String)>,layers_expected:usize,layers_ready:usize,layers_empty:usize,clip_handles:Vec<Handle<AnimationClip>>,
    ground:Option<Ground>,build_wait:u32,player:Option<Entity>,pub spawn:Vec3,
    pub tri_count:usize,pub controls_rows:usize,pub combat_bindings:usize,pub world_bounds:Option<(f32,f32,f32)>,
    boot_shown:Option<Instant>,boot_shot:bool,pub load_seconds:f32,
    pub selftest:Option<SelfTest>,
}

#[derive(Clone)] pub struct SelfTest{pub out:PathBuf,step:usize,t:f32,samples:Vec<Value>,start:Vec3,checks:Vec<Value>,shot:bool,shot_wait:f32}
impl SelfTest{pub fn new(out:PathBuf)->Self{Self{out,step:0,t:0.,samples:vec![],start:Vec3::ZERO,checks:vec![],shot:false,shot_wait:0.}}}

fn src(rel:&str)->String{bridge::data_root().join("files").join("data").join(rel.replace('/',"\\")).to_string_lossy().into_owned()}

impl Game{
    pub fn new(selftest:Option<SelfTest>)->Self{
        let mut jobs=VecDeque::new();
        let strap=src("boot/strapwarn_standard_english.gsh");
        jobs.push_back(Job{key:"boot".into(),req:json!({"command":"preview","source":strap})});
        let big=src("world/world.big");
        jobs.push_back(Job{key:"filelist".into(),req:json!({"command":"inspect","source":format!("{big}::worldfilelist.csv")})});
        jobs.push_back(Job{key:"bounds".into(),req:json!({"command":"inspect","source":format!("{big}::world.csv")})});
        jobs.push_back(Job{key:"controls".into(),req:json!({"command":"inspect","source":format!("{}::controls.csv",src("csvs.viv"))})});
        let model=format!("{}::alicia.viv::alicia.o",src("characters/models/characters.viv"));
        let skel=format!("{}::player_skel.ske",src("characters/player_anims.viv"));let bank=format!("{}::player_anims.anm",src("characters/player_anims.viv"));
        for (name,index) in CLIPS{jobs.push_back(Job{key:format!("clip:{name}"),req:json!({"command":"preview","source":model,"skeleton":skel,"bank":bank,"index":index})});}
        Self{bridge:Bridge::start(),jobs,pending:None,results:HashMap::new(),phase:Phase::Loading,started:Instant::now(),log:vec!["Boot: reading original data files".into()],
            boot_image:None,boot_egui:None,layers:vec![],layers_expected:0,layers_ready:0,layers_empty:0,clip_handles:vec![],ground:None,build_wait:0,player:None,spawn:Vec3::ZERO,
            tri_count:0,controls_rows:0,combat_bindings:0,world_bounds:None,boot_shown:None,boot_shot:false,load_seconds:0.,selftest}
    }
}

fn in_game(mode:Res<AppMode>)->bool{*mode==AppMode::Game}
pub fn plugin(app:&mut App){
    app.init_resource::<GameInput>().insert_resource(Time::<Fixed>::from_hz(TICK_HZ as f64))
        .add_systems(Update,(pump,build,read_input,camera,animate,selftest).chain().run_if(in_game))
        .add_systems(bevy_egui::EguiPrimaryContextPass,hud.run_if(in_game))
        .add_systems(FixedUpdate,movement.run_if(in_game));
}

fn scene_path(v:&Value)->Option<String>{v["asset"].as_str().map(|s|s.to_owned())}

fn pump(mut commands:Commands,mut g:Option<ResMut<Game>>,assets:Res<AssetServer>,mut graphs:ResMut<Assets<AnimationGraph>>,mut settings:ResMut<WinitSettings>,mut redraw:MessageWriter<RequestRedraw>){
    let Some(g)=g.as_mut() else{return};let g=&mut **g;
    settings.focused_mode=UpdateMode::Continuous;settings.unfocused_mode=UpdateMode::Continuous;redraw.write(RequestRedraw);
    if g.phase!=Phase::Loading{return}
    let responses:Vec<Value>=g.bridge.rx.lock().unwrap().try_iter().collect();
    for r in responses{
        let Some((id,key))=g.pending.clone() else{continue};
        if r["id"].as_u64()!=Some(id){continue}
        g.pending=None;
        if r["ok"]!=true{let e=format!("{key}: {}",r["error"].as_str().unwrap_or("decoder error"));g.log.push(format!("FAILED {e}"));g.phase=Phase::Failed(e);return}
        let value=r["value"].clone();
        g.log.push(format!("loaded {key}"));
        match key.as_str(){
            "boot"=>{if let Some(p)=scene_path(&value){g.boot_image=Some(assets.load(p));}}
            "filelist"=>{
                // worldfilelist.csv (FILE_NAME,IS_HIGH): every row is a world layer; IS_HIGH=1 rows are the detailed area chunks,
                // the IS_HIGH=0 row is the whole-world mesh.
                let rows=value["rows"].as_array().cloned().unwrap_or_default();
                let big=src("world/world.big");
                for row in rows.iter().skip(1){
                    let (name,high)=(row[0].as_str().unwrap_or(""),row[1].as_str().unwrap_or("1"));
                    {for s in WORLD_SUFFIXES{
                        let file=if high=="1"{format!("{name}-high{s}.o")}else{format!("{name}{s}.o")}; // IS_HIGH=1 entries map to <name>-high[-variant].o inside world.big
                        g.jobs.push_back(Job{key:format!("layer:{file}"),req:json!({"command":"preview","source":format!("{big}::{file}")})});
                        g.layers_expected+=1;
                    }}
                }
            }
            "bounds"=>{if let Some(row)=value["rows"].as_array().and_then(|r|r.get(1)){let f=|i:usize|row[i].as_str().unwrap_or("0").parse::<f32>().unwrap_or(0.);g.world_bounds=Some((f(0),f(1),f(2)));}}
            "controls"=>{let rows=value["rows"].as_array().cloned().unwrap_or_default();g.controls_rows=rows.len().saturating_sub(1);g.combat_bindings=rows.iter().filter(|r|r[1]=="STATE_COMBAT").count();}
            k if k.starts_with("layer:")=>{
                if scene_path(&value).is_none(){
                    // Decoder reports an empty draw list (see FINDINGS.md: eight empty models); nothing to spawn.
                    g.layers_expected-=1;g.layers_empty+=1;g.log.push(format!("{} has an empty draw list; skipped",k.trim_start_matches("layer:")));
                }
                if let Some(p)=scene_path(&value){
                    let name=k.trim_start_matches("layer:").to_string();
                    let e=commands.spawn((GameEntity,WorldLayer(name.clone()),WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(p.clone()))),Transform::IDENTITY,
                        OriginalAsset{evidence:EvidenceLevel::AssetDerived,source:value["source"].as_str().unwrap_or("").into(),decoder_version:value["decoder_version"].as_str().unwrap_or("").into()})).observe(layer_ready).id();
                    let _=e;g.layers.push((name,p));
                }
            }
            k if k.starts_with("clip:")=>{
                let p=scene_path(&value).unwrap_or_default();
                g.clip_handles.push(assets.load(GltfAssetLabel::Animation(0).from_asset(p.clone())));
                g.results.insert(format!("scene:{}",k),json!(p));
            }
            _=>{}
        }
        g.results.insert(key,value);
    }
    if g.pending.is_none()&&g.jobs.is_empty()&&g.started.elapsed().as_secs_f32()>30.&&g.layers_ready<g.layers_expected&&g.layers_ready>0{
        // Never hang on the boot screen: continue with the layers that did arrive and say so.
        let (r,e)=(g.layers_ready,g.layers_expected);g.log.push(format!("WARNING: only {r}/{e} world layers became ready; continuing"));g.layers_expected=r;
    }
    if g.pending.is_none(){
        if let Some(job)=g.jobs.pop_front(){let id=g.bridge.request(job.req);g.pending=Some((id,job.key));}
        else if g.layers_ready>=g.layers_expected && g.clip_handles.len()==CLIPS.len(){
            let _=&mut graphs;
            let boot_done=g.boot_shown.map(|t|t.elapsed().as_secs_f32()>=MIN_BOOT_SECONDS).unwrap_or(g.boot_image.is_none());
            if boot_done{g.phase=Phase::Building;g.log.push("world and player assets ready".into());}
        }
    }
}

fn layer_ready(_e:On<WorldInstanceReady>,mut g:Option<ResMut<Game>>){if let Some(g)=g.as_mut(){g.layers_ready+=1;let n=g.layers_ready;g.log.push(format!("world layer ready ({n})"));}}

fn build(mut commands:Commands,mut g:Option<ResMut<Game>>,assets:Res<AssetServer>,meshes:Res<Assets<Mesh>>,layers:Query<(Entity,&WorldLayer)>,children:Query<&Children>,mesh_q:Query<(&Mesh3d,&GlobalTransform)>,mut graphs:ResMut<Assets<AnimationGraph>>,cam:Query<Entity,With<GameCamera>>){
    let Some(g)=g.as_mut() else{return};let g=&mut **g;
    if g.phase!=Phase::Building{return}
    g.build_wait+=1;if g.build_wait<3{return} // let transforms propagate
    let mut tris=Vec::new();let mut mesh_count=0;
    for (e,layer) in &layers{
        if !(layer.0.ends_with("-high.o")||layer.0=="world-low-all.o"){continue} // walkable surface: opaque base layers
        for d in children.iter_descendants(e){if let Ok((m,t))=mesh_q.get(d){
            let Some(mesh)=meshes.get(&m.0) else{continue};mesh_count+=1;
            let Some(pos)=mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a|a.as_float3()) else{continue};
            let idx:Vec<u32>=match mesh.indices(){Some(i)=>i.iter().map(|x|x as u32).collect(),None=>(0..pos.len() as u32).collect()};
            let mat=t.to_matrix();
            for tri in idx.chunks_exact(3){tris.push([0,1,2].map(|n|mat.transform_point3(Vec3::from(pos[tri[n] as usize]))));}
        }}
    }
    g.tri_count=tris.len();
    if tris.is_empty(){let m=format!("world mesh data unavailable ({mesh_count} meshes)");g.log.push(m.clone());g.phase=Phase::Failed(m);return}
    let ground=Ground::build(tris);
    // Provisional spawn (original spawn table not decoded): nearest point with walkable ground to the hub.
    let hub=Vec2::new(20.,-56.);
    let mut best:Option<(f32,Vec3)>=None;let mut covered=0;let mut total=0;
    for gx in -30..30{for gz in -30..30{
        let (x,z)=(hub.x+gx as f32*2.,hub.y+gz as f32*2.);total+=1;
        if let Some(y)=ground.height(x,z,50.){covered+=1;let d=Vec2::new(x,z).distance(hub);if best.is_none_or(|b|d<b.0){best=Some((d,Vec3::new(x,y,z)));}}
    }}
    g.log.push(format!("ground coverage around hub: {covered}/{total} samples"));
    g.spawn=best.map(|b|b.1).unwrap_or(Vec3::new(hub.x,-5.,hub.y));g.ground=Some(ground);
    g.load_seconds=g.started.elapsed().as_secs_f32();
    // Animation graph over the three decoded clips.
    let mut graph=AnimationGraph::new();
    let nodes:Vec<AnimationNodeIndex>=g.clip_handles.iter().map(|c|graph.add_clip(c.clone(),1.,graph.root)).collect();
    let handle=graphs.add(graph);
    let idle_scene=g.results.get("scene:clip:S_idle").and_then(|v|v.as_str()).map(|s|s.to_owned()).unwrap_or_default();
    let source=g.results["clip:S_idle"]["source"].as_str().unwrap_or("").to_string();
    let dv=g.results["clip:S_idle"]["decoder_version"].as_str().unwrap_or("").to_string();
    let player=commands.spawn((GameEntity,Player{loco:Locomotion::default(),vy:0.,grounded:true,speed:0.,facing:Vec3::Z,weights:[1.,0.,0.],anim_ready:false},Transform::from_translation(g.spawn),Visibility::default(),
        OriginalAsset{evidence:EvidenceLevel::AssetDerived,source,decoder_version:dv})).id();
    let nodes2=nodes.clone();let h2=handle.clone();
    commands.spawn((GameEntity,PlayerModel,ChildOf(player),WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(idle_scene))),Transform::default()))
        .observe(move|ev:On<WorldInstanceReady>,mut commands:Commands,children:Query<&Children>,mut players:Query<&mut AnimationPlayer>,mut pq:Query<&mut Player>|{
            for c in children.iter_descendants(ev.entity){if let Ok(mut p)=players.get_mut(c){
                for (i,n) in nodes2.iter().enumerate(){let a=p.play(*n);a.repeat();a.set_weight(if i==0{1.}else{0.});}
                commands.entity(c).insert(AnimationGraphHandle(h2.clone()));
                for mut pl in &mut pq{pl.anim_ready=true;}
            }}
        });
    g.player=Some(player);
    if cam.is_empty(){commands.spawn((GameEntity,GameCamera{yaw:0.,pitch:0.28,distance:k::HINGE_CAMERA_DISTANCE},Camera3d::default(),Transform::from_translation(g.spawn+Vec3::new(0.,2.,4.))));}
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
}

fn movement(g:Option<Res<Game>>,input:Res<GameInput>,cam:Query<&GameCamera>,mut q:Query<(&mut Player,&mut Transform)>){
    let Some(g)=g else{return};if g.phase!=Phase::Playing{return}
    let (Some(ground),Ok(cam))=(&g.ground,cam.single()) else{return};
    let dt=1./TICK_HZ;let dt_ms=(1000./TICK_HZ) as i32;
    for (mut p,mut t) in &mut q{
        let step=p.loco.update(dt_ms,InputKind::Digital,input.x,input.y,cam.yaw,k::STATE_MAX_SPEED);
        p.speed=step.speed;
        if let Some(a)=step.move_angle{
            // Camera at `yaw` looks along -(sin,cos); stick angle is measured from camera-forward.
            let f=Vec3::new(-cam.yaw.sin(),0.,-cam.yaw.cos());let r=Vec3::new(cam.yaw.cos(),0.,-cam.yaw.sin());
            let dir=f*a.cos()+r*a.sin();p.facing=dir;
            let delta=dir*step.speed*dt;
            let ny=t.translation.y;
            // Horizontal move is accepted if walkable ground exists within step-up height (display mesh; not original collision).
            for cand in [delta,Vec3::new(delta.x,0.,0.),Vec3::new(0.,0.,delta.z)]{
                let np=t.translation+cand;
                if let Some(gy)=ground.height(np.x,np.z,ny+STEP_UP){if !p.grounded||gy>=ny-1.5{t.translation.x=np.x;t.translation.z=np.z;break}}
            }
        }
        if input.jump&&p.grounded{p.vy=PROVISIONAL_JUMP_SPEED;p.grounded=false;}
        p.vy-=PROVISIONAL_GRAVITY*dt*(!p.grounded) as i32 as f32;
        let mut y=t.translation.y+p.vy*dt;
        match ground.height(t.translation.x,t.translation.z,y+STEP_UP){
            Some(gy) if y<=gy+0.02&&p.vy<=0.||p.grounded&&gy>=t.translation.y-0.5=>{y=gy;p.vy=0.;p.grounded=true}
            _=>{p.grounded=false}
        }
        t.translation.y=y;
        if y<g.spawn.y-40.{t.translation=g.spawn;p.vy=0.;}
    }
}

fn camera(mut c:Query<(&mut GameCamera,&mut Transform),Without<Player>>,p:Query<(&Player,&Transform),Without<GameCamera>>,keys:Res<ButtonInput<KeyCode>>,mouse:Res<ButtonInput<MouseButton>>,motion:Res<AccumulatedMouseMotion>,scroll:Res<AccumulatedMouseScroll>,time:Res<Time>){
    let (Ok((mut cam,mut ct)),Ok((player,pt)))=(c.single_mut(),p.single()) else{return};
    if mouse.pressed(MouseButton::Right)||mouse.pressed(MouseButton::Middle){cam.yaw-=motion.delta.x*0.005;cam.pitch=(cam.pitch+motion.delta.y*0.005).clamp(-0.2,1.3);}
    let key=|k:KeyCode|keys.pressed(k) as i32 as f32;
    cam.yaw+=(key(KeyCode::KeyQ)-key(KeyCode::KeyE))*1.8*time.delta_secs();
    cam.distance=(cam.distance*(-scroll.delta.y*0.1).exp()).clamp(1.2,12.);
    // EVENT_CAMERA_REORIENT (controls.csv: STATE_COMBAT, BUTTON B): swing behind the player.
    if keys.just_pressed(KeyCode::KeyR){cam.yaw=(-player.facing.x).atan2(-player.facing.z)+std::f32::consts::PI;}
    let target=pt.translation+Vec3::Y*k::FRAME_CAMERA_HEIGHT;
    let off=Vec3::new(cam.yaw.sin()*cam.pitch.cos(),cam.pitch.sin(),cam.yaw.cos()*cam.pitch.cos())*cam.distance;
    *ct=Transform::from_translation(target+off).looking_at(target,Vec3::Y);
}

fn animate(mut q:Query<(&mut Player,&Children)>,children:Query<&Children>,mut players:Query<&mut AnimationPlayer>,mut models:Query<&mut Transform,(With<PlayerModel>,Without<Player>)>,time:Res<Time>){
    for (mut p,kids) in &mut q{
        let target=if p.speed<0.1{[1.,0.,0.]}else if p.speed<0.75*k::STATE_MAX_SPEED{[0.,1.,0.]}else{[0.,0.,1.]};
        let blend=(time.delta_secs()*8.).min(1.);
        for i in 0..3{p.weights[i]+=(target[i]-p.weights[i])*blend;}
        let w=p.weights;
        for kid in kids.iter(){
            if let Ok(mut m)=models.get_mut(kid){
                // Model +Z is treated as forward until orientation is compared against the game.
                m.rotation=Quat::from_rotation_y(p.facing.x.atan2(p.facing.z));
            }
            for d in std::iter::once(kid).chain(children.iter_descendants(kid)){if let Ok(mut pl)=players.get_mut(d){
                for (i,weight) in w.iter().enumerate(){if let Some(a)=pl.animation_mut(AnimationNodeIndex::new(i+1)){a.set_weight(*weight);}}
            }}
        }
    }
}

fn hud(images:Res<Assets<Image>>,mut contexts:EguiContexts,mut g:Option<ResMut<Game>>,mut mode:ResMut<AppMode>,keys:Res<ButtonInput<KeyCode>>,time:Res<Time>,player:Query<(&Player,&Transform)>)->Result{
    let Some(g)=g.as_mut() else{return Ok(())};
    if g.boot_image.is_some()&&g.boot_egui.is_none(){if let Some(h)=g.boot_image.clone(){g.boot_egui=Some(contexts.add_image(EguiTextureHandle::Strong(h)));}}
    let ctx=contexts.ctx_mut()?;
    if keys.just_pressed(KeyCode::Escape){*mode=AppMode::Menu;return Ok(())}
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
            egui::Area::new(egui::Id::new("hud")).fixed_pos(egui::pos2(12.,12.)).show(ctx,|ui|{
                egui::Frame::popup(ui.style()).show(ui,|ui|{
                    ui.label(egui::RichText::new("EA PLAYGROUND — reconstruction slice").strong());
                    ui.label(format!("pos {:.1}, {:.1}, {:.1}   speed {:.2}/{:.1}   {:.0} fps",pos.x,pos.y,pos.z,speed,k::STATE_MAX_SPEED,1./time.delta_secs().max(1e-4)));
                    ui.label(format!("world: {} ground triangles • loaded in {:.1}s",g.tri_count,g.load_seconds));
                    if let Some((x,z,r))=g.world_bounds{ui.label(format!("world.csv bounds: min ({x}, {z}) radius {r}"));}
                    ui.label(format!("controls.csv: {} bindings ({} in STATE_COMBAT)",g.controls_rows,g.combat_bindings));
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(104,200,170),"Locomotion: ELF-derived (LocalCharacterControl::Update)");
                    ui.colored_label(egui::Color32::YELLOW,"Terrain: display mesh, not original collision");
                    ui.colored_label(egui::Color32::YELLOW,"Jump/gravity: provisional");
                    ui.label(egui::RichText::new("WASD/arrows move • Space jump • Q/E or right-drag camera • wheel zoom • R behind • Esc menu").small());
                });
            });
        }
    }
    Ok(())
}

fn selftest(mut commands:Commands,mut g:Option<ResMut<Game>>,mut input:ResMut<GameInput>,time:Res<Time>,player:Query<(&Player,&Transform)>,cam:Query<&Transform,With<GameCamera>>,meshes:Query<&Mesh3d>,players:Query<&AnimationPlayer>,mut exit:MessageWriter<AppExit>,_window:Query<&Window,With<PrimaryWindow>>){
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
    st.t+=time.delta_secs();
    let record=|st:&mut SelfTest,name:&str|{st.samples.push(json!({"step":name,"t":st.t,"pos":[t.translation.x,t.translation.y,t.translation.z],"speed":p.speed,"grounded":p.grounded,"weights":p.weights}));};
    input.scripted=true;
    // Scripted input sequence: settle, walk forward, run diagonally, jump, settle.
    let script=[(0.,0.,0.,false,1.5,"settle"),(0.,1.,0.,false,2.5,"forward"),(1.,1.,0.,false,2.0,"diagonal"),(0.,0.,0.,true,0.4,"jump"),(0.,0.,0.,false,1.0,"land")];
    if st.step==0&&st.t<1e-3+time.delta_secs()*2.{st.start=t.translation;}
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
                check("world layers spawned from worldfilelist.csv",g.layers_ready==g.layers_expected&&g.layers_expected+g.layers_empty==WORLD_SUFFIXES.len()*5,format!("{}/{} layers ready, {} empty draw lists skipped",g.layers_ready,g.layers_expected,g.layers_empty));
                check("world geometry decoded",g.tri_count>20_000,format!("{} walkable-source triangles",g.tri_count));
                check("player model + 3 clips loaded",g.clip_handles.len()==CLIPS.len()&&p.anim_ready,format!("{} clips, anim player bound: {}",g.clip_handles.len(),p.anim_ready));
                check("controls.csv parsed",g.combat_bindings>=8,format!("{} rows, {} STATE_COMBAT",g.controls_rows,g.combat_bindings));
                check("mesh entities in scene",meshes.iter().count()>100,format!("{} Mesh3d entities",meshes.iter().count()));
                check("animation players active",players.iter().count()>=1,format!("{} players",players.iter().count()));
                check("player walked on recovered locomotion",moved>5.,format!("moved {:.2} m horizontally (5 m/s max for 4.5 s)",moved));
                check("walk/run clip blended in while moving",walk_w>0.5,format!("peak walk+run weight {walk_w:.2} (full stick = run)"));
                check("player grounded on world surface",p.grounded,format!("y={:.2}",t.translation.y));
                let jumped=st.samples.iter().any(|s|s["step"]=="jump"||s["step"]=="land");
                check("jump script executed",jumped,"jump/land samples recorded".into());
                check("camera follows player",cam.single().map(|c|c.translation.distance(t.translation)<15.).unwrap_or(false),"camera within 15 m".into());
                let passed=checks.iter().all(|c|c["ok"]==true);
                let report=json!({"passed":passed,"load_seconds":g.load_seconds,"checks":checks,"samples":st.samples,"log":g.log,"evidence":{"locomotion":"ExecutableDerived: LocalCharacterControl::Update 0x802eeb28, constants from recovered.rs","terrain":"AssetDerived display mesh","jump_gravity":"provisional"}});
                let _=std::fs::write(st.out.join("selftest.json"),serde_json::to_string_pretty(&report).unwrap());
                exit.write(if passed{AppExit::Success}else{AppExit::from_code(1)});
            }
        }
    }
}

impl Game{pub fn is_playing(&self)->bool{self.phase==Phase::Playing}}

/// Leave game mode: drop the loader and all game entities.
pub fn teardown(commands:&mut Commands,entities:&Query<Entity,With<GameEntity>>){
    for e in entities{commands.entity(e).try_despawn();}
    commands.remove_resource::<Game>();
}
