"""Shared headless research API, also used by the desktop inspector."""
import hashlib
import json
import math
import re
from pathlib import Path
from collections import Counter,defaultdict
from dataclasses import asdict
from archives import decompress,is_refpack,safe_target
from containers import big_entries,u8_entries,Elf
from formats import inspect_bytes,tpl,tpl_rgba


def entries(data):
    if data[:4] in (b'BIGF',b'BIG4'):return big_entries(data)
    if data[:4]==b'U\xaa8-':return u8_entries(data)
    return None


def read_virtual(source):
    """Read outer.big::inner.viv::file.o without extracting siblings."""
    parts=str(source).split('::')
    data=decompress(Path(parts[0]).read_bytes())
    name=Path(parts[0]).name
    for member in parts[1:]:
        table=None if Path(name).suffix.lower()=='.bh' else entries(data)
        if table is None:raise ValueError(f'{name} is not a supported container')
        matches=[e for e in table if e['name'].replace('\\','/')==member.replace('\\','/')]
        if len(matches)!=1:raise ValueError(f'Expected one entry named {member!r}; found {len(matches)}')
        e=matches[0];data=decompress(data[e['offset']:e['offset']+e['size']]);name=e['name']
    return data,name


def materialize(data,name):
    import core
    digest=hashlib.sha256(data).hexdigest()
    target=core.HOME/'cache/research'/digest/(core.filename(Path(name).name))
    target.parent.mkdir(parents=True,exist_ok=True)
    if not target.exists():target.write_bytes(data)
    return target


def inspect(source,deep=False):
    data,name=read_virtual(source)
    report=inspect_bytes(data,name)
    report.update(source=str(source),sha256=hashlib.sha256(data).hexdigest())
    if deep:
        try:report['payload']=payload_report(data,name)
        except Exception as exc:report['payload']=dict(status='unsupported_or_invalid',error=str(exc))
    return report


def payload_report(data,name):
    import core
    path=materialize(data,name)
    suffix=path.suffix.lower()
    if suffix=='.o':
        import model_structure
        structure=model_structure.inspect(data)
        if structure and structure['empty_draw_lists']:
            return dict(status='structural',object_family='empty model',**structure,caveats=['Model draw lists contain no primitives. Bounds and named nodes are retained; no triangle payload is declared.'])
        symbols=[s['name'] for s in Elf(data).symbols]
        family='frontend TextureApt' if any(s in ('TextureApt','GouraudApt') for s in symbols) else 'PlaygroundShadow' if any('PlaygroundShadow' in s for s in symbols) else 'other model'
        inspector=core.eagl_inspect.inspect_o_file(path)
        import shadow_models,frontend_models,hwskin_models
        frontend=frontend_models.parse(path)
        hwskin=hwskin_models.parse(path) if frontend is None else None
        result=frontend[0] if frontend is not None else hwskin[0] if hwskin is not None else shadow_models.parse(path)
        if result is None:result=core.model_parser.parse_o_file(path,inspect_result=inspector,layouts_path=core.LEGACY/'eagl_layouts.json')
        invalid=0;nonfinite=0
        for m in result.meshes:
            nonfinite+=sum(not math.isfinite(x) for v in m.positions+m.normals+m.uvs for x in v)
            for face in m.faces:
                invalid+=sum(not(0<=p<len(m.positions) and 0<=n<len(m.normals) and 0<=t<len(m.uvs)) for p,n,t in face)
        return dict(status='partial' if result.ok and not invalid and not nonfinite else 'unsupported_or_invalid',model=result.model_name,object_family=family,
                    confidence=str(inspector.confidence),meshes=len(result.meshes),empty_meshes=sum(not m.ok for m in result.meshes),triangles=result.total_faces,
                    layouts=dict(Counter(m.layout for m in result.meshes)),invalid_face_indices=invalid,nonfinite_values=nonfinite,
                    warnings=result.log+[w for m in result.meshes for w in m.warnings],
                    caveats=['Geometry export is not proof of descriptor coverage, topology, scale, material or skinning correctness.'])
    if suffix=='.ske':
        skel=core.load_skeleton(path)
        return dict(status='partial',bones=[dict(index=b.index,name=b.name,parent=b.parent_idx,translation=b.local_translation,rotation=b.quaternion,scale=b.scale) for b in skel.bones],warnings=skel.log)
    if suffix=='.anm':
        bank=core.AnimationBank(path)
        return dict(status='partial',clip_count=len(bank.blocks),clips=[dict(index=b.index,name=bank.names[i] if i<len(bank.names) else None,block=asdict(b)) for i,b in enumerate(bank.blocks)],
                    caveats=['Clip structures indexed. Use decode with --skeleton for sample data.'])
    if data[:4]==b'SHPG':
        gsh,raw=core.gsh_parser.parse_gsh(path);images=[]
        for e in gsh.entries:
            item=dict(index=e.index,name=e.full_name or e.name,format=e.format_label,recovered=e.recovered,orphan=e.orphan)
            try:
                if e.record_id in (24,25) and not e.palette:raise ValueError('Missing palette')
                rgba,w,h=core.gsh_parser.decode_entry_rgba(e,raw)
                if len(rgba)!=w*h*4:raise ValueError('Wrong decoded pixel count')
                item.update(status='decoded_pixels',width=w,height=h,pixel_sha256=hashlib.sha256(rgba).hexdigest())
            except Exception as exc:item.update(status='error',error=str(exc))
            images.append(item)
        return dict(status='partial',declared_entries=gsh.object_count,parsed_entries=len(images),images=images,
                    caveats=['Pixel byte count checked; visual fidelity and recovered/stub record boundaries remain separate questions.'])
    if data[:4]==b'\x00 \xaf0':
        images=[]
        for e in tpl(data)['entries']:
            rgba,w,h=tpl_rgba(data,e)
            images.append(dict(index=e['index'],width=w,height=h,pixel_sha256=hashlib.sha256(rgba).hexdigest(),status='decoded_pixels'))
        return dict(status='partial',images=images)
    return dict(status='not_implemented',caveats=['No semantic payload decoder registered for this format.'])


