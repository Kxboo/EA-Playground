"""Original ResetRound/ResetMiniGame/SetUpServer with explicit engine service boundaries.

Ball, angle, matrix, server-selection, distance and reset stores execute unhooked.
HUD/animation initialization and AI/database services have supplied results.
"""
import copy, hashlib, json, random, sys, math
from pathlib import Path
from tetherball_lifecycle_oracle import LifecycleEmu, GAME, CHARS, AI, ANIM, POLE, CAMERA, OBJ, SHA, bits, from_bits
from ppc_emu2 import sx, f32
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_reset_golden.json'
NEW_AI=[0x71401000,0x71402000]; CONTROL=[0x71403000,0x71404000]
VTABLE=0x71405000; DIFFICULTY=0x71406000
WORDS={'game_mode_044':0x44,'base_player_count_070':0x70,'initial_rotation_178':0x178,'rotation_limit_430':0x430,'ai_special_case_0c0':0xc0,
 'word_224':0x224,'counter_260':0x260,'counter_264':0x264,'counter_268':0x268,'counter_270':0x270,'game_marker_matrix_278':0x278,
 'pending_count_2e4':0x2e4,'field_334_guid':0x334,'receiver_220':0x220}
FLOATS={'camera_heading_394':0x394,'timer_26c':0x26c,'field_35c':0x35c}
BYTES={'field_25c':0x25c,'field_229':0x229,'field_444':0x444,'alternate_server_445':0x445}
ARRAYS={'world_position_110':(0x110,3),'world_matrix_398':(0x398,16),'start_angles_248':(0x248,2),'ai_initial_values_27c':(0x27c,2),'round_tunables_34c_358':(0x34c,4)}

