use bevy::{prelude::*,camera::{Viewport,visibility::RenderLayers,CameraOutputMode},render::render_resource::BlendState,window::{PrimaryWindow,RequestRedraw},winit::{WinitSettings,UpdateMode},world_serialization::WorldInstanceReady};
use bevy_egui::{egui,EguiContexts,EguiTextureHandle,EguiGlobalSettings,PrimaryEguiContext};
use serde::Deserialize;
use serde_json::{Value,json};
use std::{time::Duration,sync::Arc};
use crate::{bridge::Bridge,game::OriginalAsset,menu::AppMode};

#[derive(Clone,Deserialize)]
pub struct AssetRow {pub id:usize,pub name:String,pub source:String,pub kind:String,pub status:String,pub size:u64,#[serde(default)]pub family:String}

#[derive(Resource)]
pub struct Viewer {
    pub assets:Arc<Vec<AssetRow>>,pub selected:Option<usize>,pub query:String,pub category:String,
    pub model:Option<usize>,pub skeleton:Option<usize>,pub bank:Option<usize>,pub texture:Option<usize>,pub with_model:bool,pub with_animation:bool,
    pub index:usize,pub items:Vec<Value>,pub item_filter:String,pub busy:bool,pub latest:u64,pub status:String,pub detail:String,pub report:Value,
    pub playing:bool,pub speed:f32,pub position:f32,pub duration:f32,pub seek:Option<f32>,pub bones:bool,pub z_up:bool,pub unlit:bool,
    pub image:Option<Handle<Image>>,pub image_id:Option<egui::TextureId>,pub zoom:f32,
    pub authored_unlit:std::collections::HashMap<AssetId<StandardMaterial>,bool>,
    pub loaded:bool,pub inspector:bool,pub old_images:Vec<AssetId<Image>>,pub startup_asset:Option<String>,pub startup_bank:Option<String>,pub startup_clip:usize,pub capture:Option<String>,pub capture_at:f32,pub captured:bool,
}
impl Default for Viewer {fn default()->Self{Self{
    assets:Arc::new(vec![]),selected:None,query:String::new(),category:"Model".into(),model:None,skeleton:None,bank:None,texture:None,with_model:true,with_animation:false,
    index:0,items:vec![],item_filter:String::new(),busy:true,latest:0,status:"Reading asset catalog…".into(),detail:String::new(),report:Value::Null,
    playing:true,speed:1.,position:0.,duration:0.,seek:None,bones:false,z_up:false,unlit:false,image:None,image_id:None,zoom:1.,loaded:false,inspector:false,old_images:vec![],
    startup_asset:None,startup_bank:None,startup_clip:0,capture:None,capture_at:15.,captured:false,
    authored_unlit:std::collections::HashMap::new(),
}}}

#[derive(Component)] pub struct PreviewRoot;
#[derive(Component)] pub struct PreviewCamera;
#[derive(Component)] pub struct PreviewLight;
#[derive(Component)] pub struct ClipGraph{handle:Handle<AnimationGraph>,index:AnimationNodeIndex}
#[derive(Resource)] pub struct Orbit{pub target:Vec3,pub distance:f32,pub yaw:f32,pub pitch:f32,pub radius:f32}
impl Default for Orbit{fn default()->Self{Self{target:Vec3::ZERO,distance:4.,yaw:0.6,pitch:0.2,radius:1.}}}

pub fn setup(mut commands:Commands,mut egui_settings:ResMut<EguiGlobalSettings>){
    egui_settings.auto_create_primary_context=false;
    commands.spawn((Camera3d::default(),PreviewCamera,Transform::from_xyz(3.,2.,4.).looking_at(Vec3::ZERO,Vec3::Y)));
    commands.spawn((PrimaryEguiContext,Camera2d,RenderLayers::none(),Camera{order:1,output_mode:CameraOutputMode::Write{blend_state:Some(BlendState::ALPHA_BLENDING),clear_color:ClearColorConfig::None},clear_color:ClearColorConfig::Custom(Color::NONE),..default()}));
    commands.spawn((PreviewLight,DirectionalLight{illuminance:8000.,shadow_maps_enabled:false,..default()},Transform::from_rotation(Quat::from_euler(EulerRot::XYZ,-0.7,-0.6,0.))));
}

fn source(v:&Viewer,id:Option<usize>)->Option<String>{id.and_then(|i|v.assets.get(i)).map(|a|a.source.clone())}
fn request(v:&mut Viewer,bridge:&Bridge){
    let Some(row)=v.selected.and_then(|i|v.assets.get(i)).cloned() else{return};
    let mut req=json!({"command":"select","source":row.source,"index":v.index});
    if let Some(s)=source(v,v.skeleton){req["skeleton"]=json!(s);}
    if let Some(s)=source(v,v.texture){req["textures"]=json!([s]);}
    if row.kind=="Animation" && v.with_model {if let Some(s)=source(v,v.model){req["model"]=json!(s);}}
    if row.kind=="Model" && v.with_animation {if let Some(s)=source(v,v.bank){req["bank"]=json!(s);}}
    v.latest=bridge.request(req);v.busy=true;v.loaded=false;v.status=format!("Decoding {}…",row.name);
}
fn select(v:&mut Viewer,id:usize,bridge:&Bridge){
    v.selected=Some(id);v.index=0;v.items.clear();v.item_filter.clear();v.detail.clear();v.inspector=false;
    if v.category!="All"{v.category=v.assets[id].kind.clone();}
    let row=&v.assets[id];let parent=row.source.rsplit_once("::").map(|x|x.0.to_string()).unwrap_or_else(||std::path::Path::new(&row.source).parent().unwrap().to_string_lossy().into_owned());
    let same:Vec<_>=v.assets.iter().filter(|a|a.kind=="Skeleton" && a.source.starts_with(&parent)).map(|a|a.id).collect();
    if same.len()==1{v.skeleton=Some(same[0]);}
    else if row.source.to_lowercase().contains("characters") || row.name.starts_with("player_") {v.skeleton=v.assets.iter().find(|a|a.name=="player_skel.ske").map(|a|a.id);}
    else if row.kind=="Model"{v.skeleton=None;v.with_animation=false;}
    match row.kind.as_str(){"Model"=>v.model=Some(id),"Skeleton"=>v.skeleton=Some(id),"Animation"=>{v.bank=Some(id);v.with_animation=true;},_=>{}}
    request(v,bridge);
}

pub fn receive(mut commands:Commands,bridge:Res<Bridge>,mut v:ResMut<Viewer>,assets:Res<AssetServer>,mut graphs:ResMut<Assets<AnimationGraph>>,roots:Query<Entity,With<PreviewRoot>>,mut orbit:ResMut<Orbit>){
    let responses:Vec<_>=bridge.rx.lock().unwrap().try_iter().collect();
    for response in responses {
        let id=response["id"].as_u64().unwrap_or(0);
        if id!=0 && id!=v.latest{continue}
        if !response["ok"].as_bool().unwrap_or(false){
            v.busy=false;v.loaded=false;v.duration=0.;v.report=Value::Null;
            for e in &roots{commands.entity(e).despawn();}
            if let Some(image)=v.image.take(){v.old_images.push(image.id());}v.image_id=None;
            v.status=response["error"].as_str().unwrap_or("Decoder error").into();continue
        }
        let value=&response["value"];
        if id==0 {
            v.assets=Arc::new(serde_json::from_value(value["assets"].clone()).unwrap_or_default());v.busy=false;v.status=format!("{} distinct assets • select an asset to preview",v.assets.len());
            if let Some(name)=v.startup_asset.take(){if let Some(i)=v.assets.iter().position(|a|a.name==name){
                select(&mut v,i,&bridge);
                if let Some(bank)=v.startup_bank.take(){v.bank=v.assets.iter().find(|a|a.name==bank).map(|a|a.id);v.with_animation=true;v.index=v.startup_clip;request(&mut v,&bridge);}
            }}
            continue
        }
        v.busy=false;
        if value.get("metadata").is_none(){v.detail=serde_json::to_string_pretty(value).unwrap_or_default();v.status="Structural inspection ready".into();v.inspector=true;continue}
        v.items=value["metadata"]["items"].as_array().cloned().unwrap_or_default();
        for e in &roots{commands.entity(e).despawn();}
        if let Some(image)=v.image.take(){v.old_images.push(image.id());}v.image_id=None;
        if let Some(error)=value["preview_error"].as_str(){v.status=error.into();v.detail=serde_json::to_string_pretty(&value["metadata"]).unwrap_or_default();v.report=Value::Null;v.duration=0.;continue}
        v.image=None;v.image_id=None;v.position=0.;v.duration=0.;v.inspector=false;v.report=value["preview"].clone();v.detail=serde_json::to_string_pretty(&v.report).unwrap_or_default();
        let preview=value["preview"].clone();let kind=preview["kind"].as_str().unwrap_or("inspection");
        v.duration=preview["duration"].as_f64().unwrap_or(0.) as f32;
        v.status=format!("{} • {:.0} ms{}",preview["name"].as_str().unwrap_or("Asset"),preview["decode_ms"].as_f64().unwrap_or(0.),if preview["cache_hit"]==true{" • cached"}else{""});
        if let Some(path)=preview["asset"].as_str(){
            if kind=="image"{v.image=Some(assets.load(path.to_owned()));v.zoom=1.;v.loaded=true;}
            else {
                let transform=Transform::from_rotation(if v.z_up{Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)}else{Quat::IDENTITY});
                let mut entity=commands.spawn((PreviewRoot,WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.to_owned()))),transform,OriginalAsset{evidence:crate::game::EvidenceLevel::AssetDerived,source:preview["source"].as_str().unwrap_or("").into(),decoder_version:preview["decoder_version"].as_str().unwrap_or("").into()}));
                if v.duration>0.{let (graph,index)=AnimationGraph::from_clip(assets.load(GltfAssetLabel::Animation(0).from_asset(path.to_owned())));entity.insert(ClipGraph{handle:graphs.add(graph),index});}
                entity.observe(scene_ready);
                if let Some(bounds)=preview["bounds"].as_array(){
                    let vec=|n:usize|Vec3::new(bounds[n][0].as_f64().unwrap_or(0.) as f32,bounds[n][1].as_f64().unwrap_or(0.) as f32,bounds[n][2].as_f64().unwrap_or(0.) as f32);
                    orbit.target=transform.rotation*((vec(0)+vec(1))*0.5);orbit.radius=(vec(1)-vec(0)).length().max(0.001)*0.5;orbit.distance=orbit.radius*3.;
                }else{orbit.target=Vec3::new(0.,0.7,0.);orbit.radius=1.;orbit.distance=3.;}
                if preview["frontend"]==true{orbit.yaw=0.;orbit.pitch=0.;}
                v.bones=kind=="skeleton";
            }
        }else{v.loaded=true;}
    }
}

