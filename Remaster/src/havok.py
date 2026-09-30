"""Havok 4.6 packfile (.hkx) deserializer driven by the reflection tables inside playgroundz.elf.

The executable links Havok's `hk*Class` reflection objects: `hkClass` (36 bytes: name, parent, objectSize,
numImplementedInterfaces, declaredEnums, numDeclaredEnums, declaredMembers, numDeclaredMembers, defaults)
and `hkClassMember` (20 bytes: name, class, enum, type u8, subtype u8, cArraySize u16, flags u16, offset u16).
Sizes were checked against symbol sizes (e.g. hkxAttributeClass = 36, its two members = 40).

Packfile layout (big-endian, Havok-4.6.0-r1): header 0x40 bytes, then 0x30-byte section headers
{name[19], 0xFF pad, absoluteDataStart, local, global, virtual, exports, imports, end}.  Fixups:
local (src,dst) same-section pointers, global (src,section,dst), virtual (src,section,classNameOffset) = object starts.
"""
import struct,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'research'),str(ROOT/'research/deps'),str(ROOT/'src')]

T=dict(VOID=0,BOOL=1,CHAR=2,INT8=3,UINT8=4,INT16=5,UINT16=6,INT32=7,UINT32=8,INT64=9,UINT64=10,REAL=11,VECTOR4=12,QUATERNION=13,MATRIX3=14,ROTATION=15,QSTRANSFORM=16,MATRIX4=17,TRANSFORM=18,ZERO=19,POINTER=20,FUNCTIONPOINTER=21,ARRAY=22,INPLACEARRAY=23,ENUM=24,STRUCT=25,SIMPLEARRAY=26,HOMOGENEOUSARRAY=27,VARIANT=28,CSTRING=29,ULONG=30,FLAGS=31,HALF=32)
TN={v:k for k,v in T.items()}
SCALAR={T['BOOL']:('>B',1),T['CHAR']:('>b',1),T['INT8']:('>b',1),T['UINT8']:('>B',1),T['INT16']:('>h',2),T['UINT16']:('>H',2),T['INT32']:('>i',4),T['UINT32']:('>I',4),
        T['INT64']:('>q',8),T['UINT64']:('>Q',8),T['REAL']:('>f',4),T['ULONG']:('>I',4),T['HALF']:('>H',2)}
FIXED_FLOATS={T['VECTOR4']:4,T['QUATERNION']:4,T['MATRIX3']:12,T['ROTATION']:12,T['QSTRANSFORM']:12,T['MATRIX4']:16,T['TRANSFORM']:16}

class Reflection:
    """hkClass tables recovered from the ELF.

    The `hk*Class` objects are constructed at startup: each `__sinit_` initialiser for an `hk<Name>Class` calls
    `hkClass::hkClass(name, parent, objectSize, interfaces, nInterfaces, enums, nEnums, members, nMembers, defaults)`
    (0x801e25c8).  We run those initialisers on the interpreter in tools/ppc_emu.py, capture the arguments, and read the
    static member tables (20-byte hkClassMember records) from .rodata."""
    CTOR='__ct__7hkClassFPCcPC7hkClassiPPC7hkClassiPC11hkClassEnumiPC13hkClassMemberiPCv'
    def __init__(self,exe=None):
        sys.path.insert(0,str(ROOT.parent/'_bevy'/'tools'))
        from elf_trace import Executable
        import ppc_emu
        self.exe=exe or Executable();self.by_addr={};self.classes={}
        emu=ppc_emu.Emu(self.exe);self.emu=emu;captured=[]
        def hook(em):
            r=em.r;sp=r[1]
            captured.append(dict(this=r[3],name=r[4],parent=r[5],size=r[6],members=struct.unpack('>I',em.rd(sp+8,4))[0],nmembers=struct.unpack('>I',em.rd(sp+12,4))[0]))
        emu.hooks[self.exe.symbols[self.CTOR]['value']]=hook
        # Reflection does not depend on these helpers; stub them so class initialisers run to their hkClass constructor.
        for hn,hs in self.exe.symbols.items():
            if hs['value'] and (hn.startswith('getVtable') or hn.startswith('finishLoadedObject') or hn.startswith('cleanupLoadedObject')):
                emu.hooks[hs['value']]=lambda em:em.r.__setitem__(3,0)
        import re
        for n,sym in self.exe.symbols.items():
            if n.startswith('__sinit_') and n.endswith('Class_cpp') and 'hk' in n and sym['size']:
                try:emu.call(n,[])
                except Exception as ex:pass  # non-class initialisers are skipped
        for c in captured:
            name=self.cstr(c['name']);self.by_addr[c['this']]=name
            self.classes[name]=dict(name=name,parent_addr=c['parent'],size=c['size'],maddr=c['members'],nmembers=c['nmembers'],members=None)
    def cstr(self,a):
        out=bytearray()
        while True:
            c=bytes(self.exe.read(a+len(out),1))
            if c==bytes(1):break
            out+=c
        return out.decode('latin-1')
    def u32(self,a):return struct.unpack('>I',bytes(self.exe.read(a,4)))[0]
    def get(self,name):
        c=self.classes.get(name)
        if c is None:return None
        if c['members'] is None:
            c['parent_name']=self.by_addr.get(c['parent_addr']);c['members']=[]
            for i in range(c['nmembers']):
                m=c['maddr']+20*i;mn,mc,me,ty,sub,csz,fl,off=struct.unpack('>3IBBHHH',bytes(self.exe.read(m,20)))
                c['members'].append(dict(name=self.cstr(mn),cls=self.by_addr.get(mc),enum=bool(me),type=ty,subtype=sub,csize=csz,flags=fl,offset=off))
        return c
    def all_members(self,name):
        c=self.get(name)
        if c is None:return []
        base=self.all_members(c['parent_name']) if c.get('parent_name') else []
        return base+c['members']

