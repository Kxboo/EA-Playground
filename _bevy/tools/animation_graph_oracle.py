"""Native CSV AnimationStateGraph projection; CSV/CString services are supplied.

State conversion, graph stores, alternate selection and case-sensitive bank
lookup execute original instructions. No animation Update/playback is claimed.
"""
import csv, hashlib, io, json, os, struct, sys
from pathlib import Path
from tetherball_startup_oracle import StartupEmu, SHA, ROOT
import re_functions as rf
sys.path.insert(0, str(ROOT/'Remaster/src'))
from research import read_virtual
from legacy.exporter.anm_exporter import parse_anm_clips

OUT=ROOT/'_bevy/tests/data/animation_graph_golden.json'
NAMES=ROOT/'_bevy/src/animation_state_names.json'
GRAPH=0x73000000; FN=0x73100000; BANK=0x73101000; TABLE=0x73102000

class GraphEmu(StartupEmu):
 def step(self,pc):
  w=self.word(pc)
  if w>>26==19 and (w>>1)&1023==193:
   d,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31;mask=1<<(3-(d&3))
   self.cr[d>>2]=(self.cr[d>>2]&~mask)|((self.crbit(a)^self.crbit(b))<<(3-(d&3)))
   return pc+4
  return super().step(pc)
 def __init__(self,exe,names):
  super().__init__(exe);self.strings={};self.heap=0x73200000;self.rows=[];self.row=-1
  self.w32(FN,0x804eb6dc);self.w32(FN+4,BANK)
  self.w32(BANK+4,len(names));self.w32(BANK+0x14,TABLE)
  for i,n in enumerate(names):self.w32(TABLE+i*4,self.text(n))
  def put(e,p,value):
   e.strings[p]=value;ptr=e.text(value);e.w32(p,ptr)
  def assign(e,copy=False):put(e,e.r[3],e.strings[e.r[4]] if copy else e.cstring(e.r[4]))
  self.hooks[0x803ccc08]=lambda e:put(e,e.r[3],'')
  for a in [0x803ccc78,0x803cce0c]:self.hooks[a]=lambda e:assign(e)
  for a in [0x803ccd20,0x803ccd84]:self.hooks[a]=lambda e:assign(e,True)
  self.hooks[0x803cce94]=lambda e:None
  self.hooks[0x803cd454]=lambda e:e.r.__setitem__(3,e.r32(e.r[3]))
  self.hooks[0x803cd38c]=lambda e:e.r.__setitem__(3,self.compare(e.strings[e.r[3]],e.cstring(e.r[4])))
  self.hooks[0x80025f14]=lambda e:e.r.__setitem__(3,self.compare(e.cstring(e.r[3]),e.cstring(e.r[4])))
  self.hooks[0x803cd20c]=lambda e:e.r.__setitem__(3,e.strings[e.r[3]].find(chr(e.r[4]),e.r[5])&0xffffffff)
  self.hooks[0x803cd13c]=lambda e:put(e,e.r[3],e.strings[e.r[3]][:e.r[4]]+(e.strings[e.r[3]][e.r[4]+e.r[5]:] if e.r[5]!=0xffffffff else ''))
  self.hooks[0x803cd064]=lambda e:put(e,e.r[3],e.strings[e.r[4]][e.r[5]:] if e.r[6]==0xffffffff else e.strings[e.r[4]][e.r[5]:e.r[5]+e.r[6]])
  self.hooks[0x802ed23c]=lambda e:put(e,e.r[3],e.strings[e.r[4]]+e.strings[e.r[5]])
  self.hooks[0x802f2c84]=lambda e:None
  self.hooks[0x802f2cf0]=lambda e:None
  def next_row(e):e.row+=1;e.r[3]=int(e.row<len(e.rows))
  self.hooks[0x802f2e00]=next_row
  def field(e,kind):
   value=e.rows[e.row].get(e.cstring(e.r[4]),'')
   if kind=='string':put(e,e.r[5],value)
   elif value:
    if kind=='int':e.w32(e.r[5],int(value))
    else:e.wr(e.r[5],struct.pack('>f',float(value)))
   e.r[3]=int(bool(value))
  self.hooks[0x802f2eb0]=lambda e:field(e,'string')
  self.hooks[0x802f3054]=lambda e:field(e,'int')
  self.hooks[0x802f2f80]=lambda e:field(e,'float')
  self.hooks[0x8002451c]=lambda e:e.wr(e.r[3],(e.cstring(e.r[4])%e.r[5]).encode()+b'\0')
 def text(self,value):
  p=self.heap;self.heap+=len(value)+17;self.w32(p-12,len(value));self.wr(p,value.encode()+b'\0');return p
 @staticmethod
 def compare(a,b):return ((a>b)-(a<b))&0xffffffff
 def apply(self,rows):
  self.rows=rows;self.row=-1;self.call(0x803c92d8,[GRAPH,0,0,FN],max_steps=5_000_000)
 def snapshot(self):
  states=[]
  for i in range(246):
   p=GRAPH+i*0x4c
   if self.rd(p+0x3c,1)==b'\0':states.append(None);continue
   state=dict(asset=self.strings.get(p,''),clips=[self.r32(p+4+j*4) for j in range(self.rd(p+0x34,1)[0])],
    blend_bits=self.r32(p+0xc),start_bits=self.r32(p+0x10),trim_bits=self.r32(p+0x14),next=self.r32(p+0x38),
    looping=bool(self.rd(p+0x30,1)[0]),reverse=bool(self.rd(p+0x31,1)[0]),random=bool(self.rd(p+0x32,1)[0]),frame_time=bool(self.rd(p+0x33,1)[0]),
    props=[self.strings.get(p+o,'') for o in [0x40,0x44,0x48]],events=[])
   for j in range(2):
    q=p+0x18+j*12
    state['events'].append(dict(name=self.strings.get(q,''),time_bits=self.r32(q+4)) if self.rd(q+8,1)!=b'\0' else None)
   states.append(state)
  return states

