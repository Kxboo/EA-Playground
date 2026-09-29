import sys,json,hashlib
from pathlib import Path
from collections import Counter
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
r=json.loads((core.HOME/'research/coverage.json').read_text('utf-8'))
(core.HOME/'research/coverage.md').write_text(research.coverage_markdown(r),encoding='utf-8')
models={x['sha256']:x for x in r['records'] if x['extension']=='.o'}
print('Model families',Counter((x['payload'].get('object_family'),x['payload']['status']) for x in models.values()))
print('Structural errors',[(x['source'],x.get('error')) for x in r['records'] if x['status']=='error'])
print('Traversal errors',r['errors'])
hashes={str(p.relative_to(core.HOME)):hashlib.sha256(p.read_bytes()).hexdigest() for p in (core.HOME/'src').rglob('*') if p.is_file() and p.suffix in ('.py','.json')}
(core.HOME/'docs/current-source-hashes.json').write_text(json.dumps(hashes,indent=2),encoding='utf-8')
