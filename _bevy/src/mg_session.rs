//! In-world presentation of a minigame whose original code runs in the PowerPC VM (`mgvm`).
//!
//! The VM boots the engine objects, launches the game through `WorldMan::StartMinigame` and is stepped once per frame
//! with the keyboard mapped onto Wii Remote button bits.  After each frame the guest's characters (position, heading,
//! skeleton pose), model draws and camera are read back and shown with the Bevy world that is already on screen.
use crate::{assets, bridge, character, game, mgvm};
use bevy::{prelude::*, render::view::window::screenshot::*, window::RequestRedraw, winit::{UpdateMode, WinitSettings}};
use std::{
    collections::HashMap,
    sync::{mpsc, Mutex},
};

/// Request to start minigame `ty` (see `mgvm::launch`).
#[derive(Resource, Clone)]
pub struct MgLaunch {
    pub ty: i32,
    pub humans: usize,
}

#[derive(Resource)]
pub struct MgActive;

#[derive(Component)]
pub struct MgEntity;
#[derive(Component)]
struct MgChar(u32);
#[derive(Component)]
struct MgJoint {
    ptr: u32,
    bone: usize,
}
#[derive(Component)]
struct MgProp(usize);

struct Loaded {
    player: character::CharacterData,
    kids: Vec<Option<assets::BuiltModel>>,
    kid_assets: Vec<String>,
    /// Model name -> loaded model for the props the game draws.
    props: HashMap<String, assets::BuiltModel>,
}

enum Msg {
    Loaded(Box<Loaded>),
    Failed(String),
}

pub struct Session {
    pub ty: i32,
    pub humans: usize,
    rx: Mutex<mpsc::Receiver<Msg>>,
    loaded: Option<Box<Loaded>>,
    vm: Option<(mgvm::MgVm, mgvm::MgHost)>,
    pub state: SessionState,
    pub log: Vec<String>,
    chars: HashMap<u32, (Entity, Vec<Entity>)>,
    props: Vec<(Entity, String)>,
    prop_models: HashMap<String, assets::Uploaded>,
    kid_uploads: Vec<Option<assets::Uploaded>>,
    default_upload: Option<assets::Uploaded>,
    pub t: f64,
    pub frames: i32,
    skip_close: i32,
    pub last_events: Vec<mgvm::FeEvent>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum SessionState {
    Loading,
    Running,
    Failed(String),
}

fn data_dir(rel: &str) -> String {
    bridge::data_root().join("files").join("data").join(rel.replace('/', "\\")).to_string_lossy().into_owned()
}

/// Find `name` (e.g. `dodgeball.o`) in the minigame archives.
fn find_model_source(name: &str) -> Option<String> {
    let root = bridge::data_root().join("files").join("data");
    let mut dirs = vec![root.join("minigames")];
    if let Ok(rd) = std::fs::read_dir(root.join("minigames")) {
        dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    dirs.push(root.join("worldprops"));
    dirs.push(root.join("microgames"));
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for f in rd.flatten() {
            let p = f.path();
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "viv" && ext != "big" {
                continue;
            }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(data) = crate::archive::decompress(&bytes) else { continue };
            let Ok(entries) = crate::archive::big_entries(&data) else { continue };
            if entries.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
                let real = entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)).unwrap().name.clone();
                return Some(format!("{}::{}", p.to_string_lossy(), real));
            }
        }
    }
    None
}

fn loader(tx: mpsc::Sender<Msg>) {
    let run = || -> Result<Loaded, String> {
        let schemas = crate::model::Schemas::embedded();
        let dir = bridge::data_root().join("files").join("data").join("db");
        let (v, b) = (std::fs::read(dir.join("db.vlt")).map_err(|e| e.to_string())?, std::fs::read(dir.join("db.bin")).map_err(|e| e.to_string())?);
        let db = crate::vlt::Database::load(&v, &b, crate::vlt::known_names())?;
        let list = db.find_collection("character_select", "character").ok_or("character_select/character")?;
        let chars: Vec<String> = db.attribute(list, "characterlist").and_then(|v| v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())).ok_or("characterlist")?;
        let mut kid_assets = vec![];
        for key in &chars {
            let c = db.find_collection("bestiary", key).ok_or_else(|| format!("bestiary/{key}"))?;
            let mut cur = Some(c);
            let mut asset = None;
            while let Some(col) = cur {
                if let Some(v) = db.attribute(col, "asset_name") {
                    asset = v.as_str().map(String::from);
                    break;
                }
                cur = db.collections.iter().find(|x| x.class_key == col.class_key && x.key == col.parent_key && col.parent_key != 0);
            }
            kid_assets.push(asset.ok_or("asset_name")?);
        }
        let player = character::load(&data_dir("characters"), &schemas)?;
        let kids = kid_assets.iter().map(|a| if a == "alicia" { None } else { character::load_kid_model(&data_dir("characters"), a, &schemas).ok() }).collect();
        Ok(Loaded { player, kids, kid_assets, props: HashMap::new() })
    };
    let _ = tx.send(match run() {
        Ok(l) => Msg::Loaded(Box::new(l)),
        Err(e) => Msg::Failed(e),
    });
}

