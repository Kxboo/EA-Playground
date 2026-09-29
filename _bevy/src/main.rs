#![cfg_attr(target_os="windows",windows_subsystem="windows")]
mod bridge;
mod viewer;
mod game;
mod menu;
mod locomotion;
mod recovered;

use bevy::{prelude::*,render::view::window::screenshot::{Screenshot,save_to_disk},winit::WinitSettings};
use bevy_egui::{EguiPlugin,EguiPrimaryContextPass};
use serde_json::json;
use std::time::Instant;

#[derive(Resource)] struct RunClock(Instant);
/// `--selftest <dir>`: boot straight into the game, run a scripted input sequence, write a report, exit.
#[derive(Resource)] pub struct SelfTestOut(pub Option<std::path::PathBuf>);

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
    let mut state=viewer::Viewer::default();
    state.startup_asset=arg("--asset").or(Some("basketball.o".into()));
    state.startup_bank=arg("--bank");state.startup_clip=arg("--clip").and_then(|s|s.parse().ok()).unwrap_or(0);
    state.capture=arg("--capture");state.capture_at=arg("--capture-after").and_then(|s|s.parse().ok()).unwrap_or(15.);
    let selftest=arg("--selftest").map(std::path::PathBuf::from);
    let start=match arg("--mode").as_deref(){Some("viewer")=>menu::AppMode::Viewer,Some("game")=>menu::AppMode::Game,Some("proof")=>menu::AppMode::Proof,Some(_)|None=>if selftest.is_some(){menu::AppMode::Game}else if state.capture.is_some()||arg("--asset").is_some(){menu::AppMode::Viewer}else{menu::AppMode::Menu}};
    let bridge=bridge::Bridge::start();bridge.catalog();
    App::new()
        .insert_resource(start).insert_resource(SelfTestOut(selftest)).init_resource::<menu::Proof>()
        .insert_resource(state).insert_resource(bridge).insert_resource(viewer::Orbit::default())
        .insert_resource(RunClock(Instant::now()))
        .insert_resource(ClearColor(Color::srgb(0.055,0.066,0.082)))
        .insert_resource(GlobalAmbientLight{color:Color::WHITE,brightness:800.,..default()})
        .insert_resource(WinitSettings::desktop_app())
        .add_plugins(DefaultPlugins.set(WindowPlugin{primary_window:Some(Window{title:"EA Playground — Bevy Asset Workbench".into(),resolution:(1440,900).into(),..default()}),..default()})
            .set(AssetPlugin{file_path:bridge::root().join("assets").to_string_lossy().into_owned(),..default()}))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup,viewer::setup)
        .add_plugins(game::plugin)
        .add_systems(Update,(menu::on_mode_change,(viewer::receive,viewer::animate).chain().run_if(in_mode(menu::AppMode::Viewer))).chain())
        .add_systems(EguiPrimaryContextPass,(viewer::ui.run_if(in_mode(menu::AppMode::Viewer)),menu::ui.run_if(in_mode(menu::AppMode::Menu).or(in_mode(menu::AppMode::Proof)))))
        .add_systems(PostUpdate,(viewer::bones.run_if(in_mode(menu::AppMode::Viewer)),capture).after(TransformSystems::Propagate))
        .run();
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
