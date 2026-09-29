"""
eagl_parser_v10.py
-------------------
Multi-mesh parser for EA Sports EAGL Wii .o model files.

Changes over v9:
  - Fixed _scan_gx_strips silently accepting out-of-bounds normal indices.
    The stride-probe scorer (_validate_stride_gx) already bounds-checked
    pos/norm/uv indices when picking a candidate stride, but the function
    that actually extracted the final triangle data only bounds-checked
    pos and uv — max_norm was computed and then discarded before reaching
    _scan_gx_strips. Once a stride guess was even slightly off, garbage
    bytes decoded as the normal index were silently accepted into the
    final mesh instead of being rejected. _scan_gx_strips now takes a
    max_norm parameter and rejects any strip whose normal index exceeds
    it, the same way pos/uv already do.
  - Extended _validate_stride_gx's candidate search from "flip uv_sz only"
    to a full combinatorial probe over pos_sz/norm_sz/uv_sz. Small declared
    arrays (common in low-LOD B2 terrain meshes with very few unique
    normals) make the attr table's INDEX8/INDEX16 format byte ambiguous
    for pos and norm, not just uv as previously assumed. Each width
    combination's stride is recomputed and re-scored; the best-scoring
    combination wins.
  - Added a 0-score fallback in _validate_stride_gx: if every candidate
    (including the original attr-table guess) scores zero on the probe,
    the function now returns the original guess unchanged instead of
    picking an arbitrary tied "winner." A genuine 0-score result means no
    tried width is correct, and that should surface as an empty mesh for
    investigation rather than confidently-wrong geometry.

  Net effect on world-low-all.o: 65,132 -> 98,402 triangles, with zero
  remaining out-of-bounds normal-index references across all 631 meshes
  (previously ~50% of B2 meshes had normal-index utilization over 100%,
  i.e. referenced indices past the end of their own normals array).

"""

import struct
import re
import math
import json as _json
import logging
import warnings
import functools
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

_log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------

@dataclass
class MeshChunk:
    index:       int
    desc_offset: int
    stride:      int    # bytes-per-vertex in GX stream: 3 (INDEX8) or 6 (INDEX16)
    layout:      str    # 'A' (byte[0]=0x07) or 'B' (byte[0]=0x04)
    positions:   list[tuple[float, float, float]]
    normals:     list[tuple[float, float, float]]
    uvs:         list[tuple[float, float]]
    faces:       list[tuple[tuple[int,int,int], tuple[int,int,int], tuple[int,int,int]]]
    warnings:    list[str] = field(default_factory=list)
    # Layout D / D2 only: per-vertex (w1, w2) bone-weight pairs from the
    # weight table at desc+0x44. Empty for layouts A/B/C.
    bone_weights: list[tuple[float, float, float, int, int, int]] = field(default_factory=list)
    # Layout D / D2 only: maps each face vertex key (pos_idx, norm_idx,
    # uv_idx) to a weight_slot index into bone_weights. Empty for A/B/C.
    vertex_joints: dict = field(default_factory=dict)

    @property
    def ok(self) -> bool:
        return len(self.faces) > 0

    def summary(self) -> str:
        if self.layout in ("D", "D2"):
            mode = "INDEX8+MTX" if self.stride <= 5 else "INDEX16+MTX"
        else:
            mode = "INDEX16" if self.stride > 3 else "INDEX8"
        s = (f"Mesh[{self.index}] @0x{self.desc_offset:04x}  {mode}  layout={self.layout}  "
             f"pos={len(self.positions)} norm={len(self.normals)} "
             f"uv={len(self.uvs)} tris={len(self.faces)}")
        if self.bone_weights:
            s += f"  weights={len(self.bone_weights)}"
        if self.warnings:
            s += f"  WARN: {'; '.join(self.warnings)}"
        return s


@dataclass
class ParseResult:
    model_name:    str
    material_name: str
    meshes:        list[MeshChunk]
    log:           list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return any(m.ok for m in self.meshes)

    @property
    def total_faces(self) -> int:
        return sum(len(m.faces) for m in self.meshes)

    def summary(self) -> str:
        lines = self.log[:]
        lines.append(f"Meshes found: {len(self.meshes)}")
        for m in self.meshes:
            lines.append("  " + m.summary())
        lines.append(f"Total triangles: {self.total_faces}")
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# ELF helpers
# ---------------------------------------------------------------------------

def _read_sections(data: bytes) -> tuple[list[dict], int]:
    e_shoff     = struct.unpack_from("<I", data, 32)[0]
    e_shentsize = struct.unpack_from("<H", data, 46)[0]
    e_shnum     = struct.unpack_from("<H", data, 48)[0]
    e_shstrndx  = struct.unpack_from("<H", data, 50)[0]

    sections = []
    for i in range(e_shnum):
        o = e_shoff + i * e_shentsize
        sh = struct.unpack_from("<IIIIIIIIII", data, o)
        sections.append({
            "name_idx": sh[0], "type": sh[1],
            "offset": sh[4], "size": sh[5],
            "link": sh[6], "info": sh[7], "entsize": sh[9],
        })

    shstrtab = sections[e_shstrndx]
    base = shstrtab["offset"]
    for s in sections:
        end = data.index(b'\x00', base + s["name_idx"])
        s["name"] = data[base + s["name_idx"]:end].decode("ascii", errors="replace")

    return sections, sections[1]["offset"]


def _read_cstr(data: bytes, offset: int) -> str:
    end = data.index(b'\x00', offset)
    return data[offset:end].decode("ascii", errors="replace")


