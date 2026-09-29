"""APT geometry from executable-confirmed shader fields and model draw lists.

Evidence: research/texture-apt-disassembly.txt, model-render-disassembly.txt,
model-scale-disassembly.txt and apt-bind-texture-disassembly.txt.
UVs here are pixel coordinates. bindTexture divides by SHAPE width/height;
prepare_export performs that operation only when the exact texture is resolved.
"""
import math,re,struct,copy
from containers import Elf,span
from pcode import decode


def triangles(stream,width,count):
    result=[];cursor=0
    while cursor<len(stream):
        if stream[cursor]==0:cursor+=1;continue
        mode=stream[cursor]&0xf8
        if mode not in (0x80,0x90,0x98,0xa0):raise ValueError(f'Unsupported APT GX opcode {stream[cursor]:02x}')
        n=struct.unpack('>H',span(stream,cursor+1,2))[0];cursor+=3
        data=span(stream,cursor,n*width,'APT draw vertices');cursor+=n*width
        vertices=[int.from_bytes(data[i:i+width],'big') for i in range(0,len(data),width)]
        if any(i>=count for i in vertices):raise ValueError('APT position index out of bounds')
        if mode==0x98:faces=[(vertices[i],vertices[i+1],vertices[i+2]) if i%2==0 else (vertices[i+1],vertices[i],vertices[i+2]) for i in range(n-2)]
        elif mode==0xa0:faces=[(vertices[0],vertices[i],vertices[i+1]) for i in range(1,n-1)]
        elif mode==0x90:
            if n%3:raise ValueError('Incomplete APT triangles')
            faces=[tuple(vertices[i:i+3]) for i in range(0,n,3)]
        else:
            if n%4:raise ValueError('Incomplete APT quads')
            faces=[f for i in range(0,n,4) for f in ((vertices[i],vertices[i+1],vertices[i+2]),(vertices[i],vertices[i+2],vertices[i+3]))]
        result.extend(f for f in faces if len(set(f))==3)
    return result


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
    if not any(name in ('TextureApt','GouraudApt') for name in external.values()):return None
    be=lambda p:struct.unpack('>I',span(raw,p,4))[0]
    floats=lambda p,n:struct.unpack('>'+str(n)+'f',span(raw,p,n*4))
    models=sorted((s for s in elf.symbols if s['name'].startswith('__Model:::')),key=lambda s:s['value'])
    meshes=[];materials={};submodels=[];used=set()
    for model in models:
        m=model['value'];name=model['name'].split(':::',1)[1]
        scale=floats(m+0x4c,3);center=floats(m+0x5c,3)
        groups=be(m+0x9c)
        if not 0<groups<=4096:raise ValueError('Invalid APT geometry group count')
        cursor=local[m+0xcc]+4;indices=[]
        for group in range(groups):
            size=be(cursor);cursor+=4
            if not 0<size<=4096:raise ValueError('Invalid APT primitive count')
            for _ in range(size):
                anchor=local[cursor];cursor+=4;used.add(anchor)
                family=external.get(anchor)
                if family not in ('TextureApt','GouraudApt'):raise ValueError(f'APT shader {family} not decoded yet')
                descriptor=local[anchor+4]
                program=decode(raw,descriptor,local,anchor)
                if set(program['attributes'])!={9} or program['attributes'][9][0]!='index':raise ValueError('Unknown APT index descriptor')
                width=program['attributes'][9][1]
                stream=span(raw,program['offset'],program['length'],'APT GX display list')
                coordinate_field=56 if family=='TextureApt' else 40
                count=be(anchor+coordinate_field)
                if not 0<count<=1_000_000:raise ValueError('Invalid APT coordinate count')
                packed=list(struct.iter_unpack('>3h',span(raw,local[anchor+coordinate_field+4],count*6,'APT coordinates')))
                # Model::Draw passes +4c/+5c to ModelSetScale; shader uses Q15.
                positions=[tuple(p[i]/32768*scale[i]+center[i] for i in range(3)) for p in packed]
                matrix=floats(local[anchor+52],16) if family=='TextureApt' else None
                uv_pixels=[tuple(sum(p[k]*matrix[k*4+j] for k in range(3))+matrix[12+j] for j in range(2)) for p in positions] if matrix else [(0.,0.)]*count
                if any(not math.isfinite(x) for p in positions+uv_pixels for x in p):raise ValueError('Nonfinite APT geometry')
                color=span(raw,local[anchor+(84 if family=='TextureApt' else 68)],4,'APT diffuse color')
                texture_name=None
                if family=='TextureApt':
                    texture=external.get(anchor+44,'')
                    match=re.search(r'(?:^|;)1=([^,;]+),',texture)
                    if not match:raise ValueError('APT texture identifier missing')
                    texture_name=match[1]
                faces=triangles(stream,width,count)
                index=len(meshes);indices.append(index);materials[index]={texture_name or 'apt-color-'+color.hex()}
                mesh=MeshChunk(index,descriptor,width,family,positions,[(0.,0.,1.)],uv_pixels,[tuple((p,0,p) for p in f) for f in faces])
                mesh.apt_texture=texture_name
                mesh.apt_color=[x/255 for x in color]
                mesh.apt_uv_pixels=uv_pixels
                mesh.apt_model=name
                mesh.apt_anchor=anchor
                mesh.apt_matrix=matrix
                meshes.append(mesh)
        submodels.append(dict(index=len(submodels),name=name,mesh_indices=indices,scale=scale,center=center))
    declared={offset for offset,name in external.items() if name in ('TextureApt','GouraudApt')}
    if used!=declared:raise ValueError('APT draw lists did not cover every shader primitive')
    result=ParseResult(path.stem,'TextureApt',meshes,log=['APT Q15 positions, model scale/offset and pixel-coordinate texture matrices decoded from executable-confirmed fields.','APT timeline placement, masking and dynamic color transforms remain separate from shape geometry.'])
    result.submodels=submodels
    return result,materials


def prepare_export(result,images,log):
    """Never mutate the decoder's raw pixel UVs; exported UVs are normalized."""
    if not hasattr(result,'submodels'):return result
    result=copy.deepcopy(result)
    for mesh in result.meshes:
        # APT authoring coordinates point down; glTF/Bevy's preview points up.
        mesh.positions=[(x,-y,z) for x,y,z in mesh.positions]
        mesh.faces=[tuple(reversed(f)) for f in mesh.faces]
        if mesh.apt_texture is None:continue
        png=images.get(mesh.apt_texture)
        if png:
            width,height=struct.unpack('>II',png[16:24])
            mesh.uvs=[(u/width,v/height) for u,v in mesh.apt_uv_pixels]
        else:
            log(f'Missing APT texture {mesh.apt_texture}; pixel UVs retained without invented normalization')
    return result
