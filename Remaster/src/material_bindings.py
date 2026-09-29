"""Material ownership from the ELF's named shader fields, not nearby strings.

Offsets are pointer offsets relative to the GeoPrim shader anchor, extracted
from playgroundz.elf FieldNames/FieldSizes (research/shader-schemas.json).
Secondary texture stages are recorded without pretending they are diffuse.
"""
import re,struct,math
from containers import Elf,span

TEXTURES={
 'Gouraud':(), 'GouraudApt':(), 'PlaygroundAnimatedTexture':(60,),
 'PlaygroundCharFaceToonShade_HWSkin':(76,),
 'PlaygroundCompressPlaceableShadow_HWSkin':(), 'PlaygroundCompressShadow_HWSkin':(),
 'PlaygroundCompressToonShade_HWSkin':(84,), 'PlaygroundLitPlaceable_HWSkin':(76,),
 'PlaygroundShadow':(), 'PlaygroundShadow_HWSkin':(), 'PlaygroundTexture':(52,),
 'PlaygroundTextureAlphaDXT':(52,60), 'PlaygroundTextureAlphaDXTBase':(60,68,76),
 'PlaygroundTextureAlphaDXTBaseShadow':(68,76,84,92), 'PlaygroundTextureAlphaDXTShadow':(60,68,76),
 'PlaygroundTextureBakedLight':(52,), 'PlaygroundTextureBakedLightShadow':(60,68),
 'PlaygroundTextureBakedLightShadowDouble':(60,68), 'PlaygroundTextureShadow':(60,68),
 'PlaygroundTextureStaticShadow':(68,76,84), 'PlaygroundTexture_HWSkin':(60,),
 'PlaygroundToonShade':(60,), 'PlaygroundToonShade_HWSkin':(76,),
 'PlaygroundWater2':(76,84), 'TextureApt':(44,),
}
ARRAYS={
 'Gouraud':(28,None),'GouraudApt':(44,None),'PlaygroundAnimatedTexture':(28,44),
 'PlaygroundCharFaceToonShade_HWSkin':(52,68),'PlaygroundCompressPlaceableShadow_HWSkin':(36,None),
 'PlaygroundCompressShadow_HWSkin':(36,None),'PlaygroundCompressToonShade_HWSkin':(60,76),
 'PlaygroundLitPlaceable_HWSkin':(52,68),'PlaygroundShadow':(28,None),'PlaygroundShadow_HWSkin':(36,None),
 'PlaygroundTexture':(28,44),'PlaygroundTextureAlphaDXT':(28,44),'PlaygroundTextureAlphaDXTBase':(28,44),
 'PlaygroundTextureAlphaDXTBaseShadow':(36,52),'PlaygroundTextureAlphaDXTShadow':(36,52),
 'PlaygroundTextureBakedLight':(28,44),'PlaygroundTextureBakedLightShadow':(36,52),
 'PlaygroundTextureBakedLightShadowDouble':(36,52),'PlaygroundTextureShadow':(36,52),
 'PlaygroundTextureStaticShadow':(36,52),'PlaygroundTexture_HWSkin':(36,52),'PlaygroundToonShade':(36,52),
 'PlaygroundToonShade_HWSkin':(52,68),'PlaygroundWater2':(44,68),'TextureApt':(60,None),
}
COLOURS={'PlaygroundTexture':36,'PlaygroundTextureBakedLight':36,
         'PlaygroundTextureShadow':44,'PlaygroundTextureBakedLightShadowDouble':44}

def vertex_colours(mesh,raw,anchor,pointer,local):
    """The legacy parser called GX colour indices 'normal indices'.

    Keep colour seams distinct when remapping glTF normals; do not merge
    vertices just because their position and UV happen to be the same.
    """
    if mesh.source_arrays['normals']!=local[anchor+pointer]:
        raise ValueError('Shader colour pointer disagrees with decoded attribute array')
    count=struct.unpack('>I',span(raw,anchor+pointer-4,4))[0]
    colors=[tuple(x/255 for x in c) for c in struct.iter_unpack('4B',span(raw,local[anchor+pointer],count*4))]
    accum=[[0.,0.,0.] for _ in mesh.positions]
    for face in mesh.faces:
        a,b,c=[mesh.positions[v[0]] for v in face]
        u=[b[j]-a[j] for j in range(3)];v=[c[j]-a[j] for j in range(3)]
        cross=(u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0])
        for pi,ci,ui in face:
            if not 0<=ci<count:raise ValueError('Shader colour index out of bounds')
            accum[pi]=[accum[pi][j]+cross[j] for j in range(3)]
    normals=[tuple(x/(math.sqrt(sum(v*v for v in p)) or 1.) for x in p) for p in accum]
    keys={};outnorm=[];outcolors={};faces=[]
    for face in mesh.faces:
        converted=[]
        for key in face:
            pi,ci,ui=key
            if key not in keys:
                mapped=(pi,len(outnorm),ui);keys[key]=mapped
                outnorm.append(normals[pi]);outcolors[mapped]=colors[ci]
            converted.append(keys[key])
        faces.append(tuple(converted))
    mesh.normals=outnorm;mesh.faces=faces;mesh.vertex_colors=outcolors
    mesh.material_unlit=True