class ResetEmu(LifecycleEmu):
 def step(self,pc):
  w=self.word(pc);op=w>>26;d,a,b,c=(w>>21)&31,(w>>16)&31,(w>>11)&31,(w>>6)&31
  if op==4 and ((w>>1)&1023)==528: # ps_merge00
   self.f[d],self.ps1[d]=self.f[a],self.f[b];return pc+4
  if op==4 and ((w>>1)&31) in (25,29): # ps_mul / ps_madd
   kind=(w>>1)&31
   low=f32(self.f[a]*self.f[c]) if kind==25 else f32(math.fma(self.f[a],self.f[c],self.f[b]))
   high=f32(self.ps1[a]*self.ps1[c]) if kind==25 else f32(math.fma(self.ps1[a],self.ps1[c],self.ps1[b]))
   self.f[d],self.ps1[d]=low,high;return pc+4
  return super().step(pc)
 def __init__(self,exe):
  super().__init__(exe);self.input={};self.keys={};self.next_key=1
  del self.hooks[0x803994f4]
  self.hooks[0x80317ca8]=lambda e:e.events.append(['timer_visible',sx(e.r[3],32)])
  def hud(e):
   e.events.append(['initialize_hud'])
  self.hooks[0x8039c1d8]=hud
  self.hooks[0x803d82f8]=lambda e:e.events.append(['pole_matrix',e.r[3],sx(e.r[4],32),e.vector_bits(e.r[5],16)])
  for addr,name in [(0x80397390,'camera_position'),(0x803973a4,'camera_target'),(0x803973c4,'camera_start'),(0x803bcd5c,'camera_direction')]:
   self.hooks[addr]=lambda e,name=name:e.events.append([name,e.r[3],e.vector_bits(e.r[4],3)])
  for addr,name in [(0x803bcf10,'camera_desired_position'),(0x803bd000,'camera_desired_target')]:
   self.hooks[addr]=lambda e,name=name:e.events.append([name,e.r[3],e.vector_bits(e.r[4],3),e.r[5]])
  self.hooks[0x803973b8]=lambda e:e.events.append(['camera_backwards',e.r[3],bits(e.f[1])])
  self.hooks[0x803bd0b0]=lambda e:e.events.append(['camera_rotation',e.r[3],bits(e.f[1]),e.r[4]])
  def camera(e):
   e.events.append(['camera_lookup',e.r[4],CAMERA]);e.r[3]=CAMERA
  self.hooks[0x803becb0]=camera
  def create_ai(e):
   i=CHARS.index(e.r[4]);entity=NEW_AI[i];e.events.append(['create_ai',i,e.r[4],entity]);e.r[3]=entity
  self.hooks[0x80399afc]=create_ai
  self.hooks[0x802e7910]=lambda e:e.events.append(['bind_ai',CONTROL.index(e.r[3]),e.r[4]])
  self.hooks[0x80395354]=lambda e:e.events.append(['initialize_ai',e.r[3],bool(e.r[4]),e.r[5],e.r32(e.r[6])])
  self.hooks[0x80328104]=lambda e:e.events.append(['character_position',CHARS.index(e.r[3]-0x130),e.vector_bits(e.r[4],3)])
  def init_animations(e):
   e.events.append(['initialize_animations'])
   for i,x in enumerate(e.input['initialized_animations']):e.w32(GAME+0x190+4*i,x)
  self.hooks[0x8039be84]=init_animations
  def marker_id(e):
   i=ANIM.index(e.r[3]);j=e.r[4];value=e.input['markers'][i][j]['id'];e.events.append(['marker_id',i,j,value]);e.r[3]=value
  def marker_matrix(e):
   i=ANIM.index(e.r[3]);j=e.r[4];value=e.input['markers'][i][j]['matrix'];e.events.append(['marker_matrix',i,j,value]);e.r[3]=value
  self.hooks[0x803c8308]=marker_id;self.hooks[0x803c82f8]=marker_matrix
  def random_range(e):
   value=e.input['random_server'];e.events.append(['random',e.r[3],e.r[4],value]);e.r[3]=value
  self.hooks[0x803b2344]=random_range
  def key(e):
   name=e.cstring(e.r[4]);token=next((k for k,v in e.keys.items() if v==name),None)
   if token is None:token=e.next_key;e.keys[token]=name;e.next_key+=1
   e.r[3]=0;e.r[4]=token
  self.hooks[0x802f45e4]=key
  def collection_keys(e):
   e.events.append(['collection',e.keys[e.r[6]],e.keys[e.r[8]]]);e.r[3]=0x71407000
  self.hooks[0x802f4794]=collection_keys
  def collection_names(e):
   e.events.append(['collection',e.cstring(e.r[4]),e.cstring(e.r[5])]);e.r[3]=0x71407000
  self.hooks[0x802f46bc]=collection_names
  self.hooks[0x802f4900]=lambda e:e.events.append(['destroy_collection'])
  def float_value(e):
   name=e.cstring(e.r[4]);i=e.r[5];word=e.input['tuning'][name][i];e.events.append(['db_float',name,i,word]);e.f[1]=from_bits(word)
  self.hooks[0x802f50e4]=float_value
  def count(e):
   name=e.cstring(e.r[4]);value=e.input['scoring_count'];e.events.append(['db_count',name,value]);e.r[3]=value
  self.hooks[0x802f5de8]=count
  def uint(e):
   name=e.cstring(e.r[4]);i=e.r[5];value=e.input['scoring'][name][i];e.events.append(['db_uint',name,i,value]);e.r[3]=value
  self.hooks[0x802f4fe8]=uint
  def difficulty(e):
   value=e.input['scoring_difficulty'];e.events.append(['difficulty',sx(e.r[4],32),value]);e.r[3]=value
  self.hooks[DIFFICULTY]=difficulty

 def write_reset(self,state,rules,ball,aux,inputs):
  self.write(state,rules,ball);self.input=inputs
  for addr in [0x806012ac,0x8060214c,0x806018d4]:self.w32(addr,0x71408000)
  self.wr(CAMERA,bytes(0x500));self.w32(GAME,VTABLE);self.w32(VTABLE+0x50,DIFFICULTY)
  for name,offset in {**WORDS,**FLOATS}.items():self.w32(GAME+offset,aux[name])
  for name,offset in BYTES.items():self.wr(GAME+offset,bytes([aux[name]]))
  for name,(offset,n) in ARRAYS.items():
   for i,v in enumerate(aux[name]):self.w32(GAME+offset+4*i,v)
  for i,v in enumerate(aux['side_flags_244_245_32a_32b']):self.wr(GAME+[0x244,0x245,0x32a,0x32b][i],bytes([v]))
  for i,row in enumerate(aux['slots_284']):
   for j,v in enumerate(row):self.w32(GAME+0x284+12*i+4*j,v)
  self.w32(OBJ+0x74,aux['ball_owner_074']);self.w32(OBJ+0x78,aux['ball_matrix_078'])
  for i,v in enumerate(aux['ball_fx_140']):self.w32(OBJ+0x140+4*i,v)
  self.w32(0x80601f60,inputs['invalid_game_guid']);self.w32(0x80601f78,inputs['invalid_ball_guid'])
  self.wr(0x80607e79,bytes([inputs['ai_enabled']]))
  for i in range(2):
   self.wr(NEW_AI[i],bytes(0x90));self.w32(CHARS[i]+0x128,CONTROL[i]);self.w32(ANIM[i]+0x1670,len(inputs['markers'][i]))

 def read_reset(self,initial):
  state,ball=self.read(initial)
  for i,p in enumerate(state['players']):p['ai_distance']=sx(self.r32(self.r32(GAME+0x128+i*4)+0x74),32)
  aux={name:sx(self.r32(GAME+offset),32) for name,offset in WORDS.items()}
  for name in ['field_334_guid','game_marker_matrix_278']:aux[name]=self.r32(GAME+WORDS[name])
  aux.update({name:self.r32(GAME+offset) for name,offset in FLOATS.items()})
  aux.update({name:bool(self.rd(GAME+offset,1)[0]) for name,offset in BYTES.items()})
  aux.update({name:self.vector_bits(GAME+offset,n) for name,(offset,n) in ARRAYS.items()})
  aux['ai_initial_values_27c']=[sx(v,32) for v in aux['ai_initial_values_27c']]
  aux['side_flags_244_245_32a_32b']=[bool(self.rd(GAME+off,1)[0]) for off in [0x244,0x245,0x32a,0x32b]]
  aux['slots_284']=[self.vector_bits(GAME+0x284+i*12,3) for i in range(8)]
  aux['ball_owner_074']=self.r32(OBJ+0x74);aux['ball_matrix_078']=self.r32(OBJ+0x78);aux['ball_fx_140']=self.vector_bits(OBJ+0x140,3)
  aux['ai_entities']=[dict(handle=self.r32(GAME+0x128+i*4),ball=self.r32(self.r32(GAME+0x128+i*4)+0x60),angle=self.r32(self.r32(GAME+0x128+i*4)+0x64),value=self.r32(self.r32(GAME+0x128+i*4)+0x68),distance=sx(self.r32(self.r32(GAME+0x128+i*4)+0x74),32)) for i in range(2)]
  aux['camera_direct']={'height':self.r32(CAMERA+0x39c),'zero':self.r32(CAMERA+0x3a0),'target':self.vector_bits(CAMERA+0x3c0,3)}
  return state,ball,aux