def catalog(root,deep=False,progress=lambda s:None,max_depth=8,max_bytes=2*1024**3):
    root=Path(root).resolve()
    records=[];errors=[];cache={};expanded={};total=0
    def visit(source,name,stored,depth):
        nonlocal total
        if depth>max_depth:raise ValueError('Archive nesting limit reached')
        data=decompress(stored)
        total+=len(data)
        if total>max_bytes:raise ValueError('Expanded-byte budget reached')
        sha=hashlib.sha256(data).hexdigest()
        key=(sha,Path(name).suffix.lower())
        if key not in cache:
            try:
                r=inspect_bytes(data,name,full=False)
                if deep and (Path(name).suffix.lower() in ('.o','.ske','.anm','.gsh','.tpl') or data[:4] in (b'SHPG',b'\x00 \xaf0')):
                    try:r['payload']=payload_report(data,name)
                    except Exception as exc:r['payload']=dict(status='unsupported_or_invalid',error=str(exc))
            except Exception as exc:r=dict(format='parse failure',status='error',error=str(exc),signature_hex=data[:32].hex())
            cache[key]=r
        record=dict(source=source,name=name,extension=Path(name).suffix.lower() or '(none)',stored_size=len(stored),size=len(data),refpack=is_refpack(stored),sha256=sha,**{k:v for k,v in cache[key].items() if k!='size'})
        records.append(record)
        if len(records)%100==0:progress(f'{len(records)} records, {len(cache)} distinct contents')
        # Retain every container instance, expand each content once and record
        # which previously expanded source owns its identical member set.
        if sha in expanded:
            record['members_expanded_at']=expanded[sha]
            return
        table=None if Path(name).suffix.lower()=='.bh' else entries(data)
        if table is not None:
            expanded[sha]=source
            for e in table:
                child=source+'::'+e['name']
                try:visit(child,e['name'],data[e['offset']:e['offset']+e['size']],depth+1)
                except Exception as exc:errors.append(dict(source=child,error=str(exc)))
    paths=sorted(p for p in root.rglob('*') if p.is_file()) if root.is_dir() else [root]
    for path in paths:
        try:visit(str(path),path.name,path.read_bytes(),0)
        except Exception as exc:errors.append(dict(source=str(path),error=str(exc)))
        if total>max_bytes:break
    return dict(root=str(root),deep=deep,scope='All loose files; recursively expanded BIG/VIV and U8 containers. Identical container content expanded once.',
                records=records,errors=errors,summary=dict(loose_files=len(paths),records=len(records),unique_content=len({r['sha256'] for r in records}),
                    extensions=dict(Counter(r['extension'] for r in records)),formats=dict(Counter(r['format'] for r in records)),
                    statuses=dict(Counter(r['status'] for r in records)),expanded_bytes=total),
                caveats=['Statuses are not a completeness score. Structural/recognized/unknown records still need semantic work. Files inside unsupported container/compression types are not enumerated.'])


