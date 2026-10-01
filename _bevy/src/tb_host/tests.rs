use super::*;
use crate::tetherball_gestures::GestureCallback;

fn load_db() -> Option<Rc<Database>> {
    let dir = crate::bridge::data_root().join("files/data/db");
    let v = std::fs::read(dir.join("db.vlt")).ok()?;
    let b = std::fs::read(dir.join("db.bin")).ok()?;
    Some(Rc::new(Database::load(&v, &b, crate::vlt::known_names()).unwrap()))
}

/// Run the recovered startup and a complete match against the host with a bot standing in for the human.
fn play(humans: usize, area: i32, rotations: i32, seconds: i32, verbose: bool) -> Option<(Runtime, TbHost, Vec<String>)> {
    let db = load_db()?;
    let assets = Rc::new(AnimAssets::load().ok()?);
    let mut cfg = Config::quick(humans);
    cfg.area = area;
    cfg.rotations = rotations;
    let mut host = TbHost::new(db, assets, cfg, 0x1234_5678).unwrap();
    let (mut rt, _st) = host.start().expect("startup");
    let mut log = vec![];
    let mut last_state = u32::MAX;
    let mut hud_at = None;
    let mut pregame_at = None;
    let mut anim_at = None;
    let ms = 16;
    let mut tick = 0;
    while tick * ms < seconds * 1000 {
        tick += 1;
        let now = tick * ms;
        let mut gesture = None;
        for out in host.take_out() {
            if verbose {
                log.push(format!("{now:6} {out:?}"));
            }
            match &out {
                Out::Pregame { .. } => pregame_at = pregame_at.or(Some(now + 300)),
                Out::Hud(Hud::OpenScreen(n)) if n == "TetherballHud" => hud_at = Some(now + 200),
                _ => {}
            }
        }
        if pregame_at.is_some_and(|t| now >= t) {
            pregame_at = None;
            host.pregame_done(&mut rt);
        }
        if hud_at.is_some_and(|t| now >= t) {
            hud_at = None;
            host.hud_loaded(&mut rt);
            anim_at = Some(now + 500);
        }
        if anim_at.is_some_and(|t| now >= t) {
            anim_at = None;
            host.start_anim_complete(&mut rt);
        }
        let state = rt.life.match_state.state_code;
        if state != last_state {
            log.push(format!("{now:6} state {state} (server {}, rot {:?}, ang {:.2})", rt.life.server, rt.life.match_state.rotations, rt.ball.angle));
            last_state = state;
        }
        // Bot: serve toss, then strike; return the ball when it enters the swing window.
        if rt.life.players[0].controller.is_some() {
            match state {
                27 if rt.life.server == 0 => gesture = Some(if rt.ball.tossed { GestureCallback::RegularStrike } else { GestureCallback::ServeToss }),
                28 | 29 if rt.life.receiver == 0 => gesture = Some(GestureCallback::RegularStrike),
                _ => {}
            }
        }
        if verbose && matches!(state,28|29) && tick % 6 == 0 { log.push(format!("{now:6} st{state} ang {:.2} av {:.2} recv {} act {:?} anim {:?} latch {:?} c260 {} start {:?}", rt.ball.angle, rt.ball.angular_velocity, rt.life.receiver, rt.life.action_states, [rt.life.players[0].current_animation, rt.life.players[1].current_animation], rt.life.latches, rt.state.reset.counter_260 as i32, rt.state.reset.start_angles_248)); }
        if let Some(g) = gesture {
            if tick % 4 == 0 {
                host.gesture(&mut rt, g, 0);
            }
        }
        let held = 0;
        let r = host.frame(&mut rt, ms, [held, 0]).expect("frame");
        let _ = r;
        if rt.life.match_state.state_code == 8 || rt.life.match_state.state_code == 9 {
            break;
        }
    }
    Some((rt, host, log))
}

#[test]
fn startup_builds_the_recovered_world() {
    let Some(db) = load_db() else { return };
    let Ok(assets) = AnimAssets::load() else { return };
    let mut host = TbHost::new(db, Rc::new(assets), Config::quick(1), 7).unwrap();
    let (rt, st) = host.start().expect("startup");
    assert_eq!(rt.life.player_count, 2);
    assert_eq!(rt.life.match_state.state_code, 1, "ends in the pregame instructions state");
    assert!(rt.state.ai[0].is_some() && rt.state.ai[1].is_some());
    assert_eq!(host.chars.len(), 2);
    // school pole placeable
    assert!((host.origin[2] + 52.67).abs() < 0.01, "origin {:?}", host.origin);
    assert!(st.pole_glow_340 != 0);
    let outs = host.take_out();
    assert!(outs.iter().any(|o| matches!(o, Out::Pregame { kind: 2, .. })));
    assert!(outs.iter().any(|o| matches!(o, Out::PlayMusic(2))));
}

#[test]
fn a_full_match_runs_through_the_recovered_state_machine() {
    let Some((rt, host, log)) = play(1, 0, 3, 600, std::env::var("TB_VERBOSE").is_ok()) else { return };
    for l in &log {
        eprintln!("{l}");
    }
    eprintln!("final state {} rot {:?} winner {} stats {:?}", rt.life.match_state.state_code, rt.life.match_state.rotations, rt.life.match_state.match_winner, rt.life.statistics);
    let _ = host;
    assert!(log.iter().any(|l| l.contains("state 27")), "serve state reached");
    assert!(log.iter().any(|l| l.contains("state 28") || l.contains("state 29")), "rally reached");
}