def _extract_names(data: bytes, sections: list[dict]) -> tuple[str, str, int]:
    sym_sec = next((s for s in sections if s["name"] == ".symtab"), None)
    str_sec = next((s for s in sections if s["name"] == ".strtab"), None)
    if not sym_sec or not str_sec:
        return "model", "material", 0

    model_name    = ""
    materials: list[str] = []
    str_base      = str_sec["offset"]
    entry_size    = sym_sec["entsize"] or 16

    for i in range(sym_sec["size"] // entry_size):
        o = sym_sec["offset"] + i * entry_size
        name_idx = struct.unpack_from("<I", data, o)[0]
        name = _read_cstr(data, str_base + name_idx)
        if ":::" in name and name.startswith("__Model") and not model_name:
            parts = name.split(":::")
            if len(parts) >= 2:
                model_name = parts[1]
        if "TAR:::RUNTIME_ALLOC" in name:
            m = re.search(r"1=([^,;]+)", name)
            if m:
                mat = m.group(1)
                if mat not in materials:
                    materials.append(mat)

    material_name = materials[0] if materials else "material"
    return model_name or "model", material_name, len(materials)


def _extract_bbox(data: bytes, sections: list[dict],
                  data_start: int) -> "tuple | None":
    """
    Return (min_xyz, max_xyz) from the __BBOX::: symbol, or None.
    Used to derive the s16→float position scale for Layout D / D2 meshes.
    """
    sym_sec = next((s for s in sections if s["name"] == ".symtab"), None)
    str_sec = next((s for s in sections if s["name"] == ".strtab"), None)
    if not sym_sec or not str_sec:
        return None
    str_base   = str_sec["offset"]
    entry_size = sym_sec["entsize"] or 16
    for i in range(sym_sec["size"] // entry_size):
        o = sym_sec["offset"] + i * entry_size
        ni, val = struct.unpack_from("<II", data, o)
        name = _read_cstr(data, str_base + ni)
        if name.startswith("__BBOX:::"):
            base = data_start + val
            try:
                mn = tuple(struct.unpack_from(">f", data, base + j * 4)[0] for j in range(3))
                mx = tuple(struct.unpack_from(">f", data, base + 12 + j * 4)[0] for j in range(3))
                if all(math.isfinite(v) for v in mn + mx):
                    return mn, mx
            except Exception:
                pass
    return None


_FALLBACK_SCALE = 1.0 / 16384.0   # confirmed Layout D/D2 fixed-point scale
                                  # (rigid-vertex-to-bone-chain validation
                                  # across alicia.o + timothy.o converged
                                  # on this constant)

_BBOX_SCALE_WARN_TOLERANCE = 2.0  # ratio; warn if bbox estimate disagrees
                                  # with the fixed constant by more than 2x


def _estimate_bbox_scale(bbox) -> "float | None":
    """
    Legacy bbox-half-diagonal scale estimate. NO LONGER used to derive the
    actual position scale for Layout D/D2 -- rigid-vertex-to-bind-bone
    distance testing (across two independent character models) showed this
    formula systematically under-scales positions by ~4x (median error
    0.45m vs 0.11m with the fixed constant). Kept only as a diagnostic
    cross-check: if a given mesh's bbox-derived estimate disagrees sharply
    with 1/16384, that's a signal this particular mesh may be an exception
    worth inspecting by hand, not that the fixed constant is wrong.
    """
    if bbox is None:
        return None
    mn, mx = bbox
    half_diag = math.sqrt(sum((b - a) ** 2 for a, b in zip(mn, mx))) / 2.0
    if half_diag < 1e-6:
        return None
    return half_diag / 32767.0


def _derive_pos_scale(bbox, log: "list[str] | None" = None) -> float:
    """
    Return the s16 -> float world-space scale for Layout D/D2 positions.

    CONFIRMED: fixed-point scale is 1/16384 (0.00006103515625), not a
    per-model bbox-derived value. Validated via rigid (single-weight,
    weight==1.0) vertex-to-bind-bone-position distance across alicia.o and
    timothy.o: median error dropped from ~0.45m (bbox-derived formula) to
    ~0.11m (fixed 1/16384) on both independently-rigged characters.

    The bbox estimate is still computed and compared as a diagnostic --
    large disagreement gets logged so genuine per-mesh exceptions (if any
    turn out to exist) don't get silently steamrolled by the constant.
    """
    scale = _FALLBACK_SCALE
    bbox_est = _estimate_bbox_scale(bbox)
    if bbox_est is not None and log is not None:
        ratio = bbox_est / scale if scale else float("inf")
        if ratio > _BBOX_SCALE_WARN_TOLERANCE or ratio < 1.0 / _BBOX_SCALE_WARN_TOLERANCE:
            log.append(
                f"Layout D scale mismatch: bbox-derived estimate={bbox_est:.8f} "
                f"disagrees with expected 1/16384={scale:.8f} "
                f"(ratio={ratio:.2f}x) -- worth inspecting this mesh by hand."
            )
    return scale


# ---------------------------------------------------------------------------
# Relocation table
# ---------------------------------------------------------------------------

def _build_reloc_map(data: bytes, sections: list[dict], data_start: int) -> dict[int, int]:
    rel_sec = next((s for s in sections if s["name"] == ".rel.data"), None)
    if not rel_sec:
        return {}
    relocs = {}
    for i in range(rel_sec["size"] // 8):
        o = rel_sec["offset"] + i * 8
        r_off, r_info = struct.unpack_from("<II", data, o)
        if (r_info & 0xFF) == 2:
            relocs[r_off] = struct.unpack_from("<I", data, data_start + r_off)[0]
    return relocs


# ---------------------------------------------------------------------------
# Layout definitions — loaded from eagl_layouts.json
# ---------------------------------------------------------------------------

@functools.lru_cache(maxsize=8)
def _load_layouts_cached(json_path_str: str, mtime_ns: int) -> tuple[list[dict], int, int]:
    """Actual parse, cached on (path, mtime) -- see _load_layouts below.
    mtime_ns is part of the cache key purely to invalidate automatically
    when the file changes; it isn't otherwise used."""
    spec = _json.loads(Path(json_path_str).read_text())
    scoring = spec.get("_scoring", {})
    min_score = scoring.get("MIN_SCORE", 100)
    margin    = scoring.get("MARGIN",    30)

    layouts = []
    for entry in spec["layouts"]:
        # Normalise: JSON uses plain ints for offsets; parser expects the same.
        # off_pgx / off_cpos etc. may be null → None.
        layout = {k: v for k, v in entry.items()}
        layouts.append(layout)
    return layouts, min_score, margin


def _load_layouts(json_path: "str | Path | None" = None) -> tuple[list[dict], int, int]:
    """
    Load layout definitions and scoring config from eagl_layouts.json.

    Searches for the JSON file next to this script if no path is given.
    Returns (layouts, MIN_SCORE, MARGIN).

    Cached on (resolved path, mtime): a batch run parsing hundreds of .o
    files with the same layouts_path no longer re-reads and re-parses this
    JSON on every single call, but editing eagl_layouts.json mid-session
    (the hot-reload workflow this was originally built for) still busts
    the cache automatically via the mtime change.
    """
    if json_path is None:
        json_path = Path(__file__).with_name("eagl_layouts.json")
    json_path = Path(json_path)
    mtime_ns = json_path.stat().st_mtime_ns  # raises FileNotFoundError, same as before
    return _load_layouts_cached(str(json_path), mtime_ns)


# Load once at import time from the JSON file sitting next to the parser.
# parse_o_file() also accepts an explicit layouts_path argument to override.
try:
    _LAYOUTS, _MIN_SCORE, _MARGIN = _load_layouts()
except FileNotFoundError:
    # Fallback: hard-coded defaults so the parser still runs without the JSON.
    _LAYOUTS = [
        {"magic": 0x07, "name": "A",
         "off_cpos": 0x30, "off_ppos": 0x34, "off_cnorm": 0x38, "off_pnorm": 0x3c,
         "off_cuv":  0x40, "off_puv":  0x44,
         "norm_stride": 6, "gx_from_reloc": False, "off_pgx": None,
         "signature_checks": []},
        {"magic": 0x04, "name": "B",
         "off_cpos": 0x34, "off_ppos": 0x38, "off_cnorm": 0x3c, "off_pnorm": 0x40,
         "off_cuv":  0x44, "off_puv":  0x48,
         "norm_stride": 6, "gx_from_reloc": False, "off_pgx": None,
         "signature_checks": []},
        {"magic": 0x02, "name": "C",
         "off_cpos": None, "off_ppos": 0x34, "off_cnorm": None, "off_pnorm": 0x3c,
         "off_cuv":  None, "off_puv":  0x44,
         "norm_stride": 4, "gx_from_reloc": True, "off_pgx": 0x09,
         "signature_checks": []},
    ]
    _MIN_SCORE, _MARGIN = 0, 0


# ---------------------------------------------------------------------------
# Mesh descriptor discovery — scoring-based
# ---------------------------------------------------------------------------

def _score_layout(desc_off: int, layout: dict,
                  data: bytes, data_start: int,
                  relocs: dict[int, int],
                  nonaligned_relocs: "set[int] | None" = None) -> int:
    """
    Evaluate all signature_checks for *layout* at *desc_off* and return the
    total score.  Unknown check types contribute 0 and are silently skipped so
    that new check types added to the JSON don't break older parser builds.

    nonaligned_relocs — optional precomputed set of non-4-aligned reloc offsets,
    used to accelerate the reloc_nonaligned_in_window check from O(n_relocs)
    to O(window_size/4).  When None the check falls back to the linear scan.

    Checks marked ``"required": true`` in the JSON act as hard gates: if the
    check does not pass, the function returns 0 immediately regardless of the
    points accumulated so far.  This prevents high-accumulation false matches
    (e.g. Layout D matching world-mesh data that happens to satisfy every check
    *except* the reloc_self anchor).
    """
    raw   = data[data_start + desc_off : data_start + desc_off + 0x80]
    score = 0

    for chk in layout.get("signature_checks", []):
        t    = chk["type"]
        hit  = False

        if t == "magic":
            hit = chk["offset"] < len(raw) and raw[chk["offset"]] == chk["value"]

        elif t == "reloc_exists":
            hit = (desc_off + chk["offset"]) in relocs

        elif t == "reloc_nonzero":
            hit = relocs.get(desc_off + chk["offset"], 0) > 0

        elif t == "reloc_absent":
            hit = (desc_off + chk["offset"]) not in relocs

        elif t == "reloc_order":
            offs = chk["offsets"]
            vals = [relocs.get(desc_off + o, 0) for o in offs]
            hit  = all(vals[i] < vals[i + 1] for i in range(len(vals) - 1)) and vals[0] > 0

        elif t == "count_range":
            o = chk["offset"]
            if o + 4 <= len(raw):
                val = struct.unpack_from(">I", raw, o)[0]
                hit = chk["min"] <= val <= chk["max"]

        elif t == "reloc_self":
            # The unique Layout D structural anchor: reloc[desc+offset] == desc_off.
            hit = relocs.get(desc_off + chk["offset"]) == desc_off

        elif t == "reloc_nonaligned_in_window":
            ws = desc_off + chk["window_start"]
            we = desc_off + chk["window_end"]
            if nonaligned_relocs is not None:
                # Fast path: only probe offsets in the window that are non-aligned.
                hit = any(r in nonaligned_relocs for r in range(ws | 1, we, 2)
                          if r % 4 != 0)
            else:
                hit = any(ws <= r < we and r % 4 != 0 for r in relocs)

        elif t == "byte_value":
            o = chk["offset"]
            hit = o < len(raw) and raw[o] == chk["value"]

        # Unknown types: skip (forward-compatible).

        if hit:
            score += chk["score"]
        elif chk.get("required"):
            # Hard gate: a required check that doesn't pass vetoes the entire layout.
            return 0

    return score


def _find_descriptors(data: bytes, data_start: int, data_size: int,
                      relocs: dict[int, int],
                      layouts: list[dict] | None = None,
                      min_score: int | None = None,
                      margin: int | None = None) -> list[tuple[int, dict]]:
    """
    Scan the .data section for EAGL mesh descriptors using score-based layout
    detection.

    For every 4-byte-aligned candidate offset every layout is scored via its
    signature_checks.  The highest-scoring layout wins the offset if:
      1. Its score >= min_score  (absolute quality threshold), and
      2. It leads the runner-up by >= margin  (confidence gap).

    Returns a list of (desc_offset, layout_dict) sorted by pos-array pointer.
    """
    if layouts   is None: layouts   = _LAYOUTS
    if min_score is None: min_score = _MIN_SCORE
    if margin    is None: margin    = _MARGIN

    found = []
    seen  = set()

    # Precompute a set of all non-4-aligned reloc offsets once; passed to
    # _score_layout to accelerate the reloc_nonaligned_in_window check from
    # O(n_relocs) per call down to O(window_size).
    nonaligned_relocs: set[int] = {r for r in relocs if r % 4 != 0}

    # Index layouts by magic byte so we only score the (usually 1–2) layouts
    # whose magic matches the candidate byte, skipping the others entirely.
    # The world-low-all.o file has 582k 4-byte-aligned offsets but only ~19k
    # match any known magic byte, making this gate ~30× faster than scoring
    # all 5 layouts at every offset.
    _magic_index: dict[int, list[dict]] = {}
    for layout in layouts:
        for chk in layout.get("signature_checks", []):
            if chk["type"] == "magic" and chk["offset"] == 0:
                _magic_index.setdefault(chk["value"], []).append(layout)
                break

    for off in range(0, data_size - 0x60, 4):
        # Fast gate: only consider offsets whose first byte matches a known magic.
        first_byte = data[data_start + off]
        candidate_layouts = _magic_index.get(first_byte)
        if not candidate_layouts:
            continue

        # Score only the layouts that share this magic byte.
        scores = [
            (layout, _score_layout(off, layout, data, data_start, relocs,
                                   nonaligned_relocs))
            for layout in candidate_layouts
        ]
        scores.sort(key=lambda x: x[1], reverse=True)

        best_layout, best_score = scores[0]
        second_score = scores[1][1] if len(scores) > 1 else 0

        # Per-layout min_score overrides the global threshold (used by D/D2
        # to require the self-pointer check, which adds 60 pts, keeping
        # aliased world-file B descriptors from falsely matching).
        effective_min = best_layout.get("min_score", min_score)

        # Skip if below absolute threshold or margin is too narrow.
        if best_score < effective_min:
            continue
        if best_score - second_score < margin:
            continue

        if off not in seen:
            found.append((off, best_layout))
            seen.add(off)

    # ── De-alias Layout A/B collisions ──────────────────────────────────────
    # Layout A's field offsets are exactly Layout B's minus 4 bytes (off_ppos
    # 52 vs 56, etc). When a genuine Layout B descriptor's magic byte happens
    # to be followed 4 bytes later by another valid-looking magic byte, the
    # scanner matches the SAME physical mesh twice — once as B at offset N,
    # once as A at offset N+4 — both resolving to the identical ptr_pos and
    # therefore identical geometry. Detect and drop the duplicate, preferring
    # the lower descriptor offset (the "outer" / more complete descriptor).
    by_ptr_pos: dict[int, list[tuple[int, dict]]] = {}
    for off, layout in found:
        ptr_pos = relocs.get(off + layout["off_ppos"], 0)
        by_ptr_pos.setdefault(ptr_pos, []).append((off, layout))

    _STATIC_LAYOUTS = {"A", "B", "B2", "C"}

    deduped = []
    for ptr_pos, group in by_ptr_pos.items():
        if len(group) == 1 or ptr_pos == 0:
            deduped.extend(group)
            continue

        names = {l["name"] for _, l in group}
        offs = sorted(o for o, _ in group)

        # Pattern 1 — the documented A/B aliasing collision: a genuine
        # Layout B descriptor whose magic byte at +4 also happens to score
        # as Layout A, exactly 4 bytes later, resolving to the identical
        # ptr_pos (off_ppos for A is B's off_ppos - 4). Keep the lower
        # offset (the genuine B descriptor).
        is_ab_alias = (
            len(group) == 2
            and names == {"A", "B"}
            and (offs[1] - offs[0]) == 4
        )

        # Pattern 2 — a D/D2 descriptor colliding with a static-layout
        # descriptor's ptr_pos. Confirmed on garbagecan.o and
        # rc_buggybody.o: in both cases the D/D2 half of the pair was a
        # scorer misfire that also happened to reuse the static mesh's own
        # GX stream (identical gx_start, matching pos/norm/uv counts) --
        # not a real independent skinned mesh -- while the static A/B/B2/C
        # half was the genuine, triangle-producing geometry. Unlike
        # Pattern 1, D/D2's off_ppos resolution is unreliable enough
        # (see "could not find GX ptr" / "bone weight table out of range")
        # that a *different*, unrelated static mesh can also coincidentally
        # collide -- so this only fires for an actual 2-way D/D2-vs-static
        # pair, and always keeps the static side regardless of which
        # offset is lower.
        is_d_vs_static = (
            len(group) == 2
            and len(names & {"D", "D2"}) == 1
            and len(names & _STATIC_LAYOUTS) == 1
        )

        # Pattern 3 — a 3-way collision: the documented A/B alias pair
        # (Pattern 1) PLUS a D/D2 ghost sharing the same ptr_pos. Confirmed
        # on world-low-all.o desc 0x21f314 (D2): it out-scores every other
        # layout on structural relocation checks alone (self-reloc, count
        # ranges, pos<norm<uv ordering) but its GX-ptr scan then fails to
        # find a valid 0x98 strip opcode -- it's the scorer re-reading the
        # genuine B/A mesh's own pointer slots from a different descriptor
        # offset, not an independent second mesh. Neither Pattern 1 nor
        # Pattern 2 fires here because they only match 2-member groups;
        # this generalises Pattern 2's reasoning to "A/B alias + ghost".
        static_members = [g for g in group if g[1]["name"] in _STATIC_LAYOUTS]
        dd_members     = [g for g in group if g[1]["name"] in ("D", "D2")]
        static_offs    = sorted(o for o, _ in static_members)
        is_ab_alias_plus_ghost = (
            len(static_members) == 2
            and {l["name"] for _, l in static_members} == {"A", "B"}
            and (static_offs[1] - static_offs[0]) == 4
            and len(dd_members) >= 1
            and len(dd_members) + len(static_members) == len(group)
        )

        if is_ab_alias:
            group.sort(key=lambda x: x[0])
            deduped.append(group[0])  # lower-offset = genuine B
        elif is_d_vs_static:
            deduped.append(next(g for g in group if g[1]["name"] in _STATIC_LAYOUTS))
        elif is_ab_alias_plus_ghost:
            static_members.sort(key=lambda x: x[0])
            deduped.append(static_members[0])  # lower-offset genuine B; drop A alias + D/D2 ghost(s)
        else:
            # Any other pairing sharing a ptr_pos is not a confirmed
            # collision pattern -- keep both rather than guessing which
            # one is real (per-mesh-guard rule: only merge patterns that
            # were actually diagnosed).
            deduped.extend(group)

    found = deduped
    found.sort(key=lambda x: relocs.get(x[0] + x[1]["off_ppos"], 0))
    return found


# ---------------------------------------------------------------------------
# GX attribute list → vertex stride
# ---------------------------------------------------------------------------

GX_INDEX8  = 0x08
GX_INDEX16 = 0x09

# GX attribute TAG bytes
_GX_VA_PNMTXIDX = 0xc0
_GX_VA_POS       = 0x09
_GX_VA_NRM       = 0x0b
_GX_VA_CLR0      = 0x0a
_GX_VA_TEX0      = 0x0d

_VALID_ATTR_TAGS = frozenset({_GX_VA_PNMTXIDX, _GX_VA_POS, _GX_VA_NRM, _GX_VA_CLR0, _GX_VA_TEX0})
_VALID_ATTR_FMTS = frozenset({0x00, 0x08, 0x09})


def _parse_attr_table(attr_bytes: bytes) -> tuple[int, int, int, int, int, int, int, int]:
    """
    Parse the GX attribute table starting at the beginning of attr_bytes.

    Iterates pairs sequentially by TAG rather than reading fixed byte positions.
    This correctly handles 4-entry tables with a leading GX_VA_PNMTXIDX byte
    (world meshes with hardware-skinning matrix indices) which shift POS/NRM/TEX0
    one byte to the right.

    Format byte encoding (file-specific, not the GXAttrType SDK enum):
      0x09 → 2 bytes (INDEX16)
      0x08 or 0x00 → 1 byte (INDEX8 / GX_DIRECT)

    Returns:
      (stride, leading_skip, pos_sz, norm_sz, uv_sz, pos_off, norm_off, uv_off)
    """
    byte_off  = 0
    pos_off   = norm_off = uv_off   = -1
    pos_sz    = norm_sz  = uv_sz    = 0
    leading_skip = 0

    i = 0
    while i + 1 < len(attr_bytes):
        tag, fmt = attr_bytes[i], attr_bytes[i + 1]
        if tag == 0x00:
            break
        # Unknown tag → table is malformed at this position; abort.
        if tag not in _VALID_ATTR_TAGS or fmt not in _VALID_ATTR_FMTS:
            byte_off = 0   # signal failure
            break
        sz = 2 if fmt == GX_INDEX16 else 1
        if tag == _GX_VA_POS:
            pos_off, pos_sz = byte_off, sz
        elif tag in (_GX_VA_NRM, _GX_VA_CLR0):
            norm_off, norm_sz = byte_off, sz
        elif tag == _GX_VA_TEX0:
            uv_off, uv_sz = byte_off, sz
        elif tag == _GX_VA_PNMTXIDX:
            leading_skip += sz
        byte_off += sz
        i += 2

    # Fallback if parse failed or no attrs found
    if byte_off == 0 or pos_off < 0:
        pos_off, pos_sz   = 0, 1
        norm_off, norm_sz = 1, 1
        uv_off,  uv_sz    = 2, 1
        byte_off          = 3
        leading_skip      = 0
    else:
        if norm_off < 0:
            norm_off, norm_sz = pos_off + pos_sz, 1
        if uv_off < 0:
            uv_off, uv_sz = norm_off + norm_sz, 1

    return byte_off, leading_skip, pos_sz, norm_sz, uv_sz, pos_off, norm_off, uv_off


def _find_attr_table(raw: bytes, layout_off_attr: int) -> int:
    """
    Find the actual start offset of the GX attribute table within a descriptor's
    raw bytes by scanning from byte 8 forward.

    Most descriptors have the attr table exactly at layout_off_attr (8 or 12).
    Some Layout B descriptors with non-standard byte[1] values have it later
    (e.g. at +0x14 instead of +0x0c).

    A valid attr table start must:
      - Begin with a byte in _VALID_ATTR_TAGS
      - Have a following format byte in _VALID_ATTR_FMTS for every pair
      - Contain at least one GX_VA_POS (0x09) entry
      - Be terminated by 0x00

    Returns the found offset, or layout_off_attr as fallback.
    """
    # Try the layout-specified offset first (fast path for the common case)
    for start in [layout_off_attr] + list(range(8, 0x20)):
        if start >= len(raw) - 2:
            break
        if raw[start] not in _VALID_ATTR_TAGS:
            continue
        # Validate the full sequence
        has_pos = False
        j = start
        while j + 1 < min(len(raw), start + 20):
            tag, fmt = raw[j], raw[j + 1]
            if tag == 0x00:
                break
            if tag not in _VALID_ATTR_TAGS or fmt not in _VALID_ATTR_FMTS:
                has_pos = False
                break
            if tag == _GX_VA_POS:
                has_pos = True
            j += 2
        if has_pos:
            return start

    return layout_off_attr  # fallback


# ---------------------------------------------------------------------------
# GX display-list scanner — per-attribute stride-aware
# ---------------------------------------------------------------------------

# GX draw-call opcodes
#
# 0x90 (GX_TRIANGLES), 0xA0 (GX_TRIANGLE_FAN), and 0xA8 (GX_QUADS) are real
# GX opcodes, but none have been confirmed present in any EAGL file we've
# examined — every display list seen so far uses 0x98 (GX_TRIANGLE_STRIP)
# exclusively. Recognizing 0x90 as a command opcode is actively dangerous:
# it's also a perfectly ordinary INDEX8 vertex-index value (144), and it
# occurs naturally inside real vertex data (confirmed: 12/62 meshes in
# alicia.o have a literal 0x90 byte in their GX region). Treating it as an
# opcode causes false-positive primitive triggers that corrupt the parse
# of files that decoded correctly when only 0x98 was recognized. Until we
# have a confirmed real file using GX_TRIANGLES/FAN/QUADS, only 0x98 is
# scanned — this matches the pre-multi-primitive behavior intentionally.
_GX_TRIANGLES      = 0x90  # NOT scanned for — see note above
_GX_TRIANGLE_STRIP = 0x98  # triangle strip — alternating winding (only one scanned)
_GX_TRIANGLE_FAN   = 0xA0  # NOT scanned for — see note above
_GX_QUADS          = 0xA8  # NOT scanned for — see note above
_GX_DRAW_CMDS      = frozenset([
    _GX_TRIANGLE_STRIP,
])

# Sentinel values that mean "skip this vertex" (not a bounds violation)
_SKIP8  = 0xFF    # GX_INDEX8  skip sentinel
_SKIP16 = 0xFFFF  # GX_INDEX16 skip sentinel

# Minimum vertex counts per primitive type
_MIN_VC = {
    _GX_TRIANGLES:      3,
    _GX_TRIANGLE_STRIP: 3,
    _GX_TRIANGLE_FAN:   3,
    _GX_QUADS:          4,
}


def _validate_stride_gx(
    region: bytes,
    stride: int, pos_sz: int, norm_sz: int, uv_sz: int, leading_skip: int,
    max_pos: int, max_norm: int, max_uv: int,
    n_probe: int = 100,
) -> tuple[int, int, int, int, int]:
    """
    Validate the attr-table-derived stride against the actual GX stream.

    Probes the first n_probe 0x98 strip headers with the candidate stride and
    plausible neighbouring byte-width combinations. The attr table's format
    byte (0x00 vs 0x09) is ambiguous whenever the corresponding index range
    is small enough to fit in either an 8-bit or 16-bit field (the hardware
    will happily use 16-bit indices for a small array, e.g. when storage is
    shared/templated across mesh variants), so all three of pos_sz, norm_sz,
    uv_sz are independently flip-tested, not just uv_sz.

    Also probes leading_skip=0 as an alternative to a nonzero attr-table-
    derived leading_skip. GX_VA_PNMTXIDX (the hardware-skinning matrix-index
    byte) legitimately only appears in Layout D/D2 streams, which have their
    own dedicated hwskin scan path — Layout A/B/C describe static, unskinned
    geometry and should never have a real per-vertex matrix-index byte. A
    handful of world-low-all.o Layout B descriptors have a stray (0xc0, 0x08)
    byte pair sitting immediately before the real pos/norm/uv tag list —
    coincidentally a well-formed-looking PNMTXIDX+INDEX8 pair — which
    _parse_attr_table has no way to distinguish from a genuine one purely
    from the descriptor bytes. Confirmed on Mesh[451] in world-low-all.o:
    the attr-table reading (leading_skip=1) yields exactly 1 strip (7 faces)
    from a 1116-byte GX region; leading_skip=0 recovers 39 strips / 199
    faces from that same region. Scoring both here keeps the fix data-driven
    per mesh instead of a blanket "Layout B never has PNMTXIDX" rule change.

    Returns (stride, pos_sz, norm_sz, uv_sz, leading_skip) — the best-scoring
    combination.

    Tie-breaking: bounds-checking alone frequently ties two structurally
    different byte-width combinations (both stay in-range but only one
    matches the real per-vertex layout). Confirmed on rccartrack23.o
    Mesh[51]: stride=3 (p1/n1/u1) and stride=5 (p2/n1/u2) both score
    100/100 in-bounds strips, but hand-decoding the raw GX bytes shows
    stride=5 produces a clean monotonically-increasing index sequence
    ((0,0,0),(1,1,1),(2,2,2)...) while stride=3 does not and collapses
    ~27% of all vertex references onto position index 0 (a visible
    "fan to a point" artifact on export). To break such ties, each
    candidate also tracks a position-index concentration penalty: the
    fraction of all probed vertex refs landing on the single most-common
    position index. Lower concentration (more diverse position index
    usage, as real geometry exhibits) wins on ties in raw in-bounds score.
    """
    def _score(st: int, p: int, n: int, u: int, skip: int) -> tuple[int, float]:
        """Count in-bounds strips, and measure per-attribute index
        concentration (pos, norm, uv), for the first n_probe 0x98 primitives.

        Concentration is tracked independently per attribute rather than
        just position: a candidate width split can keep position indices
        perfectly diverse and in-bounds while still misreading the norm or
        uv byte (e.g. reading a narrower width than the true attribute
        needs, which silently degrades into a narrow, always-in-bounds
        range instead of failing the bounds check). Confirmed on
        world-low-all.o Mesh[375]/[398]: stride=5 (pos2/norm1/uv2) passed
        every bounds check yet collapsed ~99.8% of normal references onto
        index 0 -- a corruption that position-only concentration could not
        see. Tracking all three attributes' concentration and taking the
        worst (max) as the tie-break penalty catches this.
        """
        # Malformed/unsupported attribute layouts can declare a stride smaller
        # than the attributes this scorer reads. Reject, rather than indexing
        # past the GX region on rccartrack26.o.
        if st < skip + p + n + u:
            return 0, 1.0
        good = 0
        i = 0
        checked = 0
        pos_counts: dict[int, int] = {}
        norm_counts: dict[int, int] = {}
        uv_counts: dict[int, int] = {}
        total_refs = 0
        while i < len(region) - 3 and checked < n_probe:
            if region[i] != 0x98:
                i += 1
                continue
            vc = (region[i + 1] << 8) | region[i + 2]
            end = i + 3 + vc * st
            if vc < 3 or vc > 4096 or end > len(region):
                i += 1
                continue
            b = i + 3
            ok = True
            strip_verts = []
            for _ in range(vc):
                b += skip
                pi = (region[b] << 8 | region[b + 1]) if p == 2 else region[b]; b += p
                ni = (region[b] << 8 | region[b + 1]) if n == 2 else region[b]; b += n
                ui = (region[b] << 8 | region[b + 1]) if u == 2 else region[b]; b += u
                if pi > max_pos or ni > max_norm or ui > max_uv:
                    ok = False; break
                strip_verts.append((pi, ni, ui))
            if ok:
                good += 1
                for pi, ni, ui in strip_verts:
                    pos_counts[pi] = pos_counts.get(pi, 0) + 1
                    norm_counts[ni] = norm_counts.get(ni, 0) + 1
                    uv_counts[ui] = uv_counts.get(ui, 0) + 1
                    total_refs += 1
            checked += 1
            i = end
        if not total_refs:
            return good, 1.0
        pos_top  = max(pos_counts.values())  / total_refs
        norm_top = max(norm_counts.values()) / total_refs
        uv_top   = max(uv_counts.values())   / total_refs
        # Worst (highest) concentration across all three attributes -- a
        # candidate is only as good as its most-corrupted attribute.
        worst_top = max(pos_top, norm_top, uv_top)
        return good, worst_top

    base_score, base_top_frac = _score(stride, pos_sz, norm_sz, uv_sz, leading_skip)
    candidates = [(base_score, -base_top_frac, stride, pos_sz, norm_sz, uv_sz, leading_skip)]

    # Build every plausible (p, n, u) combination reachable by flipping one
    # or more of the three widths between 1 and 2 bytes, recomputing stride
    # as the byte-delta implies. Width 1 (INDEX8) is only physically valid
    # when the declared count fits in 0-254 (0xFF is the GX skip sentinel,
    # so 255 is not usable as a real index) -- GX hardware cannot address a
    # larger array with a 1-byte index at all, so offering width=1 as a
    # candidate when max_val > 254 was letting an impossible reading win
    # tie-breaks purely because a narrow read is trivially always in-bounds.
    # Confirmed root cause of the Mesh[375]/[398] corruption above.
    def _candidates_for(sz, max_val):
        widths = set()
        if max_val <= 254:
            widths.add(sz)
            widths.add(1)
        widths.add(2)
        return widths

    pos_opts  = _candidates_for(pos_sz, max_pos)
    norm_opts = _candidates_for(norm_sz, max_norm)
    uv_opts   = _candidates_for(uv_sz, max_uv)

    # leading_skip candidates: the attr-table reading, plus 0 (dropping a
    # possibly-spurious PNMTXIDX byte) whenever the reading is non-zero.
    skip_opts = {leading_skip}
    if leading_skip > 0:
        skip_opts.add(0)

    for skip in skip_opts:
        for p in pos_opts:
            for n in norm_opts:
                for u in uv_opts:
                    if (skip, p, n, u) == (leading_skip, pos_sz, norm_sz, uv_sz):
                        continue
                    st = skip + p + n + u
                    if st < 3:
                        continue
                    s, top_frac = _score(st, p, n, u, skip)
                    candidates.append((s, -top_frac, st, p, n, u, skip))

    best = max(candidates, key=lambda x: (x[0], x[1]))
    # If every candidate (including the original) scores 0, keep the
    # original attr-table-derived guess rather than silently picking an
    # arbitrary tied combination — a 0-score result means none of the
    # tried widths is right, and downstream code should surface that as
    # an empty mesh instead of pretending a guess is validated.
    if best[0] == 0:
        return stride, pos_sz, norm_sz, uv_sz, leading_skip
    return best[2], best[3], best[4], best[5], best[6]


def _scan_gx_strips(region: bytes, max_pos: int, max_uv: int,
                    stride: int, pos_sz: int, norm_sz: int, uv_sz: int,
                    leading_skip: int = 0, max_norm: int | None = None):
    """
    Scan a GX display-list region for all triangle-producing draw commands:

        0x90  GX_TRIANGLES       — independent triangle list
        0x98  GX_TRIANGLE_STRIP  — alternating-winding strip
        0xA0  GX_TRIANGLE_FAN    — fan around v0
        0xA8  GX_QUADS           — quad list (4 verts → 2 tris each)

    Each attribute (pos, norm, uv) is read independently with its own byte
    width (1 = INDEX8, 2 = INDEX16), so mixed-width meshes like
    pos=INDEX16 + norm=INDEX8 + uv=INDEX8 are handled correctly.

    Sentinel handling (per GX spec §4.2):
      INDEX8  sentinel 0xFF   → vertex is skipped, strip continues
      INDEX16 sentinel 0xFFFF → vertex is skipped, strip continues

    INDEX8 streams also enforce the hardware limit of max index 254
    regardless of the declared array size.

    Returns a list of (opcode, [vertex_tuples]) pairs where each
    vertex_tuple is (pos_idx, norm_idx, uv_idx).
    """
    primitives = []   # list of (opcode, verts_list)

    # INDEX8 hardware ceiling (0xFF is sentinel, so valid range is 0–254)
    pos_max8  = min(max_pos, 254) if pos_sz == 1 else max_pos
    uv_max8   = min(max_uv,  254) if uv_sz  == 1 else max_uv
    norm_max8 = (min(max_norm, 254) if norm_sz == 1 else max_norm) if max_norm is not None else None

    i = 0
    n = len(region)

    while i < n - 3:
        cmd = region[i]
        if cmd not in _GX_DRAW_CMDS:
            i += 1
            continue

        vc = (region[i + 1] << 8) | region[i + 2]
        min_vc = _MIN_VC[cmd]

        # GX_QUADS requires a multiple of 4
        if cmd == _GX_QUADS and vc % 4 != 0:
            i += 1
            continue

        if vc < min_vc or vc > 4096:
            i += 1
            continue

        start = i + 3
        end   = start + vc * stride
        if end > n:
            i += 1
            continue

        verts   = []
        corrupt = False
        b       = start

        for _ in range(vc):
            # ── skip leading bytes (e.g. GX_VA_PNMTXIDX) ──────────────
            b += leading_skip

            # ── pos index ──────────────────────────────────────────────
            if pos_sz == 2:
                pi = (region[b] << 8) | region[b + 1]
            else:
                pi = region[b]
            b += pos_sz

            # ── norm index ─────────────────────────────────────────────
            if norm_sz == 2:
                ni = (region[b] << 8) | region[b + 1]
            else:
                ni = region[b]
            b += norm_sz

            # ── uv index ───────────────────────────────────────────────
            if uv_sz == 2:
                ui = (region[b] << 8) | region[b + 1]
            else:
                ui = region[b]
            b += uv_sz

            # ── sentinel check (skip, do not break the primitive) ──────
            skip_pos  = (pos_sz == 1 and pi == _SKIP8)  or (pos_sz == 2 and pi == _SKIP16)
            skip_norm = (norm_sz == 1 and ni == _SKIP8) or (norm_sz == 2 and ni == _SKIP16)
            skip_uv   = (uv_sz  == 1 and ui == _SKIP8) or (uv_sz  == 2 and ui == _SKIP16)
            if skip_pos or skip_norm or skip_uv:
                continue  # GX spec: skip vertex, stream continues

            # ── bounds check ───────────────────────────────────────────
            if pi > pos_max8 or ui > uv_max8 or (norm_max8 is not None and ni > norm_max8):
                corrupt = True
                break

            verts.append((pi, ni, ui))

        if not corrupt and len(verts) >= min_vc:
            primitives.append((cmd, verts))
            i = end
        else:
            i += 1

    return primitives


def _validate_stride_gx_hwskin(
    region: bytes,
    pos_sz: int, norm_sz: int, uv_sz: int, mtx_stride: int,
    max_pos: int, max_norm: int, max_uv: int,
    n_probe: int = 100,
) -> tuple[int, int, int]:
    """
    Validate the count-derived pos/norm/uv byte widths for a D/D2 HWSkin GX
    stream against the actual data, the same way _validate_stride_gx does
    for the regular A/B/B2/C path.

    _scan_gx_strips_hwskin previously picked a single idx_width for all
    three attributes from one global "any count > 255" test, with no
    fallback if that guess was wrong. That's the same class of ambiguity
    documented for Layout A/B (pos=INDEX16 + norm/uv=INDEX8 mixed-width
    meshes) — nothing about D/D2's descriptor flags the width per-array,
    so a wrong global guess silently produced zero strips instead of a
    partially-wrong mesh, since every vertex in the stream shifts.

    Only GX_TRIANGLE_STRIP (0x98) is observed in HWSkin display lists, so
    that's the only opcode probed. mtx_stride (PNMTXIDX+TEXMTXIDX) is not
    varied — it's confirmed fixed at 2 bytes for every observed mesh.

    Returns (pos_sz, norm_sz, uv_sz) — the best-scoring combination.
    """
    def _score(p: int, n: int, u: int) -> int:
        vsize = mtx_stride + p + n + u
        good = 0
        i = 0
        checked = 0
        while i < len(region) - 3 and checked < n_probe:
            if region[i] != _GX_TRIANGLE_STRIP:
                i += 1
                continue
            vc = (region[i + 1] << 8) | region[i + 2]
            end = i + 3 + vc * vsize
            if vc < 3 or vc > 4096 or end > len(region):
                i += 1
                continue
            b = i + 3
            ok = True
            for _ in range(vc):
                b += mtx_stride
                pi = (region[b] << 8 | region[b + 1]) if p == 2 else region[b]; b += p
                ni = (region[b] << 8 | region[b + 1]) if n == 2 else region[b]; b += n
                ui = (region[b] << 8 | region[b + 1]) if u == 2 else region[b]; b += u
                if pi > max_pos or ni > max_norm or ui > max_uv:
                    ok = False; break
            if ok:
                good += 1
            checked += 1
            i = end
        return good

    base_score = _score(pos_sz, norm_sz, uv_sz)
    candidates = [(base_score, pos_sz, norm_sz, uv_sz)]

    def _candidates_for(sz, max_val):
        widths = {sz}
        if max_val <= 254:
            widths.add(1)
        widths.add(2)
        return widths

    pos_opts  = _candidates_for(pos_sz,  max_pos)
    norm_opts = _candidates_for(norm_sz, max_norm)
    uv_opts   = _candidates_for(uv_sz,   max_uv)

    for p in pos_opts:
        for n in norm_opts:
            for u in uv_opts:
                if (p, n, u) == (pos_sz, norm_sz, uv_sz):
                    continue
                candidates.append((_score(p, n, u), p, n, u))

    best = max(candidates, key=lambda x: x[0])
    # Same safety net as _validate_stride_gx: if nothing scores, keep the
    # original guess rather than confidently committing to a 0-score combo.
    if best[0] == 0:
        return pos_sz, norm_sz, uv_sz
    return best[1], best[2], best[3]


def _scan_gx_strips_hwskin(region: bytes, max_pos: int, max_norm: int, max_uv: int,
                           pos_sz: int, norm_sz: int, uv_sz: int, mtx_stride: int = 2):
    """
    GX display-list scanner for HWSkin Layout D / D2 meshes.

    Every vertex is prefixed with skinning-matrix index bytes before the
    geometry indices:

        [PNMTXIDX u8] [TEXMTXIDX u8] [pos_idx] [norm_idx] [uv_idx]

    PNMTXIDX/3 is a GX matrix-memory slot number; dividing by 3 gives the
    index into the mesh's bone-weight table (see _parse_mesh). TEXMTXIDX
    has been confirmed across every observed mesh to always equal
    PNMTXIDX + 30 (a fixed two-bank matrix layout), so it carries no
    independent information and is captured but not separately tracked.

    pos_sz/norm_sz/uv_sz are each independently 1 (INDEX8) or 2 (INDEX16),
    matching the generic _scan_gx_strips — see _validate_stride_gx_hwskin,
    which is what actually determines these widths per mesh (the old
    single-global-idx_width guess produced zero strips whenever a mesh's
    true widths were mixed, e.g. pos=INDEX16 + norm/uv=INDEX8).

    Using the generic _scan_gx_strips() on a D/D2 stream would misread the
    leading matrix-index bytes as part of the position index, shifting
    every vertex and producing distorted geometry — this is why D/D2 needs
    its own scanner. Only GX_TRIANGLE_STRIP (0x98) is observed in HWSkin
    display lists, consistent with the rest of this parser.

    Returns a list of (opcode, [vertex_tuples]) pairs where each
    vertex_tuple is (pos_idx, norm_idx, uv_idx, mtx0_slot, mtx1_slot).
    """
    # INDEX8 hardware ceiling (0xFF is sentinel, so valid range is 0–254)
    pos_max8  = min(max_pos,  254) if pos_sz  == 1 else max_pos
    norm_max8 = min(max_norm, 254) if norm_sz == 1 else max_norm
    uv_max8   = min(max_uv,   254) if uv_sz   == 1 else max_uv

    vertex_size = mtx_stride + pos_sz + norm_sz + uv_sz
    primitives  = []
    i = 0
    n = len(region)

    while i < n - 3:
        cmd = region[i]
        if cmd != _GX_TRIANGLE_STRIP:
            i += 1
            continue

        vc = (region[i + 1] << 8) | region[i + 2]
        if vc < 3 or vc > 4096:
            i += 1
            continue

        start = i + 3
        end   = start + vc * vertex_size
        if end > n:
            i += 1
            continue

        verts   = []
        corrupt = False
        b       = start
        for _ in range(vc):
            mtx0, mtx1 = region[b], region[b + 1]
            b += mtx_stride

            pi = (region[b] << 8) | region[b + 1] if pos_sz  == 2 else region[b]; b += pos_sz
            ni = (region[b] << 8) | region[b + 1] if norm_sz == 2 else region[b]; b += norm_sz
            ui = (region[b] << 8) | region[b + 1] if uv_sz   == 2 else region[b]; b += uv_sz

            skip_pos  = (pos_sz  == 1 and pi == _SKIP8) or (pos_sz  == 2 and pi == _SKIP16)
            skip_norm = (norm_sz == 1 and ni == _SKIP8) or (norm_sz == 2 and ni == _SKIP16)
            skip_uv   = (uv_sz   == 1 and ui == _SKIP8) or (uv_sz   == 2 and ui == _SKIP16)
            if skip_pos or skip_norm or skip_uv:
                continue  # GX skip sentinel; strip continues

            if pi > pos_max8 or ni > norm_max8 or ui > uv_max8:
                corrupt = True
                break
            verts.append((pi, ni, ui, mtx0, mtx1))

        if not corrupt and len(verts) >= 3:
            primitives.append((cmd, verts))
            i = end
        else:
            i += 1

    return primitives


def _is_degenerate(a, b, c) -> bool:
    """True if any two position indices in a triangle are the same."""
    return a[0] == b[0] or b[0] == c[0] or a[0] == c[0]


def _strips_to_faces(primitives):
    """
    Convert a list of (opcode, verts) pairs into a flat list of triangles.

    GX_TRIANGLES      — every 3 verts is one face; no winding adjustment
    GX_TRIANGLE_STRIP — alternating-winding strip (even: ABC, odd: BAC)
    GX_TRIANGLE_FAN   — v0 is the shared hub: (v0, v[i], v[i+1])
    GX_QUADS          — every 4 verts → (v0,v1,v2) + (v0,v2,v3)
    """
    faces = []
    # NOTE: GX winding (as decoded above) comes out backwards relative to
    # the stored vertex normals for glTF's right-handed CCW-front convention
    # -- verified empirically: cross(b-a, c-a) disagreed in sign with the
    # stored normal on ~99.5% of faces across timothy.o. Rather than patch
    # every branch above, flip each triangle once here at emission time.
    app = lambda tri: faces.append((tri[0], tri[2], tri[1]))

    for cmd, verts in primitives:
        nv = len(verts)

        if cmd == _GX_TRIANGLES:
            # Independent triangles — groups of 3, no winding flip
            for j in range(0, nv - 2, 3):
                a, b, c = verts[j], verts[j + 1], verts[j + 2]
                if not _is_degenerate(a, b, c):
                    app((a, b, c))

        elif cmd == _GX_TRIANGLE_STRIP:
            # Alternating winding
            for j in range(nv - 2):
                if j & 1 == 0:
                    a, b, c = verts[j], verts[j + 1], verts[j + 2]
                else:
                    a, b, c = verts[j + 1], verts[j], verts[j + 2]
                if not _is_degenerate(a, b, c):
                    app((a, b, c))

        elif cmd == _GX_TRIANGLE_FAN:
            # v0 is the shared hub
            v0 = verts[0]
            for j in range(1, nv - 1):
                a, b, c = v0, verts[j], verts[j + 1]
                if not _is_degenerate(a, b, c):
                    app((a, b, c))

        elif cmd == _GX_QUADS:
            # Every 4 verts is one quad → 2 tris: (0,1,2) and (0,2,3)
            for j in range(0, nv - 3, 4):
                q = verts[j:j + 4]
                if len(q) < 4:
                    break
                tri1 = (q[0], q[1], q[2])
                tri2 = (q[0], q[2], q[3])
                if not _is_degenerate(*tri1):
                    app(tri1)
                if not _is_degenerate(*tri2):
                    app(tri2)

    return faces


# ---------------------------------------------------------------------------
# Single mesh-chunk parser
# ---------------------------------------------------------------------------

def _parse_mesh(data: bytes, data_start: int, data_size: int,
                relocs: dict[int, int], desc_off: int, layout: dict,
                index: int, gx_end: int, pos_scale: float = _FALLBACK_SCALE) -> MeshChunk:
    warnings = []
    # Layout D fields extend to offset 0x6c (+4 bytes), so we need 0x70.
    # Use 0x80 for headroom; safe because _score_layout already reads that far.
    raw = data[data_start + desc_off : data_start + desc_off + 0x80]

    ptr_pos  = relocs[desc_off + layout["off_ppos"]]
    ptr_norm = relocs[desc_off + layout["off_pnorm"]]
    ptr_uv   = relocs[desc_off + layout["off_puv"]]

    norm_stride = layout["norm_stride"]  # JSON default: 6 for A/B, 4 for C
    pos_stride  = layout.get("pos_stride", 12)
    pos_format  = layout.get("pos_format", "f32")
    norm_format = layout.get("norm_format", "f32")

    # For Layouts A/B: override norm_stride from the (ptr_uv - ptr_norm) / count_norm gap.
    # The JSON default of 6 is wrong for world terrain files (s8x3+pad = stride 4).
    # Only when counts are declared (not gap-derived) and both ptrs are known.
    if layout["name"] in ("A", "B") and layout.get("off_cnorm") is not None:
        _cn_raw = struct.unpack_from(">I", raw, layout["off_cnorm"])[0]
        if 1 <= _cn_raw <= 8000 and ptr_norm > 0 and ptr_uv > ptr_norm:
            _gap = ptr_uv - ptr_norm
            if _gap % _cn_raw == 0:
                norm_stride = _gap // _cn_raw

    # Counts: from descriptor fields for A/B/D; from pointer gaps for C
    if layout["name"] in ("D", "D2"):
        # Layout D / D2: counts from descriptor fields; GX ptr from non-aligned reloc scan
        count_pos  = struct.unpack_from(">I", raw, layout["off_cpos"])[0]
        count_norm = struct.unpack_from(">I", raw, layout["off_cnorm"])[0]
        count_uv   = struct.unpack_from(">I", raw, layout["off_cuv"])[0]

        # GX ptr: scan for a non-aligned reloc in the window defined by off_pgx_scan
        scan_start = desc_off + layout.get("off_pgx_scan", [0, 40])[0]
        scan_end   = desc_off + layout.get("off_pgx_scan", [0, 40])[1]
        gx_start   = None
        for r_off in sorted(relocs):
            if scan_start <= r_off < scan_end and r_off % 4 != 0 and relocs[r_off] > 0:
                gx_start = relocs[r_off]
                break
        if gx_start is None:
            warnings.append(f"Layout {layout['name']}: could not find GX ptr via non-aligned reloc scan")
            return MeshChunk(index, desc_off, 3, layout["name"], [], [], [], [], warnings)

        _log.debug("Mesh[%d] Layout %s: pos=%d norm=%d uv=%d gx=0x%06x",
                   index, layout['name'], count_pos, count_norm, count_uv, gx_start)

    elif layout["off_cpos"] is not None:
        count_pos  = struct.unpack_from(">I", raw, layout["off_cpos"])[0]
        count_norm = struct.unpack_from(">I", raw, layout["off_cnorm"])[0]
        count_uv   = struct.unpack_from(">I", raw, layout["off_cuv"])[0]
        # GX start: immediately after UV array, 32-byte aligned
        gx_start = (ptr_uv + count_uv * 8 + 31) & ~31
    else:
        # Layout C: derive counts from pointer gaps; GX start from direct reloc
        count_pos  = (ptr_norm - ptr_pos)  // 12
        count_norm = (ptr_uv   - ptr_norm) // norm_stride
        ptr_gx     = relocs.get(desc_off + layout["off_pgx"])
        if ptr_gx is None:
            warnings.append(
                f"Layout C: GX ptr reloc missing at +0x{layout['off_pgx']:02x} "
                f"(desc=0x{desc_off:05x})"
            )
            return MeshChunk(index, desc_off, 3, layout["name"], [], [], [], [], warnings)
        count_uv   = (ptr_gx   - ptr_uv)   // 8
        gx_start   = ptr_gx
        _log.debug("Mesh[%d] Layout C: pos_count=%d norm_count=%d uv_count=%d gx_start=0x%06x",
                   index, count_pos, count_norm, count_uv, gx_start)

    # Detect GX index mode from attribute list.
    # off_attr is layout-specific: Layout A/C/D use +0x08, Layout B uses +0x0c.
    # Some descriptors have non-standard layout (extended header) that shifts the
    # attr table further; scan dynamically to find the real start.
    attr_off_hint = layout.get("off_attr", 8)
    attr_off      = _find_attr_table(raw, attr_off_hint)
    attr_bytes    = raw[attr_off : attr_off + 16]   # 16 bytes covers up to 7 pairs + terminator
    stride, leading_skip, pos_sz, norm_sz, uv_sz, pos_off, norm_off, uv_off = _parse_attr_table(attr_bytes)

    # Sanity checks
    for name, count, ptr, arr_stride in [
        ("pos",  count_pos,  ptr_pos,  pos_stride),
        ("norm", count_norm, ptr_norm, norm_stride),
        ("uv",   count_uv,   ptr_uv,   8),
    ]:
        if not (1 <= count <= 8000) or ptr + count * arr_stride > data_size:
            warnings.append(f"{name} array out of range (count={count} ptr=0x{ptr:04x})")

    if warnings:
        return MeshChunk(index, desc_off, 3, layout["name"], [], [], [], [], warnings)

    # Positions
    if pos_format == "s16":
        # Layout D/D2: big-endian s16×3, stride 6.
        # Scale: pos_world = s16_val * (bbox_half_diagonal / 32767)
        positions = [
            tuple(v * pos_scale for v in struct.unpack_from(">hhh", data, data_start + ptr_pos + i * 6))
            for i in range(count_pos)
        ]
    else:
        # Layouts A/B/C: BE float×3, stride 12
        positions = [
            struct.unpack_from(">fff", data, data_start + ptr_pos + i * 12)
            for i in range(count_pos)
        ]

    # Normals
    if norm_format == "s16":
        # Layout D/D2: big-endian s16×3, stride 6.
        # CONFIRMED against playgroundz.elf: EAGL::DrawImmediate::SetVertexDescription
        # sets GXSetVtxAttrFmt(..., GX_VA_NRM, cnt=3, GX_S16, frac=0xe) for the
        # s16-normal path -- i.e. real GX hardware quantization is value / 2^14,
        # a power of two, same convention as positions (frac=0xf -> 1/32768).
        # The previous 2.0/32767.0 constant was an empirical bbox-magnitude fit,
        # not the actual hardware formula -- it happens to sit within ~0.003% of
        # 1/16384, which is why it passed the alicia.o magnitude check without
        # being correct. Reuses _FALLBACK_SCALE (1/16384) for consistency with
        # the already-confirmed position scale.
        normals = [
            tuple(v * _FALLBACK_SCALE for v in struct.unpack_from(">hhh", data, data_start + ptr_norm + i * 6))
            for i in range(count_norm)
        ]
    else:
        # Layouts A/B/C: signed-byte×3, stride norm_stride, normalised ÷127
        normals = [
            tuple(b / 127.0 for b in struct.unpack_from("bbb", data, data_start + ptr_norm + i * norm_stride))
            for i in range(count_norm)
        ]

    # UVs: BE float×2, stride 8. Raw GX UV space is top-left origin, same as
    # glTF's TEXCOORD convention -- no V flip needed for glTF export. (The
    # old `1.0 - v` flip was a leftover from an earlier OBJ-based pipeline;
    # OBJ uses bottom-left origin so it needed the flip, glTF does not.)
    uv_buf = data[data_start + ptr_uv : data_start + ptr_uv + count_uv * 8]
    uvs = [(u, v) for u, v in struct.iter_unpack(">ff", uv_buf)]

    # GX display list bounded by next mesh's pos-array start
    gx_end    = min(gx_end, data_size)
    gx_region = data[data_start + gx_start : data_start + gx_end]

    primitives_is_hwskin = layout["name"] in ("D", "D2")
    if primitives_is_hwskin:
        # HWSkin GX streams prefix every vertex with PNMTXIDX/TEXMTXIDX
        # bytes that the generic A/B/C scanner doesn't know to skip. Seed
        # an initial per-array width guess from the counts (no static
        # descriptor field flags INDEX8 vs INDEX16 for D/D2, unlike A/B/C),
        # then validate/refine it against the actual stream the same way
        # the regular path does — the old single global guess produced
        # zero strips whenever a mesh's true widths were mixed.
        seed_pos_sz  = 2 if count_pos  > 255 else 1
        seed_norm_sz = 2 if count_norm > 255 else 1
        seed_uv_sz   = 2 if count_uv   > 255 else 1
        pos_sz, norm_sz, uv_sz = _validate_stride_gx_hwskin(
            gx_region, seed_pos_sz, seed_norm_sz, seed_uv_sz, mtx_stride=2,
            max_pos=count_pos - 1, max_norm=count_norm - 1, max_uv=count_uv - 1,
        )
        primitives = _scan_gx_strips_hwskin(
            gx_region, count_pos - 1, count_norm - 1, count_uv - 1,
            pos_sz, norm_sz, uv_sz, mtx_stride=2,
        )
        stride = 2 + pos_sz + norm_sz + uv_sz  # 2 mtx-idx bytes + 3 geometry indices
    else:
        # Validate stride empirically against the GX stream.
        # The attr table's TEX0 format byte (0x00 vs 0x09) is ambiguous for
        # large meshes where UV indices exceed 255 — the attr table alone can
        # yield stride=5 when stride=6 is actually required.  Probe the first
        # N strips with the candidate stride and each immediate neighbour; pick
        # the one that gives the highest fraction of in-bounds strips.
        stride, pos_sz, norm_sz, uv_sz, leading_skip = _validate_stride_gx(
            gx_region, stride, pos_sz, norm_sz, uv_sz, leading_skip,
            count_pos - 1, count_norm - 1, count_uv - 1,
        )
        primitives = _scan_gx_strips(gx_region, count_pos - 1, count_uv - 1, stride, pos_sz, norm_sz, uv_sz, leading_skip, max_norm=count_norm - 1)

    # Log primitive type breakdown for debugging
    if primitives:
        _CMD_NAMES = {
            _GX_TRIANGLES:      "TRI",
            _GX_TRIANGLE_STRIP: "STRIP",
            _GX_TRIANGLE_FAN:   "FAN",
            _GX_QUADS:          "QUAD",
        }
        cmd_counts = Counter(_CMD_NAMES.get(cmd, f"0x{cmd:02x}") for cmd, _ in primitives)
        _log.debug("Mesh[%d] GX primitives: %s", index, dict(cmd_counts))

    faces = _strips_to_faces(primitives)

    if not faces:
        warnings.append("no faces extracted from GX display list")

    # Bone weights (Layout D/D2 only): f32 (w1, w2) pairs, stride 16,
    # at the table pointed to by off_pweight / counted by off_cweight.
    bone_weights: list[tuple[float, float]] = []
    vertex_joints: dict = {}
    if primitives_is_hwskin:
        off_cweight = layout.get("off_cweight")
        off_pweight = layout.get("off_pweight")
        if off_cweight is not None and off_pweight is not None:
            count_weight = struct.unpack_from(">I", raw, off_cweight)[0]
            ptr_weight   = relocs.get(desc_off + off_pweight, 0)
            if ptr_weight > 0 and 1 <= count_weight <= 256 \
                    and ptr_weight + count_weight * 16 <= data_size:
                bone_weights = []
                for i in range(count_weight):
                    base = data_start + ptr_weight + i * 16
                    raw_entry = data[base:base + 16]
                    w0, w1, w2, _ = struct.unpack_from(">4f", data, base)
                    b0 = raw_entry[3]
                    b1 = raw_entry[7]  if w1 > 1e-4 else 0
                    b2 = raw_entry[11] if w2 > 1e-4 else 0
                    bone_weights.append((w0, w1, w2, b0, b1, b2))
            elif count_weight:
                warnings.append(f"bone weight table out of range (count={count_weight} ptr=0x{ptr_weight:04x})")

        # Map each face vertex (pos_idx, norm_idx, uv_idx) directly to its
        # (w0, w1, w2, bone_A, bone_B, bone_C) tuple so downstream consumers
        # (exporter, inspect) need no secondary bone_weights lookup.
        if bone_weights:
            n_weights = len(bone_weights)
            for tri in faces:
                for pi, ni, ui, mtx0, _mtx1 in tri:
                    slot = mtx0 // 3
                    if 0 <= slot < n_weights:
                        vertex_joints[(pi, ni, ui)] = bone_weights[slot]
                    else:
                        warnings.append(
                            f"weight slot {slot} out of range (table size {n_weights}) "
                            f"for vertex ({pi},{ni},{ui})"
                        )

        # Truncate faces back to the plain (pos_idx, norm_idx, uv_idx) shape
        # that the exporter and OBJ writer expect; skinning info lives in
        # vertex_joints, keyed by that same 3-tuple.
        faces = [
            tuple((pi, ni, ui) for pi, ni, ui, _m0, _m1 in tri)
            for tri in faces
        ]

    mesh=MeshChunk(index, desc_off, stride, layout["name"], positions, normals, uvs,
                   faces, warnings, bone_weights, vertex_joints)
    # Preserve the actual source arrays for exact shader/material ownership.
    # A heuristic descriptor start can precede the real PCode by a few bytes.
    mesh.source_arrays={'positions':ptr_pos,'normals':ptr_norm,'uvs':ptr_uv,'display_list':gx_start}
    return mesh


# ---------------------------------------------------------------------------
# Inspector integration helper
# ---------------------------------------------------------------------------

def _apply_inspect_result(inspect_result, relocs, layouts, min_score, margin,
                          data, data_start, data_size, log):
    """
    Convert an InspectResult into the (desc_offset, layout_dict) list that the
    rest of parse_o_file expects, or return None to signal "run full scan".

    Strategy
    --------
    HIGH / MEDIUM confidence
        The inspector resolved exact descriptor offsets from the symbol table.
        We trust them, look up the matching layout dict by predicted_layout
        name, and skip _find_descriptors entirely.  Each offset is still
        re-scored to confirm it passes min_score; any that don't fall back
        individually to a score-based winner so nothing is silently dropped.

    LOW confidence or no descriptor offsets
        Return None.  The caller runs the full heuristic scan as before.
        The caller could also seed the scan with the inspector's offsets, but
        in practice LOW confidence means the symbol table was ambiguous, so
        it's cleaner to let the scorer decide on its own.
    """
    if inspect_result is None:
        return None

    confidence       = getattr(inspect_result, "confidence", "LOW")
    predicted_layout = getattr(inspect_result, "predicted_layout", None)
    desc_offsets     = getattr(inspect_result, "descriptor_offsets", [])

    if not desc_offsets:
        log.append("Inspector: no descriptor offsets — falling back to heuristic scan.")
        return None

    if confidence == "LOW":
        log.append(f"Inspector: confidence LOW — falling back to heuristic scan "
                   f"(predicted={predicted_layout}).")
        return None

    log.append(f"Inspector fast-path: confidence={confidence}  "
               f"predicted_layout={predicted_layout}  "
               f"descriptors={[hex(o) for o in desc_offsets]}")

    # Find the layout dict that matches the prediction (used only as a
    # logging hint now — see note below on why we can't trust it alone).
    predicted_dict = next(
        (L for L in layouts if L["name"] == predicted_layout), None
    )

    # Precompute non-aligned reloc set once; avoids O(n_relocs) scan inside
    # _score_layout for every descriptor × layout combination.
    nonaligned_relocs: set[int] = {r for r in relocs if r % 4 != 0}

    # Index layouts by magic byte for the same gate used in _find_descriptors.
    _magic_index: dict[int, list[dict]] = {}
    for layout in layouts:
        for chk in layout.get("signature_checks", []):
            if chk["type"] == "magic" and chk["offset"] == 0:
                _magic_index.setdefault(chk["value"], []).append(layout)
                break

    descriptors = []
    for desc_off in desc_offsets:
        # IMPORTANT: we always score against every layout and pick the
        # margin-clearing winner, even when the inspector's prediction
        # passes MIN_SCORE on its own. The inspector's self-pointer rule
        # (desc+0x2c) only proves "this is the D *family*" — it can't tell
        # D from D2, since both share that marker and the same magic byte.
        # A true D2 descriptor scores ~165 against D's rules (high enough
        # to clear MIN_SCORE=130) but should score ~220 against D2's rules
        # and win by margin. Accepting on the predicted layout alone (as a
        # short-circuit) silently locks in the wrong layout for every D2
        # descriptor in the file — this was a real, confirmed bug.
        scores = [
            (L, _score_layout(desc_off, L, data, data_start, relocs,
                              nonaligned_relocs))
            for L in _magic_index.get(
                data[data_start + desc_off] if desc_off < data_size else 0,
                layouts  # fall back to all layouts if magic byte is unknown
            )
        ]
        scores.sort(key=lambda x: x[1], reverse=True)
        best_layout, best_score = scores[0]
        second_score = scores[1][1] if len(scores) > 1 else 0

        # Enforce global min_score, per-layout min_score, and margin.
        # Per-layout min_score (e.g. D/D2 = 160) requires high-value checks
        # like reloc_self to have fired; this prevents aliased world-file
        # B descriptors from false-positiving as D/D2 (which score ~145
        # via aliasing without a self-ptr but ~200+ when the self-ptr fires).
        effective_min = max(min_score, best_layout.get("min_score", min_score))
        if best_score >= effective_min and (best_score - second_score) >= margin:
            descriptors.append((desc_off, best_layout))
            if predicted_layout is not None and best_layout["name"] != predicted_layout:
                log.append(f"  desc 0x{desc_off:04x}: winner={best_layout['name']} "
                           f"score={best_score}  (inspector predicted {predicted_layout} — "
                           f"corrected by full re-score) ✓")
            else:
                log.append(f"  desc 0x{desc_off:04x}: layout {best_layout['name']} "
                           f"score={best_score} ✓")
        else:
            log.append(f"  desc 0x{desc_off:04x}: no layout cleared thresholds "
                       f"(best={best_score}) — skipping.")

    if not descriptors:
        log.append("Inspector fast-path yielded no valid descriptors — "
                   "falling back to heuristic scan.")
        return None

    # Sort by pos-array pointer, same as _find_descriptors does.
    descriptors.sort(key=lambda x: relocs.get(x[0] + x[1]["off_ppos"], 0))
    return descriptors


# ---------------------------------------------------------------------------
# Main parse entry point
# ---------------------------------------------------------------------------

def parse_o_file(path: str | Path,
                 layouts_path: "str | Path | None" = None,
                 inspect_result=None,
                 data: bytes | None = None) -> ParseResult:
    """
    Parse an EA Sports EAGL Wii .o file and return a ParseResult.

    layouts_path   — override the eagl_layouts.json location.  Defaults to the
                     file sitting next to this script.
    inspect_result — optional InspectResult from eagl_inspect.inspect_o_file().
                     When provided with HIGH or MEDIUM confidence, the parser
                     skips the heuristic descriptor scan and uses the inspector's
                     symbol-table-derived descriptor offsets and layout prediction
                     directly.  On LOW confidence (or if None), the full heuristic
                     scan runs as normal, but any descriptor_offsets from the
                     inspector are still used to seed the candidate list so the
                     scan has fewer false positives to compete with.
    data           — pre-read file bytes, for callers (e.g. batch export) that
                     already have the file in memory and want to avoid a
                     second disk read of a potentially multi-MB .o file.
                     Defaults to reading `path` as before.
    """
    # Load layout definitions (and scoring thresholds) for this parse.
    # Cached on (path, mtime) -- see _load_layouts -- so re-loading per call
    # is cheap even across a large batch, and hot-reload still works.
    try:
        layouts, min_score, margin = _load_layouts(layouts_path)
    except FileNotFoundError:
        layouts, min_score, margin = _LAYOUTS, _MIN_SCORE, _MARGIN

    path = Path(path)
    if data is None:
        data = path.read_bytes()
    log  = []

    if data[:4] != b"\x7fELF":
        return ParseResult("", "", [], log=["Not an ELF file"])
    log.append(f"ELF OK  ({len(data)} bytes)")

    sections, data_start = _read_sections(data)
    data_sec = next((s for s in sections if s["name"] == ".data"), None)
    if not data_sec:
        return ParseResult("", "", [], log=log + ["No .data section"])

    data_size = data_sec["size"]
    log.append(f".data @ 0x{data_start:04x}  size 0x{data_size:04x}")

    model_name, material_name, material_count = _extract_names(data, sections)
    log.append(f"Model: {model_name}  total materials: {material_count}")

    relocs = _build_reloc_map(data, sections, data_start)
    log.append(f"Reloc entries: {len(relocs)}")

    # Derive s16 position scale for Layout D / D2 meshes.
    bbox      = _extract_bbox(data, sections, data_start)
    pos_scale = _derive_pos_scale(bbox, log)
    log.append(f"BBOX: {bbox}  pos_scale={pos_scale:.8f}")

    # ------------------------------------------------------------------
    # Inspector fast-path: use symbol-table anchors when available.
    # ------------------------------------------------------------------
    descriptors = _apply_inspect_result(
        inspect_result, relocs, layouts, min_score, margin,
        data, data_start, data_size, log,
    )

    if descriptors is None:
        # Inspector gave no usable result — run the full heuristic scan.
        log.append(f"Scoring: {len(layouts)} layouts  min_score={min_score}  margin={margin}")
        descriptors = _find_descriptors(data, data_start, data_size, relocs,
                                        layouts=layouts,
                                        min_score=min_score,
                                        margin=margin)

    desc_labels = [f"0x{d[0]:04x}(L{d[1]['name']})" for d in descriptors]
    log.append(f"Mesh descriptors: {len(descriptors)} at {desc_labels}")

    if not descriptors:
        return ParseResult(model_name, material_name, [], log=log + ["No descriptors found"])

    # Pre-compute per-mesh GX upper bound = next mesh's pos-array start.
    #
    # Special case — paired descriptors sharing one vertex array:
    # Some meshes appear as two consecutive descriptors whose ptr_pos
    # resolves identically — they describe the same underlying vertex
    # arrays from two different "views" (e.g. a Layout B render descriptor
    # paired with a Layout D skinning-weight overlay for the same mesh).
    # The originally-documented case was Layout A aliased 4 bytes after a
    # genuine Layout B descriptor. The same symptom also shows up between
    # D/D2 and B with no fixed byte gap (confirmed on garbagecan.o and
    # world-low-all.o Mesh[480] etc. — matching declared pos/norm/uv counts
    # between the D and paired B descriptor is the tell). Keying off the
    # shared ptr_pos value directly (rather than a fixed offset delta)
    # catches both: using the immediate next descriptor's ptr_pos as this
    # mesh's gx_end would otherwise equal this mesh's OWN ptr_pos, giving
    # a zero/negative-length region and silently zero faces.
    gx_ends = []
    for i, (desc_off, layout) in enumerate(descriptors):
        if i + 1 < len(descriptors):
            next_off, next_layout = descriptors[i + 1]
            next_pos = relocs.get(next_off + next_layout["off_ppos"], data_size)
            # Detect paired descriptor: next one resolves to the same
            # ptr_pos as this mesh -> both draw from/describe the same
            # underlying vertex arrays. Look one step further ahead for
            # the true upper bound.
            if next_pos != 0 and next_pos == relocs.get(desc_off + layout["off_ppos"], -1):
                if i + 2 < len(descriptors):
                    after_off, after_layout = descriptors[i + 2]
                    next_pos = relocs.get(after_off + after_layout["off_ppos"], data_size)
                else:
                    next_pos = data_size
        else:
            next_pos = data_size
        gx_ends.append(next_pos)

    meshes = [
        _parse_mesh(data, data_start, data_size, relocs, off, layout, i, gx_ends[i],
                    pos_scale=pos_scale)
        for i, (off, layout) in enumerate(descriptors)
    ]

    ok_count = sum(1 for m in meshes if m.ok)
    log.append(f"OK: {ok_count}/{len(meshes)} meshes  total tris: {sum(len(m.faces) for m in meshes)}")

    return ParseResult(model_name, material_name, meshes, log)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import sys
    if len(sys.argv) < 2:
        print("Usage: python parser.py <file.o>")
        sys.exit(1)

    input_path = Path(sys.argv[1])

    # Run the inspector first so the parser can skip the heuristic scan when
    # the symbol table gives us reliable descriptor offsets.
    inspect_result = None
    try:
        import importlib.util as _ilu
        _insp_path = Path(__file__).with_name("eagl_inspect.py")
        _spec = _ilu.spec_from_file_location("eagl_inspect", _insp_path)
        _insp = _ilu.module_from_spec(_spec)
        _spec.loader.exec_module(_insp)
        inspect_result = _insp.inspect_o_file(input_path)
        print(f"[inspector] layout={inspect_result.predicted_layout}  "
              f"confidence={inspect_result.confidence}  "
              f"descriptors={[hex(o) for o in inspect_result.descriptor_offsets]}")
    except Exception as e:
        print(f"[inspector] unavailable ({e}), running heuristic scan only.")

    result = parse_o_file(input_path, inspect_result=inspect_result)
    print(result.summary())

    if result.ok:
        out_path = input_path.with_suffix(".glb")
        try:
            import importlib.util as _ilu
            _exp_path = Path(__file__).with_name("eagl_exporter.py")
            _spec = _ilu.spec_from_file_location("eagl_exporter", _exp_path)
            _exp  = _ilu.module_from_spec(_spec)
            _spec.loader.exec_module(_exp)

            glb_data = _exp.build_gltf(result)
            out_path.write_bytes(glb_data)
            print(f"✅ Successfully exported to {out_path}")
        except Exception as e:
            print(f"❌ Failed to build GLB: {e}")
