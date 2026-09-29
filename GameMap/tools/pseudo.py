"""IR cleaner and pseudo-C printer for lift.py output.

`clean()` removes only things that cannot change the observable behaviour compared by verify_lift.py (non-stack stores,
call trace, return values): dead loads, and stack stores that are never read back and never escape (prologue/epilogue saves,
LR/callee-saved spills). Verification is run on the *cleaned* IR, so printed code == proven code.
"""
import re

import lift as L
import mwdemangle

M32 = 0xFFFFFFFF


# ---------------------------------------------------------------------------------------------- cleaning
def _leaves(e, out):
    if not isinstance(e, tuple):
        return
    if e and e[0] == "t":
        out.add(e[1])
        return
    for x in e[1:]:
        _leaves(x, out)


def _uses(e):
    s = set()
    _leaves(e, s)
    return s


def _stack_off(e):
    """If e is init-r1 + const, return the signed offset from entry SP, else None."""
    if e[0] == "init" and e[1] == "r1":
        return 0
    if e[0] == "add" and e[2][0] == "c":
        b = _stack_off(e[1])
        if b is not None:
            v = e[2][1]
            return b + (v - (1 << 32) if v & 0x80000000 else v)
    return None


def clean(lifted):
    """Return a new Lifted with dead loads and dead/non-escaping stack stores removed."""
    st = list(lifted.stmts)
    # escape analysis: stack addresses appearing (anywhere) in call args or non-stack store values or return
    escaped_any = False

    def mentions_sp(e):
        if not isinstance(e, tuple):
            return False
        if e[0] == "init" and e[1] == "r1":
            return True
        return any(mentions_sp(x) for x in e[1:])
    for g, s in st:
        if s[0] == "call":
            if any(mentions_sp(a) for a in s[3]) or mentions_sp(s[2]):
                escaped_any = True
        elif s[0] == "st" and _stack_off(s[2]) is None and mentions_sp(s[3]):
            escaped_any = True
    if mentions_sp(lifted.ret_g):
        escaped_any = True
    changed = True
    while changed:
        changed = False
        used = set()
        for g, s in st:
            _leaves(g, used)
            if s[0] == "ld":
                _leaves(s[4], used)
            elif s[0] == "st":
                _leaves(s[2], used)
                _leaves(s[3], used)
            elif s[0] == "call":
                _leaves(s[2], used)
                for a in s[3] + s[4]:
                    _leaves(a, used)
        _leaves(lifted.ret_g, used)
        _leaves(lifted.ret_f, used)
        # a stack store is live if some remaining load could read it (same slot) or the stack escapes
        live_slots = set()
        for g, s in st:
            if s[0] == "ld" and s[1] in used:
                off = _stack_off(s[4])
                if off is None:
                    live_slots.add(None)  # unknown address load: may alias
                else:
                    for k in range(s[2]):
                        live_slots.add(off + k)
        new = []
        for g, s in st:
            if s[0] == "ld" and s[1] not in used:
                changed = True
                continue
            if s[0] == "st" and not escaped_any:
                off = _stack_off(s[2])
                if off is not None and None not in live_slots:
                    if not any((off + k) in live_slots for k in range(s[1])):
                        changed = True
                        continue
            new.append((g, s))
        st = new
    out = L.Lifted()
    out.stmts = st
    out.ret_g, out.ret_f = lifted.ret_g, lifted.ret_f
    out.blocks = lifted.blocks
    out.calls = lifted.calls
    out.notes = set(lifted.notes)
    return out


