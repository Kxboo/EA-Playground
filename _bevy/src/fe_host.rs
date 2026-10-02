//! Game side of the frontend API: the APT scripts call `CallGameFunc(name, "k=v&k2=v2", ...)`, which becomes either
//! `getURL("FSCommand:name?params")` (no result) or `LoadVars.load("name?params")` (result = variables set on the
//! LoadVars object).  Handler names/parameters come from the executable's handler tables and `debug/ApiStubData`.
use crate::apt_vm::{Vm,V};

#[derive(Default)]
pub struct Fe{
    pub first_screen:String,
    pub locale:i32,
    pub calls:Vec<String>,
    /// `SetAPTRenderCallback`: script function the engine invokes after `StartAPTRender` (render resumed).
    pub render_cb:Option<(String,String)>,
    pub render_cb_due:Option<u32>,
    /// Exposed-function calls the game makes back into the frontend; run after the calling script returns.
    pub todo:Vec<(String,Vec<V>)>,
    /// Calls delayed by a number of ticks (screens register their exposed functions while they initialise).
    pub later:Vec<(u32,String,Vec<V>)>,
    pub profiles:Profiles,
    pub mp:Mp,
    /// Set when the front end hands over to a minigame.
    pub launch:Option<String>,
    pub launch_done:bool,
    pub hud_loaded:bool,
    /// A minigame runs in the PowerPC VM (`mg_session`): FE buttons are forwarded as guest callbacks through `mg_cmds`.
    pub vm_session:bool,
    /// `MinigameLVHandlers::GetNumberOfHuds` answer of the VM-hosted game (published by `mg_session` every frame).
    pub mg_num_huds:i32,
    pub mg_cmds:Vec<String>,
    pub start_anim_done:bool,
    pub serve_bubble:bool,
    /// Choice made on a post-game screen for the minigame to act on (0 replay, 1 leave); see `UpdatePostGame`.
    pub postgame_choice:Option<i32>,
    /// Post-game state shared with `minigame_session` (tournament, payload, singleton flags).
    pub pg:crate::fe_postgame::Pg,
    /// `MGTetherball::OpenPauseMenu` hands the pre-game screen its parameters: [1, controlling player, multiplayer, game type].
    pub pause_words:Option<[u32;4]>,
    /// Front-end calls to run once the minigame has finished leaving (screens to return to).
    pub after_exit:Vec<(u32,String,Vec<V>)>,
    /// Pause overlay state: set while the `PauseMenu` overlay is open; the pause buttons leave a request here.
    pub paused:bool,
    pub pause_req:Option<PauseReq>,
    /// Menu camera: collection of `main_menu_nis` to fly to, set by the game when a main-menu choice is made.
    pub nis:Option<String>,
    /// Class name of the screen that last reported `ScreenReady`.
    pub screen:String,
    pub screen_changed:bool,
    /// Test hook: the `pause` script command requests the pause menu.
    pub script_pause:bool,
    /// Test hook: `wr` / `ws` open the report card / sticker book from the world.
    pub script_world:Option<&'static str>,
    /// Selectable-kid scene (see `kid_pick`): roster from the vault, hovered kid, click awaiting a pick, chosen kids (per player).
    pub roster:Vec<crate::kid_pick::KidInfo>,
    pub kid_mode:u8,
    pub kid_hover:Option<usize>,
    pub kid_click:Option<(f32,f32)>,
    pub kid_chosen:Vec<Option<usize>>,
    /// Set by the kid scene when a kid is clicked on the select-kid screen; the host then opens the name keyboard.
    pub kid_picked:Option<usize>,
    /// The name keyboard overlay is open: clicks belong to it, not to the kids behind it.
    pub keyboard_open:bool,
    /// Whether the player-setup Next button was last told to show (it is only toggled on a change: re-running its `animateIn`/`animateOut` rebuilds the button without its loaded art).
    pub next_vis:bool,
}

#[derive(Clone,Copy,Debug,PartialEq)]
pub enum PauseReq{Resume,Restart,Quit}

#[derive(Clone,Copy,Debug)]
pub struct MpResult{pub winner:i32,pub scores:[i32;2],pub hits:[i32;2],pub power_hits:[i32;2]}

/// Session state of the multiplayer front-end flow.
#[derive(Clone,Debug,Default)]
pub struct Mp{
    pub active:bool,pub quick:bool,pub players:i32,pub avatars:Vec<i32>,pub minigame:i32,pub rounds:i32,pub style:i32,pub rules:Vec<i32>,
    pub results:Option<MpResult>,
    /// `aiTeams` of the team-select screen (one entry per player; `SetTeams` reads < 2 as team 0); empty = not a team game.
    pub teams:Vec<i32>,
}

