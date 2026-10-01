#![cfg_attr(target_os="windows",windows_subsystem="windows")]
mod bridge;
mod kid_pick;
mod fe_sfx;
mod viewer;
mod game;
mod menu;
mod locomotion;
mod sim_time;
mod multiplayer;
mod controller;
mod control_bindings;
mod character_input;
mod character_movement;
mod area_transform;
mod tetherball;
mod tetherball_scene;
mod tetherball_match;
mod tetherball_lifecycle;
mod tetherball_tuning;
mod tetherball_reset;
mod tetherball_gestures;
mod tetherball_angles;
mod tetherball_serve;
mod tetherball_rally_rules;
mod tetherball_hit_animation;
mod tetherball_hit;
mod tetherball_rally;
mod tetherball_ai;
mod tetherball_ai_reset;
mod tetherball_ai_hit;
mod tetherball_ai_move;
mod tetherball_animation_init;
mod tetherball_reset_runtime;
mod tetherball_frontend;
mod tetherball_runtime;
mod tetherball_initialize;
mod tetherball_player_init;
mod tetherball_ai_init;
mod tetherball_additional_player;
mod tetherball_ball_init;
mod tetherball_startup;
mod tetherball_shadow_setup;
mod tetherball_play;
mod tb_anim;
mod tb_host;
mod apt;
mod apt_player;
mod apt_view;
mod apt_vm;
mod apt_lib;
mod apt_anim;
mod fe_host;
mod apt_geom;
mod fntg;
mod apt_mat;
mod apt_text;
mod recovered;
mod vlt;
mod havok;
mod sha256;
mod archive;
mod gsh;
mod tpl;
mod locale;
mod conga;
mod mp3_tables;
mod mp3;
mod audio;
mod utk;
mod playback;
mod placement;
mod conversation;
mod model;
mod assets;
mod skeleton;
mod anim;
mod preview;
mod character;
mod apt_dump;
mod export;
mod corpus;
mod formats;
mod formats2;
mod nw4r;
mod aems;
mod ppc;
mod vp6;
mod vp6_tables;

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
        if input.to_lowercase().ends_with(".bnk")||input.to_lowercase().ends_with(".abk"){
            let which=arg("--stream").and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);
            let bank=if input.to_lowercase().ends_with(".abk"){match audio::abk_bank(&d){Ok(Some(b))=>b,Ok(None)=>{eprintln!("module bank without samples");std::process::exit(1)}Err(e)=>{eprintln!("{e}");std::process::exit(1)}}}else{&d[..]};
            if std::env::args().any(|a|a=="--list"){
                if let Ok(list)=audio::parse_bank(bank){for (i,s) in list.iter().enumerate(){eprintln!("{i}: codec {:#x} {} ch {} Hz {} samples {:.2}s",s.header.codec,s.header.channels,s.header.sample_rate,s.header.samples,s.header.samples as f32/s.header.sample_rate.max(1) as f32);}}
                return
            }
            match audio::decode_bank_sound(bank,which){Ok(p)=>{std::fs::write(&out,audio::to_wav(&p)).expect("write");eprintln!("{} samples at {} Hz",p.samples.len(),p.sample_rate);}Err(e)=>{eprintln!("{e}");std::process::exit(1)}}
            return
        }
        let list=audio::streams(&d);let which=arg("--stream").and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);
        eprintln!("{} stream(s) in {input}",list.len());
        let (a,b)=*list.get(which).expect("no such stream");
        let res=match audio::parse_header(&d[a..b]).map(|h|h.0.codec){Ok(0x0a)=>audio::decode_xa(&d[a..b]),Ok(0x04)=>audio::decode_utk(&d[a..b]),_=>audio::decode(&d[a..b],arg("--blocks").and_then(|b|b.parse().ok()))};
        match res{
            Ok(p)=>{std::fs::write(&out,audio::to_wav(&p)).expect("write");eprintln!("{} samples/ch at {} Hz, {} channels, {} frames, stats {:?}",p.samples.len()/p.channels,p.sample_rate,p.channels,p.frames,p.stats);}
            Err(e)=>{eprintln!("{e}");std::process::exit(1)}
        }
        return
    }
    if let Some(input)=arg("--vp6-debug"){
        // --vp6-debug <movie.vp6> --out <dir> [--frames N]: decode frames to PNG until the first error.
        let out=std::path::PathBuf::from(arg("--out").expect("--out <dir>"));std::fs::create_dir_all(&out).unwrap();
        let limit=arg("--frames").and_then(|s|s.parse::<usize>().ok()).unwrap_or(usize::MAX);
        let d=std::fs::read(&input).expect("input");let m=vp6::movie(&d).expect("movie");
        let mut dec=vp6::Decoder::new();let mut n=0;
        for (tag,at,size) in &m.chunks{
            if !(tag==b"MV0K"||tag==b"MV0F"){continue}
            if n>=limit{break}
            match dec.decode(&d[at+8..at+size]){
                Ok(k)=>{let c=dec.current().unwrap();let (w,h)=dec.display;std::fs::write(out.join(format!("{n:05}.png")),export::png(&c.rgba(w,h),w,h).unwrap()).unwrap();eprintln!("frame {n} ok key={k} consumed {:?}",dec.consumed);}
                Err(e)=>{eprintln!("frame {n}: {e}");break}
            }
            n+=1;
        }
        return
    }
    if let Some(out)=arg("--decode-all"){
        // --decode-all <out dir> [--data <DATA>] [--only ext,ext]: native decode of the complete game data.
        let data=arg("--data").map(std::path::PathBuf::from).unwrap_or_else(bridge::data_root);
        let only=arg("--only").map(|s|s.split(',').map(|e|e.trim().trim_start_matches('.').to_lowercase()).collect());
        match corpus::run(&data,std::path::Path::new(&out),only){
            Ok(r)=>{let bad=r.iter().filter(|x|x.duplicate_of.is_none()&&matches!(x.status,corpus::Status::Failed|corpus::Status::Unsupported)).count();eprintln!("{} records written to {out}; {bad} distinct records not decoded (see REPORT.md)",r.len());}
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
    let start=match arg("--mode").as_deref(){Some("viewer")=>menu::AppMode::Viewer,Some("game")=>menu::AppMode::Game,Some("tetherball")=>menu::AppMode::Tetherball,Some("apt")=>menu::AppMode::Apt,Some("proof")=>menu::AppMode::Proof,Some("menu")=>menu::AppMode::Menu,Some(_)|None=>if selftest.is_some(){menu::AppMode::Game}else if state.capture.is_some()||arg("--asset").is_some(){menu::AppMode::Viewer}else{menu::AppMode::Apt}};
    let bridge=bridge::Bridge::start();bridge.catalog();
    App::new()
        .insert_resource(start).insert_resource(SelfTestOut(selftest)).insert_resource(Flow{on:std::env::args().any(|a|a=="--flow-test"),step:0,since:0.,log:vec![]}).insert_resource(Shot{path:arg("--shot"),at:arg("--shot-at").and_then(|s|s.parse().ok()).unwrap_or(3.),taken:false}).init_resource::<menu::Proof>()
        .insert_resource(state).insert_resource(bridge).insert_resource(viewer::Native::new()).insert_resource(viewer::Orbit::default())
        .insert_resource(RunClock(Instant::now()))
        .insert_resource(ClearColor(Color::srgb(0.055,0.066,0.082)))
        .insert_resource(GlobalAmbientLight{color:Color::WHITE,brightness:800.,..default()})
        .insert_resource(WinitSettings::desktop_app())
        .add_plugins(DefaultPlugins.set(WindowPlugin{primary_window:Some(Window{title:"EA Playground".into(),resolution:(1280,720).into(),..default()}),..default()})
            .set(AssetPlugin{file_path:bridge::root().join("assets").to_string_lossy().into_owned(),..default()}))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup,viewer::setup)
        .add_plugins(game::plugin).add_plugins(tetherball_play::plugin).add_plugins(apt_view::plugin).add_plugins(kid_pick::plugin).insert_resource(menu::AptStart(arg("--apt").unwrap_or_else(||"main".into()))).add_systems(Update,(shot,flow))
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
