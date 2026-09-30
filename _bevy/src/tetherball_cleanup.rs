//! Enclosing MGTetherball::UnInitialize (0x803973d8), composed with the
//! recovered base and ball cleanup bodies. Engine objects remain services.
use crate::tetherball_ball_init::{BallCleanupServices, uninitialize_ball};
use crate::tetherball_runtime::Runtime;
use crate::tetherball_startup::{
    Effect as StartupEffect, StartupServices, StartupState, uninitialize_base,
};

#[derive(Clone, Debug, PartialEq)]
pub enum CleanupEffect {
    DespawnCharacter { character: u32, destroy: bool },
    PopController { controller: u32 },
    DeleteBigfileAssets(u32),
    FreeBall(u32),
    ResetWorldPlayerAnimation(u32),
    SwitchToLocalControl(u32),
    ClearCharacterActivityFlag(u32),
    PurgeCallbacks,
    TimerVisible(i32),
    PoleIndicator(f32),
    UnduckMusic,
    StopMusic,
    UnloadAudio(i32),
    RestoreArea,
}

pub trait CleanupServices: StartupServices + BallCleanupServices {
    fn cleanup_effect(&mut self, effect: CleanupEffect);
    fn world_player_character(&mut self, index: i32) -> u32;
    /// Character +12c: nonzero suppresses SwitchToLocalControl.
    fn character_local_control_block(&mut self, character: u32) -> u8;
    /// Independent game particle sentinel at 0x80601f60, not ball's trail token.
    fn cleanup_null_game_fx(&mut self) -> u32;
}

/// Complete enclosing cleanup for represented two-player state. Invalid ball
/// ownership corresponds to native invalid dereferences; stop rather than invent
/// a successful exit. Base pointers/AI handles/counts remain as the native leaves
/// them; the host must discard this session after engine effects finish.
pub fn uninitialize(
    runtime: &mut Runtime,
    state: &mut StartupState,
    host: &mut impl CleanupServices,
) -> Result<(), String> {
    // Native cmpw interprets +210 as signed, even though the shared owner is
    // usize. Negative raw words skip both actor loops without changing +210.
    let player_count = runtime.life.player_count as u32 as i32;
    if player_count > 2 {
        return Err("unsupported tetherball cleanup character count".into());
    }
    if runtime.state.reset.ball_handle_104 == 0 || state.ball_resources.is_none() {
        return Err("tetherball cleanup requires live ball resource ownership".into());
    }
    runtime.state.frontend.field_424 = 0;
    for player in 0..player_count {
        let p = player as usize;
        if state.player_init.player_init_flags_130[p] != 0 {
            host.cleanup_effect(CleanupEffect::DespawnCharacter {
                character: runtime.state.reset.player_handles_120[p],
                destroy: true,
            });
            runtime.state.reset.player_handles_120[p] = 0;
        }
    }
    host.effect(StartupEffect::PlaceableVisible {
        handle: runtime.state.reset.pole_handle_184,
        visible: false,
    });
    host.effect(StartupEffect::PlaceableVisible {
        handle: state.alternate_pole_188,
        visible: true,
    });
    let controller = host.controller_handle(0);
    host.cleanup_effect(CleanupEffect::PopController { controller });
    uninitialize_base(runtime, state, host);
    host.effect(StartupEffect::AncientEvilByte(1));
    host.cleanup_effect(CleanupEffect::DeleteBigfileAssets(state.asset_handle_100));
    uninitialize_ball(
        state.ball_resources.as_mut().unwrap(),
        &mut runtime.state.scene,
        host,
    );
    host.cleanup_effect(CleanupEffect::FreeBall(runtime.state.reset.ball_handle_104));
    runtime.state.reset.ball_handle_104 = 0;
    // Rust relinquishes the released object; its final fields were already
    // written by uninitialize_ball before FreeBall was dispatched.
    state.ball_resources = None;
    for player in 0..player_count {
        let character = runtime.state.reset.player_handles_120[player as usize];
        if character == 0 {
            continue;
        }
        if character == host.world_player_character(0) {
            host.cleanup_effect(CleanupEffect::ResetWorldPlayerAnimation(character));
        }
        if host.character_local_control_block(character) == 0 {
            host.cleanup_effect(CleanupEffect::SwitchToLocalControl(character));
        }
        host.cleanup_effect(CleanupEffect::ClearCharacterActivityFlag(character));
    }
    let null = host.cleanup_null_game_fx();
    if runtime.state.reset.field_334_guid != null {
        host.cleanup_destroy_fx(runtime.state.reset.field_334_guid, 0);
        runtime.state.reset.field_334_guid = host.cleanup_null_game_fx();
    }
    let null = host.cleanup_null_game_fx();
    if state.pole_glow_340 != null {
        host.cleanup_destroy_fx(state.pole_glow_340, 0);
        state.pole_glow_340 = host.cleanup_null_game_fx();
    }
    host.cleanup_effect(CleanupEffect::PurgeCallbacks);
    if runtime.life.hud_ready {
        host.cleanup_effect(CleanupEffect::TimerVisible(0));
        host.mega_visible(0, 0);
        host.mega_visible(1, 0);
        runtime.life.hud_ready = false;
        host.clear_hud();
        host.close_screen();
    }
    host.cleanup_effect(CleanupEffect::PoleIndicator(0.));
    runtime.state.frontend.field_424 = 0;
    host.cleanup_effect(CleanupEffect::UnduckMusic);
    host.cleanup_effect(CleanupEffect::StopMusic);
    host.cleanup_effect(CleanupEffect::UnloadAudio(8));
    host.effect(StartupEffect::RendererWord1c0(0));
    host.cleanup_effect(CleanupEffect::RestoreArea);
    Ok(())
}
