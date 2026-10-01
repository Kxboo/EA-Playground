//! Engine-side host for the recovered MGTetherball frame graph (`tetherball_runtime::Runtime`).
//!
//! Everything the recovered modules call "engine" lives here as plain data and is reported to the presentation layer
//! as `Out` events: characters (position, heading, `tb_anim::Animator`), AI compulsion scheduling, controllers
//! (`controller::Controller` with the shipped `controlsmgtetherball.csv`), the follow-camera state, part-fx handles,
//! database reads and the front-end calls.  Recovered game logic is never reimplemented here; this file only supplies the
//! boundaries.
//!
//! Provisional host behaviour (listed in docs/TETHERBALL_PLAY.md): character locomotion toward a move-compulsion target,
//! the AI compulsion scheduler loop around the recovered `evaluate / think / has_expired`, the camera eye placement,
//! sound azimuth, and `current_animation` being synchronised once per frame (the recovered modules read it as plain data).
use crate::{
    area_transform::{self as matrix, AreaTransform, Matrix},
    controller::Controller,
    tb_anim::{Animator, BoneXf, Graph, Library},
    tetherball::{BallMotion, Direction, ScoreWeights, Zone},
    tetherball_ai::{AiEntity, AiGeometry, Compulsion},
    tetherball_ai_hit::{HitCompulsionServices, HitCompulsionTuning},
    tetherball_ai_move::MoveInputs,
    tetherball_frontend::{FrontendServices, FrontendState},
    tetherball_gestures::GestureState,
    tetherball_hit::{HitServices, HitState},
    tetherball_hit_animation::{HitAnimations, ReadyAnimation},
    tetherball_initialize::{init_game_logic_state, InitGameLogicInputs, NativeTunables},
    tetherball_lifecycle::{IntroServices, Lifecycle, Player, Services},
    tetherball_match::{MatchRules, MatchState, WinnerUi},
    tetherball_player_init::{ExistingCharacter, PlayerInitInput, PlayerInitServices, PlayerInitState, SpawnCharacterCall},
    tetherball_rally_rules::RallyRuleState,
    tetherball_reset::{Marker, ResetEffect, ResetInputs, ResetState, RoundTuning},
    tetherball_runtime::{FrameInputs, Runtime, RuntimeHost, RuntimeState},
    tetherball_scene::{Attachment, BallScene, SceneServices, Trail},
    tetherball_serve::{ServeServices, ServeState},
    tetherball_shadow_setup::{ShadowSetupServices, ShadowViewOptions},
    tetherball_startup::{CharacterKind, Effect, Stage, StartupServices, StartupState},
    vlt::Database,
};
use std::{collections::HashMap, rc::Rc};

mod camera;
mod frame;
mod services;
pub use camera::Camera;

/// MGTetherball's object ids are engine pointers; the port hands out opaque handles in this range.
const HANDLE_BASE: u32 = 0x7100_0000;
/// Marker id whose matrix the server's ball is attached to (SetUpServer compares marker ids with 0x3f).
pub const BALL_MARKER: i32 = 0x3f;

/// What the launcher (front end / world) decides before `MGTetherball::Initialize` runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// Tetherball area (0 school, 1 stadium, 2 forest) = `InitTunablesForMultiPlayer` parameter 0.
    pub area: i32,
    /// Difficulty / game mode 0..=3 = parameter 4.
    pub difficulty: i32,
    /// Rotations to win = parameter 8 (multiplayer) or the single-player tunable.
    pub rotations: i32,
    /// Rounds = parameter 0x10.
    pub rounds: i32,
    /// Human players (1 or 2).
    pub humans: usize,
    /// Single-player session (reads `tunables` from the database instead of the multiplayer parameters).
    pub single_player: bool,
    pub female: [bool; 2],
    /// Mega ability per slot (`SetupMultiPlayerAbility` / `SetupSinglePlayerAbility`).
    pub mega: bool,
    /// Signature celebration state (`ANIM_CELEB_<KID>`) per seat, -1 for none.
    pub special: [i32; 2],
}

