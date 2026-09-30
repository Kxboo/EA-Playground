"""Original PowerPC Tetherball motion/serve routines and MGTetherball scoring oracle."""
import hashlib, json, math, random, struct, sys
from pathlib import Path
from ppc_emu2 import Emu, f32, sx
ROOT=Path(__file__).resolve().parents[2]
SHA256='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
OUT=ROOT/'_bevy/tests/data/tetherball_golden.json'

class TetherEmu(Emu):
    def step(self,pc):
        if pc==0x8039dc84 and getattr(self,'stop_before_render',False):
            return 0xDEAD0000
        w=self.word(pc)
        if w>>26==31 and (w>>1)&1023==183: # stwux
            src=(w>>21)&31;ra=(w>>16)&31;rb=(w>>11)&31
            addr=(self.r[ra]+self.r[rb])&0xffffffff
            self.w32(addr,self.r[src]);self.r[ra]=addr
            return pc+4
        if w>>26==63 and (w>>1)&1023 in (0,32): # fcmpu
            a=self.f[(w>>16)&31];b=self.f[(w>>11)&31]
            self.cr[(w>>23)&7]=1 if math.isnan(a) or math.isnan(b) else 8 if a<b else 4 if a>b else 2
            return pc+4
        if w>>26==19 and (w>>1)&1023==449: # cror
            dst=(w>>21)&31;a=(w>>16)&31;b=(w>>11)&31
            bit=1<<(3-(dst&3))
            self.cr[dst>>2]=(self.cr[dst>>2]&~bit) | (bit if self.crbit(a) or self.crbit(b) else 0)
            return pc+4
        return super().step(pc)

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA256
    em=TetherEmu(); obj=0x100000
    # Execute original rmfAbs helper; no behavior hooks.
    fields={'vertical_velocity':0x80,'angular_velocity':0x88,'acceleration':0x8c,'secondary_acceleration':0x90,'secondary_velocity':0x94,'radius':0xb0,'desired_radius':0xb4,'height':0xb8,'target_height':0xbc,'base_hit_speed':0x14c,'power_modifier':0x150,'mega_modifier':0x154,'angle':0x50,'hit_angle':0x54,'secondary_angle':0x58,'target_velocity':0x98,'secondary_target_velocity':0x9c,'spin_acceleration':0x168,'pole_height':0x160}
    ints={'toss_time':0x84,'hit_type':0xa0,'direction':0xa4,'hit_direction':0xa8,'zone':0xac}
    flags={'grabbed':0x70,'tossed':0x7c,'spinning_up':0x164,'spinning_down':0x165}
    def write(state):
        em.wr(obj,bytes(0x170))
        for k,off in fields.items():em.wr(obj+off,struct.pack('>f',state[k]))
        for k,off in ints.items():em.w32(obj+off,state[k])
        for k,off in flags.items():em.wr(obj+off,bytes([state[k]]))
    def read():
        return {**{k:em.r32(obj+off) for k,off in fields.items()},**{k:sx(em.r32(obj+off),32) for k,off in ints.items()},**{k:bool(em.rd(obj+off,1)[0]) for k,off in flags.items()}}
    signatures={'serve':'F19TetherballDirectionf','power':'FUi','high':'FUi','toss':'Fv','miss':'Fv','set_radius':'Ff','desired_radius':'Ff','angular':'Ff','zone':'F14TetherballZone','drop':'Fv','hit':'F19TetherballDirectionff19MGTetherballHitType','update':'Fi','spin_down':'Fv'}
    names={'serve':'Serve','power':'CheckForPowerServe','high':'CheckForHighServe','toss':'Toss','miss':'Miss','set_radius':'SetRadius','desired_radius':'SetDesiredRadius','angular':'SetAngularVelocity','zone':'SetZone','drop':'DropOneZone','hit':'Hit','update':'Update','spin_down':'SpinDownPole'}
    rng=random.Random(674)
    cases=[]
    for i in range(100):
        initial={k:f32(rng.uniform(-8,8)) for k in fields}
        initial.update(radius=f32(rng.uniform(.1,3)),desired_radius=f32(rng.uniform(.1,3)),height=f32(rng.uniform(0,2)),zone=i%2,direction=i%2,hit_type=3,toss_time=123,hit_direction=i%2,grabbed=True,tossed=True,spinning_up=True,spinning_down=True)
        if i<6:
            initial['vertical_velocity']=[0.0,-0.0,0.5,-0.5,3.6,-3.6][i]
            initial['angular_velocity']=0.0 if i%2 else -0.0
            initial['secondary_velocity']=-0.0 if i%2 else 0.0
            initial['height']=f32(0.7)
        if i<12:
            initial['radius']=f32(0.7);initial['desired_radius']=f32(0.7)
            initial['height']=f32(0.5);initial['target_height']=f32(0.5)
            initial['angular_velocity']=[-6.0,6.0,0.0,-0.0][i%4]
            initial['spin_acceleration']=-8.0 if i%2==0 else 8.0
            initial['spinning_up']=False;initial['spinning_down']=True
        write(initial)
        steps=[]
        actions=[('power',[i*13],[]),('high',[i*13],[]),('serve',[i%2],[f32(rng.uniform(0,12))]),('miss',[],[]),('zone',[i%2],[]),('drop',[],[]),('desired_radius',[],[f32(rng.uniform(.1,3))]),('angular',[],[f32(rng.uniform(-12,12))]),('set_radius',[],[f32(rng.uniform(.1,3))]),('toss',[],[]),('power',[400],[]),('high',[400],[])]
        if i<8:
            # Threshold behavior around power/high serve boundaries, including uint conversion.
            dt=[0,1,100,400,748,749,0xfffffffe,0xffffffff][i]
            actions.insert(0,('power',[dt],[]));actions.insert(1,('high',[dt],[]))
        actions = [('update',[0],[]),('update',[16],[])] + actions
        actions += [('hit',[i%2,i%8],[f32(rng.uniform(0,12)),f32(rng.uniform(-20,20))]),('hit',[(i+1)%2,4],[f32(rng.uniform(0,12)),f32(rng.uniform(-20,20))]),('update',[16],[]),('spin_down',[],[]),('update',[60],[]),('update',[400],[]),('toss',[],[]),('update',[399],[]),('update',[1],[]),('update',[60],[])]
        for kind,args,fargs in actions:
            em.stop_before_render=kind=='update'
            result=em.call(em.e.symbols[names[kind]+'__10Tetherball'+signatures[kind]]['value'],(obj,*args),fargs)
            steps.append(dict(kind=kind,args=args,fargs=[struct.unpack('>I',struct.pack('>f',x))[0] for x in fargs],expected=read(),result=bool(result) if kind in ('power','high') else None))
        cases.append(dict(initial={**{k:struct.unpack('>I',struct.pack('>f',v))[0] for k,v in initial.items() if k in fields},**{k:v for k,v in initial.items() if k not in fields}},steps=steps))
    scores=[]
    for i in range(128):
        # CalcScore ignores its difficulty argument; second argument selects coefficient row.
        stats=[rng.randint(-10000,10000) for _ in range(3)]
        coeff=[rng.randint(-10000,10000) for _ in range(3)]
        if i==0:stats=[0x7fffffff,-0x80000000,17];coeff=[3,-1,2147483647]
        for off,val in zip((0x138,0x13c,0x140),stats):em.w32(obj+off,val)
        for off,val in zip((0x14c,0x150,0x154),coeff):em.w32(obj+off,val)
        got=sx(em.call(0x8039cd10,(obj,i%4,0)),32)
        scores.append(dict(stats=stats,coefficients=coeff,result=got))
    angles=[]
    em.stop_before_render=False
    for value in (0.0,-0.0,f32(6.2831854820251465),f32(-6.2831854820251465),-20.0,20.0,-1000.0,1000.0):
        em.wr(obj,struct.pack('>f',value));em.call(0x802cd4e8,(obj,))
        angles.append(dict(input=struct.unpack('>I',struct.pack('>f',value))[0],result=em.r32(obj)))
    return dict(elf_sha256=SHA256,cases=cases,scores=scores,angles=angles)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text())==value
        print(f"Tetherball: {sum(len(c['steps']) for c in value['cases'])} motion/serve/hit/update transitions and {len(value['scores'])} scores match original PowerPC")
    else:
        OUT.write_text(json.dumps(value,indent=1)+'\n');print('Wrote',OUT)

