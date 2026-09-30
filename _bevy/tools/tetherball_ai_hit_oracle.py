"""Run original TetherballHitCompulsion PPC with VLT and AIRand boundaries."""
import copy
import hashlib
import json
import random
import struct
import sys
from pathlib import Path

from character_input_oracle import InputEmu, fbits
from ppc_emu2 import sx
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_ai_hit_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
HIT = 0x71010000
OWNER = 0x71020000
BALL = 0x71000000
GAME = 0x71100000
WORLD = 0x805e8320
DATABASE = 0x71110000
COLLECTION = 0x71111000
EVENTS = 0x71500000
TBLL = 0x54424c4c
CTOR = 0x80395c18
SET_ATTRIBUTES = 0x80395ce4
SET_DIFFICULTY = 0x80395d64
ACTIVATE = 0x80395d84
HAS_EXPIRED = 0x80396214
THINK = 0x80396288
IS_INTERRUPTIBLE = 0x8039636c
GET_NAME = 0x80396374
DEACTIVATE = 0x80396380
BASE_ACTIVATE = 0x802cc868
WORLD_GET_TETHERBALL = 0x80319a54
DB_GET_KEY = 0x802f45e4
DB_GET_COLLECTION = 0x802f4794
DB_GET_INT16_ARRAY = 0x802f4bf8
DB_DESTROY_COLLECTION = 0x802f4900
AI_RANDOM = 0x802cc578
TABLE_NAMES = [
    'hit_returnanglepredelta', 'hit_returnanglepostdelta',
    'hit_accelanglepredelta', 'hit_accelanglepostdelta',
]
TABLE_OFFSETS = [0xb0, 0xbc, 0xc8, 0xd4]


