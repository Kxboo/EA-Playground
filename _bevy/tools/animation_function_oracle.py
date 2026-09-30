"""Unhooked FnCompound UseFPS/GetLength and complete native attribute lookup.

Original archive records plus synthetic cache/missing/zero-FPS cases. No
function allocation, evaluation or constructor default is claimed here.
"""
import hashlib,json,struct,sys
from pathlib import Path
from animation_graph_oracle import ROOT,SHA,parse_anm_clips,read_virtual
from ppc_emu2 import Emu
import re_functions as rf

OUT=ROOT/'_bevy/tests/data/animation_function_golden.json'
OBJ=0x75000000;MAP=0x75001000;ATTR=0x75002000;RESULT=0x75003000
FUNCTIONS=[0x803fecac,0x803ffaf8,0x803ffb54,0x803fcdf8,0x803fce60,0x803fc424,0x803fc478]

def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 source=ROOT/'eagl EA PLAYGROUND/extra/more/eaplayground files/DATA/files/data/characters/player_anims.viv'
 raw=read_virtual(str(source)+'::player_anims.anm')[0]
 path=ROOT/'_bevy/logs/animation-function-bank.anm';path.write_bytes(raw)
 data,reloc,ds,clips,names=parse_anm_clips(path)
 seeds=[]
 for cb,name in zip(clips,names):
  if cb.container_tag!=15:continue
  count=struct.unpack_from('>H',data,cb.abs_off+10)[0]
  attr=reloc.get(cb.rel_off+4);entries=[]
  if attr is not None:
   q=ds+attr;n=struct.unpack_from('>I',data,q)[0]
   entries=[(struct.unpack_from('>H',data,q+4+i*8)[0],data[q+8+i*8]) for i in range(n)]
  seeds.append(dict(name=name,index=cb.index,initial=dict(samples=count,use_fps=False,fps=0),attributes=entries,has_attributes=attr is not None))
 for i in range(96):
  entries=[[],[(1,0)],[(1,24)],[(0,19),(1,37),(2,99)],[(0,30),(2,60)],[(1,255)]][i%6]
  seeds.append(dict(name=f'synthetic_{i}',initial=dict(samples=[0,1,2,12,31,65535][i%6],use_fps=bool(i&1),fps=[0,0,30,255][i%4]),attributes=entries,has_attributes=bool(i%7)))
 exe=rf.load();cases=[]
 for seed in seeds:
  em=Emu(exe);em.wr(OBJ,bytes(32));em.w32(OBJ,0x804edf18);em.w32(OBJ+12,MAP)
  em.w32(MAP+4,ATTR if seed['has_attributes'] else 0)
  em.wr(MAP+10,struct.pack('>H',seed['initial']['samples']))
  em.wr(OBJ+20,bytes([int(seed['initial']['use_fps']),seed['initial']['fps']]))
  em.w32(ATTR,len(seed['attributes']))
  for i,(id,value) in enumerate(seed['attributes']):em.wr(ATTR+4+i*8,struct.pack('>H2xB3x',id,value))
  steps=[]
  for enabled in [False,True,True,False,True]:
   em.call(0x803fecac,[OBJ,int(enabled)])
   fps=em.rd(OBJ+21,1)[0];length=None
   # The local interpreter does not model PPC divide-by-zero/FPSCR behavior.
   # Verify the cache/flag stores but do not manufacture native length output.
   if not enabled or fps:
    result=em.call(0x803ffaf8,[OBJ,RESULT]);assert result==1;length=em.r32(RESULT)
   assert bool(em.rd(OBJ+20,1)[0])==enabled
   steps.append(dict(enabled=enabled,fps=fps,length_bits=length))
  seed['steps']=steps
  # Null attributes must remain distinct from a populated supplied block.
  if not seed['has_attributes']:seed['attributes']=[]
  seed['attributes']=[list(entry) for entry in seed['attributes']]
  cases.append(seed)
 return dict(elf_sha256=SHA,bank_sha256=hashlib.sha256(raw).hexdigest(),functions=[dict(address=hex(a),symbol=exe.addresses[a],size=exe.symbols[exe.addresses[a]]['size'],code_sha256=hashlib.sha256(exe.read(a,exe.symbols[exe.addresses[a]]['size'])).hexdigest()) for a in FUNCTIONS],cases=cases)

if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 evidence=ROOT/'_bevy/docs/evidence/animation_function';exe=rf.load()
 evidence.mkdir(parents=True,exist_ok=True)
 for function in value['functions']:
  text='\n'.join(line.rstrip() for line in rf.annotate(exe,function['symbol']).splitlines())+'\n'
  path=evidence/(function['address'][2:]+'-'+function['symbol'].split('__')[0]+'.txt')
  if '--check' in sys.argv:assert path.read_text()==text
  else:path.write_text(text)
 mapping={0x803fecac:('_bevy/src/animation_function.rs','set_use_fps'),0x803ffaf8:('_bevy/src/animation_function.rs','length'),0x803ffb54:('_bevy/src/anim.rs','compound_function')}
 functions=[]
 for function in value['functions']:
  record=dict(function)
  file,method=mapping.get(int(record['address'],16),('_bevy/src/animation_function.rs','attribute_byte'))
  lines=(ROOT/file).read_text().splitlines();line=next(i+1 for i,text in enumerate(lines) if 'pub fn '+method+'(' in text)
  record.update(rust=dict(path=file,method=method,line=line),disassembly=record['address'][2:]+'-'+record['symbol'].split('__')[0]+'.txt',status='finite differential projection; native routine unhooked')
  functions.append(record)
 sources=['_bevy/src/animation_function.rs','_bevy/src/anim.rs','_bevy/src/character.rs','_bevy/tools/animation_function_oracle.py','_bevy/tools/ppc_emu2.py','_bevy/tools/re_functions.py','_bevy/tests/data/animation_function_golden.json']
 index=dict(elf_sha256=SHA,bank_sha256=value['bank_sha256'],functions=functions,fixture_cases=len(value['cases']),installed_hooks=[],source_sha256={file:hashlib.sha256((ROOT/file).read_bytes()).hexdigest() for file in sources},comparison='flag/FPS stores and finite length float bits; original-bank Rust test verifies metadata and Bevy durations',limits=['zero-FPS length division not executed by oracle','pose evaluation and interpolation unverified','native allocator defaults not established'])
 text=json.dumps(index,indent=2)+'\n';path=evidence/'index.json'
 if '--check' in sys.argv:assert json.loads(path.read_text())==index
 else:path.write_text(text)
 print('Verified',len(value['cases']),'compound functions; native length/FPS/attribute bodies unhooked')
