"""Generate tests/data/tpl_golden.json from the Python TPL decoder (Remaster/src/formats.py).

Key = SHA-256 of the .tpl file; value = {"images":["ok:WxH:pixelhash16" | "err", ...]} or {"error": message}.
The Rust decoder (src/tpl.rs) must reproduce every entry.
"""
import sys,json,hashlib
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parent.parent
sys.path[:0]=[str(REPO/'Remaster'/'src')]
import research,formats,core
cov=json.loads((REPO/'Remaster'/'research'/'coverage.json').read_text(encoding='utf-8'))
seen={};out={}
for r in cov['records']:
    if r['extension']=='.tpl' and r['sha256'] not in seen:seen[r['sha256']]=r['source']
for h,src in seen.items():
    data,_=research.read_virtual(src)
    try:entries=formats.tpl(data)['entries']
    except Exception as e:out[h]=dict(error=str(e)[:100]);continue
    images=[]
    for e in entries:
        try:
            rgba,w,hh=formats.tpl_rgba(data,e);images.append(f'ok:{w}x{hh}:{hashlib.sha256(bytes(rgba)).hexdigest()[:16]}')
        except Exception:images.append('err')
    out[h]=dict(images=images)
p=HERE.parent/'tests'/'data'/'tpl_golden.json'
p.write_text(json.dumps(out,separators=(',',':')),encoding='utf-8',newline='\n')
n=sum(len(v.get('images',[])) for v in out.values())
print('wrote',p,len(out),'files',n,'images',sum(1 for v in out.values() for i in v.get('images',[]) if i=='err'),'undecodable',sum(1 for v in out.values() if 'error' in v),'unparseable')
