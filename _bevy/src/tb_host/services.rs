//! The engine-service trait implementations of `TbHost` (see the parent module for the contract).
use super::*;

impl TbHost {
    /// Everything `ResetRound` / `SetUpServer` ask the engine for.
    pub(super) fn build_reset_inputs(&mut self, _life: &Lifecycle, _state: &ResetState, _full: bool) -> ResetInputs {
        let tuning = std::array::from_fn(|i| RoundTuning {
            base_hit_speed: self.db_float("mg_tetherball", "tunables", "ball_basehitspeed", i),
            accelerate_modifier: self.db_float("mg_tetherball", "tunables", "ball_acceleratemodifier", i),
            power_modifier: self.db_float("mg_tetherball", "tunables", "ball_powermodifier", i),
            mega_modifier: self.db_float("mg_tetherball", "tunables", "ball_megamodifier", i),
        });
        let weights = std::array::from_fn(|i| ScoreWeights {
            accuracy_points: self.db_int("mg_tetherball", "scoring", "accuracy_points", i),
            power_hit_points: self.db_int("mg_tetherball", "scoring", "powerhit_points", i),
            mega_hit_points: self.db_int("mg_tetherball", "scoring", "megahit_points", i),
        });
        let created = [self.handle(), self.handle()];
        let camera = self.camera_handle();
        let random = self.rand(0, 1);
        let markers = std::array::from_fn(|p| match self.chars.get(p) {
            Some(c) => vec![Marker { id: BALL_MARKER, matrix_handle: c.handle + 1 }],
            None => vec![],
        });
        ResetInputs {
            round_tuning: tuning,
            scoring_difficulty: crate::tetherball_tuning::difficulty_index(self.cfg.difficulty),
            scoring_array_count: 4,
            scoring_weights: weights,
            camera_position_offset: [0., 1.075, 0.],
            camera_target_offset: [0., 0., 0.],
            ai_global_enable: false,
            initialized_animation_states: [0; 2],
            created_ai_entities: created,
            random_server_result: random,
            camera_lookup_result: camera,
            markers,
        }
    }

    pub(super) fn camera_handle(&self) -> u32 {
        HANDLE_BASE + 0x0F_0000
    }

    pub(super) fn apply_effect(&mut self, effect: ResetEffect) {
        match effect {
            ResetEffect::TimerVisible { visible, .. } => self.out.push(Out::Hud(Hud::TimerVisible(i32::from(visible)))),
            ResetEffect::ServeBubbleVisible { args } => self.out.push(Out::Hud(Hud::ServeBubble(args))),
            ResetEffect::DestroyPartFx { guid, fade } => self.out.push(Out::ParticleDestroy { id: guid, fade_ms: fade }),
            ResetEffect::PoleTextureMatrix { matrix_bits, .. } => {
                let m = f32::from_bits(matrix_bits[13]);
                self.pole_offset = m;
                self.out.push(Out::PoleIndicator(m));
            }
            ResetEffect::GrabBall { character, .. } => self.grab = self.char_index(character),
            ResetEffect::CameraPositionOffset { offset_bits, .. } => self.camera.set_position_offset(offset_bits.map(f32::from_bits)),
            ResetEffect::CameraDesiredPositionOffset { offset_bits, ms, .. } => self.camera.set_desired_position_offset(offset_bits.map(f32::from_bits), ms),
            ResetEffect::CameraTargetOffset { offset_bits, .. } => self.camera.set_target_offset(offset_bits.map(f32::from_bits)),
            ResetEffect::CameraDesiredTargetOffset { offset_bits, ms, .. } => self.camera.set_desired_target_offset(offset_bits.map(f32::from_bits), ms),
            ResetEffect::CameraBackwardsOffset { offset_bits, .. } => self.camera.set_backwards(f32::from_bits(offset_bits)),
            ResetEffect::CameraDistance { distance_bits, target_height_bits, .. } => {
                self.camera.set_scalars(f32::from_bits(distance_bits), f32::from_bits(target_height_bits))
            }
            ResetEffect::CameraDesiredRotation { angle_bits, ms, .. } => self.camera.set_desired_rotation(f32::from_bits(angle_bits), ms),
            ResetEffect::CameraStartPosition { position_bits, .. } | ResetEffect::CameraTargetPosition { position_bits, .. } => {
                self.camera.set_start(position_bits.map(f32::from_bits))
            }
            ResetEffect::CameraDirection { direction_bits, .. } => self.camera.set_dir(direction_bits.map(f32::from_bits)),
            ResetEffect::CreateAiEntity { player, entity, .. } => {
                if let Some(c) = self.chars.get_mut(player) {
                    c.ai_entity = entity;
                }
                self.active[player] = None;
            }
            ResetEffect::CharacterPosition { player, position_bits, .. } => {
                if let Some(c) = self.chars.get_mut(player) {
                    c.pos = position_bits.map(f32::from_bits);
                    c.target = None;
                }
            }
            ResetEffect::AnimationNextState { player, state, force, blend } => self.set_animation(player, state, force, blend),
            ResetEffect::ScoreboardVisible { args } => self.out.push(Out::Hud(Hud::Scoreboard(args))),
            // Database reads, random draws, marker queries and the AI bookkeeping that `apply_reset_ai` already
            // consumed are observations of work `build_reset_inputs` did.
            _ => {}
        }
    }

