"""Original enclosing MGTetherball Initialize with four composed gameplay helpers.

Animation initialization, distance selection, state-1 entry and shadow setup
execute natively. Remaining named helper calls are explicit supplied boundaries.
"""
import copy,hashlib,json,math,struct,sys
from pathlib import Path
from tetherball_reset_oracle import ResetEmu
from tetherball_lifecycle_oracle import GAME,CHARS,AI,ANIM,POLE,CAMERA
from tetherball_scene_oracle import bits,from_bits
from ppc_emu2 import sx,f32
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'_bevy/tests/data/tetherball_startup_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
WORLD=0x72100000;BALL=0x72101000;ALT=0x72102000;REN=0x72103000;ASSET=0x72104000;PHYS=0x72105000;COLL=0x72106000;VT=0x72107000
class StartupEmu(ResetEmu):
 def step(self,pc):
  if pc in self.hooks:self.hooks[pc](self);return self.lr
  w=self.word(pc)
  if w>>26==31 and (w>>1)&1023==183:
   d,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31;addr=(self.r[a]+self.r[b])&0xffffffff;self.w32(addr,self.r[d]);self.r[a]=addr;return pc+4
  return super().step(pc)
 def prepare(self,c):
  self.case=c;self.events=[];self.keys={};self.next_key=1
  self.wr(GAME,bytes(0x460));self.w32(GAME,VT);self.w32(VT+0x10,0x803ab470)
  for o,v in c['initial_game_words'].items():self.w32(GAME+int(o,16),v)
  self.w32(0x805e8320+0x8c,WORLD)
  for o,p in [(8,0x72108000),(12,0x72109000),(16,0x7210a000),(20,0x7210b000),(24,0x7210c000),(32,PHYS)]:self.w32(WORLD+o,p)
  for a,p in [(0x806012ac,0x7210d000),(0x8060215c,REN),(0x8060214c,0x7210e000),(0x8060223c,ASSET),(0x806021e4,0x7210f000),(0x80602008,0x72110000),(0x806018d4,0x72111000)]:self.w32(a,p)
  self.wr(0x7210d000,b'\x01');self.wr(0x80607e78,bytes(c['input']['forced_ai']))
  self.vector(POLE+0xf0,list(map(from_bits,c['input']['pole_position'])));self.wr(POLE+0xa8,b'\x25');self.wr(ALT+0xa8,b'\x73');self.wr(CAMERA,bytes(0x3b0));self.wr(BALL,bytes(0x170))
  self.initial_game={hex(o):self.r32(GAME+o) for o in range(0,0x460,4)}
 def helper(self,name,args):
  self.events.append([name]+args)
  for off,size,value in self.case['input']['boundary_writes'].get(name,[]):
   if size==4:self.w32(GAME+off,value)
   elif size==1:self.wr(GAME+off,bytes([value]))
   else:raise AssertionError(size)
 def install(self):
  def record(name,indices=()):return lambda em:em.events.append([name]+[sx(em.r[i],32) for i in indices])
  for a,name,ix in [(0x803b40c0,'home_disable',()),(0x803ad790,'home_icon',()),(0x803bf054,'vsync',(4,)),(0x8041b590,'sync_add',(3,4,5)),(0x8041b650,'sync_del',(3,)),(0x803b40b0,'home_enable',()),(0x802e15e0,'audio_load',(4,)),(0x802e1fd0,'music',(4,))]:self.hooks[a]=record(name,ix)
  self.hooks[0x803ab470]=lambda em:em.helper('base_initialize',[])
  self.hooks[0x8039ceac]=lambda em:em.helper('init_logic',[])
  def alloc(em):em.events.append(['allocate',em.r[3],em.r[4],em.r[5],em.cstring(em.r[6]),BALL]);em.r[3]=BALL
  self.hooks[0x803cfa08]=alloc
  self.hooks[0x8039d264]=lambda em:em.helper('ball_ctor',[em.r[3]])
  def placeable(em):
   name=em.cstring(em.r[4]);handle=ALT if name.endswith('_w_ball') else POLE;em.events.append(['placeable',name,handle]);em.r[3]=handle
  self.hooks[0x803da02c]=placeable
  self.hooks[0x803abefc]=lambda em:em.helper('set_area',[sx(em.r[4],32)])
  self.hooks[0x803becb0]=lambda em:(em.events.append(['camera_get',em.r[4],CAMERA]),em.r.__setitem__(3,CAMERA))
  def player(name):
   def f(em):em.helper(name,[em.vector_bits(em.r[4],3),bits(em.f[1]),em.r[5],em.r[6]])
   return f
  for a,name in [(0x8039ba00,'init_player'),(0x8039bbec,'init_ai_player'),(0x8039bd20,'init_additional_player')]:self.hooks[a]=player(name)
  self.hooks[0x80395354]=lambda em:em.helper('init_ai_entity',[em.r[3],bool(em.r[4]),em.r[5],em.r32(em.r[6])])
  # Override inherited camera hooks to retain exact engine arguments.
  for a,name in [(0x80397390,'camera_position'),(0x803973a4,'camera_target'),(0x803973c4,'camera_start'),(0x803bcd5c,'camera_direction')]:self.hooks[a]=lambda em,name=name:em.events.append([name,em.r[3],em.vector_bits(em.r[4],3)])
  for a,name in [(0x803bcf10,'camera_desired_position'),(0x803bd000,'camera_desired_target')]:self.hooks[a]=lambda em,name=name:em.events.append([name,em.r[3],em.vector_bits(em.r[4],3),em.r[5]])
  self.hooks[0x803973b8]=lambda em:em.events.append(['camera_backwards',em.r[3],bits(em.f[1])])
  self.hooks[0x803bd0b0]=lambda em:em.events.append(['camera_rotation',em.r[3],bits(em.f[1]),em.r[4]])
  self.hooks[0x803cb218]=lambda em:(em.events.append(['load_bigfile',em.cstring(em.r[4]),em.r[5],em.r[6],777]),em.r.__setitem__(3,777))
  self.hooks[0x803b8018]=lambda em:(em.events.append(['ground_height',em.vector_bits(em.r[4],3),bits(em.f[1]),em.case['input']['ground_height']]),em.f.__setitem__(1,from_bits(em.case['input']['ground_height'])))
  self.hooks[0x8039d3d8]=lambda em:em.helper('ball_init',[em.r[4],em.vector_bits(em.r[5],3),bits(em.f[1]),em.r[6]])
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
  self.hooks[0x802f4bf8]=int16;self.hooks[0x802f4900]=record('destroy_collection')
  self.hooks[0x803ab580]=lambda em:em.helper('open_pregame',[sx(em.r[4],32)])
  self.hooks[0x8039b1b0]=lambda em:em.helper('setup_server',[])
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 em=StartupEmu(rf.load());em.install();em.call(0x803c63a8);cases=[]
 for variant in range(3):
  for mode in range(4):
   for kind in [3,6]:
    i=len(cases);initial={'0x3c':variant,'0x44':mode,'0x40':0,'0x48':0xffffffff,'0xc0':kind,'0x430':6,'0x434':2,'0x438':3,'0x78':0x12345678,'0x7c':0x9abcdef0,'0xb8':0x11223344,'0xbc':0x55667788}
    boundary={'init_logic':[[0x430,4,4+mode],[0x434,4,2],[0x438,4,3]],'init_player':[[0x120,4,CHARS[0]],[0x128,4,AI[0]],[0x210,4,1],[0x40,4,1]],'init_ai_player':[[0x124,4,CHARS[1]],[0x12c,4,AI[1]],[0x210,4,2]],'init_additional_player':[[0x124,4,CHARS[1]],[0x12c,4,AI[1]],[0x210,4,2],[0x40,4,2]],'set_distance':[[0x18c,4,2]],'init_animations':[[0x190,4,56],[0x194,4,88]],'open_pregame':[[0x58,1,0]],'change_state':[[0x34,4,1],[0x250,4,0],[0x254,4,0]],'setup_server':[[0x21c,4,i%2],[0x220,4,1-i%2]]}
    c={'label':f'variant{variant}-mode{mode}-kind{kind}','initial_game_words':initial,'input':dict(variant=variant,mode=mode,kind_c0=kind,forced_ai=[i%2,(i//2)%2],pole_position=[bits(v) for v in [(i-8)*1.25,(-2+i%5)*.75,3-i*.5]],ground_height=bits((i%7)-3.25),boundary_writes=boundary,tuning=[bits(v) for v in [4.+mode,.25+i*.01,1.75,3.25]],angle_degrees=[[10+i,-12,16],[20,-23+i,27],[30,31,32-i],[40,41,-42]])}
    initial['0x84']=(i%2)<<24;initial['0xc4']=((i//2)%2)<<24;initial['0x328']=(((i//4)%2)<<8)|((i//8)%2)
    for name in ['set_distance','init_animations','change_state']:boundary.pop(name,None)
    if i in (0,1):c['input']['pole_position']=[bits(-0.0),bits(0.0),bits(-0.0)]
    em.prepare(c);em.call(0x803966c4,[GAME,WORLD],max_steps=100000)
    c['initial_game_words']=em.initial_game;c['expected_game_words']={hex(o):em.r32(GAME+o) for o in range(0,0x460,4)};c['effects']=copy.deepcopy(em.events)
    c['expected_camera_words']={hex(o):em.r32(CAMERA+o) for o in [0x39c,0x3a0]};c['expected_placeable_visible']=[em.rd(p+0xa8,1)[0] for p in [POLE,ALT]];c['expected_ai_words']=[{hex(o):em.r32(p+o) for o in [0x64,0x68,0x74]} for p in AI];c['expected_ball_radius']=em.r32(BALL+0xb0);c['expected_ball_desired_radius']=em.r32(BALL+0xb4);c['expected_renderer_word_1c0']=em.r32(0x7210f000+0x1c0);c['expected_ancient_byte']=em.rd(0x7210d000,1)[0];cases.append(c)
 return dict(elf_sha256=SHA,world_services=[em.r32(WORLD+o) for o in [8,12,16,20,24,32]],pool=em.r32(0x805ffae4),boundary='Enclosing Initialize executes; animation initialization, player-distance selection, state-1 entry, shadow setup and SetRadius execute natively; remaining named gameplay helpers are supplied boundary_writes.',handles=dict(game=GAME,world=WORLD,ball=BALL,pole=POLE,alternate_pole=ALT,camera=CAMERA,players=CHARS,ai=AI),camera_position_offset=em.vector_bits(0x805e3750,3),camera_target_offset=em.vector_bits(0x805e37b0,3),cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
 print('Verified',len(value['cases']),'original enclosing startup calls')
