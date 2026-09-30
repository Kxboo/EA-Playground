#!/usr/bin/env python3
"""Build the data for the GameMap progress site (GameMap/site).

    python3 GameMap/tools/build_site.py [--snapshot "label"]

Reads only committed data (GameMap/data/*), writes GameMap/site/data.js (window.GM = {...}).
No code is embedded: the optional code viewer reads a local, git-ignored "code pack" (tools/build_code_pack.py).

Per-function state (highest applicable wins):
  reviewed    a human-written annotation with evidence exists (data/annotations.json, verified_by set) - the strongest state
  proven      the lifted pseudo-C was proven equivalent to the original machine code by randomized emulation (tools/verify_lift.py)
              or the function is covered by a byte-exact reference model verified against the original
  partial     the lifter produced code but not every path was exercised / some paths were unverifiable
  decompiled  Ghidra decompiles it cleanly (no bad instructions, no warnings) - readable but NOT proven
  flagged     Ghidra output contains bad instructions/warnings or the function needs manual work
  mapped      only the map facts (name, unit, class, callers) exist
Scope: game + engine code. Middleware (SDK, Havok, Lua, ...) is listed for size only and excluded from the progress percentages.
"""
import argparse
import collections
import datetime
import glob
import hashlib
import json
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")
DATA = os.path.join(ROOT, "data")
SITE = os.path.join(ROOT, "site")

STATES = ["reviewed", "proven", "partial", "decompiled", "flagged", "mapped"]
KINDS = {"leaf": 0, "logic": 1, "loop": 2, "switch": 3, "float": 4, "ps": 5, "vcall": 6, "other": 7}


def rd_tsv(path):
    rows = []
    with open(path) as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            rows.append(line.rstrip("\n").split("\t"))
    return rows


