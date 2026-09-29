"""Evidence-led format inspection, with explicit unknown/partial status.

This module never equates a recognized signature or successful parse with a
complete semantic decode. Text cells and unrecognized lines stay lossless.
"""
import csv
import io
import re
import struct
from collections import Counter
from pathlib import Path
from containers import Elf,span,cstr,big_entries,u8_entries


def text_decode(data):
    if data.startswith((b'\xff\xfe',b'\xfe\xff')):return data.decode('utf-16'),'utf-16 BOM'
    if data.startswith(b'\xef\xbb\xbf'):return data.decode('utf-8-sig'),'utf-8 BOM'
    if b'\0' in data:raise ValueError('Binary NUL in text without a Unicode BOM')
    try:return data.decode('utf-8'),'utf-8'
    except UnicodeDecodeError:return data.decode('cp1252'),'cp1252 fallback (unconfirmed)'


def csv_table(data):
    text,encoding=text_decode(data)
    # The localized home CSVs are quoted TSVs. Inspect separators outside
    # quotes using csv itself rather than splitting embedded multiline cells.
    choices=[]
    for delimiter in (',','\t',';'):
        try:rows=list(csv.reader(io.StringIO(text,newline=''),delimiter=delimiter,strict=True))
        except csv.Error:continue
        widths=Counter(len(row) for row in rows if row)
        populated=[row for row in rows if row]
        header_width=len(populated[0]) if populated else 0
        score=(header_width>1, sum(len(row)==header_width for row in populated)/max(1,len(populated)),header_width,max(widths,default=0))
        choices.append((score,delimiter,rows,widths))
    if not choices:raise ValueError('No valid comma, tab, or semicolon CSV interpretation')
    _,delimiter,rows,widths=max(choices,key=lambda x:x[0])
    header=rows[0] if rows else []
    schema='animation_state_table' if 'ANIM_ASSET' in header else 'tabular_text'
    if {'MINIMUM_X','MINIMUM_Z','RADIUS'}<=set(header):schema='world_bounds'
    links=[]
    if schema=='animation_state_table':
        idx=header.index('ANIM_ASSET')
        links=[{'row':i+1,'asset':row[idx]} for i,row in enumerate(rows[1:],1) if len(row)>idx and row[idx]]
    return dict(encoding=encoding,delimiter=delimiter,rows=rows,row_count=len(rows),column_counts=dict(widths),schema=schema,asset_references=links,
                caveats=['Cells retained as strings; header roles are identified only for known schemas. Ragged rows are retained.'])


def lion_text(data):
    text,encoding=text_decode(data)
    root={'tag':'document','children':[],'properties':[]};stack=[root];pending=None;unknown=[]
    for i,line in enumerate(text.splitlines(),1):
        value=line.strip()
        if not value:continue
        match=re.fullmatch(r'<([\w:]+)(.*?)>',value)
        if match:
            node={'tag':match[1],'attributes':dict(re.findall(r'(\w+)\s*=\s*"([^"]*)"',match[2])),'line':i,'children':[],'properties':[]}
            stack[-1]['children'].append(node);pending=node
        elif value=='{':
            if pending is None:raise ValueError(f'Line {i}: block without a tag')
            stack.append(pending);pending=None
        elif value=='}':
            if len(stack)==1:raise ValueError(f'Line {i}: unexpected closing brace')
            stack.pop()
        elif '=' in value:
            key,val=value.split('=',1)
            stack[-1]['properties'].append({'key':key.strip(),'value':val.strip(),'line':i})
        else:unknown.append({'line':i,'text':line})
    if len(stack)!=1:raise ValueError('Unclosed LION block')
    return dict(encoding=encoding,tree=root,unparsed_lines=unknown,caveats=['Property names and raw values parsed; particle simulation semantics are not implemented.'])


