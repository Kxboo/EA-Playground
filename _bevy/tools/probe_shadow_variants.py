import decoder_bridge as b,struct
from containers import Elf
for name in ('rc_buggybody_shadow.o','rc_touringbody_shadow.o'):
    source=next(a['source'] for a in b.catalog()['assets'] if a['name']==name)
    data,_=b.raw(source);elf=Elf(data);raw=elf.section_bytes(elf.section('.data'))
    syms={(s['table'],s['index']):s for s in elf.symbols}
    rel={r['offset']:struct.unpack_from('<I',raw,r['offset'])[0] for r in elf.relocations}
    anchors=[r['offset'] for r in elf.relocations if syms[(r['symbol_table'],r['symbol_index'])]['name']=='PlaygroundShadow']
    print(name)
    for s in anchors:
        d=rel[s+4];op=next(p-1 for p in rel if d<=p-1<d+16 and raw[p-1]==7)
        gx=rel[op+1];size=struct.unpack_from('>I',raw,op+5)[0]
        print('SHADER',hex(s),'PROGRAM',hex(d),'OP',hex(op),'GX',hex(gx),'BYTES',size,'POS',struct.unpack_from('>I',raw,s+24)[0],'COLORS',struct.unpack_from('>I',raw,s+32)[0])
        print('PROGRAM',raw[d:s].hex(' '));print('ARGS',raw[s:s+64].hex(' '));print('GX',raw[gx:gx+min(size,90)].hex(' '))
