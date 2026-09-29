"""EA Attrib "Vault" databases (.vlt/.bin) as loaded by the Wii executable.

Layout recovered from playgroundz.elf (see Remaster/research/vlt-disassembly.txt):
* Attrib::Vault::Vault (0x802d9800) walks big-endian chunks [tag][u32 size incl. header]: Vers, DepN, StrN,
  DatN, ExpN, PtrN.  Vers carries a u64 key at +8; ExpN entries (count at +0xc, from +0x10) are 0x18 bytes:
  {u64 id, u64 type, u32 size, u32 offset}; DepN (count at +0xc) lists dependency files.
* Attrib::Vault::Initialize (0x802d9d00) applies PtrN (entries from +0x10... 16 bytes each):
  {u32 dest, u16 kind, u16 block, u32 a, u32 b}; kind 0 ends, 1 writes null, 2 selects the base block,
  3 writes block_base+b, 4 resolves export (a:b) of a dependency vault.
* Attrib::hash64 (0x802d86b0) is a lookup8 variant (a=b=seed, c=golden ratio) with seed 0xABCDEF0011223344 over the raw bytes;
  it is verified against the executable's own code by tools/ppc_emu.py (see tests).
"""
import struct
M=(1<<64)-1
GOLD=0x9e3779b97f4a7c13
SEED=0xABCDEF0011223344

def _mix(a,b,c):
    a=(a-b-c)&M;a^=c>>43;b=(b-c-a)&M;b^=(a<<9)&M;c=(c-a-b)&M;c^=b>>8
    a=(a-b-c)&M;a^=c>>38;b=(b-c-a)&M;b^=(a<<23)&M;c=(c-a-b)&M;c^=b>>5
    a=(a-b-c)&M;a^=c>>35;b=(b-c-a)&M;b^=(a<<49)&M;c=(c-a-b)&M;c^=b>>11
    a=(a-b-c)&M;a^=c>>12;b=(b-c-a)&M;b^=(a<<18)&M;c=(c-a-b)&M;c^=b>>22
    return a,b,c

def hash64(key:bytes,level:int=SEED)->int:
    a=b=level;c=GOLD;n=len(key);k=key;L=n  # EA variant: seed in a/b, golden ratio in c (Attrib::hash64 0x802d86b0)
    while L>=24:
        a=(a+struct.unpack('<Q',k[0:8])[0])&M;b=(b+struct.unpack('<Q',k[8:16])[0])&M;c=(c+struct.unpack('<Q',k[16:24])[0])&M
        a,b,c=_mix(a,b,c);k=k[24:];L-=24
    c=(c+n)&M
    for i in range(L-1,-1,-1):  # tail bytes
        if i>=16:c=(c+(k[i]<<(8*(i-15))))&M
        elif i>=8:b=(b+(k[i]<<(8*(i-8))))&M
        else:a=(a+(k[i]<<(8*i)))&M
    a,b,c=_mix(a,b,c);return c

def string_hash64(s:str)->int:return hash64(s.encode('latin-1')) if s else 0

class Vault:
    def __init__(self,data:bytes):
        self.data=data;self.chunks={};o=0
        while o+8<=len(data):
            tag=data[o:o+4];size=struct.unpack('>I',data[o+4:o+8])[0]
            if size<8:break
            self.chunks[tag]=(o,size);o+=size
        if b'Vers' in self.chunks:
            o,_=self.chunks[b'Vers'];self.key=struct.unpack('>Q',data[o+8:o+16])[0]
        self.exports=self._exports();self.deps=self._deps()
    def _exports(self):
        if b'ExpN' not in self.chunks:return []
        o,_=self.chunks[b'ExpN'];n=struct.unpack('>I',self.data[o+12:o+16])[0];out=[]
        for i in range(n):
            e=o+16+i*24;idh,typ,size,off=struct.unpack('>QQII',self.data[e:e+24]);out.append(dict(id=idh,type=typ,size=size,offset=off))
        return out
    def _deps(self):
        if b'DepN' not in self.chunks:return []
        o,sz=self.chunks[b'DepN'];n=struct.unpack('>I',self.data[o+12:o+16])[0]
        strings=self.data[o+16:o+sz].split(b'\0');return [s.decode('latin-1') for s in strings if s][:max(n,0)] if n else [s.decode('latin-1') for s in strings if s]
    def pointer_table(self):
        if b'PtrN' not in self.chunks:return []
        o,sz=self.chunks[b'PtrN'];out=[]
        for e in range(o+16,o+sz,16):
            dest,kind,block,a,b=struct.unpack('>IHHII',self.data[e:e+16]);out.append(dict(dest=dest,kind=kind,block=block,a=a,b=b))
        return out


