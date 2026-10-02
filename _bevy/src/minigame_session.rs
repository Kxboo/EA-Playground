//! Shared Minigame session flow recovered from the original PowerPC executable.
//! Host effects are synchronous; tournament arithmetic uses the existing recovered port.
//! This state must be populated from the owning game, not kept as a second live copy.
use crate::multiplayer::{MultiplayerError, MultiplayerMode};
use serde::{Deserialize, Serialize};

/// ConversationManager's original static MGID table, in MinigameEnum order.
pub const MINIGAME_IDS: [[u8; 4]; 9] = [
    *b"DART", *b"RCCR", *b"TBLL", *b"DBLL", *b"FOOT", *b"PAIR", *b"WBLL", *b"HURD", *b"FRTH",
];

pub fn minigame_type(id: [u8; 4]) -> i32 {
    MINIGAME_IDS.iter().position(|candidate| *candidate == id).map_or(10, |i| i as i32)
}

/// Live PlaygroundWorld/AreaManager fields used by Minigame::SetArea and
/// RestoreArea. The regular world area is preserved while a minigame overrides
/// the physical geometry and abstract placeable selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaSelection {
    pub world_area:i32, // PlaygroundWorld +5c
    pub physical_area:i32, // AreaManager +2c
    pub placeable_area:i32, // AreaManager embedded PlaceableManager +1b0+18
}
impl AreaSelection {
    pub fn set_minigame_area(&mut self,area:i32) {
        self.physical_area=match area {4=>0,5=>1,6=>2,7|8=>3,_=>area};
        self.placeable_area=area;
    }
    pub fn restore(&mut self) {self.set_minigame_area(self.world_area);}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchRequest {
    pub id: [u8; 4],
    pub level: i32,
    pub difficulty: i32,
    pub dare: i32,
    /// Original Teams image: count +0, preserved padding +4, records +8.
    pub teams: [u8; 0x88],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchState {
    pub pending: LaunchRequest,
    pub fade_pending: bool,
    pub active: bool,
    pub kind: i32,
    pub playground: u32,
    pub current_world: u32,
    pub minigame: u32,
    /// Opaque host rule handle, corresponding to the original +144 pointer.
    pub rules: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameFactory {
    pub kind: i32,
    pub native_size: u32,
    pub constructor: u32,
    pub second_scene: bool,
}

impl GameFactory {
    pub fn for_kind(kind: i32) -> Option<Self> {
        let (native_size, constructor) = match kind {
            0 => (0x2c0, 0x80347da8), 1 => (0x310, 0x8037e02c),
            2 => (0x450, 0x80396410), 3 => (0x3d0, 0x80357434),
            4 => (0x2b8, 0x80368830), 5 => (0x2b0, 0x8036e16c),
            6 => (0x258, 0x8039f954), 8 => (0x480, 0x80334b40),
            _ => return None,
        };
        Some(Self { kind, native_size, constructor, second_scene: kind == 1 })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchEffect {
    FadeIn(u32, i32), UnloadAudio(i32), StopMusic, StopAmbience,
    SavePlayerConversation(i32), UnspawnArea(u32), CleanArea(u32),
    SetLevel(u32, i32), SetDifficulty(u32, i32), SetDare(u32, i32),
    SetRules(u32, u32), Initialize(u32, u32), FadeRenders(u32, bool, bool),
    AddSceneEntity(i32, u32),
}

/// Calls are synchronous and may alter the live manager before its next read.
/// Game constructors/derived virtual methods remain concrete host responsibilities.
pub trait LaunchServices {
    type Error;
    fn effect(&mut self, state: &mut LaunchState, effect: LaunchEffect);
    fn multiplayer_enabled(&mut self, state: &LaunchState) -> bool;
    /// Return a valid nonzero game handle or an error; the native null allocation
    /// path dereferences null and is outside the supported execution domain.
    fn construct(&mut self, state: &mut LaunchState, factory: GameFactory) -> Result<u32, Self::Error>;
    /// The authoritative base image for the live game, not a detached snapshot.
    fn base_image(&mut self, game: u32) -> &mut [u8; 0x100];
}

/// WorldMan's temporary time multiplier, separate from frame capping and pause.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTimeScale {
    pub enabled: bool, // +a4
    pub multiplier: f32, // +94
    pub elapsed_ms: i32, // +98
    pub consumed_ms: f32, // +9c
    pub duration_ms: i32, // +a0
}

impl WorldTimeScale {
    pub fn step(&mut self, milliseconds: i32) -> i32 {
        if !self.enabled { return milliseconds; }
        self.elapsed_ms = self.elapsed_ms.wrapping_add(milliseconds);
        let scaled = (self.elapsed_ms as f32 - self.consumed_ms) * self.multiplier;
        // fctiwz's finite representable domain. Invalid float/FPSCR behavior is
        // outside this host contract, rather than silently using Rust saturation.
        assert!(scaled.is_finite() && scaled >= -2147483648.0 && scaled < 2147483648.0);
        let result = scaled as i32;
        let consumed = if self.multiplier > 0.0 { result as f32 / self.multiplier } else { 0.0 };
        self.consumed_ms += consumed;
        if self.elapsed_ms >= self.duration_ms { self.enabled = false; }
        result
    }
}

pub trait WorldUpdateServices: ExitServices {
    fn update_world(&mut self, state: &mut LaunchState, world: u32, milliseconds: i32) -> i32;
    /// Default teardown composes the recovered body against these live services.
    fn end_minigame(&mut self, state: &mut LaunchState, aborted: bool) where Self:Sized {
        state.end(aborted,self);
    }
    fn fade_is_complete(&mut self, playground: u32) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitEffect {
    HomeMenu(bool), ResetHomeIcon, AddHomeSyncTask, RemoveHomeSyncTask,
    VSync(bool), WaitForDraws, RunSyncTasks,
    Uninitialize(u32), RemoveSceneEntity(i32,u32), Destroy(u32),
    FadeOut(u32,i32), FadeRenders(u32,bool,bool), ClearHandlers,
    Camera(i32,u32), GameState(i32), SpawnArea(u32), RefillArea(u32),
    PlayMusic(i32), StartAmbience, LoadAudio(i32),
    SetFacing(u32,u32), CameraImmediate(bool), ProfileResult(i32), LocalControl(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerPlacement {
    /// Original float bits, retained exactly when copying the saved position.
    pub position: [u32;3],
    pub pending: bool,
}

pub trait ExitServices: LaunchServices {
    fn exit_effect(&mut self, state:&mut LaunchState, effect:ExitEffect);
    /// Final lookup uses the original global WorldMan, while earlier lookups
    /// use this manager. A null final player skips the local-control callback.
    fn player_character(&mut self, global:bool, slot:i32)->u32;
    fn saved_conversation_position(&mut self, player:u32)->[u32;3];
    fn pending_player_placement(&mut self, player:u32)->&mut PlayerPlacement;
    fn saved_conversation_rotation(&mut self, player:u32)->u32;
}

/// Database and character services used by the GUID-based single-player launch.
/// Keys are the original 64-bit VLT identifiers; collection values are opaque.
pub trait SinglePlayerLaunchServices: LaunchServices {
    fn collection_named(&mut self,name:&str,key:u64)->u32;
    fn collection_keys(&mut self,class:u64,key:u64)->u32;
    fn key(&mut self,name:&str)->u64;
    fn int_array(&mut self,collection:u32,name:&str,index:u32)->i32;
    fn byte_array(&mut self,collection:u32,name:&str,index:u32)->i8;
    fn array_count(&mut self,collection:u32,key:u64)->i32;
    fn array_string(&mut self,collection:u32,name:&str,index:i32)->String;
    fn array_reference(&mut self,collection:u32,key:u64,index:i32)->u64;
    fn player_key(&mut self,global:bool,slot:i32)->u64;
    fn random_range(&mut self,low:i32,high:i32)->i32;
    fn destroy_collection(&mut self,collection:u32);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinglePlayerLaunchError { InvalidKind, TooManyTeammates, TooManyOpponents, InvalidRandomIndex }

/// The only live services needed in addition to the ordinary launch host when
/// using the decoded, immutable VLT database adapter below.
pub trait LaunchPlayerServices: LaunchServices {
    fn player_key(&mut self,global:bool,slot:i32)->u64;
    fn random_range(&mut self,low:i32,high:i32)->i32;
}

/// Validated database inputs for one original minigame definition. The adapter
/// resolves inheritance before array indexing, exactly like the native getters.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DatabaseLaunchDefinition {
    pub key:u64,
    pub kind:i32,
    pub level:i32,
    pub difficulty:i32,
    pub boss:i8,
    pub teammate_count:i32,
    pub character_names:[String;8],
    pub opponents:Vec<u64>,
}

impl DatabaseLaunchDefinition {
    pub fn read(db:&crate::vlt::Database,key:u64)->Result<Self,String> {
        use crate::vlt::{Collection,string_hash64};
        use serde_json::Value;
        let collection=|class:&str,key:u64| db.collections.iter().find(|c|c.class_key==string_hash64(class)&&c.key==key)
            .ok_or_else(||format!("missing {class} launch collection {key:#018x}"));
        fn inherited<'a>(db:&'a crate::vlt::Database,mut c:&'a Collection,name:&str)->Result<Value,String> {
            let key=crate::vlt::string_hash64(name);
            let mut seen=Vec::new();
            loop {
                if seen.contains(&c.key){return Err(format!("launch inheritance cycle at {:#018x}",c.key));}
                seen.push(c.key);
                if let Some(a)=c.attributes.iter().find(|a|a.name_key==key){return db.value(a);}
                if c.parent_key==0{return Ok(Value::Null);}
                c=db.collections.iter().find(|p|p.class_key==c.class_key&&p.key==c.parent_key)
                    .ok_or_else(||format!("missing launch parent {:#018x}",c.parent_key))?;
            }
        }
        fn elements(v:Value)->Vec<Value> {match v{Value::Null=>vec![],Value::Array(a)=>a,v=>vec![v]}}
        let definition=collection("minigames",key)?;
        let integer=|name:&str|->Result<i32,String>{
            let values=elements(inherited(db,definition,name)?);
            match values.first(){None=>Ok(0),Some(v)=>v.as_i64().map(|i|i as i32).or_else(||v.as_u64().map(|i|i as i32))
                .ok_or_else(||format!("{name} is not an integer"))}
        };
        let kind=integer("minigame_id")?;
        if !(0..9).contains(&kind){return Err(format!("invalid minigame kind {kind}"));}
        let level=integer("minigame_level")?;
        let difficulty=integer("minigame_difficulty")?;
        let boss=integer("ai_bosscharacterindex")? as i8;
        let teammate_count=elements(inherited(db,definition,"ai_teammates")?).len();
        if teammate_count>3{return Err(format!("too many teammates: {teammate_count}"));}
        let character=collection("character_select",string_hash64("character"))?;
        let roster=elements(inherited(db,character,"characterlist")?);
        let character_names:Vec<String>=(0..8).map(|i|roster.get(i).and_then(Value::as_str).map(str::to_owned)
            .ok_or_else(||format!("missing characterlist entry {i}"))).collect::<Result<_,_>>()?;
        let opponents=elements(inherited(db,definition,"ai_opponents")?).iter().map(|v|
            v.get("collection_key").and_then(Value::as_u64).ok_or_else(||"invalid ai_opponents reference".to_owned()))
            .collect::<Result<Vec<_>,_>>()?;
        if opponents.len()>4{return Err(format!("too many opponents: {}",opponents.len()));}
        Ok(Self{key,kind,level,difficulty,boss,teammate_count:teammate_count as i32,
            character_names:character_names.try_into().unwrap(),opponents})
    }
}

/// Concrete VLT bridge: validates input before any launch effects, then reuses
/// the recovered service-driven overload instead of implementing a second flow.
pub struct VltLaunchServices<'a,H> {
    pub host:&'a mut H,
    definition:DatabaseLaunchDefinition,
}
impl<'a,H:LaunchPlayerServices> VltLaunchServices<'a,H> {
    pub fn new(db:&crate::vlt::Database,key:u64,host:&'a mut H)->Result<Self,String> {
        Ok(Self{host,definition:DatabaseLaunchDefinition::read(db,key)?})
    }
}
impl<H:LaunchPlayerServices> LaunchServices for VltLaunchServices<'_,H> {
    type Error=H::Error;
    fn effect(&mut self,s:&mut LaunchState,e:LaunchEffect){self.host.effect(s,e)}
    fn multiplayer_enabled(&mut self,s:&LaunchState)->bool{self.host.multiplayer_enabled(s)}
    fn construct(&mut self,s:&mut LaunchState,f:GameFactory)->Result<u32,Self::Error>{self.host.construct(s,f)}
    fn base_image(&mut self,g:u32)->&mut [u8;0x100]{self.host.base_image(g)}
}
impl<H:LaunchPlayerServices> SinglePlayerLaunchServices for VltLaunchServices<'_,H> {
    fn collection_named(&mut self,name:&str,key:u64)->u32 {assert_eq!(name,"minigames");assert_eq!(key,self.definition.key);1}
    fn collection_keys(&mut self,class:u64,key:u64)->u32 {
        assert_eq!(class,crate::vlt::string_hash64("character_select"));assert_eq!(key,crate::vlt::string_hash64("character"));2
    }
    fn key(&mut self,name:&str)->u64{crate::vlt::string_hash64(name)}
    fn int_array(&mut self,c:u32,name:&str,index:u32)->i32 {
        assert_eq!(c,1);assert_eq!(index,0);
        match name{"minigame_id"=>self.definition.kind,"minigame_level"=>self.definition.level,
            "minigame_difficulty"=>self.definition.difficulty,_=>unreachable!("unexpected launch field")}
    }
    fn byte_array(&mut self,c:u32,name:&str,index:u32)->i8 {
        assert_eq!((c,name,index),(1,"ai_bosscharacterindex",0));self.definition.boss
    }
    fn array_count(&mut self,c:u32,key:u64)->i32 {
        assert_eq!(c,1);
        if key==crate::vlt::string_hash64("ai_teammates"){self.definition.teammate_count}
        else {assert_eq!(key,crate::vlt::string_hash64("ai_opponents"));self.definition.opponents.len() as i32}
    }
    fn array_string(&mut self,c:u32,name:&str,index:i32)->String {
        assert_eq!((c,name),(2,"characterlist"));self.definition.character_names[index as usize].clone()
    }
    fn array_reference(&mut self,c:u32,key:u64,index:i32)->u64 {
        assert_eq!(c,1);assert_eq!(key,crate::vlt::string_hash64("ai_opponents"));self.definition.opponents[index as usize]
    }
    fn player_key(&mut self,global:bool,slot:i32)->u64{self.host.player_key(global,slot)}
    fn random_range(&mut self,lo:i32,hi:i32)->i32{self.host.random_range(lo,hi)}
    fn destroy_collection(&mut self,c:u32){assert_eq!(c,1);}
}

impl LaunchState {
    /// GUID/database overload of StartMinigame. Builds the original player/AI
    /// Teams records and dispatches through the same fade request as multiplayer.
    pub fn request_singleplayer<H:SinglePlayerLaunchServices>(&mut self,key:u64,dare:i32,host:&mut H)->Result<(),SinglePlayerLaunchError> {
        let definition=host.collection_named("minigames",key);
        let kind=host.int_array(definition,"minigame_id",0);
        let id=*MINIGAME_IDS.get(kind as usize).ok_or(SinglePlayerLaunchError::InvalidKind)?;
        let level=host.int_array(definition,"minigame_level",0);
        let difficulty=host.int_array(definition,"minigame_difficulty",0);
        let mut teams=[0;0x88];
        teams[..4].copy_from_slice(&1i32.to_be_bytes());
        let boss=host.byte_array(definition,"ai_bosscharacterindex",0) as i32;
        let teammates_key=host.key("ai_teammates");
        let teammate_count=host.array_count(definition,teammates_key);
        let excluded=host.player_key(false,0);
        let class=host.key("character_select");
        let character_key=host.key("character");
        let characters=host.collection_keys(class,character_key);
        let mut candidates=Vec::with_capacity(8);
        for index in 0..8 {
            let name=host.array_string(characters,"characterlist",index);
            let key=host.key(&name);
            if key!=excluded {candidates.push(key);}
        }
        let player=host.player_key(true,0);
        let record=|teams:&mut [u8;0x88],offset:usize,key:u64,control:i32| {
            teams[offset..offset+8].copy_from_slice(&key.to_be_bytes());
            teams[offset+8..offset+12].copy_from_slice(&control.to_be_bytes());
        };
        record(&mut teams,8,player,1);
        if teammate_count>3 || teammate_count as i64>candidates.len() as i64 {return Err(SinglePlayerLaunchError::TooManyTeammates);}
        for index in 0..teammate_count {
            let selected=host.random_range(0,candidates.len() as i32-1);
            if selected<0 || selected as usize>=candidates.len() {return Err(SinglePlayerLaunchError::InvalidRandomIndex);}
            let key=candidates.swap_remove(selected as usize);
            record(&mut teams,0x18+index as usize*16,key,6);
        }
        let opponents_key=host.key("ai_opponents");
        let opponents=host.array_count(definition,opponents_key);
        if opponents>4 {return Err(SinglePlayerLaunchError::TooManyOpponents);}
        for index in 0..opponents {
            let key=host.array_reference(definition,opponents_key,index);
            let offset=0x48+index as usize*16;
            record(&mut teams,offset,key,6);
            if index==boss {teams[offset+12]=1;}
        }
        host.destroy_collection(definition);
        self.request(&LaunchRequest{id,level,difficulty,dare,teams},host);
        Ok(())
    }

    /// Original EndMinigame orchestration, including the single-player restore
    /// and multiplayer frontend branches. Render, resource, profile and task
    /// services execute synchronously against their live owners.
    pub fn end<H:ExitServices>(&mut self, aborted:bool, host:&mut H) {
        use ExitEffect::*;
        host.exit_effect(self,HomeMenu(false));
        host.exit_effect(self,ResetHomeIcon);
        host.exit_effect(self,AddHomeSyncTask);
        host.exit_effect(self,VSync(false));
        host.exit_effect(self,WaitForDraws);
        let mut result=i32::from_be_bytes(host.base_image(self.minigame)[0x60..0x64].try_into().unwrap());
        host.exit_effect(self,RunSyncTasks);
        host.exit_effect(self,Uninitialize(self.minigame));
        host.exit_effect(self,RemoveSceneEntity(1,self.minigame));
        if self.minigame!=0 {host.exit_effect(self,Destroy(self.minigame));}
        self.minigame=0;
        self.active=false;
        host.exit_effect(self,RunSyncTasks);
        self.current_world=self.playground;
        host.exit_effect(self,FadeOut(self.playground,-1));
        host.exit_effect(self,FadeRenders(self.playground,false,true));
        host.exit_effect(self,ClearHandlers);
        host.exit_effect(self,RunSyncTasks);
        if host.multiplayer_enabled(self) {
            host.exit_effect(self,Camera(2,0));
            host.exit_effect(self,GameState(4));
        } else {
            host.exit_effect(self,RunSyncTasks);
            let effect=if self.kind==8 {RefillArea(self.playground)} else {SpawnArea(self.playground)};
            host.exit_effect(self,effect);
            host.exit_effect(self,RunSyncTasks);
            host.exit_effect(self,PlayMusic(1));
            host.exit_effect(self,StartAmbience);
            host.exit_effect(self,LoadAudio(2));
            host.exit_effect(self,RunSyncTasks);
            let player=host.player_character(false,0);
            let position=host.saved_conversation_position(player);
            *host.pending_player_placement(player)=PlayerPlacement{position,pending:true};
            let facing_player=host.player_character(false,0);
            let rotation=host.saved_conversation_rotation(player);
            host.exit_effect(self,SetFacing(facing_player,rotation));
            host.exit_effect(self,RunSyncTasks);
            host.exit_effect(self,Camera(0,0));
            host.exit_effect(self,CameraImmediate(true));
            host.exit_effect(self,RunSyncTasks);
            if aborted {result=1;}
            host.exit_effect(self,RunSyncTasks);
            if self.kind!=8 {host.exit_effect(self,ProfileResult(result));}
            host.exit_effect(self,RunSyncTasks);
            let player=host.player_character(true,0);
            if player!=0 {host.exit_effect(self,LocalControl(player));}
            host.exit_effect(self,RunSyncTasks);
        }
        host.exit_effect(self,RunSyncTasks);
        host.exit_effect(self,VSync(true));
        host.exit_effect(self,RemoveHomeSyncTask);
        host.exit_effect(self,ResetHomeIcon);
        host.exit_effect(self,HomeMenu(true));
    }

    /// Original WorldMan::Update: update the current world first, consume its
    /// exit status, then complete a pending launch if the fade has finished.
    pub fn update<H: WorldUpdateServices>(&mut self, time: &mut WorldTimeScale, milliseconds: i32, host: &mut H) -> Result<(), H::Error> {
        let milliseconds = time.step(milliseconds);
        let result = host.update_world(self, self.current_world, milliseconds);
        if self.active && result == 2 { host.end_minigame(self, false); }
        if self.fade_pending && host.fade_is_complete(self.playground) { self.fade_complete(host)?; }
        Ok(())
    }

    /// StartMinigame(MGID, level, difficulty, Teams, dare). Rules remain unchanged.
    pub fn request(&mut self, request: &LaunchRequest, host: &mut impl LaunchServices) {
        self.fade_pending = true;
        self.pending.id = request.id;
        self.pending.level = request.level;
        self.pending.difficulty = request.difficulty;
        self.pending.teams[..4].copy_from_slice(&request.teams[..4]);
        self.pending.teams[8..].copy_from_slice(&request.teams[8..]);
        self.pending.dare = request.dare;
        host.effect(self, LaunchEffect::FadeIn(self.playground, -1));
        host.effect(self, LaunchEffect::UnloadAudio(2));
        host.effect(self, LaunchEffect::StopMusic);
        host.effect(self, LaunchEffect::StopAmbience);
    }

    /// The rules-pointer overload always forwards dare=-1.
    pub fn request_with_rules(&mut self, request: &LaunchRequest, rules: u32, host: &mut impl LaunchServices) {
        self.rules = rules;
        let mut request = request.clone();
        request.dare = -1;
        self.request(&request, host);
    }

    /// Complete original WorldMan fade callback and factory dispatch. Pending
    /// launch parameters are snapshotted before any engine callbacks run.
    pub fn fade_complete<H: LaunchServices>(&mut self, host: &mut H) -> Result<(), H::Error> {
        let request = self.pending.clone();
        self.fade_pending = false;
        self.kind = minigame_type(request.id);
        self.active = true;
        if !host.multiplayer_enabled(self) {
            host.effect(self, LaunchEffect::SavePlayerConversation(0));
            let area = if self.kind == 8 { LaunchEffect::CleanArea(self.playground) }
                else { LaunchEffect::UnspawnArea(self.playground) };
            host.effect(self, area);
        }
        if let Some(factory) = GameFactory::for_kind(self.kind) {
            let game = host.construct(self, factory)?;
            assert_ne!(game, 0, "constructor must return a valid game handle");
            self.minigame = game;
            self.current_world = game;
            host.effect(self, LaunchEffect::SetLevel(self.minigame, request.level));
            host.effect(self, LaunchEffect::SetDifficulty(self.minigame, request.difficulty));
            host.effect(self, LaunchEffect::SetDare(self.minigame, request.dare));
            set_up_teams_image(host.base_image(self.minigame), &request.teams);
            host.effect(self, LaunchEffect::SetRules(self.minigame, self.rules));
            host.base_image(self.minigame)[0x38..0x3c].copy_from_slice(&request.id);
            host.effect(self, LaunchEffect::Initialize(self.minigame, self.playground));
        } else {
            self.minigame = 0;
            self.active = false;
            // Native default branch deliberately preserves current_world.
        }
        host.effect(self, LaunchEffect::FadeRenders(self.playground, true, false));
        host.effect(self, LaunchEffect::AddSceneEntity(1, self.minigame));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub camera_type: i32, // +2c
    pub control_type: i32, // +30
    pub game_state: i32, // +34
    pub dare: i32, // +48
    pub initialized: bool, // +4d
    pub pause_menu_open: bool, // +4e
    pub pregame_ready: bool, // +58
    pub postgame_choice: i32, // +5c
    pub team_count: i32, // +70, distinct from human count +40
    pub pause_block_ms: i32, // +fc
    pub world_paused: bool, // World +24
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    CameraViewInfo(bool), CameraReinitialize(i32, u32), ControllerReinitialize(usize, i32),
    FrontendFlags(bool, bool), SetupPreGame(i32, [i32; 4]), ClearPreGame,
    OpenScreen(&'static str), CloseScreen, OpenOverlay(&'static str), CloseOverlay,
    FadeRenders(bool, bool), FadeOut(i32), FadeIn(i32),
    PlaySfx(i32, i32, i32), AudioPause(i32), AudioUnpause,
    ClearPostGame, SetupPostGame(i32, Box<PostGameInfo>),
}

pub trait Services {
    fn effect(&mut self, effect: Effect);
    /// World::Update is a separate host boundary; preserve its return value.
    fn world_update(&mut self, milliseconds: i32) -> u32;
}

/// Full original payload, including bytes that this routine deliberately leaves alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostGameInfo(pub [u8; 0x118]);

/// Values consumed by the original postgame CmdComposer. The UI adapter owns
/// composing/releasing its buffer; these fields retain the native order/names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostGameField {
    Int(&'static str,i32),
    Array(&'static str,Vec<i32>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostGameDataError { InvalidKind, InvalidCharacter, InvalidStatCount, InvalidRank, Tournament(MultiplayerError) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostGameTournamentQuery { PointsWon, TotalPoints, GamesWon, TotalGamesWon }
pub trait PostGameProfileServices {
    fn high_score(&mut self,kind:i32)->i32;
    /// Opaque handle to the current profile's original wide-string name.
    fn profile_name(&mut self)->u32;
    fn set_high_score(&mut self,kind:i32,score:i32,name:u32);
    fn multiplayer_enabled(&mut self)->bool;
    fn trigger_save_popup(&mut self);
}
impl PostGameInfo {
    pub fn word(&self, offset: usize) -> i32 {
        i32::from_be_bytes(self.0[offset..offset + 4].try_into().unwrap())
    }
    pub fn set_word(&mut self, offset: usize, value: i32) {
        self.0[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    pub fn placements(&self) -> [i32; 4] { std::array::from_fn(|i| self.word(0x40 + i * 4)) }
    pub fn on_load_fields(&self,kind:i32)->Vec<PostGameField> {
        vec![PostGameField::Int("iMiniGameType",kind),PostGameField::Int("iGameMode",i32::from(self.word(0)>1))]
    }
    pub fn stats_fields(&self,character:usize)->Result<Vec<PostGameField>,PostGameDataError> {
        if character>=4{return Err(PostGameDataError::InvalidCharacter);}
        let count=self.word(0x54);
        if !(0..=10).contains(&count){return Err(PostGameDataError::InvalidStatCount);}
        Ok(vec![PostGameField::Array("aiStats",(0..count as usize).map(|i|self.word(0x58+character*0x28+i*4)).collect())])
    }
    pub fn multiplayer_fields(&self,kind:i32,tournament:&MultiplayerMode)->Result<Vec<PostGameField>,PostGameDataError> {
        use PostGameField::{Int,Array};
        let count=self.word(4);
        if !(0..=4).contains(&count){return Err(PostGameDataError::InvalidCharacter);}
        let mut order=[-1;4];
        for character in 0..count as usize {
            let rank=self.word(0x40+character*4);
            if !(0..4).contains(&rank){return Err(PostGameDataError::InvalidRank);}
            order[rank as usize]=character as i32;
        }
        let match_order=order[..count as usize].to_vec();
        // Retail reuses the match array, rather than clearing it. Preserve that
        // detail even when duplicate match ranks left unused entries behind.
        for character in 0..count as usize {
            let rank=tournament.player_rank(character).map_err(PostGameDataError::Tournament)?;
            if !(0..4).contains(&rank){return Err(PostGameDataError::InvalidRank);}
            order[rank as usize]=character as i32;
        }
        Ok(vec![Int("iMultiplayerGameType",i32::from(tournament.is_point_series())),
            Int("iNumPlayers",self.word(0)),Int("iNumCharactersInGame",count),
            Int("iIsTeamGame",i32::from(matches!(kind,3|4))),Int("iWinningTeam",self.word(0x38)),
            Array("aiRank",match_order),Array("aiTotalRank",order[..count as usize].to_vec())])
    }
    pub fn team_fields(&self)->Vec<PostGameField> {
        [("aiNumPlayers",8),("aiTeam1",0x18),("aiTeam2",0x28)].into_iter()
            .map(|(name,offset)|PostGameField::Array(name,(0..4).map(|i|self.word(offset+i*4)).collect())).collect()
    }
    pub fn tournament_fields(&self,query:PostGameTournamentQuery,player:i32,tournament:&MultiplayerMode)->Result<Vec<PostGameField>,PostGameDataError> {
        use PostGameTournamentQuery::*;
        if player<0{return Err(PostGameDataError::InvalidCharacter);}
        let name=match query {PointsWon=>"iPointsWon",TotalPoints=>"iTotalPointsWon",GamesWon=>"iGamesWon",TotalGamesWon=>"iTotalGamesWon"};
        let value=if player>=self.word(0){0}else{
            let p=player as usize;
            match query {
                PointsWon=>tournament.points_in_this_match(p),TotalPoints=>tournament.point_total(p),
                GamesWon=>tournament.won_last_game(p).map(i32::from),TotalGamesWon=>tournament.win_total(p),
            }.map_err(PostGameDataError::Tournament)?
        };
        Ok(vec![PostGameField::Int(name,value)])
    }
    pub fn last_tournament_fields(tournament:&MultiplayerMode)->Vec<PostGameField> {
        vec![PostGameField::Int("iIsLastTourneyGame",i32::from(tournament.rounds_left()<=0)),
            PostGameField::Int("iWinningPlayerId",tournament.player_by_rank(0).map_or(-1,|p|p as i32))]
    }
    /// Original SPInfo also updates the profile on a tied high score. Actual
    /// profile storage and save-popup dispatch remain synchronous host services.
    pub fn singleplayer_fields(&self,kind:i32,profile:&mut impl PostGameProfileServices)->Result<Vec<PostGameField>,PostGameDataError> {
        use PostGameField::{Int,Array};
        if !(0..9).contains(&kind){return Err(PostGameDataError::InvalidKind);}
        let count=self.word(4);
        if !(0..=4).contains(&count){return Err(PostGameDataError::InvalidCharacter);}
        let mut order=[-1;4];
        for character in 0..count as usize {
            let rank=self.word(0x40+character*4);
            if !(0..4).contains(&rank){return Err(PostGameDataError::InvalidRank);}
            order[rank as usize]=character as i32;
        }
        let mut high=profile.high_score(kind);
        let score=self.word(0xf8);
        if score>=high {
            let name=profile.profile_name();
            profile.set_high_score(kind,score,name);
            high=score;
            if profile.multiplayer_enabled(){profile.trigger_save_popup();}
        }
        let dare=self.word(0x114);
        let score_na=match kind {
            5=>score==-1,
            0=>dare != -1 && !(9..12).contains(&dare),
            _=>dare != -1,
        };
        Ok(vec![Int("iScore",score),Int("iHighScore",high),Int("iUserWon",i32::from(self.word(0x3c)==0)),
            Int("iNumPlayers",self.word(0)),Int("iNumCharactersInGame",count),Int("iAllowReplay",self.0[0x50] as i32),
            Array("aiRank",order[..count as usize].to_vec()),Int("iIsScoreNA",i32::from(score_na))])
    }
    /// Original DoJobLV IDs 0..10. Unknown IDs return no fields or effects.
    /// The input composer reads iPlayerId only for jobs 2, 6, 7, 9 and 10.
    pub fn query_fields(&self,job:i32,kind:i32,player:i32,tournament:&MultiplayerMode,profile:&mut impl PostGameProfileServices)->Result<Vec<PostGameField>,PostGameDataError> {
        use PostGameTournamentQuery::*;
        match job {
            0=>Ok(self.on_load_fields(kind)),1=>self.singleplayer_fields(kind,profile),
            2=>self.stats_fields(player as usize),3=>self.multiplayer_fields(kind,tournament),
            4=>Ok(Self::last_tournament_fields(tournament)),
            5=>Ok(vec![PostGameField::Int("iIsNextGameLastTourneyGame",i32::from(tournament.rounds_left()==1))]),
            6=>self.tournament_fields(PointsWon,player,tournament),7=>self.tournament_fields(TotalPoints,player,tournament),
            8=>Ok(self.team_fields()),9=>self.tournament_fields(GamesWon,player,tournament),
            10=>self.tournament_fields(TotalGamesWon,player,tournament),_=>Ok(vec![]),
        }
    }
}

/// Fields from the original MultiplayerMode singleton that are not tournament counters.
pub struct MultiplayerContext {
    pub enabled: bool, // singleton +0
    pub players: usize, // singleton +8
    /// Winning-team character control types, four records at team*64 + 0x18, stride 16.
    /// Values 2..=5 designate controller/player 0..=3. Others do not receive a win.
    pub team_controls: [[i32; 4]; 2],
}

/// Decoded big-endian base-object image for authentic startup composition.
/// Stored native addresses are evidence/opaque handles, never Rust pointers.
/// Preserve the prior allocation: the original does NOT initialize every field.
pub fn construct_base_image(image: &mut [u8; 0x100], scene: u32, default_id: [u8; 4]) {
    fn store(image: &mut [u8], offset: usize, value: u32) {
        image[offset..offset+4].copy_from_slice(&value.to_be_bytes());
    }
    // Complete World constructor followed by complete Minigame constructor.
    for offset in (4..=0x20).step_by(4) { store(image, offset, 0); }
    image[0x24] = 0;
    store(image, 0, 0x804d_ddc0);
    store(image, 0x28, scene);
    for offset in [0x2c,0x30,0x40,0x44,0x50,0x54,0x70,0xf8,0xfc] { store(image,offset,0); }
    // The last three bytes are sign-extended before shifted addition, not ORed.
    let id = ((default_id[0] as u32) << 24)
        .wrapping_add((default_id[1] as i8 as u32) << 16)
        .wrapping_add((default_id[2] as i8 as u32) << 8)
        .wrapping_add(default_id[3] as i8 as u32);
    store(image,0x38,id);
    for offset in [0x48,0x5c] { store(image,offset,u32::MAX); }
    for offset in [0x4c,0x4d,0x4e,0x58] { image[offset]=0; }
    for offset in [0x60,0x64,0x68] { store(image,offset,2); }
    image[0x78..0xf8].fill(0);
}

/// SetUpTeams copies the count and exactly 128 bytes starting at source +8.
/// Both structures' +4 padding is preserved, rather than accidentally copied.
pub fn set_up_teams_image(image: &mut [u8; 0x100], teams: &[u8; 0x88]) {
    image[0x70..0x74].copy_from_slice(&teams[0..4]);
    image[0x78..0xf8].copy_from_slice(&teams[8..0x88]);
}

/// World-taking Initialize copies only these six manager handles before virtual
/// Initialize(). +1c (world effects) and +24 (paused) are intentionally preserved.
pub fn inherit_world_image(image: &mut [u8; 0x100], world: &[u8; 0x28]) {
    for offset in [8,0xc,0x10,0x14,0x18,0x20] {
        image[offset..offset+4].copy_from_slice(&world[offset..offset+4]);
    }
}

/// Shared prefix of Minigame::Update, for hosts that already own the base fields.
/// The caller must immediately execute World::Update and propagate its result.
pub fn advance_pause_delay(pause_block_ms: &mut i32, ms: i32, current_world_paused: bool) {
    if !current_world_paused && *pause_block_ms > 0 {
        *pause_block_ms = pause_block_ms.wrapping_sub(ms);
    }
}

impl Session {
    pub fn initialize(&mut self, s: &mut impl Services) {
        s.effect(Effect::CameraViewInfo(false));
        s.effect(Effect::CameraReinitialize(self.camera_type, 0));
        for i in 0..4 { s.effect(Effect::ControllerReinitialize(i, self.control_type)); }
        self.initialized = true;
        self.pause_menu_open = false;
    }
    pub fn uninitialize(&mut self, s: &mut impl Services) {
        for i in 0..4 { s.effect(Effect::ControllerReinitialize(i, 0)); }
        self.pause_menu_open = false;
        self.initialized = false;
    }
    pub fn update(&mut self, ms: i32, current_world_paused: bool, s: &mut impl Services) -> u32 {
        // Reads the global current World pause flag, not necessarily this object's flag.
        advance_pause_delay(&mut self.pause_block_ms, ms, current_world_paused);
        s.world_update(ms)
    }
    fn pregame_info(&self, pause: bool, controller: i32) -> [i32; 4] {
        [i32::from(pause), controller, i32::from(self.team_count > 1), self.dare]
    }
    pub fn open_pregame(&mut self, kind: i32, s: &mut impl Services) {
        s.effect(Effect::FrontendFlags(true, true));
        s.effect(Effect::SetupPreGame(kind, self.pregame_info(false, 0)));
        s.effect(Effect::OpenScreen("PreGameInstructions"));
        s.effect(Effect::FadeRenders(true, false));
    }
    pub fn close_pregame(&mut self, close_screen: bool, s: &mut impl Services) {
        s.effect(Effect::ClearPreGame);
        if close_screen { s.effect(Effect::CloseScreen); }
        s.effect(Effect::FadeOut(400));
        s.effect(Effect::FadeRenders(false, true));
        self.pregame_ready = false;
    }
    pub fn open_pause(&mut self, kind: i32, controller: i32, s: &mut impl Services) {
        if self.pause_menu_open || self.pause_block_ms > 0 { return; }
        self.pause_menu_open = true;
        self.world_paused = true;
        s.effect(Effect::SetupPreGame(kind, self.pregame_info(true, controller)));
        s.effect(Effect::FrontendFlags(true, true));
        s.effect(Effect::OpenOverlay("PreGameInstructions"));
        s.effect(Effect::PlaySfx(12, 0, 100));
        s.effect(Effect::AudioPause(2));
    }
    pub fn close_pause(&mut self, s: &mut impl Services) {
        self.world_paused = false;
        s.effect(Effect::CloseOverlay);
        s.effect(Effect::AudioUnpause);
        self.pause_menu_open = false;
        self.pause_block_ms = 1200;
    }
    pub fn on_pause_quit(&mut self, s: &mut impl Services) {
        self.close_pause(s);
        s.effect(Effect::FadeIn(-1));
        self.game_state = 9;
    }
    // Base OnPauseContinue and OnPauseReset both call close_pause; derived overrides differ.
    pub fn on_play(&mut self) { self.pregame_ready = true; }
    pub fn on_replay(&mut self) { self.postgame_choice = 0; }
    pub fn on_done(&mut self) { self.postgame_choice = 1; }
    /// Original PostGameFSHandlers::DoJobFS with both nested selectors and
    /// concrete OnReplay/OnDone. `multiplayer_flag_1` is the live singleton +1
    /// byte; preserve it on all paths that do not explicitly set it.
    /// Job 13 reads iSelected; job 14 reads iButton; other jobs need no argument.
    pub fn handle_postgame_command(&mut self,job:i32,selection:i32,multiplayer_flag_1:&mut bool,point_series:bool) {
        match job {
            11=>self.on_replay(),
            12=>self.on_done(),
            13=>match selection {
                0=>self.on_done(),
                1=>{*multiplayer_flag_1=true;self.on_done();},
                2=>self.on_replay(),
                _=>{},
            },
            14=>match selection {
                0=>{*multiplayer_flag_1=true;self.on_done();},
                1=>self.on_replay(),
                2=>{if point_series{*multiplayer_flag_1=true;}self.on_done();},
                3=>self.on_done(),
                _=>{},
            },
            _=>{},
        }
    }
    pub fn close_postgame(&mut self, close_screen: bool, s: &mut impl Services) {
        s.effect(Effect::ClearPostGame);
        if close_screen { s.effect(Effect::CloseScreen); }
        self.postgame_choice = -1;
    }

    /// Complete base OpenPostGameScreen. Checks unsafe original indices before mutations.
    /// Unknown kind IDs retain original behavior: open results without adding a tournament result.
    pub fn open_postgame(
        &mut self, kind: i32, info: &mut PostGameInfo, context: &MultiplayerContext,
        tournament: &mut MultiplayerMode, s: &mut impl Services,
    ) -> Result<(), MultiplayerError> {
        // Work on a candidate to keep rejected unsafe indices atomic.
        let mut next = tournament.clone();
        let mut payload = info.clone();
        if context.enabled { payload.0[0x50] = 1; payload.set_word(0x114, -1); }
        else { payload.set_word(0x114, self.dare); }
        if self.team_count > 1 {
            let winner = info.word(0x3c);
            match kind {
                2 | 6 => {
                    if !(-1..=1).contains(&winner) { return Err(MultiplayerError::InvalidPlayer); }
                    if next.is_point_series() {
                        next.add_round_results([if winner == 0 {50} else {0}, if winner == 1 {50} else {0}, -1, -1]);
                    }
                    next.set_last_placement(if winner == 0 {[0, 1, -1, -1]} else {[1, 0, -1, -1]});
                    next.add_win_results([if winner == -1 {None} else {Some(winner as usize)}, None])?;
                }
                3 | 4 => {
                    if !(0..=1).contains(&winner) { return Err(MultiplayerError::InvalidPlayer); }
                    let mut scores = [0i32; 4];
                    let mut winners = Vec::new();
                    for code in context.team_controls[winner as usize] {
                        if (2..=5).contains(&code) {
                            let p = (code - 2) as usize;
                            winners.push(p);
                            scores[p] += 50;
                        }
                    }
                    next.set_last_placement(info.placements());
                    next.add_win_results([winners.first().copied(), winners.get(1).copied()])?;
                    if next.is_point_series() { next.add_round_results(scores); }
                }
                0 | 1 | 5 => {
                    if !(2..=4).contains(&context.players)
                        || tournament.point_total(context.players - 1).is_err()
                        || tournament.point_total(context.players).is_ok() {
                        return Err(MultiplayerError::InvalidPlayerCount);
                    }
                    let mut scores = [0; 4];
                    let mut winner = None;
                    for (p, rank) in info.placements().into_iter().enumerate().take(context.players) {
                        scores[p] = match rank { 0 => {winner = Some(p); 50}, 1 => 25, 2 => 10, _ => 0 };
                    }
                    next.set_last_placement(info.placements());
                    next.add_win_results([winner, None])?;
                    if next.is_point_series() { next.add_round_results(scores); }
                }
                _ => {}
            }
        }
        s.effect(Effect::FrontendFlags(true, true));
        *info = payload;
        *tournament = next;
        s.effect(Effect::SetupPostGame(kind, Box::new(info.clone())));
        s.effect(Effect::OpenScreen("PostGame"));
        Ok(())
    }
}

/// Original stable score ordering; ranks are unique even when scores are tied.
pub fn placement_ffa(scores: &[i32], higher_wins: bool, out: &mut [i32; 4]) -> Result<(), MultiplayerError> {
    if scores.len() > 4 { return Err(MultiplayerError::InvalidPlayerCount); }
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| if higher_wins {scores[b].cmp(&scores[a])} else {scores[a].cmp(&scores[b])});
    for (rank, p) in order.into_iter().enumerate() { out[p] = rank as i32; }
    Ok(())
}

/// Winning team members precede losing members, retaining each team's input order.
pub fn placement_teams(winner: usize, teams: [&[usize]; 2], out: &mut [i32; 4]) -> Result<(), MultiplayerError> {
    if winner > 1 || teams.iter().any(|t| t.len() > 4 || t.iter().any(|&p| p > 3)) {
        return Err(MultiplayerError::InvalidPlayer);
    }
    for (rank, &player) in teams[winner].iter().chain(teams[1-winner]).enumerate() { out[player] = rank as i32; }
    Ok(())
}

pub fn placement_1vs1(winner: usize, out: &mut [i32; 4]) -> Result<(), MultiplayerError> {
    if winner > 1 { return Err(MultiplayerError::InvalidPlayer); }
    out[winner] = 0; out[1-winner] = 1; Ok(())
}
