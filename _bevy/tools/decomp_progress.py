#!/usr/bin/env python3
"""Build the decompilation progress map: every function of playgroundz.elf with its original PowerPC code, Ghidra's C,
the Rust that replicates or replaces it, and what the gekko VM does with it.

usage: python _bevy/tools/decomp_progress.py OUT_DIR   (view with _bevy/tools/decomp-map.html copied into OUT_DIR)
inputs (defaults): Remaster/reference/playgroundz.elf, D:/tools/decomp.c, scratch_progress/classify.tsv
(`mglab classify`), scratch_progress/cov_*.tsv (`EAGL_PPC_COVER` runs), _bevy/src/**/*.rs
outputs: OUT_DIR/index.json (summary of every function and unit) and OUT_DIR/chunks/NNN.json (asm / C / Rust per
function, 256 functions per chunk in address order)
"""
import bisect, glob, json, os, re, sys
from elftools.elf.elffile import ELFFile
import capstone

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ELF = os.path.join(ROOT, 'Remaster', 'reference', 'playgroundz.elf')
DECOMP = os.environ.get('DECOMP_C', 'D:/tools/decomp.c')
SCRATCH = os.path.join(ROOT, 'scratch_progress')
SRC = os.path.join(ROOT, '_bevy', 'src')
CHUNK = 256

# ---------------------------------------------------------------------------------------------------------------------
# CodeWarrior name demangling (enough for display: Class::method(args))

BASIC = {'v': 'void', 'b': 'bool', 'c': 'char', 's': 'short', 'i': 'int', 'l': 'long', 'x': 'long long', 'f': 'float',
         'd': 'double', 'w': 'wchar_t', 'e': '...'}


def _name(s, i):
    """length-prefixed name or Qn qualified name at s[i]; returns (text, next index)."""
    if i < len(s) and s[i] == 'Q' and i + 1 < len(s) and s[i + 1].isdigit():
        n = int(s[i + 1]); i += 2; parts = []
        for _ in range(n):
            t, i = _name(s, i)
            parts.append(t)
        return '::'.join(parts), i
    j = i
    while j < len(s) and s[j].isdigit():
        j += 1
    if j == i:
        raise ValueError
    n = int(s[i:j])
    t = s[j:j + n]
    if len(t) < n:
        raise ValueError
    return t, j + n


def _type(s, i):
    pre = ''
    quals = []
    while i < len(s) and s[i] in 'CVU S':
        if s[i] == 'C': quals.append('const')
        elif s[i] == 'U': pre = 'unsigned '
        elif s[i] == 'S': pre = 'signed '
        i += 1
    if i >= len(s):
        raise ValueError
    c = s[i]
    q = (' '.join(quals) + ' ') if quals else ''
    if c in 'PR':
        t, i = _type(s, i + 1)
        return q + t + ('*' if c == 'P' else '&'), i
    if c in BASIC:
        return q + pre + BASIC[c], i + 1
    if c == 'F':  # function pointer type
        args, i = _args(s, i + 1)
        if i < len(s) and s[i] == '_':
            r, i = _type(s, i + 1)
        else:
            r = 'void'
        return f'{r}(*)({args})', i
    if c == 'A':
        j = i + 1
        while j < len(s) and s[j].isdigit():
            j += 1
        t, k = _type(s, j + 1)
        return f'{t}[{s[i + 1:j]}]', k
    if c == 'M':
        cls, i = _name(s, i + 1)
        t, i = _type(s, i)
        return f'{t} {cls}::*', i
    t, i = _name(s, i)
    return q + t, i


def _args(s, i):
    out = []
    while i < len(s) and s[i] != '_':
        t, i = _type(s, i)
        out.append(t)
    return ', '.join(a for a in out if a != 'void'), i


SPECIAL = {'__ct': '{cls}', '__dt': '~{cls}', '__as': 'operator=', '__eq': 'operator==', '__ne': 'operator!=',
           '__pl': 'operator+', '__mi': 'operator-', '__ml': 'operator*', '__dv': 'operator/', '__vc': 'operator[]',
           '__cl': 'operator()', '__nw': 'operator new', '__dl': 'operator delete', '__nwa': 'operator new[]',
           '__dla': 'operator delete[]', '__lt': 'operator<', '__gt': 'operator>', '__le': 'operator<=',
           '__ge': 'operator>=', '__apl': 'operator+=', '__ami': 'operator-=', '__amu': 'operator*=',
           '__adv': 'operator/=', '__rf': 'operator->', '__op': 'operator', '__aa': 'operator&&', '__oo': 'operator||',
           '__nt': 'operator!', '__ad': 'operator&', '__or': 'operator|', '__er': 'operator^', '__ls': 'operator<<',
           '__rs': 'operator>>', '__md': 'operator%', '__pp': 'operator++', '__mm': 'operator--'}