def localization(data):
    if span(data,0,4)!=b'LOCH':raise ValueError('Not LOCH')
    header_size=struct.unpack_from('<I',data,4)[0]
    chunk=header_size
    if span(data,chunk,4)!=b'LOCL':raise ValueError('Missing LOCL chunk')
    size,unknown,count=struct.unpack('<III',span(data,chunk+4,12))
    end=chunk+size;span(data,chunk,size,'LOCL chunk')
    table=chunk+16;span(data,table,count*4,'localization offsets')
    offsets=struct.unpack('<'+'I'*count,span(data,table,count*4))
    strings=[]
    for index,rel in enumerate(offsets):
        off=chunk+rel
        if off<table+count*4 or off>=end:raise ValueError('String pointer outside LOCL string pool')
        stop=off
        while stop+2<=end and data[stop:stop+2]!=b'\0\0':stop+=2
        if stop+2>end:raise ValueError('Unterminated localization string')
        value=data[off:stop].decode('utf-16-le')
        strings.append(dict(index=index,offset=off,text=value))
    return dict(encoding='utf-16-le',count=count,strings=strings,header_hex=data[:header_size].hex(),unknown_word=unknown,
                caveats=['String indices decoded; IDs require the matching string.idx. LOCH control fields remain partially understood.'])


def locale_index(data):
    version,count=struct.unpack('>II',span(data,0,8))
    if version!=0x20040519 or len(data)!=8+8*count:raise ValueError('Not the observed localization IDX layout')
    records=[dict(hash=f'{key:08x}',string_index=index) for key,index in struct.iter_unpack('>II',data[8:])]
    return dict(version=f'{version:08x}',count=count,records=records,caveats=['Hash values and indices decoded; original string identifiers/hash algorithm not recovered.'])


