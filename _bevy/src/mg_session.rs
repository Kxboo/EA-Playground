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
/// A world placeable whose visibility the running minigame changed (`Placeable+0xa8`); restored at teardown.
#[derive(Component)]
pub struct MgPlaceable(Visibility);
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
    /// Character database key -> model asset name (`bestiary/*/asset_name`).
    asset_by_key: HashMap<u64, String>,
    /// Model name -> loaded model for the props the game draws.
    props: HashMap<String, assets::BuiltModel>,
}

enum Msg {
    Loaded(Box<Loaded>),
    Failed(String),
}

pub struct Session {
    /// The playground's curvature radius while a minigame bends its world differently.
    world_radius: Option<f32>,
    /// Launched from world play (Free Throw at a hoop) rather than the front end.
    from_world: bool,
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
    /// Models of kids outside the eight-kid list, loaded when they first appear.
    extra_uploads: HashMap<u64, Option<assets::Uploaded>>,
    pub t: f64,
    pub frames: i32,
    skip_close: i32,
    pub last_events: Vec<mgvm::FeEvent>,
    /// Accelerometer offsets still to play out (one entry per frame): the keyboard's stand-in for swinging the Wii Remote.
    gesture: [std::collections::VecDeque<[i16; 3]>; 4],
    /// `EAGL_MG_POSTGAME=done|replay`: frame at which the post-game button is pressed (testing aid).
    auto_postgame: Option<i32>,
    /// `EAGL_MG_PAUSEBTN=resume|restart|quit`: frame at which that pause-menu button is pressed (testing aid).
    auto_pause: Option<i32>,
    /// The minigame's music track (`Audio::PlayMusic`), playing while the session lives.
    music: Option<crate::playback::Music>,
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
    // name -> "archive::entry" (misses too: the search reads every archive)
    static CACHE: std::sync::Mutex<Option<HashMap<String, Option<String>>>> = std::sync::Mutex::new(None);
    let key = name.to_lowercase();
    if let Some(hit) = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(&key) {
        return hit.clone();
    }
    let root = bridge::data_root().join("files").join("data");
    // every archive under minigames/ (tracks sit in minigames/rccars/tracks), then world props and microgames
    let mut dirs = vec![root.join("minigames")];
    let mut i = 0;
    while i < dirs.len() {
        if let Ok(rd) = std::fs::read_dir(&dirs[i]) {
            dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
        i += 1;
    }
    dirs.push(root.join("worldprops"));
    dirs.push(root.join("microgames"));
    let mut found = None;
    'search: for d in dirs {
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
            if let Some(e) = entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
                found = Some(format!("{}::{}", p.to_string_lossy(), e.name));
                break 'search;
            }
        }
    }
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, found.clone());
    found
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
        // every bestiary entry's model: the multiplayer handlers fill teams from a longer kid list than the eight above
        let bestiary = crate::vlt::string_hash64("bestiary");
        let mut asset_by_key = HashMap::new();
        for c in db.collections.iter().filter(|c| c.class_key == bestiary) {
            let mut cur = Some(c);
            while let Some(col) = cur {
                if let Some(a) = db.attribute(col, "asset_name").and_then(|v| v.as_str().map(String::from)) {
                    asset_by_key.insert(c.key, a);
                    break;
                }
                cur = db.collections.iter().find(|x| x.class_key == col.class_key && x.key == col.parent_key && col.parent_key != 0);
            }
        }
        let player = character::load(&data_dir("characters"), &schemas)?;
        let kids = kid_assets.iter().map(|a| if a == "alicia" { None } else { character::load_kid_model(&data_dir("characters"), a, &schemas).ok() }).collect();
        Ok(Loaded { player, kids, kid_assets, asset_by_key, props: HashMap::new() })
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
            world_radius: None,
            from_world: false,
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
            extra_uploads: HashMap::new(),
            t: 0.,
            frames: 0,
            skip_close: 0,
            last_events: vec![],
            gesture: Default::default(),
            auto_postgame: None,
            music: None,
            auto_pause: None,
            fe: None,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (launch_or_teardown, pump, build, step, present, crate::mg_draw::render).chain());
    app.add_systems(Update, world_microgames);
}

