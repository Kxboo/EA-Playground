"""A small verified lifter: PowerPC (Gekko integer/FP subset) -> guarded statement IR -> pseudo-C.

Scope (deliberately narrow, so that every output can be *proved* against the machine code):
  * structured control flow: acyclic parts are predicated (guarded statements, `ite` merges); single-entry loops become
    ('loop', ...) statements (no jump tables / irreducible loops),
  * integer ALU, rotate/shift/mask, carry ops, compares, loads/stores (incl. update/indexed forms, lmw/stmw),
  * single/double FP arithmetic, conversions, compares,
  * calls (`bl`, `bctrl`, tail `b`) as opaque events: (target, r3..r10, f1..f8) -> deterministic oracle results.
Anything else raises Unsupported(reason); callers record the reason.

The IR is executed by `run()` (a Python interpreter) and compared with Unicorn by `verify_lift.py`.
"""
import math
import re
import struct

M32 = 0xFFFFFFFF


class Unsupported(Exception):
    pass


class Undefined(Exception):
    """A PowerPC operation with architecturally undefined result was reached (skip that test case)."""


# ----------------------------------------------------------------------------------------------------
# expressions: tuples ('op', ...). leaves: ('c',v) ('init',regname) ('t',id) ('cr',callid,regname) ('sp',)
# ----------------------------------------------------------------------------------------------------
def C(v):
    return ("c", v & M32)


def is_c(e):
    return e[0] == "c"


def mk(op, *a):
    """Constant-folding constructor for pure integer ops."""
    if op in _FOLD and all(x[0] == "c" for x in a):
        try:
            return ("c", _FOLD[op](*[x[1] for x in a]) & M32)
        except Undefined:
            pass
    if op == "add":
        if a[1] == C(0):
            return a[0]
        if a[0] == C(0):
            return a[1]
        if a[0][0] == "add" and a[0][2][0] == "c" and a[1][0] == "c":  # (x + c1) + c2
            return mk("add", a[0][1], C(a[0][2][1] + a[1][1]))
    if op == "sub" and a[1] == C(0):
        return a[0]
    if op in ("or", "xor") and a[1] == C(0):
        return a[0]
    if op == "and" and a[1] == C(M32):
        return a[0]
    if op == "ite" and a[1] == a[2]:
        return a[1]
    if op == "ite" and a[0] == ("c", 1):
        return a[1]
    if op == "ite" and a[0] == ("c", 0):
        return a[2]
    return (op,) + tuple(a)


def _s32(x):
    x &= M32
    return x - (1 << 32) if x & 0x80000000 else x


def _mask(mb, me):
    if mb <= me:
        return ((M32 >> mb) & (M32 << (31 - me))) & M32
    return ((M32 >> mb) | (M32 << (31 - me))) & M32


def _rotl(x, n):
    n &= 31
    x &= M32
    return ((x << n) | (x >> (32 - n))) & M32 if n else x


def _divw(a, b):
    a, b = _s32(a), _s32(b)
    if b == 0 or (a == -(1 << 31) and b == -1):
        raise Undefined("divw")
    q = abs(a) // abs(b)
    return -q if (a < 0) != (b < 0) else q


def _divwu(a, b):
    if b == 0:
        raise Undefined("divwu")
    return a // b


def _sraw(a, n):
    n &= 0x3F
    return (_s32(a) >> min(n, 31)) if n < 32 else (M32 if _s32(a) < 0 else 0)


def _cntlzw(a):
    a &= M32
    return 32 if a == 0 else 32 - a.bit_length()


_FOLD = {
    "add": lambda a, b: a + b, "sub": lambda a, b: a - b, "mul": lambda a, b: a * b,
    "and": lambda a, b: a & b, "or": lambda a, b: a | b, "xor": lambda a, b: a ^ b,
    "nand": lambda a, b: ~(a & b), "nor": lambda a, b: ~(a | b), "eqv": lambda a, b: ~(a ^ b),
    "andc": lambda a, b: a & ~b, "orc": lambda a, b: a | ~b, "neg": lambda a: -a, "not": lambda a: ~a,
    "mulhw": lambda a, b: (_s32(a) * _s32(b)) >> 32, "mulhwu": lambda a, b: (a * b) >> 32,
    "divw": _divw, "divwu": _divwu, "cntlzw": _cntlzw,
    "ext8": lambda a: (a & 0xFF) - 0x100 if a & 0x80 else a & 0xFF,
    "ext16": lambda a: (a & 0xFFFF) - 0x10000 if a & 0x8000 else a & 0xFFFF,
    "slw": lambda a, n: 0 if (n & 0x3F) >= 32 else a << (n & 0x3F),
    "srw": lambda a, n: 0 if (n & 0x3F) >= 32 else (a & M32) >> (n & 0x3F),
    "sraw": _sraw,
}


# ----------------------------------------------------------------------------------------------------
# instruction decoding helpers
# ----------------------------------------------------------------------------------------------------
_REG = re.compile(r"^r(\d+)$")
_FREG = re.compile(r"^f(\d+)$")
_MEM = re.compile(r"^(-?(?:0x)?[0-9a-fA-F]+)\((r\d+)\)$")
_CR = re.compile(r"^cr(\d)$")


def _imm(s):
    return int(s, 0)


def split_ops(op):
    return [x.strip() for x in op.split(",")] if op.strip() else []


COND_BR = {  # mnemonic -> (crbit, sense)  sense True = branch if bit set
    "beq": ("eq", True), "bne": ("eq", False), "blt": ("lt", True), "bge": ("lt", False),
    "bgt": ("gt", True), "ble": ("gt", False), "bso": ("un", True), "bns": ("un", False),
    "bun": ("un", True), "bnu": ("un", False),
}
FLOAT_MN = {"fadd", "fadds", "fsub", "fsubs", "fmul", "fmuls", "fdiv", "fdivs", "fmadd", "fmadds", "fmsub", "fmsubs",
            "fnmadd", "fnmadds", "fnmsub", "fnmsubs", "fmr", "fneg", "fabs", "fnabs", "frsp", "fctiwz", "fctiw",
            "fsel", "fcmpu", "fcmpo", "lfs", "lfd", "stfs", "stfd", "lfsx", "lfdx", "stfsx", "stfdx", "lfsu", "lfdu",
            "stfsu", "stfdu", "fsqrts", "fsqrt", "fres", "frsqrte", "stfiwx"}


class Instr:
    __slots__ = ("addr", "mn", "ops", "raw", "target", "rc")

    def __init__(self, addr, mn, opstr):
        self.addr = addr
        self.raw = opstr
        self.rc = mn.endswith(".") and mn not in ("bl.",)
        self.mn = mn.rstrip(".").rstrip("+-") if mn[0] == "b" else mn.rstrip(".")
        self.ops = split_ops(opstr)
        self.target = None
        if self.mn[0] == "b" and self.ops and self.ops[-1].startswith("0x"):
            self.target = int(self.ops[-1], 16)


