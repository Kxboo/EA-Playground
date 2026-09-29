"""Executable-confirmed Model::Draw list structure, including empty models."""
import struct,math
from containers import Elf,span

def inspect(data):
    elf=Elf(data);section=elf.section('.data')
    if section is None:return None
    raw=elf.section_bytes(section);symbols={(s['table'],s['index']):s for s in elf.symbols}
    local={r['offset']:struct.unpack('<I',span(raw,r['offset'],4))[0]+symbols[r['symbol_table'],r['symbol_index']]['value'] for r in elf.relocations if r['target_section']==section['index'] and symbols[r['symbol_table'],r['symbol_index']]['section']==section['index']}
    models=[]
    for s in elf.symbols:
        if not s['name'].startswith('__Model:::'):continue
        m=s['value'];count=struct.unpack('>I',span(raw,m+0x9c,4))[0]
        if count>65536:raise ValueError('Invalid model geometry count')
        groups=[]
        if count:
            cursor=local[m+0xcc]+4
            for _ in range(count):
                size=struct.unpack('>I',span(raw,cursor,4))[0];cursor+=4
                if size>65536:raise ValueError('Invalid model primitive count')
                span(raw,cursor,size*4)
                groups.append([local[cursor+4*i] for i in range(size)]);cursor+=size*4
        bounds=[struct.unpack('>3f',span(raw,m+offset,12)) for offset in (0x6c,0x7c)]
        if any(not math.isfinite(x) for row in bounds for x in row):raise ValueError('Nonfinite model bounds')
        models.append(dict(name=s['name'].split(':::',1)[1],offset=m,bounds=bounds,geometry_groups=groups,primitive_count=sum(map(len,groups))))
    return dict(models=models,empty_draw_lists=bool(models) and all(not m['primitive_count'] for m in models))
