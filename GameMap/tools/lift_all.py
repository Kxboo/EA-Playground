#!/usr/bin/env python3
"""Lift + verify every game/engine function (parallel). Writes status-only JSONL (no code):

    python3 GameMap/tools/lift_all.py --elf Remaster/reference/playgroundz.elf --out GameMap/data/lift_status.jsonl [--jobs 4]

Record: {addr, status, reason?, ok?, faults?, blocks?, covered?, calls?, stmts?}
  status: verified | partial | untestable | failed | unsupported | error
"""
import argparse
import json
import multiprocessing as mp
import os
import random
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import elfmap  # noqa: E402
import lift as L  # noqa: E402
import pseudo as PS  # noqa: E402
import verify_lift as VL  # noqa: E402

_G = {}


def _init(elf):
    E = elfmap.Elf(elf)
    _G["E"] = E
    _G["V"] = VL.LiftVerifier(E)


def _work(fn):
    V = _G["V"]
    rec = {"addr": fn["addr"]}
    try:
        lifted, instrs, sites = V.prepare(fn)
    except L.Unsupported as e:
        rec.update(status="unsupported", reason=str(e)[:60])
        return rec
    except Exception as e:  # noqa
        rec.update(status="error", reason="lift:" + type(e).__name__ + ":" + str(e)[:50])
        return rec
    rec["calls"] = lifted.calls
    rec["stmts"] = len(lifted.stmts)
    try:
        rec["sum"] = PS.one_line(_G["E"], fn, lifted)[:160]
        rec["_c"] = PS.Printer(_G["E"], fn).render(lifted)
    except Exception as e:  # noqa
        rec["sum"] = ""
        rec["_c"] = "// printer error: %s" % e
    V.rng = random.Random(fn["addr"])
    try:
        r = V.verify(fn, lifted, sites)
    except Exception as e:  # noqa
        rec.update(status="error", reason="verify:" + type(e).__name__ + ":" + str(e)[:50])
        return rec
    rec.update(r)
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--limit", type=int, default=0)
    a = ap.parse_args()
    E = elfmap.Elf(a.elf)
    funcs = [f for f in E.functions() if f["tier"] in ("game", "engine") and f["size"]]
    if a.limit:
        funcs = funcs[:a.limit]
    slim = [{"addr": f["addr"], "size": f["size"], "name": f["name"], "cls": f["cls"], "method": f["method"], "file": f["file"]} for f in funcs]
    cdir = os.path.join(HERE, "..", "decomp", "lifted")
    os.makedirs(cdir, exist_ok=True)
    byfile = {}
    t0 = time.time()
    done = 0
    with mp.Pool(a.jobs, initializer=_init, initargs=(a.elf,)) as pool, open(a.out, "w") as out:
        unit_of = {f["addr"]: f["file"] for f in funcs}
        for rec in pool.imap_unordered(_work, slim, chunksize=8):
            c = rec.pop("_c", None)
            if c and rec.get("status") in ("verified", "partial", "untestable"):
                byfile.setdefault(unit_of[rec["addr"]], []).append((rec["addr"], rec["status"], c))
            out.write(json.dumps(rec) + "\n")
            done += 1
            if done % 250 == 0:
                out.flush()
                print("%d/%d  %.0fs" % (done, len(slim), time.time() - t0), flush=True)
    for unit, items in byfile.items():
        safe = "".join(ch if ch.isalnum() or ch in "._-" else "_" for ch in unit)
        with open(os.path.join(cdir, safe + ".c"), "w") as fh:
            fh.write("// LOCAL ONLY (git-ignored): pseudo-C lifted from the machine code of %s\n\n" % unit)
            for addr, status, c in sorted(items):
                fh.write("// [%s]\n%s\n\n" % (status, c))
    print("done", done, "in %.0fs" % (time.time() - t0))


if __name__ == "__main__":
    main()
