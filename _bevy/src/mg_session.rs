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
    /// The front end's choices (kids, teams, rules); `None` = a quick test launch (`gmN`, `EAGL_MG_ALLAI`).
    pub fe: Option<mgvm::FeLaunch>,
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
    /// Accelerometer offsets still to play out (one entry per frame): the keyboard's stand-in for swinging the Wii Remote.
    gesture: [std::collections::VecDeque<[i16; 3]>; 4],
    /// `EAGL_MG_POSTGAME=done|replay`: frame at which the post-game button is pressed (testing aid).
    auto_postgame: Option<i32>,
    pub fe: Option<mgvm::FeLaunch>,
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
            gesture: Default::default(),
            auto_postgame: None,
            fe: None,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (launch_or_teardown, pump, build, step, present).chain());
}

/// The minigame has ended (or F10): remove the session and everything it spawned.
#[derive(Resource)]
struct MgTeardown;

fn launch_or_teardown(world: &mut World) {
    if let Some(l) = world.remove_resource::<MgLaunch>() {
        if world.get_non_send_resource::<Session>().is_none() {
            let mut s = Session::new(l.ty, l.humans);
            s.fe = l.fe;
            world.insert_non_send_resource(s);
        }
    }
    let quit = world.get_non_send_resource::<Session>().is_some() && world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F10);
    if quit || world.remove_resource::<MgTeardown>().is_some() {
        if let Some(mut v) = world.get_non_send_resource_mut::<crate::apt_view::AptViewNs>() {
            v.0.vm.fe.vm_session = false;
        }
        world.remove_non_send_resource::<Session>();
        world.remove_resource::<MgSnapshot>();
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
    let (ty, humans, fe) = (s.ty, s.humans, s.fe.clone());
    let start = |vm: &mut mgvm::MgVm, host: &mut mgvm::MgHost| match &fe {
        Some(l) if std::env::var("EAGL_MG_ALLAI").is_err() => mgvm::launch_fe(vm, host, l),
        _ => mgvm::launch(vm, host, ty, humans),
    };
    match mgvm::boot().and_then(|(mut vm, mut host)| start(&mut vm, &mut host).map(|_| (vm, host))) {
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

/// Keyboard layout of a seat.  Alone, player 1 may also use the arrow keys; with two players, player 2 takes the arrows,
/// Enter / right Shift and the numeric keypad.
struct SeatKeys {
    up: &'static [KeyCode],
    down: &'static [KeyCode],
    left: &'static [KeyCode],
    right: &'static [KeyCode],
    a: &'static [KeyCode],
    b: &'static [KeyCode],
    one: &'static [KeyCode],
    two: &'static [KeyCode],
    plus: &'static [KeyCode],
    /// toss, strike left, strike right, boost, shot, juggle, dodge left, dodge right
    gestures: [KeyCode; 8],
}

const P1_SOLO: SeatKeys = SeatKeys {
    up: &[KeyCode::ArrowUp, KeyCode::KeyW],
    down: &[KeyCode::ArrowDown, KeyCode::KeyS],
    left: &[KeyCode::ArrowLeft, KeyCode::KeyA],
    right: &[KeyCode::ArrowRight, KeyCode::KeyD],
    a: &[KeyCode::Space, KeyCode::KeyZ],
    b: &[KeyCode::KeyX, KeyCode::ControlLeft],
    one: &[KeyCode::Digit1],
    two: &[KeyCode::Digit2],
    plus: &[KeyCode::KeyP, KeyCode::Escape],
    gestures: [KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyB, KeyCode::KeyI, KeyCode::KeyO, KeyCode::KeyQ, KeyCode::KeyE],
};
const P1_SHARED: SeatKeys = SeatKeys { up: &[KeyCode::KeyW], down: &[KeyCode::KeyS], left: &[KeyCode::KeyA], right: &[KeyCode::KeyD], ..P1_SOLO };
const P2: SeatKeys = SeatKeys {
    up: &[KeyCode::ArrowUp],
    down: &[KeyCode::ArrowDown],
    left: &[KeyCode::ArrowLeft],
    right: &[KeyCode::ArrowRight],
    a: &[KeyCode::Enter, KeyCode::NumpadEnter],
    b: &[KeyCode::ShiftRight],
    one: &[KeyCode::NumpadDivide],
    two: &[KeyCode::NumpadMultiply],
    plus: &[KeyCode::Backspace],
    gestures: [KeyCode::Numpad7, KeyCode::Numpad8, KeyCode::Numpad9, KeyCode::NumpadAdd, KeyCode::Numpad5, KeyCode::Numpad0, KeyCode::Numpad4, KeyCode::Numpad6],
};

fn seat_keys(seat: usize, humans: usize) -> Option<&'static SeatKeys> {
    if seat >= humans.max(1) {
        return None;
    }
    match (seat, humans) {
        (0, h) if h <= 1 => Some(&P1_SOLO),
        (0, _) => Some(&P1_SHARED),
        (1, _) => Some(&P2),
        _ => None,
    }
}

/// Keyboard -> Wii Remote buttons for a seat (WPAD bit values).
fn pad_buttons(keys: &ButtonInput<KeyCode>, k: &SeatKeys) -> u16 {
    let any = |ks: &[KeyCode]| ks.iter().any(|c| keys.pressed(*c));
    let mut b = 0u16;
    for (set, bit) in [(k.up, 0x0008), (k.down, 0x0004), (k.left, 0x0001), (k.right, 0x0002), (k.a, 0x0800), (k.b, 0x0400), (k.one, 0x0200), (k.two, 0x0100), (k.plus, 0x0010)] {
        if any(set) {
            b |= bit;
        }
    }
    b
}

/// Wii Remote swings as accelerometer pulses (offsets from rest; 1 g is about 100 counts). Waveforms found by sweeping the
/// original Conga state machines (`tools/gsweep.py`).
fn gesture_for(keys: &ButtonInput<KeyCode>, k: &SeatKeys) -> Option<Vec<[i16; 3]>> {
    let pulse = |axis: usize, v: i16, n: usize| vec![{ let mut a = [0i16; 3]; a[axis] = v; a }; n];
    let cat = |a: Vec<[i16; 3]>, b: Vec<[i16; 3]>| a.into_iter().chain(b).collect::<Vec<_>>();
    let g = k.gestures;
    let j = |i: usize| keys.just_pressed(g[i]);
    if j(0) {
        Some(pulse(1, 450, 3)) // toss / throw / shoot
    } else if j(1) {
        Some(cat(pulse(1, 450, 3), pulse(0, -450, 3))) // wind up, swing left (regular strike)
    } else if j(2) {
        Some(cat(pulse(1, 450, 3), pulse(0, 450, 3))) // wind up, swing right (reverse strike)
    } else if j(3) {
        Some(cat(pulse(1, -450, 3), pulse(1, 450, 3))) // paper plane boost
    } else if j(4) {
        Some(pulse(0, 450, 3)) // football shot
    } else if j(5) {
        Some(pulse(2, 450, 3)) // juggle
    } else if j(6) {
        Some(cat(pulse(0, 450, 3), pulse(0, -450, 3))) // dodge left
    } else if j(7) {
        Some(cat(pulse(0, -450, 3), pulse(0, 450, 3))) // dodge right
    } else {
        None
    }
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

fn step(mut commands: Commands, s: Option<NonSendMut<Session>>, keys: Res<ButtonInput<KeyCode>>, mouse: Res<ButtonInput<MouseButton>>, windows: Query<&Window, With<bevy::window::PrimaryWindow>>, time: Res<Time<Real>>, mut backdrop: Option<ResMut<game::Backdrop>>, snap: Option<ResMut<MgSnapshot>>, mut fe: Option<NonSendMut<crate::apt_view::AptViewNs>>) {
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
    // the mouse stands in for pointing the remote at the screen
    let scripted_pointer = std::env::var("EAGL_MG_POINTER").ok().and_then(|v| { let mut it = v.split(',').filter_map(|t| t.parse::<f32>().ok()); Some([it.next()?, it.next()?]) });
    host.pointer = scripted_pointer.or_else(|| windows.iter().next().and_then(|w| w.cursor_position().map(|c| [c.x / w.width() - 0.5, 0.5 - c.y / w.height()])));
    let rest = [512i16, 512, 616];
    for seat in 0..4 {
        let Some(k) = seat_keys(seat, s.humans) else {
            host.pads[seat].buttons = 0;
            host.pads[seat].acc = rest;
            continue;
        };
        let mut buttons = pad_buttons(&keys, k);
        if seat == 0 {
            buttons |= scripted_pad(s.frames) | if mouse.pressed(MouseButton::Left) { 0x0400 } else { 0 } | if mouse.pressed(MouseButton::Right) { 0x0800 } else { 0 };
        }
        host.pads[seat].buttons = buttons;
        if let Some(g) = gesture_for(&keys, k) {
            s.gesture[seat] = g.into();
        }
        let mut d = s.gesture[seat].pop_front().unwrap_or([0; 3]);
        let held = |set: &[KeyCode]| set.iter().any(|c| keys.pressed(*c));
        if s.ty == 5 {
            // paper airplane: pitch and bank by tilting the remote
            d[1] += if held(k.up) { -100 } else if held(k.down) { 100 } else { 0 };
            d[0] += if held(k.left) { -150 } else if held(k.right) { 150 } else { 0 };
        }
        if s.ty == 1 {
            // RcCars steers by tilting the remote
            d[0] += if held(k.left) { -150 } else if held(k.right) { 150 } else { 0 };
        }
        host.pads[seat].acc = [rest[0] + d[0], rest[1] + d[1], rest[2] + d[2]];
    }
    if let Err(e) = mgvm::frame(vm, host, ms) {
        eprintln!("[mg] frame failed: {e}");
        s.log.push(format!("frame failed: {e}"));
        s.state = SessionState::Failed(e);
        return;
    }
    // sounds the original picked (`AuAEMSManager::PlaySFX` id switch -> Csis class + variant)
    for (class, variant) in std::mem::take(&mut host.sounds) {
        crate::sfx::play_class(&class, variant, 1.);
    }
    let events: Vec<mgvm::FeEvent> = host.events.drain(..).collect();
    if let Some(v) = fe.as_mut() {
        forward_events(&mut v.0.vm, &events, s);
    }
    // front-end button presses become guest callbacks
    let cmds: Vec<String> = fe.as_mut().map(|v| std::mem::take(&mut v.0.vm.fe.mg_cmds)).unwrap_or_default();
    let mut pause_req = fe.as_mut().and_then(|v| v.0.vm.fe.pause_req.take());
    if s.auto_postgame == Some(s.frames) {
        if let Some(v) = fe.as_mut() {
            let button = if std::env::var("EAGL_MG_POSTGAME").as_deref() == Ok("replay") { "PostGame_OnReplay" } else { "PostGame_OnDone" };
            crate::fe_postgame::command(&mut v.0.vm, button, 0);
        }
    }
    // post-game buttons: `Minigame::OnReplay` / `OnDone`; the minigame's UpdatePostGame acts on the choice
    let postgame_choice = fe.as_mut().and_then(|v| v.0.vm.fe.postgame_choice.take());
    let (vm, host) = s.vm.as_mut().unwrap();
    if std::env::var("EAGL_MG_DEBUG").is_ok() && s.frames % 300 == 0 {
        let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
        eprintln!("[mg] frame {} state {} ms {}", s.frames, vm.r32(mg + 0x34), ms);
    }
    for c in &cmds {
        let r = match c.as_str() {
            "OnPlay" => {
                let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
                vm.call_by_name(host, "OnPlay__8MinigameFv", &[mg], &[]).map(|_| true)
            }
            other if other.starts_with("OnPlaneSelected:") => {
                let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
                let plane = other.rsplit(':').next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                vm.call_by_name(host, "OnPlaneSelected__16MGPaperAirplanesFi", &[mg, plane], &[]).map(|_| true)
            }
            other => mgvm::game_callback(vm, host, other),
        };
        if let Err(e) = r {
            s.log.push(format!("callback {c} failed: {e}"));
        }
    }
    if let Some(choice) = postgame_choice {
        let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
        let sym = if choice == 0 { "OnReplay__8MinigameFv" } else { "OnDone__8MinigameFv" };
        if mg != 0 {
            if let Err(e) = vm.call_by_name(host, sym, &[mg], &[]) {
                s.log.push(format!("{sym} failed: {e}"));
            }
        }
    }
    if let Some(req) = pause_req.take() {
        use crate::fe_host::PauseReq;
        if matches!(req, PauseReq::Quit) {
            if let Some(v) = fe.as_mut() {
                if v.0.vm.fe.after_exit.is_empty() {
                    v.0.vm.fe.mp.active = false;
                    v.0.vm.fe.after_exit = vec![(0, "ClearScreenStack".into(), vec![]), (10, "OpenScreen".into(), vec![crate::apt_vm::V::Str("MainMenu".into())])];
                }
            }
        }
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
    // `WorldMan::EndMinigame` destroyed the minigame: hand the screen back to the front end and leave
    if vm.r32(mgvm::snapshot::WORLD_MAN + 0x90) == 0 {
        if let Some(v) = fe.as_mut() {
            let fe = &mut v.0.vm.fe;
            for (delay, name, args) in std::mem::take(&mut fe.after_exit) {
                if delay == 0 { fe.todo.push((name, args)) } else { fe.later.push((delay, name, args)) }
            }
            fe.launch_done = true;
            fe.vm_session = false;
        }
        s.log.push("minigame ended".into());
        commands.insert_resource(MgTeardown);
        return;
    }
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
    let mut replace_next = false;
    for (i, e) in events.iter().enumerate() {
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
                let f = if std::mem::take(&mut replace_next) { "ReplaceScreen" } else { "OpenScreen" };
                vm.call_exposed(f, vec![V::Str(str_arg(0).as_str().into())]);
            }
            // closing a screen and opening the next in the same frame swaps them; popping first would briefly
            // reveal (and start reloading) whatever screen lies below
            "FEManager::CloseAptScreen" if events[i + 1..].iter().find(|n| n.name.starts_with("FEManager::")).is_some_and(|n| n.name == "FEManager::OpenAptScreen") => replace_next = true,
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
            "PostGameInfo" => {
                let words: Vec<u32> = e.args.iter().skip(1).map(|a| match a {
                    mgvm::FeArg::Int(n) => *n as u32,
                    _ => 0,
                }).collect();
                let kind = match e.args.first() {
                    Some(mgvm::FeArg::Int(k)) => *k,
                    _ => s.ty,
                };
                crate::fe_postgame::setup(vm, kind, &words);
                if std::env::var("EAGL_MG_POSTGAME").is_ok() {
                    s.auto_postgame = Some(s.frames + 90);
                }
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
    if std::env::var("EAGL_MG_DEBUG").is_ok() && s.frames % 120 == 0 {
        for c in &snapshot.chars {
            eprintln!("[mg] char {:#x} key {:x} anim {} bones {} spawned {} q1 {:?}", c.ptr, c.key, c.anim_state, c.pose.len(), s.chars.contains_key(&c.ptr), c.pose.get(1).map(|b| b.0));
        }
        eprintln!("[mg] joints {}", joints.iter().count());
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
