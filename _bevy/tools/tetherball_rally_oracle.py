"""Original Return/Accelerate and their complete hit/animation/charge graph.

The only replaced routines are engine services. Game helpers execute from the
pinned ELF, including waiting swings, hit windows, human/AI attempts, charge,
zone selection, ball Hit/Miss, multiplier, indicator, winner and pause entry.
"""
import copy, hashlib, json, math, random, sys
from pathlib import Path
from tetherball_serve_oracle import ServeEmu, GAME, CHARS, AI, ANIM, OBJ, SHA, bits
from ppc_emu2 import sx, f32
import re_functions as rf

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_rally_golden.json'
WORD_OFFSETS=[0x44,0x234,0x238,0x248,0x24c,0x268,0x26c,0x274,0x27c,0x280,
              0x334,0x338,0x33c,0x35c,0x428,0x43c]+list(range(0x360,0x390,4))
ANIMATION_OFFSETS=[o for o in range(0x198,0x1f8,4) if o not in [0x1c8,0x1cc,0x1e0,0x1e4]]
BYTE_OFFSETS=[0x32c,0x32d,0x42d,0x42e]
FUNCTIONS={'return':(0x80398368,1820),'accelerate':(0x80398a84,1668),
           'hit':(0x8039a2c0,1548),'multiplier':(0x8039c220,500),'indicator':(0x8039c538,420)}
PARTICLE=0x71600000