def rd_jsonl(path):
    out = {}
    if not os.path.exists(path):
        return out
    with open(path) as fh:
        for line in fh:
            if line.strip():
                d = json.loads(line)
                out[d["addr"]] = d
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--snapshot", default="", help="append a history snapshot with this label")
    ap.add_argument("--lift", default=os.path.join(DATA, "lift_status.jsonl"))
    a = ap.parse_args()

    ghidra = rd_jsonl(os.path.join(DATA, "ghidra_metrics.jsonl"))
    facts = rd_jsonl(os.path.join(DATA, "function_facts.jsonl"))
    lift = rd_jsonl(a.lift)
    ann = json.load(open(os.path.join(DATA, "annotations.json"))) if os.path.exists(os.path.join(DATA, "annotations.json")) else {}

    callers = collections.Counter()
    callees = collections.Counter()
    for r in rd_tsv(os.path.join(DATA, "callgraph.tsv")):
        callees[int(r[0], 16)] += 1
        callers[int(r[1], 16)] += 1

    funcs = []
    for path in sorted(glob.glob(os.path.join(DATA, "functions", "*.tsv"))):
        base = os.path.basename(path)[:-4]
        tier, sub = base.split("-", 1)
        for r in rd_tsv(path):
            addr = int(r[0], 16)
            size = int(r[1])
            cls, meth, args, const, unit, mangled = r[2], r[3], r[4], r[5], r[6], r[7] if len(r) > 7 else ""
            name = (cls + "::" if cls else "") + meth
            g = ghidra.get(addr, {})
            f = facts.get(addr, {})
            l = lift.get(addr, {})
            an = ann.get(mangled)
            st = "mapped"
            why = ""
            if g:
                if g.get("bad") or g.get("warn"):
                    st = "flagged"
                    why = "ghidra: " + ("bad instructions" if g.get("bad") else "warnings")
                elif g.get("ok"):
                    st = "decompiled"
            ls = l.get("status")
            if ls == "partial":
                st = "partial"
            if ls == "verified":
                st = "proven"
            if ls in ("unsupported", "failed", "untestable") and not why:
                why = "lift: " + (l.get("reason") or ls)
            if an and an.get("verified_by"):
                st = "reviewed"
            funcs.append({
                "a": addr, "s": size, "n": name, "g": args, "c": cls, "u": unit, "t": tier, "y": sub,
                "st": st, "why": why,
                "k": f.get("kind", "other"), "ni": f.get("ni", size // 4),
                "fi": callers.get(addr, 0), "fo": callees.get(addr, 0),
                "gh": [g.get("lines", 0), g.get("loops", 0), g.get("gotos", 0), g.get("switch", 0), g.get("warn", 0)] if g else None,
                "lf": [l.get("ok", 0), l.get("blocks", 0), l.get("covered", 0), l.get("stmts", 0)] if ls in ("verified", "partial") else None,
                "ls": ls or "",
                "sum": (an or {}).get("summary") or l.get("sum") or "",
                "ev": {k: v for k, v in (an or {}).items() if k in ("doc", "verified_by")} or None,
                "m": mangled,
            })
    funcs.sort(key=lambda x: x["a"])

    # ---- units -------------------------------------------------------------
    units = []
    fu = collections.defaultdict(list)
    for f in funcs:
        fu[(f["u"], f["t"], f["y"])].append(f)
    mw = []
    for r in rd_tsv(os.path.join(DATA, "source_units.tsv")):
        start, end, nbytes, nfuncs, tier, sub, unit = r[0], r[1], int(r[2]), int(r[3]), r[4], r[5], r[6]
        if tier == "middleware":
            mw.append({"u": unit, "s": nbytes, "f": nfuncs, "y": sub, "a": int(start, 16)})
    for (u, t, y), fl in fu.items():
        cnt = collections.Counter(x["st"] for x in fl)
        byt = collections.Counter()
        for x in fl:
            byt[x["st"]] += x["s"]
        units.append({"u": u, "t": t, "y": y, "a": min(x["a"] for x in fl), "n": len(fl), "s": sum(x["s"] for x in fl),
                      "fc": {k: cnt.get(k, 0) for k in STATES}, "bc": {k: byt.get(k, 0) for k in STATES}})
    units.sort(key=lambda x: x["a"])

    # ---- totals --------------------------------------------------------------
    def totals(rows):
        fc = collections.Counter(x["st"] for x in rows)
        bc = collections.Counter()
        for x in rows:
            bc[x["st"]] += x["s"]
        return {"n": len(rows), "s": sum(x["s"] for x in rows),
                "fc": {k: fc.get(k, 0) for k in STATES}, "bc": {k: bc.get(k, 0) for k in STATES}}

    subs = collections.defaultdict(list)
    for f in funcs:
        subs[(f["t"], f["y"])].append(f)
    sublist = [dict(totals(v), t=k[0], y=k[1]) for k, v in sorted(subs.items())]

    blockers = collections.Counter()
    blocker_bytes = collections.Counter()
    for f in funcs:
        if f["st"] in ("decompiled", "flagged", "mapped") and f["why"]:
            blockers[f["why"]] += 1
            blocker_bytes[f["why"]] += f["s"]
    blk = [{"why": k, "n": v, "s": blocker_bytes[k]} for k, v in blockers.most_common()]

    meta = json.load(open(os.path.join(DATA, "elf_facts.json")))
    tot = totals(funcs)
    now = datetime.datetime.utcnow().replace(microsecond=0).isoformat() + "Z"

    hist_path = os.path.join(DATA, "progress_history.json")
    hist = json.load(open(hist_path)) if os.path.exists(hist_path) else []
    if a.snapshot:
        hist = [h for h in hist if h["label"] != a.snapshot]
        hist.append({"label": a.snapshot, "date": now, "n": tot["n"], "s": tot["s"], "fc": tot["fc"], "bc": tot["bc"]})
        json.dump(hist, open(hist_path, "w"), indent=1)

    reconstruction_path = os.path.join(DATA, "reconstruction_progress.json")
    reconstruction = json.load(open(reconstruction_path)) if os.path.exists(reconstruction_path) else None
    proof_path = os.path.join(ROOT, "..", "_bevy", "docs", "proof-report.json")
    if reconstruction and os.path.exists(proof_path):
        proof = json.load(open(proof_path))
        # The latest actual report is authoritative; never infer a full pass.
        reconstruction["proof"] = {key: proof.get(key) for key in ("generated", "passed", "passed_count", "total")}

    gm = {
        "generated": now,
        "reconstruction": reconstruction,
        "elf_sha256": meta.get("elf_sha256"),
        "states": STATES,
        "total": tot,
        "middleware": {"n": sum(x["f"] for x in mw), "s": sum(x["s"] for x in mw)},
        "subsystems": sublist,
        "units": units,
        "middleware_units": sorted(mw, key=lambda x: -x["s"])[:200],
        "functions": funcs,
        "blockers": blk,
        "history": hist,
        "lift_run": bool(lift),
        "lift_tested": len(lift),
    }
    os.makedirs(SITE, exist_ok=True)
    with open(os.path.join(SITE, "data.js"), "w") as fh:
        fh.write("window.GM = ")
        json.dump(gm, fh, separators=(",", ":"))
        fh.write(";\n")
    # Version public assets so returning visitors see a newly published snapshot.
    digest = hashlib.sha256()
    for name in ("data.js", "app.js", "style.css"):
        with open(os.path.join(SITE, name), "rb") as fh:
            digest.update(fh.read())
    index_path = os.path.join(SITE, "index.html")
    with open(index_path, encoding="utf-8") as fh:
        index = fh.read()
    index = re.sub(r'((?:src|href)="(?:data\.js|app\.js|style\.css))(?:\?v=[^"]+)?',
                   lambda m: m[1] + "?v=" + digest.hexdigest()[:12], index)
    with open(index_path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(index)
    print("functions", len(funcs), "units", len(units), "states", tot["fc"])
    print("data.js %.0f KB" % (os.path.getsize(os.path.join(SITE, "data.js")) / 1024))


if __name__ == "__main__":
    main()
