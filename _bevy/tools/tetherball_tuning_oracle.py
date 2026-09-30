"""Original PPC tuning selectors, accessors and initialization arithmetic; --check never writes."""
import argparse, hashlib, json, os, struct, sys, subprocess, tempfile
from pathlib import Path
HERE=Path(__file__).resolve().parent; ROOT=HERE.parent.parent
sys.path[:0]=[str(HERE),str(ROOT/'Remaster/src')]
import re_functions as rf
import vlt
from character_input_oracle import InputEmu
PIN='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
FIX=HERE.parent/'tests/data/tetherball_tuning_golden.json'
OBJ=0x71000000; BASE=0x804dcbe8
FIELDS=['game_play_type','single_player_num_rounds','distance_from_pole','handicap','game_duration','single_player_rotations_to_win']
SPEED=['ball_basehitspeed','ball_powermodifier','ball_megamodifier','ball_acceleratemodifier']
ANGLES=['hit_returnanglepredelta','hit_returnanglepostdelta','hit_accelanglepredelta','hit_accelanglepostdelta']
AI=['ai_toofastchance','ai_tooslowchance','ai_wrongheightchance','ai_powerhitchance','ai_megahitchance','ai_powermessupfactor','ai_megamessupfactor']
def load():
    data=Path(os.environ.get('EAGL_DATA',r'D:\_eagl\eagl EA PLAYGROUND\extra\more\eaplayground files\DATA'))/'files/data/db'
    names=(HERE.parent/'src/vlt_names.txt').read_text(encoding='utf-8').splitlines()+FIELDS+SPEED+ANGLES+AI+['dares_speed_rounds','dares_time','dares_endurance']
    db=vlt.load_database((data/'db.vlt').read_bytes(),(data/'db.bin').read_bytes(),{vlt.string_hash64(n):n for n in names})
    return db,{n:hashlib.sha256((data/n).read_bytes()).hexdigest() for n in ['db.vlt','db.bin']}
class Oracle(InputEmu):
    def __init__(self,exe,db):
        super().__init__(exe);self.db=db;self.collection='tunables';self.current=None;self.alloc=0x72000000;self.reads=[]
        self.hooks.update({0x802f45e4:self.key,0x802d6ee4:self.string_key,0x802f46bc:self.collection_string,0x802f4794:self.collection_key,0x802f4900:lambda e:None,0x802d8f1c:self.get,0x802f5e60:lambda e:None,0x802da404:lambda e:None,0x802d4a9c:self.array,0x80319a54:lambda e:self.set_result(OBJ),0x802cd4e8:lambda e:None})
        self.w32(OBJ,OBJ+0x1000);self.w32(OBJ+0x1050,0x803abea8)
        self.w32(0x806018d4,0x73001000)
        self.lookup=InputEmu(exe);self.lookup.hooks[0x802d4d10]=self.find_node
        self.collection_ptr={c['key']:0x74000000+i*0x1000 for i,c in enumerate(db['collections'].values())}
        self.by_pointer={self.collection_ptr[c['key']]:c for c in db['collections'].values()}
        self.attribute_ptr={}
        for p,c in self.by_pointer.items():
            self.lookup.w32(p+12,self.collection_ptr.get(vlt.string_hash64(c['parent'] or ''),0))
            for i,a in enumerate(c['attributes']):self.attribute_ptr[p+i*16+0x75000000]=a
    def find_node(self,e):
        p=e.r[3];key=(e.r[5]<<32)|e.r[6];e.r[3]=0
        for i,a in enumerate(self.by_pointer[p]['attributes']):
            if a['name_key']==key:e.r[3]=p+i*16+0x75000000;break
    def set_result(self,x):self.r[3]=x
    def cstr(self,p):
        if not p:return None
        b=bytearray()
        while self.rd(p,1)!=b'\0':b+=self.rd(p,1);p+=1
        return b.decode('latin1')
    def key(self,e):
        k=vlt.string_hash64(self.cstr(self.r[4]) or '');self.r[3]=k>>32;self.r[4]=k&0xffffffff
    def string_key(self,e):
        k=vlt.string_hash64(self.cstr(self.r[3]) or '');self.r[3]=k>>32;self.r[4]=k&0xffffffff
    def collection_string(self,e):self.collection=self.cstr(self.r[5]);self.r[3]=0x73000000
    def collection_key(self,e):self.collection=self.db['collections'][(self.r[7]<<32)|self.r[8]]['name'];self.r[3]=0x73000000
    def resolved(self,key):
        p=self.collection_ptr[vlt.string_hash64(self.collection)]
        node=self.lookup.call(0x802d4c60,[p,0,key>>32,key&0xffffffff,0x76000000])
        if not node:return None,None
        return self.by_pointer[self.lookup.r32(0x76000000)],self.attribute_ptr[node]
    def get(self,e):
        out=self.r[3];key=(self.r[5]<<32)|self.r[6];c,a=self.resolved(key)
        self.wr(out,bytes(16));self.current=a
        if a is None:return
        self.reads.append({'collection':self.collection,'source':c['name'],'attribute':a['name']})
        mem=self.db['_memory'];self.alloc+=0x100;node=self.alloc
        if a['flags']&2:
            self.w32(out+8,node);self.wr(node+15,b'\x02');raw=a['raw'];count,cap,size,_=struct.unpack('>HHHH',mem.read(raw,8));self.wr(node+32,mem.read(raw,8+cap*size))
        else:
            self.w32(out+12,node);self.wr(node,struct.pack('>I',a['raw']) if a['flags']&0x40 else mem.read(a['raw'],4))
    def array(self,e):self.r[3]+=32
    def block(self,start,end,registers):
        self.r=[0]*32;self.f=[0.0]*32;self.r[1]=0x7fff0000;self.r[2]=self.e.symbols['_SDA2_BASE_']['value'];self.r[13]=self.e.symbols['_SDA_BASE_']['value']
        for i,x in registers.items():self.r[i]=x
        self.w32(self.r[1]+0x140,0x43300000);self.w32(self.r[1]+0x148,0x43300000)
        pc=start;count=0
        while pc!=end:
            pc=self.step(pc);count+=1
            assert count<200000
    def selector(self,session_mode,dare):
        self.w32(OBJ+0x40,session_mode);self.w32(OBJ+0x48,dare)
        return self.cstr(self.call(0x8039ce38,[OBJ]))
