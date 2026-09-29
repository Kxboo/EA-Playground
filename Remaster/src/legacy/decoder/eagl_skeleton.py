"""
eagl_skeleton.py
----------------
Parser for EA Sports EAGL Wii .ske skeleton files.

Companion to parser.py / eagl_inspect.py / eagl_exporter.py — reuses
the same ELF helpers (_read_sections, _read_cstr, _build_reloc_map) so
the four files stay structurally consistent.

Key format facts (from eagl_skeleton_anim_notes.md):
  - ELF32, MIPS R3000, single .data section.
  - NO .rel.data section — bone references are plain integer indices.
    _build_reloc_map() correctly returns {} here; don't treat as error.
  - 68 bones: 68x __Bone:::Root.<name> symbols + 1x __Skeleton:::Root.
  - Symbol size field = 4 (tags first word only).

.data layout:
  0x000 – 0x42F   Bone index lookup table   (68 × 16 bytes)
  0x430           Last bone slot (unnamed, index 67)
  0x440 – 0x45F   Skeleton header           (32 bytes, __Skeleton:::Root)
  0x460 – 0x21FF  Per-bone records          (68 × 112 bytes)

Per-bone record (112 bytes = 28 × f32 BE):
  [0:3]   f32x3  scale
  [3]     i32    parent_idx   (-1 = root)
  [4:8]   f32x4  quaternion   (x, y, z, w) — unit length, LOCAL (parent-relative) rotation
  [8:11]  f32x3  local translation (parent-relative bind-pose offset) — see note below
  [11]    f32    pad (0.0)
  [12:24] f32x12 4×4 row-major rotation matrix (rows 0-2; each 4th f = 0)
                 — this is a CACHED WORLD-space rotation, derived from the
                   local quaternion chain (world[i] = world[parent] @ local(quat[i])).
                   Redundant with `quaternion`; do not use directly, it is
                   exposed for diagnostics only.
  [24:27] f32x3  UNRELIABLE — originally documented as "world-space bind-pose
                 translation" but this does not hold up: treating it as flat
                 world-space position produces anatomically impossible bone
                 lengths for ~30% of bones (e.g. a 1.7-unit eyebrow bone) and
                 broken left/right mirror symmetry. Exact meaning unresolved;
                 kept for diagnostics only. DO NOT use for posing/export.
  [27]    f32    homogeneous row terminator (1.0)

  NOTE — verified bind-pose formula (see BoneRecord docstring for the full
  derivation): the field at [8:11] (originally logged as "unknown vec3") is
  the real LOCAL bone-to-parent translation. Correct world-space position is:
      world_pos[i] = world_pos[parent] + world_matrix[parent] @ local_pos[i]
  This reconstruction produces zero anatomically-implausible bone lengths
  (vs 20/67 under the old [24:27]-as-world reading), exact left/right
  mirror symmetry to 4 decimal places, and a monotonic head-to-toe height
  progression. Use BoneRecord.world_translation / .world_matrix rather than
  the raw `translation` / `matrix` fields.

Usage (standalone):
    from eagl_skeleton import parse_ske_file
    result = parse_ske_file("player_skel.ske")
    print(result.summary())

Usage (integrated with inspect + parser toolchain):
    from eagl_skeleton import parse_ske_file, build_skeleton_gltf
    skel = parse_ske_file("player_skel.ske")
    glb  = build_skeleton_gltf(skel)   # visualise armature as glTF nodes
    open("player_skel.glb", "wb").write(glb)
"""

from __future__ import annotations

import struct
import math
import json
from dataclasses import dataclass, field
from pathlib import Path


# ---------------------------------------------------------------------------
# Minimal 3x3 rotation math (row-major, flat 9-tuples) — used to compose the
# verified local-to-world bone transform. No numpy dependency by design, to
# match the rest of this module's standalone/duck-typed style.
# ---------------------------------------------------------------------------

def _quat_to_matrix(x: float, y: float, z: float, w: float) -> tuple[float, ...]:
    """Unit quaternion (x,y,z,w) -> row-major 3x3 rotation matrix (9-tuple)."""
    xx, yy, zz = x*x, y*y, z*z
    xy, xz, yz = x*y, x*z, y*z
    wx, wy, wz = w*x, w*y, w*z
    return (
        1 - 2*(yy+zz),     2*(xy-wz),     2*(xz+wy),
            2*(xy+wz), 1 - 2*(xx+zz),     2*(yz-wx),
            2*(xz-wy),     2*(yz+wx), 1 - 2*(xx+yy),
    )


