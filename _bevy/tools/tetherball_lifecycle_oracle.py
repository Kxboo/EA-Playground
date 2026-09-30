"""Execute complete original tetherball entry/animation/distance/camera routines.

UI, controller, animation, camera, particle, AI-takeover and random services
are hooked. RoundEnd records its separately recovered ResetRound dependency.
Outer-Update cases additionally record delegated gameplay/ball/base handlers;
other cases execute nested winner decisions, ball operations and state entry.
"""
import copy,hashlib,json,random,struct,sys
from pathlib import Path
from tetherball_scene_oracle import SceneEmu,FLOATS,INTS,FLAGS,OBJ,bits,from_bits
from tetherball_match_oracle import WORDS,SIGNED,RULES
from ppc_emu2 import sx,f32
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_lifecycle_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
GAME=0x71100000;CHARS=[0x71101000,0x71102000];AI=[0x71103000,0x71104000]
HUMAN=[0x71105000,0x71106000];ANIM=[0x71107000,0x71108000]
POLE=0x71109000;ACTIVE=0x7110a000;CAMERA=0x7110b000
LWORDS={'session_mode':0x40,'game_type':0x48,'variant':0x3c,'player_count':0x210,
 'server':0x214,'receiver':0x218,'focus_player':0x21c,'round_number':0x418,
 'total_rounds':0x438,'round_timer_ms':0x420,'distance_mode':0x174,'current_distance':0x18c}
LFLOATS={'winner_base_angle':0x208,'indicator_current':0x344,'indicator_target':0x348}
LFLAGS={'scoreboard_visible':0x41d,'round_visible':0x41c,'field_32f':0x32f,'serve_bubble_visible':0x330,'hud_ready':0x32e,'paused':0x24}
LARRAYS={'action_states':(0x22c,4,4),'mega_values':(0x314,4,4),'mega_states':(0x30c,4,4),
 'win_animations':(0x1f8,4,4),'lose_animations':(0x190,4,4),'mega_enabled':(0x31c,1,1),
 'winner_turns':(0x200,1,1),'latches':(0x228,2,1)}

