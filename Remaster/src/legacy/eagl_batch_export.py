#!/usr/bin/env python3
"""
eagl_batch_export.py
---------------------
Batch-convert a folder of EAGL .o model files to GLB, with materials
resolved from ground-truth relocation data (eagl_mesh_material_map) and
textures baked in from a set of .gsh archives (gsh_parser).

Designed to be driven either from the CLI or imported by eagl_ui.py's
batch-export dialog — all UI-facing state (progress, per-file results,
logging) goes through a callback rather than print(), so the GUI can
show it live.

Usage (library)
---------------
    from eagl_batch_export import batch_export, BatchItemResult

    results = batch_export(
        input_dir="models/",
        gsh_paths=["textures/"],           # a folder of .gsh, or individual files, or both mixed
        output_dir="out/",
        merge_meshes=True,
        on_progress=lambda done, total, name: None,
        on_log=lambda line: print(line),
    )
    for r in results:
        print(r.o_path.name, "OK" if r.ok else r.error)

Usage (CLI)
-----------
    python3 eagl_batch_export.py models/ --gsh textures/ -o out/
    python3 eagl_batch_export.py models/ --gsh world.gsh world-misc.gsh -o out/
"""
from __future__ import annotations

import argparse
import re
import traceback
from dataclasses import dataclass, field
from pathlib import Path


# ---------------------------------------------------------------------------
# Texture index — merges every .gsh into one name -> (entry, data) lookup
# ---------------------------------------------------------------------------

def _norm(name: str) -> str:
    """Loosen a name for fuzzy matching: lowercase, strip non-alphanumerics."""
    return re.sub(r"[^a-z0-9]", "", name.lower())


class TextureIndex:
    """
    Merges entries from one or more parsed .gsh files into lookup tables
    keyed by full_name and by the 4-char short name, both normalized, so
    material names coming out of the .o's TAR symbols (e.g. "field_grass",
    "bball_brickwall") resolve to the right GshEntry regardless of which
    archive it physically lives in.
    """

    def __init__(self, gsh_mod):
        self._gsh_mod = gsh_mod
        self._by_full: dict[str, tuple] = {}   # norm(full_name) -> (entry, data)
        self._by_short: dict[str, list] = {}   # norm(name)      -> [(entry, data), ...]
        self._png_cache: dict[int, bytes] = {}   # id(entry) -> png bytes
        self._rgba_cache: dict[int, tuple] = {}  # id(entry) -> (rgba, w, h)
        self.sources: list[str] = []

    def add_gsh(self, path: str | Path):
        gsh, data = self._gsh_mod.parse_gsh(path)
        self.sources.append(str(path))
        for e in gsh.entries:
            if e.full_name:
                self._by_full.setdefault(_norm(e.full_name), (e, data))
            if e.name:
                self._by_short.setdefault(_norm(e.name), []).append((e, data))
        return gsh

    def resolve(self, material_name: str):
        """Return (entry, data) for the best-matching texture, or None."""
        key = _norm(material_name)
        if key in self._by_full:
            return self._by_full[key]
        # fall back to short-name prefix match (gsh short names are truncated
        # to 4 chars, e.g. "bball_brickwall" material vs "bbal" entry name)
        short_key = key[:4]
        candidates = self._by_short.get(short_key)
        if candidates:
            # prefer a candidate whose full_name (if any) shares the prefix
            for e, data in candidates:
                if e.full_name and _norm(e.full_name).startswith(key[:6]):
                    return e, data
            return candidates[0]
        return None

    def decode_rgba(self, entry, data: bytes):
        cache_key = id(entry)
        if cache_key not in self._rgba_cache:
            self._rgba_cache[cache_key] = self._gsh_mod.decode_entry_rgba(entry, data)
        return self._rgba_cache[cache_key]

    def decode_png(self, entry, data: bytes) -> bytes:
        cache_key = id(entry)
        if cache_key not in self._png_cache:
            rgba, w, h = self.decode_rgba(entry, data)
            self._png_cache[cache_key] = self._gsh_mod.rgba_to_png_bytes(rgba, w, h)
        return self._png_cache[cache_key]

    def alpha_mode(self, entry, data: bytes) -> str:
        rgba, _, _ = self.decode_rgba(entry, data)
        return self._gsh_mod.classify_alpha(rgba)