    pub fn set_animation(&mut self, player: usize, state: i32, force: bool, timer_ms: i32) {
        if state < 0 {
            return;
        }
        let assets = self.assets.clone();
        let mut rng = self.rng;
        let Some(c) = self.chars.get_mut(player) else { return };
        let graph = assets.graph(c.female);
        c.animator.set(graph, &assets.lib, state as usize, 1., force, timer_ms, |lo, hi| {
            let span = (hi - lo + 1).max(1) as u64;
            lo + (xorshift(&mut rng) % span) as i32
        });
        self.rng = rng;
    }

    /// Look-at point and eye of the follow camera as of the last frame.
    pub fn camera_focus(&self) -> [f32; 3] {
        self.eye_target.1
    }
    pub fn eye(&self) -> [f32; 3] {
        self.eye_target.0
    }
}

impl Services for TbHost {
    fn scoreboard(&mut self, args: [i32; 4]) {
        self.out.push(Out::Hud(Hud::Scoreboard(args)));
    }
    fn round(&mut self, args: [i32; 3]) {
        self.out.push(Out::Hud(Hud::Round(args)));
    }
    fn serve_bubble(&mut self, args: [i32; 3]) {
        self.out.push(Out::Hud(Hud::ServeBubble(args)));
    }
    fn mega_visible(&mut self, player: i32, visible: i32) {
        self.out.push(Out::Hud(Hud::MegaVisible(player, visible)));
    }
    fn mega_value(&mut self, player: i32, value: i32) {
        self.out.push(Out::Hud(Hud::MegaValue(player, value)));
    }
    fn winner_visible(&mut self, ui: WinnerUi, player: i32, visible: bool) {
        self.out.push(Out::Hud(Hud::Winner(ui, player, visible)));
    }
    fn sound(&mut self, frontend: bool, sound: i32, variant: i32, volume: i32) {
        self.out.push(Out::Sound { frontend, id: sound, variant, volume });
    }
    fn controller_pop(&mut self, controller: i32) {
        if let Some(c) = self.controllers.get_mut(controller as usize) {
            c.pop_state();
        }
    }
    fn controller_set(&mut self, controller: i32, state: i32) {
        if let Some(c) = self.controllers.get_mut(controller as usize) {
            c.push_state(state as u32);
        }
    }
    fn animation(&mut self, player: usize, state: i32, force: bool, blend: i32) {
        self.set_animation(player, state, force, blend);
    }
    fn switch_to_ai(&mut self, _player: usize) {}
    fn camera_offset(&mut self, target: bool, offset: [f32; 3], ms: u32) {
        if target {
            self.camera.set_desired_target_offset(offset, ms)
        } else {
            self.camera.set_desired_position_offset(offset, ms)
        }
    }
    fn wrap_particle(&mut self, player: usize, position: [f32; 3], fade_ms: i32) {
        self.out.push(Out::WrapParticle { player, position, fade_ms });
    }
    fn fade_in(&mut self, value: i32) {
        self.out.push(Out::Fade { kind: "in", ms: value });
    }
    fn random_range(&mut self, low: i32, high: i32) -> i32 {
        self.rand(low, high)
    }
    fn clear_hud(&mut self) {
        self.out.push(Out::Hud(Hud::Clear));
    }
    fn close_screen(&mut self) {
        self.out.push(Out::Hud(Hud::CloseScreen));
    }
    fn reset_round(&mut self, _state: &mut Lifecycle, _ball: &mut BallMotion) {
        unreachable!("Runtime's frame adapter owns reset_round")
    }
    fn post_game(&mut self, kind: i32, words: &[u32; 70]) {
        self.out.push(Out::Hud(Hud::PostGame(kind, words.to_vec())));
    }
}