class HitCompulsionEmu(InputEmu):
    """Original compulsion bodies; only database and AIRand calls are hooked.

    WorldMan::GetTetherballMinigame, GetTunablesCollectionName, the MGID
    constructor, and every rmAngle helper execute from the pinned ELF.
    """
    def __init__(self, exe):
        super().__init__(exe)
        self.case = {}
        self.events = []
        self.random_index = 0
        self.hooks[DB_GET_KEY] = self._get_key
        self.hooks[DB_GET_COLLECTION] = self._get_collection
        self.hooks[DB_GET_INT16_ARRAY] = self._get_int16_array
        self.hooks[DB_DESTROY_COLLECTION] = self._destroy_collection
        self.hooks[AI_RANDOM] = self._random

    def step(self, pc):
        word = self.word(pc)
        if word >> 26 == 53:  # stfsu fD,d(rA)
            d, a = (word >> 21) & 31, (word >> 16) & 31
            address = ((self.r[a] if a else 0) + sx(word, 16)) & 0xffffffff
            self.wr(address, struct.pack('>I', fbits(self.f[d])))
            self.r[a] = address
            return pc + 4
        return super().step(pc)

    def cstring(self, address):
        data = bytearray()
        while True:
            value = self.rd(address + len(data), 1)[0]
            if value == 0:
                return data.decode('ascii')
            data.append(value)

    def _get_key(self, em):
        name = self.cstring(em.r[4])
        em.events.append(['vlt_key', name])
        # The hash pairs are opaque to this call graph; GetCollection is the
        # database boundary and returns a prepared native collection object.
        em.r[3] = 0x12345678
        em.r[4] = 0x9abcdef0

    def _get_collection(self, em):
        em.events.append(['vlt_collection', COLLECTION])
        em.r[3] = COLLECTION

    def _get_int16_array(self, em):
        name = self.cstring(em.r[4])
        index = em.r[5]
        values = em.case['tunables_degrees'][name]
        value = sx(values[index], 16)
        em.events.append(['vlt_i16', name, index, value])
        em.r[3] = value & 0xffffffff

    def _destroy_collection(self, em):
        em.events.append(['vlt_destroy', em.r[4]])

    def _random(self, em):
        low, high = sx(em.r[3], 32), sx(em.r[4], 32)
        raw = em.case['random_values'][em.random_index]
        em.random_index += 1
        if low <= high:
            value = low + raw % (high - low + 1)
        else:
            value = high + raw % (low - high + 1)
        em.events.append(['random', low, high, value])
        em.r[3] = value & 0xffffffff

    def prepare(self, case, sentinel=0xa5):
        self.case = case
        self.events = []
        self.random_index = 0
        self.wr(HIT, bytes([sentinel]) * 0x180)
        self.wr(BALL, bytes(0x170))
        self.wr(GAME, bytes(0x460))
        self.wr(WORLD, bytes(0x100))
        self.w32(self.e.symbols['_SDA_BASE_']['value'] - 0x260c, DATABASE)
        self.w32(WORLD + 0x90, GAME)
        self.w32(GAME + 0x38, case.get('native_game_id', TBLL))
        self.w32(GAME + 0x40, case.get('session_mode', 2))
        self.w32(GAME + 0x48, case.get('dare', -1))
        self.w32(BALL + 0xa0, case.get('ball_hit_type', 0))
        self.w32(BALL + 0xac, case.get('ball_zone', 0))
        self.w32(BALL + 0x50, case.get('ball_angle_bits', fbits(0.0)))
        # C++ constructor and all angle helpers execute natively.
        self.call(CTOR, (HIT, OWNER, case.get('priority', 50)))
        # SlotPool provides this binding outside the compulsion constructor.
        self.w32(HIT + 0x84, BALL)
        self.case = case
        self.events = []
        self.random_index = 0

    def set_attributes(self, attrs):
        a0, a1 = 0x71112000, 0x71112004
        self.w32(a0, attrs['angle_bits'])
        self.w32(a1, attrs['expiry_angle_bits'])
        self.call(SET_ATTRIBUTES, (HIT, a0, int(attrs['acceleration_mode']), attrs['distance'],
                                   attrs['power_type'], a1), (struct.unpack('>f', struct.pack('>I', attrs['rate_bits']))[0],))

    def set_difficulty(self, values):
        self.call(SET_DIFFICULTY, (HIT, *values))

    def activate(self):
        # Install the asset-backed signed Int16 values seen by the real VLT API.
        self.call(ACTIVATE, (HIT,))

    def state(self):
        return {
            'owner': self.r32(HIT + 4),
            'active': bool(self.rd(HIT + 8, 1)[0]),
            'priority': self.rd(HIT + 9, 1)[0],
            'ball_handle': self.r32(HIT + 0x84),
            'hit_category': sx(self.r32(HIT + 0x88), 32),
            'hit_attempt': sx(self.r32(HIT + 0x8c), 32),
            'target_angle_bits': self.r32(HIT + 0x90),
            'base_angle_bits': self.r32(HIT + 0x94),
            'rate_bits': self.r32(HIT + 0x98),
            'acceleration_mode': bool(self.rd(HIT + 0x9c, 1)[0]),
            'player_distance': self.r32(HIT + 0xa0),
            'power_type': self.r32(HIT + 0xa4),
            'difficulty': list(self.rd(HIT + 0xa8, 7)),
            'angle_tables_bits': [
                [self.r32(HIT + off + i * 4) for i in range(3)]
                for off in TABLE_OFFSETS
            ],
            'expiry_angle_bits': self.r32(HIT + 0xe0),
        }

    def run(self, case):
        self.prepare(case)
        kind = case['kind']
        if kind == 'construct':
            result = None
        elif kind == 'set_attributes':
            self.set_attributes(case['attributes'])
            result = None
        elif kind == 'set_difficulty_variables':
            self.set_difficulty(case['difficulty'])
            result = None
        elif kind == 'activate':
            self.set_attributes(case['attributes'])
            self.set_difficulty(case['difficulty'])
            self.activate()
            result = None
        elif kind == 'has_expired':
            self.set_attributes(case['attributes'])
            result = self.call(HAS_EXPIRED, (HIT, case.get('think_lod', 0)))
        elif kind == 'think':
            # This function reads its persistent +88/+90 fields and the bound
            # ball pointer. Seed those values exactly as prior Activate did.
            self.w32(HIT + 0x88, case['hit_category'])
            self.w32(HIT + 0x90, case['target_angle_bits'])
            result = self.call(THINK, (HIT, case.get('milliseconds', 16)))
        elif kind == 'activate_base':
            result = self.call(BASE_ACTIVATE, (HIT,))
        elif kind == 'deactivate':
            result = self.call(DEACTIVATE, (HIT,))
        elif kind == 'is_interruptible':
            result = self.call(IS_INTERRUPTIBLE, (HIT,))
        elif kind == 'get_name':
            result = self.cstring(self.call(GET_NAME, (HIT,)))
        else:
            raise ValueError(kind)
        return {'result': result, 'state': self.state(), 'events': copy.deepcopy(self.events)}