def gpr(s):
    m = _REG.match(s)
    if not m:
        raise Unsupported("operand " + s)
    return int(m.group(1))


def fpr(s):
    m = _FREG.match(s)
    if not m:
        raise Unsupported("foperand " + s)
    return int(m.group(1))


def mem(s):
    m = _MEM.match(s)
    if not m:
        raise Unsupported("mem " + s)
    return _imm(m.group(1)), int(m.group(2)[1:])


# ----------------------------------------------------------------------------------------------------
# lifter
# ----------------------------------------------------------------------------------------------------
class State:
    def __init__(self):
        self.g = {}    # gpr index -> expr
        self.f = {}    # fpr index -> expr
        self.cr = {}   # field -> (kind, a, b)
        self.ca = C(0)
        self.ctr = ("init", "ctr")
        self.lr = ("init", "lr")

    def copy(self):
        s = State()
        s.g, s.f, s.cr, s.ca, s.ctr, s.lr = dict(self.g), dict(self.f), dict(self.cr), self.ca, self.ctr, self.lr
        return s

    def G(self, n):
        if n == 0 and False:
            return C(0)
        return self.g.get(n) or (("init", "r%d" % n))

    def F(self, n):
        return self.f.get(n) or ("init", "f%d" % n)


class Lifted:
    def __init__(self):
        self.stmts = []      # (guard, stmt) ; stmt tuples below
        self.ret_g = None    # expr for r3 at return
        self.ret_f = None    # expr for f1 at return
        self.blocks = []     # (start addr, guard) for coverage
        self.calls = 0
        self.notes = set()

    def all_blocks(self):
        """Start addresses of every block, including those inside loop bodies."""
        out = {s for s, g in self.blocks}

        def walk(stmts):
            for g, s in stmts:
                if s[0] == "loop":
                    out.update(b[0] for b in s[4])
                    walk(s[3])
        walk(self.stmts)
        return out


def ite_chain(edges, getter, none_ok=False):
    """edges: [(cond, state)]; getter(state) -> expr. Merge with ite over mutually exclusive conditions."""
    vals = [(c, getter(s)) for c, s in edges]
    if all(v == vals[0][1] for _, v in vals):
        return vals[0][1]
    out = vals[-1][1]
    for c, v in reversed(vals[:-1]):
        out = mk("ite", c, v, out)
    return out


def p_and(a, b):
    if a == ("c", 1):
        return b
    if b == ("c", 1):
        return a
    if a == ("c", 0) or b == ("c", 0):
        return ("c", 0)
    if a == b:
        return a
    if a == p_not(b):
        return ("c", 0)
    return ("pand", a, b)


def p_or(a, b):
    if a == ("c", 1) or b == ("c", 1):
        return ("c", 1)
    if a == ("c", 0):
        return b
    if b == ("c", 0):
        return a
    if a == b:
        return a
    # (g & c) | (g & !c) == g ; also c | !c == 1
    if a == p_not(b):
        return ("c", 1)
    if a[0] == "pand" and b[0] == "pand" and a[1] == b[1] and (a[2] == p_not(b[2])):
        return a[1]
    if a[0] == "pand" and a[1] == b and False:
        return b
    return ("por", a, b)


def p_not(a):
    if a[0] == "c":
        return ("c", 0 if a[1] else 1)
    if a[0] == "pnot":
        return a[1]
    return ("pnot", a)


def _subst(x, m, memo):
    """Structural substitution of loop-variable leaves ('lv', lid, key) using m[(lid, key)]."""
    if isinstance(x, tuple):
        if len(x) == 3 and x[0] == "lv" and (x[1], x[2]) in m:
            return m[(x[1], x[2])]
        r = memo.get(id(x))
        if r is not None:
            return r[1]
        res = tuple(_subst(y, m, memo) for y in x)
        memo[id(x)] = (x, res)
        return res
    if isinstance(x, list):
        return [_subst(y, m, memo) for y in x]
    return x


def _lx_walk(x, used, seen):
    if isinstance(x, (tuple, list)):
        if id(x) in seen:
            return
        seen[id(x)] = x
        if isinstance(x, tuple) and len(x) == 4 and x[0] == "lx":
            used.add((x[1], x[2], x[3]))
        for y in x:
            _lx_walk(y, used, seen)


def _walk_stmts(stmts, used):
    seen = {}
    for g, s in stmts:
        _lx_walk(g, used, seen)
        if s[0] == "loop":
            _, lid, entries, body, blocks, conts, exits = s
            _lx_walk(entries, used, seen)
            _lx_walk(blocks, used, seen)
            _lx_walk(conts, used, seen)
            for j, c, assigns in exits:
                _lx_walk(c, used, seen)
                for key, x in assigns:
                    if (lid, j, key) in used:
                        _lx_walk(x, used, seen)
            _walk_stmts(body, used)
        else:
            _lx_walk(s, used, seen)


def _prune_stmts(stmts, used):
    out = []
    for g, s in stmts:
        if s[0] == "loop":
            _, lid, entries, body, blocks, conts, exits = s
            exits = tuple((j, c, tuple((k, x) for k, x in assigns if (lid, j, k) in used)) for j, c, assigns in exits)
            s = ("loop", lid, entries, tuple(_prune_stmts(body, used)), blocks, conts, exits)
        out.append((g, s))
    return out