def _mat3_mul(a: tuple[float, ...], b: tuple[float, ...]) -> tuple[float, ...]:
    """Row-major 3x3 matrix multiply: returns a @ b."""
    return (
        a[0]*b[0]+a[1]*b[3]+a[2]*b[6],  a[0]*b[1]+a[1]*b[4]+a[2]*b[7],  a[0]*b[2]+a[1]*b[5]+a[2]*b[8],
        a[3]*b[0]+a[4]*b[3]+a[5]*b[6],  a[3]*b[1]+a[4]*b[4]+a[5]*b[7],  a[3]*b[2]+a[4]*b[5]+a[5]*b[8],
        a[6]*b[0]+a[7]*b[3]+a[8]*b[6],  a[6]*b[1]+a[7]*b[4]+a[8]*b[7],  a[6]*b[2]+a[7]*b[5]+a[8]*b[8],
    )


def _mat3_vec_mul(m: tuple[float, ...], v: tuple[float, float, float]) -> tuple[float, float, float]:
    """Row-major 3x3 matrix * column vector."""
    return (
        m[0]*v[0] + m[1]*v[1] + m[2]*v[2],
        m[3]*v[0] + m[4]*v[1] + m[5]*v[2],
        m[6]*v[0] + m[7]*v[1] + m[8]*v[2],
    )


# ---------------------------------------------------------------------------
# Public data classes
# ---------------------------------------------------------------------------

@dataclass
class BoneRecord:
    """
    One bone's full bind-pose data, as decoded from the .ske per-bone table.

    --------------------------------------------------------------------
    CORRECTED FIELD SEMANTICS (superseding the original reverse-engineering
    notes in eagl_skeleton_anim_notes.md) — verified by:
      (1) composing world_matrix[i] = world_matrix[parent] @ local_matrix(quat[i])
          up the parent chain and finding an EXACT (0.00000) match against the
          stored `matrix` field for every bone tested, proving `matrix` is a
          cached WORLD rotation derived from the LOCAL `quaternion`, not an
          independently-meaningful value;
      (2) the field documented as "world-space translation" (`translation`,
          bytes [24:27]) produces anatomically impossible bone lengths for
          20/67 bones (e.g. a 1.7-unit eyebrow bone) and is NOT reliable;
      (3) the field previously labeled `unknown_vec` (bytes [8:11]) is the
          real LOCAL (parent-relative) bone offset — using it with the
          standard local-to-world transform
              world_pos[i] = world_pos[parent] + world_matrix[parent] @ unknown_vec[i]
          produces a skeleton with zero anatomically-implausible bone
          lengths (vs 20/67 before), EXACT left/right mirror symmetry to
          4 decimal places on every limb pair, and a monotonic head-to-toe
          Y-axis height progression — all of which only hold under this
          interpretation.

    Practical upshot: `quaternion` and `unknown_vec` are the ground-truth
    LOCAL (parent-relative) rotation/translation. `matrix` and `translation`
    are retained for diagnostic/back-compat purposes but should NOT be used
    for posing, export, or skinning — use the `world_translation` /
    `world_matrix` properties (or `local_translation`) instead.
    --------------------------------------------------------------------
    """

    index:       int
    name:        str                          # e.g. "l_arm", "Root", "BONE"

    scale:       tuple[float, float, float]   # almost always (1,1,1)
    parent_idx:  int                          # -1 for root
    quaternion:  tuple[float, float, float, float]  # (x, y, z, w) — LOCAL rotation, verified
    unknown_vec: tuple[float, float, float]   # LOCAL translation (parent-relative), verified
    matrix:      tuple[float, ...]            # 12 floats — cached WORLD rotation (redundant; do not use directly)
    translation: tuple[float, float, float]   # UNRELIABLE — do not use; kept for diagnostics only

    # Populated by _link_skeleton() after all bones are decoded — lets each
    # bone resolve its own world transform without needing the full
    # SkeletonResult passed around everywhere.
    _skeleton: "SkeletonResult | None" = field(default=None, repr=False, compare=False)

    # Memoization slots for the (recursive) world-space properties below —
    # avoids O(2^depth) blowup if a UI repeatedly queries world transforms.
    _world_matrix_cache:      "tuple[float, ...] | None" = field(default=None, repr=False, compare=False)
    _world_translation_cache: "tuple[float, float, float] | None" = field(default=None, repr=False, compare=False)

    @property
    def is_root(self) -> bool:
        return self.parent_idx == -1

    @property
    def quat_magnitude(self) -> float:
        x, y, z, w = self.quaternion
        return math.sqrt(x*x + y*y + z*z + w*w)

    @property
    def local_translation(self) -> tuple[float, float, float]:
        """The verified local (parent-relative) bone offset. Alias of unknown_vec."""
        return self.unknown_vec

    @property
    def local_matrix(self) -> tuple[float, ...]:
        """3x3 row-major rotation matrix derived from the local quaternion."""
        return _quat_to_matrix(*self.quaternion)

    @property
    def world_matrix(self) -> tuple[float, ...]:
        """
        3x3 row-major WORLD rotation matrix, composed from the local
        quaternion chain. Matches the stored `matrix` field exactly
        (verified) — provided as a computed property so it doesn't depend
        on trusting the raw bytes.
        """
        if self._world_matrix_cache is not None:
            return self._world_matrix_cache
        if self._skeleton is None or self.is_root:
            result = self.local_matrix
        else:
            parent = self._skeleton.bones[self.parent_idx]
            result = _mat3_mul(parent.world_matrix, self.local_matrix)
        self._world_matrix_cache = result
        return result

    @property
    def world_translation(self) -> tuple[float, float, float]:
        """
        Correct world-space bind-pose position, computed as:
            world_pos = parent.world_pos + parent.world_matrix @ local_translation
        This is the value to use for skinning, export, and any spatial query
        — NOT the raw `translation` field.
        """
        if self._world_translation_cache is not None:
            return self._world_translation_cache
        if self._skeleton is None or self.is_root:
            result = self.local_translation
        else:
            parent = self._skeleton.bones[self.parent_idx]
            pwx, pwy, pwz = parent.world_translation
            rx, ry, rz = _mat3_vec_mul(parent.world_matrix, self.local_translation)
            result = (pwx + rx, pwy + ry, pwz + rz)
        self._world_translation_cache = result
        return result

    def summary(self) -> str:
        par = f"parent={self.parent_idx}" if not self.is_root else "ROOT"
        tx, ty, tz = self.world_translation
        return (f"  [{self.index:>2}] {self.name:<24}  {par:<12}  "
                f"world=({tx:+.4f}, {ty:+.4f}, {tz:+.4f})")


