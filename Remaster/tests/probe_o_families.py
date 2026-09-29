import sys,json,struct
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
from containers import Elf
r=json.loads((core.HOME/'research/coverage.json').read_text())
for family in ('frontend TextureApt','PlaygroundShadow'):
    rows=[x for x in r['records'] if x.get('payload',{}).get('object_family')==family]
    sample=sorted(rows,key=lambda x:x['size'])[0]
    data,name=research.read_virtual(sample['source']);elf=Elf(data)
    print('SOURCE',sample['source'],len(data))
    print('SECTIONS',elf.sections)
    print('SYMBOLS',elf.symbols)
    print('RELOCS',elf.relocations)
    sec=elf.section('.data');raw=elf.section_bytes(sec)
    for off in range(0,min(len(raw),1024),16):
        b=raw[off:off+16]
        print(hex(off),b.hex(' '),[round(v[0],4) for v in struct.iter_unpack('>f',b[:len(b)//4*4])])