def f32bits(value):
    return fbits(value)


def base_case(kind):
    names = {
        'hit_returnanglepredelta': [5, 15, 30],
        'hit_returnanglepostdelta': [9, 20, 42],
        'hit_accelanglepredelta': [7, 18, 36],
        'hit_accelanglepostdelta': [11, 24, 48],
    }
    return {
        'kind': kind, 'priority': 50, 'session_mode': 2, 'dare': -1,
        'ball_hit_type': 0, 'ball_zone': 0, 'ball_angle_bits': f32bits(0.0),
        'attributes': dict(angle_bits=f32bits(0.0), rate_bits=f32bits(1.0),
                           acceleration_mode=False, distance=2, power_type=0,
                           expiry_angle_bits=f32bits(0.0)),
        'difficulty': [0, 0, 0, 0, 0, 0, 0],
        'tunables_degrees': names,
        'random_values': [0] * 16,
    }


def generate():
    assert hashlib.sha256((ROOT / 'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    emu = HitCompulsionEmu(rf.load())
    cases = []

    cases.append(base_case('construct'))
    c = base_case('set_attributes')
    c['attributes'] = dict(angle_bits=f32bits(-0.75), rate_bits=f32bits(-1.5),
                           acceleration_mode=True, distance=1, power_type=5,
                           expiry_angle_bits=f32bits(7.0))
    cases.append(c)
    for values in ([0, 0, 0, 0, 0, 0, 0], [255, 254, 253, 252, 251, 250, 249], [1, 3, 17, 33, 75, 127, 200]):
        c = base_case('set_difficulty_variables')
        c['difficulty'] = list(values)
        cases.append(c)

    # Cross every native activation gate with both movement modes, signed rate,
    # all distance/zone and hit/power categories, and probability edge arrays.
    probability_sets = [
        [0, 0, 0, 0, 0, 0, 0],
        [100, 100, 100, 100, 100, 100, 100],
        [50, 75, 25, 60, 70, 15, 20],
    ]
    random_sets = [
        [0, 0, 0, 0, 0, 0, 0, 0],
        [99, 99, 99, 99, 99, 99, 99, 99],
        [49, 50, 74, 75, 25, 60, 70, 15],
    ]
    for accelerating in (False, True):
        for rate in (-1.5, 0.0, 1.5):
            for distance in range(3):
                for zone in (0, 1):
                    for ball_kind in (0, 1, 3, 7):
                        for power_type in (0, 1, 2, 5):
                            for probs, draws in zip(probability_sets, random_sets):
                                c = base_case('activate')
                                c['ball_zone'] = zone
                                c['ball_hit_type'] = ball_kind
                                c['attributes'].update(angle_bits=f32bits(0.45), rate_bits=f32bits(rate),
                                                       acceleration_mode=accelerating, distance=distance,
                                                       power_type=power_type,
                                                       expiry_angle_bits=f32bits(5.8))
                                c['difficulty'] = probs
                                c['random_values'] = list(draws) + [0] * 8
                                c['tunables_degrees'] = {
                                    'hit_returnanglepredelta': [5 + distance, 15 + distance, 30 + distance],
                                    'hit_returnanglepostdelta': [9 + distance, 20 + distance, 42 + distance],
                                    'hit_accelanglepredelta': [7 + distance, 18 + distance, 36 + distance],
                                    'hit_accelanglepostdelta': [11 + distance, 24 + distance, 48 + distance],
                                }
                                cases.append(c)

    # HasExpired uses an inclusive rmAngle interval; vary endpoint, wrap,
    # orientation and a zero/nonzero SetAttributes rate.
    for expiry, ball_angle in [(0.0, 0.0), (0.0, 1.0), (0.0, 3.141592741),
                               (5.9, 0.1), (5.9, 4.0), (2.2, 5.2)]:
        for rate in (0.0, 1.0, -1.0):
            c = base_case('has_expired')
            c['attributes'].update(rate_bits=f32bits(rate), expiry_angle_bits=f32bits(expiry))
            c['ball_angle_bits'] = f32bits(ball_angle)
            cases.append(c)

    # Think's action window and world minigame write gate. The exact 0.15f
    # boundary and adjacent representable floats distinguish PPC's strict
    # comparison from a closed interval; extra turns check rmAngle wrapping.
    limit = f32bits(0.15)
    think_pairs = [
        (0.0, struct.unpack('>f', struct.pack('>I', limit - 1))[0]),
        (0.0, struct.unpack('>f', struct.pack('>I', limit))[0]),
        (0.0, struct.unpack('>f', struct.pack('>I', limit + 1))[0]),
        (0.5, 0.5), (0.5, 0.65), (0.5, 0.65001),
        (0.5, 0.5 + 2.0 * 3.1415927410125732),
        (0.5, 0.5 - 2.0 * 3.1415927410125732),
        (0.1, -0.05), (6.1, 0.1), (2.0, 2.2),
    ]
    for target, ball_angle in think_pairs:
        for category in (-1, 0, 1, 3, 7):
            for is_tetherball in (False, True):
                c = base_case('think')
                c['hit_category'] = category
                c['target_angle_bits'] = f32bits(target)
                c['ball_angle_bits'] = f32bits(ball_angle)
                c['tetherball_game'] = is_tetherball
                # GetTetherballMinigame executes the real WorldMan helper. A
                # mismatched active MGID makes its native return null.
                c['native_game_id'] = TBLL if is_tetherball else 0x4f544852
                cases.append(c)

    # Trivial virtual/base activation records.
    for kind in ('activate_base', 'deactivate', 'is_interruptible', 'get_name'):
        cases.append(base_case(kind))

    # Replace active-id setup for Think cases, run native routines, and capture
    # only fields owned by this compulsion and its MGTetherball output pair.
    vectors = []
    for i, case in enumerate(cases):
        emu.prepare(case)
        if case['kind'] == 'think':
            emu.w32(GAME + 0x38, case['native_game_id'])
        if case['kind'] == 'activate':
            emu.set_attributes(case['attributes'])
            emu.set_difficulty(case['difficulty'])
            # Expose exactly the post-call f32 values written by original VLT
            # conversion as the typed tuning record consumed by Rust.
            expected = emu.run({**case, 'kind': 'activate'})
            case['tuning_bits'] = expected['state']['angle_tables_bits']
            actual = expected
        else:
            # Prepare already constructed a base object. run() resets it and
            # performs the selected direct body with the appropriate setup.
            actual = emu.run(case)
        if case['kind'] == 'think':
            actual['rally'] = [sx(emu.r32(GAME + 0x234), 32), sx(emu.r32(GAME + 0x238), 32)]
        case['expected'] = actual
        case['label'] = f'{case["kind"]}-{i}'
        vectors.append(case)
    return {'elf_sha256': SHA, 'cases': vectors}


if __name__ == '__main__':
    value = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == value, 'Tetherball hit-compulsion PPC vectors differ'
        print(f'Verified {len(value["cases"])} original TetherballHitCompulsion vectors')
    else:
        OUT.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8', newline='\n')
        print('Wrote', OUT)
