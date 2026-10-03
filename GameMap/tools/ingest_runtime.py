#!/usr/bin/env python3
"""Record what the Rust/Bevy remake's PowerPC VM does with every function: GameMap/data/runtime_functions.tsv.

    python3 GameMap/tools/ingest_runtime.py --elf playgroundz.elf --classify classify.tsv --cover 'cov_*.tsv'

Inputs come from the remake (`_bevy`):
  --classify  `cargo run --release --bin mglab -- classify OUT` - every hook the gekko VM installs (host / observe / stub / trap);
              functions without a hook run as original code ("native")
  --cover     files written by runs with `EAGL_PPC_COVER=FILE` (one `addr<TAB>entries` line per entered function);
              the file name `cov_<id>.tsv` names the scenario (labels below)
Output: one row per ELF function (all tiers, middleware included): addr, size, vm kind, scenario bit mask, entry count, symbol.
The scenario labels are written to the header. No code is recorded - only measurements.
"""
import argparse
import glob
import os

from elftools.elf.elffile import ELFFile

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "data", "runtime_functions.tsv")

SCENARIO_LABELS = {
    "mg0": "Dart Shootout", "mg1": "RC Cars", "mg2": "Tetherball", "mg3": "Dodgeball", "mg4": "Footie",
    "mg5": "Paper Airplanes", "mg6": "Wallball", "mg8": "Free Throw", "wrace": "World: dare, race, sticker award",
    "wdrib": "World: Dribbling", "wbug": "World: area gate + Bug Hunt", "wking": "World: Sticker King gauntlet",
    "wbook": "World: sticker book + report card"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    ap.add_argument("--classify", required=True)
    ap.add_argument("--cover", default="", help="glob of coverage files (cov_<id>.tsv)")
    a = ap.parse_args()

    elf = ELFFile(open(a.elf, "rb"))
    funcs = sorted({(s["st_value"], s["st_size"], s.name) for s in elf.get_section_by_name(".symtab").iter_symbols()
                    if s["st_info"]["type"] == "STT_FUNC" and s["st_size"] > 0})

    vm = {}
    for line in open(a.classify, encoding="utf-8"):
        addr, _name, kind = line.rstrip("\n").split("\t")
        vm[int(addr, 16)] = kind

    files = sorted(glob.glob(a.cover)) if a.cover else []
    ids = [os.path.basename(p)[4:-4] if os.path.basename(p).startswith("cov_") else os.path.basename(p) for p in files]
    cov = {}
    for k, path in enumerate(files):
        for line in open(path, encoding="utf-8"):
            addr, n = line.split("\t")
            addr = int(addr, 16)
            mask, total = cov.get(addr, (0, 0))
            cov[addr] = (mask | (1 << k), total + int(n))

    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("# scenarios\t" + "\t".join(SCENARIO_LABELS.get(i, i) for i in ids) + "\n")
        fh.write("# addr\tsize\tvm\tscenario_mask\tentries\tsymbol\n")
        for addr, size, name in funcs:
            mask, total = cov.get(addr, (0, 0))
            fh.write("%08x\t%d\t%s\t%d\t%d\t%s\n" % (addr, size, vm.get(addr, "native"), mask, total, name))
    print("runtime: %d functions, %d hooked, %d entered across %d scenarios -> %s" % (
        len(funcs), sum(1 for f in funcs if vm.get(f[0], "native") != "native"), sum(1 for f in funcs if f[0] in cov), len(ids), OUT))


if __name__ == "__main__":
    main()
