"""Execute original PhysicsDynamicCharacter::BuildCharacterInput with host support/proxy boundaries."""
import hashlib,json,math,random,struct,sys
from pathlib import Path
from ppc_emu2 import Emu,M,sx,f32
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/character_input_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'

def fbits(x):return struct.unpack('>I',struct.pack('>f',x))[0]

class InputEmu(Emu):
 def __init__(self,exe):super().__init__(exe);self.payload={}
 def step(self,pc):
  w=self.word(pc);op=w>>26;d,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31;xo=(w>>1)&1023
  if op==4: # Compiler paired register restores: following lfd handles scalar register.
   assert (w&63)==12,f'Unexpected paired instruction {pc:x}: {w:x}'
   return pc+4
  if op==31:
   if xo==599:self.f[d]=struct.unpack('>d',self.rd((self.r[a] if a else 0)+self.r[b],8))[0];return pc+4
   if xo==183:addr=(self.r[a]+self.r[b])&M;self.w32(addr,self.r[d]);self.r[a]=addr;return pc+4
   if xo==19:self.r[d]=sum(v<<(28-4*i) for i,v in enumerate(self.cr));return pc+4
   if xo==60:self.r[a]=self.r[d]&(~self.r[b]&M);return pc+4
   if xo==792:self.r[a]=(sx(self.r[d],32)>>(self.r[b]&63))&M;return pc+4
   if xo==75:self.r[d]=(sx(self.r[a],32)*sx(self.r[b],32)>>32)&M;return pc+4
  if op==19 and xo==449:
   value=self.crbit(a)|self.crbit(b);field=d>>2;bit=3-(d&3)
   self.cr[field]=(self.cr[field]&~(1<<bit))|(value<<bit);return pc+4
  if op in (12,13):
   t=self.r[a]+(sx(w,16)&M);self.r[d]=t&M;self.ca=int(t>M)
   if op==13:self.setcr0(self.r[d])
   return pc+4
  if op==63:
   if xo in (0,32):
    x,y=self.f[a],self.f[b];self.cr[(w>>23)&7]=1 if math.isnan(x) or math.isnan(y) else 8 if x<y else 4 if x>y else 2;return pc+4
   if xo==264:self.f[d]=abs(self.f[b]);return pc+4
   if xo==15:
    x=math.trunc(self.f[b]);self.payload[d]=x&M if -0x80000000<=x<=0x7fffffff else 0x80000000;return pc+4
   low=xo&31;c=(w>>6)&31
   if low in (18,20,21,25,28,29,30,31):
    x,y,z=self.f[a],self.f[b],self.f[c]
    self.f[d]={18:lambda:x/y,20:lambda:x-y,21:lambda:x+y,25:lambda:x*z,28:lambda:math.fma(x,z,-y),29:lambda:math.fma(x,z,y),30:lambda:-math.fma(x,z,-y),31:lambda:-math.fma(x,z,y)}[low]()
    self.payload.pop(d,None);return pc+4
  if op==54 and d in self.payload:self.wr((self.r[a] if a else 0)+sx(w,16),struct.pack('>II',0xfff80000,self.payload[d]));return pc+4
  if op in (48,50,59,63):self.payload.pop(d,None)
  return super().step(pc)


def capture(exe,v):
 e=InputEmu(exe);obj=0x71000000;out=0x71001000;step=0x71002000;proxy=0x71003000;pos=0x71004000;vel=0x71005000
 def vec(addr,x):e.wr(addr,struct.pack('>'+'f'*len(x),*x))
 e.w32(obj+0xa4,proxy)
 vec(obj+0x38,[v['speed'],v['orientation']]);vec(obj+0x20,v['gravity'])
 vec(pos,v['position']);vec(vel,v['velocity'])
 e.wr(obj+0xac,bytes([v['gravity_override'] is not None]))
 vec(obj+0xb0,v['gravity_override'] or [0,0,0])
 vec(obj+0x70,v['impulse']);vec(obj+0x80,v['impulse_decay']);e.w32(obj+0x90,v['impulse_ms'])
 if 'apply_velocity' in v:
  vec(0x71006000,v['apply_velocity']);e.call(0x803b6768,(obj,0x71006000,v['duration_ms']))
 seconds=f32(f32(v['dt_ms'])/1000);vec(step,[0,0,seconds,f32(1/seconds) if seconds else 0])
 def support(em):
  em.w32(em.r[5],v['support_kind']);vec(em.r[5]+0x10,v['surface_normal']);vec(em.r[5]+0x20,v['surface_velocity'])
 e.hooks[0x801d9480]=support
 e.hooks[0x801db7ec]=lambda em:em.r.__setitem__(3,pos)
 e.hooks[0x801db814]=lambda em:em.r.__setitem__(3,vel)
 e.call(0x803b67f4,(obj,out,step))
 return dict(v,output_words=[e.r32(out+i*4) for i in range(40)],gravity_override_consumed=not bool(e.rd(obj+0xac,1)[0]),next_impulse_bits=[e.r32(obj+0x70+i*4) for i in range(3)],next_decay_bits=[e.r32(obj+0x80+i*4) for i in range(3)],next_impulse_ms=sx(e.r32(obj+0x90),32))