fn scene_ready(event:On<WorldInstanceReady>,mut commands:Commands,children:Query<&Children>,graphs:Query<&ClipGraph>,mut players:Query<&mut AnimationPlayer>,mut v:ResMut<Viewer>,mut materials:ResMut<Assets<StandardMaterial>>){
    if let Ok(graph)=graphs.get(event.entity){
        for child in children.iter_descendants(event.entity){if let Ok(mut player)=players.get_mut(child){
            player.play(graph.index).repeat();commands.entity(child).insert(AnimationGraphHandle(graph.handle.clone()));
        }}
    }
    v.authored_unlit.retain(|id,_|materials.get(*id).is_some());
    let force_unlit=v.unlit;
    for (id,material) in materials.iter_mut(){let authored=*v.authored_unlit.entry(id).or_insert(material.unlit);material.unlit=force_unlit || authored;}
    v.loaded=true;
}

fn chooser(ui:&mut egui::Ui,label:&str,value:&mut Option<usize>,rows:&[AssetRow],kind:&str)->bool {
    let mut changed=false;
    egui::ComboBox::from_id_salt(label).selected_text(value.and_then(|i|rows.get(i)).map(|a|a.name.as_str()).unwrap_or("None")).width(225.).show_ui(ui,|ui|{
        changed|=ui.selectable_value(value,None,"None").changed();
        for r in rows.iter().filter(|a|a.kind==kind || (kind=="TextureBank" && a.name.to_lowercase().ends_with(".gsh"))){changed|=ui.selectable_value(value,Some(r.id),&r.name).changed();}
    });changed
}

