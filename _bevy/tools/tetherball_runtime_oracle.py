"""Full MGTetherball frames: gesture queue -> ball scene -> real state handler.

Only external engine services and the World update/indicator boundary
are replaced. Reset composition has its own oracle; these frames never reset.
"""
import copy, hashlib, json, sys
from pathlib import Path
import re_functions as rf
from ppc_emu2 import sx
from tetherball_rally_oracle import RallyEmu, GAME, OBJ, SHA, bits
from tetherball_scene_oracle import OWNER, LOCAL, AREA, IDENTITY
from tetherball_frontend_oracle import Emu as FrontendEmu

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_runtime_golden.json'

class FrameEmu(RallyEmu):
    def __init__(self,exe):
        super().__init__(exe)
        front=FrontendEmu(exe)
        for address in (0x8031f294,0x8031e0d0,0x8031f2a4,0x8031e0e0,0x803e0174,
                        0x803e022c,0x803176ec,0x80327178,0x802e18a4,0x803270e8):
            self.hooks[address]=front.hooks[address]
        self.hooks[0x803e0238]=lambda e:(e.events.append(['fade_complete',e.fade_complete]),e.r.__setitem__(3,int(e.fade_complete)))
        self.hooks[0x8039e354]=lambda e:e.events.append(['shadow',e.r[3]==0x71005100,e.vector_bits(e.r[4],16)])
        self.hooks[0x8039cab4]=lambda e:e.events.append(['indicator',bits(e.f[1])])
        self.hooks[0x803e15f8]=lambda e:e.events.append(['world_update',sx(e.r[4],32)])
        def unexpected_reset(e): raise AssertionError('reset belongs to separate composition oracle')
        self.hooks[0x803994f4]=unexpected_reset

    def scene(self):
        return dict(position=self.vector_bits(OBJ+0x40,3),ball_matrix=self.vector_bits(OBJ+0xc0,16),
                    rope_matrix=self.vector_bits(OBJ+0x100,16),trails=self.vector_bits(OBJ+0x140,3))

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    seeds=json.loads((ROOT/'_bevy/tests/data/tetherball_rally_golden.json').read_text(encoding='utf-8'))['cases']
    em=FrameEmu(rf.load());cases=[]
    for i in range(180):
        c=copy.deepcopy(seeds[(i*35)%1200]);code=[1,3,8,9,26,27,28,29,30][i%9]
        for key in ('expected','expected_ball','expected_aux','expected_rally','effects','returned'):c.pop(key,None)
        c.update(label=f'frame-{i}-state-{code}',command=['frame',0],milliseconds=[0,1,16,60,-1][(i//9)%5],world_paused=bool(i%4))
        c['aux']['pause_block_count_0fc']=[-1,0,1,17,1200][i%5]
        s=c['initial'];s.update(paused=bool((i//9)%2),focus_player=i%2,player_count=2)
        s['match_state'].update(state_code=code,state_ms=[0,999,1000,1001][(i//18)%4],round_winner=i%2)
        c['ball'].update(grabbed=bool((i//18)%2),radius=bits(1.),desired_radius=bits(1.),height=bits(1.),angle=bits(0.5))
        # Negative frame deltas must not wrap round-end into the reset domain.
        if code==30:s['match_state']['state_ms']=250
        scene=dict(anchor=[bits(x) for x in (2.,3.,4.)],world=[bits(x) for x in IDENTITY],
                   local=None,shadows=[bool(i%2),bool(i%3)],trails=[0xffffffff,123,456],
                   area_radius=bits(250.),disabled=bool(i%5),
                   position=c['aux']['ball_position'],ball_matrix=[bits(x) for x in IDENTITY],rope_matrix=[bits(x) for x in IDENTITY])
        scene['world'][12:15]=[bits(3.),bits(1.),bits(5.)]
        c['scene']=scene
        c['front']=dict(pregame_ready_058=i%2,postgame_choice_05c=1 if (i//9)%2 else -1,field_424=1)
        c['intro']=dict(initial_receiver=1-i%2,field_25c=c['aux']['field_25c'],scoreboard_needs_reset=bool(i%3))
        c['queue']=[dict(kind=k,auxiliary=bits(0.),controller_id=p) for k,p in [(1,0),(0,1),(2,0),(0,0)][:i%5]]
        c['input']['fade_complete']=bool((i//9)%2)
        em.prepare(c);em.fade_complete=c['input']['fade_complete']
        for key,off in [('pregame_ready_058',0x58),('field_424',0x424)]:em.wr(GAME+off,bytes([c['front'][key]]))
        em.w32(GAME+0x5c,c['front']['postgame_choice_05c'])
        em.w32(GAME+0x220,c['intro']['initial_receiver']);em.wr(GAME+0x444,bytes([c['intro']['scoreboard_needs_reset']]))
        em.w32(GAME+0x2e4,len(c['queue']))
        for j,q in enumerate(c['queue']):
            for k,key in enumerate(('kind','auxiliary','controller_id')):em.w32(GAME+0x284+j*12+k*4,q[key])
        for off,key in [(0x60,'anchor'),(0xc0,'ball_matrix'),(0x100,'rope_matrix'),(0x140,'trails')]:
            for j,v in enumerate(scene[key]):em.w32(OBJ+off+j*4,v)
        em.w32(OBJ+0x74,OWNER);em.w32(OBJ+0x78,0)
        for j,v in enumerate(scene['world']):em.w32(OWNER+0x20+j*4,v)
        em.w32(OBJ+4,0x71005000 if scene['shadows'][0] else 0);em.w32(OBJ+8,0x71005100 if scene['shadows'][1] else 0)
        em.w32(0x805e8320+0x8c,0x71006000);em.w32(0x71006008,AREA);em.w32(0x80601f78,0xffffffff)
        em.wr(0x71006024,bytes([c['world_paused']]))
        em.area(250.,scene['disabled'])
        c['returned']=em.call(0x80397640,(GAME,c['milliseconds']))
        c['expected'],c['expected_ball']=em.read(c['initial']);c['expected_aux']=em.aux(c['aux'])
        c['expected_scene']=em.scene();c['expected_aux']['ball_position']=c['expected_scene']['position']
        c['expected_rally']=em.read_rally(c);c['effects']=copy.deepcopy(em.events)
        c['expected_front']=dict(pregame_ready_058=em.rd(GAME+0x58,1)[0],postgame_choice_05c=sx(em.r32(GAME+0x5c),32),field_424=em.rd(GAME+0x424,1)[0])
        c['expected_intro']=dict(initial_receiver=em.r32(GAME+0x220),field_25c=bool(em.rd(GAME+0x25c,1)[0]),scoreboard_needs_reset=bool(em.rd(GAME+0x444,1)[0]))
        c['expected_queue_count']=em.r32(GAME+0x2e4)
        cases.append(c)
    return dict(elf_sha256=SHA,cases=cases)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
    else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
    print('Verified',len(value['cases']),'complete original tetherball frames')
