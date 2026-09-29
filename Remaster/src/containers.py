"""Bounded structural readers. Offsets refer to decompressed bytes."""
import struct


def span(data, offset, length, label='data'):
    if offset < 0 or length < 0 or offset+length > len(data):
        raise ValueError(f'{label}: span 0x{offset:x}+0x{length:x} outside 0x{len(data):x} bytes')
    return data[offset:offset+length]


def cstr(data,offset,end=None,encoding='utf-8'):
    end=len(data) if end is None else end
    span(data,offset,end-offset,'string range')
    stop=data.find(b'\0',offset,end)
    if stop<0:raise ValueError(f'Unterminated string at 0x{offset:x}')
    return data[offset:stop].decode(encoding,errors='replace')


class Elf:
    def __init__(self,data):
        self.data=data
        if span(data,0,16)[:4]!=b'\x7fELF' or data[4]!=1 or data[5] not in (1,2):
            raise ValueError('Expected ELF32 with declared byte order')
        self.endian='<' if data[5]==1 else '>'
        h=struct.unpack(self.endian+'HHIIIIIHHHHHH',span(data,16,36,'ELF header'))
        self.header=dict(zip(('type','machine','version','entry','phoff','shoff','flags','ehsize','phentsize','phnum','shentsize','shnum','shstrndx'),h))
        self.sections=[];self.symbols=[];self.relocations=[]
        if h[11]==0 or h[10]<40:raise ValueError('Unsupported empty/extended ELF section table')
        span(data,h[5],h[10]*h[11],'section table')
        for i in range(h[11]):
            values=struct.unpack(self.endian+'10I',span(data,h[5]+i*h[10],40))
            s=dict(zip(('name_index','type','flags','address','offset','size','link','info','alignment','entry_size'),values))
            s['index']=i
            if s['type']!=8:span(data,s['offset'],s['size'],f'section {i}')
            self.sections.append(s)
        if h[12]>=len(self.sections):raise ValueError('Invalid section-name table index')
        names=self.section_bytes(self.sections[h[12]])
        for s in self.sections:s['name']=cstr(names,s['name_index'])
        for section in self.sections:
            if section['type'] not in (2,11):continue
            strings=self.section_bytes(self.sections[section['link']])
            step=section['entry_size'] or 16
            if step<16 or section['size']%step:raise ValueError('Invalid symbol table stride')
            for index,offset in enumerate(range(section['offset'],section['offset']+section['size'],step)):
                name,value,size,info,other,shndx=struct.unpack(self.endian+'IIIBBH',span(data,offset,16))
                self.symbols.append(dict(index=index,table=section['index'],name=cstr(strings,name),value=value,size=size,info=info,section=shndx))
        for section in self.sections:
            if section['type'] not in (9,4):continue
            width=8 if section['type']==9 else 12
            step=section['entry_size'] or width
            if step<width or section['size']%step:raise ValueError('Invalid relocation table stride')
            for offset in range(section['offset'],section['offset']+section['size'],step):
                pos,info=struct.unpack(self.endian+'II',span(data,offset,8))
                r=dict(offset=pos,symbol_index=info>>8,type=info&255,target_section=section['info'],symbol_table=section['link'])
                if width==12:r['addend']=struct.unpack(self.endian+'i',span(data,offset+8,4))[0]
                self.relocations.append(r)

    def section_bytes(self,section):return span(self.data,section['offset'],section['size'])
    def section(self,name):return next((s for s in self.sections if s['name']==name),None)
    def summary(self):
        return dict(header=self.header,endian='little' if self.endian=='<' else 'big',sections=self.sections,symbols=self.symbols,relocations=self.relocations,
                    section_count=len(self.sections),symbol_count=len(self.symbols),relocation_count=len(self.relocations),
                    caveat='EA assets use little-endian ELF containers with big-endian payloads. ELF machine=8 does not mean the Wii payload is MIPS code.' if self.header['machine']==8 else None)


def big_entries(data,external_payload=False):
    if span(data,0,16)[:4] not in (b'BIGF',b'BIG4'):raise ValueError('Not BIGF/BIG4')
    count,end=struct.unpack_from('>II',data,8)
    if end<16 or end>len(data) or count>(end-16)//9:raise ValueError('Invalid BIG directory')
    pos=16;entries=[]
    for i in range(count):
        offset,size=struct.unpack('>II',span(data,pos,8,'BIG entry'));pos+=8
        name=cstr(data,pos,end);pos=data.index(b'\0',pos,end)+1
        if offset<end:raise ValueError('BIG entry overlaps directory')
        if not external_payload:span(data,offset,size,name)
        entries.append(dict(index=i,name=name,offset=offset,size=size))
    return entries


def u8_entries(data):
    if span(data,0,4)!=b'U\xaa8-':raise ValueError('Not Nintendo U8')
    root,header_size,data_start=struct.unpack('>III',span(data,4,12))
    span(data,root,header_size,'U8 header')
    kind,parent,count=struct.unpack('>III',span(data,root,12))
    if kind>>24!=1 or count<1 or count>header_size//12:raise ValueError('Invalid U8 root')
    names=root+count*12
    stack=[('',count,0)];entries=[]
    for i in range(1,count):
        while stack and i>=stack[-1][1]:stack.pop()
        if not stack:raise ValueError('Invalid U8 directory extent')
        word,offset,size=struct.unpack('>III',span(data,root+i*12,12))
        name=cstr(data,names+(word&0xffffff),root+header_size)
        full=stack[-1][0]+name
        if word>>24==1:
            if not i<size<=stack[-1][1] or offset!=stack[-1][2]:raise ValueError('Invalid U8 parent or subtree')
            stack.append((full+'/',size,i))
        elif word>>24==0:
            if offset<data_start:raise ValueError('U8 file precedes data')
            span(data,offset,size,full)
            entries.append(dict(index=i,name=full,offset=offset,size=size))
        else:raise ValueError('Unknown U8 node type')
    return entries
