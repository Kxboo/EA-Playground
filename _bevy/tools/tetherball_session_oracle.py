import json,random,sys
from tetherball_startup_oracle import StartupEmu,SHA,ROOT
from tetherball_lifecycle_oracle import GAME
import re_functions as rf
OUT=ROOT/'_bevy/tests/data/tetherball_session_golden.json'
def generate():
 exe=rf.load();em=StartupEmu(exe);rng=random.Random(0x803ab3f0);cases=[];teams=0x72300000
 # Check actual tetherball virtual dispatch targets before executing their bodies.
 for offset,target in [(0x24,0x803ab3e8),(0x28,0x803ab380),(0x44,0x8039cd48),(0x48,0x803ab428)]:
  assert int.from_bytes(exe.read(0x804dd1fc+offset,4),'big')==target
 for i in range(48):
  initial=bytes(rng.randrange(256) for _ in range(0x450));participants=bytes(rng.randrange(256) for _ in range(128))
  a=dict(level=[-1,0,1,2,0x7fffffff,-0x80000000][i%6],difficulty=i%4,dare_type=i%9-1,
   rules=rng.getrandbits(32),game_id=rng.getrandbits(32),team_count=[-1,0,1,2,4,0x7fffffff][i%6],participants=list(participants))
  em.wr(GAME,initial);em.w32(teams,a['team_count']);em.w32(teams+4,rng.getrandbits(32));em.wr(teams+8,participants)
  for addr,value in [(0x803ab3e8,a['level']),(0x803ab380,a['difficulty']),(0x8039cd48,a['dare_type']),(0x803ab3f0,teams),(0x803ab428,a['rules'])]:
   em.call(addr,[GAME,value&0xffffffff])
  em.w32(GAME+0x38,a['game_id']) # WorldMan's direct stw at the end of setup.
  cases.append(dict(input=a,initial_bytes=list(initial),expected_bytes=list(em.rd(GAME,0x450))))
 return dict(elf_sha256=SHA,caller='0x803e1b2c',cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['cases']),'native session setter/team images')
