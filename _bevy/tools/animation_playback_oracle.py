"""Original AnimationState selection, timing, events and complete Update.

Only clip allocation/query, pose operations, markers and registered handlers
are engine boundaries. Native nested timing/event/state bodies stay unhooked.
"""
import copy,hashlib,json,struct,sys
from animation_graph_oracle import GraphEmu, GRAPH, FN, ROOT, SHA
from ppc_emu2 import sx
import re_functions as rf

OUT=ROOT/'_bevy/tests/data/animation_playback_golden.json'
OBJ=0x74000000;ASSETS=0x74100000;FUNC=0x74200000;FVT=0x74201000;MGR=0x74202000;MVT=0x74203000
BUFFERS=[0x74300000,0x74301000,0x74302000];PROC=0x74400000;PVT=0x74401000
FLOATS={'time':0x48,'duration':0x4c,'speed':0x50,'start':0x58,'trim':0x5c,'blend_remaining':0x60,'blend_total':0x64,'auto_after':0x6c}
WORDS={'function':0x3c,'current':0x54,'pose_words':0x40,'marker_count':0x1670,'handler_count':0x28}
FLAGS={'pose_valid':0x44,'blending':0x68,'skip_advance':0x69}
def bits(v):return struct.unpack('>I',struct.pack('>f',v))[0]
def float_(v):return struct.unpack('>f',struct.pack('>I',v))[0]

