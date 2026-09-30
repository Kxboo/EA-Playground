//! One owner and dispatcher for the recovered tetherball frame graph.
//!
//! Engine objects remain host services. Construction requires decoded startup
//! state; there is deliberately no default or fixture-backed game constructor.
use crate::area_transform::AreaTransform;
use crate::tetherball::BallMotion;
use crate::tetherball_ai::AiEntity;
use crate::tetherball_frontend::{self as frontend, FrontendServices, FrontendState};
use crate::tetherball_gestures::{self as gestures, GestureCallback, GestureState};
use crate::tetherball_hit::{HitInputs, HitServices, HitState};
use crate::tetherball_hit_animation::HitAnimations;
use crate::tetherball_lifecycle::{FrameServices, IntroServices, IntroState, Lifecycle, Services};
use crate::tetherball_match::{MatchRules, WinnerUi};
use crate::tetherball_rally::{RallyPhase, update_rally};
use crate::tetherball_rally_rules::RallyRuleState;
use crate::tetherball_reset::{ResetEffect, ResetInputs, ResetState};
use crate::tetherball_scene::{Attachment, BallScene, SceneServices};
use crate::tetherball_serve::{ServeInputs, ServeState, update_serve};

pub struct Runtime {
    pub life: Lifecycle,
    pub ball: BallMotion,
    pub state: RuntimeState,
}

pub struct RuntimeState {
    pub reset: ResetState,
    pub serve: ServeState,
    pub gestures: GestureState,
    pub rally: RallyRuleState,
    pub hit: HitState,
    pub animations: HitAnimations,
    pub scene: BallScene,
    pub ai: [Option<AiEntity>; 2],
    /// Authoritative match rules; ResetState's legacy +430 projection is
    /// refreshed from rotation_limit at each reset boundary.
    pub rules: MatchRules,
    pub frontend: FrontendState,
    /// Original +2fc/+304 names, filled by initialization/asset loading.
    pub fx_names: [[String; 2]; 2],
}

pub struct FrameInputs {
    pub milliseconds: i32,
    /// World singleton (+8c), byte +24; distinct from this minigame's pause.
    pub world_paused: bool,
    pub area: AreaTransform,
    pub attachment: Option<Attachment>,
    pub controller_fx_offset: [f32; 3],
}

/// Engine boundaries not owned by a tetherball gameplay handler. ResetInputs
/// are the existing reset projection's external query results; effect dispatch
/// must not reenter/mutate the game during the ordered reset batch.
pub trait RuntimeHost: HitServices + SceneServices + IntroServices + FrontendServices {
    fn pole_indicator(&mut self, offset: f32);
    fn world_update(&mut self, milliseconds: i32);
    fn reset_inputs(&mut self, life: &Lifecycle, state: &ResetState, full: bool) -> ResetInputs;
    fn reset_effect(&mut self, effect: ResetEffect);
}

impl Runtime {
    pub fn capture_gesture(&mut self, callback: GestureCallback, controller: i32) -> bool {
        gestures::capture_lifecycle_gesture(
            &mut self.state.gestures,
            &self.life,
            self.state.frontend.field_424 != 0,
            self.life.focus_player as usize,
            self.state.reset.receiver_220 as usize,
            callback,
            controller,
        )
    }

    /// Runs gestures, ball transforms, the selected handler, and base effects
    /// in original order. Status 2 is preserved for completed exit fades.
    /// A missing database dependency returns an error; such a frame is not
    /// transactional and the caller must stop using that incomplete match.
    pub fn update(
        &mut self,
        input: &FrameInputs,
        db: &crate::vlt::Database,
        host: &mut impl RuntimeHost,
    ) -> Result<Option<u32>, String> {
        let mut adapter = FrameAdapter {
            state: &mut self.state,
            input,
            db,
            host,
            failure: None,
        };
        let result = self
            .life
            .update(input.milliseconds, &mut self.ball, &mut adapter);
        match adapter.failure {
            Some(error) => Err(error),
            None => Ok(result),
        }
    }

