"""Unified, schema-driven decoder for EAGL `.o` models (prototype for the Rust port in _bevy/src/model.rs).

Every shader family in research/shader-schemas.json is a struct whose fields are (count, pointer) pairs at fixed offsets;
the PCode descriptor at +4 (ProcessPCode 0x803ef24c) declares the GX vertex attributes and the display list.  Geometry
is therefore decoded from executable-confirmed layouts instead of layout scoring/heuristics.
"""
import json,math,re,struct
from pathlib import Path
from containers import Elf,span
from pcode import decode

SCHEMAS=json.loads((Path(__file__).resolve().parents[1]/'research'/'shader-schemas.json').read_text(encoding='utf-8'))['schemas']
FIELD={fam:{f['name']:(f['count_offset'],f['pointer_offset'],f['element_size']) for f in fields} for fam,fields in SCHEMAS.items()}

def gx_faces(stream,width,count):
    """GX display list -> list of (a,b,c) vertex slots.  `width` is the per-vertex byte stride and slot = running vertex number."""
    return None

def parse(data):
    elf=Elf(data);section=elf.section('.data')
    if section is None:return None
    raw=elf.section_bytes(section);symbols={(s['table'],s['index']):s for s in elf.symbols}
    local={};external={}
    for r in elf.relocations:
        if r['target_section']!=section['index']:continue
        sym=symbols[(r['symbol_table'],r['symbol_index'])]
        if sym['section']==section['index']:local[r['offset']]=struct.unpack('<I',span(raw,r['offset'],4))[0]+sym['value']
        elif sym['section']==0:external[r['offset']]=sym['name']
    be=lambda p:struct.unpack('>I',span(raw,p,4))[0]
    models=sorted((s for s in elf.symbols if s['name'].startswith('__Model:::')),key=lambda s:s['value'])
    out=[]
    for model in models:
        m=model['value'];count=be(m+0x9c)
        if not count:continue
        cursor=local[m+0xcc]+4;anchors=[]
        for _ in range(count):
            size=be(cursor);cursor+=4
            for _ in range(size):anchors.append(local[cursor]);cursor+=4
        out.append((model['name'].split(':::',1)[1],m,anchors))
    return raw,local,external,out

def decode_anchor(raw,local,external,anchor):
    family=external.get(anchor)
    if family not in FIELD:raise ValueError(f'shader family {family!r} has no schema')
    fields=FIELD[family];be=lambda p:struct.unpack('>I',span(raw,p,4))[0]
    program=decode(raw,local[anchor+4],local,anchor)
    arrays={}
    for name in ('Coordinates','Normals','Colours','UVs','Weights'):
        if name in fields:
            co,po,es=fields[name];n=be(anchor+co)
            if not 0<n<=1_000_000:raise ValueError(f'{family}: bad {name} count {n}')
            arrays[name]=(n,span(raw,local[anchor+po],n*es,name),es)
    return family,program,arrays


def stream_vertices(stream,stride,attrs):
    """Yield (mode, [vertex tuples of attribute indices]) for each GX draw command."""
    cursor=0
    order=sorted(attrs.items())
    while cursor<len(stream):
        op=stream[cursor];cursor+=1
        if op==0:continue
        mode=op&0xf8
        if mode not in (0x80,0x90,0x98,0xa0):raise ValueError(f'unknown GX opcode {op:02x}')
        n=struct.unpack('>H',span(stream,cursor,2))[0];cursor+=2
        blob=span(stream,cursor,n*stride,'vertices');cursor+=n*stride
        verts=[]
        for j in range(n):
            off=j*stride;v={}
            for attr,(kind,width) in order:
                v[attr]=int.from_bytes(blob[off:off+width],'big');off+=width
            verts.append(tuple(v.items()))
        yield mode,verts

def faces_of(mode,verts):
    n=len(verts)
    if mode==0x98:return [(verts[i],verts[i+1],verts[i+2]) if i%2==0 else (verts[i+1],verts[i],verts[i+2]) for i in range(n-2)]
    if mode==0xa0:return [(verts[0],verts[i],verts[i+1]) for i in range(1,n-1)]
    if mode==0x90:return [tuple(verts[i:i+3]) for i in range(0,n-n%3,3)]
    return [f for i in range(0,n-n%4,4) for f in ((verts[i],verts[i+1],verts[i+2]),(verts[i],verts[i+2],verts[i+3]))]

def triangles_for(data):
    raw,local,external,models=parse(data)
    total=0;total_pos=0;fams=set();seen=set()
    for name,m,anchors in models:
        for a in anchors:
            if a in seen:continue
            seen.add(a)
            family,program,arrays=decode_anchor(raw,local,external,a);fams.add(family)
            stride=sum(w for _,(k,w) in program['attributes'].items())
            stream=span(raw,program['offset'],program['length'],'display list')
            for mode,verts in stream_vertices(stream,stride,program['attributes']):
                for f in faces_of(mode,verts):
                    if len(set(f))==3:total+=1
                    if len({dict(v)[9] for v in f})==3:total_pos+=1
    return total,total_pos,fams


TEXTURE_FIELDS=('Texture','Texture1','Texture2','Texture3')