class Packfile:
    def __init__(self,data:bytes,refl:Reflection):
        self.d=data;self.refl=refl
        if struct.unpack('>2I',data[:8])!=(0x57e0e057,0x10c0c010):raise ValueError('not a Havok packfile')
        self.nsec=struct.unpack('>i',data[20:24])[0];self.sections=[]
        for i in range(self.nsec):
            o=0x40+i*0x30;name=data[o:o+19].split(b'\0')[0].decode()
            start,loc,glob,virt,exp,imp,end=struct.unpack('>7I',data[o+20:o+48])
            self.sections.append(dict(name=name,start=start,local=loc,global_=glob,virtual=virt,exports=exp,imports=imp,end=end))
        self.local={};self.glob={};self.virt={}
        for si,s in enumerate(self.sections):
            b=s['start']
            for o in range(b+s['local'],b+s['global_'],8):
                if o+8>len(data):break
                src,dst=struct.unpack('>ii',data[o:o+8])
                if src==-1:break
                self.local[(si,src)]=dst
            for o in range(b+s['global_'],b+s['virtual'],12):
                if o+12>len(data):break
                src,ts,dst=struct.unpack('>iii',data[o:o+12])
                if src==-1:break
                self.glob[(si,src)]=(ts,dst)
            for o in range(b+s['virtual'],b+s['exports'],12):
                if o+12>len(data):break
                src,ts,nameoff=struct.unpack('>iii',data[o:o+12])
                if src==-1:break
                cn=data[self.sections[ts]['start']+nameoff:].split(b'\0')[0].decode()
                self.virt[(si,src)]=cn
    def sec(self,name):
        for i,s in enumerate(self.sections):
            if s['name']==name:return i
    def objects(self):
        """All objects: {(section,offset): class name} sorted by offset."""
        return dict(sorted(self.virt.items()))
    def abs(self,si,off):return self.sections[si]['start']+off
    # --- value decoding ---
    def read(self,si,off,n):a=self.abs(si,off);return self.d[a:a+n]
    def ptr(self,si,off):
        if (si,off) in self.local:return (si,self.local[(si,off)])
        if (si,off) in self.glob:t,dst=self.glob[(si,off)];return (t,dst)
        return None
    def decode_object(self,si,off,cls,depth=0):
        out={}
        for m in self.refl.all_members(cls):
            out[m['name']]=self.decode_member(si,off+m['offset'],m,depth)
        return out
    def decode_member(self,si,off,m,depth):
        ty=m['type'];cs=m['csize']
        def one(o):return self.decode_type(si,o,ty,m,depth)
        if cs and ty not in(T['ARRAY'],T['INPLACEARRAY']):
            width=self.width(ty,m);return [one(off+i*width) for i in range(cs)]
        return one(off)
    def width(self,ty,m):
        if ty in SCALAR:return SCALAR[ty][1]
        if ty in FIXED_FLOATS:return 4*FIXED_FLOATS[ty] if ty!=T['VECTOR4'] and ty!=T['QUATERNION'] else 16
        if ty==T['POINTER'] or ty==T['CSTRING'] or ty==T['FUNCTIONPOINTER']:return 4
        if ty==T['STRUCT']:c=self.refl.get(m['cls']);return c['size'] if c else 0
        if ty==T['ENUM']:return SCALAR.get(m['subtype'],('>I',4))[1]
        if ty in(T['ARRAY'],T['SIMPLEARRAY'],T['HOMOGENEOUSARRAY']):return 12
        return 4
    def decode_type(self,si,o,ty,m,depth):
        if ty in SCALAR:
            f,n=SCALAR[ty];v=struct.unpack(f,self.read(si,o,n))[0];return bool(v) if ty==T['BOOL'] else v
        if ty in FIXED_FLOATS:return list(struct.unpack('>%df'%FIXED_FLOATS[ty],self.read(si,o,4*FIXED_FLOATS[ty])))
        if ty==T['ENUM']:f,n=SCALAR.get(m['subtype'],('>I',4));return struct.unpack(f,self.read(si,o,n))[0]
        if ty==T['ZERO'] or ty==T['VOID']:return None
        if ty in(T['POINTER'],T['CSTRING']):
            t=self.ptr(si,o)
            if t is None:return None
            if ty==T['CSTRING']:a=self.abs(*t);return self.d[a:self.d.index(0,a)].decode('latin-1')
            return {'$ref':t,'class':self.virt.get(t)}
        if ty==T['STRUCT']:
            if depth>6 or not m['cls']:return {'$struct':m['cls']}
            return self.decode_object(si,o,m['cls'],depth+1)
        if ty in(T['ARRAY'],T['SIMPLEARRAY'],T['HOMOGENEOUSARRAY']):
            n=struct.unpack('>i',self.read(si,o+4,4))[0];t=self.ptr(si,o)
            return {'$array':True,'count':n,'data':t,'elem':TN.get(m['subtype']),'elem_class':m['cls']}
        return {'$unsupported':TN.get(ty,ty)}
    def array_elements(self,arr,limit=None):
        """Decode array elements for scalar/vector/struct element types."""
        if not arr or not arr.get('data'):return []
        si,off=arr['data'];et=T.get(arr['elem']);n=arr['count'] if limit is None else min(limit,arr['count']);out=[]
        m=dict(cls=arr['elem_class'],type=et,subtype=0,csize=0)
        w=self.width(et,m) if et is not None else 0
        if et==T['POINTER']:w=4
        for i in range(n):out.append(self.decode_type(si,off+i*w,et,m,1))
        return out
