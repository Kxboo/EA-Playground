#!/usr/bin/env python3
"""Build the LOCAL, git-ignored code pack the progress site's Compare/Compiled/Ghidra/Lifted tabs read.

    python3 GameMap/tools/build_code_pack.py --elf path/to/playgroundz.elf

Writes GameMap/site/code/<unit>.json = { "<addr hex>": {"asm": ..., "ghidra": ..., "lifted": ...} } for every game/engine function.
  asm     original PowerPC listing (from your ELF)
  ghidra  Ghidra 11 decompilation            (GameMap/decomp/ghidra.jsonl, produced by tools/ghidra/ExportDecomp.py)
  lifted  verified pseudo-C from tools/lift.py (GameMap/decomp/lifted/*.c, produced by tools/lift_all.py)
The pack contains code derived from EA's executable, so site/code/ is git-ignored: keep it local, do not publish it.
"""
import argparse
import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import elfmap  # noqa: E402

ROOT = os.path.join(HERE, "..")


def safe(u):
    return re.sub(r"[^A-Za-z0-9._-]", "_", u)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    a = ap.parse_args()
    E = elfmap.Elf(a.elf)
    ghidra = {}
    gp = os.path.join(ROOT, "decomp", "ghidra.jsonl")
    if os.path.exists(gp):
        for line in open(gp):
            d = json.loads(line)
            if d.get("code"):
                ghidra[d["addr"]] = d["code"].strip("\n")
    lifted = {}
    for path in glob.glob(os.path.join(ROOT, "decomp", "lifted", "*.c")):
        txt = open(path).read()
        for chunk in re.split(r"\n(?=// \[(?:verified|partial|untestable)\]\n)", txt):
            m = re.match(r"// \[(\w+)\]\n(.*)", chunk, re.S)
            if not m:
                continue
            am = re.search(r"@0x([0-9a-f]{8})", m.group(2))
            if am:
                lifted[int(am.group(1), 16)] = "// status: %s\n%s" % (m.group(1), m.group(2).strip("\n"))
    out = os.path.join(ROOT, "site", "code")
    os.makedirs(out, exist_ok=True)
    shards = {}
    n = 0
    for f in E.functions():
        if f["tier"] not in ("game", "engine") or not f["size"]:
            continue
        rec = {"asm": E.disasm_text(f["addr"], f["size"])}
        if f["addr"] in ghidra:
            rec["ghidra"] = ghidra[f["addr"]]
        if f["addr"] in lifted:
            rec["lifted"] = lifted[f["addr"]]
        shards.setdefault(safe(f["file"]), {})["%08x" % f["addr"]] = rec
        n += 1
    for k, v in shards.items():
        with open(os.path.join(out, k + ".json"), "w") as fh:
            json.dump(v, fh, separators=(",", ":"))
    print("code pack: %d functions in %d shards -> %s" % (n, len(shards), out))
    print("ghidra: %d, lifted: %d" % (len(ghidra), len(lifted)))


if __name__ == "__main__":
    main()
