"""Persistent JSON-lines decoder worker shared by native viewer and headless tests.

No network service. Original files are read-only. Derived previews live in the
Bevy asset cache; they are the same GLB/PNG assets that the game can consume.
"""
import sys,os,json,hashlib,time,traceback,struct,io,contextlib
from pathlib import Path
from functools import lru_cache

BASE=Path(sys.executable).parent if getattr(sys,'frozen',False) else Path(__file__).resolve().parents[1]
REMASTER=BASE.parent/'Remaster'
sys.path.insert(0,str(REMASTER/'src'))
import core,research
from formats import tpl,tpl_rgba
VERSION='native-5-materials-4'
CACHE=BASE/'assets/generated'


def catalog():
    report_path=BASE/'research/coverage.json'
    if not report_path.exists():report_path=REMASTER/'research/coverage.json'
    report=json.loads(report_path.read_text('utf-8'))
    records=[];seen=set()
    for r in report['records']:
        key=(r['sha256'],r['extension'])
        if key in seen:continue
        seen.add(key)
        ext=r['extension']
        kind={'.o':'Model','.gsh':'Image','.tpl':'Image','.anm':'Animation','.ske':'Skeleton','.csv':'Table','.loc':'Table','.lef':'Table','.big':'Archive','.viv':'Archive','.arc':'Archive'}.get(ext,'Other')
        records.append(dict(id=len(records),name=Path(r['name']).name,source=r['source'],kind=kind,extension=ext,size=r['size'],status=r.get('payload',{}).get('status',r['status']),family=r.get('payload',{}).get('object_family','')))
    # The reference bank is present in DATA too; keep source identity from the
    # inventory and let the UI choose compatible files explicitly.
    return dict(assets=records,root=str(core.DEFAULT_DATA),decoder_version=VERSION)


@lru_cache(maxsize=8)
def _raw(source,mtime,size):
    return research.read_virtual(source)


def raw(source):
    stat=Path(source.split('::')[0]).stat()
    return _raw(source,stat.st_mtime_ns,stat.st_size)


def local(source):
    data,name=raw(source)
    return research.materialize(data,name)


def siblings(source,suffix):
    if '::' in source:
        parent,name=source.rsplit('::',1);data,_=raw(parent)
        prefix=name.replace('\\','/').rsplit('/',1)[0]+'/' if '/' in name.replace('\\','/') else ''
        return [parent+'::'+e['name'] for e in research.entries(data) or [] if e['name'].lower().endswith(suffix) and e['name'].replace('\\','/').startswith(prefix)]
    return [str(p) for p in Path(source).parent.glob('*'+suffix)]


def metadata(source):
    data,name=raw(source);ext=Path(name).suffix.lower()
    out=dict(source=source,name=Path(name).name,extension=ext)
    if ext=='.anm':
        bank=core.AnimationBank(local(source))
        out['items']=[dict(index=i,name=n) for i,n in enumerate(bank.names)]
    elif data[:4]==b'SHPG':
        g,_=core.gsh_parser.parse_gsh(local(source))
        out['items']=[dict(index=i,name=e.full_name or e.name,width=e.width,height=e.height,format=e.format_label) for i,e in enumerate(g.entries)]
    elif data[:4]==b'\x00 \xaf0':
        out['items']=[dict(index=i,name=f"Image {i}",width=e['width'],height=e['height'],format=e['format_name']) for i,e in enumerate(tpl(data)['entries'])]
    elif ext=='.o':
        import frontend_models
        parsed=frontend_models.parse(local(source))
        if parsed:out['items']=parsed[0].submodels;out['item_kind']='shape'
    elif ext not in ('.o','.ske'):
        out['inspection']=research.inspect(source)
    return out