pub fn ui(mut contexts:EguiContexts,mut v:ResMut<Viewer>,bridge:Res<Bridge>,mut orbit:ResMut<Orbit>,mut camera:Single<(&mut Camera,&mut Transform,&mut Projection),With<PreviewCamera>>,window:Single<&Window,With<PrimaryWindow>>,mut roots:Query<&mut Transform,(With<PreviewRoot>,Without<PreviewCamera>)>,mut materials:ResMut<Assets<StandardMaterial>>,mut mode:ResMut<AppMode>)->Result {
    for id in v.old_images.drain(..){contexts.remove_image(id);}
    if v.image_id.is_none(){if let Some(image)=v.image.clone(){v.image_id=Some(contexts.add_image(EguiTextureHandle::Strong(image)));}}
    let ctx=contexts.ctx_mut()?;
    let mut root=egui::Ui::new(ctx.clone(),"workbench".into(),egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()));
    let mut reload=false;let mut selected=None;
    egui::Panel::top("header").show(&mut root,|ui|{ui.horizontal(|ui|{
        if ui.button("← Menu").clicked(){*mode=AppMode::Menu;}
        ui.heading("EA Playground");ui.label(egui::RichText::new("ASSET WORKBENCH").small().color(egui::Color32::from_rgb(104,200,170)));
        ui.separator();ui.label("Bevy • local files");
        if v.busy{ui.spinner();} ui.label(&v.status);
    });});
    egui::Panel::left("library").default_size(280.).resizable(true).show(&mut root,|ui|{
        ui.add(egui::TextEdit::singleline(&mut v.query).hint_text("Search assets…").desired_width(f32::INFINITY));
        egui::ComboBox::from_id_salt("category").selected_text(&v.category).width(235.).show_ui(ui,|ui|{for name in ["All","Model","Image","Animation","Skeleton","Table","Archive","Other"]{ui.selectable_value(&mut v.category,name.into(),name);}});
        let query=v.query.to_lowercase();
        let filtered:Vec<usize>=v.assets.iter().filter(|a|(v.category=="All" || a.kind==v.category) && (a.name.to_lowercase().contains(&query)||a.source.to_lowercase().contains(&query))).map(|a|a.id).collect();
        ui.label(format!("{} assets",filtered.len()));ui.separator();
        egui::ScrollArea::vertical().show_rows(ui,25.,filtered.len(),|ui,range|{for n in range{
            let id=filtered[n];let row=&v.assets[id];
            if ui.selectable_label(v.selected==Some(id),&row.name).on_hover_text(format!("{}\n{} • {} bytes\n{}",row.source,row.status,row.size,row.family)).clicked(){selected=Some(id);}
        }});
    });
    egui::Panel::right("settings").default_size(280.).resizable(true).show(&mut root,|ui|{
        ui.heading("Preview");
        if let Some(i)=v.selected{ui.label(egui::RichText::new(&v.assets[i].name).strong());}
        ui.separator();
        // Share the immutable catalog without copying strings each frame.
        let rows=&v.assets.clone();
        ui.label("Skeleton");reload|=chooser(ui,"skeleton",&mut v.skeleton,rows,"Skeleton");
        ui.label("Model for animation");reload|=chooser(ui,"model",&mut v.model,rows,"Model");
        reload|=ui.checkbox(&mut v.with_model,"Animate selected model").changed();
        ui.label("Animation bank");reload|=chooser(ui,"bank",&mut v.bank,rows,"Animation");
        reload|=ui.checkbox(&mut v.with_animation,"Apply animation to model").changed();
        ui.label("Additional texture bank");reload|=chooser(ui,"textures",&mut v.texture,rows,"TextureBank");
        if !v.items.is_empty(){
            ui.separator();ui.label(if v.report["frontend"]==true{"Shapes"}else if v.duration>0.{"Clips"}else{"Images / clips"});
            ui.add(egui::TextEdit::singleline(&mut v.item_filter).hint_text("Filter entries…"));
            let search=v.item_filter.to_lowercase();let entries=v.items.clone();
            egui::ScrollArea::vertical().id_salt("entries").max_height(190.).show(ui,|ui|{for item in entries.iter().filter(|i|i["name"].as_str().unwrap_or("").to_lowercase().contains(&search)){
                let i=item["index"].as_u64().unwrap_or(0) as usize;
                if ui.selectable_value(&mut v.index,i,item["name"].as_str().unwrap_or("unnamed")).changed(){reload=true;}
            }});
        }
        ui.separator();ui.checkbox(&mut v.bones,"Skeleton overlay");
        if ui.checkbox(&mut v.z_up,"Source uses Z up").changed(){for mut t in &mut roots{t.rotation=if v.z_up{Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)}else{Quat::IDENTITY};}orbit.target=Vec3::ZERO;}
        if ui.checkbox(&mut v.unlit,"Unlit materials").changed(){for (id,m) in materials.iter_mut(){m.unlit=v.unlit || v.authored_unlit.get(&id).copied().unwrap_or(m.unlit);}}
        if ui.button("Fit view").clicked(){orbit.distance=orbit.radius*3.;}
        if ui.button("Inspect decoded structure").clicked(){if let Some(s)=source(&v,v.selected){v.latest=bridge.request(json!({"command":"inspect","source":s}));v.busy=true;}}
        if v.inspector && ui.button("Back to live preview").clicked(){v.inspector=false;}
        if ui.button("Reload preview").clicked(){reload=true;}
        ui.separator();
        for (label,key) in [("Triangles","triangles"),("Meshes","meshes"),("Textures","textures"),("Materials","materials"),("Bones","bones")]{if let Some(n)=v.report[key].as_u64(){ui.label(format!("{label}: {n}"));}}
        if let Some(bindings)=v.report["material_report"].as_array(){
            let resolved=bindings.iter().filter(|b|b["status"]=="resolved").count();
            if !bindings.is_empty(){ui.label(format!("Texture bindings: {resolved}/{} resolved",bindings.len()));}
        }
        if let Some(warnings)=v.report["warnings"].as_array(){
            let meaningful:Vec<_>=warnings.iter().filter_map(|w|w.as_str()).filter(|w|{let lower=w.to_lowercase();lower.contains("missing")||lower.contains("untextured")||lower.contains("undecoded")||lower.contains("unresolved")||lower.contains("not reconstructed")||lower.contains("warn")}).collect();
            if !meaningful.is_empty(){
                egui::CollapsingHeader::new(egui::RichText::new(format!("{} preview warnings",meaningful.len())).color(egui::Color32::YELLOW)).show(ui,|ui|{
                    egui::ScrollArea::vertical().id_salt("material-warnings").max_height(130.).show(ui,|ui|{for warning in meaningful{ui.label(warning);}});
                });
            }
        }
        ui.label(egui::RichText::new("Preview shading is approximate.\nAnimation timing assumes 30 fps.").small().color(egui::Color32::GRAY));
    });
    egui::Panel::bottom("timeline").show(&mut root,|ui|{ui.horizontal(|ui|{
        if ui.button(if v.playing{"Pause"}else{"Play"}).clicked(){v.playing=!v.playing;}
        let duration=v.duration.max(0.001);
        if ui.add_enabled(v.duration>0.,egui::Slider::new(&mut v.position,0.0..=duration).suffix(" s")).changed(){v.seek=Some(v.position);}
        ui.add(egui::Slider::new(&mut v.speed,0.1..=2.0).text("Speed"));
        ui.label("Drag: orbit • right drag: pan • wheel: zoom");
    });});
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(&mut root,|ui|{
        let rect=ui.available_rect_before_wrap();let response=ui.allocate_rect(rect,egui::Sense::drag());
        let scale=window.scale_factor();
        camera.0.viewport=Some(Viewport{physical_position:UVec2::new((rect.left()*scale).max(0.) as u32,(rect.top()*scale).max(0.) as u32),physical_size:UVec2::new((rect.width()*scale).max(1.) as u32,(rect.height()*scale).max(1.) as u32),..default()});
        if response.dragged_by(egui::PointerButton::Primary){let d=ctx.input(|i|i.pointer.delta());orbit.yaw-=d.x*0.008;orbit.pitch=(orbit.pitch+d.y*0.008).clamp(-1.5,1.5);}
        if response.dragged_by(egui::PointerButton::Secondary){let d=ctx.input(|i|i.pointer.delta());let shift=camera.1.right()*(-d.x)+camera.1.up()*d.y;let distance=orbit.distance;orbit.target+=shift*distance*0.002;}
        if response.hovered(){let d=ctx.input(|i|i.smooth_scroll_delta.y);orbit.distance=(orbit.distance*(-d*0.003).exp()).clamp(0.0001,1e7);if v.image.is_some(){v.zoom=(v.zoom*(d*0.003).exp()).clamp(0.05,20.);}}
        if v.inspector {
            let mut panel=ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(24.)));
            egui::ScrollArea::both().show(&mut panel,|ui|{ui.add(egui::Label::new(egui::RichText::new(&v.detail).monospace()).selectable(true));});
        }else if let Some(id)=v.image_id.filter(|_|!v.busy) {
            let w=v.report["width"].as_f64().unwrap_or(1.) as f32;let h=v.report["height"].as_f64().unwrap_or(1.) as f32;
            let fit=(rect.width()/w).min(rect.height()/h)*0.94*v.zoom;let image_rect=egui::Rect::from_center_size(rect.center(),egui::vec2(w*fit,h*fit));
            let painter=ui.painter().with_clip_rect(rect);
            for y in 0..(image_rect.height()/16.).ceil().min(200.) as i32{for x in 0..(image_rect.width()/16.).ceil().min(200.) as i32{painter.rect_filled(egui::Rect::from_min_size(image_rect.min+egui::vec2(x as f32*16.,y as f32*16.),egui::vec2(16.,16.)),0.,if (x+y)%2==0{egui::Color32::from_gray(42)}else{egui::Color32::from_gray(62)});}}
            painter.image(id,image_rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
        }else if v.report["kind"]=="inspection" || !v.loaded {
            let mut panel=ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(24.)));
            if !v.loaded{panel.heading(if v.busy{"Loading asset…"}else{"Select an asset"});panel.label(&v.status);}
            else{egui::ScrollArea::both().show(&mut panel,|ui|{ui.add(egui::Label::new(egui::RichText::new(&v.detail).monospace()).selectable(true));});}
        }
    });
    if let Some(id)=selected{select(&mut v,id,&bridge);}else if reload{request(&mut v,&bridge);}
    let eye=orbit.target+Vec3::new(orbit.yaw.sin()*orbit.pitch.cos(),orbit.pitch.sin(),orbit.yaw.cos()*orbit.pitch.cos())*orbit.distance;
    *camera.1=Transform::from_translation(eye).looking_at(orbit.target,Vec3::Y);
    if let Projection::Perspective(p)=&mut *camera.2{p.near=(orbit.radius*0.001).max(0.00001);p.far=(orbit.radius*1000.).max(100.);}
    Ok(())
}

