"""Original PPC movement/context execution; frsqrte instruction model, no movement hooks."""
import hashlib,json,math,random,struct,sys
from pathlib import Path
from character_input_oracle import InputEmu,fbits,SHA,ROOT
from ppc_emu2 import f32,sx
import re_functions as rf
OUT=ROOT/'_bevy/tests/data/character_movement_golden.json'
# Numeric hardware estimate data reported by Dolphin 2506 FloatUtils.cpp.
# Source attribution and instruction-model limitations: docs/CHARACTER_MOVEMENT.md.
BASE=[0x1a7e800,0x17cb800,0x1552800,0x130c000,0x10f2000,0x0eff000,0x0d2e000,0x0b7c000,0x09e5000,0x0867000,0x06ff000,0x05ab800,0x046a000,0x0339800,0x0218800,0x0105800,0x3ffa000,0x3c29000,0x38aa000,0x3572000,0x3279000,0x2fb7000,0x2d26000,0x2ac0000,0x2881000,0x2665000,0x2468000,0x2287000,0x20c1000,0x1f12000,0x1d79000,0x1bf4000]
DEC=[0x568,0x4f3,0x48d,0x435,0x3e7,0x3a2,0x365,0x32e,0x2fc,0x2d0,0x2a8,0x283,0x261,0x243,0x226,0x20b,0x7a4,0x700,0x670,0x5f2,0x584,0x524,0x4cc,0x47e,0x43a,0x3fa,0x3c2,0x38e,0x35e,0x332,0x30a,0x2e6]

def estimate(x):
 assert x>0 and math.isfinite(x)
 bits=struct.unpack('>Q',struct.pack('>d',x))[0];exponent=(bits>>52)&2047
 assert exponent!=0,'subnormal double outside validated instruction domain'
 index=((exponent&1)<<15)|((bits&((1<<52)-1))>>37)
 out_exponent=(3068-exponent)//2
 mantissa=(BASE[index>>11]-DEC[index>>11]*(index&2047))<<26
 return struct.unpack('>d',struct.pack('>Q',(out_exponent<<52)|mantissa))[0]

class MovementEmu(InputEmu):
 def step(self,pc):
  w=self.word(pc);op=w>>26;d,a,b=(w>>21)&31,(w>>16)&31,(w>>11)&31;xo=(w>>1)&1023
  if op in (46,47):
   address=(self.r[a] if a else 0)+sx(w,16)
   for reg in range(d,32):
    if op==46:self.r[reg]=self.r32(address+4*(reg-d))
    else:self.w32(address+4*(reg-d),self.r[reg])
   return pc+4
  if op==63 and xo==26:self.f[d]=estimate(self.f[b]);return pc+4
  if op==59:
   low=xo&31;c=(w>>6)&31
   if low in (28,29,30,31):
    x,y,z=self.f[a],self.f[b],self.f[c]
    self.f[d]=f32(math.fma(x,z,-y) if low==28 else math.fma(x,z,y) if low==29 else -math.fma(x,z,-y) if low==30 else -math.fma(x,z,y));return pc+4
  return super().step(pc)

