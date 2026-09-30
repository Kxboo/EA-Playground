"""Execute original PowerPC timing routines to capture/check deterministic vectors.

Only external OS/input/Havok effects and libc ceil are hooked; timing arithmetic,
unsigned conversion, cap/fixed/pause branches and slice distribution execute ELF words.
"""
import argparse
import hashlib
import json
import math
import random
import struct
from pathlib import Path
from ppc_emu2 import Emu, M, sx
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
ELF = ROOT / 'Remaster/reference/playgroundz.elf'
GOLDEN = ROOT / '_bevy/tests/data/timing_golden.json'
SHA256 = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'


def bits(value):
    return struct.unpack('>I', struct.pack('>f', value))[0]


class TimingEmu(Emu):
    """Extra Gekko instructions needed by the original conversion helper."""
    def __init__(self, exe):
        super().__init__(exe)
        self.integer_fprs = {}
        self.tick = 0

    def rd(self, address, length):
        result = bytearray()
        for offset in range(length):
            try:
                result.extend(super().rd(address + offset, 1))
            except ValueError:  # ELF NOBITS globals / OS low memory start zeroed
                result.append(0)
        return bytes(result)

    def step(self, pc):
        word = self.word(pc)
        op = word >> 26
        dest, a, b = (word >> 21) & 31, (word >> 16) & 31, (word >> 11) & 31
        if pc == 0x803acdfc:
            assert word == 0x7c0c42e6
            self.r[0] = self.tick
            return pc + 4
        if op == 63:
            xo = (word >> 1) & 1023
            if xo == 0:  # fcmpu
                x, y = self.f[a], self.f[b]
                self.cr[(word >> 23) & 7] = 1 if math.isnan(x) or math.isnan(y) else 8 if x < y else 4 if x > y else 2
                return pc + 4
            if xo == 20:  # fsub (double)
                self.f[dest] = self.f[a] - self.f[b]
                self.integer_fprs.pop(dest, None)
                return pc + 4
            if xo == 15:  # fctiwz stores an integer payload, not an IEEE double
                value = math.trunc(self.f[b])
                self.integer_fprs[dest] = value & M if -0x80000000 <= value <= 0x7fffffff else 0x80000000
                return pc + 4
        if op == 54 and dest in self.integer_fprs:
            addr = (self.r[a] if a else 0) + sx(word, 16)
            self.wr(addr, struct.pack('>II', 0xfff80000, self.integer_fprs[dest]))
            return pc + 4
        if op in (48, 50, 59, 63):
            self.integer_fprs.pop(dest, None)
        return super().step(pc)


def frame(exe, v):
    emu = TimingEmu(exe)
    emu.tick = v['tick']
    # TRC early-exit gate; unsupported state dispatch is disabled by state=-1.
    emu.w32(0x80602094, 0x71000000)
    emu.w32(0x80602024, v['previous_cycles'])
    emu.w32(0x800000fc, v['clock_hz'])
    emu.wr(0x805ff804, bytes([v['cap_enabled']]))
    emu.w32(0x805ff800, v['cap_ms'])
    emu.wr(0x80602005, bytes([v['fixed_enabled']]))
    emu.w32(0x805ff808, v['fixed_ms'])
    emu.wr(0x80602004, bytes([v['paused']]))
    emu.w32(0x805ff80c, M)
    captured = []
    emu.hooks[0x8041b6ac] = lambda e: None
    emu.hooks[0x803b40d0] = lambda e: e.r.__setitem__(3, 0)
    emu.hooks[0x8032c608] = lambda e: e.r.__setitem__(3, 0x71001000)
    emu.hooks[0x8032caf8] = lambda e: captured.append(e.r[4])
    emu.call(0x803acdc4)
    assert len(captured) == 1
    return dict(v, frame_ms_bits=emu.r32(0x8060201c), simulation_ms=emu.r32(0x80602010), input_ms=captured[0])


def physics(exe, dt, world, paused):
    emu = TimingEmu(exe)
    manager = 0x71002000
    emu.w32(manager + 0x1e8, 0x71003000 if world else 0)
    emu.wr(manager + 5, bytes([paused]))
    steps = []
    emu.hooks[0x8002e958] = lambda e: e.f.__setitem__(1, float(math.ceil(e.f[1])))
    emu.hooks[0x8017b6d0] = lambda e: steps.append(bits(e.f[1]))
    emu.call(0x803b6f84, (manager, dt))
    return dict(dt_ms=dt, has_world=world, paused=paused, step_bits=steps)


def capture():
    actual = hashlib.sha256(ELF.read_bytes()).hexdigest()
    assert actual == SHA256, f'Unexpected ELF SHA256: {actual}'
    exe = rf.load()
    rng = random.Random(0x803acdc4)
    frames = []
    for cycles in [0, 1, 485999, 486000, 8259999, 8260000, 29159999, 29160000, 29646000, 97200000, M]:
        for cap, fixed, paused in [(False, False, False), (True, False, False), (True, True, False), (True, True, True)]:
            frames.append(frame(exe, dict(tick=0, previous_cycles=(-cycles) & M, clock_hz=486000000, cap_enabled=cap, cap_ms=60, fixed_enabled=fixed, fixed_ms=16, paused=paused)))
    for _ in range(116):
        frames.append(frame(exe, dict(tick=rng.randrange(1 << 32), previous_cycles=rng.randrange(1 << 32), clock_hz=rng.choice([243000000, 486000000, 729000000, 1000000]), cap_enabled=bool(rng.getrandbits(1)), cap_ms=rng.choice([0, 1, 16, 60, 200]), fixed_enabled=bool(rng.getrandbits(1)), fixed_ms=rng.choice([0, 1, 16, 60, 80, 200]), paused=bool(rng.getrandbits(1)))))
    physics_cases = [physics(exe, dt, world, paused) for dt in [-1, 0, 1, 59, 60, 61, 119, 120, 121, 199, 200, 201, 0x7fffffff] for world in [False, True] for paused in [False, True]]
    physics_cases += [physics(exe, dt, True, False) for dt in range(1, 201)]
    return dict(elf_sha256=SHA256, frames=frames, physics=physics_cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Compare with committed fixture without writing')
    args = parser.parse_args()
    vectors = capture()
    if args.check:
        assert json.loads(GOLDEN.read_text()) == vectors, 'Timing golden fixture differs from original executable'
        action = 'Verified'
    else:
        GOLDEN.parent.mkdir(parents=True, exist_ok=True)
        GOLDEN.write_text(json.dumps(vectors, indent=2) + '\n', encoding='utf-8')
        action = 'Captured'
    print(f"{action} {len(vectors['frames'])} frame and {len(vectors['physics'])} physics vectors from {SHA256}")


if __name__ == '__main__':
    main()