class Memory:
    """Virtual address space over the vault's data blocks (block 0 = .vlt, block 1.. = dependencies)."""
    BASES=[0x10000000,0x20000000,0x30000000]
    def __init__(self,blocks):
        self.blocks=[bytearray(b) for b in blocks]
    def base(self,i):return self.BASES[i]
    def find(self,addr):
        for i,b in enumerate(self.blocks):
            if self.BASES[i]<=addr<self.BASES[i]+len(b):return i,addr-self.BASES[i]
        raise ValueError(f'address {addr:#x} outside blocks')
    def read(self,addr,n):
        i,o=self.find(addr);return bytes(self.blocks[i][o:o+n])
    def u32(self,addr):return struct.unpack('>I',self.read(addr,4))[0]
    def u16(self,addr):return struct.unpack('>H',self.read(addr,2))[0]
    def u8(self,addr):return self.read(addr,1)[0]
    def u64(self,addr):return struct.unpack('>Q',self.read(addr,8))[0]
    def cstr(self,addr):
        i,o=self.find(addr);b=self.blocks[i];e=b.index(0,o);return bytes(b[o:e]).decode('latin-1')
    def w32(self,addr,v):
        i,o=self.find(addr);self.blocks[i][o:o+4]=struct.pack('>I',v&0xFFFFFFFF)

def load_database(vlt_bytes:bytes,bin_bytes:bytes,names=None):
    """Load a Wii Attrib vault pair into a plain dict (classes, collections, types).

    `names` maps u64 hashes to strings (from string_hash64 of known identifiers); unknown keys stay hex.
    Follows Vault::Initialize (0x802d9d00) for relocation and the Database/Class/Collection policies
    (0x802d7778, 0x802d73cc, 0x802d71d4/0x802d45bc) for structure."""
    v=Vault(vlt_bytes);mem=Memory([vlt_bytes,bin_bytes]);names=names or {}
    def nm(k):return names.get(k,f'{k:#018x}')
    # --- PtrN relocations (16-byte entries starting at chunk+8) ---
    o,sz=v.chunks[b'PtrN'];base=0;end=False;relocs=0
    for e in range(o+8,o+sz,16):
        dest,kind,block,a,b=struct.unpack('>IHHII',vlt_bytes[e:e+16])
        if kind==0:break
        if kind==1:mem.w32(base+dest,0)
        elif kind==2:base=mem.base(block)
        elif kind==3:mem.w32(base+dest,mem.base(block)+b)
        elif kind==4:raise NotImplementedError('cross-vault export reference')
        relocs+=1
    out=dict(vault_key=v.key,relocations=relocs,types=[],classes={},collections={})
    B0=mem.base(0)
    exp={e['type']:[] for e in v.exports}
    for e in v.exports:exp[e['type']].append(e)
    tname={t:n for n,t in ((n,string_hash64(n)) for n in ('Attrib::DatabaseLoadData','Attrib::ClassLoadData','Attrib::CollectionLoadData'))}
    dbs=[e for e in v.exports if e['type'] in exp and e['type'] not in [string_hash64('Attrib::ClassLoadData'),string_hash64('Attrib::CollectionLoadData')]]
    ct=string_hash64('Attrib::ClassLoadData');cot=string_hash64('Attrib::CollectionLoadData')
    # --- Database: type table ---
    for e in dbs:
        p=B0+e['offset']
        n_classes,default_size,n_types,names_ptr=mem.u32(p),mem.u32(p+4),mem.u32(p+8),mem.u32(p+12)
        sizes=[mem.u32(p+0x10+4*i) for i in range(n_types)];s=names_ptr
        for i in range(n_types):
            name=mem.cstr(s);out['types'].append(dict(name=name,size=sizes[i],key=string_hash64(name)));s+=len(name)+1
        out['database']=dict(classes=n_classes,default_data_size=default_size,types=n_types)
    typ={t['key']:t for t in out['types']}
    # --- Classes ---
    for e in v.exports:
        if e['type']!=ct:continue
        p=B0+e['offset'];key=mem.u64(p);n_fields=mem.u16(p+0x26) if False else None
        # ClassLoadData fields as stored by ClassExportPolicy::Initialize (0x802d73cc)
        numcoll=mem.u32(p+8);nfields=mem.u32(p+0xc);fptr=mem.u32(p+0x10);layout=mem.u32(p+0x14)
        fields=[]
        for i in range(nfields):
            f=fptr+i*0x18
            fields.append(dict(name=nm(mem.u64(f)),name_key=mem.u64(f),type=typ.get(mem.u64(f+8),{}).get('name',f'{mem.u64(f+8):#x}'),offset=mem.u16(f+0x10),size=mem.u16(f+0x12),max_count=mem.u16(f+0x14),flags=mem.u8(f+0x16)))
        out['classes'][key]=dict(key=key,name=nm(key),layout_size=layout,collections=numcoll,fields=fields)
    # --- Collections ---
    for e in v.exports:
        if e['type']!=cot:continue
        p=B0+e['offset'];key,cls,parent=mem.u64(p),mem.u64(p+8),mem.u64(p+0x10)
        n_attr=mem.u32(p+0x20);n_types=mem.u16(p+0x26)
        tkeys=[mem.u64(p+0x30+8*i) for i in range(n_types)];nodes=p+0x30+8*n_types;attrs=[]
        for i in range(n_attr):
            q=nodes+16*i;akey=mem.u64(q);val=mem.u32(q+8);ti=mem.u16(q+0xc);fl=mem.u8(q+0xe)
            attrs.append(dict(name=nm(akey),name_key=akey,type=typ.get(tkeys[ti],{}).get('name',f'{tkeys[ti]:#x}') if ti<len(tkeys) else '?',flags=fl,raw=val))
        out['collections'][key]=dict(key=key,name=nm(key),cls=nm(cls),cls_key=cls,parent=nm(parent) if parent else None,attributes=attrs)
    out['_memory']=mem
    return out


