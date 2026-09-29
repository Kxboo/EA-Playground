"""
anm_exporter.py
-----------------
Exports each clip in player_anims.anm to its own .glb, skinned onto
player_skel.ske's node hierarchy, using ONLY disassembly-confirmed
structure:

  - codec decode math: reused verbatim from anm_validate.py /
    fndeltaf3_validate.py / fndeltaf1_validate.py (no reimplementation)
  - channel -> bone mapping:
      FnDeltaQFast : marker+0x0C ptr, u8 entries, bone = entry directly
      FnDeltaF3/F1 : marker+0x04 ptr, u16 entries, bone = entry // 3
    both confirmed via capstone disassembly of GetAnimatedBonesAux in
    playgroundz.elf this session (see chat writeup).
  - bind pose / hierarchy / names: player_skel.ske via eagl_skeleton.py

Supported combos (verified corpus-wide, sample_count(primary) ==
sample_count(secondary) for all 244 of these):
  - FnDeltaQFast (rotation) + FnDeltaF3 (translation)   -- 224 clips
  - FnDeltaQFast (rotation) + FnDeltaF1 (translation)   --  20 clips

NOT exported (codec not yet decoded -- not guessed at):
  - whole_clip (FnStatelessQ container)                --  12 clips
  - FnDeltaSingleQ primary / anomalous secondary        --   9 clips

Unanimated bones are left at the skeleton's bind pose (no animation
channel emitted for them) -- this is standard, correct glTF behavior,
not an approximation.

Frame rate is a PLACEHOLDER (30fps), consistent with the rest of this
project's documented uncertainty on true playback FPS.
"""

import struct
import sys
import os
import json
from pathlib import Path

# Bootstrap: ensure the package root (parent of this file's directory) is
# on sys.path, so `from decoder...` imports work whether this script is
# run directly (`python3 exporter/anm_exporter.py`) or as a module
# (`python3 -m exporter.anm_exporter`).
_PACKAGE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PACKAGE_ROOT not in sys.path:
    sys.path.insert(0, _PACKAGE_ROOT)

from decoder.eagl_anm_decoder import (
    _read_sections, _read_symbols, _self_reloc_map,
    _find_bank, _parse_bank_header, _read_table_a, _parse_clip_block,
    _find_clip_name_table, TrackMarker, MAX_BONES, _parse_track_marker,
)
from decoder.eagl_skeleton import parse_ske_file
from decoder.fn_delta_qfast import decode_clip_fndeltaqfast
from decoder.fn_delta_f3 import decode_clip_fndeltaf3
from decoder.fn_delta_f1 import decode_clip_fndeltaf1
from decoder.fn_stateless_f3 import decode_clip_fnstatelessf3
from decoder.fn_stateless_q import decode_clip_fnstatelessq
from decoder.fn_delta_singleq import decode_clip_fndeltasingleq
from decoder.paths import ANM_PATH, SKE_PATH, OUTPUT_DIR
from decoder.bone_mask import load_translation_mask

OUT_DIR = Path(OUTPUT_DIR)


# ---------------------------------------------------------------------------
# Channel -> bone table readers (session 25 disassembly-confirmed)
# ---------------------------------------------------------------------------

def read_qfast_channel_bones(data: bytes, data_start: int, marker: TrackMarker,
                              channel_count: int) -> list[int] | None:
    """u8 entries at marker+0x0C, bone index directly (no transform)."""
    if marker.dense_list_ptr_rel is None:
        return None
    abs_off = data_start + marker.dense_list_ptr_rel
    if abs_off + channel_count > len(data):
        return None
    return [data[abs_off + i] for i in range(channel_count)]


