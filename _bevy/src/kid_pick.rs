//! The selectable-kid scene of the front end (`SelectableCharacter`, FEManager states 5 and 16): the kids stand in the
//! playground behind the select-kid / player-setup screens and are picked with the pointer.
//!
//! Evidence: roster, genders, names and unlock links come from `db.vlt` (`character_select/character` + `bestiary/g_*`), the
//! camera poses from `character_select/{single_player,multi_player}`.  Select-kid stand positions, the confirm position and the
//! yaw are constants the executable builds in `FEManager::__sinit` / `__sinit_selectablecharacter` (read from the ELF);
//! multiplayer positions are the vault's `positions` array (slot 0 has no depth in the data and falls back to its select-kid spot).
//! Animations: idle, the run clip while walking at the character speed, `S_PickMe_01` on hover and each kid's own
//! `S_<Name>_Celebration` at the confirm position.  Which clip the original plays on hover, and the filter semantics of the
//! Boys / Girls buttons, are not recovered (the other gender simply stays unpickable).
use bevy::{animation::{AnimationPlayer,graph::{AnimationGraph,AnimationGraphHandle}},mesh::skinning::SkinnedMeshInverseBindposes,prelude::*};
use std::sync::{mpsc,Mutex};
use crate::{apt_view::AptViewNs,assets,bridge,character,model,vlt,skeleton::Skeleton,anim::Clip,menu::AppMode};

#[derive(Clone,Debug)]
pub struct KidInfo{pub asset:String,pub name_key:String,pub boy:bool,pub unlocked:bool,/// Multiplayer stand position and yaw from `character_select/multi_player/positions`: (x, y, z, rotation).
    pub mp:Option<[f32;4]>}

struct Loaded{roster:Vec<KidInfo>,models:Vec<Option<assets::BuiltModel>>,skeleton:Skeleton,clips:Vec<Clip>,/// Bank indices of the extra clips (pick-me, then one celebration per distinct index) and, per roster kid, its celebration's position in `clips`.
    celebrate:Vec<usize>}

#[derive(Component)] pub struct KidMarker;
#[derive(Component)] struct Kid{index:usize,sp:Vec3,mp:Vec3,rot_sp:f32,rot_mp:f32,at:Vec3,yaw:f32,model:Entity,nodes:[AnimationNodeIndex;4],anim:u8}
#[derive(Component)] struct Ring{player:Option<usize>}

enum Load{Idle,Loading(Mutex<mpsc::Receiver<Result<Loaded,String>>>),Ready,Failed}

#[derive(Resource)]
pub struct KidStage{load:Load,loaded:Option<Loaded>,spawned:bool,clips:Vec<Handle<AnimationClip>>,ring:Option<(Handle<Mesh>,Vec<Handle<StandardMaterial>>)>,pub log:Vec<String>}
impl Default for KidStage{fn default()->Self{Self{load:Load::Idle,loaded:None,spawned:false,clips:vec![],ring:None,log:vec![]}}}

/// Select-kid stand positions, `FEManager::__sinit` (0x803279d8) array at 0x805e2570.
const SP_POSITIONS:[[f32;3];8]=[[12.2299,0.91,-59.5899],[13.27,0.91,-59.5899],[14.26,0.91,-59.5899],[12.79,0.31,-58.93],[13.7,0.31,-58.93],[12.31,0.15,-58.4],[13.31,0.15,-58.4],[14.26,0.15,-58.4]];
/// Where a picked kid walks to for the confirm dialog (`IsAtConfirmAKidPosition` compares with 0x805e28a0, set by `__sinit_selectablecharacter`).
const CONFIRM_POSITION:[f32;3]=[13.4,0.51,-58.8];
const PLAYER_COLOURS:[Color;5]=[Color::srgb(0.95,0.2,0.2),Color::srgb(0.25,0.45,0.95),Color::srgb(0.2,0.8,0.3),Color::srgb(0.98,0.85,0.2),Color::srgb(1.,1.,1.)];

pub fn plugin(app:&mut App){
    app.init_resource::<KidStage>().add_systems(Update,kid_stage.after(crate::apt_view::view_step).run_if(|m:Res<AppMode>|*m==AppMode::Apt));
}

fn data_dir(rel:&str)->String{bridge::data_root().join("files").join("data").join(rel.replace('/',"\\")).to_string_lossy().into_owned()}

