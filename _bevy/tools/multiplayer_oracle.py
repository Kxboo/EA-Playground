"""Execute original MultiplayerMode PowerPC routines; emit Rust golden fixtures.
No reference model and no game image contents are distributed in the output.
"""
import hashlib, json, random, sys
from pathlib import Path
from ppc_emu2 import Emu, sx

OUT = Path(__file__).resolve().parents[1] / 'tests/data/multiplayer_golden.json'
ELF = Path(__file__).resolve().parents[2] / 'Remaster/reference/playgroundz.elf'
SHA256 = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'

def generate():
    assert hashlib.sha256(ELF.read_bytes()).hexdigest() == SHA256, 'unexpected original ELF SHA-256'
    e = Emu()
    # Compiler register spill helpers are irrelevant to these isolated calls.
    for a in (0x80035680, 0x80035688, 0x800356cc, 0x800356d4):
        e.hooks[a] = lambda em: None
    e.hooks[0x8041af54] = lambda em: em.wr(em.r[3], em.rd(em.r[4], em.r[5]))
    obj = 0x100000
    def call(name, signature, *args):
        return sx(e.call(e.e.symbols[name + '__15MultiplayerMode' + signature]['value'], (obj, *args)), 32)
    def snapshot():
        words = lambda off, stride=4: [sx(e.r32(obj+off+i*stride),32) for i in range(4)]
        return dict(points=words(0x90,8), point_ranks=words(0x94,8), wins=words(0xb0,8),
                    win_ranks=words(0xb4,8), last_points=words(0xd0), last_winners=words(0x100),
                    placement=words(0x110), rounds_left=sx(e.r32(obj+0xe4),32),
                    results_count=sx(e.r32(obj+0xe8),32), point_series=bool(e.rd(obj+0xe0,1)[0]))
    rng = random.Random(193)
    sessions=[]
    for n in (2,3,4):
        for series in (False,True):
            e.wr(obj, bytes(0x128)); e.w32(obj+8,n)
            # Rust defines deterministic zero initialization for fields untouched by original start calls.
            actions=[dict(kind='series' if series else 'free', args=[3] if series else [])]
            call('StartPointSeries','Fi',3) if series else call('StartFreePlay','Fv')
            steps=[dict(action=actions[0], expected=snapshot())]
            cases=[('round',[10,10,10 if n>2 else -1,10 if n>3 else -1]),
                   ('round',[0,10,-1,-1]), ('win',[0,0]), ('placement',[0,1,2,3]),
                   ('round',[10,0,-1,-1]), ('win',[-1,-1])]
            for _ in range(24):
                cases.append(('round',[rng.randint(-2,50),rng.randint(-2,50),rng.choice([-1,rng.randint(-2,50)]) if n>2 else -1,rng.choice([-1,rng.randint(-2,50)]) if n>3 else -1]) if rng.randrange(2) else ('win',[rng.choice([-1,*range(n)]),rng.choice([-1,*range(n)])]))
            # Include restart behavior: free play retains points and last match points.
            cases += [('free',[]),('series',[-1]),('round',[2147483647,2147483647,-1,-1]),('round',[1,1,-1,-1])]
            for kind,args in cases:
                fn,sig={'round':('AddRoundResults','Fiiii'),'win':('AddWinResults','Fii'),'placement':('SetLastPlacement','Fiiii'),'free':('StartFreePlay','Fv'),'series':('StartPointSeries','Fi')}[kind]
                call(fn,sig,*args)
                snap=snapshot()
                snap['point_total_queries']=[call('GetPointTotal','Fi',p) for p in range(n)]
                snap['win_total_queries']=[call('GetWinTotal','Fi',p) for p in range(n)]
                snap['last_points_queries']=[call('GetPlayerPointsInThisMatch','Fi',p) for p in range(n)]
                snap['rounds_left_query']=call('GetNumRoundsLeft','Fv')
                snap['ranks']=[call('GetPlayerRank','Fi',p) for p in range(n)]
                snap['rank_players']=[call('GetPlayerNumByRank','Fi',p) for p in range(-1,n+1)]
                snap['won_last']=[bool(call('WonLastGame','Fi',p)) for p in range(n)]
                steps.append(dict(action=dict(kind=kind,args=args),expected=snap))
            sessions.append(dict(players=n,steps=steps))
    return dict(source='playgroundz.elf original PowerPC MultiplayerMode routines', elf_sha256=SHA256, sessions=sessions)

if __name__ == '__main__':
    result=generate(); encoded=json.dumps(result,indent=2)+'\n'
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text())==result, 'golden fixtures differ from original PowerPC'
        print('MultiplayerMode: 210 state transitions match original PowerPC fixtures')
    else:
        OUT.parent.mkdir(parents=True,exist_ok=True); OUT.write_text(encoded)
        print('Wrote',OUT)