# ---------------------------------------------------------------------------
# CORRECTED bone/axis resolver for all vector-translation codecs
# (FnDeltaF3, FnStatelessF3, FnDeltaF1)
#
# The ORIGINAL formula here (bone = entry // 3, axis = entry % 3) was never
# actually range-checked against MAX_BONES and was WRONG -- verified this
# session by decoding real corpus entries and finding bone values up to 226
# (impossible on a 68-bone skeleton). Direct inspection of raw table entries
# across FnDeltaF3/FnStatelessF3/FnDeltaF1 clips shows a single shared
# encoding instead:
#
#     raw_entry = 8 + bone_index * 12 + axis_offset
#
# where axis_offset is always 0 for FnDeltaF3/FnStatelessF3 (whose records
# already store a full XYZ triple -- there is no axis selector, entries just
# always land on the "+8" (X-slot) boundary of the per-bone 0x30-byte pose
# record: bone*12+8, confirmed via mod-12 == 8 for 1217/1218 real
# FnDeltaF3 entries and 100/100 FnStatelessF3 entries), and 0/1/2 for
# FnDeltaF1 (a genuinely single-axis codec, confirmed via mod-12 landing on
# 8/9/10 across its real corpus, cleanly separable as
# axis = (entry-8) % 12).
#
# This is NOT a generic "entry // 3" channel-index scheme -- it is byte
# address arithmetic against the SAME 0x30-byte-per-bone pose record the
# rotation codecs write into: bone*0x30 + 0x20 is the translation slot
# (raw_entry*4 == bone*0x30+0x20 when raw_entry == bone*12+8), directly
# beside FnDeltaSingleQ's confirmed bone*0x30+0x10 quaternion slot.
# ---------------------------------------------------------------------------

def _decode_pose_field_entry(raw_entry: int) -> tuple[int, str, int] | None:
    """General decode of a raw channel-table entry against the shared
    0x30-byte-per-bone SQT pose record (confirmed layout, see
    PoseBoneSQTToGlobal disassembly writeup):

        word  0- 2  scale.x/y/z     (bytes 0x00-0x0C)
        word  3     pad / scale.w   (bytes 0x0C-0x10)
        word  4- 7  quat.x/y/z/w    (bytes 0x10-0x20)
        word  8-10  trans.x/y/z     (bytes 0x20-0x2C)
        word 11     pad             (bytes 0x2C-0x30)

    raw_entry is a flat word index: bone = raw_entry // 12,
    field_word = raw_entry % 12. Returns (bone, field_name, axis) where
    field_name in {'scale', 'quat', 'trans', 'pad'} and axis is the
    0-based component within that field (0 for 'pad').

    NOTE: the OLD formula here (`num = raw_entry - 8; bone = num//12;
    axis = num%12; require axis in (0,1,2)`) implicitly assumed every
    entry addresses the translation field, and silently mis-decoded
    (or rejected) any entry addressing scale/quat/pad instead. Session
    27: found one real corpus entry (S_sk_bag_reach, channel 1) that
    addresses bone 30's scale.x field, not translation -- rare (1/1218
    vector-channel entries corpus-wide) but real, and the old code's
    all-or-nothing table resolution meant that ONE unrecognized entry
    discarded the OTHER 11 valid translation channels in that clip."""
    if raw_entry < 0:
        return None
    bone = raw_entry // 12
    rem = raw_entry % 12
    if 0 <= rem <= 2:
        return bone, "scale", rem
    if rem == 3:
        return bone, "pad", 0
    if 4 <= rem <= 7:
        return bone, "quat", rem - 4
    if 8 <= rem <= 10:
        return bone, "trans", rem - 8
    return bone, "pad", 0  # rem == 11


def _decode_bone_axis_entry(raw_entry: int) -> tuple[int, int] | None:
    """Backward-compat wrapper: only used where a translation-slot entry
    is required (bone, axis) -- returns None for anything else, exactly
    matching the old behavior for translation-slot entries."""
    decoded = _decode_pose_field_entry(raw_entry)
    if decoded is None:
        return None
    bone, field, axis = decoded
    if field != "trans" or not (0 <= bone < MAX_BONES):
        return None
    return bone, axis


def read_vector_channel_bones(data: bytes, reloc: dict, data_start: int,
                               marker: TrackMarker, channel_count: int
                               ) -> tuple[list[int | None], list[str], dict[int, int]] | None:
    """FnDeltaF3 / FnStatelessF3: each channel already stores a full XYZ
    triple, so the axis field is not a component selector -- just decode
    the bone index and require field=='trans'.

    Returns (bones, notes, scale_channels) where bones[i] is the bone
    index for channel i, or None if that channel addresses a
    non-translation field -- in which case the caller should skip just
    that channel, not the whole clip. scale_channels maps
    {channel_index: bone_index} for any channel addressing the scale
    field, so the caller can route it to scale animation instead of
    silently dropping it. `notes` lists any skipped channels as caveats.
    Returns None (not a tuple) only for structural failures: no
    relocation found, or the table runs off the end of data."""
    marker_rel = marker.abs_off - data_start
    ptr_rel = reloc.get(marker_rel + 4)
    if ptr_rel is None:
        return None
    abs_off = data_start + ptr_rel
    if abs_off + channel_count * 2 > len(data):
        return None
    out = []
    notes = []
    scale_channels = {}
    for i in range(channel_count):
        raw = struct.unpack_from(">H", data, abs_off + i * 2)[0]
        decoded = _decode_pose_field_entry(raw)
        if decoded is None:
            out.append(None)
            notes.append(f"channel {i}: raw entry {raw} out of range, skipped")
            continue
        bone, field, axis = decoded
        if field == "scale":
            out.append(None)
            scale_channels[i] = bone
            notes.append(f"channel {i}: bone {bone} animated SCALE (exported as scale channel)")
            continue
        if field != "trans":
            out.append(None)
            notes.append(f"channel {i}: addresses bone {bone}'s {field}.{axis} field, "
                          f"not translation -- skipped (not yet supported)")
            continue
        out.append(bone)
    return out, notes, scale_channels