def capture(exe,v):
 em=MovementEmu(exe);inp=0x71000000;out=0x71001000;ctx=0x71002000;manager=0x71003000;ground=0x71004000;air=0x71005000
 def vec(a,x):em.wr(a,struct.pack('>'+'f'*len(x),*x))
 if v['kind']=='utility':
  for off,key in [(0,'gain'),(0x60,'max_acceleration')]:vec(inp+off,[v[key]])
  for off,key in [(0x10,'forward'),(0x20,'up'),(0x30,'normal'),(0x40,'velocity'),(0x50,'desired'),(0x70,'surface_velocity')]:vec(inp+off,v[key])
  vec(out,v['initial_output']);em.call(0x801dd11c,(inp,out));state=None
 else:
  vec(inp,[v['lr'],v['ud']]);em.wr(inp+8,b'\0');em.wr(inp+0x30,bytes([0,v['supported']]))
  for off,key in [(0x10,'up'),(0x20,'forward'),(0x40,'normal'),(0x50,'surface_velocity'),(0x80,'velocity'),(0x90,'gravity')]:vec(inp+off,v[key])
  vec(inp+0x60,[0,0,v['seconds'],1/v['seconds'] if v['seconds'] else 0])
  em.w32(ctx+8,manager);em.w32(ctx+0xc,v['state']);em.w32(manager+8,ground);em.w32(manager+16,air)
  em.w32(ground,0x80481b18);em.w32(air,0x80481a98)
  vec(ground+8,[v['ground_gain'],v['ground_speed']]);vec(air+8,[v['air_gain'],v['air_speed']]);em.wr(ground+0x10,bytes(v['ground_flags']))
  em.call(0x801dc3a0,(ctx,inp,out));state=em.r32(ctx+0xc)
 return dict(v,output_bits=[em.r32(out+i*4) for i in range(4)],next_state=state)

def generate():
 assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
 exe=rf.load();cases=[]
 utility=dict(kind='utility',gain=1,forward=[0,0,-1,0],up=[0,1,0,0],normal=[0,1,0,0],velocity=[0,0,0,0],desired=[5,0,0,0],surface_velocity=[0,0,0,0],max_acceleration=100,initial_output=[7,8,9,0])
 state=dict(kind='state',state=0,ground_gain=1,ground_speed=20,air_gain=1,air_speed=20,ground_flags=[1,0,0],lr=0,ud=0.25,supported=True,seconds=0.016,forward=[0,0,-1,0],up=[0,1,0,0],normal=[0,1,0,0],velocity=[0,0,0,0],surface_velocity=[0,0,0,0],gravity=[0,-9.81,0,1])
 for s in [0,2]:
  for supported in [False,True]:
   for dt in [0,0.001,0.016,0.06,0.2]:
    for desired in [[0,0],[0,0.25],[0.5,0.5],[-1,-1]]:
     cases.append(dict(state,state=s,supported=supported,seconds=dt,lr=desired[0],ud=desired[1]))
 for f in [[0,0,-1,0],[0,1,0,0],[0,0,0,0],[0.0001,0,0,0]]:cases.append(dict(utility,forward=f))
 for f in [[0,1,0,0],[0,0,0,0]]:
  for s in [0,2]:cases.append(dict(state,forward=f,state=s,velocity=[1,2,3,0]))
 for normal in [[0,0,0,0],[0,0.8,0.6,0],[0,-1,0,0]]:cases.append(dict(utility,normal=normal))
 cases.append(dict(utility,up=[1,0,0,0],normal=[1,0,0,0],velocity=[1,2,3,4],surface_velocity=[0.5,0.2,-0.1,1]))
 for s in [0,2]:cases.append(dict(state,state=s,supported=s==0,up=[1,0,0,0],normal=[1,0,0,0],gravity=[-9.81,0,0,1],velocity=[1,2,3,4],surface_velocity=[0.5,0.2,-0.1,1]))
 for gain in [0,0.05,0.5,2]:
  for s in [0,2]:cases.append(dict(state,state=s,supported=s==0,ground_gain=gain,air_gain=gain,ground_speed=7,air_speed=13,velocity=[1,-2,3,0]))
 rng=random.Random(0x801dd11c)
 for i in range(160):
  angle=rng.uniform(-3.14,3.14);slope=rng.uniform(-0.7,0.7)
  forward=[math.sin(angle),0,math.cos(angle),0];normal=[slope,math.sqrt(1-slope*slope),0,0]
  common=dict(forward=forward,normal=normal,velocity=[rng.uniform(-20,20) for _ in range(3)]+[0],surface_velocity=[rng.uniform(-3,3) for _ in range(3)]+[0])
  if i%2:cases.append(dict(utility,**common,gain=rng.choice([0,0.05,0.5,1,2]),desired=[rng.uniform(-20,20),rng.uniform(-20,20),0,0],max_acceleration=rng.choice([0,1,10,100])))
  else:cases.append(dict(state,**common,state=rng.choice([0,2]),supported=rng.choice([True,False]),seconds=rng.choice([0.001,0.016,0.06,0.2]),lr=rng.uniform(-1,1),ud=rng.uniform(-1,1),ground_flags=rng.choice([[1,0,0],[0,0,0],[1,1,0],[1,0,1]])))
 return dict(elf_sha256=SHA,estimate_source='dolphin-emu/dolphin tag 2506 FloatUtils.cpp numeric table',vectors=[capture(exe,v) for v in cases])