def preview(req):
    source=req['source'];data,name=raw(source);ext=Path(name).suffix.lower()
    inputs=[source]+[req[k] for k in ('model','skeleton','bank') if req.get(k)]
    from asset_links import texture_sources
    textures=texture_sources(req.get('model') or source)
    textures+=req.get('textures',[])
    textures=list(dict.fromkeys(textures))
    # Include actual input content in cache keys; decoding is safe across reloads.
    hashes=[hashlib.sha256(raw(s)[0]).hexdigest() for s in inputs+textures]
    key=hashlib.sha256(json.dumps([VERSION,req,hashes],sort_keys=True).encode()).hexdigest()[:24]
    folder=CACHE/key;report=folder/'preview.json'
    if report.exists():
        value=json.loads(report.read_text());value['cache_hit']=True;return value
    started=time.perf_counter();folder.mkdir(parents=True,exist_ok=True)
    warnings=[];out=dict(kind='inspection',source=source,name=Path(name).name,warnings=warnings,cache_hit=False)
    path=local(source);index=int(req.get('index',0))
    if index<0:raise ValueError('Preview index must be nonnegative')
    if ext=='.o':
        import model_structure
        structure=model_structure.inspect(data)
        if structure and structure['empty_draw_lists']:
            out.update(inspection=research.inspect(source,True),decode_ms=round((time.perf_counter()-started)*1000,2),decoder_version=VERSION)
            report.write_text(json.dumps(out,indent=2),encoding='utf-8')
            return out
    skel_source=req.get('skeleton')
    if not skel_source and ext in ('.o','.anm'):
        options=siblings(source,'.ske')
        if len(options)==1:skel_source=options[0]
    skel=core.load_skeleton(local(skel_source)) if skel_source else None
    if ext=='.o' or (ext=='.anm' and req.get('model')):
        model_source=req.get('model') if ext=='.anm' else source
        mpath=local(model_source);result,materials=core.load_model(mpath)
        frontend=hasattr(result,'submodels')
        if frontend:
            selected=result.submodels[index]
            result.meshes=[m for m in result.meshes if m.index in selected['mesh_indices']]
            materials={m.index:materials[m.index] for m in result.meshes}
            out.update(frontend=True,shape=selected['name'],shape_count=len(result.submodels))
        if skel:
            joint_ids=[j for m in result.meshes for weights in m.vertex_joints.values() for j in weights[3:6]]
            if joint_ids and max(joint_ids)>=len(skel.bones):raise ValueError('Selected skeleton has too few bones for this model; choose a matching skeleton')
        bank_source=source if ext=='.anm' else req.get('bank')
        clip=None
        if bank_source:
            if not skel:raise ValueError('Choose the matching skeleton to play this animation on a model')
            clip=core.AnimationBank(local(bank_source)).decode(index,skel)
            warnings.extend(clip.caveats)
        if result.total_faces>1_000_000 or len(result.meshes)>4096:raise ValueError('Model exceeds interactive preview limits; inspect/export headlessly')
        material_report=[]
        texture_paths=[local(t) for t in textures]
        blob=core.model_glb(mpath,result,materials,skeleton=skel,clip=clip,textures=texture_paths,log=warnings.append,material_report=material_report)
        origins={str(p):s for p,s in zip(texture_paths,textures)}
        for material in material_report:
            for ref in material.get('sources',[]):ref['bank']=origins.get(ref['bank'],ref['bank'])
        out['material_report']=material_report
        out['texture_banks']=textures
        out['material_warnings']=[w for w in warnings if w.startswith(('Material ','Texture ','Missing APT'))]
        out['shader_families']=sorted({getattr(m,'shader_family',m.layout) for m in result.meshes})
        out['material_bindings']=[dict(mesh=m.index,shader=getattr(m,'shader_family',m.layout),textureless=getattr(m,'textureless',False),textures=getattr(m,'material_bindings',[])) for m in result.meshes]
        doc=core.validate_glb(blob)
        (folder/'asset.glb').write_bytes(blob)
        coords=[p for m in result.meshes for p in m.positions]
        if frontend:coords=[(x,-y,z) for x,y,z in coords]
        out.update(kind='model',asset=f'generated/{key}/asset.glb',triangles=result.total_faces,meshes=len(doc.get('meshes',[])),materials=len(doc.get('materials',[])),textures=len(doc.get('images',[])),bones=len(skel.bones) if skel else 0,
                   bounds=[list(map(min,zip(*coords))),list(map(max,zip(*coords)))],duration=clip.frame_times[-1] if clip else 0,animation=clip.name if clip else '',skinned=bool(doc.get('skins')))
        warnings.extend(result.log)
        warnings.extend(w for m in result.meshes for w in m.warnings)
        warnings[:]=list(dict.fromkeys(warnings))
        out['material_warnings']=[w for w in warnings if w.startswith(('Material ','Texture ','Missing APT'))]
    elif ext in ('.anm','.ske'):
        if ext=='.ske':skel=core.load_skeleton(path)
        if not skel:raise ValueError('Choose the matching skeleton to preview this animation bank')
        clip=core.AnimationBank(path).decode(index,skel) if ext=='.anm' else None
        blob=core.AnimationBank(path).export(clip,skel) if clip else core.eagl_skeleton.build_skeleton_gltf(skel)
        (folder/'asset.glb').write_bytes(blob)
        coords=[b.world_translation for b in skel.bones]
        out.update(kind='skeleton',asset=f'generated/{key}/asset.glb',bones=len(skel.bones),bounds=[list(map(min,zip(*coords))),list(map(max,zip(*coords)))],duration=clip.frame_times[-1] if clip else 0,animation=clip.name if clip else '')
        if clip:warnings.extend(clip.caveats)
    elif data[:4] in (b'SHPG',b'\x00 \xaf0'):
        if data[:4]==b'SHPG':
            g,_=core.gsh_parser.parse_gsh(path);entry=g.entries[index]
            rgba,w,h=core.gsh_parser.decode_entry_rgba(entry,data)
            out['image_name']=entry.full_name or entry.name
        else:rgba,w,h=tpl_rgba(data,tpl(data)['entries'][index])
        if w*h>16_777_216:raise ValueError('Image exceeds interactive pixel limit')
        (folder/'image.png').write_bytes(core.encode(rgba,w,h))
        out.update(kind='image',asset=f'generated/{key}/image.png',width=w,height=h)
    else:out['inspection']=research.inspect(source)
    out.update(decode_ms=round((time.perf_counter()-started)*1000,2),decoder_version=VERSION)
    if out.get('asset') and (folder/Path(out['asset']).name).stat().st_size>128*1024**2:raise ValueError('Preview exceeds 128 MiB transport budget')
    report.write_text(json.dumps(out,default=research.json_default,allow_nan=False,indent=2),encoding='utf-8')
    return out


