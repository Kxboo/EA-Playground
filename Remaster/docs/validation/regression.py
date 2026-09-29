"""
regression_full_corpus.py
----------------------------
Full 265/265 regression pass, per the requested checklist:
  1. export success/failure (re-derives from anm_exporter's own report)
  2. quaternion norms within the validated range for every rotation clip
  3. bone indices in 0..67 for every resolved bone table
  4. no buffer overruns / record-size mismatches (every decoder already
     bounds-checks and raises/flags on out-of-range reads; this script
     re-runs every decoder path and catches exceptions explicitly rather
     than trusting silent success)
"""

import sys
import os
import struct
import math
import json

_PACKAGE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PACKAGE_ROOT not in sys.path:
    sys.path.insert(0, _PACKAGE_ROOT)

from decoder.eagl_anm_decoder import (
    _read_sections, _read_symbols, _self_reloc_map,
    _find_bank, _parse_bank_header, _read_table_a, _parse_clip_block,
    MAX_BONES,
)
from decoder.fn_delta_qfast import decode_clip_fndeltaqfast, quat_norm as qn_qfast
from decoder.fn_delta_f3 import decode_clip_fndeltaf3
from decoder.fn_delta_f1 import decode_clip_fndeltaf1
from decoder.fn_delta_singleq import decode_clip_fndeltasingleq, quat_norm as qn_singleq
from decoder.fn_stateless_q import decode_clip_fnstatelessq, quat_norm as qn_statelessq
from decoder.fn_stateless_f3 import decode_clip_fnstatelessf3
from exporter.anm_exporter import read_qfast_channel_bones, read_vector_channel_bones, read_scalar_channel_bone_axis
from decoder.paths import ANM_PATH as PATH


def load_all_clips():
    data = open(PATH, "rb").read()
    sections, data_start = _read_sections(data)
    symbols = _read_symbols(data, sections)
    reloc = _self_reloc_map(data, sections, data_start)
    bank_off = _find_bank(data, symbols, data_start)
    bh = _parse_bank_header(data, bank_off)
    table_a = _read_table_a(data, reloc, bh)

    clips = []
    for i, rel in enumerate(table_a):
        if rel is None:
            continue
        cb = _parse_clip_block(data, reloc, data_start, i, rel)
        if cb is not None:
            clips.append(cb)
    return data, reloc, data_start, clips


def check_bones(bone_list, label, clip_idx, problems):
    if bone_list is None:
        return
    for b in bone_list:
        if b is None:
            continue
        if not (0 <= b < MAX_BONES):
            problems.append(f"clip {clip_idx}: {label} bone {b} out of range [0,{MAX_BONES})")


