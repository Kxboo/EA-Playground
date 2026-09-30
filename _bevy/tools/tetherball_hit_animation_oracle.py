"""Run the original PowerPC tetherball hit/ready animation routines.

Only engine/controller services are intercepted. InHitAnimation,
StartReadyAnimation, StartHitAnimation, CheckForWaitingCharacterSwing,
IsBallInHitRange and all rmAngle helpers execute from the retail ELF.
"""
import copy
import hashlib
import json
import random
import sys
from pathlib import Path

from ppc_emu2 import sx
from tetherball_lifecycle_oracle import LifecycleEmu, GAME, CHARS, ANIM, OBJ, bits
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_hit_animation_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
EVENT_PTR = 0x71500000
CONTROLLER_PTR = 0x71200000
AUDIO_PTR = 0x71501000
IN_HIT = 0x8039a8cc
START_HIT = 0x80399f88
START_READY = 0x8039a178
CHECK_WAITING = 0x80399db8
IN_RANGE = 0x80399cb0
RULES = dict(mode=0, rotation_limit=6, wins_required=2, time_limit_seconds=30000)


class HitEmu(LifecycleEmu):
    """Add IsBetween compare ops and dispatch native tail-called services."""
    def step(self, pc):
        # StartReadyAnimation tail-branches to SetNextAnimState. The base
        # emulator hooks BL calls only, so dispatch a service at its entry too.
        if pc in self.hooks:
            self.hooks[pc](self)
            return self.lr
        word = self.word(pc)
        opcode = word >> 26
        xo = (word >> 1) & 0x3ff
        if opcode == 63 and xo == 32:  # fcmpu crfD,frA,frB
            field = (word >> 23) & 7
            a = self.f[(word >> 16) & 31]
            b = self.f[(word >> 11) & 31]
            self.cr[field] = 8 if a < b else 4 if a > b else 2 if a == b else 1
            return pc + 4
        if opcode == 19 and xo == 449:  # cror bt,ba,bb
            bt = (word >> 21) & 31
            ba = (word >> 16) & 31
            bb = (word >> 11) & 31
            value = self.crbit(ba) | self.crbit(bb)
            field = bt >> 2
            mask = 1 << (3 - (bt & 3))
            self.cr[field] = (self.cr[field] & ~mask) | (value * mask)
            return pc + 4
        return super().step(pc)

    def __init__(self, exe):
        super().__init__(exe)
        self.input = {}
        self.event_reads = {}
        self.hooks[0x8032c608] = lambda e: e.r.__setitem__(3, CONTROLLER_PTR + e.r[3] * 256)

        def event(e):
            controller = (e.r[3] - CONTROLLER_PTR) // 256
            action = e.r[4]
            key = f'{controller}:{action}'
            response = e.input.get('events', {}).get(str(controller), {}).get(str(action), False)
            cursor = e.event_reads.get(key, 0)
            e.event_reads[key] = cursor + 1
            if isinstance(response, list):
                value = bool(response[min(cursor, len(response) - 1)]) if response else False
            else:
                value = bool(response)
            e.events.append(['event', controller, action, value])
            e.wr(EVENT_PTR, bytes([value]))
            e.r[3] = EVENT_PTR

        self.hooks[0x8032d9b0] = event

        def random_range(e):
            low, high = sx(e.r[3], 32), sx(e.r[4], 32)
            raw = e.randoms[e.random_index]
            e.random_index += 1
            value = raw % (high - low + 1) + low
            e.events.append(['random', low, high, value])
            e.r[3] = value & 0xffffffff

        self.hooks[0x803b2344] = random_range

        def azimuth(e):
            player = CHARS.index(e.r[3] - 0x180)
            value = int(e.input['azimuth'][player])
            e.events.append(['azimuth', player, value])
            e.r[3] = value & 0xffffffff

        self.hooks[0x802e2108] = azimuth
        self.hooks[0x802e1124] = lambda e: e.r.__setitem__(3, AUDIO_PTR)

    def prepare(self, case):
        self.write(case['initial'], RULES, case['ball'])
        self.input = case.get('input', {'azimuth': [0, 0], 'events': {}})
        self.event_reads = {}
        self.randoms = [case.get('random_raw', 0)]
        self.random_index = 0

        table = case['animations']
        arrays = {
            'ready_power_state': 0x1a0,
            'ready_power_suppress': 0x1a8,
            'ready_reverse_state': 0x1d0,
            'ready_reverse_suppress': 0x1d8,
            'ready_zone_zero_state': 0x1b8,
            'ready_zone_zero_suppress': 0x1c0,
            'ready_zone_one_state': 0x1e8,
            'ready_zone_one_suppress': 0x1f0,
            'hit_power': 0x198,
            'hit_zone_zero': 0x1b0,
        }
        for name, offset in arrays.items():
            for player, value in enumerate(table[name]):
                self.w32(GAME + offset + player * 4, value)
        serve = case['serve']
        for player in range(2):
            self.w32(GAME + 0x1c8 + player * 4, serve['power_animations'][player])
            self.w32(GAME + 0x1e0 + player * 4, serve['high_animations'][player])
            self.w32(CHARS[player] + 0x1e8, serve['voice_types'][player])
        self.wr(GAME + 0x42e, bytes([case.get('mega_ability', False)]))
        reset = case.get('reset', {})
        for player in range(2):
            self.w32(GAME + 0x248 + player * 4, reset.get('start_angles', [bits(0.0)] * 2)[player])
            self.w32(GAME + 0x27c + player * 4, reset.get('scale', [1, 1])[player])

    def run(self, case):
        self.prepare(case)
        kind = case['kind']
        player = case.get('player', 0)
        if kind == 'in_hit_animation':
            returned = self.call(IN_HIT, (GAME, player))
        elif kind == 'start_ready_animation':
            self.call(START_READY, (GAME, player, case['hit_type']))
            returned = None
        elif kind == 'start_hit_animation':
            self.call(START_HIT, (GAME, player, int(case['normal']), int(case['reverse']), case['hit_type']))
            returned = None
        elif kind == 'check_for_waiting_character_swing':
            self.call(CHECK_WAITING, (GAME,))
            returned = None
        elif kind == 'is_ball_in_hit_range':
            returned = self.call(IN_RANGE, (GAME, player), (case['half_width'], case['center_offset']))
        else:
            raise ValueError(kind)
        return {
            'returned': returned,
            'action_states': [sx(self.r32(GAME + 0x22c + p * 4), 32) for p in range(2)],
            'effects': copy.deepcopy(self.events),
        }


