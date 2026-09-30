"""Write src/havok_classes.json: Havok 4.6 class reflection recovered from playgroundz.elf.

Runs every hk*Class initialiser on the PowerPC interpreter (tools/ppc_emu.py) and records each class's name,
parent, object size and 20-byte member records.  --check verifies the checked-in file is current.
"""
import sys,json
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parent.parent
sys.path[:0]=[str(HERE),str(REPO/'Remaster'/'src')]
import havok

def build():
    r=havok.Reflection();out={}
    for name in sorted(r.classes):
        c=r.get(name)
        out[name]=dict(parent=c.get('parent_name'),size=c['size'],members=[dict(n=m['name'],c=m['cls'],t=m['type'],s=m['subtype'],a=m['csize'],f=m['flags'],o=m['offset']) for m in c['members']])
    return out

if __name__=='__main__':
    data=json.dumps(build(),separators=(',',':'),sort_keys=True)
    tgt=HERE.parent/'src'/'havok_classes.json'
    if '--check' in sys.argv:
        ok=tgt.exists() and tgt.read_text(encoding='utf-8')==data;print('havok_classes.json','up to date' if ok else 'STALE');sys.exit(0 if ok else 1)
    tgt.write_text(data,encoding='utf-8',newline='\n');print('wrote',tgt,len(json.loads(data)),'classes',len(data),'bytes')
