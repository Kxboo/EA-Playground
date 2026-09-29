import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
for source in ('files/data/fe/components/artwork/stdart.big::StdArt.gsh','files/data/fe/main.big::Main.gsh'):
    data,name=research.read_virtual(str(core.DEFAULT_DATA/source));g,raw=core.gsh_parser.parse_gsh(research.materialize(data,name))
    for e in g.entries[:3]:
        print(name,{k:v for k,v in vars(e).items() if k!='palette'})
        print('PALETTE',data[e.img_end:e.img_end+48].hex(' '))
        print('BASE END',data[e.img_offset+e.width*e.height:e.img_offset+e.width*e.height+48].hex(' '))
