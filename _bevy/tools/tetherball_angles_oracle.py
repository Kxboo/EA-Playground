"""Execute rmAngle::Wrap and IsBetween from the retail PowerPC ELF."""
import hashlib
import json
import random
import struct
import sys
from pathlib import Path

from ppc_emu2 import Emu, rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_angles_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
ANGLE = 0x71000000
WRAP = 0x802cd4e8
IS_BETWEEN = 0x802ecc6c


class AngleEmu(Emu):
    """Add only the two scalar PPC instructions used by IsBetween."""
    def step(self, pc):
        word = self.word(pc)
        opcode = word >> 26
        xo = (word >> 1) & 0x3ff
        if opcode == 63 and xo == 32:  # fcmpo crfD,frA,frB (CR result; FPSCR exceptions are not modeled)
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


def float_value(bits):
    return struct.unpack('>f', struct.pack('>I', bits & 0xffffffff))[0]


def run_wrap(emu, bits):
    emu.wr(ANGLE, struct.pack('>I', bits))
    emu.call(WRAP, (ANGLE,), max_steps=10000)
    return emu.r32(ANGLE)


def run_between(emu, angle_bits, first_bits, second_bits, reverse):
    emu.wr(ANGLE, struct.pack('>III', angle_bits, first_bits, second_bits))
    return emu.call(IS_BETWEEN, (ANGLE, ANGLE + 4, ANGLE + 8, int(reverse)), max_steps=1000) != 0


def generate():
    assert hashlib.sha256((ROOT / 'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    exe = rf.load()
    emu = AngleEmu(exe)

    # Include exact endpoints, neighboring representable values, signed zero,
    # negative angles, several turns, and NaNs. Infinity is omitted from Wrap:
    # the retail repeated-subtraction loop does not terminate on it.
    wrap_inputs = [
        0x00000000, 0x80000000, 0x00000001, 0x80000001,
        0x40c90fda, 0x40c90fdb, 0x40c90fdc,
        0xc0c90fda, 0xc0c90fdb, 0xc0c90fdc,
        0x3e800000, 0xbf800000, 0x41200000, 0xc1200000,
        0x42c80000, 0xc2c80000, 0x7fc12345, 0xffc54321,
    ]
    for multiple in range(-12, 13):
        wrap_inputs.append(struct.unpack('>I', struct.pack('>f', multiple * 6.2831854820251465 + 0.375))[0])
    wrap_cases = [{'input_bits': bits, 'result_bits': run_wrap(emu, bits)} for bits in wrap_inputs]

    between_inputs = set()

    def add(angle, first, second):
        between_inputs.add((angle & 0xffffffff, first & 0xffffffff, second & 0xffffffff))

    # Dense interval endpoint evidence for ascending, descending, equal, and
    # signed/negative bounds, including raw multi-turn inputs (IsBetween does
    # not wrap its three input records).
    values = [0., -0., -6.2831855, -2., -1., -0.5, 0.5, 1., 1.5, 2., 6.2831855, 8., 14.]
    pairs = [(1., 2.), (2., 1.), (1., 1.), (-2., -1.), (-1., -2.),
             (0., 0.), (6.2831855, 8.), (8., 6.2831855), (-6.2831855, 6.2831855)]
    for angle in values:
        for first, second in pairs:
            add(*[struct.unpack('>I', struct.pack('>f', value))[0] for value in (angle, first, second)])

    # Explicit IEEE exceptional and boundary-bit cases expose the retail
    # unordered-comparison behavior and exact endpoint inclusivity.
    special = [0x7fc12345, 0xffc54321, 0x7f800000, 0xff800000,
               0x80000000, 0x00000000, 0x3f800000, 0x40000000]
    for angle in special:
        for first in special:
            for second in special:
                add(angle, first, second)

    rng = random.Random(0x802ecc6c)
    # Arbitrary bit patterns are safe for this compare-only routine and include
    # subnormals, infinities, NaNs, negative values, and large magnitudes.
    for _ in range(384):
        add(rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32))

    between_cases = []
    for angle, first, second in sorted(between_inputs):
        for reverse in (False, True):
            between_cases.append({
                'angle_bits': angle, 'first_bits': first, 'second_bits': second,
                'reverse': reverse,
                'result': run_between(emu, angle, first, second, reverse),
            })
    return {'elf_sha256': SHA, 'wrap': wrap_cases, 'between': between_cases}


if __name__ == '__main__':
    value = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == value
        print(f'Tetherball angles: {len(value["wrap"])} wrap and {len(value["between"])} IsBetween PPC vectors match')
    else:
        OUT.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8', newline='\n')
        print('Wrote', OUT)