def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();table=exe.symbols['sAnimStateNames__19AnimationStateGraph']
 names=[]
 for i in range(246):
  p=struct.unpack('>I',exe.read(table['value']+4*i,4))[0];names.append(exe.read(p,100).split(b'\0')[0].decode())
 data=Path(os.environ.get('EAGL_DATA',ROOT/'eagl EA PLAYGROUND/extra/more/eaplayground files/DATA'))
 source=str(data/'files/data/characters/player_anims.viv')
 bank=read_virtual(source+'::player_anims.anm')[0]
 path=ROOT/'_bevy/logs/animation-graph-bank.anm'
 path.parent.mkdir(exist_ok=True);path.write_bytes(bank);bank_names=parse_anm_clips(path)[4]
 em=GraphEmu(exe,bank_names);em.wr(GRAPH,bytes(246*0x4c));cases=[]
 for filename in ['player.csv','player_female.csv']:
  raw=read_virtual(source+'::'+filename)[0];rows=list(csv.DictReader(io.StringIO(raw.decode())))
  em.apply(rows);cases.append(dict(file=filename,sha256=hashlib.sha256(raw).hexdigest(),states=em.snapshot()))
 base=dict(ANIM_STATE='ANIM_TB_IDLE',ANIM_ASSET='missing|TB_Idle',ANIM_LOOPING='-2',RANDOM_START_TIME='3',ANIM_BLEND_TIME='0.25',ANIM_NEXT_STATE='invalid',ANIM_REVERSE='4',USE_FRAMETIME='5',START_TIME='0.5',TRUNCATE_TIME='0.75',EVENT_NAME1='TB_RegularStrike',EVENT_TIME1='0.25',EVENT_NAME2='',EVENT_TIME2='',CS_L_PROP_BONE='l',CS_R_PROP_BONE='r',CS_BODY_PROP_BONE='body')
 rows=[base,dict(base,ANIM_STATE='unknown'),dict(base,ANIM_STATE='ANIM_TB_SERVEIDLE',ANIM_ASSET='missing'),dict(base,ANIM_STATE='ANIM_TB_SERVEEND',ANIM_ASSET='TB_ServeEnd|TB_Idle|TB_ServeIdle')]
 em.apply(rows);cases.append(dict(rows=rows,states=em.snapshot()))
 return dict(elf_sha256=SHA,state_names=names,bank_names=bank_names,cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:
  assert json.loads(OUT.read_text())==value
  assert json.loads(NAMES.read_text())==value['state_names']
 else:
  OUT.write_text(json.dumps(value,indent=1)+'\n');NAMES.write_text(json.dumps(value['state_names'],indent=1)+'\n')
 print('Verified native CSV graph initialization, female overlay and synthetic failure/alternate cases')
