"""Generate src/sjis_table.bin from playgroundz.elf: the Wii OS Unicode -> Shift-JIS tables used by OSUTF32toSJIS
(0x80043c0c: page pointer table at 0x80452958, 256 pages of 256 big-endian u16 Shift-JIS codes, 0 = unmapped).

The output is the inverse mapping as sorted big-endian (sjis u16, unicode u16) pairs, first Unicode code point
winning for duplicate Shift-JIS codes.  Usage:  py -3.14 tools/extract_sjis_table.py
"""
import sys,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]/'Remaster'
sys.path[:0]=[str(ROOT/'src'),str(ROOT/'research/deps'),str(ROOT/'research')]
from elf_trace import Executable

PAGE_TABLE=0x80452958
e=Executable()
ptrs=struct.unpack('>256I',e.read(PAGE_TABLE,1024))
inv={}
for hi,p in enumerate(ptrs):
    if not p:continue
    page=struct.unpack('>256H',e.read(p,512))
    for lo,sj in enumerate(page):
        if sj and sj not in inv:inv[sj]=(hi<<8)|lo
out=b''.join(struct.pack('>HH',k,inv[k]) for k in sorted(inv))
dst=Path(__file__).resolve().parents[1]/'src'/'sjis_table.bin'
dst.write_bytes(out)
print(f'{len(inv)} Shift-JIS codes -> {dst}')
