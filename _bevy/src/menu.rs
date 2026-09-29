//! Front menu: asset viewer, the reconstructed game, and the proof-of-decode report.
use bevy::{prelude::*,camera::visibility::RenderLayers,window::PrimaryWindow};
use bevy_egui::{egui,EguiContexts};
use serde_json::Value;
use std::sync::{Arc,Mutex};
use crate::{bridge,game,viewer};

#[derive(Resource,Clone,Copy,PartialEq,Eq,Debug)]
pub enum AppMode{Menu,Viewer,Game,Proof}

#[derive(Resource,Default)]
pub struct Proof{report:Option<Value>,running:Arc<Mutex<Option<String>>>,loaded:bool}

fn report_path()->std::path::PathBuf{bridge::root().join("docs").join("proof-report.json")}
fn load_report(p:&mut Proof){p.report=std::fs::read_to_string(report_path()).ok().and_then(|s|serde_json::from_str(&s).ok());p.loaded=true;}

fn run_prove(p:&Proof){
    let running=p.running.clone();
    if running.lock().unwrap().is_some(){return}
    *running.lock().unwrap()=Some("running".into());
    std::thread::spawn(move||{
        let base=bridge::root();
        let mut c=std::process::Command::new("py");c.args(["-3.14","-u"]).arg(base.join("tools/prove.py")).current_dir(&base);
        #[cfg(windows)] {use std::os::windows::process::CommandExt;c.creation_flags(0x08000000);}
        let out=c.output();
        *running.lock().unwrap()=Some(match out{Ok(o)=>if o.status.success(){"done".into()}else{format!("finished with failures (exit {:?})",o.status.code())},Err(e)=>format!("could not start python: {e}")});
    });
}