impl Config {
    pub fn quick(humans: usize) -> Self {
        Config { area: 0, difficulty: 1, rotations: 6, rounds: 3, humans, single_player: humans == 1, female: [false; 2], mega: true, special: [229, 229] }
    }
}

/// `ANIM_CELEB_<KID>` state for a roster asset (Alicia when unnamed), -1 for kids without one.
pub fn celebration_state(asset: Option<&str>) -> i32 {
    let name = asset.unwrap_or("alicia").to_lowercase();
    for (i, k) in ["alicia", "jazz", "josun", "kalia", "ken", "nerdy", "skater", "timothy", "stickerkid"].iter().enumerate() {
        if name.contains(k) {
            return 229 + i as i32;
        }
    }
    -1
}

/// Decoded animation data shared by both characters.
pub struct AnimAssets {
    pub male: Graph,
    pub female: Graph,
    pub lib: Library,
    pub bind: Vec<BoneXf>,
    pub skeleton: crate::skeleton::Skeleton,
}

impl AnimAssets {
    pub fn load() -> Result<AnimAssets, String> {
        let dir = crate::bridge::data_root().join("files").join("data").join("characters").to_string_lossy().replace('\\', "/");
        let male = Graph::load(&dir, false)?;
        let female = Graph::load(&dir, true)?;
        let skeleton = crate::skeleton::Skeleton::parse(&crate::archive::read_virtual(&format!("{dir}/player_anims.viv::player_skel.ske"))?.0)?;
        let bank = crate::anim::Bank::parse(crate::archive::read_virtual(&format!("{dir}/player_anims.viv::player_anims.anm"))?.0)?;
        let mut lib = Library::default();
        let wanted: Vec<usize> = [0usize, 1].into_iter().chain(56..=96).chain(225..=237).collect();
        for graph in [&male, &female] {
            for asset in graph.assets_of(wanted.iter().copied()) {
                if lib.clips.contains_key(&asset) {
                    continue;
                }
                let name = format!("S_{asset}");
                let Some(index) = bank.names.iter().position(|n| *n == name) else {
                    // Only the signature celebrations (229..) may be absent for a graph row; the tetherball states must exist.
                    if graph.states.iter().flatten().any(|i| i.assets.contains(&asset)) && wanted.iter().any(|&w| w < 229 && graph.info(w).is_some_and(|i| i.assets.contains(&asset))) {
                        return Err(format!("clip {name} missing"));
                    }
                    continue;
                };
                lib.clips.insert(asset, bank.decode(index, &skeleton)?);
            }
        }
        let bind = crate::tb_anim::bind_pose(&skeleton);
        Ok(AnimAssets { male, female, lib, bind, skeleton })
    }
    fn graph(&self, female: bool) -> &Graph {
        if female { &self.female } else { &self.male }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Hud {
    Scoreboard([i32; 4]),
    Round([i32; 3]),
    ServeBubble([i32; 3]),
    MegaVisible(i32, i32),
    MegaValue(i32, i32),
    Winner(WinnerUi, i32, bool),
    TimerVisible(i32),
    ResetScoreboard,
    Clear,
    CloseScreen,
    OpenScreen(String),
    OpenOverlay(String),
    CloseOverlay,
    PostGame(i32, Vec<u32>),
    SetupHandlers(i32),
    ClearPregameHandlers,
    ClearPostgameHandlers,
}

/// Presentation-facing events, in the order the original issued them.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Hud(Hud),
    Sound { frontend: bool, id: i32, variant: i32, volume: i32 },
    WiimoteSound { player: usize, id: i32, flags: i32 },
    Rumble { controller: i32, ms: u32, strength: f32 },
    CameraShake { ms: i32, strength: f32 },
    Particle { id: u32, name: String, position: [f32; 3] },
    ParticleMove { id: u32, position: [f32; 3] },
    ParticleScale { id: u32, scale: f32 },
    ParticleDestroy { id: u32, fade_ms: i32 },
    WrapParticle { player: usize, position: [f32; 3], fade_ms: i32 },
    PoleIndicator(f32),
    PoleVisible { handle: u32, visible: bool },
    ShadowMatrix { rope: bool, matrix: Matrix },
    ShadowViewport { mode: i32, options: ShadowViewOptions },
    Fade { kind: &'static str, ms: i32 },
    Pregame { kind: i32, argument: i32 },
    PauseInfo { kind: i32, argument: i32, words: [u32; 4] },
    AudioPause(i32),
    AudioUnpause,
    LoadAudio(i32),
    PlayMusic(i32),
    CharacterSpawned { index: usize, handle: u32 },
}


pub struct Char {
    pub handle: u32,
    pub index: usize,
    pub pos: [f32; 3],
    pub dir: [f32; 3],
    pub identity: [u32; 2],
    pub ai_entity: u32,
    /// Human controller slot, `None` for computer characters.
    pub human: Option<usize>,
    pub target: Option<[f32; 3]>,
    pub animator: Animator,
    pub female: bool,
    pub moving: bool,
}

impl Char {
    /// Character world matrix (translation + heading about Y) as the engine's row-major `Matrix` (row vectors).
    pub fn world(&self) -> Matrix {
        let (x, z) = (self.dir[0], self.dir[2]);
        let l = (x * x + z * z).sqrt().max(1e-6);
        let (s, c) = (x / l, z / l);
        // Rotation about Y taking +Z to `dir`.
        [c, 0., -s, 0., 0., 1., 0., 0., s, 0., c, 0., self.pos[0], self.pos[1], self.pos[2], 1.]
    }
}

enum Active {
    Move(crate::tetherball_ai_move::MoveCompulsion),
    Hit(crate::tetherball_ai_hit::TetherballHitCompulsion),
}

pub struct TbHost {
    pub db: Rc<Database>,
    pub cfg: Config,
    pub assets: Rc<AnimAssets>,
    pub out: Vec<Out>,
    pub chars: Vec<Char>,
    pub controllers: Vec<Controller>,
    pub camera: Camera,
    pub eye_target: ([f32; 3], [f32; 3]),
    pub ball_resources: Option<crate::tetherball_ball_init::BallResources>,
    pub pole_height: f32,
    pub origin: [f32; 3],
    pub inverse_world: Matrix,
    pub ground: Box<dyn Fn([f32; 3]) -> f32>,
    pub area: AreaTransform,
    pub trails: HashMap<u32, (Trail, [f32; 3])>,
    pub fx: HashMap<u32, [f32; 3]>,
    pub ball_matrix: Matrix,
    pub rope_matrix: Matrix,
    pub pole_offset: f32,
    pub time_ms: u64,
    pub fade_left_ms: i32,
    pub fade_pending: bool,
    pub paused_audio: bool,
    pub shake_ms: i32,
    pub grab: Option<usize>,
    pub generation: u32,
    /// `Lifecycle::players[i].direction` as last applied to the character (the lifecycle turns winners at round end).
    pub applied_dir: [[f32; 3]; 2],
    player_init: PlayerInitState,
    next_handle: u32,
    rng: u64,
    keys: HashMap<u64, String>,
    collections: HashMap<u32, (String, String)>,
    active: [Option<Active>; 2],
    hit_tuning: Option<HitCompulsionTuning>,
    ability: bool,
    quit: bool,
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

impl TbHost {
    pub fn new(db: Rc<Database>, assets: Rc<AnimAssets>, cfg: Config, seed: u64) -> Result<TbHost, String> {
        let dir = crate::bridge::data_root().join("files/data/csvs.viv");
        let mut bindings = vec![];
        for name in ["controls.csv", "controlsmgtetherball.csv"] {
            let bytes = crate::archive::read_virtual(&format!("{}::{name}", dir.to_string_lossy().replace('\\', "/")))?.0;
            bindings.extend(crate::control_bindings::parse(&bytes)?);
        }
        let controllers = (0..2).map(|i| { let mut c = Controller::new(bindings.clone(), 2); c.pad_index = i; c }).collect();
        Ok(TbHost {
            db,
            cfg,
            assets,
            out: vec![],
            chars: vec![],
            controllers,
            camera: Camera::default(),
            eye_target: ([0.; 3], [0.; 3]),
            ball_resources: None,
            pole_height: 2.0,
            origin: [0.; 3],
            inverse_world: matrix::IDENTITY,
            ground: Box::new(|p| p[1]),
            area: AreaTransform { radius: 0., disabled: true },
            trails: HashMap::new(),
            fx: HashMap::new(),
            ball_matrix: matrix::IDENTITY,
            rope_matrix: matrix::IDENTITY,
            pole_offset: 0.,
            time_ms: 0,
            fade_left_ms: 0,
            fade_pending: false,
            paused_audio: false,
            shake_ms: 0,
            grab: None,
            generation: 0,
            applied_dir: [[0., 0., 1.]; 2],
            player_init: PlayerInitState::new([0; 2]),
            next_handle: HANDLE_BASE,
            rng: seed | 1,
            keys: HashMap::new(),
            collections: HashMap::new(),
            active: [None, None],
            hit_tuning: None,
            ability: false,
            quit: false,
        })
    }

    fn handle(&mut self) -> u32 {
        self.next_handle += 0x1000;
        self.next_handle
    }

    fn rand(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        let span = (high as i64 - low as i64 + 1) as u64;
        low + (xorshift(&mut self.rng) % span) as i32
    }

    fn char_index(&self, handle: u32) -> Option<usize> {
        self.chars.iter().position(|c| c.handle == handle)
    }

    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// A fresh `MGTetherball` as the constructor (0x80396410) leaves it, plus the base `Minigame` fields that matter.
    pub fn new_runtime(&self) -> Runtime {
        let player = Player {
            controller: None,
            special_win_animation: -1,
            current_animation: 0,
            player_flag: false,
            facing: 0.,
            direction: [0., 0., 1.],
            movement_speed: 1.2,
            ai_distance: -1,
        };
        let ready = ReadyAnimation { state: [0; 2], suppress_if_current: [0; 2] };
        let life = Lifecycle {
            match_state: MatchState {
                rotations: [0; 2],
                round_wins: [0; 2],
                elapsed_ms: 0,
                state_ms: 0,
                previous_state_ms: 0,
                state_code: 0,
                round_winner: -1,
                match_winner: -1,
                match_over: false,
                result: 2,
                final_result: 2,
            },
            paused: false,
            session_mode: 0,
            game_type: -1,
            variant: self.cfg.area,
            player_count: 0,
            server: 0,
            receiver: 0,
            focus_player: 0,
            players: [Player { special_win_animation: self.cfg.special[0], ..player.clone() }, Player { special_win_animation: self.cfg.special[1], ..player }],
            round_number: 0,
            total_rounds: 0,
            scoreboard_visible: false,
            round_visible: false,
            round_timer_ms: 0,
            latches: [false; 2],
            action_states: [2; 2],
            mega_values: [0; 2],
            mega_states: [1; 2],
            mega_enabled: [false; 2],
            field_32f: false,
            serve_bubble_visible: false,
            hud_ready: false,
            statistics: [[0; 5]; 2],
            score_weights: [0; 3],
            postgame_win_flag: false,
            distance_mode: 0,
            current_distance: 0,
            pole_position: [0.; 3],
            indicator_target: 0.,
            indicator_current: 0.,
            win_animations: [0; 2],
            lose_animations: [0; 2],
            winner_turns: [false; 2],
            winner_base_angle: 0.,
            active_tetherball_variant: Some(self.cfg.area),
        };
        let mut reset = ResetState::blank();
        // Constructor stores (0x80396410..): +0x43c = -1, +0x440 = -1 (match winner), counters zero.
        reset.word_224 = -1;
        reset.field_25c = false;
        reset.field_229 = false;
        reset.counter_260 = 0;
        reset.counter_264 = 0;
        reset.counter_268 = 0;
        reset.timer_26c = 0.;
        reset.counter_270 = 0;
        reset.pending_count_2e4 = 0;
        reset.field_35c = 0.;
        reset.initial_rotation_178 = 0;
        reset.rotation_limit_430 = 6;
        reset.base_player_count_070 = self.cfg.humans as i32;
        // The base class tells MGTetherball how many humans took part: one human + computer, or two humans.
        reset.ai_special_case_0c0 = if self.cfg.humans >= 2 { 3 } else { 6 };
        reset.game_mode_044 = self.cfg.difficulty;
        let ball = BallMotion {
            angle: 0.,
            hit_angle: 0.,
            secondary_angle: 0.,
            target_velocity: 0.,
            secondary_target_velocity: 0.,
            spin_acceleration: 0.,
            hit_direction: Direction::Zero,
            grabbed: false,
            vertical_velocity: 0.,
            toss_time: 0,
            angular_velocity: 0.,
            acceleration: 0.,
            secondary_acceleration: 0.,
            secondary_velocity: 0.,
            hit_type: 0,
            direction: Direction::Zero,
            zone: Zone::Zero,
            radius: 0.7,
            desired_radius: 0.7,
            height: 0.,
            target_height: 0.,
            base_hit_speed: 0.,
            power_modifier: 0.,
            mega_modifier: 0.,
            pole_height: 0.,
            tossed: false,
            spinning_up: false,
            spinning_down: false,
        };
        Runtime {
            life,
            ball,
            state: RuntimeState {
                reset,
                serve: ServeState {
                    pause_block_count_0fc: 0,
                    pause_menu_open: false,
                    power_serve_enabled: false,
                    return_angles: [0.; 2],
                    power_animations: [0; 2],
                    high_animations: [0; 2],
                    voice_types: [0; 2],
                    ai_waiting: [false; 2],
                    forced_ai: [false; 2],
                    frontend_flags: [false; 2],
                },
                gestures: GestureState::default(),
                rally: RallyRuleState { ai_hit_attempt_234: 0, ai_power_hit_type_238: 0, ai_charge: [0; 2] },
                hit: HitState {
                    field_32c: false,
                    field_32d: false,
                    mega_ability_42e: false,
                    pending_zone_274: 0,
                    power_hit_type_43c: -1,
                    hit_multiplier_428: 1.,
                    hit_multiplier_enabled_42d: false,
                    indicator_scale_338: 0.,
                    indicator_rate_33c: 0.,
                    indicator_angles_360_36c_378_384: [[0.; 3]; 4],
                },
                animations: HitAnimations {
                    ready_power: ready.clone(),
                    ready_reverse: ready.clone(),
                    ready_zone_zero: ready.clone(),
                    ready_zone_one: ready,
                    hit_power: [0; 2],
                    hit_zone_zero: [0; 2],
                },
                scene: BallScene {
                    anchor: [0.; 3],
                    position: [0.; 3],
                    ball_matrix: matrix::IDENTITY,
                    rope_matrix: matrix::IDENTITY,
                    trails: [u32::MAX; 3],
                    null_trail: u32::MAX,
                    ball_shadow: false,
                    rope_shadow: false,
                },
                ai: [None, None],
                rules: MatchRules { mode: 0, rotation_limit: 6, wins_required: 0, time_limit_seconds: 0 },
                frontend: FrontendState { pregame_ready_058: 0, postgame_choice_05c: -1, field_424: 0 },
                fx_names: std::array::from_fn(|_| std::array::from_fn(|_| String::new())),
            },
        }
    }

    pub fn new_startup_state(&self) -> StartupState {
        StartupState {
            world_services: [0; 6],
            tag_02c: 0,
            tag_030: 0,
            asset_handle_100: 0,
            alternate_pole_188: 0,
            mode_2ec: 0,
            pole_glow_340: 0,
            inverse_world_3d8: matrix::IDENTITY,
            identities_078_0b8: [[0x7000_0001, 0], [0x7000_0002, 0]],
        }
    }

    // ---- database ----------------------------------------------------------------------------------------------

    fn collection_value(&self, class: &str, name: &str, field: &str, index: usize) -> Option<serde_json::Value> {
        let c = self.db.find_collection(class, name)?;
        match crate::tetherball_tuning::inherited_attribute(&self.db, c, field).ok()?? {
            serde_json::Value::Array(a) => a.get(index).cloned(),
            v if index == 0 => Some(v),
            _ => None,
        }
    }
    fn db_float(&self, class: &str, name: &str, field: &str, index: usize) -> f32 {
        self.collection_value(class, name, field, index).and_then(|v| v.as_f64()).unwrap_or(0.) as f32
    }
    fn db_int(&self, class: &str, name: &str, field: &str, index: usize) -> i32 {
        self.collection_value(class, name, field, index).and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|x| x as i64))).unwrap_or(0) as i32
    }