# ---------------------------------------------------------------------------------------------- printing
class Printer:
    def __init__(self, E, fn, image_str=None):
        self.E = E
        self.fn = fn
        self.has_this = bool(fn.get("cls")) and fn.get("method") not in ("__sinit",)
        self.static_like = False

    # names -------------------------------------------------------------
    def arg(self, n):
        if n == 3 and self.has_this:
            return "this"
        return "a%d" % (n - 3)

    def sym(self, addr):
        s = self.E.sym_at(addr)
        if s and "+" not in s:
            return mwdemangle.pretty(s)
        return None

    def const(self, v):
        v &= M32
        if v >= 0xFFFF0000:
            return str(v - (1 << 32))
        if v < 0x10000:
            return str(v) if v < 10 else hex(v)
        if 0x80004000 <= v < 0x80609000:
            s = self.E.cstr(v) if self.E.section_of(v) in (".rodata", ".sdata", ".data", ".sdata2") else None
            if s and len(s) >= 2 and self.E.rd(v - 1, 1) in (b"\0", b""):
                return '"%s"' % s.replace("\n", "\\n")[:60]
            n = self.E.sym_at(v)
            if n:
                return "&" + mwdemangle.pretty(n) if not n.startswith("@") else hex(v)
        return hex(v)

    def sp(self, off):
        return "sp" if off == 0 else ("sp - 0x%x" % -off if off < 0 else "sp + 0x%x" % off)

    # expressions ------------------------------------------------------
    def ex(self, e, top=False):
        op = e[0]
        if op == "c":
            return self.const(e[1])
        if op == "fc":
            v = e[1]
            return repr(float(v)) + "f" if v == v and abs(v) != float("inf") else ("NAN" if v != v else "INFINITY")
        if op == "init":
            r = e[1]
            if r[0] == "r":
                n = int(r[1:])
                if 3 <= n <= 10:
                    return self.arg(n)
                if n == 1:
                    return "sp"
                return "in_r%d" % n
            if r[0] == "f":
                n = int(r[1:])
                return "fa%d" % (n - 1) if 1 <= n <= 8 else "in_f%d" % n
            return "in_" + r
        if op == "t":
            return "v%d" % e[1]
        if op == "cr":
            return "ret%d" % e[1] if e[2] in ("r3", "f1") else "ret%d_%s" % (e[1], e[2])
        if op == "lr":
            return "LR"
        if op == "add":
            so = _stack_off(e)
            if so is not None:
                return "(" + self.sp(so) + ")"
            if e[2][0] == "c" and (e[2][1] & 0x80000000):
                return "(%s - %s)" % (self.ex(e[1]), hex((-e[2][1]) & M32))
            return "(%s + %s)" % (self.ex(e[1]), self.ex(e[2]))
        bin_ = {"sub": "-", "mul": "*", "and": "&", "or": "|", "xor": "^"}
        if op in bin_:
            return "(%s %s %s)" % (self.ex(e[1]), bin_[op], self.ex(e[2]))
        if op == "neg":
            return "(-%s)" % self.ex(e[1])
        if op == "not":
            return "(~%s)" % self.ex(e[1])
        if op == "nand":
            return "~(%s & %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "nor":
            return "~(%s | %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "andc":
            return "(%s & ~%s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "orc":
            return "(%s | ~%s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "eqv":
            return "~(%s ^ %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "divw":
            return "((s32)%s / (s32)%s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "divwu":
            return "(%s / %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "mulhw":
            return "mulhw(%s, %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "mulhwu":
            return "mulhwu(%s, %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "slw":
            return "(%s << %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "srw":
            return "(%s >> %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "sraw":
            return "((s32)%s >> %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "cntlzw":
            return "cntlzw(%s)" % self.ex(e[1])
        if op == "ext8":
            return "(s8)%s" % self.ex(e[1])
        if op == "ext16":
            return "(s16)%s" % self.ex(e[1])
        if op == "rlw":
            return self._rlw(e[1], e[2], e[3], e[4])
        if op == "rlwnm":
            return "(rotl(%s, %s & 31) & %s)" % (self.ex(e[1]), self.ex(e[2]), hex(L._mask(e[3], e[4])))
        if op == "rlwimi":
            m = L._mask(e[4], e[5])
            return "((%s & %s) | (rotl(%s, %d) & %s))" % (self.ex(e[1]), hex(~m & M32), self.ex(e[2]), e[3], hex(m))
        if op == "cin":
            return self.ex(e[1])
        if op == "carry":
            return "carry(%s, %s, %s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        if op in ("srawi_ca", "sraw_ca"):
            return "sra_carry(%s, %s)" % (self.ex(e[1]), e[2] if isinstance(e[2], int) else self.ex(e[2]))
        if op == "ite":
            return "(%s ? %s : %s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        if op == "pand":
            return "(%s && %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "por":
            return "(%s || %s)" % (self.ex(e[1]), self.ex(e[2]))
        if op == "pnot":
            return "!%s" % self.ex(e[1])
        if op == "pbit":
            _, bit, kind, a, b = e
            A, B = self.ex(a), self.ex(b)
            if kind == "s":
                A, B = "(s32)%s" % A, "(s32)%s" % B
            if bit == "eq":
                return "(%s == %s)" % (A, B)
            if bit == "lt":
                return "(%s < %s)" % (A, B)
            if bit == "gt":
                return "(%s > %s)" % (A, B)
            return "isunordered(%s, %s)" % (A, B)
        f2 = {"fadd": "+", "fadds": "+", "fsub": "-", "fsubs": "-", "fmul": "*", "fmuls": "*", "fdiv": "/", "fdivs": "/"}
        if op in f2:
            return "(%s %s %s)" % (self.ex(e[1]), f2[op], self.ex(e[2]))
        if op in ("fmadd", "fmadds"):
            return "fma(%s, %s, %s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        if op in ("fmsub", "fmsubs"):
            return "fma(%s, %s, -%s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        if op in ("fnmadd", "fnmadds"):
            return "-fma(%s, %s, %s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        if op in ("fnmsub", "fnmsubs"):
            return "-fma(%s, %s, -%s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        if op == "fmr":
            return self.ex(e[1])
        if op == "fneg":
            return "(-%s)" % self.ex(e[1])
        if op == "fabs":
            return "fabs(%s)" % self.ex(e[1])
        if op == "fnabs":
            return "(-fabs(%s))" % self.ex(e[1])
        if op == "frsp":
            return "(f32)%s" % self.ex(e[1])
        if op in ("fctiwz", "fctiw"):
            return "(s32)%s" % self.ex(e[1])
        if op == "fsel":
            return "(%s >= 0.0 ? %s : %s)" % (self.ex(e[1]), self.ex(e[2]), self.ex(e[3]))
        return "?%s?" % op

    def _rlw(self, x, sh, mb, me):
        m = L._mask(mb, me)
        xs = self.ex(x)
        if sh == 0:
            return xs if m == M32 else "(%s & %s)" % (xs, hex(m))
        # left shift by n with mask covering bits n..31: (x << n)
        if m == (M32 << sh) & M32:
            return "(%s << %d)" % (xs, sh)
        # logical right shift by n: rotl 32-n, mask low bits
        n = 32 - sh
        if m == (M32 >> n):
            return "(%s >> %d)" % (xs, n)
        return "(rotl(%s, %d) & %s)" % (xs, sh, hex(m))

    # statements -------------------------------------------------------
    TYPES = {(1, False): "u8", (1, True): "s8", (2, False): "u16", (2, True): "s16", (4, False): "u32", (4, True): "s32"}

    def stmt(self, s):
        k = s[0]
        if k == "ld":
            _, t, w, sg, ea, kind = s
            ty = "f32" if (kind == "f" and w == 4) else "f64" if kind == "f" else self.TYPES[(w, sg)]
            return "v%d = *(%s*)%s;" % (t, ty, self.ex(ea))
        if k == "st":
            _, w, ea, val, kind = s
            ty = "f32" if (kind == "f" and w == 4) else "f64" if kind == "f" else {1: "u8", 2: "u16", 4: "u32"}[w]
            return "*(%s*)%s = %s;" % (ty, self.ex(ea), self.ex(val))
        if k == "call":
            _, c, tgt, args, fargs = s
            name = None
            arity = None
            if tgt[0] == "c":
                sy = self.E.sym_at(tgt[1])
                if sy and "+" not in sy:
                    name = mwdemangle.pretty(sy)
                    d = mwdemangle.demangle(sy)
                    if not d["args"].startswith("?"):
                        na = 0 if d["args"] == "" else d["args"].count(",") + 1
                        arity = na + (1 if d["cls"] else 0)
            callee = name if name else ("(*%s)" % self.ex(tgt) if tgt[0] != "c" else hex(tgt[1]))
            if arity is None:  # strip trailing junk args (call clobbers / untouched non-arg registers)
                n = 8
                while n > 0 and (args[n - 1][0] == "cr" or (args[n - 1][0] == "init" and args[n - 1][1] not in ("r%d" % r for r in range(3, 11)))):
                    n -= 1
                arity = n
            al = [self.ex(a) for a in args[:arity]]
            fa = [self.ex(x) for x in fargs if x[0] != "init" or x[1] in ("f%d" % i for i in range(1, 9))]
            fa = [self.ex(x) for x in fargs if not (x[0] == "cr" or (x[0] == "init" and x[1] not in ("f%d" % i for i in range(1, 9))))]
            arglist = ", ".join(al + fa[:0])
            return "ret%d = %s(%s);" % (c, callee, arglist)
        return "?"

    def render(self, lifted, header=None):
        out = []
        f = self.fn
        sig = mwdemangle.pretty(f["name"])
        out.append("// %s   @0x%08x  size %d  unit %s" % (sig, f["addr"], f["size"], f.get("file", "")))
        out.append("// generated from the machine code by GameMap/tools/lift.py; equivalence-tested (see verify_lift.py)")
        out.append("%s {" % sig)
        i = 0
        stmts = lifted.stmts
        while i < len(stmts):
            g = stmts[i][0]
            j = i
            block = []
            while j < len(stmts) and stmts[j][0] == g:
                block.append(self.stmt(stmts[j][1]))
                j += 1
            if g == ("c", 1):
                for b in block:
                    out.append("    " + b)
            else:
                out.append("    if (%s) {" % self.ex(g))
                for b in block:
                    out.append("        " + b)
                out.append("    }")
            i = j
        rg, rf = lifted.ret_g, lifted.ret_f
        if rf != ("init", "f1"):
            out.append("    return_f %s;" % self.ex(rf))
        out.append("    return %s;" % self.ex(rg))
        out.append("}")
        return "\n".join(out)