def animation_table():
    return {
        'ready_power_state': [171, 271], 'ready_power_suppress': [181, 281],
        'ready_reverse_state': [172, 272], 'ready_reverse_suppress': [182, 282],
        'ready_zone_zero_state': [173, 273], 'ready_zone_zero_suppress': [183, 283],
        'ready_zone_one_state': [174, 274], 'ready_zone_one_suppress': [184, 284],
        'hit_power': [191, 291], 'hit_zone_zero': [192, 292],
    }


def base_case(seed, kind, player=0):
    state = copy.deepcopy(seed['initial'])
    ball = copy.deepcopy(seed['ball'])
    state.update(server=player, receiver=player, focus_player=0, current_distance=0, lose_animations=[151, 152],
                 action_states=[0, 0], mega_enabled=[False, False])
    for i, p in enumerate(state['players']):
        p['controller'] = 0 if i == player else None
        p['current_animation'] = 59
    ball.update(zone=0, angle=bits(0.0))
    return {
        'kind': kind, 'player': player, 'initial': state, 'ball': ball,
        'animations': animation_table(),
        'serve': {'power_animations': [161, 261], 'high_animations': [162, 262], 'voice_types': [0, 1]},
        'input': {'azimuth': [-37, 125], 'events': {}}, 'random_raw': 0,
        'mega_ability': False, 'reset': {'start_angles': [bits(0.0), bits(0.0)], 'scale': [1, 1]},
    }