fn load_all()->Result<Loaded,String>{
    let dir=bridge::data_root().join("files").join("data").join("db");
    let (v,b)=(std::fs::read(dir.join("db.vlt")).map_err(|e|format!("db.vlt: {e}"))?,std::fs::read(dir.join("db.bin")).map_err(|e|format!("db.bin: {e}"))?);
    let db=vlt::Database::load(&v,&b,vlt::known_names())?;
    let text=|c:&vlt::Collection,n:&str|db.attribute(c,n).and_then(|v|v.as_str().map(String::from));
    let list=db.find_collection("character_select","character").ok_or("character_select/character missing")?;
    let chars:Vec<String>=db.attribute(list,"characterlist").and_then(|v|v.as_array().map(|a|a.iter().filter_map(|s|s.as_str().map(String::from)).collect())).ok_or("characterlist")?;
    let unlock:Vec<String>=db.attribute(list,"unlockLinks").and_then(|v|v.as_array().map(|a|a.iter().filter_map(|s|s.as_str().map(String::from)).collect())).unwrap_or_default();
    // Attributes inherit through `parent`.
    let inherited=|c:&vlt::Collection,n:&str|->Option<serde_json::Value>{
        let mut cur=Some(c);
        while let Some(col)=cur{
            if let Some(v)=db.attribute(col,n){return Some(v)}
            cur=db.collections.iter().find(|x|x.class_key==col.class_key&&x.key==col.parent_key&&col.parent_key!=0);
        }
        None
    };
    // `GetVector4FromArray` hands the game (x, y, z, yaw); the vault entries decode here as (z, yaw, x, y).
    let mp_positions:Vec<[f32;4]>=db.find_collection("character_select","multi_player").and_then(|c|db.attribute(c,"positions"))
        .and_then(|v|v.as_array().map(|a|a.iter().filter_map(|e|{let e=e.as_array()?;let f=|i:usize|e.get(i).and_then(|x|x.as_f64()).unwrap_or(0.) as f32;Some([f(2),f(3),f(0),f(1)])}).collect())).unwrap_or_default();
    let mut roster=vec![];
    for (i,key) in chars.iter().enumerate(){
        let c=db.find_collection("bestiary",key).ok_or_else(||format!("bestiary/{key} missing"))?;
        let asset=inherited(c,"asset_name").and_then(|v|v.as_str().map(String::from)).ok_or("asset_name")?;
        let name_key=inherited(c,"name").and_then(|v|v.as_str().map(String::from)).unwrap_or_default();
        let boy=inherited(c,"gender").and_then(|v|v.as_i64()).unwrap_or(0)==0;
        let unlocked=unlock.get(i).is_none_or(|u|u=="0");
        roster.push(KidInfo{asset,name_key,boy,unlocked,mp:mp_positions.get(i).copied()});
    }
    let _=text;
    let schemas=model::Schemas::embedded();
    let viv=data_dir("characters");
    let rig=character::load(&viv,&schemas)?;
    let mut models=vec![];
    for k in &roster{
        models.push(if k.unlocked{ if k.asset=="alicia"{None}else{character::load_kid_model(&viv,&k.asset,&schemas).ok()} }else{None});
    }
    // Alicia comes with the rig loader.
    let alicia=roster.iter().position(|k|k.asset=="alicia");
    // Run, pick-me and each kid's own celebration clip (`S_<Name>_Celebration`; the other kids use the generic one).
    let celeb_bank=|asset:&str|->usize{match asset{"alicia"=>5,"jazz"=>80,"josunfu"=>81,"kalia"=>82,"ken"=>83,"nerdy"=>89,"skater"=>164,"timothy"=>208,_=>71}};
    let mut extra:Vec<usize>=vec![100];
    let mut celebrate=vec![];
    for k in &roster{let b=celeb_bank(&k.asset);let pos=extra.iter().position(|&x|x==b).unwrap_or_else(||{extra.push(b);extra.len()-1});celebrate.push(3+pos);}
    let mut clips=rig.clips;
    clips.extend(character::load_clips(&viv,&rig.skeleton,&extra)?);
    let mut out=Loaded{roster,models,skeleton:rig.skeleton,clips,celebrate};
    if let Some(i)=alicia{out.models[i]=Some(rig.model);}
    Ok(out)
}

fn wanted(screen:&str)->u8{match screen{"SelectKid"=>1,"PlayerSetup"=>2,"ConfirmKid"=>3,_=>0}}

