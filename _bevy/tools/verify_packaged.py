"""Exercise the shipping executable's headless protocol and isolated worker."""
from pathlib import Path
import json,subprocess,uuid,os,shutil,time

base=Path(__file__).resolve().parents[1]
exe=base/'EAGL-Workbench.exe'
checks=[]
def run(command,**args):
    argv=[str(exe),'--headless',command]
    for key,value in args.items():argv += ['--'+key,str(value)]
    start=time.perf_counter()
    p=subprocess.run(argv,cwd=base,capture_output=True,encoding='utf-8',timeout=120)
    assert p.returncode==0,(command,p.returncode,p.stdout,p.stderr)
    result=json.loads(p.stdout);assert result['ok'],result
    checks.append(dict(command=command,source=args.get('source'),milliseconds=round((time.perf_counter()-start)*1000),status='passed'))
    return result['value']

rows=run('catalog')['assets']
def find(name):return next(r['source'] for r in rows if r['name']==name)
model=run('preview',source=find('basketball.o'));assert model['textures']==1 and model['triangles']==384
animated=run('preview',source=find('alicia.o'),skeleton=find('player_skel.ske'),bank=find('player_anims.anm'),index=0)
assert animated['skinned'] and animated['bones']==68 and animated['duration']>0 and animated['textures']==3
shadow=run('preview',source=find('rc_buggybody_shadow.o'));assert shadow['triangles']>0
assert shadow['material_warnings']==[] and shadow['material_report']==[]
net=run('preview',source=find('net.o'));assert net['textures']==2 and not net['material_warnings']
assert next(m for m in net['material_report'] if m['name']=='net')['candidates']==2
buggy=run('preview',source=find('rc_buggybody.o'));assert buggy['textures']==5 and not buggy['material_warnings']
world=run('preview',source=find('world-low-all.o'));assert world['textures']>100 and not world['material_warnings']
frontend=run('preview',source=find('HelpEditor.o'));assert frontend['frontend'] and frontend['textures']==1 and frontend['triangles']==2
assert run('metadata',source=find('HelpEditor.o'))['item_kind']=='shape'
car=run('preview',source=find('rc_track_car.o'),bank=find('rc_track_car_anims.anm'),index=0)
assert car['triangles']==980 and car['skinned'] and car['bones']==2 and car['duration']>0 and car['textures']==5
empty=run('preview',source=find('rc_track_car_shadow.o'));assert empty['inspection']['payload']['empty_draw_lists']
texture=run('preview',source=find('strapwarn_standard_english.gsh'));assert (texture['width'],texture['height'])==(640,480)
assert run('inspect',source=find('home.csv'))['format']=='Delimited table'
assert len(run('metadata',source=find('player_anims.anm'))['items'])==265
archive=min((r for r in rows if r['name'].lower().endswith('.big')),key=lambda r:r['size'])
# Normal workspace directories preserve inherited Windows ACLs across the
# sandboxed test runner and its packaged child process. Keep outputs inspectable.
temporary=base/'exports'/('verify-'+uuid.uuid4().hex[:8])
temporary.mkdir(parents=True)
extracted=run('extract',source=archive['source'],out=temporary)
assert extracted['extracted']>0 and any(temporary.iterdir())

# This folder has no sibling Remaster/src: the worker must contain every decoder.
isolated=base/'build/package/EAGL-Decoder'
shutil.copytree(base/'reference',isolated/'reference',dirs_exist_ok=True)
(isolated/'research').mkdir(exist_ok=True)
shutil.copy2(base/'research/coverage.json',isolated/'research/coverage.json')
request=dict(command='preview',source=find('alicia.o'),skeleton=find('player_skel.ske'),bank=find('player_anims.anm'),index=0)
p=subprocess.run([str(isolated/'EAGL-Decoder.exe')],input=json.dumps(request)+'\n',capture_output=True,encoding='utf-8',timeout=120,cwd=isolated)
assert p.returncode==0,(p.stdout,p.stderr)
isolated_result=json.loads(p.stdout);assert isolated_result['ok'],isolated_result
assert isolated_result['value']['skinned'] and isolated_result['value']['textures']==3
checks.append(dict(command='isolated-packaged-worker',status='passed'))

p=subprocess.run([str(exe),'--headless','preview','--source','missing-file.o'],capture_output=True,encoding='utf-8',timeout=30)
assert p.returncode!=0 and not json.loads(p.stdout)['ok']
checks.append(dict(command='missing-source-error',status='passed'))
(base/'docs/packaged-verification.json').write_text(json.dumps(dict(checks=checks,assets=len(rows)),indent=2),encoding='utf-8')
print(len(checks),'packaged checks passed;',len(rows),'catalog assets')
