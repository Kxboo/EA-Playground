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
        0x2c => scalar!(s.tag_02c),
        0x30 => scalar!(s.tag_030),
        0x34 => scalar!(r.life.match_state.state_code),
        0x3c => scalar!(r.life.variant),
        0x40 => scalar!(r.life.session_mode),
        0x44 => scalar!(r.state.reset.game_mode_044),
        0x48 => scalar!(r.life.game_type),
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
    forbidden!(mega_visible(p:i32,v:i32));
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
    forbidden!(clear_hud());
    forbidden!(close_screen());
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
    fn helper(&mut self, name: &str, args: Vec<Value>, r: &mut Runtime, s: &mut StartupState) {
        let mut event = vec![json!(name)];
        event.extend(args);
        self.events.push(json!(event));
        if let Some(rows) = self.case["input"]["boundary_writes"][name].as_array() {
            for row in rows {
                let offset = u(&row[0]) as usize;
                let size = u(&row[1]);
                let value = u(&row[2]);
                let (offset, value) = if size == 1 {
                    let o = offset & !3;
                    let shift = (3 - (offset & 3)) * 8;
                    let before = word(r, s, o, None).expect("byte owner");
                    (o, (before & !(255 << shift)) | (value << shift))
                } else {
                    assert_eq!(size, 4);
                    (offset, value)
                };
                assert!(
                    word(r, s, offset, Some(value)).is_some(),
                    "unmapped helper store {offset:x}"
                );
            }
        }
    }
}
impl StartupServices for Host<'_> {
    fn effect(&mut self, e: Effect) {
        let event = match e {
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
    fn stage(&mut self, stage: Stage, r: &mut Runtime, s: &mut StartupState) -> Result<(), String> {
        let (name, args) = match stage {
            Stage::BaseInitialize { world } => {
                assert_eq!(world, self.handle("world"));
                s.world_services = std::array::from_fn(|i| u(&self.fixture["world_services"][i]));
                ("base_initialize", vec![])
            }
            Stage::InitGameLogic => ("init_logic", vec![]),
            Stage::ConstructBall { handle } => ("ball_ctor", vec![json!(handle)]),
            Stage::SetArea(v) => ("set_area", vec![json!(v)]),
            Stage::Character { kind, input } => {
                let name = match kind {
                    CharacterKind::Player => "init_player",
                    CharacterKind::Ai => "init_ai_player",
                    CharacterKind::AdditionalPlayer => "init_additional_player",
                };
                (
                    name,
                    vec![
                        json!(input.position.map(f32::to_bits)),
                        json!(input.heading.to_bits()),
                        json!(input.identity_words[0]),
                        json!(input.identity_words[1]),
                    ],
                )
            }
            Stage::Ai {
                player,
                enabled,
                difficulty,
                heading,
            } => {
                let handle = r.state.reset.current_ai_entities_128[player];
                r.state.ai[player] = Some(crate::tetherball_ai::AiEntity::new(handle, player, 0.));
                (
                    "init_ai_entity",
                    vec![
                        json!(handle),
                        json!(enabled),
                        json!(difficulty),
                        json!(heading.to_bits()),
                    ],
                )
            }
            Stage::Ball {
                difficulty,
                anchor,
                heading,
                pole,
            } => (
                "ball_init",
                vec![
                    json!(difficulty),
                    json!(anchor.map(f32::to_bits)),
                    json!(heading.to_bits()),
                    json!(pole),
                ],
            ),
            Stage::OpenPregame(v) => ("open_pregame", vec![json!(v)]),
            Stage::SetUpServer => ("setup_server", vec![]),
        };
        self.helper(name, args, r, s);
        Ok(())
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
        if name == "mg_tetherball" {
            1
        } else {
            assert_eq!(name, "tunables");
            2
        }
    }
    fn collection(&mut self, class: u64, name: u64) -> u32 {
        assert_eq!((class, name), (1, 2));
        self.events
            .push(json!(["collection", "mg_tetherball", "tunables"]));
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
        let mut s = StartupState {
            world_services: [0; 6],
            tag_02c: 0,
            tag_030: 0,
            asset_handle_100: 0,
            alternate_pole_188: 0,
            mode_2ec: 0,
            pole_glow_340: 0,
            inverse_world_3d8: [0.; 16],
            identities_078_0b8: [[0; 2]; 2],
        };
        for (o, v) in c["initial_game_words"].as_object().unwrap() {
            let off = usize::from_str_radix(&o[2..], 16).unwrap();
            word(&mut r, &mut s, off, Some(u(v)));
        }
        r.state.serve.forced_ai = std::array::from_fn(|i| u(&c["input"]["forced_ai"][i]) != 0);
        let mut host = Host {
            fixture: &fixture,
            case: c,
            events: vec![],
            camera_words: [0; 2],
            visible: [0x25, 0x73],
            renderer: 0,
            ancient: 1,
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
        assert_eq!(json!(host.visible), c["expected_placeable_visible"]);
        assert_eq!(
            json!({"0x39c":host.camera_words[0],"0x3a0":host.camera_words[1]}),
            c["expected_camera_words"]
        );
        for p in 0..2 {
            let ai = r.state.ai[p].as_ref().unwrap();
            assert_eq!(
                json!({"0x64":ai.angle.to_bits(),"0x68":ai.direction_scale.to_bits(),"0x74":r.life.players[p].ai_distance}),
                c["expected_ai_words"][p]
            );
        }
        assert_eq!(json!(r.ball.radius.to_bits()), c["expected_ball_radius"]);
        assert_eq!(
            json!(r.ball.desired_radius.to_bits()),
            c["expected_ball_desired_radius"]
        );
        assert_eq!(json!(host.renderer), c["expected_renderer_word_1c0"]);
        assert_eq!(json!(host.ancient), c["expected_ancient_byte"]);
    }
}