class LifecycleEmu(SceneEmu):
 def step(self,pc):
  w=self.word(pc)
  if w>>26==31 and (w>>1)&1023==151: # stwx used by original placement builder
   source,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31
   self.w32(((self.r[a] if a else 0)+self.r[b])&0xffffffff,self.r[source]);return pc+4
  return super().step(pc)
 def __init__(self,exe):
  super().__init__(exe);self.events=[];self.randoms=[];self.random_index=0;self.controllers={}
  self.wr(0x805e3750,bytes(0x90));self.call(0x8039d134)
  def record(name,indices):
   return lambda e:e.events.append([name]+[sx(e.r[i],32) for i in indices])
  for address,name,args in [(0x80318550,'scoreboard',[3,4,5,6]),(0x80317c0c,'round',[3,4,5]),
    (0x803179f8,'serve_bubble',[3,4,5]),(0x80318468,'mega_visible',[3,4]),(0x803184dc,'mega_value',[3,4]),
    (0x80317a94,'winner_single',[3,4]),(0x80317b24,'winner_multi',[3,4]),
    (0x803e01b8,'fade',[4])]:self.hooks[address]=record(name,args)
  self.hooks[0x802e1124]=lambda e:e.r.__setitem__(3,0x7110c000)
  self.hooks[0x802e190c]=record('sound_frontend',[4,5,6]);self.hooks[0x802e19a4]=record('sound_backend',[4,5,6])
  self.hooks[0x8032c608]=lambda e:e.r.__setitem__(3,0x71200000+e.r[3]*256)
  self.hooks[0x8032dc20]=lambda e:e.events.append(['controller_pop',(e.r[3]-0x71200000)//256])
  self.hooks[0x8032dc38]=lambda e:e.events.append(['controller_set',(e.r[3]-0x71200000)//256,sx(e.r[4],32)])
  self.hooks[0x803c8b70]=lambda e:e.events.append(['animation',ANIM.index(e.r[3]),sx(e.r[4],32),bool(e.r[5]),sx(e.r[6],32)])
  self.hooks[0x802e979c]=lambda e:e.events.append(['switch_ai',CHARS.index(e.r[3])])
  for address,target in [(0x803bd000,True),(0x803bcf10,False)]:
   self.hooks[address]=lambda e,target=target:e.events.append(['camera',target,e.vector_bits(e.r[4],3),e.r[5]])
  def create(e):
   e.events.append(['particle_create',e.cstring(e.r[4]),e.vector_bits(e.r[5],3)]);e.r[3]=0xabcdef
  self.hooks[0x802f78a4]=create
  self.hooks[0x802f7abc]=lambda e:e.events.append(['particle_destroy',e.r32(e.r[4]),sx(e.r[5],32)])
  def random_range(e):
   low,high=sx(e.r[3],32),sx(e.r[4],32)
   result=self.randoms[self.random_index]%(high-low+1)+low;self.random_index+=1
   e.events.append(['random',low,high,result]);e.r[3]=result
  self.hooks[0x803b2344]=random_range
  self.hooks[0x80317878]=lambda e:e.r.__setitem__(3,0x7110f000)
  self.hooks[0x803176fc]=lambda e:e.events.append(['clear_hud'])
  self.hooks[0x80324388]=lambda e:e.r.__setitem__(3,0x71110000)
  self.hooks[0x8032713c]=lambda e:e.events.append(['close_screen'])
  self.hooks[0x803994f4]=lambda e:e.events.append(['reset_round'])
  self.hooks[0x8031922c]=lambda e:e.events.append(['reset_scoreboard'])
  self.hooks[0x803ab6fc]=lambda e:e.events.append(['post_game',sx(e.r[4],32),e.vector_bits(e.r[5],70)])

 def write(self,state,rules,ball):
  self.wr(GAME,bytes(0x460));self.wr(OBJ,bytes(0x170));self.events=[];self.random_index=0
  m=state['match_state']
  for name,offset in WORDS.items():self.w32(GAME+offset,m[name])
  for name,offset in [('rotations',0x132),('round_wins',0x134)]:self.wr(GAME+offset,bytes(x&255 for x in m[name]))
  self.wr(GAME+0x20c,bytes([m['match_over']]))
  for name,offset in RULES.items():self.w32(GAME+offset,rules[name])
  for name,offset in {**LWORDS,**LFLOATS}.items():self.w32(GAME+offset,state[name])
  for name,offset in LFLAGS.items():self.wr(GAME+offset,bytes([state[name]]))
  for name,(offset,stride,size) in LARRAYS.items():
   for i,x in enumerate(state[name]):
    if size==4:self.w32(GAME+offset+i*stride,x)
    else:self.wr(GAME+offset+i*stride,bytes([x]))
  for i,p in enumerate(state['players']):
   self.wr(CHARS[i],bytes(0x200));self.w32(GAME+0x120+i*4,CHARS[i]);self.w32(GAME+0x128+i*4,AI[i])
   self.w32(CHARS[i]+0x18,ANIM[i]);self.w32(ANIM[i]+0x54,p['current_animation'])
   self.w32(CHARS[i]+0x124,HUMAN[i] if p['controller'] is not None else 0)
   self.w32(HUMAN[i]+0xa8,p['controller'] or 0)
   self.w32(CHARS[i]+0x1d8,p['special_win_animation']);self.w32(CHARS[i]+0x1b0,p['facing'])
   for j,x in enumerate(p['direction']):self.w32(CHARS[i]+0x1a0+j*4,x)
   self.w32(CHARS[i]+0x14c,p['movement_speed']);self.w32(AI[i]+0x74,p['ai_distance'])
   self.wr(GAME+0x84+i*0x40,bytes([p['player_flag']]))
  self.w32(GAME+0x184,POLE);self.w32(GAME+0x104,OBJ);self.w32(GAME+0x390,CAMERA)
  for i,x in enumerate(state['pole_position']):self.w32(POLE+0xf0+i*4,x)
  for name,offset in {**FLOATS,**INTS}.items():self.w32(OBJ+offset,ball[name])
  for name,offset in FLAGS.items():self.wr(OBJ+offset,bytes([ball[name]]))
  active=state['active_tetherball_variant'];self.w32(0x805e8320+0x90,ACTIVE if active is not None else 0)
  tag=self.rd(self.e.symbols['_SDA_BASE_']['value']-0x4dc7,4)
  packed=(tag[0]<<24)+(sx(tag[1],8)<<16)+(sx(tag[2],8)<<8)+sx(tag[3],8)
  self.w32(ACTIVE+0x38,packed);self.w32(ACTIVE+0x3c,active or 0)
  self.w32(0x805e8320+0x88,0x7110d000);self.w32(0x80602008,0x7110e000)
  self.w32(0x8060204c,0x71111000);self.wr(0x71111000,bytes([state['postgame_win_flag']]))
  for i,row in enumerate(state['statistics']):
   for j,x in enumerate(row):self.w32(GAME+0x144+i*0x14+j*4,x)
  for i,x in enumerate(state['score_weights']):self.w32(GAME+0x138+i*4,x)

 def read(self,initial):
  state=copy.deepcopy(initial);m=state['match_state']
  for name,offset in WORDS.items():m[name]=sx(self.r32(GAME+offset),32) if name in SIGNED else self.r32(GAME+offset)
  for name,offset in [('rotations',0x132),('round_wins',0x134)]:m[name]=[sx(x,8) for x in self.rd(GAME+offset,2)]
  m['match_over']=bool(self.rd(GAME+0x20c,1)[0])
  for name,offset in LWORDS.items():state[name]=sx(self.r32(GAME+offset),32)
  for name,offset in LFLOATS.items():state[name]=self.r32(GAME+offset)
  for name,offset in LFLAGS.items():state[name]=bool(self.rd(GAME+offset,1)[0])
  for name,(offset,stride,size) in LARRAYS.items():state[name]=[sx(self.r32(GAME+offset+i*stride),32) if size==4 else bool(self.rd(GAME+offset+i*stride,1)[0]) for i in range(2)]
  for i,p in enumerate(state['players']):
   p['movement_speed']=self.r32(CHARS[i]+0x14c);p['ai_distance']=sx(self.r32(AI[i]+0x74),32)
   p['facing']=self.r32(CHARS[i]+0x1b0);p['direction']=self.vector_bits(CHARS[i]+0x1a0,3)
  state['statistics']=[[sx(self.r32(GAME+0x144+i*0x14+j*4),32) for j in range(5)] for i in range(2)]
  state['score_weights']=[sx(self.r32(GAME+0x138+i*4),32) for i in range(3)]
  ball={**{k:self.r32(OBJ+off) for k,off in FLOATS.items()},**{k:sx(self.r32(OBJ+off),32) for k,off in INTS.items()},**{k:bool(self.rd(OBJ+off,1)[0]) for k,off in FLAGS.items()}}
  return state,ball

def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 e=LifecycleEmu(rf.load());rng=random.Random(0x8039a94c)
 ball_cases=json.loads((ROOT/'_bevy/tests/data/tetherball_golden.json').read_text(encoding='utf-8'))['cases']
 cases=[]
 for i in range(260):
  m=dict(rotations=[rng.choice([-128,-6,-5,-4,-3,-2,-1,0,1,2,3,4,5,6,126,127]) for _ in range(2)],round_wins=[rng.choice([-128,0,1,2,127]) for _ in range(2)],
   elapsed_ms=rng.choice([0,999,1000,1001,30000,0xffffffff]),state_ms=rng.choice([0,249,250,999,1000,1001,4750,4751,4999,5000,5001,0xffffffff]),previous_state_ms=173,
   state_code=[0,1,3,8,9,26,27,28,29,30,31,0xffffffff][i%12],round_winner=i%2,match_winner=(i+1)%2,match_over=i%3==0,result=i%3,final_result=(i+1)%3)
  state=dict(match_state=m,paused=bool((i//12)%2),session_mode=i%4,game_type=rng.choice([-1,0,5,6,7,8,9]),variant=i%3,player_count=i%3,server=i%2,receiver=(i+1)%2,focus_player=rng.choice([-1,0,1]),
   players=[dict(controller=rng.choice([None,0,1,3]),special_win_animation=rng.choice([-1,85,95,229]),current_animation=rng.choice([-1,0,85,95,225,226,227,228,229]),player_flag=bool(rng.randrange(2)),facing=bits(f32(rng.uniform(-8,8))),direction=[0,0,bits(1.)],movement_speed=bits(3.),ai_distance=-1) for _ in range(2)],
   round_number=rng.choice([-1,0,1,2,3]),total_rounds=rng.choice([1,3,5]),scoreboard_visible=bool(i%2),round_visible=bool(i%3),round_timer_ms=-123,latches=[True,True],
   action_states=[5,6],mega_values=[63,100],mega_states=[3,4],mega_enabled=[False,False],field_32f=True,serve_bubble_visible=False,hud_ready=True,
   statistics=[[rng.choice([1,2,20,100]),rng.choice([0,5,29,0x7fffffff]),7,-3,43] for _ in range(2)],score_weights=[10,20,50],postgame_win_flag=bool(i%2),
   distance_mode=rng.choice([-1,0,0,0,1,2,3,4]),current_distance=rng.randrange(3),pole_position=[bits(f32(rng.uniform(-20,20))) for _ in range(3)],
   indicator_target=bits(rng.choice([-0.5,0.,0.009,0.01,0.011,0.5])),indicator_current=bits(rng.choice([-0.5,0.,0.5])),win_animations=[13,14],lose_animations=[15,16],winner_turns=[False,True],winner_base_angle=bits(0.5),active_tetherball_variant=rng.choice([None,0,1,2]))
  rules=dict(mode=i%3,rotation_limit=rng.choice([-2,1,3,6,128]),wins_required=rng.choice([-128,1,2,3,128]),time_limit_seconds=rng.choice([0,1,30]))
  ball=copy.deepcopy(ball_cases[i%len(ball_cases)]['initial']);ball['angular_velocity']=bits(rng.choice([-12.,-0.,0.,1.,6.]))
  e.randoms=[rng.randrange(100) for _ in range(12)]
  commands=[['change',code] for code in [0,9,26,27,28,29,30,31,0xffffffff]] if i<30 else [[rng.choice(['change','resetting','win_animations','celebrations','distance','camera']),rng.choice([26,27,28,29,30])]]
  commands.append(['round_end',16])
  commands.append(['intro',16])
  commands.append(['update',rng.choice([-1,0,1,16,60,2147483647])])
  for command in commands:
   if command[0]=='camera':command[1]=rng.choice([-1,0,1,2])
   case_state=copy.deepcopy(state)
   intro=dict(initial_receiver=(i+1)%2,field_25c=bool(i%2),scoreboard_needs_reset=bool((i//2)%2))
   if command[0]=='intro':
    case_state['focus_player']=i%2
    case_state['hud_ready']=bool((i//4)%2)
   e.write(case_state,rules,ball)
   e.w32(GAME+0x220,intro['initial_receiver']);e.wr(GAME+0x25c,bytes([intro['field_25c']]));e.wr(GAME+0x444,bytes([intro['scoreboard_needs_reset']]))
   name,arg=command
   address={'intro':0x803979e4,'change':0x8039a94c,'resetting':0x80397ae4,'win_animations':0x8039c07c,'celebrations':0x8039d0bc,'distance':0x8039aeb8,'camera':0x8039b504,'round_end':0x80399108,'update':0x80397640}[name]
   hooks=e.hooks.copy()
   if name=='update':
    e.hooks[0x8039b674]=lambda em:em.events.append(['gestures'])
    e.hooks[0x8039d904]=lambda em:em.events.append(['ball_update',sx(em.r[4],32)])
    e.hooks[0x8039cab4]=lambda em:em.events.append(['indicator',bits(em.f[1])])
    e.hooks[0x803ab4f0]=lambda em:em.events.append(['base_update',sx(em.r[4],32)])
    for code,addr in [(1,0x80397988),(3,0x803979e4),(8,0x80399b60),(9,0x80399be0),(26,0x80397ae4),(27,0x80397b74),(28,0x80398368),(29,0x80398a84),(30,0x80399108)]:
     def handler(em,code=code):
      em.events.append(['handler',code,sx(em.r[4],32)]);em.wr(GAME+0x24,bytes([code==9]));em.r[3]=code%2
     e.hooks[addr]=handler
   returned=e.call(address,(GAME,arg))
   e.hooks=hooks
   expected,expected_ball=e.read(case_state)
   expected_intro=dict(initial_receiver=e.r32(GAME+0x220),field_25c=bool(e.rd(GAME+0x25c,1)[0]),scoreboard_needs_reset=bool(e.rd(GAME+0x444,1)[0]))
   has_return=name in ['intro','resetting','celebrations','round_end'] or (name=='update' and m['state_code'] in [1,3,8,9,26,27,28,29,30])
   cases.append(dict(initial=case_state,intro=intro,expected_intro=expected_intro,rules=rules,ball=ball,randoms=e.randoms,command=command,expected=expected,expected_ball=expected_ball,effects=e.events[:],returned=returned if has_return else None))
 return dict(elf_sha256=SHA,cases=cases)

if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:
  assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Lifecycle differs from original PPC'
  print('Verified',len(value['cases']),'tetherball lifecycle/orchestration calls')
 else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n');print('Wrote',OUT)