pub fn animate(mut v:ResMut<Viewer>,mut players:Query<&mut AnimationPlayer>,graphs:Query<&ClipGraph>,mut roots:Query<&mut Visibility,With<PreviewRoot>>,mut settings:ResMut<WinitSettings>,mut redraw:MessageWriter<RequestRedraw>){
    for mut visibility in &mut roots{*visibility=if v.busy || v.inspector{Visibility::Hidden}else{Visibility::Visible};}
    let seek=v.seek.take();
    for graph in &graphs {for mut player in &mut players{if let Some(a)=player.animation_mut(graph.index){
        a.set_speed(v.speed);if v.playing{a.resume();}else{a.pause();}if let Some(t)=seek{a.set_seek_time(t);}else{v.position=a.seek_time();}
    }}}
    let active=v.busy || (v.playing && v.duration>0. && !v.inspector) || (!v.loaded && !roots.is_empty());
    settings.focused_mode=if active{UpdateMode::Continuous}else{UpdateMode::reactive_low_power(Duration::from_millis(250))};
    settings.unfocused_mode=UpdateMode::reactive_low_power(Duration::from_millis(if active{33}else{1000}));
    if active{redraw.write(RequestRedraw);}
}

pub fn bones(v:Res<Viewer>,roots:Query<Entity,With<PreviewRoot>>,children:Query<&Children>,nodes:Query<(&GlobalTransform,Option<&Name>)>,parents:Query<&ChildOf>,mut gizmos:Gizmos){
    if !v.bones || v.busy || v.inspector{return}
    for root in &roots{for e in children.iter_descendants(root){if let Ok((t,Some(name)))=nodes.get(e){
        if name.as_str().starts_with("mesh") || name.as_str().starts_with("Mesh"){continue}
        if let Ok(parent)=parents.get(e){if let Ok((pt,_))=nodes.get(parent.parent()){gizmos.line(pt.translation(),t.translation(),Color::srgb(0.2,1.,0.75));}}
    }}}
}
