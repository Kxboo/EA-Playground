//! Session setters used by WorldMan::StartMinigameFadeComplete (0x803e1b2c).
use crate::tetherball_constructor::{ConstructorInputs, GAME_BYTES};
use crate::tetherball_runtime::Runtime;
use crate::tetherball_startup::{self, StartupInput, StartupServices, StartupState};

#[derive(Clone, Debug)]
pub struct SessionInputs {
    pub level: i32,
    pub difficulty: i32,
    pub dare_type: i32,
    pub rules: u32,
    pub game_id: u32,
    pub team_count: i32,
    /// Teams +8..+87. The reserved +4 word is excluded by the native copy.
    pub participants: [u8; 128],
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
/// Original setter stores, including all bytes copied by SetUpTeams 0x803ab3f0.
pub fn configure_storage(storage: &mut [u8; GAME_BYTES], input: &SessionInputs) {
    for (offset, value) in [
        (0x3c, input.level as u32),
        (0x44, input.difficulty as u32),
        (0x48, input.dare_type as u32),
        (0x70, input.team_count as u32),
    ] {
        storage[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    storage[0x78..0xf8].copy_from_slice(&input.participants);
    storage[0xf8..0xfc].copy_from_slice(&input.rules.to_be_bytes());
    storage[0x38..0x3c].copy_from_slice(&input.game_id.to_be_bytes());
}
/// Project the session fields consumed by the recovered runtime. Remaining team
/// bytes stay in the caller's session input; they are not additional live owners.
pub fn configure_runtime(
    runtime: &mut Runtime,
    state: &mut StartupState,
    input: &SessionInputs,
) -> Result<(), String> {
    let metadata = state
        .constructor_metadata
        .as_mut()
        .ok_or("session requires constructed game")?;
    metadata.rules_0f8 = input.rules;
    metadata.tag_038 = input.game_id;
    runtime.life.variant = input.level;
    runtime.state.reset.game_mode_044 = input.difficulty;
    runtime.life.game_type = input.dare_type;
    runtime.state.reset.base_player_count_070 = input.team_count;
    for (player, offset) in [0, 64].into_iter().enumerate() {
        state.identities_078_0b8[player] = [
            word(&input.participants, offset),
            word(&input.participants, offset + 4),
        ];
        runtime.life.players[player].player_flag = input.participants[offset + 12] != 0;
    }
    runtime.state.reset.ai_special_case_0c0 = word(&input.participants, 72) as i32;
    Ok(())
}
/// Shared entry for normal selection and developer settings. Allocation state,
/// decoded constructor inputs and engine readiness remain explicit dependencies.
pub fn start_session(
    runtime: &mut Runtime,
    state: &mut StartupState,
    constructor: ConstructorInputs,
    session: &SessionInputs,
    startup: StartupInput,
    host: &mut impl StartupServices,
) -> Result<(), String> {
    crate::tetherball_constructor::construct_runtime_fields(runtime, state, constructor);
    configure_runtime(runtime, state, session)?;
    tetherball_startup::initialize(runtime, state, startup, host)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_session_stores() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/tetherball_session_golden.json"))
                .unwrap();
        assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
        for case in fixture["cases"].as_array().unwrap() {
            let a = &case["input"];
            let input = SessionInputs {
                level: a["level"].as_i64().unwrap() as i32,
                difficulty: a["difficulty"].as_i64().unwrap() as i32,
                dare_type: a["dare_type"].as_i64().unwrap() as i32,
                rules: a["rules"].as_u64().unwrap() as u32,
                game_id: a["game_id"].as_u64().unwrap() as u32,
                team_count: a["team_count"].as_i64().unwrap() as i32,
                participants: std::array::from_fn(|i| a["participants"][i].as_u64().unwrap() as u8),
            };
            let mut storage =
                std::array::from_fn(|i| case["initial_bytes"][i].as_u64().unwrap() as u8);
            configure_storage(&mut storage, &input);
            assert_eq!(
                serde_json::json!(storage.as_slice()),
                case["expected_bytes"]
            );
        }
    }
}
