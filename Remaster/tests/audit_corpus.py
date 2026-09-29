"""Repeatable read-only checks on real game files; exports stay in Remaster."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import hashlib
import json
import math
import struct
import time
from collections import Counter
import core
from archives import BigArchive
from preview import pose


def check_glb(data):
    doc=core.validate_glb(data)
    jlen=struct.unpack_from('<I',data,12)[0]
    blob=data[20+jlen+8:]
    sizes={'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4,'MAT4':16}
    for acc in doc.get('accessors',[]):
        view=doc['bufferViews'][acc['bufferView']]
        components=sizes[acc['type']]
        width={5121:1,5123:2,5125:4,5126:4}[acc['componentType']]
        size=acc['count']*components*width
        assert acc.get('byteOffset',0)+size<=view['byteLength']
        if acc['componentType']==5126:
            off=view.get('byteOffset',0)+acc.get('byteOffset',0)
            assert all(math.isfinite(x[0]) for x in struct.iter_unpack('<f',blob[off:off+size]))
    for anim in doc.get('animations',[]):
        for sampler in anim['samplers']:
            assert doc['accessors'][sampler['input']]['count']==doc['accessors'][sampler['output']]['count']
        for channel in anim['channels']:
            assert channel['target']['node']<len(doc['nodes'])
            if channel['target']['path']=='rotation':
                sampler=anim['samplers'][channel['sampler']]
                acc=doc['accessors'][sampler['output']]
                view=doc['bufferViews'][acc['bufferView']]
                off=view.get('byteOffset',0)+acc.get('byteOffset',0)
                values=struct.iter_unpack('<4f',blob[off:off+acc['count']*16])
                assert all(abs(sum(x*x for x in q)-1)<1e-5 for q in values)
    return doc


def main():
    report={'archives':{},'animations':{},'models':[],'textures':[],'errors':[]}
    started=time.time()
    files=sorted(list(core.DEFAULT_DATA.rglob('*.big'))+list(core.DEFAULT_DATA.rglob('*.viv')))
    count=compressed=0
    extensions=Counter()
    old={}
    for p in core.DEFAULT_OLD.rglob('*'):
        if p.is_file():old.setdefault(p.name.casefold(),[]).append(p)
    matched=0
    for path in files:
        try:
            a=BigArchive(path)
            count+=len(a.entries)
            for e in a.entries:
                extensions[Path(e.name).suffix.lower()]+=1
                if e.compressed:
                    data=a.read(e)
                    compressed+=1
                    candidates=old.get(Path(e.name).name.casefold(),[])
                    if any(p.stat().st_size==len(data) and p.read_bytes()==data for p in candidates):matched+=1
        except Exception as exc:report['errors'].append({'file':str(path),'error':str(exc)})
    report['archives']={'count':len(files),'entries':count,'refpack_decoded':compressed,'exact_old_file_matches':matched,'entry_extensions':dict(extensions)}
    print('ARCHIVES',report['archives'],flush=True)
    bank=core.AnimationBank(core.HOME/'reference/player_anims.anm')
    skel=core.eagl_skeleton.parse_ske_file(core.HOME/'reference/player_skel.ske')
    clips=[]
    for i in range(len(bank.blocks)):
        try:
            clip=bank.decode(i,skel)
            glb=bank.export(clip,skel)
            check_glb(glb)
            assert all(math.isfinite(x) for p in pose(skel,clip,clip.sample_count-1) for x in p)
            clips.append({'index':i,'name':clip.name,'samples':clip.sample_count,'bytes':len(glb),'status':'ok'})
            if i in (0,1,100):
                folder=core.HOME/'exports'/'verified_samples'
                folder.mkdir(parents=True,exist_ok=True)
                (folder/f'{i:03d}_{core.filename(clip.name)}.glb').write_bytes(glb)
        except Exception as exc:
            clips.append({'index':i,'status':'error','error':str(exc)})
        if i%50==0:print('CLIPS',i,flush=True)
    report['animations']={'bones':len(skel.bones),'clips':clips,'passed':sum(c['status']=='ok' for c in clips),'assumed_fps':30}
    paths=[core.DEFAULT_OLD/'WorldProps'/name for name in ('basketball.o','ds_lite.o','rc_controller.o','paper_plane_big.o','footieball.o')]
    paths += [core.DEFAULT_OLD/'World'/'world-low-all.o']
    chars=list((core.DEFAULT_OLD/'Characters').rglob('alicia.o'))+list((core.DEFAULT_OLD/'Characters').rglob('timothy.o'))
    paths+=chars[:2]
    for path in paths:
        try:
            result,mats=core.load_model(path)
            for mesh in result.meshes:
                for face in mesh.faces:
                    for p,n,t in face:
                        assert 0<=p<len(mesh.positions) and 0<=n<len(mesh.normals) and 0<=t<len(mesh.uvs)
            notes=[]
            glb=core.model_glb(path,result,mats,skel if path in chars else None,log=notes.append)
            doc=check_glb(glb)
            if path in chars:
                animated=core.model_glb(path,result,mats,skel,bank.decode(0,skel),log=notes.append)
                a=check_glb(animated)
                assert a.get('skins') and a.get('animations')
                (core.HOME/'exports/verified_samples'/f'{path.stem}_animated.glb').write_bytes(animated)
            (core.HOME/'exports/verified_samples'/f'{path.stem}.glb').write_bytes(glb)
            item={'file':str(path),'meshes':len(result.meshes),'triangles':result.total_faces,'images':len(doc.get('images',[])),'status':'ok','notes':notes}
        except Exception as exc:item={'file':str(path),'status':'error','error':str(exc)}
        report['models'].append(item)
        print('MODEL',path.name,item.get('status'),item.get('triangles'),item.get('error',''),flush=True)
    tex_paths=[core.DEFAULT_OLD/'WorldProps/basketball.gsh']+list((core.DEFAULT_OLD/'World/auto').glob('*.gsh'))
    for path in tex_paths:
        try:
            gsh,data=core.gsh_parser.parse_gsh(path)
            entries=[]
            for entry in gsh.entries:
                try:
                    if entry.record_id in (24,25) and not entry.palette:raise ValueError('Missing palette')
                    rgba,w,h=core.gsh_parser.decode_entry_rgba(entry,data)
                    png=core.encode(rgba,w,h)
                    entries.append({'name':entry.full_name or entry.name,'format':entry.format_label,'status':'ok','size':[w,h]})
                    if path.stem=='basketball':(core.HOME/'exports/verified_samples/basketball.png').write_bytes(png)
                except Exception as exc:entries.append({'name':entry.name,'status':'error','error':str(exc)})
            report['textures'].append({'file':str(path),'entries':entries,'passed':sum(e['status']=='ok' for e in entries)})
            print('TEXTURE',path.name,len(entries),flush=True)
        except Exception as exc:report['errors'].append({'file':str(path),'error':str(exc)})
    report['seconds']=round(time.time()-started,2)
    (core.HOME/'docs/verification.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('REPORT',core.HOME/'docs/verification.json',flush=True)


if __name__=='__main__':main()
