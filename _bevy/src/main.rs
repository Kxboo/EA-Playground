#![cfg_attr(target_os="windows",windows_subsystem="windows")]
mod bridge;
mod viewer;
mod game;
mod menu;
mod locomotion;
mod recovered;
mod vlt;
mod havok;
mod sha256;
mod archive;
mod gsh;
mod tpl;
mod locale;
mod mp3_tables;
mod mp3;
mod audio;
mod placement;
mod conversation;
mod model;
mod assets;
mod skeleton;
mod anim;
mod preview;
mod character;

use bevy::{prelude::*,render::view::window::screenshot::{Screenshot,save_to_disk},winit::WinitSettings};
use bevy_egui::{EguiPlugin,EguiPrimaryContextPass};
use serde_json::json;
use std::time::Instant;

#[derive(Resource)] struct RunClock(Instant);
/// `--selftest <dir>`: boot straight into the game, run a scripted input sequence, write a report, exit.
#[derive(Resource)] pub struct SelfTestOut(pub Option<std::path::PathBuf>);
/// `--shot <png> [--shot-at secs]`: screenshot the current mode (menu/proof) and exit.
/// `--flow-test`: drive menu->game->menu->viewer->menu->game and report that every hop worked.
#[derive(Resource)] struct Flow{on:bool,step:usize,since:f32,log:Vec<String>}
#[derive(Resource)] struct Shot{path:Option<String>,at:f32,taken:bool}

fn main(){
    let args:Vec<String>=std::env::args().collect();
    let arg=|key:&str|args.iter().position(|a|a==key).and_then(|i|args.get(i+1)).cloned();
    if let Some(command)=arg("--headless"){
        let mut req=json!({"command":command});
        for key in ["source","model","skeleton","bank","out","offset","length","minimum"]{if let Some(v)=arg(&format!("--{key}")){req[key]=json!(v);}}
        if args.iter().any(|a|a=="--deep"){req["deep"]=json!(true);}
        if let Some(i)=arg("--index"){req["index"]=json!(i.parse::<usize>().unwrap_or(0));}
        match bridge::headless(req){Ok(v)=>{println!("{}",serde_json::to_string_pretty(&v).unwrap());if v["ok"]!=true || v["value"]["exit_code"].as_u64().unwrap_or(0)!=0{std::process::exit(2)}},Err(e)=>{eprintln!("{e}");std::process::exit(1)}}
        return
    }
    if let Some(input)=arg("--decode-audio"){
        // --decode-audio <in.asf/.ast> --out <out.wav>: decode an EA Layer 3 stream to a WAV file.
        let out=arg("--out").expect("--out <file.wav>");
        let d=std::fs::read(&input).expect("input");
        // A file may hold several streams (.ast banks): --stream <n> picks one (default 0).
        let list=audio::streams(&d);let which=arg("--stream").and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);
        eprintln!("{} stream(s) in {input}",list.len());
        let (a,b)=*list.get(which).expect("no such stream");
        match audio::decode(&d[a..b],arg("--blocks").and_then(|b|b.parse().ok())){
            Ok(p)=>{std::fs::write(&out,audio::to_wav(&p)).expect("write");eprintln!("{} samples/ch at {} Hz, {} channels, {} frames, stats {:?}",p.samples.len()/p.channels,p.sample_rate,p.channels,p.frames,p.stats);}
            Err(e)=>{eprintln!("{e}");std::process::exit(1)}
        }
        return
    }
    if let Some(out)=arg("--vlt-dump"){
        let dir=bridge::data_root().join("files").join("data").join("db");
        let (v,b)=(std::fs::read(dir.join("db.vlt")).expect("db.vlt"),std::fs::read(dir.join("db.bin")).expect("db.bin"));
        let db=vlt::Database::load(&v,&b,vlt::known_names()).expect("vault load");
        std::fs::write(&out,serde_json::to_string_pretty(&db.to_json()).unwrap()).expect("write");
        eprintln!("wrote {out}: {} types, {} classes, {} collections",db.types.len(),db.classes.len(),db.collections.len());return
    }
    let mut state=viewer::Viewer::default();
    state.startup_asset=arg("--asset").or(Some("basketball.o".into()));
    state.startup_bank=arg("--bank");state.startup_clip=arg("--clip").and_then(|s|s.parse().ok()).unwrap_or(0);
    state.capture=arg("--capture");state.capture_at=arg("--capture-after").and_then(|s|s.parse().ok()).unwrap_or(15.);
    let selftest=arg("--selftest").map(std::path::PathBuf::from);
    let start=match arg("--mode").as_deref(){Some("viewer")=>menu::AppMode::Viewer,Some("game")=>menu::AppMode::Game,Some("proof")=>menu::AppMode::Proof,Some(_)|None=>if selftest.is_some(){menu::AppMode::Game}else if state.capture.is_some()||arg("--asset").is_some(){menu::AppMode::Viewer}else{menu::AppMode::Menu}};
    let bridge=bridge::Bridge::start();bridge.catalog();
    App::new()
        .insert_resource(start).insert_resource(SelfTestOut(selftest)).insert_resource(Flow{on:std::env::args().any(|a|a=="--flow-test"),step:0,since:0.,log:vec![]}).insert_resource(Shot{path:arg("--shot"),at:arg("--shot-at").and_then(|s|s.parse().ok()).unwrap_or(3.),taken:false}).init_resource::<menu::Proof>()
        .insert_resource(state).insert_resource(bridge).insert_resource(viewer::Native::new()).insert_resource(viewer::Orbit::default())
        .insert_resource(RunClock(Instant::now()))
        .insert_resource(ClearColor(Color::srgb(0.055,0.066,0.082)))
        .insert_resource(GlobalAmbientLight{color:Color::WHITE,brightness:800.,..default()})
        .insert_resource(WinitSettings::desktop_app())
        .add_plugins(DefaultPlugins.set(WindowPlugin{primary_window:Some(Window{title:"EA Playground — Bevy Asset Workbench".into(),resolution:(1440,900).into(),..default()}),..default()})
            .set(AssetPlugin{file_path:bridge::root().join("assets").to_string_lossy().into_owned(),..default()}))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup,viewer::setup)
        .add_plugins(game::plugin).add_systems(Update,(shot,flow))
        .add_systems(Update,(menu::on_mode_change,(viewer::receive,viewer::animate).chain().run_if(in_mode(menu::AppMode::Viewer))).chain())
        .add_systems(EguiPrimaryContextPass,(viewer::ui.run_if(in_mode(menu::AppMode::Viewer)),menu::ui.run_if(in_mode(menu::AppMode::Menu).or(in_mode(menu::AppMode::Proof)))))
        .add_systems(PostUpdate,(viewer::bones.run_if(in_mode(menu::AppMode::Viewer)),capture).after(TransformSystems::Propagate))
        .run();
}

