//! Bevy presentation of APT movies: loads `.big` screens, steps the timelines at the movie rate and draws the
//! flattened display list with an orthographic camera (1 world unit = 1 movie pixel, y flipped).
use bevy::{asset::RenderAssetUsages,camera::{visibility::RenderLayers,ScalingMode},image::{ImageAddressMode,ImageSampler,ImageSamplerDescriptor},mesh::{Indices,PrimitiveTopology},prelude::*,
    render::render_resource::{Extent3d,TextureDimension,TextureFormat},window::RequestRedraw,winit::{UpdateMode,WinitSettings}};
use bevy_egui::{egui,EguiContexts};
use crate::apt_mat::AptMat;
use std::{collections::HashMap,rc::Rc};
use crate::{apt::{Apt,Character},apt_geom,apt_player::{DrawItem,Kind,Movie,NodeId,Player,Xf},apt_vm::Vm,archive,assets::{AlphaKind,Wrap},bridge,menu::AppMode,model};

pub fn fe_root()->std::path::PathBuf{bridge::data_root().join("files").join("data").join("fe")}

/// Load a movie by its frontend-relative path (`screens/hud/TetherballHud`, with or without `.big`/`.swf`).
pub fn load_movie(key:&str)->Result<Movie,String>{
    let mut rel=key.trim().replace('\\',"/");
    for ext in [".swf",".big",".apt"]{if rel.to_lowercase().ends_with(ext){rel.truncate(rel.len()-ext.len());}}
    let path=fe_root().join(format!("{rel}.big"));
    let data=std::fs::read(&path).map_err(|e|format!("{}: {e}",path.display()))?;
    let entries=archive::big_entries(&data)?;
    let find=|ext:&str|entries.iter().find(|e|e.name.to_lowercase().ends_with(ext)).ok_or_else(||format!("{}: no {ext}",path.display()));
    let (a,c)=(find(".apt")?,find(".const")?);
    let o=find(".o").ok();
    let apt=Apt::parse(&data[a.offset..a.offset+a.size],&data[c.offset..c.offset+c.size])?;
    let geom=match o{Some(o)=>apt_geom::load(&format!("{}::{}",path.display(),o.name),&model::Schemas::embedded())?,None=>apt_geom::Geometry::default()};
    Ok(Movie{key:crate::apt_vm::norm_key(&rel),apt,geom,links:Default::default()})
}

pub struct AptView{
    pub vm:Vm,pub root:NodeId,pub root_key:String,
    pub logged:usize,pub inspected:bool,pub paused:bool,pub accum:f32,pub frame_ms:f32,pub ticks:u64,pub error:Option<String>,
    mesh_cache:HashMap<(String,u32,usize),Handle<Mesh>>,image_cache:HashMap<(String,usize),Handle<Image>>,
    pool:Vec<Entity>,pub show_tree:bool,pub last_specs:usize,script_last:u64,shared:Option<Handle<AptMat>>,text_mesh_cache:HashMap<(String,u32,String),Handle<Mesh>>,font_images:HashMap<String,Handle<Image>>,
}
// The view lives on the main thread only (Rc inside); Bevy needs `Send` for resources, so it is stored as a NonSend resource.

pub struct AptViewNs(pub AptView);

#[derive(Component)] pub struct AptEntity;
#[derive(Component)] struct AptPooled(usize);
#[derive(Component)] struct AptCamera;
/// 3D cameras that share the front end's 16:9 letterbox.
#[derive(Component)] pub struct LetterboxCamera;

impl AptView{
    pub fn open(key:&str)->Result<AptView,String>{
        let mut vm=Vm::new();
        vm.loader=Some(load_movie);
        let ld=bridge::data_root().join("files").join("data").join("locale");
        if let (Ok(l),Ok(i))=(std::fs::read(ld.join("eng_us.loc")),std::fs::read(ld.join("string.idx"))){
            if let Ok(loc)=crate::locale::Locale::parse(&l,&i){vm.locale=Some(Rc::new(loc));}
        }
        if let (Ok(l),Ok(i))=(std::fs::read(ld.join("trc").join("eng_us.loc")),std::fs::read(ld.join("trc").join("string.idx"))){
            if let Ok(loc)=crate::locale::Locale::parse(&l,&i){vm.locale_trc=Some(Rc::new(loc));}
        }
        let movie=vm.load_movie_rec(key)?;
        let root=vm.player.spawn_root(movie.clone(),None,0,"_root");
        vm.root=Some(root);
        vm.process_pending();
        let ms=movie.apt.ms_per_frame.max(1) as f32;
        Ok(AptView{vm,root,root_key:movie.key.clone(),logged:0,inspected:false,paused:false,accum:0.,frame_ms:ms,ticks:0,error:None,mesh_cache:HashMap::new(),image_cache:HashMap::new(),pool:vec![],show_tree:false,last_specs:0,script_last:0,shared:None,text_mesh_cache:HashMap::new(),font_images:HashMap::new()})
    }
    pub fn root_movie(&self)->Option<Rc<Movie>>{self.vm.movies.get(&self.root_key).cloned()}
}