class PlaybackEmu(GraphEmu):
 def __init__(self,exe,fixture):
  super().__init__(exe,fixture['bank_names']);self.fixture=fixture
  self.hooks.pop(0x803c8b70,None)
  # Pose-mask construction is an engine boundary; record mask presence at each
  # consuming pose operation rather than emulate skeleton evaluation here.
  self.hooks[0x803fca44]=lambda e:None
  self.hooks[0x803fc9d4]=lambda e:None
  self.w32(ASSETS,FN);self.w32(ASSETS+4,GRAPH)
  self.w32(0x8060242c,MGR);self.w32(MGR,MVT);self.w32(FUNC,FVT)
  for i,p in enumerate(BUFFERS):self.w32(OBJ+0x30+i*4,p)
  def hook(offset,callback):
   address=0x74500000+offset;self.w32(FVT+offset,address);self.hooks[address]=callback
  def set_frame(e):e.mode=bool(e.r[4]);e.effects.append(['use_fps',e.r[3],e.mode])
  def time(e):e.effects.append(['function_length',e.r[3],e.mode]);e.w32(e.r[4],bits(e.input['seconds'] if e.mode else e.input['samples']))
  def evaluate(e):e.effects.append(['evaluate',e.r[3],bits(e.f[1]),BUFFERS.index(e.r[4]),bool(e.r[5])])
  hook(0x10,set_frame);hook(0x18,time);hook(0x20,evaluate)
  def get_clip(e):e.effects.append(['clip',e.r[4]]);e.r[3]=0x74600000+e.r[4]*4
  self.hooks[0x803c84fc]=get_clip
  def allocate(e):e.effects.append(['allocate',e.r[4]]);e.r[3]=FUNC
  def release(e):e.effects.append(['release',e.r[4]])
  self.w32(MVT+0x30,0x74500100);self.hooks[0x74500100]=allocate
  self.w32(MVT+0x34,0x74500104);self.hooks[0x74500104]=release
  self.hooks[0x803b2344]=lambda e:(e.effects.append(['random_index',e.r[4]]),e.r.__setitem__(3,e.input['random_index']))
  self.hooks[0x803b2394]=lambda e:(e.effects.append(['random_time',bits(e.f[2])]),e.f.__setitem__(1,float_(e.input['random_time_bits'])))
  self.hooks[0x8041af54]=lambda e:e.effects.append(['copy',e.r[5]])
  self.hooks[0x803f3fe0]=lambda e:e.effects.append(['still',BUFFERS.index(e.r[4]),bool(e.r[5])])
  self.hooks[0x803f6f68]=lambda e:e.effects.append(['blend',bits(e.f[1]),bool(e.r[7])])
  self.hooks[0x803f4418]=lambda e:e.effects.append(['skin',bool(e.r[6])])
  self.hooks[0x803c9bf0]=lambda e:e.effects.append(['marker',(e.r[3]-OBJ-0x70)//0xb0])
  self.w32(PROC,PVT);self.w32(PVT+8,0x74500200);self.hooks[0x74500200]=lambda e:e.effects.append(['procedural'])
  self.w32(0x74700000,0x74701000);self.w32(0x74701000+8,0x74500204)
  def event(e):
   e.effects.append(['event',e.r[5],e.strings[e.r[4]],e.r32(e.r[4]+4)])
   if e.input.get('shrink_handlers'):e.w32(OBJ+0x28,1)
  self.hooks[0x74500204]=event
 def prepare(self,c):
  self.input=c['input'];self.effects=[];self.mode=False
  self.wr(OBJ,bytes(0x1680));self.w32(OBJ+4,ASSETS);self.w32(OBJ,FN)
  for i,p in enumerate(BUFFERS):self.w32(OBJ+0x30+i*4,p)
  self.w32(OBJ+0x2c,PROC if self.input['procedural'] else 0)
  for name,offset in {**FLOATS,**WORDS}.items():self.w32(OBJ+offset,c['initial'][name])
  for name,offset in FLAGS.items():self.wr(OBJ+offset,bytes([int(c['initial'][name])]))
  for i in range(4):self.w32(OBJ+8+i*8,0x74700000);self.w32(OBJ+12+i*8,i)
  self.wr(GRAPH,bytes(246*0x4c))
  for id,s in c['graph'].items():
   i=int(id)
   if s is None:continue
   p=GRAPH+i*0x4c;self.strings[p]=s['asset'];self.wr(p+0x3c,b'\1');self.wr(p+0x34,bytes([len(s['clips'])]))
   for j,v in enumerate(s['clips']):self.w32(p+4+j*4,v)
   for name,o in [('blend_bits',12),('start_bits',16),('trim_bits',20),('next',56)]:self.w32(p+o,s[name])
   for name,o in [('looping',48),('reverse',49),('random',50),('frame_time',51)]:self.wr(p+o,bytes([int(s[name])]))
   for j,event in enumerate(s['events']):
    q=p+24+j*12
    if event:self.strings[q]=event['name'];self.w32(q+4,event['time_bits']);self.wr(q+8,b'\1')
 def snapshot(self):
  s={name:self.r32(OBJ+off) for name,off in {**FLOATS,**WORDS}.items()}
  s.update({name:bool(self.rd(OBJ+off,1)[0]) for name,off in FLAGS.items()});return s

def generate():
 fixture=json.loads((ROOT/'_bevy/tests/data/animation_graph_golden.json').read_text());assert fixture['elf_sha256']==SHA
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 em=PlaybackEmu(rf.load(),fixture);cases=[]
 for i in range(256):
  graph=copy.deepcopy(fixture['cases'][0]['states'])
  ids=[0,56,58,59,60,63,96];current=ids[i%len(ids)];target=ids[(i+1)%len(ids)]
  for id in ids:
   s=graph[id];s['looping']=bool(i&1);s['reverse']=bool(i&2);s['frame_time']=bool(i&4);s['random']=bool(i&8)
   s['blend_bits']=bits([0.,.15,.25,.4][id%4]);s['start_bits']=bits([0.,.25][i%2]);s['trim_bits']=bits(.1)
   s['next']=ids[(ids.index(id)+1)%len(ids)] if i&16 else 247
   s['events']=[dict(name='TB_test_0',time_bits=bits(.2)),dict(name='TB_test_1',time_bits=bits(.5))]
  initial=dict(function=FUNC if i&1 else 0,pose_valid=bool(i&1),time=bits([0.,.2,.5,.95,1.2,-.2][i%6]),duration=bits(1.),speed=bits([.5,1.,2.,-1.][i%4]),current=current,start=bits(.1),trim=bits(.2),blend_remaining=bits(.25),blend_total=bits(.4),blending=bool(i&4),skip_advance=bool(i&8),auto_after=bits([-.001,0.,.01,.1][i%4]),pose_words=96,marker_count=[-1,0,1,2][i%4],handler_count=[0,1,2,4][i%4])
  input=dict(target=target,force=bool(i&2),speed_bits=bits([.5,1.,2.][i%3]),after_ms=[-1,0,1,200][i%4],samples=[2.,16.,31.,61.][i%4],seconds=[2./30.,16./30.,31./30.,61./30.][i%4],random_index=i%2,random_time_bits=bits(.3),delta_bits=bits([0.,.016,.2,.6,1.5][i%5]),request_pose=bool(i&32),use_mask=bool(i&64),procedural=bool(i&128),shrink_handlers=i%17==0,value_bits=bits([-.5,0.,.5,1.,1.5][i%5]))
  if i%7==0:graph[target]['clips'].append(graph[target]['clips'][0])
  if i%13==0:target=current;input['target']=current
  if i%19==0:graph[target]=None
  c=dict(operation=['select','set_next','update','events','time'][i%5],initial=initial,input=input)
  if c['operation']=='update':initial['function']=FUNC
  # select is only valid on loaded targets with clip-count>0.
  if c['operation']=='select' and graph[target] is None:graph[target]=copy.deepcopy(fixture['cases'][0]['states'][target])
  if graph[current] is None:graph[current]=copy.deepcopy(fixture['cases'][0]['states'][current])
  c['graph']={str(id):graph[id] for id in ids}
  em.prepare(c)
  if c['operation']=='select':em.call(0x803c8318,[OBJ,target])
  elif c['operation']=='set_next':c['result']=em.call(0x803c8b78,[OBJ,target,int(input['force']),input['after_ms']&0xffffffff],[float_(input['speed_bits'])])
  elif c['operation']=='update':em.call(0x803c851c,[OBJ,int(input['request_pose']),int(input['use_mask'])],[float_(input['delta_bits'])])
  elif c['operation']=='events':em.call(0x803c89a4,[OBJ],[float_(input['delta_bits'])])
  else:
   em.call(0x803c8c84,[OBJ],[float_(input['value_bits'])]);em.call(0x803c8cd4,[OBJ]);c['result_bits']=bits(em.f[1])
  c['expected']=em.snapshot();c['effects']=copy.deepcopy(em.effects);cases.append(c)
 return dict(elf_sha256=SHA,cases=cases)
if __name__=='__main__':
 value=generate()
 if '--check' in sys.argv:assert json.loads(OUT.read_text())==value
 else:OUT.write_text(json.dumps(value,indent=1)+'\n')
 print('Verified',len(value['cases']),'native AnimationState calls with nested timing/event transitions')
