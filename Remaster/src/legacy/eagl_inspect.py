"""
eagl_inspect.py
---------------
Decoupled EAGL pre-flight inspector for EA Sports Wii .o model files.

PURPOSE
-------
This script runs BEFORE the parser's heuristic layout-scoring phase.
It extracts everything the ELF symbol table and section headers can tell
us with certainty — no guessing required — and emits a structured report
that the parser (or a human) can use as ground-truth anchor points.

WHAT IT ESTABLISHES (symbol-table phase, zero heuristics)
----------------------------------------------------------
  1. ELF sanity     — magic, class, endian, machine, type, flags
  2. Section map    — name, type, offset, size for every section
  3. Model identity — model name(s), mesh count, descriptor-list offset,
                      and the .data offset of the __Model::: symbol itself
  4. Variations     — variation1/2/3 symbols and whether they all share
                      the same descriptor-list pointer (common for Alicia)
  5. BBOX           — raw min/max float values decoded from the BBOX symbol
  6. Materials      — every material name extracted from TAR RUNTIME_ALLOC
                      strings, plus shader names (SpecularMap, etc.)
  7. Toollib tag    — EAGL_TOOLLIB_VERSION string (e.g. VERSION-6-WII)
  8. Reloc summary  — total count, null vs non-null, non-aligned relocs
                      (the Layout C / off_pgx=9 fingerprint)
  9. Descriptor     — the raw bytes and the reloc map *inside* the first
                      descriptor block so the parser can skip the scan and
                      go straight to reading geometry
 10. Path to follow — a printed decision tree telling you which layout
                      the inspector predicts, and why, with confidence level

USAGE
-----
  python eagl_inspect.py <file.o> [<file.o> ...]
  python eagl_inspect.py *.o

OUTPUT
------
  Human-readable report to stdout.
  Machine-readable dict returned from inspect_o_file() for use by the parser.

INTEGRATION WITH parser.py
---------------------------
  The returned InspectResult carries:
    .descriptor_offsets  — list of .data offsets for each mesh descriptor
    .predicted_layout    — 'A', 'B', 'C', or None
    .confidence          — 'HIGH' | 'MEDIUM' | 'LOW'
    .anchor_relocs       — {desc_off: {rel_off: resolved}} for each descriptor
  Pass these into parse_o_file() to skip (or seed) the scoring scan.
"""

import struct
import re
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

@dataclass
class SectionInfo:
    idx:     int
    name:    str
    sh_type: int
    offset:  int
    size:    int
    entsize: int
    link:    int
    info:    int

    TYPE_NAMES = {0:"NULL",1:"PROGBITS",2:"SYMTAB",3:"STRTAB",
                  4:"RELA",9:"REL",11:"DYNSYM"}

    @property
    def type_name(self) -> str:
        return self.TYPE_NAMES.get(self.sh_type, f"0x{self.sh_type:x}")


@dataclass
class SymbolInfo:
    idx:   int
    name:  str
    value: int          # offset within .data (or 0 for undefined externals)
    size:  int
    bind:  int          # 0=LOCAL 1=GLOBAL 2=WEAK
    stype: int          # 0=NOTYPE 1=OBJECT 2=FUNC ...
    shndx: int          # section index; 0 = undefined external

    BIND_NAMES  = {0:"LOCAL",1:"GLOBAL",2:"WEAK"}
    TYPE_NAMES  = {0:"NOTYPE",1:"OBJECT",2:"FUNC",3:"SECTION",4:"FILE"}

    @property
    def bind_name(self) -> str:  return self.BIND_NAMES.get(self.bind, str(self.bind))
    @property
    def type_name(self) -> str:  return self.TYPE_NAMES.get(self.stype, str(self.stype))
    @property
    def is_defined(self) -> bool: return self.shndx != 0
    @property
    def is_data_local(self) -> bool: return self.shndx == 1  # section 1 = .data


@dataclass
class ModelEntry:
    symbol_name:    str           # e.g. "rc_controller_final"
    model_name:     str           # e.g. "rc_controller_final"
    data_offset:    int           # .data offset of the __Model::: symbol
    desc_list_ptr:  int | None    # resolved reloc at data_offset → first descriptor
    mesh_count:     int           # from raw[8:12] at the model symbol (BE u32)
    is_variation:   bool
    is_geoprimstate: bool = False  # True when __Model sym → GeoPrimState wrapper (world files)


@dataclass
class BboxEntry:
    symbol_name: str
    model_name:  str
    data_offset: int
    min_xyz:     tuple[float, float, float] | None
    max_xyz:     tuple[float, float, float] | None


@dataclass
class DescriptorInfo:
    data_offset:   int            # offset into .data of this descriptor
    raw_header:    bytes          # first 0x80 bytes
    magic_byte:    int            # raw_header[0]
    relocs_inside: dict[int, int] # {offset_within_descriptor: resolved_value}