/// Background music of the frontend: `fe.asf` (`Audio::PlayMusic` in FEManager states 2 and 13), the minigame's stream while a match runs.
#[derive(Resource,Default)]
struct FeMusic{track:String,music:Option<crate::playback::Music>}
fn fe_music(world:Option<Res<crate::game::WorldPlay>>,v:Option<NonSend<AptViewNs>>,tb:Option<Res<crate::tb_session::TbActive>>,mut m:ResMut<FeMusic>){
    let want=if v.is_none()||std::env::args().any(|a|a=="--mute")||world.is_some(){""}else if tb.is_some(){"tetherball.asf"}else{"fe.asf"};
    if m.track==want{return}
    m.music=None;m.track=want.to_string();
    if !want.is_empty(){
        m.music=Some(crate::playback::Music::start(bridge::data_root().join("files").join("data").join("audio").join("music").join(want),0.4));
    }
}

pub fn plugin(app:&mut App){
    app.init_resource::<FeMusic>().add_systems(Update,fe_music.run_if(|m:Res<AppMode>|*m==AppMode::Apt));
    app.add_plugins(crate::apt_mat::AptMatPlugin).add_systems(Update,(view_step,view_camera,view_draw).chain().run_if(|m:Res<AppMode>|*m==AppMode::Apt))
        .add_systems(bevy_egui::EguiPrimaryContextPass,view_ui.run_if(|m:Res<AppMode>|*m==AppMode::Apt));
}

pub fn enter(world:&mut World,key:&str){
    match AptView::open(key){
        Ok(v)=>{
            let m=v.root_movie().unwrap();let (w,h)=(m.apt.width.max(1.),m.apt.height.max(1.));
            world.spawn((AptEntity,AptCamera,RenderLayers::layer(7),Camera3d::default(),Camera{clear_color:ClearColorConfig::Custom(Color::BLACK),..default()},
                Projection::Orthographic(OrthographicProjection{near:-2000.,far:2000.,scaling_mode:ScalingMode::Fixed{width:w,height:h},..OrthographicProjection::default_3d()}),
                Transform::from_xyz(w/2.,-h/2.,500.)));
            world.insert_non_send_resource(AptViewNs(v));
            // The playground world is loaded behind the menus (the original shows it through the `main_menu_nis` cameras).
            if std::env::args().all(|a|a!="--no-world"){
                let (eye,target,_)=crate::game::nis_pose("general").unwrap_or((Vec3::new(28.66,6.38,-40.52),Vec3::new(25.9,5.8,-42.82),3.));
                world.insert_resource(crate::game::Backdrop::new(eye,target));
                world.insert_resource(crate::game::Game::new(None));
            }
        }
        Err(e)=>{eprintln!("APT: {e}");world.insert_non_send_resource(AptViewNs(AptView{vm:Vm::new(),root:0,root_key:String::new(),logged:0,inspected:false,paused:true,accum:0.,frame_ms:16.,ticks:0,error:Some(e),mesh_cache:HashMap::new(),image_cache:HashMap::new(),pool:vec![],show_tree:false,last_specs:0,script_last:0,shared:None,text_mesh_cache:HashMap::new(),font_images:HashMap::new()}));}
    }
}

pub fn leave(world:&mut World){
    if let Some(mut m)=world.get_resource_mut::<FeMusic>(){m.music=None;m.track.clear();}
    world.remove_resource::<crate::game::Backdrop>();
    world.remove_resource::<crate::game::Game>();
    let ge:Vec<Entity>=world.query_filtered::<Entity,With<crate::game::GameEntity>>().iter(world).collect();
    for e in ge{if let Ok(ec)=world.get_entity_mut(e){ec.despawn();}}
    let ents:Vec<Entity>=world.query_filtered::<Entity,With<AptEntity>>().iter(world).collect();
    for e in ents{if let Ok(ec)=world.get_entity_mut(e){ec.despawn();}}
    world.remove_non_send_resource::<AptViewNs>();
}

