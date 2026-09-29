import sys,json,math,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
import decoder_bridge as b
sys.path.insert(0,str(b.REMASTER/'tests'))
from audit_corpus import check_glb
rows=b.catalog()['assets'];results=[];failures=[]
for row in rows:
    if row['family']!='PlaygroundShadow':continue
    try:
        result,mat=b.core.load_model(b.local(row['source']))
        assert result.ok
        doc=check_glb(b.core.model_glb(b.local(row['source']),result,mat))
        results.append(dict(source=row['source'],triangles=result.total_faces,meshes=len(result.meshes)))
    except Exception as exc:failures.append(dict(source=row['source'],error=str(exc)))
print('SHADOWS',len(results),'passed',len(failures),'failed',flush=True)
if failures:print(failures[:5])
checks=[]
def find(name):return next(a['source'] for a in rows if a['name']==name)
for request in [dict(source=find('basketball.o')),dict(source=find('alicia.o'),skeleton=find('player_skel.ske'),bank=find('player_anims.anm'),index=0),dict(source=find('player_anims.anm'),skeleton=find('player_skel.ske'),index=0),dict(source=find('Main.gsh'),index=0),dict(source=find('home.csv'))]:
    p=b.preview(request)
    if p.get('asset','').endswith('.glb'):check_glb((b.BASE/'assets'/p['asset']).read_bytes())
    print('PREVIEW',p['name'],p['kind'],p.get('triangles'),p.get('textures'),flush=True)
    checks.append(p)
(b.BASE/'docs/decoder-verification.json').write_text(json.dumps(dict(shadows=results,shadow_failures=failures,previews=checks),indent=2),encoding='utf-8')
assert not failures
