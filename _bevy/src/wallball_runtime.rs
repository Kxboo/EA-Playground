//! Complete MGWallball speed progression and game-logic frame orchestration.
use crate::wallball_rules::{RulesState, Services};

#[derive(Clone, Debug, PartialEq)]
pub struct FrameState {
    pub pause_block_ms: i32, // Minigame +fc
    pub ball_count: i32, // +108 (retail allocation supports one ball)
    pub character_count: i32, // +238
    pub ball_knots: [f32; 6], // +110
    pub movement_knots: [f32; 6], // +128
    pub ball_multiplier: f32, // Wallball +54
    pub movement_multipliers: [f32; 2], // WallballCharacter +20
}
pub trait FrameServices {
    fn disable_collisions(&mut self, rules: &mut RulesState, frame: &mut FrameState);
    fn current_world_paused(&mut self) -> bool;
    fn world_update(&mut self, rules: &mut RulesState, frame: &mut FrameState, ms: i32) -> i32;
    fn handle_actions(&mut self, rules: &mut RulesState, frame: &mut FrameState, ms: i32);
    fn ball_update(&mut self, rules: &mut RulesState, frame: &mut FrameState, ball: usize, ms: i32);
    fn powerups_update(&mut self, rules: &mut RulesState, frame: &mut FrameState, ms: i32);
    fn character_update(&mut self, rules: &mut RulesState, frame: &mut FrameState, player: usize, ms: i32);
    fn camera_update(&mut self, rules: &mut RulesState, frame: &mut FrameState, ms: i32);
    fn serve_bubble_update(&mut self, rules: &mut RulesState, frame: &mut FrameState);
    fn court_in_bounds(&mut self, rules: &mut RulesState, frame: &mut FrameState) -> bool;
    fn win_loss(&mut self, rules: &mut RulesState, frame: &mut FrameState, player: usize, won: bool, ai: bool, match_over: bool);
    fn change_state(&mut self, rules: &mut RulesState, frame: &mut FrameState, code: i32);
}
pub fn update_speed_progression(rules: &RulesState, frame: &mut FrameState) {
    let interval = i32::from(rules.progression_interval);
    assert!(interval != 0);
    let mut level = rules.progression_hits / interval;
    let remainder = rules.progression_hits - level * interval;
    let mut fraction = remainder as f32 / interval as f32;
    if level >= 5 { level = 4; fraction = 1.0; }
    // Negative levels index preceding object storage in retail; these are not
    // valid six-knot tuning arrays and are deliberately outside the safe API.
    assert!((0..=4).contains(&level));
    let i = level as usize;
    let blend = |knots: &[f32; 6]| {
        let upper = fraction * knots[i + 1];
        let lower_weight = 1.0f32 - fraction;
        (lower_weight as f64 * knots[i] as f64 + upper as f64) as f32
    };
    let ball = blend(&frame.ball_knots);
    let movement = blend(&frame.movement_knots);
    assert!((0..=1).contains(&frame.ball_count) && (0..=2).contains(&frame.character_count));
    // These two setters are complete original child bodies (one stfs each).
    if frame.ball_count > 0 { frame.ball_multiplier = ball; }
    for player in 0..frame.character_count as usize { frame.movement_multipliers[player] = movement; }
}
pub fn update_game_logic(rules: &mut RulesState, frame: &mut FrameState, ms: i32, services: &mut impl FrameServices) -> i32 {
    crate::minigame_session::advance_pause_delay(&mut frame.pause_block_ms, ms, services.current_world_paused());
    let result = services.world_update(rules, frame, ms);
    if result != 1 { return result; }
    services.handle_actions(rules, frame, ms);
    let mut ball = 0;
    while ball < frame.ball_count {
        assert!(ball < 1);
        services.ball_update(rules, frame, ball as usize, ms); ball += 1;
    }
    services.powerups_update(rules, frame, ms);
    let mut player = 0;
    while player < frame.character_count {
        assert!(player < 2);
        services.character_update(rules, frame, player as usize, ms); player += 1;
    }
    // Complete WallballCourt::Update at 0x803a9584 is a single blr.
    services.camera_update(rules, frame, ms);
    services.serve_bubble_update(rules, frame);
    1
}
/// Concrete UpdateGame composition: speed and game logic execute recovered
/// bodies; only their named child boundaries are delegated to the host.
pub struct Composition<'a, S> { pub frame: &'a mut FrameState, pub services: &'a mut S }
impl<S: FrameServices> Services for Composition<'_, S> {
    fn character_count(&self, _: &RulesState, _: usize) -> usize { self.frame.character_count.max(0) as usize }
    fn disable_collisions(&mut self, r: &mut RulesState) { self.services.disable_collisions(r, self.frame); }
    fn update_speed_progression(&mut self, r: &mut RulesState) { update_speed_progression(r, self.frame); }
    fn update_game_logic(&mut self, r: &mut RulesState, ms: i32) -> i32 { update_game_logic(r, self.frame, ms, self.services) }
    fn court_in_bounds(&mut self, r: &mut RulesState) -> bool { self.services.court_in_bounds(r, self.frame) }
    fn win_loss(&mut self, r: &mut RulesState, p: usize, won: bool, ai: bool, over: bool) { self.services.win_loss(r, self.frame, p, won, ai, over); }
    fn change_state(&mut self, r: &mut RulesState, code: i32) { self.services.change_state(r, self.frame, code); }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BallState {
    pub position: [f32; 3], // +30
    pub velocity: [f32; 3], // +70
    pub target: [f32; 3], // +40
    pub world_position: [f32; 3], // +10
    pub spin_speed: f32, // +5c
    pub angle: f32, // +60
    pub grabbed: bool, // +c0
    pub warped: bool, // +c1
    pub floor_sound_played: bool, // +c2
    pub adjust_return_speed: bool, // +120
    pub hit_wall: bool, // +121
    pub trail: u32, // +114
    pub trail_ms: i32, // +118
    pub azimuth: i32, // +124
}
impl BallState {
    /// Complete SetGrabbed; grabbing damps spin only on a false-to-true edge.
    pub fn set_grabbed(&mut self, grabbed: bool) {
        if self.grabbed == grabbed { return; }
        self.grabbed = grabbed;
        if grabbed { self.spin_speed = 0.0; }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerState {
    pub state: i32, // WallballCharacter +6c
    pub state_ms: i32, // +70
    pub held_ball: u32, // +74, opaque original handle
}
impl ServerState {
    /// Complete SetPlayerAsServer after the WorldMan lookup boundary. `None`
    /// means no active Wallball minigame; held-ball storage is then preserved.
    pub fn set_as_server(&mut self, ball: Option<(&mut BallState, u32)>) {
        self.state = 11; self.state_ms = 0;
        if let Some((ball, handle)) = ball {
            ball.set_grabbed(true); self.held_ball = handle;
        }
    }
}
/// Additional character storage used by the complete DoServe request body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeState {
    pub special_requested: bool, // +59
    pub special_enabled: bool, // +ec
    pub controller: i32, // +0c
    pub azimuth: i32, // +110
}
pub trait ServeServices {
    fn play_sound(&mut self, server: &mut ServerState, serve: &mut ServeState, id: i32, azimuth: i32, volume: i32);
    fn play_controller_sound(&mut self, server: &mut ServerState, serve: &mut ServeState, controller: i32, id: i32, volume: i32);
}
/// Complete DoServe at 0x803a625c. The animation-driven serve state handler
/// subsequently releases/launches the ball; this request itself does neither.
pub fn do_serve(server: &mut ServerState, serve: &mut ServeState, special: bool, services: &mut impl ServeServices) {
    if server.state != 11 { return; }
    server.state = 12; server.state_ms = 0; serve.special_requested = special;
    let enabled = serve.special_enabled && special;
    let azimuth = serve.azimuth;
    services.play_sound(server, serve, if enabled { 110 } else { 108 }, azimuth, 100);
    // The controller is re-read after the synchronous audio call, as retail does.
    if serve.controller != -1 {
        let controller = serve.controller;
        services.play_controller_sound(server, serve, controller, if enabled { 55 } else { 52 }, 4096);
    }
}
/// Complete SetGrabPos (0x803a3fb8). The matrix is distinct ball +80 storage;
/// `world_position` remains the single owner of +10.
pub fn set_grab_position(ball: &mut BallState, grab_matrix: &mut [f32; 16], matrix: &[f32; 16]) {
    if !ball.grabbed { return; }
    *grab_matrix = *matrix;
    ball.world_position = [matrix[12], matrix[13], matrix[14]];
}
/// Original GetHeldBallMatrix multiplication after the engine supplies the
/// current bone pose. Inputs use the original row-vector matrix convention.
pub fn held_ball_matrix(bone: &[f32; 16], character_world: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| {
        let row = i / 4; let col = i % 4;
        let x = bone[row * 4] * character_world[col];
        let y = bone[row * 4 + 1].mul_add(character_world[4 + col], x);
        let z = bone[row * 4 + 2].mul_add(character_world[8 + col], y);
        bone[row * 4 + 3].mul_add(character_world[12 + col], z)
    })
}
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchState {
    pub speed_type: i32, // +50
    pub special: bool, // +58
    pub vertical_modifier: f32, // +11c
}
#[derive(Clone, Debug)]
pub struct LaunchRequest {
    pub position: [f32; 3],
    pub speed_type: usize,
    pub direction: [f32; 3],
    pub special: bool,
    pub blend: f32,
    pub adjust_return_speed: bool,
}
pub trait LaunchServices {
    fn create_particle(&mut self, ball: &mut BallState, launch: &mut LaunchState, name: &str, position: [f32; 3]) -> u32;
    fn destroy_particle(&mut self, ball: &mut BallState, launch: &mut LaunchState, guid: u32);
    fn disable_and_destroy_particle(&mut self, ball: &mut BallState, launch: &mut LaunchState, guid: u32, delay_ms: i32);
    fn enable_particle(&mut self, ball: &mut BallState, launch: &mut LaunchState, guid: u32);
    fn play_sound(&mut self, ball: &mut BallState, launch: &mut LaunchState, id: i32, azimuth: i32, volume: i32);
}
pub const LAUNCH_LOW: [f32; 4] = [7.1, 11.75, 12.75, 13.75];
pub const LAUNCH_HIGH: [f32; 4] = [10.05, 11.75, 12.75, 13.75];
pub const LAUNCH_VERTICAL: [f32; 4] = [4.6, 5.1, 5.6, 6.0];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeRequestState {
    pub special_requested: bool,
    pub special_enabled: bool,
    pub controller: i32,
}
/// Additional live character frame storage. Existing request/state owners are
/// embedded, not copied into a second character snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterFrame {
    pub server: ServerState,
    pub serve: ServeRequestState, // +59/+ec/+0c; +110 belongs to swing.azimuth
    pub swing: SwingState,
    pub held_matrix: [f32;16], // +78; translation is also +a8 launch origin
    pub position: [f32;3], // Character +180
    pub world_matrix: [f32;16], // Character +20
    pub court_inverse: [f32;16], // Court +48
    pub special_power: bool, // +ee
    pub mega_hits: i32, // +f4
    pub meter_max: i32, // PowerMeter +4
    pub heading: f32, // +10
    pub base_move_speed: f32, // +18
    pub is_moving: bool, // +b8
    pub facing_offset: f32, // +c4
    pub position_dirty: bool, // Character +190
}
pub trait DiveServices {
    fn movement_direction(&mut self, character:&mut CharacterFrame, angle:f32);
    fn movement_facing(&mut self, character:&mut CharacterFrame, angle:f32);
    fn movement_speed(&mut self, character:&mut CharacterFrame, speed:f32);
    fn movement_stop(&mut self, character:&mut CharacterFrame);
}
/// Complete original UpdateDiveMovement geometry (428 bytes). Its forced move
/// and null-stop AddMoveInputEvent paths execute below; only CharacterMovement
/// setters remain engine boundaries. Multiplier is FrameState's existing +20.
pub fn update_dive_movement(character:&mut CharacterFrame,ball:&BallState,court:&BallCourt,game_state:i32,milliseconds:i32,multiplier:f32,services:&mut impl DiveServices)->bool {
    if game_state!=6 || !character.swing.active || !ball_in_play(ball,court){return false;}
    let local=court_transform(character.position,&character.court_inverse);
    let mut target=if ball.warped{[court.play_line_x,0.0,0.0]}else{ball.target};
    target[0]=local[0];target[1]=0.0;
    target[2]=if local[2]<target[2]{target[2]-0.8f32}else{target[2]+0.8f32};
    let destination=court_transform(target,&court.matrix);
    let mut difference=std::array::from_fn::<_,3,_>(|i|destination[i]-character.position[i]);difference[1]=0.0;
    let speed=character.base_move_speed*multiplier;
    let step=1.1f32*((milliseconds as f32*(3.0f32*speed))/1000.0f32);
    let distance=(difference[0].mul_add(difference[0],difference[2]*difference[2]) as f64).sqrt() as f32;
    if distance>step {
        let facing=crate::tetherball_angles::wrap_angle(character.heading);
        let direction=crate::tetherball_angles::wrap_angle((difference[0] as f64).atan2(difference[2] as f64) as f32);
        services.movement_direction(character,direction);services.movement_facing(character,facing);
        let speed=3.0f32*(character.base_move_speed*multiplier);services.movement_speed(character,speed);character.is_moving=true;
        true
    }else{character.position=destination;character.position_dirty=true;false}
}
fn stop_dive_move_input(character:&mut CharacterFrame,services:&mut impl DiveServices){
    let movement_state=matches!(character.server.state,0|3|4|5|6);
    services.movement_stop(character);character.is_moving=false;
    let mut facing=crate::tetherball_angles::wrap_angle(character.heading);
    if character.server.state==21{facing=crate::tetherball_angles::wrap_angle(facing+character.facing_offset);}
    services.movement_facing(character,facing);
    if character.server.state!=21 && movement_state{character.server.state=0;}
}
pub trait CharacterFrameServices: LaunchServices + DiveServices {
    fn character_count(&mut self, _character:&CharacterFrame, _rules:&RulesState, initial:usize)->usize {initial}
    fn animation_code(&mut self, character:&mut CharacterFrame) -> i32;
    fn bone_pose(&mut self, character:&mut CharacterFrame, bone:i32) -> [f32;16];
    fn history_output(&mut self, character:&mut CharacterFrame, index:i32) -> Option<[f32;3]>;
    fn backend_sound(&mut self, character:&mut CharacterFrame, id:i32, azimuth:i32, volume:i32);
    fn controller_sound(&mut self, character:&mut CharacterFrame, controller:i32, id:i32, volume:i32);
    fn hud_sound(&mut self, character:&mut CharacterFrame, id:i32, variant:i32, volume:i32);
    fn rumble(&mut self, character:&mut CharacterFrame, milliseconds:i32, strength:f32);
    fn grunt(&mut self, character:&mut CharacterFrame, mega:bool, volume:i32);
    fn camera_shake(&mut self, character:&mut CharacterFrame, milliseconds:i32, strength:f32);
    fn cycle_effect(&mut self, character:&mut CharacterFrame, rules:&mut RulesState, effect:crate::wallball_rules::Effect);
    fn set_ball_visibility(&mut self, ball:&mut BallState, visible:bool);
}
pub fn peak_controller_magnitude(character:&mut CharacterFrame, samples:i32, services:&mut impl CharacterFrameServices)->f32 {
    if character.serve.controller == -1 {return 1.0;}
    let mut peak=200.0f32;
    for index in 0..samples {
        let Some(acceleration)=services.history_output(character,index) else {break;};
        if services.history_output(character,index.wrapping_add(1)).is_none(){break;}
        let [x,y,z]=acceleration;
        let magnitude=(z.mul_add(z,x.mul_add(x,y*y)) as f64).sqrt() as f32;
        if peak < magnitude {peak=magnitude;}
    }
    if peak>800.0 {peak=800.0;}
    (peak-200.0f32)/600.0f32
}
fn cycle_for_character(character:&mut CharacterFrame,rules:&mut RulesState,count:usize,services:&mut impl CharacterFrameServices){
    struct Adapter<'a,S>{character:&'a mut CharacterFrame,services:&'a mut S}
    impl<S:CharacterFrameServices>crate::wallball_rules::CycleServices for Adapter<'_,S>{
        fn character_count(&mut self,rules:&RulesState,initial:usize)->usize{self.services.character_count(self.character,rules,initial)}
        fn effect(&mut self,rules:&mut RulesState,effect:crate::wallball_rules::Effect){self.services.cycle_effect(self.character,rules,effect);}
    }
    rules.cycle_active_synchronous(count,&mut Adapter{character,services});
}
pub fn update_serve_state(character:&mut CharacterFrame,ball:&mut BallState,launch:&mut LaunchState,grab_matrix:&mut[f32;16],court:&BallCourt,multiplier:f32,rules:Option<&mut RulesState>,characters:usize,services:&mut impl CharacterFrameServices){
    match character.server.state {
        12 if character.server.state_ms>650=>{
            let origin=court_transform([character.held_matrix[12],character.held_matrix[13],character.held_matrix[14]],&character.court_inverse);
            let peak=peak_controller_magnitude(character,20,services);
            launch.vertical_modifier=(-(peak-0.2f32))*0.5f32;
            let mega=character.serve.special_enabled && character.serve.special_requested;
            let request=LaunchRequest{position:origin,speed_type:if mega{3}else{0},direction:[1.0,0.0,0.3],special:character.special_power,blend:1.0,adjust_return_speed:false};
            launch_ball(ball,launch,court,multiplier,&request,services);
            services.rumble(character,if mega{250}else{200},if mega{1.0}else{0.82});
            services.grunt(character,mega,if mega{100}else{50});
            ball.set_grabbed(false);character.server.held_ball=0;
            if let Some(rules)=rules {cycle_for_character(character,rules,characters,services);}
            character.server.state=13;
        }
        13=>{
            if !matches!(services.animation_code(character),202|203){character.server.state=0;character.server.state_ms=0;}
        }
        _=>{}
    }
    if character.server.held_ball!=0 {
        let bone=services.bone_pose(character,16);
        let matrix=held_ball_matrix(&bone,&character.world_matrix);
        character.held_matrix=matrix;set_grab_position(ball,grab_matrix,&matrix);
        services.set_ball_visibility(ball,true);
    }
}
fn increment_power(character:&mut CharacterFrame,services:&mut impl CharacterFrameServices){
    if character.swing.meter_count<character.meter_max && character.swing.meter_count.wrapping_add(1)>=character.meter_max {
        services.hud_sound(character,34,0,100);
    }
    let value=character.swing.meter_count.wrapping_add(1);
    character.swing.meter_count=if value<character.meter_max{value}else{character.meter_max};
}
/// Complete UpdateSwingState control/launch body. Engine movement and resources
/// execute synchronously; elapsed +70 is advanced by the outer character frame.
pub fn update_swing_state(character:&mut CharacterFrame,ball:&mut BallState,launch:&mut LaunchState,court:&BallCourt,multiplier:f32,movement_multiplier:f32,rules:&mut RulesState,characters:usize,game_state:i32,milliseconds:i32,mega_threshold:i32,history_samples:i32,services:&mut impl CharacterFrameServices){
    let reaching=(14..=17).contains(&character.server.state);
    let lead=if reaching{150}else{30};let duration=if reaching{400}else{310};
    if !character.swing.hit_processed && character.server.state_ms>lead && character.server.state_ms<lead+duration && game_state!=4 {
        if reaching && !update_dive_movement(character,ball,court,game_state,milliseconds,movement_multiplier,services){stop_dive_move_input(character,services);}
        let position=court_transform(character.position,&character.court_inverse);
        let difference=std::array::from_fn::<_,3,_>(|i|ball.position[i]-position[i]);
        if difference[0]<=0.8 && difference[0]>=-0.55 && difference[2].abs()<=0.9 {
            let forehand=matches!(character.server.state,8|10|14|15);
            let timing=if forehand{1.0f32-character.swing.timing}else{character.swing.timing};
            let offset=if forehand{0.3f32}else{-0.3f32};
            let mega_available=character.swing.mega_enabled && character.swing.meter_count==mega_threshold;
            let sound=if character.swing.modifier==0 && mega_available{Some((106,55))}
                else if reaching{Some((109,51))}else if character.swing.modifier==1 || character.server.state==8{Some((104,51))}
                else if character.server.state==7{Some((105,50))}else{None};
            if let Some((backend,controller_sound))=sound {
                let azimuth=character.swing.azimuth;services.backend_sound(character,backend,azimuth,100);
                if character.serve.controller != -1 {let controller=character.serve.controller;services.controller_sound(character,controller,controller_sound,4096);}
            }
            let mut speed_type=0;
            if character.swing.modifier==0 && character.swing.meter_count>=2 && character.swing.mega_enabled && character.swing.meter_count==mega_threshold && !reaching {
                services.camera_shake(character,400,0.2);
                character.swing.meter_count=0;character.mega_hits=character.mega_hits.wrapping_add(1);speed_type=2;
                services.rumble(character,200,1.0);
            }else{increment_power(character,services);services.rumble(character,200,0.82);}
            let mut angle=offset+0.8f32.mul_add(timing-0.5f32,1.5707964f32);
            angle=crate::tetherball_angles::wrap_angle(angle);
            let peak=if character.swing.special{character.swing.charge}else{peak_controller_magnitude(character,if reaching{20}else{history_samples},services)};
            let adjust=character.swing.modifier==1;
            launch.vertical_modifier=if adjust{peak*0.5f32}else{(-(peak-0.4f32))*0.5f32};
            let (sin,cos)=crate::character_input::ea_sin_cos(angle);
            let request=LaunchRequest{position:ball.position,speed_type,direction:[sin,0.0,cos],special:character.special_power,blend:peak,adjust_return_speed:adjust};
            launch_ball_source(ball,launch,court,multiplier,&request,true,services);
            cycle_for_character(character,rules,characters,services);character.swing.hit_processed=true;
        }
    }
    let animation=services.animation_code(character);
    if !matches!(animation,198|199|200|201|206|207){character.server.state=0;}
}
#[derive(Clone, Debug, PartialEq)]
pub struct SwingState {
    pub active: bool, // +40
    pub timing: f32, // +44
    pub hit_processed: bool, // +48
    pub modifier: i32, // +4c
    pub good_hit: bool, // +50
    pub charge: f32, // +54
    pub special: bool, // +58
    pub meter_count: i32, // PowerMeter +8
    pub mega_enabled: bool, // +ed
    pub azimuth: i32, // +110
}
pub trait SwingServices {
    fn play_grunt(&mut self, server: &mut ServerState, swing: &mut SwingState, mega: bool, volume: i32);
    fn play_sound(&mut self, server: &mut ServerState, swing: &mut SwingState, id: i32, azimuth: i32, volume: i32);
    fn set_character_animation(&mut self, server: &mut ServerState, swing: &mut SwingState, restart: bool);
}
pub fn is_swing_state(state: i32) -> bool { matches!(state,7|8|9|10|14|15|16|17) }
/// CalcTimeToXPos leaves r3 (the original object handle) unchanged for zero/NaN
/// X velocity. Exposing that argument preserves this otherwise unusual result.
pub fn calc_time_to_x(ball: &BallState, court: &BallCourt, x: f32, object_handle: u32) -> i32 {
    let vx=ball.velocity[0];
    let value=if vx>0.0 {
        1000.0f32 * ((court.half_length-ball.position[0])/vx + (court.half_length-x)/(0.6f32*vx))
    } else if vx<0.0 { 1000.0f32*((x-ball.position[0])/vx) } else { return object_handle as i32; };
    if !value.is_finite() || value>=2147483648.0 || value < -2147483648.0 { i32::MIN } else { value as i32 }
}
/// Complete CalcHitTimingWindow. Invalid inputs preserve the supplied output
/// locals, just as the original body does (DoSwing's caller stack was not reset).
pub fn calc_hit_window(swing: &SwingState, ball: Option<&BallState>, court: &BallCourt, position: [f32;3], reaching: bool, object_handle: u32, window: &mut [i32;2]) -> bool {
    let Some(ball)=ball else {return false;};
    if !ball_in_play(ball,court) || !swing.active {return false;}
    let lead=if reaching {150} else {30};let duration=if reaching {400} else {310};
    window[0]=calc_time_to_x(ball,court,position[0]+0.8,object_handle).wrapping_sub(lead+duration);
    window[1]=calc_time_to_x(ball,court,position[0]+(-0.55f32),object_handle).wrapping_sub(lead);
    true
}
/// Complete DoSwing. Position is the character's world +180 transformed by
/// Court +48; the real matrix computation executes here, not in a service.
/// `window` explicitly supplies retail's surviving caller-stack local values
/// when CalcHitTimingWindow declines to write them. Valid rallies overwrite it.
pub fn do_swing(server:&mut ServerState,swing:&mut SwingState,ball:&BallState,launch:&LaunchState,court:&BallCourt,character_position:[f32;3],court_inverse:&[f32;16],swing_type:i32,mut modifier:i32,special:bool,charge:f32,mega_threshold:i32,object_handle:u32,window:&mut[i32;2],services:&mut impl SwingServices)->bool {
    if !swing.active || matches!(server.state,11|12|13) || is_swing_state(server.state) || !ball.hit_wall {return false;}
    swing.special=special;swing.charge=charge;
    let position=court_transform(character_position,court_inverse);
    let target=if ball.warped {[court.play_line_x,0.0,0.0]} else {ball.target};
    let reaching=(target[2]-position[2]).abs()>0.85;
    let side=reaching && position[2]>=target[2];
    let valid=calc_hit_window(swing,Some(ball),court,position,reaching,object_handle,window);
    window[0]=window[0].wrapping_add(20);
    let start=window[0];let end=window[1];
    swing.timing=if start<0 && valid {
        let ratio=(start.wrapping_neg() as f32)/(end.wrapping_sub(start) as f32);
        let clamped=if ratio>0.8 {0.8} else if ratio<0.2 {0.2} else {ratio};
        (clamped-0.2f32)/0.6f32
    } else if start<0 {0.0} else {1.0};
    swing.good_hit=start<0 && end>0;
    if server.state==18 && !swing.good_hit {return false;}
    if launch.speed_type>1 {modifier=1;}
    let mega=swing.meter_count>=2 && modifier==0 && swing.mega_enabled && swing.meter_count==mega_threshold;
    match swing_type {
        0 => server.state=if reaching {if side {14} else {15}} else if mega {10} else {8},
        1 => server.state=if reaching {if side {16} else {17}} else if mega {9} else {7},
        _=>{}
    }
    services.play_grunt(server,swing,mega,if mega || reaching {100} else {40});
    if !swing.good_hit {let azimuth=swing.azimuth;services.play_sound(server,swing,107,azimuth,100);}
    swing.modifier=modifier;server.state_ms=0;swing.hit_processed=false;
    services.set_character_animation(server,swing,true);
    true
}
pub fn compute_return_time(ball: &BallState, court: &BallCourt, before_wall: bool) -> f32 {
    let front = court.half_length - court.radius;
    let initial = if before_wall { (front - ball.position[0]) / ball.velocity[0] } else { 0.0 };
    let speed = if ball.velocity[0] >= 0.0 { 0.6f32 * ball.velocity[0] } else { -ball.velocity[0] };
    initial + (front - court.play_line_x) / speed
}
/// Complete ComputeFinalYPos, including its original launch-velocity correction.
pub fn compute_final_y(ball: &mut BallState, court: &BallCourt, before_wall: bool, mut time: f32) -> f32 {
    let mut vy = ball.velocity[1]; let mut y = ball.position[1]; let g = court.gravity[1];
    let mut wall_contact = 0.0;
    if before_wall {
        wall_contact = (court.half_length - ball.position[0]) / ball.velocity[0];
        let limit = -(0.5f32 * g).mul_add(wall_contact, -((-y) / wall_contact));
        if vy < limit { vy = limit; }
        time -= wall_contact;
        y += vy.mul_add(wall_contact, wall_contact * ((0.5f32 * g) * wall_contact));
        vy = g.mul_add(wall_contact, vy);
        vy *= if vy > 0.0 { -0.6 } else { 0.6 };
    }
    if vy > 0.0 { vy = -vy; }
    let root = (vy.mul_add(vy, -((2.0f32 * g) * (y - court.radius))) as f64).sqrt() as f32;
    let first = -(vy + root) / g; let second = -(vy - root) / g;
    let ground_contact = if first > second { first } else { second };
    let remainder = time - ground_contact;
    let bounce = -g.mul_add(ground_contact, vy);
    let height = (0.9f32 * bounce).mul_add(remainder, remainder * ((0.5f32 * g) * remainder));
    if before_wall && height < 2.0f32 * court.radius {
        let sum = (-(g * remainder)) / 0.72f32 + (g * ground_contact) / 0.6f32;
        let mut corrected = -g.mul_add(wall_contact, -sum);
        if corrected > LAUNCH_VERTICAL[3] { corrected = LAUNCH_VERTICAL[3]; }
        if ball.velocity[1] < corrected { ball.velocity[1] = corrected; }
    }
    height
}
fn launch_particles(ball: &mut BallState, launch: &mut LaunchState, court: &BallCourt, position: [f32; 3], services: &mut impl LaunchServices) {
    let before_wall = position[0] < 0.0;
    if ball.trail != court.invalid_guid {
        let guid = ball.trail; services.destroy_particle(ball, launch, guid);
        ball.trail = court.invalid_guid; ball.trail_ms = 0;
    }
    let speed = launch.speed_type;
    let name = match speed {
        2 => "pg_wallball_balltrail_3", 1 => "pg_wallball_balltrail_2", 3 => "pg_wallball_balltrail_4",
        _ if launch.special => "pg_wallball_balltrail_power_hand", _ => "pg_wallball_balltrail_1",
    };
    let position = ball.world_position;
    ball.trail = services.create_particle(ball, launch, name, position);
    if (1..=3).contains(&speed) { let azimuth=ball.azimuth; services.play_sound(ball, launch, 116, azimuth, 100); }
    let guid=ball.trail; services.enable_particle(ball, launch, guid);
    if before_wall {
        let position=ball.world_position;
        let guid=services.create_particle(ball, launch, "pg_wallball_hit_impact", position);
        services.disable_and_destroy_particle(ball, launch, guid, 2500);
    }
}
/// Complete LaunchBall and ComputeVelocityAndFinalPosAlt2. `speed_multiplier`
/// is FrameState's live +54 value, supplied without a duplicate owner.
/// Retail leaves grabbed unchanged: the animation handler releases separately.
pub fn launch_ball(ball: &mut BallState, launch: &mut LaunchState, court: &BallCourt, speed_multiplier: f32, request: &LaunchRequest, services: &mut impl LaunchServices) {
    launch_ball_source(ball,launch,court,speed_multiplier,request,false,services);
}
fn launch_ball_source(ball: &mut BallState, launch: &mut LaunchState, court: &BallCourt, speed_multiplier: f32, request: &LaunchRequest, position_is_ball:bool, services: &mut impl LaunchServices) {
    let speed=request.speed_type; assert!(speed < 4);
    ball.adjust_return_speed=request.adjust_return_speed; ball.hit_wall=false; launch.special=request.special;
    let d=request.direction;
    let magnitude=(d[2].mul_add(d[2],d[0].mul_add(d[0],d[1]*d[1])) as f64).sqrt() as f32;
    let normalized=d.map(|v|v*(1.0f32/magnitude));
    let before_wall=request.position[0]<0.0; ball.position=request.position;
    ball.velocity[0]=speed_multiplier * request.blend.mul_add(LAUNCH_HIGH[speed], (1.0f32-request.blend)*LAUNCH_LOW[speed]);
    if launch.special { ball.velocity[0] *= 1.25; }
    if ball.adjust_return_speed { ball.velocity[0] *= 0.5; }
    if ball.velocity[0]<7.1 { ball.velocity[0]=7.1; }
    ball.velocity[1]=launch.vertical_modifier.mul_add(LAUNCH_VERTICAL[speed], LAUNCH_VERTICAL[speed]);
    ball.velocity[2]=normalized[2]*(ball.velocity[0]/normalized[0]);
    let time=compute_return_time(ball,court,before_wall);
    ball.target[0]=court.play_line_x;
    ball.target[1]=compute_final_y(ball,court,before_wall,time);
    ball.target[2]=compute_return_z(ball,court,time);
    launch.speed_type=speed as i32;
    let source=if position_is_ball{ball.position}else{request.position};
    launch_particles(ball,launch,court,source,services);
    ball.spin_speed=(-43.9823f32).mul_add(request.blend,18.849556f32*(1.0f32-request.blend));
    if ball.trail != court.invalid_guid { let guid=ball.trail;services.destroy_particle(ball,launch,guid);ball.trail=court.invalid_guid; }
    launch.speed_type=speed as i32;ball.floor_sound_played=false;
    let source=if position_is_ball{ball.position}else{request.position};
    launch_particles(ball,launch,court,source,services);
}
/// Live court/static tuning. Gravity is initialized to [0,-9.81,0] by the
/// original wallball_cpp initializer; dimensions and radius are runtime globals.
#[derive(Clone, Debug, PartialEq)]
pub struct BallCourt {
    pub half_width: f32,
    pub half_length: f32,
    pub radius: f32,
    pub play_line_x: f32,
    pub gravity: [f32; 3],
    pub matrix: [f32; 16], // Court +8, original row-vector layout
    pub invalid_guid: u32,
}
pub trait BallServices {
    /// Complete UpdatePowerUps is a synchronous dependency. It may change the
    /// live ball's position/velocity/flags before the original rebound stores.
    fn update_powerups(&mut self, ball: &mut BallState);
    fn create_bounce_particle(&mut self, ball: &mut BallState, position: [f32; 3]) -> u32;
    fn destroy_particle(&mut self, ball: &mut BallState, guid: u32, delay_ms: i32);
    fn play_sound(&mut self, ball: &mut BallState, id: i32, azimuth: i32, volume: i32);
    fn move_trail(&mut self, ball: &mut BallState, guid: u32, position: [f32; 3]);
    fn enable_trail(&mut self, ball: &mut BallState, guid: u32, enabled: bool);
    fn audio_azimuth(&mut self, ball: &mut BallState, position: [f32; 3]) -> i32;
}
pub const BOUNCE_PARTICLE: &str = "pg_wallball_ball_impact";
fn madd(a: f32, b: f32, c: f32) -> f32 { a.mul_add(b, c) }
fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { std::array::from_fn(|i| a[i] + b[i]) }
fn scale3(v: [f32; 3], s: f32) -> [f32; 3] { v.map(|x| x * s) }
fn integrate_position(p: [f32; 3], v: [f32; 3], g: [f32; 3], t: f32) -> [f32; 3] {
    let half_t_squared = 0.5f32 * (t * t);
    add3(p, add3(scale3(v, t), scale3(g, half_t_squared)))
}
fn court_transform(p: [f32; 3], m: &[f32; 16]) -> [f32; 3] {
    std::array::from_fn(|i| {
        let value = p[0] * m[i];
        let value = madd(p[1], m[4 + i], value);
        let value = madd(p[2], m[8 + i], value);
        madd(1.0, m[12 + i], value)
    })
}
pub fn ball_in_play(ball: &BallState, court: &BallCourt) -> bool {
    !ball.grabbed && ball.velocity[0].abs() > 0.0
        && ball.position[0] <= court.half_length && ball.position[0] >= -court.half_length
        && ball.position[2] <= court.half_width && ball.position[2] >= -court.half_width
}
fn surface_type(boundary: i32, position: [f32; 3]) -> i32 {
    match boundary { 0 => 1, 3 if position[1] > 0.9 => 2, _ => 0 }
}
/// Complete false branch of ComputeFinalYPos. `compute_final_y` also exposes
/// the launch branch and its original velocity correction.
pub fn compute_return_y(ball: &BallState, court: &BallCourt, time: f32) -> f32 {
    let mut vy = ball.velocity[1];
    if vy > 0.0 { vy = -vy; }
    let g = court.gravity[1];
    let discriminant = madd(vy, vy, -((2.0f32 * g) * (ball.position[1] - court.radius)));
    let root = (discriminant as f64).sqrt() as f32;
    let first = -(vy + root) / g;
    let second = -(vy - root) / g;
    let contact = if first > second { first } else { second };
    let remainder = time - contact;
    let bounce = -madd(g, contact, vy);
    let half_g = 0.5f32 * g;
    let quadratic = remainder * (half_g * remainder);
    madd(0.9f32 * bounce, remainder, quadratic)
}
/// Complete ComputeFinalZPos; repeated predicted side bounces do not mutate ball.
pub fn compute_return_z(ball: &BallState, court: &BallCourt, mut time: f32) -> f32 {
    let limit = court.half_width - court.radius;
    let mut position = ball.position[2];
    let mut velocity = ball.velocity[2];
    loop {
        let candidate = madd(velocity, time, position);
        if candidate.abs() <= limit { return candidate; }
        let boundary = if candidate > limit { limit } else { -limit };
        let contact = (boundary - position) / velocity;
        position = boundary;
        time -= contact;
        velocity = -0.6f32 * velocity;
    }
}
/// Complete Wallball::Update numerical and ordered service body (0x803a33bc).
/// Each service executes synchronously; there is no external collision model.
pub fn update_ball(ball: &mut BallState, court: &BallCourt, milliseconds: i32, services: &mut impl BallServices) {
    if ball.grabbed || ball.warped { return; }
    let dt = milliseconds as f32 / 1000.0f32;
    let candidate = integrate_position(ball.position, ball.velocity, court.gravity, dt);
    let mut candidate_velocity = add3(ball.velocity, scale3(court.gravity, dt));
    let front = court.half_length - court.radius;
    let side = court.half_width - court.radius;
    let mut collision_position = [0.0; 3];
    let mut collided = false;
    let mut front_hit = false;
    let mut boundary = 1;
    if candidate[0] >= front {
        let time = (front - ball.position[0]) / ball.velocity[0];
        services.update_powerups(ball);
        candidate_velocity[1] = ball.velocity[1];
        let remainder = dt - time;
        ball.velocity[0] = -0.6f32 * ball.velocity[0];
        ball.position[0] = madd(ball.velocity[0], remainder, front);
        candidate_velocity[1] *= if candidate_velocity[1] > 0.0 { -0.6f32 } else { 0.6f32 };
        collision_position = integrate_position(ball.position, ball.velocity, court.gravity, time);
        collided = true; front_hit = true;
    } else { ball.position[0] = candidate[0]; }
    let side_boundary = if candidate[2] <= -side { boundary = 2; Some(-side) }
                        else if candidate[2] >= side { boundary = 3; Some(side) }
                        else { None };
    if let Some(side_boundary) = side_boundary {
        let time = (side_boundary - ball.position[2]) / ball.velocity[2];
        ball.velocity[2] *= -0.6f32;
        ball.position[2] = madd(ball.velocity[2], dt - time, side_boundary);
        if !collided {
            collision_position = integrate_position(ball.position, ball.velocity, court.gravity, time);
            collided = true;
        }
    } else { ball.position[2] = candidate[2]; }
    if candidate[1] <= court.radius {
        if ball.position[1] > court.radius {
            let gy = court.gravity[1];
            let time = if gy != 0.0 {
                let discriminant = madd(ball.velocity[1], ball.velocity[1], -((2.0f32 * gy) * (ball.position[1] - court.radius)));
                let root = (discriminant as f64).sqrt() as f32;
                let first = (-ball.velocity[1] + root) / gy;
                let second = (-ball.velocity[1] - root) / gy;
                if first >= 0.0 && first <= dt { first } else { second }
            } else { -court.radius / ball.velocity[1] };
            if !collided {
                collision_position = integrate_position(ball.position, ball.velocity, court.gravity, time);
                collided = true; boundary = 0;
            }
            let remainder = dt - time;
            let bounce = -0.6f32 * madd(gy, time, ball.velocity[1]);
            let quadratic_factor = (0.5f32 * gy) * remainder;
            let linear = bounce * remainder;
            ball.velocity[1] = bounce;
            ball.position[1] = court.radius + madd(remainder, quadratic_factor, linear);
        } else {
            ball.position[1] = court.radius; ball.velocity[1] = 0.0;
        }
    } else {
        ball.position[1] = candidate[1]; ball.velocity[1] = candidate_velocity[1];
    }
    if collided {
        let world_collision = court_transform(collision_position, &court.matrix);
        let particle = services.create_bounce_particle(ball, world_collision);
        services.destroy_particle(ball, particle, 2000);
        ball.spin_speed *= 0.4f32;
        if ball_in_play(ball, court) {
            let sound = match surface_type(boundary, collision_position) {
                0 => 112,
                1 if !ball.floor_sound_played => 114,
                1 => 115,
                2 => 113,
                _ => unreachable!(),
            };
            services.play_sound(ball, sound, ball.azimuth, 100);
            if sound == 114 { ball.floor_sound_played = true; }
        }
    }
    if front_hit {
        ball.hit_wall = true;
        if ball.adjust_return_speed {
            let previous_vx = ball.velocity[0];
            let mut factor = 1.1f32;
            loop {
                ball.velocity[0] = previous_vx * factor;
                let time_to_line = -(front - court.play_line_x) / (0.6f32 * ball.velocity[0]);
                let gy = court.gravity[1];
                let discrim = madd(candidate_velocity[1], candidate_velocity[1], -((2.0f32 * gy) * candidate[1]));
                let root = (discrim as f64).sqrt() as f32;
                let first = -(candidate_velocity[1] + root) / gy;
                let second = -(candidate_velocity[1] - root) / gy;
                let contact = if first > 0.0 { first } else { second };
                let remainder = time_to_line - contact;
                if remainder <= 0.0 { break; }
                let bounced = -0.6f32 * madd(gy, contact, candidate_velocity[1]);
                let quadratic = remainder * ((0.5f32 * gy) * remainder);
                let height = madd(bounced, remainder, quadratic);
                let done = height >= court.radius;
                factor += 0.1f32;
                if done { break; }
            }
            let numerator = front - court.play_line_x;
            let time = if ball.velocity[0] < 0.0 { -numerator / ball.velocity[0] } else { numerator / ball.velocity[0] };
            ball.target[0] = court.play_line_x;
            ball.target[1] = compute_return_y(ball, court, time);
            ball.target[2] = compute_return_z(ball, court, time);
        }
    }
    ball.world_position = court_transform(ball.position, &court.matrix);
    if ball.trail != court.invalid_guid {
        if ball.trail_ms < 1000 {
            let guid = ball.trail;
            services.move_trail(ball, guid, ball.world_position);
            services.enable_trail(ball, guid, true);
            ball.trail_ms = ball.trail_ms.wrapping_add(milliseconds);
        } else {
            let guid = ball.trail;
            services.destroy_particle(ball, guid, 500);
            ball.trail = court.invalid_guid; ball.trail_ms = 0;
        }
    }
    ball.angle = madd(dt, ball.spin_speed, ball.angle);
    let turn = f32::from_bits(0x40c9_0fdb);
    while ball.angle > turn { ball.angle -= turn; }
    while ball.angle < 0.0 { ball.angle += turn; }
    ball.azimuth = services.audio_azimuth(ball, ball.world_position);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryState {
    pub state_ms: u32, // +1cc
    pub round_index: i32, // +1ec
    pub phase_flags: [bool; 4], // +1d1..1d4
    pub scoreboard_visible: bool, // +1f2
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryEffect {
    Round { round: i32, visible: bool, kind: i32 },
    Scoreboard { visible: i32, animate: i32, player: i32, score: i32 },
    ControlIndicator { player: usize, enabled: bool, milliseconds: i32 },
    Server(usize),
    Sound { id: i32, variant: i32, volume: i32 },
    FadeIn(i32),
}
pub trait EntryServices {
    fn entry_effect(&mut self, state_code: &mut i32, entry: &mut EntryState, rules: &mut RulesState, frame: &mut FrameState, effect: EntryEffect);
}
/// Complete state entry, including state stores for unsupported/negative codes.
/// `state_code` borrows the existing shared base +34 owner (Session.game_state).
pub fn change_game_state(state_code: &mut i32, entry: &mut EntryState, rules: &mut RulesState, frame: &mut FrameState, code: i32, services: &mut impl EntryServices) {
    *state_code = code; entry.state_ms = 0;
    match code {
        3 => {
            services.entry_effect(state_code, entry, rules, frame, EntryEffect::Round { round: entry.round_index, visible: true, kind: 1 });
            entry.phase_flags = [false; 4];
        }
        6 => {
            services.entry_effect(state_code, entry, rules, frame, EntryEffect::Round { round: entry.round_index, visible: false, kind: 1 });
            if entry.scoreboard_visible {
                services.entry_effect(state_code, entry, rules, frame, EntryEffect::Scoreboard { visible: 0, animate: 0, player: 0, score: 0 });
            }
            entry.scoreboard_visible = false; entry.phase_flags = [true; 4];
            let mut player = 0;
            while player < frame.character_count {
                assert!(player < 2);
                services.entry_effect(state_code, entry, rules, frame, EntryEffect::ControlIndicator { player: player as usize, enabled: true, milliseconds: 750 });
                if player == rules.active {
                    services.entry_effect(state_code, entry, rules, frame, EntryEffect::Server(player as usize));
                }
                player += 1;
            }
            services.entry_effect(state_code, entry, rules, frame, EntryEffect::Sound { id: 35, variant: 0, volume: 100 });
        }
        9 => services.entry_effect(state_code, entry, rules, frame, EntryEffect::FadeIn(-1)),
        _ => {}
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionState {
    pub human_count: i32, // base +40, distinct from +238 characters
    pub character_controllers: [i32; 2], // WallballCharacter +0c; -1 is AI
}
pub trait ActionServices {
    /// Reads the real Controller event175 active byte, not raw button state.
    fn pause_pressed(&mut self, actions: &mut ActionState, frame: &mut FrameState, controller: i32) -> bool;
    fn pause_menu_open(&self) -> bool; // existing base +4e owner
    /// Synchronous original Minigame::OpenPauseMenu(kind6, controller).
    /// Implementations bind +fc to frame.pause_block_ms and +4e to their live
    /// base owner; a successful open affects subsequent controllers immediately.
    fn open_pause(&mut self, actions: &mut ActionState, frame: &mut FrameState, kind: i32, controller: i32);
}
pub fn get_player_character(actions: &ActionState, frame: &FrameState, controller: i32) -> Option<usize> {
    let mut player = 0;
    while player < frame.character_count {
        assert!(player < 2);
        let id = actions.character_controllers[player as usize];
        if id != -1 && id == controller { return Some(player as usize); }
        player += 1;
    }
    None
}
/// Complete HandleActions and native player-to-controller lookup. Milliseconds
/// is intentionally absent: the original nominal argument is never read.
pub fn handle_actions(actions: &mut ActionState, frame: &mut FrameState, services: &mut impl ActionServices) {
    let mut controller = 0;
    while controller < actions.human_count {
        if let Some(player) = get_player_character(actions, frame, controller) {
            let controller_id = actions.character_controllers[player];
            if services.pause_pressed(actions, frame, controller_id) && !services.pause_menu_open() && frame.pause_block_ms <= 0 {
                services.open_pause(actions, frame, 6, controller);
            }
        }
        controller += 1;
    }
}
