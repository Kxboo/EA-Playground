import sys,json,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'src'))
import core,research
from containers import Elf
schemas=json.loads((ROOT/'research/shader-schemas.json').read_text())['schemas']
rows={r['sha256']:r for r in json.loads((ROOT/'research/coverage.json').read_text())['records'] if r['name'] in ('shadow_shadow.o','rc_track_car.o','swingset_shadow.o','teeter_totter.o','teeter_totter_shadow.o')}
for row in rows.values():
    data,name=research.read_virtual(row['source']);elf=Elf(data);raw=elf.section_bytes(elf.section('.data'))
    syms={(s['table'],s['index']):s for s in elf.symbols}
    local={r['offset']:struct.unpack_from('<I',raw,r['offset'])[0] for r in elf.relocations if syms[r['symbol_table'],r['symbol_index']]['section']==1}
    print('\nFILE',name)
    for r in elf.relocations:
        family=syms[r['symbol_table'],r['symbol_index']]['name']
        if family not in schemas:continue
        a=r['offset'];d=local[a+4]
        print('SHADER',family,hex(a),'PROGRAM',hex(d),raw[d:a].hex(' '))
        for f in schemas[family]:
            count=struct.unpack_from('>I',raw,a+f['count_offset'])[0];p=local.get(a+f['pointer_offset'])
            print(f['name'],count,hex(p) if p is not None else 'external',raw[p:p+min(count*f['element_size'],48)].hex(' ') if p is not None else '')
        for p in local:
            if d<=p-1<a and raw[p-1]==7:
                gx=local[p];print('GX',hex(gx),raw[gx:gx+96].hex(' '))