def rust_check(result):
 import subprocess
 def scalar(x):return f'f32::from_bits({fbits(x)})'
 def array(xs):return '['+','.join(scalar(x) for x in xs)+']'
 lines=['#[path='+json.dumps(str(ROOT/'_bevy/src/character_input.rs'))+'] mod character_input;',
  '#[path='+json.dumps(str(ROOT/'_bevy/src/character_movement.rs'))+'] mod character_movement;',
  'use character_input::*;use character_movement::*;fn main(){']
 for index,v in enumerate(result['vectors']):
  if v['kind']=='utility':
   fields=','.join(k+':'+scalar(v[k]) for k in ['gain','max_acceleration'])+','+','.join(k+':'+array(v[k]) for k in ['forward','up','normal','velocity','desired','surface_velocity'])
   lines.append('let out=calculate_movement(MovementInput{'+fields+'},'+array(v['initial_output'])+');')
  else:
   state='MovementState::Grounded' if v['state']==0 else 'MovementState::InAir'
   config=','.join(k+':'+scalar(v[k]) for k in ['ground_gain','ground_speed','air_gain','air_speed'])+',ground_flags:['+','.join(str(bool(x)).lower() for x in v['ground_flags'])+']'
   lines.append('let mut state=CharacterMovementState{state:'+state+',config:MovementConfig{'+config+'}};')
   fields='input_lr:'+scalar(v['lr'])+',input_ud:'+scalar(v['ud'])+',want_jump:false,at_ladder:false,supported:'+str(v['supported']).lower()+',position:[0.;4],surface_normal:'+array(v['normal'])+',step_info:'+array([0,0,v['seconds'],1/v['seconds'] if v['seconds'] else 0])+','+','.join(k+':'+array(v[k]) for k in ['forward','up','velocity','surface_velocity','gravity'])
   lines.append('let result=state.update(&CharacterInput{'+fields+'});let out=result.velocity;assert_eq!(result.state,'+('MovementState::Grounded' if v['next_state']==0 else 'MovementState::InAir')+',"state '+str(index)+'");')
  lines.append('assert_eq!(out.map(f32::to_bits),'+str(v['output_bits'])+',"output '+str(index)+'");')
 lines.append('println!("Matched '+str(len(result['vectors']))+' original movement vectors");}')
 source=ROOT/'_bevy/logs/character_movement_check.rs';binary=source.with_suffix('.exe')
 try:
  source.write_text('\n'.join(lines),encoding='utf-8');subprocess.run(['rustc','--edition=2024',str(source),'-o',str(binary)],check=True);subprocess.run([str(binary)],check=True)
 finally:
  source.unlink(missing_ok=True);binary.unlink(missing_ok=True);binary.with_suffix('.pdb').unlink(missing_ok=True)

if __name__=='__main__':
 result=generate()
 if '--rust-check' in sys.argv:rust_check(result)
 if '--check' in sys.argv:
  assert json.loads(OUT.read_text(encoding='utf-8'))==result,'Movement differs from original PPC under documented instruction model'
  print('Verified',len(result['vectors']),'original PPC movement vectors')
 else:
  OUT.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print('Captured',len(result['vectors']),'original PPC movement vectors')