def read_scalar_channel_bone_axis(data: bytes, reloc: dict, data_start: int,
                                   marker: TrackMarker, channel_count: int
                                   ) -> list[tuple[int, int]] | None:
    """FnDeltaF1: single-axis codec -- axis genuinely selects X/Y/Z."""
    marker_rel = marker.abs_off - data_start
    ptr_rel = reloc.get(marker_rel + 4)
    if ptr_rel is None:
        return None
    abs_off = data_start + ptr_rel
    if abs_off + channel_count * 2 > len(data):
        return None
    out = []
    for i in range(channel_count):
        raw = struct.unpack_from(">H", data, abs_off + i * 2)[0]
        decoded = _decode_bone_axis_entry(raw)
        if decoded is None:
            return None
        out.append(decoded)
    return out


# Backward-compat alias retired: the old read_axis_channel_bones() name is
# intentionally NOT kept, so nothing can silently keep calling the wrong
# formula under the old name.


from exporter.gltf_writer import GlbBuilder, build_clip_glb, PLACEHOLDER_FPS


# ---------------------------------------------------------------------------
# Main export driver
# ---------------------------------------------------------------------------

def safe_filename(s: str) -> str:
    return "".join(c if c.isalnum() or c in "_-" else "_" for c in s)


def parse_anm_clips(anm_path=None):
    """Parse an .anm bank into (data, reloc, data_start, clips, clip_names)
    without decoding any frame data. Cheap, skeleton-independent -- used by
    both `main()` and any external caller (e.g. a UI adapter) that just
    wants clip/bone metadata before committing to a full decode."""
    with open(anm_path or ANM_PATH, "rb") as stream:
        data = stream.read()
    sections, data_start = _read_sections(data)
    symbols = _read_symbols(data, sections)
    reloc = _self_reloc_map(data, sections, data_start)
    bank_off = _find_bank(data, symbols, data_start)
    bh = _parse_bank_header(data, bank_off)
    table_a = _read_table_a(data, reloc, bh)
    # table_b parallels table_a and points to each clip's string through the
    # same self-relocation mechanism. Do not scan incidental S_ strings.
    data_section = next(s for s in sections if s['name'] == '.data')
    data_end = data_start + data_section['size']
    clip_names = []
    for i in range(bh.field1):
        name_rel = reloc.get(bh.table_b_off + i * 4)
        if name_rel is None or not 0 <= name_rel < data_section['size']:
            raise ValueError(f'Clip {i}: unresolved name-table pointer')
        off = data_start + name_rel
        end = data.find(b'\0', off, data_end)
        if end < 0:raise ValueError(f'Clip {i}: unterminated name')
        clip_names.append(data[off:end].decode('ascii'))

    clips = []
    for i, rel in enumerate(table_a):
        if rel is None:
            raise ValueError(f'Clip {i}: unresolved clip-table pointer')
        cb = _parse_clip_block(data, reloc, data_start, i, rel)
        if cb is None:
            raise ValueError(f'Clip {i}: invalid block')
        clips.append(cb)
    return data, reloc, data_start, clips, clip_names