def capture():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==PIN
    exe=rf.load();db,hashes=load();o=Oracle(exe,db)
    selectors=[{'session_mode':p,'dare':d,'collection':o.selector(p,d)} for p in [-1,0,1,2,4] for d in [-2147483648,-2,-1,*range(11),2147483647]]
    indices=[{'difficulty':d,'index':o.call(0x803abea8,[OBJ,d])} for d in [-2,-1,*range(7)]]
    multiplayer=[]
    for parameters in [[1,0,6,0,3],[2,3,12,0xdeadbeef,9],[0xffffffff,0xffffffff,0xffffffff,0xffffffff,0xffffffff]]:
        o.w32(OBJ+0xf8,OBJ+0x6000)
        for i,x in enumerate(parameters):o.w32(OBJ+0x6000+i*4,x)
        o.call(0x8039d080,[OBJ]);multiplayer.append(dict(parameters=parameters,fields=[o.r32(OBJ+x) for x in [0x3c,0x44,0x16c,0x170,0x174,0x178,0x17c,0x180]]))
    heights=[]
    for zone in range(3):
        for height in [0.0,1.25,-3.0,100.0]:
            o=Oracle(exe,db);o.w32(OBJ+0xac,zone);o.wr(OBJ+0x30f4,struct.pack('>f',height));o.block(0x8039d6fc,0x8039d738,{28:0,29:OBJ,31:OBJ+0x3000})
            heights.append(dict(zone=zone,pole_height_bits=o.r32(OBJ+0x160),height_bits=o.r32(OBJ+0xb8)))
    records=[]
    for dare in [-1,*range(9)]:
        for difficulty in range(4):
            o=Oracle(exe,db);collection=o.selector(1,dare);o.w32(OBJ+0x44,difficulty)
            o.call(0x8039cf90,[OBJ]);single=[o.r32(OBJ+x) for x in [0x16c,0x170,0x174,0x178,0x17c,0x180]]
            o.block(0x80397184,0x803972ec,{28:OBJ,29:difficulty,31:BASE})
            speed=[o.r32(OBJ+x) for x in [0x34c,0x350,0x354,0x358]];angles=[[o.r32(OBJ+x+i*4) for i in range(3)] for x in [0x360,0x36c,0x378,0x384]]
            o.block(0x8039d668,0x8039d738,{27:0x804dd268,28:0,29:OBJ+0x2000,30:difficulty,31:OBJ+0x3000})
            ball=[o.r32(OBJ+0x2000+x) for x in [0x14c,0x150,0x154,0x158]]
            o.w32(OBJ+0x4000,0);o.call(0x80395354,[OBJ+0x5000,1,difficulty,OBJ+0x4000]);ai=list(o.rd(OBJ+0x5078,7))
            records.append(dict(dare=dare,difficulty=difficulty,collection=collection,single=single,speed_bits=speed,angle_bits=angles,ball_speed_bits=ball,ai=ai,reads=o.reads))
    return dict(elf_sha256=PIN,corpus_sha256=hashes,selectors=selectors,indices=indices,multiplayer=multiplayer,heights=heights,records=records)
def main():
    p=argparse.ArgumentParser();p.add_argument('--check',action='store_true');p.add_argument('--rust-check',action='store_true');a=p.parse_args();result=capture()
    if a.check:assert json.loads(FIX.read_text(encoding='utf-8'))==result,'fixture differs'
    else:FIX.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    if a.rust_check:
        dep=HERE.parent/'target/debug/deps';lib=next(dep.glob('libserde_json*.rlib'))
        data=Path(os.environ.get('EAGL_DATA',r'D:\_eagl\eagl EA PLAYGROUND\extra\more\eaplayground files\DATA'))
        with tempfile.NamedTemporaryFile(suffix='.rs',delete=False,mode='w',encoding='utf-8') as f:
            src=Path(f.name);exe=src.with_suffix('.exe')
            f.write(f'#[path="{(HERE.parent/"src/vlt.rs").as_posix()}"] mod vlt;\n#[path="{(HERE.parent/"src/tetherball_tuning.rs").as_posix()}"] mod tetherball_tuning;\nmod bridge {{pub fn data_root()->std::path::PathBuf{{std::path::PathBuf::from(r#"{data}"#)}}}}\n')
        try:
            subprocess.run(['rustc','--edition=2024','--crate-name','tetherball_tuning_check','--test',str(src),'--extern',f'serde_json={lib}','-L',f'dependency={dep}','-o',str(exe)],check=True)
            subprocess.run([str(exe),'tetherball_tuning::tests::original_tuning_fixture'],check=True)
        finally:
            for path in [src,exe,exe.with_suffix('.pdb')]:path.unlink(missing_ok=True)
    print(f"PASS {len(result['selectors'])} selectors, {len(result['indices'])} difficulty indices, {len(result['records'])} lifecycle tuning records")
if __name__=='__main__':main()