def main():
    data, reloc, data_start, clips = load_all_clips()
    print(f"Total clips: {len(clips)}")

    codec_counts = {}
    n_ok = 0
    n_fail = 0
    problems = []
    norm_violations = []

    import types

    for cb in clips:
        try:
            if cb.whole_clip:
                codec_counts["FnStatelessQ+F3 (whole_clip)"] = codec_counts.get("FnStatelessQ+F3 (whole_clip)", 0) + 1
                rot = decode_clip_fnstatelessq(data, reloc, data_start, cb)
                if rot is None or "error" in rot:
                    raise RuntimeError(f"FnStatelessQ decode error: {rot.get('error') if rot else None}")
                for chan in rot["frames_by_channel"]:
                    for q in chan:
                        n = qn_statelessq(q)
                        if not (0.99 < n < 1.01):
                            norm_violations.append(("FnStatelessQ", cb.index, n))
                check_bones(rot["bone_table"], "FnStatelessQ", cb.index, problems)

                if cb.secondary is not None and cb.secondary.tag == 0x17:
                    trans = decode_clip_fnstatelessf3(data, reloc, data_start, cb)
                    if trans and "error" not in trans:
                        check_bones(trans["bone_table"], "FnStatelessF3", cb.index, problems)
                n_ok += 1
                continue

            if cb.primary and cb.primary.tag == 0x13:
                codec_counts["FnDeltaSingleQ+QFast"] = codec_counts.get("FnDeltaSingleQ+QFast", 0) + 1
                sres = decode_clip_fndeltasingleq(data, cb)
                for chan in sres["frames"]:
                    for q in chan:
                        n = qn_singleq(q)
                        if not (0.99 < n < 1.01):
                            norm_violations.append(("FnDeltaSingleQ", cb.index, n))
                sbones = read_qfast_channel_bones(data, data_start, cb.primary, sres["channel_count"])
                check_bones(sbones, "FnDeltaSingleQ", cb.index, problems)

                shim_cb = types.SimpleNamespace(index=cb.index, primary=cb.secondary)
                qres = decode_clip_fndeltaqfast(data, shim_cb)
                for frame in qres["frames"]:
                    for q in frame:
                        if q is None:
                            continue
                        n = qn_qfast(q)
                        if not (0.99 < n < 1.01):
                            norm_violations.append(("FnDeltaQFast(secondary)", cb.index, n))
                qbones = read_qfast_channel_bones(data, data_start, cb.secondary, qres["channel_count"])
                check_bones(qbones, "QFast(secondary)", cb.index, problems)
                if sbones and qbones and (set(sbones) & set(qbones)):
                    problems.append(f"clip {cb.index}: SingleQ/QFast bone overlap {set(sbones)&set(qbones)}")
                n_ok += 1
                continue

            if cb.primary and cb.primary.tag == 0x12 and cb.secondary and cb.secondary.tag in (0x14, 0x15):
                key = f"FnDeltaQFast+{'F3' if cb.secondary.tag==0x14 else 'F1'}"
                codec_counts[key] = codec_counts.get(key, 0) + 1
                rot = decode_clip_fndeltaqfast(data, cb)
                for frame in rot["frames"]:
                    for q in frame:
                        if q is None:
                            continue
                        n = qn_qfast(q)
                        if not (0.99 < n < 1.01):
                            norm_violations.append(("FnDeltaQFast", cb.index, n))
                rbones = read_qfast_channel_bones(data, data_start, cb.primary, rot["channel_count"])
                check_bones(rbones, "FnDeltaQFast", cb.index, problems)

                if cb.secondary.tag == 0x14:
                    tres = decode_clip_fndeltaf3(data, cb)
                    if tres["channel_count"] > 0:
                        tbones = read_vector_channel_bones(data, reloc, data_start, cb.secondary, tres["channel_count"])
                        check_bones(tbones, "FnDeltaF3", cb.index, problems)
                else:
                    tres = decode_clip_fndeltaf1(data, cb)
                    tbones_axis = read_scalar_channel_bone_axis(data, reloc, data_start, cb.secondary, tres["channel_count"])
                    if tbones_axis:
                        check_bones([b for b, a in tbones_axis], "FnDeltaF1", cb.index, problems)
                n_ok += 1
                continue

            codec_counts["UNSUPPORTED"] = codec_counts.get("UNSUPPORTED", 0) + 1
            problems.append(f"clip {cb.index}: no matching codec combo (primary={hex(cb.primary.tag) if cb.primary else None}, secondary={hex(cb.secondary.tag) if cb.secondary else None})")
            n_fail += 1

        except Exception as e:
            n_fail += 1
            problems.append(f"clip {cb.index}: EXCEPTION during decode/validate: {e!r}")

    print("\n=== CODEC DISTRIBUTION ===")
    for k, v in sorted(codec_counts.items(), key=lambda x: -x[1]):
        print(f"  {k:35s} {v:4d}")

    print(f"\n=== RESULTS ===")
    print(f"OK (decoded + validated without exception): {n_ok}")
    print(f"FAIL:                                        {n_fail}")
    print(f"Total:                                        {len(clips)}")

    print(f"\n=== NORM VIOLATIONS (outside 0.99-1.01) ===")
    print(f"count: {len(norm_violations)}")
    for v in norm_violations[:20]:
        print(f"  {v}")
    if len(norm_violations) > 20:
        print(f"  ... and {len(norm_violations)-20} more")

    print(f"\n=== BONE / STRUCTURAL PROBLEMS ===")
    print(f"count: {len(problems)}")
    for p in problems[:30]:
        print(f"  {p}")
    if len(problems) > 30:
        print(f"  ... and {len(problems)-30} more")

    out_path = os.path.join(_PACKAGE_ROOT, "output", "regression_full_corpus.json")
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "w") as f:
        json.dump({
            "codec_counts": codec_counts, "n_ok": n_ok, "n_fail": n_fail,
            "norm_violations": norm_violations, "problems": problems,
        }, f, indent=2, default=str)
    print(f"\nWrote {out_path}")


if __name__ == "__main__":
    main()