def lift(instrs, image_read=None, func_start=None, func_end=None):
    """instrs: list[Instr] of one function in address order. image_read(addr,n)->bytes|b'' for constant folding of
    loads from read-only data. Returns Lifted.

    Control flow: the CFG is decomposed into strongly connected components in topological order. Acyclic parts are
    predicated (guards + ite merges). A single-entry cycle becomes a ('loop', ...) statement whose body is lifted the same
    way with loop-carried registers as ('lv', loop, key) leaves; its exits export register values as ('lx', loop, exit, key)."""
    if not instrs:
        raise Unsupported("empty")
    by_addr = {i.addr: k for k, i in enumerate(instrs)}
    lo, hi = instrs[0].addr, instrs[-1].addr + 4
    # ---- leaders
    leaders = {lo}
    for k, i in enumerate(instrs):
        mn = i.mn
        if mn in ("bdnzt", "bdzt", "bdnzf", "bdzf", "bctr", "blrl", "bdnzlr", "bdzlr"):
            raise Unsupported("ctr-branch " + mn)
        if mn == "b" or mn in COND_BR or mn in ("bdnz", "bdz") or (mn.endswith("lr") and mn[:-2] in COND_BR) or mn == "blr":
            if i.target is not None and lo <= i.target < hi:
                leaders.add(i.target)
            if k + 1 < len(instrs):
                leaders.add(instrs[k + 1].addr)
    lifted = Lifted()
    tid = [0]
    cid = [0]
    lid_c = [0]

    def newt():
        tid[0] += 1
        return tid[0]

    order = sorted(leaders)
    bend = {a_: (order[k + 1] if k + 1 < len(order) else hi) for k, a_ in enumerate(order)}
    for a_ in order:
        if a_ not in by_addr:
            raise Unsupported("branch into data")

    # ---- static successors
    succs = {}
    for a_ in order:
        last = instrs[by_addr[bend[a_] - 4]]
        mn = last.mn
        fall = bend[a_] if bend[a_] < hi else None
        if mn == "b":
            s_ = [last.target] if last.target is not None and lo <= last.target < hi else []
        elif mn == "blr":
            s_ = []
        elif mn in COND_BR or mn in ("bdnz", "bdz"):
            if last.target is None or not (lo <= last.target < hi):
                raise Unsupported("cond branch out of function")
            s_ = [last.target] + ([fall] if fall is not None else [])
        elif mn.endswith("lr") and mn[:-2] in COND_BR:
            s_ = [fall] if fall is not None else []
        else:
            s_ = [fall] if fall is not None else []
        succs[a_] = s_

    def merge(edges):
        """Merge incoming (cond, state) edges into (guard, state)."""
        if len(edges) == 1:
            return edges[0][0], edges[0][1].copy()
        guard = ("c", 0)
        for c, _ in edges:
            guard = p_or(guard, c)
        st = State()
        keys_g = set().union(*[set(s.g) for _, s in edges])
        keys_f = set().union(*[set(s.f) for _, s in edges])
        for n in keys_g:
            st.g[n] = ite_chain(edges, lambda s, n=n: s.G(n))
        for n in keys_f:
            st.f[n] = ite_chain(edges, lambda s, n=n: s.F(n))
        for fld in set().union(*[set(s.cr) for _, s in edges]):
            kinds = {s.cr.get(fld, (None,))[0] for _, s in edges}
            if len(kinds) != 1 or None in kinds:
                continue  # not defined on every path: a later branch on it would be Unsupported("branch on unset cr")
            a2 = ite_chain(edges, lambda s, fld=fld: s.cr[fld][1])
            b2 = ite_chain(edges, lambda s, fld=fld: s.cr[fld][2])
            st.cr[fld] = (kinds.pop(), a2, b2)
        st.ca = ite_chain(edges, lambda s: s.ca)
        st.ctr = ite_chain(edges, lambda s: s.ctr)
        return guard, st

    def run_block(start, guard, st, emit):
        """Lift one basic block. Returns outgoing edges [(target addr | None for leaving the function, cond, state)]."""
        end = bend[start]
        outs = []

        def out(addr, cond, state):
            outs.append((addr if (addr is not None and addr < hi) else None, cond, state))

        k = by_addr[start]
        while k < len(instrs) and instrs[k].addr < end:
            i = instrs[k]
            k += 1
            mn, o = i.mn, i.ops
            if mn == "bl" or mn == "bctrl":
                if mn == "bl":
                    tsym = C(i.target)
                else:
                    tsym = st.ctr
                cid[0] += 1
                lifted.calls += 1
                c = cid[0]
                emit(("call", c, tsym, tuple(st.G(n) for n in range(3, 11)), tuple(st.F(n) for n in range(1, 9))))
                for n in [0] + list(range(3, 13)):
                    st.g[n] = ("cr", c, "r%d" % n)
                for n in list(range(0, 14)):
                    st.f[n] = ("cr", c, "f%d" % n)
                st.ca = ("cr", c, "ca")
                st.cr = {}
                continue
            if mn == "b":
                if i.target is None:
                    raise Unsupported("indirect b")
                if lo <= i.target < hi:
                    out(i.target, guard, st.copy())
                else:  # tail call
                    cid[0] += 1
                    lifted.calls += 1
                    c = cid[0]
                    emit(("call", c, C(i.target), tuple(st.G(n) for n in range(3, 11)), tuple(st.F(n) for n in range(1, 9))))
                    for n in [0] + list(range(3, 13)):
                        st.g[n] = ("cr", c, "r%d" % n)
                    for n in range(0, 14):
                        st.f[n] = ("cr", c, "f%d" % n)
                    out(None, guard, st)
                return outs
            if mn == "blr":
                out(None, guard, st)
                return outs
            if mn in ("bdnz", "bdz"):
                newc = mk("add", st.ctr, C(M32))
                pred = ("pctr", newc, mn == "bdnz")
                st.ctr = newc
                out(i.target, p_and(guard, pred), st.copy())
                out(end, p_and(guard, p_not(pred)), st)
                return outs
            if mn in COND_BR or (mn.endswith("lr") and mn[:-2] in COND_BR):
                is_ret = mn.endswith("lr") and mn not in COND_BR
                base = mn[:-2] if is_ret else mn
                bit, sense = COND_BR[base]
                fld = 0
                if not is_ret and len(o) == 2:
                    m = _CR.match(o[0])
                    fld = int(m.group(1))
                elif is_ret and o:
                    m = _CR.match(o[0])
                    fld = int(m.group(1)) if m else 0
                cmpv = st.cr.get(fld)
                if cmpv is None:
                    raise Unsupported("branch on unset cr")
                pred = ("pbit", bit, cmpv[0], cmpv[1], cmpv[2])
                taken = pred if sense else p_not(pred)
                if is_ret:
                    out(None, p_and(guard, taken), st.copy())
                else:
                    if not (lo <= i.target < hi):
                        raise Unsupported("cond branch out of function")
                    out(i.target, p_and(guard, taken), st.copy())
                out(end, p_and(guard, p_not(taken)), st)
                return outs
            _step(i, st, emit, newt, image_read)
        out(end, guard, st)  # fell off the end of the block
        return outs

    def tarjan(nodes, g):
        index, low, onst, stack, res, idx = {}, {}, set(), [], [], [0]

        def sc(v):
            index[v] = low[v] = idx[0]
            idx[0] += 1
            stack.append(v)
            onst.add(v)
            for w in g[v]:
                if w not in index:
                    sc(w)
                    low[v] = min(low[v], low[w])
                elif w in onst:
                    low[v] = min(low[v], index[w])
            if low[v] == index[v]:
                comp = []
                while True:
                    w = stack.pop()
                    onst.discard(w)
                    comp.append(w)
                    if w == v:
                        break
                res.append(comp)
        for v in nodes:
            if v not in index:
                sc(v)
        res.reverse()   # topological order of the condensation
        return res

    def lift_region(blockset, incoming, stmts, blocks_out, cut):
        nodes = sorted(blockset)
        entry_nodes = {n for n in nodes if incoming.get(n)}
        gr = {n: [s_ for s_ in succs[n] if s_ in blockset and s_ != cut] for n in nodes}
        leaving = []

        def route(t, cond, state):
            if t is not None and t in blockset and t != cut:
                incoming[t].append((cond, state))
            else:
                leaving.append((t, cond, state))

        for scc in tarjan(nodes, gr):
            if len(scc) == 1 and scc[0] not in gr[scc[0]]:
                n = scc[0]
                if not incoming[n]:
                    continue
                guard, st = merge(incoming[n])
                blocks_out.append((n, guard))
                for t, c, s in run_block(n, guard, st, lambda stmt, guard=guard: stmts.append((guard, stmt))):
                    route(t, c, s)
                continue
            # ---------------- a loop
            sccset = set(scc)
            heads = [n for n in scc if n in entry_nodes or any(n in gr[m] for m in nodes if m not in sccset)]
            if len(heads) != 1:
                raise Unsupported("irreducible loop")
            head = heads[0]
            if not incoming[head]:
                continue
            g_in, E = merge(incoming[head])
            lid_c[0] += 1
            lid = lid_c[0]
            B = State()
            for n in range(32):
                B.g[n] = ("lv", lid, ("g", n))
                B.f[n] = ("lv", lid, ("f", n))
            B.ctr = ("lv", lid, ("ctr",))
            B.ca = ("lv", lid, ("ca",))
            for fld, (kind, _a, _b) in E.cr.items():
                B.cr[fld] = (kind, ("lv", lid, ("cra", fld)), ("lv", lid, ("crb", fld)))
            B.lr = E.lr
            body, bblocks = [], []
            sub_in = {n: [] for n in sccset}
            sub_in[head] = [(("c", 1), B)]
            inner = lift_region(sccset, sub_in, body, bblocks, head)
            conts = [(c, s) for t, c, s in inner if t == head]
            exits_ = [(t, c, s) for t, c, s in inner if t != head]
            if not conts:
                raise Unsupported("loop without back edge")

            def keys_of(s):
                ks = {("g", n): s.G(n) for n in range(32)}
                ks.update({("f", n): s.F(n) for n in range(32)})
                ks[("ctr",)] = s.ctr
                ks[("ca",)] = s.ca
                for fld, (kind, a2, b2) in s.cr.items():
                    ks[("cra", fld)] = a2
                    ks[("crb", fld)] = b2
                return ks
            ek = keys_of(E)
            cont_vals = [keys_of(s) for c, s in conts]

            def refs(x, out, seen):
                if isinstance(x, (tuple, list)):
                    if id(x) in seen:
                        return
                    seen[id(x)] = x
                    if isinstance(x, tuple) and len(x) == 3 and x[0] == "lv" and x[1] == lid:
                        out.add(x[2])
                        return
                    for y in x:
                        refs(y, out, seen)
            # live loop variables: read before being written in some iteration (or exported); iterate to a fixpoint
            live, seen = set(), {}
            refs((body, bblocks, [c for c, s in conts], [(c, s2) for t, c, s2 in exits_]), live, seen)
            for t, c, s in exits_:
                for key, v0 in keys_of(s).items():
                    refs(v0, live, seen)
            carried = []
            while True:
                nc = [k for k in ek if k in live and any(cv.get(k, ("lv", lid, k)) != ("lv", lid, k) for cv in cont_vals)]
                for k in nc:
                    for cv in cont_vals:
                        refs(cv.get(k), live, seen)
                if len(nc) == len(carried):
                    break
                carried = nc
            for fld, (kind, _a, _b) in E.cr.items():
                if ("cra", fld) in live or ("crb", fld) in live:
                    for c, s in conts:
                        if fld not in s.cr or s.cr[fld][0] != kind:
                            raise Unsupported("cr kind changes in loop")
            m = {(lid, key): ek[key] for key in ek if key not in carried}
            memo = {}
            body_s = tuple(_subst(list(body), m, memo))
            body_blocks = tuple(_subst(list(bblocks), m, memo))
            conts_s = tuple((_subst(c, m, memo), tuple((key, _subst(cv.get(key, ("lv", lid, key)), m, memo)) for key in carried))
                            for (c, s), cv in zip(conts, cont_vals))
            entries = tuple((key, ek[key]) for key in carried)
            exits_s = []
            outer = []
            for j, (t, c, s) in enumerate(exits_):
                sk = keys_of(s)
                assigns = []
                st2 = E.copy()
                for key, v0 in sk.items():
                    if key[0] in ("cra", "crb"):
                        continue
                    val = _subst(v0, m, memo)
                    if val == ek[key]:
                        continue
                    assigns.append((key, val))
                    sym = ("lx", lid, j, key)
                    if key[0] == "g":
                        st2.g[key[1]] = sym
                    elif key[0] == "f":
                        st2.f[key[1]] = sym
                    elif key[0] == "ctr":
                        st2.ctr = sym
                    else:
                        st2.ca = sym
                for fld in list(E.cr):
                    if fld not in s.cr:
                        st2.cr.pop(fld, None)   # clobbered on this exit
                for fld, (kind, a2, b2) in s.cr.items():
                    ea, eb = _subst(a2, m, memo), _subst(b2, m, memo)
                    e0 = E.cr.get(fld)
                    if e0 is not None and e0[0] == kind and e0[1] == ea and e0[2] == eb:
                        st2.cr[fld] = e0
                        continue
                    na = e0[1] if (e0 is not None and e0[1] == ea) else ("lx", lid, j, ("cra", fld))
                    nb = e0[2] if (e0 is not None and e0[2] == eb) else ("lx", lid, j, ("crb", fld))
                    if na[0] == "lx":
                        assigns.append((("cra", fld), ea))
                    if nb[0] == "lx":
                        assigns.append((("crb", fld), eb))
                    st2.cr[fld] = (kind, na, nb)
                exits_s.append((j, _subst(c, m, memo), tuple(assigns)))
                outer.append((t, p_and(g_in, ("lxc", lid, j)) if len(exits_) > 1 else g_in, st2))
            stmts.append((g_in, ("loop", lid, entries, body_s, body_blocks, conts_s, tuple(exits_s))))
            blocks_out.append((head, g_in))
            for t, c, s in outer:
                route(t, c, s)
        return leaving

    incoming0 = {a_: [] for a_ in order}
    incoming0[lo] = [(("c", 1), State())]
    leaving = lift_region(set(order), incoming0, lifted.stmts, lifted.blocks, None)
    exits = [(c, s) for t, c, s in leaving]
    if not exits:
        raise Unsupported("no return")
    lifted.ret_g = ite_chain(exits, lambda s: s.G(3))
    lifted.ret_f = ite_chain(exits, lambda s: s.F(1))
    # drop loop exports nothing refers to
    used = set()
    _lx_walk(lifted.ret_g, used, {})
    _lx_walk(lifted.ret_f, used, {})
    n0 = -1
    while len(used) != n0:
        n0 = len(used)
        _walk_stmts(lifted.stmts, used)
    lifted.stmts = _prune_stmts(lifted.stmts, used)
    return lifted


