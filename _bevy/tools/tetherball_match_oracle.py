"""Execute retail tetherball winner predicates, recording external UI/state entry.

The original ChangeGameState prologue and common field stores execute; its
state-specific model/animation/UI dispatch is skipped to the original epilogue.
No winner, timeout, counter or result predicate is replaced with Python logic.
"""
import hashlib, json, random, sys
from pathlib import Path
from ppc_emu2 import Emu, sx

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_match_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
OBJ = 0x71000000
WORDS = {'elapsed_ms':0x258, 'state_ms':0x250, 'previous_state_ms':0x254,
         'state_code':0x34, 'round_winner':0x204, 'match_winner':0x440,
         'result':0x60, 'final_result':0x64}
SIGNED = {'round_winner', 'match_winner', 'result', 'final_result'}
RULES = {'mode':0x16c, 'rotation_limit':0x430, 'wins_required':0x434,
         'time_limit_seconds':0x17c}


class MatchEmu(Emu):
    def step(self, pc):
        if pc == 0x8039a994:
            assert self.word(pc) == 0x41810508
            self.effects.append(['state', self.r[4]])
            return 0x8039ae9c
        w = self.word(pc)
        if w >> 26 == 31 and (w >> 1) & 1023 == 183:  # stwux
            src, a, b = (w >> 21) & 31, (w >> 16) & 31, (w >> 11) & 31
            addr = (self.r[a] + self.r[b]) & 0xffffffff
            self.w32(addr, self.r[src]); self.r[a] = addr
            return pc + 4
        return super().step(pc)

    def write_state(self, state):
        self.wr(OBJ, bytes(0x450))
        for name, offset in WORDS.items(): self.w32(OBJ + offset, state[name])
        for name, offset in [('rotations',0x132), ('round_wins',0x134)]:
            self.wr(OBJ + offset, bytes(x & 255 for x in state[name]))
        self.wr(OBJ + 0x20c, bytes([state['match_over']]))

    def read_state(self):
        state = {name:(sx(self.r32(OBJ+offset),32) if name in SIGNED else self.r32(OBJ+offset))
                 for name,offset in WORDS.items()}
        for name,offset in [('rotations',0x132), ('round_wins',0x134)]:
            state[name] = [sx(x,8) for x in self.rd(OBJ+offset,2)]
        state['match_over'] = bool(self.rd(OBJ+0x20c,1)[0])
        return state


def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    em = MatchEmu()
    em.hooks[0x80317a94] = lambda e:e.effects.append(['winner','single',sx(e.r[3],32),bool(e.r[4])])
    em.hooks[0x80317b24] = lambda e:e.effects.append(['winner','multi',sx(e.r[3],32),bool(e.r[4])])
    rng = random.Random(0x8039af7c)
    cases = []
    for i in range(300):
        limit = rng.choice([-1,0,1,3,6,127,128])
        wins = rng.choice([-128,0,1,2,3,127,128])
        seconds = rng.choice([-1,0,1,30,2147483647,4294968])
        elapsed = rng.choice([0,999,1000,30000,0xffffffff,(seconds*1000-1)&0xffffffff,(seconds*1000)&0xffffffff])
        state = dict(rotations=[rng.choice([-128,0,1,3,6,127]) for _ in range(2)],
                     round_wins=[rng.choice([-128,-1,0,1,2,3,126,127]) for _ in range(2)],
                     elapsed_ms=elapsed, state_ms=rng.randrange(10000), previous_state_ms=rng.randrange(10000),
                     state_code=rng.choice([27,28,29,30]), round_winner=rng.choice([-1,0,1]),
                     match_winner=rng.randrange(2), match_over=bool(i%11==0),
                     result=rng.randrange(3), final_result=rng.randrange(3))
        mode = i % 3 if i >= 16 else i % 2
        rules = dict(mode=mode,rotation_limit=limit,wins_required=wins,time_limit_seconds=seconds)
        session_mode = rng.choice([0,1,2,3])
        if i < 16:
            # P0/P1 simultaneous thresholds, exact timeout, signed byte wrap,
            # equality rather than >=, sticky result, and already-over replay.
            rules.update(rotation_limit=3,wins_required=2,time_limit_seconds=1)
            state.update(rotations=[3,3] if i<4 else [0,3] if i<8 else [0,0],
                         round_wins=[[1,1],[0,127],[0,2],[2,1]][i%4],
                         elapsed_ms=[999,1000,1001,0][i%4],result=1)
            if i>=12: rules['time_limit_seconds']=0
        em.write_state(state)
        for name,offset in RULES.items(): em.w32(OBJ+offset,rules[name])
        em.w32(OBJ+0x40,session_mode)
        steps = []
        for _ in range(3):
            em.effects = []
            em.call(0x8039af5c,(OBJ,))
            steps.append(dict(expected=em.read_state(),effects=em.effects))
        cases.append(dict(initial=state,rules=rules,session_mode=session_mode,steps=steps))
    return dict(elf_sha256=SHA,cases=cases)


if __name__ == '__main__':
    value = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == value
        print(f"Tetherball match: {sum(len(c['steps']) for c in value['cases'])} original predicate/state-store transitions match")
    else:
        OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8')
        print('Wrote', OUT)
