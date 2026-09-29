import sys
from pathlib import Path
from collections import Counter
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core
root=core.DEFAULT_DATA
print('LOOSE',Counter(p.suffix.lower() for p in root.rglob('*') if p.is_file()))
print('OLD',Counter(p.suffix.lower() for p in core.DEFAULT_OLD.rglob('*') if p.is_file()))
paths=list(root.rglob('*.csv'))+list(core.DEFAULT_OLD.rglob('rc_track_car*'))+list(core.DEFAULT_OLD.rglob('world.csv'))+list(core.DEFAULT_OLD.rglob('player.csv'))
for p in paths:
    if p.is_file():
        data=p.read_bytes()
        print(str(p),len(data),repr(data[:180]))
        if data.startswith(b'\x7fELF'):
            sections,start=core.model_parser._read_sections(data)
            print('SECTIONS',sections)
            symbols=next((s for s in sections if s['name']=='.strtab'),None)
            if symbols:print('SYMBOL STRINGS',data[symbols['offset']:symbols['offset']+symbols['size']])
for ext in ('.tpl','.lef','.abk','.loc','.bnk','.atd','.csi','.arc','.hkx','.gfn','.bts','.idx','.vlt','.gsm','.con'):
    paths=list(root.rglob('*'+ext))
    if paths:
        p=paths[0]
        print(ext,str(p.relative_to(root)),p.stat().st_size,p.read_bytes()[:80].hex())
