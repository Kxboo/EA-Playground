"""Execute original rally rule bodies; hook only RNG, HUD/audio/camera services."""
import copy, hashlib, json, sys
from pathlib import Path
from tetherball_lifecycle_oracle import LifecycleEmu, GAME, AI, SHA
from ppc_emu2 import sx
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_rally_rules_golden.json'
REFS = 0x71600000
ADDRESSES = dict(attempt=0x8039b894, power=0x8039b968, zone=0x80399c1c,
                 consume=0x8039c4c0, increment=0x8039c414, drop=0x8039b9ac)

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    seed = json.loads((ROOT/'_bevy/tests/data/tetherball_lifecycle_golden.json').read_text(encoding='utf-8'))['cases'][1]
    em = LifecycleEmu(rf.load())
    cases = []
    def add(operation, label, **kwargs):
        c = dict(operation=operation, label=label, initial=copy.deepcopy(seed['initial']),
                 ball=copy.deepcopy(seed['ball']), rules=copy.deepcopy(seed['rules']),
                 state=dict(ai_hit_attempt_234=-1, ai_power_hit_type_238=0, ai_charge=[17,23]),
                 attempt=dict(strike=False, reverse_strike=False, power_type=99),
                 word_224=-123, counter_270=0, hit_attempt_marker=False, argument=0, player=0, randoms=[0])
        c.update(kwargs)
        c['initial'].update(server=0, receiver=1, focus_player=0, current_distance=0)
        cases.append(c)
        return c
    for raw in [-0x80000000,-2,-1,0,1,2,3,4,5,6,7,8,0x7fffffff]:
        for power in [-1,0,1,2,3,4,7,8]:
            for flags in [(False,False),(True,True)]:
                c=add('attempt',f'attempt-{raw}-{power}-{flags}')
                c['state'].update(ai_hit_attempt_234=raw,ai_power_hit_type_238=power)
                c['attempt'].update(strike=flags[0],reverse_strike=flags[1])
                c['hit_attempt_marker']=flags[0]
    for raw in [-0x80000000,-1,0,1,2,3,4,6,7,8,0x7fffffff]:
        c=add('power',f'power-{raw}');c['state']['ai_power_hit_type_238']=raw
    for zone in (0,1):
        for hit in (0,1,2,3,7):
            for random in (0,1):
                c=add('zone',f'zone-{zone}-{hit}-{random}',argument=hit,randoms=[random]);c['ball']['zone']=zone
    for player in (0,1):
        for charge in [0,1,4,5,6,0x7fffffff,0x80000000,0xffffffff]:
            for amount in [0,1,4,5,6,0x80000000,0xffffffff]:
                c=add('consume',f'consume-{player}-{charge}-{amount}',player=player,argument=amount)
                c['initial']['mega_values'][player]=sx(charge,32)
            for delta in [-0x80000000,-1,0,1,2,5,0x7fffffff]:
                c=add('increment',f'increment-{player}-{charge}-{delta}',player=player)
                c['initial']['mega_values'][player]=sx(charge,32);c['initial']['mega_states'][player]=delta
    for counter in [0,1,2,3,4,0x7fffffff,0xfffffffe,0xffffffff]:
        for zone in (0,1):
            for distance in (0,1,2):
                c=add('drop',f'drop-{counter}-{zone}-{distance}',counter_270=counter)
                c['ball']['zone']=zone;c['initial']['current_distance']=distance
    for c in cases:
        em.write(c['initial'],c['rules'],c['ball']);em.randoms=c['randoms']
        for key,offset in [('ai_hit_attempt_234',0x234),('ai_power_hit_type_238',0x238)]:em.w32(GAME+offset,c['state'][key])
        for p in range(2):em.w32(AI[p]+0x70,c['state']['ai_charge'][p])
        em.w32(GAME+0x224,c['word_224']);em.w32(GAME+0x270,c['counter_270'])
        em.wr(GAME+0x229,bytes([c['hit_attempt_marker']]))
        em.wr(REFS,bytes([c['attempt']['strike'],c['attempt']['reverse_strike']]))
        em.w32(REFS+4,c['attempt']['power_type'])
        op=c['operation']
        args = (GAME,c['player'],REFS,REFS+1,REFS+4) if op=='attempt' else (GAME,c['player'],c['argument']) if op=='consume' else (GAME,c['argument']) if op=='zone' else (GAME,c['player']) if op in ('power','increment') else (GAME,)
        returned=em.call(ADDRESSES[op],args)
        c['expected'],c['expected_ball']=em.read(c['initial'])
        c['expected_state']=dict(ai_hit_attempt_234=sx(em.r32(GAME+0x234),32),ai_power_hit_type_238=sx(em.r32(GAME+0x238),32),ai_charge=[em.r32(p+0x70) for p in AI])
        c['expected_attempt']=dict(strike=bool(em.rd(REFS,1)[0]),reverse_strike=bool(em.rd(REFS+1,1)[0]),power_type=sx(em.r32(REFS+4),32))
        c['expected_word_224']=sx(em.r32(GAME+0x224),32);c['expected_counter_270']=em.r32(GAME+0x270)
        c['expected_hit_attempt_marker']=bool(em.rd(GAME+0x229,1)[0])
        c['effects']=copy.deepcopy(em.events)
        # Void functions leave scratch return registers; only meaningful returns.
        c['returned']=returned if op in ('power','zone','consume') else None
    return dict(elf_sha256=SHA,cases=cases)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'Rally rules differ from original PPC'
        print('Verified',len(value['cases']),'original rally rule calls')
    else:
        OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
        print('Wrote',len(value['cases']),'original rally rule calls')
