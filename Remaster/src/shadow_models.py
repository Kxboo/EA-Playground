"""PlaygroundShadow geometry through shader relocations, not stride guesses.

Confirmed on the local shadow corpus: shader argument +0x18 is position
count, +0x1c the relocated float32 XYZ array; display-list opcode 7 points
to a length-bounded GX stream. Shadow rendering semantics remain unresolved.
"""
import struct,math
from containers import Elf,span
from pcode import decode


def parse(path):
    from parser import ParseResult,MeshChunk
    data=path.read_bytes();elf=Elf(data);section=elf.section('.data')
    symbols={(s['table'],s['index']):s for s in elf.symbols}
    anchors=[r for r in elf.relocations if symbols[(r['symbol_table'],r['symbol_index'])]['name']=='PlaygroundShadow']
    if not anchors:return None
    raw=elf.section_bytes(section);local={r['offset']:struct.unpack('<I',span(raw,r['offset'],4))[0] for r in elf.relocations if r['target_section']==section['index'] and symbols[(r['symbol_table'],r['symbol_index'])]['section']==section['index']}
    be=lambda off:struct.unpack('>I',span(raw,off,4))[0]
    meshes=[]
    for anchor in anchors:
        s=anchor['offset'];descriptor=local[s+4]
        program=decode(raw,descriptor,local,s)
        stream=span(raw,program['offset'],program['length'],'shadow GX stream')
        attrs=[]
        for tag,(kind,width) in sorted(program['attributes'].items()):
            if tag not in (9,11) or kind!='index':raise ValueError('Unknown shadow vertex attribute')
            attrs.append((tag,width))
        if not attrs or attrs[0][0]!=9:raise ValueError('Missing shadow positions')
        count=be(s+24);position=local[s+28];colors=be(s+32)
        if not 0<count<=1_000_000:raise ValueError('Invalid shadow position count')
        positions=list(struct.iter_unpack('>3f',span(raw,position,count*12,'shadow positions')))
        if any(not math.isfinite(x) for p in positions for x in p):raise ValueError('Nonfinite shadow position')
        indices=[];cursor=0;stride=sum(n for _,n in attrs)
        while cursor<len(stream):
            if stream[cursor]==0:cursor+=1;continue
            mode=stream[cursor]&0xf8
            if mode not in (0x80,0x90,0x98,0xa0):raise ValueError(f'Unknown shadow GX command {stream[cursor]:02x}')
            n=struct.unpack('>H',span(stream,cursor+1,2))[0];cursor+=3
            verts=span(stream,cursor,n*stride);cursor+=n*stride;v=[]
            for j in range(n):
                offset=j*stride
                for tag,width in attrs:
                    idx=int.from_bytes(verts[offset:offset+width],'big');offset+=width
                    if tag==9:
                        if idx>=count:raise ValueError('Shadow vertex index out of bounds')
                        v.append(idx)
                    elif idx>=colors:raise ValueError('Shadow color index out of bounds')
            if mode==0x98:tri=[(v[j],v[j+1],v[j+2]) if j%2==0 else (v[j+1],v[j],v[j+2]) for j in range(n-2)]
            elif mode==0xa0:tri=[(v[0],v[j],v[j+1]) for j in range(1,n-1)]
            elif mode==0x90:
                if n%3:raise ValueError('Incomplete shadow triangle list')
                tri=[tuple(v[j:j+3]) for j in range(0,n,3)]
            else:
                if n%4:raise ValueError('Incomplete shadow quad list')
                tri=[t for j in range(0,n,4) for t in ((v[j],v[j+1],v[j+2]),(v[j],v[j+2],v[j+3]))]
            indices.extend(t for t in tri if len(set(t))==3)
        normals=[[0.,0.,0.] for p in positions]
        for a,b,c in indices:
            u=[positions[b][j]-positions[a][j] for j in range(3)];v=[positions[c][j]-positions[a][j] for j in range(3)]
            cross=(u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0])
            for i in (a,b,c):normals[i]=[normals[i][j]+cross[j] for j in range(3)]
        normals=[tuple(x/(math.sqrt(sum(v*v for v in n)) or 1.) for x in n) for n in normals]
        faces=[tuple((p,p,0) for p in t) for t in indices]
        meshes.append(MeshChunk(len(meshes),descriptor,stride,'Shadow',positions,normals,[(0.,0.)],faces,warnings=['Normals generated for inspection; shadow-volume rendering and alpha semantics are not reconstructed.']))
    return ParseResult(path.stem,'PlaygroundShadow',meshes,log=['Decoded PlaygroundShadow through shader relocations and bounded GX commands.'])
