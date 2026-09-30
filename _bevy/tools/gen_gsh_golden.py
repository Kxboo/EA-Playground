"""Generate tests/data/gsh_golden.json: per-image RGBA hashes from the Python GSH decoder over the whole corpus.

Key = SHA-256 of the .gsh file; value = list of [index, name, full_name, record_id, width, height, result] where result is
'ok:<first 16 hex of sha256(rgba)>' or 'err'.  The Rust decoder (src/gsh.rs) is tested against this file.
"""
import sys,json,hashlib,tempfile,os
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parent.parent
sys.path[:0]=[str(REPO/'Remaster'/'src'),str(REPO/'Remaster'/'src'/'legacy')]
import research
from legacy import gsh_parser as gp

def main():
    cov=json.loads((REPO/'Remaster'/'research'/'coverage.json').read_text(encoding='utf-8'))
    seen={};out={}
    for r in cov['records']:
        if r['extension']!='.gsh' or r['sha256'] in seen:continue
        seen[r['sha256']]=r['source']
    for h,src in seen.items():
        data,_=research.read_virtual(src)
        with tempfile.NamedTemporaryFile(delete=False,suffix='.gsh') as f:f.write(data);tmp=f.name
        try:
            gsh,_=gp.parse_gsh(tmp)
        except Exception:
            out[h]='parse-error';os.unlink(tmp);continue
        os.unlink(tmp)
        rows=[]
        for e in gsh.entries:
            try:
                rgba,w,hh=gp.decode_entry_rgba(e,data);res='ok:'+hashlib.sha256(rgba).hexdigest()[:16]
            except Exception:res='err'
            rows.append([e.index,e.name,e.full_name,e.record_id,e.width,e.height,res])
        out[h]=rows
    p=HERE.parent/'tests'/'data'/'gsh_golden.json'
    p.write_text(json.dumps(out,separators=(',',':')),encoding='utf-8',newline='\n')
    n=sum(len(v) for v in out.values() if isinstance(v,list))
    print('wrote',p,len(out),'files',n,'images',sum(1 for v in out.values() if isinstance(v,list) for r in v if r[6]!='err'),'decodable')
main()
