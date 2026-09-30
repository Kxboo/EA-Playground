"""Native enclosing cleanup with original base/ball cleanup and ball destructor.

Renderer/character/UI/audio services, cached/shadow destruction and pole-indicator
rendering are explicit boundaries. Fixtures contain retained and spawned actors,
all ball resource null combinations, independent particle sentinels and HUD gates.
"""
import copy,hashlib,json,sys
from pathlib import Path
from tetherball_startup_oracle import StartupEmu,WORLD,BALL,VT,REN,MANAGER,CONTROLLERS,SHA,ROOT
from tetherball_lifecycle_oracle import GAME,CHARS,ANIM,POLE
from tetherball_scene_oracle import OBJ,read_motion,bits
import re_functions as rf
OUT=ROOT/'_bevy/tests/data/tetherball_cleanup_golden.json'
SHADOWVT=0x72400000;SHADOWDT=0x72400100
CACHED=[0x72500000+i*0x100 for i in range(4)];SHADOWS=[0x72501000,0x72502000]
def install(em):
 em.hooks.pop(0x8039d768,None)
 em.hooks[0x802ec78c]=lambda e:e.events.append(['despawn',e.r[4],bool(e.r[5])])
 em.hooks[0x8032dc20]=lambda e:e.events.append(['controller_pop_handle',e.r[3]])
 em.hooks[0x803cb708]=lambda e:e.events.append(['delete_assets',e.r[4]])
 em.hooks[0x803bd5f4]=lambda e:e.events.append(['cached_destroy',e.r[3],e.r[4]])
 em.hooks[0x803c4c28]=lambda e:e.events.append(['remove_shadow',e.r[4],e.r[5]])
 em.hooks[SHADOWDT]=lambda e:e.events.append(['shadow_destroy',e.r[3],e.r[4]])
 em.hooks[0x803cfa04]=lambda e:e.events.append(['free_ball',e.r[3]])
 def worldplayer(e):
  handle=e.case['cleanup']['world_player'];e.events.append(['world_player',e.r[4],handle]);e.r[3]=handle
 em.hooks[0x802d0120]=worldplayer
 em.hooks[0x803c8b70]=lambda e:e.events.append(['animation_reset',e.r[3],e.r[4],e.r[5],e.r[6]])
 em.hooks[0x802e9734]=lambda e:e.events.append(['local_control',e.r[3]])
 em.hooks[0x802f7abc]=lambda e:e.events.append(['destroy_fx',e.r32(e.r[4]),e.r[5]])
 em.hooks[0x80256e0c]=lambda e:e.events.append(['purge_callbacks'])
 em.hooks[0x80317ca8]=lambda e:e.events.append(['timer_visible',e.r[3]])
 em.hooks[0x80318468]=lambda e:e.events.append(['mega_visible',e.r[3],e.r[4]])
 em.hooks[0x803176fc]=lambda e:e.events.append(['clear_hud'])
 em.hooks[0x8032713c]=lambda e:e.events.append(['close_screen'])
 em.hooks[0x8039cab4]=lambda e:e.events.append(['indicator',bits(e.f[1])])
 em.hooks[0x802e2080]=lambda e:e.events.append(['unduck_music'])
 em.hooks[0x802e2010]=lambda e:e.events.append(['stop_music'])
 em.hooks[0x802e162c]=lambda e:e.events.append(['unload_audio',e.r[4]])
 em.hooks[0x803e02a8]=lambda e:e.events.append(['restore_area'])
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 startup=json.loads((ROOT/'_bevy/tests/data/tetherball_startup_golden.json').read_text())
 exe=rf.load();em=StartupEmu(exe);em.install();install(em);em.call(0x803c63a8);cases=[]
 for i in range(64):
  c=copy.deepcopy(startup['cases'][i%24]);em.install();em.prepare(c);em.call(0x803966c4,[GAME,WORLD]);install(em)
  null=[0xffffffff,0,0x12345678,0x80000000][i%4]
  particles=[null if (i>>j)&1 else 0x34560000+j for j in range(3)]
  resources=[0 if (i>>j)&1 else CACHED[j] for j in range(4)]
  shadows=[0 if (i>>(j+2))&1 else SHADOWS[j] for j in range(2)]
  players=[0 if i%8==j else CHARS[j] for j in range(2)]
  spawn=[255 if (i>>j)&1 else 0 for j in range(2)]
  control=[(i>>4)&1,(i>>5)&1];activity=[41,73]
  cfg=dict(null_game_fx=null,spawn=spawn,players=players,local_block=control,activity=activity,
           world_player=CHARS[i%2],trails=particles,cached=resources,shadows=shadows,count=[0,1,2,-1][(i//4)%4])
  c['cleanup']=cfg
  em.w32(0x80601f60,null);em.w32(GAME+0x210,cfg['count'])
  em.wr(GAME+0x130,bytes(spawn));em.wr(GAME+0x32e,bytes([i%2]));em.wr(GAME+0x424,b'\xff')
  em.w32(GAME+0x334,null if i%3==0 else 0x45670000);em.w32(GAME+0x340,null if i%5==0 else 0x45670001)
  for j in range(2):
   em.w32(GAME+0x120+j*4,players[j]);em.w32(CHARS[j]+0x18,ANIM[j])
   em.wr(CHARS[j]+0x12c,bytes([control[j]]));em.wr(CHARS[j]+0x120,bytes([activity[j]]))
  for j,v in enumerate(particles):em.w32(BALL+0x140+j*4,v)
  for o,v in zip([0xc,0x10,0x20,0x24],resources):em.w32(BALL+o,v)
  for j,v in enumerate(shadows):
   em.w32(BALL+4+j*4,v)
   if v:em.w32(v,SHADOWVT)
  em.w32(SHADOWVT+8,SHADOWDT);em.wr(BALL+0x15c,b'\x01')
  row=dict(input=c['input'],cleanup=cfg,initial_game_words={hex(o):em.r32(GAME+o) for o in range(0,0x460,4)},
           initial_ball_resources=[em.r32(BALL+j*4) for j in range(1,13)])
  em.wr(OBJ,em.rd(BALL,0x170));row['initial_ball']=read_motion(em)
  em.w32(0x805e8320+0x88,WORLD)
  em.events=[];em.call(0x803973d8,[GAME])
  row.update(effects=copy.deepcopy(em.events),expected_game_words={hex(o):em.r32(GAME+o) for o in range(0,0x460,4)},
   expected_activity=[em.rd(CHARS[j]+0x120,1)[0] for j in range(2)],expected_trails=[em.r32(BALL+0x140+j*4) for j in range(3)],
   expected_ball_resources=[em.r32(BALL+j*4) for j in range(1,13)],expected_flag_15c=em.rd(BALL+0x15c,1)[0],
   expected_visible=[em.rd(POLE+0xa8,1)[0],em.rd(0x72102000+0xa8,1)[0]])
  cases.append(row)
 return dict(elf_sha256=SHA,handles={**startup['handles'],'animations':ANIM},world_services=startup['world_services'],cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['cases']),'native complete cleanup cases')
