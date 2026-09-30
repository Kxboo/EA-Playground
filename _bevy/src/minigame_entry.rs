//! Shared native minigame entry helpers, from the pinned Wii executable.
pub trait PregameServices {
    fn setup_pregame_handlers(&mut self, kind: i32, mode: i32, words: [u32; 4]);
    fn open_pregame_screen(&mut self, name: &str);
    fn pregame_fade_renders(&mut self, first: bool, second: bool);
}
/// PlaygroundWorld::ConvertAreaAbstractToPhysical (0x803de608).
/// Unrecognized values are returned unchanged, including negative raw enums.
pub fn physical_area(area: i32) -> i32 {
    match area {
        4 => 0,
        5 => 1,
        6 => 2,
        7 | 8 => 3,
        _ => area,
    }
}
/// Minigame::OpenPreGameScreen (0x803ab580), including PreGameInfo's
/// constructor (0x803ab634). This does not clear the minigame ready byte +58.
pub fn open_pregame(
    frontend_flags: &mut [bool; 2],
    player_count: i32,
    dare_type: i32,
    kind: i32,
    host: &mut impl PregameServices,
) {
    *frontend_flags = [true; 2];
    host.setup_pregame_handlers(
        kind,
        0,
        [0, 0, u32::from(player_count > 1), dare_type as u32],
    );
    host.open_pregame_screen("PreGameInstructions");
    host.pregame_fade_renders(true, false);
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    struct Host {
        events: Vec<Value>,
    }
    impl PregameServices for Host {
        fn setup_pregame_handlers(&mut self, kind: i32, mode: i32, words: [u32; 4]) {
            self.events
                .push(json!(["pregame_handlers", kind, mode, words]));
        }
        fn open_pregame_screen(&mut self, name: &str) {
            self.events.push(json!(["pregame_screen", name]));
        }
        fn pregame_fade_renders(&mut self, first: bool, second: bool) {
            self.events.push(json!(["pregame_fade", first, second]));
        }
    }
    #[test]
    fn native_entry_helpers() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/data/minigame_entry_golden.json")).unwrap();
        assert_eq!(fixture["elf_sha256"], crate::recovered::ELF_SHA256);
        for case in fixture["areas"].as_array().unwrap() {
            assert_eq!(
                physical_area(case["input"].as_i64().unwrap() as i32),
                case["result"].as_i64().unwrap() as i32
            );
        }
        for case in fixture["pregames"].as_array().unwrap() {
            let mut host = Host { events: vec![] };
            let mut flags = [false, true];
            let a = &case["input"];
            open_pregame(
                &mut flags,
                a["players"].as_i64().unwrap() as i32,
                a["dare"].as_i64().unwrap() as i32,
                a["kind"].as_i64().unwrap() as i32,
                &mut host,
            );
            assert_eq!(json!(host.events), case["events"]);
            assert_eq!(json!(flags), case["frontend_flags"]);
            // Native entry preserves every byte of the existing game allocation.
            assert_eq!(case["initial_game_bytes"], case["expected_game_bytes"]);
        }
    }
}
