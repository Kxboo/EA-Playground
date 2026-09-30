"""Original enclosing MGTetherball Initialize with composed gameplay helpers.

Base initialization/cleanup, ball construction, game-logic initialization, animation initialization, distance selection, state-1 entry and shadow setup
execute natively. Remaining named helper calls are explicit supplied boundaries.
"""
import copy,hashlib,json,math,struct,sys
from pathlib import Path
from tetherball_reset_oracle import ResetEmu
from tetherball_lifecycle_oracle import GAME,CHARS,AI,ANIM,POLE,CAMERA
from tetherball_scene_oracle import bits,from_bits,read_motion,OBJ
from ppc_emu2 import sx,f32
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'_bevy/tests/data/tetherball_startup_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
WORLD=0x72100000;BALL=0x72101000;ALT=0x72102000;REN=0x72103000;ASSET=0x72104000;PHYS=0x72105000;COLL=0x72106000;VT=0x72107000
FRONTEND=0x72700000
MANAGER=0x7210e000;CONTROLLERS=[0x72200000+i*0x1000 for i in range(4)]
class StartupEmu(ResetEmu):
 def step(self,pc):
  if pc in self.hooks:self.hooks[pc](self);return self.lr
  w=self.word(pc)
  if w>>26==31 and (w>>1)&1023==202:
   d,a=(w>>21)&31,(w>>16)&31;total=self.r[a]+self.ca
   self.r[d]=total&0xffffffff;self.ca=int(total>0xffffffff);return pc+4
  if w>>26==31 and (w>>1)&1023==183:
   d,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31;addr=(self.r[a]+self.r[b])&0xffffffff;self.w32(addr,self.r[d]);self.r[a]=addr;return pc+4
  return super().step(pc)
 def prepare(self,c):
  self.case=c;self.events=[];self.keys={};self.next_key=1
  self.spawn_index=0;self.ai_allocate_index=0;self.ball_allocation=0;self.ball_strings={}
  self.wr(GAME,bytes(0x460));self.w32(GAME,VT);self.w32(VT+0x10,0x803ab470)
  for o,v in c['initial_game_words'].items():self.w32(GAME+int(o,16),v)
  for j,ai in enumerate(AI):
   self.wr(ai,bytes(0x90));self.w32(ai+0x68,c['input']['ai_initial_scales'][j])
  self.w32(0x805e8320+0x8c,WORLD)
  self.w32(0x805e8320+0x88,WORLD);self.wr(FRONTEND,bytes(0x60));self.wr(FRONTEND+0x48,bytes([0xab,0xcd]))
  for o,p in [(8,0x72108000),(12,0x72109000),(16,0x7210a000),(20,0x7210b000),(24,0x7210c000),(32,PHYS)]:self.w32(WORLD+o,p)
  for a,p in [(0x806012ac,0x7210d000),(0x8060215c,REN),(0x8060214c,0x7210e000),(0x8060223c,ASSET),(0x806021e4,0x7210f000),(0x80602008,0x72110000),(0x806018d4,0x72111000)]:self.w32(a,p)
  self.w32(MANAGER,CAMERA)
  self.wr(0x7210d000,b'\x01');self.wr(0x80607e78,bytes(c['input']['forced_ai']))
  self.vector(POLE+0xf0,list(map(from_bits,c['input']['pole_position'])));self.wr(POLE+0xa8,b'\x25');self.wr(ALT+0xa8,b'\x73');self.wr(CAMERA,bytes(0x3b0));self.wr(BALL,bytes(0x170))
  self.w32(0x8060204c,0x72113000);self.wr(0x72113000,bytes([int(c['input']['logic']['multi_flag'])]))
  self.w32(0x80601f78,c['input']['null_trail'])
  self.wr(OBJ,self.rd(BALL,0x170));c['initial_ball']=read_motion(self)
  for j,char in enumerate(CHARS):
   self.w32(char+0x18,ANIM[j]);self.w32(ANIM[j]+0x1670,0) # Old graph; SetNextAnimState supplies new count.
   self.w32(char+0x124,char+0x1000);self.w32(char+0x128,char+0x2000)
   self.w32(char+0x1000+0xa8,j)
  for j,v in enumerate(c['input']['character']['identity']):self.w32(CHARS[0]+0x130+j*4,v)
  self.initial_game={hex(o):self.r32(GAME+o) for o in range(0,0x460,4)}
 def install(self):
  def record(name,indices=()):return lambda em:em.events.append([name]+[sx(em.r[i],32) for i in indices])
  for a,name,ix in [(0x803b40c0,'home_disable',()),(0x803ad790,'home_icon',()),(0x803bf054,'vsync',(4,)),(0x8041b590,'sync_add',(3,4,5)),(0x8041b650,'sync_del',(3,)),(0x803b40b0,'home_enable',()),(0x802e15e0,'audio_load',(4,)),(0x802e1fd0,'music',(4,))]:self.hooks[a]=record(name,ix)
  self.hooks.pop(0x803ab470,None)
  self.hooks[0x803be1b8]=lambda em:em.events.append(['camera_view_info',em.r[3],bool(em.r[4])])
  self.hooks[0x803bea7c]=lambda em:em.events.append(['camera_reinitialize',em.r[3],em.r[4],em.r[5]])
  def controller(em):
   index=sx(em.r[3],32);handle=CONTROLLERS[index]
   em.events.append(['controller_get',index,handle]);em.r[3]=handle
  self.hooks[0x8032c608]=controller
  self.hooks[0x8032ca88]=lambda em:em.events.append(['controller_reinitialize',em.r[3],em.r[4]])
  self.hooks.pop(0x8039ceac,None)
  def single(em):
   em.events.append(['single_tunables'])
   for o,v in zip([0x16c,0x170,0x174,0x178,0x17c,0x180],em.case['input']['logic']['single']):em.w32(GAME+o,v)
  def multi(em):
   em.events.append(['multi_tunables'])
   variant,mode,rounds,rotations=em.case['input']['logic']['multi']
   for o,v in zip([0x3c,0x44,0x16c,0x170,0x174,0x178,0x17c,0x180],[variant,mode,0,rounds,0,0,0,rotations]):em.w32(GAME+o,v)
  def stats(em):
   em.events.append(['reset_stats']);em.wr(GAME+0x138,bytes(0x34))
   for i,v in enumerate(em.case['input']['logic']['scores']):em.w32(GAME+0x138+i*4,v)
  self.hooks[0x8039cf90]=single;self.hooks[0x8039d080]=multi;self.hooks[0x8039cbd0]=stats
  def alloc(em):
   if em.r[3]==0x170:handle=BALL
   else:
    i=em.ball_allocation;em.ball_allocation+=1
    handle=0 if i in em.case['input']['null_shadows'] else 0x73000000+i*0x100
   em.events.append(['allocate',em.r[3],em.r[4],em.r[5],em.cstring(em.r[6]),handle]);em.r[3]=handle
  self.hooks[0x803cfa08]=alloc
  self.hooks.pop(0x8039d264,None)
  def placeable(em):
   name=em.cstring(em.r[4]);handle=ALT if name.endswith('_w_ball') else POLE;em.events.append(['placeable',name,handle]);em.r[3]=handle
  self.hooks[0x803da02c]=placeable
  self.hooks.pop(0x803abefc,None)
  self.hooks[0x803da0b0]=lambda em:em.events.append(['placeable_area',em.r[3],sx(em.r[4],32)])
  self.hooks[0x803becb0]=lambda em:(em.events.append(['camera_get',em.r[4],CAMERA]),em.r.__setitem__(3,CAMERA))
  for addr in [0x8039ba00,0x8039bbec,0x8039bd20]:self.hooks.pop(addr,None)
  def get_player(em):
   handle=CHARS[0] if em.case['input']['character']['existing'] else 0
   em.events.append(['get_player_character',sx(em.r[4],32),handle]);em.r[3]=handle
  def spawn(em):
   slot=0 if em.r[5]==em.r32(GAME+0x78) else 1;handle=CHARS[slot]
   em.events.append(['spawn_character',[em.r[5],em.r[6]],em.vector_bits(em.r[7],3),em.r[8],em.r[9],em.r[10],[sx(em.r32(em.r[1]+8),32),sx(em.r32(em.r[1]+12),32)],handle]);em.r[3]=handle
  def allocate_ai(em):
   handle=AI[em.ai_allocate_index];em.ai_allocate_index+=1
   em.events.append(['allocate_ai_slot',handle]);em.r[3]=handle
  def bind_ai(em):
   em.events.append(['set_character_ai_entity',em.r[3]-0x2000,em.r[4]])
  self.hooks[0x802d0120]=get_player;self.hooks[0x802ec308]=spawn
  self.hooks[0x80328104]=lambda em:em.events.append(['set_character_state_position',em.r[3]-0x130,em.vector_bits(em.r[4],3)])
  self.hooks[0x80336498]=lambda em:em.events.append(['set_character_state_direction',em.r[3]-0x130,em.vector_bits(em.r[4],3)])
  self.hooks[0x803d5894]=allocate_ai
  self.hooks.pop(0x80395290,None)
  self.hooks[0x802cc5d4]=lambda em:em.events.append(['construct_tetherball_ai',em.r[3],em.r[4]])
  self.hooks[0x802cd048]=lambda em:em.events.append(['add_ai_entity',em.r[4]])
  self.hooks[0x802e7910]=bind_ai
  self.hooks[0x8039ce0c]=lambda em:em.events.append(['setup_multiplayer_ability'])
  self.hooks[0x8039cd50]=lambda em:em.events.append(['setup_single_player_ability'])
  self.hooks[0x8032dc38]=lambda em:em.events.append(['set_controller_state',em.r[3],sx(em.r[4],32)])
  self.hooks.pop(0x80395354,None)
  def ai_game(em):
   present=em.case['input']['ai_game_present'];em.events.append(['ai_game',present]);em.r[3]=GAME if present else 0
  self.hooks[0x80319a54]=ai_game
  names_ai=['ai_toofastchance','ai_tooslowchance','ai_wrongheightchance','ai_powerhitchance','ai_megahitchance','ai_powermessupfactor','ai_megamessupfactor']
  def ai_byte(em):
   name=em.cstring(em.r[4]);value=em.case['input']['ai_tuning'][names_ai.index(name)]
   em.events.append(['byte',name,em.r[5],value]);em.r[3]=value
  self.hooks[0x802f4df0]=ai_byte
  # Override inherited camera hooks to retain exact engine arguments.
  for a,name in [(0x80397390,'camera_position'),(0x803973a4,'camera_target'),(0x803973c4,'camera_start'),(0x803bcd5c,'camera_direction')]:self.hooks[a]=lambda em,name=name:em.events.append([name,em.r[3],em.vector_bits(em.r[4],3)])
  for a,name in [(0x803bcf10,'camera_desired_position'),(0x803bd000,'camera_desired_target')]:self.hooks[a]=lambda em,name=name:em.events.append([name,em.r[3],em.vector_bits(em.r[4],3),em.r[5]])
  self.hooks[0x803973b8]=lambda em:em.events.append(['camera_backwards',em.r[3],bits(em.f[1])])
  self.hooks[0x803bd0b0]=lambda em:em.events.append(['camera_rotation',em.r[3],bits(em.f[1]),em.r[4]])
  self.hooks[0x803cb218]=lambda em:(em.events.append(['load_bigfile',em.cstring(em.r[4]),em.r[5],em.r[6],777]),em.r.__setitem__(3,777))
  self.hooks[0x803b8018]=lambda em:(em.events.append(['ground_height',em.vector_bits(em.r[4],3),bits(em.f[1]),em.case['input']['ground_height']]),em.f.__setitem__(1,from_bits(em.case['input']['ground_height'])))
  self.hooks.pop(0x8039d3d8,None)
  def string_ctor(em):em.ball_strings[em.r[3]]=em.cstring(em.r[4])
  def string_append(em):em.ball_strings[em.r[3]]+=em.cstring(em.r[4])
  def string_ptr(em):
   value=em.ball_strings[em.r[3]];em.r[3]=0x72600000;em.wr(em.r[3],value.encode()+b'\0')
  def ball_asset(em,texture):
   slot=(em.r[5]-BALL)//4;handle=0x72000000+slot*0x100;asset_id=1000+slot
   em.events.append(['texture' if texture else 'model',em.cstring(em.r[4]),asset_id,handle]+([] if texture else [em.r[6]]))
   em.w32(em.r[5],asset_id);em.r[3]=handle
  self.hooks.update({0x803ccc78:string_ctor,0x803ccfd0:string_append,0x803cd454:string_ptr,0x803cce94:lambda em:None,
   0x803cb934:lambda em:ball_asset(em,True),0x803cb930:lambda em:ball_asset(em,False),
   0x803e3440:lambda em:em.events.append(['textures',em.r[3],em.r[4]]),
   0x803bd5d0:lambda em:em.events.append(['cached_ctor',em.r[3]]),
   0x803bd664:lambda em:em.events.append(['scale',em.r[3],em.r32(em.r[3]+0x44)]),
   0x8039ea78:lambda em:em.events.append(['shadow_ctor',em.r[3],em.r[4]]),
   0x803c4bf8:lambda em:em.events.append(['add_entity',em.r[4],em.r[5]])})
  self.hooks[0x803c5594]=lambda em:em.events.append(['set_viewport',em.r[3],em.r[4],[em.r32(em.r[5]+i*4) for i in (0,1,2,4,5,6,8,9,10,12,13,14,16)]])
  for address in [0x8039c6dc,0x8039be84,0x8039cb30,0x8039a94c]:self.hooks.pop(address,None)
  # SetRadius original 16B executes (remove inherited hook if ever installed).
  self.hooks.pop(0x8039e564,None)
  self.hooks[0x802e979c]=lambda em:em.events.append(['switch_ai',em.r[3]])
  self.hooks[0x80256d20]=lambda em:em.r.__setitem__(3,0x72112000)
  self.hooks[0x80256e4c]=lambda em:em.events.append(['callback',em.cstring(em.r[4]),em.r[5],em.r[6]])
  self.hooks[0x802f78a4]=lambda em:(em.events.append(['particle_create',em.cstring(em.r[4]),em.vector_bits(em.r[5],3),888]),em.r.__setitem__(3,888))
  def key(em):
   name=em.cstring(em.r[4]);token=len(em.keys)+1;em.keys[token]=name;em.events.append(['key',name]);em.r[3]=0;em.r[4]=token
  self.hooks[0x802f45e4]=key
  self.hooks[0x802f4794]=lambda em:(em.events.append(['collection',em.keys[em.r[6]],em.keys[em.r[8]]]),em.r.__setitem__(3,COLL))
  names=['ball_basehitspeed','ball_acceleratemodifier','ball_powermodifier','ball_megamodifier']
  def float(em):
   name=em.cstring(em.r[4]);value=em.case['input']['tuning'][names.index(name)];em.events.append(['float',name,em.r[5],value]);em.f[1]=from_bits(value)
  self.hooks[0x802f50e4]=float
  names16=['hit_returnanglepredelta','hit_returnanglepostdelta','hit_accelanglepredelta','hit_accelanglepostdelta']
  def int16(em):
   name=em.cstring(em.r[4]);value=em.case['input']['angle_degrees'][names16.index(name)][em.r[5]];em.events.append(['int16',name,em.r[5],value]);em.r[3]=value&0xffffffff
  def destroy_collection(em):
   em.events.append(['destroy_collection'])
   if em.ball_allocation==6 and em.rd(BALL+0x15c,1)[0]==0:
    # Synthetic engine mutation validates the original late placeable read.
    em.w32(POLE+0xf4,em.case['input']['late_pole_height'])
  self.hooks[0x802f4bf8]=int16;self.hooks[0x802f4900]=destroy_collection
  self.hooks.pop(0x803ab580,None)
  self.hooks[0x80324388]=lambda em:em.r.__setitem__(3,FRONTEND)
  self.hooks[0x8031f294]=lambda em:em.r.__setitem__(3,0x72701000)
  self.hooks[0x8031f29c]=lambda em:em.events.append(['pregame_handlers',sx(em.r[4],32),sx(em.r[5],32),[em.r32(em.r[6]+j*4) for j in range(4)]])
  self.hooks[0x803270e8]=lambda em:em.events.append(['pregame_screen',em.cstring(em.r[4])])
  self.hooks[0x803e022c]=lambda em:em.events.append(['pregame_fade',bool(em.r[4]),bool(em.r[5])])
  self.hooks.pop(0x8039b1b0,None)
  def random_server(em):
   value=em.case['input']['server_random'];em.events.append(['server_random',em.r[3],em.r[4],value]);em.r[3]=value
  self.hooks[0x803b2344]=random_server
  def marker_id(em):
   player=ANIM.index(em.r[3]);index=em.r[4];value=em.case['input']['server_markers'][player][index]['id']
   em.events.append(['marker_id',player,index,value]);em.r[3]=value&0xffffffff
  def marker_matrix(em):
   player=ANIM.index(em.r[3]);index=em.r[4];value=em.case['input']['server_markers'][player][index]['matrix']
   em.events.append(['marker_matrix',player,index,value]);em.r[3]=value
  self.hooks[0x803c8308]=marker_id;self.hooks[0x803c82f8]=marker_matrix
  def server_animation(em):
   player=ANIM.index(em.r[3]);em.events.append(['animation',player,sx(em.r[4],32),bool(em.r[5]),sx(em.r[6],32)])
   em.w32(ANIM[player]+0x1670,em.case['input']['server_marker_counts'][player]&0xffffffff)
  self.hooks[0x803c8b70]=server_animation
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();assert int.from_bytes(exe.read(0x804dd1fc+0x10,4),"big")==0x803ab470
 em=StartupEmu(exe);em.install();em.call(0x803c63a8);cases=[]
 for variant in range(3):
  for mode in range(4):
   for kind in [3,6]:
    i=len(cases);initial={'0x3c':variant,'0x44':mode,'0x40':0,'0x48':0xffffffff,'0xc0':kind,'0x430':6,'0x434':2,'0x438':3,'0x78':0x12345678,'0x7c':0x9abcdef0,'0xb8':0x11223344,'0xbc':0x55667788}
    c={'label':f'variant{variant}-mode{mode}-kind{kind}','initial_game_words':initial,'input':dict(variant=variant,mode=mode,kind_c0=kind,forced_ai=[i%2,(i//2)%2],pole_position=[bits(v) for v in [(i-8)*1.25,(-2+i%5)*.75,3-i*.5]],ground_height=bits((i%7)-3.25),tuning=[bits(v) for v in [4.+mode,.25+i*.01,1.75,3.25]],angle_degrees=[[10+i,-12,16],[20,-23+i,27],[30,31,32-i],[40,41,-42]])}
    initial['0x70']=[0,1,2,-1,-0x80000000,0x7fffffff][i%6]&0xffffffff
    initial['0x58']=(i+17)<<24
    c['input']['server_random']=i%2
    c['input']['server_markers']=[[dict(id=k,matrix=0x72900000+p*0x1000+j*0x100) for j,k in enumerate([5,63,7,63] if i%3==0 else [5,7] if i%3==1 else [63])] for p in range(2)]
    c['input']['server_marker_counts']=[len(row) if i%5 else -1 for row in c['input']['server_markers']]
    c['input']['ai_game_present']=i%7!=0
    c['input']['ai_initial_scales']=[bits(i+.125),bits(-i-.25)]
    c['input']['ai_tuning']=[(i*23+j*17)%256 for j in range(7)]
    initial['0x48']=[-1,0,1,2,3,4,5,6,7,8][i%10]&0xffffffff
    c['input']['null_shadows']=[j for j in [4,5] if (i>>(j-4))&1]
    c['input']['character']=dict(existing=i%3!=0,identity=[0x12345678,0x9abcdef0 if i%3==1 else 0x9abcdef1])
    initial['0x4c']=(0xab<<24)|((i%4)<<16)|((i%2)<<8)|0xcd
    c['input']['null_trail']=[0xffffffff,0,0x12345678][i%3]
    c['input']['logic']=dict(multi_flag=kind==6,single=[0,3,0,(-1 if i%2 else 1)*(i%4),0,4+mode],multi=[variant,mode,3,4+mode],scores=[11+i,23+i,37+i])
    initial['0x84']=(i%2)<<24;initial['0xc4']=((i//2)%2)<<24;initial['0x328']=(((i//4)%2)<<8)|((i//8)%2)
    if i in (0,1):c['input']['pole_position']=[bits(-0.0),bits(0.0),bits(-0.0)]
    c['input']['late_pole_height']=bits(from_bits(c['input']['pole_position'][1])+.25) if i%4==0 else c['input']['pole_position'][1]
    em.prepare(c);em.call(0x803966c4,[GAME,WORLD],max_steps=100000)
    em.wr(OBJ,em.rd(BALL,0x170));c['expected_ball']=read_motion(em);c['expected_ball_trails']=[em.r32(BALL+0x140+j*4) for j in range(3)]
    c['initial_game_words']=em.initial_game;c['expected_game_words']={hex(o):em.r32(GAME+o) for o in range(0,0x460,4)};c['effects']=copy.deepcopy(em.events)
    c['expected_server_camera_target']=em.vector_bits(CAMERA+0x3c0,3)
    c['expected_ball_attachment']=[em.r32(BALL+o) for o in [0x74,0x78]]
    c['expected_ball_resources']=[em.r32(BALL+4+j*4) for j in range(12)]
    c['expected_ball_accelerate_modifier']=em.r32(BALL+0x158);c['expected_ball_flag_15c']=em.rd(BALL+0x15c,1)[0]
    c['expected_physical_area']=sx(em.r32(em.r32(WORLD+8)+0x2c),32)
    c['expected_frontend_flags']=[bool(v) for v in em.rd(FRONTEND+0x48,2)]
    c['expected_ai_configuration']=[dict(difficulty=list(em.rd(ai+0x78,7)),enabled=em.rd(ai+0x7f,1)[0]!=0,heading=em.r32(ai+0x80),charge=em.r32(ai+0x70)) for ai in AI]
    c['expected_ai_ball_handles']=[em.r32(ai+0x60) for ai in AI]
    c['expected_camera_words']={hex(o):em.r32(CAMERA+o) for o in [0x39c,0x3a0]};c['expected_placeable_visible']=[em.rd(p+0xa8,1)[0] for p in [POLE,ALT]];c['expected_ai_words']=[{hex(o):em.r32(p+o) for o in [0x64,0x68,0x74]} for p in AI];c['expected_ball_radius']=em.r32(BALL+0xb0);c['expected_ball_desired_radius']=em.r32(BALL+0xb4);c['expected_renderer_word_1c0']=em.r32(0x7210f000+0x1c0);c['expected_ancient_byte']=em.rd(0x7210d000,1)[0]
    em.events=[];em.call(0x803ab524,[GAME]);c['uninitialize_effects']=copy.deepcopy(em.events);c['uninitialized_game_words']={hex(o):em.r32(GAME+o) for o in range(0,0x460,4)};cases.append(c)
 return dict(elf_sha256=SHA,world_services=[em.r32(WORLD+o) for o in [8,12,16,20,24,32]],pool=em.r32(0x805ffae4),boundary='Enclosing Initialize executes; animation initialization, player-distance selection, state-1 entry, shadow setup, InitGameLogicState, Tetherball constructor, both base Initialize bodies, all three character append helpers, ball asset initialization, AI construction/configuration and tuning selector, area selection, pregame entry and PreGameInfo constructor, SetUpServer and Grab, UnInitialize and SetRadius execute natively; all former generic stages execute; engine services, decoded VLT tunable helpers and ResetStats inputs remain explicit boundaries.',handles=dict(game=GAME,world=WORLD,ball=BALL,pole=POLE,alternate_pole=ALT,camera=CAMERA,players=CHARS,ai=AI,camera_manager=MANAGER,controllers=CONTROLLERS),camera_position_offset=em.vector_bits(0x805e3750,3),camera_target_offset=em.vector_bits(0x805e37b0,3),cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
 print('Verified',len(value['cases']),'original enclosing startup calls')
