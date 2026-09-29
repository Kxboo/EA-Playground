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


def _leaves_all(x, out):
    """Like _leaves but walks every element (used for loop statements, whose parts are (guard, stmt) pairs)."""
    if isinstance(x, (tuple, list)):
        if len(x) == 2 and x[0] == "t" and isinstance(x[1], int):
            out.add(x[1])
            return
        for y in x:
            _leaves_all(y, out)


def _mentions_sp_all(x):
    if isinstance(x, (tuple, list)):
        if len(x) == 2 and x[0] == "init" and x[1] == "r1":
            return True
        return any(_mentions_sp_all(y) for y in x)
    return False


def _flat(stmts):
    for g, s in stmts:
        yield g, s
        if s[0] == "loop":
            yield from _flat(s[3])


def _drop_dead_loads(stmts, used):
    """Recursively remove loads whose value is unused. Returns (new list, changed)."""
    out, changed = [], False
    for g, s in stmts:
        if s[0] == "ld" and s[1] not in used:
            changed = True
            continue
        if s[0] == "loop":
            body, ch = _drop_dead_loads(s[3], used)
            if ch:
                changed = True
                s = s[:3] + (tuple(body),) + s[4:]
        out.append((g, s))
    return out, changed


def _uses(e):
    s = set()
    _leaves(e, s)
    return s


def _is_frame(e):
    """The dynamically aligned frame base: init-r1 + non-constant (from `stwux r1, r1, rX` prologues)."""
    return e[0] == "add" and e[1] == ("init", "r1") and e[2][0] != "c" and _mentions_only_sp(e[2])


def _mentions_only_sp(e):
    if not isinstance(e, tuple):
        return True
    if e[0] == "init":
        return e[1] == "r1"
    if e[0] == "c":
        return True
    return all(_mentions_only_sp(x) for x in e[1:])


def _stack_off(e):
    """Offset from the entry SP (or, for aligned frames, from the frame base, tagged +1<<40); None if not stack-based."""
    if e[0] == "init" and e[1] == "r1":
        return 0
    if _is_frame(e):
        return 1 << 40
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
    sp_into_loop = False

    def mentions_sp(e):
        if not isinstance(e, tuple):
            return False
        if e[0] == "init" and e[1] == "r1":
            return True
        return any(mentions_sp(x) for x in e[1:])
    for g, s in _flat(st):
        if s[0] == "call":
            if any(mentions_sp(a) for a in s[3]) or mentions_sp(s[2]):
                escaped_any = True
        elif s[0] == "st" and _stack_off(s[2]) is None and mentions_sp(s[3]):
            escaped_any = True
        elif s[0] == "loop" and _mentions_sp_all(s[2]):
            sp_into_loop = True   # a stack address becomes a loop variable: loop loads may read any slot
    if mentions_sp(lifted.ret_g):
        escaped_any = True
    changed = True
    while changed:
        changed = False
        used = set()
        for g, s in _flat(st):
            _leaves_all(g, used)
            if s[0] == "ld":
                _leaves_all(s[4], used)
            elif s[0] == "st":
                _leaves_all(s[2], used)
                _leaves_all(s[3], used)
            elif s[0] == "call":
                _leaves_all(s[2], used)
                for a in s[3] + s[4]:
                    _leaves_all(a, used)
            elif s[0] == "loop":
                _leaves_all((s[2], s[4], s[5], s[6]), used)
        _leaves(lifted.ret_g, used)
        _leaves(lifted.ret_f, used)
        # a stack store is live if some remaining load could read it (same slot) or the stack escapes
        live_slots = {None} if sp_into_loop else set()
        for g, s in _flat(st):
            if s[0] == "ld" and s[1] in used:
                off = _stack_off(s[4])
                if off is None:
                    # a load through an unknown pointer can only hit this frame if the frame's address escaped or the
                    # address itself is derived from sp; pointers passed in by callers point *above* the entry SP
                    if escaped_any or mentions_sp(s[4]):
                        live_slots.add(None)
                else:
                    for k in range(s[2]):
                        live_slots.add(off + k)
        new = []
        for g, s in st:
            if s[0] == "st":
                off = _stack_off(s[2])
                save_like = s[3] in (("lr",),) or (s[3][0] == "init" and (s[3][1] == "r1" or (s[3][1][0] == "r" and int(s[3][1][1:]) >= 14) or s[3][1][0] == "f"))
                if off is not None and None not in live_slots and (not escaped_any or save_like):
                    if not any((off + k) in live_slots for k in range(s[1])):
                        changed = True
                        continue
            new.append((g, s))
        new, ch = _drop_dead_loads(new, used)
        changed = changed or ch
        st = new
    out = L.Lifted()
    out.stmts = st
    out.ret_g, out.ret_f = lifted.ret_g, lifted.ret_f
    out.blocks = lifted.blocks
    out.calls = lifted.calls
    out.notes = set(lifted.notes)
    return out