pub fn view_step(mut play:Option<ResMut<crate::game::WorldPlay>>,mut game:Option<ResMut<crate::game::Game>>,mut mode:ResMut<AppMode>,mut backdrop:Option<ResMut<crate::game::Backdrop>>,mut commands:Commands,tbres:Option<Res<crate::tb_session::TbActive>>,v:Option<NonSendMut<AptViewNs>>,time:Res<Time<Real>>,keys:Res<ButtonInput<KeyCode>>,mouse:Res<ButtonInput<MouseButton>>,windows:Query<&Window>,mut settings:ResMut<WinitSettings>,mut redraw:MessageWriter<RequestRedraw>){
    let Some(mut v)=v else{return};let v=&mut v.0;
    settings.focused_mode=UpdateMode::Continuous;settings.unfocused_mode=UpdateMode::Continuous;redraw.write(RequestRedraw);
    if v.error.is_some(){return}
    if keys.just_pressed(KeyCode::F2){v.paused=!v.paused;}
    if keys.just_pressed(KeyCode::F1){v.show_tree=!v.show_tree;}
    // Keyboard -> Wii buttons (codes from fw.datatypes.KeyCode; a release is code + 1000).
    const MAP:[(KeyCode,i32);12]=[(KeyCode::Enter,302),(KeyCode::Space,302),(KeyCode::Escape,303),(KeyCode::Backspace,303),(KeyCode::KeyX,305),(KeyCode::KeyZ,304),
        (KeyCode::ArrowUp,38),(KeyCode::ArrowDown,40),(KeyCode::ArrowLeft,37),(KeyCode::ArrowRight,39),(KeyCode::Equal,301),(KeyCode::Minus,300)];
    if v.error.is_none(){
        for (k,c) in MAP{
            if keys.just_pressed(k){v.vm.key_event(c,0,true);}
            if keys.just_released(k){v.vm.key_event(c,0,false);}
        }
        if let Ok(w)=windows.single(){
            if let (Some(m),Some(pos))=(v.root_movie(),w.cursor_position().filter(|_|std::env::args().all(|a|a!="--apt-script"))){
                let (mw,mh)=(m.apt.width.max(1.),m.apt.height.max(1.));let (sw,sh)=(w.width(),w.height());
                let s=(sw/mw).min(sh/mh);let (ox,oy)=((sw-mw*s)/2.,(sh-mh*s)/2.);
                let (px,py)=((pos.x-ox)/s,(pos.y-oy)/s);
                v.vm.pointer_move(px,py);
                if mouse.just_pressed(MouseButton::Left){v.vm.pointer_button(true);v.vm.key_event(302,0,true);}
                if mouse.just_released(MouseButton::Left){v.vm.pointer_button(false);v.vm.key_event(302,0,false);}
            }
        }
    }
    let mut steps=0;
    if !v.paused{
        v.accum+=time.delta_secs()*1000.;
        while v.accum>=v.frame_ms&&steps<4{v.accum-=v.frame_ms;steps+=1;}
        if v.accum>v.frame_ms*4.{v.accum=0.;}
    }
    if keys.just_pressed(KeyCode::F3){steps+=1;}
    for _ in 0..steps{let ms=v.frame_ms as f64;let t0=std::time::Instant::now();v.vm.tick(ms);v.ticks+=1;let el=t0.elapsed().as_millis();if el>100{v.vm.log.push(format!("slow tick {} ms",el));}if v.ticks%100==0{let m=format!("tick {} at {:.1}s",v.ticks,time.elapsed_secs());v.vm.log.push(m);}}
    // Scripted input for headless checks: --apt-script "tick:code,tick:code" (code per fw.datatypes.KeyCode; +1000 = release).
    if let Some(list)=std::env::args().skip_while(|a|a!="--apt-script").nth(1){
        for item in list.split(','){
            let Some((t,c))=item.split_once(':') else{continue};
            let Ok(t)=t.trim().parse::<u64>() else{continue};
            if !(t>v.script_last&&t<=v.ticks){continue}
            let c=c.trim();
            // `mX_Y` moves the pointer, `d`/`u` press/release the pointer button; numbers are Wii key codes (+1000 = release).
            if let Some(xy)=c.strip_prefix('m'){
                if let Some((x,y))=xy.split_once('_'){if let (Ok(x),Ok(y))=(x.parse::<f32>(),y.parse::<f32>()){v.vm.pointer_move(x,y);}}
            }else if let Some(name)=c.strip_prefix('o'){
                // `oNAME` opens a frontend screen (testing aid).
                v.vm.fe.todo.push(("OpenScreen".into(),vec![crate::apt_vm::V::Str(name.into())]));
            }else if let Some(n)=c.strip_prefix("tb").and_then(|n|n.parse::<i32>().ok()){
                // `tbN` starts a tetherball match for N human players straight away (testing aid).
                v.vm.fe.mp.players=n;v.vm.fe.mp.minigame=2;v.vm.fe.mp.quick=n==1;v.vm.fe.launch=Some("tetherball".into());
            }else if c=="g2"{v.vm.fe.mp.minigame=2;v.vm.fe.mp.players=2;
            }else if c=="wr"||c=="ws"{v.vm.fe.script_world=Some(if c=="wr"{"ReportCard"}else{"StickerBookCover"});
            }else if c=="pause"{v.vm.fe.script_pause=true;
            }else if let Some(path)=c.strip_prefix('x'){
                // `xPATH` hides a clip (debugging aid for bisecting a bad draw).
                let o=v.vm.resolve_path_object(path);
                if let Some(id)=v.vm.value_node_pub(&o){v.vm.player.nodes[id].visible=false;} else {v.vm.log.push(format!("script: no clip at {path}"));}
            }else if let Some(path)=c.strip_prefix('c'){
                // `cPATH` points at the centre of a clip, e.g. `c_root.window425.mcBtnMultiplayer`.
                let o=v.vm.resolve_path_object(path);
                if let Some(id)=v.vm.value_node_pub(&o){ if let Some(b)=v.vm.world_bounds(id){v.vm.pointer_move((b[0]+b[2])/2.,(b[1]+b[3])/2.);} else {v.vm.log.push(format!("script: no bounds for {path}"));} }
                else{v.vm.log.push(format!("script: no clip at {path}"));}
            }else if c=="d"{v.vm.pointer_button(true);v.vm.key_event(302,0,true);}
            else if c=="u"{v.vm.pointer_button(false);v.vm.key_event(302,0,false);}
            else if let Ok(c)=c.parse::<i32>(){ if c>=1000{v.vm.key_event(c-1000,0,false);}else{v.vm.key_event(c,0,true);} }
        }
    }
    v.script_last=v.ticks;
    // Mirror the script log to a file (the app has no console on Windows).
    if v.logged<v.vm.log.len(){
        use std::io::Write;
        if let Ok(mut f)=std::fs::OpenOptions::new().create(true).append(true).open(bridge::root().join("docs").join("apt-log.txt")){
            for l in &v.vm.log[v.logged..]{let _=writeln!(f,"[{}] {l}",v.ticks);}
        }
        v.logged=v.vm.log.len();
    }
    let at=std::env::args().skip_while(|a|a!="--apt-inspect-at").nth(1).and_then(|s|s.parse::<u64>().ok()).unwrap_or(120);
    if v.ticks==at&&!v.inspected{
        v.inspected=true;
        if let Some(list)=std::env::args().skip_while(|a|a!="--apt-inspect").nth(1){
            for path in list.split(';'){let d=v.vm.inspect(path.trim());v.vm.log.push(format!("INSPECT {path} = {d}"));}
        }
    }
    // Menu camera: fly to the pose of the chosen main-menu entry, back to the general pose on the main menu / title.
    if v.vm.fe.screen_changed{
        v.vm.fe.screen_changed=false;
        let o=v.vm.resolve_path_object("_root.window425.window");
        let name=v.vm.get_member(&o,"m_strClassName");let name=v.vm.to_str(&name).to_string();
        if name!=v.vm.fe.screen{
            v.vm.fe.screen=name.clone();
            match name.as_str(){
                "MainMenu"|"Title"=>v.vm.fe.nis=Some("general".into()),
                "SelectKid"=>v.vm.fe.nis=Some("select_kid".into()),
                "PlayerSetup"=>v.vm.fe.nis=Some("select_kid_mp".into()),
                _=>{}
            }
        }
    }
    if let Some(bd)=backdrop.as_mut(){
        if let Some(n)=v.vm.fe.nis.take(){
            if let Some((eye,target,dur))=crate::game::nis_pose(&n){bd.move_to(eye,target,if n=="general"{2.0}else{dur});}
        }
    }
    // World play: pause overlay (Esc / P), report card (- , the HUD's minus icon), sticker book (+ / =); the world stops while a menu is up.
    if let Some(p)=play.as_mut(){
        use crate::fe_host::PauseReq;
        let fe=&mut v.vm.fe;
        if !p.paused{
            if keys.just_pressed(KeyCode::Escape)||keys.just_pressed(KeyCode::KeyP)||std::mem::take(&mut fe.script_pause){
                p.paused=true;fe.paused=true;fe.pause_req=None;
                fe.todo.push(("OpenOverlay".into(),vec![crate::apt_vm::V::Str("PauseMenu".into())]));
            }else if keys.just_pressed(KeyCode::Minus)||keys.just_pressed(KeyCode::Equal)||fe.script_world.is_some(){
                p.paused=true;p.screen_pause=true;
                let scr=if let Some(s)=fe.script_world.take(){s}else if keys.just_pressed(KeyCode::Minus){"ReportCard"}else{"StickerBookCover"};
                fe.todo.push(("OpenScreen".into(),vec![crate::apt_vm::V::Str(scr.into())]));
            }
        }else if p.screen_pause{
            if fe.screen!="WorldHud"{p.seen_other=true;}
            else if p.seen_other{
                p.paused=false;p.screen_pause=false;p.seen_other=false;
                // The HUD movie was reloaded: show its icons again once it has registered its functions.
                fe.later.push((100,"Stickerbook_SetVisible".into(),vec![crate::apt_vm::V::Num(1.)]));
                fe.later.push((100,"ReportCard_SetVisible".into(),vec![crate::apt_vm::V::Num(1.)]));
            }
        }else{
            match fe.pause_req.take(){
                Some(PauseReq::Resume)|Some(PauseReq::Restart)=>{p.paused=false;fe.paused=false;fe.todo.push(("CloseOverlay".into(),vec![]));}
                Some(PauseReq::Quit)=>{
                    fe.paused=false;
                    fe.todo.push(("CloseOverlay".into(),vec![]));
                    fe.todo.push(("ReplaceScreen".into(),vec![crate::apt_vm::V::Str("MainMenu".into())]));
                    commands.remove_resource::<crate::game::WorldPlay>();
                    let (eye,target,_)=crate::game::nis_pose("general").unwrap_or((Vec3::new(28.66,6.38,-40.52),Vec3::new(25.9,5.8,-42.82),3.));
                    commands.insert_resource(crate::game::Backdrop::new(eye,target));
                    if let Some(gm)=game.as_mut(){
                        gm.stop_music();
                        if let Some(pl)=gm.player_entity(){commands.entity(pl).insert(Visibility::Hidden);}
                    }
                }
                None=>{}
            }
        }
    }
    if let Some(g)=v.vm.fe.launch.take(){
        if g=="world"&&play.is_none(){
            // Single Player: the world the menus were drawn over becomes playable; the front end keeps drawing the HUD.
            commands.remove_resource::<crate::game::Backdrop>();
            commands.insert_resource(crate::game::WorldPlay::default());
            if let Some(gm)=game.as_mut(){
                gm.start_music();
                if let Some(pl)=gm.player_entity(){commands.entity(pl).insert(Visibility::Inherited);}
            }
            v.vm.fe.later.push((20,"OpenScreen".into(),vec![crate::apt_vm::V::Str("WorldHud".into())]));
            v.vm.fe.later.push((120,"Stickerbook_SetVisible".into(),vec![crate::apt_vm::V::Num(1.)]));
            v.vm.fe.later.push((120,"ReportCard_SetVisible".into(),vec![crate::apt_vm::V::Num(1.)]));
        }
        if g=="tetherball"&&tbres.is_none(){let fe=&v.vm.fe;
            let kid=|i:usize|fe.mp.avatars.get(i).and_then(|&k|fe.roster.get(k as usize)).map(|k|k.asset.clone());
            let female=|i:usize|fe.mp.avatars.get(i).and_then(|&k|fe.roster.get(k as usize)).map(|k|!k.boy).unwrap_or(false);
            let humans=(fe.mp.players.max(1) as usize).min(2);
            let mut cfg=crate::tb_host::Config::quick(humans);
            cfg.single_player=fe.mp.quick&&humans==1;
            cfg.rounds=if fe.mp.rounds>0{fe.mp.rounds}else{3};
            if let [loc,diff,rot,mega,rounds]=fe.mp.rules[..]{cfg.area=loc;cfg.difficulty=diff;cfg.rotations=rot;cfg.mega=mega!=0;cfg.rounds=rounds;}
            cfg.female=[female(0),female(1)];
            cfg.special=[crate::tb_host::celebration_state(kid(0).as_deref()),crate::tb_host::celebration_state(kid(1).as_deref())];
            if let Some(a)=std::env::var("EAGL_TB_AREA").ok().and_then(|a|a.parse().ok()){cfg.area=a;}
            let kids=[kid(0),kid(1)];
            commands.insert_resource(crate::tb_session::TbLaunch{cfg,kids});}
    }
    let hc:Vec<_>=v.vm.host_calls.drain(..).collect();
    for (n,a) in hc{let mut parts=vec![];for x in &a{parts.push(v.vm.to_str(x).to_string());}let line=format!("HOST {n}({})",parts.join(", "));v.vm.log.push(line);}
}

