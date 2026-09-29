"""Explicit integration boundary around recovered parsers."""
from pathlib import Path
import hashlib
import json
import os
import re
import sys
import struct
import math
from types import SimpleNamespace
from archives import BigArchive, decompress
from png import encode

HOME = Path(sys.executable).parent if getattr(sys, 'frozen', False) else Path(__file__).resolve().parents[1]
LEGACY = Path(__file__).parent / 'legacy'
sys.path.insert(0, str(LEGACY))
os.environ['EAGL_DATA_DIR'] = str(HOME / 'reference')
os.environ['EAGL_ELF_PATH'] = str(HOME / 'reference' / 'playgroundz.elf')
import parser as model_parser
import gsh_parser
import eagl_exporter
import eagl_inspect
import eagl_skeleton
import eagl_mesh_material_map
from exporter.anm_exporter import parse_anm_clips, decode_clip_bones
from exporter.gltf_writer import build_clip_glb

gsh_parser.rgba_to_png_bytes = encode
gsh_parser.save_png = lambda rgba, w, h, path: Path(path).write_bytes(encode(rgba, w, h))

SOURCE = HOME.parent / 'eagl EA PLAYGROUND' / 'extra' / 'more' / 'eaplayground files'
DEFAULT_DATA = SOURCE / 'DATA'
DEFAULT_OLD = SOURCE / 'OLD ATTEMPTS' / 'EA PLAYGROUND EXTRACTED'
SUPPORTED = {'.o', '.gsh', '.anm', '.ske', '.big', '.viv', '.png'}


def filename(name):
    value = re.sub(r'[^\w.\- ]', '_', name or 'unnamed').strip(' .')[:120] or 'unnamed'
    if value.split('.')[0].upper() in {'CON','PRN','AUX','NUL', *(f'COM{i}' for i in range(1,10)), *(f'LPT{i}' for i in range(1,10))}:
        value = '_' + value
    return value


