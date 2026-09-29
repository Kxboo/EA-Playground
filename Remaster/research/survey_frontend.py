import sys,json,struct,collections
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
from containers import Elf
report=json.loads((core.HOME/'research/coverage.json').read_text())
rows={r['sha256']:r for r in report['records'] if r.get('payload',{}).get('object_family')=='frontend TextureApt'}
counts=collections.Counter();samples=[]
for r in rows.values():
    data,name=research.read_virtual(r['source']);e=Elf(data);sec=e.section('.data');raw=e.section_bytes(sec)
    symbols={(s['table'],s['index']):s for s in e.symbols}
    local={q['offset']:struct.unpack_from('<I',raw,q['offset'])[0]+symbols[(q['symbol_table'],q['symbol_index'])]['value'] for q in e.relocations if symbols[(q['symbol_table'],q['symbol_index'])]['section']==sec['index']}
    external={q['offset']:symbols[(q['symbol_table'],q['symbol_index'])]['name'] for q in e.relocations if symbols[(q['symbol_table'],q['symbol_index'])]['section']==0}
    models=[s for s in e.symbols if s['name'].startswith('__Model:::')]
    for off,family in external.items():
        if family not in ('TextureApt','GouraudApt'):continue
        counts[family]+=1
        if family=='TextureApt':
            d=local[off+4];counts['pcode:'+raw[d:d+2].hex()]+=1
            tex=external.get(off+44,'?')
            coords=local[off+60];n=struct.unpack_from('>I',raw,off+56)[0]
            mat=struct.unpack_from('>16f',raw,local[off+52]);positions=list(struct.iter_unpack('>3h',raw[coords:coords+n*6]))
            if len(samples)<12:samples.append(dict(source=r['source'],anchor=off,models=[dict(name=s['name'],offset=s['value'],scale=struct.unpack_from('>3f',raw,s['value']+0x4c),center=struct.unpack_from('>3f',raw,s['value']+0x5c)) for s in models[:3]],texture=tex,matrix=mat,positions=positions[:4]))
print(counts)
print(json.dumps(samples,indent=2))
