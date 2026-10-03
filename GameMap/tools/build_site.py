#!/usr/bin/env python3
"""Build the data for the GameMap progress site (GameMap/site).

    python3 GameMap/tools/build_site.py [--snapshot "label"]

Reads committed data only (GameMap/data/*, the Rust sources in _bevy/src) and writes
  site/data.js         window.GM = {...}: every function of the executable with its decoding and runtime state
  site/rust/<hi>.json  the Rust that ports, hooks or drives each function (our own code), sharded by address >> 16
No game code is embedded: the optional code viewer reads a local, git-ignored "code pack" (tools/build_code_pack.py).

Two independent measurements per function:

Decoding (game + engine code only; middleware is "untracked") - highest applicable wins:
  reviewed    a human-written annotation with evidence exists (data/annotations.json, verified_by set)
  proven      lifted pseudo-C proven equivalent to the machine code by randomized emulation (tools/verify_lift.py)
  partial     the lifter produced code but not every path was exercised / some paths were unverifiable
  decompiled  Ghidra decompiles it cleanly (no bad instructions, no warnings) - readable but NOT proven
  flagged     Ghidra output contains bad instructions/warnings or the function needs manual work
  mapped      only the map facts (name, unit, class, callers) exist

Runtime (every function; data/runtime_functions.tsv from tools/ingest_runtime.py + the Rust sources):
  port    hand-written Rust cites the function (address or name)
  host    a Rust host function replaces it in the remake's PowerPC VM (or observes it)
  run     the original code ran in the VM during the recorded scenarios
  native  allowed to run as original code, not reached by any recorded scenario
  stub    the VM skips it (returns 0)
  trap    an engine service with no Rust version yet; entering it stops the VM
"""
import argparse
import bisect
import collections
import datetime
import glob
import hashlib
import json
import os
import re
import shutil

import mwdemangle
import rustrefs

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")
DATA = os.path.join(ROOT, "data")
SITE = os.path.join(ROOT, "site")
BEVY = os.path.join(ROOT, "..", "_bevy")

STATES = ["reviewed", "proven", "partial", "decompiled", "flagged", "mapped"]
RSTATES = ["port", "host", "run", "native", "stub", "trap"]
VMKINDS = ["native", "host", "observe", "stub", "trap"]


def rd_tsv(path):
    rows = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            rows.append(line.rstrip("\n").split("\t"))
    return rows


def rd_jsonl(path):
    out = {}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                if line.strip():
                    d = json.loads(line)
                    out[d["addr"]] = d
    return out


def rd_runtime(path):
    """(scenario labels, {addr: (size, vm, mask, entries, symbol)}) - empty when the remake data was never ingested."""
    labels, rows = [], {}
    if not os.path.exists(path):
        return labels, rows
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("# scenarios"):
                labels = line.rstrip("\n").split("\t")[1:]
            elif not line.startswith("#") and line.strip():
                a, size, vm, mask, entries, sym = line.rstrip("\n").split("\t")
                rows[int(a, 16)] = (int(size), vm, int(mask), int(entries), sym)
    return labels, rows


def decoding_state(g, l, an):
    st, why = "mapped", ""
    if g:
        if g.get("bad") or g.get("warn"):
            st, why = "flagged", "ghidra: " + ("bad instructions" if g.get("bad") else "warnings")
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
    return st, why, ls or ""


