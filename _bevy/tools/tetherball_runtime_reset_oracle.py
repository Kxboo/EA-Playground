"""Native reset graph with animation, HUD, and AI bodies executed; external engine reads supplied."""
import copy,hashlib,json,sys
from pathlib import Path
from tetherball_reset_oracle import ResetEmu,GAME,NEW_AI,CHARS,SHA
from tetherball_animation_init_oracle import OFFSETS
from tetherball_tuning_oracle import Oracle,load,AI as FIELDS,OBJ as TUNING_GAME
from ppc_emu2 import sx
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'_bevy/tests/data/tetherball_runtime_reset_golden.json'
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 base=json.loads((ROOT/'_bevy/tests/data/tetherball_reset_runtime_golden.json').read_text(encoding='utf-8'));exe=rf.load();db,hashes=load();d=Oracle(exe,db);e=ResetEmu(exe)
 for a in [0x8039be84,0x80395354,0x8039c1d8]:del e.hooks[a]
 e.hooks[0x80319a54]=lambda em:em.r.__setitem__(3,GAME)
 e.hooks[0x803176ec]=lambda em:em.events.append(['setup',em.r[4]])
 e.hooks[0x803270e8]=lambda em:em.events.append(['open_screen',em.cstring(em.r[4])])
 # Collection traffic of native AI initialization is internal corpus access,
 # separate from ResetStats's supplied database effect boundary.
 for a in [0x802f46bc,0x802f4794,0x802f4900]:
  old=e.hooks[a]
  def boundary(em,old=old):
   if not 0x80395354<=em.lr<0x803954a8:old(em)
   elif old is not None:
    before=len(em.events);old(em);del em.events[before:]
  e.hooks[a]=boundary
 def byte(em):
  name=em.cstring(em.r[4]);assert name in FIELDS
  d.w32(TUNING_GAME+0x40,em.r32(GAME+0x40));d.w32(TUNING_GAME+0x48,em.r32(GAME+0x48));d.collection=d.cstr(d.call(0x8039ce38,[TUNING_GAME]));d.wr(0x77000000,name.encode()+b'\0');em.r[3]=d.call(0x802f4df0,[0x73000000,0x77000000,em.r[5]])
 e.hooks[0x802f4df0]=byte
 cases=[]
 for operation in ['round','minigame']:
  seeds=[c for c in base['cases'] if c['operation']==operation and c['initial']['player_count']==2][:12]
  for seed in seeds:
   c=copy.deepcopy(seed);i=len(cases);c['label']='runtime-'+operation+'-'+str(i);c['initial']['session_mode']=1;c['initial']['game_type']=[-1,0,3,6][i%4]
   c['live_queue']=[dict(kind=(j%3),auxiliary=0x3f800000+j,controller_id=13+j) for j in range(i%7)]
   c['aux']['pending_count_2e4']=len(c['live_queue']);c['aux']['field_229']=bool(i%2)
   c['inputs']['invalid_ball_guid']=0xffffffff
   c['live_trails']=[0xffffffff if (i+j)%3==0 else 0xabc000+i*3+j for j in range(3)];c['aux']['ball_fx_140']=c['live_trails']
   e.write_reset(c['initial'],dict(mode=0,rotation_limit=6,wins_required=2,time_limit_seconds=30000),c['ball'],c['aux'],c['inputs']);s=c['initial_serve']
   e.wr(0x71110000+0x48,bytes(s['frontend_flags']))
   for p in range(2):
    e.wr(NEW_AI[p]+0x6c,bytes([s['ai_waiting'][p]]));e.call(0x80395290,[NEW_AI[p],CHARS[p]])
   for off in OFFSETS:
    for p in range(2):e.w32(GAME+off+p*4,c['words'][hex(off)][p])
   e.call({'round':0x803994f4,'minigame':0x80399460}[operation],[GAME],max_steps=30000)
   c['expected'],c['expected_ball'],c['expected_aux']=e.read_reset(c['initial']);c['effects']=copy.deepcopy(e.events)
   c['expected_words']={hex(o):[sx(e.r32(GAME+o+p*4),32) for p in range(2)] for o in OFFSETS}
   c['expected_serve']=copy.deepcopy(s)
   for name,off in [('power_animations',0x1c8),('high_animations',0x1e0)]:c['expected_serve'][name]=[sx(e.r32(GAME+off+p*4),32) for p in range(2)]
   c['expected_serve']['frontend_flags']=list(map(bool,e.rd(0x71110000+0x48,2)))
   c['ai']=[dict(ball=e.r32(p+0x60),angle=e.r32(p+0x64),scale=e.r32(p+0x68),charge=e.r32(p+0x70),difficulty=list(e.rd(p+0x78,7)),enabled=bool(e.rd(p+0x7f,1)[0]),heading=e.r32(p+0x80),waiting=bool(e.rd(p+0x6c,1)[0])) for p in NEW_AI]
   cases.append(c)
 return dict(elf_sha256=SHA,corpus_sha256=hashes,handles=base['handles'],camera_globals=base['camera_globals'],cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
 print('Verified',len(value['cases']),'original combined runtime reset calls')
