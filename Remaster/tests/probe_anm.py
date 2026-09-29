import sys,struct
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core
from containers import Elf
from dataclasses import asdict
p=core.DEFAULT_OLD/'placeables/rc_trackcar'
for name in ('rc_track_car_skel.ske','rc_track_car_anims.anm'):
    data=(p/name).read_bytes();elf=Elf(data)
    print(name,elf.symbols)
    sec=elf.section('.data');payload=elf.section_bytes(sec)
    for i in range(0,len(payload),16):print(f'{i:04x}',payload[i:i+16].hex(' '))
    if name.endswith('.anm'):
        bank=core.AnimationBank(p/name)
        print('RELOCS',bank.reloc)
        print('BLOCKS',[asdict(b) for b in bank.blocks])