def execute(req):
    command=req.get('command','preview')
    if command in ('extract','decode','inventory','validate','hexdump','strings'):
        import cli
        argv=[command,req['source']]
        for key in ('out','skeleton','offset','length','minimum'):
            if req.get(key) is not None:argv += ['--'+key,str(req[key])]
        if command=='decode' and 'index' in req:argv += ['--clip',str(req['index'])]
        if req.get('deep'):argv.append('--deep')
        stdout=io.StringIO();stderr=io.StringIO()
        with contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(stderr):
            code=cli.main(argv)
        if code==1:raise ValueError(stderr.getvalue().strip())
        result=json.loads(stdout.getvalue());result['exit_code']=code
        return result
    if command=='catalog':return catalog()
    if command=='metadata':return metadata(req['source'])
    if command=='inspect':return research.inspect(req['source'],True)
    if command=='preview':return preview({k:v for k,v in req.items() if k not in ('id','command')})
    if command=='select':
        value=dict(metadata=metadata(req['source']))
        if req.get('bank') and req['source'].lower().endswith('.o'):
            value['metadata']['items']=metadata(req['bank']).get('items',[])
        try:value['preview']=preview({k:v for k,v in req.items() if k not in ('id','command')})
        except Exception as exc:value['preview_error']=str(exc)
        return value
    raise ValueError('Unknown bridge command')


def main():
    for line in sys.stdin:
        req={}
        try:
            req=json.loads(line);value=execute(req)
            response=dict(id=req.get('id',0),ok=True,value=value)
        except Exception as exc:
            response=dict(id=req.get('id',0),ok=False,error=str(exc))
            traceback.print_exc(file=sys.stderr)
        print(json.dumps(response,ensure_ascii=False,default=research.json_default,allow_nan=False),flush=True)


if __name__=='__main__':
    sys.stdout.reconfigure(encoding='utf-8');sys.stdin.reconfigure(encoding='utf-8')
    main()