impl IntroServices for TbHost {
    fn reset_scoreboard(&mut self) {
        self.out.push(Out::Hud(Hud::ResetScoreboard));
    }
}

impl FrontendServices for TbHost {
    fn clear_pregame_handlers(&mut self) {
        self.out.push(Out::Hud(Hud::ClearPregameHandlers));
    }
    fn clear_postgame_handlers(&mut self) {
        self.out.push(Out::Hud(Hud::ClearPostgameHandlers));
    }
    fn start_fade_out(&mut self, ms: i32) {
        self.fade_left_ms = ms;
        self.fade_pending = true;
        self.out.push(Out::Fade { kind: "out", ms });
    }
    fn fade_out_renders(&mut self, _first: bool, _second: bool) {}
    fn setup_minigame_handlers(&mut self, kind: i32) {
        self.out.push(Out::Hud(Hud::SetupHandlers(kind)));
    }
    fn open_apt_screen(&mut self, name: &str) {
        self.out.push(Out::Hud(Hud::OpenScreen(name.to_string())));
    }
    fn reset_minigame(&mut self, _life: &mut Lifecycle, _ball: &mut BallMotion) {
        unreachable!("Runtime's frame adapter owns reset_minigame")
    }
    fn close_apt_overlay(&mut self) {
        self.out.push(Out::Hud(Hud::CloseOverlay));
    }
    fn audio_unpause(&mut self) {
        self.paused_audio = false;
        self.out.push(Out::AudioUnpause);
    }
    fn minigame_fade_complete(&mut self) -> bool {
        self.fade_left_ms <= 0
    }
    fn timer_visible(&mut self, visible: i32) {
        self.out.push(Out::Hud(Hud::TimerVisible(visible)));
    }
}

impl ServeServices for TbHost {
    fn azimuth(&mut self, player: usize) -> i32 {
        // Provisional: signed angle (degrees) of the character from the camera's forward axis, wrapped to +-180.
        let Some(c) = self.chars.get(player) else { return 0 };
        let eye = self.eye();
        let forward = sub(self.camera_focus(), eye);
        let to = sub(c.pos, eye);
        let mut d = (to[0].atan2(to[2]) - forward[0].atan2(forward[2])).to_degrees() % 360.;
        if d > 180. {
            d -= 360.;
        } else if d < -180. {
            d += 360.;
        }
        d as i32
    }
    fn event(&mut self, controller: i32, action: i32) -> bool {
        self.controllers.get(controller as usize).is_some_and(|c| c.event(action as u32).active)
    }
    fn timer_visible(&mut self, visible: i32) {
        self.out.push(Out::Hud(Hud::TimerVisible(visible)));
    }
    fn wiimote_sound(&mut self, player: usize, sound: i32, flags: i32) {
        self.out.push(Out::WiimoteSound { player, id: sound, flags });
    }
    fn camera_shake(&mut self, milliseconds: i32, strength: f32) {
        self.shake_ms = milliseconds;
        self.out.push(Out::CameraShake { ms: milliseconds, strength });
    }
    fn rumble(&mut self, controller: i32, milliseconds: u32, strength: f32) {
        self.out.push(Out::Rumble { controller, ms: milliseconds, strength });
    }
    fn serve_particle(&mut self, name: &str, position: [f32; 3], fade_ms: i32) {
        let id = self.handle();
        self.out.push(Out::Particle { id, name: name.to_string(), position });
        self.out.push(Out::ParticleDestroy { id, fade_ms });
    }
    fn pregame(&mut self, kind: i32, argument: i32, words: [u32; 4]) {
        self.out.push(Out::PauseInfo { kind, argument, words });
    }
    fn overlay(&mut self, name: &str) {
        self.out.push(Out::Hud(Hud::OpenOverlay(name.to_string())));
    }
    fn audio_pause(&mut self, mode: i32) {
        self.paused_audio = true;
        self.out.push(Out::AudioPause(mode));
    }
}