/// Letterbox the camera to the movie's aspect so nothing outside the authored frame shows.
fn view_camera(v:Option<NonSend<AptViewNs>>,windows:Query<&Window>,mut cams:Query<(&mut Camera,Option<&AptCamera>,Option<&crate::game::GameEntity>),Or<(With<AptCamera>,With<LetterboxCamera>)>>,world3d:Query<(),(With<LetterboxCamera>,Without<AptCamera>)>,tb:Option<Res<crate::tb_session::TbActive>>,mut world_vis:Query<&mut Visibility,(With<crate::game::GameEntity>,Without<AptEntity>,Without<crate::game::Player>,Without<crate::tb_session::TbHidden>)>){
    let Some(v)=v else{return};let Some(m)=v.0.root_movie() else{return};
    // The menu world is hidden while a minigame scene is up.
    let want=Visibility::Inherited;let _=&tb;
    for mut vis in &mut world_vis{ if *vis!=want{*vis=want;} }
    let Ok(w)=windows.single() else{return};
    let (mw,mh)=(m.apt.width.max(1.),m.apt.height.max(1.));
    let (pw,ph)=(w.physical_width() as f32,w.physical_height() as f32);
    let s=(pw/mw).min(ph/mh);let (cw,ch)=((mw*s).floor().max(1.),(mh*s).floor().max(1.));
    let (ox,oy)=(((pw-cw)/2.).floor(),((ph-ch)/2.).floor());
    for (mut c,apt,world) in &mut cams{
        // The menu world is only shown while no minigame scene is running.
        if world.is_some()&&!c.is_active{c.is_active=true;}
        c.viewport=Some(bevy::camera::Viewport{physical_position:UVec2::new(ox as u32,oy as u32),physical_size:UVec2::new(cw as u32,ch as u32),..default()});
        if apt.is_some(){
            c.order=10;
            c.clear_color=if !world3d.is_empty(){ClearColorConfig::None}else{ClearColorConfig::Custom(Color::BLACK)};
        }
    }
}