    pub fn reset(
        &mut self,
        full: bool,
        db: &crate::vlt::Database,
        host: &mut impl RuntimeHost,
    ) -> Result<(), String> {
        self.state
            .reset(&mut self.life, &mut self.ball, full, db, host)
    }
}

impl RuntimeState {
    fn reset(
        &mut self,
        life: &mut Lifecycle,
        ball: &mut BallMotion,
        full: bool,
        db: &crate::vlt::Database,
        host: &mut impl RuntimeHost,
    ) -> Result<(), String> {
        // Legacy stand-alone ResetState projections mirror these live owners
        // only at this boundary. Never reset stale trails or discard the queue.
        self.reset.ball_fx_140 = self.scene.trails;
        self.reset.ball_invalid_guid = self.scene.null_trail;
        self.reset.pending_count_2e4 = self.gestures.pending.len() as u32;
        self.reset.field_229 = self.gestures.hit_attempt_marker;
        self.reset.rotation_limit_430 = self.rules.rotation_limit;
        let input = host.reset_inputs(life, &self.reset, full);
        let reset_fn = if full {
            crate::tetherball_reset_runtime::reset_minigame_with_animations
        } else {
            crate::tetherball_reset_runtime::reset_round_with_animations
        };
        let effects = reset_fn(
            life,
            &mut self.reset,
            ball,
            &input,
            &mut self.serve,
            &mut self.animations,
        );
        gestures::reset_pending_gestures(&mut self.gestures);
        self.scene.trails = self.reset.ball_fx_140;
        let effects = crate::tetherball_ai_reset::apply_reset_ai(
            effects,
            &mut self.ai,
            life,
            &mut self.rally,
            db,
        )?;
        for effect in effects {
            // Reset's HUD initialization uses the same FE flag owner as pause
            // and pregame, without inventing an immediate load-complete event.
            if effect == ResetEffect::InitializeHud {
                frontend::initialize_apt_hud(&mut self.serve, host);
            } else {
                host.reset_effect(effect);
            }
        }
        Ok(())
    }
}

struct FrameAdapter<'a, H> {
    state: &'a mut RuntimeState,
    input: &'a FrameInputs,
    db: &'a crate::vlt::Database,
    host: &'a mut H,
    failure: Option<String>,
}

impl<H: RuntimeHost> FrameServices for FrameAdapter<'_, H> {
    fn process_gestures(&mut self, life: &mut Lifecycle, _: &mut BallMotion) {
        gestures::process_lifecycle_gestures(&mut self.state.gestures, life);
    }
    fn update_ball(&mut self, _: &mut Lifecycle, ball: &mut BallMotion, ms: i32) {
        self.state
            .scene
            .update(ball, ms, self.input.attachment, self.input.area, self.host);
    }
    fn update_handler(
        &mut self,
        life: &mut Lifecycle,
        ball: &mut BallMotion,
        code: u32,
        ms: i32,
    ) -> u32 {
        let rules = self.state.rules;
        match code {
            1 => frontend::update_pregame_instructions(
                life,
                &mut self.state.frontend,
                &mut self.state.serve,
                ms,
                rules,
                ball,
                self.host,
            ),
            3 => {
                let mut intro = IntroState {
                    initial_receiver: self.state.reset.receiver_220 as usize,
                    field_25c: self.state.reset.field_25c,
                    scoreboard_needs_reset: self.state.reset.field_444,
                };
                let result = life.update_intro(&mut intro, rules, ball, self.host);
                self.state.reset.field_25c = intro.field_25c;
                self.state.reset.field_444 = intro.scoreboard_needs_reset;
                u32::from(result)
            }
            8 => {
                // This handler's three fields are not touched by ResetMiniGame;
                // its synchronous callback can borrow all other runtime owners.
                let mut front = self.state.frontend.clone();
                let result = frontend::update_postgame(life, &mut front, ms, rules, ball, self);
                self.state.frontend = front;
                result
            }
            9 => frontend::update_wait_for_apocalypse(ms, self.host),
            26 => u32::from(life.update_resetting(rules, ball, self.host)),
            27 => {
                let input = ServeInputs {
                    milliseconds: ms,
                    ball_position: self.state.scene.position,
                    fx_names: self.state.fx_names.clone(),
                };
                u32::from(update_serve(
                    life,
                    &mut self.state.reset,
                    &mut self.state.gestures,
                    ball,
                    &mut self.state.serve,
                    rules,
                    &input,
                    self.host,
                ))
            }
            28 | 29 => {
                let input = HitInputs {
                    ball_position: self.state.scene.position,
                    fx_names: self.state.fx_names.clone(),
                    controller_fx_offset: self.input.controller_fx_offset,
                };
                u32::from(update_rally(
                    if code == 28 {
                        RallyPhase::Return
                    } else {
                        RallyPhase::Accelerate
                    },
                    life,
                    &mut self.state.reset,
                    &mut self.state.gestures,
                    ball,
                    &mut self.state.serve,
                    &mut self.state.rally,
                    &mut self.state.hit,
                    &self.state.animations,
                    rules,
                    ms,
                    &input,
                    self.host,
                ))
            }
            30 => u32::from(life.update_round_end(rules, ball, self)),
            _ => unreachable!("Lifecycle dispatches only native handler states"),
        }
    }
    fn pole_indicator(&mut self, offset: f32) {
        RuntimeHost::pole_indicator(self.host, offset);
    }
    fn base_update(&mut self, ms: i32) {
        if !self.input.world_paused && self.state.serve.pause_block_count_0fc > 0 {
            self.state.serve.pause_block_count_0fc =
                self.state.serve.pause_block_count_0fc.wrapping_sub(ms);
        }
        self.host.world_update(ms);
    }
}