impl HitServices for TbHost {
    fn create_part_fx(&mut self, name: &str, position: [f32; 3]) -> u32 {
        let id = self.handle();
        self.fx.insert(id, position);
        self.out.push(Out::Particle { id, name: name.to_string(), position });
        id
    }
    fn disable_and_destroy_part_fx(&mut self, guid: u32, delay_ms: i32) {
        self.fx.remove(&guid);
        self.out.push(Out::ParticleDestroy { id: guid, fade_ms: delay_ms });
    }
    fn get_part_fx(&mut self, guid: u32) -> u32 {
        if self.fx.contains_key(&guid) { guid } else { 0 }
    }
    fn set_part_fx_position(&mut self, part_fx: u32, position: [f32; 3]) {
        self.fx.insert(part_fx, position);
        self.out.push(Out::ParticleMove { id: part_fx, position });
    }
    fn set_part_fx_scale(&mut self, part_fx: u32, scale: f32) {
        self.out.push(Out::ParticleScale { id: part_fx, scale });
    }
    fn character_position(&mut self, player: usize) -> Option<[f32; 3]> {
        self.chars.get(player).map(|c| c.pos)
    }
}

impl SceneServices for TbHost {
    fn create_trail(&mut self, kind: Trail, position: [f32; 3]) -> u32 {
        let id = self.handle();
        self.trails.insert(id, (kind, position));
        self.out.push(Out::Particle { id, name: kind.name().to_string(), position });
        id
    }
    fn destroy_trail(&mut self, handle: u32, fade_ms: i32) {
        self.trails.remove(&handle);
        self.out.push(Out::ParticleDestroy { id: handle, fade_ms });
    }
    fn move_trail(&mut self, handle: u32, position: [f32; 3]) {
        if let Some(t) = self.trails.get_mut(&handle) {
            t.1 = position;
        }
        self.out.push(Out::ParticleMove { id: handle, position });
    }
    fn shadow_matrix(&mut self, rope: bool, matrix: Matrix) {
        self.out.push(Out::ShadowMatrix { rope, matrix });
    }
}

impl ShadowSetupServices for TbHost {
    fn set_shadow_viewport(&mut self, mode: i32, options: ShadowViewOptions) {
        self.out.push(Out::ShadowViewport { mode, options });
    }
}

impl HitCompulsionServices for TbHost {
    fn random_range(&mut self, low: i32, high: i32) -> i32 {
        self.rand(low, high)
    }
    fn is_tetherball_minigame(&mut self) -> bool {
        true
    }
}

impl RuntimeHost for TbHost {
    fn pole_indicator(&mut self, offset: f32) {
        self.pole_offset = offset;
        self.out.push(Out::PoleIndicator(offset));
    }
    fn world_update(&mut self, _milliseconds: i32) {}
    fn reset_inputs(&mut self, life: &Lifecycle, state: &ResetState, full: bool) -> ResetInputs {
        self.build_reset_inputs(life, state, full)
    }
    fn reset_effect(&mut self, effect: ResetEffect) {
        self.apply_effect(effect);
    }
}

