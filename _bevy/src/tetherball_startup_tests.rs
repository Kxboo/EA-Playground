use super::*;
use crate::tetherball_hit_animation::{HitAnimations, ReadyAnimation};
use crate::tetherball_runtime::RuntimeState;
use serde_json::{Value, json};
fn u(v: &Value) -> u32 {
    v.as_u64()
        .map(|x| x as u32)
        .unwrap_or_else(|| v.as_i64().unwrap() as u32)
}
fn f(v: &Value) -> f32 {
    f32::from_bits(u(v))
}
fn vector(v: &Value) -> [f32; 3] {
    std::array::from_fn(|i| f(&v[i]))
}

// Test-only preexisting object: every represented top-level field is then
// overwritten from the original fixture's initial memory. Never used by Game.
fn runtime_seed() -> Runtime {
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_runtime_reset_golden.json"
    ))
    .unwrap();
    let (life, reset, ball, _) =
        crate::tetherball_reset::tests::seed(&fixture["cases"][0], &fixture);
    let ready = ReadyAnimation {
        state: [0; 2],
        suppress_if_current: [0; 2],
    };
    Runtime {
        life,
        ball,
        state: RuntimeState {
            reset,
            serve: crate::tetherball_serve::ServeState {
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
            gestures: Default::default(),
            rally: crate::tetherball_rally_rules::RallyRuleState {
                ai_hit_attempt_234: 0,
                ai_power_hit_type_238: 0,
                ai_charge: [0; 2],
            },
            hit: crate::tetherball_hit::HitState {
                field_32c: false,
                field_32d: false,
                mega_ability_42e: false,
                pending_zone_274: 0,
                power_hit_type_43c: 0,
                hit_multiplier_428: 0.,
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
            scene: crate::tetherball_scene::BallScene {
                anchor: [0.; 3],
                position: [0.; 3],
                ball_matrix: matrix::IDENTITY,
                rope_matrix: matrix::IDENTITY,
                trails: [0; 3],
                null_trail: u32::MAX,
                ball_shadow: false,
                rope_shadow: false,
            },
            ai: [None, None],
            rules: crate::tetherball_match::MatchRules {
                mode: 0,
                rotation_limit: 0,
                wins_required: 0,
                time_limit_seconds: 0,
            },
            frontend: crate::tetherball_frontend::FrontendState {
                pregame_ready_058: 0,
                postgame_choice_05c: 0,
                field_424: 0,
            },
            fx_names: std::array::from_fn(|_| std::array::from_fn(|_| String::new())),
        },
    }
}

// Only named owned fields appear here. Full memory comparison below also
// rejects unexpected changes to unmapped fields in the original body.
fn word(r: &mut Runtime, s: &mut StartupState, o: usize, write: Option<u32>) -> Option<u32> {
    macro_rules! scalar {
        ($x:expr) => {{
            if let Some(v) = write {
                $x = v as _;
            }
            Some($x as u32)
        }};
    }
    macro_rules! float {
        ($x:expr) => {{
            if let Some(v) = write {
                $x = f32::from_bits(v);
            }
            Some($x.to_bits())
        }};
    }
    match o {
        0x8 | 0xc | 0x10 | 0x14 | 0x18 => scalar!(s.world_services[(o - 8) / 4]),
        0x20 => scalar!(s.world_services[5]),
        0x4 | 0x1c => {
            let metadata = s.constructor_metadata.as_mut()?;
            scalar!(metadata.world_words_004_01c[usize::from(o == 0x1c)])
        }
        0x28 => scalar!(s.constructor_metadata.as_mut()?.scene_028),
        0x38 => scalar!(s.constructor_metadata.as_mut()?.tag_038),
        0x50 | 0x54 => scalar!(s.constructor_metadata.as_mut()?.ui_words_050_054[(o - 0x50) / 4]),
        0x68 => scalar!(s.constructor_metadata.as_mut()?.result_068),
        0xf8 => scalar!(s.constructor_metadata.as_mut()?.rules_0f8),
        0x2e8 => scalar!(s.constructor_metadata.as_mut()?.scene_2e8),
        0x2f0 => scalar!(s.constructor_metadata.as_mut()?.word_2f0),
        0x24 => {
            if let Some(v) = write {
                r.life.paused = v >> 24 != 0;
            }
            Some(u32::from(r.life.paused) << 24)
        }
        0x5c => scalar!(r.state.frontend.postgame_choice_05c),
        0x60 => scalar!(r.life.match_state.result),
        0x64 => scalar!(r.life.match_state.final_result),
        0x70 => scalar!(r.state.reset.base_player_count_070),
        0xfc => scalar!(r.state.serve.pause_block_count_0fc),
        0x134 => {
            if let Some(v) = write {
                r.life.match_state.round_wins = [(v >> 24) as u8 as i8, (v >> 16) as u8 as i8];
            }
            Some(
                ((r.life.match_state.round_wins[0] as u8 as u32) << 24)
                    | ((r.life.match_state.round_wins[1] as u8 as u32) << 16),
            )
        }
        0x20c => {
            if let Some(v) = write {
                r.life.match_state.match_over = v >> 24 != 0;
            }
            Some(u32::from(r.life.match_state.match_over) << 24)
        }
        0x214 => scalar!(r.life.server),
        0x218 => scalar!(r.life.receiver),
        0x224 => scalar!(r.state.reset.word_224),
        0x228 => {
            if let Some(v) = write {
                r.life.latches = [v >> 24 != 0, (v >> 8) & 255 != 0];
                r.state.gestures.hit_attempt_marker = (v >> 16) & 255 != 0;
                r.state.reset.field_229 = r.state.gestures.hit_attempt_marker;
            }
            Some(
                (u32::from(r.life.latches[0]) << 24)
                    | (u32::from(r.state.gestures.hit_attempt_marker) << 16)
                    | (u32::from(r.life.latches[1]) << 8),
            )
        }
        0x22c | 0x230 => scalar!(r.life.action_states[(o - 0x22c) / 4]),
        0x234 => scalar!(r.state.rally.ai_hit_attempt_234),
        0x238 => scalar!(r.state.rally.ai_power_hit_type_238),
        0x258 => scalar!(r.life.match_state.elapsed_ms),
        0x25c => {
            if let Some(v) = write {
                r.state.reset.field_25c = v >> 24 != 0;
            }
            Some(u32::from(r.state.reset.field_25c) << 24)
        }
        0x260 => scalar!(r.state.reset.counter_260),
        0x264 => scalar!(r.state.reset.counter_264),
        0x268 => scalar!(r.state.reset.counter_268),
        0x26c => float!(r.state.reset.timer_26c),
        0x270 => scalar!(r.state.reset.counter_270),
        0x274 => scalar!(r.state.hit.pending_zone_274),
        0x278 => scalar!(r.state.reset.game_marker_matrix_278),
        0x284..=0x2e0 => {
            let slot = (o - 0x284) / 12;
            match (o - 0x284) % 12 {
                0 => scalar!(r.state.reset.slots_284[slot].kind),
                4 => float!(r.state.reset.slots_284[slot].value),
                _ => scalar!(r.state.reset.slots_284[slot].counter),
            }
        }
        0x2e4 => scalar!(r.state.reset.pending_count_2e4),
        0x30c | 0x310 => scalar!(r.life.mega_states[(o - 0x30c) / 4]),
        0x314 | 0x318 => scalar!(r.life.mega_values[(o - 0x314) / 4]),
        0x330 => {
            if let Some(v) = write {
                r.life.serve_bubble_visible = v >> 24 != 0;
            }
            Some(u32::from(r.life.serve_bubble_visible) << 24)
        }
        0x338 => float!(r.state.hit.indicator_scale_338),
        0x33c => float!(r.state.hit.indicator_rate_33c),
        0x344 => float!(r.life.indicator_current),
        0x35c => float!(r.state.reset.field_35c),
        0x41c => {
            if let Some(v) = write {
                r.life.round_visible = v >> 24 != 0;
                r.life.scoreboard_visible = (v >> 16) & 255 != 0;
            }
            Some(
                (u32::from(r.life.round_visible) << 24)
                    | (u32::from(r.life.scoreboard_visible) << 16),
            )
        }
        0x420 => scalar!(r.life.round_timer_ms),
        0x428 => float!(r.state.hit.hit_multiplier_428),
        0x42c => {
            if let Some(v) = write {
                r.state.serve.power_serve_enabled = v >> 24 != 0;
                r.state.hit.hit_multiplier_enabled_42d = (v >> 16) & 255 != 0;
                r.state.hit.mega_ability_42e = (v >> 8) & 255 != 0;
            }
            Some(
                (u32::from(r.state.serve.power_serve_enabled) << 24)
                    | (u32::from(r.state.hit.hit_multiplier_enabled_42d) << 16)
                    | (u32::from(r.state.hit.mega_ability_42e) << 8),
            )
        }
        0x43c => scalar!(r.state.hit.power_hit_type_43c),
        0x444 => {
            if let Some(v) = write {
                r.state.reset.field_444 = v >> 24 != 0;
                r.state.reset.alternate_server_445 = (v >> 16) & 255 != 0;
            }
            Some(
                (u32::from(r.state.reset.field_444) << 24)
                    | (u32::from(r.state.reset.alternate_server_445) << 16),
            )
        }
        0x2c => scalar!(s.tag_02c),
        0x30 => scalar!(s.tag_030),
        0x34 => scalar!(r.life.match_state.state_code),
        0x3c => scalar!(r.life.variant),
        0x40 => scalar!(r.life.session_mode),
        0x44 => scalar!(r.state.reset.game_mode_044),
        0x48 => scalar!(r.life.game_type),
        0x4c => {
            if let Some(v) = write {
                s.base_flags_04c_04d = [(v >> 24) as u8, (v >> 16) as u8];
                r.state.serve.pause_menu_open = (v >> 8) & 255 != 0;
                s.base_padding_04f = v as u8;
            }
            Some(
                ((s.base_flags_04c_04d[0] as u32) << 24)
                    | ((s.base_flags_04c_04d[1] as u32) << 16)
                    | (u32::from(r.state.serve.pause_menu_open) << 8)
                    | s.base_padding_04f as u32,
            )
        }
        0x58 => {
            if let Some(v) = write {
                r.state.frontend.pregame_ready_058 = (v >> 24) as u8;
            }
            Some((r.state.frontend.pregame_ready_058 as u32) << 24)
        }
        0x78 | 0x7c => scalar!(s.identities_078_0b8[0][(o - 0x78) / 4]),
        0xb8 | 0xbc => scalar!(s.identities_078_0b8[1][(o - 0xb8) / 4]),
        0x84 | 0xc4 => {
            let p = (o - 0x84) / 0x40;
            if let Some(v) = write {
                r.life.players[p].player_flag = v >> 24 != 0;
            }
            Some(u32::from(r.life.players[p].player_flag) << 24)
        }
        0x198 | 0x19c => scalar!(r.state.animations.hit_power[(o - 0x198) / 4]),
        0x1a0 | 0x1a4 => scalar!(r.state.animations.ready_power.state[(o - 0x1a0) / 4]),
        0x1a8 | 0x1ac => {
            scalar!(r.state.animations.ready_power.suppress_if_current[(o - 0x1a8) / 4])
        }
        0x1b0 | 0x1b4 => scalar!(r.state.animations.hit_zone_zero[(o - 0x1b0) / 4]),
        0x1b8 | 0x1bc => scalar!(r.state.animations.ready_zone_zero.state[(o - 0x1b8) / 4]),
        0x1c0 | 0x1c4 => {
            scalar!(r.state.animations.ready_zone_zero.suppress_if_current[(o - 0x1c0) / 4])
        }
        0x1c8 | 0x1cc => scalar!(r.state.serve.power_animations[(o - 0x1c8) / 4]),
        0x1d0 | 0x1d4 => scalar!(r.state.animations.ready_reverse.state[(o - 0x1d0) / 4]),
        0x1d8 | 0x1dc => {
            scalar!(r.state.animations.ready_reverse.suppress_if_current[(o - 0x1d8) / 4])
        }
        0x1e0 | 0x1e4 => scalar!(r.state.serve.high_animations[(o - 0x1e0) / 4]),
        0x1e8 | 0x1ec => scalar!(r.state.animations.ready_zone_one.state[(o - 0x1e8) / 4]),
        0x1f0 | 0x1f4 => {
            scalar!(r.state.animations.ready_zone_one.suppress_if_current[(o - 0x1f0) / 4])
        }
        0x328 => {
            if let Some(v) = write {
                r.state.reset.server_side_flags[2] = (v >> 8) & 255 != 0;
                r.state.reset.server_side_flags[3] = v & 255 != 0;
            }
            Some(
                (u32::from(r.state.reset.server_side_flags[2]) << 8)
                    | u32::from(r.state.reset.server_side_flags[3]),
            )
        }
        0xc0 => scalar!(r.state.reset.ai_special_case_0c0),
        0x100 => scalar!(s.asset_handle_100),
        0x104 => scalar!(r.state.reset.ball_handle_104),
        0x110..=0x118 => float!(r.state.reset.world_position_110[(o - 0x110) / 4]),
        0x120 | 0x124 => scalar!(r.state.reset.player_handles_120[(o - 0x120) / 4]),
        0x128 | 0x12c => scalar!(r.state.reset.current_ai_entities_128[(o - 0x128) / 4]),
        0x130 => {
            if let Some(v) = write {
                r.life.match_state.rotations = [(v >> 8) as u8 as i8, v as u8 as i8];
            }
            if let Some(v) = write {
                s.player_init.player_init_flags_130 = [(v >> 24) as u8, (v >> 16) as u8];
            }
            Some(
                ((s.player_init.player_init_flags_130[0] as u32) << 24)
                    | ((s.player_init.player_init_flags_130[1] as u32) << 16)
                    | ((r.life.match_state.rotations[0] as u8 as u32) << 8)
                    | r.life.match_state.rotations[1] as u8 as u32,
            )
        }
        0x138..=0x140 => scalar!(r.life.score_weights[(o - 0x138) / 4]),
        0x144..=0x168 => scalar!(r.life.statistics[(o - 0x144) / 20][((o - 0x144) % 20) / 4]),
        0x16c => scalar!(r.state.rules.mode),
        0x170 => scalar!(s.tunables_170_180[0]),
        0x174 => scalar!(r.life.distance_mode),
        0x178 => scalar!(r.state.reset.initial_rotation_178),
        0x17c => scalar!(r.state.rules.time_limit_seconds),
        0x180 => scalar!(s.tunables_170_180[1]),
        0x204 => scalar!(r.life.match_state.round_winner),
        0x348 => float!(r.life.indicator_target),
        0x440 => scalar!(r.life.match_state.match_winner),
        0x184 => scalar!(r.state.reset.pole_handle_184),
        0x188 => scalar!(s.alternate_pole_188),
        0x18c => scalar!(r.life.current_distance),
        0x190 | 0x194 => scalar!(r.life.lose_animations[(o - 0x190) / 4]),
        0x210 => scalar!(r.life.player_count),
        0x21c => scalar!(r.life.focus_player),
        0x220 => scalar!(r.state.reset.receiver_220),
        0x23c | 0x240 => float!(r.state.serve.return_angles[(o - 0x23c) / 4]),
        0x244 => {
            if let Some(v) = write {
                r.state.reset.server_side_flags[0] = v >> 24 != 0;
                r.state.reset.server_side_flags[1] = (v >> 16) & 255 != 0;
            }
            Some(
                (u32::from(r.state.reset.server_side_flags[0]) << 24)
                    | (u32::from(r.state.reset.server_side_flags[1]) << 16),
            )
        }
        0x248 | 0x24c => float!(r.state.reset.start_angles_248[(o - 0x248) / 4]),
        0x250 => scalar!(r.life.match_state.state_ms),
        0x254 => scalar!(r.life.match_state.previous_state_ms),
        0x27c | 0x280 => scalar!(r.state.reset.ai_initial_values_27c[(o - 0x27c) / 4]),
        0x2ec => scalar!(s.mode_2ec),
        0x2fc | 0x300 | 0x304 | 0x308 => {
            let (p, k, address, name) = match o {
                0x2fc => (0, 0, 0x804dcc89, "pg_tetherball_normimpact_plr1"),
                0x300 => (1, 0, 0x804dcca7, "pg_tetherball_normimpact_plr2"),
                0x304 => (0, 1, 0x804dccc5, "pg_tetherball_powerimpact_plr1"),
                _ => (1, 1, 0x804dcce4, "pg_tetherball_powerimpact_plr2"),
            };
            if let Some(v) = write {
                r.state.fx_names[p][k] = if v == 0 {
                    String::new()
                } else {
                    assert_eq!(v, address);
                    name.into()
                };
            }
            if r.state.fx_names[p][k].is_empty() {
                Some(0)
            } else {
                assert_eq!(r.state.fx_names[p][k], name);
                Some(address)
            }
        }
        0x32c => {
            if let Some(v) = write {
                r.state.hit.field_32c = v >> 24 != 0;
                r.state.hit.field_32d = (v >> 16) & 255 != 0;
                r.life.hud_ready = (v >> 8) & 255 != 0;
                r.life.field_32f = v & 255 != 0;
            }
            Some(
                (u32::from(r.state.hit.field_32c) << 24)
                    | (u32::from(r.state.hit.field_32d) << 16)
                    | (u32::from(r.life.hud_ready) << 8)
                    | u32::from(r.life.field_32f),
            )
        }
        0x334 => scalar!(r.state.reset.field_334_guid),
        0x424 => {
            if let Some(v) = write {
                r.state.frontend.field_424 = (v >> 24) as u8;
            }
            Some((r.state.frontend.field_424 as u32) << 24)
        }
        0x340 => scalar!(s.pole_glow_340),
        0x34c..=0x358 => float!(r.state.reset.round_tunables_34c_358[(o - 0x34c) / 4]),
        0x360..=0x38c => float!(
            r.state.hit.indicator_angles_360_36c_378_384[(o - 0x360) / 12][((o - 0x360) % 12) / 4]
        ),
        0x390 => scalar!(r.state.reset.camera_handle_390),
        0x394 => float!(r.state.reset.camera_heading_394),
        0x398..=0x3d4 => float!(r.state.reset.world_matrix_398[(o - 0x398) / 4]),
        0x3d8..=0x414 => float!(s.inverse_world_3d8[(o - 0x3d8) / 4]),
        0x418 => scalar!(r.life.round_number),
        0x430 => {
            if let Some(v) = write {
                r.state.reset.rotation_limit_430 = v as i32;
            }
            scalar!(r.state.rules.rotation_limit)
        }
        0x434 => scalar!(r.state.rules.wins_required),
        0x438 => scalar!(r.life.total_rounds),

        _ => None,
    }
}

struct Host<'a> {
    fixture: &'a Value,
    case: &'a Value,
    events: Vec<Value>,
    camera_words: [u32; 2],
    visible: [u8; 2],
    renderer: u32,
    ancient: u8,
    character_activity: [u8; 2],
    ai_ball_handles: [u32; 2],
    physical_area: i32,
    camera_target: [u32; 3],
}
impl crate::tetherball_shadow_setup::ShadowSetupServices for Host<'_> {
    fn set_shadow_viewport(
        &mut self,
        mode: i32,
        options: crate::tetherball_shadow_setup::ShadowViewOptions,
    ) {
        self.events.push(json!([
            "set_viewport",
            0x7210f000u32,
            mode,
            options.native_words()
        ]));
    }
}
// Native SetPlayerDistance(2) is a no-op in startup and state 1 has no entry
// effects. Any unexpected engine call through these composed bodies fails.
macro_rules! forbidden {($name:ident($($arg:ident:$ty:ty),*))=>{
    fn $name(&mut self,$($arg:$ty),*){let _=($($arg,)*);panic!("unexpected startup lifecycle call {}",stringify!($name));}
};}
impl crate::tetherball_lifecycle::Services for Host<'_> {
    forbidden!(scoreboard(v:[i32;4]));
    forbidden!(round(v:[i32;3]));
    forbidden!(serve_bubble(v:[i32;3]));
    fn mega_visible(&mut self, p: i32, v: i32) {
        self.events.push(json!(["mega_visible", p, v]));
    }
    forbidden!(mega_value(p:i32,v:i32));
    forbidden!(winner_visible(ui:crate::tetherball_match::WinnerUi,p:i32,v:bool));
    forbidden!(sound(f:bool,s:i32,v:i32,volume:i32));
    forbidden!(controller_pop(c:i32));
    forbidden!(controller_set(c:i32,s:i32));
    forbidden!(animation(p:usize,s:i32,f:bool,b:i32));
    forbidden!(switch_to_ai(p:usize));
    forbidden!(camera_offset(t:bool,p:[f32;3],ms:u32));
    forbidden!(wrap_particle(p:usize,v:[f32;3],ms:i32));
    forbidden!(fade_in(v:i32));
    fn clear_hud(&mut self) {
        self.events.push(json!(["clear_hud"]));
    }
    fn close_screen(&mut self) {
        self.events.push(json!(["close_screen"]));
    }
    forbidden!(reset_round(l:&mut crate::tetherball_lifecycle::Lifecycle,b:&mut crate::tetherball::BallMotion));
    forbidden!(post_game(k:i32,w:&[u32;70]));
    fn random_range(&mut self, _lo: i32, _hi: i32) -> i32 {
        panic!("unexpected startup random query")
    }
}
impl Host<'_> {
    fn handle(&self, name: &str) -> u32 {
        u(&self.fixture["handles"][name])
    }
}
impl crate::tetherball_player_init::PlayerInitServices for Host<'_> {
    fn get_player_character(
        &mut self,
        player: i32,
    ) -> Option<crate::tetherball_player_init::ExistingCharacter> {
        let input = &self.case["input"]["character"];
        let handle = if input["existing"].as_bool().unwrap() {
            u(&self.fixture["handles"]["players"][0])
        } else {
            0
        };
        self.events
            .push(json!(["get_player_character", player, handle]));
        (handle != 0).then(|| crate::tetherball_player_init::ExistingCharacter {
            handle,
            identity_words: std::array::from_fn(|i| u(&input["identity"][i])),
        })
    }
    fn spawn_character(&mut self, call: crate::tetherball_player_init::SpawnCharacterCall) -> u32 {
        let player = if call.identity_words_r5_r6[0] == u(&self.case["initial_game_words"]["0x78"])
        {
            0
        } else {
            1
        };
        let handle = u(&self.fixture["handles"]["players"][player]);
        self.events.push(json!([
            "spawn_character",
            call.identity_words_r5_r6,
            call.position_r7.map(f32::to_bits),
            call.arg8,
            call.arg9,
            call.arg10,
            call.stack_words,
            handle
        ]));
        handle
    }
    fn set_character_state_position(&mut self, character: u32, p: [f32; 3]) {
        self.events.push(json!([
            "set_character_state_position",
            character,
            p.map(f32::to_bits)
        ]));
    }
    fn set_character_state_direction(&mut self, character: u32, p: [f32; 3]) {
        self.events.push(json!([
            "set_character_state_direction",
            character,
            p.map(f32::to_bits)
        ]));
    }
    fn allocate_ai_slot(&mut self) -> u32 {
        let count = self
            .events
            .iter()
            .filter(|e| e[0] == "allocate_ai_slot")
            .count();
        let handle = u(&self.fixture["handles"]["ai"][count]);
        self.events.push(json!(["allocate_ai_slot", handle]));
        handle
    }
    fn construct_tetherball_ai(&mut self, entity: u32, character: u32) {
        self.events
            .push(json!(["construct_tetherball_ai", entity, character]));
    }
    fn add_ai_entity(&mut self, entity: u32) {
        self.events.push(json!(["add_ai_entity", entity]));
    }
    fn set_character_ai_entity(&mut self, character: u32, entity: u32) {
        self.events
            .push(json!(["set_character_ai_entity", character, entity]));
    }
    fn set_ai_ball_handle(&mut self, entity: u32, ball: u32) {
        let player = self.fixture["handles"]["ai"]
            .as_array()
            .unwrap()
            .iter()
            .position(|v| u(v) == entity)
            .unwrap();
        self.ai_ball_handles[player] = ball;
    }
    fn multiplayer_enable_byte(&mut self) -> u8 {
        u8::from(self.case["input"]["logic"]["multi_flag"].as_bool().unwrap())
    }
    fn setup_multiplayer_ability(&mut self) {
        self.events.push(json!(["setup_multiplayer_ability"]));
    }
    fn setup_single_player_ability(&mut self) {
        self.events.push(json!(["setup_single_player_ability"]));
    }
    fn player_info_handle(&mut self, character: u32) -> u32 {
        character + 0x1000
    }
    fn controller_index(&mut self, info: u32) -> i32 {
        if info == u(&self.fixture["handles"]["players"][0]) + 0x1000 {
            0
        } else {
            1
        }
    }
    fn controller_handle(&mut self, index: i32) -> u32 {
        StartupServices::controller_handle(self, index)
    }
    fn set_controller_state(&mut self, controller: u32, state: i32) {
        self.events
            .push(json!(["set_controller_state", controller, state]));
    }
}
impl crate::tetherball_ball_init::BallInitServices for Host<'_> {
    fn pole_height(&mut self, pole: u32) -> f32 {
        assert_eq!(pole, self.handle("pole"));
        assert_eq!(self.events.last().unwrap(), &json!(["destroy_collection"]));
        f(&self.case["input"]["late_pole_height"])
    }
    fn texture(&mut self, name: &str) -> crate::tetherball_ball_init::LoadedAsset {
        self.ball_asset(name, true)
    }
    fn model(&mut self, name: &str, flags: i32) -> crate::tetherball_ball_init::LoadedAsset {
        assert_eq!(flags, 0);
        self.ball_asset(name, false)
    }
    fn set_textures(&mut self, model: u32, texture: u32) {
        self.events.push(json!(["textures", model, texture]));
    }
    fn allocate(&mut self, bytes: u32, tag: &str) -> u32 {
        let i = self
            .events
            .iter()
            .filter(|e| e[0] == "allocate" && e[1] == 0x4c)
            .count();
        let null = self.case["input"]["null_shadows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| u(v) as usize == i);
        let handle = if null {
            0
        } else {
            0x73000000 + i as u32 * 0x100
        };
        self.events.push(json!([
            "allocate",
            bytes,
            self.fixture["pool"],
            0,
            tag,
            handle
        ]));
        handle
    }
    fn construct_cached_model(&mut self, handle: u32) {
        self.events.push(json!(["cached_ctor", handle]));
    }
    fn bind_cached_model(&mut self, cached: u32, model: u32) {
        self.events.push(json!(["scale", cached, model]));
    }
    fn construct_shadow(&mut self, handle: u32, model: u32) {
        self.events.push(json!(["shadow_ctor", handle, model]));
    }
    fn add_scene_entity(&mut self, layer: i32, entity: u32) {
        self.events.push(json!(["add_entity", layer, entity]));
    }
    fn database_key(&mut self, name: &str) -> u64 {
        StartupServices::database_key(self, name)
    }
    fn collection(&mut self, class: u64, name: u64) -> u32 {
        StartupServices::collection(self, class, name)
    }
    fn float_from_array(&mut self, collection: u32, field: &str, index: u32) -> f32 {
        StartupServices::float_array(self, collection, field, index)
    }
    fn destroy_collection(&mut self, collection: u32) {
        StartupServices::destroy_collection(self, collection)
    }
}
impl Host<'_> {
    fn ball_asset(
        &mut self,
        name: &str,
        texture: bool,
    ) -> crate::tetherball_ball_init::LoadedAsset {
        let slot = match name {
            "teatherball.gsh" => 7,
            "teatherball.o" => 5,
            "teatherball_shadow.o" => 6,
            "teatherball_rope.gsh" => 12,
            "teatherball_rope.o" => 10,
            "teatherball_rope_shadow.o" => 11,
            _ => panic!("unknown asset"),
        };
        let id = 1000 + slot;
        let handle = 0x72000000 + slot * 0x100;
        self.events.push(if texture {
            json!(["texture", name, id, handle])
        } else {
            json!(["model", name, id, handle, 0])
        });
        crate::tetherball_ball_init::LoadedAsset { id, handle }
    }
}
impl crate::tetherball_ai::AiInitServices for Host<'_> {
    fn database_key(&mut self, name: &str) -> u64 {
        StartupServices::database_key(self, name)
    }
    fn collection(&mut self, class: u64, name: u64) -> Result<u32, String> {
        Ok(StartupServices::collection(self, class, name))
    }
    fn byte_array(&mut self, _collection: u32, field: &str, index: u32) -> Result<u8, String> {
        let slot = crate::tetherball_tuning::AI_FIELDS
            .iter()
            .position(|n| *n == field)
            .unwrap();
        let value = u(&self.case["input"]["ai_tuning"][slot]) as u8;
        self.events.push(json!(["byte", field, index, value]));
        Ok(value)
    }
    fn destroy_collection(&mut self, collection: u32) {
        StartupServices::destroy_collection(self, collection)
    }
}
impl crate::minigame_entry::PregameServices for Host<'_> {
    fn setup_pregame_handlers(&mut self, kind: i32, mode: i32, words: [u32; 4]) {
        self.events
            .push(json!(["pregame_handlers", kind, mode, words]));
    }
    fn open_pregame_screen(&mut self, name: &str) {
        self.events.push(json!(["pregame_screen", name]));
    }
    fn pregame_fade_renders(&mut self, first: bool, second: bool) {
        self.events.push(json!(["pregame_fade", first, second]));
    }
}
impl crate::tetherball_server::ServerServices for Host<'_> {
    fn random_server(&mut self) -> i32 {
        let value = self.case["input"]["server_random"].as_i64().unwrap() as i32;
        self.events.push(json!(["server_random", 0, 1, value]));
        value
    }
    fn server_camera(&mut self) -> u32 {
        StartupServices::camera(self, 0)
    }
    fn camera_target(&mut self, _camera: u32, p: [f32; 3]) {
        self.camera_target = p.map(f32::to_bits);
    }
    fn camera_direction(&mut self, camera: u32, p: [f32; 3]) {
        StartupServices::effect(self, Effect::CameraDirection { camera, value: p });
    }
    fn server_animation(&mut self, p: usize, state: i32, force: bool, blend: i32) {
        self.events
            .push(json!(["animation", p, state, force, blend]));
    }
    fn marker_count(&mut self, p: usize) -> i32 {
        assert_eq!(self.events.last().unwrap()[0], "animation");
        assert_eq!(u(&self.events.last().unwrap()[1]) as usize, 1 - p);
        self.case["input"]["server_marker_counts"][p]
            .as_i64()
            .unwrap() as i32
    }
    fn marker_id(&mut self, p: usize, index: i32) -> i32 {
        let id = self.case["input"]["server_markers"][p][index as usize]["id"]
            .as_i64()
            .unwrap() as i32;
        self.events.push(json!(["marker_id", p, index, id]));
        id
    }
    fn marker_matrix(&mut self, p: usize, index: i32) -> u32 {
        let value = u(&self.case["input"]["server_markers"][p][index as usize]["matrix"]);
        self.events.push(json!(["marker_matrix", p, index, value]));
        value
    }
    fn null_trail(&mut self) -> u32 {
        u(&self.case["input"]["null_trail"])
    }
    fn destroy_trail(&mut self, handle: u32, fade: i32) {
        self.events.push(json!(["particle_destroy", handle, fade]));
    }
}
impl StartupServices for Host<'_> {
    fn server_services(&mut self) -> &mut impl crate::tetherball_server::ServerServices {
        self
    }
    fn pregame_services(&mut self) -> &mut impl crate::minigame_entry::PregameServices {
        self
    }
    fn ai_services(&mut self) -> &mut impl crate::tetherball_ai::AiInitServices {
        self
    }
    fn ai_allocation_direction_scale(&mut self, entity: u32) -> f32 {
        let player = self.fixture["handles"]["ai"]
            .as_array()
            .unwrap()
            .iter()
            .position(|v| u(v) == entity)
            .unwrap();
        f(&self.case["input"]["ai_initial_scales"][player])
    }
    fn ai_game_context(&mut self, runtime: &Runtime) -> Option<(i32, i32)> {
        let present = self.case["input"]["ai_game_present"].as_bool().unwrap();
        self.events.push(json!(["ai_game", present]));
        present.then_some((runtime.life.session_mode, runtime.life.game_type))
    }
    fn ball_services(&mut self) -> &mut impl crate::tetherball_ball_init::BallInitServices {
        self
    }
    fn player_services(&mut self) -> &mut impl crate::tetherball_player_init::PlayerInitServices {
        self
    }
    fn effect(&mut self, e: Effect) {
        let event = match e {
            Effect::MinigameArea {
                abstract_area,
                physical_area,
            } => {
                self.physical_area = physical_area;
                json!([
                    "placeable_area",
                    u(&self.fixture["world_services"][0]) + 0x1b0,
                    abstract_area
                ])
            }

            Effect::CameraViewInfo { camera, flag } => json!(["camera_view_info", camera, flag]),
            Effect::CameraReinitialize {
                manager,
                camera_type,
                player,
            } => json!(["camera_reinitialize", manager, camera_type, player]),
            Effect::ControllerReinitialize {
                controller,
                control_type,
            } => json!(["controller_reinitialize", controller, control_type]),
            Effect::HomeMenu(v) => json!([if v { "home_enable" } else { "home_disable" }]),
            Effect::ResetHomeIcon => json!(["home_icon"]),
            Effect::SyncTask(true) => json!(["sync_add", 0x803ad79cu32 as i32, 0, 0]),
            Effect::SyncTask(false) => json!(["sync_del", 0x803ad79cu32 as i32]),
            Effect::VSync(v) => json!(["vsync", i32::from(v)]),
            Effect::AncientEvilByte(v) => {
                self.ancient = v;
                return;
            }
            Effect::PlaceableVisible { handle, visible } => {
                let p = if handle == self.handle("pole") {
                    0
                } else {
                    assert_eq!(handle, self.handle("alternate_pole"));
                    1
                };
                self.visible[p] = u8::from(visible);
                return;
            }
            Effect::CameraOffset {
                camera,
                target,
                desired_ms,
                value,
            } => {
                let name = match (target, desired_ms.is_some()) {
                    (false, false) => "camera_position",
                    (false, true) => "camera_desired_position",
                    (true, false) => "camera_target",
                    (true, true) => "camera_desired_target",
                };
                let mut e = vec![json!(name), json!(camera), json!(value.map(f32::to_bits))];
                if let Some(ms) = desired_ms {
                    e.push(json!(ms));
                }
                json!(e)
            }
            Effect::CameraBackwards { camera, value } => {
                json!(["camera_backwards", camera, value.to_bits()])
            }
            Effect::CameraScalars {
                camera,
                height_39c,
                value_3a0,
            } => {
                assert_eq!(camera, self.handle("camera"));
                self.camera_words = [height_39c.to_bits(), value_3a0.to_bits()];
                return;
            }
            Effect::CameraRotation {
                camera,
                value,
                milliseconds,
            } => json!(["camera_rotation", camera, value.to_bits(), milliseconds]),
            Effect::CameraStart { camera, value } => {
                json!(["camera_start", camera, value.map(f32::to_bits)])
            }
            Effect::CameraDirection { camera, value } => {
                json!(["camera_direction", camera, value.map(f32::to_bits)])
            }
            Effect::SwitchToAi(c) => json!(["switch_ai", c]),
            Effect::RegisterCallback { name, address } => {
                json!(["callback", name, address, self.handle("game")])
            }
            Effect::LoadAudio(v) => json!(["audio_load", v]),
            Effect::PlayMusic(v) => json!(["music", v]),
            Effect::RendererWord1c0(v) => {
                self.renderer = v;
                return;
            }
        };
        self.events.push(event);
    }
    fn game_logic_inputs(
        &mut self,
        _runtime: &Runtime,
    ) -> Result<crate::tetherball_initialize::InitGameLogicInputs, String> {
        use crate::tetherball_initialize::{InitGameLogicInputs, NativeTunables};
        let input = &self.case["input"]["logic"];
        let tunables = if input["multi_flag"].as_bool().unwrap() {
            self.events.push(json!(["multi_tunables"]));
            NativeTunables::MultiPlayer {
                variant: u(&input["multi"][0]) as i32,
                game_mode: u(&input["multi"][1]) as i32,
                rounds: u(&input["multi"][2]) as i32,
                rotations_to_win: u(&input["multi"][3]) as i32,
            }
        } else {
            self.events.push(json!(["single_tunables"]));
            NativeTunables::SinglePlayer {
                fields: std::array::from_fn(|i| u(&input["single"][i]) as i32),
            }
        };
        self.events.push(json!(["reset_stats"]));
        Ok(InitGameLogicInputs {
            tunables,
            score_weights: std::array::from_fn(|i| u(&input["scores"][i]) as i32),
        })
    }
    fn world_service_handles(&mut self, world: u32) -> Result<[u32; 6], String> {
        assert_eq!(world, self.handle("world"));
        Ok(std::array::from_fn(|i| {
            u(&self.fixture["world_services"][i])
        }))
    }
    fn camera_manager(&mut self) -> u32 {
        u(&self.fixture["handles"]["camera_manager"])
    }
    fn current_camera(&mut self, manager: u32) -> u32 {
        assert_eq!(manager, self.camera_manager());
        self.handle("camera")
    }
    fn controller_handle(&mut self, index: i32) -> u32 {
        let handle = u(&self.fixture["handles"]["controllers"][index as usize]);
        self.events.push(json!(["controller_get", index, handle]));
        handle
    }
    fn null_ball_trail(&mut self) -> u32 {
        u(&self.case["input"]["null_trail"])
    }
    fn allocate_ball(&mut self, bytes: u32, tag: &str) -> u32 {
        let handle = self.handle("ball");
        self.events.push(json!([
            "allocate",
            bytes,
            self.fixture["pool"],
            0,
            tag,
            handle
        ]));
        handle
    }
    fn placeable(&mut self, name: &str) -> (u32, [f32; 3]) {
        let h = self.handle(if name.ends_with("_w_ball") {
            "alternate_pole"
        } else {
            "pole"
        });
        self.events.push(json!(["placeable", name, h]));
        (h, vector(&self.case["input"]["pole_position"]))
    }
    fn camera(&mut self, player: u32) -> u32 {
        let h = self.handle("camera");
        self.events.push(json!(["camera_get", player, h]));
        h
    }
    fn load_bigfile(&mut self, name: &str, load: bool, pool: u32) -> u32 {
        self.events
            .push(json!(["load_bigfile", name, u32::from(load), pool, 777]));
        777
    }
    fn ground_height(&mut self, p: [f32; 3], range: f32) -> f32 {
        let value = f(&self.case["input"]["ground_height"]);
        self.events.push(json!([
            "ground_height",
            p.map(f32::to_bits),
            range.to_bits(),
            value.to_bits()
        ]));
        value
    }
    fn create_particle(&mut self, name: &str, p: [f32; 3]) -> u32 {
        self.events
            .push(json!(["particle_create", name, p.map(f32::to_bits), 888]));
        888
    }
    fn database_key(&mut self, name: &str) -> u64 {
        self.events.push(json!(["key", name]));
        match name {
            "mg_tetherball" => 1,
            "tunables" => 2,
            "dares_speed_rounds" => 3,
            "dares_time" => 4,
            "dares_endurance" => 5,
            _ => panic!("unknown collection key {name}"),
        }
    }
    fn collection(&mut self, class: u64, name: u64) -> u32 {
        assert_eq!(class, 1);
        let name = match name {
            2 => "tunables",
            3 => "dares_speed_rounds",
            4 => "dares_time",
            5 => "dares_endurance",
            _ => panic!("unknown collection token"),
        };
        self.events
            .push(json!(["collection", "mg_tetherball", name]));
        0x72106000
    }
    fn float_array(&mut self, collection: u32, field: &str, index: u32) -> f32 {
        assert_eq!(collection, 0x72106000);
        let slot = [
            "ball_basehitspeed",
            "ball_acceleratemodifier",
            "ball_powermodifier",
            "ball_megamodifier",
        ]
        .iter()
        .position(|n| *n == field)
        .unwrap();
        let value = f(&self.case["input"]["tuning"][slot]);
        self.events
            .push(json!(["float", field, index, value.to_bits()]));
        value
    }
    fn int16_array(&mut self, collection: u32, field: &str, index: u32) -> i16 {
        assert_eq!(collection, 0x72106000);
        let table = crate::tetherball_tuning::ANGLE_FIELDS
            .iter()
            .position(|n| *n == field)
            .unwrap();
        let value = self.case["input"]["angle_degrees"][table][index as usize]
            .as_i64()
            .unwrap();
        self.events.push(json!(["int16", field, index, value]));
        value as i16
    }
    fn destroy_collection(&mut self, collection: u32) {
        assert_eq!(collection, 0x72106000);
        self.events.push(json!(["destroy_collection"]));
    }
}

