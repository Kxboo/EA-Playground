#!/usr/bin/env python3
"""
eagl_mesh_material_map.py

Maps each mesh (descriptor) in an EAGL .o file to the material/texture
(TAR symbol) it actually references, using ground-truth ELF relocation
data instead of assuming gsh-directory-order == symtab-order.

How it works
------------
Every material a mesh uses is bound via an `__EAGL::TAR:::RUNTIME_ALLOC`
symbol, and the linker emits an R_MIPS_32 relocation at the exact byte
offset in .data where that TAR pointer is stored (inside the mesh's
GeoPrimState / descriptor block). By reading the relocation table
directly we can see, per mesh, exactly which TAR symbol(s) it points
to -- no ordering assumption required, and it can catch multi-material
meshes if they exist.

This module is import-friendly: `map_meshes_to_materials()` takes the
already-loaded `parser` module and (optionally) an already-computed
ParseResult, so callers like eagl_ui.py never need a second parse pass
or a hardcoded `import parser`.

CLI usage
---------
    python3 eagl_mesh_material_map.py model.o [--layouts eagl_layouts.json]
"""
from __future__ import annotations

import argparse
import re
import struct
from pathlib import Path
from types import ModuleType


def _read_symtab(data: bytes, sections: list[dict], parser_module: ModuleType) -> list[str]:
    sym_sec = next((s for s in sections if s["name"] == ".symtab"), None)
    str_sec = next((s for s in sections if s["name"] == ".strtab"), None)
    if not sym_sec or not str_sec:
        return []
    names = []
    entry_size = sym_sec["entsize"] or 16
    str_base = str_sec["offset"]
    for i in range(sym_sec["size"] // entry_size):
        o = sym_sec["offset"] + i * entry_size
        name_idx = struct.unpack_from("<I", data, o)[0]
        names.append(parser_module._read_cstr(data, str_base + name_idx))
    return names


def _tar_symbol_table(names: list[str]) -> dict[int, str]:
    """symbol index -> short material name, for TAR:::RUNTIME_ALLOC symbols only."""
    out = {}
    for idx, name in enumerate(names):
        if "TAR:::RUNTIME_ALLOC" in name:
            m = re.search(r"1=([^,;]+)", name)
            if m:
                out[idx] = m.group(1)
    return out


def _rel_data_section(sections: list[dict]) -> dict | None:
    return next((s for s in sections if s["name"] in (".rel.data", ".rela.data")), None)


def _iter_tar_relocs(data: bytes, sections: list[dict], tar_syms: dict[int, str]):
    """Yield (offset_in_data, material_name) for every relocation that targets a TAR symbol."""
    rel_sec = _rel_data_section(sections)
    if not rel_sec:
        return
    is_rela = rel_sec["name"] == ".rela.data"
    entsize = rel_sec["entsize"] or (12 if is_rela else 8)
    o = rel_sec["offset"]
    end = o + rel_sec["size"]
    while o < end:
        r_offset, r_info = struct.unpack_from("<II", data, o)
        sym = r_info >> 8
        if sym in tar_syms:
            yield r_offset, tar_syms[sym]
        o += entsize


def map_meshes_to_materials(
    o_path: str,
    parser_module: ModuleType,
    result=None,
    layouts_path: str | None = None,
    data: bytes | None = None,
):
    """
    Returns (result, mesh_materials, header_like_offsets):
      result             — ParseResult (reused if passed in, else computed here)
      mesh_materials      — dict[mesh.index -> set[str material names]]
      header_like_offsets — set of reloc offsets excluded as model-header noise

    data — pre-read file bytes, for callers (e.g. batch export) that already
           read o_path for parse_o_file and want to avoid a second disk read
           of a potentially multi-MB .o file. Defaults to reading o_path.
    """
    if data is None:
        data = Path(o_path).read_bytes()
    sections, data_start = parser_module._read_sections(data)
    names = _read_symtab(data, sections, parser_module)
    tar_syms = _tar_symbol_table(names)

    if result is None:
        result = parser_module.parse_o_file(o_path, layouts_path=layouts_path, data=data)
    meshes = result.meshes

    header_like_offsets = set()
    tar_hits = list(_iter_tar_relocs(data, sections, tar_syms))
    tar_hits.sort()

    # Detect the tail "variation header" cluster: groups of >=3 distinct
    # materials within a tight window are model/variation metadata, not a
    # mesh, and must be excluded or they'd look like a multi-material mesh.
    i = 0
    mesh_material_relocs = []
    while i < len(tar_hits):
        off, mat = tar_hits[i]
        window = [(off, mat)]
        j = i + 1
        while j < len(tar_hits) and tar_hits[j][0] - off <= 0x40:
            window.append(tar_hits[j])
            j += 1
        distinct = {m for _, m in window}
        if len(window) >= 3 and len(distinct) >= 3:
            for k in range(i, j):
                header_like_offsets.add(tar_hits[k][0])
            i = j
        else:
            mesh_material_relocs.append((off, mat))
            i += 1

    # Assign each remaining reloc to the mesh whose descriptor window contains it
    mesh_materials: dict[int, set[str]] = {m.index: set() for m in meshes}
    sorted_meshes = sorted(meshes, key=lambda m: m.desc_offset)
    for off, mat in mesh_material_relocs:
        owner = None
        for k, m in enumerate(sorted_meshes):
            lo = m.desc_offset
            hi = sorted_meshes[k + 1].desc_offset if k + 1 < len(sorted_meshes) else off + 1
            if lo <= off < hi:
                owner = m
                break
        if owner is not None:
            mesh_materials[owner.index].add(mat)

    return result, mesh_materials, header_like_offsets


def summarize(o_path: str, parser_module: ModuleType, layouts_path: str | None = None):
    result, mesh_materials, _ = map_meshes_to_materials(o_path, parser_module, layouts_path=layouts_path)

    print(f"# {o_path}")
    print(f"model: {result.model_name}")
    print()

    runs = []
    for m in sorted(result.meshes, key=lambda m: m.index):
        mats = mesh_materials.get(m.index, set())
        mat = ",".join(sorted(mats)) if mats else "UNRESOLVED"
        if runs and runs[-1][2] == mat and runs[-1][1] == m.index - 1:
            runs[-1] = (runs[-1][0], m.index, mat)
        else:
            runs.append((m.index, m.index, mat))

    for lo, hi, mat in runs:
        rng = f"mesh {lo}" if lo == hi else f"mesh {lo}-{hi}"
        flag = "  ⚠ multi-material" if "," in mat else ""
        print(f"  {rng:16s} -> {mat}{flag}")

    unresolved = [m for m in result.meshes if not mesh_materials.get(m.index)]
    if unresolved:
        print()
        print(f"  {len(unresolved)} mesh(es) had no TAR reloc in range (likely untextured / vtx-color only):")
        print("   ", [m.index for m in unresolved])


if __name__ == "__main__":
    import sys
    sys.path.insert(0, str(Path(__file__).parent))
    import parser as _parser_cli

    ap = argparse.ArgumentParser()
    ap.add_argument("o_file")
    ap.add_argument("--layouts", default=None)
    args = ap.parse_args()
    summarize(args.o_file, _parser_cli, layouts_path=args.layouts)
