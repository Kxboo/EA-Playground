"""Execute retail MGTetherball::InitializeAI with only engine calls hooked."""
import hashlib, json, random, struct, sys
from pathlib import Path
import re_functions as rf
from tetherball_scene_oracle import SceneEmu

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_ai_init_golden.json'
SHA='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
GAME=0x71000000;WORLD=0x71006000;CHARACTER=0x71002000;CHAR_CONTROL=0x71003000;AI=0x71005000;POSITION=0x7100b000
SLOT=0x7100d000

def u32(x):return x&0xffffffff
def sx(x):return x-0x100000000 if x&0x80000000 else x
def bitsf(x):return struct.unpack('>I',struct.pack('>f',x))[0]
def vec(e,address):return [e.r32(address+i*4) for i in range(3)]

def execute(exe,c):
    e=SceneEmu(exe);events=[]
    e.w32(GAME+0x210,c['player_count']);e.w32(GAME+0x40,c['session_mode']);e.w32(GAME+0x104,c['ball_handle'])
    for i,x in enumerate(c['player_handles']):e.w32(GAME+0x120+i*4,x)
    for i,x in enumerate(c['ai_handles']):e.w32(GAME+0x128+i*4,x)
    e.wr(GAME+0x130,bytes(c['flags_130']))
    e.w32(0x805e8320+0x8c,WORLD);e.w32(WORLD+0x18,0x71007000);e.w32(0x806012ac,SLOT)
    e.w32(CHARACTER+0x128,c['control_handle'])
    e.wr(POSITION,struct.pack('>3I',*c['position_bits']))
    char_control=CHARACTER+0x128

    def spawn(em):
        events.append(['spawn_character',em.r[5],em.r[6],vec(em,em.r[7]),em.r[8],em.r[9],em.r[10],
                       [sx(em.r32(em.r[1]+8)),sx(em.r32(em.r[1]+12))]])
        em.r[3]=CHARACTER
    def allocate(em):events.append(['allocate_ai_slot']);em.r[3]=c['ai_entity_handle']
    def construct(em):events.append(['construct_tetherball_ai',em.r[3],em.r[4]]);em.r[3]=c['ai_entity_handle']
    def register(em):events.append(['add_ai_entity',em.r[4]])
    def bind(em):
        assert em.r[3]==c['control_handle'],'SetAIEntity must receive the character control pointer loaded by native code'
        events.append(['set_ai_control_entity',em.r[3],em.r[4]])
    def set_direction(em):
        assert em.r[3]==CHARACTER+0x130,'SetDir must receive CharacterState at character+0x130'
        events.append(['set_character_state_direction',CHARACTER,vec(em,em.r[4])])
    e.hooks.update({0x802ec308:spawn,0x803d5894:allocate,0x80395290:construct,
                    0x802cd048:register,0x802e7910:bind,0x80336498:set_direction})
    e.call(0x8039bbec,(GAME,POSITION,*c['identity_words']),
           (struct.unpack('>f',struct.pack('>I',c['heading_bits']))[0],))
    return {'input':c,'events':events,'player_count':e.r32(GAME+0x210),'session_mode':e.r32(GAME+0x40),
            'player_handles':[e.r32(GAME+0x120+i*4) for i in range(2)],
            'ai_handles':[e.r32(GAME+0x128+i*4) for i in range(2)],
            'flags_130':list(e.rd(GAME+0x130,2)),'ai_ball_handle_60':e.r32(c['ai_entity_handle']+0x60)}

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load();rng=random.Random(0x8039bbec);cases=[]
    headings=[-25.132741, -6.2831855,-3.1415927,-0.0,0.0,1.5707964,8.0,19.0,100.0]
    for count in (0,1):
        for i in range(9):
            c={'player_count':count,'session_mode':rng.choice([-1,0,1,2,0x7fffffff]),
               'ball_handle':0x12340000+count,'player_handles':[0xaaaa0000,0xbbbb0000],
               'ai_handles':[0xcccc0000,0xdddd0000],'flags_130':[0xa5,0x5a],
               'control_handle':CHAR_CONTROL,
               'ai_entity_handle': AI,
               'identity_words':[rng.getrandbits(32),rng.getrandbits(32)],
               'position_bits':[bitsf(rng.uniform(-20,20)) for _ in range(3)],'heading_bits':bitsf(headings[i])}
            cases.append(execute(exe,c))
    # The signed native count comparison is covered on its safe nonnegative
    # memory domain; counts >=2 return before every host call and store.
    for count in (2,3,255):
        c={'player_count':count,'session_mode':rng.choice([-1,0,1,2,0x7fffffff]),
           'ball_handle':0x12345678,'player_handles':[0x11111111,0x22222222],
           'ai_handles':[0x33333333,0x44444444],'flags_130':[0x55,0x66],
           'control_handle':CHAR_CONTROL,'ai_entity_handle':AI,'identity_words':[1,2],
           'position_bits':[bitsf(1.0),bitsf(2.0),bitsf(3.0)],'heading_bits':bitsf(-9.0)}
        cases.append(execute(exe,c))
    return {'elf_sha256':SHA,'function':'InitializeAI__12MGTetherballFPC9rmVector3fUx',
            'address':'0x8039bbec','engine_boundary':'SpawnCharacter, AI slot allocation/constructor/registration/binding, and CharacterState::SetDir are hooked; native math and all game-owned stores execute.',
            'cases':cases}

if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8'))==value,'fixture differs from retail InitializeAI execution'
        print(f'PASS {len(value["cases"])} original InitializeAI calls')
    else:OUT.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8',newline='\n')
