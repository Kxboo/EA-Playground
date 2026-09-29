"""Resolve every model's actual diffuse binding and retain texture provenance."""
import sys,json,collections
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'src'),str(ROOT.parent/'_bevy/tools')]
import core,research,model_structure,decoder_bridge as bridge
from asset_links import texture_sources

def main():
    rows={r['sha256']:r for r in json.loads((ROOT/'research/coverage.json').read_text())['records'] if r['extension']=='.o'}
    results=[];errors=[];families=collections.Counter()
    for i,row in enumerate(rows.values()):
        try:
            path=bridge.local(row['source'])
            if model_structure.inspect(path.read_bytes())['empty_draw_lists']:continue
            result,materials=core.load_model(path);warnings=[];report=[]
            sources=texture_sources(row['source']);paths=[bridge.local(s) for s in sources]
            requested={m.index:materials.get(m.index,set()) for m in result.meshes if m.ok and not getattr(m,'textureless',False)}
            images,modes=core.texture_images(requested,paths,warnings.append,report)
            origins={str(p):s for p,s in zip(paths,sources)}
            for material in report:
                for ref in material.get('sources',[]):ref['bank']=origins.get(ref['bank'],ref['bank'])
            ownership=[w for m in result.meshes for w in m.warnings if 'Material ' in w]
            for m in result.meshes:
                if m.ok:families[getattr(m,'shader_family','unresolved')]+=1
            results.append(dict(source=row['source'],materials=report,warnings=warnings,ownership_warnings=ownership,textureless_meshes=sum(getattr(m,'textureless',False) for m in result.meshes)))
        except Exception as exc:errors.append(dict(source=row['source'],error=str(exc)))
        if i%100==0:print('Materials',i,flush=True)
    statuses=collections.Counter(m['status'] for r in results for m in r['materials'])
    out=dict(models=results,errors=errors,statuses=dict(statuses),shader_meshes=dict(families),warning_models=sum(bool(r['warnings'] or r['ownership_warnings']) for r in results))
    (ROOT/'research/material-verification.json').write_text(json.dumps(out,indent=2))
    print('RESULT',len(results),'errors',len(errors),'warning_models',out['warning_models'],'statuses',dict(statuses),flush=True)
    print('ERRORS',errors[:5])
    for r in results:
        if r['warnings'] or r['ownership_warnings']:print(r['source'].split('::')[-1],r['warnings'][:8],r['ownership_warnings'][:3])
    assert not errors

if __name__=='__main__':main()