def hkx(data):
    span(data,0,64,'Havok header')
    endian='<' if data[17] else '>'
    version=struct.unpack_from(endian+'I',data,12)[0]
    count=struct.unpack_from(endian+'I',data,20)[0]
    if version!=4:raise ValueError(f'Havok packfile version {version} not implemented')
    span(data,64,count*48,'Havok sections')
    sections=[]
    for i in range(count):
        off=64+i*48
        name=cstr(data,off,off+19)
        absolute,*bounds=struct.unpack(endian+'7I',span(data,off+20,28))
        if bounds!=sorted(bounds):raise ValueError('Havok section regions not monotonic')
        span(data,absolute,bounds[-1],name)
        s=dict(name=name,offset=absolute,regions=dict(zip(('local','global','virtual','exports','imports','end'),bounds)))
        s['fixups']={}
        for j,(label,step) in enumerate((('local',8),('global',12),('virtual',12))):
            rows=[]
            for p in range(absolute+bounds[j],absolute+bounds[j+1]-step+1,step):
                values=struct.unpack(endian+('I'*(step//4)),span(data,p,step))
                if values[0]==0xffffffff:continue
                rows.append(values)
            s['fixups'][label]=rows
        sections.append(s)
    return dict(version=version,layout_rules=list(data[16:20]),sdk=cstr(data,40,56),sections=sections,
                caveats=['Packfile sections and fixup records decoded. Collision shapes, class members and constraint semantics are not yet decoded.'])


TPL_FORMATS={0:('I4',8,8,32),1:('I8',8,4,32),2:('IA4',8,4,32),3:('IA8',4,4,32),4:('RGB565',4,4,32),5:('RGB5A3',4,4,32),6:('RGBA8',4,4,64),8:('C4',8,8,32),9:('C8',8,4,32),10:('C14X2',4,4,32),14:('CMPR',8,8,32)}


def tpl(data):
    magic,count,table=struct.unpack('>III',span(data,0,12))
    if magic!=0x20af30:raise ValueError('Not TPL')
    span(data,table,count*8,'TPL directory');entries=[]
    for i in range(count):
        image,palette=struct.unpack('>II',span(data,table+i*8,8))
        h,w,fmt,off=struct.unpack('>HHII',span(data,image,12))
        if fmt not in TPL_FORMATS:raise ValueError(f'Unknown GX texture format {fmt}')
        label,bw,bh,bs=TPL_FORMATS[fmt]
        size=((w+bw-1)//bw)*((h+bh-1)//bh)*bs
        span(data,off,size,'TPL base image')
        entry=dict(index=i,width=w,height=h,format=fmt,format_name=label,offset=off,size=size,header_offset=image)
        if palette:
            n,unpacked,pad,pfmt,poff=struct.unpack('>HBBII',span(data,palette,12))
            span(data,poff,n*2,'TPL palette')
            entry['palette']=dict(count=n,format=pfmt,offset=poff)
        entries.append(entry)
    return dict(entries=entries,caveats=['Base-level images supported; mip levels and sampler flags are not reconstructed.'])


def tpl_rgba(data,entry):
    import core
    g=core.gsh_parser
    w,h,fmt=entry['width'],entry['height'],entry['format']
    raw=span(data,entry['offset'],entry['size'])
    if fmt==14:return g.decode_cmpr(raw,w,h),w,h
    if fmt==6:return g.decode_rgba8(raw,w,h),w,h
    if fmt==5:return g.decode_rgb5a3(raw,w,h),w,h
    if fmt==4:return g._detile_gx_16bpp(raw,w,h,g._rgb565_to_rgba8888),w,h
    palette=[]
    if fmt in (8,9,10):
        p=entry.get('palette')
        if not p:raise ValueError('Indexed TPL has no palette')
        for (value,) in struct.iter_unpack('>H',span(data,p['offset'],p['count']*2)):
            if p['format']==0:color=(value&255,)*3+(value>>8,)
            elif p['format']==1:color=g._rgb565_to_rgba8888(value)
            elif p['format']==2:color=g._rgb5a3_to_rgba8888(value)
            else:raise ValueError('Unknown TPL palette format')
            palette.append(color)
    _,bw,bh,bs=TPL_FORMATS[fmt]
    result=bytearray(w*h*4);pos=0
    for by in range(0,h,bh):
        for bx in range(0,w,bw):
            tile=raw[pos:pos+bs];pos+=bs
            for y in range(bh):
                for x in range(bw):
                    index=y*bw+x
                    if fmt in (0,8):v=(tile[index//2]>>(4 if index%2==0 else 0))&15
                    elif fmt in (1,2,9):v=tile[index]
                    else:v=struct.unpack_from('>H',tile,index*2)[0]
                    if fmt==0:color=(v*17,)*3+(255,)
                    elif fmt==1:color=(v,)*3+(255,)
                    elif fmt==2:color=((v&15)*17,)*3+((v>>4)*17,)
                    elif fmt==3:color=(v&255,)*3+(v>>8,)
                    else:
                        idx=v&0x3fff if fmt==10 else v
                        if idx>=len(palette):raise ValueError('TPL palette index out of bounds')
                        color=palette[idx]
                    if bx+x<w and by+y<h:
                        out=((by+y)*w+bx+x)*4;result[out:out+4]=bytes(color)
    return bytes(result),w,h


KNOWN_MAGICS={b'ABKC':'EA audio bank',b'BNKb':'EA audio bank',b'SCHl':'EA audio stream',b'MADk':'EA video stream',b'MVhd':'EA video stream',b'FntG':'EA font',b'RSTM':'Nintendo stream',b'RBNK':'Nintendo sound bank',b'RSAR':'Nintendo sound archive',b'RFNT':'Nintendo font',b'RLYT':'Nintendo layout',b'RLAN':'Nintendo layout animation',b'Vers':'EA VLT database',b'MOIR':'EA audio metadata'}


def nintendo_blocks(data):
    span(data,0,16,'Nintendo resource header')
    if data[4:6] not in (b'\xfe\xff',b'\xff\xfe'):raise ValueError('Invalid resource byte-order mark')
    endian='>' if data[4:6]==b'\xfe\xff' else '<'
    version,size,start,count=struct.unpack_from(endian+'HIHH',data,6)
    if size!=len(data) or start<16:raise ValueError('Resource file/header length mismatch')
    blocks=[];pos=start
    for i in range(count):
        tag,length=struct.unpack(endian+'4sI',span(data,pos,8))
        if length<8:raise ValueError('Resource block shorter than its header')
        block=span(data,pos,length)
        item=dict(index=i,kind=tag.decode('ascii'),offset=pos,size=length)
        if tag in (b'txl1',b'fnl1'):
            n=struct.unpack(endian+'H',span(block,8,2))[0]
            span(block,12,n*8,'resource name offsets')
            item['names']=[cstr(block,12+struct.unpack_from(endian+'I',block,12+j*8)[0]) for j in range(n)]
        if tag==b'lyt1':
            item['centered']=bool(span(block,8,1)[0])
            item['width'],item['height']=struct.unpack(endian+'ff',span(block,12,8))
        blocks.append(item);pos+=length
    return dict(version=version,endian='big' if endian=='>' else 'little',blocks=blocks,trailing_bytes=len(data)-pos,
                caveats=['Block boundaries and layout resource names decoded. Pane transforms, materials, font glyphs and animation keyframes remain undecoded.'])


def dol(data):
    words=struct.unpack('>57I',span(data,0,228,'DOL header'))
    sections=[]
    for i in range(18):
        off,address,size=words[i],words[18+i],words[36+i]
        if not size:continue
        if off<256:raise ValueError('DOL section overlaps header')
        span(data,off,size,'DOL section')
        sections.append(dict(kind='text' if i<7 else 'data',index=i if i<7 else i-7,offset=off,address=address,size=size))
    if not sections:raise ValueError('DOL has no loadable sections')
    return dict(sections=sections,bss_address=words[54],bss_size=words[55],entrypoint=words[56],
                caveats=['Executable section ranges decoded; PowerPC instructions and game code semantics are not decompiled.'])


def inspect_bytes(data,name='',full=True):
    suffix=Path(name).suffix.lower();head=data[:4]
    result=dict(format='unknown binary',status='unknown',caveats=['Payload semantics not decoded.'],signature_hex=data[:32].hex())
    if head in (b'BIGF',b'BIG4') and suffix=='.bh':
        result=dict(format='EA BIG external directory',status='structural',entries=big_entries(data,external_payload=True),caveats=['Directory-only .bh: offsets refer to the companion archive; no payload bytes in this file.'])
    elif head in (b'BIGF',b'BIG4'):
        result=dict(format='EA BIG archive',status='structural',entries=big_entries(data),caveats=['Container directory decoded; each child needs its own decoder.'])
    elif head==b'U\xaa8-':result=dict(format='Nintendo U8 archive',status='structural',entries=u8_entries(data),caveats=['Container directory decoded; child semantics separate.'])
    elif head==b'\x7fELF':result=dict(format='ELF32 / EA relocatable' if data[5]==1 else 'ELF32 executable',status='structural',**Elf(data).summary())
    elif head==b'\x00 \xaf0':result=dict(format='Nintendo TPL',status='partial',**tpl(data))
    elif head==b'LOCH':result=dict(format='EA localization',status='partial',**localization(data))
    elif head==bytes.fromhex('20040519'):result=dict(format='EA localization index',status='partial',**locale_index(data))
    elif data[:8]==bytes.fromhex('57e0e05710c0c010'):result=dict(format='Havok packfile',status='structural',**hkx(data))
    elif suffix=='.csv':result=dict(format='Delimited table',status='decoded_text',**csv_table(data))
    elif data.lstrip().startswith(b'<LION_'):result=dict(format='LION effect tree',status='structural',**lion_text(data))
    elif head==b'SHPG':result=dict(format='EA GSH texture archive',status='partial',caveats=['Recovered image decoder available; record semantics and visual correctness need per-image checks.'])
    elif data.startswith(b'Apt Data:'):result=dict(format='EA APT UI program',status='recognized',version_prefix=data[:16].hex(),caveats=['UI program signature identified; timeline, shapes and script bytecode not decoded.'])
    elif data.startswith(b'Apt constant file'):result=dict(format='EA APT constants',status='recognized',caveats=['Companion constant-pool signature identified; typed records not yet decoded.'])
    elif head in (b'RLYT',b'RLAN',b'RFNT'):result=dict(format=KNOWN_MAGICS[head],status='structural',**nintendo_blocks(data))
    elif suffix=='.dol':result=dict(format='Nintendo DOL executable',status='structural',**dol(data))
    elif head in KNOWN_MAGICS:result=dict(format=KNOWN_MAGICS[head],status='recognized',caveats=['Signature identified; payload not semantically decoded.'])
    elif suffix in ('.txt','.ini','.bts','.con','.mkr','.cpt') or (data and b'\0' not in data[:4096]):
        try:
            text,encoding=text_decode(data)
            if sum(c.isprintable() or c in '\r\n\t' for c in text)/max(1,len(text))>.95:
                result=dict(format='Text',status='decoded_text',encoding=encoding,text=text,caveats=['Raw text decoded; schema/field meanings are not automatically established.'])
        except (UnicodeError,ValueError):pass
    result['size']=len(data)
    if not full:
        result={k:v for k,v in result.items() if k not in ('text','tree','strings','rows','records','relocations','symbols','sections','entries')}
    return result