#[allow(clippy::too_many_arguments)]
fn kid_stage(mut commands:Commands,mut stage:ResMut<KidStage>,v:Option<NonSendMut<AptViewNs>>,game:Option<Res<crate::game::Game>>,
    mut meshes:ResMut<Assets<Mesh>>,mut materials:ResMut<Assets<StandardMaterial>>,mut images:ResMut<Assets<Image>>,mut ibp:ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut clips:ResMut<Assets<AnimationClip>>,mut graphs:ResMut<Assets<AnimationGraph>>,
    cams:Query<(&Camera,&GlobalTransform),With<crate::game::GameCamera>>,
    mut kids:Query<(Entity,&mut Kid,&mut Transform,&mut Visibility)>,mut rings:Query<(Entity,&Ring,&mut Transform,&mut Visibility),Without<Kid>>,
    existing:Query<Entity,With<KidMarker>>,mut players:Query<&mut AnimationPlayer>,time:Res<Time<Real>>){
    let Some(mut v)=v else{return};let v=&mut v.0;
    let screen=v.vm.fe.screen.clone();
    let want=wanted(&screen);
    // Leaving the scene: remove the kids.
    if want==0{
        if stage.spawned{for e in &existing{commands.entity(e).despawn();}stage.spawned=false;v.vm.fe.kid_hover=None;}
        v.vm.fe.kid_click=None;
        return
    }
    let Some(g)=game else{return};
    // Load the roster on first use.
    match &stage.load{
        Load::Idle=>{
            let (tx,rx)=mpsc::channel();
            std::thread::spawn(move||{let _=tx.send(load_all());});
            stage.load=Load::Loading(Mutex::new(rx));return
        }
        Load::Loading(rx)=>{
            let r=rx.lock().ok().and_then(|r|r.try_recv().ok());
            match r{
                Some(Ok(l))=>{
                    v.vm.fe.roster=l.roster.clone();
                    stage.log.push(format!("kid roster: {} kids, {} models",l.roster.len(),l.models.iter().flatten().count()));
                    stage.loaded=Some(l);stage.load=Load::Ready;
                }
                Some(Err(e))=>{stage.log.push(format!("kid roster FAILED: {e}"));eprintln!("kid roster: {e}");stage.load=Load::Failed;}
                None=>{}
            }
            return
        }
        Load::Failed=>return,
        Load::Ready=>{}
    }
    // Spawn once.
    if !stage.spawned{
        let Some(l)=stage.loaded.take() else{return};
        let radius=g.world_radius;
        let handles:Vec<Handle<AnimationClip>>=l.clips.iter().filter_map(|c|character::animation_clip(c).ok().map(|a|clips.add(a))).collect();
        let graph=AnimationGraph::new();let nodes:Vec<AnimationNodeIndex>=vec![];
        let _=(&nodes,&graph);
        let unlocked:Vec<usize>=l.roster.iter().enumerate().filter(|(_,k)|k.unlocked).map(|(i,_)|i).collect();
        for (slot,&i) in unlocked.iter().enumerate(){
            let Some(m)=&l.models[i] else{continue};
            // Select-kid: the executable's static stand positions (FEManager `__sinit`), ground height + 0.1, yaw 0.08.
            let s3=SP_POSITIONS[slot.min(7)];
            let mut sp=Vec3::new(s3[0],s3[1],s3[2]);
            if let Some(h)=g.ground_height(sp.x,sp.z){sp.y=h+0.1;}
            // Multiplayer: the vault's positions (an entry without a depth falls back to the select-kid spot).
            let (mut mpv,rot_mp)=match l.roster[i].mp{Some(p) if p[2]!=0.=>(Vec3::new(p[0],p[1],p[2]),p[3]),_=>(sp,0.08)};
            if let Some(h)=g.ground_height(mpv.x,mpv.z){mpv.y=h.max(mpv.y.min(h+0.2));}
            let p=sp;
            let up=assets::upload_skinned(m,&mut meshes,&mut materials,&mut images);
            // Per-kid graph: idle, run, pick-me, own celebration.
            let mut kgraph=AnimationGraph::new();
            let pick=[0usize,2,3,l.celebrate[i]];
            let knodes:[AnimationNodeIndex;4]=std::array::from_fn(|n|kgraph.add_clip(handles[pick[n]].clone(),1.,kgraph.root));
            let kgraph=graphs.add(kgraph);
            let model=commands.spawn((crate::game::GameEntity,Transform::default(),Visibility::default())).id();
            let root=commands.spawn((crate::game::GameEntity,KidMarker,Kid{index:i,sp,mp:mpv,rot_sp:0.08,rot_mp,at:p,yaw:0.08,model,nodes:knodes,anim:0},Transform::from_matrix(g.display(p)),Visibility::default())).id();
            commands.entity(model).insert(ChildOf(root));
            let mut ap=AnimationPlayer::default();
            for (ni,n) in knodes.iter().enumerate(){let a=ap.play(*n);a.repeat();a.set_weight(if ni==0{1.}else{0.});a.seek_to(slot as f32*0.37);}
            commands.entity(model).insert((ap,AnimationGraphHandle(kgraph.clone())));
            let joints=character::spawn_rig(&mut commands,&mut ibp,&l.skeleton,&up,model,model);
            let _=joints;
            let _=radius;
        }
        // Selection rings (the original's `TarManager` player indicators): one per player colour.
        let ring=meshes.add(Annulus::new(0.34,0.46));
        let mats:Vec<Handle<StandardMaterial>>=PLAYER_COLOURS.iter().map(|c|materials.add(StandardMaterial{base_color:*c,unlit:true,cull_mode:None,..default()})).collect();
        for p in 0..PLAYER_COLOURS.len(){
            commands.spawn((crate::game::GameEntity,KidMarker,Ring{player:if p==4{None}else{Some(p)}},Mesh3d(ring.clone()),MeshMaterial3d(mats[p].clone()),Transform::IDENTITY,Visibility::Hidden));
        }
        stage.ring=Some((ring,mats));
        stage.clips=handles;stage.loaded=Some(l);stage.spawned=true;
        return
    }
    let ptr=v.vm.pointer[0];let ptr_free=v.vm.hover.is_none();
    let (mw,mh)=v.root_movie().map(|m|(m.apt.width.max(1.),m.apt.height.max(1.))).unwrap_or((747.,420.));
    let fe=&mut v.vm.fe;
    let Ok((cam,cgt))=cams.single() else{return};
    let vp=cam.logical_viewport_size().unwrap_or(Vec2::new(mw,mh));
    let to_movie=|world:Vec3|->Option<Vec2>{ cam.world_to_viewport(cgt,world).ok().map(|p|Vec2::new(p.x/vp.x*mw,p.y/vp.y*mh)) };
    let roster=fe.roster.clone();let chosen0=fe.kid_chosen.clone();
    let filter_boy=fe.profiles.filter==0;
    // Which kids are shown / pickable.
    let shown=|idx:usize|->bool{
        let Some(k)=roster.get(idx) else{return false};
        match want{1|2=>k.unlocked,3=>chosen0.contains(&Some(idx)),_=>false}
    };
    // Select-kid: the Boys / Girls choice decides which kids can be picked (the others keep standing).
    let pickable=|idx:usize|->bool{shown(idx)&&(want!=1||roster.get(idx).is_some_and(|k|k.boy==filter_boy))};
    let pointer=Vec2::new(ptr.0,ptr.1);
    // Hit test: pointer against the head-to-foot segment of each shown kid.
    let mut hover:Option<(usize,f32)>=None;
    if want==1||want==2{
        for (_,k,t,_) in kids.iter(){
            if !pickable(k.index){continue}
            let (Some(foot),Some(head))=(to_movie(t.translation),to_movie(t.translation+t.rotation*Vec3::Y*1.45)) else{continue};
            let seg=head-foot;let l2=seg.length_squared().max(1e-3);
            let u=((pointer-foot).dot(seg)/l2).clamp(0.,1.);
            let d=(foot+seg*u-pointer).length();
            let radius=seg.length()*0.22+4.;
            if d<=radius&&hover.is_none_or(|(_,bd)|d<bd){hover=Some((k.index,d));}
        }
    }
    fe.kid_hover=hover.map(|h|h.0);
    // Click: picks (single: opens the confirm dialog, multi: assigns the next player).
    if let Some(_c)=fe.kid_click.take(){
        if let (Some(h),true,false)=(fe.kid_hover,ptr_free,fe.keyboard_open){
            if want==1{
                fe.kid_chosen=vec![Some(h)];fe.profiles.kid=h as i32;fe.profiles.typed.clear();
                fe.kid_picked=Some(h);
            }else if want==2{
                let n=fe.mp.players.max(2) as usize;
                if fe.kid_chosen.len()!=n{fe.kid_chosen=vec![None;n];}
                if let Some(p)=fe.kid_chosen.iter().position(|c|*c==Some(h)){fe.kid_chosen[p]=None;}
                else if let Some(p)=fe.kid_chosen.iter().position(|c|c.is_none()){fe.kid_chosen[p]=Some(h);}
                let done=fe.kid_chosen.iter().all(|c|c.is_some());
                if done!=fe.next_vis{fe.next_vis=done;fe.todo.push(("SetNextButtonVisibility".into(),vec![crate::apt_vm::V::Num(done as i32 as f64)]));}
            }
        }
    }
    // Place kids and rings.

    let chosen=fe.kid_chosen.clone();let multi=want==2||(want==3&&fe.mp.active);
    let hover_idx=fe.kid_hover;
    let mut targets:Vec<(Vec3,Option<usize>)>=vec![];
    for (_,mut k,mut t,mut vis) in kids.iter_mut(){
        let show=shown(k.index)||(want==3&&chosen.contains(&Some(k.index)));
        *vis=if show{Visibility::Inherited}else{Visibility::Hidden};
        // During the confirm dialog the chosen kid steps to the front-centre.
        let confirm=Vec3::new(CONFIRM_POSITION[0],g.ground_height(CONFIRM_POSITION[0],CONFIRM_POSITION[2]).map(|h|h+0.1).unwrap_or(CONFIRM_POSITION[1]),CONFIRM_POSITION[2]);
        let is_chosen=chosen.contains(&Some(k.index));
        let (goal,rot)=match want{3=>(confirm,0.),1=>(k.sp,k.rot_sp),_=>(k.mp,k.rot_mp)};
        // Walk (the select-kid run clip) at the character top speed, then settle facing the camera.
        let flat=Vec3::new(goal.x-k.at.x,0.,goal.z-k.at.z);let dist=flat.length();
        let dt=time.delta_secs().min(0.1);
        let anim;
        if dist>0.05{
            let step=(4.5*dt).min(dist);
            let dir=flat/dist;
            k.at.x+=dir.x*step;k.at.z+=dir.z*step;k.at.y+= (goal.y-k.at.y)*(step/dist).min(1.);
            k.yaw=dir.x.atan2(dir.z);anim=1;
        }else{
            k.at=goal;
            let d=(rot-k.yaw+std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)-std::f32::consts::PI;
            k.yaw+=d*0.2;
            anim=if want==3&&is_chosen{3}else if hover_idx==Some(k.index)&&want!=3{2}else{0};
        }
        if anim!=k.anim{
            k.anim=anim;
            if let Ok(mut ap)=players.get_mut(k.model){for (ni,n) in k.nodes.iter().enumerate(){if let Some(a)=ap.animation_mut(*n){a.set_weight(if ni as u8==anim{1.}else{0.});}}}
        }
        *t=Transform::from_matrix(g.display(k.at)*Mat4::from_rotation_y(k.yaw));
        if show{
            if let Some(p)=chosen.iter().position(|c|*c==Some(k.index)){targets.push((k.at,if multi{Some(p)}else{None}));}
            else if hover_idx==Some(k.index){targets.push((k.at,None));}
        }
    }
    for (_,r,mut t,mut vis) in rings.iter_mut(){
        let hit=targets.iter().find(|(_,p)|*p==r.player);
        match (hit,want){
            (Some((at,_)),1|2|3)=>{*vis=Visibility::Inherited;*t=Transform::from_matrix(g.display(*at+Vec3::Y*0.09)*Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2));}
            _=>{*vis=Visibility::Hidden;}
        }
    }
    let _=(&mut commands,Entity::PLACEHOLDER);
    // The original asks for the kid's name on the system keyboard (FEManager state 10: `TRC::OpenKeyboard(7, ..., T_NameKid, <kid name>)`).
    if let Some(h)=v.vm.fe.kid_picked.take(){
        let name=crate::fe_host::kid_name(&v.vm,h);
        let title=v.vm.locale_string("T_NameKid");
        use crate::apt_vm::V;
        v.vm.fe.keyboard_open=true;
        v.vm.fe.todo.push(("TRCDisplayKeyboard".into(),vec![V::Num(7.),V::Str("".into()),V::Str(title.as_str().into()),V::Num(1.),V::Str(name.as_str().into())]));
    }
}
