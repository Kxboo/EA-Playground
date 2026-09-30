"""Execute the retail InitializePlayer body and snapshot its direct stores/calls."""
import hashlib, json, random, struct, sys
from pathlib import Path
from tetherball_scene_oracle import SceneEmu
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_player_init_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
GAME, EXISTING, SPAWNED = 0x71000000, 0x71001000, 0x71002000
PLAYER_INFO, CTRL, AI, WORLD, FLAG = 0x71003000, 0x71004000, 0x71005000, 0x71006000, 0x7100c000
EXISTING_CONTROL, SPAWN_CONTROL = 0x7100d000, 0x7100e000


def bits(x): return struct.unpack('>I', struct.pack('>f', x))[0]
def fvec(e, address, n=3): return [e.r32(address + i * 4) for i in range(n)]
def sx(v): return v - 0x100000000 if v & 0x80000000 else v


class PlayerEmu(SceneEmu):
    def step(self,pc):
        word=self.word(pc)
        if word>>26==31 and (word>>1)&1023==183:
            source,a,b=(word>>21)&31,(word>>16)&31,(word>>11)&31
            address=(self.r[a]+self.r[b])&0xffffffff
            self.w32(address,self.r[source]);self.r[a]=address;return pc+4
        return super().step(pc)

def execute(exe, c):
    e = PlayerEmu(exe)
    events = []
    get_index = 0
    e.w32(GAME + 0x210, c['player_count'])
    e.w32(GAME + 0x40, c['session_mode'])
    e.w32(GAME + 0x104, c['ball_handle'])
    e.w32(GAME + 0x120, c['initial_player_handles'][0])
    e.w32(GAME + 0x124, c['initial_player_handles'][1])
    e.w32(GAME + 0x128, c['initial_ai_handles'][0])
    e.w32(GAME + 0x12c, c['initial_ai_handles'][1])
    e.wr(GAME + 0x130, bytes(c['initial_flags']))
    e.w32(0x805e8320 + 0x8c, WORLD)
    e.w32(WORLD + 0x18, 0x71007000)
    e.w32(0x806012ac,0x7100f000)
    e.w32(0x8060204c, FLAG)
    e.wr(FLAG, bytes([c['multiplayer_enable']]))
    e.w32(EXISTING + 0x130, c['existing_identity'][0])
    e.w32(EXISTING + 0x134, c['existing_identity'][1])
    e.w32(EXISTING + 0x124, PLAYER_INFO)
    e.w32(EXISTING + 0x128, EXISTING_CONTROL)
    e.w32(PLAYER_INFO + 0xa8, c['controller_index'])
    e.w32(CTRL + 0, 0x71009000)
    e.w32(SPAWNED + 0x124, PLAYER_INFO)
    e.w32(SPAWNED + 0x128, SPAWN_CONTROL)
    e.w32(PLAYER_INFO + 0xa8, c['controller_index'])
    e.wr(0x7100b000, struct.pack('>3f', *(struct.unpack('>f', struct.pack('>I', x))[0] for x in c['position'])))
    control_to_character = {EXISTING_CONTROL: EXISTING, SPAWN_CONTROL: SPAWNED}

    def get_player(em):
        nonlocal get_index
        get_index += 1
        events.append(['get_player_character', sx(em.r[4])])
        em.r[3] = EXISTING if c['existing'] else 0

    def spawn(em):
        event = ['spawn_character', [em.r[5], em.r[6]], fvec(em, em.r[7]), em.r[8], em.r[9], em.r[10], [sx(em.r32(em.r[1] + 8)), sx(em.r32(em.r[1] + 12))]]
        events.append(event)
        em.r[3] = SPAWNED

    def set_pos(em): events.append(['set_character_state_position', em.r[3] - 0x130, fvec(em, em.r[4])])
    def set_dir(em): events.append(['set_character_state_direction', em.r[3] - 0x130, fvec(em, em.r[4])])
    def malloc(em): events.append(['allocate_ai_slot']); em.r[3] = AI
    def ctor(em): events.append(['construct_tetherball_ai', em.r[3], em.r[4]]); em.r[3] = AI
    def add_ai(em): events.append(['add_ai_entity', em.r[4]])
    def bind_ai(em): events.append(['set_character_ai_entity', control_to_character.get(em.r[3], 0), em.r[4]])
    def multi(em): events.append(['setup_multiplayer_ability'])
    def single(em): events.append(['setup_single_player_ability'])
    def controller_get(em): events.append(['controller_get', sx(em.r[3])]); em.r[3] = CTRL
    def controller_set(em): events.append(['set_controller_state', em.r[3], sx(em.r[4])])

    e.hooks.update({
        0x802d0120: get_player,
        0x802ec308: spawn,
        0x80328104: set_pos,
        0x80336498: set_dir,
        0x803d5894: malloc,
        0x80395290: ctor,
        0x802cd048: add_ai,
        0x802e7910: bind_ai,
        0x8039ce0c: multi,
        0x8039cd50: single,
        0x8032c608: controller_get,
        0x8032dc38: controller_set,
    })
    e.call(0x8039ba00, (GAME, 0x7100b000, *c['identity']), (struct.unpack('>f', struct.pack('>I', c['heading']))[0],))
    slot = c['player_count'] if c['player_count'] < 2 else None
    return {
        'input': c,
        'events': events,
        'player_count': e.r32(GAME + 0x210),
        'session_mode': e.r32(GAME + 0x40),
        'player_handles': [e.r32(GAME + 0x120 + i * 4) for i in range(2)],
        'ai_handles': [e.r32(GAME + 0x128 + i * 4) for i in range(2)],
        'flags_130': list(e.rd(GAME + 0x130, 2)),
        'ai_ball_handle_60': e.r32(AI + 0x60),
        'active_slot': slot,
    }


