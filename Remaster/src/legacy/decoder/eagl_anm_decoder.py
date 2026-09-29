"""
eagl_anm_decoder.py
--------------------
Standalone decoder for EA Playground (Wii) proprietary `.anm` animation bank
files. This version REPLACES the old brute-force marker/quaternion-scanning
approach with a structural walk of `table_a`, following the confirmed
`ClipBlock` layout (sessions 15-19 + this session's `+4`/marker sub-pointer
findings). The old heuristic scanner is NOT trustworthy -- see the previous
docstring's own "OPEN QUESTIONS" section, which documented indistinguishable
frame-major/bone-major results and gaps that didn't correlate with bone
count. That was a symptom of anchoring on the wrong offsets. This version
anchors on real, self-relocated pointers instead.

INTERFACE (matches what eagl_exporter.py expects):

    from eagl_anm_decoder import AnmFile
    anm = AnmFile("player_anims.anm")
    for clip in anm.clips:
        clip.index          # int
        clip.name           # str | None
        clip.frame_times    # list[float]
        for track in clip.tracks:
            track.bone_idx      # int
            track.keyframes     # list[(x, y, z, w)]
            track.frame_times   # list[float] | None

STRUCTURALLY CONFIRMED THIS SESSION (re-derived directly against
player_anims.anm, not assumed -- see CLI report for live counts):

  table_a (AnimBank+0x10, bank_header.field1 == 265 entries) resolves
  265/265 via the self-relocation table -- matches session 15.

  Each table_a[i] is a ClipBlock:
    +0x00  4-byte container tag: 00 TT <family16>
             TT     = 0x0f (default container) | 0x16 (whole-clip FnStatelessQ)
             family = 0xc6e4 | 0x0606 | 0xe580 | 0xe664
           -- exact match to session 17's table (216/32/6/6/3/2 = 265).
    +0x04  self-relocated ptr -> bone/channel-index table:
             header = u32 BE(1), u16 BE(1), u16 BE(1), u8 count, 3 bytes pad
             (pad bytes are NOT always zero -- observed 0x30 0x30 0x30 on
             clip 0; treat as opaque, don't assume zero), then a strictly
             ascending run of u16 BE bone indices (< MAX_BONES) starting
             immediately after the 12-byte header.
    +0x0C  self-relocated ptr -> primary track marker (usually tag 0x12
           FnDeltaQFast, sometimes 0x13 FnDeltaSingleQ). NOT present in the
           normal marker form for the 12 whole-clip-0x16 clips -- for those,
           +0x0C instead points directly at a dense ascending-u8 list with
           no marker prefix (see below). Confirmed directly on a sampled
           0x16-container clip this session.
    +0x10  self-relocated ptr -> secondary track marker (usually 0x14
           FnDeltaF3, sometimes 0x15/0x17, or 0x12 for 9 unexplained
           clips) -- present even for the 12 whole-clip-0x16 clips
           (confirmed: sampled 0x16 clip's +0x10 resolved to a normal
           0x17/FnStatelessF3 marker).
    +0x20  embedded ascending u16 bone-index run (session 15) -- same
           table content-wise as +0x04's list (see below), just reached a
           different way.

  Track marker record (what +0x0C / +0x10 point to), decoded THIS SESSION:
    +0x00  4-byte marker: 00 TT <family16> (same TT vocabulary as the
           container: 0x12/0x13/0x14/0x15/0x17)
    +0x04  4 bytes, unexplained (not yet decoded -- flags/count?)
    +0x08  self-relocated ptr -> the SAME bone/channel-index list as
           ClipBlock+0x04 (verified byte-identical target offset on
           clip 0: both resolve to file-relative 4652 / abs 0x126c).
           This resolves session 15/16's open question: ClipBlock+0x04's
           list and the primary track's own bone-list pointer are the same
           table, not two independent lists.
    +0x0C  self-relocated ptr -> a DIFFERENT, denser ascending u8 list
           (values 0-~0xff, lengths roughly 20-55 vs the u16 list's ~9-174)
           -- confirmed on every sampled clip, not yet explained by
           anything in sessions 1-19. Exposed here as `dense_list_ptr_rel`
           but not otherwise interpreted.
    +0x10  2 bytes, unexplained (observed constant-ish "03 00" on samples)
    +0x12  keyframe/track payload starts here.

WHAT IS STILL HEURISTIC / UNRESOLVED:

  * The actual keyframe payload encoding at marker+0x12 is NOT re-derived
    from scratch this session. The multi-encoding unit-quaternion scanner
    from the previous version is kept, but is now seeded starting exactly
    at marker+0x12 (a real structural offset) instead of a 700-byte blind
    search window from a misaligned marker -- this should substantially
    improve precision, but the frame-major-vs-bone-major ambiguity noted
    in the previous docstring version was never resolved and is NOT
    re-tested here. Treat keyframe data as best-effort; check
    `track.confidence`.
  * The two unexplained fields at track-marker+0x04 (4 bytes) and +0x10
    (2 bytes), and the dense u8 list's purpose, are exposed on
    `TrackMarker`/`ClipBlock` but not interpreted.
  * The 9 clips where the secondary (+0x10) track resolves to 0x12
    (FnDeltaQFast) instead of a 3-float class are surfaced via
    `clip.diagnostics` but not specially handled.
"""

