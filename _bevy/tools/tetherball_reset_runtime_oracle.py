"""Original ResetRound with unhooked InitializePlayerAnimations dependency."""
import copy, hashlib, json, random, sys
from pathlib import Path
from tetherball_reset_oracle import ResetEmu, GAME, SHA, bits
from tetherball_animation_init_oracle import OFFSETS
from ppc_emu2 import sx
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_reset_runtime_golden.json'

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    base=json.loads((ROOT/'_bevy/tests/data/tetherball_reset_golden.json').read_text(encoding='utf-8'))
    em=ResetEmu(rf.load());del em.hooks[0x8039be84]
    rng=random.Random(0x803994f4+0x8039be84);cases=[]
    for seed in [c for c in base['cases'] if c['operation']=='round']:
      for count in (0,1,2):
       for flags in range(4):
        c=copy.deepcopy(seed)
        for key in ('expected','expected_ball','expected_aux','effects'):c.pop(key)
        c['label']=f'round-{len(cases)//12}-count-{count}-base-{flags}'
        c['initial']['player_count']=count
        for p in range(2):c['initial']['players'][p]['player_flag']=bool(flags&(1<<p))
        c['initial_serve']=dict(pause_block_count_0fc=-1,pause_menu_open=False,power_serve_enabled=True,
          return_angles=[bits(1.),bits(2.)],power_animations=[sx(rng.getrandbits(32),32) for _ in range(2)],
          high_animations=[sx(rng.getrandbits(32),32) for _ in range(2)],voice_types=[0,1],
          ai_waiting=[True,False],forced_ai=[False,c['inputs']['ai_enabled']],frontend_flags=[False,True])
        c['words']={hex(o):[sx(rng.getrandbits(32),32) for _ in range(2)] for o in OFFSETS}
        em.write_reset(c['initial'],dict(mode=0,rotation_limit=6,wins_required=2,time_limit_seconds=30000),c['ball'],c['aux'],c['inputs'])
        s=c['initial_serve'];em.w32(GAME+0xfc,s['pause_block_count_0fc']);em.wr(GAME+0x4e,bytes([s['pause_menu_open']]));em.wr(GAME+0x42c,bytes([s['power_serve_enabled']]))
        for p in range(2):
            for name,off in [('return_angles',0x23c),('power_animations',0x1c8),('high_animations',0x1e0)]:em.w32(GAME+off+p*4,s[name][p])
            em.wr(0x80607e78+p,bytes([s['forced_ai'][p]]))
        for off in OFFSETS:
            for p in range(2):em.w32(GAME+off+p*4,c['words'][hex(off)][p])
        em.call(0x803994f4,(GAME,),max_steps=30000)
        c['expected'],c['expected_ball'],c['expected_aux']=em.read_reset(c['initial'])
        c['expected_serve']=copy.deepcopy(s)
        for name,off in [('power_animations',0x1c8),('high_animations',0x1e0)]:c['expected_serve'][name]=[sx(em.r32(GAME+off+p*4),32) for p in range(2)]
        c['expected_words']={hex(o):[sx(em.r32(GAME+o+p*4),32) for p in range(2)] for o in OFFSETS}
        c['effects']=copy.deepcopy(em.events)
        assert not any(e[0]=='initialize_animations' for e in c['effects'])
        cases.append(c)
    return dict(elf_sha256=SHA,handles=base['handles'],camera_globals=base['camera_globals'],cases=cases)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Reset animation composition differs from original PPC'
        print('Verified',len(value['cases']),'original composed ResetRound calls')
    else:
        OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
        print('Wrote',len(value['cases']),'original composed ResetRound calls')
