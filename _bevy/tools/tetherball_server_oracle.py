"""Native live SetUpServer and Grab after a composed startup, including moving balls."""
import copy,hashlib,json,sys
from tetherball_startup_oracle import StartupEmu,WORLD,BALL,CAMERA,SHA,ROOT
from tetherball_lifecycle_oracle import GAME,ANIM
from tetherball_scene_oracle import OBJ,bits,read_motion
import re_functions as rf
OUT=ROOT/'_bevy/tests/data/tetherball_server_golden.json'
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 source=json.loads((ROOT/'_bevy/tests/data/tetherball_startup_golden.json').read_text())
 exe=rf.load();em=StartupEmu(exe);em.install();em.call(0x803c63a8);cases=[]
 for i in range(64):
  c=copy.deepcopy(source['cases'][i%24]);em.prepare(c);em.call(0x803966c4,[GAME,WORLD])
  a=c['input'];a['server_random']=i%2
  a['server_markers']=[[dict(id=id,matrix=0x72940000+p*0x1000+j*0x100) for j,id in enumerate([3,63,8,63] if i%4==0 else [3,8] if i%4==1 else [63] if i%4==2 else [])] for p in range(2)]
  a['server_marker_counts']=[len(row) if i%7 else -1 for row in a['server_markers']]
  count=[-0x80000000,-1,0,1,2,3,0x7fffffff,0][i%8]
  em.w32(GAME+0x70,count&0xffffffff);em.wr(GAME+0x445,bytes([(i//8)%2]));em.w32(GAME+0x278,0x729ffff0)
  velocity=[bits(0.),bits(-0.),bits(1.25),bits(-3.),0x7fc00000][i%5]
  em.w32(BALL+0x88,velocity);em.w32(BALL+0x94,bits(5.5));em.w32(BALL+0x50,bits(9.5))
  em.wr(BALL+0x164,bytes([1,1]));em.w32(BALL+0x74,0x72800000);em.w32(BALL+0x78,0x72801000)
  trails=[a['null_trail'] if (i>>j)&1 else 0x72820000+j for j in range(3)]
  for j,v in enumerate(trails):em.w32(BALL+0x140+j*4,v)
  for graph in ANIM:em.w32(graph+0x1670,0)
  initial_game={hex(o):em.r32(GAME+o) for o in range(0,0x450,4)}
  em.wr(OBJ,em.rd(BALL,0x170));initial_ball=read_motion(em);em.events=[]
  em.call(0x8039b1b0,[GAME]);em.wr(OBJ,em.rd(BALL,0x170))
  cases.append(dict(input=a,initial_game_words=initial_game,initial_ball=initial_ball,initial_ball_attachment=[0x72800000,0x72801000],initial_trails=trails,
   expected_game_words={hex(o):em.r32(GAME+o) for o in range(0,0x450,4)},expected_ball=read_motion(em),
   expected_trails=[em.r32(BALL+0x140+j*4) for j in range(3)],expected_ball_attachment=[em.r32(BALL+o) for o in [0x74,0x78]],
   expected_camera_target=em.vector_bits(CAMERA+0x3c0,3),effects=copy.deepcopy(em.events)))
 return dict(elf_sha256=SHA,address='0x8039b1b0',handles=source['handles'],world_services=source['world_services'],pool=source['pool'],cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['cases']),'native live server/Grab transitions')
