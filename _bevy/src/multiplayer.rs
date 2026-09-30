//! Renderer-independent port of the original MultiplayerMode tournament arithmetic.
//! See docs/MULTIPLAYER.md for addresses, verified quirks, and safe API boundaries.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplayerError { InvalidPlayerCount, InvalidPlayer }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplayerMode {
    players: usize,
    points: [i32; 4],
    point_ranks: [i32; 4],
    wins: [i32; 4],
    win_ranks: [i32; 4],
    last_points: [i32; 4],
    last_winners: [i32; 4],
    placement: [i32; 4],
    point_series: bool,
    rounds_left: i32,
    results_count: i32,
}

impl MultiplayerMode {
    /// New deterministic state. The original constructor leaves score storage untouched;
    /// callers must start free play or a point series before submitting results.
    pub fn new(players: usize) -> Result<Self, MultiplayerError> {
        if !(2..=4).contains(&players) { return Err(MultiplayerError::InvalidPlayerCount); }
        Ok(Self { players, points: [0;4], point_ranks: [0;4], wins: [0;4],
            win_ranks: [0;4], last_points: [0;4], last_winners: [0;4], placement: [0;4],
            point_series: false, rounds_left: 0, results_count: 0 })
    }
    pub fn start_free_play(&mut self) {
        self.wins = [0;4]; self.win_ranks = [0,1,2,3];
        self.placement = [0;4]; self.last_winners = [-1;4];
        self.point_series = false; self.rounds_left = 0; self.results_count = 0;
    }
    /// Original accepts every signed round count and permits the counter to go negative.
    pub fn start_point_series(&mut self, rounds: i32) {
        self.start_free_play(); self.points = [0;4]; self.point_ranks = [0,1,2,3];
        self.point_series = true; self.rounds_left = rounds;
    }
    /// Slots 0 and 1 always add, including -1; -1 skips slots 2 and 3 and preserves
    /// their last_points. All four slots are stored even with fewer active players.
    pub fn add_round_results(&mut self, scores: [i32;4]) {
        let previous = self.point_ranks;
        for (i, score) in scores.into_iter().enumerate() {
            if i < 2 || score != -1 {
                self.points[i] = self.points[i].wrapping_add(score);
                self.last_points[i] = score;
            }
        }
        rerank(self.players, &self.points, &previous, &mut self.point_ranks);
        self.results_count = self.results_count.wrapping_add(1);
        if self.point_series { self.rounds_left = self.rounds_left.wrapping_sub(1); }
    }
    /// None represents original -1. Duplicate winners each receive a win.
    /// Validate both indices before mutation, unlike the original unchecked pointers.
    pub fn add_win_results(&mut self, winners: [Option<usize>;2]) -> Result<(), MultiplayerError> {
        for p in winners.into_iter().flatten() { self.validate_player(p)?; }
        let previous = self.win_ranks;
        self.last_winners = [-1;4];
        for (slot, p) in winners.into_iter().enumerate() {
            if let Some(p) = p {
                self.wins[p] = self.wins[p].wrapping_add(1);
                self.last_winners[slot] = p as i32;
            }
        }
        rerank(self.players, &self.wins, &previous, &mut self.win_ranks);
        self.results_count = self.results_count.wrapping_add(1);
        Ok(())
    }
    /// Preserve the original bug: fourth argument overwrites the third slot;
    /// fourth storage slot is untouched. Values are placement records, not indices.
    pub fn set_last_placement(&mut self, values: [i32;4]) {
        self.placement[0] = values[0]; self.placement[1] = values[1];
        if values[2] != -1 { self.placement[2] = values[2]; }
        if values[3] != -1 { self.placement[2] = values[3]; }
    }
    fn validate_player(&self, player: usize) -> Result<(), MultiplayerError> {
        if player < self.players { Ok(()) } else { Err(MultiplayerError::InvalidPlayer) }
    }
    pub fn point_total(&self, player: usize) -> Result<i32, MultiplayerError> {
        self.validate_player(player)?; Ok(self.points[player])
    }
    pub fn win_total(&self, player: usize) -> Result<i32, MultiplayerError> {
        self.validate_player(player)?; Ok(self.wins[player])
    }
    pub fn player_rank(&self, player: usize) -> Result<i32, MultiplayerError> {
        self.validate_player(player)?;
        Ok(if self.point_series { self.point_ranks[player] } else { self.win_ranks[player] })
    }
    pub fn player_by_rank(&self, rank: i32) -> Option<usize> {
        (0..self.players).find(|&p| self.player_rank(p) == Ok(rank))
    }
    pub fn won_last_game(&self, player: usize) -> Result<bool, MultiplayerError> {
        self.validate_player(player)?; Ok(self.last_winners.contains(&(player as i32)))
    }
    pub fn points_in_this_match(&self, player: usize) -> Result<i32, MultiplayerError> {
        self.validate_player(player)?; Ok(self.last_points[player])
    }
    pub fn rounds_left(&self) -> i32 { self.rounds_left }
    pub fn results_count(&self) -> i32 { self.results_count }
    pub fn is_point_series(&self) -> bool { self.point_series }
    pub fn last_placement(&self) -> [i32;4] { self.placement }
}

