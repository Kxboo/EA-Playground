import json
from pathlib import Path
from collections import Counter,defaultdict
d=json.loads(Path('Remaster/research/corpus-v2.json').read_text('utf-8'))
rows=d['records']
print('PARSE ERRORS',[(r['source'],r.get('error')) for r in rows if r['status']=='error'])
models={r['sha256']:r for r in rows if r['extension']=='.o'}
print('MODELS',len(models),Counter(r.get('payload',{}).get('status',r['status']) for r in models.values()))
for r in models.values():
    p=r.get('payload',{})
    if p.get('status')!='partial' or p.get('empty_meshes',0)>0:print('MODEL GAP',r['source'],p.get('status'),p.get('layouts'),p.get('empty_meshes'),p.get('triangles'),r.get('error'))
textures={r['sha256']:r for r in rows if r['extension']=='.gsh'}
print('GSH',len(textures),Counter(i['status'] for r in textures.values() for i in r.get('payload',{}).get('images',[])))
print('TPL',len({r['sha256'] for r in rows if r['extension']=='.tpl'}))
seen=set()
for r in rows:
    if r['status']=='unknown' and r['extension'] not in seen:
        seen.add(r['extension']);print('UNKNOWN',r['extension'],r['signature_hex'],r['source'])
