import sys,struct
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core
from containers import Elf,cstr
for p in (core.HOME/'reference/player_anims.anm',core.DEFAULT_OLD/'placeables/rc_trackcar/rc_track_car_anims.anm'):
    b=core.AnimationBank(p);elf=Elf(b.data);start=elf.section('.data')['offset']
    symbol=next(s for s in elf.symbols if s['name'].startswith('__AnimationBank:::'))
    bank=symbol['value'];base=start+bank
    count=struct.unpack_from('>I',b.data,base+4)[0]
    table=b.reloc.get(bank+20)
    names=[cstr(b.data,start+b.reloc[table+i*4]) if table+i*4 in b.reloc else None for i in range(count)]
    print(p.name,count,table,len(names),sum(a!=z for a,z in zip(names,b.names)),names[:5])
