"""Recover the APT<->native command bindings ("AIP handlers") from the executable.

The frontend/HUD movies (APT, a Flash-like player) reach native code through:
  * FSHandlers  -- `fscommand("Name", ...)`  (AIP::RegisterFSHandler,  dispatch `DoJobFS(int, CmdDecomposer)`)
  * LVHandlers  -- `loadVariables("Name",..)` (AIP::RegisterLVHandler,  dispatch `DoJobLV(int, CmdDecomposer, CmdComposer)`)
Each handler class' constructor registers every name with a sequential job index (`li r5, N`);
`DoJob*` switches on that index (usually through a jump table). Both are read from the code here.
"""
import re

import mwdemangle


def _regs_before(rows, i):
    return rows[i - 1]["regs"] if i else {}


def registrations(E):
    """{class: [(kind, index, apt_name, ctor_addr)]} from every constructor calling Register{FS,LV}Handler."""
    targets = {}
    for nm, kind in (("RegisterFSHandler__3AIPFPCcPQ23AIP17FSCommandHandler5i", "FS"),
                     ("RegisterLVHandler__3AIPFPCcPQ23AIP21LoadVariablesHandler5i", "LV")):
        if nm in E.by_name:
            targets[E.by_name[nm][0]] = kind
    out = {}
    scan_subs = {"frontend", "apt-ui-runtime", "trc-wii-requirements", "core-flow"}
    for f in E.functions():
        if f["subsystem"] not in scan_subs or f["size"] < 16:
            continue
        a, sz, n = f["addr"], f["size"], f["name"]
        cls = f["cls"]
        rows = list(E.disasm(a, sz))
        for i, r in enumerate(rows):
            if r["mn"] == "bl" and r["target"] in targets:
                rg = _regs_before(rows, i)
                s = E.cstr(rg.get(3)) if rg.get(3) else None
                idx = rg.get(5)
                if s is not None and idx is not None:
                    out.setdefault(cls, []).append((targets[r["target"]], idx, s, a))
    return out


_COND ={"beq": "eq", "bne": "ne", "blt": "lt", "bge": "ge", "bgt": "gt", "ble": "le"}


def _cond_true(cr, c):
    lt, gt, eq = cr
    return {"eq": eq, "ne": not eq, "lt": lt, "ge": not lt, "gt": gt, "le": not gt}[c]


def _num(x):
    try:
        return int(x, 0)
    except (TypeError, ValueError):
        return None


