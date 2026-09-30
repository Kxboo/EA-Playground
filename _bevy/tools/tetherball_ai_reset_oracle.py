"""ResetRound with original AI constructors and Initialize executed, not supplied."""
import copy,hashlib,json,sys
from pathlib import Path
from tetherball_reset_oracle import ResetEmu,GAME,NEW_AI,CHARS,SHA
from tetherball_tuning_oracle import Oracle,load,AI as FIELDS,OBJ as TUNING_GAME
import re_functions as rf
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'_bevy/tests/data/tetherball_ai_reset_golden.json'

def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest()==SHA
    exe=rf.load();db,hashes=load();dbemu=Oracle(exe,db)
    base=json.loads((ROOT/'_bevy/tests/data/tetherball_reset_golden.json').read_text(encoding='utf-8'))
    e=ResetEmu(exe);e.hooks.pop(0x80395354)
    e.hooks[0x80319a54]=lambda em:em.r.__setitem__(3,GAME)
    # Database accessor is a real corpus-backed service. Its implementation
    # runs on a second original-code emulator, preserving the caller registers.
    def ai_byte(em):
        name=em.cstring(em.r[4]);assert name in FIELDS
        dbemu.w32(TUNING_GAME+0x40,em.r32(GAME+0x40));dbemu.w32(TUNING_GAME+0x48,em.r32(GAME+0x48))
        dbemu.collection=dbemu.cstr(dbemu.call(0x8039ce38,[TUNING_GAME]))
        dbemu.wr(0x77000000,name.encode('ascii')+b'\0')
        em.r[3]=dbemu.call(0x802f4df0,[0x73000000,0x77000000,em.r[5]])
    e.hooks[0x802f4df0]=ai_byte
    cases=[]
    for i,seed in enumerate(c for c in base['cases'] if c['operation']=='round'):
        c=copy.deepcopy(seed);c['initial']['session_mode']=1;c['initial']['game_type']=[-1,0,3,6][i%4]
        e.write_reset(c['initial'],dict(mode=0,rotation_limit=6,wins_required=2,time_limit_seconds=30000),c['ball'],c['aux'],c['inputs'])
        # The allocation service returns objects whose original constructors
        # have executed. Unwritten storage has explicit zero seed in this corpus.
        for player,pointer in enumerate(NEW_AI):
            e.wr(pointer+0x6c,b'\x01')
            e.call(0x80395290,[pointer,CHARS[player]])
        e.call(0x803994f4,[GAME])
        c['expected'],c['expected_ball'],c['expected_aux']=e.read_reset(c['initial'])
        c['effects']=copy.deepcopy(e.events)
        c['ai']=[dict(ball=e.r32(p+0x60),angle=e.r32(p+0x64),scale=e.r32(p+0x68),charge=e.r32(p+0x70),difficulty=list(e.rd(p+0x78,7)),enabled=bool(e.rd(p+0x7f,1)[0]),heading=e.r32(p+0x80),waiting=e.rd(p+0x6c,1)[0]) for p in NEW_AI]
        assert all(ai['waiting']==1 for ai in c['ai'])
        cases.append(c)
    return dict(elf_sha256=SHA,corpus_sha256=hashes,handles=base['handles'],camera_globals=base['camera_globals'],cases=cases)
if __name__=='__main__':
    value=generate()
    if '--check' in sys.argv:assert json.loads(OUT.read_text(encoding='utf-8'))==value
    else:OUT.write_text(json.dumps(value,indent=1)+'\n',encoding='utf-8',newline='\n')
    print('Verified',len(value['cases']),'original resets with AI construction and initialization')
