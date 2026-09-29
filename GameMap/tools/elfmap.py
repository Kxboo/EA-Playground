"""Read-only analysis library for the EA Playground (Wii, RPXE) executable.

The ELF is not stored in this repository (see `.gitignore`). Every generated file under
`GameMap/data` can be regenerated from it with `gen_map.py --elf <path>`.

Facts this module relies on (all verified against the supplied binary, see docs/):
  * ELF32 big-endian PowerPC, Metrowerks CodeWarrior output, unstripped (symbols + names).
  * Wii small-data bases are set in `__init_registers` (0x80006290): r2 = 0x8060A540
    (.sdata2 base), r13 = 0x80603EE0 (.sdata base), r1 = 0x80647E80.
  * MWCC emits each translation unit's `__sinit_\\<file>_cpp` at the *end* of that unit's code and
    its STT_FILE symbol precedes the unit's local symbols, so a unit's end address is the end of its
    last local function. Units are contiguous in link order.
"""
import bisect
import io
import os
import re
import struct
from collections import defaultdict

from elftools.elf.elffile import ELFFile

try:
    from capstone import CS_ARCH_PPC, CS_MODE_32, CS_MODE_BIG_ENDIAN, CS_MODE_PS, Cs
except ImportError:  # disassembly is optional for pure symbol work
    Cs = None

import mwdemangle

SDA_R2 = 0x8060A540
SDA_R13 = 0x80603EE0
TEXT_LO, TEXT_HI = 0x80004000, 0x8041CEE0

