"""Shader-field/GX descriptor decoding for remaining hardware-skinned models.

Field offsets come from the ELF's named shader schemas. Position precision
comes from ProcessPCode, independently per primitive. Shadow render state
and game-driven transforms remain outside this geometry decoder.
"""
import math,re,struct
from containers import Elf,span
from pcode import decode
from frontend_models import triangles

FIELDS={
    'PlaygroundCompressShadow_HWSkin':{'Weights':(16,16),'Coordinates':(32,6),'Colours':(40,4)},
    'PlaygroundCompressPlaceableShadow_HWSkin':{'Weights':(16,16),'Coordinates':(32,6),'Colours':(40,4)},
    'PlaygroundTexture_HWSkin':{'Weights':(16,16),'Coordinates':(32,6),'Colours':(40,4),'UVs':(48,8),'Texture':(56,92)},
    'PlaygroundLitPlaceable_HWSkin':{'Weights':(24,16),'Coordinates':(48,6),'Normals':(56,6),'UVs':(64,8),'Texture':(72,92)},
}


def parse(path):
    from parser import ParseResult,MeshChunk
    elf=Elf(path.read_bytes());section=elf.section('.data')
    if section is None:return None
    raw=elf.section_bytes(section);symbols={(s['table'],s['index']):s for s in elf.symbols}
    local={};external={}
    for r in elf.relocations:
        if r['target_section']!=section['index']:continue
        symbol=symbols[(r['symbol_table'],r['symbol_index'])]
        if symbol['section']==section['index']:
            local[r['offset']]=struct.unpack('<I',span(raw,r['offset'],4))[0]+symbol['value']
        elif symbol['section']==0:external[r['offset']]=symbol['name']
    anchors=sorted((s,name) for s,name in external.items() if name in FIELDS)
    if not anchors:return None
    # Avoid partially taking over a file containing another shader family.
    if any(name.startswith('Playground') and ':::' not in name and name not in FIELDS for name in external.values()):return None
    be=lambda p:struct.unpack('>I',span(raw,p,4))[0]
    meshes=[];materials={};log=[]
    for anchor,family in anchors:
        fields=FIELDS[family];arrays={}
        descriptor=local[anchor+4];program=decode(raw,descriptor,local,anchor)
        fraction=program['fraction']
        if fraction is None:raise ValueError('Missing HWSkin position precision')
        for name,(offset,stride) in fields.items():
            if name=='Texture':continue
            count=be(anchor+offset)
            if not 0<count<=1_000_000:raise ValueError(f'Invalid {family} {name} count')
            arrays[name]=span(raw,local[anchor+offset+4],count*stride,name)
        positions=[tuple(x/(1<<fraction) for x in p) for p in struct.iter_unpack('>3h',arrays['Coordinates'])]
        normals=[tuple(x/16384 for x in p) for p in struct.iter_unpack('>3h',arrays.get('Normals',b''))]
        colors=[tuple(x/255 for x in p) for p in struct.iter_unpack('4B',arrays.get('Colours',b''))]
        uvs=list(struct.iter_unpack('>2f',arrays.get('UVs',b'')))
        if any(not math.isfinite(x) for p in positions+normals+uvs for x in p):raise ValueError('Nonfinite HWSkin array')
        weights=[]
        for off in range(0,len(arrays['Weights']),16):
            w=arrays['Weights'][off:off+16];values=struct.unpack('>3f',w[:12]);bones=(w[3],w[7],w[11])
            if any(not math.isfinite(x) or not 0<=x<=1.0001 for x in values) or not .99<=sum(values)<=1.01:raise ValueError('Invalid HWSkin weight record')
            weights.append((*values,*bones))
        attrs=sorted(program['attributes'].items())
        if program['attributes'].get(0)!=('direct',1) or 9 not in program['attributes']:raise ValueError('Missing HWSkin matrix/position attributes')
        for attr,(kind,width) in attrs:
            if attr not in (0,2,9,10,11,13) or kind!=('direct' if attr<9 else 'index'):raise ValueError(f'Unsupported HWSkin attribute {attr}/{kind}')
        stride=sum(spec[1] for _,spec in attrs)
        stream=span(raw,program['offset'],program['length'],'HWSkin display list')
        cursor=0;lookup={};outpos=[];outnorm=[];outuv=[];outcolor={};outweights={};encoded=bytearray()
        arrays_by_attr={9:positions,10:normals,11:colors,13:uvs}
        while cursor<len(stream):
            opcode=stream[cursor];cursor+=1
            if opcode==0:continue
            if opcode&0xf8 not in (0x80,0x90,0x98,0xa0):raise ValueError(f'Unknown HWSkin GX opcode {opcode:02x}')
            n=struct.unpack('>H',span(stream,cursor,2))[0];cursor+=2
            vertices=span(stream,cursor,n*stride,'HWSkin vertices');cursor+=n*stride
            encoded.extend(bytes([opcode])+struct.pack('>H',n))
            for j in range(n):
                offset=j*stride;vertex={}
                for attr,(_,width) in attrs:
                    idx=int.from_bytes(vertices[offset:offset+width],'big');offset+=width;vertex[attr]=idx
                    if attr>=9 and idx>=len(arrays_by_attr[attr]):raise ValueError(f'HWSkin attribute {attr} index out of bounds')
                slot=vertex[0]
                if slot%3 or slot//3>=len(weights):raise ValueError('HWSkin matrix palette index out of bounds')
                if 2 in vertex and vertex[2]!=slot+30:raise ValueError('Unexpected HWSkin texture matrix selector')
                key=tuple(vertex.values())
                if key not in lookup:
                    index=len(outpos);lookup[key]=index
                    outpos.append(positions[vertex[9]])
                    outnorm.append(normals[vertex[10]] if 10 in vertex else (0.,0.,0.))
                    outuv.append(uvs[vertex[13]] if 13 in vertex else (0.,0.))
                    outweights[(index,index,index)]=weights[slot//3]
                    if 11 in vertex:outcolor[(index,index,index)]=colors[vertex[11]]
                encoded.extend(struct.pack('>I',lookup[key]))
        # GX front winding is opposite glTF's CCW convention. Stored normals
        # independently agree after this reversal on >99% of lit prop faces.
        indices=[(a,c,b) for a,b,c in triangles(encoded,4,len(outpos))]
        if not normals:
            outnorm=[[0.,0.,0.] for _ in outpos]
            for a,b,c in indices:
                u=[outpos[b][j]-outpos[a][j] for j in range(3)];v=[outpos[c][j]-outpos[a][j] for j in range(3)]
                cross=(u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0])
                for i in (a,b,c):outnorm[i]=[outnorm[i][j]+cross[j] for j in range(3)]
            outnorm=[tuple(x/(math.sqrt(sum(v*v for v in p)) or 1.) for x in p) for p in outnorm]
        index=len(meshes)
        mesh=MeshChunk(index,descriptor,stride,family,outpos,outnorm,outuv,[tuple((p,p,p) for p in f) for f in indices],bone_weights=weights,vertex_joints=outweights)
        if outcolor:mesh.vertex_colors=outcolor
        mesh.position_fraction=fraction;mesh.shader_anchor=anchor
        if 'Shadow' in family:mesh.warnings.append('Shadow volume geometry; game shadow compositing is not reconstructed.')
        meshes.append(mesh)
        if 'Texture' in fields:
            texture=external.get(anchor+fields['Texture'][0]+4,'');match=re.search(r'(?:^|;)1=([^,;]+),',texture)
            if not match:raise ValueError('Missing HWSkin texture identifier')
            materials[index]={match[1]}
        else:materials[index]={family}
        log.append(f'{family} at 0x{anchor:x}: Q{fraction} positions, {len(weights)} palette entries, {len(indices)} triangles.')
    return ParseResult(path.stem,'HWSkin',meshes,log=log),materials