#[test]
fn original_enclosing_startup() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/tetherball_startup_golden.json")).unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    for c in fixture["cases"].as_array().unwrap() {
        let mut r = runtime_seed();
        let mut s = startup_seed();
        for (o, v) in c["initial_game_words"].as_object().unwrap() {
            let off = usize::from_str_radix(&o[2..], 16).unwrap();
            word(&mut r, &mut s, off, Some(u(v)));
        }
        r.ball = crate::tetherball::tests::ball(&c["initial_ball"]);
        r.state.serve.forced_ai = std::array::from_fn(|i| u(&c["input"]["forced_ai"][i]) != 0);
        let mut host = Host {
            fixture: &fixture,
            case: c,
            events: vec![],
            camera_words: [0; 2],
            visible: [0x25, 0x73],
            renderer: 0,
            ancient: 1,
            character_activity: [0; 2],
            ai_ball_handles: [0; 2],
            physical_area: i32::MIN,
            camera_target: [0; 3],
        };
        initialize(
            &mut r,
            &mut s,
            StartupInput {
                world: u(&fixture["handles"]["world"]),
                camera_position_offset: vector(&fixture["camera_position_offset"]),
                camera_target_offset: vector(&fixture["camera_target_offset"]),
            },
            &mut host,
        )
        .unwrap();
        assert_eq!(
            json!(host.events),
            c["effects"],
            "{} call order",
            c["label"]
        );
        let mut actual = c["initial_game_words"].clone();
        for (o, v) in actual.as_object_mut().unwrap() {
            let off = usize::from_str_radix(&o[2..], 16).unwrap();
            if let Some(value) = word(&mut r, &mut s, off, None) {
                *v = json!(value);
            }
        }
        assert_eq!(
            actual, c["expected_game_words"],
            "{} game stores",
            c["label"]
        );
        assert_eq!(json!(host.ai_ball_handles), c["expected_ai_ball_handles"]);
        assert_eq!(json!(host.physical_area), c["expected_physical_area"]);
        assert_eq!(
            json!(host.camera_target),
            c["expected_server_camera_target"]
        );
        assert_eq!(
            json!([
                r.state.reset.current_ball_owner_074,
                r.state.reset.current_ball_matrix_078
            ]),
            c["expected_ball_attachment"]
        );
        assert_eq!(
            json!(r.state.serve.frontend_flags),
            c["expected_frontend_flags"]
        );
        assert_eq!(json!(host.visible), c["expected_placeable_visible"]);
        assert_eq!(
            json!({"0x39c":host.camera_words[0],"0x3a0":host.camera_words[1]}),
            c["expected_camera_words"]
        );
        for p in 0..2 {
            let ai = r.state.ai[p].as_ref().unwrap();
            assert_eq!(
                json!({"difficulty":ai.difficulty,"enabled":ai.enabled,"heading":ai.heading.to_bits(),"charge":r.state.rally.ai_charge[p]}),
                c["expected_ai_configuration"][p]
            );
            assert_eq!(
                json!({"0x64":ai.angle.to_bits(),"0x68":ai.direction_scale.to_bits(),"0x74":r.life.players[p].ai_distance}),
                c["expected_ai_words"][p]
            );
        }
        crate::tetherball::tests::assert_bits(
            &r.ball,
            &crate::tetherball::tests::ball(&c["expected_ball"]),
            0,
            0,
        );
        assert_eq!(json!(r.state.scene.trails), c["expected_ball_trails"]);
        let resources = s.ball_resources.as_ref().unwrap();
        let [
            ball_model,
            ball_shadow_model,
            ball_texture,
            rope_model,
            rope_shadow_model,
            rope_texture,
        ] = resources.asset_ids;
        assert_eq!(
            json!([
                resources.shadows[0],
                resources.shadows[1],
                resources.cached[0],
                resources.cached[1],
                ball_model,
                ball_shadow_model,
                ball_texture,
                resources.cached[2],
                resources.cached[3],
                rope_model,
                rope_shadow_model,
                rope_texture
            ]),
            c["expected_ball_resources"]
        );
        assert_eq!(
            json!(resources.accelerate_modifier_158.to_bits()),
            c["expected_ball_accelerate_modifier"]
        );
        assert_eq!(json!(resources.flag_15c), c["expected_ball_flag_15c"]);
        assert_eq!(r.state.scene.ball_shadow, resources.shadows[0] != 0);
        assert_eq!(r.state.scene.rope_shadow, resources.shadows[1] != 0);
        assert_eq!(json!(r.ball.radius.to_bits()), c["expected_ball_radius"]);
        assert_eq!(
            json!(r.ball.desired_radius.to_bits()),
            c["expected_ball_desired_radius"]
        );
        assert_eq!(json!(host.renderer), c["expected_renderer_word_1c0"]);
        assert_eq!(json!(host.ancient), c["expected_ancient_byte"]);
        host.events.clear();
        uninitialize_base(&mut r, &mut s, &mut host);
        assert_eq!(json!(host.events), c["uninitialize_effects"]);
        let mut actual = c["expected_game_words"].clone();
        for (o, v) in actual.as_object_mut().unwrap() {
            let off = usize::from_str_radix(&o[2..], 16).unwrap();
            if let Some(value) = word(&mut r, &mut s, off, None) {
                *v = json!(value);
            }
        }
        assert_eq!(actual, c["uninitialized_game_words"]);
    }
}

