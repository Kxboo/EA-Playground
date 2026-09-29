"""Read-only PowerPC disassembly of named functions from playgroundz.elf, with
small-data float/int constants resolved.  Usage:  re_functions.py <substring> [--max N]
Evidence tool: every constant printed is read from the executable, not guessed.
"""
import sys,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]/'Remaster'
sys.path[:0]=[str(ROOT/'src'),str(ROOT/'research/deps'),str(ROOT/'research')]
from elf_trace import Executable
import capstone

def load():
    e=Executable();return e

def sda_bases(e):
    b={}
    for n in('_SDA_BASE_','_SDA2_BASE_'):
        b[n]=e.symbols[n]['value'] if n in e.symbols else None
    return b

def annotate(e,name):
    s=e.symbols[name];size=s['size'];bases=sda_bases(e)
    cs=capstone.Cs(capstone.CS_ARCH_PPC,capstone.CS_MODE_32|capstone.CS_MODE_BIG_ENDIAN);cs.detail=False
    data=e.read(s['value'],size);out=[f'{name} @ {s["value"]:#x} size={size}']
    for off in range(0,size,4):
        a=s['value']+off;w=data[off:off+4];op=int.from_bytes(w,'big')>>26
        paired=op in(4,56,57,60,61)
        ins=None if paired else next(cs.disasm(w,a),None)
        t=f'{ins.mnemonic} {ins.op_str}' if ins else f'.long 0x{w.hex()}'
        note=''
        if ins and ins.mnemonic in('lfs','lfd','lwz','lha','lhz','lbz') and ins.op_str.endswith('(r2)') or (ins and ins.mnemonic in('lfs','lfd','lwz') and ins.op_str.endswith('(r13)')):
            try:
                d,r=ins.op_str.split(',')[1].strip().split('(');r=r.rstrip(')')
                d=int(d,0);base=bases['_SDA2_BASE_' if r=='r2' else '_SDA_BASE_']
                if base is not None:
                    addr=base+d
                    if ins.mnemonic=='lfs':note=f' ; f32 {struct.unpack(">f",e.read(addr,4))[0]!r} @{addr:#x}'
                    elif ins.mnemonic=='lfd':note=f' ; f64 {struct.unpack(">d",e.read(addr,8))[0]!r} @{addr:#x}'
                    else:note=f' ; @{addr:#x}'
            except Exception as ex:pass
        if ins and ins.mnemonic in('bl','b'):
            try:note=' ; '+e.addresses.get(int(ins.op_str,16),'')
            except ValueError:pass
        out.append(f'{a:08x}: {w.hex()}  {t}{note}')
    return '\n'.join(out)

if __name__=='__main__':
    e=load();q=sys.argv[1]
    for n,s in e.symbols.items():
        if q in n and s['size'] and s['value']:
            sec=e.elf.sections[s['section']] if s['section']<len(e.elf.sections) else None
            if sec and sec['flags']&4:print(annotate(e,n)+'\n')
