#!/usr/bin/env python3
"""Probe EA "VLT" attribute databases (db.vlt + db.bin) using the executable's own hash.

    python3 GameMap/tools/vlt_probe.py --vlt db.vlt --bin db.bin [--elf playgroundz.elf] [--out GameMap/data/vlt_schema.tsv]

What it establishes (see GameMap/docs/07-data-formats-and-hashes.md):
  * chunk table (Vers / DepN / StrN / DatN / ExpN / PtrN), each chunk = tag[4] + size[4, BE, includes the 8-byte header]
  * ExpN = export table: 24-byte records {name_hash u64, type_hash u64, size u32, offset u32}, big-endian,
    `offset` is an ABSOLUTE file offset; records tile the DatN chunk. Exports are ordered class-first: one
    `Attrib::ClassLoadData` record followed by that class's `Attrib::CollectionLoadData` records.
  * keys are Attrib::StringHash64 (Bob Jenkins lookup8, seed 0xABCDEF0011223344) of the name -- see hashes.py
  * a collection block starts {collection_hash u64, class_hash u64, parent_hash u64, ...} and then lists field
    entries whose first word is the field-name hash.
Names are recovered by hashing candidate strings (the .bin string pool + every string in the ELF). Only schema
(table names, row counts, field names) is reported; row *values* are not written anywhere.
"""
import argparse
import collections
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hashes  # noqa: E402


def chunks(d):
    o, out = 0, []
    while o + 8 <= len(d):
        tag = d[o:o + 4]
        sz = struct.unpack(">I", d[o + 4:o + 8])[0]
        if not tag.isalpha() or sz < 8 or o + sz > len(d):
            break
        out.append((tag.decode(), o, sz))
        o += sz
    return out


def candidates(bin_path=None, elf_path=None):
    c = set()
    if bin_path:
        bn = open(bin_path, "rb").read()
        for m in re.finditer(rb"[\x21-\x7e]{2,}", bn):
            c.add(m.group().decode())
    if elf_path:
        import elfmap
        E = elfmap.Elf(elf_path)
        for ad, sz, off, nm, _ in E.secs:
            if nm in (".rodata", ".data", ".sdata", ".sdata2"):
                for m in re.finditer(rb"[\x20-\x7e]{3,}", E.raw[off:off + sz]):
                    s = m.group().decode()
                    c.add(s)
                    c.update(re.findall(r"[A-Za-z0-9_:]{3,}", s))
    return c


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vlt", required=True)
    ap.add_argument("--bin")
    ap.add_argument("--elf")
    ap.add_argument("--out")
    a = ap.parse_args()
    vlt = open(a.vlt, "rb").read()
    ch = chunks(vlt)
    print("chunks:", [(t, o, s) for t, o, s in ch])
    H = {}
    for s in candidates(a.bin, a.elf):
        H.setdefault(hashes.attrib_hash64(s), s)
    ex = next((o, s) for t, o, s in ch if t == "ExpN")
    count = struct.unpack(">I", vlt[ex[0] + 12:ex[0] + 16])[0]
    ents = []
    for i in range(count):
        o = ex[0] + 16 + 24 * i
        name, typ, sz, off = struct.unpack(">QQII", vlt[o:o + 24])
        ents.append((name, typ, sz, off))
    nm = lambda h: H.get(h, "%016x" % h)
    classes = collections.OrderedDict()
    cur = None
    other = []
    for name, typ, sz, off in ents:
        t = H.get(typ)
        if t == "Attrib::ClassLoadData":
            cur = name
            classes[cur] = {"cols": 0, "fields": collections.OrderedDict(), "unres": 0}
        elif t == "Attrib::CollectionLoadData" and cur is not None:
            c = classes[cur]
            c["cols"] += 1
            blk = vlt[off:off + sz]
            for i in range(24, sz - 7, 4):
                k = struct.unpack(">Q", blk[i:i + 8])[0]
                if k in H and not H[k].startswith(("EA::Reflection", "Attrib::", "Enums::")):
                    c["fields"].setdefault(H[k], 0)
        else:
            other.append((name, typ, sz, off))
    print("exports: %d  classes: %d  collections: %d  other: %d" %
          (len(ents), len(classes), sum(c["cols"] for c in classes.values()), len(other)))
    rows = []
    for ch_, c in classes.items():
        rows.append((nm(ch_), c["cols"], ",".join(c["fields"])))
        print("%-32s collections=%-4d fields=%s" % (nm(ch_), c["cols"], ",".join(list(c["fields"])[:14])))
    if a.out:
        with open(a.out, "w", newline="\n") as f:
            f.write("# table(class)\tcollections(rows)\tfield names resolvable from strings (schema only; no values)\n")
            for r in rows:
                f.write("\t".join(str(x) for x in r) + "\n")


if __name__ == "__main__":
    main()