impl PlayerInitServices for TbHost {
    fn get_player_character(&mut self, _player: i32) -> Option<ExistingCharacter> {
        None
    }
    fn spawn_character(&mut self, call: SpawnCharacterCall) -> u32 {
        let handle = self.handle();
        let index = self.chars.len();
        let human = (index < self.cfg.humans).then_some(index);
        let female = self.cfg.female.get(index).copied().unwrap_or(false);
        let animator = Animator::new(self.assets.bind.clone(), female);
        self.chars.push(Char {
            handle,
            index,
            pos: call.position_r7,
            dir: [0., 0., 1.],
            identity: call.identity_words_r5_r6,
            ai_entity: 0,
            human,
            target: None,
            animator,
            female,
            moving: false,
        });
        self.out.push(Out::CharacterSpawned { index, handle });
        handle
    }
    fn set_character_state_position(&mut self, character: u32, position: [f32; 3]) {
        if let Some(i) = self.char_index(character) {
            self.chars[i].pos = position;
        }
    }
    fn set_character_state_direction(&mut self, character: u32, direction: [f32; 3]) {
        if let Some(i) = self.char_index(character) {
            self.chars[i].dir = direction;
        }
    }
    fn allocate_ai_slot(&mut self) -> u32 {
        self.handle()
    }
    fn construct_tetherball_ai(&mut self, _entity: u32, _character: u32) {}
    fn add_ai_entity(&mut self, _entity: u32) {}
    fn set_character_ai_entity(&mut self, character: u32, entity: u32) {
        if let Some(i) = self.char_index(character) {
            self.chars[i].ai_entity = entity;
        }
    }
    fn set_ai_ball_handle(&mut self, _entity: u32, _ball: u32) {}
    fn multiplayer_enable_byte(&mut self) -> u8 {
        u8::from(!self.cfg.single_player)
    }
    fn setup_multiplayer_ability(&mut self) {}
    fn setup_single_player_ability(&mut self) {}
    fn player_info_handle(&mut self, character: u32) -> u32 {
        character
    }
    fn controller_index(&mut self, player_info: u32) -> i32 {
        self.char_index(player_info).and_then(|i| self.chars[i].human).map(|h| h as i32).unwrap_or(-1)
    }
    fn controller_handle(&mut self, index: i32) -> u32 {
        HANDLE_BASE + 0x0E_0000 + index.max(0) as u32
    }
    fn set_controller_state(&mut self, controller: u32, state: i32) {
        let index = (controller - (HANDLE_BASE + 0x0E_0000)) as usize;
        if let Some(c) = self.controllers.get_mut(index) {
            c.push_state(state as u32);
        }
    }
}

impl crate::tetherball_ball_init::BallInitServices for TbHost {
    fn texture(&mut self, _name: &str) -> crate::tetherball_ball_init::LoadedAsset {
        let handle = self.handle();
        crate::tetherball_ball_init::LoadedAsset { handle, id: handle ^ 0x55 }
    }
    fn model(&mut self, _name: &str, _flags: i32) -> crate::tetherball_ball_init::LoadedAsset {
        let handle = self.handle();
        crate::tetherball_ball_init::LoadedAsset { handle, id: handle ^ 0x33 }
    }
    fn set_textures(&mut self, _model: u32, _texture: u32) {}
    fn allocate(&mut self, _bytes: u32, _tag: &str) -> u32 {
        self.handle()
    }
    fn construct_cached_model(&mut self, _handle: u32) {}
    fn bind_cached_model(&mut self, _cached: u32, _model: u32) {}
    fn construct_shadow(&mut self, _handle: u32, _model: u32) {}
    fn add_scene_entity(&mut self, _layer: i32, _entity: u32) {}
    fn database_key(&mut self, name: &str) -> u64 {
        let key = crate::vlt::string_hash64(name);
        self.keys.insert(key, name.to_string());
        key
    }
    fn collection(&mut self, class: u64, name: u64) -> u32 {
        let id = self.handle();
        let class = self.keys.get(&class).cloned().unwrap_or_default();
        let name = self.keys.get(&name).cloned().unwrap_or_default();
        self.collections.insert(id, (class, name));
        id
    }
    fn float_from_array(&mut self, collection: u32, field: &str, index: u32) -> f32 {
        let (class, name) = self.collections.get(&collection).cloned().unwrap_or_default();
        self.db_float(&class, &name, field, index as usize)
    }
    fn destroy_collection(&mut self, collection: u32) {
        self.collections.remove(&collection);
    }
}

