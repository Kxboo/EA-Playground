import sys,json
from pathlib import Path
from collections import Counter
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
d=json.loads((core.HOME/'research/corpus-v2.json').read_text())
unique={r['sha256']:r for r in d['records'] if r['extension']=='.gsh'}
results=[]
for i,r in enumerate(unique.values()):
    data,name=research.read_virtual(r['source'])
    try:p=research.payload_report(data,name)
    except Exception as exc:p={'status':'error','error':str(exc)}
    results.append(dict(source=r['source'],sha256=r['sha256'],payload=p))
    if i%100==0:print('GSH',i,flush=True)
counts=Counter(e['status'] for r in results for e in r['payload'].get('images',[]))
errors=Counter(e.get('error') for r in results for e in r['payload'].get('images',[]) if e['status']=='error')
print('RESULT',counts,errors)
(core.HOME/'research/texture-recheck.json').write_text(json.dumps(dict(results=results,counts=dict(counts),errors=dict(errors)),indent=2),encoding='utf-8')