def sampler(mesh,png):
    """TAR properties 22/23 are GX S/T wrap; NPOT axes force clamp.

    Confirmed in RuntimeAllocTARConstructor at 0x803e91b0, 0x803e945c,
    0x803e9528. Keep filter defaults until their runtime source is decoded.
    """
    refs=getattr(mesh,'material_bindings',[])
    if not refs:return {}
    properties=dict(re.findall(r'(?:^|;)(\d+)=([^;]+)',refs[0]['symbol']))
    dimensions=struct.unpack('>II',png[16:24]);out={}
    for prop,axis,size in zip(('22','23'),('wrapS','wrapT'),dimensions):
        if prop not in properties:continue
        value=int(properties[prop],16 if properties[prop].lower().startswith('0x') else 10)
        if value not in (0,1,2):raise ValueError(f'Unknown TAR wrap mode {value}')
        if size&(size-1):value=0
        out[axis]={0:33071,1:10497,2:33648}[value]
    return out

def bind(path,result,materials):
    elf=Elf(path.read_bytes());section=elf.section('.data')
    raw=elf.section_bytes(section);symbols={(s['table'],s['index']):s for s in elf.symbols}
    local={};external={}
    for r in elf.relocations:
        if r['target_section']!=section['index']:continue
        symbol=symbols[r['symbol_table'],r['symbol_index']]
        if symbol['section']==section['index']:
            local[r['offset']]=struct.unpack('<I',span(raw,r['offset'],4))[0]+symbol['value']
        elif symbol['section']==0:external[r['offset']]=symbol['name']
    anchors=[(a,f,local.get(a+4)) for a,f in external.items() if f in TEXTURES]
    out=dict(materials)
    for mesh in result.meshes:
        if not mesh.ok:continue
        candidates=[(a,f) for a,f,p in anchors if p is not None and p<=mesh.desc_offset<a]
        if not candidates and hasattr(mesh,'source_arrays'):
            source=mesh.source_arrays
            candidates=[(a,f) for a,f,p in anchors if ARRAYS[f][1] is not None and local.get(a+ARRAYS[f][0])==source['positions'] and local.get(a+ARRAYS[f][1])==source['uvs']]
            if len(candidates)>1:
                # Display-list relocation provides a third independent identity
                # when multiple primitives share position and UV arrays.
                candidates=[(a,f) for a,f in candidates if any(p<=off-1<a and raw[off-1]==7 and target==source['display_list'] for aa,ff,p in anchors if aa==a and p is not None for off,target in local.items())]
        if len(candidates)!=1:
            mesh.warnings.append('Material shader ownership unresolved; legacy material assignment retained.')
            continue
        anchor,family=candidates[0];refs=[]
        if family=='PlaygroundToonShade' and hasattr(mesh,'source_arrays'):
            # ModelRenderPlaygroundToonShade at 0x8001ba2c sets GX S16 Q14.
            # The generic legacy path treated these bytes as signed int8.
            pointer=local[anchor+44]
            if pointer!=mesh.source_arrays['normals']:raise ValueError('Shader normal pointer disagrees with decoded attribute array')
            count=struct.unpack('>I',span(raw,anchor+40,4))[0]
            normals=[tuple(x/16384 for x in row) for row in struct.iter_unpack('>3h',span(raw,pointer,count*6))]
            if any(ni>=count for face in mesh.faces for pi,ni,ui in face):raise ValueError('Shader normal index out of bounds')
            mesh.normals=normals
        if family in COLOURS and hasattr(mesh,'source_arrays'):
            vertex_colours(mesh,raw,anchor,COLOURS[family],local)
        if family=='PlaygroundTexture_HWSkin' and hasattr(mesh,'vertex_colors'):
            mesh.material_unlit=True
        for offset in TEXTURES[family]:
            symbol=external.get(anchor+offset,'')
            match=re.search(r'(?:^|;)1=([^,;]+),',symbol)
            refs.append(dict(pointer_offset=offset,symbol=symbol,name=match[1] if match else None))
        mesh.shader_family=family;mesh.material_bindings=refs
        mesh.textureless=not refs
        if not refs:continue
        if refs[0]['name'] is None:
            mesh.warnings.append('Material base texture binding unresolved; no diffuse texture selected.')
            out[mesh.index]=set()
        else:out[mesh.index]={refs[0]['name']}
    return result,out