def resolve_material_images(material_names, tex_index: TextureIndex,
                             on_log=None) -> tuple[dict[str, bytes], dict[str, str]]:
    """
    material_names : iterable[str]
    Returns (images, alpha_modes):
      images      — dict[material_name -> PNG bytes] for every name that
                    resolved to a texture. Names with no match are
                    silently skipped (the exporter falls back to a
                    flat-color placeholder material for those).
      alpha_modes — dict[material_name -> "opaque"|"mask"|"blend"],
                    content-classified from the decoded alpha channel
                    (see gsh_parser.classify_alpha). Only present for
                    names that also appear in `images`.
    """
    images: dict[str, bytes] = {}
    alpha_modes: dict[str, str] = {}
    for name in sorted(set(material_names)):
        hit = tex_index.resolve(name)
        if hit is None:
            if on_log:
                on_log(f"  ⚠ no texture match for material '{name}'")
            continue
        entry, data = hit
        try:
            images[name] = tex_index.decode_png(entry, data)
            alpha_modes[name] = tex_index.alpha_mode(entry, data)
        except Exception as exc:
            if on_log:
                on_log(f"  ⚠ could not decode texture for '{name}' "
                       f"(entry '{entry.full_name or entry.name}'): {exc}")
    return images, alpha_modes


# ---------------------------------------------------------------------------
# Batch driver
# ---------------------------------------------------------------------------

@dataclass
class BatchItemResult:
    o_path: Path
    ok: bool
    out_path: Path | None = None
    mesh_count: int = 0
    material_count: int = 0
    textured_material_count: int = 0
    unresolved_materials: list = field(default_factory=list)
    error: str | None = None


def _expand_gsh_paths(gsh_paths) -> list[Path]:
    """
    Accepts a mix of individual .gsh file paths and folder paths; folders
    are expanded to every *.gsh file directly inside them (non-recursive,
    matching how _find_o_files treats the models folder — but gsh archives
    are typically flat, so no need to recurse). De-duplicated, sorted.
    """
    out: list[Path] = []
    seen: set[Path] = set()
    for gp in gsh_paths:
        p = Path(gp)
        if p.is_dir():
            found = sorted(p.glob("*.gsh"))
            for f in found:
                rp = f.resolve()
                if rp not in seen:
                    seen.add(rp)
                    out.append(f)
        elif p.is_file():
            rp = p.resolve()
            if rp not in seen:
                seen.add(rp)
                out.append(p)
    return out


def _find_o_files(input_dir: Path) -> list[Path]:
    return sorted(p for p in input_dir.rglob("*.o") if p.is_file())


