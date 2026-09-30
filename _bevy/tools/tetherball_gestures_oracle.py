"""Execute retail tetherball gesture callbacks and hit-attempt sampling."""
import hashlib
import json
import random
import struct
import sys
from pathlib import Path

from ppc_emu2 import Emu, rf, sx

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_gestures_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
MG = 0x71000000
PLAYER_BASE = 0x72000000
CHAR_BASE = 0x73000000
CTRL_BASE = 0x74000000
EVENT_BASE = 0x75000000
OUT_BASE = 0x76000000
BALL_BASE = 0x77000000
GLOBAL_GAME_PTR = 0x805E83AC

CALLBACKS = {
    'regular': 0x8039c884,
    'reverse': 0x8039c970,
    'overhand': 0x8039c9fc,
    'serve_toss': 0x8039ca88,
}


class GestureEmu(Emu):
    def __init__(self, exe=None):
        super().__init__(exe)
        self.events = {}
        self.track_action_writes = False
        self.action_writes = []
        self.hooks[0x8032c608] = self.controller_get
        self.hooks[0x8032d9b0] = self.event_get

    def w32(self, address, value):
        super().w32(address, value)
        if self.track_action_writes and MG + 0x22c <= address < MG + 0x22c + 4 * 8 and (address - (MG + 0x22c)) % 4 == 0:
            self.action_writes.append(((address - (MG + 0x22c)) // 4, sx(value, 32)))

    def controller_get(self, emu):
        emu.r[3] = CTRL_BASE + (emu.r[3] & 0xff) * 0x100

    def event_get(self, emu):
        action = emu.r[4]
        controller_id = (emu.r[3] - CTRL_BASE) // 0x100
        ptr = EVENT_BASE + (action & 0xff) * 4
        emu.wr(ptr, bytes([int(bool(self.events.get((controller_id, action), False)))]))
        emu.r[3] = ptr


def write_initial(em, case):
    em.wr(MG, bytes(0x500))
    em.wr(BALL_BASE, bytes(0x170))
    em.w32(MG + 0x104, BALL_BASE)
    em.w32(MG + 0x210, case['player_count'])
    em.w32(MG + 0x21c, case['forward_player'])
    em.w32(MG + 0x220, case['reverse_player'])
    em.w32(MG + 0x2e4, len(case['pending']))
    em.wr(MG + 0x424, bytes([int(case['callbacks_enabled'])]))
    em.wr(MG + 0x42e, bytes([int(case['mega_ability'])]))
    em.wr(MG + 0x229, bytes([int(case['hit_attempt_marker'])]))
    em.w32(MG + 0x214, case['forward_player'])
    em.wr(GLOBAL_GAME_PTR, struct.pack('>I', MG))
    em.wr(MG + 0x24, bytes([int(case['paused'])]))

    em.wr(PLAYER_BASE, bytes(0x400))
    em.wr(CHAR_BASE, bytes(0x800))
    em.wr(CTRL_BASE, bytes(0x800))
    for i, player in enumerate(case['players']):
        char = CHAR_BASE + i * 0x100
        em.w32(MG + 0x120 + i * 4, char)
        em.w32(MG + 0x22c + i * 4, player['action'])
        if player['controller_id'] is None:
            em.w32(char + 0x124, 0)
        else:
            controller = CTRL_BASE + i * 0x100
            em.w32(char + 0x124, controller)
            em.w32(controller + 0xa8, player['controller_id'])
    for i, record in enumerate(case['pending']):
        base = MG + 0x284 + i * 12
        em.w32(base, record['kind'])
        em.w32(base + 4, record['aux_bits'])
        em.w32(base + 8, record['controller_id'])
    em.wr(OUT_BASE, bytes(16))


def read_state(em, attempt_result=None):
    count = sx(em.r32(MG + 0x2e4), 32)
    records = []
    if 0 <= count <= 7:
        for i in range(count):
            base = MG + 0x284 + i * 12
            records.append({'kind': sx(em.r32(base), 32), 'aux_bits': em.r32(base + 4),
                            'controller_id': sx(em.r32(base + 8), 32)})
    players = []
    player_count = sx(em.r32(MG + 0x210), 32)
    for i in range(max(0, min(player_count, 8))):
        players.append({'controller_id': None, 'action': sx(em.r32(MG + 0x22c + i * 4), 32)})
    result = {'pending_count': count, 'pending': records, 'players': players,
              'hit_attempt_marker': bool(em.rd(MG + 0x229, 1)[0])}
    if attempt_result is not None:
        result['attempt'] = attempt_result
    return result


def snapshot_players(em, case):
    count = len(case['players'])
    return [{'controller_id': p['controller_id'], 'action': sx(em.r32(MG + 0x22c + i * 4), 32)}
            for i, p in enumerate(case['players'][:count])]


def state_after(em, case, attempt_result=None):
    count = sx(em.r32(MG + 0x2e4), 32)
    records = []
    if 0 <= count <= 7:
        for i in range(count):
            base = MG + 0x284 + i * 12
            records.append({'kind': sx(em.r32(base), 32), 'aux_bits': em.r32(base + 4),
                            'controller_id': sx(em.r32(base + 8), 32)})
    return {'pending_count': count, 'pending': records,
            'players': snapshot_players(em, case),
            'hit_attempt_marker': bool(em.rd(MG + 0x229, 1)[0]),
            **({'attempt': attempt_result} if attempt_result is not None else {})}


def run_op(em, case, op):
    if op['op'] == 'callback':
        output = OUT_BASE + 0x100
        em.wr(output, bytes(12))
        em.w32(output + 8, op['controller_id'])
        em.call(CALLBACKS[op['callback']], (output, MG), max_steps=1000)
        return None
    if op['op'] == 'process':
        em.action_writes = []
        em.track_action_writes = True
        try:
            em.call(0x8039b674, (MG,), max_steps=1000)
        finally:
            em.track_action_writes = False
        return {'effects': [list(effect) for effect in em.action_writes]}
    if op['op'] == 'hit_attempt':
        active_player = op['active_player']
        em.w32(MG + 0x214, active_player)
        em.wr(MG + 0x42e, bytes([int(op['mega_ability'])]))
        em.wr(MG + 0x22c + active_player * 4, struct.pack('>I', op['player_action'] & 0xffffffff))
        em.w32(BALL_BASE + 0xac, op['ball_zone'])
        controller_id = case['players'][active_player]['controller_id']
        em.events = {(controller_id, 0x5c): op['normal'], (controller_id, 0x5d): op['reverse']}
        em.wr(OUT_BASE, bytes([int(op['normal_out']), int(op['reverse_out'])]))
        em.w32(OUT_BASE + 4, op['power_type'])
        em.call(0x8039b76c, (MG, op['argument_player'], OUT_BASE, OUT_BASE + 1, OUT_BASE + 4), max_steps=1000)
        result = {'normal': bool(em.rd(OUT_BASE, 1)[0]),
                  'reverse': bool(em.rd(OUT_BASE + 1, 1)[0]),
                  'power_type': sx(em.r32(OUT_BASE + 4), 32)}
        return result
    raise ValueError(op)


def generate():
    assert hashlib.sha256((ROOT / 'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    exe = rf.load()
    rng = random.Random(0x8039b674)
    cases = []

    def run_case(case, ops):
        em = GestureEmu(exe)
        write_initial(em, case)
        steps = []
        for op in ops:
            detail = run_op(em, case, op)
            expected = state_after(em, case, detail if op['op'] == 'hit_attempt' else None)
            if op['op'] == 'process':
                expected['effects'] = detail['effects']
            steps.append({'operation': op, 'expected': expected})
        cases.append({'initial': case, 'steps': steps})

    for ci in range(8):
        player_count = 1 + rng.randrange(4)
        ids = [rng.choice([None, 0, 1, 2, 3, 4]) for _ in range(player_count)]
        case = {
            'paused': bool(rng.randrange(2)), 'callbacks_enabled': bool(rng.randrange(2)),
            'mega_ability': bool(rng.randrange(2)), 'hit_attempt_marker': bool(rng.randrange(2)),
            'player_count': player_count,
            'players': [{'controller_id': ids[i], 'action': rng.choice([0, 1, 2, -1])}
                        for i in range(player_count)],
            'forward_player': rng.randrange(player_count), 'reverse_player': rng.randrange(player_count),
            'pending': [{'kind': rng.choice([0, 0, 1, 2]), 'aux_bits': rng.choice([0, 0x80000000, rng.getrandbits(32)]),
                         'controller_id': rng.choice([0, 1, 2, 3, 4, -1])}
                        for _ in range(rng.randrange(8))],
        }
        # Include queue-cap pressure, unmatched players, both hit actions, and pause/callback gates.
        if ci < 16:
            case['paused'] = bool(ci & 1)
            case['callbacks_enabled'] = not bool(ci & 2)
            case['players'] = [{'controller_id': 7 if i == 0 else i - 1,
                                'action': [0, 1, 2, -1][i % 4]} for i in range(4)]
            case.update(player_count=4, forward_player=0, reverse_player=1,
                        pending=[{'kind': i % 3, 'aux_bits': i * 0x1020304, 'controller_id': i % 4}
                                 for i in range(ci % 8)])
        player_count = case['player_count']
        ops = []
        callback_cycle = ['regular', 'reverse', 'overhand', 'serve_toss']
        rng.shuffle(callback_cycle)
        for kind in callback_cycle[:3 + (ci % 2)]:
            ops.append({'op': 'callback', 'callback': kind,
                        'controller_id': rng.choice([0, 1, 2, 3, 4, -1])})
            if rng.randrange(2) == 0:
                ops.append({'op': 'process'})
        ops.append({'op': 'process'})
        pi = ci % player_count
        ops.append({'op': 'hit_attempt', 'active_player': pi,
                    'argument_player': (pi + 1) % player_count,
                    'player_action': 0 if ci % 2 == 0 else 1,
                    'ball_zone': ci % 2, 'normal': bool(ci & 1),
                    'reverse': bool(ci & 2), 'normal_out': bool(ci & 4),
                    'reverse_out': bool(ci & 1), 'power_type': ci % 4,
                    'mega_ability': case['mega_ability']})
        run_case(case, ops)

    # Target accepted coverage for every native callback, with a consuming call
    # after each append so each original wrapper independently crosses its gates.
    accepted = {
        'paused': False, 'callbacks_enabled': True, 'mega_ability': False,
        'hit_attempt_marker': False, 'player_count': 4,
        'players': [{'controller_id': 7, 'action': 3}, {'controller_id': 8, 'action': -1},
                    {'controller_id': 2, 'action': 2}, {'controller_id': 9, 'action': 1}],
        'forward_player': 0, 'reverse_player': 1, 'pending': [],
    }
    accepted_ops = []
    for callback, controller_id in [('regular', 7), ('reverse', 8), ('overhand', 90), ('serve_toss', 91)]:
        accepted_ops.extend([{'op': 'callback', 'callback': callback, 'controller_id': controller_id},
                             {'op': 'process'}])
    run_case(accepted, accepted_ops)

    # Regular/reverse callbacks must reject a mismatched controller ID.
    wrong_controller = dict(accepted, pending=[])
    wrong_controller_ops = [
        {'op': 'callback', 'callback': 'regular', 'controller_id': 8},
        {'op': 'callback', 'callback': 'reverse', 'controller_id': 7},
        {'op': 'process'},
    ]
    run_case(wrong_controller, wrong_controller_ops)

    # Every callback is rejected at the original seven-record limit.
    full_queue = dict(accepted, pending=[{'kind': 2, 'aux_bits': 0, 'controller_id': 100 + i}
                                         for i in range(7)])
    full_queue_ops = [{'op': 'callback', 'callback': name, 'controller_id': controller_id}
                      for name, controller_id in [('regular', 7), ('reverse', 8), ('overhand', 90), ('serve_toss', 91)]]
    full_queue_ops.append({'op': 'process'})
    run_case(full_queue, full_queue_ops)

    # Three players share controller 9. The PPC consumer's queue-major,
    # player-minor stores and last-record-wins behavior are captured directly.
    duplicates = dict(accepted, players=[{'controller_id': 9, 'action': -1},
                                         {'controller_id': 9, 'action': 2},
                                         {'controller_id': 2, 'action': 3},
                                         {'controller_id': 9, 'action': 1}],
                      player_count=4, forward_player=0, reverse_player=1, pending=[])
    duplicate_ops = [
        {'op': 'callback', 'callback': 'regular', 'controller_id': 9},
        {'op': 'callback', 'callback': 'serve_toss', 'controller_id': 9},
        {'op': 'callback', 'callback': 'overhand', 'controller_id': 9},
        {'op': 'process'},
    ]
    run_case(duplicates, duplicate_ops)

    # Full zone/action/event/ability cross product; every sample calls the
    # original GetPlayerHitAttempt with an intentionally different r4 player.
    matrix = dict(accepted, players=[{'controller_id': 7, 'action': 1},
                                     {'controller_id': 8, 'action': -1}],
                  player_count=2, forward_player=0, reverse_player=1, pending=[])
    matrix_ops = []
    for zone in (0, 1):
        for action in (0, 1):
            for normal, reverse in ((False, False), (False, True), (True, False), (True, True)):
                for ability in (False, True):
                    ordinal = len(matrix_ops)
                    matrix_ops.append({
                        'op': 'hit_attempt', 'active_player': 0, 'argument_player': 1,
                        'player_action': action, 'ball_zone': zone, 'normal': normal,
                        'reverse': reverse, 'normal_out': bool(ordinal & 1),
                        'reverse_out': bool(ordinal & 2), 'power_type': ordinal % 4,
                        'mega_ability': ability,
                    })
    run_case(matrix, matrix_ops)
    return {'elf_sha256': SHA, 'cases': cases}


if __name__ == '__main__':
    value = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == value
        transitions = sum(len(c['steps']) for c in value['cases'])
        print(f'Tetherball gestures: {transitions} original callback/consume/input transitions match')
    else:
        OUT.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8', newline='\n')
        print('Wrote', OUT)