def demangle(m):
    """(display name, class or '') for a CodeWarrior-mangled symbol; plain names come back unchanged."""
    start = 2 if m.startswith('__') else 1
    k = m.find('__', start)
    while k != -1:
        base, rest = m[:k], m[k + 2:]
        try:
            i = 0; cls = ''
            if rest and (rest[0].isdigit() or rest[0] == 'Q'):
                cls, i = _name(rest, 0)
            const = ''
            if i < len(rest) and rest[i] == 'C' and i + 1 < len(rest) and rest[i + 1] == 'F':
                const = ' const'; i += 1
            if i < len(rest) and rest[i] == 'F':
                args, j = _args(rest, i + 1)
                short = cls.split('::')[-1].split('<')[0] if cls else ''
                nm = SPECIAL.get(base, base).replace('{cls}', short)
                return (f'{cls}::{nm}({args}){const}' if cls else f'{nm}({args})'), cls
            if cls and i == len(rest):  # static data / no signature
                return f'{cls}::{base}', cls
        except (ValueError, IndexError):
            pass
        k = m.find('__', k + 1)
    return m, ''


# ---------------------------------------------------------------------------------------------------------------------

def load_elf():
    f = ELFFile(open(ELF, 'rb'))
    syms = list(f.get_section_by_name('.symtab').iter_symbols())
    text = f.get_section_by_name('.text')
    init = f.get_section_by_name('.init')
    code = {}
    for sec in (text, init):
        code[sec['sh_addr']] = sec.data()
    funcs = sorted({(s['st_value'], s['st_size'], s.name) for s in syms
                    if s['st_info']['type'] == 'STT_FUNC' and s['st_size'] > 0})
    # translation units: a FILE symbol is followed by that file's local symbols; their addresses give the file's range
    units = {}
    cur = None
    for s in syms:
        t = s['st_info']['type']
        if t == 'STT_FILE':
            cur = s.name
        elif cur and s['st_info']['bind'] == 'STB_LOCAL' and t in ('STT_FUNC', 'STT_OBJECT', 'STT_NOTYPE') \
                and text['sh_addr'] <= s['st_value'] < text['sh_addr'] + text['sh_size']:
            lo, hi = units.get(cur, (s['st_value'], s['st_value'] + max(s['st_size'], 1)))
            units[cur] = (min(lo, s['st_value']), max(hi, s['st_value'] + max(s['st_size'], 4)))
    return funcs, units, code


def read_code(code, addr, size):
    for base, data in code.items():
        if base <= addr < base + len(data):
            o = addr - base
            return data[o:o + size]
    return b''


