"""Run the game's own MicroTalk decoder (decodemut/initmut, PowerPC) on the first blocks of a speech stream."""
import sys,struct,json
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parent.parent
sys.path[:0]=[str(HERE),str(REPO/'Remaster'/'src')]
import ppc_emu2,research
e=ppc_emu2.Emu();S=e.e.symbols
INIT=S['initmut__Q23Snd10CMTBLKDecfFPUcPQ23Snd15UTALKSTATE_CODAi']['value']
DEC=S['decodemut__3SndFPQ23Snd15UTALKSTATE_CODA']['value']
STATE=0x10000000;DATA=0x20000000
# memcpy through the function pointer at _SDA_BASE_-0x6158
FP=e.e.symbols['_SDA_BASE_']['value']-0x6158
e.w32(FP,0xDEAD1000)
def memcpy(emu):
    d,s,n=emu.r[3],emu.r[4],emu.r[5];emu.wr(d,emu.rd(s,n))
e.hooks[0xDEAD1000]=memcpy

def blocks(path):
    d,_=research.read_virtual(path)
    p=struct.unpack('<I',d[4:8])[0];out=[]
    while p+8<=len(d):
        tag=d[p:p+4];sz=struct.unpack_from('<I',d,p+4)[0]
        if tag==b'SCDl':out.append(d[p+8:p+sz])
        if sz<8:break
        p+=sz
    return out

def run(path,nblocks=1,frames=3,offset=8,verbose=True):
    bl=blocks(path)
    res=[]
    for bi,pl in enumerate(bl[:nblocks]):
        want=struct.unpack('>I',pl[:4])[0];body=pl[offset:]
        e.wr(DATA,body)
        if bi==0:
            e.call(INIT,(0,DATA,STATE,1))
        else:
            e.call(INIT,(0,DATA,STATE,0))
        for f in range(frames):
            e.call(DEC,(STATE,),max_steps=5_000_000)
            ptr=e.r32(STATE);fr=struct.unpack('>432f',e.rd(STATE+0x684,432*4))
            pk=max(abs(x) for x in fr)
            res.append((bi,f,ptr-DATA,pk,fr))
            if verbose:print(f'block {bi} frame {f}: ptr {ptr-DATA}/{len(body)} peak {pk:.1f} first {fr[:4]}')
            # next frame: re-init bit reader at ptr (byte after the preloaded lookahead), as CMTBLKDec::Decode does
            e.w32(STATE+8,8);e.w32(STATE,ptr+1);e.w32(STATE+4,e.rd(ptr,1)[0])
    return res
if __name__=='__main__':
    src=[r for r in json.load(open(REPO/'Remaster'/'research'/'coverage.json',encoding='utf-8'))['records'] if r['source'].replace(chr(92),'/').endswith('spchdat.viv::Challenge.dat')][0]['source']
    run(src,nblocks=int(sys.argv[1]) if len(sys.argv)>1 else 1,frames=int(sys.argv[2]) if len(sys.argv)>2 else 4)