def generate():
    assert hashlib.sha256((ROOT / 'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    exe = rf.load()
    rng = random.Random(0x8039ba00)
    cases = []
    scenarios = [
        # Fresh spawn, reuse by identity, mismatch-and-spawn; both ability arms.
        (False, [0, 0], 0, 0x80410000),
        (True, [0x11223344, 0x55667788], 1, 0x7fffffff),
        (True, [0x11223344, 0x55667789], 0, 0xffffffff),
    ]
    for count in (0, 1):
        for idx, (existing, old_id, multi, session) in enumerate(scenarios):
            c = dict(player_count=count, session_mode=session, ball_handle=0x12340000 + count,
                     initial_player_handles=[0xaaaa0000, 0xbbbb0000], initial_ai_handles=[0xcccc0000, 0xdddd0000],
                     initial_flags=[0xa5, 0x5a], existing=existing, existing_identity=old_id,
                     identity=[0x11223344, 0x55667788], multiplayer_enable=multi,
                     controller_index=idx - 1, heading=bits([-7.0, -3.1415927, 0.0, 8.0, 19.0, 1.25][count * 3 + idx]),
                     position=[bits(rng.uniform(-20, 20)) for _ in range(3)])
            cases.append(execute(exe, c))
    # Guard must make no service call or store, independent of the +0x40 word.
    for count, sess in ((2, 0), (3, 0xffffffff)):
        c = dict(player_count=count, session_mode=sess, ball_handle=0x12345678,
                 initial_player_handles=[0x11, 0x22], initial_ai_handles=[0x33, 0x44],
                 initial_flags=[0x55, 0x66], existing=True, existing_identity=[1, 2],
                 identity=[1, 2], multiplayer_enable=1, controller_index=0,
                 heading=bits(-9.0), position=[bits(1.0), bits(2.0), bits(3.0)])
        cases.append(execute(exe, c))
    return {'elf_sha256': SHA, 'function': 'InitializePlayer__12MGTetherballFPC9rmVector3fUx', 'cases': cases}


if __name__ == '__main__':
    value = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == value, 'InitializePlayer differs from original PPC'
        print('Verified original InitializePlayer:', len(value['cases']), 'cases')
    else:
        OUT.parent.mkdir(parents=True, exist_ok=True)
        OUT.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8', newline='\n')
        print('Captured original InitializePlayer:', len(value['cases']), 'cases')
