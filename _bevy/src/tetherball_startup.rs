//! Enclosing MGTetherball::Initialize (0x803966c4), composed with recovered
//! gameplay helpers. Only synchronous engine/data operations remain services.
use crate::area_transform::{self as matrix, Matrix};
use crate::tetherball_angles::wrap_angle;
use crate::tetherball_player_init::PlayerInitInput;
use crate::tetherball_runtime::Runtime;

#[derive(Clone, Debug)]
pub struct StartupState {
    /// Base Minigame pointers copied from World +8,+c,+10,+14,+18,+20.
    pub world_services: [u32; 6],
    /// Tunable words +170/+180, separate from live round/cap words +438/+430.
    pub tunables_170_180: [i32; 2],
    pub constructor_metadata: Option<crate::tetherball_constructor::ConstructorMetadata>,
    /// Base +4c is preserved; +4d is set by Initialize and cleared by UnInitialize.
    pub base_flags_04c_04d: [u8; 2],
    pub base_padding_04f: u8,
    pub player_init: crate::tetherball_player_init::PlayerInitState,
    pub ball_resources: Option<crate::tetherball_ball_init::BallResources>,
    pub tag_02c: u32,
    pub tag_030: u32,
    pub asset_handle_100: u32,
    pub alternate_pole_188: u32,
    pub mode_2ec: u32,
    pub pole_glow_340: u32,
    pub inverse_world_3d8: Matrix,
    pub identities_078_0b8: [[u32; 2]; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct StartupInput {
    pub world: u32,
    /// Static vectors at 0x805e3750 / 0x805e37b0, initialized outside this body.
    pub camera_position_offset: [f32; 3],
    pub camera_target_offset: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterKind {
    Player,
    Ai,
    AdditionalPlayer,
}

#[derive(Clone, Debug)]
pub enum Effect {
    /// Native AreaManager +2c write, followed by PlaceableManager SetCurrentArea.
    MinigameArea {
        abstract_area: i32,
        physical_area: i32,
    },
    CameraViewInfo {
        camera: u32,
        flag: bool,
    },
    CameraReinitialize {
        manager: u32,
        camera_type: u32,
        player: u32,
    },
    ControllerReinitialize {
        controller: u32,
        control_type: u32,
    },
    HomeMenu(bool),
    ResetHomeIcon,
    SyncTask(bool),
    VSync(bool),
    AncientEvilByte(u8),
    PlaceableVisible {
        handle: u32,
        visible: bool,
    },
    CameraOffset {
        camera: u32,
        target: bool,
        desired_ms: Option<u32>,
        value: [f32; 3],
    },
    CameraBackwards {
        camera: u32,
        value: f32,
    },
    CameraScalars {
        camera: u32,
        height_39c: f32,
        value_3a0: f32,
    },
    CameraRotation {
        camera: u32,
        value: f32,
        milliseconds: u32,
    },
    CameraStart {
        camera: u32,
        value: [f32; 3],
    },
    CameraDirection {
        camera: u32,
        value: [f32; 3],
    },
    SwitchToAi(u32),
    RegisterCallback {
        name: &'static str,
        address: u32,
    },
    LoadAudio(i32),
    PlayMusic(i32),
    RendererWord1c0(u32),
}

/// Synchronous original engine/data boundaries. Helpers apply all gameplay
/// stores to their existing owners. An error leaves a partially initialized
/// object; callers must discard it, rather than start a match with that state.
pub trait StartupServices:
    crate::tetherball_lifecycle::Services + crate::tetherball_shadow_setup::ShadowSetupServices
{
    fn effect(&mut self, effect: Effect);
    fn player_services(&mut self) -> &mut impl crate::tetherball_player_init::PlayerInitServices;
    fn ball_services(&mut self) -> &mut impl crate::tetherball_ball_init::BallInitServices;
    fn pregame_services(&mut self) -> &mut impl crate::minigame_entry::PregameServices;
    fn server_services(&mut self) -> &mut impl crate::tetherball_server::ServerServices;
    fn ai_services(&mut self) -> &mut impl crate::tetherball_ai::AiInitServices;
    /// Read the constructor-preserved AI +68 allocation word; no gameplay writes.
    fn ai_allocation_direction_scale(&mut self, entity: u32) -> f32;
    /// WorldMan::GetTetherballMinigame lookup and referenced live game fields.
    /// This read-only query must not infer presence from the selected area.
    fn ai_game_context(&mut self, runtime: &Runtime) -> Option<(i32, i32)>;
    /// Decoded VLT/ResetStats results only; gameplay stores are applied here.
    fn game_logic_inputs(
        &mut self,
        runtime: &Runtime,
    ) -> Result<crate::tetherball_initialize::InitGameLogicInputs, String>;
    /// Particle-system null trail token at 0x80601f78.
    fn null_ball_trail(&mut self) -> u32;
    fn world_service_handles(&mut self, world: u32) -> Result<[u32; 6], String>;
    fn camera_manager(&mut self) -> u32;
    fn current_camera(&mut self, manager: u32) -> u32;
    fn controller_handle(&mut self, index: i32) -> u32;
    fn allocate_ball(&mut self, bytes: u32, tag: &str) -> u32;
    fn placeable(&mut self, name: &str) -> (u32, [f32; 3]);
    fn camera(&mut self, player: u32) -> u32;
    fn load_bigfile(&mut self, name: &str, load: bool, pool: u32) -> u32;
    fn ground_height(&mut self, position: [f32; 3], range: f32) -> f32;
    fn create_particle(&mut self, name: &str, position: [f32; 3]) -> u32;
    fn database_key(&mut self, name: &str) -> u64;
    fn collection(&mut self, class: u64, name: u64) -> u32;
    fn float_array(&mut self, collection: u32, field: &str, index: u32) -> f32;
    fn int16_array(&mut self, collection: u32, field: &str, index: u32) -> i16;
    fn destroy_collection(&mut self, collection: u32);
}

/// Minigame::Initialize(World*) (0x803ab430), with the Minigame virtual
/// Initialize() body (0x803ab470) used by MGTetherball. Camera/controller
/// reinitialization are synchronous engine effects, in original order.
pub fn initialize_base(
    runtime: &mut Runtime,
    state: &mut StartupState,
    world: u32,
    host: &mut impl StartupServices,
) -> Result<(), String> {
    state.world_services = host.world_service_handles(world)?;
    let manager = host.camera_manager();
    let camera = host.current_camera(manager);
    host.effect(Effect::CameraViewInfo {
        camera,
        flag: false,
    });
    // The original reloads the singleton after GetCameraViewInfo.
    let manager = host.camera_manager();
    host.effect(Effect::CameraReinitialize {
        manager,
        camera_type: state.tag_02c,
        player: 0,
    });
    for index in 0..4 {
        let controller = host.controller_handle(index);
        host.effect(Effect::ControllerReinitialize {
            controller,
            control_type: state.tag_030,
        });
    }
    state.base_flags_04c_04d[1] = 1;
    runtime.state.serve.pause_menu_open = false;
    Ok(())
}

/// Minigame::UnInitialize (0x803ab524), the matching base cleanup body.
/// Activity-owned asset/entity destruction remains the derived host's job.
pub fn uninitialize_base(
    runtime: &mut Runtime,
    state: &mut StartupState,
    host: &mut impl StartupServices,
) {
    for index in 0..4 {
        let controller = host.controller_handle(index);
        host.effect(Effect::ControllerReinitialize {
            controller,
            control_type: 0,
        });
    }
    runtime.state.serve.pause_menu_open = false;
    state.base_flags_04c_04d[1] = 0;
}

fn direction(angle: f32) -> [f32; 3] {
    let (s, c) = crate::character_input::ea_sin_cos(wrap_angle(angle));
    [s, 0., c]
}

/// Matrix44InverseRotTrans, preserving its fmsubs/fnmsubs rounding and zeros.
fn inverse_rot_trans(m: &Matrix) -> Matrix {
    let mut result = matrix::IDENTITY;
    for row in 0..3 {
        for col in 0..3 {
            result[row * 4 + col] = m[col * 4 + row];
        }
    }
    for i in 0..3 {
        let partial = (-m[12]).mul_add(m[i * 4], -(m[13] * m[i * 4 + 1]));
        result[12 + i] = -(m[14].mul_add(m[i * 4 + 2], -partial));
    }
    result
}

/// Shared character append path; gameplay stores are owned by the recovered
/// helpers rather than supplied as host stage writes.
pub fn initialize_character(
    runtime: &mut Runtime,
    state: &mut StartupState,
    kind: CharacterKind,
    input: PlayerInitInput,
    services: &mut impl crate::tetherball_player_init::PlayerInitServices,
) {
    match kind {
        CharacterKind::Player => crate::tetherball_player_init::initialize_player(
            &mut runtime.life,
            &mut runtime.state.reset,
            &mut state.player_init,
            input,
            services,
        ),
        CharacterKind::Ai => crate::tetherball_ai_init::initialize_ai(
            &mut runtime.life,
            &mut runtime.state.reset,
            &mut state.player_init,
            input,
            services,
        ),
        CharacterKind::AdditionalPlayer => {
            crate::tetherball_additional_player::initialize_additional_player(
                &mut runtime.life,
                &mut runtime.state.reset,
                &mut state.player_init,
                input,
                services,
            )
        }
    }
}

/// Project the just-constructed character AI into its live owner, then run
/// native tuning setup. Engine services may not reenter gameplay during append.
pub fn initialize_startup_ai(
    runtime: &mut Runtime,
    player: usize,
    enabled: bool,
    difficulty: u32,
    heading: f32,
    host: &mut impl StartupServices,
) -> Result<(), String> {
    if player >= 2 || runtime.life.player_count != player + 1 {
        return Err("startup AI requires its appended character".into());
    }
    let handle = runtime.state.reset.current_ai_entities_128[player];
    if handle == 0 {
        return Err("native AI allocation failure has no successful startup path".into());
    }
    let mut ai = crate::tetherball_ai::AiEntity::new(
        handle,
        player,
        host.ai_allocation_direction_scale(handle),
    );
    ai.ball_handle = runtime.state.reset.ball_handle_104;
    runtime.state.ai[player] = Some(ai);
    runtime.state.rally.ai_charge[player] = 0;
    let game = host.ai_game_context(runtime);
    runtime.state.ai[player]
        .as_mut()
        .unwrap()
        .initialize_with_services(game, enabled, difficulty, heading, host.ai_services())
}

/// Complete enclosing startup for supported areas 0..2, game modes 0..3,
/// and native participant configurations 3/6. Invalid domains have no recovered
/// successful native startup path and return errors instead of guessing defaults.
pub fn initialize(
    runtime: &mut Runtime,
    state: &mut StartupState,
    input: StartupInput,
    host: &mut impl StartupServices,
) -> Result<(), String> {
    host.effect(Effect::HomeMenu(false));
    host.effect(Effect::ResetHomeIcon);
    host.effect(Effect::SyncTask(true));
    host.effect(Effect::VSync(false));
    state.tag_030 = 11;
    state.tag_02c = 7;
    host.effect(Effect::AncientEvilByte(0));
    runtime.life.current_distance = 2;
    initialize_base(runtime, state, input.world, host)?;
    let inputs = host.game_logic_inputs(runtime)?;
    state.tunables_170_180 = match inputs.tunables {
        crate::tetherball_initialize::NativeTunables::SinglePlayer { fields } => {
            [fields[1], fields[5]]
        }
        crate::tetherball_initialize::NativeTunables::MultiPlayer {
            rounds,
            rotations_to_win,
            ..
        } => [rounds, rotations_to_win],
    };
    // InitGameLogicState (0x8039ceac) consumes the prior constructor cap.
    crate::tetherball_initialize::init_game_logic_state(
        &mut runtime.life,
        &mut runtime.state.reset,
        &mut runtime.state.rules,
        inputs,
    );
    let ball = host.allocate_ball(0x170, "Minigame::Tetherball");
    if ball != 0 {
        state.ball_resources = Some(crate::tetherball_ball_init::construct_ball(
            &mut runtime.ball,
            &mut runtime.state.scene,
            host.null_ball_trail(),
        ));
    }
    runtime.state.reset.ball_handle_104 = ball;
    runtime.state.reset.ai_initial_values_27c = [1, -1];
    let variant = runtime.life.variant;
    let names = match variant {
        0 => ["tetherball_pole_school", "tetherball_pole_school_w_ball"],
        1 => ["tetherball_pole_stadium", "tetherball_pole_stadium_w_ball"],
        2 => ["tetherball_pole_forest", "tetherball_pole_forest_w_ball"],
        _ => return Err(format!("unsupported native tetherball area {variant}")),
    };
    let (pole, origin) = host.placeable(names[0]);
    runtime.state.reset.pole_handle_184 = pole;
    state.alternate_pole_188 = host.placeable(names[1]).0;
    runtime.state.reset.camera_heading_394 = 0.;
    runtime.state.reset.world_position_110 = origin;
    // Lifecycle's pole position represents the external placeable, not +110.
    runtime.life.pole_position = origin;
    host.effect(Effect::MinigameArea {
        abstract_area: variant,
        physical_area: crate::minigame_entry::physical_area(variant),
    });
    host.effect(Effect::PlaceableVisible {
        handle: pole,
        visible: true,
    });
    host.effect(Effect::PlaceableVisible {
        handle: state.alternate_pole_188,
        visible: false,
    });
    let heading = runtime.state.reset.camera_heading_394;
    let (s, c) = crate::character_input::ea_sin_cos(heading);
    let rotation = [c, 0., -s, 0., 0., 1., 0., 0., s, 0., c, 0., 0., 0., 0., 1.];
    runtime.state.reset.world_matrix_398 =
        matrix::multiply(&rotation, &matrix::translation(origin));
    state.inverse_world_3d8 = inverse_rot_trans(&runtime.state.reset.world_matrix_398);
    let transform = runtime.state.reset.world_matrix_398;
    let separation = [1.25, 1.45, 1.65][runtime.life.current_distance as usize];
    let positions = [
        crate::tetherball_ai::transform_point([0. + separation, 0., 0.], &transform),
        crate::tetherball_ai::transform_point([0. - separation, 0., 0.], &transform),
    ];
    let first_heading = wrap_angle(f32::from_bits(0x4096cbe4) + heading);
    let camera = host.camera(0);
    runtime.state.reset.camera_handle_390 = camera;
    runtime.state.fx_names = [
        [
            "pg_tetherball_normimpact_plr1".into(),
            "pg_tetherball_powerimpact_plr1".into(),
        ],
        [
            "pg_tetherball_normimpact_plr2".into(),
            "pg_tetherball_powerimpact_plr2".into(),
        ],
    ];
    let mode = runtime.state.reset.game_mode_044;
    if !(0..=3).contains(&mode) {
        return Err(format!("unsupported native game mode {mode}"));
    }
    let difficulty = mode as u32;
    state.mode_2ec = 1;
    initialize_character(
        runtime,
        state,
        CharacterKind::Player,
        PlayerInitInput {
            position: positions[0],
            heading: first_heading,
            identity_words: state.identities_078_0b8[0],
        },
        host.player_services(),
    );
    initialize_startup_ai(
        runtime,
        0,
        runtime.state.serve.forced_ai[0],
        1,
        wrap_angle(heading),
        host,
    )?;
    let second_kind = match runtime.state.reset.ai_special_case_0c0 {
        6 => CharacterKind::Ai,
        3 => CharacterKind::AdditionalPlayer,
        v => return Err(format!("unsupported native participant configuration {v}")),
    };
    initialize_character(
        runtime,
        state,
        second_kind,
        PlayerInitInput {
            position: positions[1],
            heading: std::f32::consts::PI + first_heading,
            identity_words: state.identities_078_0b8[1],
        },
        host.player_services(),
    );
    initialize_startup_ai(
        runtime,
        1,
        second_kind == CharacterKind::Ai || runtime.state.serve.forced_ai[1],
        difficulty,
        wrap_angle(heading),
        host,
    )?;
    let camera_start = crate::tetherball_ai::transform_point([0.5, 0., 0.], &transform);
    runtime.life.set_player_distance(2, &mut runtime.ball, host);
    for (target, value) in [
        (false, input.camera_position_offset),
        (true, input.camera_target_offset),
    ] {
        host.effect(Effect::CameraOffset {
            camera,
            target,
            desired_ms: None,
            value,
        });
        host.effect(Effect::CameraOffset {
            camera,
            target,
            desired_ms: Some(600),
            value,
        });
    }
    host.effect(Effect::CameraBackwards {
        camera,
        value: [4.5, 4.75, 5.][runtime.life.current_distance as usize],
    });
    host.effect(Effect::CameraScalars {
        camera,
        height_39c: 2.2,
        value_3a0: 0.,
    });
    let camera_angle = heading + 5.47;
    host.effect(Effect::CameraRotation {
        camera,
        value: camera_angle,
        milliseconds: 600,
    });
    host.effect(Effect::CameraStart {
        camera,
        value: camera_start,
    });
    host.effect(Effect::CameraDirection {
        camera,
        value: direction(camera_angle),
    });
    state.asset_handle_100 =
        host.load_bigfile("data/minigames/tetherball/mgtetherball.viv", true, 0);
    crate::tetherball_animation_init::initialize_player_animations(
        &mut runtime.life,
        &runtime.state.reset,
        &mut runtime.state.serve,
        &mut runtime.state.animations,
    );
    let mut anchor = crate::tetherball_ai::transform_point([0.; 3], &transform);
    anchor[1] = 2.2 + host.ground_height(anchor, 50.);
    state.ball_resources = Some(crate::tetherball_ball_init::initialize_from_placeable(
        &mut runtime.ball,
        &mut runtime.state.scene,
        crate::tetherball_ball_init::BallPlacementInput {
            difficulty,
            anchor,
            heading,
            pole,
        },
        host.ball_services(),
    ));
    runtime
        .ball
        .set_radius([0.7, 0.9, 1.1][runtime.life.current_distance as usize]);
    runtime.state.serve.return_angles = [
        wrap_angle(heading),
        wrap_angle(wrap_angle(heading) + std::f32::consts::PI),
    ];
    runtime.state.reset.server_side_flags[0] = true;
    runtime.state.reset.server_side_flags[1] = false;
    runtime.state.reset.start_angles_248 = [
        wrap_angle(std::f32::consts::PI + first_heading),
        wrap_angle(first_heading),
    ];
    for player in 0..2 {
        let ai = runtime.state.ai[player]
            .as_mut()
            .ok_or("startup requires two initialized AI entities")?;
        ai.angle = wrap_angle(runtime.state.reset.start_angles_248[player]);
        ai.direction_scale = runtime.state.reset.ai_initial_values_27c[player] as f32;
        runtime.life.players[player].ai_distance = runtime.life.current_distance;
    }
    host.effect(Effect::SwitchToAi(
        runtime.state.reset.player_handles_120[0],
    ));
    host.effect(Effect::SwitchToAi(
        runtime.state.reset.player_handles_120[1],
    ));
    for (name, address) in [
        ("TB_RegularStrike", 0x8039c884),
        ("TB_RegularStrikeReverse", 0x8039c970),
        ("TB_OverhandStrike", 0x8039c9fc),
        ("TB_ServeToss", 0x8039ca88),
    ] {
        host.effect(Effect::RegisterCallback { name, address });
    }
    let mut glow = crate::tetherball_ai::transform_point([0.; 3], &transform);
    glow[1] += if runtime.life.variant == 2 { 1.27 } else { 1.1 };
    state.pole_glow_340 = host.create_particle("pg_tetherball_poleglow", glow);
    let class = host.database_key("mg_tetherball");
    let name = host.database_key("tunables");
    let collection = host.collection(class, name);
    for (field, slot) in [
        ("ball_basehitspeed", 0),
        ("ball_acceleratemodifier", 3),
        ("ball_powermodifier", 1),
        ("ball_megamodifier", 2),
    ] {
        runtime.state.reset.round_tunables_34c_358[slot] =
            host.float_array(collection, field, difficulty);
    }
    for zone in 0..3 {
        for (table, field) in crate::tetherball_tuning::ANGLE_FIELDS.iter().enumerate() {
            runtime.state.hit.indicator_angles_360_36c_378_384[table][zone] =
                f32::from_bits(0x3c8efa35)
                    * host.int16_array(collection, field, zone as u32) as f32;
        }
    }
    host.destroy_collection(collection);
    host.effect(Effect::LoadAudio(8));
    host.effect(Effect::PlayMusic(2));
    host.effect(Effect::RendererWord1c0(1));
    crate::tetherball_shadow_setup::setup_shadow_options(&runtime.state.reset, host);
    runtime.life.round_number = 1;
    crate::minigame_entry::open_pregame(
        &mut runtime.state.serve.frontend_flags,
        runtime.state.reset.base_player_count_070,
        runtime.life.game_type,
        2,
        host.pregame_services(),
    );
    runtime
        .life
        .change_state(1, runtime.state.rules, &mut runtime.ball, host);
    crate::tetherball_server::setup_server(runtime, host.server_services())?;
    host.effect(Effect::VSync(true));
    host.effect(Effect::SyncTask(false));
    host.effect(Effect::ResetHomeIcon);
    host.effect(Effect::HomeMenu(true));
    Ok(())
}

#[cfg(test)]
#[path = "tetherball_startup_tests.rs"]
mod tests;
