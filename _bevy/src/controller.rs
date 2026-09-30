//! Original Controller button timers and CSV event dispatch; hardware/rumble/FE excluded.
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub struct Binding {
    pub action:u32, pub state:u32, pub transition:u32, pub kind:u32,
    pub required:[u32;2], pub forbidden:[u32;2], pub button:u32,
}
#[derive(Debug,Clone,Copy,Default,PartialEq,Eq)]
pub struct EventState { pub active:bool, pub held_ms:u32 }
#[derive(Debug,Clone)]
pub struct Controller {
    bindings:Vec<Binding>, events:[EventState;189], held:u32,
    since_down:[u32;14], between_down:[u32;14], held_ms:[u32;14],
    states:Vec<u32>,
    pub pad_index:u32, pub debug_enabled:bool, pub freecam_enabled:bool,
}
impl Controller {
    pub fn new(bindings:Vec<Binding>,state:u32)->Self {
        Self { bindings,events:[EventState::default();189],held:0,
            since_down:[u32::MAX;14],between_down:[u32::MAX;14],held_ms:[0;14],
            states:vec![state],pad_index:0,debug_enabled:true,freecam_enabled:true }
    }
    pub fn current_state(&self)->u32 { *self.states.last().unwrap() }
    pub fn push_state(&mut self,state:u32) { if self.states.len()<64 {self.states.push(state);} }
    /// Original PopState immediately removes one context, retaining the root.
    pub fn pop_state(&mut self) { if self.states.len()>1 {self.states.pop();} }
    pub fn event(&self,action:u32)->EventState { self.events.get(action as usize).copied().unwrap_or_default() }
    pub fn update(&mut self,held:u32,input_ms:i32) {
        let released=self.held & !held;
        let down=held & !self.held;
        let dt=input_ms as u32;
        self.held=held;
        for b in 0..14 {
            let mask=1u32<<b;
            if down&mask!=0 {self.held_ms[b]=0;} else if held&mask!=0 {self.held_ms[b]=self.held_ms[b].wrapping_add(dt);}
            if down&mask!=0 {self.between_down[b]=self.since_down[b]; self.since_down[b]=0;}
            else {self.since_down[b]=self.since_down[b].wrapping_add(dt);}
        }
        self.events.fill(EventState::default());
        let current=self.current_state();
        let mut next=current;
        for row in &self.bindings {
            if row.state!=current && (row.state!=28 || next==row.transition) {continue;}
            let b=row.button as usize;
            if b>=14 {continue;}
            let modifiers=row.required.iter().all(|&m|m==0 || (m<32 && held&(1<<m)!=0))
                && row.forbidden.iter().all(|&m|m==0 || (m<32 && held&(1<<m)==0));
            if !modifiers {continue;}
            let mask=1u32<<b;
            let time=self.held_ms[b];
            let fires=match row.kind {
                0=>released&mask!=0,
                1=>down&mask!=0,
                2=>down&mask!=0 && self.between_down[b]<500,
                3=>held&mask!=0,
                4=>held&mask!=0 && time>=1000 && (time.wrapping_sub(dt) as i32)<1000,
                5=>released&mask!=0 && time<180,
                6=>held&mask!=0 && time>=180 && time.wrapping_sub(dt)<180,
                7=>held&mask!=0 && time>=120,
                _=>false,
            };
            if !fires {continue;}
            if let Some(event)=self.events.get_mut(row.action as usize) {*event=EventState{active:true,held_ms:time};}
            if row.transition!=31 {next=row.transition;}
            if next==8 && (self.pad_index!=0 || !self.debug_enabled) {next=current;}
            if next==6 && (self.pad_index!=0 || !self.freecam_enabled) {next=current;}
        }
        if next!=current {
            if next==27 { if self.states.len()>1 {self.states.pop();} }
            else {self.push_state(next);}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_powerpc_controller_vectors() {
        let root:serde_json::Value=serde_json::from_str(include_str!("../tests/data/controller_golden.json")).unwrap();
        assert_eq!(root["elf_sha256"].as_str(),Some(crate::recovered::ELF_SHA256));
        for session in root["sessions"].as_array().unwrap() {
            let rows=session["bindings"].as_array().unwrap().iter().map(|r| {
                let u=|k:&str|r[k].as_u64().unwrap() as u32;
                let pair=|k:&str| std::array::from_fn(|i|r[k][i].as_u64().unwrap() as u32);
                Binding{action:u("action"),state:u("state"),transition:u("transition"),kind:u("kind"),required:pair("required"),forbidden:pair("forbidden"),button:u("button")}
            }).collect();
            let mut c=Controller::new(rows,session["state"].as_u64().unwrap() as u32);
            c.pad_index=session["pad_index"].as_u64().unwrap() as u32;
            c.debug_enabled=session["debug_enabled"].as_bool().unwrap(); c.freecam_enabled=session["freecam_enabled"].as_bool().unwrap();
            for frame in session["frames"].as_array().unwrap() {
                if frame["pop"].as_bool().unwrap_or(false) {c.pop_state();}
                c.update(frame["held"].as_u64().unwrap() as u32,frame["ms"].as_i64().unwrap() as i32);
                assert_eq!(c.current_state(),frame["current_state"].as_u64().unwrap() as u32,"{frame}");
                let events:Vec<(u32,u32)>=(0..189).filter_map(|a| {let e=c.event(a);e.active.then_some((a,e.held_ms))}).collect();
                let expected:Vec<(u32,u32)>=frame["events"].as_array().unwrap().iter().map(|a|(a[0].as_u64().unwrap() as u32,a[1].as_u64().unwrap() as u32)).collect();
                assert_eq!(events,expected,"{frame}");
                for (key,values) in [("since_down",c.since_down),("between_down",c.between_down),("held_ms",c.held_ms)] {
                    let expected:Vec<u32>=frame[key].as_array().unwrap().iter().map(|a|a.as_u64().unwrap() as u32).collect();
                    assert_eq!(values.to_vec(),expected,"{key}: {frame}");
                }
            }
        }
    }
}
