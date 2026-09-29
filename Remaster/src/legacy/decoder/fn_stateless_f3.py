"""
fnstatelessf3_validate.py
----------------------------
Validation decoder for FnStatelessF3 (tag 0x17), the nested vector track
found at ClipBlock+0x10 inside all 12 whole-clip (FnStatelessQ) containers.

Built from a FRESH disassembly this session of:
  - SetAnimMemoryMap__FnStatelessF3 (0x803fea54): confirms `this+0x0C = map`,
    same convention as every other codec class in this project.
  - EvalSQT__FnStatelessF3 (0x803fe43c, 0x59c bytes): traced in full.

CONFIRMED THIS SESSION (independently -- NOT assumed identical to
FnStatelessQ, which shares only the top-level "map object with a
keyframe-time-table + counts" shape, not the field offsets or the
per-frame math):

  map = this+0x0C, and for this nested (tag 0x17) object, map == the
  TrackMarker's own abs_off (ClipBlock+0x10's relocation target) --
  the marker header doubles as its own AnimMemoryMap, same pattern
  FnStatelessQ showed for the tag-0x16 container.

  map+0x08  ptr   KEYFRAME TIME table (u16 array) -- corresponds to
                  TrackMarker.bone_list_ptr_rel in the current parser
                  (a generic/misleading name; for this codec it is a
                  time table, not a bone list -- same relabeling
                  FnStatelessQ's map+0x08 needed).
  map+0x0C  ptr   PER-CHANNEL BONE INDEX table, u16 entries -- corresponds
                  to TrackMarker.dense_list_ptr_rel. This IS the
                  channel->bone mapping (confirmed via its direct use:
                  `lhz r7,0(r5); slwi r7,r7,2; add r11,r4,r7` -- each
                  channel's u16 entry is multiplied by 4 and added to
                  the output pose-buffer base, i.e. a float-index bone
                  offset, read fresh per channel from this table).
  map+0x10  u16   KEYFRAME_COUNT (the search loop's `lhz r6,0x10(r8)`
                  upper bound).
  map+0x12  u8    CHANNEL_COUNT.
  map+0x13  u8    EXTRA_CHANNEL_COUNT (mirrors FnStatelessQ's map+0x17,
                  a second smaller region handled by a separate copy loop
                  at the end of EvalSQT, confirmed present but its
                  source data is a plain float copy, not decoded here --
                  see "extra channels" note below).
  map+0x18  --    BASIS table, CHANNEL_COUNT * 0x20 (32) bytes/channel.
                  Only 3 of its fields are read in the per-frame path:
                    +0x10  f32  scale_x
                    +0x14  f32  scale_y
                    +0x18  f32  scale_z
                  (+0x00..+0x0C are not read anywhere in EvalSQT's main
                  path -- likely bind-pose/init-only fields, unconfirmed,
                  not needed for a straight sequential decode).
  (map+0x18 + CHANNEL_COUNT*0x20)
            --    KEYFRAME DATA table, KEYFRAME_COUNT blocks of
                  CHANNEL_COUNT * 6 bytes each (keyframe-major,
                  channel-minor within a keyframe: 3 x i16 BE per
                  channel = raw X,Y,Z).

  Per-channel, per-query-frame decode (EvalSQT main path, non-bonemask):
    1. Binary-search map+0x08's time table for the query frame, giving
       a keyframe index k and (if between two keyframes) an interpolation
       fraction `t` in [0,1] (computed via fsubs/fdivs against the two
       bracketing keyframe times) -- exact mechanism mirrors a standard
       lerp-keyframe search, confirmed via the fcmpu/fdivs sequence at
       0x803fe5a4-0x803fe634.
    2. If the query lands exactly on a keyframe (no interpolation needed,
       flagged via r6==0): decode ONE raw XYZ triple per channel, apply
       int_to_float_trick (raw i16, sign-flipped via `xoris ...,0x8000`,
       i.e. treated as an *offset-binary* 16-bit value, not a plain
       signed int16 -- same trick shape as elsewhere in this project but
       a different bit convention, confirmed via the explicit
       0x8000-XOR immediately preceding every trick load) times the
       per-axis scale_x/y/z. NO bias/pivot term is added -- output is a
       pure `raw_trick * scale` product, unlike every delta codec in
       this project which adds a base/pivot on top of a scaled term.
    3. If interpolation is needed: decode the SAME way at both bracketing
       keyframes k and k+1, then linearly interpolate the two scaled
       results with fraction `t`:
           out = lerp(scaled_k, scaled_k1, t) = scaled_k + t*(scaled_k1-scaled_k)
       (confirmed via `fmadds f3,f2,f0,f9` where f2=t, f9=scaled_k,
       f0=scaled_k1-scaled_k -- a standard FMA-lerp, x3 for X/Y/Z).
    4. Output is written bone-major into a float pose buffer: bone_idx
       (read fresh per channel from map+0x0C) * 4 gives a float-index
       offset into the caller-supplied buffer `r4`; X goes to
       buffer[bone_idx*4+0], Y to +1, Z to +2 (NOT a full SQT/quaternion
       slot -- just 3 floats, i.e. this really is a translation-shaped
       write, structurally different from FnStatelessQ's quaternion
       write into a different buffer region).

  EXTRA CHANNELS (map+0x13, present when >0): a second, separate copy
  loop (0x803fe93c-0x803fe9b8) that reads floats directly (not raw i16 +
  scale) from a data region computed via a DIFFERENT stride formula and
  copies 3 floats per extra-channel entry straight into the same
  bone-major output buffer (bone index again read per-channel from the
  SAME map+0x0C table, offset past the main channel_count entries).
  This mirrors the "extra channel" region flagged-but-unexplained in
  FnStatelessQ's own session -- NOT independently decoded numerically
  in this validator (flagged, not guessed), since the source data
  layout for these entries needs its own disassembly pass.

NOT decoded here (flagged, not silently skipped):
  - EXTRA_CHANNEL_COUNT region's raw byte layout (see above).
  - The BoneMask-gated variant (EvalSQTMask, 0x803fdfc8) -- irrelevant
    for a full-channel sequential decode.
  - map+0x00..+0x0C of each 32-byte basis record (bind-pose/init fields,
    unused in this per-frame path).
"""