/// The minigame has ended (or F10): remove the session and everything it spawned.
#[derive(Resource)]
struct MgTeardown;

/// Free-throw hoops of the playground (school, park, stadium): `kFreeThrowPosition` (set up by `__sinit_mgfreethrow_cpp`;
/// the stadium spot's z as its `pg_starburst` marker shows it) with the reach `UpdateMicroGameFreeThrow` tests (0x803ddbd8,
/// squared XZ distance 4).
const FREE_THROW_SPOTS: [[f32; 2]; 3] = [[45.63, -54.87], [27.04, -13.26], [-54.7745, 12.539]];

/// `PlaygroundWorld::UpdateMicroGameFreeThrow`: in world play, standing at a hoop shows the World HUD's "press A" and A
/// starts Free Throw (the original asks through an accessibility popup first).
fn world_microgames(
    play: Option<Res<game::WorldPlay>>,
    session: Option<NonSend<Session>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut Transform, With<game::Player>>,
    mut fe: Option<NonSendMut<crate::apt_view::AptViewNs>>,
    mut shown: Local<bool>,
    mut scripted_press: Local<bool>,
) {
    let Some(fe) = fe.as_mut() else { return };
    let vm = &mut fe.0.vm;
    if let Some((x, z)) = vm.fe.script_teleport.take() {
        for mut t in &mut players {
            t.translation.x = x;
            t.translation.z = z;
        }
    }
    let active = play.as_ref().is_some_and(|p| !p.paused) && session.is_none();
    let near = active && players.iter().any(|t| FREE_THROW_SPOTS.iter().any(|s| (t.translation.x - s[0]).powi(2) + (t.translation.z - s[1]).powi(2) <= 4.));
    if near != *shown {
        vm.call_exposed("PressA_SetVisible", vec![crate::apt_vm::V::Num(near as i32 as f64)]);
        *shown = near;
    }
    // `EAGL_WORLD_PRESS_A` presses A once at the first hoop reached (testing aid)
    let a = [KeyCode::Space, KeyCode::KeyZ].iter().any(|k| keys.just_pressed(*k)) || (std::env::var("EAGL_WORLD_PRESS_A").is_ok() && !*scripted_press);
    if near && a && vm.fe.launch.is_none() {
        *scripted_press = true;
        vm.call_exposed("PressA_SetVisible", vec![crate::apt_vm::V::Num(0.)]);
        *shown = false;
        vm.fe.launch = Some("mg:8".into());
    }
}

/// A playground entity hidden while the minigame draws its own environment, with its visibility before.
#[derive(Component)]
struct MgHiddenWorld(Visibility);

fn snapshot_hides_world(snap: Option<&MgSnapshot>) -> bool {
    snap.and_then(|s| s.0.as_ref()).is_some_and(|s| !s.render_world)
}

