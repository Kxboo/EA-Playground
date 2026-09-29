import sys,json,collections,math
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'src'))
import core,research
from audit_corpus import check_glb

rows={r['sha256']:r for r in json.loads((ROOT/'research/coverage.json').read_text())['records'] if r.get('payload',{}).get('object_family')=='frontend TextureApt'}
passed=[];failed=[]
for row in rows.values():
    try:
        path=research.materialize(*research.read_virtual(row['source']))
        result,materials=core.load_model(path)
        assert result.ok
        check_glb(core.model_glb(path,result,materials,textures=[]))
        passed.append(dict(source=row['source'],models=len(result.submodels),meshes=len(result.meshes),triangles=result.total_faces))
    except Exception as exc:failed.append(dict(source=row['source'],error=str(exc)))
out=dict(passed=passed,failed=failed)
(ROOT/'research/frontend-verification.json').write_text(json.dumps(out,indent=2))
print('passed',len(passed),'failed',len(failed),'shapes',sum(r['models'] for r in passed),'meshes',sum(r['meshes'] for r in passed))
print(collections.Counter(r['error'] for r in failed))
print(json.dumps(failed[:4],indent=2))
