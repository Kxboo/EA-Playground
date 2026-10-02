//! Original MGWallball scoring, rally rotation and out-of-bounds decisions.
//! Engine services are ordered requests; see docs/WALLBALL_RULES.md.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesState {
    pub active: i32,
    pub points: [i32; 2],
    pub winner: i32,
    pub total_rounds: i32,
    pub match_over: bool,
    pub rally_hits: i32,
    pub progression_hits: i32,
    pub first_hit: bool,
    pub hit_counter_visible: bool,
    pub challenge_target: i32,
    pub mode: i32,
    pub progression_interval: u8,
    pub paused: bool,
    pub ai: [bool; 2],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Active { player: usize, active: bool },
    Positive { player: usize },
    HitCounter(i32),
    Sound { id: i32, variant: i32, volume: i32 },
    Collisions(bool),
    SpeedProgression,
    GameLogic(i32),
    WinLoss { player: usize, won: bool, ai: bool, match_over: bool },
    ChangeState(i32),
}
/// Synchronous boundaries of the original UpdateGame call graph. Delegates may
/// write live rules state; later predicates observe those writes immediately.
pub trait Services {
    fn character_count(&self, _state: &RulesState, initial: usize) -> usize { initial }
    fn disable_collisions(&mut self, state: &mut RulesState);
    fn update_speed_progression(&mut self, state: &mut RulesState);
    fn update_game_logic(&mut self, state: &mut RulesState, milliseconds: i32) -> i32;
    fn court_in_bounds(&mut self, state: &mut RulesState) -> bool;
    fn win_loss(&mut self, state: &mut RulesState, player: usize, won: bool, ai: bool, match_over: bool);
    fn change_state(&mut self, state: &mut RulesState, code: i32);
}
pub trait CycleServices {
    fn character_count(&mut self, _state:&RulesState, initial:usize)->usize {initial}
    fn effect(&mut self, state:&mut RulesState, effect:Effect);
}
impl RulesState {
    /// Complete synchronous CycleActiveCharacter. Every retail re-read occurs
    /// after the preceding callback, including active index, first-hit and the
    /// counter/interval gates. The vector helper below is a pure convenience.
    pub fn cycle_active_synchronous(&mut self, initial_count:usize, services:&mut impl CycleServices){
        let count=services.character_count(self,initial_count);
        assert!((1..=2).contains(&count) && self.active>=0 && self.active<count as i32);
        self.active=self.active.wrapping_add(1)%(count as i32);
        let mut player=0;
        while player<services.character_count(self,initial_count){
            assert!(player<2);
            services.effect(self,Effect::Active{player,active:player as i32==self.active});player+=1;
        }
        if !self.first_hit{self.first_hit=true;return;}
        self.rally_hits=self.rally_hits.wrapping_add(1);self.progression_hits=self.progression_hits.wrapping_add(1);
        if challenge(self.mode) && self.rally_hits==self.challenge_target{services.effect(self,Effect::Positive{player:0});}
        if self.hit_counter_visible{services.effect(self,Effect::HitCounter(self.rally_hits));}
        assert!(self.progression_interval!=0);
        if self.progression_hits%(self.progression_interval as i32)==0{services.effect(self,Effect::Sound{id:88,variant:0,volume:100});}
    }
    /// Valid original memory domain: one or two characters, nonzero interval,
    /// and an active index within character storage. The original has no guard.
    pub fn cycle_active(&mut self, characters: usize) -> Vec<Effect> {
        assert!((1..=2).contains(&characters) && self.active >= 0 && self.active < characters as i32);
        assert!(self.progression_interval != 0);
        self.active = self.active.wrapping_add(1) % characters as i32;
        let mut effects = (0..characters).map(|player| Effect::Active { player, active: player as i32 == self.active }).collect::<Vec<_>>();
        if !self.first_hit { self.first_hit = true; return effects; }
        self.rally_hits = self.rally_hits.wrapping_add(1);
        self.progression_hits = self.progression_hits.wrapping_add(1);
        if challenge(self.mode) && self.rally_hits == self.challenge_target { effects.push(Effect::Positive { player: 0 }); }
        if self.hit_counter_visible { effects.push(Effect::HitCounter(self.rally_hits)); }
        if self.progression_hits % self.progression_interval as i32 == 0 { effects.push(Effect::Sound { id: 88, variant: 0, volume: 100 }); }
        effects
    }
    /// Complete UpdateGame orchestration and rule body. Services run exactly at
    /// their original call sites, before subsequent live-state reads.
    pub fn update_game(&mut self, characters: usize, milliseconds: i32, services: &mut impl Services) -> i32 {
        assert!((1..=2).contains(&characters));
        services.disable_collisions(self);
        if self.paused { return 1; }
        services.update_speed_progression(self);
        let logic_result = services.update_game_logic(self, milliseconds);
        if logic_result != 1 { return logic_result; }
        if services.court_in_bounds(self) { return 1; }
        let mut player = 0;
        while player < services.character_count(self, characters) {
            assert!(player < 2);
            if player as i32 != self.active {
                self.winner = player as i32;
                self.points[player] = self.points[player].wrapping_add(1);
                if self.points[player] as u32 > (self.total_rounds / 2) as u32 { self.match_over = true; }
                services.win_loss(self, player, true, self.ai[player], self.match_over);
            } else if challenge(self.mode) {
                self.match_over = true;
                let won = self.rally_hits >= self.challenge_target;
                services.win_loss(self, player, won, false, true);
                self.winner = if won { player as i32 } else { 1 };
            } else {
                services.win_loss(self, player, false, self.ai[player], self.match_over);
            }
            player += 1;
        }
        services.change_state(self, 7);
        1
    }
}
fn challenge(mode: i32) -> bool { (mode.wrapping_sub(32) as u32) <= 2 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionRules { pub kind: i32, pub difficulty: i32, pub rounds_to_win: i32, pub powerups: i32, pub alternate: i32 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedRules { pub kind: i32, pub difficulty: i32, pub total_rounds: i32, pub powerups: [bool; 3], pub alternate: bool }
impl AppliedRules {
    pub fn process(&mut self, rules: Option<SessionRules>) {
        if let Some(r) = rules {
            self.kind = r.kind; self.difficulty = r.difficulty;
            self.total_rounds = r.rounds_to_win.wrapping_mul(2).wrapping_sub(1);
            self.powerups = [r.powerups == 1; 3]; self.alternate = r.alternate == 1;
        }
    }
}
/// CalcScore counts hits (+f4), warp balls (+104), super hits (+100),
/// and round points (+1dc). Mega boosts are deliberately excluded.
pub fn calc_score(counts: [i32; 4], weights: [i32; 4]) -> i32 {
    counts.into_iter().zip(weights).fold(0i32, |sum, (count, weight)| sum.wrapping_add(count.wrapping_mul(weight)))
}