impl StartupServices for TbHost {
    fn effect(&mut self, effect: Effect) {
        match effect {
            Effect::PlaceableVisible { handle, visible } => self.out.push(Out::PoleVisible { handle, visible }),
            Effect::CameraOffset { target, desired_ms, value, .. } => match (target, desired_ms) {
                (false, None) => self.camera.set_position_offset(value),
                (true, None) => self.camera.set_target_offset(value),
                (false, Some(ms)) => self.camera.set_desired_position_offset(value, ms),
                (true, Some(ms)) => self.camera.set_desired_target_offset(value, ms),
            },
            Effect::CameraBackwards { value, .. } => self.camera.set_backwards(value),
            Effect::CameraScalars { height_39c, value_3a0, .. } => self.camera.set_scalars(height_39c, value_3a0),
            Effect::CameraRotation { value, milliseconds, .. } => self.camera.set_desired_rotation(value, milliseconds),
            Effect::CameraStart { value, .. } => self.camera.set_start(value),
            Effect::CameraDirection { value, .. } => self.camera.set_dir(value),
            Effect::LoadAudio(id) => self.out.push(Out::LoadAudio(id)),
            Effect::PlayMusic(id) => self.out.push(Out::PlayMusic(id)),
            _ => {}
        }
    }
    fn stage(&mut self, stage: Stage, runtime: &mut Runtime, state: &mut StartupState) -> Result<(), String> {
        self.stage_impl(stage, runtime, state)
    }
    fn allocate_ball(&mut self, _bytes: u32, _tag: &str) -> u32 {
        self.handle()
    }
    fn placeable(&mut self, name: &str) -> (u32, [f32; 3]) {
        let handle = self.handle();
        let lookup = |n: &str| {
            self.db
                .find_collection("placeables", n)
                .and_then(|c| crate::tetherball_tuning::inherited_attribute(&self.db, c, "position").ok().flatten())
        };
        let position = lookup(name)
            .and_then(|v| v.as_array().map(|a| std::array::from_fn(|i| a[i].as_f64().unwrap_or(0.) as f32)))
            .unwrap_or([0.; 3]);
        if !name.ends_with("_w_ball") {
            self.origin = position;
        }
        (handle, position)
    }
    fn camera(&mut self, _player: u32) -> u32 {
        self.camera_handle()
    }
    fn load_bigfile(&mut self, _name: &str, _load: bool, _pool: u32) -> u32 {
        self.handle()
    }
    fn ground_height(&mut self, position: [f32; 3], _range: f32) -> f32 {
        (self.ground)(position)
    }
    fn create_particle(&mut self, name: &str, position: [f32; 3]) -> u32 {
        let id = self.handle();
        self.out.push(Out::Particle { id, name: name.to_string(), position });
        id
    }
    fn database_key(&mut self, name: &str) -> u64 {
        crate::tetherball_ball_init::BallInitServices::database_key(self, name)
    }
    fn collection(&mut self, class: u64, name: u64) -> u32 {
        crate::tetherball_ball_init::BallInitServices::collection(self, class, name)
    }
    fn float_array(&mut self, collection: u32, field: &str, index: u32) -> f32 {
        crate::tetherball_ball_init::BallInitServices::float_from_array(self, collection, field, index)
    }
    fn int16_array(&mut self, collection: u32, field: &str, index: u32) -> i16 {
        let (class, name) = self.collections.get(&collection).cloned().unwrap_or_default();
        self.db_int(&class, &name, field, index as usize) as i16
    }
    fn destroy_collection(&mut self, collection: u32) {
        self.collections.remove(&collection);
    }
}
