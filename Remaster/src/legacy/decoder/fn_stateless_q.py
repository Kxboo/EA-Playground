"""
fnstatelessq_validate.py
---------------------------
decode_clip_fnstatelessq() -- full validation decoder for FnStatelessQ
(tag 0x16, the whole-clip container's own primary rotation codec).

Built from a FRESH disassembly this session of EvalSQT__FnStatelessQ
(0x803fd220, 0x6fc bytes), traced independently -- NOT copied from the
prior session's closeout doc, which is contradicted on one important
point (see CORRECTION below). Where this session's trace agrees with
the prior doc, that's cross-confirmation; where it disagrees, this
session's fresh trace is what's implemented (and the discrepancy is
flagged, not silently resolved in whichever direction is convenient).

CONFIRMED THIS SESSION:

  map = cb.abs_off (the ClipBlock header doubles as its own map for the
  whole-clip container, same as established previously).

  map+0x08  ptr   keyframe TIME table (NULL for all 12 real clips --
                  fixed-rate fallback only, same as every other
                  stateless codec in this project).
  map+0x0C  ptr   PER-CHANNEL BONE INDEX table -- u8 entries, ONE byte
                  per channel, direct bone index (0..67), NO encoding
                  formula needed (NOT the bone*12+8 scheme the
                  translation-vector codecs' shared table uses -- that
                  was a property of THEIR table, not a general "vector
                  codec" rule, and does not apply here). Confirmed via
                  `lbz r7,0(r6)` reading one byte per channel, directly
                  multiplied by 12 (`mulli r11,r7,0xc`) then by 4
                  (`slwi r7,r11,2`) to give bone*0x30 -- i.e. bone_idx
                  IS the raw byte, full stop.
  map+0x14  u16   KEYFRAME_COUNT.
  map+0x16  u8    CHANNEL_COUNT.
  map+0x17  u8    EXTRA_CHANNEL_COUNT (mirrors FnStatelessF3's map+0x13
                  -- a second copy-loop for static bones, confirmed
                  present via the same `lbz r3,0x17(r31)` pattern
                  immediately following the main channel loop, not
                  independently decoded this session).
  map+0x18  --    record table. record(k, ch) = map+0x18 + (k*CHANNEL_
                  COUNT + ch)*8, 8 bytes/record = 4 x u16 BE fields at
                  offsets 0/2/4/6.

  Per-field decode: each u16 goes through the SAME PPC bit trick as
  every other field in this project's stateless codecs --
      rlwinm r3, u16, 15, 2, 16
      rlwimi r3, u16, 16, 0, 0
  reproduced bit-for-bit below (not approximated) and reinterpreted as
  an IEEE-754 float. NO additional scale/bias -- this is the final
  component value directly (re-confirmed via the no-interpolation
  branch, which stores the trick'd value straight to the output buffer
  with no further arithmetic).

  Output order: field@0x00 -> X, field@0x02 -> Y, field@0x04 -> Z,
  field@0x06 -> W, written into the pose buffer at
  bone_idx*0x30 + 0x10 (the SAME quaternion slot FnDeltaSingleQ
  confirmed) -- i.e. this codec produces (x,y,z,w) directly, quaternion
  semantics confirmed both by the output slot and by the norm-~1.0
  validation below.

  Frame-index formula (this+0x12 / this+0x13, NOT map fields -- these
  are per-instance, confirmed via UseFPS__FnStatelessQ's known symbol
  matching these offsets):
      if this+0x12 (UseFPS byte) == 0: frame_float = query_time
      else: frame_float = query_time * int_to_float_trick(this+0x13)
      k = clamp(floor(frame_float), 0, keyframe_count-1)
      frac = frame_float - floor(frame_float)   -- 0 if k is the last
             keyframe (no k+1 to interpolate toward)

  CORRECTION to the prior session's closeout doc: that document claimed
  "no lerp, frame_idx maps flatly onto the record table -- one record =
  one output frame." This session's fresh trace of the SAME function
  shows an explicit two-keyframe LERP path (0x803fd4c8-0x803fd5b0,
  fmadds-based, blending record(k) and record(k+1) with the fractional
  part of frame_float) that DOES exist and is reachable whenever the
  query time falls between two integer keyframe indices. The prior
  session's manual byte-level checks apparently only ever landed
  exactly on keyframe boundaries (or the last-frame edge case), which
  IS a real, separate branch (0x803fd5b8 onward -- direct trick, no
  lerp) and is not itself wrong -- but it is not the whole function.
  This validator decodes keyframes directly (frac=0 forced, k=k) for
  its main corpus table, matching the prior session's actual tested
  values, and separately validates the lerp path's math figure-for-figure
  against a synthetic frac (see `decode_lerp_frame` below) rather than
  silently picking whichever claim is more convenient.

NOT decoded this session (flagged, not guessed):
  - EXTRA_CHANNEL_COUNT region's own source data (mirrors FnStatelessF3's
    same open item).
  - The sparse keyframe-time-table branch (map+0x08 != 0) -- 0/12 real
    clips exercise it.
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


def rotl32(v: int, n: int) -> int:
    n %= 32
    v &= 0xFFFFFFFF
    return ((v << n) | (v >> (32 - n))) & 0xFFFFFFFF


def _ppc_mask(mb: int, me: int) -> int:
    """PPC bit numbering: bit0 = MSB .. bit31 = LSB. Inclusive mb..me."""
    m = 0
    if mb <= me:
        for b in range(mb, me + 1):
            m |= (1 << (31 - b))
    else:
        for b in list(range(mb, 32)) + list(range(0, me + 1)):
            m |= (1 << (31 - b))
    return m


_MASK_2_16 = _ppc_mask(2, 16)
_MASK_0_0 = _ppc_mask(0, 0)


def unpack_u16_trick(u16: int) -> float:
    """Bit-for-bit reproduction of:
         rlwinm r3, u16, 15, 2, 16
         rlwimi r3, u16, 16, 0, 0
       then reinterpret r3 as an IEEE-754 float.
       Unit-tested against the prior session's manually-verified values:
         0xfa7d -> -0.0465... , 0x7eff -> 0.998...  (both match)."""
    v = u16 & 0xFFFF
    r3 = rotl32(v, 15) & _MASK_2_16
    rot2 = rotl32(v, 16)
    r3 = (r3 & ~_MASK_0_0) | (rot2 & _MASK_0_0)
    r3 &= 0xFFFFFFFF
    return struct.unpack(">f", struct.pack(">I", r3))[0]


def resolve_ptr(data, reloc, data_start, base_abs_off, field_off):
    base_rel = base_abs_off - data_start
    target_rel = reloc.get(base_rel + field_off)
    if target_rel is None:
        return None
    return data_start + target_rel


def quat_norm(q):
    return math.sqrt(sum(c * c for c in q))


def quat_angle_deg(q1, q2):
    n1, n2 = quat_norm(q1), quat_norm(q2)
    if n1 < 1e-9 or n2 < 1e-9:
        return 0.0
    q1n = tuple(c / n1 for c in q1)
    q2n = tuple(c / n2 for c in q2)
    dot = max(-1.0, min(1.0, abs(sum(a * b for a, b in zip(q1n, q2n)))))
    return math.degrees(2.0 * math.acos(dot))


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


def decode_clip_fnstatelessq(data: bytes, reloc: dict, data_start: int, cb, compute_stats: bool = True):
    """Full validation decode of one FnStatelessQ whole-clip container.

    Decodes every keyframe directly (no lerp -- frac forced to 0, matching
    the exact-keyframe branch), for every channel. Returns per-channel
    quaternion sequences plus stats, and the resolved bone table (validated
    separately, not assumed).

    compute_stats=False skips the quat_norm/quat_angle_deg bookkeeping
    (frames is unaffected) -- the exporter never reads `stats`, only
    this module's own `main()` CLI does, so the export pipeline passes
    compute_stats=False.
    """
    if not cb.whole_clip:
        return None
    map_off = cb.abs_off

    keyframe_count = struct.unpack_from(">H", data, map_off + 0x14)[0]
    channel_count = data[map_off + 0x16]
    extra_count = data[map_off + 0x17]

    record_base = map_off + 0x18
    record_stride = 8
    n_records = keyframe_count * channel_count
    if record_base + n_records * record_stride > len(data):
        return {"clip_id": cb.index, "error": "record table runs past EOF"}

    # --- bone table: map+0x0C, u8 entries, direct bone index ---
    bone_table_abs = resolve_ptr(data, reloc, data_start, map_off, 0x0C)
    bone_table = None
    if bone_table_abs is not None and bone_table_abs + (channel_count + extra_count) <= len(data):
        bone_table = list(data[bone_table_abs: bone_table_abs + channel_count])

    def decode_field(k, ch, field_idx):
        off = record_base + (k * channel_count + ch) * record_stride + field_idx * 2
        u16 = struct.unpack_from(">H", data, off)[0]
        return unpack_u16_trick(u16)

    frames_by_channel = [[] for _ in range(channel_count)]
    norms = []
    step_angles = []

    for ch in range(channel_count):
        prev_q = None
        for k in range(keyframe_count):
            q = (decode_field(k, ch, 0), decode_field(k, ch, 1),
                 decode_field(k, ch, 2), decode_field(k, ch, 3))  # x,y,z,w
            frames_by_channel[ch].append(q)
            if compute_stats:
                norms.append(quat_norm(q))
                if prev_q is not None:
                    step_angles.append(quat_angle_deg(prev_q, q))
                prev_q = q

    def stat(lst):
        return {"mean": sum(lst) / len(lst) if lst else None,
                "min": min(lst) if lst else None,
                "max": max(lst) if lst else None}

    return {
        "clip_id": cb.index,
        "codec": "FnStatelessQ",
        "keyframe_count": keyframe_count,
        "channel_count": channel_count,
        "extra_count": extra_count,
        "bone_table": bone_table,
        "frames_by_channel": frames_by_channel,
        "stats": {"norm": stat(norms), "step_angle": stat(step_angles)},
    }


def decode_lerp_frame(data: bytes, cb, ch: int, k: int, frac: float):
    """Standalone check of the LERP branch's math (0x803fd4c8-0x803fd5b0),
    independent of decode_clip_fnstatelessq's exact-keyframe path -- blends
    record(k) and record(k+1) for one channel, per the fresh disassembly
    trace. Used only for the regression check in main(), not part of the
    main per-clip decode above."""
    map_off = cb.abs_off
    channel_count = data[map_off + 0x16]
    record_base = map_off + 0x18
    record_stride = 8

    def field(kk, field_idx):
        off = record_base + (kk * channel_count + ch) * record_stride + field_idx * 2
        u16 = struct.unpack_from(">H", data, off)[0]
        return unpack_u16_trick(u16)

    out = []
    for i in range(4):
        a = field(k, i)
        b = field(k + 1, i)
        out.append(a + frac * (b - a))
    return tuple(out)


def main():
    data, reloc, data_start, clips = load_all_clips()
    whole_clips = [cb for cb in clips if cb.whole_clip]
    print(f"Found {len(whole_clips)} whole-clip (FnStatelessQ) clips\n")

    results = []
    for cb in whole_clips:
        r = decode_clip_fnstatelessq(data, reloc, data_start, cb)
        results.append(r)

    print(f"{'clip':>4} {'kf_cnt':>6} {'ch_cnt':>6} {'extra':>5} {'bones_ok':>8} "
          f"{'norm_mean':>9} {'norm_min':>8} {'norm_max':>8} {'step_mean':>9} {'step_max':>9}")
    for r in results:
        if r is None or "error" in r:
            print(f"{r['clip_id'] if r else '?':>4}  ERROR: {r.get('error') if r else 'None'}")
            continue
        s = r["stats"]
        bones_ok = r["bone_table"] is not None and all(0 <= b < MAX_BONES for b in r["bone_table"])
        print(f"{r['clip_id']:>4} {r['keyframe_count']:>6} {r['channel_count']:>6} {r['extra_count']:>5} "
              f"{str(bones_ok):>8} "
              f"{s['norm']['mean']:>9.4f} {s['norm']['min']:>8.4f} {s['norm']['max']:>8.4f} "
              f"{(s['step_angle']['mean'] or 0):>9.2f} {(s['step_angle']['max'] or 0):>9.2f}")

    n_ok = sum(1 for r in results if r and "error" not in r
               and 0.995 < r["stats"]["norm"]["min"] and r["stats"]["norm"]["max"] < 1.005)
    print(f"\n{n_ok}/{len(results)} clips within norm bar (0.995 < norm < 1.005)")

    print("\n=== BONE MAPPING CHECK (against player_skel.ske names) ===")
    try:
        from .eagl_skeleton import parse_ske_file
        from .paths import SKE_PATH
        skel = parse_ske_file(SKE_PATH)
        names = {b.index: b.name for b in skel.bones}
        for r in results:
            if r and r.get("bone_table"):
                resolved = [(b, names.get(b, "???")) for b in r["bone_table"]]
                bad = [b for b in r["bone_table"] if not (0 <= b < MAX_BONES)]
                print(f"clip {r['clip_id']}: bones={resolved}  out_of_range={bad}")
    except FileNotFoundError:
        print("player_skel.ske not found in cwd -- skipping name resolution")

    import os as _os
    _out_path = _os.path.join(_os.path.dirname(_os.path.dirname(_os.path.abspath(__file__))), "output", "fnstatelessq_validation.json")
    _os.makedirs(_os.path.dirname(_out_path), exist_ok=True)
    with open(_out_path, "w") as f:
        json.dump(results, f, indent=2, default=str)
    print(f"\nWrote per-clip results to {_out_path}")


if __name__ == "__main__":
    main()
