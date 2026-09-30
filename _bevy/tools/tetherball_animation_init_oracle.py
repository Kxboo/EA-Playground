"""Execute complete original InitializePlayerAnimations; no body/service hooks."""
import copy, hashlib, json, random, sys
from pathlib import Path
from tetherball_serve_oracle import ServeEmu, GAME, SHA
from ppc_emu2 import sx
import re_functions as rf

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_animation_init_golden.json'
ENTRY=0x8039be84
SIZE=504
OFFSETS=[0x198,0x1a0,0x1a8,0x1b0,0x1b8,0x1c0,0x1d0,0x1d8,0x1e8,0x1f0]

class AnimationEmu(ServeEmu):
    def __init__(self,e):
        self.instructions=set();self.branches=set()
        super().__init__(e)
    def step(self,pc):
        next_pc=super().step(pc)
        if ENTRY<=pc<ENTRY+SIZE:
            self.instructions.add(pc)
            if self.word(pc)>>26==16:self.branches.add((pc,next_pc))
        return next_pc

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    seeds=json.loads((ROOT/'_bevy/tests/data/tetherball_serve_golden.json').read_text(encoding='utf-8'))['cases']
    e=AnimationEmu(rf.load());rng=random.Random(ENTRY);cases=[]
    for count in (0,1,2):
      for base_mask in range(4):
       for side_mask in range(4):
        source=seeds[(len(cases)*7+1)%len(seeds)]
        c={k:copy.deepcopy(source[k]) for k in ('initial','ball','rules','aux','input','randoms')}
        c['label']=f'count-{count}-base-{base_mask}-side-{side_mask}'
        c['initial']['player_count']=count
        for p in range(2):c['initial']['players'][p]['player_flag']=bool(base_mask&(1<<p))
        c['side_flags']=[bool(rng.randrange(2)),bool(rng.randrange(2)),bool(side_mask&1),bool(side_mask&2)]
        c['words']={hex(o):[sx(rng.getrandbits(32),32) for _ in range(2)] for o in OFFSETS}
        # Distinct seeds expose unprocessed-player and unrelated-table stores.
        c['initial']['lose_animations']=[sx(rng.getrandbits(32),32) for _ in range(2)]
        c['initial']['win_animations']=[sx(rng.getrandbits(32),32) for _ in range(2)]
        c['aux']['power_animations']=[sx(rng.getrandbits(32),32) for _ in range(2)]
        c['aux']['high_animations']=[sx(rng.getrandbits(32),32) for _ in range(2)]
        e.prepare(c)
        for p in range(2):e.wr(GAME+0x244+p,bytes([c['side_flags'][p]]));e.wr(GAME+0x32a+p,bytes([c['side_flags'][2+p]]))
        for o in OFFSETS:
            for p in range(2):e.w32(GAME+o+p*4,c['words'][hex(o)][p])
        e.call(ENTRY,(GAME,))
        c['expected'],c['expected_ball']=e.read(c['initial']);c['expected_aux']=e.aux(c['aux'])
        for name,off in [('power_animations',0x1c8),('high_animations',0x1e0)]:
            c['expected_aux'][name]=[sx(e.r32(GAME+off+p*4),32) for p in range(2)]
        c['expected_words']={hex(o):[sx(e.r32(GAME+o+p*4),32) for p in range(2)] for o in OFFSETS}
        c['expected_side_flags']=[bool(e.rd(GAME+o,1)[0]) for o in (0x244,0x245,0x32a,0x32b)]
        assert c['side_flags']==c['expected_side_flags']
        c['effects']=copy.deepcopy(e.events)
        assert not c['effects'], 'initializer unexpectedly called an engine service'
        cases.append(c)
    assert len(e.instructions)==SIZE//4, 'initializer instruction coverage incomplete'
    for pc in range(ENTRY,ENTRY+SIZE,4):
        if e.word(pc)>>26==16:assert sum(p==pc for p,target in e.branches)==2,(hex(pc),'branch outcome missing')
    return dict(elf_sha256=SHA,coverage=dict(instructions=len(e.instructions),total=SIZE//4,
                branches=[[hex(p),hex(n)] for p,n in sorted(e.branches)]),cases=cases)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Animation initialization differs from original PPC'
        print('Verified',len(value['cases']),'original animation initialization calls')
    else:
        OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
        print('Wrote',len(value['cases']),'original animation initialization calls')
