"""Tiny PowerPC (Gekko integer subset) interpreter used to run leaf functions from playgroundz.elf.

Purpose: reference-execute recovered code (e.g. Attrib::hash64) to validate independent Python/Rust
reimplementations.  Supports only the integer instructions those leaf functions use; anything else
raises, so a silent mis-emulation is not possible.  Calls to the register save/restore helpers
(`_savegpr_N`/`_restgpr_N`) are treated as no-ops.
"""
import sys,re,struct
from pathlib import Path
HERE=Path(__file__).resolve().parent
sys.path[:0]=[str(HERE),str(HERE.parent.parent/'Remaster'/'research'/'deps')]
import capstone
import re_functions as rf
M32=0xFFFFFFFF

def rol(v,n):n&=31;return ((v<<n)|(v>>(32-n)))&M32 if n else v
def mask(mb,me):
    """PowerPC MASK(mb,me): bit 0 is the most significant bit; wraps when mb>me."""
    def bits(lo,hi):return ((1<<(hi-lo+1))-1)<<(31-hi)
    return bits(mb,me) if mb<=me else (bits(mb,31)|bits(0,me))
def sx16(v):v&=0xFFFF;return v-0x10000 if v&0x8000 else v

class Emu:
    def __init__(self,exe=None):
        self.e=exe or rf.load();self.cs=capstone.Cs(capstone.CS_ARCH_PPC,capstone.CS_MODE_32|capstone.CS_MODE_BIG_ENDIAN)
        self.r=[0]*32;self.ca=0;self.ctr=0;self.lr=0;self.cr=[0,0,0]  # lt,gt,eq of cr0
        self.mem={}  # sparse writes
        self.cache={}
    def rd(self,a,n):
        out=bytearray()
        for i in range(n):
            x=a+i
            if x in self.mem:out.append(self.mem[x])
            else:out+=bytes(self.e.read(x,1))
        return bytes(out)
    def wr(self,a,b):
        for i,x in enumerate(b):self.mem[a+i]=x
    def ins(self,pc):
        if pc not in self.cache:
            w=bytes(self.e.read(pc,4));i=next(self.cs.disasm(w,pc),None)
            if i is None:raise RuntimeError(f'undecodable {w.hex()} at {pc:#x}')
            self.cache[pc]=(i.mnemonic,i.op_str,w)
        return self.cache[pc]
    def call(self,name,args,max_steps=200000):
        s=self.e.symbols[name];pc=s['value'];end_lr=0xDEAD0000
        self.r=[0]*32
        for i,a in enumerate(args):self.r[3+i]=a&M32
        self.r[1]=0x7FFF0000;self.lr=end_lr;steps=0
        while pc!=end_lr:
            steps+=1
            if steps>max_steps:raise RuntimeError('step limit')
            pc=self.step(pc)
        return self.r[3],self.r[4]
    def step(self,pc):
        m,o,w=self.ins(pc);r=self.r;nxt=pc+4
        ops=[x.strip() for x in o.split(',')] if o else []
        def R(x):return int(x[1:])
        def imm(x):return int(x,0)
        def setrc(v):
            v&=M32;s=v-(1<<32) if v&0x80000000 else v;self.cr=[s<0,s>0,s==0]
        rc=m.endswith('.');mm=m.rstrip('.')
        if mm in('li','lis','addi','addis'):
            if mm=='li':r[R(ops[0])]=imm(ops[1])&M32
            elif mm=='lis':r[R(ops[0])]=(imm(ops[1])<<16)&M32
            else:
                a=0 if ops[1]=='0' else r[R(ops[1])];v=imm(ops[2]);r[R(ops[0])]=(a+(v<<16 if mm=='addis' else v))&M32
        elif mm=='mr':r[R(ops[0])]=r[R(ops[1])]
        elif mm in('addc','adde','subfc','subfe'):
            d,a,b=R(ops[0]),r[R(ops[1])],r[R(ops[2])]
            if mm=='addc':t=a+b
            elif mm=='adde':t=a+b+self.ca
            elif mm=='subfc':t=(~a&M32)+b+1
            else:t=(~a&M32)+b+self.ca
            self.ca=1 if t>M32 else 0;r[d]=t&M32
            if rc:setrc(r[d])
        elif mm=='addze':
            t=r[R(ops[1])]+self.ca;self.ca=1 if t>M32 else 0;r[R(ops[0])]=t&M32
        elif mm=='xor':r[R(ops[0])]=r[R(ops[1])]^r[R(ops[2])]
        elif mm=='slwi':r[R(ops[0])]=(r[R(ops[1])]<<imm(ops[2]))&M32
        elif mm=='srwi':r[R(ops[0])]=r[R(ops[1])]>>imm(ops[2])
        elif mm=='rotlwi':r[R(ops[0])]=rol(r[R(ops[1])],imm(ops[2]))
        elif mm=='srawi':
            v=r[R(ops[1])];sh=imm(ops[2]);sv=v-(1<<32) if v&0x80000000 else v
            self.ca=1 if (sv<0 and sh and (v&((1<<sh)-1))) else 0;r[R(ops[0])]=(sv>>sh)&M32
        elif mm in('rlwimi','rlwinm'):
            d,s=R(ops[0]),r[R(ops[1])];sh,mb,me=imm(ops[2]),imm(ops[3]),imm(ops[4])
            rot=rol(s,sh);mk=mask(mb,me)
            if mm=='rlwimi':r[d]=(rot&mk)|(r[d]&~mk&M32)
            else:r[d]=rot&mk
        elif mm=='lbz':
            off,base=ops[1].split('(');r[R(ops[0])]=self.rd((r[R(base.rstrip(')'))]+imm(off))&M32,1)[0]
        elif mm=='lwz':
            off,base=ops[1].split('(');r[R(ops[0])]=struct.unpack('>I',self.rd((r[R(base.rstrip(')'))]+imm(off))&M32,4))[0]
        elif mm=='lwzx':
            a=(0 if ops[1]=='0' else r[R(ops[1])])+r[R(ops[2])];r[R(ops[0])]=struct.unpack('>I',self.rd(a&M32,4))[0]
        elif mm in('stw','stwu'):
            off,base=ops[1].split('(');b=R(base.rstrip(')'));a=(r[b]+imm(off))&M32;self.wr(a,struct.pack('>I',r[R(ops[0])]))
            if mm=='stwu':r[b]=a
        elif mm=='divwu':r[R(ops[0])]=r[R(ops[1])]//r[R(ops[2])] if r[R(ops[2])] else 0
        elif mm=='cmplwi':
            a=r[R(ops[0])];b=imm(ops[1])&0xFFFF;self.cr=[a<b,a>b,a==b]
        elif mm=='mflr':r[R(ops[0])]=self.lr
        elif mm=='mtlr':self.lr=r[R(ops[0])]
        elif mm=='mtctr':self.ctr=r[R(ops[0])]
        elif mm=='blr':return self.lr
        elif mm=='bctr':return self.ctr
        elif mm in('b','bl'):
            t=imm(o)
            if mm=='bl':
                name=self.e.addresses.get(t,'')
                if not (name.startswith('_savegpr') or name.startswith('_restgpr')):raise RuntimeError(f'call to {name or hex(t)} not emulated')
            else:return t
        elif mm=='blt':return imm(o) if self.cr[0] else nxt
        elif mm=='bgt':return imm(o) if self.cr[1] else nxt
        elif mm=='beq':return imm(o) if self.cr[2] else nxt
        elif mm=='bge':return imm(o) if not self.cr[0] else nxt
        elif mm=='ble':return imm(o) if not self.cr[1] else nxt
        elif mm=='bne':return imm(o) if not self.cr[2] else nxt
        elif mm=='bdnz':
            self.ctr=(self.ctr-1)&M32;return imm(o) if self.ctr else nxt
        else:raise RuntimeError(f'unsupported instruction {m} {o} at {pc:#x}')
        return nxt

def hash64_ref(emu,data:bytes,level=0xABCDEF0011223344):
    buf=0x10000000;emu.wr(buf,data+b'\0')
    hi,lo=emu.call('hash64__6AttribFPCUcUiUx',[buf,len(data),level>>32,level&M32]);return (hi<<32)|lo