def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 e=ResetEmu(rf.load());rng=random.Random(0x803994f4)
 source=json.loads((ROOT/'_bevy/tests/data/tetherball_lifecycle_golden.json').read_text(encoding='utf-8'))['cases']
 cases=[]
 for i in range(24):
  base=source[(i*47)%len(source)];state=copy.deepcopy(base['initial']);rules=base['rules'];ball=copy.deepcopy(base['ball'])
  state['focus_player']=i%2;state['server']=i%2;state['receiver']=1-i%2;state['player_count']=2;state['hud_ready']=bool(i%2);state['serve_bubble_visible']=bool((i//2)%2);state['current_distance']=i%3
  aux={name:17 for name in WORDS};aux.update(game_mode_044=i%4,base_player_count_070=[-1,0,1,2][(i//2)%4],initial_rotation_178=[-129,-6,0,3,128,255][i%6],rotation_limit_430=6,ai_special_case_0c0=[5,6][i%2],receiver_220=1-i%2,pending_count_2e4=i%8,game_marker_matrix_278=0x7140a000,field_334_guid=0xaaaa if i%2 else 1234)
  aux.update({name:bits(0.3) for name in FLOATS});aux.update({name:bool((i//(j+1))%2) for j,name in enumerate(BYTES)});aux['alternate_server_445']=bool(i%2);aux['camera_heading_394']=bits([-7.0,-3.1415927410125732,0.3,8.0][i%4])
  aux.update(world_position_110=[bits(x) for x in [3.0,-1.25,7.0]],world_matrix_398=[bits(x) for x in [0.,0.,-1.,0.,0.,1.,0.,0.,1.,0.,0.,0.,2.,3.,4.,1.]],start_angles_248=[bits(-1.25+i*.1),bits(4.5-i*.07)],ai_initial_values_27c=[-3,4],round_tunables_34c_358=[bits(.3)]*4,
   slots_284=[[j,bits(j*.2),99+j] for j in range(8)],side_flags_244_245_32a_32b=[bool(i%2)]*4,ball_owner_074=CHARS[1],ball_matrix_078=0x7140b000,ball_fx_140=[0xbbbb,444,0xbbbb if i%2 else 555])
  inputs=dict(random_server=i%2,invalid_game_guid=0xaaaa,invalid_ball_guid=0xbbbb,ai_enabled=bool((i//2)%2),initialized_animations=[71+i,91+i],scoring_difficulty=i%5,scoring_count=[0,2,4][i%3],
   tuning={n:[bits(x+j*.25) for j in range(4)] for n,x in [('ball_basehitspeed',4.),('ball_acceleratemodifier',.5),('ball_powermodifier',2.),('ball_megamodifier',3.)]},
   scoring={n:[x+j for j in range(4)] for n,x in [('accuracy_points',10),('powerhit_points',20),('megahit_points',50)]},
   markers=[[dict(id=x,matrix=0x7140c000+p*256+j*16) for j,x in enumerate(([1,63,2,63] if i%3 else []))] for p in range(2)])
  for operation,address in [('round',0x803994f4),('minigame',0x80399460),('server',0x8039b1b0)]:
   e.write_reset(state,rules,ball,aux,inputs)
   initial,initial_ball,initial_aux=e.read_reset(state)
   e.events=[];e.call(address,(GAME,),max_steps=30000)
   expected,expected_ball,expected_aux=e.read_reset(state)
   cases.append(dict(operation=operation,initial=initial,ball=initial_ball,aux=initial_aux,inputs=inputs,expected=expected,expected_ball=expected_ball,expected_aux=expected_aux,effects=e.events[:]))
 return dict(elf_sha256=SHA,handles=dict(players=CHARS,old_ai=AI,new_ai=NEW_AI,camera=CAMERA,pole=POLE,ball=OBJ),camera_globals=dict(position=e.vector_bits(0x805e3750,3),target=e.vector_bits(0x805e37b0,3)),cases=cases)

if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:
  assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Reset fixture differs from original PowerPC'
  print('Verified',len(value['cases']),'original reset/server calls')
 else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n');print('Wrote',OUT)