def _add_cin(e, cin):
    if cin == C(0):
        return e
    if cin == C(1):
        return mk("add", e, C(1))
    return mk("add", e, ("cin", cin))


def _ea(st, ra, off=None, rb=None):
    base = C(0) if ra == 0 else st.G(ra)
    if rb is not None:
        return mk("add", base, st.G(rb))
    return mk("add", base, C(off))


LOADS = {"lbz": (1, False), "lbzu": (1, False), "lbzx": (1, False), "lbzux": (1, False),
         "lhz": (2, False), "lhzu": (2, False), "lhzx": (2, False), "lhzux": (2, False),
         "lha": (2, True), "lhau": (2, True), "lhax": (2, True), "lhaux": (2, True),
         "lwz": (4, False), "lwzu": (4, False), "lwzx": (4, False), "lwzux": (4, False)}
STORES = {"stb": 1, "stbu": 1, "stbx": 1, "stbux": 1, "sth": 2, "sthu": 2, "sthx": 2, "sthux": 2,
          "stw": 4, "stwu": 4, "stwx": 4, "stwux": 4}
FLOADS = {"lfs": 4, "lfsu": 4, "lfsx": 4, "lfsux": 4, "lfd": 8, "lfdu": 8, "lfdx": 8, "lfdux": 8}
FSTORES = {"stfs": 4, "stfsu": 4, "stfsx": 4, "stfsux": 4, "stfd": 8, "stfdu": 8, "stfdx": 8, "stfdux": 8}