@dataclass
class InspectResult:
    # ── identity ──────────────────────────────────────────────────────────
    file_path:      Path
    file_size:      int
    is_valid_elf:   bool
    elf_machine:    int           # 0x0008 = MIPS
    elf_flags:      int
    toollib_version: str          # "EAGL_TOOLLIB_VERSION-6-WII" or ""

    # ── sections ──────────────────────────────────────────────────────────
    sections:       list[SectionInfo] = field(default_factory=list)
    data_offset:    int = 0       # file offset where .data begins
    data_size:      int = 0       # size of .data in bytes

    # ── symbols ───────────────────────────────────────────────────────────
    all_symbols:    list[SymbolInfo] = field(default_factory=list)
    model_entries:  list[ModelEntry] = field(default_factory=list)
    bbox_entries:   list[BboxEntry]  = field(default_factory=list)
    material_names: list[str]        = field(default_factory=list)
    shader_names:   list[str]        = field(default_factory=list)

    # ── relocs ────────────────────────────────────────────────────────────
    reloc_map:          dict[int, int] = field(default_factory=dict)
    reloc_null_count:   int = 0
    reloc_nonzero_count: int = 0
    has_nonaligned_reloc: bool = False   # Layout C fingerprint (reloc at non-4-aligned offset)
    nonaligned_reloc_offsets: list[int] = field(default_factory=list)

    # ── descriptors ───────────────────────────────────────────────────────
    descriptors:        list[DescriptorInfo] = field(default_factory=list)

    # ── prediction ────────────────────────────────────────────────────────
    predicted_layout:   str | None = None   # 'A', 'B', 'C'
    confidence:         str = "LOW"         # 'HIGH', 'MEDIUM', 'LOW'
    prediction_reasons: list[str] = field(default_factory=list)

    # ── convenience (for parser integration) ──────────────────────────────
    @property
    def descriptor_offsets(self) -> list[int]:
        return [d.data_offset for d in self.descriptors]

    @property
    def anchor_relocs(self) -> dict[int, dict[int, int]]:
        """{ desc_data_offset: { rel_offset_within_desc: resolved_value } }"""
        return {d.data_offset: d.relocs_inside for d in self.descriptors}

    @property
    def primary_model_name(self) -> str:
        # Prefer the symbol without a .variationN suffix
        for m in self.model_entries:
            if not m.is_variation:
                return m.model_name
        return self.model_entries[0].model_name if self.model_entries else ""

    @property
    def primary_material_name(self) -> str:
        return self.material_names[0] if self.material_names else ""


# ---------------------------------------------------------------------------
# Low-level ELF helpers
# ---------------------------------------------------------------------------

def _u16le(data: bytes, off: int) -> int: return struct.unpack_from("<H", data, off)[0]
def _u32le(data: bytes, off: int) -> int: return struct.unpack_from("<I", data, off)[0]
def _u32be(data: bytes, off: int) -> int: return struct.unpack_from(">I", data, off)[0]
def _f32be(data: bytes, off: int) -> float: return struct.unpack_from(">f", data, off)[0]

def _read_cstr(data: bytes, off: int) -> str:
    end = off
    while data[end]:
        end += 1
    return data[off:end].decode("ascii", errors="replace")


# ---------------------------------------------------------------------------
# Section parsing
# ---------------------------------------------------------------------------

def _parse_sections(data: bytes) -> list[SectionInfo]:
    e_shoff     = _u32le(data, 32)
    e_shentsize = _u16le(data, 46)
    e_shnum     = _u16le(data, 48)
    e_shstrndx  = _u16le(data, 50)

    raw = []
    for i in range(e_shnum):
        o  = e_shoff + i * e_shentsize
        sh = struct.unpack_from("<IIIIIIIIII", data, o)
        raw.append(SectionInfo(
            idx=i, name="", sh_type=sh[1],
            offset=sh[4], size=sh[5], entsize=sh[9],
            link=sh[6], info=sh[7],
        ))

    # Resolve names from .shstrtab
    shstr = raw[e_shstrndx]
    sb    = shstr.offset
    for s in raw:
        end = sb + s.name_idx_hack(data, e_shoff, e_shentsize)
        # Re-read name_idx directly (dataclass has no name_idx field by design)
        ni  = struct.unpack_from("<I", data, e_shoff + s.idx * e_shentsize)[0]
        end = sb + ni
        while data[end]:
            end += 1
        s.name = data[sb + ni : end].decode("ascii", errors="replace")

    return raw


# The above is a bit awkward because SectionInfo doesn't store name_idx.
# Let's redo cleanly without that method.

