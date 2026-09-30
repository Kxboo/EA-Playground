"""PowerPC (Gekko) interpreter with integer + single-precision float support, decoding raw instruction words.

Used as a reference oracle: run functions of playgroundz.elf (e.g. the MicroTalk decoder) on prepared memory and
compare the results with the Rust reimplementations.  Unsupported opcodes raise, so silent mis-emulation is not
possible.  Paired-single instructions (only used by the compiler to spill f31.. in prologues) are treated as no-ops.
"""
import sys,struct
from pathlib import Path
HERE=Path(__file__).resolve().parent
sys.path[:0]=[str(HERE)]
import re_functions as rf
M=0xFFFFFFFF

def sx(v,bits):v&=(1<<bits)-1;return v-(1<<bits) if v>>(bits-1) else v
def rol(v,n):n&=31;return ((v<<n)|(v>>(32-n)))&M if n else v
def mask(mb,me):
    def bits(lo,hi):return ((1<<(hi-lo+1))-1)<<(31-hi)
    return bits(mb,me) if mb<=me else (bits(mb,31)|bits(0,me))
def f32(x):
    try:return struct.unpack('>f',struct.pack('>f',x))[0]
    except OverflowError:return float('inf') if x>0 else float('-inf')

class Emu:
    def __init__(self,exe=None):
        self.e=exe or rf.load();self.r=[0]*32;self.f=[0.0]*32;self.ca=0;self.ctr=0;self.lr=0
        self.cr=[0]*8  # 4-bit fields lt,gt,eq,so
        self.mem={};self.hooks={};self.steps=0;self.trace=None
    # memory -------------------------------------------------------------------------------------------------------
    def rd(self,a,n):
        a&=M;out=bytearray()
        for i in range(n):
            x=a+i
            if x in self.mem:out.append(self.mem[x])
            else:out+=bytes(self.e.read(x,1)) if 0x80000000<=x<0x81000000 else b'\0'
        return bytes(out)
    def wr(self,a,b):
        a&=M
        for i,x in enumerate(b):self.mem[a+i]=x
    def r32(self,a):return struct.unpack('>I',self.rd(a,4))[0]
    def w32(self,a,v):self.wr(a,struct.pack('>I',v&M))
    # helpers ------------------------------------------------------------------------------------------------------
    def setcr0(self,v):
        s=sx(v,32);self.cr[0]=(8 if s<0 else 0)|(4 if s>0 else 0)|(2 if s==0 else 0)
    def word(self,pc):return struct.unpack('>I',bytes(self.e.read(pc,4)))[0]
    def crbit(self,bi):return (self.cr[bi>>2]>>(3-(bi&3)))&1
    def branch_cond(self,bo,bi):
        if not(bo&4):
            self.ctr=(self.ctr-1)&M
            ctr_ok=(self.ctr!=0)^bool(bo&2)
        else:ctr_ok=True
        cond_ok=True if bo&16 else (self.crbit(bi)==((bo>>3)&1))
        return ctr_ok and cond_ok
    def call(self,addr,args=(),fargs=(),max_steps=2_000_000,sp=0x7FFF0000):
        self.r=[0]*32;self.f=[0.0]*32
        for i,a in enumerate(args):self.r[3+i]=a&M
        for i,a in enumerate(fargs):self.f[1+i]=a
        self.r[1]=sp;self.lr=0xDEAD0000;pc=addr;self.steps=0
        sy=self.e.symbols
        self.r[2]=sy['_SDA2_BASE_']['value'];self.r[13]=sy['_SDA_BASE_']['value']
        while pc!=0xDEAD0000:
            self.steps+=1
            if self.steps>max_steps:raise RuntimeError('step limit')
            pc=self.step(pc)
        return self.r[3]
    def step(self,pc):
        w=self.word(pc);op=w>>26;r=self.r;f=self.f;nxt=pc+4
        rd=(w>>21)&31;ra=(w>>16)&31;rb=(w>>11)&31;imm=sx(w,16);uimm=w&0xFFFF;rc=w&1
        A=lambda:0 if ra==0 else r[ra]
        if self.trace:self.trace(pc,w)
        if op==14:r[rd]=(A()+imm)&M
        elif op==15:r[rd]=(A()+(imm<<16))&M
        elif op==8:t=(~r[ra]&M)+(imm&M)+1;self.ca=1 if t>M else 0;r[rd]=t&M
        elif op==7:r[rd]=(sx(r[ra],32)*imm)&M
        elif op==26:r[ra]=r[rd]^uimm
        elif op==27:r[ra]=r[rd]^(uimm<<16)
        elif op==24:r[ra]=r[rd]|uimm
        elif op==25:r[ra]=r[rd]|(uimm<<16)
        elif op==28:r[ra]=r[rd]&uimm;self.setcr0(r[ra])
        elif op==21:
            sh=(w>>11)&31;mb=(w>>6)&31;me=(w>>1)&31
            r[ra]=rol(r[rd],sh)&mask(mb,me)
            if rc:self.setcr0(r[ra])
        elif op==20:
            sh=(w>>11)&31;mb=(w>>6)&31;me=(w>>1)&31;mk=mask(mb,me)
            r[ra]=(rol(r[rd],sh)&mk)|(r[ra]&~mk&M)
        elif op==11: # cmpi
            a=sx(r[ra],32);self.cr[(w>>23)&7]=(8 if a<imm else 0)|(4 if a>imm else 0)|(2 if a==imm else 0)
        elif op==10: # cmpli
            a=r[ra];self.cr[(w>>23)&7]=(8 if a<uimm else 0)|(4 if a>uimm else 0)|(2 if a==uimm else 0)
        elif op==32:r[rd]=self.r32(A()+imm)
        elif op==33:a=(r[ra]+imm)&M;r[rd]=self.r32(a);r[ra]=a
        elif op==34:r[rd]=self.rd(A()+imm,1)[0]
        elif op==36:self.w32(A()+imm,r[rd])
        elif op==37:a=(r[ra]+imm)&M;self.w32(a,r[rd]);r[ra]=a
        elif op==38:self.wr(A()+imm,bytes([r[rd]&255]))
        elif op==44:self.wr(A()+imm,struct.pack('>H',r[rd]&0xFFFF))
        elif op==40:r[rd]=struct.unpack('>H',self.rd(A()+imm,2))[0]
        elif op==42:r[rd]=struct.unpack('>h',self.rd(A()+imm,2))[0]&M
        elif op==48:f[rd]=struct.unpack('>f',self.rd(A()+imm,4))[0]
        elif op==50:f[rd]=struct.unpack('>d',self.rd(A()+imm,8))[0]
        elif op==52:self.wr(A()+imm,struct.pack('>f',f[rd]) if abs(f[rd])<3.4e38 or f[rd]!=f[rd] else struct.pack('>f',float('inf') if f[rd]>0 else float('-inf')))
        elif op==54:self.wr(A()+imm,struct.pack('>d',f[rd]))
        elif op in(56,57,60,61):pass  # paired-single load/store (register spills only)
        elif op==18:
            li=w&0x03FFFFFC;li=li-0x04000000 if li&0x02000000 else li
            tgt=(li if w&2 else pc+li)&M
            if w&1:
                self.lr=nxt
                name=self.e.addresses.get(tgt,'')
                if tgt in self.hooks:self.hooks[tgt](self);return nxt
                if name.startswith('_savegpr_'):
                    n=int(name.split('_')[-1]);base=r[11]
                    for i in range(n,32):self.w32(base-4*(32-i),r[i])
                    return nxt
                if name.startswith('_restgpr_'):
                    n=int(name.split('_')[-1]);base=r[11]
                    for i in range(n,32):r[i]=self.r32(base-4*(32-i))
                    return nxt
            return tgt
        elif op==16:
            bo=(w>>21)&31;bi=(w>>16)&31;bd=sx(w&0xFFFC,16)
            if w&1:self.lr=nxt
            if self.branch_cond(bo,bi):return (bd if w&2 else pc+bd)&M
        elif op==19:
            xo=(w>>1)&0x3FF;bo=rd;bi=ra
            if xo==16:
                t=self.lr
                if w&1:self.lr=nxt
                if self.branch_cond(bo,bi):return t
            elif xo==528:
                t=self.ctr
                if w&1:
                    self.lr=nxt
                    if t in self.hooks:self.hooks[t](self);return nxt
                if self.branch_cond(bo,bi):return t
            else:raise RuntimeError(f'op19 xo {xo} at {pc:#x}')
        elif op==31:
            xo=(w>>1)&0x3FF
            if xo==266:r[rd]=(r[ra]+r[rb])&M
            elif xo==40:r[rd]=(r[rb]-r[ra])&M
            elif xo==8:t=(~r[ra]&M)+r[rb]+1;self.ca=1 if t>M else 0;r[rd]=t&M
            elif xo==136:t=(~r[ra]&M)+r[rb]+self.ca;self.ca=1 if t>M else 0;r[rd]=t&M
            elif xo==104:r[rd]=(-r[ra])&M
            elif xo==235:r[rd]=(sx(r[ra],32)*sx(r[rb],32))&M
            elif xo==491:
                d=sx(r[rb],32);r[rd]=(int(sx(r[ra],32)/d)&M) if d else 0
            elif xo==459:r[rd]=r[ra]//r[rb] if r[rb] else 0
            elif xo==28:r[ra]=r[rd]&r[rb]
            elif xo==444:r[ra]=r[rd]|r[rb]
            elif xo==316:r[ra]=r[rd]^r[rb]
            elif xo==24:s=r[rb]&63;r[ra]=(r[rd]<<s)&M if s<32 else 0
            elif xo==536:s=r[rb]&63;r[ra]=r[rd]>>s if s<32 else 0
            elif xo==824:
                v=sx(r[rd],32);sh=(w>>11)&31;self.ca=1 if(v<0 and sh and (r[rd]&((1<<sh)-1))) else 0;r[ra]=(v>>sh)&M
            elif xo==922:r[ra]=sx(r[rd],16)&M
            elif xo==954:r[ra]=sx(r[rd],8)&M
            elif xo==26:v=r[rd];r[ra]=32-v.bit_length()
            elif xo==0:
                a,b=sx(r[ra],32),sx(r[rb],32);self.cr[(w>>23)&7]=(8 if a<b else 0)|(4 if a>b else 0)|(2 if a==b else 0)
            elif xo==32:
                a,b=r[ra],r[rb];self.cr[(w>>23)&7]=(8 if a<b else 0)|(4 if a>b else 0)|(2 if a==b else 0)
            elif xo==23:r[rd]=self.r32(A()+r[rb])
            elif xo==87:r[rd]=self.rd(A()+r[rb],1)[0]
            elif xo==535:f[rd]=struct.unpack('>f',self.rd(A()+r[rb],4))[0]
            elif xo==567:a=(r[ra]+r[rb])&M;f[rd]=struct.unpack('>f',self.rd(a,4))[0];r[ra]=a
            elif xo==663:self.wr(A()+r[rb],struct.pack('>f',f[rd]))
            elif xo==339:
                spr=((w>>16)&31)|(((w>>11)&31)<<5)
                r[rd]=self.lr if spr==8 else self.ctr if spr==9 else (_ for _ in ()).throw(RuntimeError(f'mfspr {spr}'))
            elif xo==467:
                spr=((w>>16)&31)|(((w>>11)&31)<<5)
                if spr==8:self.lr=r[rd]
                elif spr==9:self.ctr=r[rd]
                else:raise RuntimeError(f'mtspr {spr}')
            else:raise RuntimeError(f'op31 xo {xo} at {pc:#x}')
            if rc and xo not in(0,32):self.setcr0(r[rd] if xo in(266,40,8,136,104,235,491,459) else r[ra])
        elif op==59:
            xo=(w>>1)&31;fa,fb,fc=(w>>16)&31,(w>>11)&31,(w>>6)&31
            if xo==29:f[rd]=f32(f[fa]*f[fc]+f[fb])
            elif xo==30:f[rd]=f32(-(f[fa]*f[fc]-f[fb]))
            elif xo==25:f[rd]=f32(f[fa]*f[fc])
            elif xo==21:f[rd]=f32(f[fa]+f[fb])
            elif xo==20:f[rd]=f32(f[fa]-f[fb])
            elif xo==18:f[rd]=f32(f[fa]/f[fb])
            else:raise RuntimeError(f'op59 xo {xo} at {pc:#x}')
        elif op==63:
            xo=(w>>1)&0x3FF;fb=(w>>11)&31
            if xo==40:f[rd]=-f[fb]
            elif xo==72:f[rd]=f[fb]
            elif xo==12:f[rd]=f32(f[fb])
            else:raise RuntimeError(f'op63 xo {xo} at {pc:#x}')
        else:raise RuntimeError(f'opcode {op} (word {w:08x}) at {pc:#x}')
        return nxt