impl Session {
    pub fn new(ty: i32, humans: usize) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || loader(tx));
        Session {
            ty,
            humans,
            rx: Mutex::new(rx),
            loaded: None,
            vm: None,
            state: SessionState::Loading,
            log: vec![],
            chars: HashMap::new(),
            props: vec![],
            prop_models: HashMap::new(),
            kid_uploads: vec![],
            default_upload: None,
            t: 0.,
            frames: 0,
            skip_close: 0,
            last_events: vec![],
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (launch_or_teardown, pump, build, step, present).chain());
}

fn launch_or_teardown(world: &mut World) {
    if let Some(l) = world.remove_resource::<MgLaunch>() {
        if world.get_non_send_resource::<Session>().is_none() {
            world.insert_non_send_resource(Session::new(l.ty, l.humans));
        }
    }
    let quit = world.get_non_send_resource::<Session>().is_some() && world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F10);
    if quit {
        if let Some(mut v) = world.get_non_send_resource_mut::<crate::apt_view::AptViewNs>() {
            v.0.vm.fe.vm_session = false;
        }
        world.remove_non_send_resource::<Session>();
        world.remove_resource::<MgActive>();
        world.remove_resource::<crate::tb_session::TbActive>();
        let ents: Vec<Entity> = world.query_filtered::<Entity, With<MgEntity>>().iter(world).collect();
        for e in ents {
            if let Ok(em) = world.get_entity_mut(e) {
                em.despawn();
            }
        }
    }
}