def _parse_sections_clean(data: bytes) -> list[SectionInfo]:
    e_shoff     = _u32le(data, 32)
    e_shentsize = _u16le(data, 46)
    e_shnum     = _u16le(data, 48)
    e_shstrndx  = _u16le(data, 50)

    raws = []
    for i in range(e_shnum):
        o  = e_shoff + i * e_shentsize
        sh = struct.unpack_from("<IIIIIIIIII", data, o)
        raws.append((sh[0], sh[1], sh[4], sh[5], sh[9], sh[6], sh[7]))  # name_idx,type,off,size,entsz,link,info

    shstr_off = raws[e_shstrndx][2]
    sections  = []
    for i, (ni, stype, soff, ssize, entsz, link, info) in enumerate(raws):
        end = shstr_off + ni
        while data[end]:
            end += 1
        name = data[shstr_off + ni : end].decode("ascii", errors="replace")
        sections.append(SectionInfo(idx=i, name=name, sh_type=stype,
                                    offset=soff, size=ssize, entsize=entsz,
                                    link=link, info=info))
    return sections


# ---------------------------------------------------------------------------
# Symbol table parsing
# ---------------------------------------------------------------------------

def _parse_symbols(data: bytes, sym_sec: SectionInfo,
                   str_sec: SectionInfo) -> list[SymbolInfo]:
    esz  = sym_sec.entsize or 16
    stb  = str_sec.offset
    syms = []
    for i in range(sym_sec.size // esz):
        o = sym_sec.offset + i * esz
        st_name, st_value, st_size = struct.unpack_from("<III", data, o)
        st_info, _, st_shndx       = struct.unpack_from("<BBH", data, o + 12)
        name = _read_cstr(data, stb + st_name)
        syms.append(SymbolInfo(
            idx=i, name=name, value=st_value, size=st_size,
            bind=st_info >> 4, stype=st_info & 0xF, shndx=st_shndx,
        ))
    return syms


# ---------------------------------------------------------------------------
# Reloc table parsing
# ---------------------------------------------------------------------------

def _parse_relocs(data: bytes, rel_sec: SectionInfo,
                  data_start: int) -> dict[int, int]:
    relocs: dict[int, int] = {}
    for i in range(rel_sec.size // 8):
        o = rel_sec.offset + i * 8
        r_off, r_info = struct.unpack_from("<II", data, o)
        if (r_info & 0xFF) == 2:   # R_MIPS_32
            resolved = _u32le(data, data_start + r_off)
            relocs[r_off] = resolved
    return relocs


# ---------------------------------------------------------------------------
# Name extraction helpers
# ---------------------------------------------------------------------------

_MODEL_RE    = re.compile(r"__Model:::([^:]+)")
_MAT_RE      = re.compile(r"1=([^,;]+)")
_TOOLLIB_RE  = re.compile(r"EAGL_TOOLLIB_VERSION:::([^\s]+)")
_SHADER_RE   = re.compile(r"TAR\*:::([^\s]+)")

def _extract_model_name(sym_name: str) -> str:
    m = _MODEL_RE.search(sym_name)
    return m.group(1) if m else sym_name

def _extract_material_name(sym_name: str) -> str | None:
    """Pull the first '1=<name>' value out of a TAR RUNTIME_ALLOC string."""
    if "RUNTIME_ALLOC" not in sym_name:
        return None
    m = _MAT_RE.search(sym_name)
    return m.group(1) if m else None

def _extract_bbox(data: bytes, data_start: int, offset: int
                  ) -> tuple[tuple[float,float,float], tuple[float,float,float]] | None:
    """
    The BBOX symbol's .data offset points to:
      [0x00] min_x  (BE f32)
      [0x04] min_y
      [0x08] min_z
      [0x0c] max_x
      [0x10] max_y
      [0x14] max_z
      [0x18..] padding / flags
    """
    base = data_start + offset
    try:
        mn = (_f32be(data, base + 0x00), _f32be(data, base + 0x04), _f32be(data, base + 0x08))
        mx = (_f32be(data, base + 0x0c), _f32be(data, base + 0x10), _f32be(data, base + 0x14))
        # Sanity: infinity or NaN → bad read
        import math
        if any(not math.isfinite(v) for v in mn + mx):
            return None
        return mn, mx
    except Exception:
        return None


# ---------------------------------------------------------------------------
# Model-symbol → descriptor resolution
# ---------------------------------------------------------------------------

def _resolve_model_symbol(data: bytes, data_start: int, data_size: int,
                           sym: SymbolInfo,
                           relocs: dict[int, int]) -> ModelEntry:
    """
    __Model::: symbol layout at sym.value (.data offset):
      [0x00] reloc → ptr to first descriptor (resolved by relocs[sym.value])
      [0x04] 0x00000000  (padding / null terminator)
      [0x08] u32 BE  — mesh count inside this variation
      [0x0c] u32 BE  — always 0x00000001?
      [0x10] f32 BE  — 1.0 (scale?)
      ...
    """
    val = sym.value
    desc_list_ptr = relocs.get(val)  # resolved pointer to first descriptor block

    # Mesh count: BE u32 at +0x08 relative to the model symbol offset
    mesh_count = 0
    base = data_start + val
    if base + 0x10 <= len(data):
        mesh_count = _u32be(data, base + 0x08)

    model_name  = _extract_model_name(sym.name)
    is_variation = bool(re.search(r"\.variation\d+$", model_name))

    # Detect GeoPrimState wrapper: the desc_list_ptr targets a render-state
    # node rather than a geometry descriptor array.  The node starts with a
    # reloc pointing to the ASCII string "GeoPrimState::State".  World/static
    # files use this two-level hierarchy; when detected, the inspector cannot
    # resolve individual descriptors from the symbol table alone.
    is_geoprimstate = False
    if desc_list_ptr is not None and 0 < desc_list_ptr < data_size - 4:
        # The first word of a GeoPrimState node is a reloc to its type-string.
        string_ptr = relocs.get(desc_list_ptr)
        if string_ptr is not None and 0 < string_ptr < data_size - 20:
            candidate = data[data_start + string_ptr : data_start + string_ptr + 20]
            if candidate.startswith(b"GeoPrimState"):
                is_geoprimstate = True

    return ModelEntry(
        symbol_name   = sym.name,
        model_name    = model_name,
        data_offset   = val,
        desc_list_ptr = desc_list_ptr,
        mesh_count    = mesh_count,
        is_variation  = is_variation,
        is_geoprimstate = is_geoprimstate,
    )


# ---------------------------------------------------------------------------
# Descriptor block extraction
# ---------------------------------------------------------------------------

def _extract_descriptor(data: bytes, data_start: int, data_size: int,
                         desc_off: int,
                         relocs: dict[int, int]) -> DescriptorInfo:
    """
    Given the .data offset of a descriptor block, capture:
      - First 0x80 raw bytes
      - All relocs whose r_offset falls within [desc_off, desc_off+0x80)
    """
    base  = data_start + desc_off
    end   = min(base + 0x80, len(data))
    raw   = data[base:end]

    inside = {
        roff - desc_off: rval
        for roff, rval in relocs.items()
        if desc_off <= roff < desc_off + 0x80
    }

    return DescriptorInfo(
        data_offset   = desc_off,
        raw_header    = raw,
        magic_byte    = raw[0] if raw else 0xFF,
        relocs_inside = inside,
    )


# ---------------------------------------------------------------------------
# Layout prediction (symbol/reloc-based — no scanning)
# ---------------------------------------------------------------------------

def _predict_layout(result: "InspectResult") -> tuple[str | None, str, list[str]]:
    """
    Use the information gathered from symbols and relocs to predict the
    descriptor layout without touching the heuristic scanner.

    Returns (layout_name, confidence, [reasons]).
    """
    reasons: list[str] = []
    layout:  str | None = None
    confidence = "LOW"

    if not result.descriptors:
        reasons.append("No descriptor blocks resolved — cannot predict layout.")
        return None, "LOW", reasons

    # If any model entry is a GeoPrimState wrapper, the __Model symbol doesn't
    # chain directly to geometry descriptors.  The inspector's candidate list
    # is unreliable for these world/static files; tell the parser to run the
    # full heuristic scan instead of the inspector fast-path.
    if any(getattr(m, "is_geoprimstate", False) for m in result.model_entries):
        reasons.append(
            "Model symbol points to a GeoPrimState render-state wrapper "
            "(world/static-scene file). Descriptor addresses cannot be resolved "
            "reliably from the symbol table alone — heuristic scan required."
        )
        return None, "LOW", reasons

    # Rule 0 — Self-pointer at desc+0x2c (offset 44) is exclusive to Layout D.
    # Check before magic-byte rules since Layout D shares magic 0x02 with Layout C.
    for d in result.descriptors:
        if 44 in d.relocs_inside and d.relocs_inside[44] == d.data_offset:
            reasons.append(
                f"Self-pointer at desc+0x2c (relocs[0x{d.data_offset+44:06x}] == 0x{d.data_offset:06x}) "
                f"→ Layout D (HWSkin skinned character)."
            )
            return "D", "HIGH", reasons

    # Rule 1 — Non-aligned reloc at desc+0x09 is the exclusive Layout C marker
    if result.has_nonaligned_reloc:
        for off in result.nonaligned_reloc_offsets:
            # Check if it is at (desc_off + 9) for any known descriptor
            for d in result.descriptors:
                if off == d.data_offset + 9:
                    reasons.append(f"Non-aligned reloc at desc+0x09 (abs 0x{off:06x}) → Layout C (track/terrain).")
                    return "C", "HIGH", reasons

    # Rule 2 — Magic byte from the first descriptor's raw header
    for d in result.descriptors:
        magic = d.magic_byte
        if magic == 0x07:
            layout = "A"
            reasons.append(f"Magic byte 0x07 at desc 0x{d.data_offset:06x} → Layout A (complex/multi-mesh).")
            confidence = "HIGH"
            break
        elif magic == 0x04:
            layout = "B"
            reasons.append(f"Magic byte 0x04 at desc 0x{d.data_offset:06x} → Layout B (simple/single-mesh).")
            confidence = "HIGH"
            break
        elif magic == 0x02:
            layout = "C"
            reasons.append(f"Magic byte 0x02 at desc 0x{d.data_offset:06x} → Layout C (track/terrain).")
            confidence = "HIGH"
            break
        else:
            reasons.append(f"Unrecognised magic byte 0x{magic:02x} at desc 0x{d.data_offset:06x}.")

    if layout is None:
        confidence = "LOW"
        reasons.append("Magic byte did not match any known layout.")

    # Rule 3 — Corroborate with reloc structure inside first descriptor
    if result.descriptors:
        d = result.descriptors[0]
        ri = d.relocs_inside

        # Layout A: relocs at +0x34, +0x3c, +0x44 (off_ppos/pnorm/puv = 52,60,68)
        # Layout B: relocs at +0x38, +0x40, +0x48 (off_ppos/pnorm/puv = 56,64,72)
        # Layout C: relocs at +0x34, +0x3c, +0x44 + non-aligned +0x09

        has_34 = 52 in ri and ri[52] > 0   # +0x34
        has_38 = 56 in ri and ri[56] > 0   # +0x38
        has_3c = 60 in ri and ri[60] > 0   # +0x3c
        has_40 = 64 in ri and ri[64] > 0   # +0x40
        has_44 = 68 in ri and ri[68] > 0   # +0x44
        has_48 = 72 in ri and ri[72] > 0   # +0x48

        if has_34 and has_3c and has_44:
            if layout in ("A", "C"):
                confidence = "HIGH"
                reasons.append("Relocs at desc+0x34/0x3c/0x44 confirm pos/norm/uv pointers (Layout A or C).")
            elif layout == "B":
                confidence = "MEDIUM"
                reasons.append("WARNING: relocs at +0x34/0x3c/0x44 suggest A/C offset layout, but magic says B.")

        if has_38 and has_40 and has_48:
            if layout == "B":
                confidence = "HIGH"
                reasons.append("Relocs at desc+0x38/0x40/0x48 confirm pos/norm/uv pointers (Layout B).")
            elif layout in ("A", "C"):
                confidence = "MEDIUM"
                reasons.append("WARNING: relocs at +0x38/0x40/0x48 suggest B offset layout, but magic says A/C.")

        # Ordering check: ptr_pos < ptr_norm < ptr_uv
        for label, p_off, n_off, u_off in [
            ("A/C", 52, 60, 68),
            ("B",   56, 64, 72),
        ]:
            p, n, u = ri.get(p_off, 0), ri.get(n_off, 0), ri.get(u_off, 0)
            if p > 0 and n > 0 and u > 0 and p < n < u:
                reasons.append(f"Reloc order check passed for Layout {label}: "
                                f"pos(0x{p:06x}) < norm(0x{n:06x}) < uv(0x{u:06x}).")

    # Rule 4 — Mesh count cross-check
    for m in result.model_entries:
        if not m.is_variation and m.mesh_count > 0:
            reasons.append(f"Model symbol reports {m.mesh_count} mesh(es).")
            if m.mesh_count == 1 and layout == "A":
                reasons.append("Single-mesh count with Layout A magic is unusual — verify.")
            break

    return layout, confidence, reasons


# ---------------------------------------------------------------------------
# Main inspector
# ---------------------------------------------------------------------------

def inspect_o_file(path: str | Path) -> InspectResult:
    """
    Inspect an EAGL Wii .o file using only symbol table and section header
    information.  Returns an InspectResult.
    """
    path = Path(path)
    data = path.read_bytes()

    result = InspectResult(
        file_path = path,
        file_size = len(data),
        is_valid_elf   = False,
        elf_machine    = 0,
        elf_flags      = 0,
        toollib_version = "",
    )

    # ── ELF magic ─────────────────────────────────────────────────────────
    if data[:4] != b"\x7fELF":
        return result
    result.is_valid_elf = True
    result.elf_machine  = _u16le(data, 18)
    result.elf_flags    = _u32le(data, 36)

    # ── Sections ──────────────────────────────────────────────────────────
    sections = _parse_sections_clean(data)
    result.sections = sections

    def _sec(name: str):
        return next((s for s in sections if s.name == name), None)

    data_sec = _sec(".data")
    sym_sec  = _sec(".symtab")
    str_sec  = _sec(".strtab")
    rel_sec  = _sec(".rel.data")

    if not data_sec:
        return result

    data_start = data_sec.offset
    data_size  = data_sec.size
    result.data_offset = data_start
    result.data_size   = data_size

    # ── Symbols ───────────────────────────────────────────────────────────
    if sym_sec and str_sec:
        syms = _parse_symbols(data, sym_sec, str_sec)
        result.all_symbols = syms

        for sym in syms:
            n = sym.name

            # Model entry
            if n.startswith("__Model:::"):
                pass  # handled below after reloc map is built

            # Toollib version
            m = _TOOLLIB_RE.search(n)
            if m:
                result.toollib_version = m.group(1)

            # Shader names (TAR* symbols)
            if "TAR*:::" in n:
                m2 = _SHADER_RE.search(n)
                if m2:
                    result.shader_names.append(m2.group(1))

            # Material names from RUNTIME_ALLOC TAR strings
            mat = _extract_material_name(n)
            if mat and mat not in result.material_names:
                result.material_names.append(mat)

    # ── Relocation map ────────────────────────────────────────────────────
    if rel_sec:
        relocs = _parse_relocs(data, rel_sec, data_start)
        result.reloc_map = relocs

        null_cnt = sum(1 for v in relocs.values() if v == 0)
        result.reloc_null_count    = null_cnt
        result.reloc_nonzero_count = len(relocs) - null_cnt

        # Non-aligned relocs: r_offset not divisible by 4 → Layout C marker
        nonaligned = [off for off in relocs if off % 4 != 0]
        result.has_nonaligned_reloc          = bool(nonaligned)
        result.nonaligned_reloc_offsets      = sorted(nonaligned)
    else:
        relocs = {}

    # ── Model / BBOX symbol resolution (needs reloc map) ──────────────────
    if sym_sec and str_sec:
        for sym in result.all_symbols:
            n = sym.name
            if n.startswith("__Model:::"):
                entry = _resolve_model_symbol(data, data_start, data_size, sym, relocs)
                result.model_entries.append(entry)

            elif n.startswith("__BBOX:::"):
                m_name = n.split(":::")[1] if ":::" in n else n
                bbox = _extract_bbox(data, data_start, sym.value)
                mn, mx = (bbox if bbox else (None, None))
                result.bbox_entries.append(BboxEntry(
                    symbol_name = n,
                    model_name  = m_name,
                    data_offset = sym.value,
                    min_xyz     = mn,
                    max_xyz     = mx,
                ))

    # ── Descriptor extraction ─────────────────────────────────────────────
    # Instead of guessing fixed offsets near the model symbol, find every 
    # unique destination address targeted by the ELF data relocations.
    # If a relocation target points to a valid descriptor magic byte, extract it.
    valid_magics = {0x02, 0x04, 0x07}
    candidate_offsets = set()

    # 1. Incoming target relocations
    for target_off in relocs.values():
        if 0 <= target_off < data_size:
            candidate_offsets.add(target_off)

    # 2. Master Model pointer layout definitions
    for entry in result.model_entries:
        if entry.desc_list_ptr is not None and 0 <= entry.desc_list_ptr < data_size:
            candidate_offsets.add(entry.desc_list_ptr)

    # 3. Proximity boundaries of outgoing properties
    for r_off in relocs.keys():
        start_bound = max(0, (r_off - 0x80) // 4 * 4)
        for possible_start in range(start_bound, r_off + 4, 4):
            if possible_start < data_size:
                candidate_offsets.add(possible_start)

    # 4. Aligned 4-byte structural scan
    for off in range(0, max(0, data_size - 4), 4):
        base = data_start + off
        if base < len(data) and data[base] in valid_magics:
            candidate_offsets.add(off)

    # Pre-build lookup mappings for lightning-fast verification
    reloc_offsets = set(relocs.keys())
    reloc_targets = set(relocs.values())
    model_ptrs = {m.desc_list_ptr for m in result.model_entries if m.desc_list_ptr is not None}

    seen_desc_ptrs = set()
    for target_off in sorted(candidate_offsets):
        base = data_start + target_off
        if base >= len(data):
            continue
            
        magic = data[base]
        if magic in valid_magics and target_off not in seen_desc_ptrs:
            # Confirm structural viability via internal link integrity check
            is_valid = (
                target_off in reloc_targets or
                target_off in model_ptrs or
                any((target_off + i) in reloc_offsets for i in range(0, 0x60, 4))
            )
            
            if is_valid:
                seen_desc_ptrs.add(target_off)
                d = _extract_descriptor(data, data_start, data_size, target_off, relocs)
                result.descriptors.append(d)

    if not result.descriptors:
        for entry in result.model_entries:
            if entry.desc_list_ptr and entry.desc_list_ptr not in seen_desc_ptrs:
                d = _extract_descriptor(data, data_start, data_size, entry.desc_list_ptr, relocs)
                result.descriptors.append(d)

    def _pos_ptr(d: DescriptorInfo) -> int:
        return d.relocs_inside.get(52) or d.relocs_inside.get(56) or d.data_offset

    result.descriptors.sort(key=_pos_ptr)

    layout, conf, reasons = _predict_layout(result)
    result.predicted_layout    = layout
    result.confidence          = conf
    result.prediction_reasons  = reasons

    return result


# ---------------------------------------------------------------------------
# Human-readable report
# ---------------------------------------------------------------------------

_CONF_COLOUR = {"HIGH": "✅", "MEDIUM": "⚠️ ", "LOW": "❌"}

def _fmt_xyz(xyz) -> str:
    if xyz is None:
        return "???"
    return f"({xyz[0]:+.4f}, {xyz[1]:+.4f}, {xyz[2]:+.4f})"


def print_report(r: InspectResult, verbose: bool = False) -> None:
    W = 72
    print("=" * W)
    print(f"  EAGL Inspector  ·  {r.file_path.name}  ({r.file_size:,} bytes)")
    print("=" * W)

    if not r.is_valid_elf:
        print("  ✗  Not a valid ELF file.\n")
        return

    print(f"\n  ELF  machine=0x{r.elf_machine:04x}  flags=0x{r.elf_flags:08x}  "
          f"toollib={r.toollib_version or '(unknown)'}")

    # ── Sections ──────────────────────────────────────────────────────────
    print(f"\n  ── SECTIONS ({len(r.sections)}) " + "─" * 42)
    print(f"  {'#':>3}  {'Name':<20} {'Type':<10} {'Offset':>8}  {'Size':>8}  {'EntSz':>5}")
    for s in r.sections:
        print(f"  {s.idx:>3}  {s.name:<20} {s.type_name:<10} 0x{s.offset:06x}   0x{s.size:06x}  {s.entsize:>5}")

    # ── Model symbols ─────────────────────────────────────────────────────
    print(f"\n  ── MODEL SYMBOLS ({len(r.model_entries)}) " + "─" * 37)
    if not r.model_entries:
        print("  (none found)")
    for m in r.model_entries:
        var_tag = " [variation]" if m.is_variation else ""
        print(f"  {m.model_name}{var_tag}")
        print(f"      sym.value   = 0x{m.data_offset:06x}  (model list @ .data+{m.data_offset})")
        if m.desc_list_ptr is not None:
            print(f"      desc_list → 0x{m.desc_list_ptr:06x}  (first descriptor .data offset)")
        else:
            print(f"      desc_list → (no reloc — external or missing)")
        print(f"      mesh_count  = {m.mesh_count}")

    # ── BBOX ──────────────────────────────────────────────────────────────
    print(f"\n  ── BBOX ({len(r.bbox_entries)}) " + "─" * 47)
    for b in r.bbox_entries:
        print(f"  {b.model_name}")
        print(f"      .data+0x{b.data_offset:06x}  min={_fmt_xyz(b.min_xyz)}  max={_fmt_xyz(b.max_xyz)}")

    # ── Materials ─────────────────────────────────────────────────────────
    print(f"\n  ── MATERIALS ({len(r.material_names)}) " + "─" * 42)
    for mat in r.material_names:
        print(f"  · {mat}")
    if r.shader_names:
        print(f"  shaders: {', '.join(r.shader_names)}")

    # ── Relocations ───────────────────────────────────────────────────────
    total_relocs = len(r.reloc_map)
    print(f"\n  ── RELOCATIONS " + "─" * 51)
    print(f"  total={total_relocs}  non-zero={r.reloc_nonzero_count}  null={r.reloc_null_count}")
    if r.has_nonaligned_reloc:
        offsets_str = ", ".join(f"0x{o:06x}" for o in r.nonaligned_reloc_offsets[:8])
        print(f"  ⚠️  NON-ALIGNED relocs detected at: {offsets_str}")
        print(f"       → This is the Layout C (track/terrain) fingerprint.")
    else:
        print(f"  All relocs are 4-byte aligned  → no Layout C fingerprint.")

    # ── Descriptors ───────────────────────────────────────────────────────
    print(f"\n  ── DESCRIPTORS RESOLVED ({len(r.descriptors)}) " + "─" * 33)
    if not r.descriptors:
        print("  (could not resolve any descriptor blocks from symbol table)")
    for i, d in enumerate(r.descriptors):
        print(f"\n  Descriptor[{i}]  .data+0x{d.data_offset:06x}  magic=0x{d.magic_byte:02x}")

        if verbose:
            hex_rows = [d.raw_header[j:j+16] for j in range(0, min(len(d.raw_header), 0x50), 16)]
            for row_off, row in enumerate(hex_rows):
                print(f"    +{row_off*16:02x}: {' '.join(f'{b:02x}' for b in row)}")

        if d.relocs_inside:
            print(f"    relocs inside (offset_within_desc → resolved_value):")
            for roff in sorted(d.relocs_inside):
                rval = d.relocs_inside[roff]
                null_tag = "  [NULL ptr]" if rval == 0 else ""
                print(f"      +0x{roff:02x} (abs 0x{d.data_offset+roff:06x}) → 0x{rval:06x}{null_tag}")

    # ── Layout prediction ─────────────────────────────────────────────────
    print(f"\n  ── LAYOUT PREDICTION " + "─" * 45)
    icon = _CONF_COLOUR.get(r.confidence, "?")
    lay  = r.predicted_layout or "UNKNOWN"
    print(f"  {icon} Layout {lay}  (confidence: {r.confidence})")
    for reason in r.prediction_reasons:
        print(f"    · {reason}")

    # ── Path to follow ────────────────────────────────────────────────────
    print(f"\n  ── PATH TO FOLLOW " + "─" * 48)
    _print_path(r)

    print()


def _print_path(r: InspectResult) -> None:
    """
    Print the decision tree / recommended parse path based on what the
    inspector found.  This is the ruleset that replaces blind scanning.
    """
    if not r.is_valid_elf:
        print("  STOP: file is not ELF.")
        return

    if not r.model_entries:
        print("  WARN: no __Model::: symbol found.")
        print("        → Fall back to heuristic scan (no symbol-table anchor).")
        return

    print("  STEP 1  ELF OK — .data, .symtab, .strtab, .rel.data all present.")

    primary = next((m for m in r.model_entries if not m.is_variation), r.model_entries[0])
    print(f"  STEP 2  Model name  : '{primary.model_name}'")
    print(f"          Mesh count  : {primary.mesh_count}  (from symbol raw[+0x08])")
    if r.material_names:
        print(f"          Material(s) : {', '.join(r.material_names)}")
    if r.bbox_entries:
        b = r.bbox_entries[0]
        print(f"          BBOX        : min={_fmt_xyz(b.min_xyz)}  max={_fmt_xyz(b.max_xyz)}")

    if primary.desc_list_ptr is not None:
        print(f"  STEP 3  Descriptor list starts at .data+0x{primary.desc_list_ptr:06x}")
        print(f"          → Skip scanning; read descriptor directly at this offset.")
    else:
        print(f"  STEP 3  No reloc at model symbol offset — external .data, must scan.")

    if r.predicted_layout == "D":
        print(f"  STEP 4  Self-ptr at desc+0x2c → Layout D (HWSkin skinned character).")
        print(f"          pos: s16×3 stride 6 (÷32767). norm: s16×3 stride 6 (÷32767).")
        print(f"          uv: f32×2 stride 8. Bone-weight table at desc+0x44.")
        print(f"          GX ptr: scan non-aligned reloc in desc+0x00..+0x27.")
        print(f"          Counts from descriptor fields at off_cpos=88, off_cnorm=96, off_cuv=104.")
    elif r.has_nonaligned_reloc:
        print(f"  STEP 4  Non-aligned reloc(s) found → select Layout C parser path.")
        print(f"          norm_stride=4, counts derived from ptr gaps, GX from off_pgx=9.")
    elif r.predicted_layout == "A":
        print(f"  STEP 4  Magic 0x07 → Layout A.")
        print(f"          norm_stride=6, counts from descriptor fields, GX after UV array.")
        print(f"          attr list at desc+0x08.")
    elif r.predicted_layout == "B":
        print(f"  STEP 4  Magic 0x04 → Layout B.")
        print(f"          norm_stride=6, counts from descriptor fields, GX after UV array.")
        print(f"          attr list at desc+0x0c  (note: +4 vs Layout A).")
    elif r.predicted_layout == "C":
        print(f"  STEP 4  Magic 0x02 → Layout C.")
        print(f"          norm_stride=4, counts derived from ptr gaps, GX from off_pgx=9.")
    else:
        print(f"  STEP 4  Layout unknown — run heuristic scorer as fallback.")

    if r.confidence == "HIGH":
        print(f"  STEP 5  Confidence HIGH — proceed directly with selected layout.")
        print(f"          Heuristic scoring is not required for this file.")
    elif r.confidence == "MEDIUM":
        print(f"  STEP 5  Confidence MEDIUM — recommend running scorer as sanity check.")
    else:
        print(f"  STEP 5  Confidence LOW — run full heuristic scan; treat prediction as hint only.")

    # Variations note
    variations = [m for m in r.model_entries if m.is_variation]
    if variations:
        shared = all(m.desc_list_ptr == primary.desc_list_ptr for m in variations)
        print(f"\n  NOTE    {len(variations)} variation symbol(s) found.")
        if shared:
            print(f"          All share the same descriptor list → single parse pass covers all.")
        else:
            print(f"          Variations use different descriptor lists → may need separate passes.")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> None:
    args = sys.argv[1:]
    if not args:
        print("Usage: python eagl_inspect.py <file.o> [<file.o> ...]")
        sys.exit(1)

    verbose = "--verbose" in args or "-v" in args
    files   = [a for a in args if not a.startswith("-")]

    for fpath in files:
        p = Path(fpath)
        if not p.exists():
            print(f"File not found: {fpath}")
            continue
        result = inspect_o_file(p)
        print_report(result, verbose=verbose)


if __name__ == "__main__":
    main()