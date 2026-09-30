"""Complete native game/base/world constructor memory, with ResetStats inputs.

All original constructor and angle bodies execute. ResetStats database results
are supplied; its represented stores match the existing independent reset port.
"""
import hashlib,json,random,sys
from pathlib import Path
from tetherball_startup_oracle import StartupEmu,SHA,ROOT
from tetherball_lifecycle_oracle import GAME
import re_functions as rf
OUT=ROOT/'_bevy/tests/data/tetherball_constructor_golden.json'
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();assert int.from_bytes(exe.read(0x803e1d94,4),"big")==0x38600450
 em=StartupEmu(exe);rng=random.Random(0x80396410);cases=[]
 def stats(e):
  e.wr(GAME+0x138,bytes(0x34))
  for j,v in enumerate(e.constructor_input['score_weights']):e.w32(GAME+0x138+j*4,v)
 em.hooks[0x8039cbd0]=stats
 for i in range(32):
  initial=bytes(rng.randrange(256) for _ in range(0x450));em.wr(GAME,initial)
  tags=[[0,0,0,0],[255,255,255,255],[0,128,128,128],[1,127,128,255],[128,1,2,3],[0,0,0,255]]
  input=dict(scene=[0,0x72100000,0xffffffff,0x81234567][i%4],base_tag=tags[i%len(tags)],
   invalid_game_fx=[0,0xffffffff,0x12345678,0x80000000][i%4],score_weights=[i-16,0x7fffffff,-0x80000000])
  em.constructor_input=input
  em.wr(exe.symbols['_SDA_BASE_']['value']-0x46e8,bytes(input['base_tag']))
  em.w32(0x80601f60,input['invalid_game_fx'])
  ret=em.call(0x80396410,[GAME,input['scene']])
  cases.append(dict(input=input,initial_bytes=list(initial),expected_bytes=list(em.rd(GAME,0x450)),result=ret))
 projections=[]
 startup=json.loads((ROOT/'_bevy/tests/data/tetherball_startup_golden.json').read_text())
 for i,c in enumerate(startup['cases']):
  initial=dict(c['expected_game_words'])
  for offset,value in [(0x224,33),(0x228,0x01010100),(0x234,7),(0x238,3),(0x258,77),(0x25c,0x01000000),
    (0x260,51),(0x264,52),(0x268,53),(0x26c,0x3f800000),(0x270,54),(0x274,2),(0x278,0x72140000),
    (0x2e4,0),(0x32c,0x01010101),(0x330,0x01000000),(0x338,0x3f800000),(0x33c,0x3f000000),
    (0x344,0x3f800000),(0x348,0x3f800000),(0x35c,0x3f800000),(0x41c,0x01010000),
    (0x428,0x40000000),(0x42c,0x01010100),(0x43c,3),(0x444,0x01010000)]:initial[hex(offset)]=value
  for player in range(2):
   initial[hex(0x22c+player*4)]=8;initial[hex(0x30c+player*4)]=3;initial[hex(0x314+player*4)]=99
  for offset,value in initial.items():em.w32(GAME+int(offset,16),value)
  input=cases[i]['input'];em.constructor_input=input
  em.wr(exe.symbols['_SDA_BASE_']['value']-0x46e8,bytes(input['base_tag']));em.w32(0x80601f60,input['invalid_game_fx'])
  em.call(0x80396410,[GAME,input['scene']])
  projections.append(dict(input=input,initial_game_words=initial,expected_game_words={hex(o):em.r32(GAME+o) for o in range(0,0x450,4)}))
 return dict(elf_sha256=SHA,address='0x80396410',allocation_size=0x450,game=GAME,cases=cases,projection_cases=projections)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['cases']),'native complete game constructor images')
