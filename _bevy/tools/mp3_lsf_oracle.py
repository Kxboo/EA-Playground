"""Compare MPEG-2 scale factors and spectral preprocessing with original PowerPC.

The original GetLsfScaleFactors/GetLsfScaleData, Dequantize and Reorder instruction
words execute in ppc_emu2. Only the bit-input primitive and elementary ScaleSamples
vector multiply are hooked. Runtime scale table values are initialized as powers
of two; no expected partitions, preflag, scale mapping or band ranges are guessed.
Default builds a tiny rustc test harness; --harness accepts an existing test binary.
Does not require game DATA, cargo, or commit any original bytes.
"""
import argparse, hashlib, os, shutil, struct, subprocess, uuid
from pathlib import Path
import re_functions as rf
from ppc_emu2 import Emu, f32

ROOT=Path(__file__).resolve().parents[1]
# Full pinned hash (kept explicit to guard the provenance of every executed word).
PINNED='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
OBJ=0x71000000
BUF=0x71001000
OUT=0x71002000
SCALE=0x71003000
INPUT=bytes([0xa5,0x3c,0xe7,0x19])*64

class Oracle(Emu):
    def step(self,pc):
        word=self.word(pc);op=word>>26
        rd,ra,rb=(word>>21)&31,(word>>16)&31,(word>>11)&31
        xo=(word>>1)&1023
        if op==63 and xo==0: # fcmpu
            x,y=self.f[ra],self.f[rb]
            self.cr[(word>>23)&7]=8 if x<y else 4 if x>y else 2
            return pc+4
        if op==31 and xo==407: # sthx
            address=((self.r[ra] if ra else 0)+self.r[rb])&0xffffffff
            self.wr(address,struct.pack('>H',self.r[rd]&0xffff));return pc+4
        if op==31 and xo==343: # lhax
            address=((self.r[ra] if ra else 0)+self.r[rb])&0xffffffff
            self.r[rd]=struct.unpack('>h',self.rd(address,2))[0]&0xffffffff;return pc+4
        if op==31 and xo==695: # stfsux
            address=(self.r[ra]+self.r[rb])&0xffffffff
            self.wr(address,struct.pack('>f',self.f[rd]));self.r[ra]=address
            return pc+4
        return super().step(pc)

def object_fields(e,block,sr=3,scale=0,pre=0):
    e.wr(OBJ,bytes(0x300))
    e.wr(OBJ+0x3c,bytes([sr]))
    e.wr(OBJ+0x5f,bytes([int(block>0),2 if block>0 else 0,int(block==2)]))
    e.wr(OBJ+0x6b,bytes([pre]));e.w32(OBJ+0x6c,scale)
    e.wr(OBJ+0x68,bytes([0,1,2]))

def scaler(exe,compress,block):
    e=Oracle(exe);object_fields(e,block)
    e.wr(OBJ+0x5c,struct.pack('>H',compress))
    position=0
    def bits(e):
        nonlocal position
        n=e.r[4];value=0
        for _ in range(n):
            value=(value<<1)|((INPUT[position>>3]>>(7-(position&7)))&1);position+=1
        e.r[3]=value
    e.hooks[0x8028165c]=bits
    e.call(0x80282488,(OBJ,0,0))
    values=[struct.unpack('>H',e.rd(OBJ+0xb8+2*s,2))[0] for s in range(22)]
    values.extend(struct.unpack('>H',e.rd(OBJ+0xb8+0x2e+2*s+0x1a*w,2))[0] for s in range(13) for w in range(3))
    return [position,e.rd(OBJ+0x6b,1)[0],values]