def write_new(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as f:
        f.write(data)
    return path


def prepared(path):
    path = Path(path)
    data = path.read_bytes()
    raw = decompress(data)
    if raw is data:
        return path
    folder = HOME / 'cache' / hashlib.sha256(data).hexdigest()[:16]
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / path.name
    target.write_bytes(raw)
    return target


def load_model(path):
    path = prepared(path)
    from material_bindings import bind
    import frontend_models
    frontend=frontend_models.parse(path)
    if frontend is not None:return bind(path,*frontend)
    import hwskin_models
    hwskin=hwskin_models.parse(path)
    if hwskin is not None:return bind(path,*hwskin)
    import shadow_models
    shadow=shadow_models.parse(path)
    if shadow is not None:
        if not shadow.ok:raise ValueError('No shadow triangles decoded')
        return bind(path,shadow,{m.index:{'PlaygroundShadow'} for m in shadow.meshes})
    inspect = eagl_inspect.inspect_o_file(path)
    result = model_parser.parse_o_file(path, inspect_result=inspect, layouts_path=LEGACY/'eagl_layouts.json')
    if not result.ok:
        import model_structure
        structure=model_structure.inspect(path.read_bytes())
        if structure and structure['empty_draw_lists']:
            raise ValueError('This model declares empty draw lists; use Inspect decoded structure for its bounds and named nodes.')
        raise ValueError(result.summary())
    for mesh in result.meshes:
        if any(not math.isfinite(x) for row in mesh.positions + mesh.normals + mesh.uvs for x in row):
            raise ValueError('Non-finite model coordinates: this layout is not safely decoded')
        if any(not (0 <= p < len(mesh.positions) and 0 <= n < len(mesh.normals) and 0 <= t < len(mesh.uvs))
               for face in mesh.faces for p, n, t in face):
            raise ValueError('Model face index exceeds a decoded vertex array')
    _, materials, _ = eagl_mesh_material_map.map_meshes_to_materials(str(path), model_parser, result=result)
    return bind(path,result,materials)


def load_skeleton(path):
    result = eagl_skeleton.parse_ske_file(prepared(path))
    if not result.bones or not result.ok:
        raise ValueError('No supported skeleton decoded. This file may use a different layout.\n'+'\n'.join(result.log))
    return result


def texture_images(materials, paths, log=lambda text: None, report=None):
    # Only exact full-name matches or unique short-name matches; never silently
    # take the first four-letter collision from a global texture database.
    entries = []
    for path in dict.fromkeys(map(Path,paths)):
        try:
            gsh, data = gsh_parser.parse_gsh(prepared(path))
            entries.extend((e, data, str(path)) for e in gsh.entries)
        except Exception as exc:
            log(f'Texture {path.name}: {exc}')
    images, modes = {}, {}
    for name in sorted({n for names in materials.values() for n in names}):
        if name.startswith('apt-color-'):continue
        matches = [(e,d,p) for e,d,p in entries if (e.full_name or '').strip() == name.strip()]
        match_kind='full_name'
        if not matches:
            matches = [(e,d,p) for e,d,p in entries if (e.full_name or '').strip().casefold() == name.strip().casefold()]
            match_kind='casefold_full_name'
        if not matches:
            # A four-character directory alias is not evidence for overriding
            # a different full name. Only unnamed entries may use this fallback.
            matches = [(e,d,p) for e,d,p in entries if not (e.full_name or '').strip() and e.name.strip().casefold() in (name.strip().casefold(),name[:4].casefold())]
            match_kind='unnamed_directory_alias'
        item=dict(name=name,match=match_kind,candidates=len(matches))
        if report is not None:report.append(item)
        if not matches:
            item['status']='missing'
            log(f'Material {name}: no texture matches in dependency banks; left untextured')
            continue
        try:
            decoded={}
            for e,d,p in matches:
                if e.record_id in (24,25) and not e.palette:raise ValueError('Missing color palette')
                rgba,w,h=gsh_parser.decode_entry_rgba(e,d)
                fingerprint=(w,h,hashlib.sha256(rgba).hexdigest())
                decoded.setdefault(fingerprint,(rgba,w,h,[]))[3].append(dict(bank=p,entry=e.index,name=e.full_name or e.name))
            item['distinct_images']=len(decoded)
            if len(decoded)!=1:
                item.update(status='ambiguous',sources=[source for v in decoded.values() for source in v[3]])
                log(f'Material {name}: {len(decoded)} different textures share this identifier; left untextured')
                continue
            rgba,w,h,sources=next(iter(decoded.values()))
            images[name] = encode(rgba,w,h)
            modes[name] = gsh_parser.classify_alpha(rgba)
            item.update(status='resolved',width=w,height=h,pixel_sha256=next(iter(decoded))[2],sources=sources,alpha_mode=modes[name])
        except Exception as exc:
            item.update(status='error',error=str(exc))
            log(f'Material {name}: {exc}')
    return images, modes


def model_glb(path, result, materials, skeleton=None, clip=None, textures=None, log=lambda text: None, material_report=None):
    paths = textures if textures is not None else list(Path(path).parent.glob('*.gsh'))
    requested={m.index:materials.get(m.index,set()) for m in result.meshes if m.ok and not getattr(m,'textureless',False)}
    images, modes = texture_images(requested, paths, log,material_report)
    import frontend_models
    result=frontend_models.prepare_export(result,images,log)
    anim = SimpleNamespace(clips=[normalized_clip(clip)]) if clip else None
    return eagl_exporter.build_gltf(result, skel=skeleton, anm=anim, mesh_materials=materials,
                                    material_images=images, material_alpha_modes=modes)


def model_obj(result):
    lines = ['# EAGL Remaster geometry export']
    vp = vn = vt = 1
    for mesh in result.meshes:
        if not mesh.ok:
            continue
        lines.append(f'o mesh_{mesh.index}')
        lines.extend('v ' + ' '.join(map(str,v)) for v in mesh.positions)
        lines.extend('vt ' + ' '.join(map(str,v)) for v in mesh.uvs)
        lines.extend('vn ' + ' '.join(map(str,v)) for v in mesh.normals)
        for face in mesh.faces:
            lines.append('f ' + ' '.join(f'{p+vp}/{t+vt}/{n+vn}' for p,n,t in face))
        vp += len(mesh.positions)
        vt += len(mesh.uvs)
        vn += len(mesh.normals)
    return ('\n'.join(lines) + '\n').encode()


class AnimationBank:
    def __init__(self, path):
        self.path = Path(path)
        self.data, self.reloc, self.start, self.blocks, self.names = parse_anm_clips(prepared(path))

    def decode(self, index, skeleton):
        if not skeleton.bones:
            raise ValueError('The selected skeleton has no decoded bones')
        # The player bone mask is executable- and skeleton-specific.
        # Do not apply that 68-bone table to prop skeletons.
        player = len(skeleton.bones) == 68 and any(b.name == 'l_SideCheek' for b in skeleton.bones)
        result = decode_clip_bones(self.data, self.reloc, self.start, skeleton, self.blocks[index], apply_bone_mask=player)
        if result['status'] != 'ok':
            raise ValueError(result.get('reason', 'Unsupported clip codec'))
        n = result['sample_count']
        if n < 1:
            raise ValueError('Clip has no samples')
        for field in ('rot_by_bone','trans_by_bone','scale_by_bone'):
            for bone,values in result.get(field,{}).items():
                if not 0 <= bone < len(skeleton.bones) or len(values) != n:
                    raise ValueError(f'{field}: invalid bone {bone} or mismatched sample count')
                if any(v is not None and any(not math.isfinite(x) for x in v) for v in values):
                    raise ValueError(f'{field}: non-finite sample values')
        cb=self.blocks[index]
        if cb.whole_clip:
            extra=self.data[cb.abs_off+0x17]
            if self.reloc.get(cb.rel_off+8) is not None:
                raise NotImplementedError('Sparse stateless rotation times are not decoded; refusing fixed-rate export')
            if extra:result['caveats'].append(f'{extra} extra static rotation channels remain undecoded')
            if cb.secondary and cb.secondary.tag==0x17:
                extra=self.data[cb.secondary.abs_off+0x13]
                if self.reloc.get(cb.secondary.abs_off-self.start+8) is not None:
                    raise NotImplementedError('Sparse stateless translation times are not decoded; refusing fixed-rate export')
                if extra:result['caveats'].append(f'{extra} extra static translation channels remain undecoded')
        return SimpleNamespace(index=index, name=self.names[index] if index < len(self.names) else f'clip_{index}',
                               frame_times=[i/30 for i in range(n)], **result)

    def export(self, clip, skeleton):
        clip = normalized_clip(clip)
        return build_clip_glb(skeleton, clip.index, clip.name, clip.rot_by_bone, clip.trans_by_bone, clip.sample_count,
                              scale_by_bone=getattr(clip,'scale_by_bone',{}))


def normalized_clip(clip):
    """glTF requires unit rotations; keep the recovered raw codec samples intact."""
    rotations = {}
    for bone, samples in clip.rot_by_bone.items():
        values = []
        for sample in samples:
            q = sample or (0,0,0,1)
            norm = math.sqrt(sum(x*x for x in q))
            if not math.isfinite(norm) or norm < 1e-10:
                raise ValueError(f'Invalid quaternion for bone {bone}')
            q = tuple(x/norm for x in q)
            if values and sum(a*b for a,b in zip(values[-1],q)) < 0:
                q = tuple(-x for x in q)
            values.append(q)
        rotations[bone] = values
    result = dict(vars(clip))
    result['rot_by_bone'] = rotations
    return SimpleNamespace(**result)


def validate_glb(data):
    magic, version, total = struct.unpack_from('<4sII', data)
    if magic != b'glTF' or version != 2 or total != len(data):
        raise ValueError('Invalid GLB envelope')
    n, tag = struct.unpack_from('<I4s', data, 12)
    if tag != b'JSON':
        raise ValueError('GLB has no JSON chunk')
    doc = json.loads(data[20:20+n])
    for view in doc.get('bufferViews', []):
        if view.get('byteOffset',0) + view['byteLength'] > doc['buffers'][0]['byteLength']:
            raise ValueError('GLB buffer view is out of bounds')
    return doc