fn launch_or_teardown(world: &mut World) {
    if let Some(l) = world.remove_resource::<MgLaunch>() {
        if world.get_non_send_resource::<Session>().is_none() {
            let mut s = Session::new(l.ty, l.humans);
            s.fe = l.fe;
            // started from world play (Free Throw): the world waits, the session's camera rig takes over, the kid steps aside
            if let Some(mut p) = world.get_resource_mut::<game::WorldPlay>() {
                p.minigame = true;
                s.from_world = true;
                if world.get_resource::<game::Backdrop>().is_none() {
                    world.insert_resource(game::Backdrop::new(Vec3::new(0., 2., 0.), Vec3::new(0., 0., 1.)));
                }
                if let Some(pl) = world.get_resource::<game::Game>().and_then(|g| g.player_entity()) {
                    if let Ok(mut em) = world.get_entity_mut(pl) {
                        em.insert(Visibility::Hidden);
                    }
                }
            }
            world.insert_non_send_resource(s);
        }
    }
    let quit = world.get_non_send_resource::<Session>().is_some() && world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F10);
    if quit || world.remove_resource::<MgTeardown>().is_some() {
        if let Some(mut v) = world.get_non_send_resource_mut::<crate::apt_view::AptViewNs>() {
            v.0.vm.fe.vm_session = false;
        }
        if world.get_non_send_resource::<Session>().is_some_and(|s| s.from_world) {
            world.remove_resource::<game::Backdrop>();
            if let Some(mut p) = world.get_resource_mut::<game::WorldPlay>() {
                p.minigame = false;
            }
            if let Some(pl) = world.get_resource::<game::Game>().and_then(|g| g.player_entity()) {
                if let Ok(mut em) = world.get_entity_mut(pl) {
                    em.insert(Visibility::Inherited);
                }
            }
        }
        if let Some(r) = world.get_non_send_resource::<Session>().and_then(|s| s.world_radius) {
            if let Some(mut g) = world.get_resource_mut::<game::Game>() {
                g.world_radius = r;
            }
        }
        world.remove_non_send_resource::<Session>();
        world.remove_resource::<MgSnapshot>();
        if let Some(mut fx) = world.get_resource_mut::<crate::fx::FxWorld>() {
            fx.instances.clear();
        }
        let mut hidden = world.query::<(Entity, &MgHiddenWorld)>();
        let back: Vec<(Entity, Visibility)> = hidden.iter(world).map(|(e, h)| (e, h.0)).collect();
        for (e, v) in back {
            if let Ok(mut em) = world.get_entity_mut(e) {
                em.insert(v).remove::<MgHiddenWorld>();
            }
        }
        let mut swapped = world.query::<(Entity, &MgPlaceable)>();
        let back: Vec<(Entity, Visibility)> = swapped.iter(world).map(|(e, p)| (e, p.0)).collect();
        for (e, v) in back {
            if let Ok(mut em) = world.get_entity_mut(e) {
                em.insert(v).remove::<MgPlaceable>();
            }
        }
        // the world camera's lens back to Bevy's default
        let mut cams = world.query_filtered::<&mut Projection, With<game::GameCamera>>();
        for mut p in cams.iter_mut(world) {
            if let Projection::Perspective(pp) = &mut *p {
                pp.fov = std::f32::consts::FRAC_PI_4;
                pp.near = PerspectiveProjection::default().near;
            }
        }
        // the minigame's track stopped with the session; the menus / playground get theirs back
        if let Some(mut g) = world.get_resource_mut::<game::Game>() {
            g.start_music();
        }
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

/// `EAGL_MG_TILT="from-to:dx[,dy,dz];..."` adds accelerometer offsets by frame number (testing aid).
fn scripted_tilt(frame: i32) -> [i16; 3] {
    let mut d = [0i16; 3];
    for part in std::env::var("EAGL_MG_TILT").unwrap_or_default().split(';') {
        let Some((range, v)) = part.split_once(':') else { continue };
        let Some((a, z)) = range.split_once('-') else { continue };
        let (Ok(a), Ok(z)) = (a.parse::<i32>(), z.parse::<i32>()) else { continue };
        if frame >= a && frame < z {
            for (i, t) in v.split(',').enumerate().take(3) {
                d[i] += t.trim().parse::<i16>().unwrap_or(0);
            }
        }
    }
    d
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

fn step(mut commands: Commands, s: Option<NonSendMut<Session>>, keys: Res<ButtonInput<KeyCode>>, mouse: Res<ButtonInput<MouseButton>>, windows: Query<&Window, With<bevy::window::PrimaryWindow>>, time: Res<Time<Real>>, mut backdrop: Option<ResMut<game::Backdrop>>, snap: Option<ResMut<MgSnapshot>>, mut fe: Option<NonSendMut<crate::apt_view::AptViewNs>>, mut world_game: Option<ResMut<game::Game>>, mut projection: Query<&mut Projection, With<game::GameCamera>>, mut fxw: Option<ResMut<crate::fx::FxWorld>>) {
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
        if seat == 0 {
            let t = scripted_tilt(s.frames);
            d = [d[0] + t[0], d[1] + t[1], d[2] + t[2]];
        }
        host.pads[seat].acc = [rest[0] + d[0], rest[1] + d[1], rest[2] + d[2]];
    }
    if let Err(e) = mgvm::frame(vm, host, ms) {
        eprintln!("[mg] frame failed: {e}");
        s.log.push(format!("frame failed: {e}"));
        s.state = SessionState::Failed(e);
        return;
    }
    // music: the minigame's track replaces the world's; "world" (EndMinigame) hands back to it
    if let Some(track) = host.music.take() {
        s.music = None;
        match track.as_str() {
            "" => {}
            "world" => {
                if let Some(g) = world_game.as_mut() {
                    g.start_music();
                }
            }
            file => {
                if let Some(g) = world_game.as_mut() {
                    g.stop_music();
                }
                if !std::env::args().any(|a| a == "--mute") {
                    s.music = Some(crate::playback::Music::start(bridge::data_root().join("files").join("data").join("audio").join("music").join(file), 0.4));
                }
            }
        }
    }
    // particle effects (`PartFxManager`), simulated and drawn by `fx`
    if let Some(fx) = fxw.as_mut() {
        for e in std::mem::take(&mut host.fx) {
            match e {
                mgvm::FxEvent::Spawn { id, name, pos } => fx.spawn(id, &name, pos),
                mgvm::FxEvent::Move { id, pos } => fx.move_to(id, pos),
                mgvm::FxEvent::Stop { id, delay } => fx.stop(id, delay),
            }
        }
        fx.update(ms as f32 / 1000.);
    }
    // sounds the original picked (`AuAEMSManager::PlaySFX` id switch -> Csis class + variant)
    for (class, variant) in std::mem::take(&mut host.sounds) {
        crate::sfx::play_class(&class, variant, 1.);
    }
    let events: Vec<mgvm::FeEvent> = host.events.drain(..).collect();
    if let Some(v) = fe.as_mut() {
        // answers for the HUD's own queries (`MinigameLVHandlers`)
        let mg = vm.r32(mgvm::snapshot::WORLD_MAN + 0x90);
        v.0.vm.fe.mg_num_huds = match (s.ty, mg) {
            (_, 0) => 0,
            (0, _) => vm.call_by_name(host, "GetNumHuds__14MGDartShootoutFv", &[mg], &[]).unwrap_or(0) as i32,
            (5, _) => vm.call_by_name(host, "GetNumHuds__16MGPaperAirplanesFv", &[mg], &[]).unwrap_or(0) as i32,
            (1, _) => vm.r32(mg + 0x16c) as i32,
            _ => 0,
        };
        v.0.vm.fe.mg_lv = minigame_lv(vm, host, s.ty, mg);
        forward_events(&mut v.0.vm, &events, s);
    }
    // front-end button presses become guest callbacks
    let cmds: Vec<String> = fe.as_mut().map(|v| std::mem::take(&mut v.0.vm.fe.mg_cmds)).unwrap_or_default();
    if s.auto_pause == Some(s.frames) {
        if let Some(v) = fe.as_mut() {
            use crate::fe_host::PauseReq;
            v.0.vm.fe.pause_req = Some(match std::env::var("EAGL_MG_PAUSEBTN").as_deref() {
                Ok("quit") => PauseReq::Quit,
                Ok("restart") => PauseReq::Restart,
                _ => PauseReq::Resume,
            });
        }
    }
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
    // the minigame's own curvature (restored at teardown)
    if let (Some(r), Some(g)) = (snapshot.curved_radius, world_game.as_mut()) {
        if g.world_radius != r {
            if s.world_radius.is_none() {
                s.world_radius = Some(g.world_radius);
            }
            g.world_radius = r;
        }
    }
    if let (Some(bd), Some((eye, target, _))) = (backdrop.as_mut(), snapshot.camera) {
        // development aid: watch from this many units behind the game's camera
        let back = std::env::var("EAGL_MG_CAMBACK").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.);
        let (eye, target) = (Vec3::from(eye), Vec3::from(target));
        bd.set_now(eye - (target - eye).normalize_or_zero() * back, target);
    }
    for mut p in &mut projection {
        if let Projection::Perspective(pp) = &mut *p {
            pp.fov = snapshot.fov;
            pp.near = snapshot.near;
        }
    }
    match snap {
        Some(mut r) => r.0 = Some(snapshot),
        None => {
            commands.insert_resource(MgSnapshot(Some(snapshot)));
        }
    }
}

