//! Front-end side of the post-game screen, driven by the verified shared minigame port (`minigame_session`):
//! `Minigame::OpenPostGameScreen` (tournament points / wins / placement), the `PostGameLVHandlers` queries
//! (`PostGameInfo::query_fields`, job ids 0..10 from the executable's handler table) and `PostGameFSHandlers`
//! (`Session::handle_postgame_command`, jobs 11..14).  The screen shown is always `PostGame`; the multiplayer screens
//! `PostGameMP` / `MultiPlayer_PostGame*` of earlier builds are not reachable in this executable's handler table.
use crate::{
    apt_vm::{V, Vm},
    minigame_session::{Effect, MultiplayerContext, PostGameField, PostGameInfo, PostGameProfileServices, Services, Session},
    multiplayer::MultiplayerMode,
};
use std::collections::HashMap;

pub struct Pg {
    pub session: Session,
    pub info: PostGameInfo,
    pub kind: i32,
    pub tournament: MultiplayerMode,
    pub ctx: MultiplayerContext,
    /// Live singleton byte +1 (`multiplayer_flag_1`): the minigame leaves to the multiplayer menus.
    pub flag1: bool,
    /// Player count the running tournament was created for (None = no series yet).
    pub series_players: Option<usize>,
    scores: HashMap<i32, i32>,
}

impl Default for Pg {
    fn default() -> Self {
        Pg {
            session: Session {
                camera_type: 0,
                control_type: 0,
                game_state: 0,
                dare: -1,
                initialized: true,
                pause_menu_open: false,
                pregame_ready: false,
                postgame_choice: -1,
                team_count: 1,
                pause_block_ms: 0,
                world_paused: false,
            },
            info: PostGameInfo([0; 0x118]),
            kind: 2,
            tournament: MultiplayerMode::new(2).expect("two players"),
            ctx: MultiplayerContext { enabled: false, players: 2, team_controls: [[0; 4]; 2] },
            flag1: false,
            series_players: None,
            scores: HashMap::new(),
        }
    }
}

struct Sink(Vec<Effect>);
impl Services for Sink {
    fn effect(&mut self, effect: Effect) {
        self.0.push(effect);
    }
    fn world_update(&mut self, _milliseconds: i32) -> u32 {
        0
    }
}

struct Profile<'a>(&'a mut HashMap<i32, i32>, bool);
impl PostGameProfileServices for Profile<'_> {
    fn high_score(&mut self, kind: i32) -> i32 {
        self.0.get(&kind).copied().unwrap_or(0)
    }
    fn profile_name(&mut self) -> u32 {
        0
    }
    fn set_high_score(&mut self, kind: i32, score: i32, _name: u32) {
        self.0.insert(kind, score);
    }
    fn multiplayer_enabled(&mut self) -> bool {
        self.1
    }
    fn trigger_save_popup(&mut self) {}
}

impl Pg {
    /// Start (or continue) the multiplayer series for a launch: one tournament object per run of games with the same players.
    pub fn begin_game(&mut self, players: usize, multiplayer: bool, rounds: Option<i32>) {
        self.ctx.enabled = multiplayer;
        self.ctx.players = players.clamp(2, 4);
        self.session.team_count = if multiplayer { players as i32 } else { 1 };
        if multiplayer && self.series_players != Some(players) {
            if let Ok(mut t) = MultiplayerMode::new(players.clamp(2, 4)) {
                match rounds {
                    Some(r) if r > 0 => t.start_point_series(r),
                    _ => t.start_free_play(),
                }
                self.tournament = t;
                self.series_players = Some(players);
            }
        }
    }
    pub fn end_series(&mut self) {
        self.series_players = None;
    }
}

fn pairs(fields: Vec<PostGameField>) -> Vec<(String, String)> {
    fields
        .into_iter()
        .map(|f| match f {
            PostGameField::Int(n, v) => (n.to_string(), v.to_string()),
            PostGameField::Array(n, v) => (n.to_string(), v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(&crate::fe_host::DELIM.to_string())),
        })
        .collect()
}

