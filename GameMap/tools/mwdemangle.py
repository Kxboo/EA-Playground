"""Best-effort demangler for Metrowerks CodeWarrior (MWCC) C++ symbols.

Only what the EA Playground ELF needs: `method__<class><const?>F<args>`,
`__ct__`/`__dt__`/operator names, nested `Q<n>` qualifiers and a subset of argument
types. `demangle()` never raises; when arguments cannot be parsed the raw argument
string is preserved so nothing is silently invented.
"""
import re

_PRIM = {"v": "void", "i": "int", "l": "long", "s": "short", "c": "char", "b": "bool",
         "f": "float", "d": "double", "w": "wchar_t", "x": "long long", "e": "..."}
_OPS = {"ct": "constructor", "dt": "destructor", "as": "operator=", "eq": "operator==",
        "ne": "operator!=", "pl": "operator+", "mi": "operator-", "ml": "operator*",
        "dv": "operator/", "apl": "operator+=", "ami": "operator-=", "aml": "operator*=",
        "adv": "operator/=", "vc": "operator[]", "rf": "operator->", "nw": "operator new",
        "dl": "operator delete", "nwa": "operator new[]", "dla": "operator delete[]",
        "lt": "operator<", "gt": "operator>", "le": "operator<=", "ge": "operator>=",
        "cl": "operator()", "nt": "operator!", "vc": "operator[]", "md": "operator%",
        "ls": "operator<<", "rs": "operator>>", "aa": "operator&&", "oo": "operator||"}


class _P:
    def __init__(self, s):
        self.s, self.i = s, 0

    def peek(self, n=1):
        return self.s[self.i:self.i + n]

    def eof(self):
        return self.i >= len(self.s)

    def num(self):
        m = re.compile(r"\d+").match(self.s, self.i)
        if not m:
            raise ValueError("num")
        self.i = m.end()
        return int(m.group())

    def name(self):
        n = self.num()
        t = self.s[self.i:self.i + n]
        if len(t) != n:
            raise ValueError("name")
        self.i += n
        return t


def _qual(p):
    """Parse a class name: <len>name or Q<n><len>name..."""
    if p.peek() == "Q":
        p.i += 1
        n = int(p.peek())
        p.i += 1
        return "::".join(_cls(p) for _ in range(n))
    return _cls(p)


def _cls(p):
    name = p.name()
    if p.peek() == "<":  # template args, keep raw up to matching '>'
        depth, j = 0, p.i
        while j < len(p.s):
            depth += (p.s[j] == "<") - (p.s[j] == ">")
            j += 1
            if depth == 0:
                break
        name += p.s[p.i:j]
        p.i = j
    return name


def _type(p, subs):
    q = ""
    while p.peek() in ("P", "R", "C", "U"):
        c = p.peek()
        if c == "U":  # unsigned prefix
            p.i += 1
            t = _type(p, subs)
            return "unsigned " + t + q
        if c == "P":
            p.i += 1
            t = _type(p, subs)
            return t + "*" + q
        if c == "R":
            p.i += 1
            t = _type(p, subs)
            return t + "&" + q
        if c == "C":
            p.i += 1
            t = _type(p, subs)
            return "const " + t + q
    c = p.peek()
    if c == "F":  # function pointer/type
        p.i += 1
        args = []
        while p.peek() != "_" and not p.eof():
            args.append(_type(p, subs))
        p.i += 1
        ret = _type(p, subs)
        return "%s(*)(%s)" % (ret, ", ".join(args))
    if c == "T":  # repeat of earlier arg
        p.i += 1
        n = int(p.peek())
        p.i += 1
        return subs[n] if n < len(subs) else "T%d" % n
    if c == "N":  # N<count><idx>: repeat count
        p.i += 1
        cnt = int(p.peek()); p.i += 1
        idx = int(p.peek()); p.i += 1
        return "/*%dx*/" % cnt + (subs[idx] if idx < len(subs) else "?")
    if c in _PRIM:
        p.i += 1
        return _PRIM[c]
    if c.isdigit() or c == "Q":
        return _qual(p)
    raise ValueError("type " + c)


def demangle(sym):
    """Return dict(cls, method, args, const, raw)."""
    out = {"raw": sym, "cls": "", "method": sym, "args": "", "const": False, "parsed": False}
    m = re.match(r"^(__[a-z]+|[A-Za-z_][A-Za-z0-9_$<>,*:&\\ ]*?)__((?:Q\d)?\d.*)$", sym)
    if not m:
        m2 = re.match(r"^(.*?)__F(.*)$", sym)  # free function with args
        if m2 and m2.group(1):
            out["method"] = m2.group(1)
            out["args"] = _args("F" + m2.group(2), out)
            out["parsed"] = not out["args"].startswith("?")
        return out
    meth, rest = m.group(1), m.group(2)
    if meth.startswith("__"):
        meth = _OPS.get(meth[2:], meth)
    out["method"] = meth
    p = _P(rest)
    try:
        out["cls"] = _qual(p)
    except Exception:
        return out
    if p.peek() == "C":
        out["const"] = True
        p.i += 1
    if p.peek() == "F":
        out["args"] = _args(p.s[p.i:], out)
        out["parsed"] = not out["args"].startswith("?")
    return out


def _args(s, out):
    p = _P(s)
    p.i = 1  # skip F
    subs, res = [], []
    try:
        while not p.eof():
            t = _type(p, subs)
            subs.append(t)
            res.append(t)
    except Exception:
        return "?" + s  # keep raw
    if res == ["void"]:
        return ""
    return ", ".join(res)


def pretty(sym):
    d = demangle(sym)
    base = (d["cls"] + "::" if d["cls"] else "") + d["method"]
    return "%s(%s)%s" % (base, d["args"], " const" if d["const"] else "")


if __name__ == "__main__":
    import sys
    for s in sys.argv[1:]:
        print(pretty(s))