def load_decomp():
    out = {}
    head = re.compile(r'^//==== (.*) @ ([0-9a-f]{8})$')
    cur, buf = None, []
    with open(DECOMP, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = head.match(line.rstrip('\n'))
            if m:
                if cur is not None:
                    out[cur] = ''.join(buf).strip('\n')
                cur, buf = int(m.group(2), 16), []
            elif cur is not None:
                buf.append(line)
    if cur is not None:
        out[cur] = ''.join(buf).strip('\n')
    return out


def load_classify():
    out = {}
    p = os.path.join(SCRATCH, 'classify.tsv')
    if os.path.exists(p):
        for line in open(p, encoding='utf-8'):
            a, name, kind = line.rstrip('\n').split('\t')
            out[int(a, 16)] = kind
    return out


def load_coverage():
    """{addr: (scenario mask, entries)} and the scenario names (from cov_<name>.tsv)."""
    files = sorted(glob.glob(os.path.join(SCRATCH, 'cov_*.tsv')))
    names = [os.path.basename(p)[4:-4] for p in files]
    cov = {}
    for k, p in enumerate(files):
        for line in open(p, encoding='utf-8'):
            a, n = line.split('\t')
            a = int(a, 16)
            m, t = cov.get(a, (0, 0))
            cov[a] = (m | (1 << k), t + int(n))
    return cov, names


SCENARIO_LABELS = {
    'mg0': 'Dart Shootout', 'mg1': 'RC Cars', 'mg2': 'Tetherball', 'mg3': 'Dodgeball', 'mg4': 'Footie',
    'mg5': 'Paper Airplanes', 'mg6': 'Wallball', 'mg8': 'Free Throw', 'wrace': 'World: dare, race, sticker award',
    'wdrib': 'World: Dribbling', 'wbug': 'World: area gate + Bug Hunt', 'wking': 'World: Sticker King gauntlet',
    'wbook': 'World: sticker book + report card'}

# ---------------------------------------------------------------------------------------------------------------------
# Rust references

FN_DEF = re.compile(r'^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)')
ITEM_DEF = re.compile(r'^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:fn|struct|enum|const|static|impl|mod|trait)\b')


def block_from(lines, i, cap=140):
    """lines i.. through the end of the item that starts at line i (brace matched); capped."""
    depth = 0; seen = False; out = []
    for j in range(i, min(len(lines), i + cap)):
        l = lines[j]
        out.append(l)
        code = re.sub(r'//.*', '', l)
        code = re.sub(r'"(?:[^"\\]|\\.)*"', '""', code)
        depth += code.count('{') - code.count('}')
        if '{' in code:
            seen = True
        if seen and depth <= 0:
            break
        if not seen and code.rstrip().endswith(';'):
            break
    return out


def snippet(lines, i):
    """(enclosing fn name, first line index, code) for a reference on line i."""
    l = lines[i].strip()
    if l.startswith('//'):
        # a doc / line comment describes the item below it
        j = i
        while j < len(lines) and lines[j].strip().startswith(('//', '#[')):
            j += 1
        if j < len(lines) and ITEM_DEF.match(lines[j]):
            start = i
            while start > 0 and lines[start - 1].strip().startswith(('///', '//!', '//')):
                start -= 1
            m = FN_DEF.match(lines[j])
            return (m.group(1) if m else lines[j].strip()[:60]), start, lines[start:j] + block_from(lines, j)
    j = i
    while j >= 0 and not FN_DEF.match(lines[j]):
        j -= 1
    if j < 0:
        return '', max(0, i - 8), lines[max(0, i - 8):i + 12]
    start = j
    while start > 0 and lines[start - 1].strip().startswith(('///', '#[')):
        start -= 1
    blk = block_from(lines, j)
    if j + len(blk) <= i:  # reference is outside that fn (module level)
        return '', max(0, i - 8), lines[max(0, i - 8):i + 12]
    return FN_DEF.match(lines[j]).group(1), start, lines[start:j] + blk


def scan_rust(funcs, by_mangled, by_demangled):
    starts = [f[0] for f in funcs]
    refs = {}  # addr -> list of refs

    def add(addr, kind, path, line_i, lines):
        fn, start, code = snippet(lines, line_i)
        rel = os.path.relpath(path, os.path.join(ROOT, '_bevy')).replace('\\', '/')
        lst = refs.setdefault(addr, [])
        key = (kind, rel, fn or start)
        if any((r['kind'], r['file'], r['fn'] or r['start']) == key for r in lst):
            return
        lst.append({'kind': kind, 'file': rel, 'line': line_i + 1, 'fn': fn, 'start': start + 1,
                    'code': ''.join(code[:140])})

    def func_of(a):
        k = bisect.bisect_right(starts, a) - 1
        if k >= 0 and a < funcs[k][0] + funcs[k][1]:
            return funcs[k][0]
        return None

    addr_re = re.compile(r'0x(80[0-9a-fA-F]{6}|80[0-9a-fA-F]{2}_[0-9a-fA-F]{4})\b')
    tick_re = re.compile(r'`([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_~][A-Za-z0-9_]*)+)(?:\([^`]*\))?`')
    str_re = re.compile(r'"([A-Za-z_][A-Za-z0-9_<>,]*__[A-Za-z0-9_<>,]+)"')
    for path in glob.glob(os.path.join(SRC, '**', '*.rs'), recursive=True):
        lines = open(path, encoding='utf-8', errors='replace').readlines()
        text = ''.join(lines)
        rel = path.replace('\\', '/')
        in_vm = '/mgvm/' in rel or '/gekko/' in rel
        line_at = lambda pos: text.count('\n', 0, pos)
        # hook bindings: bind(vm, &["A", "B"], f)
        for m in re.finditer(r'bind\(\s*vm,\s*&\[(.*?)\],\s*([A-Za-z_][A-Za-z0-9_:]*)\s*\)', text, re.S):
            target = m.group(2).split('::')[-1]
            defs = [k for k, l in enumerate(lines) if re.match(rf'^\s*(?:pub(?:\([a-z]+\))?\s+)?fn\s+{target}\b', l)]
            for nm in re.findall(r'"([^"]+)"', m.group(1)):
                a = by_mangled.get(nm)
                if a is not None:
                    add(a, 'hook', path, defs[0] if defs else line_at(m.start()), lines)
        for m in re.finditer(r'\.(observe|hook)\(\s*"([^"]+)"', text):
            a = by_mangled.get(m.group(2))
            if a is not None:
                add(a, 'observe' if m.group(1) == 'observe' else 'hook', path, line_at(m.start()), lines)
        for k, l in enumerate(lines):
            for m in addr_re.finditer(l):
                a = func_of(int(m.group(1).replace('_', ''), 16))
                if a is not None:
                    add(a, 'vm' if in_vm else 'port', path, k, lines)
            if '`' in l and l.lstrip().startswith('//'):
                for m in tick_re.finditer(l):
                    for a in by_demangled.get(m.group(1), [])[:4]:
                        add(a, 'vm' if in_vm else 'port', path, k, lines)
            if in_vm and 'call_by_name' in l:
                for m in str_re.finditer(l):
                    a = by_mangled.get(m.group(1))
                    if a is not None:
                        add(a, 'drive', path, k, lines)
    return refs

# ---------------------------------------------------------------------------------------------------------------------

def disasm(md, addr, data, names):
    out = []
    off = 0
    while off < len(data):
        ins = next(md.disasm(data[off:off + 4], addr + off), None)
        word = data[off:off + 4].hex()
        if ins is None:
            out.append(f'{addr + off:08x}  {word}  .word 0x{word}')
        else:
            op = ins.op_str
            if ins.mnemonic.startswith('b') and op:
                m = re.search(r'0x([0-9a-f]+)$', op)
                if m:
                    t = int(m.group(1), 16)
                    if t in names:
                        op = op[:m.start()] + names[t]
                    elif addr <= t < addr + len(data):
                        op = op[:m.start()] + f'.L_{t:08x}'
            out.append(f'{addr + off:08x}  {word}  {ins.mnemonic:<8} {op}'.rstrip())
        off += 4
    return '\n'.join(out)


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(SCRATCH, 'site')
    os.makedirs(os.path.join(out_dir, 'chunks'), exist_ok=True)
    funcs, units, code = load_elf()
    print('functions', len(funcs), 'units', len(units))
    names = {a: n for a, s, n in funcs}
    dem = {a: demangle(n) for a, s, n in funcs}
    by_mangled = {n: a for a, s, n in funcs}
    by_demangled = {}
    for a, (d, cls) in dem.items():
        q = d.split('(')[0]
        by_demangled.setdefault(q, []).append(a)
    decomp = load_decomp()
    print('ghidra bodies', len(decomp))
    vm = load_classify()
    cov, scen = load_coverage()
    print('classified', len(vm), 'covered', len(cov), 'scenarios', scen)
    refs = scan_rust(funcs, by_mangled, by_demangled)
    print('functions with rust refs', len(refs))

    # unit of each function: the file whose local-symbol range contains it, else the nearest preceding range
    ulist = sorted((lo, hi, n) for n, (lo, hi) in units.items())
    ustarts = [u[0] for u in ulist]
    unit_names = [u[2] for u in ulist] + ['(unattributed)']
    md = capstone.Cs(capstone.CS_ARCH_PPC, capstone.CS_MODE_32 | capstone.CS_MODE_BIG_ENDIAN
                     | getattr(capstone, 'CS_MODE_PS', 0))
    rows = []
    chunks = {}
    for idx, (a, size, name) in enumerate(funcs):
        k = bisect.bisect_right(ustarts, a) - 1
        unit = k if k >= 0 and a < ulist[k][1] + 0x10000 else len(ulist)
        kind = vm.get(a, 'native')
        m, entries = cov.get(a, (0, 0))
        rr = refs.get(a, [])
        kinds = {r['kind'] for r in rr}
        # primary status (one colour per function)
        if 'port' in kinds:
            st = 'port'
        elif kind in ('host', 'observe'):
            st = 'host'
        elif kind == 'native' and entries:
            st = 'run'
        elif kind == 'native':
            st = 'native'
        else:
            st = kind  # stub / trap
        d, cls = dem[a]
        rows.append([a, size, unit, st, kind, m, entries, len(rr), d, name])
        body = read_code(code, a, size)
        chunks.setdefault(idx // CHUNK, {})[f'{a:08x}'] = {
            'asm': disasm(md, a, body, names),
            'c': decomp.get(a, ''),
            'rust': rr,
        }
    for k, data in chunks.items():
        with open(os.path.join(out_dir, 'chunks', f'{k:03d}.json'), 'w', encoding='utf-8') as f:
            json.dump(data, f, separators=(',', ':'))
    index = {
        'source': 'playgroundz.elf (EA Playground, Wii)',
        'chunk': CHUNK,
        'units': unit_names,
        'scenarios': [SCENARIO_LABELS.get(s, s) for s in scen],
        'cols': ['addr', 'size', 'unit', 'status', 'vm', 'scenarios', 'entries', 'rustRefs', 'name', 'mangled'],
        'funcs': rows,
    }
    with open(os.path.join(out_dir, 'index.json'), 'w', encoding='utf-8') as f:
        json.dump(index, f, separators=(',', ':'))
    from collections import Counter
    c = Counter(r[3] for r in rows)
    b = Counter()
    for r in rows:
        b[r[3]] += r[1]
    total = sum(r[1] for r in rows)
    for k in sorted(c, key=lambda k: -b[k]):
        print(f'{k:8} {c[k]:6} funcs {b[k] / total * 100:6.2f}% bytes')


if __name__ == '__main__':
    main()