pub fn ui(mut contexts:EguiContexts,mut mode:ResMut<AppMode>,mut proof:ResMut<Proof>,window:Single<&Window,With<PrimaryWindow>>,mut exit:MessageWriter<AppExit>)->Result{
    let ctx=contexts.ctx_mut()?;
    let _=window;
    let mut root=egui::Ui::new(ctx.clone(),"menu".into(),egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()));
    let accent=egui::Color32::from_rgb(104,200,170);
    if *mode==AppMode::Proof{
        if !proof.loaded{load_report(&mut proof);}
        let state=proof.running.lock().unwrap().clone();
        if state.as_deref().is_some_and(|s|s!="running")&&proof.loaded{ if let Some(s)=state.as_deref(){if s.starts_with("done")||s.starts_with("finished"){load_report(&mut proof);*proof.running.lock().unwrap()=Some(format!("last run: {s}"));}}}
        egui::CentralPanel::default().show(&mut root,|ui|{
            ui.horizontal(|ui|{
                if ui.button("← Menu").clicked(){*mode=AppMode::Menu;}
                ui.heading("Proof of decode");
                let running=proof.running.lock().unwrap().as_deref()==Some("running");
                if ui.add_enabled(!running,egui::Button::new("Run verification")).clicked(){run_prove(&proof);}
                if running{ui.spinner();ui.label("running tools/prove.py…");}
                else if let Some(s)=proof.running.lock().unwrap().as_ref(){ui.label(s);}
            });
            ui.separator();
            match &proof.report{
                None=>{ui.label("No report yet. Press “Run verification” (needs Python 3.14 and the game files).");}
                Some(r)=>{
                    let passed=r["passed"]==true;
                    ui.horizontal(|ui|{
                        ui.colored_label(if passed{accent}else{egui::Color32::LIGHT_RED},if passed{"ALL CHECKS PASSED"}else{"SOME CHECKS FAILED"});
                        ui.label(format!("{} / {} checks • {}",r["passed_count"],r["total"],r["generated"].as_str().unwrap_or("")));
                    });
                    ui.label(egui::RichText::new(format!("ELF SHA-256 {}",r["elf_sha256"].as_str().unwrap_or("?"))).small().monospace());
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui,|ui|{
                        for c in r["checks"].as_array().cloned().unwrap_or_default(){
                            let ok=c["ok"]==true;
                            ui.horizontal_wrapped(|ui|{
                                ui.colored_label(if ok{accent}else{egui::Color32::LIGHT_RED},if ok{"✔"}else{"✘"});
                                ui.label(egui::RichText::new(c["name"].as_str().unwrap_or("")).strong());
                            });
                            ui.label(egui::RichText::new(c["detail"].as_str().unwrap_or("")).small().color(egui::Color32::GRAY));
                            ui.add_space(4.);
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("Scope").strong());
                        for l in r["scope"].as_array().cloned().unwrap_or_default(){ui.label(egui::RichText::new(l.as_str().unwrap_or("")).small());}
                    });
                }
            }
        });
        return Ok(())
    }
    egui::CentralPanel::default().show(&mut root,|ui|{
        ui.vertical_centered(|ui|{
            ui.add_space((ui.available_height()*0.18).max(20.));
            ui.label(egui::RichText::new("EA PLAYGROUND").size(44.).strong());
            ui.label(egui::RichText::new("REVERSE-ENGINEERING WORKBENCH • BEVY").color(accent));
            ui.add_space(28.);
            let big=|t:&str|egui::Button::new(egui::RichText::new(t).size(20.)).min_size(egui::vec2(340.,52.));
            if ui.add(big("Asset Viewer")).on_hover_text("Browse and preview every decoded model, texture, animation and table").clicked(){*mode=AppMode::Viewer;}
            ui.add_space(8.);
            if ui.add(big("Play — reconstructed game")).on_hover_text("Boot from the original files: world, player, recovered locomotion").clicked(){*mode=AppMode::Game;}
            ui.add_space(8.);
            if ui.add(big("Proof of decode")).on_hover_text("Executable/data verification report").clicked(){*mode=AppMode::Proof;}
            ui.add_space(8.);
            if ui.add(big("Quit")).clicked(){exit.write(AppExit::Success);}
            ui.add_space(30.);
            let data=bridge::data_root();
            ui.label(egui::RichText::new(format!("DATA: {}  {}",data.display(),if data.exists(){"✔"}else{"✘ not found"})).small().color(egui::Color32::GRAY));
            ui.label(egui::RichText::new("Locomotion constants come from playgroundz.elf; terrain, jump and camera framing are provisional.\nSee docs/RECONSTRUCTION.md for the evidence policy.").small().color(egui::Color32::GRAY));
        });
    });
    Ok(())
}

/// Reconcile cameras/entities with the active mode. Runs every frame; acts only on change.
pub fn on_mode_change(mode:Res<AppMode>,mut commands:Commands,mut v:ResMut<viewer::Viewer>,roots:Query<Entity,With<viewer::PreviewRoot>>,game_entities:Query<Entity,With<game::GameEntity>>,mut preview:Single<&mut Camera,With<viewer::PreviewCamera>>,mut last:Local<Option<AppMode>>,mut light:Query<&mut Visibility,With<viewer::PreviewLight>>,selftest:Res<crate::SelfTestOut>){
    if *last==Some(*mode){return}
    let previous=*last;*last=Some(*mode);
    if previous==Some(AppMode::Game){game::teardown(&mut commands,&game_entities);}
    if *mode!=AppMode::Viewer{
        for e in &roots{commands.entity(e).despawn();}
        v.loaded=false;v.busy=false;v.image=None;v.image_id=None;
        preview.viewport=None;
    }
    preview.is_active=*mode!=AppMode::Game;
    for mut vis in &mut light{*vis=if *mode==AppMode::Game{Visibility::Hidden}else{Visibility::Inherited};}
    if *mode==AppMode::Game{commands.insert_resource(game::Game::new(selftest.0.clone().map(game::SelfTest::new)));}
    let _=RenderLayers::none();
}