def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();vectors=[]
 base=dict(dt_ms=16,speed=2.5,orientation=0,gravity=[0,-9.81,0],gravity_override=None,position=[1,2,3,0],velocity=[4,5,6,0],support_kind=2,surface_normal=[0,1,0,0],surface_velocity=[0,0,0,0],impulse=[0,0,0],impulse_decay=[0,0,0],impulse_ms=0)
 cases=[]
 for speed in [-40,-20,-2.5,0,2.5,20,40]:
  for angle in [0,1.5707963705062866,3.1415927410125732,-1.5707963705062866]:
   cases.append(dict(base,speed=speed,orientation=angle))
 for dt in [0,1,16,60,200,1000]:
  for impulse in [[0,0,0],[0,0,20],[20,0,0],[-20,0,-20],[0,20,0]]:
   cases.append(dict(base,dt_ms=dt,impulse=impulse,impulse_decay=[-1,-2,-3],impulse_ms=120))
 for duration in [-1,1,17,60,200,1000]:
  for dt in [1,16,60,201]:
   cases.append(dict(base,dt_ms=dt,apply_velocity=[7,-3,11],duration_ms=duration))
 import random
 rng=random.Random(0x803b67f4)
 for i in range(100):
  cases.append(dict(base,dt_ms=rng.choice([0,1,7,16,33,60,200,201]),speed=rng.uniform(-45,45),orientation=rng.uniform(-6.2,6.2),support_kind=i%4,gravity_override=[1,-2,3] if i%3==0 else None,impulse=[rng.uniform(-25,25) for _ in range(3)],impulse_decay=[rng.uniform(-4,4) for _ in range(3)],impulse_ms=rng.choice([-1,0,1,17,200])))
 for v in cases:vectors.append(capture(exe,v))
 return dict(elf_sha256=SHA,vectors=vectors)

def rust_check(result):
 """Compile a dependency-free harness in a temporary directory, without Cargo."""
 import subprocess
 def scalar(x):return f'f32::from_bits({fbits(x)})'
 def array(xs):return '['+','.join(scalar(x) for x in xs)+']'
 lines=['#[path = '+json.dumps(str(ROOT/'_bevy/src/character_input.rs'))+'] mod character_input;','use character_input::*;fn main(){']
 for i,v in enumerate(result['vectors']):
  override='None' if v['gravity_override'] is None else 'Some('+array(v['gravity_override'])+')'
  lines.append('let mut s=CharacterInputState{speed:'+scalar(v['speed'])+',orientation:'+scalar(v['orientation'])+',gravity:'+array(v['gravity'])+',gravity_override:'+override+',impulse:'+array(v['impulse'])+',impulse_decay:'+array(v['impulse_decay'])+',impulse_ms:'+str(v['impulse_ms'])+'};')
  if 'apply_velocity' in v:lines.append('s.apply_velocity('+array(v['apply_velocity'])+','+str(v['duration_ms'])+');')
  lines.append('let o=s.build('+str(v['dt_ms'])+',SurfaceSupport{kind:'+str(v['support_kind'])+',normal:'+array(v['surface_normal'])+',velocity:'+array(v['surface_velocity'])+'},'+array(v['position'])+','+array(v['velocity'])+');')
  lines.append('assert_eq!(o.output_words(),'+str(v['output_words'])+',"output '+str(i)+'");')
  lines.append('assert_eq!(s.impulse.map(f32::to_bits),'+str(v['next_impulse_bits'])+',"impulse '+str(i)+'");assert_eq!(s.impulse_ms,'+str(v['next_impulse_ms'])+');')
  lines.append('assert_eq!(s.impulse_decay.map(f32::to_bits),'+str(v['next_decay_bits'])+');')
 lines.append('println!("Matched '+str(len(result['vectors']))+' original vectors");}')
 source=ROOT/'_bevy/logs/character_input_check.rs';binary=ROOT/'_bevy/logs/character_input_check.exe'
 try:
  source.write_text('\n'.join(lines),encoding='utf-8')
  subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],check=True)
  subprocess.run([str(binary)],check=True)
 finally:
  source.unlink(missing_ok=True);binary.unlink(missing_ok=True);binary.with_suffix('.pdb').unlink(missing_ok=True)

if __name__=='__main__':
 result=generate()
 if '--rust-check' in sys.argv:rust_check(result)
 if '--check' in sys.argv:
  assert json.loads(OUT.read_text(encoding='utf-8'))==result,'Character input differs from original PPC'
  print('Verified',len(result['vectors']),'original character input vectors')
 else:
  OUT.parent.mkdir(parents=True,exist_ok=True);OUT.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print('Captured',len(result['vectors']),'original character input vectors')