import struct
import sys
import math
import json

from .eagl_anm_decoder import (
    _read_sections, _read_symbols, _self_reloc_map,
    _find_bank, _parse_bank_header, _read_table_a, _parse_clip_block,
    MAX_BONES,
)

from .paths import ANM_PATH as PATH


def int_to_float_trick_i16(raw_u16: int) -> float:
    """raw i16 field, offset-binary convention (xoris ...,0x8000 in the
    disassembly): flip the sign bit, then convert to float. The
    magic-double bit trick this used to run through reduces to a plain
    float() of the flipped value -- see fn_delta_qfast.py's
    int_to_float_trick for the full rationale (verified by exhaustive
    sweep over all 16-bit inputs)."""
    return float(raw_u16 ^ 0x8000)


def resolve_ptr(data, reloc, data_start, base_abs_off, field_off):
    base_rel = base_abs_off - data_start
    target_rel = reloc.get(base_rel + field_off)
    if target_rel is None:
        return None
    return data_start + target_rel


def load_all_clips():
    data = open(PATH, "rb").read()
    sections, data_start = _read_sections(data)
    symbols = _read_symbols(data, sections)
    reloc = _self_reloc_map(data, sections, data_start)
    bank_off = _find_bank(data, symbols, data_start)
    bh = _parse_bank_header(data, bank_off)
    table_a = _read_table_a(data, reloc, bh)

    clip_blocks = []
    for i, rel in enumerate(table_a):
        if rel is None:
            continue
        cb = _parse_clip_block(data, reloc, data_start, i, rel)
        if cb is not None:
            clip_blocks.append(cb)
    return data, reloc, data_start, clip_blocks


