"""Complete original Tetherball Update and AreaManager rendering transforms.

Only shadow and particle-object services are hooked. Matrix, trigonometric,
attachment feedback and trail-selection instructions execute in the original.
"""
import hashlib,json,math,random,struct,sys
from pathlib import Path
from character_input_oracle import InputEmu
from ppc_emu2 import f32,sx
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_scene_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
OBJ=0x71000000;OWNER=0x71001000;LOCAL=0x71002000;AREA=0x71003000;BUFFER=0x71004000
IDENTITY=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]
FLOATS={'vertical_velocity':0x80,'angular_velocity':0x88,'acceleration':0x8c,'secondary_acceleration':0x90,'secondary_velocity':0x94,'radius':0xb0,'desired_radius':0xb4,'height':0xb8,'target_height':0xbc,'base_hit_speed':0x14c,'power_modifier':0x150,'mega_modifier':0x154,'angle':0x50,'hit_angle':0x54,'secondary_angle':0x58,'target_velocity':0x98,'secondary_target_velocity':0x9c,'spin_acceleration':0x168,'pole_height':0x160}
INTS={'toss_time':0x84,'hit_type':0xa0,'direction':0xa4,'hit_direction':0xa8,'zone':0xac}
FLAGS={'grabbed':0x70,'tossed':0x7c,'spinning_up':0x164,'spinning_down':0x165}

def bits(x):return struct.unpack('>I',struct.pack('>f',x))[0]
def from_bits(x):return struct.unpack('>f',struct.pack('>I',x))[0]

class SceneEmu(InputEmu):
    def __init__(self,exe):
        super().__init__(exe);self.ps1=[0.]*32;self.words={};self.effects=[];self.next_id=1000
    def word(self,pc):
        if pc not in self.words:self.words[pc]=super().word(pc)
        return self.words[pc]
    def step(self,pc):
        w=self.word(pc);op=w>>26;d,a,b,c=(w>>21)&31,(w>>16)&31,(w>>11)&31,(w>>6)&31
        if op in (56,57,60,61):
            assert ((w>>12)&7)==0, f'Unimplemented quantization at {pc:x}'
            addr=((self.r[a] if a else 0)+sx(w,12))&0xffffffff
            one=(w>>15)&1
            if op in (56,57):
                self.f[d]=from_bits(self.r32(addr));self.ps1[d]=1. if one else from_bits(self.r32(addr+4))
            else:
                self.w32(addr,bits(self.f[d]))
                if not one:self.w32(addr+4,bits(self.ps1[d]))
            if op in (57,61):self.r[a]=addr
            return pc+4
        if op==4 and ((w>>1)&31) in (12,13,14,15):
            kind=(w>>1)&31;mult=self.f[c] if kind in (12,14) else self.ps1[c]
            values=[self.f[a],self.ps1[a]];add=[self.f[b],self.ps1[b]]
            out=[f32(v*mult) if kind in (12,13) else f32(math.fma(v,mult,add[i])) for i,v in enumerate(values)]
            self.f[d],self.ps1[d]=out;return pc+4
        if op==59 and ((w>>1)&31) in (28,29,30,31):
            kind=(w>>1)&31;x=math.fma(self.f[a],self.f[c],self.f[b] if kind in (29,31) else -self.f[b])
            self.f[d]=f32(-x if kind in (30,31) else x);return pc+4
        return super().step(pc)
    def vector(self,address,values):self.wr(address,struct.pack('>'+'f'*len(values),*values))
    def vector_bits(self,address,n):return [self.r32(address+i*4) for i in range(n)]
    def cstring(self,address):
        data=bytearray()
        while True:
            c=self.rd(address+len(data),1)[0]
            if c==0:return data.decode('ascii')
            data.append(c)
    def area(self,radius,disabled):
        self.vector(AREA+0x10,[radius])
        self.wr(self.e.symbols['_SDA_BASE_']['value']-0x1bf4,bytes([disabled]))