fn view_draw(v:Option<NonSendMut<AptViewNs>>,mut commands:Commands,mut meshes:ResMut<Assets<Mesh>>,mut images:ResMut<Assets<Image>>,mut materials:ResMut<Assets<AptMat>>,
    mut q:Query<(&AptPooled,&mut Transform,&mut Visibility,&mut Mesh3d,&MeshMaterial3d<AptMat>)>){
    let Some(mut v)=v else{return};let v=&mut v.0;
    if v.error.is_some(){return}
    let mut draws=vec![];let root=v.root;v.vm.player.flatten(root,&mut draws);
    // One pooled entity per (draw, primitive).
    let mut slot=0usize;
    let mut specs:Vec<(Handle<Mesh>,Option<Handle<Image>>,[f32;4],[f32;4],Xf,f32,Option<crate::apt_player::Mask>)>=vec![];
    for d in &draws{
        if let DrawItem::Text{movie,def,text,bounds,xf,cx,mask,..}=d{
            if let Some((mesh,image,col))=text_spec(v,&mut meshes,&mut images,movie,*def,text,*bounds){
                let c=[col[0]*cx.mul[0],col[1]*cx.mul[1],col[2]*cx.mul[2],col[3]*cx.mul[3]];
                specs.push((mesh,Some(image),c,cx.add,*xf,slot as f32*0.01,mask.clone()));slot+=1;
            }
            continue;
        }
        let DrawItem::Shape{movie,shape,xf,cx,mask,..}=d else{continue};
        let Some(prims)=movie.geom.shapes.get(shape) else{continue};
        for (pi,p) in prims.iter().enumerate(){
            let key=(movie.key.clone(),*shape,pi);
            let mesh=v.mesh_cache.entry(key).or_insert_with(||{
                let mut m=Mesh::new(PrimitiveTopology::TriangleList,RenderAssetUsages::default());
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION,p.positions.iter().map(|q|[q[0],q[1],0.]).collect::<Vec<_>>());
                m.insert_attribute(Mesh::ATTRIBUTE_NORMAL,vec![[0.,0.,1.];p.positions.len()]);
                m.insert_attribute(Mesh::ATTRIBUTE_UV_0,p.uvs.clone());
                m.insert_indices(Indices::U32(p.indices.clone()));
                meshes.add(m)
            }).clone();
            let image=p.texture.map(|ti|v.image_cache.entry((movie.key.clone(),ti*4+match (p.wrap[0],p.wrap[1]){(Wrap::Clamp,Wrap::Clamp)=>0,(Wrap::Repeat,_)=>1,_=>2})).or_insert_with(||{
                let t=&movie.geom.textures[ti];
                let mut img=Image::new(Extent3d{width:t.width as u32,height:t.height as u32,depth_or_array_layers:1},TextureDimension::D2,t.rgba.clone(),TextureFormat::Rgba8Unorm,RenderAssetUsages::default());
                let am=|w:Wrap|match w{Wrap::Clamp=>ImageAddressMode::ClampToEdge,Wrap::Repeat=>ImageAddressMode::Repeat,Wrap::Mirror=>ImageAddressMode::MirrorRepeat};
                img.sampler=ImageSampler::Descriptor(ImageSamplerDescriptor{address_mode_u:am(p.wrap[0]),address_mode_v:am(p.wrap[1]),..ImageSamplerDescriptor::linear()});
                images.add(img)}).clone());
            let c=[p.color[0]*cx.mul[0],p.color[1]*cx.mul[1],p.color[2]*cx.mul[2],p.color[3]*cx.mul[3]];
            specs.push((mesh,image,c,cx.add,*xf,slot as f32*0.01,mask.clone()));
            slot+=1;
        }
    }
    if specs.len().abs_diff(v.last_specs)>40{let m=format!("draw specs {} (pool {})",specs.len(),v.pool.len());v.vm.log.push(m);v.last_specs=specs.len();}
    let have=v.pool.len();
    for i in have..specs.len(){
        let mat=materials.add(AptMat{mul:Vec4::ONE,add:Vec4::ZERO,tex:None,mask:Default::default()});
        let e=commands.spawn((AptEntity,AptPooled(i),RenderLayers::layer(7),bevy::camera::visibility::NoFrustumCulling,Mesh3d(Handle::default()),MeshMaterial3d(mat),Transform::default(),Visibility::Hidden)).id();
        v.pool.push(e);
    }
    let mut seen=vec![false;v.pool.len()];
    for (pooled,mut t,mut vis,mut mesh,mat) in &mut q{
        let i=pooled.0;
        if i<specs.len(){
            let (m,img,c,ad,xf,z,mk)=&specs[i];
            mesh.0=m.clone();
            // Movie space is y-down; world is y-up.  Matrix layout: x' = x*a + y*c + tx, y' = x*b + y*d + ty.
            let a=xf.0;
            *t=Transform::from_matrix(Mat4::from_cols(Vec4::new(a[0],-a[1],0.,0.),Vec4::new(a[2],-a[3],0.,0.),Vec4::new(0.,0.,1.,0.),Vec4::new(a[4],-a[5],*z,1.)));
            let (nm,na)=(Vec4::from_array(*c),Vec4::from_array(*ad));
            let nmask=crate::apt_mat::MaskBuf::from(mk.as_ref().map(|m|m.as_slice()));
            let changed=materials.get(&mat.0).is_none_or(|mm|mm.mul!=nm||mm.add!=na||mm.tex!=*img||mm.mask!=nmask);
            if changed{ if let Some(mut mm)=materials.get_mut(&mat.0){mm.mul=nm;mm.add=na;mm.tex=img.clone();mm.mask=nmask;} }
            *vis=Visibility::Inherited;seen[i]=true;
        }else{*vis=Visibility::Hidden;}
    }
    let _=seen;
}

