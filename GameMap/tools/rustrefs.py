"""Find the Rust in the remake (`_bevy/src`) that ports, hooks or drives each original function.

A reference is recorded when the Rust
  - binds a VM hook to the function's symbol:   bind(vm, &["Sym__5ClassFv", ...], handler)  /  .hook("Sym") / .observe("Sym")
  - cites an address inside the function:       0x8012_3456 / 0x80123456       (port, or "vm" for code inside mgvm/gekko)
  - names it in a comment:                      `Class::Method`
  - calls the original by symbol from the VM:   call_by_name(.., "Sym__5ClassFv", ..)  ("drive")
Each reference carries the enclosing Rust item as a snippet (our own code, safe to publish).
"""
import bisect
import glob
import os
import re

FN_DEF = re.compile(r'^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)')
ITEM_DEF = re.compile(r'^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:fn|struct|enum|const|static|impl|mod|trait)\b')
ADDR = re.compile(r'0x(80[0-9a-fA-F]{6}|80[0-9a-fA-F]{2}_[0-9a-fA-F]{4})\b')
TICK = re.compile(r'`([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_~][A-Za-z0-9_]*)+)(?:\([^`]*\))?`')
SYM = re.compile(r'"([A-Za-z_][A-Za-z0-9_<>,]*__[A-Za-z0-9_<>,]+)"')
MAX_LINES = 140


def _block(lines, i):
    """Lines from i to the end of the item starting there (brace matched, capped)."""
    depth, seen, out = 0, False, []
    for j in range(i, min(len(lines), i + MAX_LINES)):
        out.append(lines[j])
        code = re.sub(r'"(?:[^"\\]|\\.)*"', '""', re.sub(r'//.*', '', lines[j]))
        depth += code.count('{') - code.count('}')
        seen = seen or '{' in code
        if (seen and depth <= 0) or (not seen and code.rstrip().endswith(';')):
            break
    return out


def _snippet(lines, i):
    """(enclosing fn name, first line index, code lines) for a reference on line i."""
    if lines[i].strip().startswith('//'):
        # a comment describes the item below it
        j = i
        while j < len(lines) and lines[j].strip().startswith(('//', '#[')):
            j += 1
        if j < len(lines) and ITEM_DEF.match(lines[j]):
            start = i
            while start > 0 and lines[start - 1].strip().startswith('//'):
                start -= 1
            m = FN_DEF.match(lines[j])
            return (m.group(1) if m else lines[j].strip()[:60]), start, lines[start:j] + _block(lines, j)
    j = i
    while j >= 0 and not FN_DEF.match(lines[j]):
        j -= 1
    if j >= 0:
        start = j
        while start > 0 and lines[start - 1].strip().startswith(('///', '#[')):
            start -= 1
        blk = _block(lines, j)
        if j + len(blk) > i:
            return FN_DEF.match(lines[j]).group(1), start, lines[start:j] + blk
    lo = max(0, i - 8)
    return '', lo, lines[lo:i + 12]


def scan(src_root, rel_root, funcs, by_symbol, by_name):
    """funcs: sorted [(addr, size)]; by_symbol: mangled -> addr; by_name: 'Class::method' -> [addr].
    Returns {addr: [{kind, file, line, fn, start, code}]} with file relative to rel_root."""
    starts = [f[0] for f in funcs]
    refs = {}

    def func_of(a):
        k = bisect.bisect_right(starts, a) - 1
        return funcs[k][0] if k >= 0 and a < funcs[k][0] + funcs[k][1] else None

    def add(addr, kind, path, i, lines):
        fn, start, code = _snippet(lines, i)
        rel = os.path.relpath(path, rel_root).replace('\\', '/')
        lst = refs.setdefault(addr, [])
        key = (kind, rel, fn or start)
        if any((r['kind'], r['file'], r['fn'] or r['start']) == key for r in lst):
            return
        lst.append({'kind': kind, 'file': rel, 'line': i + 1, 'fn': fn, 'start': start + 1, 'code': ''.join(code[:MAX_LINES])})

    for path in sorted(glob.glob(os.path.join(src_root, '**', '*.rs'), recursive=True)):
        lines = open(path, encoding='utf-8', errors='replace').readlines()
        text = ''.join(lines)
        unix = path.replace('\\', '/')
        in_vm = '/mgvm/' in unix or '/gekko/' in unix
        line_at = lambda pos: text.count('\n', 0, pos)
        for m in re.finditer(r'bind\(\s*vm,\s*&\[(.*?)\],\s*([A-Za-z_][A-Za-z0-9_:]*)\s*\)', text, re.S):
            target = m.group(2).split('::')[-1]
            defs = [k for k, l in enumerate(lines) if re.match(r'^\s*(?:pub(?:\([a-z]+\))?\s+)?fn\s+%s\b' % target, l)]
            for sym in re.findall(r'"([^"]+)"', m.group(1)):
                if sym in by_symbol:
                    add(by_symbol[sym], 'hook', path, defs[0] if defs else line_at(m.start()), lines)
        for m in re.finditer(r'\.(observe|hook)\(\s*"([^"]+)"', text):
            if m.group(2) in by_symbol:
                add(by_symbol[m.group(2)], 'observe' if m.group(1) == 'observe' else 'hook', path, line_at(m.start()), lines)
        for k, l in enumerate(lines):
            for m in ADDR.finditer(l):
                a = func_of(int(m.group(1).replace('_', ''), 16))
                if a is not None:
                    add(a, 'vm' if in_vm else 'port', path, k, lines)
            if '`' in l and l.lstrip().startswith('//'):
                for m in TICK.finditer(l):
                    for a in by_name.get(m.group(1), [])[:4]:
                        add(a, 'vm' if in_vm else 'port', path, k, lines)
            if in_vm and 'call_by_name' in l:
                for m in SYM.finditer(l):
                    if m.group(1) in by_symbol:
                        add(by_symbol[m.group(1)], 'drive', path, k, lines)
    return refs
