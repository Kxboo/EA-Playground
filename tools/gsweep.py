"""Sweep accelerometer waveforms for a minigame and report which Conga callbacks fire.
usage: python tools/gsweep.py <type> [frame0] [frames]"""
import subprocess,sys,itertools,os,concurrent.futures as cf,re
ty=sys.argv[1]; f0=int(sys.argv[2]) if len(sys.argv)>2 else 300; frames=int(sys.argv[3]) if len(sys.argv)>3 else f0+150
rest=[512,512,616]
prefix=os.environ.get('PREFIX','')  # extra EAGL_MG_ACC segments held fixed
def wave(segs):
    # segs: list of (axis, delta, length)
    out=[];t=f0
    for ax,dl,ln in segs:
        v=list(rest); v[ax]+=dl
        out.append(f"{t}-{t+ln}:{v[0]},{v[1]},{v[2]}"); t+=ln
    return ';'.join(([prefix] if prefix else [])+out)
cands={}
for ax in range(3):
    for sg in (1,-1):
        for mag in (250,450):
            for ln in (1,3,6):
                cands[f"ax{ax}{'+' if sg>0 else '-'} m{mag} l{ln}"]=[(ax,sg*mag,ln)]
        for mag in (250,450):
            cands[f"ax{ax}{'+' if sg>0 else '-'}then{'-' if sg>0 else '+'} m{mag}"]=[(ax,sg*mag,3),(ax,-sg*mag,3)]
def run(item):
    name,segs=item
    env=dict(os.environ,EAGL_MG_ACC=wave(segs),EAGL_MG_FRAMES=str(frames),EAGL_PPC_CALLS="Callback__")
    r=subprocess.run(['D:/_eagl/_bevy/target/lab/mglab.exe','probe',ty],capture_output=True,text=True,env=env,cwd='D:/_eagl/_bevy')
    hits=sorted(set(re.findall(r'\[call\] (\w+Callback)__',r.stdout+r.stderr)))
    hits=[h for h in hits if 'Conga' not in h and 'Alloc' not in h]
    return name,hits
if os.environ.get('PAIRS'):
    if os.environ.get('PAIRS')=='only': cands={}
    for a1 in range(3):
        for s1 in (1,-1):
            for a2 in range(3):
                for s2 in (1,-1):
                    for ln in (2,4):
                        for gap in (0,3):
                            segs=[(a1,s1*450,ln)]+([(a1,0,gap)] if gap else [])+[(a2,s2*450,ln)]
                            cands[f"{a1}{'+' if s1>0 else '-'}>{a2}{'+' if s2>0 else '-'} l{ln} g{gap}"]=segs
with cf.ThreadPoolExecutor(4) as ex:
    for name,hits in ex.map(run,cands.items()):
        if hits: print(name,hits,flush=True)
