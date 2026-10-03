#!/usr/bin/env python3
"""Build the LOCAL, git-ignored code pack the progress site's code panes read.

    python3 GameMap/tools/build_code_pack.py --elf path/to/playgroundz.elf [--ghidra-c path/to/decomp.c]

Writes GameMap/site/code/<addr >> 14, hex>.json = { "<addr hex>": {"asm": ..., "ghidra": ..., "gsrc": ..., "lifted": ...} }
for every function of the executable (all tiers).
  asm     original PowerPC listing from your ELF (address, word, instruction; branch targets named)
  ghidra  Ghidra decompilation: GameMap/decomp/ghidra.jsonl (tools/ghidra/ExportDecomp.py) first, then --ghidra-c, a full
          program export with one `//==== <symbol> @ <addr>` header per function (default: $DECOMP_C)
  lifted  pseudo-C from tools/lift.py: GameMap/decomp/lifted/*.c (tools/lift_all.py) and GameMap/decomp/lifted.jsonl
The pack contains code derived from EA's executable, so site/code/ is git-ignored: keep it local, do not publish it.
"""
import argparse
import glob
import json
import os
import re
import shutil

import capstone
from elftools.elf.elffile import ELFFile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")
SHIFT = 14


def load_ghidra(path_c):
    out, src = {}, {}
    gp = os.path.join(ROOT, "decomp", "ghidra.jsonl")
    if os.path.exists(gp):
        for line in open(gp, encoding="utf-8"):
            d = json.loads(line)
            if d.get("code"):
                out[d["addr"]] = d["code"].strip("\n")
                src[d["addr"]] = "ghidra.jsonl"
    if path_c and os.path.exists(path_c):
        head = re.compile(r"^//==== (.*) @ ([0-9a-f]{8})$")
        cur, buf = None, []

        def flush():
            if cur is not None and cur not in out:
                out[cur] = "".join(buf).strip("\n")
                src[cur] = os.path.basename(path_c)
        for line in open(path_c, encoding="utf-8", errors="replace"):
            m = head.match(line.rstrip("\n"))
            if m:
                flush()
                cur, buf = int(m.group(2), 16), []
            elif cur is not None:
                buf.append(line)
        flush()
    return out, src


def load_lifted():
    out = {}
    for path in glob.glob(os.path.join(ROOT, "decomp", "lifted", "*.c")):
        txt = open(path, encoding="utf-8").read()
        for chunk in re.split(r"\n(?=// \[(?:verified|partial|untestable)\]\n)", txt):
            m = re.match(r"// \[(\w+)\]\n(.*)", chunk, re.S)
            am = m and re.search(r"@0x([0-9a-f]{8})", m.group(2))
            if am:
                out[int(am.group(1), 16)] = "// status: %s\n%s" % (m.group(1), m.group(2).strip("\n"))
    lp = os.path.join(ROOT, "decomp", "lifted.jsonl")
    if os.path.exists(lp):
        for line in open(lp, encoding="utf-8"):
            d = json.loads(line)
            out.setdefault(d["addr"], d["code"])
    return out


def disasm(md, addr, data, names):
    out = []
    for off in range(0, len(data) - 3, 4):
        word = data[off:off + 4].hex()
        ins = next(md.disasm(data[off:off + 4], addr + off), None)
        if ins is None:
            out.append("%08x  %s  .word 0x%s" % (addr + off, word, word))
            continue
        op = ins.op_str
        if ins.mnemonic.startswith("b") and op:
            m = re.search(r"0x([0-9a-f]+)$", op)
            if m:
                t = int(m.group(1), 16)
                if t in names:
                    op = op[:m.start()] + names[t]
                elif addr <= t < addr + len(data):
                    op = op[:m.start()] + ".L_%08x" % t
        out.append(("%08x  %s  %-8s %s" % (addr + off, word, ins.mnemonic, op)).rstrip())
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    ap.add_argument("--ghidra-c", default=os.environ.get("DECOMP_C", ""))
    a = ap.parse_args()

    elf = ELFFile(open(a.elf, "rb"))
    secs = [(s["sh_addr"], s.data()) for s in elf.iter_sections() if s.name in (".init", ".text")]
    funcs = sorted({(s["st_value"], s["st_size"], s.name) for s in elf.get_section_by_name(".symtab").iter_symbols()
                    if s["st_info"]["type"] == "STT_FUNC" and s["st_size"] > 0})
    names = {f[0]: f[2] for f in funcs}
    ghidra, gsrc = load_ghidra(a.ghidra_c)
    lifted = load_lifted()
    md = capstone.Cs(capstone.CS_ARCH_PPC, capstone.CS_MODE_32 | capstone.CS_MODE_BIG_ENDIAN | capstone.CS_MODE_PS)

    out = os.path.join(ROOT, "site", "code")
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    shards = {}
    for addr, size, _name in funcs:
        body = next((d[addr - b:addr - b + size] for b, d in secs if b <= addr < b + len(d)), b"")
        rec = {"asm": disasm(md, addr, body, names)}
        if addr in ghidra:
            rec["ghidra"], rec["gsrc"] = ghidra[addr], gsrc[addr]
        if addr in lifted:
            rec["lifted"] = lifted[addr]
        shards.setdefault("%05x" % (addr >> SHIFT), {})["%08x" % addr] = rec
    for k, v in shards.items():
        with open(os.path.join(out, k + ".json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(v, fh, separators=(",", ":"))
    print("code pack: %d functions in %d shards -> %s" % (len(funcs), len(shards), out))
    print("ghidra: %d, lifted: %d" % (len(ghidra), len(lifted)))


if __name__ == "__main__":
    main()
