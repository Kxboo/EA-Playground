"""Original frontend handlers and base close helpers; full reset is supplied boundary."""
import copy,hashlib,json,sys
from pathlib import Path
import re_functions as rf
from tetherball_lifecycle_oracle import LifecycleEmu,GAME,SHA
from ppc_emu2 import sx
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_frontend_golden.json'
FE=0x71110000;VT=0x71900000
FUNCS={'pregame':0x80397988,'start_complete':0x803978a8,'hud_complete':0x803978b4,'pause_reset':0x80397904,'hud_init':0x8039c1d8,'postgame':0x80399b60,'wait':0x80399be0}
class Emu(LifecycleEmu):
 def __init__(self,exe):
  super().__init__(exe)
  self.hooks[0x8031f294]=lambda e:e.r.__setitem__(3,0x71901000)
  self.hooks[0x8031e0d0]=lambda e:e.r.__setitem__(3,0x71902000)
  for address,name,indices in [(0x8031f2a4,'clear_pregame',[]),(0x8031e0e0,'clear_postgame',[]),(0x803e0174,'fade_out',[4]),(0x803e022c,'fade_renders',[4,5]),(0x803176ec,'setup',[4]),(0x80327178,'close_overlay',[]),(0x802e18a4,'audio_unpause',[]),(0x80317ca8,'timer_visible',[3])]:
   self.hooks[address]=lambda e,name=name,indices=indices:e.events.append([name]+[sx(e.r[i],32) for i in indices])
  self.hooks[0x803270e8]=lambda e:e.events.append(['open_screen',e.cstring(e.r[4])])
  self.hooks[0x71903000]=lambda e:e.events.append(['reset_minigame'])
  self.hooks[0x803e0238]=lambda e:(e.events.append(['fade_complete',self.fade_complete]),e.r.__setitem__(3,int(self.fade_complete)))
 def front(self):
  return dict(pregame_ready_058=self.rd(GAME+0x58,1)[0],postgame_choice_05c=sx(self.r32(GAME+0x5c),32),field_424=self.rd(GAME+0x424,1)[0],pause_menu_flag_04e=self.rd(GAME+0x4e,1)[0],pause_delay_ms_0fc=sx(self.r32(GAME+0xfc),32),frontend_flags=list(self.rd(FE+0x48,2)))
def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 seeds=json.loads((ROOT/'_bevy/tests/data/tetherball_lifecycle_golden.json').read_text(encoding='utf-8'))['cases'];e=Emu(rf.load());cases=[]
 for i in range(112):
  seed=seeds[i];initial=copy.deepcopy(seed['initial']);ball=copy.deepcopy(seed['ball']);rules=seed['rules'];operation=list(FUNCS)[i%7]
  front=dict(pregame_ready_058=[0,1,2,255][i%4],postgame_choice_05c=[-2147483648,-1,0,1,2,2147483647][i%6],field_424=173,pause_menu_flag_04e=i%2,pause_delay_ms_0fc=-987,frontend_flags=[i%2,(i//2)%2]);fade_complete=bool((i//7)%2)
  e.write(initial,rules,ball);e.w32(GAME,VT);e.w32(VT+0x30,0x71903000)
  for key,off in [('pregame_ready_058',0x58),('field_424',0x424),('pause_menu_flag_04e',0x4e)]:e.wr(GAME+off,bytes([front[key]]))
  for key,off in [('postgame_choice_05c',0x5c),('pause_delay_ms_0fc',0xfc)]:e.w32(GAME+off,front[key])
  e.wr(FE+0x48,bytes(front['frontend_flags']));e.fade_complete=fade_complete
  result=e.call(FUNCS[operation],[GAME,i-56]);expected,expected_ball=e.read(initial)
  cases.append(dict(operation=operation,initial=initial,ball=ball,rules=rules,front=front,fade_complete=fade_complete,expected=expected,expected_ball=expected_ball,expected_front=e.front(),effects=copy.deepcopy(e.events),result=result if operation in ('pregame','postgame','wait') else None))
 return dict(elf_sha256=SHA,cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
 print('Verified',len(value['cases']),'original frontend calls')
