"""Original PPC button timer instruction slice + complete UpdateInput event dispatcher.

Hardware GetInput is replaced at its call boundary with original timer instructions
(0x80329fdc..0x8032a09c). The branch at 0x8032d240 skips ancillary motor/frontend
processing and jumps to the original epilogue, after CSV event/context decisions.
The externally writable +0x248 deferred-pop flag remains zero in this scope.
No event, timer, modifier, transition or threshold predicate is a Python model.
"""
import hashlib,json,random,struct,sys
from pathlib import Path
from ppc_emu2 import Emu,M,sx
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/controller_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
OBJ=0x71000000
ARRAYS=[0x71100000,0x71100100,0x71100200,0x71100300]

class ControllerEmu(Emu):
    def step(self,pc):
        if pc==0x8032d240:
            assert self.word(pc)==0x801d0140
            return 0x8032d990
        word=self.word(pc);op=word>>26
        d,a,b=(word>>21)&31,(word>>16)&31,(word>>11)&31
        if op==31 and ((word>>1)&1023) in (151,215):
            addr=(self.r[a] if a else 0)+self.r[b]
            if ((word>>1)&1023)==151:self.w32(addr,self.r[d])
            else:self.wr(addr,bytes([self.r[d]&255]))
            return pc+4
        return super().step(pc)

    def hardware_input(self):
        # GetInput ABI: r3=this r4=pad r5=up* r6=down* r7=held* r8=dt.
        saved=self.r[:]; old=saved[3]
        self.r[22]=old;self.r[30]=self.held
        self.r[24]=saved[5];self.r[25]=saved[6];self.r[26]=saved[7];self.r[27]=saved[8]
        pc=0x80329fdc
        while pc!=0x8032a09c:pc=self.step(pc)
        self.r=saved


def binding(action,kind,button=4,state=3,transition=31,required=(0,0),forbidden=(0,0)):
    return dict(action=action,state=state,transition=transition,kind=kind,required=list(required),forbidden=list(forbidden),button=button)


def session(exe,rows,frames,state=3,pad=0,debug=True,freecam=True):
    emu=ControllerEmu(exe)
    emu.w32(OBJ+0x254,pad)
    for off,addr in zip([0x258,0x25c,0x260,0x268],ARRAYS):emu.w32(OBJ+off,addr)
    for addr in ARRAYS[:2]:
        for i in range(14):emu.w32(addr+4*i,M)
    for i in range(64):emu.w32(OBJ+0x144+4*i,state)
    emu.w32(OBJ+0xca6c,len(rows))
    for i,row in enumerate(rows):
        addr=OBJ+0x26c+i*100
        for off,k in [(0,'action'),(0x44,'state'),(0x48,'transition'),(0x4c,'kind'),(0x60,'button')]:emu.w32(addr+off,row[k])
        for base,k in [(0x50,'required'),(0x58,'forbidden')]:
            for j,value in enumerate(row[k]):emu.w32(addr+base+j*4,value)
    emu.wr(0x80601c38,bytes([debug]))
    emu.wr(0x80601c39,bytes([freecam]))
    emu.hooks[0x80329cb4]=lambda e:e.hardware_input()
    result=[]
    for frame in frames:
        if frame.get('pop'):emu.call(0x8032dc20,(OBJ,))
        emu.held=frame['held']
        emu.call(0x8032cb58,(OBJ,frame['ms']))
        depth=emu.r32(OBJ+0x244)
        # Only valid stack paths are generated; underflow is an original unchecked bug.
        assert depth<64
        events=[[i,emu.r32(ARRAYS[3]+i*8+4)] for i in range(189) if emu.rd(ARRAYS[3]+i*8,1)[0]]
        result.append(dict(frame,current_state=emu.r32(OBJ+0x144+4*depth),events=events,
            since_down=[emu.r32(ARRAYS[0]+4*i) for i in range(14)],
            between_down=[emu.r32(ARRAYS[1]+4*i) for i in range(14)],
            held_ms=[emu.r32(ARRAYS[2]+4*i) for i in range(14)]))
    return dict(bindings=rows,state=state,pad_index=pad,debug_enabled=debug,freecam_enabled=freecam,frames=result)


def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load();rng=random.Random(0x8032cb58);sessions=[]
    rows=[binding(i,i) for i in range(8)]
    rows += [binding(8,1,required=(5,0)),binding(9,1,forbidden=(5,0)),binding(10,3,button=0),binding(11,9),binding(190,9)]
    frames=[]
    for held,ms in [(0,16),(16,16),(16,119),(16,1),(16,59),(16,1),(16,819),(16,1),(0,16),(16,16),(0,16),(0,500),(16,0),(0,180),(48,1),(0,1)]:frames.append(dict(held=held,ms=ms))
    for _ in range(100):frames.append(dict(held=rng.randrange(1<<14),ms=rng.choice([0,1,16,60,119,120,179,180,499,500,1000,-1,0x7fffffff])))
    sessions.append(session(exe,rows,frames))
    for button in range(14):
        # Exact threshold neighbors, release duration retains last held timer.
        rows=[binding(i,i,button) for i in range(8)]
        fs=[dict(held=0,ms=501),dict(held=1<<button,ms=16)]
        fs += [dict(held=1<<button,ms=dt) for dt in [119,1,59,1,819,1]]
        fs += [dict(held=0,ms=16),dict(held=1<<button,ms=16),dict(held=0,ms=16)]
        sessions.append(session(exe,rows,fs))
    # Row ordering, pending-state wildcard debounce and RETURN pop.
    rows=[binding(0,1,4,28,6),binding(1,1,4,28,6),binding(2,1,5,6,27),binding(3,3,4,3),binding(4,1,6,28,8),binding(5,1,7,8,27)]
    fs=[dict(held=h,ms=16) for h in [16,0,32,0,64,0,128,0,16,0,32]]
    for pad,debug,freecam in [(0,True,True),(1,True,True),(0,False,False)]:sessions.append(session(exe,rows,fs,pad=pad,debug=debug,freecam=freecam))
    return dict(elf_sha256=SHA,sessions=sessions)

if __name__=='__main__':
    result=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text())==result,'Controller vectors differ from original PPC'
        print('Verified',sum(len(s['frames']) for s in result['sessions']),'original controller frames')
    else:
        OUT.parent.mkdir(parents=True,exist_ok=True);OUT.write_text(json.dumps(result,indent=2)+'\n')
        print('Captured',sum(len(s['frames']) for s in result['sessions']),'original controller frames')
