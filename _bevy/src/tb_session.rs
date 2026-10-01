//! Bevy presentation of the recovered tetherball minigame (`tb_host::TbHost` + `tetherball_runtime::Runtime`).
//!
//! The game logic is the recovered runtime; this module only (1) loads the original models, (2) turns host state into
//! entities (poles, ball and rope from the scene matrices, the two rigs posed by `tb_anim`), (3) maps keyboard input to
//! Wii buttons / Conga gestures, and (4) forwards the host's front-end events to the original `TetherballHud` /
//! pause / post-game APT screens.  The minigame is played inside the loaded world: the menu `Backdrop` camera is driven
//! with the host's follow camera.
use bevy::{prelude::*, window::RequestRedraw, winit::{UpdateMode, WinitSettings}};
use std::{rc::Rc, sync::{mpsc, Mutex}};

use crate::{
    apt_vm::V,
    assets, bridge, character,
    model,
    sim_time::FramePolicy,
    tb_host::{AnimAssets, Config, Hud, Out, TbHost},
    tetherball_gestures::GestureCallback,
    tetherball_runtime::Runtime,
    vlt,
};

/// Marker resource: a tetherball session exists (other systems test for it).
#[derive(Resource)]
pub struct TbActive;

#[derive(Component)]
struct TbEntity;
#[derive(Component)]
struct TbRoot(usize);
#[derive(Component)]
struct TbJoint {
    char: usize,
    bone: usize,
}
#[derive(Component)]
struct TbBall;
#[derive(Component)]
struct TbRope;
#[derive(Component)]
struct TbPole;
/// World props the minigame hides (kept hidden by the front end's visibility sync).
#[derive(Component)]
pub struct TbHidden;

struct Loaded {
    db: vlt::Database,
    assets: AnimAssets,
    pole: assets::BuiltModel,
    ball: assets::BuiltModel,
    rope: assets::BuiltModel,
    player: character::CharacterData,
    kids: [Option<assets::BuiltModel>; 2],
}

enum Msg {
    Loaded(Box<Loaded>),
    Failed(String),
}

#[derive(PartialEq, Clone, Debug)]
enum Load {
    Loading,
    Running,
    Failed(String),
}

pub struct Session {
    rx: Mutex<mpsc::Receiver<Msg>>,
    load: Load,
    cfg: Config,
    area: i32,
    loaded: Option<Box<Loaded>>,
    host: Option<TbHost>,
    rt: Option<Runtime>,
    hud_wait: bool,
    anim_wait: bool,
    anim_playing: bool,
    held_pause: u8,
    /// Seconds since the session started (for scripted tests).
    t: f32,
    accum: f32,
    pub log: Vec<String>,
    fe: bool,
    auto: bool,
    over: bool,
    hidden_props: Vec<Entity>,
    restart: bool,
}

fn data_dir(rel: &str) -> String {
    bridge::data_root().join("files").join("data").join(rel.replace('/', "\\")).to_string_lossy().into_owned()
}

fn loader(tx: mpsc::Sender<Msg>, area: i32, kids: [Option<String>; 2]) {
    let schemas = model::Schemas::embedded();
    let run = || -> Result<Loaded, String> {
        let pole_name = if area == 2 { "tetherball_pole_nature" } else { "tetherball_pole" };
        let pole = assets::build(&format!("{}::{pole_name}.o", data_dir(&format!("placeables/{pole_name}.viv"))), &schemas)?;
        let mg = data_dir("minigames/tetherball/mgtetherball.viv");
        let ball = assets::build(&format!("{mg}::teatherball.o"), &schemas)?;
        let rope = assets::build(&format!("{mg}::teatherball_rope.o"), &schemas)?;
        let player = character::load(&data_dir("characters"), &schemas)?;
        let kids = std::array::from_fn(|i| {
            kids[i].as_ref().filter(|a| a.as_str() != "alicia").and_then(|a| character::load_kid_model(&data_dir("characters"), a, &schemas).ok())
        });
        let dir = bridge::data_root().join("files").join("data").join("db");
        let (v, b) = (std::fs::read(dir.join("db.vlt")).map_err(|e| format!("db.vlt: {e}"))?, std::fs::read(dir.join("db.bin")).map_err(|e| format!("db.bin: {e}"))?);
        let db = vlt::Database::load(&v, &b, vlt::known_names())?;
        let assets = AnimAssets::load()?;
        Ok(Loaded { db, assets, pole, ball, rope, player, kids })
    };
    let _ = tx.send(match run() {
        Ok(l) => Msg::Loaded(Box::new(l)),
        Err(e) => Msg::Failed(e),
    });
}