#[derive(Clone,Debug,Default,serde::Serialize,serde::Deserialize)]
pub struct Slot{pub name:String,pub grade:i32,pub image:i32}
/// The three save slots of the profile screen plus the session state of the profile flow.
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct Profiles{
    pub slots:Vec<Option<Slot>>,
    #[serde(skip)] pub loaded:i32,
    #[serde(skip)] pub boot:bool,
    #[serde(skip)] pub selected:i32,
    #[serde(skip)] pub filter:i32,
    #[serde(skip)] pub kid:i32,
    /// Name typed on the keyboard screen for the kid being created.
    #[serde(skip)] pub typed:String,
    #[serde(skip)] pub loaded_from_disk:bool,
}
impl Default for Profiles{fn default()->Self{Profiles{slots:vec![None,None,None],loaded:-1,boot:true,selected:-1,filter:0,kid:0,typed:String::new(),loaded_from_disk:false}}}
impl Profiles{
    fn path()->std::path::PathBuf{crate::bridge::root().join("saves").join("profiles.json")}
    pub fn ensure_loaded(&mut self){
        if self.loaded_from_disk{return}
        self.loaded_from_disk=true;
        if let Ok(t)=std::fs::read_to_string(Self::path()){
            if let Ok(p)=serde_json::from_str::<Profiles>(&t){self.slots=p.slots;self.slots.resize(3,None);}
        }
    }
    pub fn save(&self){
        let path=Self::path();
        if let Some(d)=path.parent(){let _=std::fs::create_dir_all(d);}
        if let Ok(t)=serde_json::to_string_pretty(self){let _=std::fs::write(path,t);}
    }
}

/// Selectable kids of the select-kid screen (head image id = index into KidHeads, grade is the starting report-card grade).
/// Array delimiter of the APT <-> game protocol (`_global.DELIMITER` = `String.fromCharCode(127)`).
pub const DELIM:&str="\u{7f}";
pub const KIDS:[&str;12]=["ALICIA","CANTON","VICKY","KYLE","JOSH","MAYA","TANYA","ZACK","LUCY","MARCUS","NINA","SAM"];

/// Display name of roster kid `idx` (locale key from the vault, upper-cased like the original text).
pub fn kid_name(vm:&Vm,idx:usize)->String{
    if let Some(k)=vm.fe.roster.get(idx){
        let t=if k.name_key.is_empty(){String::new()}else{vm.locale_string(&k.name_key)};
        if !t.is_empty()&&t!=k.name_key{return t.to_uppercase()}
        return k.asset.trim_end_matches(char::is_numeric).replace('_'," ").to_uppercase()
    }
    KIDS[idx%KIDS.len()].to_string()
}

/// Start the chosen minigame: only Tetherball (`MinigameEnum` 2) exists in this port; for any other the original screens have run
/// (rules, instructions) and a system message box says so, leaving the player on the pre-game screen.
fn start_game(vm:&mut Vm){
    if vm.fe.vm_session{vm.fe.mg_cmds.push("OnPlay".into());return}
    if vm_game(vm.fe.mp.minigame){let n=vm.fe.mp.minigame;vm.fe.todo.push(("ClearScreenStack".into(),vec![]));vm.fe.launch=Some(format!("mg:{n}"));return}
    if vm.fe.mp.minigame==2{vm.fe.todo.push(("CloseScreen".into(),vec![]));vm.fe.launch=Some("tetherball".into());return}
    let ok=vm.locale_string("$OK");
    vm.fe.todo.push(("TRCDisplayPopup".into(),vec![V::Str("notimpl".into()),V::Str("This minigame is not available in this port.".into()),V::Num(1.),V::Str(ok.as_str().into()),V::Str("0".into()),V::Num(0.),V::Num(0.)]));
}