impl crate::tetherball_ball_init::BallCleanupServices for Host<'_> {
    fn cleanup_null_trail(&mut self) -> u32 {
        u(&self.case["input"]["null_trail"])
    }
    fn cleanup_destroy_fx(&mut self, handle: u32, fade: i32) {
        self.events.push(json!(["destroy_fx", handle, fade]));
    }
    fn cleanup_cached_model(&mut self, handle: u32) {
        self.events.push(json!(["cached_destroy", handle, 1]));
    }
    fn cleanup_remove_shadow(&mut self, layer: i32, handle: u32) {
        self.events.push(json!(["remove_shadow", layer, handle]));
    }
    fn cleanup_shadow_entity(&mut self, handle: u32) {
        self.events.push(json!(["shadow_destroy", handle, 1]));
    }
}
impl crate::tetherball_cleanup::CleanupServices for Host<'_> {
    fn cleanup_effect(&mut self, effect: crate::tetherball_cleanup::CleanupEffect) {
        use crate::tetherball_cleanup::CleanupEffect::*;
        let event = match effect {
            DespawnCharacter { character, destroy } => json!(["despawn", character, destroy]),
            PopController { controller } => json!(["controller_pop_handle", controller]),
            DeleteBigfileAssets(handle) => json!(["delete_assets", handle]),
            FreeBall(handle) => json!(["free_ball", handle]),
            ResetWorldPlayerAnimation(character) => {
                let p = self.fixture["handles"]["players"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|v| u(v) == character)
                    .unwrap();
                json!([
                    "animation_reset",
                    self.fixture["handles"]["animations"][p],
                    0,
                    0,
                    u32::MAX
                ])
            }
            SwitchToLocalControl(character) => json!(["local_control", character]),
            ClearCharacterActivityFlag(character) => {
                let p = self.fixture["handles"]["players"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|v| u(v) == character)
                    .unwrap();
                self.character_activity[p] = 0;
                return;
            }
            PurgeCallbacks => json!(["purge_callbacks"]),
            TimerVisible(v) => json!(["timer_visible", v]),
            PoleIndicator(v) => json!(["indicator", v.to_bits()]),
            UnduckMusic => json!(["unduck_music"]),
            StopMusic => json!(["stop_music"]),
            UnloadAudio(v) => json!(["unload_audio", v]),
            RestoreArea => json!(["restore_area"]),
        };
        self.events.push(event);
    }
    fn world_player_character(&mut self, index: i32) -> u32 {
        let handle = u(&self.case["cleanup"]["world_player"]);
        self.events.push(json!(["world_player", index, handle]));
        handle
    }
    fn character_local_control_block(&mut self, character: u32) -> u8 {
        let p = self.fixture["handles"]["players"]
            .as_array()
            .unwrap()
            .iter()
            .position(|v| u(v) == character)
            .unwrap();
        u(&self.case["cleanup"]["local_block"][p]) as u8
    }
    fn cleanup_null_game_fx(&mut self) -> u32 {
        u(&self.case["cleanup"]["null_game_fx"])
    }
}

