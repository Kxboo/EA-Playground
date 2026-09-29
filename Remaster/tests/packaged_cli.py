"""Exercise the actual console executable, including exit codes and exports."""
import sys,json,subprocess,uuid,hashlib
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core
from audit_corpus import check_glb

folder=core.HOME/'exports'/('cli-test-'+uuid.uuid4().hex[:8]);folder.mkdir()
exe=core.HOME/'EAGL-CLI.exe'
checks=[]
def run(*args,expect=0):
    p=subprocess.run([str(exe),*map(str,args)],capture_output=True,encoding='utf-8',timeout=120)
    assert p.returncode==expect,(args,p.returncode,p.stdout[:500],p.stderr[:1000])
    result=json.loads(p.stdout if expect!=1 else p.stderr)
    checks.append(dict(command=list(map(str,args)),exit_code=p.returncode))
    return result

source=str(core.DEFAULT_DATA/'files/data/fe/main.big')+'::Main.gsh'
r=run('inspect',source,'--deep')
assert r['payload']['images'] and all(i['status']=='decoded_pixels' for i in r['payload']['images'])
run('decode',source,'--out',folder/'textures')
assert list((folder/'textures').glob('*.png'))
run('decode',core.DEFAULT_OLD/'WorldProps/basketball.o','--out',folder/'model')
check_glb((folder/'model/basketball.glb').read_bytes())
run('decode',core.HOME/'reference/player_anims.anm','--skeleton',core.HOME/'reference/player_skel.ske','--clip',0,'--out',folder/'clip')
check_glb(next((folder/'clip').glob('*.glb')).read_bytes())
decoded=json.loads((folder/'clip/decoded.json').read_text())
assert decoded['clips'][0]['sample_count']>0
csv=folder/'quoted.csv';csv.write_text('key\tvalue\n1\t"a,b"\n',encoding='utf-8')
r=run('inspect',csv);assert r['rows'][1]==['1','a,b'] and r['delimiter']=='\t'
run('decode',csv,'--out',folder/'csv')
run('hexdump',core.HOME/'reference/player_skel.ske','--length','0x20')
run('strings',core.HOME/'reference/player_skel.ske')
archive=core.DEFAULT_DATA/'files/data/fe/main.big'
r=run('extract',archive,'--out',folder/'archive');assert r['extracted']>0
run('extract',archive,'--out',folder/'archive',expect=1)
bad=folder/'bad.o';bad.write_bytes(b'\x7fELF'+bytes(20))
run('validate',bad,'--deep',expect=2)
unsupported=folder/'unsupported.apt';unsupported.write_bytes(b'Apt Data:1.0\0')
run('decode',unsupported,'--out',folder/'unsupported',expect=1)
(core.HOME/'docs/cli-verification.json').write_text(json.dumps(dict(status='ok',executable_sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),output=str(folder),checks=checks),indent=2),encoding='utf-8')
print('Packaged CLI passed',len(checks),'checks; outputs:',folder)