/// `Minigame::OpenPostGameScreen(kind, info)` with the 70 big-endian payload words the minigame produced.
pub fn open(vm: &mut Vm, kind: i32, words: &[u32]) {
    let mut bytes = [0u8; 0x118];
    for (i, w) in words.iter().take(70).enumerate() {
        bytes[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
    }
    let pg = &mut vm.fe.pg;
    pg.kind = kind;
    let mut info = PostGameInfo(bytes);
    let mut sink = Sink(vec![]);
    let session = &mut pg.session;
    if let Err(e) = session.open_postgame(kind, &mut info, &pg.ctx, &mut pg.tournament, &mut sink) {
        vm.log.push(format!("OpenPostGameScreen rejected the payload: {e:?}"));
    }
    pg.info = info;
    for e in sink.0 {
        if let Effect::OpenScreen(name) = e {
            vm.call_exposed("OpenScreen", vec![V::Str(name.into())]);
        }
    }
}

/// `PostGameLVHandlers::DoJobLV` for a handler name; `None` if the name is not a post-game query.
pub fn query(vm: &mut Vm, name: &str, player: i32) -> Option<Vec<(String, String)>> {
    if name == "EndTourney_OnLoad" {
        // `EndTourneyLVHandlers::EndTourneyOnLoad` (0x80316220): GetPlayerNumByRank(0) and that player's avatar id.
        let winner = vm.fe.pg.tournament.player_by_rank(0).unwrap_or(0);
        let avatar = vm.fe.mp.avatars.get(winner).copied().unwrap_or(0);
        return Some(vec![("iWinningPlayerId".into(), winner.to_string()), ("iWinningPlayerAvatarId".into(), avatar.to_string())]);
    }
    let job = match name {
        "PostGame_OnLoad" => 0,
        "PostGame_GetSPInfo" => 1,
        "PostGame_GetStats" => 2,
        "PostGame_GetMPInfo" => 3,
        "PostGame_IsLastTourneyGame" => 4,
        "PostGame_IsNextGameLastTourneyGame" => 5,
        "PostGame_GetPointsWon" => 6,
        "PostGame_GetTotalPoints" => 7,
        "PostGame_GetTeamSetup" => 8,
        "PostGame_GetGamesWon" => 9,
        "PostGame_GetTotalGamesWon" => 10,
        _ => return None,
    };
    let pg = &mut vm.fe.pg;
    let mut profile = Profile(&mut pg.scores, pg.ctx.enabled);
    match pg.info.query_fields(job, pg.kind, player, &pg.tournament, &mut profile) {
        Ok(f) => Some(pairs(f)),
        Err(e) => {
            vm.log.push(format!("{name}: {e:?}"));
            Some(vec![])
        }
    }
}

/// `PostGameFSHandlers::DoJobFS` (11 replay, 12 done, 13 multiplayer select, 14 button click).  Leaves the minigame's
/// post-game choice for `UpdatePostGame` to consume and queues the screens to show once the minigame has exited.
pub fn command(vm: &mut Vm, name: &str, selection: i32) -> bool {
    if name == "EndTourney_OnButtonClick" {
        // 0x80316180 -> Minigame::OnDone (0x803abc14): the choice becomes "done"; the series is over.
        vm.fe.pg.session.on_done();
        vm.fe.pg.end_series();
        vm.fe.mp.active = false;
        vm.fe.mp.quick = false;
        vm.fe.postgame_choice = Some(1);
        vm.fe.after_exit = vec![(0, "ClearScreenStack".into(), vec![]), (10, "OpenScreen".into(), vec![V::Str("MainMenu".into())])];
        return true;
    }
    let job = match name {
        "PostGame_OnReplay" => 11,
        "PostGame_OnDone" => 12,
        "MultiPlayer_PostGameOnSelect" => 13,
        "PostGame_OnButtonClick" => 14,
        _ => return false,
    };
    let pg = &mut vm.fe.pg;
    let point_series = pg.tournament.is_point_series();
    pg.session.postgame_choice = -1;
    pg.session.handle_postgame_command(job, selection, &mut pg.flag1, point_series);
    let choice = pg.session.postgame_choice;
    let leave_to_menus = std::mem::take(&mut pg.flag1);
    vm.fe.postgame_choice = Some(choice);
    if choice == 1 {
        if leave_to_menus {
            // The multiplayer frontend branch of EndMinigame: back to choosing a game for the same players.
            vm.fe.after_exit = vec![(0, "ClearScreenStack".into(), vec![]), (10, "OpenScreen".into(), vec![V::Str("GameSelect".into())])];
            vm.fe.mp.active = true;
        } else {
            vm.fe.pg.end_series();
            vm.fe.mp.active = false;
            vm.fe.mp.quick = false;
            vm.fe.after_exit = vec![(0, "ClearScreenStack".into(), vec![]), (10, "OpenScreen".into(), vec![V::Str("MainMenu".into())])];
        }
    }
    true
}