#[test]
fn original_complete_cleanup() {
    use crate::tetherball_ball_init::{BallResources, uninitialize_ball};
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/tetherball_cleanup_golden.json")).unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    for (index, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let mut r = runtime_seed();
        let mut s = startup_seed();
        for (o, v) in c["initial_game_words"].as_object().unwrap() {
            word(
                &mut r,
                &mut s,
                usize::from_str_radix(&o[2..], 16).unwrap(),
                Some(u(v)),
            );
        }
        r.ball = crate::tetherball::tests::ball(&c["initial_ball"]);
        r.state.scene.trails = std::array::from_fn(|i| u(&c["cleanup"]["trails"][i]));
        r.state.scene.null_trail = u(&c["input"]["null_trail"]);
        let b = &c["initial_ball_resources"];
        let resources = BallResources {
            shadows: [u(&b[0]), u(&b[1])],
            cached: [u(&b[2]), u(&b[3]), u(&b[7]), u(&b[8])],
            asset_ids: [u(&b[4]), u(&b[5]), u(&b[6]), u(&b[9]), u(&b[10]), u(&b[11])],
            accelerate_modifier_158: 0.,
            flag_15c: 1,
        };
        r.state.scene.ball_shadow = resources.shadows[0] != 0;
        r.state.scene.rope_shadow = resources.shadows[1] != 0;
        let mut host = Host {
            fixture: &fixture,
            case: c,
            events: vec![],
            camera_words: [0; 2],
            visible: [1, 0],
            renderer: 1,
            ancient: 0,
            character_activity: std::array::from_fn(|i| u(&c["cleanup"]["activity"][i]) as u8),
            ai_ball_handles: [0; 2],
            physical_area: i32::MIN,
            camera_target: [0; 3],
        };
        // Check the composed ball resource stores separately before the enclosing
        // routine releases ownership, using the same original nested snapshot.
        let mut ball_resources = resources.clone();
        let mut scene = r.state.scene.clone();
        uninitialize_ball(&mut ball_resources, &mut scene, &mut host);
        let [b, bs, t, rope, rs, rt] = ball_resources.asset_ids;
        assert_eq!(
            json!([
                ball_resources.shadows[0],
                ball_resources.shadows[1],
                ball_resources.cached[0],
                ball_resources.cached[1],
                b,
                bs,
                t,
                ball_resources.cached[2],
                ball_resources.cached[3],
                rope,
                rs,
                rt
            ]),
            c["expected_ball_resources"]
        );
        assert_eq!(json!(ball_resources.flag_15c), c["expected_flag_15c"]);
        host.events.clear();
        s.ball_resources = Some(resources);
        crate::tetherball_cleanup::uninitialize(&mut r, &mut s, &mut host).unwrap();
        assert_eq!(
            json!(host.events),
            c["effects"],
            "cleanup effect order {index}"
        );
        let mut actual = c["initial_game_words"].clone();
        for (o, v) in actual.as_object_mut().unwrap() {
            if let Some(value) = word(
                &mut r,
                &mut s,
                usize::from_str_radix(&o[2..], 16).unwrap(),
                None,
            ) {
                *v = json!(value);
            }
        }
        assert_eq!(actual, c["expected_game_words"], "cleanup stores {index}");
        assert_eq!(json!(host.character_activity), c["expected_activity"]);
        assert_eq!(json!(r.state.scene.trails), c["expected_trails"]);
        assert_eq!(json!(host.visible), c["expected_visible"]);
        assert_eq!(host.renderer, 0);
        assert_eq!(host.ancient, 1);
        assert!(!r.state.scene.ball_shadow && !r.state.scene.rope_shadow);
        assert!(s.ball_resources.is_none());
        crate::tetherball::tests::assert_bits(
            &r.ball,
            &crate::tetherball::tests::ball(&c["initial_ball"]),
            index,
            0,
        );
    }
}

