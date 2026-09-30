//! Pure match decisions from MGTetherball's retail PowerPC code.
//! Winner UI and state-entry work is returned as ordered requests.
//! See docs/TETHERBALL_MATCH.md for the fields and oracle boundary conditions.

#[derive(Debug,Clone,Copy,PartialEq,Eq,serde::Serialize,serde::Deserialize)]
pub struct MatchRules {
    /// +0x16c: 0 = best-of rounds, 1 = time survive; other values do nothing.
    pub mode:i32,
    pub rotation_limit:i32, // +0x430
    pub wins_required:i32, // +0x434, compared for equality, not >=
    pub time_limit_seconds:i32, // +0x17c; multiplication wraps in 32 bits
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum WinnerUi {WinLose,MultiplayerWin}
#[derive(Debug,Clone,PartialEq)]
pub enum MatchEffect {
    WinnerVisible{ui:WinnerUi,player:i32,visible:bool},
    ChangeState(u32),

}

#[derive(Debug,Clone,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct MatchState {
    pub rotations:[i8;2], // +0x132/133
    pub round_wins:[i8;2], // +0x134/135; stores wrap at 8 bits
    pub elapsed_ms:u32, // +0x258; only active serve/return/accelerate states advance this
    pub state_ms:u32, // +0x250, advances even while paused
    pub previous_state_ms:u32, // +0x254
    pub state_code:u32, // +0x34
    pub round_winner:i32, // +0x204, -1 means unset
    pub match_winner:i32, // +0x440, -1 means unset
    pub match_over:bool, // +0x20c
    /// Opaque base-minigame result words +0x60/64, kept independently.
    pub result:i32,
    pub final_result:i32,

}

impl MatchState {
    fn ui(session_mode:i32)->WinnerUi{if session_mode==1{WinnerUi::WinLose}else{WinnerUi::MultiplayerWin}}
    /// Common ChangeGameState stores (0x8039a988..994). The ChangeState request also
    /// requires state-entry animation/model/UI work, which remains outside this port.
    fn change_state(&mut self,code:u32,effects:&mut Vec<MatchEffect>){
        self.state_code=code;self.state_ms=0;self.previous_state_ms=0;
        effects.push(MatchEffect::ChangeState(code));
    }
    /// CheckIfGameIsOver dispatch: unsupported rule values make no decisions.
    pub fn check(&mut self,rules:MatchRules,session_mode:i32)->Vec<MatchEffect>{
        match rules.mode{0=>self.check_best_of_rounds(rules,session_mode),1=>self.check_time_survive(rules),_=>Vec::new()}
    }
    /// BestOfNRounds, 0x8039af7c: P0 wraps take precedence over P1 and timeout.
    pub fn check_best_of_rounds(&mut self,rules:MatchRules,session_mode:i32)->Vec<MatchEffect>{
        let winner=if self.rotations[0] as i32>=rules.rotation_limit{0}
            else if self.rotations[1] as i32>=rules.rotation_limit{1}
            else if rules.time_limit_seconds>0&&self.elapsed_ms>=rules.time_limit_seconds.wrapping_mul(1000) as u32{1}
            else{return Vec::new()};
        self.round_winner=winner;self.round_wins[winner as usize]=self.round_wins[winner as usize].wrapping_add(1);
        let mut effects=vec![MatchEffect::WinnerVisible{ui:Self::ui(session_mode),player:winner,visible:true}];
        if self.round_wins[0] as i32==rules.wins_required{
            self.match_over=true;self.result=1;self.final_result=1;self.match_winner=0;
        }else if self.round_wins[1] as i32==rules.wins_required{
            self.match_over=true;self.final_result=2;if self.result!=1{self.result=2}self.match_winner=1;
        }
        self.change_state(30,&mut effects);effects
    }
    /// TimeSurvive, 0x8039b0c4: transfer P0 wrap first, then timeout wins P0.
    /// No positive-time guard; zero timeout wins immediately. An existing
    /// match_over flag repeats win accounting, so callers must follow state dispatch.
    pub fn check_time_survive(&mut self,rules:MatchRules)->Vec<MatchEffect>{
        if self.rotations[0] as i32>=rules.rotation_limit{
            self.rotations[0]=self.rotations[0].wrapping_sub(1);self.rotations[1]=self.rotations[1].wrapping_add(1);
        }
        if self.elapsed_ms>=rules.time_limit_seconds.wrapping_mul(1000) as u32{
            self.match_over=true;self.result=1;self.final_result=1;self.match_winner=0;self.round_winner=0;
        }else if self.rotations[1] as i32>=rules.rotation_limit{
            self.match_over=true;self.result=2;self.final_result=2;self.match_winner=1;self.round_winner=1;
        }
        if !self.match_over{return Vec::new()}
        // Retail indexes +0x134 by match_winner. Invalid preexisting winner values
        // are outside its memory-safe domain; do not invent a player for them.
        let Ok(winner)=usize::try_from(self.match_winner) else{return Vec::new()};
        if winner>=2{return Vec::new()}
        self.round_wins[winner]=self.round_wins[winner].wrapping_add(1);
        let mut effects=vec![MatchEffect::WinnerVisible{ui:WinnerUi::WinLose,player:self.match_winner,visible:true}];
        self.change_state(30,&mut effects);effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json,Value};
    fn int(v:&Value,k:&str)->i32{v[k].as_i64().unwrap() as i32}
    fn state(v:&Value)->MatchState{
        let bytes=|k:&str|[v[k][0].as_i64().unwrap() as i8,v[k][1].as_i64().unwrap() as i8];
        MatchState{rotations:bytes("rotations"),round_wins:bytes("round_wins"),
            elapsed_ms:int(v,"elapsed_ms") as u32,state_ms:int(v,"state_ms") as u32,
            previous_state_ms:int(v,"previous_state_ms") as u32,state_code:int(v,"state_code") as u32,
            round_winner:int(v,"round_winner"),match_winner:int(v,"match_winner"),
            match_over:v["match_over"].as_bool().unwrap(),result:int(v,"result"),final_result:int(v,"final_result")}
    }
    #[test]
    fn original_powerpc_match_decisions(){
        let data:Value=serde_json::from_str(include_str!("../tests/data/tetherball_match_golden.json")).unwrap();
        assert_eq!(data["elf_sha256"].as_str().unwrap(),crate::recovered::ELF_SHA256);
        for (i,case) in data["cases"].as_array().unwrap().iter().enumerate(){
            let r=&case["rules"];
            let rules=MatchRules{mode:int(r,"mode"),rotation_limit:int(r,"rotation_limit"),
                wins_required:int(r,"wins_required"),time_limit_seconds:int(r,"time_limit_seconds")};
            let mut s=state(&case["initial"]);
            for (j,step) in case["steps"].as_array().unwrap().iter().enumerate(){
                let effects:Vec<Value>=s.check(rules,int(case,"session_mode")).into_iter().map(|e|match e{
                    MatchEffect::WinnerVisible{ui,player,visible}=>json!(["winner",match ui{WinnerUi::WinLose=>"single",WinnerUi::MultiplayerWin=>"multi"},player,visible]),
                    MatchEffect::ChangeState(code)=>json!(["state",code]),
                }).collect();
                assert_eq!(s,state(&step["expected"]),"case {i} step {j}");
                assert_eq!(json!(effects),step["effects"],"effects case {i} step {j}");
            }
        }
    }
}
