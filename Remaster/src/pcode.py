"""Bounded reimplementation of EAGL ProcessPCode (ELF 0x803ef24c; jump table at 0x804ed6d8, skip lengths at 0x804ed6c8).

Opcodes: 0 end, 2 array base offset, 3/5/6 allocation lists, 4 flag, 7 display list,
8/9/10 set GX INDEX8/INDEX16/DIRECT (operand = attribute ID), 11 sets position fractional bits.
Unknown operations fail explicitly rather than guessing a vertex stride.
"""
import struct
from containers import span


def decode(raw, start, relocations, end=None):
    limit=len(raw) if end is None else min(end,len(raw))
    cursor=start;result={'attributes':{},'fraction':None}
    def read(offset,size):
        if offset<start or offset+size>limit:raise ValueError('Truncated PCode program')
        return span(raw,offset,size,'PCode')
    for _ in range(128):
        op=read(cursor,1)[0]
        if op==0:
            if 'offset' not in result:raise ValueError('PCode has no display list')
            return result
        if op==2:
            # 0x803ef348: u8 slot, u32 count -> arrays[slot].base += count * element size (6 bytes total).
            slot=read(cursor+1,1)[0];count=struct.unpack('>I',read(cursor+2,4))[0]
            result.setdefault('array_offsets',{})[slot]=count;cursor+=6
        elif op==3:
            # 0x803ef388: 2-byte entries terminated by 0xff (+1 pad byte).
            cursor+=1
            for _ in range(64):
                entry=read(cursor,2)
                if entry[0]==0xff:cursor+=2;break
                cursor+=2
            else:raise ValueError('Unterminated PCode op 3 list')
        elif op==6:
            # 0x803ef550: 2-byte entries terminated by 0xff (+1 pad byte).
            cursor+=1
            for _ in range(64):
                entry=read(cursor,2)
                if entry[0]==0xff:cursor+=2;break
                cursor+=2
            else:raise ValueError('Unterminated PCode op 6 list')
        elif op==4:
            read(cursor,2);cursor+=2
        elif op==5:
            cursor+=1
            for _ in range(64):
                entry=read(cursor,3);cursor+=3
                if struct.unpack('>H',entry[:2])[0]==0:break
            else:raise ValueError('Unterminated PCode allocation list')
        elif op==7:
            read(cursor,9)
            if 'offset' in result:raise ValueError('Multiple PCode display lists not supported')
            if cursor+1 not in relocations:raise ValueError('Missing PCode display-list relocation')
            result.update(offset=relocations[cursor+1],length=struct.unpack('>I',read(cursor+5,4))[0]);cursor+=9
        elif op in (8,9,10):
            attr=read(cursor+1,1)[0]
            if attr in result['attributes']:raise ValueError('Duplicate PCode attribute')
            result['attributes'][attr]=('direct',1) if op==10 else ('index',op-7)
            cursor+=2
        elif op==11:
            fraction=read(cursor+1,1)[0]
            if fraction>31:raise ValueError('Invalid PCode position fraction')
            result['fraction']=fraction;cursor+=2
        else:raise ValueError(f'Unsupported PCode opcode {op}')
    raise ValueError('Unterminated PCode program')