#[test]
fn original_constructor_runtime_projection() {
    use crate::tetherball_constructor::{ConstructorInputs, construct_runtime_fields};
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/data/tetherball_constructor_golden.json"
    ))
    .unwrap();
    for c in fixture["projection_cases"].as_array().unwrap() {
        let mut r = runtime_seed();
        let mut s = startup_seed();
        for (o, v) in c["initial_game_words"].as_object().unwrap() {
            word(
                &mut r,
                &mut s,
                usize::from_str_radix(&o[2..], 16).unwrap(),
                Some(u(v)),
            );
        }
        let prior_ball = r.ball;
        let input = ConstructorInputs {
            scene: u(&c["input"]["scene"]),
            base_tag: std::array::from_fn(|i| u(&c["input"]["base_tag"][i]) as u8),
            invalid_game_fx: u(&c["input"]["invalid_game_fx"]),
            score_weights: std::array::from_fn(|i| u(&c["input"]["score_weights"][i]) as i32),
        };
        construct_runtime_fields(&mut r, &mut s, input);
        for (o, v) in c["expected_game_words"].as_object().unwrap() {
            let offset = usize::from_str_radix(&o[2..], 16).unwrap();
            if let Some(value) = word(&mut r, &mut s, offset, None) {
                assert_eq!(value, u(v), "constructed owner at {offset:#x}");
            }
        }
        crate::tetherball::tests::assert_bits(&r.ball, &prior_ball, 0, 0);
        // Project arbitrary native session records into the existing live owners.
        let sessions: Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_session_golden.json"))
                .unwrap();
        for native in sessions["cases"].as_array().unwrap() {
            let a = &native["input"];
            let session = crate::tetherball_session::SessionInputs {
                level: a["level"].as_i64().unwrap() as i32,
                difficulty: a["difficulty"].as_i64().unwrap() as i32,
                dare_type: a["dare_type"].as_i64().unwrap() as i32,
                rules: u(&a["rules"]),
                game_id: u(&a["game_id"]),
                team_count: a["team_count"].as_i64().unwrap() as i32,
                participants: std::array::from_fn(|i| u(&a["participants"][i]) as u8),
            };
            crate::tetherball_session::configure_runtime(&mut r, &mut s, &session).unwrap();
            let expected = &native["expected_bytes"];
            for offset in [
                0x38, 0x3c, 0x44, 0x48, 0x70, 0x78, 0x7c, 0xb8, 0xbc, 0xc0, 0xf8,
            ] {
                let bytes: [u8; 4] = std::array::from_fn(|i| u(&expected[offset + i]) as u8);
                assert_eq!(
                    word(&mut r, &mut s, offset, None).unwrap(),
                    u32::from_be_bytes(bytes),
                    "session owner {offset:#x}"
                );
            }
            for (player, offset) in [0x84, 0xc4].into_iter().enumerate() {
                assert_eq!(
                    r.life.players[player].player_flag,
                    u(&expected[offset]) != 0
                );
            }
            crate::tetherball::tests::assert_bits(&r.ball, &prior_ball, 0, 0);
        }
        assert!(r.state.gestures.pending.is_empty());
        assert_eq!(
            r.state.reset.rotation_limit_430,
            r.state.rules.rotation_limit
        );
    }
}

