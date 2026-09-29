"""Diagnostic decoded texture samples, no resampling or color correction."""
import sys,json
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
from png import encode
out=bytearray(bytes((60,60,60,255))*1024*768)
x=y=rowh=0
samples=[]
sources=json.loads((core.HOME/'research/texture-recheck.json').read_text())['results']
for record in sources:
    source=record['source']
    if '/fe/' not in source.replace('\\','/'):continue
    data,name=research.read_virtual(source)
    g,_=core.gsh_parser.parse_gsh(research.materialize(data,name))
    for e in g.entries:
        if not 60<=e.width<=512 or not 40<=e.height<=300:continue
        if data[e.img_end]!=0x33:continue
        rgba,w,h=core.gsh_parser.decode_entry_rgba(e,data)
        if x+w>1024:x=0;y+=rowh+8;rowh=0
        if y+h>768:break
        for cy in range(h):
            for cx in range(w):
                i=(cy*w+cx)*4;j=((cy+y)*1024+x+cx)*4;a=rgba[i+3]
                bg=160 if ((cx+x)//16+(cy+y)//16)%2 else 210
                out[j:j+4]=bytes(tuple((rgba[i+c]*a+bg*(255-a))//255 for c in range(3))+(255,))
        samples.append(dict(file=name,index=e.index,name=e.full_name,x=x,y=y,width=w,height=h))
        x+=w+8;rowh=max(rowh,h)
    if y>500:break
(core.HOME/'research/texture-contact.png').write_bytes(encode(out,1024,768))
print(samples)