/// `default_rules` of the minigame (`RetrieveDefaultRules`, 0x8031aef8), in the order tetherball's rule rows use:
/// location, difficulty, rotations to win, mega hit on, rounds.
pub fn tetherball_default_rules()->Vec<i32>{
    static RULES:std::sync::OnceLock<Vec<i32>>=std::sync::OnceLock::new();
    RULES.get_or_init(||{
        let dir=crate::bridge::data_root().join("files").join("data").join("db");
        let fallback=vec![0,0,5,1,3];
        let (Ok(v),Ok(b))=(std::fs::read(dir.join("db.vlt")),std::fs::read(dir.join("db.bin"))) else{return fallback};
        let Ok(db)=crate::vlt::Database::load(&v,&b,crate::vlt::known_names()) else{return fallback};
        let Some(c)=db.find_collection("mg_tetherball","default_rules") else{return fallback};
        let int=|n:&str|db.attribute(c,n).and_then(|v|v.as_i64().or_else(||v.as_bool().map(|b|b as i64))).unwrap_or(0) as i32;
        vec![int("location"),int("difficulty"),int("rotations_to_win"),int("megahit_on"),int("num_rounds")]
    }).clone()
}

/// `MultiPlayerLVHandlers::SetTetherballRule` (0x8031cc14): one row of the custom-rules screen.  Rows: 0 location (three
/// areas; the original locks an area until the profile has visited it), 1 difficulty, 2 rotations to win (5..10),
/// 3 mega hit on/off, 4 rounds (odd values 3 and 5).
fn tetherball_rule(vm:&Vm,rule:usize,current:&[i32])->Vec<(String,String)>{
    let t=|k:&str|vm.locale_string(k);
    let cur=current.get(rule).copied().unwrap_or(0);
    let nums=["B_NumericZero","B_NumericOne","B_NumericTwo","B_NumericThree","B_NumericFour","B_NumericFive","B_NumericSix","B_NumericSeven","B_NumericEight","B_NumericNine","B_NumericTen"];
    let (title,text,values,default):(&str,Vec<String>,Vec<i32>,i32)=match rule{
        0=>("T_Location",["B_SchoolYard","B_Stadium","B_Woods"].iter().map(|k|t(k)).collect(),vec![0,1,2],cur),
        1=>("T_Difficulty",["B_Easy","B_Medium","B_Hard"].iter().map(|k|t(k)).collect(),vec![0,1,2],cur),
        2=>("T_RotationsToWin",(5..=10).map(|n|t(nums[n])).collect(),(5..=10).collect(),cur-5),
        3=>("T_MegaHit",vec![t("B_Off"),t("B_On")],vec![0,1],cur),
        _=>{
            // SetRoundsRule: only odd round counts are offered; the default is the option holding the current value.
            let odd:Vec<i32>=(3..=5).filter(|n|n%2!=0).collect();
            let idx=odd.iter().position(|&n|n==cur).map(|i|i as i32).unwrap_or(1);
            ("T_Rounds",odd.iter().map(|&n|t(nums[n as usize])).collect(),odd,idx)
        }
    };
    let n=values.len();
    vec![("iRuleText".into(),t(title)),("iNumberOptions".into(),n.to_string()),("aiOptionsText".into(),text.join(DELIM)),
         ("aiOptionsValue".into(),values.iter().map(|v|v.to_string()).collect::<Vec<_>>().join(DELIM)),("aiLocked".into(),vec!["0";n].join(DELIM)),
         ("iDefaultOption".into(),default.to_string())]
}