@dataclass
class SkeletonHeader:
    """Decoded __Skeleton:::Root symbol header."""
    flags:      int    # 0x060602a8 — meaning not yet cracked
    bone_count: int    # confirmed matches symbol count
    root_scale: tuple[float, float, float]
    root_parent_idx: int   # -1


@dataclass
class SkeletonResult:
    """Top-level result returned by parse_ske_file()."""

    file_path:  Path
    file_size:  int

    header:     SkeletonHeader | None
    bones:      list[BoneRecord]

    log:        list[str] = field(default_factory=list)

    def __post_init__(self):
        # Give every bone a back-reference so BoneRecord.world_translation /
        # .world_matrix can walk the parent chain without external plumbing.
        for b in self.bones:
            b._skeleton = self
        # Build O(1) children lookup — avoids O(n_bones) scan per children_of call
        self._children: dict[int, list[BoneRecord]] = {b.index: [] for b in self.bones}
        for b in self.bones:
            if b.parent_idx >= 0:
                self._children[b.parent_idx].append(b)

    # ── Convenience ──────────────────────────────────────────────────────

    @property
    def ok(self) -> bool:
        return len(self.bones) > 0

    @property
    def bone_count(self) -> int:
        return len(self.bones)

    def bone_by_name(self, name: str) -> BoneRecord | None:
        for b in self.bones:
            if b.name == name:
                return b
        return None

    def children_of(self, idx: int) -> list[BoneRecord]:
        return self._children.get(idx, [])

    def chain_to_root(self, idx: int) -> list[BoneRecord]:
        """Walk parent links from bone *idx* up to root. Returns [leaf, ..., root]."""
        visited = set()
        chain   = []
        cur     = idx
        while 0 <= cur < len(self.bones) and cur not in visited:
            visited.add(cur)
            chain.append(self.bones[cur])
            cur = self.bones[cur].parent_idx
        return chain

    def summary(self) -> str:
        lines = self.log[:]
        lines.append(f"\nSkeleton: {self.file_path.name}  ({self.file_size:,} bytes)")
        if self.header:
            h = self.header
            lines.append(f"  bone_count={h.bone_count}  flags=0x{h.flags:08x}  "
                         f"root_scale={h.root_scale}  root_parent={h.root_parent_idx}")
        lines.append(f"  Bones parsed: {self.bone_count}")
        for b in self.bones:
            lines.append(b.summary())
        return "\n".join(lines)

    def hierarchy_str(self) -> str:
        """Return an indented tree of the bone hierarchy."""
        lines: list[str] = []
        def _walk(idx: int, depth: int) -> None:
            b = self.bones[idx]
            lines.append("  " + "  " * depth + f"[{idx}] {b.name}")
            for child in self.children_of(idx):
                _walk(child.index, depth + 1)
        roots = [b for b in self.bones if b.is_root]
        for r in roots:
            _walk(r.index, 0)
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# ELF helpers — mirrors parser.py exactly so both files stay in sync
# ---------------------------------------------------------------------------