def read_motion(e):
    return {**{k:e.r32(OBJ+off) for k,off in FLOATS.items()},
            **{k:sx(e.r32(OBJ+off),32) for k,off in INTS.items()},
            **{k:bool(e.rd(OBJ+off,1)[0]) for k,off in FLAGS.items()}}

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load();e=SceneEmu(exe);rng=random.Random(0x8039dc84)
    areas=[]
    for i in range(72):
        position=[0.,0.,0.] if i<2 else [12.,0.,-53.] if i==2 else [f32(rng.uniform(-100,100)),f32(rng.uniform(-2,15)),f32(rng.uniform(-100,100))]
        radius=rng.choice([150.,250.,300.]);disabled=i%5==0
        e.area(radius,disabled);e.vector(BUFFER,position);e.call(0x803d78b8,(AREA,BUFFER,BUFFER+0x100))
        areas.append(dict(position=[bits(x) for x in position],radius=bits(radius),disabled=disabled,matrix=e.vector_bits(BUFFER+0x100,16)))
    def create(em):
        handle=em.next_id;em.next_id+=1
        em.effects.append(['create',em.cstring(em.r[4]),em.vector_bits(em.r[5],3),handle]);em.r[3]=handle
    def destroy(em):em.effects.append(['destroy',em.r32(em.r[4]),sx(em.r[5],32)])
    def get_fx(em):em.r[3]=em.r32(em.r[4])
    def move_fx(em):em.effects.append(['move',em.r[3],em.vector_bits(em.r[4],3)])
    def shadow(em):em.effects.append(['shadow',em.r[3]==0x71005100,em.vector_bits(em.r[4],16)])
    e.hooks.update({0x802f78a4:create,0x802f7abc:destroy,0x802f7bf4:get_fx,0x802f6c34:move_fx,0x8039e354:shadow})
    e.w32(0x805e8320+0x8c,0x71006000);e.w32(0x71006008,AREA)
    e.w32(0x80601f78,0xffffffff);e.w32(0x80602008,0x71007000)
    base=json.loads((ROOT/'_bevy/tests/data/tetherball_golden.json').read_text(encoding='utf-8'))
    cases=[]
    for i in range(60):
        initial=dict(base['cases'][i]['initial'])
        initial.update(grabbed=i%3!=0,tossed=i%4==0,spinning_up=False,spinning_down=i%7==0)
        if i<12:
            initial.update(angular_velocity=bits([0.,1.,3.,6.][i%4]),acceleration=bits(2.),secondary_acceleration=bits(4.))
        e.wr(OBJ,bytes(0x170))
        for name,off in FLOATS.items():e.w32(OBJ+off,initial[name])
        for name,off in INTS.items():e.w32(OBJ+off,initial[name])
        for name,off in FLAGS.items():e.wr(OBJ+off,bytes([initial[name]]))
        anchor=[f32(rng.uniform(-30,30)),f32(rng.uniform(1,4)),f32(rng.uniform(-30,30))]
        world=IDENTITY[:];world[12:15]=[anchor[0]+1.,anchor[1]-1.,anchor[2]+.3]
        # General finite affine transforms exercise real multiplication order.
        if i%2:
            world[:12]=[f32(rng.uniform(-1,1)) if k%4!=3 else 0. for k in range(12)]
        local=None
        if i%3==1:
            local=IDENTITY[:];local[12:15]=[.25,.1,-.2]
        e.vector(OBJ+0x60,anchor);e.w32(OBJ+0x74,OWNER);e.vector(OWNER+0x20,world)
        e.w32(OBJ+0x78,LOCAL if local else 0)
        if local:e.vector(LOCAL,local)
        shadows=[i%2==0,i%3==0]
        e.w32(OBJ+4,0x71005000 if shadows[0] else 0);e.w32(OBJ+8,0x71005100 if shadows[1] else 0)
        trails=[0xffffffff if (i+j)%2==0 else 100+j for j in range(3)]
        for j,handle in enumerate(trails):e.w32(OBJ+0x140+j*4,handle)
        radius=250.;disabled=i%5==0;e.area(radius,disabled);e.next_id=1000
        steps=[]
        for ms in [0,16,60,399,1,120]:
            e.effects=[];e.call(0x8039d904,(OBJ,ms))
            steps.append(dict(ms=ms,motion=read_motion(e),position=e.vector_bits(OBJ+0x40,3),ball_matrix=e.vector_bits(OBJ+0xc0,16),rope_matrix=e.vector_bits(OBJ+0x100,16),trails=[e.r32(OBJ+0x140+j*4) for j in range(3)],effects=e.effects))
        cases.append(dict(initial=initial,anchor=[bits(x) for x in anchor],world=[bits(x) for x in world],local=[bits(x) for x in local] if local else None,shadows=shadows,trails=trails,area_radius=bits(radius),disabled=disabled,steps=steps))
    return dict(elf_sha256=SHA,areas=areas,cases=cases)

if __name__=='__main__':
    result=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==result,'Tetherball scene differs from original PPC'
        print(f"Verified {len(result['areas'])} area matrices and {sum(len(c['steps']) for c in result['cases'])} complete tetherball Update transitions")
    else:
        OUT.write_text(json.dumps(result,indent=1)+'\n',encoding='utf-8');print('Wrote',OUT)
