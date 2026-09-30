"""Generate tests/data/anim_golden.json from the Python reference skeleton/animation decoders.

For every bank in the corpus (player, RC car, swing set, teeter-totter) and every clip the reference decoder is run and
summarised: status, sample count, codec, and per-bone sample counts + sums of all components (+ first/last sample).
The Rust decoders (src/skeleton.rs, src/anim.rs) must reproduce every entry.
"""
import sys,json,math
from pathlib import Path
HERE=Path(__file__).resolve().parent;REPO=HERE.parent.parent
sys.path[:0]=[str(REPO/'Remaster'/'src')]
import research,core
DATA=REPO/'eagl EA PLAYGROUND'/'extra'/'more'/'eaplayground files'/'DATA'/'files'/'data'
BANKS=[('characters/player_anims.viv','player_skel.ske','player_anims.anm'),
       ('placeables/rc_track_car.viv','rc_track_car_skel.ske','rc_track_car_anims.anm'),
       ('placeables/swingset.viv','swingset_skel.ske','swingset_anims.anm'),
       ('placeables/teeter_totter.viv','teeter_totter_skel.ske','teeter_totter_anims.anm')]
import tempfile,os

def local(viv,inner):
    data,_=research.read_virtual(f'{DATA/viv}::{inner}')
    d=Path(tempfile.mkdtemp())/inner;d.write_bytes(data);return d

def sums(mapping):
    out=[]
    for bone,vals in sorted(mapping.items()):
        flat=[x for v in vals for x in (v if v is not None else ())]
        out.append([bone,len(vals),math.fsum(flat) if False else sum(flat),list(vals[0] or ()),list(vals[-1] or ())])
    return out

result={}
for viv,ske,anm in BANKS:
    sp=local(viv,ske);ap=local(viv,anm)
    skel=core.load_skeleton(sp);bank=core.AnimationBank(ap)
    entry=dict(bones=[[b.index,b.name,b.parent_idx,list(b.scale),list(b.quaternion),list(b.local_translation),list(b.world_translation),list(b.world_matrix)] for b in skel.bones],clips=[])
    for i in range(len(bank.blocks)):
        try:
            c=bank.decode(i,skel)
            entry['clips'].append(dict(index=i,name=c.name,ok=True,samples=c.sample_count,codec=c.codec,caveats=len(c.caveats),
                rot=sums(c.rot_by_bone),trans=sums(c.trans_by_bone),scale=sums(getattr(c,'scale_by_bone',{}))))
        except Exception as e:
            entry['clips'].append(dict(index=i,name=bank.names[i] if i<len(bank.names) else '',ok=False,error=f'{type(e).__name__}: {e}'[:160]))
    result[f'{viv}::{anm}']=entry
    ok=sum(c['ok'] for c in entry['clips'])
    print(f'{anm}: {len(skel.bones)} bones, {ok}/{len(entry["clips"])} clips decode')
p=HERE.parent/'tests'/'data'/'anim_golden.json'
p.write_text(json.dumps(result,separators=(',',':'),allow_nan=False),encoding='utf-8',newline='\n')
print('wrote',p,p.stat().st_size//1024,'KiB')