def _setcr0(st, rc, val):
    if rc:
        st.cr[0] = ("s", val, C(0))


def _step(i, st, emit, newt, image_read):
    mn, o = i.mn, i.ops
    rc = i.rc
    G = st.G

    def setg(n, e):
        st.g[n] = e

    if mn == "nop":
        return
    if mn in ("crclr", "crset"):  # varargs marker (CR6.eq); only observable through a later branch on that field
        m = re.search(r"cr(\d)", o[0])
        if not m:
            raise Unsupported("cr bit op")
        st.cr.pop(int(m.group(1)), None)
        return
    # ---------------- moves / immediates
    if mn == "li":
        setg(gpr(o[0]), C(_imm(o[1]))); return
    if mn == "lis":
        setg(gpr(o[0]), C(_imm(o[1]) << 16)); return
    if mn == "mr":
        setg(gpr(o[0]), G(gpr(o[1]))); _setcr0(st, rc, G(gpr(o[1]))); return
    if mn in ("addi", "subi"):
        ra = gpr(o[1]); d = _imm(o[2]) * (1 if mn == "addi" else -1)
        setg(gpr(o[0]), mk("add", C(0) if ra == 0 and False else G(ra), C(d))); return
    if mn in ("addis", "subis"):
        ra = gpr(o[1]); d = (_imm(o[2]) << 16) * (1 if mn == "addis" else -1)
        setg(gpr(o[0]), mk("add", G(ra), C(d))); return
    if mn in ("add", "subf", "sub", "and", "or", "xor", "nand", "nor", "eqv", "andc", "orc", "mullw", "mulhw", "mulhwu",
              "divw", "divwu", "slw", "srw", "sraw"):
        d, a, b = gpr(o[0]), gpr(o[1]), gpr(o[2])
        op = {"subf": "sub", "mullw": "mul"}.get(mn, mn)
        if mn == "subf":
            e = mk("sub", G(b), G(a))
        elif mn in ("slw", "srw", "sraw"):
            e = mk(op, G(a), G(b))
            if mn == "sraw":
                st.ca = ("sraw_ca", G(a), G(b))
        else:
            e = mk(op, G(a), G(b))
        setg(d, e); _setcr0(st, rc, e); return
    if mn in ("addc", "adde", "subfc", "subfe", "addze", "subfze", "addme", "subfme"):
        d = gpr(o[0]); a = gpr(o[1]); b = gpr(o[2]) if len(o) > 2 else None
        if mn == "addc":
            x, y, cin = G(a), G(b), C(0)
        elif mn == "adde":
            x, y, cin = G(a), G(b), st.ca
        elif mn == "subfc":
            x, y, cin = mk("not", G(a)), G(b), C(1)
        elif mn == "subfe":
            x, y, cin = mk("not", G(a)), G(b), st.ca
        elif mn == "addze":
            x, y, cin = G(a), C(0), st.ca
        elif mn == "subfze":
            x, y, cin = mk("not", G(a)), C(0), st.ca
        elif mn == "addme":
            x, y, cin = G(a), C(M32), st.ca
        else:
            x, y, cin = mk("not", G(a)), C(M32), st.ca
        e = _add_cin(mk("add", x, y), cin)
        st.ca = ("carry", x, y, cin)
        setg(d, e); _setcr0(st, rc, e); return
    if mn in ("addic", "subic", "addic.", "subfic"):
        d, a = gpr(o[0]), gpr(o[1]); imm = _imm(o[2])
        if mn == "subfic":
            x, y = mk("not", G(a)), C(imm); cin = C(1)
        else:
            x, y = G(a), C(imm if mn.startswith("addic") else -imm); cin = C(0)
        e = _add_cin(mk("add", x, y), cin)
        st.ca = ("carry", x, y, cin)
        setg(d, e); _setcr0(st, rc or mn == "addic.", e); return
    if mn in ("neg", "not", "cntlzw", "extsb", "extsh"):
        d, a = gpr(o[0]), gpr(o[1])
        op = {"extsb": "ext8", "extsh": "ext16"}.get(mn, mn)
        e = mk(op, G(a)); setg(d, e); _setcr0(st, rc, e); return
    if mn in ("andi", "andis", "ori", "oris", "xori", "xoris"):
        a, s = gpr(o[0]), gpr(o[1]); imm = _imm(o[2])
        sh = 16 if mn.endswith("is") else 0
        op = mn[:-2] if mn.endswith("is") else mn[:-1] if mn.endswith("i") else mn
        e = mk({"and": "and", "or": "or", "xor": "xor"}[op], G(s), C((imm & 0xFFFF) << sh))
        setg(a, e); _setcr0(st, rc or mn in ("andi", "andis"), e); return
    if mn == "mulli":
        setg(gpr(o[0]), mk("mul", G(gpr(o[1])), C(_imm(o[2])))); return
    if mn == "srawi":
        d, s = gpr(o[0]), gpr(o[1]); n = _imm(o[2])
        st.ca = ("srawi_ca", G(s), n)
        e = mk("sraw", G(s), C(n)); setg(d, e); _setcr0(st, rc, e); return
    # ---------------- rotates
    rot = _rot_decode(mn, o)
    if rot is not None:
        kind, d, s, sh, mb, me = rot
        rs = G(s)
        if kind == "rlwimi":
            e = ("rlwimi", G(d), rs, sh, mb, me)
        else:
            e = ("rlw", rs, sh, mb, me) if not isinstance(sh, tuple) else ("rlwnm", rs, G(sh[1]), mb, me)
        setg(d, e); _setcr0(st, rc, e); return
    # ---------------- compares
    if mn in ("cmpwi", "cmplwi", "cmpw", "cmplw"):
        fld = 0
        ops = list(o)
        if ops and _CR.match(ops[0]):
            fld = int(_CR.match(ops.pop(0)).group(1))
        a = G(gpr(ops[0]))
        b = C(_imm(ops[1])) if mn.endswith("i") else G(gpr(ops[1]))
        if mn == "cmpwi":
            b = C(_imm(ops[1]))
        st.cr[fld] = ("s" if mn in ("cmpwi", "cmpw") else "u", a, b); return
    if mn in ("fcmpu", "fcmpo"):
        fld = 0
        ops = list(o)
        if _CR.match(ops[0]):
            fld = int(_CR.match(ops.pop(0)).group(1))
        st.cr[fld] = ("f", st.F(fpr(ops[0])), st.F(fpr(ops[1]))); return
    # ---------------- spr moves
    if mn == "mflr":
        setg(gpr(o[0]), ("lr",)); return
    if mn == "mtlr":
        st.lr = G(gpr(o[0])); return
    if mn == "mtctr":
        st.ctr = G(gpr(o[0])); return
    if mn == "mfctr":
        setg(gpr(o[0]), st.ctr); return
    # ---------------- loads / stores
    if mn in LOADS:
        w, sg = LOADS[mn]
        d = gpr(o[0])
        if mn.endswith("x") or mn.endswith("ux"):
            ea = _ea(st, gpr(o[1]) if not mn.endswith("ux") else gpr(o[1]), rb=gpr(o[2]))
            if gpr(o[1]) == 0 and not mn.endswith("ux"):
                ea = G(gpr(o[2]))
        else:
            off, ra = mem(o[1]); ea = _ea(st, ra, off)
            if ra == 0:
                ea = C(off)
        _emit_load(st, emit, newt, image_read, d, ea, w, sg, "i")
        if mn.endswith("u") or mn.endswith("ux"):
            ra = mem(o[1])[1] if not mn.endswith("ux") else gpr(o[1])
            if ra == d:
                raise Unsupported("update load rD==rA")
            st.g[ra] = ea
        return
    if mn in STORES:
        w = STORES[mn]
        s = gpr(o[0])
        if mn.endswith("x") or mn.endswith("ux"):
            ea = _ea(st, gpr(o[1]), rb=gpr(o[2]))
            if gpr(o[1]) == 0 and not mn.endswith("ux"):
                ea = G(gpr(o[2]))
        else:
            off, ra = mem(o[1]); ea = _ea(st, ra, off)
            if ra == 0:
                ea = C(off)
        emit(("st", w, ea, G(s), "i"))
        if mn.endswith("u") or mn.endswith("ux"):
            ra = mem(o[1])[1] if not mn.endswith("ux") else gpr(o[1])
            st.g[ra] = ea
        return
    if mn in ("lmw", "stmw"):
        rd = gpr(o[0]); off, ra = mem(o[1])
        for k, n in enumerate(range(rd, 32)):
            ea = mk("add", G(ra) if ra else C(0), C(off + 4 * k))
            if mn == "lmw":
                _emit_load(st, emit, newt, image_read, n, ea, 4, False, "i")
            else:
                emit(("st", 4, ea, G(n), "i"))
        return
    if mn in FLOADS:
        w = FLOADS[mn]; d = fpr(o[0])
        if mn.endswith("x") or mn.endswith("ux"):
            ea = _ea(st, gpr(o[1]), rb=gpr(o[2]))
            if gpr(o[1]) == 0 and not mn.endswith("ux"):
                ea = G(gpr(o[2]))
        else:
            off, ra = mem(o[1]); ea = _ea(st, ra, off)
            if ra == 0:
                ea = C(off)
        t = newt()
        cst = _fold_const_load(image_read, ea, w, "f")
        if cst is not None:
            st.f[d] = cst
        else:
            emit(("ld", t, w, False, ea, "f"))
            st.f[d] = ("t", t)
        if mn.endswith("u") or mn.endswith("ux"):
            st.g[mem(o[1])[1] if not mn.endswith("ux") else gpr(o[1])] = ea
        return
    if mn in FSTORES:
        w = FSTORES[mn]; s = fpr(o[0])
        if mn.endswith("x") or mn.endswith("ux"):
            ea = _ea(st, gpr(o[1]), rb=gpr(o[2]))
            if gpr(o[1]) == 0 and not mn.endswith("ux"):
                ea = G(gpr(o[2]))
        else:
            off, ra = mem(o[1]); ea = _ea(st, ra, off)
            if ra == 0:
                ea = C(off)
        emit(("st", w, ea, st.F(s), "f"))
        if mn.endswith("u") or mn.endswith("ux"):
            st.g[mem(o[1])[1] if not mn.endswith("ux") else gpr(o[1])] = ea
        return
    # ---------------- floating point
    if mn in ("fmr", "fneg", "fabs", "fnabs", "frsp", "fctiwz", "fctiw"):
        d, a = fpr(o[0]), fpr(o[1]); st.f[d] = (mn, st.F(a)); return
    if mn in ("fadd", "fadds", "fsub", "fsubs", "fmul", "fmuls", "fdiv", "fdivs"):
        d, a, b = fpr(o[0]), fpr(o[1]), fpr(o[2]); st.f[d] = (mn, st.F(a), st.F(b)); return
    if mn in ("fmadd", "fmadds", "fmsub", "fmsubs", "fnmadd", "fnmadds", "fnmsub", "fnmsubs", "fsel"):
        d, a, c, b = fpr(o[0]), fpr(o[1]), fpr(o[2]), fpr(o[3])
        st.f[d] = (mn, st.F(a), st.F(c), st.F(b)); return
    raise Unsupported("insn " + mn)


