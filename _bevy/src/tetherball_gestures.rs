//! Tetherball's queued Conga callbacks and controller hit-attempt sampling.
//!
//! This module stops before animation timing, hit-range checks, charge spending,
//! and `HitTetherball`. Those later stages remain controlled by the original
//! state handlers; a sampled button or gesture never becomes an instant hit.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureCallback {
    RegularStrike,
    RegularStrikeReverse,
    OverhandStrike,
    ServeToss,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureRecord {
    /// Raw MGTetherball callback type at record +0x284.
    pub kind: i32,
    /// Raw float at record +0x288. The four callbacks ported here write zero.
    pub auxiliary: f32,
    pub controller_id: i32,
}

/// Pending gesture records and the +0x229 marker used by hit-attempt sampling.
/// Player/controller/action state stays in the host's lifecycle and controller.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GestureState {
    pub pending: Vec<GestureRecord>,
    pub hit_attempt_marker: bool,
}

/// Match `MGTetherball::ResetRound` for the currently live queue prefix. The
/// original resets slot contents but leaves the pending count and +0x229 marker
/// alone, so this clears record meaning without dropping their positions.
pub fn reset_pending_gestures(state: &mut GestureState) {
    for record in &mut state.pending {
        record.kind = 2;
        record.auxiliary = 0.0;
        record.controller_id = 0;
    }
}

/// Run one original Conga callback wrapper and its queue process.
///
/// `paused` is read from the global minigame instance; `callbacks_enabled` is
/// the byte at +0x424. For regular and reverse strikes, pass the controller ID
/// attached to their selected player. Other callback kinds ignore that match.
pub fn capture_gesture(
    state: &mut GestureState,
    paused: bool,
    callbacks_enabled: bool,
    selected_controller: Option<i32>,
    callback: GestureCallback,
    controller_id: i32,
) -> bool {
    if paused || !callbacks_enabled || state.pending.len() >= 7 {
        return false;
    }
    let (kind, needs_match) = match callback {
        GestureCallback::RegularStrike | GestureCallback::RegularStrikeReverse => (0, true),
        GestureCallback::OverhandStrike => (0, false),
        GestureCallback::ServeToss => (1, false),
    };
    if needs_match && selected_controller != Some(controller_id) {
        return false;
    }
    state.pending.push(GestureRecord {
        kind,
        auxiliary: 0.0,
        controller_id,
    });
    true
}

