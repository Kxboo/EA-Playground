//! Original MGTetherball reset and server-selection decisions.
//!
//! The reset code touches the game object, the tetherball and several engine
//! objects. `ResetState` holds the game-object fields that are not represented
//! by `Lifecycle`; `Lifecycle` and `BallMotion` remain the shared typed state.
//! Calls into UI, camera, animation, particle, AI and database services are
//! returned in original call order so the host can apply them synchronously.

use crate::tetherball::ScoreWeights;
use crate::tetherball::{BallMotion, wrap_angle};
use crate::tetherball_lifecycle::Lifecycle;

const DISTANCE_RADII: [f32; 3] = [0.7, 0.9, 1.1];
const CHARACTER_SEPARATION: [f32; 3] = [1.25, 1.45, 1.65]; // 0x80442058
const CAMERA_BACKWARDS: [f32; 3] = [4.5, 4.75, 5.0]; // 0x80442070
const CAMERA_DIRECTION_OFFSET: f32 = 5.47;
const CAMERA_TARGET_HEIGHT: f32 = 2.2;
const CAMERA_TRANSITION_MS: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundSlot {
    pub kind: u32,
    pub value: f32,
    pub counter: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundTuning {
    pub base_hit_speed: f32,
    pub accelerate_modifier: f32,
    pub power_modifier: f32,
    pub mega_modifier: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub id: i32,
    pub matrix_handle: u32,
}

/// MGTetherball fields required by ResetRound/SetUpServer but not currently in
/// the shared Lifecycle value. Fields are named after their retail offsets.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetState {
    pub game_mode_044: i32,
    pub base_player_count_070: i32,
    pub initial_rotation_178: i32,
    pub rotation_limit_430: i32,
    pub camera_heading_394: f32,
    pub world_position_110: [f32; 3],
    pub world_matrix_398: [f32; 16],
    pub start_angles_248: [f32; 2],
    pub ai_initial_values_27c: [i32; 2],
    pub ai_special_case_0c0: i32,

    pub word_224: i32,
    pub field_25c: bool,
    /// Opaque +0x229 marker; ResetRound preserves it.
    pub field_229: bool,
    pub counter_260: u32,
    pub counter_264: u32,
    pub counter_268: u32,
    pub timer_26c: f32,
    pub counter_270: u32,
    pub slots_284: [RoundSlot; 8],
    pub pending_count_2e4: u32,
    pub field_334_guid: u32,
    pub round_tunables_34c_358: [f32; 4],
    pub field_35c: f32,

    pub field_444: bool,
    pub alternate_server_445: bool,
    pub server_side_flags: [bool; 4],
    /// Original initial-receiver byte at +0x220; distinct from Lifecycle.receiver.
    pub receiver_220: i32,
    pub current_ai_entities_128: [u32; 2],
    pub current_ball_owner_074: u32,
    pub current_ball_matrix_078: u32,
    pub game_marker_matrix_278: u32,
    pub ball_fx_140: [u32; 3],
    pub camera_handle_390: u32,
    pub pole_handle_184: u32,
    pub player_handles_120: [u32; 2],
    pub ball_handle_104: u32,
    pub invalid_guid: u32,
    pub ball_invalid_guid: u32,
}

