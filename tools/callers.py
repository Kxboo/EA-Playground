"""python tools/callers.py <symbol-substring>: list functions containing a bl/b to a matching symbol"""
import sys,struct
from elftools.elf.elffile import ELFFile
e=ELFFile(open('D:/_eagl/Remaster/reference/playgroundz.elf','rb'))
syms=sorted([(s['st_value'],s['st_size'],s.name) for s in e.get_section_by_name('.symtab').iter_symbols() if s['st_size']>0 and s['st_info']['type']=='STT_FUNC'])
targets={a for a,sz,n in syms if sys.argv[1] in n}
tn={a:n for a,sz,n in syms}
for sec in e.iter_sections():
    if sec['sh_type']!='SHT_PROGBITS' or not (sec['sh_flags']&4): continue
    d=sec.data(); base=sec['sh_addr']
    for a,sz,n in syms:
        if not(base<=a<base+len(d)): continue
        for o in range(0,sz,4):
            w=struct.unpack('>I',d[a-base+o:a-base+o+4])[0]
            if (w>>26)==18 and (w&1):
                off=w&0x3fffffc
                if off&0x2000000: off-=0x4000000
                t=(a+o+off)&0xffffffff
                if t in targets: print(n,hex(a+o),'->',tn[t])