def decode_value(db,typ:str,raw:int,flags:int):
    """Decode one attribute value (storage rules recovered from real nodes: flag 0x40 inline, 0x02 array, else pointer)."""
    mem=db['_memory'];size={t['name']:t['size'] for t in db['types']}.get(typ,4)
    def one(read,p):
        if typ=='EA::Reflection::Float':return struct.unpack('>f',read(p,4))[0]
        if typ=='EA::Reflection::Double':return struct.unpack('>d',read(p,8))[0]
        if typ in('EA::Reflection::Int32',) or typ.startswith('Enums::'):return struct.unpack('>i',read(p,4))[0]
        if typ=='EA::Reflection::UInt32':return struct.unpack('>I',read(p,4))[0]
        if typ=='EA::Reflection::Int16':return struct.unpack('>h',read(p,2))[0]
        if typ=='EA::Reflection::UInt16':return struct.unpack('>H',read(p,2))[0]
        if typ=='EA::Reflection::Int8':return struct.unpack('>b',read(p,1))[0]
        if typ in('EA::Reflection::UInt8','EA::Reflection::Char'):return read(p,1)[0]
        if typ=='EA::Reflection::Bool':return bool(read(p,1)[0])
        if typ in('EA::Reflection::UInt64','Attrib::Key'):return struct.unpack('>Q',read(p,8))[0]
        if typ=='EA::Reflection::Int64':return struct.unpack('>q',read(p,8))[0]
        if typ=='Attrib::Types::Vector2':return list(struct.unpack('>2f',read(p,8)))
        if typ=='Attrib::Types::Vector3':return list(struct.unpack('>3f',read(p,12)))
        if typ=='Attrib::Types::Vector4':return list(struct.unpack('>4f',read(p,16)))
        if typ=='Attrib::RefSpec':
            a,b,c=struct.unpack('>QQQ',read(p,24));return dict(class_key=a,collection_key=b,attribute_key=c)
        if typ=='EA::Reflection::Text':return mem.cstr(struct.unpack('>I',read(p,4))[0])
        return read(p,size).hex()
    if flags&0x40:
        # inline: value is left-justified in the 32-bit word
        word=struct.pack('>I',raw)
        if typ=='EA::Reflection::Text':return mem.cstr(raw)
        return one(lambda p,n:word[:n] if p==0 else b'',0) if size<=4 and not typ.startswith('Attrib') else raw
    if flags&0x02:
        count,cap,esz,_=struct.unpack('>HHHH',mem.read(raw,8));items=[]
        for i in range(count):
            items.append(one(mem.read,raw+8+i*esz) if typ!='EA::Reflection::Text' else mem.cstr(mem.u32(raw+8+i*esz)))
        return items
    return one(mem.read,raw)
