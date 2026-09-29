"""Full .o corpus export audit with textures and independent declared bounds."""
import sys,json,collections,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'src'),str(ROOT.parent/'_bevy/tools')]
import core,research,model_structure,decoder_bridge as bridge
from asset_links import texture_sources
from audit_corpus import check_glb

def main():
    rows={r['sha256']:r for r in json.loads((ROOT/'research/coverage.json').read_text())['records'] if r['extension']=='.o'}
    passed=[];failed=[];empty=[];frontend=[];hwskin=[]
    for i,row in enumerate(rows.values()):
        try:
            path=bridge.local(row['source']);structure=model_structure.inspect(path.read_bytes())
            if structure and structure['empty_draw_lists']:
                empty.append(dict(source=row['source'],models=structure['models']));continue
            result,materials=core.load_model(path)
            assert result.ok
            warnings=[]
            doc=check_glb(core.model_glb(path,result,materials,textures=[bridge.local(t) for t in texture_sources(row['source'])],log=warnings.append))
            item=dict(source=row['source'],meshes=len(result.meshes),triangles=result.total_faces,textures=len(doc.get('images',[])),warnings=warnings)
            if hasattr(result,'submodels'):
                declared={m['name']:m for m in structure['models']}
                for model in result.submodels:
                    lo,hi=declared[model['name']]['bounds']
                    for mesh in result.meshes:
                        if mesh.index not in model['mesh_indices']:continue
                        for p in mesh.positions:
                            assert all(lo[j]-.002<=p[j]<=hi[j]+.002 for j in range(3)),(model['name'],p,lo,hi)
                frontend.append(dict(**item,shapes=len(result.submodels),declared_bounds_checked=True))
            if result.material_name=='HWSkin':
                assert len(structure['models'])==1
                declared=structure['models'][0]
                assert declared['primitive_count']==len(result.meshes)
                lo,hi=declared['bounds']
                for mesh in result.meshes:
                    for p in mesh.positions:assert all(lo[j]-.002<=p[j]<=hi[j]+.002 for j in range(3)),(p,lo,hi)
                hwskin.append(dict(**item,fractions=sorted(set(m.position_fraction for m in result.meshes)),declared_bounds_checked=True))
            passed.append(item)
        except Exception as exc:failed.append(dict(source=row['source'],error=str(exc)))
        if i%100==0:print('Models',i,flush=True)
    out=dict(passed=passed,empty=empty,failed=failed,frontend=frontend,hwskin=hwskin)
    (ROOT/'research/model-verification.json').write_text(json.dumps(out,indent=2))
    frontend_sources={r['source'] for r in rows.values() if r.get('payload',{}).get('object_family')=='frontend TextureApt'}
    (ROOT/'research/frontend-verification.json').write_text(json.dumps(dict(passed=frontend,failed=[r for r in failed if r['source'] in frontend_sources]),indent=2))
    print('GEOMETRY',len(passed),'EMPTY',len(empty),'FAILED',len(failed),'FRONTEND',len(frontend),'SHAPES',sum(r['shapes'] for r in frontend),'HWSKIN',len(hwskin),flush=True)
    print(json.dumps(failed[:10],indent=2))
    assert not failed

if __name__=='__main__':main()