def _trace(E, rows, byaddr, K, table=None):
    """Concretely execute the dispatch prologue with r4 == K; return symbols called on that path.

    A deliberately small PowerPC subset (constants, add/sub, shifts, table load, compares, branches);
    unknown values make later compares 'unknown' (branch not taken). Enough for the compare-tree and
    jump-table dispatchers the game uses.
    """
    M = 0xFFFFFFFF
    v = {4: K & M, 2: 0x8060A540, 13: 0x80603EE0}  # r2/r13: small-data bases (elfmap.SDA_*)
    ctr = None
    cr = None
    pc = rows[0]["addr"]
    calls = []

    def R(t):
        m = re.fullmatch(r"r(\d+)", t.strip())
        return int(m.group(1)) if m else None
    for _ in range(4000):
        r = byaddr.get(pc)
        if r is None:
            break
        mn, op = r["mn"], r["op"]
        parts = [x.strip() for x in op.split(",")] if op else []
        nxt = pc + 4
        if mn == "li" and len(parts) == 2:
            v[R(parts[0])] = _num(parts[1]) & M if _num(parts[1]) is not None else None
        elif mn == "lis" and len(parts) == 2:
            n = _num(parts[1]); v[R(parts[0])] = (n << 16) & M if n is not None else None
        elif mn in ("addi", "subi") and len(parts) == 3:
            src = v.get(R(parts[1])) if R(parts[1]) not in (0, None) else 0
            n = _num(parts[2])
            v[R(parts[0])] = (src + (n if mn == "addi" else -n)) & M if src is not None and n is not None else None
        elif mn == "ori" and len(parts) == 3:
            src = v.get(R(parts[1])); n = _num(parts[2])
            v[R(parts[0])] = (src | n) & M if src is not None and n is not None else None
        elif mn == "mr" and len(parts) == 2:
            v[R(parts[0])] = v.get(R(parts[1]))
        elif mn == "add" and len(parts) == 3:
            x, y = v.get(R(parts[1])), v.get(R(parts[2]))
            v[R(parts[0])] = (x + y) & M if x is not None and y is not None else None
        elif mn == "slwi" and len(parts) == 3:
            x = v.get(R(parts[1])); n = _num(parts[2])
            v[R(parts[0])] = (x << n) & M if x is not None and n is not None else None
        elif mn == "lwzx" and len(parts) == 3:
            x, y = v.get(R(parts[1])), v.get(R(parts[2]))
            v[R(parts[0])] = E.u32((x + y) & M) if x is not None and y is not None else None
        elif mn == "mtctr" and parts:
            ctr = v.get(R(parts[0]))
        elif mn in ("cmpwi", "cmplwi"):
            m = re.match(r"(?:cr\d, )?r(\d+), (-?\w+)", op)
            cr = None
            if m:
                x = v.get(int(m.group(1))); imm = _num(m.group(2))
                if x is not None and imm is not None:
                    if mn == "cmpwi":
                        xs = x - (1 << 32) if x >= 1 << 31 else x
                        cr = (xs < imm, xs > imm, xs == imm)
                    else:
                        cr = (x < (imm & M), x > (imm & M), x == (imm & M))
        elif mn in _COND:
            if cr is not None and _cond_true(cr, _COND[mn]):
                nxt = r["target"] if r["target"] else int(op, 16)
        elif mn.endswith("lr") and mn[:-2] in _COND and mn != "blr":
            if cr is not None and _cond_true(cr, _COND[mn[:-2]]):
                break
        elif mn == "blr":
            break
        elif mn == "b":
            t = r["target"]
            if t is None or t not in byaddr:  # tail call out of the function
                if t:
                    strs = []
                    for reg in (3, 4, 5, 6, 7, 8):
                        x = v.get(reg)
                        if x is not None and 0x8041CEE0 <= x < 0x80608000:
                            tx = E.cstr(x)
                            if tx and len(tx) >= 2:
                                strs.append(tx)
                    calls.append((E.sym_at(t) or hex(t)) + ("  [%s]" % " | ".join(strs) if strs else ""))
                break
            nxt = t
        elif mn == "bl":
            if r["target"]:
                strs = []
                for reg in (3, 4, 5, 6, 7, 8):
                    x = v.get(reg)
                    if x is not None and 0x8041CEE0 <= x < 0x80608000:
                        t = E.cstr(x)
                        if t and len(t) >= 2:
                            strs.append(t)
                nm = E.sym_at(r["target"]) or hex(r["target"])
                calls.append(nm + ("  [%s]" % " | ".join(strs) if strs else ""))
            tsym = E.sym_at(r["target"]) if r["target"] else ""
            if not (tsym or "").startswith(("__save_", "__restore_")):  # prologue helpers keep r3-r12
                for reg in (0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12):  # volatile registers die across a call
                    v[reg] = None
        elif mn == "bctr":
            if ctr is None or ctr not in byaddr:
                break
            nxt = ctr
        pc = nxt
    return calls


def dispatch_targets(E, cls, kind):
    """{index: [callee names]} by tracing DoJob<kind>__<cls> for each registered index."""
    fn = None
    for n in E.by_name:
        if n.startswith("DoJob%s__" % kind) and mwdemangle.demangle(n)["cls"] == cls:
            fn = n
            break
    if not fn:
        return None, None
    a, sz, _ = E.by_name[fn]
    rows = list(E.disasm(a, sz))
    byaddr = {r["addr"]: r for r in rows}
    table = None
    for i, r in enumerate(rows):
        if r["mn"] == "lwzx":
            m = re.match(r"r\d+, r(\d+), r\d+", r["op"])
            if m:
                table = _regs_before(rows, i).get(int(m.group(1)))
                break
    return (lambda K: _trace(E, rows, byaddr, K, table)), fn


def _pretty_call(c):
    base, _, extra = c.partition("  [")
    return mwdemangle.pretty(base) + (" [" + extra if extra else "")


def all_rows(E):
    regs = registrations(E)
    rows = []
    for cls, L in sorted(regs.items()):
        for kind in sorted({k for k, *_ in L}):
            tracer, fn = dispatch_targets(E, cls, kind)
            for k, idx, name, ctor in sorted((x for x in L if x[0] == kind), key=lambda x: x[1]):
                calls = tracer(idx) if tracer else []
                pretty = [_pretty_call(c) for c in calls
                          if not c.startswith(("GetInstance__", "__save_gpr", "__restore_gpr"))]
                rows.append((cls, kind, idx, name, " ; ".join(pretty[:4]),
                             "%08x" % E.by_name[fn][0] if fn else "", "%08x" % ctor))
    return rows