def decode_clip_fnstatelessf3(data, reloc, data_start, cb, compute_stats: bool = True):
    if cb.secondary is None or cb.secondary.tag != 0x17:
        return None
    map_off = cb.secondary.abs_off  # marker doubles as its own map, per FnStatelessQ precedent

    keyframe_count = struct.unpack_from(">H", data, map_off + 0x10)[0]
    channel_count = data[map_off + 0x12]
    extra_count = data[map_off + 0x13]

    basis_table_off = map_off + 0x18
    basis_stride = 0x20
    keyframe_table_off = basis_table_off + channel_count * basis_stride
    keyframe_stride = channel_count * 6

    if keyframe_table_off + keyframe_count * keyframe_stride > len(data):
        return {"clip_id": cb.index, "error": "keyframe table runs past EOF -- addressing likely wrong",
                "keyframe_count": keyframe_count, "channel_count": channel_count}

    # channel -> bone table (map+0x0C)
    # CORRECTED this pass: raw u16 entries are NOT direct bone indices.
    # Cross-clip inspection showed the table is a FIXED, clip-independent
    # 68-entry array (values 8,20,32,...800,812 -- arithmetic, step 12,
    # identical across all 12 clips) from which each clip selects a subset
    # for "channel" (keyframe-animated) vs "extra" (static-copy) use.
    # Formula, confirmed by 0 invalid entries across the whole corpus:
    #     bone_idx = (raw_entry - 8) // 12   -> resolves cleanly to 0..67.
    # Matches the ASM's own `slwi r7,r7,2` (raw_entry*4): raw_entry*4 =
    # (12*bone+8)*4 = bone*0x30 + 0x20 -- writes into the SAME 0x30-byte
    # per-bone SQT record every other codec targets, at +0x20 (translation
    # slot). Flagged as inferred from corpus regularity, not independently
    # re-confirmed via disassembly of the pose-record layout itself.
    bone_table_abs = resolve_ptr(data, reloc, data_start, map_off, 0x0C)
    bone_table = None
    if bone_table_abs is not None and bone_table_abs + (channel_count + extra_count) * 2 <= len(data):
        raw = struct.unpack_from(f">{channel_count + extra_count}H", data, bone_table_abs)
        bone_table = []
        for e in raw[:channel_count]:
            num = e - 8
            bone_table.append(num // 12 if num >= 0 and num % 12 == 0 else None)

    # keyframe time table (map+0x08)
    time_table_abs = resolve_ptr(data, reloc, data_start, map_off, 0x08)
    times = None
    if time_table_abs is not None and time_table_abs + keyframe_count * 2 <= len(data):
        times = list(struct.unpack_from(f">{keyframe_count}H", data, time_table_abs))

    # per-channel scale basis
    scales = []
    for ch in range(channel_count):
        off = basis_table_off + ch * basis_stride
        sx, sy, sz = struct.unpack_from(">3f", data, off + 0x10)
        scales.append((sx, sy, sz))

    def decode_keyframe_raw(k, ch):
        off = keyframe_table_off + k * keyframe_stride + ch * 6
        rx, ry, rz = struct.unpack_from(">3H", data, off)
        sx, sy, sz = scales[ch]
        return (int_to_float_trick_i16(rx) * sx,
                int_to_float_trick_i16(ry) * sy,
                int_to_float_trick_i16(rz) * sz)

    # decode every keyframe (sequential, no query-time interpolation needed
    # for a full-sequence corpus validation -- we want the raw per-keyframe
    # values plus a densified per-sample lerp using `times` if present)
    frames_by_channel = [[] for _ in range(channel_count)]
    vec_mags = []
    step_lens = []

    for ch in range(channel_count):
        prev = None
        for k in range(keyframe_count):
            v = decode_keyframe_raw(k, ch)
            frames_by_channel[ch].append(v)
            if compute_stats:
                vec_mags.append(math.sqrt(sum(c * c for c in v)))
                if prev is not None:
                    step_lens.append(math.sqrt(sum((a - b) ** 2 for a, b in zip(v, prev))))
                prev = v

    def stat(lst):
        return {"mean": sum(lst) / len(lst) if lst else None,
                "min": min(lst) if lst else None,
                "max": max(lst) if lst else None}

    return {
        "clip_id": cb.index,
        "codec": "FnStatelessF3",
        "keyframe_count": keyframe_count,
        "channel_count": channel_count,
        "extra_count": extra_count,
        "bone_table": bone_table,
        "times": times,
        "frames_by_channel": frames_by_channel,
        "stats": {"vec_mag": stat(vec_mags), "step_len": stat(step_lens)},
    }


def main():
    data, reloc, data_start, clips = load_all_clips()
    targets = [cb for cb in clips if cb.secondary is not None and cb.secondary.tag == 0x17]
    print(f"Found {len(targets)} clips with FnStatelessF3 (tag 0x17) secondary track\n")

    results = []
    for cb in targets:
        r = decode_clip_fnstatelessf3(data, reloc, data_start, cb)
        results.append(r)

    print(f"{'clip':>4} {'kf_cnt':>6} {'ch_cnt':>6} {'extra':>5} {'bones_ok':>8} "
          f"{'times_ok':>8} {'mag_mean':>9} {'mag_max':>9} {'step_mean':>9} {'step_max':>9}")
    for r in results:
        if r is None or "error" in r:
            print(f"{r['clip_id'] if r else '?':>4}  ERROR: {r.get('error') if r else 'decode returned None'}")
            continue
        s = r["stats"]
        bones_ok = r["bone_table"] is not None
        times_ok = r["times"] is not None
        mm = s["vec_mag"]["mean"]
        mx = s["vec_mag"]["max"]
        sm = s["step_len"]["mean"] if s["step_len"]["mean"] is not None else float("nan")
        smx = s["step_len"]["max"] if s["step_len"]["max"] is not None else float("nan")
        print(f"{r['clip_id']:>4} {r['keyframe_count']:>6} {r['channel_count']:>6} {r['extra_count']:>5} "
              f"{str(bones_ok):>8} {str(times_ok):>8} "
              f"{(mm if mm is not None else float('nan')):>9.4f} {(mx if mx is not None else float('nan')):>9.4f} "
              f"{sm:>9.4f} {smx:>9.4f}")

    # bone distribution check -- the actual question the user asked
    print("\n=== CHANNEL -> BONE DISTRIBUTION (all clips) ===")
    all_bones = []
    for r in results:
        if r and r.get("bone_table"):
            all_bones.extend(r["bone_table"])
    if all_bones:
        from collections import Counter
        c = Counter(all_bones)
        print(f"Total channel entries across corpus: {len(all_bones)}")
        print(f"Distinct bones referenced: {len(c)}")
        print(f"Bone 0 (root) channel count: {c.get(0, 0)}")
        print(f"Top 10 most-referenced bones: {c.most_common(10)}")
        oob = [b for b in all_bones if b >= MAX_BONES]
        print(f"Out-of-range bone indices (>= {MAX_BONES}): {len(oob)}")
    else:
        print("No bone tables resolved -- cannot assess distribution.")

    import os as _os
    _out_path = _os.path.join(_os.path.dirname(_os.path.dirname(_os.path.abspath(__file__))), "output", "fnstatelessf3_validation.json")
    _os.makedirs(_os.path.dirname(_out_path), exist_ok=True)
    with open(_out_path, "w") as f:
        json.dump(results, f, indent=2, default=str)
    print(f"\nWrote per-clip results to {_out_path}")


if __name__ == "__main__":
    main()
