#![cfg_attr(target_os="windows",windows_subsystem="windows")]
mod bridge;
mod viewer;
mod game;

use bevy::{prelude::*,render::view::window::screenshot::{Screenshot,save_to_disk},winit::WinitSettings};
use bevy_egui::{EguiPlugin,EguiPrimaryContextPass};
use serde_json::json;
use std::time::Instant;

#[derive(Resource)] struct RunClock(Instant);

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
    let bridge=bridge::Bridge::start();bridge.catalog();
    App::new()
        .insert_resource(state).insert_resource(bridge).insert_resource(viewer::Orbit::default())
        .insert_resource(RunClock(Instant::now()))
        .insert_resource(ClearColor(Color::srgb(0.055,0.066,0.082)))
        .insert_resource(GlobalAmbientLight{color:Color::WHITE,brightness:800.,..default()})
        .insert_resource(WinitSettings::desktop_app())
        .add_plugins(DefaultPlugins.set(WindowPlugin{primary_window:Some(Window{title:"EA Playground — Bevy Asset Workbench".into(),resolution:(1440,900).into(),..default()}),..default()})
            .set(AssetPlugin{file_path:bridge::root().join("assets").to_string_lossy().into_owned(),..default()}))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup,viewer::setup)
        .add_systems(Update,(viewer::receive,viewer::animate).chain())
        .add_systems(EguiPrimaryContextPass,viewer::ui)
        .add_systems(PostUpdate,(viewer::bones,capture).after(TransformSystems::Propagate))
        .run();
}

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