def _decode_clip_bones_raw(data, reloc, data_start, skel, cb):
    """Full per-clip decode: dispatches on codec combo exactly like the
    export loop used to inline, and returns either
      {"status": "ok", "rot_by_bone": {...}, "trans_by_bone": {...},
       "sample_count": int, "caveats": [...], "codec": str}
    or
      {"status": "skip", "reason": str}
    Requires a real parsed skeleton (`skel`, from parse_ske_file) so bone
    indices can be range-checked against `len(skel.bones)` and F1's
    bind-pose axis fallback can read real bind translations.

    NOTE: this is the RAW decode -- every translation channel present in
    the file, whether or not the engine actually applies it at runtime.
    Callers should use `decode_clip_bones()` below, which applies the
    engine's `sBoneMask` gate on top of this. Decoded-in-the-file is not
    the same claim as applied-by-the-engine; see decoder/bone_mask.py.
    """
    n_bones = len(skel.bones)

    if cb.whole_clip:
        rot_result = decode_clip_fnstatelessq(data, reloc, data_start, cb, compute_stats=False)
        if rot_result is None or "error" in rot_result:
            return {"status": "skip", "reason": f"FnStatelessQ decode failed: {rot_result.get('error') if rot_result else 'None'}"}

        rot_bone_table = rot_result["bone_table"]
        if rot_bone_table is None:
            return {"status": "skip", "reason": "could not resolve FnStatelessQ bone table"}

        rot_by_bone = {}
        for ch, bone in enumerate(rot_bone_table):
            if bone >= n_bones:
                continue
            rot_by_bone[bone] = rot_result["frames_by_channel"][ch]

        trans_by_bone = {}
        caveats = []
        if cb.secondary is not None and cb.secondary.tag == 0x17:
            trans_result = decode_clip_fnstatelessf3(data, reloc, data_start, cb, compute_stats=False)
            if trans_result is None or "error" in trans_result:
                caveats.append(f"FnStatelessF3 decode failed: {trans_result.get('error') if trans_result else 'None'} -- bind-pose translation used throughout")
            else:
                trans_bone_table = trans_result["bone_table"]
                if trans_bone_table is None:
                    caveats.append("could not resolve FnStatelessF3 bone table -- bind-pose translation used throughout")
                else:
                    for ch, bone in enumerate(trans_bone_table):
                        if bone is None or bone >= n_bones:
                            continue
                        trans_by_bone[bone] = trans_result["frames_by_channel"][ch]
        else:
            caveats.append("no nested FnStatelessF3 secondary track -- bind-pose translation used throughout")

        sample_count = rot_result["keyframe_count"]
        if trans_by_bone:
            trans_kf_actual = decode_clip_fnstatelessf3(data, reloc, data_start, cb, compute_stats=False)["keyframe_count"]
            if trans_kf_actual != sample_count:
                caveats.append(f"translation keyframe_count ({trans_kf_actual}) != rotation keyframe_count ({sample_count}) -- NOT resampled")

        return {"status": "ok", "rot_by_bone": rot_by_bone, "trans_by_bone": trans_by_bone,
                "scale_by_bone": {}, "sample_count": sample_count, "caveats": caveats,
                "codec": "whole_clip (FnStatelessQ+F3)"}

    if cb.primary and cb.primary.tag == 0x13:
        singleq_result = decode_clip_fndeltasingleq(data, cb, compute_stats=False)
        singleq_channel_count = singleq_result["channel_count"]
        singleq_bone_table = read_qfast_channel_bones(data, data_start, cb.primary, singleq_channel_count)

        import types
        shim_cb = types.SimpleNamespace(index=cb.index, primary=cb.secondary)
        qfast_result = decode_clip_fndeltaqfast(data, shim_cb, compute_stats=False)
        qfast_channel_count = qfast_result["channel_count"]
        qfast_bone_table = read_qfast_channel_bones(data, data_start, cb.secondary, qfast_channel_count)

        rot_by_bone = {}
        caveats = []
        if singleq_bone_table is None:
            caveats.append("could not resolve FnDeltaSingleQ bone table")
        else:
            for ch, bone in enumerate(singleq_bone_table):
                if bone >= n_bones:
                    continue
                rot_by_bone[bone] = [frame[ch] for frame in singleq_result["frames"]]
        if qfast_bone_table is None:
            caveats.append("could not resolve secondary FnDeltaQFast bone table")
        else:
            overlap = set(qfast_bone_table) & set(rot_by_bone.keys())
            if overlap:
                caveats.append(f"UNEXPECTED bone overlap between SingleQ and QFast tracks: {sorted(overlap)}")
            for ch, bone in enumerate(qfast_bone_table):
                if bone >= n_bones:
                    continue
                rot_by_bone[bone] = [frame[ch] for frame in qfast_result["frames"]]

        if singleq_result["sample_count"] != qfast_result["sample_count"]:
            caveats.append(f"sample_count mismatch: SingleQ={singleq_result['sample_count']} QFast={qfast_result['sample_count']} -- NOT resampled")

        sample_count = singleq_result["sample_count"]
        return {"status": "ok", "rot_by_bone": rot_by_bone, "trans_by_bone": {},
                "scale_by_bone": {}, "sample_count": sample_count, "caveats": caveats,
                "codec": "FnDeltaSingleQ+QFast"}

    if not (cb.primary and cb.primary.tag == 0x12 and cb.secondary and cb.secondary.tag in (0x14, 0x15)):
        ptag = hex(cb.primary.tag) if cb.primary else None
        stag = hex(cb.secondary.tag) if cb.secondary else None
        return {"status": "skip", "reason": f"unsupported combo primary={ptag} secondary={stag}"}

    rot_result = decode_clip_fndeltaqfast(data, cb, compute_stats=False)
    rot_channel_count = rot_result["channel_count"]
    rot_bone_table = read_qfast_channel_bones(data, data_start, cb.primary, rot_channel_count)
    if rot_bone_table is None:
        return {"status": "skip", "reason": "could not resolve rotation channel->bone table"}

    rot_by_bone = {}
    for ch, bone in enumerate(rot_bone_table):
        if bone >= n_bones:
            continue
        rot_by_bone[bone] = [frame[ch] for frame in rot_result["frames"]]

    trans_by_bone = {}
    scale_by_bone = {}
    caveats = []
    if cb.secondary.tag == 0x14:
        trans_result = decode_clip_fndeltaf3(data, cb, compute_stats=False)
        trans_channel_count = trans_result["channel_count"]
        if trans_channel_count == 0:
            caveats.append("CHANNEL_COUNT==0 (anomaly) -- no translation channels, bind-pose translation used throughout")
        else:
            resolved = read_vector_channel_bones(data, reloc, data_start, cb.secondary, trans_channel_count)
            if resolved is None:
                caveats.append("could not resolve F3 channel->bone table -- bind-pose translation used throughout")
            else:
                trans_bone_table, notes, scale_channels = resolved
                caveats.extend(notes)
                for ch, bone in enumerate(trans_bone_table):
                    if bone is None or bone >= n_bones:
                        continue
                    trans_by_bone[bone] = [frame[ch] for frame in trans_result["frames"]]
                for ch, bone in scale_channels.items():
                    if bone >= n_bones:
                        continue
                    scale_by_bone[bone] = [frame[ch] for frame in trans_result["frames"]]
    else:  # tag 0x15, FnDeltaF1
        trans_result = decode_clip_fndeltaf1(data, cb, compute_stats=False)
        trans_channel_count = trans_result["channel_count"]
        trans_bone_table = read_scalar_channel_bone_axis(data, reloc, data_start, cb.secondary, trans_channel_count)
        if trans_bone_table is None:
            caveats.append("could not resolve F1 channel->bone table -- bind-pose translation used throughout")
        else:
            sample_count = trans_result["sample_count"]
            per_bone_axes: dict[int, dict[int, list[float]]] = {}
            for ch, (bone, axis) in enumerate(trans_bone_table):
                if bone >= n_bones or axis not in (0, 1, 2):
                    continue
                per_bone_axes.setdefault(bone, {})[axis] = [frame[ch] for frame in trans_result["frames"]]
            for bone, axes in per_bone_axes.items():
                if len(axes) < 3:
                    caveats.append(f"bone {bone} ({names_lookup(skel, bone)}): only {len(axes)}/3 axes present in F1 table -- missing axis held at bind pose")
                bind = skel.bones[bone].local_translation
                triples = []
                for s in range(sample_count):
                    triples.append((
                        axes.get(0, [bind[0]] * sample_count)[s],
                        axes.get(1, [bind[1]] * sample_count)[s],
                        axes.get(2, [bind[2]] * sample_count)[s],
                    ))
                trans_by_bone[bone] = triples

    sample_count = rot_result["sample_count"]
    return {"status": "ok", "rot_by_bone": rot_by_bone, "trans_by_bone": trans_by_bone,
            "scale_by_bone": scale_by_bone, "sample_count": sample_count, "caveats": caveats,
            "codec": "FnDeltaQFast+F3/F1"}