/// Mesh + font atlas + colour for one text field (cached by movie/char/resolved text).
fn text_spec(v:&mut AptView,meshes:&mut Assets<Mesh>,images:&mut Assets<Image>,movie:&Rc<Movie>,def:u32,text:&str,bounds:Option<[f32;4]>)->Option<(Handle<Mesh>,Handle<Image>,[f32;4])>{
    let (apt_movie,def,t,font_name)=crate::apt_text::resolve(movie,def)?;
    if text.is_empty(){return None}
    let shown=if text.starts_with('$')||text.starts_with("T_"){v.vm.locale_string(text)}else{text.to_string()};
    let face=crate::apt_text::face(&font_name,t.height)?;
    let tb=bounds.unwrap_or(t.bounds);
    let key=(apt_movie.key.clone(),def,format!("{shown}|{:?}",tb));
    let mesh=if let Some(m)=v.text_mesh_cache.get(&key){m.clone()}else{
        let quads=crate::apt_text::layout(&face,&shown,tb,t.align,t.multiline,t.word_wrap,t.height);
        if quads.is_empty(){return None}
        let mut pos=vec![];let mut uv=vec![];let mut idx=vec![];
        for q in &quads{
            let b=pos.len() as u32;
            pos.extend([[q.x0,q.y0,0.],[q.x1,q.y0,0.],[q.x1,q.y1,0.],[q.x0,q.y1,0.]]);
            uv.extend([[q.u0,q.v0],[q.u1,q.v0],[q.u1,q.v1],[q.u0,q.v1]]);
            idx.extend([b,b+1,b+2,b,b+2,b+3]);
        }
        let mut m=Mesh::new(PrimitiveTopology::TriangleList,RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL,vec![[0.,0.,1.];pos.len()]);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION,pos);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0,uv);
        m.insert_indices(Indices::U32(idx));
        let h=meshes.add(m);v.text_mesh_cache.insert(key,h.clone());h
    };
    let image=v.font_images.entry(face.file.clone()).or_insert_with(||{
        let f=&face.font;
        let rgba=f.rgba.clone();
        let mut img=Image::new(Extent3d{width:f.tex_w,height:f.tex_h,depth_or_array_layers:1},TextureDimension::D2,rgba,TextureFormat::Rgba8Unorm,RenderAssetUsages::default());
        img.sampler=ImageSampler::Descriptor(ImageSamplerDescriptor{address_mode_u:ImageAddressMode::ClampToEdge,address_mode_v:ImageAddressMode::ClampToEdge,..ImageSamplerDescriptor::linear()});
        images.add(img)
    }).clone();
    let c=t.color;
    Some((mesh,image,[((c>>16)&255) as f32/255.,((c>>8)&255) as f32/255.,(c&255) as f32/255.,((c>>24)&255) as f32/255.]))
}