impl Session {
    /// `kids`: roster asset names per seat; `female`: their gender; `fe`: launched from the front end (HUD / post-game screens).
    pub fn new(cfg: Config, kids: [Option<String>; 2], fe: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let area = cfg.area;
        std::thread::spawn(move || loader(tx, area, kids));
        Session {
            rx: Mutex::new(rx),
            load: Load::Loading,
            cfg,
            area,
            loaded: None,
            host: None,
            rt: None,
            hud_wait: false,
            anim_wait: false,
            anim_playing: false,
            held_pause: 0,
            t: 0.,
            accum: 0.,
            log: vec!["Tetherball: reading original data files".into()],
            fe,
            auto: std::env::var("EAGL_TB_AUTO").is_ok(),
            over: false,
            hidden_props: vec![],
            restart: false,
        }
    }
}

/// Request to start a session; honoured by an exclusive system because the session holds non-`Send` data.
#[derive(Resource)]
pub struct TbLaunch {
    pub cfg: Config,
    pub kids: [Option<String>; 2],
}
#[derive(Resource)]
struct TbTeardown;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (launch_or_teardown, pump, build, step, present).chain());
}

fn launch_or_teardown(world: &mut World) {
    if world.contains_resource::<TbTeardown>() {
        world.remove_resource::<TbTeardown>();
        teardown(world);
    }
    if let Some(l) = world.remove_resource::<TbLaunch>() {
        if world.get_non_send_resource::<Session>().is_none() {
            world.insert_non_send_resource(Session::new(l.cfg, l.kids, true));
        }
    }
}

pub fn teardown(world: &mut World) {
    let ents: Vec<Entity> = world.query_filtered::<Entity, With<TbEntity>>().iter(world).collect();
    for e in ents {
        if let Ok(ec) = world.get_entity_mut(e) {
            ec.despawn();
        }
    }
    if let Some(s) = world.remove_non_send_resource::<Session>() {
        for e in s.hidden_props {
            if let Ok(mut ec) = world.get_entity_mut(e) {
                ec.insert(Visibility::Inherited).remove::<TbHidden>();
            }
        }
    }
    world.remove_resource::<TbActive>();
}

