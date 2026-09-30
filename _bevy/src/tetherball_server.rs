//! Live SetUpServer (0x8039b1b0), including the Grab body (0x8039e5a8).
use crate::tetherball_runtime::Runtime;
/// Queries and effects are synchronous; engine callbacks may not reenter game
/// logic. Marker count is read once after both animation state requests.
pub trait ServerServices {
    fn random_server(&mut self) -> i32;
    fn server_camera(&mut self) -> u32;
    fn camera_target(&mut self, camera: u32, position: [f32; 3]);
    fn camera_direction(&mut self, camera: u32, direction: [f32; 3]);
    fn server_animation(&mut self, player: usize, state: i32, force: bool, blend: i32);
    fn marker_count(&mut self, player: usize) -> i32;
    fn marker_id(&mut self, player: usize, index: i32) -> i32;
    fn marker_matrix(&mut self, player: usize, index: i32) -> u32;
    fn null_trail(&mut self) -> u32;
    fn destroy_trail(&mut self, handle: u32, fade: i32);
}
pub fn setup_server(runtime: &mut Runtime, host: &mut impl ServerServices) -> Result<(), String> {
    let random = host.random_server();
    if !(0..=1).contains(&random) {
        return Err("server RNG result outside native [0,1]".into());
    }
    let server = crate::tetherball_reset::select_server(&mut runtime.state.reset, random);
    let player = server as usize;
    let receiver = 1 - player;
    runtime.life.server = player;
    runtime.life.receiver = receiver;
    runtime.life.focus_player = server;
    runtime.state.reset.receiver_220 = receiver as i32;
    let camera = host.server_camera();
    let (position, direction) =
        crate::tetherball_reset::server_camera(&runtime.state.reset, server);
    host.camera_target(camera, position);
    host.camera_direction(camera, direction);
    runtime.state.reset.ai_initial_values_27c = if player == 0 { [1, -1] } else { [-1, 1] };
    runtime.state.reset.server_side_flags = [player == 0, player == 1, player == 1, player == 0];
    crate::tetherball_animation_init::initialize_player_animations(
        &mut runtime.life,
        &runtime.state.reset,
        &mut runtime.state.serve,
        &mut runtime.state.animations,
    );
    host.server_animation(player, 58, false, -1);
    host.server_animation(receiver, runtime.life.lose_animations[receiver], false, -1);
    let count = host.marker_count(player);
    for index in 0..count {
        if host.marker_id(player, index) == 63 {
            runtime.state.reset.game_marker_matrix_278 = host.marker_matrix(player, index);
        }
    }
    if runtime.ball.angular_velocity == 0. {
        runtime.ball.angle =
            crate::tetherball_angles::wrap_angle(runtime.state.reset.start_angles_248[player]);
        runtime.state.reset.current_ball_owner_074 = runtime.state.reset.player_handles_120[player];
        runtime.state.reset.current_ball_matrix_078 = runtime.state.reset.game_marker_matrix_278;
        runtime.ball.grabbed = true;
        runtime.ball.desired_radius = runtime.ball.radius;
        runtime.ball.spinning_up = false;
        runtime.ball.spinning_down = false;
        runtime.ball.angular_velocity = 0.;
        runtime.ball.secondary_velocity = 0.;
        // Reload the original global sentinel for each compare and after each
        // destruction; retain the scene's current global-token projection.
        for trail in &mut runtime.state.scene.trails {
            let null = host.null_trail();
            runtime.state.scene.null_trail = null;
            if *trail != null {
                host.destroy_trail(*trail, 0);
                let null = host.null_trail();
                runtime.state.scene.null_trail = null;
                *trail = null;
            }
        }
    }
    Ok(())
}
