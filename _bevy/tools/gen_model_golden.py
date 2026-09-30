"""Generate tests/data/model_golden.json from the Python reference model decoder (Remaster/src/model_unified.py).

Key = SHA-256 of the .o file; value = {models:[names], prims:[[anchor, family, vertices, triangles, content_hash, textures]]}.
The Rust decoder (src/model.rs) must reproduce every entry exactly.
"""
import sys,json
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parent.parent
sys.path[:0]=[str(REPO/'Remaster'/'src')]
import research,model_unified as mu
cov=json.loads((REPO/'Remaster'/'research'/'coverage.json').read_text(encoding='utf-8'))
seen={};out={}
for r in cov['records']:
    if r['extension']=='.o' and r['sha256'] not in seen:seen[r['sha256']]=r['source']
for h,src in seen.items():
    data,_=research.read_virtual(src);out[h]=mu.model_summary(data)
p=HERE.parent/'tests'/'data'/'model_golden.json'
p.write_text(json.dumps(out,separators=(',',':')),encoding='utf-8',newline='\n')
print('wrote',p,len(out),'models',sum(len(v['prims']) for v in out.values()),'primitives',p.stat().st_size//1024,'KiB')
