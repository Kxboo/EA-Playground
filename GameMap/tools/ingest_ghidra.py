#!/usr/bin/env python3
"""Reduce Ghidra decompiler output to publishable metrics (no code).

    python3 GameMap/tools/ingest_ghidra.py [--in GameMap/decomp/ghidra.jsonl] [--out GameMap/data/ghidra_metrics.jsonl]

Per function: ok, lines, loops, gotos, switches, warnings, bad-instruction flag, 'unaff' (unresolved-input count),
params. The decompiled text stays in GameMap/decomp (git-ignored).
"""
import argparse
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
ap = argparse.ArgumentParser()
ap.add_argument("--in", dest="inp", default=os.path.join(HERE, "..", "decomp", "ghidra.jsonl"))
ap.add_argument("--out", default=os.path.join(HERE, "..", "data", "ghidra_metrics.jsonl"))
a = ap.parse_args()
n = 0
with open(a.out, "w", newline="\n") as out:
    for line in open(a.inp):
        r = json.loads(line)
        m = {"addr": r["addr"], "ok": bool(r.get("ok"))}
        if r.get("ok"):
            for k in ("lines", "loops", "gotos", "switch", "warn", "unaff", "params"):
                m[k] = r.get(k, 0)
            m["bad"] = bool(r.get("bad"))
        else:
            m["reason"] = r.get("reason", "")[:60]
        out.write(json.dumps(m, separators=(",", ":")) + "\n")
        n += 1
print("wrote", n, "records")
