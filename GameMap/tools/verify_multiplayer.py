#!/usr/bin/env python3
"""Verify reference/multiplayer_mode.py byte-for-byte against the original PowerPC functions.

    python3 GameMap/tools/verify_multiplayer.py --elf Remaster/reference/playgroundz.elf
"""
import argparse, os, random, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(HERE, "..", "reference"))
import elfmap
from emu import Emu
from multiplayer_mode import MultiplayerMode, SIZE, OFF_TEAMS

ap = argparse.ArgumentParser(); ap.add_argument("--elf", required=True); ap.add_argument("--runs", type=int, default=400)
a = ap.parse_args()
emu = Emu(elfmap.Elf(a.elf))
sfx = "__15MultiplayerMode"
obj = emu.alloc(SIZE)
rnd = random.Random(7)
checks = 0


def s32(v):
    return struct.pack(">i", v)


def cmp(tag):
    global checks
    orig = emu.read(obj, SIZE)
    assert orig == bytes(ref.m), "%s mismatch\n orig %s\n ref  %s" % (tag, orig.hex(), bytes(ref.m).hex())
    checks += 1


for run in range(a.runs):
    n = rnd.choice([2, 3, 4, 4, 4])
    teams = bytearray(0x88); struct.pack_into(">i", teams, 0, n)
    for k in range(4, 0x88, 4): struct.pack_into(">i", teams, k, rnd.randint(-3, 9))
    emu.write(obj, bytes(SIZE))
    ref = MultiplayerMode(); ref.constructor()
    emu.write(obj + 4, s32(-1))
    # SetupMultiplayerGame(this, const MGID*, const Teams*)
    mg = emu.alloc(4); emu.write(mg, s32(rnd.randint(0, 8))); tb = emu.alloc(0x88); emu.write(tb, bytes(teams))
    emu.call("SetupMultiplayerGame" + sfx + "FPC4MGIDPC5Teams", obj, mg, tb)
    ref.setup_multiplayer_game(struct.unpack(">i", emu.read(mg, 4))[0], bytes(teams)); cmp("setup")
    rules = [rnd.randint(-2, 20) for _ in range(5)]; rb = emu.alloc(20); emu.write(rb, b"".join(s32(x) for x in rules))
    emu.call("SetRules" + sfx + "FPCi", obj, rb); ref.set_rules(rules); cmp("rules")
    if rnd.random() < .5:
        emu.call("StartFreePlay" + sfx + "Fv", obj); ref.start_free_play()
    else:
        r = rnd.randint(1, 8); emu.call("StartPointSeries" + sfx + "Fi", obj, r); ref.start_point_series(r)
    cmp("start")
    for step in range(rnd.randint(1, 12)):
        if rnd.random() < .5:
            pts = [rnd.randint(0, 50), rnd.randint(0, 50)] + [rnd.choice([-1, rnd.randint(0, 50)]) for _ in range(2)]
            emu.call("AddRoundResults" + sfx + "Fiiii", obj, *pts); ref.add_round_results(*pts); cmp("round %s" % pts)
        else:
            w = [rnd.choice([-1, rnd.randrange(n)]) for _ in range(2)]
            emu.call("AddWinResults" + sfx + "Fii", obj, *w); ref.add_win_results(*w); cmp("win %s" % w)
        if rnd.random() < .2:
            pl = [rnd.randint(0, 3), rnd.randint(0, 3), rnd.choice([-1, 2]), rnd.choice([-1, 3])]
            emu.call("SetLastPlacement" + sfx + "Fiiii", obj, *pl); ref.set_last_placement(*pl); cmp("placement")
        for p in range(n):  # queries
            for fn, ref_fn in (("GetWinTotal", ref.get_win_total), ("GetPointTotal", ref.get_point_total),
                               ("GetPlayerRank", ref.get_player_rank), ("WonLastGame", ref.won_last_game),
                               ("GetPlayerPointsInThisMatch", ref.get_player_points_in_this_match)):
                got = emu.call(fn + sfx + "Fi", obj, p)[0]; got = got - (1 << 32) if got >= 1 << 31 else got
                assert got == ref_fn(p), (fn, p, got, ref_fn(p)); checks += 1
        for rk in range(-1, n + 1):
            got = emu.call("GetPlayerNumByRank" + sfx + "Fi", obj, rk)[0]; got = got - (1 << 32) if got >= 1 << 31 else got
            assert got == ref.get_player_num_by_rank(rk); checks += 1
        got = emu.call("GetNumRoundsLeft" + sfx + "Fv", obj)[0]; got = got - (1 << 32) if got >= 1 << 31 else got
        assert got == ref.get_num_rounds_left(); checks += 1
print("verified: %d comparisons over %d randomized sessions - reference model matches the original PowerPC code byte-for-byte" % (checks, a.runs))