from __future__ import annotations

import struct
import math
import sys
from dataclasses import dataclass, field


# ---------------------------------------------------------------------------
# Tunables
# ---------------------------------------------------------------------------

MAX_BONES = 68
ASSUMED_FPS = 30.0
NORM_TOLERANCE = 0.06
MIN_TRACK_KEYFRAMES = 2

TRACK_TAG_NAMES = {
    0x11: "FnDeltaQ",
    0x12: "FnDeltaQFast",
    0x13: "FnDeltaSingleQ",
    0x14: "FnDeltaF3",
    0x15: "FnDeltaF1",
    0x16: "FnStatelessQ",
    0x17: "FnStatelessF3",
}


# ---------------------------------------------------------------------------
# ELF plumbing (unchanged -- verified against player_anims.anm's real layout)
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
        end = data.index(b"\x00", base + s["name_idx"])
        s["name"] = data[base + s["name_idx"]:end].decode("ascii", errors="replace")

    data_sec = next((s for s in sections if s["name"] == ".data"), sections[1])
    return sections, data_sec["offset"]


def _read_cstr(data: bytes, offset: int) -> str:
    end = data.index(b"\x00", offset)
    return data[offset:end].decode("ascii", errors="replace")


def _read_symbols(data: bytes, sections: list[dict]) -> list[dict]:
    sym_sec = next((s for s in sections if s["name"] == ".symtab"), None)
    str_sec = next((s for s in sections if s["name"] == ".strtab"), None)
    if not sym_sec or not str_sec:
        return []
    entry_size = sym_sec["entsize"] or 16
    str_base = str_sec["offset"]
    out = []
    for i in range(sym_sec["size"] // entry_size):
        o = sym_sec["offset"] + i * entry_size
        name_idx, value, size, info = struct.unpack_from("<IIIB", data, o)
        out.append({
            "name": _read_cstr(data, str_base + name_idx),
            "value": value, "size": size, "info": info,
        })
    return out


def _self_reloc_map(data: bytes, sections: list[dict], data_start: int) -> dict[int, int]:
    """
    {r_off: pointed_to_offset}, both relative to data_start. See prior
    docstring revision for the empirical justification (LE pointer field
    spliced into an otherwise BE payload at every self-relocated offset).
    """
    rel_sec = next((s for s in sections if s["name"] == ".rel.data"), None)
    if not rel_sec:
        return {}
    out: dict[int, int] = {}
    for i in range(rel_sec["size"] // 8):
        o = rel_sec["offset"] + i * 8
        r_off, r_info = struct.unpack_from("<II", data, o)
        if (r_info & 0xFF) != 2:
            continue
        try:
            val = struct.unpack_from("<I", data, data_start + r_off)[0]
        except struct.error:
            continue
        if val < len(data):
            out[r_off] = val
    return out


# ---------------------------------------------------------------------------
# Bank header / table_a
# ---------------------------------------------------------------------------

@dataclass
class _BankHeader:
    field0: int
    field1: int          # clip count (265 on player_anims.anm)
    table_a_off: int      # relative offset of table_a (self-reloc'd ptr array)
    table_b_off: int


def _find_bank(data: bytes, symbols: list[dict], data_start: int) -> int | None:
    sym = next((s for s in symbols if s["name"].startswith("__AnimationBank:::")), None)
    if sym is None:
        return None
    return data_start + sym["value"]


def _parse_bank_header(data: bytes, bank_off: int) -> _BankHeader | None:
    if bank_off is None or bank_off + 24 > len(data):
        return None
    field0, field1 = struct.unpack_from(">II", data, bank_off)
    table_a, table_b = struct.unpack_from("<II", data, bank_off + 16)
    return _BankHeader(field0, field1, table_a, table_b)


def _read_table_a(data: bytes, reloc: dict[int, int], bh: _BankHeader) -> list[int | None]:
    """Resolve all clip-count entries of table_a via self-relocation.
    Returns a list of relative ClipBlock offsets (None if unresolved)."""
    out = []
    for i in range(bh.field1):
        r = bh.table_a_off + i * 4
        out.append(reloc.get(r))
    return out


# ---------------------------------------------------------------------------
# Ascending index-list reader
# ---------------------------------------------------------------------------

def _read_ascending_run(data: bytes, off: int, width: int, max_val: int,
                         max_len: int) -> list[int]:
    """width=2 -> u16 BE run, width=1 -> u8 run. Strictly ascending, stops
    at first non-ascending or out-of-range value."""
    vals: list[int] = []
    prev = -1
    o = off
    fmt = ">H" if width == 2 else ">B"
    while len(vals) < max_len and o + width <= len(data):
        v = struct.unpack_from(fmt, data, o)[0]
        if v <= prev or v >= max_val:
            break
        vals.append(v)
        prev = v
        o += width
    return vals


def _read_bone_index_table(data: bytes, abs_off: int) -> tuple[dict, list[int]]:
    """Read the u32(1)/u16(1)/u16(1)/count/pad3 header + ascending u16 bone
    list immediately following it at abs_off."""
    hdr: dict = {}
    if abs_off + 12 > len(data):
        return hdr, []
    v1, v2, v3 = struct.unpack_from(">IHH", data, abs_off)
    count_byte = data[abs_off + 8]
    pad = data[abs_off + 9:abs_off + 12]
    hdr = {"v1": v1, "v2": v2, "v3": v3, "count_byte": count_byte, "pad": pad}
    bones = _read_ascending_run(data, abs_off + 12, width=2, max_val=MAX_BONES,
                                 max_len=max(count_byte, MAX_BONES))
    return hdr, bones


def _read_dense_u8_list(data: bytes, abs_off: int, max_len: int = 256) -> list[int]:
    return _read_ascending_run(data, abs_off, width=1, max_val=256, max_len=max_len)


# ---------------------------------------------------------------------------
# Track marker (what ClipBlock+0x0C / +0x10 point to)
# ---------------------------------------------------------------------------

@dataclass
class TrackMarker:
    abs_off: int
    tag: int                        # TT byte (0x12/0x13/0x14/0x15/0x17/...)
    family: int                     # 0xc6e4 / 0x0606 / 0xe580 / 0xe664
    class_name: str
    bone_list_ptr_rel: int | None   # marker+8, should match ClipBlock+4's list
    dense_list_ptr_rel: int | None  # marker+12
    payload_off: int                # abs offset where keyframe payload starts (marker+0x12)


def _parse_track_marker(data: bytes, reloc: dict[int, int], data_start: int,
                         marker_rel: int) -> TrackMarker | None:
    abs_off = data_start + marker_rel
    if abs_off + 0x12 > len(data):
        return None
    if data[abs_off] != 0x00:
        return None  # not a real marker (e.g. the 0x16-container +0xC case)
    tag = data[abs_off + 1]
    family = struct.unpack_from(">H", data, abs_off + 2)[0]
    # A leading 0x00 byte alone isn't sufficient evidence -- the dense u8
    # list for whole-clip (tag 0x16) containers also happens to start with
    # 0x00 (its first ascending value), which produced false-positive
    # "markers" with a bogus tag before this check was added. Real markers
    # only ever use the four known family suffixes (session 17/18).
    # 0x1414 occurs consistently in the RC car container and its QFast/F3
    # markers. These bytes are asset-dependent; the old player-only whitelist
    # rejected structurally valid markers before dispatching their codecs.
    if family not in (0xc6e4, 0x0606, 0xe580, 0xe664, 0x1414):
        return None
    if tag not in TRACK_TAG_NAMES:
        return None
    bone_ptr = reloc.get(marker_rel + 8)
    dense_ptr = reloc.get(marker_rel + 12)
    return TrackMarker(
        abs_off=abs_off, tag=tag, family=family,
        class_name=TRACK_TAG_NAMES.get(tag, f"Unknown_0x{tag:02x}"),
        bone_list_ptr_rel=bone_ptr, dense_list_ptr_rel=dense_ptr,
        payload_off=abs_off + 0x12,
    )


# ---------------------------------------------------------------------------
# ClipBlock
# ---------------------------------------------------------------------------

@dataclass
class ClipBlock:
    index: int
    rel_off: int
    abs_off: int
    container_tag: int             # 0x0f or 0x16
    family: int                    # 0xc6e4 / 0x0606 / 0xe580 / 0xe664
    bone_list_hdr: dict
    bones: list[int]
    primary: TrackMarker | None    # +0x0C, usually rotation (FnDeltaQFast)
    secondary: TrackMarker | None  # +0x10, usually translation (FnDeltaF3)
    whole_clip: bool               # True for the 12 container-tag-0x16 clips
    dense_list_direct: list[int]   # for whole_clip cases: +0x0C's raw dense list


def _parse_clip_block(data: bytes, reloc: dict[int, int], data_start: int,
                       index: int, rel_off: int) -> ClipBlock | None:
    abs_off = data_start + rel_off
    if abs_off + 0x20 > len(data):
        return None
    container_tag = data[abs_off + 1]
    family = struct.unpack_from(">H", data, abs_off + 2)[0]

    bones_ptr = reloc.get(rel_off + 4)
    bone_hdr, bones = ({}, [])
    if bones_ptr is not None:
        bone_hdr, bones = _read_bone_index_table(data, data_start + bones_ptr)

    whole_clip = container_tag == 0x16

    primary = None
    dense_list_direct: list[int] = []
    p_c = reloc.get(rel_off + 0x0C)
    if p_c is not None:
        marker = _parse_track_marker(data, reloc, data_start, p_c)
        if marker is not None:
            primary = marker
        else:
            # whole-clip case: +0x0C points straight at a dense u8 list,
            # no marker prefix.
            dense_list_direct = _read_dense_u8_list(data, data_start + p_c)

    secondary = None
    p_10 = reloc.get(rel_off + 0x10)
    if p_10 is not None:
        secondary = _parse_track_marker(data, reloc, data_start, p_10)

    return ClipBlock(
        index=index, rel_off=rel_off, abs_off=abs_off,
        container_tag=container_tag, family=family,
        bone_list_hdr=bone_hdr, bones=bones,
        primary=primary, secondary=secondary,
        whole_clip=whole_clip, dense_list_direct=dense_list_direct,
    )


# ---------------------------------------------------------------------------
# Clip name string table (unchanged)
# ---------------------------------------------------------------------------

import re

_NAME_RE = re.compile(rb"S_[ -~]{1,64}\x00")


def _find_clip_name_table(data: bytes, data_start: int) -> list[str]:
    matches = list(_NAME_RE.finditer(data, data_start))
    if not matches:
        return []
    names = []
    prev_end = None
    for m in matches:
        if prev_end is not None and m.start() - prev_end > 4:
            break
        names.append(m.group().rstrip(b"\x00").decode("ascii", errors="replace"))
        prev_end = m.end()
    return names


# ---------------------------------------------------------------------------
# Multi-encoding quaternion validator (kept from previous version, but now
# seeded from a real structural offset -- marker.payload_off -- instead of
# a blind multi-hundred-byte search window)
# ---------------------------------------------------------------------------

def _quat_norm_error(q: tuple[float, float, float, float]) -> float:
    n = math.sqrt(sum(c * c for c in q))
    return abs(n - 1.0)


def _try_f32(data: bytes, off: int) -> tuple[float, float, float, float] | None:
    if off + 16 > len(data):
        return None
    q = struct.unpack_from(">4f", data, off)
    if not all(math.isfinite(v) and abs(v) <= 1.2 for v in q):
        return None
    return q


def _try_s8(data: bytes, off: int) -> tuple[float, float, float, float] | None:
    if off + 4 > len(data):
        return None
    b = struct.unpack_from(">4b", data, off)
    return tuple(v / 127.0 for v in b)


def _try_s16(data: bytes, off: int) -> tuple[float, float, float, float] | None:
    if off + 8 > len(data):
        return None
    h = struct.unpack_from(">4h", data, off)
    return tuple(v / 32767.0 for v in h)


_ENCODINGS = (("f32", _try_f32), ("s8", _try_s8), ("s16", _try_s16))
_STRIDE_CANDIDATES = {
    "f32": (16, 20, 24, 28, 32),
    "s8":  (4, 8, 12, 16, 20, 24),
    "s16": (8, 12, 16, 20, 24),
}
# Structural anchor is now exact, so we only need a small nearby window to
# absorb the 4/6-byte unexplained fields already observed at marker+0x10.
_START_OFFSETS = range(0, 16, 2)
_MIN_VALID_RATIO = 0.6
_MAX_WALK = 512
_PROBE_LEN = 40


def _probe_ratio(data: bytes, decode, p: int, stride: int, end: int, n: int) -> tuple[float, int]:
    valid = 0
    count = 0
    while p + stride <= end and count < n:
        q = decode(data, p)
        count += 1
        if q is None:
            break
        err = _quat_norm_error(q)
        if err <= NORM_TOLERANCE:
            valid += 1
        elif err > 0.5:
            break
        p += stride
    if count == 0:
        return 0.0, 0
    return valid / count, count


def _scan_track_run(data: bytes, start: int, end: int
                     ) -> tuple[str, int, list[tuple[float, float, float, float]]] | None:
    best_probe = None
    for enc_name, decode in _ENCODINGS:
        for stride in _STRIDE_CANDIDATES[enc_name]:
            for start_off in _START_OFFSETS:
                p = start + start_off
                if p + stride > end:
                    continue
                ratio, count = _probe_ratio(data, decode, p, stride, end, _PROBE_LEN)
                if count < MIN_TRACK_KEYFRAMES or ratio < _MIN_VALID_RATIO:
                    continue
                if best_probe is None or (ratio, count) > (best_probe[0], best_probe[1]):
                    best_probe = (ratio, count, enc_name, stride, start_off)

    if best_probe is None:
        return None

    _, _, enc_name, stride, start_off = best_probe
    decode = dict(_ENCODINGS)[enc_name]
    p = start + start_off
    keyframes = []
    count = 0
    while p + stride <= end and count < _MAX_WALK:
        q = decode(data, p)
        count += 1
        if q is None:
            break
        err = _quat_norm_error(q)
        if err > NORM_TOLERANCE:
            break
        keyframes.append(q)
        p += stride
    if len(keyframes) < MIN_TRACK_KEYFRAMES:
        return None
    return enc_name, stride, keyframes


# ---------------------------------------------------------------------------
# Public data model (matches eagl_exporter.py expectations)
# ---------------------------------------------------------------------------

@dataclass
class Track:
    bone_idx: int
    keyframes: list[tuple[float, float, float, float]]
    frame_times: list[float] | None = None
    encoding: str = "unknown"
    confidence: float = 0.0
    role: str = "unknown"          # "rotation" | "translation" | "unknown"
    class_name: str = "unknown"    # FnDeltaQFast / FnDeltaF3 / ...


@dataclass
class Clip:
    index: int
    name: str | None
    tracks: list[Track] = field(default_factory=list)
    frame_times: list[float] = field(default_factory=list)
    whole_clip: bool = False
    diagnostics: list[str] = field(default_factory=list)


class AnmFile:
    """
    Parses an EA Playground (Wii) `.anm` animation bank using the confirmed
    `table_a` -> `ClipBlock` -> track-marker structural chain, rather than
    the old whole-file heuristic scan.

    anm = AnmFile("player_anims.anm")
    anm.clips           -> list[Clip]
    anm.report()         -> human-readable validation summary (str)
    anm.clip_blocks       -> list[ClipBlock]   (raw structural data, for
                             further reverse-engineering of the still-open
                             fields -- dense u8 lists, unexplained marker
                             bytes, etc.)
    """

    def __init__(self, path: str):
        self.path = path
        with open(path, "rb") as f:
            self.data = f.read()

        self.sections, self.data_start = _read_sections(self.data)
        self.symbols = _read_symbols(self.data, self.sections)
        self.reloc_map = _self_reloc_map(self.data, self.sections, self.data_start)

        bank_off = _find_bank(self.data, self.symbols, self.data_start)
        self.bank_header = _parse_bank_header(self.data, bank_off) if bank_off else None

        self.clip_names = _find_clip_name_table(self.data, self.data_start)

        self.clip_blocks: list[ClipBlock] = []
        self.clips: list[Clip] = []
        self._diagnostics: list[str] = []
        self._parse()

    # -- internal ------------------------------------------------------

    def _parse(self) -> None:
        data = self.data
        data_start = self.data_start

        if self.bank_header is None:
            self._diagnostics.append("no bank header found -- cannot locate table_a.")
            return

        table_a = _read_table_a(data, self.reloc_map, self.bank_header)
        n_missing = sum(1 for x in table_a if x is None)
        self._diagnostics.append(
            f"table_a: {len(table_a)} entries declared, {n_missing} unresolved"
        )

        for i, rel_off in enumerate(table_a):
            if rel_off is None:
                continue
            cb = _parse_clip_block(data, self.reloc_map, data_start, i, rel_off)
            if cb is None:
                continue
            self.clip_blocks.append(cb)

        # summary counts, mirroring session 17/18's cross-check tables
        n_whole = sum(1 for cb in self.clip_blocks if cb.whole_clip)
        n_primary_marker = sum(1 for cb in self.clip_blocks if cb.primary is not None)
        n_secondary_marker = sum(1 for cb in self.clip_blocks if cb.secondary is not None)
        self._diagnostics.append(
            f"clip blocks parsed: {len(self.clip_blocks)}; "
            f"whole-clip (container tag 0x16): {n_whole}; "
            f"primary(+0xC) markers resolved: {n_primary_marker}; "
            f"secondary(+0x10) markers resolved: {n_secondary_marker}"
        )

        for cb in self.clip_blocks:
            clip = self._build_clip(cb)
            self.clips.append(clip)

        self._diagnostics.append(f"clips assembled: {len(self.clips)}")

    def _build_clip(self, cb: ClipBlock) -> Clip:
        diagnostics: list[str] = []
        name = self.clip_names[cb.index] if cb.index < len(self.clip_names) else None
        tracks: list[Track] = []

        n_bones = len(cb.bones)
        role_markers = []
        if cb.primary is not None:
            role_markers.append(("rotation", cb.primary))
        elif cb.whole_clip:
            diagnostics.append(
                "whole-clip container (tag 0x16): no primary marker at +0xC "
                "(dense u8 list found there directly instead, "
                f"len={len(cb.dense_list_direct)})"
            )
        if cb.secondary is not None:
            role = "translation"
            if cb.secondary.tag == 0x12:
                role = "rotation?"  # the 9 unexplained FnDeltaQFast-as-secondary cases
                diagnostics.append(
                    "secondary(+0x10) marker resolved to FnDeltaQFast (0x12), "
                    "not a 3-float class -- unexplained case (session 18 OPEN #3)."
                )
            role_markers.append((role, cb.secondary))

        for role, marker in role_markers:
            found = _scan_track_run(self.data, marker.payload_off,
                                     min(marker.payload_off + 8192, len(self.data)))
            if found is None:
                diagnostics.append(
                    f"{role} track ({marker.class_name}) at 0x{marker.abs_off:x}: "
                    f"no validated keyframe run found at payload_off=0x{marker.payload_off:x}"
                )
                continue
            enc_name, stride, keyframes = found
            bones_for_track = cb.bones if n_bones > 0 else [0]
            n_frames = max(1, len(keyframes) // max(1, len(bones_for_track)))
            frame_times = [j / ASSUMED_FPS for j in range(n_frames)]
            n_ok = sum(1 for q in keyframes if _quat_norm_error(q) <= NORM_TOLERANCE)
            conf = n_ok / len(keyframes) if keyframes else 0.0

            if len(bones_for_track) <= 1:
                bone_idx = bones_for_track[0] if bones_for_track else 0
                tracks.append(Track(
                    bone_idx=bone_idx, keyframes=keyframes[:n_frames],
                    frame_times=frame_times[:len(keyframes[:n_frames])],
                    encoding=enc_name, confidence=conf,
                    role=role, class_name=marker.class_name,
                ))
            else:
                for b_i, bone_idx in enumerate(bones_for_track):
                    track_kf = keyframes[b_i::len(bones_for_track)][:n_frames]
                    if len(track_kf) < MIN_TRACK_KEYFRAMES:
                        continue
                    tracks.append(Track(
                        bone_idx=bone_idx, keyframes=track_kf,
                        frame_times=frame_times[:len(track_kf)],
                        encoding=enc_name, confidence=conf,
                        role=role, class_name=marker.class_name,
                    ))

        frame_times = max((t.frame_times or [] for t in tracks), key=len, default=[])
        return Clip(
            index=cb.index, name=name, tracks=tracks,
            frame_times=frame_times, whole_clip=cb.whole_clip,
            diagnostics=diagnostics,
        )

    # -- public helpers --------------------------------------------------

    def report(self) -> str:
        lines = [f"AnmFile({self.path!r})"]
        lines.append(f"  sections: {[s['name'] for s in self.sections]}")
        lines.append(f"  self-relocations: {len(self.reloc_map)}")
        if self.bank_header:
            bh = self.bank_header
            lines.append(
                f"  bank header: field0={bh.field0} field1={bh.field1} "
                f"table_a=0x{bh.table_a_off:x} table_b=0x{bh.table_b_off:x}"
            )
        else:
            lines.append("  bank header: not found")
        lines.extend(f"  {d}" for d in self._diagnostics)

        from collections import Counter
        fam_counts = Counter((cb.container_tag, cb.family) for cb in self.clip_blocks)
        lines.append("  container tag/family breakdown:")
        for (tag, fam), n in sorted(fam_counts.items()):
            lines.append(f"    tag=0x{tag:02x} family=0x{fam:04x}: {n}")

        primary_tags = Counter(cb.primary.tag for cb in self.clip_blocks if cb.primary)
        secondary_tags = Counter(cb.secondary.tag for cb in self.clip_blocks if cb.secondary)
        lines.append(f"  primary(+0xC) track tags: "
                     f"{ {TRACK_TAG_NAMES.get(t, hex(t)): n for t, n in primary_tags.items()} }")
        lines.append(f"  secondary(+0x10) track tags: "
                     f"{ {TRACK_TAG_NAMES.get(t, hex(t)): n for t, n in secondary_tags.items()} }")

        for c in self.clips[:1000]:
            n_kf = sum(len(t.keyframes) for t in c.tracks)
            encs = sorted({t.encoding for t in c.tracks})
            avg_conf = (sum(t.confidence for t in c.tracks) / len(c.tracks)
                        if c.tracks else 0.0)
            wc = " [whole-clip]" if c.whole_clip else ""
            lines.append(
                f"    clip {c.index:03d} {c.name!r}{wc}: {len(c.tracks)} tracks, "
                f"{n_kf} total keyframes, frames={len(c.frame_times)}, "
                f"encodings={encs}, avg_confidence={avg_conf:.2f}"
                + (f"  !! {'; '.join(c.diagnostics)}" if c.diagnostics else "")
            )
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: python eagl_anm_decoder.py <path/to/file.anm>")
        raise SystemExit(1)
    anm = AnmFile(sys.argv[1])
    print(anm.report())