_translation_mask_cache = None


def _get_translation_mask():
    """Lazily load and cache the ELF-derived sBoneMask. Failure to load is
    NOT swallowed into "assume everything unmasked" -- that would silently
    reintroduce exactly the bug this exists to fix. Callers see the
    exception and can decide (a real pipeline failure here means we no
    longer know what the engine actually applies)."""
    global _translation_mask_cache
    if _translation_mask_cache is None:
        _translation_mask_cache = load_translation_mask()
    return _translation_mask_cache


def decode_clip_bones(data, reloc, data_start, skel, cb, apply_bone_mask=True):
    """Wraps `_decode_clip_bones_raw`, then applies the engine's runtime
    `sBoneMask` gate to `trans_by_bone` -- one central point, independent
    of which codec combo decoded the clip, instead of patching each
    codec branch individually.

    Masked-out bones have their translation channel DROPPED entirely
    (no key emitted), not zeroed: "no key" means the engine leaves the
    bone's existing transform alone, while "(0,0,0)" would mean the
    engine explicitly snaps it to bind translation -- those are different
    claims, and only the former matches what EvalSQTMask actually does.

    Rotation channels are untouched -- the mask only gates translation
    writes (per EvalSQTMask / decoder/bone_mask.py).
    """
    result = _decode_clip_bones_raw(data, reloc, data_start, cb=cb, skel=skel)
    if result["status"] != "ok" or not apply_bone_mask:
        return result

    mask = _get_translation_mask()
    trans_by_bone = result["trans_by_bone"]
    dropped = [bone for bone in trans_by_bone
               if bone < len(mask) and not mask[bone]]
    if dropped:
        result["trans_by_bone"] = {
            bone: v for bone, v in trans_by_bone.items() if bone not in dropped
        }
        result.setdefault("caveats", []).append(
            f"sBoneMask: dropped translation channel(s) for bone(s) {sorted(dropped)} "
            "(engine does not apply translation writes to these bones)"
        )
    return result