fn startup_seed() -> StartupState {
    StartupState {
        world_services: [0; 6],
        constructor_metadata: None,
        tunables_170_180: [0; 2],
        base_flags_04c_04d: [0; 2],
        base_padding_04f: 0,
        ball_resources: None,
        player_init: crate::tetherball_player_init::PlayerInitState::new([0; 2]),
        tag_02c: 0,
        tag_030: 0,
        asset_handle_100: 0,
        alternate_pole_188: 0,
        mode_2ec: 0,
        pole_glow_340: 0,
        inverse_world_3d8: [0.; 16],
        identities_078_0b8: [[0; 2]; 2],
    }
}

#[test]
fn original_live_server_setup() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/data/tetherball_server_golden.json")).unwrap();
    assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
    for (index, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let mut r = runtime_seed();
        let mut s = startup_seed();
        for (o, v) in c["initial_game_words"].as_object().unwrap() {
            word(
                &mut r,
                &mut s,
                usize::from_str_radix(&o[2..], 16).unwrap(),
                Some(u(v)),
            );
        }
        r.ball = crate::tetherball::tests::ball(&c["initial_ball"]);
        r.state.reset.current_ball_owner_074 = u(&c["initial_ball_attachment"][0]);
        r.state.reset.current_ball_matrix_078 = u(&c["initial_ball_attachment"][1]);
        r.state.scene.trails = std::array::from_fn(|i| u(&c["initial_trails"][i]));
        r.state.scene.null_trail = u(&c["input"]["null_trail"]);
        let mut host = Host {
            fixture: &fixture,
            case: c,
            events: vec![],
            camera_words: [0; 2],
            visible: [0; 2],
            renderer: 0,
            ancient: 0,
            character_activity: [0; 2],
            ai_ball_handles: [0; 2],
            physical_area: 0,
            camera_target: [0; 3],
        };
        crate::tetherball_server::setup_server(&mut r, &mut host).unwrap();
        assert_eq!(json!(host.events), c["effects"], "server effects {index}");
        for (o, v) in c["expected_game_words"].as_object().unwrap() {
            let offset = usize::from_str_radix(&o[2..], 16).unwrap();
            if let Some(value) = word(&mut r, &mut s, offset, None) {
                assert_eq!(value, u(v), "server {index} word {offset:#x}");
            }
        }
        crate::tetherball::tests::assert_bits(
            &r.ball,
            &crate::tetherball::tests::ball(&c["expected_ball"]),
            index,
            0,
        );
        assert_eq!(json!(r.state.scene.trails), c["expected_trails"]);
        assert_eq!(
            json!([
                r.state.reset.current_ball_owner_074,
                r.state.reset.current_ball_matrix_078
            ]),
            c["expected_ball_attachment"]
        );
        assert_eq!(json!(host.camera_target), c["expected_camera_target"]);
    }
}