# ---------------------------------------------------------------------------------------------
# Subsystem classification. Source-unit names come straight from the ELF's STT_FILE symbols.
# Order matters: first match wins. (regex on file name, tier, subsystem)
# ---------------------------------------------------------------------------------------------
_RULES = [
    # --- game layer (link-order block aientity.cpp .. trcUtil.cpp) ---
    (r"^(aientity|aiutils|ancientevil|compulsion|metaworld\w+|simplemovementaientity)\.cpp$", "game", "ai"),
    (r"^poi\w*\.cpp$", "game", "ai"),
    (r"^attrib\w*\.cpp$", "game", "attribute-db"),
    (r"^(Playground_AEMS|Au\w+|Audio)\.cpp$", "game", "audio"),
    (r"^(aicharactercontrol|character\w*|localcharactercontrol|npc\w+|shadowrenderentity)\.cpp$", "game", "characters"),
    (r"^conversation\w+\.cpp$", "game", "conversation"),
    (r"^(csvparser|parser|db)\.cpp$", "game", "data-parsers"),
    (r"^(partfx\w*|worldeffectmanager|Lion_Unity|Lion\w+)\.cpp$", "game", "effects"),
    (r"^\w+Handlers\.cpp$", "game", "frontend"),
    (r"^(FEManager|selectablecharacter|highres)\.cpp$", "game", "frontend"),
    (r"^(controller|pgconga)\.cpp$", "game", "input"),
    (r"^(freethrow\w*|mgfreethrow|dribblingmanager|highfivemanager)\.cpp$", "game", "mg-freethrow"),
    (r"^micro\w+\.cpp$", "game", "mg-microbug"),
    (r"^(ds\w+|aidartshootout|dartshootoutcamera|mgdartshootout)\.cpp$", "game", "mg-dartshootout"),
    (r"^(aidodgeball|dodgeball\w*|mgdodgeball)\.cpp$", "game", "mg-dodgeball"),
    (r"^(aifootie|footie\w+|mgfootie)\.cpp$", "game", "mg-footie"),
    (r"^(mgpaperairplanes|pa\w+|paperairplane\w*)\.cpp$", "game", "mg-paperairplanes"),
    (r"^(aircdriver|mgrccars|rc\w+)\.cpp$", "game", "mg-rccars"),
    (r"^(aitetherball|mgtetherball|tetherball)\.cpp$", "game", "mg-tetherball"),
    (r"^(aiwallball|mgwallball|wallball\w*)\.cpp$", "game", "mg-wallball"),
    (r"^(minigame|GameState|Locale|Main|multiplayermode|ProductCode|profile|strapwarn|trcUtil)\.cpp$", "game", "core-flow"),
    (r"^(rmangle|rMath|rmtagpoint)\.cpp$", "game", "math"),
    # --- engine layer (EAGL "Ren" renderer, cameras, world, physics glue) ---
    (r"^physics\w+\.cpp$", "engine", "physics-glue"),
    (r"^(\w*camera|cameramanager)\.cpp$", "engine", "cameras"),
    (r"^(engine|entity|boundingbox|scene|frustumtest|fadetocoloureffect|fullscreeneffectsmanager|lightmanager|worldlightmanager|shadowmanager|skydome|tarmanager|cachedmodel|cachedanimatedmodel|rendertexture)\.cpp$", "engine", "render-scene"),
    (r"^(animationmanager|animationstate|animationstategraph|marker)\.cpp$", "engine", "animation-glue"),
    (r"^(accessibilitymanager|areadrawmanager|areamanager|placeables|playgroundGate|playgroundWorld|spawnmanager|spawnregion|World|WorldManager)\.cpp$", "engine", "world"),
    (r"^(assetmanager|CString|debugmenu\w*|DipSwitch|inifile\w*|lionallocator|MemMgr|movie|pgdebugmenu|pgFile|pgmovieplayer|slotpool|Utils|screencapture|targawriter|videocapture|embeddedfontdata)\.cpp$", "engine", "engine-support"),
    (r"^(codegen|model|symbolinit|platform_model|device\w*|rendercontext\w*|rendermethod|state|tar|tevstage|texturerc|viewport\w*|profiler_cmn|fonteagl|transform|pcode|singledraw|drawimmediate|drawarray|cmn\w+|positionallight|Playground\w+|Gouraud\w*|TextureApt)\.cpp$", "engine", "renderer-eagl"),
    (r"^(wiipad\w*|gcpad|pad|\w+padmodehandler)\.cpp$", "engine", "pad-drivers"),
    # --- EA / third-party middleware and SDK ---
    (r"^hk\w+\.cpp$", "middleware", "havok"),
    (r"^(lyt_|snd_|ut_|db_|HBM|ef_|math_triangular)\w*", "middleware", "nw4r-hbm"),
    (r"^(Apt\w*|Animation\w*Class|animationObjectList|animationUtils|aip\w*|composer|decomposer|broker|EAString|StringPool|DogmaAllocator|TextFormat|aptrealfont|timer)\.cpp$", "middleware", "apt-ui-runtime"),
    (r"^TRC\w*\.cpp$", "middleware", "trc-wii-requirements"),
    (r"^l(api|code|debug|do|func|gc|lex|mem|object|opcodes|parser|state|string|table|tm|undump|vm|zio)\.cpp$", "middleware", "lua"),
    (r"^(Console|Expose|RVL|conga_master)\.cpp$", "middleware", "exposure-conga"),
    (r"^(ti\w+|MyTi\w+)\.cpp$", "middleware", "text-input"),
    (r"^(rcmp\w*|maddec\w*|madidct|avplayer|audioplayer|boolhuff|borders|clamp|deblock|decode\w+|DeInterlace|dering|DFrameR|FrameIni|Huffman|idctpart|loopfilter|pb_globals|postproc|quantize|recon\w*|scale|simpledeblocker|TokenEntropy|vfwpbdll_if|vputil|duck_mem|allocator|bigswizzler|bigyuvswizzler|hwvideodisplaylist|criticalpath|D\w*SystemDependant|d\w*optsystemdependant|u\w*optsystemdependant)\.c(pp)?$", "middleware", "video-vp6-mp3"),
    (r"^(Skeleton|Fn\w+|\w*Chan|\w*Channel|StatelessQ|StatelessF3|System\w*|ScratchBuffer|BoneMask|AnimBank|CompoundChannel|Delta\w+|PoseAnim|MemoryPoolManager|Attribute|PhaseChan)\.cpp$", "middleware", "ea-animation"),
    (r"^(font\w+|clut\w*|creates|locatshp|shp\w+|wiimem|mb\w+|mem\w+|refdecode|unpack)\.c(pp)?$", "middleware", "ea-fonts-shapes-mem"),
    (r"^(mat4math|rmtrigfl|rmv3\w+|mathvars|matrix44\S*|trig|vector[23]|cmn|crc\w*|filesys\w*|hla\w+|hls\w+|locatbig|stream|syncfile|filedev|inittmr|initvblt|printn3|signals|threads|timerthread|ustrcore|locale|abort\w*|base|conspool|printmessage|dlopen|sympool|bitfield|debugger|exit|mem(clear|copy|fill)|mutex2|printstr|systask|systemvars|validadr)\.c(pp)?$", "middleware", "ea-core-libs"),
    (r"^s\w+\.c(pp)?$", "middleware", "ea-audio-sndlib"),
    (r"^(coda|ealayer3decf|eaxadecf|lbmpeg\w*|mpeg\w+|mtdec\w+|setmemcpy|sf\w+|scrsfl|sinit\w+|slinkmix|smix\w+|stretch|sup\w+|sx87d16|sdfx|sdspmix|snddrv|spch\w+|sptick|sputil|csis)\.c(pp)?$", "middleware", "ea-audio-sndlib"),
]
_CLASS_OVERRIDE = [(re.compile(r"PadModeHandler$"), "engine", "pad-drivers")]
_COMPILED = [(re.compile(p), t, s) for p, t, s in _RULES]