def main(rotation_only: bool = False):
    skel = parse_ske_file(SKE_PATH)
    data, reloc, data_start, clips, clip_names = parse_anm_clips()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    report_lines = []
    n_exported = 0
    n_skipped = 0

    for cb in clips:
        clip_label = clip_names[cb.index] if cb.index < len(clip_names) else f"clip_{cb.index}"
        fname_base = f"{cb.index:03d}_{safe_filename(clip_label)}"
        if rotation_only:
            fname_base += "_rotonly"

        result = decode_clip_bones(data, reloc, data_start, skel, cb)
        if result["status"] == "skip":
            report_lines.append(f"SKIP  clip {cb.index:3d} ({clip_label}): {result['reason']}")
            n_skipped += 1
            continue

        rot_by_bone = result["rot_by_bone"]
        trans_by_bone = {} if rotation_only else result["trans_by_bone"]
        scale_by_bone = {} if rotation_only else result.get("scale_by_bone", {})
        sample_count = result["sample_count"]
        caveats = result["caveats"]

        glb_bytes = build_clip_glb(skel, cb.index, clip_label, rot_by_bone, trans_by_bone, sample_count, scale_by_bone)
        out_path = OUT_DIR / f"{fname_base}.glb"
        out_path.write_bytes(glb_bytes)
        n_exported += 1

        status = f"OK    clip {cb.index:3d} ({clip_label}): {result['codec']}, {sample_count} frames, {len(rot_by_bone)} rotated bones, {len(trans_by_bone)} translated bones -> {out_path.name}"
        if caveats:
            status += "  [" + "; ".join(caveats) + "]"
        report_lines.append(status)


    report_path = OUT_DIR / "_export_report.txt"
    report_path.write_text(
        f"Exported {n_exported}/{len(clips)} clips, skipped {n_skipped}.\n"
        f"Frame rate: PLACEHOLDER {PLACEHOLDER_FPS} fps (true playback fps not independently confirmed).\n"
        f"Clip names sourced from heuristic S_-prefixed string table match by\n"
        f"position (265/265 count matches clip count, but name<->clip-index\n"
        f"correspondence itself is not structurally confirmed -- verify before\n"
        f"relying on filenames for anything beyond convenience).\n\n"
        + "\n".join(report_lines)
    )

    print(f"Exported {n_exported}/{len(clips)} clips to {OUT_DIR}")
    print(f"Skipped {n_skipped}")
    print(f"Report: {report_path}")


def names_lookup(skel, idx):
    for b in skel.bones:
        if b.index == idx:
            return b.name
    return f"???{idx}"


if __name__ == "__main__":
    main(rotation_only="--rotation-only" in sys.argv)