def one_line(E, fn, lifted):
    """A short description derived mechanically from the cleaned IR (used as the auto summary for proven functions)."""
    P = Printer(E, fn)
    st = lifted.stmts
    loads = [s for g, s in st if s[0] == "ld"]
    stores = [s for g, s in st if s[0] == "st"]
    calls = [s for g, s in st if s[0] == "call"]
    cond = any(g != ("c", 1) for g, s in st) or lifted.ret_g[0] == "ite"
    if not st:
        if lifted.ret_g == ("init", "r3"):
            return "returns its first argument unchanged" if not fn.get("cls") else "returns this (no work)"
        return "returns " + P.ex(lifted.ret_g)
    if len(loads) == 1 and not stores and not calls and lifted.ret_g == ("t", loads[0][1]):
        return "getter: returns %s" % P.ex(loads[0][4]).join(["*(", ")"])
    if len(stores) == 1 and not loads and not calls and not cond:
        return "setter: " + P.stmt(stores[0])
    bits = []
    if stores:
        bits.append("%d store%s" % (len(stores), "s" if len(stores) != 1 else ""))
    if loads:
        bits.append("%d load%s" % (len(loads), "s" if len(loads) != 1 else ""))
    if calls:
        names = []
        for s in calls:
            if s[2][0] == "c":
                sy = E.sym_at(s[2][1])
                names.append(mwdemangle.pretty(sy).split("(")[0] if sy and "+" not in sy else hex(s[2][1]))
            else:
                names.append("indirect")
        uniq = list(dict.fromkeys(names))
        bits.append("calls " + ", ".join(uniq[:4]) + ("…" if len(uniq) > 4 else ""))
    if cond:
        bits.append("conditional")
    return "; ".join(bits) if bits else "computes " + P.ex(lifted.ret_g)
