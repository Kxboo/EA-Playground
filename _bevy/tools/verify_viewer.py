"""Launch the real renderer and save GPU screenshots plus scene diagnostics."""
from pathlib import Path
import subprocess,json,os,sys,time
base=Path(__file__).resolve().parents[1]
exe=Path(sys.argv[1]) if len(sys.argv)>1 else base/'target/debug/EAGL-Workbench.exe'
for label,asset,extras in [('basketball','basketball.o',[]),('alicia-animated','alicia.o',['--bank','player_anims.anm','--clip','0']),('warning-texture','strapwarn_standard_english.gsh',[]),('frontend-help','HelpEditor.o',[]),('rc-car-animated','rc_track_car.o',['--bank','rc_track_car_anims.anm','--clip','0']),('teeter-totter','teeter_totter.o',[]),('frontend-nunchuck','NunchuckRequired_English.o',[])]:
    image=base/f'docs/captures/{label}.png'
    started=time.time()
    with (base/f'logs/{label}.log').open('w',encoding='utf-8') as log:
        p=subprocess.run([str(exe),'--asset',asset,'--capture',str(image),'--capture-after','10',*extras],cwd=base,stdout=log,stderr=log,timeout=130,creationflags=0x08000000 if os.name=='nt' else 0)
    assert p.returncode==0,(label,p.returncode)
    assert image.exists(),label
    assert image.stat().st_mtime>=started-1,(label,'stale screenshot')
    assert Path(str(image)+'.json').stat().st_mtime>=started-1,(label,'stale diagnostics')
    report=json.loads(Path(str(image)+'.json').read_text())
    assert report['loaded'] and report['preview']['name']==asset,report
    if asset.endswith('.o'):assert report['mesh_entities']>0,report
    if extras:assert report['animation_players']>0 and report['animation_time']>0,report
    print(label,'rendered',report['mesh_entities'],'meshes',report['animation_players'],'animation players',flush=True)
