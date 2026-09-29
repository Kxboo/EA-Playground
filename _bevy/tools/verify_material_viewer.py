"""Capture material regression fixtures using the shipping Bevy renderer."""
from pathlib import Path
import subprocess,json,time
base=Path(__file__).resolve().parents[1]
results=[]
for label,asset,delay in [('material-net','net.o',10),('material-buggy','rc_buggybody.o',10),('material-world','world-low-all.o',30),('material-character','alicia.o',10),('material-frontend','HelpEditor.o',10)]:
    image=base/f'docs/captures/{label}.png';started=time.time()
    with (base/f'logs/{label}.log').open('w',encoding='utf-8') as log:
        p=subprocess.run([str(base/'EAGL-Workbench.exe'),'--asset',asset,'--capture',str(image),'--capture-after',str(delay)],cwd=base,stdout=log,stderr=log,timeout=130,creationflags=0x08000000)
    assert p.returncode==0 and image.exists() and image.stat().st_mtime>=started-1,(label,p.returncode)
    report=json.loads(Path(str(image)+'.json').read_text())
    assert report['loaded'] and report['mesh_entities']>0,report
    assert report['preview']['material_warnings']==[],report['preview']['material_warnings']
    assert all(m['status']=='resolved' for m in report['preview']['material_report'])
    results.append(dict(name=asset,capture=str(image),textures=report['preview']['textures'],mesh_entities=report['mesh_entities'],decoder_version=report['preview']['decoder_version']))
    print(label,'rendered;',report['preview']['textures'],'textures; no material warnings',flush=True)
(base/'docs/material-render-verification.json').write_text(json.dumps(results,indent=2))