fn shot(mut commands:Commands,mut s:ResMut<Shot>,clock:Res<RunClock>,mut exit:MessageWriter<AppExit>){
    let Some(path)=s.path.clone() else{return};let t=clock.0.elapsed().as_secs_f32();
    if t>=s.at&&!s.taken{if let Some(p)=std::path::Path::new(&path).parent(){std::fs::create_dir_all(p).ok();}commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));s.taken=true;}
    if s.taken&&t>s.at+2.{exit.write(AppExit::Success);}
}

fn flow(mut f:ResMut<Flow>,mut mode:ResMut<menu::AppMode>,clock:Res<RunClock>,g:Option<Res<game::Game>>,v:Res<viewer::Viewer>,game_entities:Query<(),With<game::GameEntity>>,mut exit:MessageWriter<AppExit>){
    if !f.on{return}
    let t=clock.0.elapsed().as_secs_f32();if f.since==0.{f.since=t}
    let dwell=t-f.since;
    let playing=g.as_ref().is_some_and(|g|g.is_playing());
    let mut hop=|f:&mut Flow,mode:&mut menu::AppMode,to:menu::AppMode,note:String|{f.log.push(format!("{t:.1}s: {note} -> {to:?}"));*mode=to;f.step+=1;f.since=t;};
    match f.step{
        0 if dwell>1.=>hop(&mut f,&mut mode,menu::AppMode::Game,"menu shown".into()),
        1 if playing&&dwell>3.=>hop(&mut f,&mut mode,menu::AppMode::Menu,"game playing".into()),
        2 if dwell>1.&&game_entities.is_empty()&&g.is_none()=>hop(&mut f,&mut mode,menu::AppMode::Viewer,"game torn down (no entities, no loader)".into()),
        3 if v.loaded&&dwell>2.=>hop(&mut f,&mut mode,menu::AppMode::Menu,format!("viewer loaded '{}'",v.status)),
        4 if dwell>1.=>hop(&mut f,&mut mode,menu::AppMode::Game,"viewer closed".into()),
        5 if playing&&dwell>3.=>{f.log.push(format!("{t:.1}s: game playing again after re-entry"));let ok=true;
            let _=std::fs::write("docs/flow-test.json",serde_json::to_string_pretty(&json!({"passed":ok,"log":f.log})).unwrap());exit.write(AppExit::Success);}
        _=>{if dwell>90.{let st=f.step;f.log.push(format!("timeout at step {st}"));let _=std::fs::write("docs/flow-test.json",serde_json::to_string_pretty(&json!({"passed":false,"log":f.log})).unwrap());exit.write(AppExit::from_code(4));}}
    }
}

fn in_mode(m:menu::AppMode)->impl Fn(Res<menu::AppMode>)->bool+Clone{move|cur:Res<menu::AppMode>|*cur==m}

fn capture(mut commands:Commands,mut v:ResMut<viewer::Viewer>,clock:Res<RunClock>,meshes:Query<&Mesh3d>,players:Query<&AnimationPlayer>,mut exit:MessageWriter<AppExit>){
    if v.capture.is_none(){return}
    let elapsed=clock.0.elapsed().as_secs_f32();
    if elapsed>=v.capture_at && v.loaded && !v.busy && !v.captured{
        let path=v.capture.clone().unwrap();if let Some(p)=std::path::Path::new(&path).parent(){std::fs::create_dir_all(p).ok();}
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        let report=json!({"loaded":v.loaded,"status":v.status,"mesh_entities":meshes.iter().count(),"animation_players":players.iter().count(),"animation_time":v.position,"preview":v.report});
        let _=std::fs::write(format!("{path}.json"),serde_json::to_string_pretty(&report).unwrap());
        v.captured=true;
    }
    if (v.captured && elapsed>v.capture_at+5.) || elapsed>120.{exit.write(AppExit::Success);}
}