fn view_ui(mut contexts:EguiContexts,v:Option<NonSendMut<AptViewNs>>,mut mode:ResMut<AppMode>,windows:Query<&Window>)->Result{
    let Some(mut v)=v else{return Ok(())};let v=&mut v.0;
    let ctx=contexts.ctx_mut()?;
    if v.show_tree||v.error.is_some(){ egui::Area::new("apt_info".into()).anchor(egui::Align2::LEFT_TOP,[8.,8.]).show(ctx,|ui|{
        egui::Frame::popup(ui.style()).show(ui,|ui|{
            ui.horizontal(|ui|{if ui.button("< Menu").clicked(){*mode=AppMode::Menu;}ui.label(egui::RichText::new(format!("APT {}",v.root_key)).strong());});
            if let Some(e)=&v.error{ui.colored_label(egui::Color32::LIGHT_RED,e);return}
            let live=v.vm.player.nodes.iter().filter(|n|n.alive).count();
            ui.label(format!("ticks {}  nodes {}  {} ms/frame  {}",v.ticks,live,v.frame_ms,if v.paused{"paused (F2)"}else{"F1 debug, F2 pause, F3 step"}));
            for l in v.vm.log.iter().rev().take(8).rev(){ui.label(egui::RichText::new(l).small().color(egui::Color32::LIGHT_YELLOW));}
            if v.show_tree{
                fn row(ui:&mut egui::Ui,p:&Player,id:NodeId,depth:usize){
                    let n=&p.nodes[id];if !n.alive{return}
                    match &n.kind{
                        Kind::Sprite{frame,frame_count,children,char_id,playing}=>{
                            ui.label(egui::RichText::new(format!("{}{} [{}] f{}/{}{}",". ".repeat(depth),if n.name.is_empty(){"(clip)"}else{&n.name},char_id,frame,frame_count,if *playing{""}else{" stopped"})).small());
                            if depth<4{for (_,&c) in children{row(ui,p,c,depth+1);}}
                        }
                        _=>{}
                    }
                }
                egui::ScrollArea::vertical().max_height(260.).show(ui,|ui|row(ui,&v.vm.player,v.root,0));
            }
        });
    }); }
    // Text fields are drawn with egui over the scene.
    let Ok(w)=windows.single() else{return Ok(())};
    let root=v.root_movie();
    let Some(root)=root else{return Ok(())};
    let (mw,mh)=(root.apt.width.max(1.),root.apt.height.max(1.));
    let (sw,sh)=(w.width(),w.height());
    let s=(sw/mw).min(sh/mh);let (ox,oy)=((sw-mw*s)/2.,(sh-mh*s)/2.);
    let mut draws=vec![];let rid=v.root;v.vm.player.flatten(rid,&mut draws);
    let _=Xf::ID;let _=AlphaKind::Opaque;let _=RenderLayers::none();
    Ok(())
}
