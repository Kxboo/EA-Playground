"""Original Tetherball constructor, including native rmAngle constructors.

Compares represented motion/resources/scene fields; unrelated native bookkeeping
and vtable stores are captured in the memory image, not claimed as ECS owners.
"""
import hashlib,json,random,sys
from pathlib import Path
import re_functions as rf
from tetherball_scene_oracle import SceneEmu,OBJ,FLOATS,INTS,FLAGS,read_motion
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_ball_constructor_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();rng=random.Random(0x8039d264);cases=[]
 base=json.loads((ROOT/'_bevy/tests/data/tetherball_golden.json').read_text())
 for i in range(32):
  em=SceneEmu(exe);initial=base['cases'][i]['initial']
  em.wr(OBJ,bytes(rng.randrange(256) for _ in range(0x170)))
  for name,o in FLOATS.items():em.w32(OBJ+o,initial[name])
  for name,o in INTS.items():em.w32(OBJ+o,initial[name])
  for name,o in FLAGS.items():em.wr(OBJ+o,bytes([int(initial[name])]))
  for o,n in [(0x40,3),(0x60,3),(0xc0,16),(0x100,16)]:
   for j in range(n):em.w32(OBJ+o+j*4,0x3f000000+i*128+j)
  null=[0xffffffff,0,0x12345678,0x80000000][i%4];em.w32(0x80601f78,null)
  before=list(em.rd(OBJ,0x170));ret=em.call(0x8039d264,[OBJ])
  cases.append(dict(initial=initial,initial_bytes=before,null_trail=null,result=ret,
   motion=read_motion(em),position=em.vector_bits(OBJ+0x40,3),anchor=em.vector_bits(OBJ+0x60,3),
   ball_matrix=em.vector_bits(OBJ+0xc0,16),rope_matrix=em.vector_bits(OBJ+0x100,16),
   resources=[em.r32(OBJ+j*4) for j in range(1,13)],trails=em.vector_bits(OBJ+0x140,3),
   accelerate_modifier=em.r32(OBJ+0x158),flag_15c=em.rd(OBJ+0x15c,1)[0],expected_bytes=list(em.rd(OBJ,0x170))))
 return dict(elf_sha256=SHA,address='0x8039d264',cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['cases']),'native ball constructor cases')
