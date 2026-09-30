"""Original Tetherball::Initialize, with engine asset/pool/VLT boundaries.

String storage is modeled at the CString boundary. Vector stores and angle wrap
execute retail instructions; snapshots include untouched motion and scene state.
"""
import hashlib, json, random, sys
from pathlib import Path
from tetherball_scene_oracle import SceneEmu, OBJ, FLOATS, INTS, FLAGS, bits, from_bits, read_motion
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_ball_init_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
POSITION, PLACEABLE, COLLECTION = 0x71010000, 0x71011000, 0x71012000
FIELDS = ['ball_basehitspeed', 'ball_acceleratemodifier', 'ball_powermodifier', 'ball_megamodifier']

def execute(exe, c):
    e = SceneEmu(exe); events = []; strings = {}; allocations = []
    for name, off in FLOATS.items(): e.w32(OBJ+off, c['initial'][name])
    for name, off in INTS.items(): e.w32(OBJ+off, c['initial'][name])
    for name, off in FLAGS.items(): e.wr(OBJ+off, bytes([c['initial'][name]]))
    for off in [0x40,0x60,0xc0,0x100]:
        for i in range(16 if off in [0xc0,0x100] else 3): e.w32(OBJ+off+i*4, bits(17.+i))
    for i in range(3): e.w32(OBJ+0x140+i*4, 100+i)
    e.w32(OBJ+0x158, bits(-31.)); e.wr(OBJ+0x15c, b'\x07')
    for i,v in enumerate(c['anchor']): e.w32(POSITION+i*4,v)
    e.w32(PLACEABLE+0xf4,c['pole_height'])
    e.w32(0x8060223c,0x71013000);e.w32(0x806018d4,0x71014000)
    e.w32(0x806021e4,0x71015000);e.w32(0x71015008,0x71016000)
    e.w32(0x805ffae4,c['pool'])
    def string_ctor(m): strings[m.r[3]]=m.cstring(m.r[4])
    def string_append(m): strings[m.r[3]]+=m.cstring(m.r[4])
    def string_ptr(m):
        text=strings[m.r[3]];m.r[3]=0x71017000;m.wr(m.r[3],text.encode()+b'\0')
    def asset(m, texture):
        slot=(m.r[5]-OBJ)//4
        handle=0x72000000+slot*0x100; asset_id=c['asset_id_base']+slot
        events.append(['texture' if texture else 'model',m.cstring(m.r[4]),asset_id,handle] + ([] if texture else [m.r[6]]))
        m.w32(m.r[5],asset_id);m.r[3]=handle
    def textures(m):events.append(['textures',m.r[3],m.r[4]])
    def allocate(m):
        i=len(allocations);handle=0 if i in c['null_shadows'] else 0x73000000+i*0x100
        events.append(['allocate',m.r[3],m.r[4],m.r[5],m.cstring(m.r[6]),handle]);allocations.append(handle);m.r[3]=handle
    def cached(m):events.append(['cached_ctor',m.r[3]])
    def scale(m):events.append(['scale',m.r[3],m.r32(m.r[3]+0x44)])
    def shadow(m):events.append(['shadow_ctor',m.r[3],m.r[4]])
    def add_entity(m):events.append(['add_entity',m.r[4],m.r[5]])
    keys={}
    def key(m):
        name=m.cstring(m.r[4]);value=[0x11000000+len(keys),0x22000000+len(keys)];keys[tuple(value)]=name
        events.append(['key',name]);m.r[3],m.r[4]=value
    def collection(m):
        events.append(['collection',keys[(m.r[5],m.r[6])],keys[(m.r[7],m.r[8])]]);m.r[3]=COLLECTION
    def get_float(m):
        name=m.cstring(m.r[4]);events.append(['float',name,m.r[5]])
        m.f[1]=from_bits(c['tuning'][FIELDS.index(name)])
    def destroy_collection(m):events.append(['destroy_collection'])
    e.hooks.update({0x803ccc78:string_ctor,0x803ccfd0:string_append,0x803cd454:string_ptr,0x803cce94:lambda m:None,
        0x803cb934:lambda m:asset(m,True),0x803cb930:lambda m:asset(m,False),0x803e3440:textures,
        0x803cfa08:allocate,0x803bd5d0:cached,0x803bd664:scale,0x8039ea78:shadow,0x803c4bf8:add_entity,
        0x802f45e4:key,0x802f4794:collection,0x802f50e4:get_float,0x802f4900:destroy_collection})
    e.call(0x8039d3d8,(OBJ,c['difficulty'],POSITION,PLACEABLE),(from_bits(c['heading']),))
    return dict(input=c,events=events,motion=read_motion(e),anchor=e.vector_bits(OBJ+0x60,3),
        position=e.vector_bits(OBJ+0x40,3),ball_matrix=e.vector_bits(OBJ+0xc0,16),rope_matrix=e.vector_bits(OBJ+0x100,16),
        trails=[e.r32(OBJ+0x140+i*4) for i in range(3)],
        resources=[e.r32(OBJ+i*4) for i in range(1,13)],accelerate_modifier=e.r32(OBJ+0x158),flag_15c=e.rd(OBJ+0x15c,1)[0])

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load();rng=random.Random(0x8039d3d8)
    base=json.loads((ROOT/'_bevy/tests/data/tetherball_golden.json').read_text(encoding='utf-8'))
    cases=[]
    for i in range(48):
        c=dict(initial=base['cases'][i]['initial'],difficulty=[0,1,2,3,4,0xffffffff][i%6],
            heading=bits([-7.,-3.1415927410125732,-0.,0.,3.1415927410125732,19.][i%6]),
            anchor=[bits(rng.uniform(-100,100)) for _ in range(3)],pole_height=bits([-2.,0.,3.,12.][i%4]),
            tuning=[bits(rng.uniform(-2,20)) for _ in FIELDS],pool=i%3,asset_id_base=1000+i*16,
            null_shadows=[j for j in [4,5] if (i>>(j-4))&1])
        cases.append(execute(exe,c))
    return dict(elf_sha256=SHA,cases=cases)

if __name__=='__main__':
    result=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==result,'ball initialization differs from original'
        print(f"Verified {len(result['cases'])} original ball initialization calls")
    else:
        OUT.write_text(json.dumps(result,indent=1)+'\n',encoding='utf-8');print('Wrote',OUT)