def _tex_name(external,anchor,fields,name):
    if name not in fields:return None
    sym=external.get(anchor+fields[name][1]);
    if not sym:return None
    m=re.search(r'(?:^|;)1=([^,;]+),',sym)
    return m[1] if m else sym

APT=('TextureApt','GouraudApt')

def decode_primitive(raw,local,external,anchor,model=None):
    """One shader primitive -> unique vertices + triangles (GX winding reversed to CCW), or raises."""
    family,program,arrays=decode_anchor(raw,local,external,anchor);fields=FIELD[family]
    attrs=program['attributes'];fraction=program['fraction']
    stride=sum(w for _,(k,w) in attrs.items())
    stream=span(raw,program['offset'],program['length'],'display list')
    def arr(name,fmt,scale=None):
        if name not in arrays:return []
        n,blob,es=arrays[name];return [tuple(v) for v in struct.iter_unpack(fmt,blob)]
    es=arrays['Coordinates'][2]
    apt_uv=None;apt_color=None
    if family in APT:
        # Model::Draw passes model +0x4c/+0x5c to ModelSetScale; shader coordinates are Q15 (ModelRenderTextureApt 0x8001d0bc).
        be=lambda p:struct.unpack('>I',span(raw,p,4))[0]
        scale=struct.unpack('>3f',span(raw,model+0x4c,12));center=struct.unpack('>3f',span(raw,model+0x5c,12))
        positions=[tuple(x/32768*scale[i]+center[i] for i,x in enumerate(p)) for p in arr('Coordinates','>3h')]
        if 'GeomName_TextureMatrix' in fields:
            mp=local[anchor+fields['GeomName_TextureMatrix'][1]];mat=struct.unpack('>16f',span(raw,mp,64))
            apt_uv=[tuple(sum(p[k]*mat[k*4+j] for k in range(3))+mat[12+j] for j in range(2)) for p in positions]
        cf='MatDiffuseColour';apt_color=bytes(span(raw,local[anchor+fields[cf][1]],4))
        uvs=apt_uv or []
    elif es==12:positions=arr('Coordinates','>3f')
    elif es==6:
        if fraction is None:raise ValueError('compressed coordinates without a position fraction')
        positions=[tuple(x/(1<<fraction) for x in p) for p in arr('Coordinates','>3h')]
    else:raise ValueError(f'unexpected coordinate size {es}')
    normals=[tuple(x/16384 for x in p) for p in arr('Normals','>3h')] if 'Normals' in arrays else []
    colors=[tuple(p) for p in arr('Colours','4B')]
    if family not in APT:uvs=arr('UVs','>2f')
    weights=[]
    if 'Weights' in arrays:
        blob=arrays['Weights'][1]
        for off in range(0,len(blob),16):
            w=blob[off:off+16];weights.append((*struct.unpack('>3f',w[:12]),w[3],w[7],w[11]))
    lookup={};verts=[];tris=[]
    src={9:positions,10:normals,11:colors,13:uvs if family not in APT else []}
    for mode,vs in stream_vertices(stream,stride,attrs):
        idx=[]
        for v in vs:
            key=v
            if key not in lookup:
                d=dict(v);lookup[key]=len(verts)
                for a in (9,10,11,13):
                    if a in d and a!=13 and d[a]>=len(src[a]):raise ValueError(f'attribute {a} index {d[a]} out of bounds')
                verts.append(dict(pos=positions[d[9]],nrm=normals[d[10]] if 10 in d and normals else None,clr=colors[d[11]] if 11 in d else None,uv=(uvs[d[13]] if 13 in d else None) if family not in APT else uvs[d[9]] if uvs else None,
                                  weight=weights[d[0]//3] if 0 in d and weights else None,pos_index=d[9]))
            idx.append(lookup[key])
        # map faces through slot indices (faces_of works on any hashable vertex value)
        for f in faces_of(mode,list(range(len(idx)))):
            a,b,c=[idx[i] for i in f]
            if verts[a]['pos_index']!=verts[b]['pos_index'] and verts[b]['pos_index']!=verts[c]['pos_index'] and verts[a]['pos_index']!=verts[c]['pos_index']:tris.append((a,c,b))
    tex={n:_tex_name(external,anchor,fields,n) for n in TEXTURE_FIELDS if n in fields}
    return dict(family=family,verts=verts,tris=tris,textures={k:v for k,v in tex.items() if v},color=apt_color)

def prim_hash(p):
    import hashlib
    h=hashlib.sha256()
    for v in p['verts']:
        h.update(struct.pack('>3f',*v['pos']))
        if v['nrm'] is not None:h.update(struct.pack('>3f',*v['nrm']))
        if v['clr'] is not None:h.update(bytes(v['clr']))
        if v['uv'] is not None:h.update(struct.pack('>2f',*v['uv']))
    for t in p['tris']:h.update(struct.pack('>3I',*t))
    return h.hexdigest()[:16]

def model_summary(data):
    raw,local,external,models=parse(data);seen=set();prims=[];info=[]
    for name,m,anchors in models:
        info.append(name)
        for a in anchors:
            if a in seen:continue
            seen.add(a);p=decode_primitive(raw,local,external,a,m)
            prims.append([hex(a),p['family'],len(p['verts']),len(p['tris']),prim_hash(p),p['textures']])
    return dict(models=info,prims=prims)
