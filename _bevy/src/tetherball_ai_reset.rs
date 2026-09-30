//! Apply ResetRound's AI construction/configuration requests to recovered state.
use crate::tetherball_ai::AiEntity;
use crate::tetherball_lifecycle::Lifecycle;
use crate::tetherball_rally_rules::RallyRuleState;
use crate::tetherball_reset::ResetEffect;

/// Apply the AI portion of ResetRound's ordered effect list. Object allocation
/// and controller binding retain their host effects; decoded AI stores and
/// initialization are consumed. The caller supplies allocation handles through
/// ResetInputs, exactly as in the reset projection.
pub fn apply_reset_ai(
    effects: Vec<ResetEffect>,
    entities: &mut [Option<AiEntity>; 2],
    life: &Lifecycle,
    rally: &mut RallyRuleState,
    db: &crate::vlt::Database,
) -> Result<Vec<ResetEffect>, String> {
    let mut remaining = Vec::with_capacity(effects.len());
    for effect in effects {
        match effect {
            ResetEffect::CreateAiEntity { player, entity, .. } => {
                // Scale is not read until the following AiStartValue store.
                entities[player] = Some(AiEntity::new(entity, player, 0.));
                rally.ai_charge[player] = 0; // Native constructor +70.
                remaining.push(effect);
            }
            ResetEffect::BindAiEntity { player, ball, .. } => {
                entities[player]
                    .as_mut()
                    .expect("AI created before binding")
                    .ball_handle = ball;
                remaining.push(effect);
            }
            ResetEffect::InitializeAi {
                entity,
                enabled,
                difficulty,
                angle_bits,
            } => {
                let ai = find(entities, entity);
                ai.initialize(
                    Some((life.session_mode, life.game_type)),
                    enabled,
                    difficulty as u32,
                    f32::from_bits(angle_bits),
                    db,
                )?;
            }
            ResetEffect::AiAngle { entity, angle_bits } => {
                find(entities, entity).angle = f32::from_bits(angle_bits)
            }
            ResetEffect::AiStartValue { entity, value_bits } => {
                find(entities, entity).direction_scale = f32::from_bits(value_bits)
            }
            ResetEffect::AiDistance { entity, distance } => {
                let ai = find(entities, entity);
                assert_eq!(
                    life.players[ai.player].ai_distance, distance,
                    "ResetRound shared distance store"
                );
            }
            other => remaining.push(other),
        }
    }
    Ok(remaining)
}
fn find(entities: &mut [Option<AiEntity>; 2], handle: u32) -> &mut AiEntity {
    entities
        .iter_mut()
        .filter_map(Option::as_mut)
        .find(|ai| ai.handle == handle)
        .expect("live reset AI handle")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn original_reset_with_ai_initialization() {
        let d: Value = serde_json::from_str(include_str!(
            "../tests/data/tetherball_ai_reset_golden.json"
        ))
        .unwrap();
        assert_eq!(d["elf_sha256"], crate::recovered::ELF_SHA256);
        let path = crate::bridge::data_root().join("files/data/db");
        let db = crate::vlt::Database::load(
            &std::fs::read(path.join("db.vlt")).unwrap(),
            &std::fs::read(path.join("db.bin")).unwrap(),
            crate::vlt::known_names(),
        )
        .unwrap();
        for c in d["cases"].as_array().unwrap() {
            let (mut life, mut reset, mut ball, input) =
                crate::tetherball_reset::tests::seed(c, &d);
            let effects =
                crate::tetherball_reset::reset_round(&mut life, &mut reset, &mut ball, &input);
            let mut entities = [None, None];
            let mut rally = RallyRuleState {
                ai_hit_attempt_234: 17,
                ai_power_hit_type_238: 23,
                ai_charge: [41, 42],
            };
            let remaining = apply_reset_ai(effects, &mut entities, &life, &mut rally, &db).unwrap();
            assert_eq!(serde_json::to_value(&life).unwrap(), c["expected"]);
            for p in 0..2 {
                let ai = entities[p].as_ref().unwrap();
                let want = &c["ai"][p];
                assert_eq!(ai.ball_handle as u64, want["ball"].as_u64().unwrap());
                assert_eq!(ai.angle.to_bits() as u64, want["angle"].as_u64().unwrap());
                assert_eq!(
                    ai.direction_scale.to_bits() as u64,
                    want["scale"].as_u64().unwrap()
                );
                assert_eq!(
                    ai.heading.to_bits() as u64,
                    want["heading"].as_u64().unwrap()
                );
                assert_eq!(ai.enabled, want["enabled"].as_bool().unwrap());
                assert_eq!(serde_json::json!(ai.difficulty), want["difficulty"]);
                assert_eq!(rally.ai_charge[p] as u64, want["charge"].as_u64().unwrap());
            }
            assert!(remaining.iter().all(|e| !matches!(
                e,
                ResetEffect::InitializeAi { .. }
                    | ResetEffect::AiAngle { .. }
                    | ResetEffect::AiStartValue { .. }
                    | ResetEffect::AiDistance { .. }
            )));
            assert_eq!(
                (rally.ai_hit_attempt_234, rally.ai_power_hit_type_238),
                (17, 23)
            );
        }
    }
}