class RallyEmu(ServeEmu):
    def step(self,pc):
        # Tail branches enter the service address without a link bit. The base
        # interpreter intercepts linked calls only; preserve the caller LR here.
        if pc in self.hooks:
            self.hooks[pc](self)
            return self.lr
        word=self.word(pc)
        if word>>26==59 and (word>>1)&31==18:
            d,a,b=(word>>21)&31,(word>>16)&31,(word>>11)&31
            if self.f[b]==0:
                assert self.f[a]!=0,'0/0 FPSCR/default-NaN behavior is outside this oracle model'
                self.f[d]=math.copysign(float('inf'),math.copysign(1.,self.f[a])*math.copysign(1.,self.f[b]));following=pc+4
            else:following=super().step(pc)
        else:following=super().step(pc)
        for name,(start,size) in FUNCTIONS.items():
            if start<=pc<start+size:
                self.coverage[name].add(pc)
                if word>>26==16:self.outcomes[name].add((pc,following))
        return following

    def __init__(self,exe):
        self.coverage={k:set() for k in FUNCTIONS};self.outcomes={k:set() for k in FUNCTIONS}
        super().__init__(exe)
        self.hooks[0x802f7bf4]=self.particle_lookup
        self.hooks[0x802f6c34]=lambda e:e.events.append(['particle_position',e.r[3],e.vector_bits(e.r[4],3)])
        self.hooks[0x802f6c00]=lambda e:e.events.append(['particle_scale',e.r[3],bits(e.f[1])])
        self.event_reads={}
        self.hooks[0x8032d9b0]=self.controller_event

    def controller_event(self,e):
        from tetherball_serve_oracle import EVENT
        controller=(e.r[3]-0x71200000)//256;action=e.r[4]
        supplied=e.input['events'].get(str(controller),{}).get(str(action),False)
        key=(controller,action);index=self.event_reads.get(key,0)
        if isinstance(supplied,list):
            assert supplied, 'empty controller read sequence'
            value=bool(supplied[min(index,len(supplied)-1)])
        else:value=bool(supplied)
        self.event_reads[key]=index+1
        e.events.append(['event',controller,action,value]);e.wr(EVENT,bytes([value]));e.r[3]=EVENT

    def particle_lookup(self,e):
        guid=e.r32(e.r[4]);e.events.append(['particle_lookup',guid,PARTICLE]);e.r[3]=PARTICLE

    def prepare(self,c):
        super().prepare(c)
        self.event_reads={}
        r=c['rally']
        for off in WORD_OFFSETS+ANIMATION_OFFSETS:self.w32(GAME+off,r['words'][hex(off)])
        for off in BYTE_OFFSETS:self.wr(GAME+off,bytes([r['bytes'][hex(off)]]))
        for i in range(2):
            for j,v in enumerate(r['positions'][i]):self.w32(CHARS[i]+0x180+j*4,v)
            self.w32(AI[i]+0x70,r['ai_charge'][i])
        self.w32(0x80601f60,r['invalid_guid'])
        for i,v in enumerate(r['controller_fx_offset']):self.w32(0x805e37d0+i*4,v)

    def aux(self,original):
        a=super().aux(original)
        a['round_tunables_34c_358']=[self.r32(GAME+0x34c+i*4) for i in range(4)]
        return a

    def read_rally(self,c):
        r=copy.deepcopy(c['rally'])
        r['words']={hex(o):self.r32(GAME+o) for o in WORD_OFFSETS+ANIMATION_OFFSETS}
        r['bytes']={hex(o):bool(self.rd(GAME+o,1)[0]) for o in BYTE_OFFSETS}
        r['ai_charge']=[self.r32(p+0x70) for p in AI]
        return r

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    em=RallyEmu(rf.load());rng=random.Random(0x80398368)
    seeds=json.loads((ROOT/'_bevy/tests/data/tetherball_serve_golden.json').read_text(encoding='utf-8'))['cases']
    cases=[]
    for i in range(240):
        seed=copy.deepcopy(seeds[i%len(seeds)])
        for k in ['expected','expected_ball','expected_aux','effects','returned']:seed.pop(k,None)
        state=seed['initial'];aux=seed['aux'];ball=seed['ball']
        state.update(paused=i%37==0,player_count=2,current_distance=i%3,focus_player=(i//2)%2,
                     latches=[bool(i%2),bool((i//2)%2)],action_states=[i%3,(i//2)%3])
        state['match_state'].update(rotations=[0,0],round_wins=[0,0],match_over=False)
        state['mega_values']=[i%7,(i+1)%7];state['mega_states']=[i%6,(i+1)%6]
        aux.update(word_224=[0,1,2,3,4,7][i%6],counter_260=[0,1,16,17,110,160,0xffffffff][i%7],
                   counter_270=i%5,field_229=bool(i%3==0),forced_ai=[bool(i%4==0),bool(i%5==0)])
        seed['input']['events']={str(p):{str(event):bool(rng.randrange(2)) for event in [0x5c,0x5d,0x5e,0x5f]} for p in range(4)}
        ball.update(angle=bits([0.,0.5,1.,1.5,2.,3.,5.,6.2][i%8]),angular_velocity=bits([-6.,-2.,2.,6.][i%4]),zone=i%2)
        words={hex(o):0 for o in WORD_OFFSETS+ANIMATION_OFFSETS}
        words.update({hex(o):70+j for j,o in enumerate(ANIMATION_OFFSETS)})
        words.update({'0x44':i%4,'0x234':(i%10-1)&0xffffffff,'0x238':i%4,
                      '0x248':bits(1.),'0x24c':bits(4.),'0x268':i%2,'0x26c':bits([1.,4.,20.,60.][i%4]),
                      '0x274':i%2,'0x27c':1,'0x280':0xffffffff,'0x334':0xffffffff if i%2 else 0x1234,
                      '0x338':bits(0.4),'0x33c':bits(0.002),'0x35c':bits([0.,0.1,0.3,1.][i%4]),
                      '0x428':bits(1.5),'0x43c':i%3})
        for off in range(0x360,0x390,4):words[hex(off)]=bits([0.25,0.4,0.6][(off//4)%3])
        seed['rally']=dict(words=words,bytes={hex(o):bool((i//(j+1))%2) for j,o in enumerate(BYTE_OFFSETS)},
                           positions=[[bits(1.),bits(2.),bits(3.)],[bits(-1.),bits(3.),bits(2.)]],
                           ai_charge=[23,24],invalid_guid=0xffffffff,controller_fx_offset=[bits(0.),bits(0.),bits(0.)])
        seed['randoms']=[rng.randrange(100) for _ in range(40)]
        for name in FUNCTIONS:
            c=copy.deepcopy(seed);c['command']=[name,i%2 if name=='multiplier' else int(i%2==0)]
            c['label']=f'{name}-{i}';c['initial']['match_state']['state_code']=28 if name=='return' else 29
            cases.append(c)
    def targeted(name,label):
        c=copy.deepcopy(cases[1])
        c.update(label='target-'+label,command=[name,0],milliseconds=16,randoms=[0,1]*20)
        s,a,r=c['initial'],c['aux'],c['rally'];w=r['words']
        s.update(paused=False,session_mode=2,game_type=0,variant=0,player_count=2,
                 server=0,receiver=1,focus_player=0,current_distance=0,distance_mode=0,
                 latches=[False,False],action_states=[0,2],mega_values=[5,5],mega_states=[1,1])
        s['match_state'].update(state_code=28 if name=='return' else 29,state_ms=0,
                               rotations=[0,0],round_wins=[0,0],elapsed_ms=0,match_over=False)
        for p in range(2):s['players'][p].update(controller=p,current_animation=58)
        a.update(word_224=0,counter_260=0,counter_264=0,counter_270=0,
                 field_229=False,forced_ai=[False,False],pause_menu_open=False,
                 pause_block_count_0fc=0,return_angles=[bits(0.),bits(3.)])
        c['ball'].update(angle=bits(1.1),zone=0,angular_velocity=bits(2.),direction=0,
                         radius=bits(1.),desired_radius=bits(1.),vertical_velocity=bits(0.))
        c['input']['events']={}
        w.update({'0x44':0,'0x234':0,'0x238':0,'0x248':bits(1.),'0x24c':bits(4.),
                  '0x27c':1,'0x280':0xffffffff,'0x26c':bits(4.),'0x274':0,
                  '0x334':0xffffffff,'0x338':bits(0.4),'0x33c':bits(0.002),
                  '0x35c':bits(0.1),'0x43c':0})
        for off in range(0x360,0x390,4):w[hex(off)]=bits(0.5)
        r['bytes']={hex(o):False for o in BYTE_OFFSETS}
        r['controller_fx_offset']=[bits(0.25),bits(-0.5),bits(0.75)]
        c['rules'].update(mode=0,rotation_limit=6,wins_required=2,time_limit_seconds=30000)
        cases.append(c)
        return c
    for name in ('return','accelerate'):
        # Human event priority crosses independent zone, charge, narrow window
        # and accumulated speed-gain dimensions instead of correlated seeds.
        for zone in (0,1):
          for power in range(4):
           for charge in (0,1,4,5,6):
            for narrow in (False,True):
             for gain in (0.1,0.3,0.5):
                c=targeted(name,f'{name}-human-z{zone}-p{power}-c{charge}-n{narrow}-g{gain}')
                c['ball']['zone']=zone;c['initial']['mega_values'][0]=charge
                c['input']['events']={'0':{'92':power in (1,3),'93':power in (2,3)}}
                c['rally']['bytes']['0x42e']=True
                c['rally']['words']['0x35c']=bits(gain)
                if not narrow:
                    c['ball']['angle']=bits(1.4)
                    for off in (0x360,0x378):c['rally']['words'][hex(off)]=bits(0.1)
        for ai in (-1,0,1,2,3,4,7,8):
          for charge in (0,1,5):
           for forced in (False,True):
                c=targeted(name,f'{name}-ai-selector{ai}-c{charge}-forced{forced}')
                c['initial']['mega_values'][0]=charge
                if not forced:c['initial']['players'][0]['controller']=None
                c['aux']['forced_ai'][0]=forced
                c['rally']['words'].update({'0x234':ai&0xffffffff,'0x238':7})
        for latch in ((False,False),(True,False),(False,True),(True,True)):
          for timer in (0,1,16,17,0xffffffff):
           for mega in (False,True):
                c=targeted(name,f'{name}-delay-{latch}-{timer}-mega{mega}')
                c['initial']['latches']=list(latch)
                c['aux'].update(counter_260=timer,field_229=True)
                c['rally']['bytes']['0x32d']=mega
                c['initial']['players'][0]['controller']=0 if mega else None
        for block in (-1,0,1):
          for opened in (False,True):
           for pause in (False,True):
                c=targeted(name,f'{name}-pause-{block}-{opened}-{pause}')
                c['initial']['paused']=pause;c['aux'].update(pause_block_count_0fc=block,pause_menu_open=opened)
                c['input']['events']={'0':{'175':True},'1':{'175':True}}
        for server in (0,1):
          for angle in (0.,0.5,1.1,3.,4.,6.2):
           for latch in ((False,False),(True,False),(False,True)):
                c=targeted(name,f'{name}-transition-s{server}-a{angle}-l{latch}')
                c['initial'].update(server=server,receiver=1-server,focus_player=server,latches=list(latch))
                c['aux'].update(field_229=True,counter_260=0)
                c['ball']['angle']=bits(angle)
        for first in (False,True):
          for second in (False,True):
           for third in (False,True):
            c=targeted(name,f'{name}-repeated-events-{first}-{second}-{third}')
            c['input']['events']={'0':{'92':[first,second],'93':[first,third]}}
            c['rally']['bytes']['0x42e']=True
    for kind in (-1,0,1,2,3,4,7,8):
      for human in (False,True):
       for mode in range(5):
        c=targeted('hit',f'hit-kind{kind}-human{human}-mode{mode}')
        c['aux']['word_224']=kind;c['rally']['words']['0x44']=mode
        c['initial']['players'][0]['controller']=0 if human else None
        c['rally']['words']['0x274']=0
    for direction in (0,1):
        c=targeted('return',f'return-schedule-direction{direction}')
        c['ball']['direction']=direction
    for value in (-1,0,1,2,3,4,5,6):
      for player in (0,1):
       for human in (False,True):
        c=targeted('multiplier',f'multiplier-v{value}-p{player}-human{human}')
        c['command'][1]=player;c['initial']['mega_states'][player]=value
        c['initial']['players'][0]['controller']=0 if human else None
    # A stationary ball produces an infinite time-to-window and zero scale
    # rate. Keep both signs of zero, including the native -0 >= 0 branch.
    for zero in (0.,-0.):
      for state in (28,29):
        c=targeted('indicator',f'indicator-stationary-{bits(zero):08x}-state{state}')
        c['command'][1]=1
        c['initial']['match_state']['state_code']=state
        c['ball']['angular_velocity']=bits(zero)
        c['rally']['words']['0x334']=c['rally']['invalid_guid']
    for c in cases:
        em.prepare(c);name,arg=c['command'];addr=FUNCTIONS[name][0]
        args=(GAME,c['milliseconds'],arg) if name=='indicator' else (GAME,arg if name=='multiplier' else c['milliseconds'])
        returned=em.call(addr,args)
        c['expected'],c['expected_ball']=em.read(c['initial']);c['expected_aux']=em.aux(c['aux'])
        c['expected_rally']=em.read_rally(c);c['effects']=copy.deepcopy(em.events)
        c['returned']=returned if name in ['return','accelerate'] else None
    coverage={name:dict(instructions=len(em.coverage[name]),total=size//4,
                       missing=[hex(pc) for pc in range(start,start+size,4) if pc not in em.coverage[name]],
                       branches=[[hex(pc),hex(target)] for pc,target in sorted(em.outcomes[name])])
              for name,(start,size) in FUNCTIONS.items()}
    for name,(start,size) in FUNCTIONS.items():
        assert not coverage[name]['missing'], (name,coverage[name]['missing'])
        for pc in range(start,start+size,4):
            if em.word(pc)>>26==16:
                assert sum(p==pc for p,target in em.outcomes[name])==2, (name,hex(pc),'conditional outcome missing')
    return dict(elf_sha256=SHA,coverage=coverage,cases=cases)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Rally graph differs from original PowerPC'
        print('Verified',len(value['cases']),'original tetherball rally/hit/indicator calls')
    else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n');print('Wrote',OUT)