def _fold_const_load(image_read, ea, w, kind):
    if image_read is None or ea[0] != "c":
        return None
    b = image_read(ea[1], w)
    if not b or len(b) != w:
        return None
    if kind == "f":
        v = struct.unpack(">f" if w == 4 else ">d", b)[0]
        return ("fc", v)
    return None


def _emit_load(st, emit, newt, image_read, d, ea, w, sg, kind):
    cst = None
    if image_read is not None and ea[0] == "c":
        b = image_read(ea[1], w)
        if b and len(b) == w:
            v = int.from_bytes(b, "big")
            if sg and v & (1 << (8 * w - 1)):
                v -= 1 << (8 * w)
            cst = C(v)
    if cst is not None:
        st.g[d] = cst
        return
    t = newt()
    emit(("ld", t, w, sg, ea, kind))
    st.g[d] = ("t", t)


def _rot_decode(mn, o):
    """Returns (kind, dest, src, sh, mb, me) or None. sh may be ('reg', rb) for rlwnm."""
    try:
        if mn == "rlwinm":
            return ("rlwinm", gpr(o[0]), gpr(o[1]), _imm(o[2]), _imm(o[3]), _imm(o[4]))
        if mn == "rlwimi":
            return ("rlwimi", gpr(o[0]), gpr(o[1]), _imm(o[2]), _imm(o[3]), _imm(o[4]))
        if mn == "rlwnm":
            return ("rlwnm", gpr(o[0]), gpr(o[1]), ("reg", gpr(o[2])), _imm(o[3]), _imm(o[4]))
        if mn == "slwi":
            n = _imm(o[2]); return ("rlwinm", gpr(o[0]), gpr(o[1]), n, 0, 31 - n)
        if mn == "srwi":
            n = _imm(o[2]); return ("rlwinm", gpr(o[0]), gpr(o[1]), (32 - n) & 31, n, 31)
        if mn == "clrlwi":
            n = _imm(o[2]); return ("rlwinm", gpr(o[0]), gpr(o[1]), 0, n, 31)
        if mn == "clrrwi":
            n = _imm(o[2]); return ("rlwinm", gpr(o[0]), gpr(o[1]), 0, 0, 31 - n)
        if mn == "rotlwi":
            return ("rlwinm", gpr(o[0]), gpr(o[1]), _imm(o[2]) & 31, 0, 31)
        if mn == "rotrwi":
            return ("rlwinm", gpr(o[0]), gpr(o[1]), (32 - _imm(o[2])) & 31, 0, 31)
        if mn == "rotlw":
            return ("rlwnm", gpr(o[0]), gpr(o[1]), ("reg", gpr(o[2])), 0, 31)
        if mn == "extlwi":
            n, b = _imm(o[2]), _imm(o[3]); return ("rlwinm", gpr(o[0]), gpr(o[1]), b, 0, n - 1)
        if mn == "extrwi":
            n, b = _imm(o[2]), _imm(o[3]); return ("rlwinm", gpr(o[0]), gpr(o[1]), (b + n) & 31, 32 - n, 31)
        if mn == "inslwi":
            n, b = _imm(o[2]), _imm(o[3]); return ("rlwimi", gpr(o[0]), gpr(o[1]), (32 - b) & 31, b, b + n - 1)
        if mn == "insrwi":
            n, b = _imm(o[2]), _imm(o[3]); return ("rlwimi", gpr(o[0]), gpr(o[1]), (32 - (b + n)) & 31, b, b + n - 1)
    except Unsupported:
        raise
    return None