def batch_export(
    input_dir: str | Path,
    gsh_paths: list[str | Path],
    output_dir: str | Path,
    *,
    merge_meshes: bool = True,
    layouts_path: str | None = None,
    parser_module=None,
    exporter_module=None,
    material_map_module=None,
    gsh_module=None,
    on_progress=None,      # (done:int, total:int, current_name:str) -> None
    on_log=None,           # (line:str) -> None
) -> list[BatchItemResult]:
    """
    Convert every .o file under input_dir to a .glb under output_dir,
    with materials assigned from relocation ground-truth and textures
    baked in from gsh_paths where a name match is found.

    gsh_paths : list[str | Path]
        Each entry may be an individual .gsh file OR a folder — folders
        are expanded to every *.gsh directly inside them. Mixing both is
        fine (e.g. ["textures/", "extra_one_off.gsh"]). All resolved
        archives are merged into one texture index, so a material can be
        found in any of them regardless of which file it lives in.
    """
    input_dir = Path(input_dir)
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    def log(line: str):
        if on_log:
            on_log(line)

    if parser_module is None:
        import parser as parser_module  # type: ignore
    if exporter_module is None:
        import eagl_exporter as exporter_module  # type: ignore
    if material_map_module is None:
        import eagl_mesh_material_map as material_map_module  # type: ignore
    if gsh_module is None:
        import gsh_parser as gsh_module  # type: ignore

    o_files = _find_o_files(input_dir)
    if not o_files:
        log(f"No .o files found under {input_dir}")
        return []

    tex_index = TextureIndex(gsh_module)
    expanded_gsh = _expand_gsh_paths(gsh_paths)
    if not expanded_gsh:
        log(f"⚠ no .gsh files found in the given texture path(s): {gsh_paths}")
    for gp in expanded_gsh:
        try:
            gsh = tex_index.add_gsh(gp)
            log(f"Loaded textures: {gp}  ({len(gsh.entries)} entries)")
        except Exception as exc:
            log(f"⚠ failed to load {gp}: {exc}")

    results: list[BatchItemResult] = []
    total = len(o_files)
    for i, o_path in enumerate(o_files, start=1):
        if on_progress:
            on_progress(i - 1, total, o_path.name)
        log(f"[{i}/{total}] {o_path.name}")
        try:
            file_bytes = o_path.read_bytes()
            result = parser_module.parse_o_file(o_path, layouts_path=layouts_path, data=file_bytes)
            if not result.ok:
                results.append(BatchItemResult(o_path, ok=False, error="parse failed / no geometry"))
                log(f"  ✗ parse failed")
                continue

            _, mesh_materials, _ = material_map_module.map_meshes_to_materials(
                o_path, parser_module, result=result, layouts_path=layouts_path, data=file_bytes,
            )
            distinct_mats = sorted({m for s in mesh_materials.values() for m in s})

            material_images, alpha_modes = resolve_material_images(distinct_mats, tex_index, on_log=log)
            unresolved = [m for m in distinct_mats if m not in material_images]

            glb_bytes = exporter_module.build_gltf(
                result,
                merge_meshes=merge_meshes,
                mesh_materials=mesh_materials,
                material_images=material_images,
                material_alpha_modes=alpha_modes,
            )

            out_path = output_dir / (o_path.stem + ".glb")
            out_path.write_bytes(glb_bytes)

            non_opaque = sorted(n for n, m in alpha_modes.items() if m != "opaque")
            results.append(BatchItemResult(
                o_path, ok=True, out_path=out_path,
                mesh_count=len([m for m in result.meshes if m.ok]),
                material_count=len(distinct_mats),
                textured_material_count=len(material_images),
                unresolved_materials=unresolved,
            ))
            tag = f"{len(material_images)}/{len(distinct_mats)} textured" if distinct_mats else "no materials"
            log(f"  ✓ {out_path.name}  ({tag})")
            if non_opaque:
                log(f"    alpha: " + ", ".join(f"{n}={alpha_modes[n]}" for n in non_opaque))
        except Exception as exc:
            log(f"  ✗ {exc}")
            results.append(BatchItemResult(o_path, ok=False, error=f"{exc}\n{traceback.format_exc()}"))

    if on_progress:
        on_progress(total, total, "")

    ok_count = sum(1 for r in results if r.ok)
    log(f"\nDone: {ok_count}/{total} exported to {output_dir}")
    return results


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input_dir", help="Folder containing .o files (searched recursively)")
    ap.add_argument("--gsh", nargs="+", required=True,
                     help="One or more .gsh files and/or folders of .gsh files")
    ap.add_argument("-o", "--output", required=True, help="Output folder for .glb files")
    ap.add_argument("--no-merge", action="store_true", help="One mesh node per chunk instead of merged")
    ap.add_argument("--layouts", default=None, help="Path to eagl_layouts.json override")
    args = ap.parse_args()

    batch_export(
        args.input_dir, args.gsh, args.output,
        merge_meshes=not args.no_merge,
        layouts_path=args.layouts,
        on_log=print,
    )


if __name__ == "__main__":
    main()