# ---------------------------------------------------------------------------------------------- printing
def _split_args(a):
    out, depth, cur = [], 0, ""
    for ch in a:
        if ch in "(<":
            depth += 1
        elif ch in ")>":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def _arity(d):
    """(#gpr args, #fpr args) from a demangled signature (implicit `this` counts as a gpr)."""
    g = 1 if d["cls"] and d["method"] not in ("__sinit",) else 0
    f = 0
    for t in _split_args(d["args"]):
        if t in ("float", "double"):
            f += 1
        elif t in ("long long", "unsigned long long"):
            g += 2
        elif t == "...":
            g += 0
        else:
            g += 1
    return min(g, 8), min(f, 8)


def _keyname(k):
    if k[0] in ("g", "f"):
        return "%s%d" % (k[0] if k[0] == "f" else "r", k[1])
    if k[0] in ("cra", "crb"):
        return "cr%d_%s" % (k[1], k[0][2])
    return k[0]


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
        if off >= (1 << 39):
            off -= 1 << 40
            return "frame" if off == 0 else ("frame - 0x%x" % -off if off < 0 else "frame + 0x%x" % off)
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
            if e[2] == "r3":
                return "ret%d" % e[1]
            if e[2] == "f1":
                return "fret%d" % e[1]
            return "clobbered_%s_%d" % (e[2], e[1])
        if op == "lr":
            return "LR"
        if op == "lv":
            return "L%d_%s" % (e[1], _keyname(e[2]))
        if op == "lx":
            return "L%d_x%d_%s" % (e[1], e[2], _keyname(e[3]))
        if op == "lxc":
            return "(L%d_exit == %d)" % (e[1], e[2])
        if op == "pctr":
            return "(%s %s 0)" % (self.ex(e[1]), "!=" if e[2] else "==")
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
        if op == "rlwimi" and e[3] == 0:
            m = L._mask(e[4], e[5])
            return "((%s & %s) | (%s & %s))" % (self.ex(e[1]), hex(~m & M32), self.ex(e[2]), hex(m))
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
                A = "(s32)%s" % A if a[0] != "c" else A
                B = "(s32)%s" % B if b[0] != "c" else B
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
            garity = farity = None
            if tgt[0] == "c":
                sy = self.E.sym_at(tgt[1])
                if sy and "+" not in sy:
                    name = mwdemangle.pretty(sy).split("(")[0]
                    d = mwdemangle.demangle(sy)
                    if d.get("parsed"):
                        garity, farity = _arity(d)
            callee = name if name else ("(*%s)" % self.ex(tgt) if tgt[0] != "c" else hex(tgt[1]))
            if garity is None:  # unknown callee: strip trailing junk registers
                n = 8
                while n > 0 and (args[n - 1][0] == "cr" or (args[n - 1][0] == "init" and args[n - 1][1] not in ("r%d" % r for r in range(3, 11)))):
                    n -= 1
                garity = n
                nf = 8
                while nf > 0 and (fargs[nf - 1][0] == "cr" or fargs[nf - 1][0] == "init"):
                    nf -= 1
                farity = nf
            al = [self.ex(a) for a in args[:garity]] + [self.ex(x) for x in fargs[:farity]]
            return "ret%d = %s(%s);" % (c, callee, ", ".join(al))
        return "?"

    def emit(self, stmts, ind, out):
        i = 0
        pad = "    " * ind
        while i < len(stmts):
            g = stmts[i][0]
            j = i
            block = []
            while j < len(stmts) and stmts[j][0] == g:
                block.append(stmts[j][1])
                j += 1
            if g == ("c", 1):
                for s in block:
                    self.emit_one(s, ind, out)
            else:
                out.append("%sif (%s) {" % (pad, self.ex(g)))
                for s in block:
                    self.emit_one(s, ind + 1, out)
                out.append(pad + "}")
            i = j

    def emit_one(self, s, ind, out):
        if s[0] == "loop":
            self.emit_loop(s, ind, out)
        else:
            out.append("    " * ind + self.stmt(s))

    def assign_par(self, pairs, ind, out):
        """pairs: [(dest name, rhs expr string)] executed as a parallel assignment."""
        pad = "    " * ind
        pairs = [(d, r) for d, r in pairs if d != r]
        clash = any(d2 in r and d2 != d for d, r in pairs for d2, _ in pairs)
        if clash:
            for k, (d, r) in enumerate(pairs):
                out.append("%s%s_n = %s;" % (pad, d, r))
            for d, r in pairs:
                out.append("%s%s = %s_n;" % (pad, d, d))
        else:
            for d, r in pairs:
                out.append("%s%s = %s;" % (pad, d, r))

    def emit_loop(self, s, ind, out):
        _, lid, entries, body, blocks, conts, exits = s
        pad = "    " * ind
        out.append("%s// loop L%d" % (pad, lid))
        for key, x in entries:
            out.append("%sL%d_%s = %s;" % (pad, lid, _keyname(key), self.ex(x)))
        out.append("%sfor (;;) {   // L%d" % (pad, lid))
        self.emit(body, ind + 1, out)
        for c, assigns in conts:
            out.append("%s    if (%s) {" % (pad, self.ex(c)))
            self.assign_par([("L%d_%s" % (lid, _keyname(k)), self.ex(x)) for k, x in assigns if x != ("lv", lid, k)], ind + 2, out)
            out.append(pad + "        continue;")
            out.append(pad + "    }")
        for idx, (j, c, assigns) in enumerate(exits):
            last = idx == len(exits) - 1
            lines = ["L%d_x%d_%s = %s;" % (lid, j, _keyname(k), self.ex(x)) for k, x in assigns]
            if len(exits) > 1:
                lines.append("L%d_exit = %d;" % (lid, j))
            if last:
                out.extend("%s    %s" % (pad, ln) for ln in lines)
                out.append(pad + "    break;")
            else:
                out.append("%s    if (%s) {" % (pad, self.ex(c)))
                out.extend("%s        %s" % (pad, ln) for ln in lines)
                out.append(pad + "        break;")
                out.append(pad + "    }")
        out.append(pad + "}")

    def render(self, lifted, header=None):
        out = []
        f = self.fn
        sig = mwdemangle.pretty(f["name"])
        out.append("// %s   @0x%08x  size %d  unit %s" % (sig, f["addr"], f["size"], f.get("file", "")))
        out.append("// generated from the machine code by GameMap/tools/lift.py; equivalence-tested (see verify_lift.py)")
        out.append("%s {" % sig)
        self.emit(lifted.stmts, 1, out)
        rg, rf = lifted.ret_g, lifted.ret_f
        tail_call = rf[0] == "cr" and rf[2] == "f1" and rg[0] == "cr" and rg[2] == "r3" and rf[1] == rg[1]
        if rf != ("init", "f1") and not tail_call and not (rf[0] == "cr" and rf[2] == "f1"):
            out.append("    return_f %s;   // f1 at return" % self.ex(rf))
        elif rf[0] == "cr" and rf[2] == "f1" and not tail_call:
            out.append("    return_f %s;   // f1 at return" % self.ex(rf))
        out.append("    return %s;   // r3 at return (unused if the function is void)" % self.ex(rg))
        out.append("}")
        return "\n".join(out)


def one_line(E, fn, lifted):
    """A short description derived mechanically from the cleaned IR (used as the auto summary for proven functions)."""
    P = Printer(E, fn)
    st = list(_flat(lifted.stmts))
    loops = [s for g, s in st if s[0] == "loop"]
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
    if loops:
        bits.append("%d loop%s" % (len(loops), "s" if len(loops) != 1 else ""))
    if cond:
        bits.append("conditional")
    return "; ".join(bits) if bits else "computes " + P.ex(lifted.ret_g)
