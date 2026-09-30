"""Execute AI entity predicates, initialization and complete compulsion selection."""
import hashlib, json, random, sys, math
from pathlib import Path
from tetherball_scene_oracle import SceneEmu, IDENTITY, bits, from_bits
from tetherball_tuning_oracle import Oracle, load, OBJ as GAME
from ppc_emu2 import f32, sx
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT/'_bevy/tests/data/tetherball_ai_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
AI, CHAR, GRAPH, BALL, COMP, ARG = [0x71700000+i*0x1000 for i in range(6)]

class EntityEmu(SceneEmu):
    def step(self,pc):
        w=self.word(pc);op=w>>26;d,a,b,c=(w>>21)&31,(w>>16)&31,(w>>11)&31,(w>>6)&31;xo=(w>>1)&1023
        if op==53:
            addr=(self.r[a]+sx(w,16))&0xffffffff;self.w32(addr,bits(self.f[d]));self.r[a]=addr;return pc+4
        if op==4 and xo in (528,560,592,624):
            x=[self.f[a],self.ps1[a]];y=[self.f[b],self.ps1[b]]
            k=(xo-528)//32;self.f[d],self.ps1[d]=x[k//2],y[k%2];return pc+4
        if op==4 and (xo&31) in (21,25,29,10,11):
            x=[self.f[a],self.ps1[a]];y=[self.f[b],self.ps1[b]];z=[self.f[c],self.ps1[c]];k=xo&31
            if k==10:out=[f32(x[0]+y[1]),z[1]]
            elif k==11:out=[z[0],f32(x[0]+y[1])]
            else:out=[f32(x[i]+y[i]) if k==21 else f32(x[i]*z[i]) if k==25 else f32(math.fma(x[i],z[i],y[i])) for i in range(2)]
            self.f[d],self.ps1[d]=out;return pc+4
        return super().step(pc)
    def __init__(self, exe):
        super().__init__(exe)
        self.game_present = True
        self.hooks[0x80319a54] = lambda e: setattr(e, 'r', e.r[:3]+[GAME if self.game_present else 0]+e.r[4:])
        self.hooks[0x803d5894] = lambda e: setattr(e, 'r', e.r[:3]+[COMP]+e.r[4:])
        self.w32(AI+4,CHAR); self.w32(CHAR+0x18,GRAPH); self.w32(AI+0x60,BALL)
        self.w32(0x806012ac,ARG)
    def geometry(self,c):
        self.vector(GAME+0x398,[from_bits(x) for x in c['world']])
        self.vector(GAME+0x3d8,[from_bits(x) for x in c['inverse']])
        self.vector(CHAR+0x180,[from_bits(x) for x in c['character']])
        self.vector(BALL+0x60,[from_bits(x) for x in c['anchor']])
        self.w32(BALL+0xb4,c['radius'])

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load(); e=EntityEmu(exe); rng=random.Random(0x803954a8)
    transforms=[]
    for i in range(64):
        vector=[bits(rng.uniform(-20,20)) for _ in range(3)]
        matrix=[bits(rng.uniform(-3,3)) for _ in range(16)]
        e.vector(ARG,list(map(from_bits,vector)));e.vector(ARG+0x100,list(map(from_bits,matrix)))
        e.call(0x802ea12c,[ARG,ARG+0x100,ARG+0x200])
        transforms.append(dict(vector=vector,matrix=matrix,result=e.vector_bits(ARG+0x200,3)))
    swinging=[dict(animation=i,result=bool((e.w32(GRAPH+0x54,i),e.call(0x80395808,[AI]))[1])) for i in [-0x80000000,*range(-1,110),0x7fffffff]]
    gameplay=[dict(state=i,result=bool((e.w32(GAME+0x34,i),e.call(0x80397950,[GAME]))[1])) for i in [-1,*range(35),0xffffffff]]
    positions=[]
    for i in range(160):
        world=IDENTITY.copy(); inv=IDENTITY.copy()
        if i%3:
            tx,tz=rng.choice([-12.,0.,18.]),rng.choice([-4.,0.,23.]); world[12]=tx;world[14]=tz;inv[12]=-tx;inv[14]=-tz
        if i%4==0:
            world[0]=world[10]=inv[0]=inv[10]=0.
            world[2]=inv[8]=1.;world[8]=inv[2]=-1.
            inv[12]=-world[14];inv[14]=world[12]
        anchor=[world[12],1.,world[14]];radius=rng.choice([0.,0.5,1.25,3.])
        x=radius+0.55+rng.choice([0.,0.0005,0.001,0.002,0.01,1.])
        character=[world[12]+(-x if i%2 else x),rng.choice([0.,1.,10.]),world[14]]
        c=dict(world=list(map(bits,world)),inverse=list(map(bits,inv)),anchor=list(map(bits,anchor)),radius=bits(radius),character=list(map(bits,character)),tolerance=bits(rng.choice([0.,0.001,0.002,-0.002,1.])),state=rng.choice([0,28,29,28,29,30]))
        e.geometry(c);e.w32(GAME+0x34,c['state']);e.f[1]=from_bits(c['tolerance'])
        c['result']=bool(e.call(0x8039565c,[AI],[from_bits(c['tolerance'])])); positions.append(c)
    evaluations=[]
    for i in range(240):
        c=dict(positions[i%len(positions)])
        c.update(present=i%13!=0,state=rng.choice([28,29,28,29,12,0,30]),priority=rng.choice([0,39,40,49,50,255]),animation=rng.choice([0,63,64,69,75,91,94]),enabled=i%4!=0,heading=bits(rng.choice([0.,1.,3.14,5.])),ball_angle=bits(rng.choice([-1.,0.,1.,3.14,6.,7.])),angle=bits(-1.25),scale=bits(rng.choice([-1.,-0.,0.,1.])),waiting=i%2==0,distance=i%3,charge=i%8,difficulty=[(i+j*13)%101 for j in range(7)])
        e.geometry(c);e.game_present=c['present'];e.w32(GAME+0x34,c['state']);e.w32(GRAPH+0x54,c['animation'])
        for offset,key in [(0x80,'heading'),(0x64,'angle'),(0x68,'scale'),(0x74,'distance'),(0x70,'charge')]:e.w32(AI+offset,c[key])
        e.w32(BALL+0x50,c['ball_angle']);e.wr(AI+0x6c,bytes([c['waiting']]));e.wr(AI+0x78,bytes(c['difficulty']+[c['enabled']]))
        e.wr(COMP,bytes([0xa5])*0x100)
        result=e.call(0x803954a8,[AI,16,c['priority'],0,0])
        assert not result or e.r32(COMP) in (0x804dcb7c,0x804dcb58)
        c['kind']='none' if not result else 'move' if e.r32(COMP)==0x804dcb7c else 'hit'
        if result:c['vtable']=e.r32(COMP);c['compulsion_bytes']=e.rd(COMP,0xe4).hex()
        evaluations.append(c)
    assert {c['kind'] for c in evaluations} == {'none','move','hit'}
    assert {c['result'] for c in positions} == {True,False}
    db,hashes=load();o=Oracle(exe,db);o.hooks.pop(0x802cd4e8)
    initializations=[]
    for i in range(96):
        present=i%7!=0; enabled=i%3!=0; dare=[-1,0,3,6][i%4]; difficulty=[0,1,2,3,4,255,0xffffffff,2][i%8]
        o.hooks[0x80319a54]=lambda em,p=present:em.set_result(GAME if p else 0)
        o.w32(GAME+0x40,1);o.w32(GAME+0x48,dare)
        o.wr(AI+0x78,bytes([11,22,33,44,55,66,77,1]));o.w32(ARG,bits([-7.,-0.,0.,1.,7.][i%5]))
        o.call(0x80395354,[AI,int(enabled),difficulty,ARG])
        initializations.append(dict(present=present,enabled=enabled,dare=dare,difficulty=difficulty,angle=o.r32(ARG),result=list(o.rd(AI+0x78,8)),heading=o.r32(AI+0x80)))
    from tetherball_ai_hit_oracle import HitCompulsionEmu,base_case,DB_GET_INT16_ARRAY
    hit=HitCompulsionEmu(exe);tables=[]
    def int16(em):
        name=em.cstring(em.r[4]);o.wr(ARG,name.encode('ascii')+b'\0')
        em.r[3]=o.call(0x802f4bf8,[0x73000000,ARG,em.r[5]])
    hit.hooks[DB_GET_INT16_ARRAY]=int16
    for session in (-1,0,1,2):
        for dare in (-1,0,3,6):
            o.collection=o.selector(session,dare)
            c=base_case('activate');c.update(session_mode=session,dare=dare)
            native=hit.run(c)
            assert ['vlt_key',o.collection] in native['events']
            tables.append(dict(session=session,dare=dare,angles=native['state']['angle_tables_bits']))
    return dict(elf_sha256=SHA,corpus_sha256=hashes,transforms=transforms,swinging=swinging,gameplay=gameplay,positions=positions,evaluations=evaluations,initializations=initializations,hit_tables=tables)

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
    else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
    print('Verified AI entity original execution:',{k:len(v) for k,v in value.items() if isinstance(v,list)})
