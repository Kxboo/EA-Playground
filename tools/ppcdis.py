"""Disassemble a function from playgroundz.elf: python tools/dis.py <symbol-substring> [maxinsn]"""
import sys
from elftools.elf.elffile import ELFFile
from capstone import *
f=open('D:/_eagl/Remaster/reference/playgroundz.elf','rb'); e=ELFFile(f)
syms=[(s['st_value'],s['st_size'],s.name) for s in e.get_section_by_name('.symtab').iter_symbols() if s['st_size']>0 and s['st_info']['type']=='STT_FUNC']
pat=sys.argv[1]
md=Cs(CS_ARCH_PPC,CS_MODE_32|CS_MODE_BIG_ENDIAN|(1<<4))  # CS_MODE_PS
md.skipdata=True
for a,sz,n in syms:
    if pat in n:
        for sec in e.iter_sections():
            if sec['sh_addr']<=a<sec['sh_addr']+sec['sh_size'] and sec['sh_type']=='SHT_PROGBITS':
                d=sec.data()[a-sec['sh_addr']:a-sec['sh_addr']+sz]
                print('==',n,hex(a),sz)
                names={x[0]:x[2] for x in syms}
                for i in md.disasm(d,a):
                    t=i.op_str
                    if i.mnemonic in('bl','b'):
                        try:
                            t=names.get(int(t,16),t)
                        except: pass
                    print('%x: %s %s'%(i.address,i.mnemonic,t))