fn pump(s: Option<NonSendMut<Session>>, mut settings: ResMut<WinitSettings>, mut redraw: MessageWriter<RequestRedraw>) {
    let Some(mut s) = s else { return };
    settings.focused_mode = UpdateMode::Continuous;
    settings.unfocused_mode = UpdateMode::Continuous;
    redraw.write(RequestRedraw);
    if s.load != Load::Loading || s.loaded.is_some() {
        return;
    }
    let msgs: Vec<Msg> = s.rx.lock().unwrap().try_iter().collect();
    for m in msgs {
        match m {
            Msg::Failed(e) => {
                s.log.push(format!("FAILED: {e}"));
                s.load = Load::Failed(e);
            }
            Msg::Loaded(l) => s.loaded = Some(l),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    s: Option<NonSendMut<Session>>,
    game: Option<Res<crate::game::Game>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut ibp: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    names: Query<(Entity, &Name)>,
) {
    let Some(mut s) = s else { return };
    if s.load != Load::Loading || s.loaded.is_none() {
        return;
    }
    let Some(game) = game else { return };
    let Some(loaded) = s.loaded.take() else { return };
    let l = *loaded;
    let Loaded { db, assets: anim_assets, pole, ball, rope, player, kids } = l;
    let mut cfg = s.cfg.clone();
    cfg.area = s.area;
    let anim_assets = Rc::new(anim_assets);
    let db = Rc::new(db);
    let mut host = match TbHost::new(db, anim_assets.clone(), cfg, 0x9e37_79b9_7f4a_7c15) {
        Ok(h) => h,
        Err(e) => {
            s.log.push(format!("FAILED: {e}"));
            s.load = Load::Failed(e);
            return;
        }
    };
    host.area = crate::area_transform::AreaTransform { radius: game.world_radius, disabled: game.world_radius <= 0. };
    // The pole's placeable and the terrain under it (queried once, like the original ground probe at the anchor).
    let probe = {
        let g = &*game;
        move |p: [f32; 3]| g.ground_height(p[0], p[2]).unwrap_or(p[1])
    };
    let ground_at_origin = host
        .db
        .find_collection("placeables", ["tetherball_pole_school_w_ball", "tetherball_pole_stadium_w_ball", "tetherball_pole_forest_w_ball"][s.area.clamp(0, 2) as usize])
        .and_then(|c| host.db.attribute(c, "position"))
        .and_then(|v| v.as_array().map(|a| [a[0].as_f64().unwrap_or(0.) as f32, a[1].as_f64().unwrap_or(0.) as f32, a[2].as_f64().unwrap_or(0.) as f32]))
        .map(|p| probe(p))
        .unwrap_or(0.);
    let _ = ground_at_origin;
    let started = host.start();
    let (rt, _st) = match started {
        Ok(v) => v,
        Err(e) => {
            s.log.push(format!("startup failed: {e}"));
            s.load = Load::Failed(e);
            return;
        }
    };
    // Hide the world's pole-with-ball placeable; the startup shows the plain pole at the same spot.
    let w_ball = ["tetherball_pole_school_w_ball", "tetherball_pole_stadium_w_ball", "tetherball_pole_forest_w_ball"][s.area.clamp(0, 2) as usize];
    for (e, n) in &names {
        if n.as_str() == w_ball {
            commands.entity(e).insert((Visibility::Hidden, TbHidden));
            s.hidden_props.push(e);
        }
    }
    let origin = Vec3::from(host.origin);
    let disp = |p: Vec3| crate::game::display_matrix(game.world_radius, p);
    let pole_up = assets::upload(&pole, &mut meshes, &mut materials, &mut images, false);
    assets::spawn(&mut commands, &pole_up, (TbEntity, TbPole, Transform::from_matrix(disp(origin))), None);
    let ball_up = assets::upload(&ball, &mut meshes, &mut materials, &mut images, false);
    assets::spawn(&mut commands, &ball_up, (TbEntity, TbBall, Transform::default()), None);
    let rope_up = assets::upload(&rope, &mut meshes, &mut materials, &mut images, false);
    assets::spawn(&mut commands, &rope_up, (TbEntity, TbRope, Transform::default()), None);
    let default_up = assets::upload_skinned(&player.model, &mut meshes, &mut materials, &mut images);
    for index in 0..2 {
        let own = kids[index].as_ref().map(|m| assets::upload_skinned(m, &mut meshes, &mut materials, &mut images));
        let up = own.as_ref().unwrap_or(&default_up);
        let root = commands.spawn((TbEntity, TbRoot(index), Transform::default(), Visibility::default())).id();
        let joints = character::spawn_rig(&mut commands, &mut ibp, &player.skeleton, up, root, root);
        for (bone, j) in joints.iter().enumerate() {
            commands.entity(*j).insert(TbJoint { char: index, bone });
        }
    }
    commands.insert_resource(TbActive);
    if std::env::var("EAGL_TB_DEBUG").is_ok() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(bridge::root().join("docs").join("tb-log.txt")) { let _ = writeln!(f, "bounds pole {:?} ball {:?} rope {:?}", pole.bounds, ball.bounds, rope.bounds); }
    }
    let ready = format!("ready: area {} origin {:?}", s.area, host.origin);
    s.log.push(ready);
    s.host = Some(host);
    s.rt = Some(rt);
    s.load = Load::Running;
}

/// Keyboard -> Wii buttons / gestures for one seat (`bit(n)` as in `input_buttons.tsv`).
struct Keys {
    swing: KeyCode,
    reverse: KeyCode,
    overhand: KeyCode,
    a: KeyCode,
    b: KeyCode,
}
const SEATS: [Keys; 2] = [
    Keys { swing: KeyCode::Space, reverse: KeyCode::KeyX, overhand: KeyCode::KeyZ, a: KeyCode::ShiftLeft, b: KeyCode::ControlLeft },
    Keys { swing: KeyCode::Enter, reverse: KeyCode::Period, overhand: KeyCode::Slash, a: KeyCode::ShiftRight, b: KeyCode::ControlRight },
];

fn fe_call(vm: &mut crate::apt_vm::Vm, name: &str, args: &[f64]) {
    vm.call_exposed(name, args.iter().map(|&n| V::Num(n)).collect());
}

#[allow(clippy::too_many_arguments)]
fn step(
    mut commands: Commands,
    s: Option<NonSendMut<Session>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut fe: Option<NonSendMut<crate::apt_view::AptViewNs>>,
    mut backdrop: Option<ResMut<crate::game::Backdrop>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut s) = s else { return };
    if s.load != Load::Running || s.over {
        return;
    }
    let s = &mut *s;
    let (Some(host), Some(rt)) = (s.host.as_mut(), s.rt.as_mut()) else { return };
    let step = FramePolicy::default().step((time.delta_secs_f64() * 1000.) as f32);
    let ms = step.simulation_ms as i32;
    s.t += step.seconds();

    // Front-end handshakes.
    if let Some(v) = fe.as_mut() {
        let vm = &mut v.0.vm;
        if s.hud_wait && vm.fe.hud_loaded {
            s.hud_wait = false;
            host.hud_loaded(rt);
            vm.fe.start_anim_done = false;
            fe_call(vm, "GameStartAnim_Play", &[]);
            s.anim_wait = true;
            s.anim_playing = true;
        }
        if s.anim_wait && vm.fe.start_anim_done {
            s.anim_wait = false;
            host.start_anim_complete(rt);
        }
        // Pause menu requests.
        if let Some(req) = vm.fe.pause_req.take() {
            use crate::fe_host::PauseReq;
            vm.fe.paused = false;
            vm.fe.pause_words = None;
            vm.fe.todo.push(("CloseOverlay".into(), vec![]));
            match req {
                PauseReq::Resume => {
                    rt.life.paused = false;
                    rt.state.serve.pause_menu_open = false;
                    rt.state.serve.pause_block_count_0fc = 1200;
                    host.take_out().into_iter().for_each(drop);
                }
                PauseReq::Restart => {
                    s.restart = true;
                }
                PauseReq::Quit => {
                    vm.fe.mp.active = false;
                    vm.fe.todo.push(("ReplaceScreen".into(), vec![V::Str("MainMenu".into())]));
                    s.over = true;
                }
            }
        }
        if std::mem::take(&mut vm.fe.script_pause) {
            s.held_pause = 3;
        }
        if let Some(choice) = vm.fe.postgame_choice.take() {
            rt.state.frontend.postgame_choice_05c = choice;
        }
    }

    // Input.
    let mut held = [0u32; 2];
    let state = rt.life.match_state.state_code;
    for (i, k) in SEATS.iter().enumerate() {
        if i >= host.cfg.humans {
            break;
        }
        if keys.pressed(k.a) || s.auto && i == 0 && false {
            held[i] |= 1 << 4;
        }
        if keys.pressed(k.b) {
            held[i] |= 1 << 5;
        }
        if keys.just_pressed(k.swing) {
            if state == 27 {
                host.gesture(rt, GestureCallback::ServeToss, i as i32);
            } else {
                // A Wiimote swing produces both strike gestures; each is accepted only for the player its controller belongs to.
                host.gesture(rt, GestureCallback::RegularStrike, i as i32);
                host.gesture(rt, GestureCallback::RegularStrikeReverse, i as i32);
            }
        }
        if keys.just_pressed(k.reverse) {
            host.gesture(rt, GestureCallback::RegularStrikeReverse, i as i32);
        }
        if keys.just_pressed(k.overhand) {
            host.gesture(rt, GestureCallback::OverhandStrike, i as i32);
        }
    }
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyP) {
        s.held_pause = 3;
    }
    if s.held_pause > 0 {
        held[0] |= 1 << 11;
        s.held_pause -= 1;
    }
    // Test aid: the human plays itself (serve, then strike whenever the ball is in range).
    if s.auto && host.cfg.humans >= 1 {
        let tick = (s.t * 1000.) as i32 / 64;
        match state {
            27 if rt.life.server == 0 && tick % 2 == 0 => {
                host.gesture(rt, if rt.ball.tossed { GestureCallback::RegularStrike } else { GestureCallback::ServeToss }, 0);
            }
            28 | 29 if host.in_swing_window(rt, 0) && tick % 2 == 0 => {
                host.gesture(rt, GestureCallback::RegularStrike, 0);
                host.gesture(rt, GestureCallback::RegularStrikeReverse, 0);
            }
            _ => {}
        }
    }
    if s.restart {
        s.restart = false;
        // OnPauseReset: the full reset, then the intro again (the paused flag is cleared by ClosePauseMenu).
        let _ = host.restart(rt);
    }

    if !s.over {
        match host.frame(rt, ms, held) {
            Ok(Some(2)) => s.over = true,
            Ok(_) => {}
            Err(e) => {
                s.log.push(format!("frame failed: {e}"));
                s.load = Load::Failed(e);
                return;
            }
        }
    }
    if let Some(bd) = backdrop.as_mut() {
        bd.set_now(Vec3::from(host.eye()), Vec3::from(host.camera_focus()));
    }

    if std::env::var("EAGL_TB_DEBUG").is_ok() && (s.t * 4.) as i32 != ((s.t - step.seconds()) * 4.) as i32 {
        let line = format!("t={:.2} state {} ball ang {:.2} r {:.2} h {:.2} bm {:?} rope {:?} eye {:?} focus {:?} chars {:?} anim {:?}", s.t, rt.life.match_state.state_code, rt.ball.angle, rt.ball.radius, rt.ball.height, &host.ball_matrix[12..15], &host.rope_matrix[12..15], host.eye(), host.camera_focus(), host.chars.iter().map(|c| c.pos).collect::<Vec<_>>(), rt.life.players.iter().map(|p| p.current_animation).collect::<Vec<_>>());
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(bridge::root().join("docs").join("tb-log.txt")) { let _ = writeln!(f, "{line}"); }
    }
    // Host -> front end.
    let outs = host.take_out();
    if let Some(v) = fe.as_mut() {
        let vm = &mut v.0.vm;
        for out in outs {
            match out {
                Out::Pregame { .. } => host.pregame_done(rt),
                Out::Hud(h) => match h {
                    Hud::OpenScreen(name) => {
                        if name == "TetherballHud" {
                            vm.fe.hud_loaded = false;
                            s.hud_wait = true;
                        }
                        vm.call_exposed("OpenScreen", vec![V::Str(name.into())]);
                    }
                    Hud::CloseScreen => {
                        vm.call_exposed("CloseScreen", vec![]);
                    }
                    Hud::CloseOverlay => vm.fe.todo.push(("CloseOverlay".into(), vec![])),
                    Hud::OpenOverlay(name) => {
                        vm.fe.paused = true;
                        vm.fe.pause_req = None;
                        vm.fe.todo.push(("OpenOverlay".into(), vec![V::Str(name.into())]));
                    }
                    Hud::Scoreboard(a) => fe_call(vm, "Scoreboard_SetVisible", &a.map(f64::from)),
                    Hud::Round(a) => fe_call(vm, "Round_SetVisible", &a.map(f64::from)),
                    Hud::ServeBubble(a) => fe_call(vm, "ServeBubble_SetVisible", &a.map(f64::from)),
                    Hud::MegaVisible(p, v) => fe_call(vm, "MegaMeter_SetVisible", &[p as f64, v as f64]),
                    Hud::MegaValue(p, v) => fe_call(vm, "MegaMeter_SetValue", &[p as f64, v as f64]),
                    Hud::TimerVisible(v) => fe_call(vm, "Timer_SetVisible", &[v as f64]),
                    Hud::ResetScoreboard => fe_call(vm, "Scoreboard_Reset", &[]),
                    Hud::Winner(ui, p, vis) => {
                        let name = match ui {
                            crate::tetherball_match::WinnerUi::WinLose => "WinLose_SetVisible",
                            crate::tetherball_match::WinnerUi::MultiplayerWin => "MultiplayerWin_SetVisible",
                        };
                        fe_call(vm, name, &[p as f64, f64::from(vis)]);
                    }
                    Hud::PostGame(_, words) => {
                        let w = |i: usize| words.get(i).copied().unwrap_or(0);
                        let n = (w(1) as usize).clamp(1, 2);
                        let winner = if w(0) > 1 { w(0x3c / 4) as i32 } else if w(0x3c / 4) == 0 { 0 } else { 1 };
                        let mut r = crate::fe_host::MpResult { winner, scores: [0; 2], hits: [0; 2], power_hits: [0; 2] };
                        for i in 0..n {
                            let o = (0x58 + i * 0x28) / 4;
                            r.scores[i] = w(0xf8 / 4 + i) as i32;
                            r.hits[i] = w(o) as i32;
                            r.power_hits[i] = w(o + 1) as i32;
                        }
                        vm.fe.mp.results = Some(r);
                        let next = if w(0) > 1 { "PostGameMP" } else { "PostGame" };
                        vm.call_exposed("OpenScreen", vec![V::Str(next.into())]);
                    }
                    Hud::SetupHandlers(_) | Hud::ClearPregameHandlers | Hud::ClearPostgameHandlers | Hud::Clear => {}
                },
                Out::PauseInfo { words, .. } => vm.fe.pause_words = Some(words),
                Out::Sound { .. } | Out::WiimoteSound { .. } => {}
                Out::PlayMusic(_) | Out::LoadAudio(_) => {}
                _ => {}
            }
        }
    }
    if rt.life.match_state.state_code == 9 && rt.state.frontend.postgame_choice_05c == -1 && !s.over {
        // Postgame "done": the wait-for-apocalypse state returns 2 once the fade finished -> leave the minigame.
    }
    if s.over {
        if let Some(v) = fe.as_mut() {
            let vm = &mut v.0.vm;
            for (delay, name, args) in std::mem::take(&mut vm.fe.after_exit) {
                if delay == 0 { vm.fe.todo.push((name, args)) } else { vm.fe.later.push((delay, name, args)) }
            }
            vm.fe.launch_done = true;
        }
        commands.insert_resource(TbTeardown);
    }
    let _ = &mut exit;
}

