#!/usr/bin/env python3
"""Verify hashes.py against the original PowerPC code by emulation (needs unicorn).

    python3 GameMap/tools/verify_hashes.py --elf Remaster/reference/playgroundz.elf
"""
import argparse, os, random, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import elfmap, hashes
from unicorn import Uc, UC_ARCH_PPC, UC_MODE_PPC32, UC_MODE_BIG_ENDIAN, UC_HOOK_CODE
from unicorn.ppc_const import *

ap = argparse.ArgumentParser(); ap.add_argument("--elf", required=True); a = ap.parse_args()
E = elfmap.Elf(a.elf)
mu = Uc(UC_ARCH_PPC, UC_MODE_PPC32 | UC_MODE_BIG_ENDIAN)
mu.mem_map(0x80004000, 0x80608000 - 0x80004000 + 0x1000)  # whole image incl. .bss; sections written below
for ad, sz, off, nm, _ in E.secs:
    mu.mem_write(ad, E.raw[off:off + sz])
STACK, BUF, RET = 0x80700000, 0x80710000, 0x80720000
for base in (STACK, BUF, RET):
    mu.mem_map(base, 0x10000)
mu.mem_write(RET, struct.pack(">I", 0x4E800020))  # blr


def call(fn, r3, r4=0, r5=0, r6=0):
    mu.reg_write(UC_PPC_REG_1, STACK + 0x8000)
    for r, v in ((UC_PPC_REG_3, r3), (UC_PPC_REG_4, r4), (UC_PPC_REG_5, r5), (UC_PPC_REG_6, r6)):
        mu.reg_write(r, v)
    mu.reg_write(UC_PPC_REG_LR, RET)
    mu.emu_start(fn, RET)
    return mu.reg_read(UC_PPC_REG_3), mu.reg_read(UC_PPC_REG_4)

random.seed(1)
tests = ["a", "EVENT_PLAYER_MOVE", "ANIM_IDLE", "MGSFX_Darts_Weapons", "x" * 24, "y" * 25, "z" * 47, "w" * 48, "0123456789abcdefghijklmnopqrstuvwxyz"]
tests += ["".join(random.choice("abcdefghijklmnopqrstuvwxyz_0123456789") for _ in range(random.randint(1, 90))) for _ in range(300)]
ok = 0
for s in tests:
    b = s.encode() + b"\0"
    mu.mem_write(BUF, b)
    hi, lo = call(E.by_name["StringHash64__6AttribFPCc"][0], BUF)
    emu64 = (hi << 32) | lo
    emu32 = call(E.by_name["ComputeHash__FPCc"][0], BUF)[0]
    assert emu64 == hashes.attrib_hash64(s), (s, hex(emu64), hex(hashes.attrib_hash64(s)))
    assert emu32 == hashes.locale_hash(s), (s, hex(emu32), hex(hashes.locale_hash(s)))
    ok += 1
print("verified %d strings: Attrib::StringHash64 and ComputeHash match the original PowerPC code" % ok)
