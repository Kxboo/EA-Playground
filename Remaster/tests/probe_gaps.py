import sys,json,struct,traceback
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
from containers import Elf
d=json.loads((core.HOME/'research/corpus-v2.json').read_text())
r=next(r for r in d['records'] if r['name']=='strapwarn_standard_english.gsh')
data,name=research.read_virtual(r['source']);g,raw=core.gsh_parser.parse_gsh(research.materialize(data,name))
for e in g.entries[:1]:
    print('GSH ENTRY',vars(e))
    print('POST',raw[e.img_end:e.img_end+96].hex(' '))
    print('PRE',raw[e.entry_offset:e.entry_offset+32].hex(' '))
r=next(r for r in d['records'] if r['name']=='rccartrack26.o')
data,name=research.read_virtual(r['source'])
try:research.payload_report(data,name)
except Exception:traceback.print_exc()
for name in ('Main.o','rc_track_car.o','teeter_totter.o','basketball_shadow.o'):
    r=next(r for r in d['records'] if r['name']==name)
    data,name=research.read_virtual(r['source']);elf=Elf(data)
    print('ELF',name,[(s['name'],s['value']) for s in elf.symbols if s['name']][:15])
    if name=='rc_track_car.o':
        path=research.materialize(data,name)
        a=core.model_parser.parse_o_file(path,layouts_path=core.LEGACY/'eagl_layouts.json')
        print('NO INSPECT',len(a.meshes),a.total_faces,a.log[:20])