# ----------------------------------------------------------------------------------------------------
# evaluation (interpreter) -- used by the verifier
# ----------------------------------------------------------------------------------------------------
def _f32(x):
    try:
        return struct.unpack(">f", struct.pack(">f", x))[0]
    except (OverflowError, struct.error):
        return math.copysign(math.inf, x)


def _fadd(a, b, single):
    r = a + b
    return _f32(r) if single else r


def _fdiv(a, b):
    if b == 0.0:
        if a == 0.0 or a != a:
            return math.nan
        return math.copysign(math.inf, a) * (math.copysign(1.0, b))
    return a / b


def _fctiwz(x):
    # NaN / out-of-range results differ between the architecture (0x7FFFFFFF vs 0x80000000) and emulators: undefined here
    if x != x or x >= 2 ** 31 or x <= -(2 ** 31) - 1:
        raise Undefined("fctiwz range")
    return int(x) & M32


class Env:
    """Concrete environment for evaluating expressions: leaf values + temp/call-result tables."""

    def __init__(self, init, mem_read, oracle):
        self.init = init          # name -> int|float  (r0..r31, f0..f31, ctr, lr, sp)
        self.t = {}
        self.cr = {}
        self.lv = {}
        self.lx = {}
        self.lxk = {}
        self.cov = set()
        self.mem_read = mem_read
        self.oracle = oracle