def generate():
    assert hashlib.sha256((ROOT / 'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    life_fixture = json.loads((ROOT / '_bevy/tests/data/tetherball_lifecycle_golden.json').read_text(encoding='utf-8'))
    seed = life_fixture['cases'][0]
    emu = HitEmu(rf.load())
    cases = []

    # Direct state predicate includes the full switch domain and values beyond it.
    for current in list(range(-3, 106)) + [0x7fffffff, -0x80000000]:
        c = base_case(seed, 'in_hit_animation')
        c['current_animation'] = current
        c['initial']['players'][0]['current_animation'] = current
        cases.append(c)

    # Every ready selector is exercised for both zones and all power-hit kinds;
    # current-state cases cover the native early exit and each suppression word.
    for player in range(2):
        for zone in (0, 1):
            for hit_type in (-1, 0, 1, 2, 3, 4):
                for mode in ('ordinary', 'early', 'suppressed', 'other'):
                    c = base_case(seed, 'start_ready_animation', player)
                    c['ball']['zone'] = zone
                    c['hit_type'] = hit_type
                    table = c['animations']
                    selector = (('ready_power' if hit_type in (2, 3) and zone == 0 else
                                 'ready_reverse' if hit_type in (1, 3) else
                                 'ready_zone_zero' if zone == 0 else 'ready_zone_one'))
                    if mode == 'early':
                        current = 1
                    elif mode == 'suppressed':
                        current = table[selector + '_suppress'][player]
                    elif mode == 'ordinary':
                        current = 59
                    else:
                        current = -17
                    c['initial']['players'][player]['current_animation'] = current
                    cases.append(c)

    # Exhaust direction flags, types, zones, players and RNG threshold edges.
    for player in range(2):
        for zone in (0, 1):
            for hit_type in (0, 1, 2, 3, 4):
                for normal in (False, True):
                    for reverse in (False, True):
                        for roll in (0, 49, 50, 74, 75, 99):
                            c = base_case(seed, 'start_hit_animation', player)
                            c['ball']['zone'] = zone
                            c['hit_type'] = hit_type
                            c['normal'] = normal
                            c['reverse'] = reverse
                            c['random_raw'] = roll
                            c['initial']['current_distance'] = (hit_type + int(normal) + int(reverse)) % 3
                            cases.append(c)

    # Event-order/state gates include all four button combinations, event
    # sequences that differ across duplicate reads, both ability values, the
    # exact hit-animation switch, state 1 and a non-hit animation.
    for player in range(2):
        for current in (1, 59, 63, 66, 87, 94, 95):
            for controller in (None, 2):
                for normal, reverse in ((False, False), (True, False), (False, True), (True, True)):
                    for ability in (False, True):
                        for action_state in (0, 1):
                            for zone in (0, 1):
                                c = base_case(seed, 'check_for_waiting_character_swing', player)
                                c['initial']['players'][player]['current_animation'] = current
                                c['initial']['players'][player]['controller'] = controller
                                c['initial']['action_states'][player] = action_state
                                c['ball']['zone'] = zone
                                c['mega_ability'] = ability
                                c['input']['events'] = {str(controller or 0): {
                                    '92': [normal, not normal], '93': [reverse, not reverse]}}
                                cases.append(c)

    # Full IsBallInHitRange path, including player/focus equality and both
    # directions, negative scale, wrapped boundaries and precise PPC floats.
    rng = random.Random(0x80399cb0)
    for player in range(2):
        for focus in (0, 1):
            for angle, base, scale, width, center in [
                (0.0, 0.0, 1, 0.6, 0.6), (1.0, 0.0, 1, 0.6, 0.6),
                (6.1, 5.9, 1, 0.5, 0.4), (0.1, 6.1, 1, 0.5, 0.4),
                (3.14159, 0.0, 1, 1.57, 1.57), (-1.0, 0.0, 1, 0.6, 0.6),
                (1.0, 0.0, -1, 0.6, 0.6), (1.0, 0.0, 0, 5.0, 3.0),
            ]:
                c = base_case(seed, 'is_ball_in_hit_range', player)
                c['initial']['focus_player'] = focus
                c['ball']['angle'] = bits(angle)
                c['reset']['start_angles'][player] = bits(base)
                c['reset']['scale'][player] = scale
                c['half_width'] = width
                c['center_offset'] = center
                cases.append(c)
    for _ in range(48):
        player = rng.randrange(2)
        c = base_case(seed, 'is_ball_in_hit_range', player)
        c['initial']['focus_player'] = rng.randrange(2)
        c['ball']['angle'] = bits(rng.uniform(-12.0, 12.0))
        c['reset']['start_angles'][player] = bits(rng.uniform(-12.0, 12.0))
        c['reset']['scale'][player] = rng.choice([-3, -1, 0, 1, 2, 3])
        c['half_width'] = rng.choice([0.0, 0.1, 0.6, 1.57, 3.2])
        c['center_offset'] = rng.choice([-3.2, -1.57, -0.6, 0.0, 0.6, 1.57, 3.2])
        cases.append(c)

    for i, case in enumerate(cases):
        result = emu.run(case)
        case['expected'] = result
        case['label'] = f'{case["kind"]}-{i}'
    return {'elf_sha256': SHA, 'cases': cases}


if __name__ == '__main__':
    value = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == value, 'Hit animation PPC vectors differ'
        print(f'Verified {len(value["cases"])} original hit-animation/range vectors')
    else:
        OUT.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8', newline='\n')
        print('Wrote', OUT)