def runtime_state(vm, entries, ref_kinds):
    if "port" in ref_kinds:
        return "port"
    if vm in ("host", "observe"):
        return "host"
    if vm == "native":
        return "run" if entries else "native"
    return vm  # stub / trap


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--snapshot", default="", help="append a history snapshot with this label")
    ap.add_argument("--lift", default=os.path.join(DATA, "lift_status.jsonl"))
    a = ap.parse_args()

    ghidra = rd_jsonl(os.path.join(DATA, "ghidra_metrics.jsonl"))
    facts = rd_jsonl(os.path.join(DATA, "function_facts.jsonl"))
    lift = rd_jsonl(a.lift)
    ann_path = os.path.join(DATA, "annotations.json")
    ann = json.load(open(ann_path, encoding="utf-8")) if os.path.exists(ann_path) else {}
    scenarios, runtime = rd_runtime(os.path.join(DATA, "runtime_functions.tsv"))

    callers, callees = collections.Counter(), collections.Counter()
    for r in rd_tsv(os.path.join(DATA, "callgraph.tsv")):
        callees[int(r[0], 16)] += 1
        callers[int(r[1], 16)] += 1

    # ---- units (all tiers, link order) -------------------------------------------------------------------------
    units, unit_ix, ranges = [], {}, []
    for r in rd_tsv(os.path.join(DATA, "source_units.tsv")):
        start, end, tier, sub, unit = int(r[0], 16), int(r[1], 16), r[4], r[5], r[6]
        if unit not in unit_ix:
            unit_ix[unit] = len(units)
            units.append({"u": unit, "t": tier, "y": sub, "a": start})
        ranges.append((start, end, unit_ix[unit]))
    ranges.sort()
    rstarts = [x[0] for x in ranges]

    def unit_at(addr):
        k = bisect.bisect_right(rstarts, addr) - 1
        return ranges[max(k, 0)][2]

    # ---- functions: GameMap's game/engine tables, then every other ELF function from the runtime table ----------
    mapped = {}
    for path in sorted(glob.glob(os.path.join(DATA, "functions", "*.tsv"))):
        tier, sub = os.path.basename(path)[:-4].split("-", 1)
        for r in rd_tsv(path):
            addr = int(r[0], 16)
            mapped[addr] = {"s": int(r[1]), "c": r[2], "n": (r[2] + "::" if r[2] else "") + r[3], "g": r[4],
                            "u": r[6], "t": tier, "y": sub, "m": r[7] if len(r) > 7 else ""}
    allf = {}
    for addr, (size, vm, mask, entries, sym) in runtime.items():
        if addr in mapped:
            continue
        d = mwdemangle.demangle(sym)
        u = units[unit_at(addr)]
        allf[addr] = {"s": size, "n": (d["cls"] + "::" if d["cls"] else "") + d["method"],
                      "g": d["args"] if d["parsed"] else "", "u": u["u"], "t": u["t"], "y": u["y"], "m": sym}
    allf.update(mapped)
    addrs = sorted(allf)

    # ---- Rust references ----------------------------------------------------------------------------------------
    by_symbol = {f["m"]: ad for ad, f in allf.items() if f["m"]}
    by_name = collections.defaultdict(list)
    for ad in addrs:
        by_name[allf[ad]["n"]].append(ad)
    refs = rustrefs.scan(os.path.join(BEVY, "src"), BEVY, [(ad, allf[ad]["s"]) for ad in addrs], by_symbol, by_name)

    # ---- rows ---------------------------------------------------------------------------------------------------
    groups, group_ix, whys, why_ix, kinds, kind_ix = [], {}, [], {}, [], {}

    def ix(table, index, key):
        if key not in index:
            index[key] = len(table)
            table.append(key)
        return index[key]

    rows = []
    for ad in addrs:
        f = allf[ad]
        tracked = ad in mapped
        if tracked:
            st, why, ls = decoding_state(ghidra.get(ad), lift.get(ad, {}), ann.get(f["m"]))
        else:
            st, why, ls = "", "", ""
        size, vm, mask, entries, _ = runtime.get(ad, (f["s"], "native", 0, 0, ""))
        rr = refs.get(ad, [])
        rst = runtime_state(vm, entries, {r["kind"] for r in rr})
        g, l, fa, an = ghidra.get(ad), lift.get(ad, {}), facts.get(ad, {}), ann.get(f["m"]) or {}
        rows.append([
            ad, f["s"],
            ix(units, unit_ix, f["u"]) if f["u"] in unit_ix else unit_at(ad),
            ix(groups, group_ix, (f["t"], f["y"])),
            STATES.index(st) if st else -1,
            ix(whys, why_ix, why) if why else -1,
            ix(kinds, kind_ix, fa["kind"]) if fa.get("kind") else -1,
            callers.get(ad, 0), callees.get(ad, 0),
            [g.get("lines", 0), g.get("loops", 0), g.get("gotos", 0), g.get("switch", 0), g.get("warn", 0)] if g else 0,
            [l.get("ok", 0), l.get("blocks", 0), l.get("covered", 0), l.get("stmts", 0)] if ls in ("verified", "partial") else 0,
            ls,
            RSTATES.index(rst), VMKINDS.index(vm) if vm in VMKINDS else 0, mask, entries, len(rr),
            f["n"], f["g"], f["m"] if f["m"] != f["n"] else "",
            an.get("summary") or l.get("sum") or "",
            {k: v for k, v in an.items() if k in ("doc", "verified_by")} or 0,
        ])

    # ---- Rust shards (published: our own code) ------------------------------------------------------------------
    rust_dir = os.path.join(SITE, "rust")
    shutil.rmtree(rust_dir, ignore_errors=True)
    os.makedirs(rust_dir)
    shards = collections.defaultdict(dict)
    for ad, rr in refs.items():
        shards["%04x" % (ad >> 16)]["%08x" % ad] = rr
    for k, v in shards.items():
        with open(os.path.join(rust_dir, k + ".json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(v, fh, separators=(",", ":"))

    # ---- totals and history -------------------------------------------------------------------------------------
    def totals(sel):
        fc, bc, rfc, rbc = collections.Counter(), collections.Counter(), collections.Counter(), collections.Counter()
        for r in sel:
            if r[4] >= 0:
                fc[STATES[r[4]]] += 1
                bc[STATES[r[4]]] += r[1]
            rfc[RSTATES[r[12]]] += 1
            rbc[RSTATES[r[12]]] += r[1]
        return {"n": len(sel), "s": sum(r[1] for r in sel),
                "fc": {k: fc.get(k, 0) for k in STATES}, "bc": {k: bc.get(k, 0) for k in STATES},
                "rfc": {k: rfc.get(k, 0) for k in RSTATES}, "rbc": {k: rbc.get(k, 0) for k in RSTATES}}

    tracked_rows = [r for r in rows if r[4] >= 0]
    tot, everything = totals(tracked_rows), totals(rows)
    now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    hist_path = os.path.join(DATA, "progress_history.json")
    hist = json.load(open(hist_path, encoding="utf-8")) if os.path.exists(hist_path) else []
    if a.snapshot:
        hist = [h for h in hist if h["label"] != a.snapshot]
        snap = {"label": a.snapshot, "date": now}
        snap.update(tot)
        snap["all"] = {k: everything[k] for k in ("n", "s", "rfc", "rbc")}
        hist.append(snap)
        with open(hist_path, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(hist, fh, indent=1)

    reconstruction_path = os.path.join(DATA, "reconstruction_progress.json")
    reconstruction = json.load(open(reconstruction_path, encoding="utf-8")) if os.path.exists(reconstruction_path) else None
    proof_path = os.path.join(BEVY, "docs", "proof-report.json")
    if reconstruction and os.path.exists(proof_path):
        proof = json.load(open(proof_path, encoding="utf-8"))
        # The latest actual report is authoritative; never infer a full pass.
        reconstruction["proof"] = {key: proof.get(key) for key in ("generated", "passed", "passed_count", "total")}

    meta = json.load(open(os.path.join(DATA, "elf_facts.json"), encoding="utf-8"))
    gm = {
        "generated": now,
        "reconstruction": reconstruction,
        "elf_sha256": meta.get("elf_sha256"),
        "states": STATES,
        "rstates": RSTATES,
        "vmkinds": VMKINDS,
        "scenarios": scenarios,
        "runtime": bool(runtime),
        "units": units,
        "groups": [{"t": t, "y": y} for t, y in groups],
        "whys": whys,
        "kinds": kinds,
        "cols": ["a", "s", "unit", "group", "st", "why", "kind", "fi", "fo", "gh", "lf", "ls",
                 "rst", "vm", "scen", "entries", "refs", "n", "g", "m", "sum", "ev"],
        "funcs": rows,
        "history": hist,
        "lift_tested": len(lift),
        "repo": "https://github.com/Kxboo/EA-Playground/blob/main/_bevy/",
    }
    with open(os.path.join(SITE, "data.js"), "w", encoding="utf-8", newline="\n") as fh:
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

    def pc(d, keys, s):
        return "%.1f%%" % (100.0 * sum(d[k] for k in keys) / s if s else 0)
    print("functions %d (%d game+engine), units %d, rust refs on %d functions" % (len(rows), len(tracked_rows), len(units), len(refs)))
    print("game+engine decoding: proven+reviewed %s, decoded %s (bytes)" % (
        pc(tot["bc"], ["reviewed", "proven"], tot["s"]), pc(tot["bc"], STATES[:4], tot["s"])))
    print("runtime (all): " + ", ".join("%s %s" % (k, pc(everything["rbc"], [k], everything["s"])) for k in RSTATES))
    print("data.js %.0f KB" % (os.path.getsize(os.path.join(SITE, "data.js")) / 1024))


if __name__ == "__main__":
    main()
