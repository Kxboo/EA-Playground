//! Enclosing MGTetherball::Initialize (0x803966c4). Game helper calls remain
//! explicit stages so their existing ports can share the same Runtime owners.
use crate::area_transform::{self as matrix, Matrix};
use crate::tetherball_angles::wrap_angle;
use crate::tetherball_player_init::PlayerInitInput;
use crate::tetherball_runtime::Runtime;

#[derive(Clone, Debug)]
pub struct StartupState {
    /// Base Minigame pointers copied from World +8,+c,+10,+14,+18,+20.
    pub world_services: [u32; 6],
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
pub enum Stage {
    BaseInitialize {
        world: u32,
    },
    InitGameLogic,
    ConstructBall {
        handle: u32,
    },
    SetArea(i32),
    Character {
        kind: CharacterKind,
        input: PlayerInitInput,
    },
    Ai {
        player: usize,
        enabled: bool,
        difficulty: u32,
        heading: f32,
    },
    Ball {
        difficulty: u32,
        anchor: [f32; 3],
        heading: f32,
        pole: u32,
    },
    OpenPregame(i32),
    SetUpServer,
}

#[derive(Clone, Debug)]
pub enum Effect {
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

/// Synchronous original boundaries. Stages must apply their writes to the
/// provided owners before returning. An error leaves a partially initialized
/// object; callers must discard it, rather than start a match with that state.
pub trait StartupServices:
    crate::tetherball_lifecycle::Services + crate::tetherball_shadow_setup::ShadowSetupServices
{
    fn effect(&mut self, effect: Effect);
    fn stage(
        &mut self,
        stage: Stage,
        runtime: &mut Runtime,
        state: &mut StartupState,
    ) -> Result<(), String>;
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
    host.stage(Stage::BaseInitialize { world: input.world }, runtime, state)?;
    host.stage(Stage::InitGameLogic, runtime, state)?;
    let ball = host.allocate_ball(0x170, "Minigame::Tetherball");
    if ball != 0 {
        host.stage(Stage::ConstructBall { handle: ball }, runtime, state)?;
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
    host.stage(Stage::SetArea(variant), runtime, state)?;
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
    host.stage(
        Stage::Character {
            kind: CharacterKind::Player,
            input: PlayerInitInput {
                position: positions[0],
                heading: first_heading,
                identity_words: state.identities_078_0b8[0],
            },
        },
        runtime,
        state,
    )?;
    host.stage(
        Stage::Ai {
            player: 0,
            enabled: runtime.state.serve.forced_ai[0],
            difficulty: 1,
            heading: wrap_angle(heading),
        },
        runtime,
        state,
    )?;
    let second_kind = match runtime.state.reset.ai_special_case_0c0 {
        6 => CharacterKind::Ai,
        3 => CharacterKind::AdditionalPlayer,
        v => return Err(format!("unsupported native participant configuration {v}")),
    };
    host.stage(
        Stage::Character {
            kind: second_kind,
            input: PlayerInitInput {
                position: positions[1],
                heading: std::f32::consts::PI + first_heading,
                identity_words: state.identities_078_0b8[1],
            },
        },
        runtime,
        state,
    )?;
    host.stage(
        Stage::Ai {
            player: 1,
            enabled: second_kind == CharacterKind::Ai || runtime.state.serve.forced_ai[1],
            difficulty,
            heading: wrap_angle(heading),
        },
        runtime,
        state,
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
    host.stage(
        Stage::Ball {
            difficulty,
            anchor,
            heading,
            pole,
        },
        runtime,
        state,
    )?;
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
    host.stage(Stage::OpenPregame(2), runtime, state)?;
    runtime
        .life
        .change_state(1, runtime.state.rules, &mut runtime.ball, host);
    host.stage(Stage::SetUpServer, runtime, state)?;
    host.effect(Effect::VSync(true));
    host.effect(Effect::SyncTask(false));
    host.effect(Effect::ResetHomeIcon);
    host.effect(Effect::HomeMenu(true));
    Ok(())
}

#[cfg(test)]
#[path = "tetherball_startup_tests.rs"]
mod tests;
