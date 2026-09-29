#!/usr/bin/env python3
"""Static per-function facts and structural kind classification for every game/engine function.

    python3 GameMap/tools/funcfacts.py --elf Remaster/reference/playgroundz.elf --out GameMap/data/function_facts.jsonl

Fields: addr, ni (instructions), br (conditional branches), back (backward branches incl. bdnz = loops), calls (direct bl),
icalls (bctrl), tails (tail-call b out), sw (bctr jump/dispatch), fp, ps (paired-single), leaf, frame (stack bytes),
kind. Kinds are structural (from the instruction shape only), not semantic.
"""
import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import elfmap  # noqa: E402

LOAD = re.compile(r"^l(bz|hz|ha|wz|fs|fd)(u|x|ux)?$")
STORE = re.compile(r"^st(b|h|w|fs|fd)(u|x|ux)?$")
COND = re.compile(r"^b(eq|ne|lt|ge|gt|le|ns|so|un|nu|dnz|dz|c)")


def facts(E, fn):
    code = E.rd(fn["addr"], fn["size"])
    ins = list(E.md.disasm(code, fn["addr"]))
    decoded = len(ins) * 4 == len(code)
    ni = len(code) // 4
    br = back = calls = icalls = tails = sw = fp = ps = 0
    loads = stores = 0
    frame = 0
    lo, hi = fn["addr"], fn["addr"] + fn["size"]
    mns = []
    for i in ins:
        mn = i.mnemonic.rstrip(".").rstrip("+-")
        mns.append(mn)
        op = i.op_str
        if mn == "bl":
            calls += 1
        elif mn == "bctrl":
            icalls += 1
        elif mn == "bctr":
            sw += 1
        elif mn == "b":
            try:
                t = int(op.split(",")[-1].strip(), 16)
                if not (lo <= t < hi):
                    tails += 1
                elif t <= i.address:
                    back += 1
            except ValueError:
                pass
        elif COND.match(mn) and not mn.endswith("lr") and mn not in ("blr", "bl"):
            br += 1
            try:
                t = int(op.split(",")[-1].strip(), 16)
                if lo <= t <= i.address:
                    back += 1
            except ValueError:
                pass
        elif mn.startswith(("ps_", "psq_")):
            ps += 1
        elif mn.startswith("f") or mn in ("lfs", "lfd", "stfs", "stfd", "lfsx", "lfdx", "stfsx", "stfdx"):
            fp += 1
        if LOAD.match(mn):
            loads += 1
        if STORE.match(mn):
            stores += 1
        if mn == "stwu" and op.startswith("r1, -"):
            try:
                frame = max(frame, int(op.split(",")[1].strip().strip("-"), 0))
            except ValueError:
                pass
    name, meth = fn["name"], fn["method"]
    if name.startswith("__sinit_"):
        kind = "static-init"
    elif ni == 1 and mns[:1] == ["blr"]:
        kind = "stub"
    elif tails and ni <= 3 and calls == 0:
        kind = "thunk"
    elif meth == "constructor":
        kind = "constructor"
    elif meth == "destructor":
        kind = "destructor"
    elif sw:
        kind = "dispatcher"
    elif back:
        kind = "loop"
    elif ni <= 5 and loads == 1 and stores == 0 and calls == 0 and mns and mns[-1] == "blr":
        kind = "getter"
    elif ni <= 5 and stores == 1 and loads == 0 and calls == 0 and mns and mns[-1] == "blr":
        kind = "setter"
    elif calls + tails == 1 and ni <= 14 and icalls == 0:
        kind = "wrapper"
    elif calls + icalls + tails == 0 and fp:
        kind = "leaf-math"
    elif calls + icalls + tails == 0:
        kind = "leaf"
    else:
        kind = "logic"
    return {"addr": fn["addr"], "ni": ni, "br": br, "back": back, "calls": calls, "icalls": icalls, "tails": tails, "sw": sw,
            "fp": fp, "ps": ps, "ld": loads, "st": stores, "frame": frame, "kind": kind, "dec": decoded}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    ap.add_argument("--out", default=os.path.join(HERE, "..", "data", "function_facts.jsonl"))
    a = ap.parse_args()
    E = elfmap.Elf(a.elf)
    n = 0
    with open(a.out, "w", newline="\n") as out:
        for f in sorted((f for f in E.functions() if f["tier"] in ("game", "engine") and f["size"]), key=lambda f: f["addr"]):
            out.write(json.dumps(facts(E, f), separators=(",", ":")) + "\n")
            n += 1
    print("wrote", n)


if __name__ == "__main__":
    main()