def _read_sections(data: bytes) -> tuple[list[dict], int]:
    """Return (sections, data_section_file_offset)."""
    e_shoff     = struct.unpack_from("<I", data, 32)[0]
    e_shentsize = struct.unpack_from("<H", data, 46)[0]
    e_shnum     = struct.unpack_from("<H", data, 48)[0]
    e_shstrndx  = struct.unpack_from("<H", data, 50)[0]

    sections = []
    for i in range(e_shnum):
        o  = e_shoff + i * e_shentsize
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

    # data_start = file offset of .data section (section index 1 in EAGL files)
    data_start = sections[1]["offset"] if len(sections) > 1 else 0
    return sections, data_start


def _read_cstr(data: bytes, offset: int) -> str:
    end = data.index(b'\x00', offset)
    return data[offset:end].decode("ascii", errors="replace")


def _build_reloc_map(data: bytes, sections: list[dict], data_start: int) -> dict[int, int]:
    """R_MIPS_32 reloc table → {r_offset: resolved_value}.  Returns {} if no .rel.data."""
    rel_sec = next((s for s in sections if s["name"] == ".rel.data"), None)
    if not rel_sec:
        return {}
    relocs: dict[int, int] = {}
    for i in range(rel_sec["size"] // 8):
        o = rel_sec["offset"] + i * 8
        r_off, r_info = struct.unpack_from("<II", data, o)
        if (r_info & 0xFF) == 2:   # R_MIPS_32
            relocs[r_off] = struct.unpack_from("<I", data, data_start + r_off)[0]
    return relocs


# ---------------------------------------------------------------------------
# Symbol-table helpers
# ---------------------------------------------------------------------------

def _parse_bone_symbols(data: bytes, sections: list[dict]
                        ) -> tuple[dict[int, str], int | None]:
    """
    Walk the symbol table to build:
      bone_names  — {sequential_index: bone_name_str}
      skel_offset — .data offset of __Skeleton:::Root symbol (or None)

    Bone index is derived from symbol.value / 0x10 (the index table slot size).
    """
    sym_sec = next((s for s in sections if s["name"] == ".symtab"), None)
    str_sec = next((s for s in sections if s["name"] == ".strtab"), None)
    if not sym_sec or not str_sec:
        return {}, None

    bone_names:  dict[int, str] = {}
    skel_offset: int | None     = None

    str_base   = str_sec["offset"]
    entry_size = sym_sec["entsize"] or 16

    for i in range(sym_sec["size"] // entry_size):
        o = sym_sec["offset"] + i * entry_size
        name_idx, sym_val = struct.unpack_from("<II", data, o)
        name = _read_cstr(data, str_base + name_idx)

        if name.startswith("__Bone:::Root."):
            # e.g. "__Bone:::Root.l_arm"  — suffix after the last '.' is the bone name
            bone_name = name.split(".")[-1]
            bone_idx  = sym_val // 0x10      # each index-table slot is 16 bytes
            bone_names[bone_idx] = bone_name

        elif name.startswith("__Skeleton:::Root"):
            skel_offset = sym_val

    return bone_names, skel_offset


# ---------------------------------------------------------------------------
# Skeleton header decoder
# ---------------------------------------------------------------------------

_SKE_HEADER_SIZE = 16   # bytes — only 4 words; bone table starts at 0x450

def _decode_skeleton_header(data: bytes, data_start: int,
                             skel_offset: int) -> SkeletonHeader:
    """
    Decode the 16-byte __Skeleton:::Root header at data_start + skel_offset.

    Confirmed layout (from raw dump):
      +0x00  u32 BE  flags       (0x060602a8 — meaning unknown)
      +0x04  u32 BE  0           (pad)
      +0x08  u32 BE  bone_count  (confirmed = 68)
      +0x0C  u32 BE  0           (pad)

    The root_scale (1,1,1) and root_parent (-1) that the notes mention
    live at +0x10..+0x1C are actually the FIRST BONE's scale+parent
    fields (bone 0 = Root), not part of the header.  The header is
    exactly 16 bytes; the per-bone table starts immediately after at 0x450.
    """
    base = data_start + skel_offset
    flags,      = struct.unpack_from(">I", data, base + 0x00)
    bone_count, = struct.unpack_from(">I", data, base + 0x08)
    return SkeletonHeader(
        flags      = flags,
        bone_count = bone_count,
        root_scale = (1.0, 1.0, 1.0),   # from bone 0 — read during bone decode
        root_parent_idx = -1,            # from bone 0 — read during bone decode
    )


# ---------------------------------------------------------------------------
# Per-bone record decoder
# ---------------------------------------------------------------------------

# The per-bone table starts at data-relative offset 0x460 (= 0x450 + 16-byte
# overlap tail from the skeleton header, confirmed by 68/68 scale-marker test).
# Each record is 112 bytes = 28 BE f32 values.

_BONE_TABLE_OFFSET = 0x450   # confirmed: header at 0x440 is 16 bytes only
_BONE_RECORD_SIZE  = 112
_BONE_FLOATS       = 28    # 112 / 4


def _decode_bone(data: bytes, data_start: int, bone_idx: int,
                 bone_name: str, table_offset: int = _BONE_TABLE_OFFSET) -> BoneRecord:
    """Decode a single 112-byte per-bone record at its computed offset."""
    base = data_start + table_offset + bone_idx * _BONE_RECORD_SIZE

    # Read all 28 floats; [3] is actually an i32 (parent_idx) — re-read below.
    floats = struct.unpack_from(f">28f", data, base)

    scale      = (floats[0], floats[1], floats[2])
    parent_idx = struct.unpack_from(">i", data, base + 3 * 4)[0]   # re-read as signed int
    quaternion = (floats[4], floats[5], floats[6], floats[7])       # x,y,z,w — LOCAL rotation (verified)
    unknown    = (floats[8], floats[9], floats[10])                 # LOCAL translation (verified — see BoneRecord docstring)
    # floats[11] = pad
    matrix     = floats[12:24]   # 12 floats = rows 0-2 of the 4×4 — cached WORLD rotation, redundant (do not use directly)
    translation = (floats[24], floats[25], floats[26])              # UNRELIABLE — see BoneRecord docstring; use world_translation instead
    # floats[27] = 1.0 (homogeneous)

    return BoneRecord(
        index       = bone_idx,
        name        = bone_name,
        scale       = scale,
        parent_idx  = parent_idx,
        quaternion  = quaternion,
        unknown_vec = unknown,
        matrix      = matrix,
        translation = translation,
    )


# ---------------------------------------------------------------------------
# Validation helpers
# ---------------------------------------------------------------------------

def _validate_bones(bones: list[BoneRecord], log: list[str]) -> None:
    """Sanity-check bones and append warnings/confirmations to log."""
    bad_quat   = 0
    bad_parent = 0
    roots      = []

    for b in bones:
        mag = b.quat_magnitude
        if not (0.999 <= mag <= 1.001):
            bad_quat += 1
            log.append(f"  WARN bone {b.index} ({b.name}): quat magnitude {mag:.4f} ≠ 1")

        if b.parent_idx < -1 or b.parent_idx >= len(bones):
            bad_parent += 1
            log.append(f"  WARN bone {b.index} ({b.name}): parent_idx {b.parent_idx} out of range")

        if b.parent_idx == -1:
            roots.append(b.index)

    if bad_quat == 0:
        log.append(f"  ✓ All {len(bones)} quaternions are unit length.")
    else:
        log.append(f"  ✗ {bad_quat}/{len(bones)} bones have non-unit quaternions.")

    if bad_parent == 0:
        log.append(f"  ✓ All parent indices in range.")
    else:
        log.append(f"  ✗ {bad_parent} bones have invalid parent indices.")

    log.append(f"  Root bone(s): {roots}  ({len(roots)} root{'s' if len(roots) != 1 else ''})")

    # ── Stored `matrix` field vs. composed quaternion-chain world matrix ──
    # This is the check that originally proved `matrix` is a cached WORLD
    # rotation derived from the LOCAL `quaternion` field — re-run on every
    # parse as a standing regression guard, since both `matrix` and
    # `quaternion` are read independently from the file.
    matrix_mismatches = 0
    for b in bones:
        composed = b.world_matrix
        stored   = b.matrix[0:3] + b.matrix[4:7] + b.matrix[8:11]
        diff = sum(abs(a - c) for a, c in zip(composed, stored))
        if diff > 0.01:
            matrix_mismatches += 1
    if bones:
        if matrix_mismatches == 0:
            log.append(f"  ✓ Composed world rotation matches the stored matrix field "
                       f"for all {len(bones)} bones (quaternion-chain composition verified).")
        else:
            log.append(f"  WARN {matrix_mismatches}/{len(bones)} bones: composed world rotation "
                       f"diverges from the stored matrix field — check parent links / quaternions.")

    # ── Bone-length sanity using the verified world_translation formula ──
    # Flags segments that are anatomically implausible (catches the same
    # class of error the old "translation field" misinterpretation produced
    # — 20/67 bones were flagged before this fix; should be 0 on good data).
    long_bones = []
    for b in bones:
        if b.is_root:
            continue
        parent = next((p for p in bones if p.index == b.parent_idx), None)
        if parent is None:
            continue
        wb, wp = b.world_translation, parent.world_translation
        length = math.sqrt(sum((wb[k]-wp[k])**2 for k in range(3)))
        if length > 0.5:
            long_bones.append((b.name, parent.name, length))

    if not long_bones:
        log.append(f"  ✓ All bone segment lengths are anatomically plausible "
                   f"(< 0.5 units, using verified local-offset transform).")
    else:
        log.append(f"  WARN {len(long_bones)} bone segment(s) exceed 0.5 units — "
                   f"possible decode issue:")
        for name, pname, length in long_bones[:8]:
            log.append(f"    {name} <- {pname}: {length:.4f}")


# ---------------------------------------------------------------------------
# Main parse entry point
# ---------------------------------------------------------------------------

def parse_ske_file(path: str | Path) -> SkeletonResult:
    """
    Parse an EAGL .ske skeleton file and return a SkeletonResult.

    The function re-uses _read_sections / _read_cstr / _build_reloc_map
    from this module (mirrors of parser.py) so it can stand alone or be
    imported alongside the mesh parser with no conflicts.
    """
    path = Path(path)
    data = path.read_bytes()
    log: list[str] = []

    if data[:4] != b"\x7fELF":
        return SkeletonResult(path, len(data), None, [],
                              log=["Not an ELF file."])
    log.append(f"ELF OK  ({len(data):,} bytes)")

    sections, data_start = _read_sections(data)
    data_sec = next((s for s in sections if s["name"] == ".data"), None)
    if not data_sec:
        return SkeletonResult(path, len(data), None, [],
                              log=log + ["No .data section."])

    log.append(f".data @ 0x{data_start:04x}  size 0x{data_sec['size']:04x}")

    # .ske has NO .rel.data — this is expected, not an error
    relocs = _build_reloc_map(data, sections, data_start)
    if relocs:
        log.append(f"Reloc entries: {len(relocs)}  (unexpected — .ske should have none)")
    else:
        log.append("No .rel.data section — expected for .ske files ✓")

    # ── Symbol table: bone names + skeleton header offset ──────────────────
    bone_names, skel_offset = _parse_bone_symbols(data, sections)
    log.append(f"Bone names from symbol table: {len(bone_names)}")
    if skel_offset is not None:
        log.append(f"__Skeleton:::Root at .data+0x{skel_offset:04x}")
    else:
        raise ValueError('Skeleton symbol missing; refusing player-specific offset fallback')

    # ── Skeleton header ────────────────────────────────────────────────────
    header = _decode_skeleton_header(data, data_start, skel_offset)
    log.append(f"Skeleton header: bone_count={header.bone_count}  "
               f"flags=0x{header.flags:08x}  root_parent={header.root_parent_idx}")

    bone_count = header.bone_count
    if bone_count == 0:
        log.append("WARN: bone_count=0 in header — attempting to derive from symbol count")
        bone_count = len(bone_names)

    # ── Per-bone records ───────────────────────────────────────────────────
    table_offset = skel_offset + _SKE_HEADER_SIZE
    expected_end = data_start + table_offset + bone_count * _BONE_RECORD_SIZE
    if bone_count <= 0 or expected_end > data_start + data_sec['size']:
        raise ValueError('Declared bone table does not fit the .data section')

    log.append(f"Parsing {bone_count} bone records @ .data+0x{table_offset:04x}  "
               f"(stride {_BONE_RECORD_SIZE} bytes)")

    bones: list[BoneRecord] = []
    for i in range(bone_count):
        name = bone_names.get(i, f"bone_{i}")
        bones.append(_decode_bone(data, data_start, i, name, table_offset))

    for bone in bones:
        if bone.parent_idx < -1 or bone.parent_idx >= len(bones):
            raise ValueError(f'Bone {bone.index}: invalid parent {bone.parent_idx}')
        seen = set()
        parent = bone.index
        while parent != -1:
            if parent in seen:raise ValueError(f'Cyclic skeleton at bone {bone.index}')
            seen.add(parent)
            parent = bones[parent].parent_idx

    result = SkeletonResult(
        file_path = path,
        file_size = len(data),
        header    = header,
        bones     = bones,
        log       = log,
    )
    # __post_init__ has now linked each BoneRecord back to `result`, so
    # world_translation / world_matrix can resolve the parent chain —
    # validate AFTER construction, not before.
    _validate_bones(bones, log)

    return result


# ---------------------------------------------------------------------------
# glTF export — skeleton-only armature (no mesh required)
# ---------------------------------------------------------------------------

def build_skeleton_gltf(result: SkeletonResult) -> bytes:
    """
    Build a .glb that contains ONLY an armature (glTF nodes, no geometry).

    Each bone becomes a glTF node.  Parent-child links are set via the
    standard glTF node 'children' array.

    glTF nodes are inherently parent-relative, and — per the verified field
    semantics documented on BoneRecord — `quaternion` and `local_translation`
    (alias of `unknown_vec`) ARE ALREADY the local, parent-relative bind-pose
    rotation/translation.  No world-to-local conversion is needed (the
    earlier version of this function incorrectly subtracted the `translation`
    field, which is not reliable bone-position data — see BoneRecord's
    docstring for how this was established).

    Returns raw GLB bytes ready to write to disk.
    """
    if not result.ok:
        raise ValueError("No bones to export")

    # ── Build glTF node list ───────────────────────────────────────────────
    gltf_nodes = []
    for bone in result.bones:
        tx, ty, tz = bone.local_translation
        qx, qy, qz, qw = bone.quaternion
        children = [c.index for c in result.children_of(bone.index)]
        node: dict = {
            "name":        bone.name,
            "translation": [tx, ty, tz],
            "rotation":    [qx, qy, qz, qw],
            "scale":       list(bone.scale),
        }
        if children:
            node["children"] = children
        gltf_nodes.append(node)

    root_nodes = [b.index for b in result.bones if b.is_root]

    gltf_json = {
        "asset":  {"version": "2.0", "generator": "eagl_skeleton.py"},
        "scene":  0,
        "scenes": [{"nodes": root_nodes,
                    "name": result.file_path.stem}],
        "nodes":  gltf_nodes,
    }

    json_bytes = json.dumps(gltf_json, separators=(",", ":")).encode("utf-8")
    # GLB JSON chunk must be 4-byte padded with spaces
    if len(json_bytes) % 4:
        json_bytes += b" " * (4 - len(json_bytes) % 4)

    # Skeleton-only GLB has no BIN chunk
    _GLB_MAGIC   = 0x46546C67
    _GLB_VERSION = 2
    _CHUNK_JSON  = 0x4E4F534A

    total_len = 12 + 8 + len(json_bytes)
    glb = (
        struct.pack("<III", _GLB_MAGIC, _GLB_VERSION, total_len) +
        struct.pack("<II",  len(json_bytes), _CHUNK_JSON) +
        json_bytes
    )
    return glb


# ---------------------------------------------------------------------------
# Tie-in: load both .ske and a mesh ParseResult, print combined report
# ---------------------------------------------------------------------------

def print_combined_report(ske_path: str | Path,
                          mesh_parse_result=None) -> SkeletonResult:
    """
    Parse a .ske file, print a formatted report, and optionally cross-reference
    bone names against a mesh ParseResult's bone_weights / vertex_joints data
    (Layout D / D2 meshes).

    Returns the SkeletonResult so the caller can use it further.
    """
    result = parse_ske_file(ske_path)

    W = 72
    print("=" * W)
    print(f"  EAGL Skeleton  ·  {result.file_path.name}  ({result.file_size:,} bytes)")
    print("=" * W)

    for line in result.log:
        print(line)

    if not result.ok:
        print("\n  No bones decoded.")
        return result

    print(f"\n  ── BONE HIERARCHY ({result.bone_count} bones) " + "─" * 30)
    print(result.hierarchy_str())

    print(f"\n  ── BIND-POSE TRANSLATIONS (world-space, verified formula) " + "─" * 7)
    print(f"  {'Idx':>3}  {'Name':<24}  {'Parent':>6}  {'Wx':>9}  {'Wy':>9}  {'Wz':>9}")
    print("  " + "-" * 66)
    for b in result.bones:
        par = str(b.parent_idx) if not b.is_root else "root"
        tx, ty, tz = b.world_translation
        print(f"  {b.index:>3}  {b.name:<24}  {par:>6}  {tx:+9.4f}  {ty:+9.4f}  {tz:+9.4f}")

    if mesh_parse_result is not None:
        _cross_reference_mesh(result, mesh_parse_result)

    return result


def _cross_reference_mesh(skel: SkeletonResult, mesh_result) -> None:
    """
    Print a cross-reference between .ske bone names and mesh bone-weight slots.
    Only meaningful for Layout D / D2 meshes that carry bone_weights.
    bone_weights entries are (w0, w1, w2, bone_A, bone_B, bone_C).
    vertex_joints maps (pi,ni,ui) directly to those tuples.
    """
    print(f"\n  ── MESH CROSS-REFERENCE " + "─" * 42)
    weighted_meshes = [m for m in mesh_result.meshes if m.ok and m.bone_weights]
    if not weighted_meshes:
        print("  No skinned (Layout D/D2) meshes found in ParseResult — nothing to cross-ref.")
        return

    for m in weighted_meshes:
        print(f"\n  Mesh[{m.index}] layout={m.layout}  "
              f"weights={len(m.bone_weights)}  vertices with joints={len(m.vertex_joints)}")

        # vertex_joints maps (pi,ni,ui) → (w0,w1,w2, bone_A,bone_B,bone_C) tuple.
        # Collect unique influence tuples and display with resolved bone names.
        unique_entries: dict = {}
        for entry in m.vertex_joints.values():
            unique_entries[entry] = unique_entries.get(entry, 0) + 1
        print(f"    Unique weight entries: {len(unique_entries)}")
        for entry in sorted(unique_entries)[:16]:
            w0, w1, w2, b0, b1, b2 = entry
            n0 = skel.bones[b0].name if b0 < skel.bone_count else "?"
            parts = [f"[{b0}]{n0}*{w0:.3f}"]
            if w1 > 1e-4:
                n1 = skel.bones[b1].name if b1 < skel.bone_count else "?"
                parts.append(f"[{b1}]{n1}*{w1:.3f}")
            if w2 > 1e-4:
                n2 = skel.bones[b2].name if b2 < skel.bone_count else "?"
                parts.append(f"[{b2}]{n2}*{w2:.3f}")
            print(f"    {' + '.join(parts)}")
        if len(unique_entries) > 16:
            print(f"    ... ({len(unique_entries) - 16} more)")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import sys

    args = sys.argv[1:]
    if not args or args[0] in ("-h", "--help"):
        print("Usage: python eagl_skeleton.py <file.ske> [--export] [--mesh <file.o>]")
        print()
        print("  --export        Write <file>.glb armature next to the .ske file")
        print("  --mesh <file>   Cross-reference with a mesh .o file via parser.py")
        sys.exit(0)

    export_glb = "--export" in args
    mesh_path  = None
    ske_args   = []

    i = 0
    while i < len(args):
        if args[i] == "--mesh" and i + 1 < len(args):
            mesh_path = Path(args[i + 1])
            i += 2
        elif args[i] == "--export":
            i += 1
        else:
            ske_args.append(args[i])
            i += 1

    if not ske_args:
        print("Error: no .ske file specified.")
        sys.exit(1)

    # Optional: load mesh ParseResult for cross-referencing
    mesh_result = None
    if mesh_path is not None:
        try:
            import importlib.util as _ilu
            _parser_path = Path(__file__).with_name("parser.py")
            _spec = _ilu.spec_from_file_location("parser", _parser_path)
            _mod  = _ilu.module_from_spec(_spec)
            _spec.loader.exec_module(_mod)
            mesh_result = _mod.parse_o_file(mesh_path)
            print(f"[mesh] Loaded {mesh_path.name}: {mesh_result.summary()}\n")
        except Exception as e:
            print(f"[mesh] Could not load {mesh_path}: {e}\n")

    for ske_arg in ske_args:
        ske_file = Path(ske_arg)
        if not ske_file.exists():
            print(f"File not found: {ske_arg}")
            continue

        result = print_combined_report(ske_file, mesh_result)

        if export_glb and result.ok:
            out_path = ske_file.with_suffix(".glb")
            try:
                glb = build_skeleton_gltf(result)
                out_path.write_bytes(glb)
                print(f"\n✅ Armature exported to {out_path}")
            except Exception as exc:
                print(f"\n❌ GLB export failed: {exc}")