def spectral(exe,sr,block,scale,pre,reorder=False):
    e=Oracle(exe);object_fields(e,block,sr,scale,pre)
    if reorder:
        e.wr(BUF,b''.join(struct.pack('>f',float(i)) for i in range(576)))
        e.wr(OUT,bytes(576*4));e.call(0x80285128,(OBJ,0,0,BUF,OUT))
        return [int(v) for v in struct.unpack('>576f',e.rd(OUT,576*4))]
    e.wr(BUF,b''.join(struct.pack('>f',1. if i%2==0 else -1.) for i in range(576)))
    e.w32(0x805fdd90,SCALE)
    e.wr(SCALE,b''.join(struct.pack('>ff',2.**(-i/2),0.) for i in range(256)))
    for s in range(22):e.wr(OBJ+0xb8+2*s,struct.pack('>H',s%5))
    for s in range(13):
        for w in range(3):e.wr(OBJ+0xb8+0x2e+2*s+0x1a*w,struct.pack('>H',0 if s==12 else (s+w)%5))
    def multiply(e):
        ptr,n=e.r[3],e.r[4];factor=e.f[1]
        for i in range(n):
            value=struct.unpack('>f',e.rd(ptr+i*4,4))[0]
            e.wr(ptr+i*4,struct.pack('>f',f32(value*factor)))
    e.hooks[0x802843a8]=multiply
    e.call(0x802844e4,(OBJ,0,0,BUF))
    return list(struct.unpack('>576I',e.rd(BUF,576*4)))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--harness',type=Path)
    args=parser.parse_args()
    actual=hashlib.sha256((ROOT.parent/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()
    assert actual==PINNED,actual
    temporary=None
    try:
        harness=args.harness
        if harness is None:
            temporary=ROOT/'exports'/('mp3-lsf-oracle-'+uuid.uuid4().hex)
            temporary.mkdir(parents=True)
            source=temporary/'harness.rs';harness=temporary/'harness.exe'
            source.write_text('\n'.join(f'#[path="{(ROOT/"src"/file).as_posix()}"] mod {name};' for file,name in [('mp3_tables.rs','mp3_tables'),('mp3.rs','mp3')]),encoding='utf-8')
            subprocess.run(['rustc','--edition','2024','--test','-O',str(source),'-o',str(harness)],check=True,capture_output=True,text=True)
        env=dict(os.environ,EAGL_LSF_ORACLE='1')
        result=subprocess.run([str(harness),'lsf_original_vectors','--nocapture'],env=env,capture_output=True,text=True,check=True)
        rust={}
        for line in result.stdout.splitlines():
            if line.startswith('LSFV '):
                _,compress,block,pos,pre,values=line.split()
                rust[('LSFV',int(compress),int(block))]=[int(pos),int(pre),list(map(int,values.split(',')))]
            elif line.startswith(('DQFV ','ROFV ')):
                kind,sr,block,scale,pre,values=line.split()
                rust[(kind,int(sr),int(block),int(scale),int(pre))]=list(map(int,values.split(',')))
        exe=rf.load()
        for compress in range(512):
            for block in range(3):
                key=('LSFV',compress,block)
                assert rust[key]==scaler(exe,compress,block),f'scale mismatch {key}'
        print('Verified 1536 LSF scale-factor vectors (all 512 compression values x long/short/mixed), original PowerPC',flush=True)
        for sr in range(6):
            for block in range(3):
                for scale in range(2):
                    for pre in range(2):
                        key=('DQFV',sr,block,scale,pre)
                        expected=spectral(exe,sr,block,scale,pre)
                        assert rust[key]==expected,f'dequant mismatch {key}: '+str([(i,a,b) for i,(a,b) in enumerate(zip(rust[key],expected)) if a!=b][:5])
                        if block:
                            key=('ROFV',sr,block,scale,pre)
                            assert rust[key]==spectral(exe,sr,block,scale,pre,True),f'reorder mismatch {key}'
        print('Verified 72 spectral scaling and 48 reorder vectors (576 lines each), MPEG-1/MPEG-2 bands, original PowerPC',flush=True)
        print('ELF sha256 '+actual)
    finally:
        if temporary:
            assert temporary.resolve().is_relative_to((ROOT/'exports').resolve())
            shutil.rmtree(temporary)

if __name__=='__main__':main()