# Anything not matched: SDK/CRT/vendor .c files (dolphin/RVL SDK, MSL C, Bluetooth stack, TRK)
_SDK_HINT = re.compile(r"\.(c|s|cp)$|^(OS|GX|AX|dvd|vi|ai|mtx|WPAD|WUD|KPAD|NAND|NWC24|ipc|fs|bta|btm|btu|l2c|sdp|rfc|hid|gap|port|usb|gki|mem_|e_|s_|w_|k_)", re.I)


def classify(fname):
    for rx, tier, sub in _COMPILED:
        if rx.match(fname):
            return tier, sub
    if fname.endswith((".c", ".s", ".cp")) or _SDK_HINT.search(fname):
        return "middleware", "wii-sdk-crt"
    return "middleware", "other-cpp"


class Elf:
    def __init__(self, path):
        self.path = path
        self.raw = open(path, "rb").read()
        self.elf = ELFFile(io.BytesIO(self.raw))
        self.secs = [(s["sh_addr"], s["sh_size"], s["sh_offset"], s.name, s["sh_type"])
                     for s in self.elf.iter_sections() if s["sh_addr"] and s["sh_type"] == "SHT_PROGBITS"]
        self.allsecs = [(s["sh_addr"], s["sh_size"], s.name) for s in self.elf.iter_sections() if s["sh_addr"]]
        self._load_symbols()
        self._build_units()
        self._refine_attribution()
        self._rebuild_units_from_attribution()
        self._md = None

    # -- memory ---------------------------------------------------------------------------
    def rd(self, addr, n):
        for a, sz, off, nm, _ in self.secs:
            if a <= addr < a + sz:
                n = min(n, a + sz - addr)
                return self.raw[off + addr - a: off + addr - a + n]
        return b""

    def u32(self, addr):
        b = self.rd(addr, 4)
        return struct.unpack(">I", b)[0] if len(b) == 4 else None

    def f32(self, addr):
        b = self.rd(addr, 4)
        return struct.unpack(">f", b)[0] if len(b) == 4 else None

    def section_of(self, addr):
        for a, sz, nm in self.allsecs:
            if a <= addr < a + sz:
                return nm
        return None

    def cstr(self, addr, maxlen=200):
        b = self.rd(addr, maxlen)
        if not b:
            return None
        z = b.split(b"\0")[0]
        if len(z) >= 1 and all(32 <= c < 127 or c in (9, 10) for c in z):
            return z.decode()
        return None

    # -- symbols --------------------------------------------------------------------------
    def _load_symbols(self):
        st = self.elf.get_section_by_name(".symtab")
        self.syms = []      # (addr,size,type,name,bind)
        self.file_syms = []  # (index, name, [local func end])
        cur = None
        for s in st.iter_symbols():
            t = s["st_info"]["type"]
            if t == "STT_FILE":
                cur = [s.name, 0, 0xFFFFFFFF]
                self.file_syms.append(cur)
                continue
            if not s.name or t not in ("STT_FUNC", "STT_OBJECT"):
                continue
            a, sz = s["st_value"], s["st_size"]
            self.syms.append((a, sz, t[4:], s.name, s["st_info"]["bind"][4:]))
            if cur and t == "STT_FUNC" and TEXT_LO <= a < TEXT_HI:
                cur[1] = max(cur[1], a + sz)
                cur[2] = min(cur[2], a)
        self.syms.sort()
        self._addrs = [s[0] for s in self.syms]
        self.by_name = {}
        for a, sz, t, n, b in self.syms:
            self.by_name.setdefault(n, (a, sz, t))

    def sym_at(self, addr):
        i = bisect.bisect_right(self._addrs, addr) - 1
        while i >= 0:
            a, sz, t, n, b = self.syms[i]
            if a <= addr < a + max(sz, 1):
                return n + ("+0x%x" % (addr - a) if addr != a else "")
            if addr - a > 0x100000:
                break
            i -= 1
            if i < 0 or self.syms[i][0] < addr - 0x40:
                break
        return None

    @staticmethod
    def _norm(x):
        return re.sub(r"[^a-z0-9]", "", x.lower())

    def _refine_attribution(self):
        """Attribute every function to a source unit, resolving units that have no local symbol.

        MWCC anchors a unit's end with its last local symbol (`__sinit_`). Units without any local function have no
        anchor, so the functions between two anchors belong to the *run of FILE entries* between them (link order).
        The run is partitioned monotonically by dynamic programming, scoring how well each function's class/name
        matches a file name. `unit_conf`: 'anchored' (only one candidate), 'inferred' (votes decided), 'assumed'.
        """
        files = self.file_syms  # [name, end, lo] in link order
        funcs = sorted((a, sz, n) for a, sz, t, n, b in self.syms if t == "FUNC" and TEXT_LO <= a < TEXT_HI)
        self.func_unit = {}
        prev_i, prev_end = -1, TEXT_LO
        anchored_idx = [i for i, f in enumerate(files) if f[1]]
        anchored_idx.sort(key=lambda i: files[i][1])
        import bisect
        faddrs = [f[0] for f in funcs]
        for j in anchored_idx:
            end = files[j][1]
            lo_i = bisect.bisect_left(faddrs, prev_end)
            hi_i = bisect.bisect_left(faddrs, end)
            seg = funcs[lo_i:hi_i]
            # candidate files: all FILE entries after the previous anchored one up to and including j (link order)
            cands = [k for k in range(min(prev_i, j) + 1, j + 1)] if prev_i < j else [j]
            if prev_i >= j:  # link order not monotone here; fall back to the anchored file only
                cands = [j]
            names = [files[k][0] for k in cands]
            stems = [self._norm(os.path.splitext(os.path.basename(n.replace("\\", "/")))[0]) for n in names]
            m = len(cands)
            if m == 1 or not seg:
                for a, sz, n in seg:
                    self.func_unit[a] = (names[-1], "anchored" if m == 1 else "assumed")
            else:
                def score(fn, k):
                    d = mwdemangle.demangle(fn)
                    cls = self._norm(d["cls"]) if d["cls"] else ""
                    nm = self._norm(fn)
                    st = stems[k]
                    if len(st) < 4:
                        return 0
                    sc = 0
                    if cls and (st == cls.split("::")[-1] or st in cls or (len(cls) > 4 and cls in st)):
                        sc = 2
                    elif st in nm:
                        sc = 1
                    return sc
                n_ = len(seg)
                NEG = -10 ** 9
                dp = [[NEG] * m for _ in range(n_ + 1)]
                back = [[0] * m for _ in range(n_ + 1)]
                for k in range(m):
                    dp[0][k] = 0
                for f_i in range(n_):
                    a, sz, fn = seg[f_i]
                    forced = None
                    mm = re.match(r"^__sinit_\\(.+)_(cpp|c)$", fn)
                    if mm:
                        cand = [k for k in range(m) if self._norm(names[k]).startswith(self._norm(mm.group(1)))]
                        forced = cand[-1] if cand else None
                    for k in range(m):
                        best, bk = NEG, 0
                        for kk in range(k + 1):  # monotone: previous state <= k
                            v = dp[f_i][kk] - (0.01 if kk != k else 0)
                            if v > best:
                                best, bk = v, kk
                        sc = score(fn, k)
                        if forced is not None:
                            sc = 100 if k == forced else -100
                        dp[f_i + 1][k] = best + sc
                        back[f_i + 1][k] = bk
                k = max(range(m), key=lambda k: (dp[n_][k], k))
                assign = [0] * n_
                for f_i in range(n_, 0, -1):
                    assign[f_i - 1] = k
                    k = back[f_i][k]
                for (a, sz, fn), k in zip(seg, assign):
                    sc = score(fn, k)
                    self.func_unit[a] = (names[k], "inferred" if sc > 0 else "assumed")
            prev_i, prev_end = j, end
        # trailing functions after the last anchor
        for a, sz, n in funcs[bisect.bisect_left(faddrs, prev_end):]:
            self.func_unit[a] = (files[anchored_idx[-1]][0], "assumed")

    def _build_units(self):
        units = sorted([(e, n, lo) for n, e, lo in self.file_syms if e], key=lambda u: u[0])
        prev = TEXT_LO
        self.units = []  # dict rows in link order
        for idx, (end, name, lo) in enumerate(units):
            tier, sub = classify(name)
            self.units.append({"idx": idx, "file": name, "start": prev, "end": end,
                               "tier": tier, "subsystem": sub})
            prev = end
        self._unit_ends = [u["end"] for u in self.units]

    def _rebuild_units_from_attribution(self):
        """Unit table from the refined attribution: one row per (file, contiguous run) with its address range."""
        rows = []
        cur = None
        for a in sorted(self.func_unit):
            name, conf = self.func_unit[a]
            sz = self.by_name and next((s[1] for s in self._by_addr_cache(a)), 0)
            if cur and cur["file"] == name:
                cur["end"] = a + sz; cur["funcs"] += 1
                if conf != "anchored":
                    cur["conf"].add(conf)
                else:
                    cur["conf"].add(conf)
            else:
                if cur:
                    rows.append(cur)
                cur = {"file": name, "start": a, "end": a + sz, "funcs": 1, "conf": {conf}}
        if cur:
            rows.append(cur)
        units = []
        for i, r in enumerate(rows):
            tier, sub = classify(r["file"])
            units.append({"idx": i, "file": r["file"], "start": r["start"], "end": r["end"], "tier": tier,
                          "subsystem": sub, "funcs_refined": r["funcs"], "conf": ",".join(sorted(r["conf"]))})
        self.units = units
        self._unit_starts = [u["start"] for u in units]

    def _by_addr_cache(self, a):
        if not hasattr(self, "_fsz"):
            self._fsz = {}
            for x, sz, t, n, b in self.syms:
                if t == "FUNC":
                    self._fsz.setdefault(x, sz)
        return [(a, self._fsz.get(a, 0))]

    def unit_of(self, addr):
        if hasattr(self, "_unit_starts"):
            i = bisect.bisect_right(self._unit_starts, addr) - 1
            if i >= 0:
                u = self.units[i]
                if addr < u["end"] or addr in self.func_unit:
                    fu = self.func_unit.get(addr)
                    if fu and fu[0] != u["file"]:  # a function inside another run (interleaved units): synthesize
                        t, sb = classify(fu[0])
                        return {"idx": -1, "file": fu[0], "start": addr, "end": addr, "tier": t, "subsystem": sb, "conf": fu[1]}
                    return u
            return None
        return self._unit_of_old(addr)

    def _unit_of_old(self, addr):
        i = bisect.bisect_right(self._unit_ends, addr)
        return self.units[i] if i < len(self.units) else None

    def functions(self):
        out = []
        for a, sz, t, n, b in self.syms:
            if t == "FUNC":
                u = self.unit_of(a) if TEXT_LO <= a < TEXT_HI else None
                d = mwdemangle.demangle(n)
                tier, sub = (u["tier"], u["subsystem"]) if u else ("", "")
                for rx, t2, s2 in _CLASS_OVERRIDE:  # units without local symbols have no range anchor
                    if d["cls"] and rx.search(d["cls"]):
                        tier, sub = t2, s2
                out.append({"addr": a, "size": sz, "name": n, "cls": d["cls"], "method": d["method"],
                            "args": d["args"], "const": d["const"], "bind": b,
                            "file": u["file"] if u else "", "tier": tier,
                            "subsystem": sub})
        return out

    # -- disassembly ----------------------------------------------------------------------
    @property
    def md(self):
        if self._md is None:
            if Cs is None:
                raise RuntimeError("capstone is required for disassembly (pip install capstone)")
            self._md = Cs(CS_ARCH_PPC, CS_MODE_32 | CS_MODE_BIG_ENDIAN | CS_MODE_PS)
            self._md.detail = True
        return self._md

    def disasm(self, addr, size):
        """Yield dicts with address, mnemonic, op_str, target (branch), regs (const-propagated)."""
        code = self.rd(addr, size)
        regs = {1: None, 2: SDA_R2, 13: SDA_R13}
        for ins in self.md.disasm(code, addr):
            m, op = ins.mnemonic, ins.op_str
            parts = [p.strip() for p in op.split(",")] if op else []
            rec = {"addr": ins.address, "mn": m, "op": op, "target": None, "const": None}

            def R(s):
                mm = re.fullmatch(r"r(\d+)", s)
                return int(mm.group(1)) if mm else None

            def I(s):
                try:
                    return int(s, 0)
                except ValueError:
                    return None
            if m in ("bl", "b") and parts and parts[0].startswith("0x"):
                rec["target"] = int(parts[0], 16)
            wrote = None
            if m == "lis" and len(parts) == 2:
                wrote = R(parts[0]); v = I(parts[1])
                regs[wrote] = ((v << 16) & 0xFFFFFFFF) if v is not None else None
            elif m == "li" and len(parts) == 2:
                wrote = R(parts[0]); v = I(parts[1])
                regs[wrote] = (v & 0xFFFFFFFF) if v is not None else None
            elif m in ("addi", "subi") and len(parts) == 3:
                wrote = R(parts[0]); ra = R(parts[1]); v = I(parts[2])
                base = regs.get(ra) if ra is not None else None
                if ra == 0 or ra is None:
                    base = 0
                if base is not None and v is not None:
                    regs[wrote] = (base + (v if m == "addi" else -v)) & 0xFFFFFFFF
                else:
                    regs[wrote] = None
            elif m in ("ori",) and len(parts) == 3:
                wrote = R(parts[0]); rs = R(parts[1]); v = I(parts[2])
                b = regs.get(rs)
                regs[wrote] = (b | v) & 0xFFFFFFFF if b is not None and v is not None else None
            elif m == "mr" and len(parts) == 2:
                wrote = R(parts[0]); regs[wrote] = regs.get(R(parts[1]))
            else:
                try:
                    for r in ins.regs_access()[1]:
                        nm = ins.reg_name(r)
                        mm = re.fullmatch(r"r(\d+)", nm or "")
                        if mm:
                            regs[int(mm.group(1))] = None
                except Exception:
                    pass
            if m in ("bl", "bctrl", "blrl"):  # call clobbers volatile regs (r3..r12), r0
                for k in list(regs):
                    if k == 0 or 3 <= k <= 12:
                        regs[k] = None
            rec["regs"] = dict(regs)
            if wrote is not None and regs.get(wrote) is not None:
                rec["const"] = regs[wrote]
            yield rec

    def disasm_text(self, name_or_addr, size=None):
        if isinstance(name_or_addr, str):
            a, sz, _ = self.by_name[name_or_addr]
        else:
            a, sz = name_or_addr, size or 256
        lines = []
        for r in self.disasm(a, size or sz):
            note = ""
            if r["target"] is not None:
                note = " ; " + (self.sym_at(r["target"]) or hex(r["target"]))
            elif r["const"] is not None and r["mn"] in ("addi", "ori", "subi"):
                s = self.cstr(r["const"])
                sy = self.sym_at(r["const"])
                note = " ; =0x%08x" % r["const"] + (' "%s"' % s if s else (" " + sy if sy else ""))
            lines.append("%08x  %-8s %s%s" % (r["addr"], r["mn"], r["op"], note))
        return "\n".join(lines)

    def string_tables_rows(self, lo=0x8041CEE0, hi=0x80608000):
        """[(symbol, index, table_addr_hex, ptr_hex, text)] for arrays of string pointers."""
        rows = []
        for ad, sz, t, name, b in self.syms:
            if t != "OBJECT" or sz < 8 or sz % 4 or sz > 8192 or name.startswith(("__vt__", "__RTTI", "@")):
                continue
            if self.section_of(ad) not in (".data", ".rodata", ".sdata"):
                continue
            words = [self.u32(ad + i) for i in range(0, sz, 4)]
            strs = [self.cstr(x) if x and lo <= x < hi else None for x in words]
            good = sum(1 for x in strs if x)
            if good >= 2 and good >= 0.6 * len(words):
                for i, (wd, st_) in enumerate(zip(words, strs)):
                    rows.append((name, i, "%08x" % ad, "%08x" % wd if wd else "0", st_ if st_ else ""))
        return rows

    # -- string-compare enum tables (ConvertStringTo* style parsers) ------------------------
    def string_enum(self, func_name, compare_hint="compare__7CStringCFPCc"):
        """Recover {token: value} from an if-chain of CString::compare + `li r3,N`."""
        a, sz, _ = self.by_name[func_name]
        cmp_addr = self.by_name[compare_hint][0]
        rows = list(self.disasm(a, sz))
        calls = [i for i, r in enumerate(rows) if r["mn"] == "bl" and r["target"] == cmp_addr]
        out = []
        for k, ci in enumerate(calls):
            r4 = rows[ci]["regs"]  # regs after call have volatile cleared; use the value before
            pre = rows[ci - 1]["regs"].get(4) if ci else None
            s = self.cstr(pre) if pre else None
            end = calls[k + 1] if k + 1 < len(calls) else len(rows)
            lis = [int(re.match(r"r3, (-?\w+)", rows[j]["op"]).group(1), 0)
                   for j in range(ci, end) if rows[j]["mn"] == "li" and rows[j]["op"].startswith("r3,")]
            if not lis:  # branchless final case: `andi. r3, r0, N`
                for j in range(ci, end):
                    mm = re.match(r"r3, r0, (\w+)", rows[j]["op"]) if rows[j]["mn"] == "andi." else None
                    if mm:
                        lis = [int(mm.group(1), 0)]
            out.append({"token": s, "value": lis[-1] if lis else None,
                        "fallback": lis[0] if len(lis) > 1 else None})
        return out