impl ResetState {
    /// Convenient deterministic seed for adapters and fixtures. Values that
    /// belong to the caller (handles, matrices, mode, tuning inputs) stay
    /// explicit; this only initializes the scalar fields ResetRound overwrites.
    pub fn blank() -> Self {
        Self {
            game_mode_044: 0,
            base_player_count_070: 1,
            initial_rotation_178: 6,
            rotation_limit_430: 6,
            camera_heading_394: 0.0,
            world_position_110: [0.0; 3],
            world_matrix_398: crate::area_transform::IDENTITY,
            start_angles_248: [0.0; 2],
            ai_initial_values_27c: [0, 0],
            ai_special_case_0c0: 0,
            word_224: -1,
            field_25c: true,
            field_229: true,
            counter_260: 99,
            counter_264: 99,
            counter_268: 99,
            timer_26c: 99.0,
            counter_270: 99,
            slots_284: [RoundSlot {
                kind: 0,
                value: 0.0,
                counter: 0,
            }; 8],
            pending_count_2e4: 3,
            field_334_guid: u32::MAX,
            round_tunables_34c_358: [0.0; 4],
            field_35c: 1.0,
            field_444: false,
            alternate_server_445: false,
            server_side_flags: [false; 4],
            receiver_220: 1,
            current_ai_entities_128: [0; 2],
            current_ball_owner_074: 0,
            current_ball_matrix_078: 0,
            game_marker_matrix_278: 0,
            ball_fx_140: [u32::MAX; 3],
            camera_handle_390: 0,
            pole_handle_184: 0,
            player_handles_120: [0; 2],
            ball_handle_104: 0,
            invalid_guid: u32::MAX,
            ball_invalid_guid: u32::MAX,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResetInputs {
    /// Four VLT values by difficulty, in the exact ResetRound read order.
    pub round_tuning: [RoundTuning; 4],
    /// ResetStats' virtual GetDifficultyType result.
    pub scoring_difficulty: usize,
    pub scoring_array_count: usize,
    pub scoring_weights: [ScoreWeights; 4],
    pub camera_position_offset: [f32; 3],
    pub camera_target_offset: [f32; 3],
    pub ai_global_enable: bool,
    pub initialized_animation_states: [i32; 2],
    /// Synchronous external return values for the engine-owned object calls.
    pub created_ai_entities: [u32; 2],
    pub random_server_result: i32,
    pub camera_lookup_result: u32,
    /// Marker records indexed by character; SetupServer queries the newly chosen server.
    pub markers: [Vec<Marker>; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResetEffect {
    TimerVisible {
        index: i32,
        visible: bool,
    },
    ServeBubbleVisible {
        args: [i32; 3],
    },
    DestroyPartFx {
        guid: u32,
        fade: i32,
    },
    InitializeHud,
    PoleTextureMatrix {
        placeable: u32,
        matrix_bits: [u32; 16],
    },
    GrabBall {
        ball: u32,
        character: u32,
        matrix: u32,
    },
    SwitchToAi {
        player: usize,
    },
    CameraPositionOffset {
        camera: u32,
        offset_bits: [u32; 3],
    },
    CameraDesiredPositionOffset {
        camera: u32,
        offset_bits: [u32; 3],
        ms: u32,
    },
    CameraTargetOffset {
        camera: u32,
        offset_bits: [u32; 3],
    },
    CameraDesiredTargetOffset {
        camera: u32,
        offset_bits: [u32; 3],
        ms: u32,
    },
    CameraBackwardsOffset {
        camera: u32,
        offset_bits: u32,
    },
    CameraDistance {
        camera: u32,
        distance_bits: u32,
        target_height_bits: u32,
    },
    CameraDesiredRotation {
        camera: u32,
        angle_bits: u32,
        ms: u32,
    },
    CameraStartPosition {
        camera: u32,
        position_bits: [u32; 3],
    },
    CameraDirection {
        camera: u32,
        direction_bits: [u32; 3],
    },
    CreateAiEntity {
        player: usize,
        character: u32,
        entity: u32,
    },
    BindAiEntity {
        player: usize,
        character: u32,
        entity: u32,
        ball: u32,
    },
    InitializeAi {
        entity: u32,
        enabled: bool,
        difficulty: i32,
        angle_bits: u32,
    },
    AiAngle {
        entity: u32,
        angle_bits: u32,
    },
    AiStartValue {
        entity: u32,
        value_bits: u32,
    },
    AiDistance {
        entity: u32,
        distance: i32,
    },
    CharacterPosition {
        player: usize,
        character: u32,
        position_bits: [u32; 3],
    },
    DatabaseCollection {
        outer: &'static str,
        inner: &'static str,
    },
    DatabaseArrayCount {
        name: &'static str,
        count: usize,
    },
    DifficultyLookup {
        mode: i32,
        result: usize,
    },
    DatabaseUInt {
        name: &'static str,
        index: usize,
        value: u32,
    },
    DatabaseFloat {
        name: &'static str,
        index: usize,
        value_bits: u32,
    },
    DestroyDatabaseCollection,
    RandomRange {
        low: i32,
        high: i32,
        value: i32,
    },
    CameraLookup {
        index: u32,
        camera: u32,
    },
    CameraTargetPosition {
        camera: u32,
        position_bits: [u32; 3],
    },
    InitializePlayerAnimations,
    AnimationNextState {
        player: usize,
        state: i32,
        force: bool,
        blend: i32,
    },
    AnimationMarkerId {
        player: usize,
        index: usize,
        id: i32,
    },
    AnimationMarkerMatrix {
        player: usize,
        index: usize,
        handle: u32,
    },
    ScoreboardVisible {
        args: [i32; 4],
    },
}

fn bits3(value: [f32; 3]) -> [u32; 3] {
    value.map(f32::to_bits)
}
fn wrap_i8(value: i32) -> i8 {
    value as u8 as i8
}
fn float_from_i32(value: i32) -> f32 {
    value as f32
}

/// Original ResetRound plus its synchronous SetUpServer dependency.
/// Database values, allocation handles, random output and animation markers
/// are explicit responses from the host services; all game scalar and ball
/// updates here follow the retail instruction order.
pub fn reset_round(
    lifecycle: &mut Lifecycle,
    state: &mut ResetState,
    ball: &mut BallMotion,
    input: &ResetInputs,
) -> Vec<ResetEffect> {
    assert!(
        (0..=1).contains(&lifecycle.focus_player),
        "ResetRound server must be player 0 or 1"
    );
    assert!(
        (0..=1).contains(&input.random_server_result),
        "rmRandRange result is outside [0,1]"
    );
    let mut effects = Vec::new();

    if lifecycle.hud_ready {
        effects.push(ResetEffect::TimerVisible {
            index: 0,
            visible: false,
        });
        if lifecycle.serve_bubble_visible {
            effects.push(ResetEffect::ServeBubbleVisible { args: [0, -1, -1] });
            lifecycle.serve_bubble_visible = false;
        }
    }
    if state.field_334_guid != state.invalid_guid {
        effects.push(ResetEffect::DestroyPartFx {
            guid: state.field_334_guid,
            fade: 0,
        });
        state.field_334_guid = state.invalid_guid;
    }
    if !lifecycle.hud_ready {
        effects.push(ResetEffect::InitializeHud);
    }

    state.field_25c = false;
    state.word_224 = -1;
    for slot in &mut state.slots_284 {
        *slot = RoundSlot {
            kind: 2,
            value: 0.0,
            counter: 0,
        };
    }
    let target = state.initial_rotation_178;
    lifecycle.match_state.rotations = [wrap_i8(target.wrapping_neg()), wrap_i8(target)];
    state.counter_260 = 0;
    state.counter_264 = 0;
    state.counter_268 = 0;
    state.timer_26c = 0.0;
    state.counter_270 = 0;
    lifecycle.latches = [false; 2];
    lifecycle.action_states = [2; 2];
    lifecycle.mega_states = [1; 2];
    lifecycle.mega_values = [0; 2];
    lifecycle.match_state.elapsed_ms = 0;
    lifecycle.match_state.round_winner = -1;
    lifecycle.match_state.match_over = false;
    lifecycle.indicator_current = 0.0;
    lifecycle.indicator_target =
        (lifecycle.match_state.rotations[0] as f32) / (state.rotation_limit_430 as f32);

    let mut pole_matrix = [0.0_f32; 16];
    pole_matrix[0] = 1.0;
    pole_matrix[5] = 1.0;
    pole_matrix[10] = 1.0;
    // ResetRound passes literal f1=0 to SetPoleIndicatorOffset.
    pole_matrix[13] = 0.0;
    pole_matrix[15] = 1.0;
    effects.push(ResetEffect::PoleTextureMatrix {
        placeable: state.pole_handle_184,
        matrix_bits: pole_matrix.map(f32::to_bits),
    });

    // ResetRound writes the active player's start angle, then calls Tetherball::Grab.
    let first_player = lifecycle.focus_player as usize;
    ball.angle = wrap_angle(state.start_angles_248[first_player]);
    grab_ball(
        state,
        ball,
        first_player,
        state.game_marker_matrix_278,
        &mut effects,
    );

    // SetPlayerDistance(2): both player controls become AI and the ball's
    // desired radius is rescaled before SetRadius later stores 1.1 directly.
    set_player_distance(lifecycle, ball, 2, &mut effects);

    let camera = state.camera_handle_390;
    effects.push(ResetEffect::CameraPositionOffset {
        camera,
        offset_bits: bits3(input.camera_position_offset),
    });
    effects.push(ResetEffect::CameraDesiredPositionOffset {
        camera,
        offset_bits: bits3(input.camera_position_offset),
        ms: CAMERA_TRANSITION_MS,
    });
    effects.push(ResetEffect::CameraTargetOffset {
        camera,
        offset_bits: bits3(input.camera_target_offset),
    });
    effects.push(ResetEffect::CameraDesiredTargetOffset {
        camera,
        offset_bits: bits3(input.camera_target_offset),
        ms: CAMERA_TRANSITION_MS,
    });
    effects.push(ResetEffect::CameraBackwardsOffset {
        camera,
        offset_bits: CAMERA_BACKWARDS[2].to_bits(),
    });
    effects.push(ResetEffect::CameraDistance {
        camera,
        distance_bits: CAMERA_TARGET_HEIGHT.to_bits(),
        target_height_bits: 0.0_f32.to_bits(),
    });
    effects.push(ResetEffect::CameraDesiredRotation {
        camera,
        angle_bits: (state.camera_heading_394 + CAMERA_DIRECTION_OFFSET).to_bits(),
        ms: CAMERA_TRANSITION_MS,
    });
    let transformed_start = transform_point([0.0; 3], [0.5, 0.0, 0.0], &state.world_matrix_398);
    let camera_start = [
        state.world_position_110[0] + transformed_start[0],
        state.world_position_110[1] + transformed_start[1],
        state.world_position_110[2] + transformed_start[2],
    ];
    effects.push(ResetEffect::CameraStartPosition {
        camera,
        position_bits: bits3(camera_start),
    });
    effects.push(ResetEffect::CameraDirection {
        camera,
        direction_bits: direction_from_angle(wrap_angle(
            state.camera_heading_394 + CAMERA_DIRECTION_OFFSET,
        ))
        .map(f32::to_bits),
    });
    ball.set_radius(DISTANCE_RADII[2]);

    let difficulty: usize = match state.game_mode_044 {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        other => panic!("ResetRound game mode {other} is outside the recovered 0..=3 domain"),
    };
    for player in 0..2 {
        let character = state.player_handles_120[player];
        let entity = input.created_ai_entities[player];
        state.current_ai_entities_128[player] = entity;
        effects.push(ResetEffect::CreateAiEntity {
            player,
            character,
            entity,
        });
        effects.push(ResetEffect::BindAiEntity {
            player,
            character,
            entity,
            ball: state.ball_handle_104,
        });
    }
    let ai_enabled = state.ai_special_case_0c0 == 6 || input.ai_global_enable;
    effects.push(ResetEffect::InitializeAi {
        entity: state.current_ai_entities_128[1],
        enabled: ai_enabled,
        difficulty: difficulty as i32,
        angle_bits: wrap_angle(state.camera_heading_394).to_bits(),
    });
    for player in 0..2 {
        let entity = state.current_ai_entities_128[player];
        let angle = wrap_angle(state.start_angles_248[player]);
        effects.push(ResetEffect::AiAngle {
            entity,
            angle_bits: angle.to_bits(),
        });
        effects.push(ResetEffect::AiStartValue {
            entity,
            value_bits: float_from_i32(state.ai_initial_values_27c[player]).to_bits(),
        });
        effects.push(ResetEffect::AiDistance {
            entity,
            distance: lifecycle.current_distance,
        });
    }

    assert_eq!(
        lifecycle.current_distance, 2,
        "ResetRound separation expects SetPlayerDistance(2)"
    );
    let separation = CHARACTER_SEPARATION[lifecycle.current_distance as usize];
    for player in 0..2 {
        let mut position = state.world_position_110;
        position[0] = if player == 0 {
            position[0] + separation
        } else {
            position[0] - separation
        };
        lifecycle.players[player].ai_distance = lifecycle.current_distance;
        effects.push(ResetEffect::CharacterPosition {
            player,
            character: state.player_handles_120[player],
            position_bits: bits3(position),
        });
    }
    // Original calls SwitchToAIControl on both characters again after SetPos.
    effects.push(ResetEffect::SwitchToAi { player: 0 });
    effects.push(ResetEffect::SwitchToAi { player: 1 });
    lifecycle.field_32f = false;
    lifecycle.serve_bubble_visible = false;
    lifecycle.match_state.match_over = false;

    let round = input.round_tuning[difficulty];
    effects.push(ResetEffect::DatabaseCollection {
        outer: "mg_tetherball",
        inner: "tunables",
    });
    effects.push(ResetEffect::DatabaseFloat {
        name: "ball_basehitspeed",
        index: difficulty,
        value_bits: round.base_hit_speed.to_bits(),
    });
    effects.push(ResetEffect::DatabaseFloat {
        name: "ball_acceleratemodifier",
        index: difficulty,
        value_bits: round.accelerate_modifier.to_bits(),
    });
    effects.push(ResetEffect::DatabaseFloat {
        name: "ball_powermodifier",
        index: difficulty,
        value_bits: round.power_modifier.to_bits(),
    });
    effects.push(ResetEffect::DatabaseFloat {
        name: "ball_megamodifier",
        index: difficulty,
        value_bits: round.mega_modifier.to_bits(),
    });
    effects.push(ResetEffect::DestroyDatabaseCollection);
    state.round_tunables_34c_358 = [
        round.base_hit_speed,
        round.power_modifier,
        round.mega_modifier,
        round.accelerate_modifier,
    ];
    state.field_35c = 0.0;

    // The function's tail invokes SetUpServer synchronously. Its random result
    // is an input; the server toggle and all stored pair fields are original.
    effects.extend(setup_server(lifecycle, state, ball, input));
    effects
}

/// Original SetUpServer decision body, also callable independently by the host.
/// Camera lookup, random output, animation initialization and marker reads stay
/// at their actual service boundaries.
pub fn setup_server(
    lifecycle: &mut Lifecycle,
    state: &mut ResetState,
    ball: &mut BallMotion,
    input: &ResetInputs,
) -> Vec<ResetEffect> {
    assert!(
        (0..=1).contains(&input.random_server_result),
        "rmRandRange result is outside [0,1]"
    );
    let mut effects = vec![ResetEffect::RandomRange {
        low: 0,
        high: 1,
        value: input.random_server_result,
    }];
    let server = select_server(state, input.random_server_result);
    let receiver = 1 - server;
    lifecycle.server = server as usize;
    lifecycle.receiver = receiver as usize;
    lifecycle.focus_player = server;
    state.receiver_220 = receiver;
    state.ai_initial_values_27c = if server == 0 { [1, -1] } else { [-1, 1] };
    // Stored byte order is +244,+245,+32a,+32b.
    state.server_side_flags = [server == 0, server == 1, server == 1, server == 0];

    let camera = input.camera_lookup_result;
    effects.push(ResetEffect::CameraLookup { index: 0, camera });
    let (position, direction) = server_camera(state, server);
    effects.push(ResetEffect::CameraTargetPosition {
        camera,
        position_bits: bits3(position),
    });
    effects.push(ResetEffect::CameraDirection {
        camera,
        direction_bits: direction.map(f32::to_bits),
    });

    effects.push(ResetEffect::InitializePlayerAnimations);
    // The initializer synchronously writes +190/+194, which Lifecycle exposes
    // as lose_animations. The receiver consumes the updated entry immediately.
    lifecycle.lose_animations = input.initialized_animation_states;
    let focus = server as usize;
    effects.push(ResetEffect::AnimationNextState {
        player: focus,
        state: 0x3a,
        force: false,
        blend: -1,
    });
    effects.push(ResetEffect::AnimationNextState {
        player: 1 - focus,
        state: lifecycle.lose_animations[1 - focus],
        force: false,
        blend: -1,
    });
    for (index, marker) in input.markers[focus].iter().enumerate() {
        effects.push(ResetEffect::AnimationMarkerId {
            player: focus,
            index,
            id: marker.id,
        });
        if marker.id == 0x3f {
            state.game_marker_matrix_278 = marker.matrix_handle;
            effects.push(ResetEffect::AnimationMarkerMatrix {
                player: focus,
                index,
                handle: marker.matrix_handle,
            });
        }
    }
    if ball.angular_velocity == 0.0 {
        ball.angle = wrap_angle(state.start_angles_248[focus]);
        grab_ball(
            state,
            ball,
            focus,
            state.game_marker_matrix_278,
            &mut effects,
        );
    }
    effects
}

/// Shared native server selection, including signed counts and alternation.
pub(crate) fn select_server(state: &mut ResetState, random: i32) -> i32 {
    match state.base_player_count_070 {
        1 => 0,
        n if n > 1 => {
            if state.alternate_server_445 {
                state.alternate_server_445 = false;
                1
            } else {
                state.alternate_server_445 = true;
                0
            }
        }
        _ => random,
    }
}

pub(crate) fn server_camera(state: &ResetState, server: i32) -> ([f32; 3], [f32; 3]) {
    let offset = if server == 1 {
        [-0.5, 0., 0.]
    } else {
        [0.5, 0., 0.]
    };
    let position = transform_point([0.; 3], offset, &state.world_matrix_398);
    let mut angle = state.camera_heading_394 + CAMERA_DIRECTION_OFFSET;
    if server == 1 {
        angle = std::f32::consts::PI + angle;
    }
    (position, direction_from_angle(wrap_angle(angle)))
}

/// ResetMiniGame (including its original ResetStats field stores) followed by
/// the complete ResetRound/SetUpServer path.
pub fn reset_minigame(
    lifecycle: &mut Lifecycle,
    state: &mut ResetState,
    ball: &mut BallMotion,
    input: &ResetInputs,
) -> Vec<ResetEffect> {
    let mut effects = Vec::new();
    lifecycle.score_weights = [0; 3];
    effects.push(ResetEffect::DatabaseCollection {
        outer: "mg_tetherball",
        inner: "scoring",
    });
    effects.push(ResetEffect::DifficultyLookup {
        mode: state.game_mode_044,
        result: input.scoring_difficulty,
    });
    let count = input.scoring_array_count;
    effects.push(ResetEffect::DatabaseArrayCount {
        name: "accuracy_points",
        count,
    });
    let difficulty = input.scoring_difficulty;
    if difficulty < count && difficulty < input.scoring_weights.len() {
        let weights = input.scoring_weights[difficulty];
        effects.push(ResetEffect::DatabaseUInt {
            name: "accuracy_points",
            index: difficulty,
            value: weights.accuracy_points as u32,
        });
        effects.push(ResetEffect::DatabaseUInt {
            name: "powerhit_points",
            index: difficulty,
            value: weights.power_hit_points as u32,
        });
        effects.push(ResetEffect::DatabaseUInt {
            name: "megahit_points",
            index: difficulty,
            value: weights.mega_hit_points as u32,
        });
        lifecycle.score_weights = [
            weights.accuracy_points,
            weights.power_hit_points,
            weights.mega_hit_points,
        ];
    }
    effects.push(ResetEffect::DestroyDatabaseCollection);
    lifecycle.statistics = [[0; 5]; 2];
    lifecycle.match_state.round_wins = [0; 2];
    lifecycle.match_state.match_winner = -1;
    lifecycle.round_number = 1;
    state.alternate_server_445 = false;
    effects.extend(reset_round(lifecycle, state, ball, input));
    state.field_444 = true;
    if lifecycle.scoreboard_visible && lifecycle.hud_ready {
        effects.push(ResetEffect::ScoreboardVisible { args: [0; 4] });
    }
    lifecycle.scoreboard_visible = false;
    effects
}

fn grab_ball(
    state: &mut ResetState,
    ball: &mut BallMotion,
    player: usize,
    matrix: u32,
    effects: &mut Vec<ResetEffect>,
) {
    state.current_ball_owner_074 = state.player_handles_120[player];
    state.current_ball_matrix_078 = matrix;
    ball.grabbed = true;
    ball.desired_radius = ball.radius;
    ball.spinning_up = false;
    ball.spinning_down = false;
    ball.angular_velocity = 0.0;
    ball.secondary_velocity = 0.0;
    effects.push(ResetEffect::GrabBall {
        ball: state.ball_handle_104,
        character: state.player_handles_120[player],
        matrix,
    });
    for guid in &mut state.ball_fx_140 {
        if *guid != state.ball_invalid_guid {
            effects.push(ResetEffect::DestroyPartFx {
                guid: *guid,
                fade: 0,
            });
            *guid = state.ball_invalid_guid;
        }
    }
}

fn set_player_distance(
    lifecycle: &mut Lifecycle,
    ball: &mut BallMotion,
    distance: usize,
    effects: &mut Vec<ResetEffect>,
) {
    if lifecycle.current_distance == distance as i32 {
        return;
    }
    for (player, value) in lifecycle.players.iter_mut().enumerate() {
        value.movement_speed = 1.2;
        value.ai_distance = distance as i32;
        effects.push(ResetEffect::SwitchToAi { player });
    }
    ball.set_desired_radius(DISTANCE_RADII[distance]);
    lifecycle.current_distance = distance as i32;
}

fn transform_point(point: [f32; 3], add: [f32; 3], matrix: &[f32; 16]) -> [f32; 3] {
    let v = [point[0] + add[0], point[1] + add[1], point[2] + add[2]];
    std::array::from_fn(|col| {
        let acc = v[0] * matrix[col];
        let acc = v[1].mul_add(matrix[4 + col], acc);
        let acc = v[2].mul_add(matrix[8 + col], acc);
        1.0_f32.mul_add(matrix[12 + col], acc)
    })
}

fn direction_from_angle(angle: f32) -> [f32; 3] {
    let (sin, cos) = crate::character_input::ea_sin_cos(angle);
    [sin, 0.0, cos]
}

#[cfg(test)]
#[path = "tetherball_reset_tests.rs"]
pub(crate) mod tests;