/// Split `k=v&k2=v2` into pairs (values percent-decoded).
pub fn parse_params(p:&str)->Vec<(String,String)>{
    p.split('&').filter(|s|!s.is_empty()).map(|kv|{
        let (k,v)=kv.split_once('=').unwrap_or((kv,""));
        (k.to_string(),unescape(v))
    }).collect()
}
pub fn unescape(s:&str)->String{
    let b=s.as_bytes();let mut o=vec![];let mut i=0;
    while i<b.len(){
        if b[i]==b'%'&&i+2<b.len()+0&&i+2<=b.len()-1{ if let Ok(v)=u8::from_str_radix(&s[i+1..i+3],16){o.push(v);i+=3;continue} }
        o.push(b[i]);i+=1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

/// Run a game API function. Returns the variables to set on the caller's LoadVars (empty for fire-and-forget).
pub fn game_call(vm:&mut Vm,name:&str,params:&str)->Vec<(String,String)>{
    let args=parse_params(params);
    let arg=|k:&str|args.iter().find(|(a,_)|a==k).map(|(_,v)|v.clone());
    vm.fe.calls.push(format!("{name}?{params}"));
    // Post-game screen: the verified shared port answers the queries and the button commands.
    if name.starts_with("PostGame_")||name.starts_with("EndTourney_")||name=="MultiPlayer_PostGameOnSelect"{
        let player=arg("iPlayerId").and_then(|v|v.parse().ok()).unwrap_or(0);
        if let Some(r)=crate::fe_postgame::query(vm,name,player){return r}
        let sel=arg("iSelected").or_else(||arg("iButton")).and_then(|v|v.parse().ok()).unwrap_or(0);
        if crate::fe_postgame::command(vm,name,sel){return vec![]}
    }
    match name{
        // `MinigameLVHandlers::GetNumberOfHuds` (0x80319520): Dart / Paper `GetNumHuds`, RcCars +0x16c, else 0
        "GetNumberOfHuds"=>vec![("iNumHuds".into(),vm.fe.mg_num_huds.to_string())],
        "GetStartScreenFromMain"=>vec![("strFirstScreen".into(),vm.fe.first_screen.clone())],
        "GetLocale"|"GetLocaleFE"=>vec![("iLocale".into(),vm.fe.locale.to_string())],
        "GetAspectRatio"=>vec![("iAspectRatio".into(),"1".into())],
        "PG_GetBuildType"=>vec![("iBuildType".into(),"0".into())],
        "GetLocalizedString"=>{
            let key=arg("LocString").unwrap_or_default();
            let text=vm.locale_string(&key);
            vec![("LocString".into(),text)]
        }
        "SetAPTRenderCallback"=>{
            let n=arg("strAptRenderCallbackName").unwrap_or_default();let s=arg("strAptRenderCallbackScope").unwrap_or_default();
            vm.fe.render_cb=Some((n,s));vec![]
        }
        "StopAPTRender"=>vec![],
        "StartAPTRender"=>{ vm.fe.render_cb_due=Some(1);vec![] }
        "ScreenReady"=>{vm.fe.screen_changed=true;vec![]}
        "MainMenu_OnSelect"=>{
            let idx=arg("iSelectedIndex").and_then(|v|v.parse::<i32>().ok()).unwrap_or(-1);
            // Quick Play shares the multiplayer game-select / rules screens with one player; Multi-Player starts the setup flow.
            if idx==1{let kid=vm.fe.profiles.slots.get(vm.fe.profiles.loaded.max(0) as usize).and_then(|s|s.as_ref()).map(|s|s.image).unwrap_or(0);
                let mp=&mut vm.fe.mp;mp.quick=true;mp.active=true;mp.players=1;mp.avatars=vec![kid];}
            else if idx==2{vm.fe.mp.quick=false;}
            else if idx==0{vm.fe.launch=Some("world".into());}
            vm.fe.nis=match idx{0=>Some("single_player"),1=>Some("quick_play"),2=>Some("multi_player"),3=>Some("my_profile"),4=>Some("credits"),_=>None}.map(String::from);
            vec![]
        }
        "PlayAEMSsfx"=>{if let Some(n)=arg("nFEsfxID"){crate::fe_sfx::play(&n);}vec![]}
        "ScreenLeaving"|"ActivateRumble"|"SetCursorVisibility"|"ReturnCursorVisibility"|"TRCUserCloseNIS"=>vec![],
        "MultiPlayer_HowManyPlayersOnLoad"=>vec![("aiActiveControllers".into(),["1";4].join(DELIM))],
        "MultiPlayer_NumPlayersBack"=>{vm.fe.mp.active=false;vec![]}
        "MultiPlayer_SetNumberPlayers"=>{
            let n=arg("iNumberPlayers").and_then(|v|v.parse::<i32>().ok()).unwrap_or(2).clamp(2,4);
            let mp=&mut vm.fe.mp;mp.active=true;mp.players=n;mp.avatars=(0..n).map(|i|i).collect();mp.teams.clear();vec![]
        }
        "MultiPlayer_SetUpOnLoad"=>{
            vm.fe.kid_mode=2;
            vm.fe.kid_chosen=vec![None;vm.fe.mp.players.max(2) as usize];
            vm.fe.next_vis=false;
            vec![("iNumPlayers".into(),vm.fe.mp.players.to_string())]
        }
        "PlayerSetup_Exit"=>{vm.fe.todo.push(("CloseScreen".into(),vec![]));vec![]}
        "PlayerSetup_OnNext"=>{
            let picks:Vec<i32>=vm.fe.kid_chosen.iter().enumerate().map(|(i,c)|c.map(|k|k as i32).unwrap_or(i as i32)).collect();
            if let Some(f)=picks.first(){vm.fe.profiles.kid=*f;}
            vm.fe.mp.avatars=picks;
            vm.fe.todo.push(("OpenScreen".into(),vec![V::Str("GameSelect".into())]));vec![]}
        "MultiPlayer_SelectGameOnLoad"=>{
            // `MultiPlayerLVHandlers::SelectGameOnLoad` (0x8031b830) fills nine slots with 1 for both arrays on this data set's build
            // (a slot is hidden only for the three games it special-cases when the game-type global is above 2), so every minigame is
            // selectable.  Games this port has not implemented are told so when they are started (see `unavailable_game`).
            vec![("aiHideMinigame".into(),["1";9].join(DELIM)),("aiLockedMinigame".into(),["1";9].join(DELIM)),
                 ("iFlowState".into(),"0".into()),("iGameType".into(),(if vm.fe.mp.quick{"0"}else{"1"}).into()),("iIsNewHighScore".into(),"0".into())]
        }
        "MultiPlayer_SetMinigame"=>{vm.fe.mp.minigame=arg("iMiniGameType").and_then(|v|v.parse().ok()).unwrap_or(2);vec![]}
        "MultiPlayer_SetNumRounds"=>{vm.fe.mp.rounds=arg("iNumberRounds").and_then(|v|v.parse().ok()).unwrap_or(3);vec![]}
        "MultiPlayer_SetGameStyle"=>{vm.fe.mp.style=arg("iMultiPlayerGameType").and_then(|v|v.parse().ok()).unwrap_or(0);vec![]}
        "ScrCreditsInit"=>{
            // `CreditScreenLVHandlers::GetScreenCredits`: header key `T_Credits_Header%.2i`, names `T_Credits_%.2i_%.2i`, `iAnyMore` while the next header exists.
            let i=arg("iCreditIndex").and_then(|v|v.parse::<usize>().ok()).unwrap_or(0);
            let has=|k:&str|vm.locale.as_ref().is_some_and(|l|l.get(k).is_some());
            let task=vm.locale_string(&format!("T_Credits_Header{i:02}"));
            let names:Vec<String>=(0..).map(|j|format!("T_Credits_{i:02}_{j:02}")).take_while(|k|has(k)).map(|k|vm.locale_string(&k)).collect();
            let more=has(&format!("T_Credits_Header{:02}",i+1));
            vec![("strTask".into(),task),("astrNames".into(),names.join(DELIM)),("iAnyMore".into(),(more as i32).to_string())]
        }
        "PreGame_OnLoad"=>{
            // iScreenType: 0 = pre-game, 1 = pause menu (BECONSTANT_PAUSE_MENU); iGameMode: 0 single player, 1 multiplayer.
            let (screen,mode)=match vm.fe.pause_words{Some(w)=>(1,w[2] as i32),None=>(0,0)};
            vec![("iMiniGameType".into(),vm.fe.mp.minigame.to_string()),("aiGestureIsLocked".into(),["0";6].join(DELIM)),("iGameMode".into(),mode.to_string()),("iScreenType".into(),screen.to_string())]
        }
        "PreGame_GetSinglePlayerInfo"=>vec![("iIsDare".into(),"0".into())],
        "PreGame_GetMultiPlayerInfo"=>vec![("iMultiPlayerMode".into(),"0".into()),("iNumPlayers".into(),vm.fe.mp.players.to_string()),("aiGamesWon".into(),"0".into()),("aiScore".into(),"0".into()),("aiRank".into(),"0".into()),("iControllingPlayer".into(),vm.fe.pause_words.map(|w|w[1]).unwrap_or(0).to_string())],
        "PreGame_GetDareText"=>vec![("iText".into(),String::new()),("strTitle".into(),String::new())],
        "PreGame_OnPlay"=>{start_game(vm);vec![]}
        "EndGame_OnLoad"=>vec![("aiBeatenMinigames".into(),["0";7].join(DELIM)),("iLastBeatenMinigame".into(),"0".into()),("iAllGamesFinished".into(),"0".into())],
        "StickerBookCover_LoadLayout"=>{
            let pr=&vm.fe.profiles;
            let name=pr.slots.get(pr.loaded.max(0) as usize).and_then(|s|s.as_ref()).map(|s|s.name.clone()).unwrap_or_else(||"PLAYER".into());
            let key=if name.ends_with('S'){"T_StickerBookCoverTitle01"}else{"T_StickerBookCoverTitle00"};
            let title=vm.locale_string(key).replace("%ls",&name);
            vec![("iPlayerName".into(),title),("iLayout".into(),["1";8].join(DELIM)),("iMarble".into(),"0".into()),("iSticker".into(),"0".into()),("iStickerTotal".into(),"0".into()),("iIsMusicEnabled".into(),"1".into())]
        }
        "StickerBookCover_IsGameDirty"=>vec![("iDirty".into(),"0".into())],
        "StickerBookCover_OnWorld"|"StickerBookCover_OnQuit"|"StickerStoreCover_OnExit"|"StickerBook_Exit"|"ReportCard_OnClose"=>{
            vm.fe.todo.push(("CloseScreen".into(),vec![]));vec![]
        }
        "SelectPlane_Exit"=>{
            let plane=args.first().and_then(|(_,v)|v.trim().parse::<i32>().ok()).unwrap_or(0);
            if std::env::var("EAGL_MG_DEBUG").is_ok(){eprintln!("[fe] SelectPlane_Exit params {args:?}");}
            if vm.fe.vm_session{vm.fe.mg_cmds.push(format!("OnPlaneSelected:{plane}"));}
            vm.fe.todo.push(("CloseScreen".into(),vec![]));vec![]
        }
        "StickerBookCover_OnSave"|"StickerBookCover_OnMusic"|"StickerBookCover_OnSelect"|"StickerBook_LayoutSave"=>vec![],
        "StickerStoreCover_OnLoad"=>vec![("aiAvailableGames".into(),["1","1","1","1","1","1","1"].join(DELIM)),("iMarble".into(),"0".into()),("iStickerTotal".into(),"0".into())],
        "SelectPlane_OnLoad"=>vec![("aiAvailablePlanes".into(),["1";5].join(DELIM))],
        "MultiPlayer_GameRulesOnLoad"=>{
            // `GameRulesOnLoad`: the per-minigame rule count (tetherball 5); only tetherball's rows are ported.
            if vm.fe.mp.rules.is_empty(){vm.fe.mp.rules=tetherball_default_rules();}
            vec![("iNumberRules".into(),(if vm.fe.mp.minigame==2{5}else{0}).to_string())]
        }
        "MultiPlayer_GetGameRule"=>{
            let rule=arg("iRule").and_then(|v|v.parse::<usize>().ok()).unwrap_or(0).min(4);
            let cur=vm.fe.mp.rules.clone();
            tetherball_rule(vm,rule,&cur)
        }
        "MultiPlayer_SetGameRules"=>{
            // `SetGameRules`: aiRules -> MultiplayerMode::SetRules (five ints), then the game launches (here: after the pre-game screen).
            if let Some(a)=arg("aiRules"){let v:Vec<i32>=a.split(DELIM).filter_map(|x|x.parse().ok()).collect();if v.len()>=5{vm.fe.mp.rules=v;}}
            if vm_game(vm.fe.mp.minigame){start_game(vm);}else{let next=if vm.fe.mp.quick{"PreGameInstructions"}else{"PreGameMP"};vm.fe.later.push((20,"OpenScreen".into(),vec![V::Str(next.into())]));}
            vec![]
        }
        "Conversation_Init"=>vec![],
        "Conversation_GetName"=>vec![("iCharacterName".into(),String::new())],
        "Conversation_GetDialogueText"=>vec![("iDialogueText".into(),String::new()),("iHasNextNode".into(),"0".into())],
        "Conversation_GetResponses"=>vec![("iResponse".into(),String::new())],
        "WorldHud_LoadComplete"|"InfoDialogue_OnButtonClick"=>vec![],
        "ReportCard_OnLoad"=>{
            // `ReportCardLVHandlers::ReportCardOnLoad` (0x80321310) at the state `StartNewProfile` leaves a profile in: every count is
            // zero.  Totals: `CharacterProfile::GetTotalAvailableAbilities` 24, `GetMaxGameStickers` 24, `GetTotalNumAreas` 4,
            // dares = non-empty `minigame_dare_1..3` of the `minigames` collections (65), hidden marbles = sum of the four `marbles`
            // position arrays (75), high fives = `bestiary` entries with `highfiveable` (27), bug hunts 2, dribbling 4, free throws 3.
            let title=vm.locale_string("T_RC_Title");
            vec![("aiValues".into(),["0";9].join(DELIM)),("aiTotals".into(),["24","24","4","65","75","27","2","4","3"].join(DELIM)),
                 ("iCurrentNumMarble".into(),"0".into()),("iCurrentNumStickers".into(),"0".into()),("iTotalNumStickers".into(),"48".into()),
                 ("iGrade".into(),"0".into()),("strTitle".into(),title)]
        }
        "Pause_OnLoadComplete"=>vec![],
        "Pause_OnKeepPlaying"=>{vm.fe.pause_req=Some(PauseReq::Resume);vec![]}
        "Pause_OnRestart"=>{vm.fe.pause_req=Some(PauseReq::Restart);vec![]}
        "Pause_OnQuit"=>{vm.fe.pause_req=Some(PauseReq::Quit);vec![]}
        "Hud_LoadComplete"=>{vm.fe.hud_loaded=true;if vm.fe.vm_session{vm.fe.mg_cmds.push("OnHudLoadComplete".into());}vec![]}
        "GameStartAnim_Complete"=>{vm.fe.start_anim_done=true;vm.fe.serve_bubble=true;if vm.fe.vm_session{vm.fe.mg_cmds.push("OnGameStartAnimComplete".into());}vec![]}
        "QuickPlay_SaveScores"|"RoundSelect_OnBack"=>vec![],
        "MultiPlayer_TeamSelectOnLoad"=>{
            let mp=&vm.fe.mp;
            let av:Vec<String>=(0..mp.players.max(2)).map(|i|mp.avatars.get(i as usize).copied().unwrap_or(i).to_string()).collect();
            vec![("iIsTeamGame".into(),((mp.players>=4) as i32).to_string()),("iIsCoop".into(),"0".into()),("iNumPlayers".into(),mp.players.to_string()),("aiPlayerAvatar".into(),av.join(DELIM))]
        }
        "MultiPlayer_SetTeams"=>{
            // `MultiPlayerFSHandlers::SetTeams` (0x8031a684): the VM-hosted game builds its Teams from these
            if let Some(a)=arg("aiTeams"){vm.fe.mp.teams=a.split(DELIM).filter_map(|x|x.parse().ok()).collect();}
            vec![]
        }
        "MultiPlayer_SetRulesType"=>{
            // "Play" (0): the rules screen stack is cleared and the game opens its pre-game instructions.
            let t=arg("iMultiPlayerRulesType").and_then(|v|v.parse::<i32>().ok()).unwrap_or(0);
            if t==0{vm.fe.mp.rules=tetherball_default_rules();}
            if t==0{if vm_game(vm.fe.mp.minigame){start_game(vm);}else{let next=if vm.fe.mp.quick{"PreGameInstructions"}else{"PreGameMP"};vm.fe.later.push((20,"OpenScreen".into(),vec![V::Str(next.into())]));}}
            vec![]
        }
        "MultiPlayer_PreRulesOnLoad"=>{
            let mp=&vm.fe.mp;
            let rules=["T_Rule_Tetherball01","T_Rule_Tetherball02","T_Rule_Tetherball03"].iter().map(|k|vm.locale_string(k)).collect::<Vec<_>>();
            let n=mp.players.max(2) as usize;
            vec![("aiRuleList".into(),rules.join(DELIM)),("aiPlayerScore".into(),vec!["0";n].join(DELIM)),("aiPlayerRank".into(),(0..n).map(|i|i.to_string()).collect::<Vec<_>>().join(DELIM)),
                 ("iMinigame".into(),mp.minigame.to_string()),("iNumberPlayers".into(),n.to_string())]
        }
        "MultiPlayer_PreGameOnPlay"=>{start_game(vm);vec![]}
        "TRCSetPopupOption"=>vec![],
        "ProfileSelect_OnLoad"=>{
            let pr=&mut vm.fe.profiles;pr.ensure_loaded();
            let j=|f:&dyn Fn(&Option<Slot>)->String|pr.slots.iter().map(|s|f(s)).collect::<Vec<_>>().join(DELIM);
            vec![
                ("aiProfileNames".into(),j(&|s|s.as_ref().map(|s|s.name.clone()).unwrap_or_default())),
                ("aiProfileGradeId".into(),j(&|s|s.as_ref().map(|s|s.grade.to_string()).unwrap_or("0".into()))),
                ("aiProfileImageId".into(),j(&|s|s.as_ref().map(|s|s.image.to_string()).unwrap_or("0".into()))),
                ("aiProfileSlots".into(),j(&|s|if s.is_some(){"1".into()}else{"0".into()})),
                ("iCurrentlyLoadedSlot".into(),pr.loaded.to_string()),
                ("iIsBootFlow".into(),(pr.boot as i32).to_string()),
            ]
        }
        "ProfileSelect_OnSelect"=>{
            let idx=arg("iSelectedProfileIndex").and_then(|v|v.parse::<i32>().ok()).unwrap_or(-1);
            let pr=&mut vm.fe.profiles;pr.selected=idx;
            let used=pr.slots.get(idx as usize).is_some_and(|s|s.is_some());
            if used{
                pr.loaded=idx;pr.boot=false;
                let boot=pr.boot;
                let _=boot;
                vm.fe.todo.push(("OpenScreen".into(),vec![V::Str("MainMenu".into())]));
            }else{
                vm.fe.todo.push(("OpenScreen".into(),vec![V::Str("SelectKid".into())]));
            }
            vec![]
        }
        "ProfileSelect_OnErase"=>{
            let idx=arg("iSelectedProfileIndex").and_then(|v|v.parse::<usize>().ok()).unwrap_or(99);
            let pr=&mut vm.fe.profiles;
            if idx<pr.slots.len(){pr.slots[idx]=None;if pr.loaded==idx as i32{pr.loaded=-1;}pr.save();}
            vm.fe.todo.push(("ProfileSelect_RefreshScreen".into(),vec![]));
            vec![]
        }
        "SelectKid_OnLoad"=>{vm.fe.kid_mode=1;vm.fe.kid_chosen.clear();vec![("iFilter".into(),vm.fe.profiles.filter.to_string())]}
        "SelectKid_OnFilterSelect"=>{
            // Only filters the 3D kids by gender; the kid itself is picked in the scene (see `kid_pick`).
            let f=arg("iFilter").and_then(|v|v.parse::<i32>().ok()).unwrap_or(0);
            vm.fe.profiles.filter=f;
            vec![]
        }
        "KidConfirm_GetDialogue"=>{
            let pr=&vm.fe.profiles;
            let name=if !pr.typed.is_empty()&&!vm.fe.mp.active{pr.typed.clone()}else{kid_name(vm,pr.kid.max(0) as usize)};let name=name.as_str();
            let t=vm.locale_string("T_KidConfirm").replace("%ls",name).replace("%s",name);
            vec![("strDialogue".into(),t)]
        }
        "TRCSetKeyboardInfo"=>{
            // The keyboard overlay closes itself; the typed name goes into the confirm dialog.
            vm.fe.keyboard_open=false;vm.fe.kid_click=None;
            let name=arg("strData").unwrap_or_default();
            vm.fe.profiles.typed=name.trim().to_uppercase();
            vm.fe.todo.push(("TRCKillKeyboard".into(),vec![]));
            vm.fe.todo.push(("OpenScreen".into(),vec![V::Str("ConfirmKid".into())]));
            vec![]
        }
        "TRCCancelKeyboard"=>{vm.fe.keyboard_open=false;vm.fe.kid_click=None;vm.fe.kid_chosen.clear();vm.fe.profiles.typed.clear();vm.fe.todo.push(("TRCKillKeyboard".into(),vec![]));vec![]}
        "KidConfirm_OnResponse"=>{
            let yes=arg("iResponse").as_deref()==Some("1");
            if vm.fe.mp.active{
                if yes{vm.fe.todo.push(("OpenScreen".into(),vec![V::Str("GameSelect".into())]));}
                else{vm.fe.todo.push(("CloseScreen".into(),vec![]));}
                return vec![];
            }
            let kid=vm.fe.profiles.kid.max(0) as usize;let kname=if vm.fe.profiles.typed.is_empty(){kid_name(vm,kid)}else{vm.fe.profiles.typed.clone()};
            let pr=&mut vm.fe.profiles;
            if yes{
                let slot=pr.selected.max(0) as usize;
                if slot<pr.slots.len(){pr.slots[slot]=Some(Slot{name:kname,grade:0,image:kid as i32});pr.loaded=slot as i32;pr.boot=false;pr.save();}
                vm.fe.todo.push(("OpenScreen".into(),vec![V::Str("MainMenu".into())]));
            }
            vec![]
        }
        "GetCursorVisibilities"=>vec![("aiVisibilities".into(),["1","0","0","0"].join(DELIM))],
        _=>{vm.warn(format!("unhandled game call {name}?{params}"));vec![]}
    }
}

pub fn _unused(_:&V){}

/// Minigames the original code runs for (`mg_session`); tetherball stays on the hand port with `EAGL_TB_HAND=1`.
pub fn vm_game(n: i32) -> bool {
    matches!(n, 0 | 1 | 3 | 4 | 5 | 6 | 8) || (n == 2 && std::env::var("EAGL_TB_HAND").is_err())
}
