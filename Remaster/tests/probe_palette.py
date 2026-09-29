import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core
p=core.DEFAULT_DATA/'files/data/boot/strapwarn_standard_english.gsh'
data=p.read_bytes();off=48
for offset in (len(data)-1100,off+int.from_bytes(data[off+1:off+4],'big'),off+16+640*480,off+16+640*480+16):
    print(len(data),hex(offset),data[offset:offset+96].hex(' '))
for needle in (b'\x31\x00',b'\x32\x00',b'\x70\0\0\0'):
    positions=[];start=0
    while True:
        pos=data.find(needle,start)
        if pos<0:break
        if data[pos+12:pos+16]==b'\0\0\0\0' or needle.startswith(b'\x70'):positions.append(hex(pos))
        start=pos+1
    print(needle,positions[-30:])
