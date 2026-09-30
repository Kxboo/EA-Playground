"""Execute original UpdateServe and its ball, angle, state-entry and pause graph.

Only controller/device, animation, sound, frontend, camera and particle services
are supplied. No serve predicate, delay, angle crossing or winner decision is
replaced with a Python result.
"""
import copy, hashlib, json, random, sys
from pathlib import Path
from tetherball_lifecycle_oracle import LifecycleEmu, GAME, CHARS, AI, ANIM, OBJ, SHA, bits
from ppc_emu2 import sx
import re_functions as rf
from tetherball_serve_cases import extend_cases

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_serve_golden.json'
FE = 0x71110000
EVENT = 0x71500000
NAMES = 0x71501000
WORDS = {'base_player_count_070':0x70, 'word_224':0x224,
         'counter_260':0x260, 'counter_264':0x264, 'counter_270':0x270,
         'pause_block_count_0fc':0xfc}
BYTES = {'field_25c':0x25c, 'field_229':0x229, 'pause_menu_open':0x4e,
         'power_serve_enabled':0x42c}

class ServeEmu(LifecycleEmu):
    def step(self,pc):
        following=super().step(pc)
        if 0x80397b74<=pc<0x80398368:
            self.executed.add(pc)
            if self.word(pc)>>26==16:self.branches.add((pc,following))
        return following

    def __init__(self, exe):
        self.executed=set();self.branches=set()
        super().__init__(exe)
        self.input = {}
        self.hooks[0x80317ca8] = lambda e:e.events.append(['timer_visible',sx(e.r[3],32)])
        def azimuth(e):
            p=CHARS.index(e.r[3]-0x180);value=e.input['azimuth'][p]
            e.events.append(['azimuth',p,value]);e.r[3]=value&0xffffffff
        self.hooks[0x802e2108]=azimuth
        self.hooks[0x802e1bc4]=lambda e:e.events.append(['sound_wiimote',sx(e.r[4],32),sx(e.r[5],32),sx(e.r[6],32)])
        self.hooks[0x803be560]=lambda e:e.events.append(['shake',sx(e.r[4],32),bits(e.f[1])])
        self.hooks[0x8032e2f4]=lambda e:e.events.append(['rumble',(e.r[3]-0x71200000)//256,e.r[4],bits(e.f[1])])
        def event(e):
            controller=(e.r[3]-0x71200000)//256;action=e.r[4]
            value=bool(e.input['events'].get(str(controller),{}).get(str(action),False))
            e.events.append(['event',controller,action,value])
            e.wr(EVENT,bytes([value]));e.r[3]=EVENT
        self.hooks[0x8032d9b0]=event
        self.hooks[0x8031f294]=lambda e:e.r.__setitem__(3,0x71503000)
        self.hooks[0x8031f29c]=lambda e:e.events.append(['pregame',e.r[4],e.r[5],e.vector_bits(e.r[6],4)])
        self.hooks[0x80327158]=lambda e:e.events.append(['overlay',e.cstring(e.r[4])])
        self.hooks[0x802e1858]=lambda e:e.events.append(['audio_pause',e.r[4]])

    def prepare(self,c):
        self.input=c['input'];self.randoms=c['randoms'];self.write(c['initial'],c['rules'],c['ball'])
        a=c['aux'];self.wr(FE,bytes(0x60));self.wr(FE+0x48,bytes(a['frontend_flags']))
        for name,off in WORDS.items():self.w32(GAME+off,a[name])
        for name,off in BYTES.items():self.wr(GAME+off,bytes([a[name]]))
        for i in range(2):
            self.w32(GAME+0x23c+i*4,a['return_angles'][i])
            self.w32(GAME+0x1c8+i*4,a['power_animations'][i])
            self.w32(GAME+0x1e0+i*4,a['high_animations'][i])
            self.w32(CHARS[i]+0x1e8,a['voice_types'][i])
            self.wr(AI[i]+0x6c,bytes([a['ai_waiting'][i]]))
            self.wr(0x80607e78+i,bytes([a['forced_ai'][i]]))
            for power in range(2):
                pointer=NAMES+(power*2+i)*64
                self.wr(pointer,f'serve_fx_{power}_{i}'.encode()+b'\0')
                self.w32(GAME+0x2fc+power*8+i*4,pointer)
        for i,v in enumerate(a['ball_position']):self.w32(OBJ+0x40+i*4,v)
        for i,v in enumerate(a['round_tunables_34c_358']):self.w32(GAME+0x34c+i*4,v)

    def aux(self,original):
        a=copy.deepcopy(original)
        for name,off in WORDS.items():a[name]=self.r32(GAME+off) if name.startswith('counter_') else sx(self.r32(GAME+off),32)
        for name,off in BYTES.items():a[name]=bool(self.rd(GAME+off,1)[0])
        a['ai_waiting']=[bool(self.rd(p+0x6c,1)[0]) for p in AI]
        a['frontend_flags']=list(self.rd(FE+0x48,2))
        return a

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    seeds=json.loads((ROOT/'_bevy/tests/data/tetherball_lifecycle_golden.json').read_text(encoding='utf-8'))['cases']
    em=ServeEmu(rf.load());rng=random.Random(0x80397b74);cases=[]
    # Deliberately cross timer equality, animation exclusion, human/AI, serve
    # quality and angle-direction boundaries. Subsequent cases target each gate.
    for i in range(320):
        seed=seeds[i%len(seeds)];state=copy.deepcopy(seed['initial']);ball=copy.deepcopy(seed['ball'])
        state.update(paused=i%29==0,session_mode=i%4,player_count=2,server=i%2,receiver=1-i%2,
                     focus_player=i%2,round_timer_ms=rng.choice([-1,0,1,16,17,2000]),
                     latches=[bool(i%2),False],distance_mode=i%5,current_distance=i%3)
        state['match_state'].update(state_code=27,state_ms=[0,2000,2001,2800,2801,0xffffffff][i%6],
                                    elapsed_ms=100,match_over=False,rotations=[0,0],round_wins=[0,0])
        state['action_states']=[i%3,(i+1)%3]
        for p in range(2):
            state['players'][p].update(controller=[None,p,p+2][(i//2)%3],current_animation=[58,59,60,81,91,92][(i//3+p)%6])
        ball.update(tossed=bool(i%2),height=bits([0.4,0.7,1.2][i%3]),vertical_velocity=bits([-1.,0.,0.53,2.][(i//3)%4]),
                    angle=bits([0.,0.5,1.,2.,3.,5.,6.2,7.][(i//2)%8]))
        a=dict(base_player_count_070=[0,1,2,-1][i%4],word_224=[-1,5,6,7][(i//4)%4],
               counter_260=[0,1,16,17,110,0xffffffff][(i//2)%6],counter_264=[0,1,16,500,0xffffffff][i%5],counter_270=99,
               pause_block_count_0fc=[-1,0,1][i%3],field_25c=False,field_229=True,pause_menu_open=bool(i%7==0),
               power_serve_enabled=bool(i%3),return_angles=[bits(1.),bits(2.)],power_animations=[91,92],high_animations=[93,94],
               voice_types=[i%2,(i+1)%2],ai_waiting=[True,True],forced_ai=[bool(i%5==0),bool(i%7==0)],
               frontend_flags=[0,0],ball_position=[bits(1.),bits(2.),bits(3.)],round_tunables_34c_358=[bits(4.),bits(1.2),bits(1.5),bits(2.)])
        inputs=dict(azimuth=[-37,125],events={str(p):{'92':bool((i+p)%2),'175':bool((i+p)%11==0)} for p in range(4)})
        rules=copy.deepcopy(seed['rules']);rules.update(rotation_limit=6,wins_required=2,time_limit_seconds=30000)
        cases.append(dict(label=f'matrix-{i}',initial=state,ball=ball,aux=a,input=inputs,rules=rules,randoms=[1,2,3,4],milliseconds=[0,1,16,110,-1][i%5]))
    # Straightforward reachable delayed-strike cases (both players, human/AI,
    # strike kinds, tossed/missed, animation exclusions and timer boundaries).
    for p in range(2):
      for kind in [-1,5,6,7]:
       for human in [False,True]:
        for tossed in [False,True]:
         for timer in [0,1,16,17]:
            c=copy.deepcopy(cases[1]);c['label']=f'strike-{p}-{kind}-{human}-{tossed}-{timer}'
            s=c['initial'];s.update(paused=False,server=p,receiver=1-p,focus_player=p,session_mode=2,latches=[True,False],round_timer_ms=16)
            s['match_state'].update(state_ms=0);s['players'][p].update(controller=p if human else None,current_animation=60)
            c['aux'].update(word_224=kind,counter_260=timer,forced_ai=[False,False],pause_menu_open=False,pause_block_count_0fc=0)
            c['ball'].update(tossed=tossed,angle=bits(0.));c['milliseconds']=16;c['input']['events']={}
            cases.append(c)
    extend_cases(cases,bits)
    c=copy.deepcopy(next(c for c in cases if c['label']=='strike-0-5-True-False-1'))
    c['label']='missed-strike-bubble-already-visible'
    c['initial']['serve_bubble_visible']=True
    cases.append(c)
    for player in range(2):
      for time in [2001,2801]:
        c=copy.deepcopy(next(c for c in cases if c['label'].startswith('target-ai-edge-2001-')))
        c['label']=f'ai-active-differs-from-focus-{player}-{time}'
        c['initial'].update(server=player,receiver=1-player,focus_player=1-player,action_states=[0,1])
        c['initial']['match_state']['state_ms']=time
        cases.append(c)
    for c in cases:
        em.prepare(c);returned=em.call(0x80397b74,(GAME,c['milliseconds']))
        c['expected'],c['expected_ball']=em.read(c['initial']);c['expected_aux']=em.aux(c['aux'])
        c['effects']=copy.deepcopy(em.events);c['returned']=returned
    selectors=[]
    for selected in [-2147483648,-1,0,1,2,17,2147483647]:
      for receiver in [-1,0,1]:
        em.w32(GAME+0x214,17);em.w32(GAME+0x218,receiver)
        em.call(0x8039a91c,(GAME,selected))
        selectors.append(dict(initial=[17,receiver],selected=selected,
                              expected=[sx(em.r32(GAME+0x214),32),sx(em.r32(GAME+0x218),32)]))
    return dict(elf_sha256=SHA,selectors=selectors,instruction_count=len(em.executed),
                missing_instructions=[hex(pc) for pc in range(0x80397b74,0x80398368,4) if pc not in em.executed],
                branch_outcomes=[[hex(pc),hex(target)] for pc,target in sorted(em.branches)],cases=cases)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Serve graph differs from original PowerPC'
        print('Verified',len(value['cases']),'complete tetherball serve calls')
    else:
        OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n');print('Wrote',OUT)