/// The second pass mutates ranks in player order; do not replace with a sort.
fn rerank(n: usize, totals: &[i32;4], previous: &[i32;4], ranks: &mut [i32;4]) {
    for i in 0..n { ranks[i] = (0..n).filter(|&j| i != j && totals[i] < totals[j]).count() as i32; }
    for i in 0..n {
        for j in 0..n {
            if i != j && ranks[i] == ranks[j] {
                if previous[i] < previous[j] { ranks[j] = ranks[j].wrapping_add(1); }
                else { ranks[i] = ranks[i].wrapping_add(1); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn array(v: &Value, key: &str) -> [i32;4] {
        std::array::from_fn(|i| v[key][i].as_i64().unwrap() as i32)
    }
    #[test]
    fn original_powerpc_golden_transitions() {
        let fixtures: Value = serde_json::from_str(include_str!("../tests/data/multiplayer_golden.json")).unwrap();
        assert_eq!(fixtures["elf_sha256"].as_str().unwrap(), crate::recovered::ELF_SHA256);
        for session in fixtures["sessions"].as_array().unwrap() {
            let n = session["players"].as_u64().unwrap() as usize;
            let mut state = MultiplayerMode::new(n).unwrap();
            for (step, row) in session["steps"].as_array().unwrap().iter().enumerate() {
                let action = &row["action"];
                let args: Vec<i32> = action["args"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as i32).collect();
                match action["kind"].as_str().unwrap() {
                    "free" => state.start_free_play(),
                    "series" => state.start_point_series(args[0]),
                    "round" => state.add_round_results(args.try_into().unwrap()),
                    "placement" => state.set_last_placement(args.try_into().unwrap()),
                    "win" => state.add_win_results(std::array::from_fn(|i| if args[i] == -1 {None} else {Some(args[i] as usize)})).unwrap(),
                    _ => unreachable!(),
                }
                let expected = &row["expected"];
                assert_eq!(state.points, array(expected,"points"), "players={n} step={step}");
                assert_eq!(state.point_ranks, array(expected,"point_ranks"), "players={n} step={step}");
                assert_eq!(state.wins, array(expected,"wins"));
                assert_eq!(state.win_ranks, array(expected,"win_ranks"));
                assert_eq!(state.last_points, array(expected,"last_points"));
                assert_eq!(state.last_winners, array(expected,"last_winners"));
                assert_eq!(state.last_placement(), array(expected,"placement"));
                assert_eq!(state.rounds_left(), expected["rounds_left"].as_i64().unwrap() as i32);
                assert_eq!(state.results_count(), expected["results_count"].as_i64().unwrap() as i32);
                assert_eq!(state.is_point_series(), expected["point_series"].as_bool().unwrap());
                if let Some(ranks) = expected["ranks"].as_array() {
                    assert_eq!(state.rounds_left(), expected["rounds_left_query"].as_i64().unwrap() as i32);
                    for (p, rank) in ranks.iter().enumerate() {
                        assert_eq!(state.player_rank(p), Ok(rank.as_i64().unwrap() as i32));
                        assert_eq!(state.point_total(p), Ok(expected["point_total_queries"][p].as_i64().unwrap() as i32));
                        assert_eq!(state.win_total(p), Ok(expected["win_total_queries"][p].as_i64().unwrap() as i32));
                        assert_eq!(state.points_in_this_match(p), Ok(expected["last_points_queries"][p].as_i64().unwrap() as i32));
                        assert_eq!(state.won_last_game(p), Ok(expected["won_last"][p].as_bool().unwrap()));
                    }
                    for (i,p) in expected["rank_players"].as_array().unwrap().iter().enumerate() {
                        let p = p.as_i64().unwrap();
                        assert_eq!(state.player_by_rank(i as i32-1), if p == -1 {None} else {Some(p as usize)});
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_invalid_players_atomically() {
        assert_eq!(MultiplayerMode::new(1), Err(MultiplayerError::InvalidPlayerCount));
        assert_eq!(MultiplayerMode::new(5), Err(MultiplayerError::InvalidPlayerCount));
        let mut state = MultiplayerMode::new(2).unwrap(); state.start_free_play();
        let before = state.clone();
        assert_eq!(state.add_win_results([Some(0),Some(2)]), Err(MultiplayerError::InvalidPlayer));
        assert_eq!(state,before);
        assert_eq!(state.player_rank(usize::MAX), Err(MultiplayerError::InvalidPlayer));
    }
}
