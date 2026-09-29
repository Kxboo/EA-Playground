"""Read-only symbol-aware PowerPC disassembly of the supplied executable.

Research dependency: pip install --target research/deps capstone.
No runtime decoder dependency on Capstone is introduced.
"""
import sys,struct,json,hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'src'),str(ROOT/'research/deps')]
from containers import Elf,span

class Executable:
    def __init__(self,path=ROOT/'reference/playgroundz.elf'):
        self.path=Path(path);self.elf=Elf(self.path.read_bytes())
        self.symbols={s['name']:s for s in self.elf.symbols if s['name']}
        self.addresses={s['value']:s['name'] for s in self.elf.symbols if s['name'] and s['value']}
    def read(self,address,length):
        for s in self.elf.sections:
            if s['type']!=8 and s['address']<=address and address+length<=s['address']+s['size']:
                return span(self.elf.data,s['offset']+address-s['address'],length)
        raise ValueError(f'Address 0x{address:x}+{length} outside file-backed sections')
    def disasm(self,name):
        import capstone
        s=self.symbols[name]
        size=s['size']
        inferred=not size
        if inferred:
            size=min(q['value'] for q in self.symbols.values() if q['section']==s['section'] and q['value']>s['value'])-s['value']
        cs=capstone.Cs(capstone.CS_ARCH_PPC,capstone.CS_MODE_32|capstone.CS_MODE_BIG_ENDIAN)
        lines=[f'{name}: {s["value"]:#x}, {size} bytes'+(' (extent inferred from next symbol)' if inferred else '')]
        data=self.read(s['value'],size)
        for off in range(0,len(data),4):
            address=s['value']+off;word=data[off:off+4]
            # Generic PPC disassemblers can mislabel Gekko paired-single words
            # as later VSX instructions. Keep those raw, explicitly unresolved.
            paired=int.from_bytes(word,'big')>>26 in (4,56,57,60,61)
            ins=None if paired else next(cs.disasm(word,address),None)
            text=f'{ins.mnemonic} {ins.op_str}' if ins else f'.long 0x{word.hex()} ('+('Gekko paired-single; undecoded' if paired else 'undecoded')+')'
            if ins and ins.mnemonic in ('bl','b'):
                try:text+=' ; '+self.addresses.get(int(ins.op_str,16),'')
                except ValueError:pass
            lines.append(f'{address:08x}: {word.hex()}  {text}')
        return '\n'.join(lines)
    def fields(self,family):
        names=self.symbols[family+'_FieldNames'];sizes=self.symbols[family+'_FieldSizes']
        pointers=struct.unpack('>'+str(names['size']//4)+'I',self.read(names['value'],names['size']))
        widths=struct.unpack('>'+str(sizes['size']//4)+'I',self.read(sizes['value'],sizes['size']))
        return [dict(name=self.read(p,1).decode() + self.read(p+1,100).split(b'\0')[0].decode(),element_size=n,count_offset=8+i*8,pointer_offset=12+i*8) for i,(p,n) in enumerate(zip(pointers,widths))]

if __name__=='__main__':
    e=Executable();query=sys.argv[1]
    if len(sys.argv)>2 and sys.argv[2]=='fields':print(json.dumps(e.fields(query),indent=2))
    else:
        for name in e.symbols:
            s=e.symbols[name]
            if query in name and s['value'] and s['section']<len(e.elf.sections) and e.elf.sections[s['section']]['flags']&4:print(e.disasm(name)+'\n')