fn present(
    s: Option<NonSendMut<Session>>,
    game: Option<Res<crate::game::Game>>,
    mut roots: Query<(&TbRoot, &mut Transform), (Without<TbJoint>, Without<TbBall>, Without<TbRope>)>,
    mut joints: Query<(&TbJoint, &mut Transform), (Without<TbRoot>, Without<TbBall>, Without<TbRope>)>,
    mut ball: Query<&mut Transform, (With<TbBall>, Without<TbRoot>, Without<TbJoint>, Without<TbRope>)>,
    mut rope: Query<&mut Transform, (With<TbRope>, Without<TbRoot>, Without<TbJoint>, Without<TbBall>)>,
) {
    let Some(s) = s else { return };
    let s = &*s;
    let (Some(host), Some(game)) = (s.host.as_ref(), game) else { return };
    let radius = game.world_radius;
    let mat = |m: &[f32; 16]| Transform::from_matrix(Mat4::from_cols_array(m));
    for mut t in &mut ball {
        *t = mat(&host.ball_matrix);
    }
    for mut t in &mut rope {
        *t = mat(&host.rope_matrix);
    }
    for (r, mut t) in &mut roots {
        let Some(c) = host.chars.get(r.0) else { continue };
        let y = game.ground_height(c.pos[0], c.pos[2]).unwrap_or(c.pos[1]);
        let heading = c.dir[0].atan2(c.dir[2]);
        let p = Vec3::new(c.pos[0], y, c.pos[2]);
        *t = Transform::from_matrix(crate::game::display_matrix(radius, p) * Mat4::from_rotation_y(heading));
    }
    for (j, mut t) in &mut joints {
        let Some(c) = host.chars.get(j.char) else { continue };
        if let Some(x) = c.animator.pose.get(j.bone) {
            t.translation = Vec3::from(x.trans);
            t.rotation = Quat::from_xyzw(x.rot[0], x.rot[1], x.rot[2], x.rot[3]).normalize();
            t.scale = Vec3::from(x.scale);
        }
    }
}
