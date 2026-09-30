"""Execute original playground Jump and LocalCharacterControl::Update differential.
The jump queue producer and consumer run their full original instruction bodies.
Only paired-single register restores are ignored (scalar lfd restores follow).
"""
import hashlib,json,sys
from pathlib import Path
from ppc_emu2 import Emu,M
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/jump_command_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'

class JumpEmu(Emu):
    def step(self,pc):
        w=self.word(pc);op=w>>26
        d,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31
        if op==4:  # paired-single register restores only on the exercised path
            assert pc in (0x802ef4d4,0x802ef4e0,0x802ef4ec,0x802ef4f8,0x802ef504,0x802ef510,0x802ef51c,0x802ef528,0x802ef534)
            return pc+4
        if op==31 and ((w>>1)&1023)==183:  # stwux
            address=(self.r[a]+self.r[b])&M
            self.w32(address,self.r[d]);self.r[a]=address
            return pc+4
        if op==63 and ((w>>1)&1023)==0:  # fcmpu
            x,y=self.f[a],self.f[b]
            self.cr[(w>>23)&7]=8 if x<y else 4 if x>y else 2
            return pc+4
        return super().step(pc)

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load();vectors=[]
    for dt in (0,1,16,60,200):
        runs=[]
        for jump in (False,True):
            emu=JumpEmu(exe);ctrl=0x71000000;move=0x71001000;state=0x71002000
            emu.w32(ctrl+0xbc,1)
            emu.w32(move+4,0x40500000)  # sentinel speed 3.25; should remain untouched
            if jump:emu.call(0x802eeac8,(ctrl,dt),(0.,0.))
            count,kind=emu.r32(ctrl+0xa4),emu.r32(ctrl+4)
            trace=[]
            emu.trace=lambda pc,w:trace.append(pc)
            emu.call(0x802eeb28,(ctrl,dt,move,state))
            runs.append(dict(jump=jump,queued_count=count,queued_kind=kind,
                remaining_count=emu.r32(ctrl+0xa4),idle_ms=emu.r32(ctrl+0xb4),
                movement_words=[emu.r32(move+4*i) for i in range(64)],
                state_words=[emu.r32(state+4*i) for i in range(64)],
                control_words=[emu.r32(ctrl+off) for off in (0xac,0xb0,0xb4,0xb8)],
                command_dispatch_trace=[pc for pc in trace if 0x802eebf4<=pc<=0x802eec28 or pc==0x802ef09c]))
        assert runs[0]['movement_words']==runs[1]['movement_words']
        assert runs[0]['state_words']==runs[1]['state_words']
        assert runs[0]['control_words']==runs[1]['control_words']
        assert runs[1]['queued_count']==1 and runs[1]['queued_kind']==4 and runs[1]['remaining_count']==0
        assert runs[1]['command_dispatch_trace']==[0x802eebf4,0x802eebf8,0x802eebfc,0x802eec00,0x802eec1c,0x802eec20,0x802eec24,0x802eec28,0x802ef09c]
        vectors.append(dict(dt_ms=dt,runs=runs))
    return dict(elf_sha256=SHA,vectors=vectors)

if __name__=='__main__':
    result=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text())==result,'Jump command fixture differs from original PPC'
        print('Verified 5 original playground jump/no-command differentials: queue consumed, state unchanged')
    else:
        OUT.parent.mkdir(parents=True,exist_ok=True);OUT.write_text(json.dumps(result,indent=2)+'\n')
        print('Captured 5 original playground jump/no-command differentials')
