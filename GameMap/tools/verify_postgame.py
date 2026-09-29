#!/usr/bin/env python3
"""Verify reference/multiplayer_mode.post_game_awards against the original Minigame::OpenPostGameScreen.

UI/handler callees (FEManager, PostGameHandlers, OpenAptScreen) are stubbed to `return`; everything else is the
original code. Compares the whole MultiplayerMode object and the PostGameInfo bytes after randomized inputs.
"""
import argparse, os, random, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(HERE, "..", "reference"))
import elfmap
from emu import Emu
from multiplayer_mode import MultiplayerMode, SIZE, INFO_SIZE

ap = argparse.ArgumentParser(); ap.add_argument("--elf", required=True); ap.add_argument("--runs", type=int, default=3000)
a = ap.parse_args()
E = elfmap.Elf(a.elf)
emu = Emu(E)
fe = emu.alloc(0x200); pgh = emu.alloc(0x200)
emu.stub("GetInstance__9FEManagerFv", fe)
emu.stub("GetInstance__16PostGameHandlersFv", pgh)
emu.stub("SetupPostGameHandlers__16PostGameHandlersFQ25Enums12MinigameTypeP12PostGameInfo")
emu.stub("OpenAptScreen__9FEManagerFPc")
mpa, mga, infoa = emu.alloc(SIZE), emu.alloc(0x100), emu.alloc(INFO_SIZE)
emu.write(elfmap.SDA_R13 - 0x1E94, struct.pack(">I", mpa))  # global MultiplayerMode* (r13-0x1e94)
rnd = random.Random(11)
for run in range(a.runs):
    raw = bytearray(rnd.getrandbits(8) for _ in range(SIZE))
    raw[0] = rnd.choice([0, 1]); raw[0xE0] = rnd.choice([0, 1])
    n = rnd.choice([2, 3, 4])
    struct.pack_into(">i", raw, 8, n)
    for t in range(4):  # team rows: 4 slots x 16 bytes, code word at +0x10 within the slot
        for i in range(4):
            struct.pack_into(">i", raw, 8 + t * 0x40 + 0x10 + 0x10 * i, rnd.choice([-1, 0, 1, 2, 3, 4, 5, 6]))
    for o in (0x90, 0xB0):  # sane score/rank blocks so ranking math stays in range
        for k in range(8): struct.pack_into(">i", raw, o + 4 * k, rnd.randint(0, 5) if k % 2 == 0 else rnd.randint(0, 3))
    struct.pack_into(">i", raw, 0xE4, rnd.randint(0, 6)); struct.pack_into(">i", raw, 0xE8, rnd.randint(0, 6))
    info = bytearray(INFO_SIZE)
    struct.pack_into(">i", info, 0x3C, rnd.randint(0, 3))
    for k in range(4): struct.pack_into(">i", info, 0x40 + 4 * k, rnd.choice([-1, 0, 1, 2, 3]))
    mgt = rnd.randint(0, 6); mgp = rnd.choice([0, 1, 2, 3, 4]); f48 = rnd.getrandbits(31)
    mg = bytearray(0x100); struct.pack_into(">i", mg, 0x48, f48); struct.pack_into(">i", mg, 0x70, mgp)
    emu.write(mpa, bytes(raw)); emu.write(mga, bytes(mg)); emu.write(infoa, bytes(info)); emu.write(fe, bytes(0x200))
    emu.call("OpenPostGameScreen__8MinigameFQ25Enums12MinigameTypeP12PostGameInfo", mga, mgt, infoa)
    ref = MultiplayerMode(bytes(raw))
    from multiplayer_mode import post_game_awards
    post_game_awards(ref, mgt, mgp, f48, info)
    assert emu.read(mpa, SIZE) == bytes(ref.m), "MP mismatch run %d type %d players %d" % (run, mgt, mgp)
    assert emu.read(infoa, INFO_SIZE) == bytes(info), "info mismatch run %d" % run
print("verified %d randomized post-game results: reference scoring matches Minigame::OpenPostGameScreen" % a.runs)