/// Lifecycle adapter for callback capture. The selected player indices map to
/// the original +0x21c and +0x220 selectors; callback-enable remains explicit
/// because it is not stored by the lifecycle port.
pub fn capture_lifecycle_gesture(
    state: &mut GestureState,
    lifecycle: &crate::tetherball_lifecycle::Lifecycle,
    callbacks_enabled: bool,
    forward_player: usize,
    reverse_player: usize,
    callback: GestureCallback,
    controller_id: i32,
) -> bool {
    let selected = match callback {
        GestureCallback::RegularStrike => lifecycle
            .players
            .get(forward_player)
            .and_then(|p| p.controller),
        GestureCallback::RegularStrikeReverse => lifecycle
            .players
            .get(reverse_player)
            .and_then(|p| p.controller),
        GestureCallback::OverhandStrike | GestureCallback::ServeToss => None,
    };
    capture_gesture(
        state,
        lifecycle.paused,
        callbacks_enabled,
        selected,
        callback,
        controller_id,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureEffect {
    pub player: usize,
    pub action: i32,
}

/// Execute `ProcessGestures` (`0x8039b674`) in original queue/player order.
///
/// It writes matching action states and consumes the queue even when records
/// have unknown kinds. This function has no pause test: outer
/// `MGTetherball::Update` calls it only when unpaused.
pub fn process_gestures(
    state: &mut GestureState,
    player_count: i32,
    controllers: &[Option<i32>],
    action_states: &mut [i32],
) -> Vec<GestureEffect> {
    let mut effects = Vec::new();
    let count = (player_count.max(0) as usize)
        .min(controllers.len())
        .min(action_states.len());
    for record in &state.pending {
        if record.kind != 0 && record.kind != 1 {
            continue;
        }
        for player in 0..count {
            if controllers[player] == Some(record.controller_id) {
                action_states[player] = record.kind;
                effects.push(GestureEffect {
                    player,
                    action: record.kind,
                });
            }
        }
    }
    state.pending.clear();
    effects
}

/// Apply the queue directly to the authoritative host lifecycle fields.
pub fn process_lifecycle_gestures(
    state: &mut GestureState,
    lifecycle: &mut crate::tetherball_lifecycle::Lifecycle,
) -> Vec<GestureEffect> {
    let controllers: Vec<Option<i32>> = lifecycle.players.iter().map(|p| p.controller).collect();
    process_gestures(
        state,
        lifecycle.player_count as i32,
        &controllers,
        &mut lifecycle.action_states,
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControllerHitEvents {
    /// Controller action event 0x5c.
    pub strike: bool,
    /// Controller action event 0x5d.
    pub reverse_strike: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HitAttempt {
    /// Caller-seeded output bytes. The original routine only sets these.
    pub strike: bool,
    pub reverse_strike: bool,
    /// Raw MGTetherballPowerHitType word; unchanged when neither event fires.
    pub power_type: i32,
}

/// Apply `GetPlayerHitAttempt` (`0x8039b76c`) after the host has selected the
/// controller attached to this player. This samples the action events and
/// updates attempt outputs only; range, animation, charge and hit dispatch stay
/// in the later original state-handler stages.
pub fn get_player_hit_attempt(
    state: &mut GestureState,
    player_action: i32,
    ball_zone: i32,
    events: ControllerHitEvents,
    mega_ability: bool,
    attempt: &mut HitAttempt,
) {
    if player_action == 0 {
        if ball_zone == 0 {
            attempt.strike = true;
        } else {
            attempt.reverse_strike = true;
        }
        state.hit_attempt_marker = true;
    }
    if events.strike {
        attempt.power_type = 1;
    }
    if events.reverse_strike {
        attempt.power_type = 2;
    }
    if events.strike && events.reverse_strike && mega_ability {
        attempt.power_type = 3;
    }
}

/// Original synchronous event reads: 0x5c, 0x5d, 0x5c again, then 0x5d
/// only if the third read succeeds. Preserve the action/marker stores before
/// querying the controller, and allow each engine read to return its own value.
pub fn sample_player_hit_attempt(
    state: &mut GestureState,
    player_action: i32,
    ball_zone: i32,
    mega_ability: bool,
    attempt: &mut HitAttempt,
    mut event: impl FnMut(i32) -> bool,
) {
    get_player_hit_attempt(state, player_action, ball_zone, ControllerHitEvents::default(), false, attempt);
    if event(0x5c) { attempt.power_type = 1; }
    if event(0x5d) { attempt.power_type = 2; }
    if event(0x5c) && event(0x5d) && mega_ability { attempt.power_type = 3; }
}

/// Lifecycle adapter for hit-attempt action state. `active_player` is the
/// current MGTetherball +0x214 selection: the original routine ignores its
/// nominal player argument and reads this field. The caller supplies events
/// already sampled from the controller attached to that selected player.
pub fn get_lifecycle_hit_attempt(
    state: &mut GestureState,
    lifecycle: &crate::tetherball_lifecycle::Lifecycle,
    active_player: usize,
    ball_zone: i32,
    events: ControllerHitEvents,
    mega_ability: bool,
    attempt: &mut HitAttempt,
) {
    if let Some(&action) = lifecycle.action_states.get(active_player) {
        get_player_hit_attempt(state, action, ball_zone, events, mega_ability, attempt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[derive(Clone)]
    struct HostState {
        paused: bool,
        callbacks_enabled: bool,
        mega_ability: bool,
        player_count: i32,
        forward_player: usize,
        reverse_player: usize,
        controllers: Vec<Option<i32>>,
        action_states: Vec<i32>,
    }

    fn i(v: &Value) -> i32 {
        v.as_i64().unwrap() as i32
    }

    fn state_from(initial: &Value) -> (GestureState, HostState) {
        let mut state = GestureState::default();
        state.hit_attempt_marker = initial["hit_attempt_marker"].as_bool().unwrap();
        state.pending = initial["pending"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| GestureRecord {
                kind: i(&r["kind"]),
                auxiliary: f32::from_bits(r["aux_bits"].as_u64().unwrap() as u32),
                controller_id: i(&r["controller_id"]),
            })
            .collect();
        let players = initial["players"].as_array().unwrap();
        let host = HostState {
            paused: initial["paused"].as_bool().unwrap(),
            callbacks_enabled: initial["callbacks_enabled"].as_bool().unwrap(),
            mega_ability: initial["mega_ability"].as_bool().unwrap(),
            player_count: i(&initial["player_count"]),
            forward_player: initial["forward_player"].as_u64().unwrap() as usize,
            reverse_player: initial["reverse_player"].as_u64().unwrap() as usize,
            controllers: players
                .iter()
                .map(|p| p["controller_id"].as_i64().map(|x| x as i32))
                .collect(),
            action_states: players.iter().map(|p| i(&p["action"])).collect(),
        };
        (state, host)
    }

    fn snapshot(state: &GestureState, host: &HostState, attempt: Option<HitAttempt>) -> Value {
        let pending: Vec<Value> = state.pending.iter().map(|r| json!({
            "kind": r.kind, "aux_bits": r.auxiliary.to_bits(), "controller_id": r.controller_id,
        })).collect();
        let players: Vec<Value> = host
            .controllers
            .iter()
            .zip(&host.action_states)
            .map(|(id, action)| {
                json!({
                    "controller_id": id, "action": action,
                })
            })
            .collect();
        let mut out = json!({"pending_count": pending.len(), "pending": pending, "players": players,
                             "hit_attempt_marker": state.hit_attempt_marker});
        if let Some(a) = attempt {
            out["attempt"] = json!({"normal": a.strike, "reverse": a.reverse_strike,
                                    "power_type": a.power_type});
        }
        out
    }

    #[test]
    fn original_powerpc_gesture_callback_and_attempt_vectors() {
        let root: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_gestures_golden.json"
        ))
        .unwrap();
        assert_eq!(
            root["elf_sha256"].as_str(),
            Some(crate::recovered::ELF_SHA256)
        );
        for case in root["cases"].as_array().unwrap() {
            let (mut state, mut host) = state_from(&case["initial"]);
            for step in case["steps"].as_array().unwrap() {
                let op = &step["operation"];
                let mut attempt_out = None;
                let mut effects_out = None;
                match op["op"].as_str().unwrap() {
                    "callback" => {
                        let callback = match op["callback"].as_str().unwrap() {
                            "regular" => GestureCallback::RegularStrike,
                            "reverse" => GestureCallback::RegularStrikeReverse,
                            "overhand" => GestureCallback::OverhandStrike,
                            "serve_toss" => GestureCallback::ServeToss,
                            x => panic!("unknown callback {x}"),
                        };
                        let selected = match callback {
                            GestureCallback::RegularStrike => {
                                host.controllers.get(host.forward_player).copied().flatten()
                            }
                            GestureCallback::RegularStrikeReverse => {
                                host.controllers.get(host.reverse_player).copied().flatten()
                            }
                            _ => None,
                        };
                        capture_gesture(
                            &mut state,
                            host.paused,
                            host.callbacks_enabled,
                            selected,
                            callback,
                            i(&op["controller_id"]),
                        );
                    }
                    "process" => {
                        effects_out = Some(process_gestures(
                            &mut state,
                            host.player_count,
                            &host.controllers,
                            &mut host.action_states,
                        ));
                    }
                    "hit_attempt" => {
                        let player = op["active_player"].as_u64().unwrap() as usize;
                        host.action_states[player] = i(&op["player_action"]);
                        let mut output = HitAttempt {
                            strike: op["normal_out"].as_bool().unwrap(),
                            reverse_strike: op["reverse_out"].as_bool().unwrap(),
                            power_type: i(&op["power_type"]),
                        };
                        let events = ControllerHitEvents {
                            strike: op["normal"].as_bool().unwrap(),
                            reverse_strike: op["reverse"].as_bool().unwrap(),
                        };
                        get_player_hit_attempt(
                            &mut state,
                            host.action_states[player],
                            i(&op["ball_zone"]),
                            events,
                            op["mega_ability"].as_bool().unwrap(),
                            &mut output,
                        );
                        attempt_out = Some(output);
                    }
                    x => panic!("unknown operation {x}"),
                }
                let mut actual = snapshot(&state, &host, attempt_out);
                if let Some(effects) = effects_out {
                    actual["effects"] = json!(
                        effects
                            .iter()
                            .map(|e| (e.player, e.action))
                            .collect::<Vec<_>>()
                    );
                }
                assert_eq!(actual, step["expected"], "{step}");
            }
        }
    }
}