    /// The `tunables` / dare collection InitTunablesForSinglePlayer reads (`GetTunablesCollectionName`).
    fn single_player_fields(&self, life: &Lifecycle) -> [i32; 6] {
        let name = crate::tetherball_tuning::collection_name(life.session_mode, life.game_type).unwrap_or("tunables");
        let index = crate::tetherball_tuning::difficulty_index(self.cfg.difficulty);
        std::array::from_fn(|i| self.db_int("mg_tetherball", name, crate::tetherball_tuning::SINGLE_FIELDS[i], index))
    }

    fn score_weights(&self) -> [i32; 3] {
        let index = crate::tetherball_tuning::difficulty_index(self.cfg.difficulty);
        [
            self.db_int("mg_tetherball", "scoring", "accuracy_points", index),
            self.db_int("mg_tetherball", "scoring", "powerhit_points", index),
            self.db_int("mg_tetherball", "scoring", "megahit_points", index),
        ]
    }

    // ---- stage bodies --------------------------------------------------------------------------------------------

    fn stage_impl(&mut self, stage: Stage, rt: &mut Runtime, st: &mut StartupState) -> Result<(), String> {
        match stage {
            Stage::BaseInitialize { .. } => {
                st.world_services = [1, 2, 3, 4, 5, 6];
                for p in 0..2 {
                    rt.life.players[p].controller = (p < self.cfg.humans).then_some(p as i32);
                }
                Ok(())
            }
            Stage::InitGameLogic => {
                let tunables = if self.cfg.single_player {
                    NativeTunables::SinglePlayer { fields: self.single_player_fields(&rt.life) }
                } else {
                    NativeTunables::MultiPlayer {
                        variant: self.cfg.area,
                        game_mode: self.cfg.difficulty,
                        rounds: self.cfg.rounds,
                        rotations_to_win: self.cfg.rotations,
                    }
                };
                let scores = self.score_weights();
                let _ = init_game_logic_state(
                    &mut rt.life,
                    &mut rt.state.reset,
                    &mut rt.state.rules,
                    InitGameLogicInputs { tunables, score_weights: scores },
                );
                Ok(())
            }
            Stage::ConstructBall { .. } => Ok(()),
            Stage::SetArea(_) => {
                rt.state.reset.camera_heading_394 = 0.;
                Ok(())
            }
            Stage::Character { kind, input } => {
                self.ability = self.cfg.mega;
                let (life, reset) = (&mut rt.life, &mut rt.state.reset);
                let mut flags = std::mem::replace(&mut self.player_init, PlayerInitState::new([0; 2]));
                match kind {
                    CharacterKind::Player => crate::tetherball_player_init::initialize_player(life, reset, &mut flags, input, self),
                    CharacterKind::Ai => crate::tetherball_ai_init::initialize_ai(life, reset, &mut flags, input, self),
                    CharacterKind::AdditionalPlayer => {
                        crate::tetherball_additional_player::initialize_additional_player(life, reset, &mut flags, input, self)
                    }
                }
                self.player_init = flags;
                let slot = rt.life.player_count.saturating_sub(1);
                rt.state.hit.mega_ability_42e = self.cfg.mega;
                rt.life.mega_enabled[slot] = self.cfg.mega;
                Ok(())
            }
            Stage::Ai { player, enabled, difficulty, heading } => {
                let handle = rt.state.reset.current_ai_entities_128[player];
                let mut ai = AiEntity::new(handle, player, 0.);
                ai.ball_handle = rt.state.reset.ball_handle_104;
                ai.initialize(Some((rt.life.session_mode, rt.life.game_type)), enabled, difficulty, heading, &self.db)?;
                rt.state.ai[player] = Some(ai);
                Ok(())
            }
            Stage::Ball { difficulty, anchor, heading, .. } => {
                let input = crate::tetherball_ball_init::BallInitInput { difficulty, anchor, heading, pole_height: self.origin[1] };
                let resources = crate::tetherball_ball_init::initialize_ball(&mut rt.ball, &mut rt.state.scene, input, self);
                self.ball_resources = Some(resources);
                Ok(())
            }
            Stage::OpenPregame(kind) => {
                self.out.push(Out::Pregame { kind, argument: 0 });
                Ok(())
            }
            Stage::SetUpServer => {
                let input = self.build_reset_inputs(&rt.life, &rt.state.reset, true);
                let effects = crate::tetherball_reset::setup_server(&mut rt.life, &mut rt.state.reset, &mut rt.ball, &input);
                for e in effects {
                    self.apply_effect(e);
                }
                Ok(())
            }
        }
    }
}