def ev(e, env):
    op = e[0]
    if op == "c":
        return e[1]
    if op == "fc":
        return e[1]
    if op == "init":
        return env.init[e[1]]
    if op == "t":
        return env.t[e[1]]
    if op == "cr":
        return env.cr[(e[1], e[2])]
    if op == "lr":
        return env.init["lr"]
    if op == "lv":
        return env.lv[(e[1], e[2])]
    if op == "lx":
        return env.lx[(e[1], e[2], e[3])]
    if op == "lxc":
        return int(env.lxk.get(e[1]) == e[2])
    if op == "pctr":
        return int(((ev(e[1], env) & M32) != 0) == bool(e[2]))
    if op == "ite":
        return ev(e[2], env) if ev(e[1], env) else ev(e[3], env)
    if op in ("pand",):
        return int(bool(ev(e[1], env)) and bool(ev(e[2], env)))
    if op == "por":
        return int(bool(ev(e[1], env)) or bool(ev(e[2], env)))
    if op == "pnot":
        return int(not ev(e[1], env))
    if op == "pbit":
        _, bit, kind, a, b = e
        x, y = ev(a, env), ev(b, env)
        if kind == "s":
            x, y = _s32(x), _s32(y)
        elif kind == "u":
            x, y = x & M32, y & M32
        if kind == "f":
            if x != x or y != y:
                lt = gt = eq = 0
                un = 1
            else:
                lt, gt, eq, un = int(x < y), int(x > y), int(x == y), 0
        else:
            lt, gt, eq, un = int(x < y), int(x > y), int(x == y), 0
        return {"lt": lt, "gt": gt, "eq": eq, "un": un}[bit]
    if op in _FOLD:
        a = [ev(x, env) for x in e[1:]]
        return _FOLD[op](*a) & M32
    if op == "rlw":
        return _rotl(ev(e[1], env), e[2]) & _mask(e[3], e[4])
    if op == "rlwnm":
        return _rotl(ev(e[1], env), ev(e[2], env) & 31) & _mask(e[3], e[4])
    if op == "rlwimi":
        m = _mask(e[4], e[5])
        return (_rotl(ev(e[2], env), e[3]) & m) | (ev(e[1], env) & ~m & M32)
    if op == "cin":
        return ev(e[1], env)
    if op == "carry":
        x, y, cin = ev(e[1], env) & M32, ev(e[2], env) & M32, ev(e[3], env)
        return int(x + y + cin > M32)
    if op == "srawi_ca":
        v, n = _s32(ev(e[1], env)), e[2]
        return int(v < 0 and n > 0 and (v & ((1 << n) - 1)) != 0)
    if op == "sraw_ca":
        v, n = _s32(ev(e[1], env)), ev(e[2], env) & 0x3F
        if n >= 32:
            return int(v < 0)
        return int(v < 0 and n > 0 and (v & ((1 << n) - 1)) != 0)
    # ---- floats
    if op in ("fmr",):
        return ev(e[1], env)
    if op == "fneg":
        return -ev(e[1], env)
    if op == "fabs":
        return abs(ev(e[1], env))
    if op == "fnabs":
        return -abs(ev(e[1], env))
    if op == "frsp":
        return _f32(ev(e[1], env))
    if op in ("fctiwz", "fctiw"):
        return ("fpr_int", _fctiwz(ev(e[1], env)))
    if op in ("fadd", "fadds", "fsub", "fsubs", "fmul", "fmuls", "fdiv", "fdivs"):
        a, b = ev(e[1], env), ev(e[2], env)
        single = op.endswith("s")
        base = op[:-1] if single else op
        if base == "fadd":
            r = a + b
        elif base == "fsub":
            r = a - b
        elif base == "fmul":
            r = a * b
        else:
            r = _fdiv(a, b)
        return _f32(r) if single else r
    if op in ("fmadd", "fmadds", "fmsub", "fmsubs", "fnmadd", "fnmadds", "fnmsub", "fnmsubs"):
        a, c, b = ev(e[1], env), ev(e[2], env), ev(e[3], env)
        from fractions import Fraction
        import math as _m
        if all(_m.isfinite(x) for x in (a, c, b)):
            fa, fc, fb = Fraction(a), Fraction(c), Fraction(b)
            base = op.rstrip("s") if op.endswith("s") else op
            r = fa * fc
            r = r + fb if base in ("fmadd", "fnmadd") else r - fb
            if base.startswith("fnm"):
                r = -r
            try:
                r = float(r)
            except OverflowError:
                r = _m.copysign(_m.inf, r)
        else:
            base = op.rstrip("s") if op.endswith("s") else op
            try:
                r = a * c
                r = r + b if base in ("fmadd", "fnmadd") else r - b
                if base.startswith("fnm"):
                    r = -r
            except (OverflowError, ValueError):
                r = _m.nan
        return _f32(r) if op.endswith("s") else r
    if op == "fsel":
        a, c, b = ev(e[1], env), ev(e[2], env), ev(e[3], env)
        return c if a >= 0.0 else b
    raise Unsupported("eval " + op)


LOOP_CAP = 5000


def run(lifted, env, mem_write):
    """Interpret the statement list. mem_write(addr,width,value,kind) records stores.
    Returns (ret_g, ret_f, calls) where calls is the list of (target, args, fargs). env.cov collects loop-internal blocks."""
    calls = []
    _exec(lifted.stmts, env, mem_write, calls)
    return ev(lifted.ret_g, env), ev(lifted.ret_f, env), calls


def _exec(stmts, env, mem_write, calls):
    for guard, s in stmts:
        if not ev(guard, env):
            continue
        k = s[0]
        if k == "ld":
            _, t, w, sg, ea, kind = s
            addr = ev(ea, env) & M32
            raw = env.mem_read(addr, w)
            if kind == "f":
                env.t[t] = struct.unpack(">f" if w == 4 else ">d", raw)[0]
            else:
                v = int.from_bytes(raw, "big")
                if sg and v & (1 << (8 * w - 1)):
                    v -= 1 << (8 * w)
                env.t[t] = v & M32
        elif k == "st":
            _, w, ea, val, kind = s
            addr = ev(ea, env) & M32
            v = ev(val, env)
            if kind == "f":
                if isinstance(v, tuple):
                    raw = struct.pack(">I", v[1]) if w == 4 else struct.pack(">q", _s32(v[1]))  # high word: sign-extended (emulator convention; unobservable)
                else:
                    raw = struct.pack(">f" if w == 4 else ">d", _f32(v) if w == 4 else v)
            else:
                raw = (v & ((1 << (8 * w)) - 1)).to_bytes(w, "big")
            mem_write(addr, raw)
        elif k == "call":
            _, c, tgt, args, fargs = s
            target = ev(tgt, env) & M32
            a = tuple(ev(x, env) & M32 for x in args)
            fa = tuple(ev(x, env) for x in fargs)
            calls.append((target, a, fa))
            res = env.oracle(len(calls) - 1, target, a, fa)
            for n, v in res["g"].items():
                env.cr[(c, "r%d" % n)] = v
            for n, v in res["f"].items():
                env.cr[(c, "f%d" % n)] = v
        elif k == "loop":
            _, lid, entries, body, blocks, conts, exits = s
            vals = [(key, ev(x, env)) for key, x in entries]
            for key, v in vals:
                env.lv[(lid, key)] = v
            it = 0
            while True:
                it += 1
                if it > LOOP_CAP:
                    raise Undefined("loop bound")
                _exec(body, env, mem_write, calls)
                for start, bg in blocks:
                    try:
                        if ev(bg, env):
                            env.cov.add(start)
                    except (KeyError, Undefined):
                        pass
                nxt = None
                for c, assigns in conts:
                    if ev(c, env):
                        nxt = [(key, ev(x, env)) for key, x in assigns]
                        break
                if nxt is None:
                    break
                for key, v in nxt:
                    env.lv[(lid, key)] = v
            for j, c, assigns in exits:
                if ev(c, env):
                    env.lxk[lid] = j
                    for key, x in assigns:
                        env.lx[(lid, j, key)] = ev(x, env)
                    break
            else:
                raise Undefined("no loop exit")


def decode(md, addr, code):
    """capstone decode with a word-wise fallback for `fcmpo` (opcode 63/xo 32), which capstone lacks."""
    ins = [Instr(i.address, i.mnemonic, i.op_str) for i in md.disasm(code, addr)]
    if len(ins) * 4 == len(code):
        return ins
    out = []
    for k in range(0, len(code), 4):
        w = struct.unpack(">I", code[k:k + 4])[0]
        a = addr + k
        got = list(md.disasm(code[k:k + 4], a))
        if got:
            out.append(Instr(a, got[0].mnemonic, got[0].op_str))
        elif (w >> 26) == 63 and ((w >> 1) & 0x3FF) == 32:
            out.append(Instr(a, "fcmpo", "cr%d, f%d, f%d" % ((w >> 23) & 7, (w >> 16) & 31, (w >> 11) & 31)))
        else:
            raise Unsupported("undecodable word %08x" % w)
    return out