/// The rest of `MinigameLVHandlers::DoJobLV` (0x803194ac): what the HUD movies query from the running game.
fn minigame_lv(vm: &mut mgvm::MgVm, host: &mut mgvm::MgHost, ty: i32, mg: u32) -> HashMap<String, Vec<(String, String)>> {
    let mut out = HashMap::new();
    // `Counter_GetText`: `lapCounterText`, the wide string `LapCounter_SetValue` last stored
    if let Some(a) = vm.img.addr("lapCounterText") {
        let text: String = char::decode_utf16((0..64).map(|i| vm.st.mem.r16(a + 2 * i)).take_while(|&c| c != 0)).map(|c| c.unwrap_or('?')).collect();
        out.insert("Counter_GetText".to_string(), vec![("strText".to_string(), text)]);
    }
    if mg != 0 && ty == 4 {
        let hits = vm.call_by_name(host, "GetMaxJuggleHits__8MGFootieCFv", &[mg], &[]).unwrap_or(4);
        out.insert("GetGameRules".to_string(), vec![("iMaxHits".to_string(), hits.to_string())]);
        out.insert("Footie_IsSaveDare".to_string(), vec![("iIsSaveDare".to_string(), ((vm.st.mem.r8(mg + 0x244) != 0) as i32).to_string())]);
    }
    if let (Some(n), Some(p)) = (vm.img.addr("sNumCheckPoints"), vm.img.addr("sCheckPointsPositions")) {
        let n = (vm.r32(n) as i32).clamp(0, 64) as u32;
        let list: Vec<String> = (0..n).map(|i| vm.st.mem.rf32(p + 4 * i).to_string()).collect();
        out.insert("PaperAirPlanes_GetCheckPoints".to_string(), vec![("aiCheckPoints".to_string(), list.join(crate::fe_host::DELIM))]);
    }
    out
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
                if std::env::var("EAGL_MG_PAUSEBTN").is_ok() {
                    s.auto_pause = Some(s.frames + 90);
                }
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

pub(crate) fn mat_to_transform(radius: f32, m: &[f32; 16]) -> Transform {
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
    mut placeables: Query<(Entity, &Name, &mut Visibility, Option<&MgPlaceable>), (With<game::PlaceableProp>, Without<MgProp>)>,
    cams: Query<&GlobalTransform, With<game::GameCamera>>,
    layers: Query<(Entity, &Visibility), (With<game::WorldLayer>, Without<MgProp>, Without<game::PlaceableProp>, Without<MgHiddenWorld>)>,
    already_hidden: Query<(), With<MgHiddenWorld>>,
) {
    // `gRenderWorld` off (RcCars): the game's own model replaces the playground (terrain and props; restored at teardown)
    if snapshot_hides_world(snap.as_deref()) {
        for (e, v) in &layers {
            commands.entity(e).insert((MgHiddenWorld(*v), Visibility::Hidden));
        }
        for (e, _, v, _) in &placeables {
            if !already_hidden.contains(e) {
                commands.entity(e).insert((MgHiddenWorld(*v), Visibility::Hidden));
            }
        }
    }
    let (Some(mut s), Some(snap), Some(game)) = (s, snap, game) else { return };
    if s.state != SessionState::Running {
        return;
    }
    let Some(snapshot) = snap.0.as_ref() else { return };
    let s = &mut *s;
    let radius = game.world_radius;
    let Some(loaded) = s.loaded.as_ref() else { return };
    // world placeables the minigame swapped (tetherball: plain pole in, pole-with-ball out)
    for (name, visible) in &snapshot.placeables {
        if std::env::var("EAGL_MG_DEBUG").is_ok() && s.frames % 300 == 1 {
            let similar: Vec<String> = placeables.iter().map(|(_, n, _, _)| n.as_str().to_string()).filter(|n| n.contains("tetherball")).collect();
            eprintln!("[mg] placeable {name} -> {visible}; world has {similar:?}");
        }
        for (e, n, mut v, mark) in &mut placeables {
            if n.as_str() == name {
                if mark.is_none() {
                    commands.entity(e).insert(MgPlaceable(*v));
                }
                *v = if *visible { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
    // characters
    for c in &snapshot.chars {
        if !s.chars.contains_key(&c.ptr) {
            let kid = s.vm.as_ref().and_then(|(_, h)| h.kid_keys.iter().position(|k| *k == c.key));
            if kid.is_none() && !s.extra_uploads.contains_key(&c.key) {
                let model = loaded.asset_by_key.get(&c.key).filter(|a| *a != "alicia").and_then(|a| character::load_kid_model(&data_dir("characters"), a, &crate::model::Schemas::embedded()).ok());
                let up = model.map(|m| assets::upload_skinned(&m, &mut meshes, &mut materials, &mut images));
                s.extra_uploads.insert(c.key, up);
            }
            let up = match kid {
                Some(k) => s.kid_uploads.get(k).and_then(|u| u.as_ref()),
                None => s.extra_uploads.get(&c.key).and_then(|u| u.as_ref()),
            }
            .or(s.default_upload.as_ref())
            .unwrap();
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
            // the pool from here on no longer matches: drop all of it (a stale entity would keep its old model and take
            // the pose of whatever draw now has its index)
            for (old, _) in s.props.drain(i.min(s.props.len())..) {
                commands.entity(old).despawn();
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
        eprintln!("[mg] frame {} camera {:?} fov {} radius {radius}", s.frames, snapshot.camera, snapshot.fov);
        eprintln!("[mg] frame {} draws {:?}", s.frames, snapshot.draws.iter().map(|(n, m)| (n.as_str(), [m[12], m[13], m[14]])).collect::<Vec<_>>());
    }
    for (p, mut t, mut v) in &mut props {
        if let Some((name, m)) = snapshot.draws.get(p.0) {
            *t = mat_to_transform(radius, m);
            // development aid: leave out models whose name contains this text
            let hidden = std::env::var("EAGL_MG_HIDE").is_ok_and(|k| name.contains(&k));
            if std::env::var("EAGL_MG_DEBUG").is_ok() && s.frames % 120 == 0 && name.contains("dartgun_high") {
                if let Ok(c) = cams.single() {
                    let inv = c.to_matrix().inverse();
                    let tm = t.to_matrix();
                    let pts = [Vec3::ZERO, Vec3::new(0., 0., -0.287), Vec3::new(0., 0.18, 0.08)].map(|p| inv.transform_point3(tm.transform_point3(p)));
                    eprintln!("[mg] gun in camera space origin/tip/back {pts:?} scale {:?}", t.scale);
                }
            }
            *v = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        } else {
            *v = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod model_bounds_tests {
    /// Bounds of the prop models named in `EAGL_MG_MODELS` (comma separated), as the session loads them.
    #[test]
    #[ignore]
    fn prop_model_bounds() {
        let Ok(names) = std::env::var("EAGL_MG_MODELS") else { return };
        for n in names.split(',') {
            let src = super::find_model_source(n);
            let m = src.as_ref().and_then(|s| crate::assets::build(s, &crate::model::Schemas::embedded()).ok());
            if let Some(m) = &m {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in m.prims.iter().flat_map(|p| p.positions.iter()) {
                    for i in 0..3 {
                        lo[i] = lo[i].min(p[i]);
                        hi[i] = hi[i].max(p[i]);
                    }
                }
                println!("{n}: vertices {lo:?}..{hi:?} prims {} skinned {}", m.prims.len(), m.prims.iter().any(|p| p.joints.is_some()));
                println!("  warnings {:?}", m.warnings);
                for (i, mt) in m.materials.iter().enumerate() {
                    let tex = mt.texture.and_then(|t| m.textures.get(t)).map(|t| (t.name.clone(), t.alpha));
                    let tris: usize = m.prims.iter().filter(|p| p.material == i).map(|p| p.indices.len() / 3).sum();
                    println!("  material {i}: {tex:?} alpha {:?} colour {:?} tris {tris}", mt.alpha, mt.base_color);
                }
            }
            println!("{n}: {src:?} bounds {:?}", m.map(|m| m.bounds));
        }
    }
}

#[cfg(test)]
mod track_lane_tests {
    /// Which surface of the RcCars track model lies under (or over) each lane point (`EAGL_RC_POINTS="x,y,z;x,y,z"`).
    #[test]
    #[ignore]
    fn lane_points_on_track() {
        let Ok(spec) = std::env::var("EAGL_RC_POINTS") else { return };
        let src = super::find_model_source("rccartrack23.o").unwrap();
        let m = crate::assets::build(&src, &crate::model::Schemas::embedded()).unwrap();
        if let Ok(dir) = std::env::var("EAGL_RC_TEXDUMP") {
            for t in &m.textures {
                let Ok(f) = std::fs::File::create(std::path::Path::new(&dir).join(format!("{}.png", t.name))) else { continue };
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), t.width as u32, t.height as u32);
                enc.set_color(png::ColorType::Rgba);
                if let Ok(mut w) = enc.write_header() {
                    let _ = w.write_image_data(&t.rgba);
                }
            }
        }
        // facing of each material's triangles (+y up / -y down) per prim
        for (pi, prim) in m.prims.iter().enumerate() {
            let tex = m.materials[prim.material].texture.and_then(|t| m.textures.get(t)).map(|t| t.name.clone()).unwrap_or_default();
            if !tex.contains("Ashphalt") && !tex.contains("asphalt") {
                continue;
            }
            let (mut up, mut down) = (0, 0);
            for t in prim.indices.chunks(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| bevy::prelude::Vec3::from(prim.positions[i as usize]));
                let n = (b - a).cross(c - a);
                if n.y > 0.7 * n.length() { up += 1 } else if n.y < -0.7 * n.length() { down += 1 }
            }
            println!("prim {pi} {tex} {} family {} up {up} down {down}", prim.indices.len() / 3, prim.family);
        }
        for pt in spec.split(';') {
            let v: Vec<f32> = pt.split(',').filter_map(|t| t.trim().parse().ok()).collect();
            let logical = bevy::prelude::Vec3::new(v[0], v[1], v[2]);
            let p = crate::game::display_matrix(250., logical).w_axis.truncate();
            let mut best: Option<(f32, String)> = None;
            for prim in &m.prims {
                for t in prim.indices.chunks(3) {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| bevy::prelude::Vec3::from(prim.positions[i as usize]));
                    // vertical line through p against the triangle (barycentric in xz)
                    let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
                    if d.abs() < 1e-9 {
                        continue;
                    }
                    let l1 = ((b.z - c.z) * (p.x - c.x) + (c.x - b.x) * (p.z - c.z)) / d;
                    let l2 = ((c.z - a.z) * (p.x - c.x) + (a.x - c.x) * (p.z - c.z)) / d;
                    let l3 = 1. - l1 - l2;
                    if l1 < 0. || l2 < 0. || l3 < 0. {
                        continue;
                    }
                    let y = l1 * a.y + l2 * b.y + l3 * c.y;
                    let dy = (y - p.y).abs();
                    if dy < 2. && best.as_ref().is_none_or(|b| dy < b.0) {
                        let tex = m.materials[prim.material].texture.and_then(|t| m.textures.get(t)).map(|t| t.name.clone()).unwrap_or_default();
                        best = Some((y - p.y, tex));
                    }
                }
            }
            println!("{logical:?} -> display {p:?}: {best:?}");
        }
    }
}