// Only reset callbacks need the full runtime. All other calls retain the
// concrete engine host and their original parameters/order.
macro_rules! forward {
    ($($name:ident($($arg:ident:$ty:ty),*) $(->$ret:ty)?;)+) => {
        $(fn $name(&mut self, $($arg:$ty),*) $(->$ret)? { self.host.$name($($arg),*) })+
    };
}
impl<H: RuntimeHost> Services for FrameAdapter<'_, H> {
    forward! {
        scoreboard(args:[i32;4]); round(args:[i32;3]); serve_bubble(args:[i32;3]);
        mega_visible(player:i32,visible:i32); mega_value(player:i32,value:i32);
        winner_visible(ui:WinnerUi,player:i32,visible:bool);
        sound(frontend:bool,sound:i32,variant:i32,volume:i32);
        controller_pop(controller:i32); controller_set(controller:i32,state:i32);
        animation(player:usize,state:i32,force:bool,blend:i32); switch_to_ai(player:usize);
        camera_offset(target:bool,offset:[f32;3],ms:u32);
        wrap_particle(player:usize,position:[f32;3],fade_ms:i32);
        fade_in(value:i32); random_range(low:i32,high:i32)->i32;
        clear_hud(); close_screen(); post_game(kind:i32,words:&[u32;70]);
    }
    fn reset_round(&mut self, life: &mut Lifecycle, ball: &mut BallMotion) {
        if let Err(e) = self.state.reset(life, ball, false, self.db, self.host) {
            self.failure = Some(e);
        }
    }
}
impl<H: RuntimeHost> FrontendServices for FrameAdapter<'_, H> {
    forward! {
        clear_pregame_handlers(); clear_postgame_handlers(); start_fade_out(ms:i32);
        fade_out_renders(first:bool,second:bool); setup_minigame_handlers(kind:i32);
        open_apt_screen(name:&str); close_apt_overlay(); audio_unpause();
        minigame_fade_complete()->bool;
    }
    fn timer_visible(&mut self, visible: i32) {
        FrontendServices::timer_visible(self.host, visible);
    }
    fn reset_minigame(&mut self, life: &mut Lifecycle, ball: &mut BallMotion) {
        if let Err(e) = self.state.reset(life, ball, true, self.db, self.host) {
            self.failure = Some(e);
        }
    }
}

#[cfg(test)]
#[path = "tetherball_runtime_reset_tests.rs"]
mod reset_tests;
