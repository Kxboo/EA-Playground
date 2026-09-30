"""Execute retail InitGameLogicState with only its VLT/reset helpers supplied."""
import hashlib, json, random
from pathlib import Path
from ppc_emu2 import Emu, f32, sx
import math

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_initialize_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
OBJ=0x71000000
SINGLE_PLAYER_FLAG=0x71002000

class InitLogicEmu(Emu):
    def __init__(self):
        super().__init__()
        self.config=None
        self.effects=[]
        self.hooks.update({0x8039cf90:self.single,0x8039d080:self.multi,0x8039cbd0:self.reset_stats})
    def single(self,e):
        self.effects.append(['single_tunables'])
        for off,val in zip([0x16c,0x170,0x174,0x178,0x17c,0x180],self.config['single']):e.w32(OBJ+off,val)
    def multi(self,e):
        self.effects.append(['multi_tunables'])
        variant,game_mode,rounds,rotations=self.config['multi']
        for off,val in zip([0x3c,0x44,0x16c,0x170,0x174,0x178,0x17c,0x180],
                           [variant,game_mode,0,rounds,0,0,0,rotations]):e.w32(OBJ+off,val)
    def reset_stats(self,e):
        self.effects.append(['reset_stats'])
        e.wr(OBJ+0x138,bytes(0x34))
        for i,val in enumerate(self.config['scores']):e.w32(OBJ+0x138+i*4,val)
    def run_case(self,c):
        self.config=c['inputs'];self.effects=[]
        e=self
        e.wr(OBJ,bytes(0x500))
        initial=c['initial']
        for off,key in [(0x3c,'variant'),(0x44,'game_mode'),(0x16c,'mode'),(0x170,'rounds'),
                        (0x174,'distance'),(0x178,'handicap'),(0x17c,'seconds'),(0x180,'rotations_to_win'),
                        (0x430,'rotation_limit'),(0x434,'wins_required'),(0x438,'rotation_count'),
                        (0x204,'round_winner'),(0x440,'match_winner')]:e.w32(OBJ+off,initial[key])
        e.wr(OBJ+0x444,bytes([initial['field_444'],initial['field_445']]))
        # ResetStats selects its tunables branch using this static byte.
        e.w32(0x8060204c,SINGLE_PLAYER_FLAG)
        e.wr(SINGLE_PLAYER_FLAG,bytes([c['inputs']['multi_flag']]))
        e.call(0x8039ceac,[OBJ])
        out={}
        for off,name in [(0x3c,'variant'),(0x44,'game_mode'),(0x16c,'mode'),(0x170,'rounds'),
                         (0x174,'distance'),(0x178,'handicap'),(0x17c,'seconds'),(0x180,'rotations_to_win'),
                         (0x430,'rotation_limit'),(0x434,'wins_required'),(0x438,'rotation_count'),
                         (0x204,'round_winner'),(0x440,'match_winner')]:out[name]=sx(e.r32(OBJ+off),32)
        out['rotations']=[sx(x,8) for x in e.rd(OBJ+0x132,2)]
        out['indicator_target_bits']=e.r32(OBJ+0x348)
        out['score_weights']=[sx(e.r32(OBJ+0x138+i*4),32) for i in range(3)]
        out['statistics']=[sx(e.r32(OBJ+0x144+i*4),32) for i in range(10)]
        out['field_444']=bool(e.rd(OBJ+0x444,1)[0]);out['field_445']=bool(e.rd(OBJ+0x445,1)[0])
        return {'expected':out,'effects':e.effects}

    def step(self,pc):
        word=self.word(pc)
        if word>>26==31 and ((word>>1)&1023)==202:  # addze: srawi's carry completes truncation toward zero
            rd=(word>>21)&31;ra=(word>>16)&31
            total=self.r[ra]+self.ca
            self.r[rd]=total&0xffffffff;self.ca=1 if total>0xffffffff else 0
            return pc+4
        if word>>26==59 and ((word>>1)&31)==18:  # fdivs with IEEE zero handling
            rd=(word>>21)&31;fa=(word>>16)&31;fb=(word>>11)&31
            a,b=self.f[fa],self.f[fb]
            if b==0.0:
                assert a!=0.0,'0/0 default-NaN/FPSCR behavior is outside this oracle model'
                self.f[rd]=math.copysign(float('inf'),a*math.copysign(1.0,b))
            else:self.f[rd]=f32(a/b)
            return pc+4
        return super().step(pc)

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    rng=random.Random(0x8039ceac);em=InitLogicEmu();cases=[]
    edge=[-2147483648,-32769,-256,-129,-128,-7,-1,0,1,7,127,128,255,256,32767,2147483647]
    for i in range(64):
        multi=i%2==1
        rounds=rng.choice(edge) if i<32 else rng.randint(-500,500)
        handicap=rng.choice(edge) if i<32 else rng.randint(-500,500)
        initial={
          'variant':rng.choice(edge),'game_mode':rng.choice(edge),'mode':rng.choice(edge),
          'rounds':rng.choice(edge),'distance':rng.choice(edge),'handicap':rng.choice(edge),
          'seconds':rng.choice(edge),'rotations_to_win':rng.choice(edge),
          'rotation_limit':rng.choice([0,-2147483648,-1,1,127,2147483647]),
          'wins_required':rng.choice(edge),'rotation_count':rng.choice(edge),
          'round_winner':rng.choice(edge),'match_winner':rng.choice(edge),
          'field_444':i%2,'field_445':(i//2)%2}
        if multi:
            tunables={'multi_flag':1,'multi':[rng.choice(edge),rng.choice(edge),rounds,rng.choice(edge)],'single':[0]*6,'scores':[rng.choice(edge) for _ in range(3)]}
        else:
            fields=[rng.choice(edge),rounds,rng.choice(edge),handicap,rng.choice(edge),rng.choice(edge)]
            tunables={'multi_flag':0,'single':fields,'multi':[0]*4,'scores':[rng.choice(edge) for _ in range(3)]}
        # Keep zero-denominator infinities, but do not claim a hardware NaN
        # payload/sign from Python's arbitrary default NaN for 0/0.
        numerator=0 if multi else sx((-tunables['single'][3])&255,8)
        if initial['rotation_limit']==0 and numerator==0:initial['rotation_limit']=1
        c={'label':f'case_{i:03d}','initial':initial,'inputs':tunables}
        c.update(em.run_case(c));cases.append(c)
    return {'elf_sha256':SHA,'function':'InitGameLogicState__12MGTetherballFv','address':'0x8039ceac',
            'dependency_boundary':'InitTunablesForSinglePlayer/InitTunablesForMultiPlayer and ResetStats are supplied at their call sites; InitGameLogicState executes natively.',
            'cases':cases}

if __name__=='__main__':
    value=generate()
    if '--check' in __import__('sys').argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'fixture differs from retail execution'
        print(f"PASS {len(value['cases'])} InitGameLogicState calls with native arithmetic/stores")
    else:OUT.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8')