fn pump(s: Option<NonSendMut<Session>>, mut settings: ResMut<WinitSettings>, mut redraw: MessageWriter<RequestRedraw>) {
    let Some(mut s) = s else { return };
    settings.focused_mode = UpdateMode::Continuous;
    settings.unfocused_mode = UpdateMode::Continuous;
    redraw.write(RequestRedraw);
    if s.state != SessionState::Loading || s.loaded.is_some() {
        return;
    }
    let msgs: Vec<Msg> = s.rx.lock().unwrap().try_iter().collect();
    for m in msgs {
        match m {
            Msg::Loaded(l) => s.loaded = Some(l),
            Msg::Failed(e) => {
                s.log.push(format!("FAILED: {e}"));
                s.state = SessionState::Failed(e);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build(mut commands: Commands, s: Option<NonSendMut<Session>>, game: Option<Res<game::Game>>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut s) = s else { return };
    if s.state != SessionState::Loading || s.loaded.is_none() || game.is_none() {
        return;
    }
    let loaded = s.loaded.take().unwrap();
    // props the game may draw: loaded lazily by name below; kid models uploaded now
    let default_up = assets::upload_skinned(&loaded.player.model, &mut meshes, &mut materials, &mut images);
    let kid_uploads: Vec<Option<assets::Uploaded>> = loaded.kids.iter().map(|m| m.as_ref().map(|m| assets::upload_skinned(m, &mut meshes, &mut materials, &mut images))).collect();
    s.default_upload = Some(default_up);
    s.kid_uploads = kid_uploads;
    let t0 = std::time::Instant::now();
    let (ty, humans) = (s.ty, s.humans);
    match mgvm::boot().and_then(|(mut vm, mut host)| mgvm::launch(&mut vm, &mut host, ty, humans).map(|_| (vm, host))) {
        Ok(v) => {
            s.log.push(format!("VM ready in {:?}", t0.elapsed()));
            s.vm = Some(v);
            s.state = SessionState::Running;
            s.loaded = Some(loaded);
            commands.insert_resource(MgActive);
            commands.insert_resource(crate::tb_session::TbActive);
        }
        Err(e) => {
            eprintln!("[mg] VM failed: {e}");
            s.log.push(format!("VM failed: {e}"));
            s.state = SessionState::Failed(e);
        }
    }
}

/// Keyboard -> Wii Remote buttons for seat 0 (WPAD bit values).
fn pad_buttons(keys: &ButtonInput<KeyCode>) -> u16 {
    let mut b = 0u16;
    let any = |ks: &[KeyCode]| ks.iter().any(|k| keys.pressed(*k));
    if any(&[KeyCode::ArrowUp, KeyCode::KeyW]) {
        b |= 0x0008;
    }
    if any(&[KeyCode::ArrowDown, KeyCode::KeyS]) {
        b |= 0x0004;
    }
    if any(&[KeyCode::ArrowLeft, KeyCode::KeyA]) {
        b |= 0x0001;
    }
    if any(&[KeyCode::ArrowRight, KeyCode::KeyD]) {
        b |= 0x0002;
    }
    if any(&[KeyCode::Space, KeyCode::KeyZ]) {
        b |= 0x0800;
    }
    if any(&[KeyCode::KeyX, KeyCode::ControlLeft]) {
        b |= 0x0400;
    }
    if any(&[KeyCode::Digit1]) {
        b |= 0x0200;
    }
    if any(&[KeyCode::Digit2]) {
        b |= 0x0100;
    }
    if any(&[KeyCode::KeyP, KeyCode::Escape]) {
        b |= 0x0010;
    }
    b
}

/// `EAGL_MG_PADS="from-to:hexbits,..."` scripts the pad by frame number (testing aid).
fn scripted_pad(frame: i32) -> u16 {
    let mut bits = 0u16;
    for part in std::env::var("EAGL_MG_PADS").unwrap_or_default().split(',') {
        if let Some((range, b)) = part.split_once(':') {
            if let Some((a, z)) = range.split_once('-') {
                if let (Ok(a), Ok(z), Ok(b)) = (a.parse::<i32>(), z.parse::<i32>(), u16::from_str_radix(b.trim_start_matches("0x"), 16)) {
                    if frame >= a && frame < z {
                        bits |= b;
                    }
                }
            }
        }
    }
    bits
}

#[derive(Resource, Default)]
pub struct MgSnapshot(pub Option<mgvm::snapshot::Snapshot>);

fn step(mut commands: Commands, s: Option<NonSendMut<Session>>, keys: Res<ButtonInput<KeyCode>>, time: Res<Time<Real>>, mut backdrop: Option<ResMut<game::Backdrop>>, snap: Option<ResMut<MgSnapshot>>, mut fe: Option<NonSendMut<crate::apt_view::AptViewNs>>) {
    let Some(mut s) = s else { return };
    if s.state != SessionState::Running {
        return;
    }
    let s = &mut *s;
    let ms = ((time.delta_secs_f64() * 1000.) as i32).clamp(1, 50);
    s.t += time.delta_secs_f64();
    let Some((vm, host)) = s.vm.as_mut() else { return };
    s.frames += 1;
    if s.frames == 30 && std::env::var("EAGL_MG_AUTOPLAY").is_ok() {
        let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
        let r = vm.call_by_name(host, "OnPlay__8MinigameFv", &[mg], &[]);
        s.log.push(format!("autoplay OnPlay {r:?}"));
    }
    host.pads[0].buttons = pad_buttons(&keys) | scripted_pad(s.frames);
    if let Err(e) = mgvm::frame(vm, host, ms) {
        eprintln!("[mg] frame failed: {e}");
        s.log.push(format!("frame failed: {e}"));
        s.state = SessionState::Failed(e);
        return;
    }
    let events: Vec<mgvm::FeEvent> = host.events.drain(..).collect();
    if let Some(v) = fe.as_mut() {
        forward_events(&mut v.0.vm, &events, s);
    }
    // front-end button presses become guest callbacks
    let cmds: Vec<String> = fe.as_mut().map(|v| std::mem::take(&mut v.0.vm.fe.mg_cmds)).unwrap_or_default();
    let mut pause_req = fe.as_mut().and_then(|v| v.0.vm.fe.pause_req.take());
    let (vm, host) = s.vm.as_mut().unwrap();
    for c in &cmds {
        let r = match c.as_str() {
            "OnPlay" => {
                let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
                vm.call_by_name(host, "OnPlay__8MinigameFv", &[mg], &[]).map(|_| true)
            }
            other => mgvm::game_callback(vm, host, other),
        };
        if let Err(e) = r {
            s.log.push(format!("callback {c} failed: {e}"));
        }
    }
    if let Some(req) = pause_req.take() {
        use crate::fe_host::PauseReq;
        let name = match req {
            PauseReq::Resume => "OnPauseContinue",
            PauseReq::Restart => "OnPauseReset",
            PauseReq::Quit => "OnPauseQuit",
        };
        let r = if matches!(req, PauseReq::Quit) {
            let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
            vm.call_by_name(host, "OnPauseQuit__8MinigameFv", &[mg], &[]).map(|_| true)
        } else {
            mgvm::game_callback(vm, host, name)
        };
        if let Err(e) = r {
            s.log.push(format!("{name} failed: {e}"));
        }
    }
    let (vm, host) = s.vm.as_mut().unwrap();
    let snapshot = mgvm::snapshot::snapshot(vm, host);
    if let (Some(bd), Some((eye, target, _))) = (backdrop.as_mut(), snapshot.camera) {
        bd.set_now(Vec3::from(eye), Vec3::from(target));
    }
    match snap {
        Some(mut r) => r.0 = Some(snapshot),
        None => {
            commands.insert_resource(MgSnapshot(Some(snapshot)));
        }
    }
}

/// Guest front-end calls -> the APT front end: `Apt::Name(args)` are `AptCallFunction`s of the running movie,
/// `FEManager::*` open / close screens and overlays, `PreGameInfo` carries the pre-game / pause screen data.
fn forward_events(vm: &mut crate::apt_vm::Vm, events: &[mgvm::FeEvent], s: &mut Session) {
    use crate::apt_vm::V;
    vm.fe.vm_session = true;
    for e in events {
        if std::env::var("EAGL_MG_DEBUG").is_ok() {
            eprintln!("[mg] event {} {:?}", e.name, e.args);
        }
        let str_arg = |i: usize| match e.args.get(i) {
            Some(mgvm::FeArg::Str(t)) => t.clone(),
            _ => String::new(),
        };
        match e.name.as_str() {
            "FEManager::OpenAptScreen" if str_arg(0) == "WorldHud" => s.skip_close += 1,
            "FEManager::CloseAptScreen" if s.skip_close > 0 => s.skip_close -= 1,
            n if n.starts_with("Apt::WorldHud_") => {}
            "FEManager::OpenAptScreen" => {
                if str_arg(0).ends_with("Hud") {
                    vm.fe.hud_loaded = false;
                }
                vm.call_exposed("OpenScreen", vec![V::Str(str_arg(0).as_str().into())]);
            }
            "FEManager::CloseAptScreen" => {
                vm.call_exposed("CloseScreen", vec![]);
            }
            "FEManager::OpenAptOverlay" => {
                vm.fe.paused = true;
                vm.fe.pause_req = None;
                vm.fe.todo.push(("OpenOverlay".into(), vec![V::Str(str_arg(0).as_str().into())]));
            }
            "FEManager::CloseAptOverlay" => vm.fe.todo.push(("CloseOverlay".into(), vec![])),
            "FEManager::ReplaceAptScreen" => {
                vm.call_exposed("ReplaceScreen", vec![V::Str(str_arg(0).as_str().into())]);
            }
            "FEManager::ClearScreenStack" => {
                vm.call_exposed("ClearScreenStack", vec![]);
            }
            "PreGameInfo" => {
                let w = |i: usize| match e.args.get(2 + i) {
                    Some(mgvm::FeArg::Int(n)) => *n as u32,
                    _ => 0,
                };
                // [pause?, controller, multiplayer, game type]
                vm.fe.pause_words = if w(0) != 0 { Some([w(0), w(1), w(2), w(3)]) } else { None };
                if let Some(mgvm::FeArg::Int(t)) = e.args.first() {
                    vm.fe.mp.minigame = *t;
                }
            }
            n => {
                if let Some(f) = n.strip_prefix("Apt::") {
                    let args: Vec<V> = e.args.iter().map(|a| match a {
                        mgvm::FeArg::Str(t) => V::Str(t.as_str().into()),
                        mgvm::FeArg::Int(i) => V::Num(*i as f64),
                        mgvm::FeArg::Float(x) => V::Num(*x as f64),
                    }).collect();
                    vm.call_exposed(f, args);
                }
            }
        }
    }
    s.last_events.extend(events.iter().cloned());
    if s.last_events.len() > 200 {
        let n = s.last_events.len() - 200;
        s.last_events.drain(..n);
    }
}

fn mat_to_transform(radius: f32, m: &[f32; 16]) -> Transform {
    let rot = Mat4::from_cols(Vec4::new(m[0], m[1], m[2], 0.), Vec4::new(m[4], m[5], m[6], 0.), Vec4::new(m[8], m[9], m[10], 0.), Vec4::W);
    let p = Vec3::new(m[12], m[13], m[14]);
    Transform::from_matrix(game::display_matrix(radius, p) * rot)
}

#[allow(clippy::too_many_arguments)]
fn present(
    mut commands: Commands,
    s: Option<NonSendMut<Session>>,
    snap: Option<Res<MgSnapshot>>,
    game: Option<Res<game::Game>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut ibp: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    mut roots: Query<(&MgChar, &mut Transform), (Without<MgJoint>, Without<MgProp>)>,
    mut joints: Query<(&MgJoint, &mut Transform), (Without<MgChar>, Without<MgProp>)>,
    mut props: Query<(&MgProp, &mut Transform, &mut Visibility), (Without<MgChar>, Without<MgJoint>)>,
) {
    let (Some(mut s), Some(snap), Some(game)) = (s, snap, game) else { return };
    if s.state != SessionState::Running {
        return;
    }
    let Some(snapshot) = snap.0.as_ref() else { return };
    let s = &mut *s;
    let radius = game.world_radius;
    let Some(loaded) = s.loaded.as_ref() else { return };
    // characters
    for c in &snapshot.chars {
        if !s.chars.contains_key(&c.ptr) {
            let kid = s.vm.as_ref().and_then(|(_, h)| h.kid_keys.iter().position(|k| *k == c.key));
            let up = kid.and_then(|k| s.kid_uploads.get(k).and_then(|u| u.as_ref())).or(s.default_upload.as_ref()).unwrap();
            let root = commands.spawn((MgEntity, MgChar(c.ptr), Transform::default(), Visibility::default())).id();
            let js = character::spawn_rig(&mut commands, &mut ibp, &loaded.player.skeleton, up, root, root);
            for (bone, j) in js.iter().enumerate() {
                commands.entity(*j).insert((MgEntity, MgJoint { ptr: c.ptr, bone }));
            }
            s.chars.insert(c.ptr, (root, js));
        }
    }
    for (r, mut t) in &mut roots {
        if let Some(c) = snapshot.chars.iter().find(|c| c.ptr == r.0) {
            let p = Vec3::new(c.pos[0], c.pos[1], c.pos[2]);
            *t = Transform::from_matrix(game::display_matrix(radius, p) * Mat4::from_rotation_y(c.angle));
        }
    }
    for (j, mut t) in &mut joints {
        if let Some(c) = snapshot.chars.iter().find(|c| c.ptr == j.ptr) {
            if let Some((q, tr, sc)) = c.pose.get(j.bone) {
                t.rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
                t.translation = Vec3::from(*tr);
                t.scale = Vec3::from(*sc);
            }
        }
    }
    // model draws (balls, props): one pooled entity per draw call
    for (i, (name, m)) in snapshot.draws.iter().enumerate() {
        if i >= s.props.len() || s.props[i].1 != *name {
            if i < s.props.len() {
                let old = s.props[i].0;
                commands.entity(old).despawn();
                s.props.truncate(i);
            }
            if !s.prop_models.contains_key(name) {
                if let Some(src) = find_model_source(name) {
                    if let Ok(model) = assets::build(&src, &crate::model::Schemas::embedded()) {
                        let up = assets::upload(&model, &mut meshes, &mut materials, &mut images, false);
                        s.prop_models.insert(name.clone(), up);
                    }
                }
            }
            if std::env::var("EAGL_MG_DEBUG").is_ok() {
                eprintln!("[mg] prop {i} {name} found={} pos {:?}", s.prop_models.contains_key(name), &m[12..15]);
            }
            let e = match s.prop_models.get(name) {
                Some(up) => assets::spawn(&mut commands, up, (MgEntity, MgProp(i), mat_to_transform(radius, m)), None),
                None => commands.spawn((MgEntity, MgProp(i), Transform::default(), Visibility::Hidden)).id(),
            };
            s.props.push((e, name.clone()));
        }
    }
    if std::env::var("EAGL_MG_DEBUG").is_ok() && s.frames % 120 == 0 {
        eprintln!("[mg] frame {} draws {:?}", s.frames, snapshot.draws.iter().map(|(n, m)| (n.as_str(), [m[12], m[13], m[14]])).collect::<Vec<_>>());
    }
    for (p, mut t, mut v) in &mut props {
        if let Some((_, m)) = snapshot.draws.get(p.0) {
            *t = mat_to_transform(radius, m);
            *v = Visibility::Inherited;
        } else {
            *v = Visibility::Hidden;
        }
    }
}