impl TbHost {
    /// `MGTetherball::Initialize`: build the object and run the recovered startup against this host.
    pub fn start(&mut self) -> Result<(Runtime, StartupState), String> {
        let mut rt = self.new_runtime();
        let mut st = self.new_startup_state();
        // The placeable's height (the pole model's top) feeds `Tetherball::Initialize`.
        let input = crate::tetherball_startup::StartupInput {
            world: 1,
            camera_position_offset: [0., 1.075, 0.],
            camera_target_offset: [0., 0., 0.],
        };
        crate::tetherball_startup::initialize(&mut rt, &mut st, input, self)?;
        self.inverse_world = st.inverse_world_3d8;
        Ok((rt, st))
    }

    /// The front end finished the instruction pages (`pregame_ready` is the flag `UpdatePreGameInstructions` polls).
    pub fn pregame_done(&self, rt: &mut Runtime) {
        rt.state.frontend.pregame_ready_058 = 1;
    }

    /// `OnHudLoadComplete` / `OnGameStartAnimComplete`, called when the HUD movie reports them.
    pub fn hud_loaded(&mut self, rt: &mut Runtime) {
        crate::tetherball_frontend::on_hud_load_complete(&mut rt.life, self);
    }
    pub fn start_anim_complete(&self, rt: &mut Runtime) {
        crate::tetherball_frontend::on_game_start_anim_complete(&mut rt.life);
    }
}


impl TbHost {
    /// `MGTetherball::OnPauseReset`: full reset, intro again, then the base `ClosePauseMenu`.
    pub fn restart(&mut self, rt: &mut Runtime) -> Result<(), String> {
        let db = self.db.clone();
        rt.reset(true, &db, self)?;
        let rules = rt.state.rules;
        rt.life.change_state(3, rules, &mut rt.ball, self);
        rt.life.paused = false;
        self.out.push(Out::Hud(Hud::CloseOverlay));
        self.out.push(Out::AudioUnpause);
        rt.state.serve.pause_menu_open = false;
        rt.state.serve.pause_block_count_0fc = 1200;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
