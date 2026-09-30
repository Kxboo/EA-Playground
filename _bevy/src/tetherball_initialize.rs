//! Native `MGTetherball::InitGameLogicState` field projection.
//!
//! The top-level `Initialize` routine has engine-owned world, asset, camera,
//! player-spawn and audio boundaries. This module isolates its first logical
//! stage: the retail helper at 0x8039ceac. The selected tunables and ResetStats
//! values are inputs because those callees perform VLT/engine work elsewhere.
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_match::MatchRules;
use crate::tetherball_reset::ResetState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTunables {
    /// Results of InitTunablesForSinglePlayer: +16c,+170,+174,+178,+17c,+180.
    SinglePlayer { fields: [i32; 6] },
    /// Results of InitTunablesForMultiPlayer. The source helper copies only
    /// parameter words +0,+4,+10,+8 to object +3c,+44,+170,+180.
    MultiPlayer {
        variant: i32,
        game_mode: i32,
        rounds: i32,
        rotations_to_win: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitGameLogicInputs {
    pub tunables: NativeTunables,
    /// Values already read by the nested ResetStats database routine.
    pub score_weights: [i32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitializeEffect {
    SinglePlayerTunables,
    MultiPlayerTunables,
    ResetStats,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InitGameLogicResult {
    pub effects: Vec<InitializeEffect>,
}

/// Execute the recovered stores in `InitGameLogicState` (0x8039ceac).
///
/// This mutates the existing Lifecycle, ResetState and MatchRules owners; it
/// does not create parallel state. Tunable reads and `ResetStats` are explicit
/// dependencies whose returned values are supplied in `inputs`.
pub fn init_game_logic_state(
    lifecycle: &mut Lifecycle,
    reset: &mut ResetState,
    rules: &mut MatchRules,
    inputs: InitGameLogicInputs,
) -> InitGameLogicResult {
    // The native division uses the old +430 value before storing +180 there.
    let prior_rotation_limit = rules.rotation_limit;
    let (rounds, handicap, rotations_to_win, effects) = match inputs.tunables {
        NativeTunables::SinglePlayer { fields } => {
            rules.mode = fields[0];
            lifecycle.total_rounds = fields[1];
            lifecycle.distance_mode = fields[2];
            reset.initial_rotation_178 = fields[3];
            rules.time_limit_seconds = fields[4];
            (
                fields[1],
                fields[3],
                fields[5],
                vec![InitializeEffect::SinglePlayerTunables],
            )
        }
        NativeTunables::MultiPlayer {
            variant,
            game_mode,
            rounds,
            rotations_to_win,
        } => {
            lifecycle.variant = variant;
            reset.game_mode_044 = game_mode;
            rules.mode = 0;
            lifecycle.total_rounds = rounds;
            lifecycle.distance_mode = 0;
            reset.initial_rotation_178 = 0;
            rules.time_limit_seconds = 0;
            (
                rounds,
                0,
                rotations_to_win,
                vec![InitializeEffect::MultiPlayerTunables],
            )
        }
    };

    // PPC srawi/addze is truncation toward zero for signed odd round counts.
    let wins_required = (rounds / 2) as u8 as i32 + 1;
    let handicap_byte = handicap as u8;
    let negative_handicap_byte = (0u8).wrapping_sub(handicap_byte) as i8;
    let positive_handicap_byte = handicap_byte as i8;

    lifecycle.match_state.rotations = [negative_handicap_byte, positive_handicap_byte];
    lifecycle.match_state.round_winner = -1;
    lifecycle.match_state.match_winner = -1;
    lifecycle.statistics = [[0; 5]; 2];
    lifecycle.score_weights = inputs.score_weights;
    reset.rotation_limit_430 = rotations_to_win;
    reset.field_444 = false;
    reset.alternate_server_445 = false;
    rules.rotation_limit = rotations_to_win;
    rules.wins_required = wins_required;

    let ratio_numerator = negative_handicap_byte as f32;
    let ratio_denominator = prior_rotation_limit as f32;
    // Match the target's fdivs invalid-operation result for 0/0 (the emulator
    // records the PPC quiet-NaN sign bit as zero).
    let indicator_ratio = if ratio_numerator == 0.0 && ratio_denominator == 0.0 {
        f32::from_bits(0x7fc0_0000)
    } else {
        ratio_numerator / ratio_denominator
    };
    lifecycle.indicator_target = indicator_ratio;
    let mut effects = effects;
    effects.push(InitializeEffect::ResetStats);
    InitGameLogicResult { effects }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn i32_at(v: &Value, key: &str) -> i32 {
        v[key].as_i64().unwrap() as i32
    }
    fn ppc_effects(effects: &[InitializeEffect]) -> Vec<Value> {
        effects
            .iter()
            .map(|e| match e {
                InitializeEffect::SinglePlayerTunables => json!(["single_tunables"]),
                InitializeEffect::MultiPlayerTunables => json!(["multi_tunables"]),
                InitializeEffect::ResetStats => json!(["reset_stats"]),
            })
            .collect()
    }

    #[test]
    fn original_init_game_logic_state() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_initialize_golden.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["elf_sha256"].as_str(),
            Some(crate::recovered::ELF_SHA256)
        );
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 64);
        let lifecycle_fixture: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_lifecycle_golden.json"
        ))
        .unwrap();
        let seed = &lifecycle_fixture["cases"][0]["initial"];

        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut lifecycle: Lifecycle = serde_json::from_value(seed.clone()).unwrap();
            let initial = &case["initial"];
            // The PPC case seeds +3c from its input; a single-player helper
            // leaves that field untouched, so seed the shared owner likewise.
            lifecycle.variant = i32_at(initial, "variant");
            let mut reset = ResetState::blank();
            reset.rotation_limit_430 = i32_at(initial, "rotation_limit");
            reset.initial_rotation_178 = i32_at(initial, "handicap");
            reset.game_mode_044 = i32_at(initial, "game_mode");
            reset.field_444 = initial["field_444"]
                .as_bool()
                .unwrap_or(initial["field_444"].as_i64().unwrap() != 0);
            reset.alternate_server_445 = initial["field_445"]
                .as_bool()
                .unwrap_or(initial["field_445"].as_i64().unwrap() != 0);
            let mut rules = MatchRules {
                mode: i32_at(initial, "mode"),
                rotation_limit: i32_at(initial, "rotation_limit"),
                wins_required: i32_at(initial, "wins_required"),
                time_limit_seconds: i32_at(initial, "seconds"),
            };
            let inputs = &case["inputs"];
            let tunables = if inputs["multi_flag"].as_i64().unwrap() != 0 {
                NativeTunables::MultiPlayer {
                    variant: inputs["multi"][0].as_i64().unwrap() as i32,
                    game_mode: inputs["multi"][1].as_i64().unwrap() as i32,
                    rounds: inputs["multi"][2].as_i64().unwrap() as i32,
                    rotations_to_win: inputs["multi"][3].as_i64().unwrap() as i32,
                }
            } else {
                NativeTunables::SinglePlayer {
                    fields: std::array::from_fn(|i| inputs["single"][i].as_i64().unwrap() as i32),
                }
            };
            let score_weights =
                std::array::from_fn(|i| inputs["scores"][i].as_i64().unwrap() as i32);
            let result = init_game_logic_state(
                &mut lifecycle,
                &mut reset,
                &mut rules,
                InitGameLogicInputs {
                    tunables,
                    score_weights,
                },
            );
            let expected = &case["expected"];
            let mut expected_lifecycle: Lifecycle = serde_json::from_value(seed.clone()).unwrap();
            expected_lifecycle.variant = i32_at(initial, "variant");
            expected_lifecycle.variant = i32_at(expected, "variant");
            expected_lifecycle.total_rounds = i32_at(expected, "rotation_count");
            expected_lifecycle.distance_mode = i32_at(expected, "distance");
            expected_lifecycle.indicator_target =
                f32::from_bits(expected["indicator_target_bits"].as_u64().unwrap() as u32);
            expected_lifecycle.match_state.rotations = [
                expected["rotations"][0].as_i64().unwrap() as i8,
                expected["rotations"][1].as_i64().unwrap() as i8,
            ];
            expected_lifecycle.match_state.round_winner = i32_at(expected, "round_winner");
            expected_lifecycle.match_state.match_winner = i32_at(expected, "match_winner");
            expected_lifecycle.statistics = [[0; 5]; 2];
            expected_lifecycle.score_weights =
                std::array::from_fn(|i| expected["score_weights"][i].as_i64().unwrap() as i32);
            assert_eq!(
                serde_json::to_value(&lifecycle).unwrap(),
                serde_json::to_value(expected_lifecycle).unwrap(),
                "case {index}: lifecycle"
            );

            assert_eq!(
                reset.rotation_limit_430,
                i32_at(expected, "rotation_limit"),
                "case {index}: +430"
            );
            assert_eq!(
                reset.initial_rotation_178,
                i32_at(expected, "handicap"),
                "case {index}: +178"
            );
            assert_eq!(
                reset.game_mode_044,
                i32_at(expected, "game_mode"),
                "case {index}: +44"
            );
            assert!(
                !reset.field_444 && !reset.alternate_server_445,
                "case {index}: flags"
            );
            assert_eq!(rules.mode, i32_at(expected, "mode"));
            assert_eq!(rules.rotation_limit, i32_at(expected, "rotation_limit"));
            assert_eq!(rules.wins_required, i32_at(expected, "wins_required"));
            assert_eq!(rules.time_limit_seconds, i32_at(expected, "seconds"));
            assert_eq!(
                json!(ppc_effects(&result.effects)),
                case["effects"],
                "case {index}: dependency order"
            );
        }
    }
}
