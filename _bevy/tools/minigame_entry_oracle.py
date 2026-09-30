"""Original shared minigame area conversion and pregame entry helpers."""
import copy,hashlib,json,random,sys
from tetherball_startup_oracle import StartupEmu,SHA,ROOT,FRONTEND
from tetherball_lifecycle_oracle import GAME
from ppc_emu2 import sx
import re_functions as rf
OUT=ROOT/'_bevy/tests/data/minigame_entry_golden.json'
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();em=StartupEmu(exe);em.install();rng=random.Random(0x803ab580)
 areas=[dict(input=n,result=sx(em.call(0x803de608,[0,n&0xffffffff]),32)) for n in [-0x80000000,-99,-1,*range(16),99,0x7fffffff]]
 seed=json.loads((ROOT/'_bevy/tests/data/tetherball_startup_golden.json').read_text())['cases'][0];pregames=[]
 for i in range(24):
  em.prepare(copy.deepcopy(seed));a=dict(players=[-0x80000000,-2,-1,0,1,2,3,0x7fffffff][i%8],dare=[-1,0,3,8,-0x80000000,0x7fffffff][i%6],kind=[-1,0,2,10,-0x80000000,0x7fffffff][i%6])
  initial=bytearray(rng.randrange(256) for _ in range(0x450))
  for off,value in [(0x70,a['players']),(0x48,a['dare'])]:initial[off:off+4]=(value&0xffffffff).to_bytes(4,'big')
  em.wr(GAME,initial);em.call(0x803ab580,[GAME,a['kind']&0xffffffff])
  pregames.append(dict(input=a,events=copy.deepcopy(em.events),frontend_flags=[bool(v) for v in em.rd(FRONTEND+0x48,2)],initial_game_bytes=list(initial),expected_game_bytes=list(em.rd(GAME,0x450))))
 return dict(elf_sha256=SHA,area_address='0x803de608',pregame_address='0x803ab580',areas=areas,pregames=pregames)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['areas']),'native area conversions and',len(value['pregames']),'pregame entries')