def coverage_markdown(report):
    groups=defaultdict(list)
    for r in report['records']:groups[r['extension']].append(r)
    lines=['# Format coverage','',report['scope'],'','A recognized header or a successful export is not a complete decode.','',
           '| Extension | Records | Formats | Statuses |','|---|---:|---|---|']
    for ext,rows in sorted(groups.items()):
        lines.append(f'| {ext} | {len(rows)} | '+', '.join(sorted({r['format'] for r in rows}))+' | '+', '.join(f'{k}: {v}' for k,v in Counter(r['status'] for r in rows).items())+' |')
    lines+=['','## Payload decoder checks','','These are distinct file contents, separate from header/structure status above.','',
            '| Extension | Unique files | Payload results |','|---|---:|---|']
    for ext,rows in sorted(groups.items()):
        unique={r['sha256']:r for r in rows if 'payload' in r}
        if not unique:continue
        counts=Counter(r['payload']['status'] for r in unique.values())
        lines.append(f'| {ext} | {len(unique)} | '+', '.join(f'{k}: {v}' for k,v in counts.items())+' |')
    lines+=['','Full paths, signatures, hashes, payload checks and parser errors are in the JSON catalog.','',f'Traversal errors: {len(report["errors"])}.']
    return '\n'.join(lines)+'\n'


def decode(source,out,skeleton=None,clip_index=None):
    import core
    data,name=read_virtual(source);path=materialize(data,name);out=Path(out)
    out.mkdir(parents=True,exist_ok=True)
    written=[]
    def write(name,value):
        target=safe_target(out,name)
        core.write_new(target,value);written.append(str(target))
    if path.suffix.lower()=='.o':
        from asset_links import texture_sources
        result,materials=core.load_model(path)
        write(path.stem+'.obj',core.model_obj(result))
        warnings=[];material_report=[]
        texture_paths=[materialize(*read_virtual(s)) for s in texture_sources(str(source))]
        write(path.stem+'.glb',core.model_glb(path,result,materials,textures=texture_paths,log=warnings.append,material_report=material_report))
        report=payload_report(data,name)
        report.update(material_report=material_report,material_warnings=warnings)
    elif path.suffix.lower()=='.ske':
        skel=core.load_skeleton(path);write(path.stem+'.glb',core.eagl_skeleton.build_skeleton_gltf(skel));report=payload_report(data,name)
    elif path.suffix.lower()=='.anm':
        if not skeleton:raise ValueError('Animation sample decode requires --skeleton PATH (virtual paths allowed)')
        sdata,sname=read_virtual(skeleton);skel=core.load_skeleton(materialize(sdata,sname));bank=core.AnimationBank(path)
        indices=range(len(bank.blocks)) if clip_index is None else [clip_index];clips=[]
        for i in indices:
            c=bank.decode(i,skel)
            write(f'{i:03d}_{core.filename(c.name)}.glb',bank.export(c,skel))
            clips.append(vars(c))
        report=dict(status='partial',clips=clips,raw_values=True,caveats=['Raw samples retained without quaternion normalization; GLB rotations normalized. Frame times still assume 30 fps.'])
    elif data[:4]==b'SHPG':
        gsh,raw=core.gsh_parser.parse_gsh(path);results=[]
        for e in gsh.entries:
            try:
                if e.record_id in (24,25) and not e.palette:raise ValueError('Missing palette')
                rgba,w,h=core.gsh_parser.decode_entry_rgba(e,raw)
                write(f'{e.index:03d}_{core.filename(e.full_name or e.name)}.png',core.encode(rgba,w,h));results.append(dict(index=e.index,status='ok'))
            except Exception as exc:results.append(dict(index=e.index,status='error',error=str(exc)))
        report=dict(status='partial',images=results)
    elif data[:4]==b'\x00 \xaf0':
        report=tpl(data)
        for e in report['entries']:
            rgba,w,h=tpl_rgba(data,e);write(f'{e["index"]:03d}.png',core.encode(rgba,w,h))
    else:
        report=inspect_bytes(data,name)
        if report['status'] in ('recognized','unknown'):raise ValueError(f'{report["format"]}: semantic decoder not implemented; use inspect/hexdump/strings')
    write('decoded.json',json.dumps(report,ensure_ascii=False,indent=2,default=json_default,allow_nan=False).encode('utf-8'))
    return dict(source=source,outputs=written,status='completed',caveat='Read decoded.json for partial support and warnings.')


def json_default(value):
    if isinstance(value,bytes):return {'hex':value.hex()}
    if isinstance(value,Path):return str(value)
    if isinstance(value,set):return sorted(value)
    raise TypeError(type(value).__name__)
