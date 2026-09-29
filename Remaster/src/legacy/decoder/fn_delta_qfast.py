"""
anm_validate.py
-----------------
Reference validation decoder for FnDeltaQFast (tag 0x12), and the
framework future codecs (FnDeltaF3, FnDeltaF1, FnStatelessF3,
FnDeltaSingleQ, FnStatelessQ) should plug into.

Confirmed structure, disassembly-verified against playgroundz.elf:

  - block stride = round_down_even(CHANNEL_COUNT*(3*(2^SHIFT-1)+6)+1)
    [SetAnimMemoryMap__FnDeltaQFast, 0x804016a8-0x804016d0]
  - stream_start  = marker.payload_off + CHANNEL_COUNT*16
    [SetAnimMemoryMap__FnDeltaQFast, 0x80401594-0x804015bc]
  - block layout: CHANNEL_COUNT*6-byte reference region (channel-major),
    then (2^SHIFT-1)*CHANNEL_COUNT*3-byte delta region (sample-major,
    channel-minor), then a 1-byte trailer IF the raw (pre-rounding) size
    was already even (else no trailer byte exists at all -- swallowed by
    the rounding)
  - per-sample reconstruction: qs[0] = block reference (read once per
    block); qs[n] = qs[n-1] + delta[n], forward accumulation only
    [UpdateNextQs__FnDeltaQFast, 0x80400994 / AddDeltaMask, 0x804000e8]
    (SubDeltaMask is the same series run backward for random-access
    seeking -- irrelevant for a full sequential decode of every frame)

This module intentionally does NOT go further into AddDeltaMask/
SubDeltaMask's bone-mask-gated skip logic, GetAnimatedBonesAux, or any
other helper -- per the current plan, those are off the critical path
now that full-sequence forward decode is validated corpus-wide.
"""

import struct
import sys
import math
import json

from .eagl_anm_decoder import (
    _read_sections, _read_symbols, _self_reloc_map,
    _find_bank, _parse_bank_header, _read_table_a, _parse_clip_block,
)

from .paths import ANM_PATH as PATH

MAGIC2_52 = 4503599627370496.0
REF_SCALE = 2.0 / 4095.0
REF_BIAS = 1.0
DELTA_SCALE_CONST = 1.0 / 63.0


def int_to_float_trick(raw: int) -> float:
    # Was a bit-for-bit replication of the PowerPC/Gekko hardware
    # int->float conversion trick (append raw as the mantissa of 2^52,
    # let the FPU reinterpret it as a double, subtract 2^52). Python's
    # native int->float conversion isn't under that CPU's constraints,
    # so this reduces to a plain float() with identical output for
    # every representable input (verified by exhaustive sweep -- see
    # eagl_python_optimization_findings.md, Finding 1).
    return float(raw)


def decode_reference(data: bytes, off: int):
    X, Y, Z = struct.unpack_from(">HHH", data, off)
    x_raw = (X >> 4) & 0xFFF
    y_raw = (Y >> 4) & 0xFFF
    z_raw = (Z >> 4) & 0xFFF
    w_raw = ((X & 0xF) << 8) | ((Y & 0xF) << 4) | (Z & 0xF)

    def comp(raw):
        return int_to_float_trick(raw) * REF_SCALE - REF_BIAS

    return comp(x_raw), comp(y_raw), comp(z_raw), comp(w_raw)


def decode_delta_raw(data: bytes, off: int):
    b0, b1, b2 = data[off], data[off + 1], data[off + 2]
    x = (b0 >> 2) & 0x3F
    y = (b1 >> 2) & 0x3F
    z = (b2 >> 2) & 0x3F
    w = ((b0 & 0x3) << 4) | ((b1 & 0x3) << 2) | (b2 & 0x3)
    return x, y, z, w


def norm_delta_component(raw: int) -> float:
    return int_to_float_trick(raw) * DELTA_SCALE_CONST


def quat_norm(q):
    return math.sqrt(sum(c * c for c in q))


def quat_normalize(q):
    n = quat_norm(q)
    return tuple(c / n for c in q) if n > 1e-9 else q


def quat_angle_deg(q1, q2):
    q1n, q2n = quat_normalize(q1), quat_normalize(q2)
    dot = max(-1.0, min(1.0, abs(sum(a * b for a, b in zip(q1n, q2n)))))
    return math.degrees(2.0 * math.acos(dot))


def looks_like_fresh_reference(data: bytes, off: int, tol: float = 0.06) -> bool:
    if off + 6 > len(data):
        return False
    return abs(quat_norm(decode_reference(data, off)) - 1.0) <= tol


def decode_clip_fndeltaqfast(data: bytes, cb, compute_stats: bool = True):
    """Full validation decode of one FnDeltaQFast-primary clip.

    Returns a dict:
      clip_id, sample_count, channel_count, shift, n_blocks,
      frames: list[sample_idx] -> list[channel_idx] -> (x,y,z,w)
      stats: {
        ref_norm_mean/min/max,
        final_norm_mean/min/max,
        boundary_hit_rate,
        max_step_angle_deg,          # worst consecutive-sample jump
        mean_step_angle_deg,
      }

    compute_stats=False skips all of the above diagnostic bookkeeping
    (quat_norm/quat_angle_deg calls, boundary-reference checks) and
    returns stats with every field None. `frames` is unaffected either
    way. The exporter never reads `stats` -- only this module's own
    `main()` CLI does -- so the export pipeline calls this with
    compute_stats=False.
    """
    marker = cb.primary
    m = marker.abs_off
    sample_count = struct.unpack_from(">H", data, m + 0x04)[0]
    channel_count = data[m + 0x06]
    shift = data[m + 0x10]
    payload = marker.payload_off
    stream_start = payload + channel_count * 16

    pivots, scales = [], []
    for ch in range(channel_count):
        basis_off = payload + ch * 16
        u = struct.unpack_from(">8H", data, basis_off)
        pivots.append([v * (2.0 / 65535.0) - 1.0 for v in u[0:4]])
        scales.append([v * (2.0 / 65535.0) for v in u[4:8]])

    block_len = 1 << shift
    n_deltas = block_len - 1
    ref_region_len = channel_count * 6
    delta_region_len = n_deltas * channel_count * 3
    raw_block_size = ref_region_len + delta_region_len + 1
    block_size = raw_block_size - (raw_block_size % 2)  # round_down_even
    has_trailer_byte = (block_size == raw_block_size)
    n_blocks = math.ceil(sample_count / block_len)

    # frames[ch] = list of quaternions, one per sample, in sample order
    frames_by_channel = [[] for _ in range(channel_count)]
    ref_norms = []
    final_norms = []
    boundary_checked = 0
    boundary_hits = 0
    step_angles = []

    off = stream_start
    for b in range(n_blocks):
        if off + block_size > len(data):
            break
        deltas_this = min(n_deltas, sample_count - 1 - b * block_len) \
            if b == n_blocks - 1 else n_deltas
        deltas_this = max(0, deltas_this)

        for ch in range(channel_count):
            ref_off = off + ch * 6
            if ref_off + 6 > len(data):
                continue
            ref_q = decode_reference(data, ref_off)
            if compute_stats:
                ref_norms.append(quat_norm(ref_q))
            frames_by_channel[ch].append(ref_q)

            qs = ref_q
            for s in range(deltas_this):
                sample_off = off + ref_region_len + s * channel_count * 3 + ch * 3
                if sample_off + 3 > len(data):
                    break
                rx, ry, rz, rw = decode_delta_raw(data, sample_off)
                pivot, scale = pivots[ch], scales[ch]
                dq = (pivot[0] + norm_delta_component(rx) * scale[0],
                      pivot[1] + norm_delta_component(ry) * scale[1],
                      pivot[2] + norm_delta_component(rz) * scale[2],
                      pivot[3] + norm_delta_component(rw) * scale[3])
                qs_new = (qs[0] + dq[0], qs[1] + dq[1], qs[2] + dq[2], qs[3] + dq[3])
                if compute_stats:
                    final_norms.append(quat_norm(qs_new))
                    step_angles.append(quat_angle_deg(qs, qs_new))
                frames_by_channel[ch].append(qs_new)
                qs = qs_new

        if b < n_blocks - 1:
            if compute_stats:
                boundary_checked += 1
                if looks_like_fresh_reference(data, off + block_size):
                    boundary_hits += 1
        off += block_size

    stats = {
        "ref_norm_mean": sum(ref_norms) / len(ref_norms) if ref_norms else None,
        "ref_norm_min": min(ref_norms) if ref_norms else None,
        "ref_norm_max": max(ref_norms) if ref_norms else None,
        "final_norm_mean": sum(final_norms) / len(final_norms) if final_norms else None,
        "final_norm_min": min(final_norms) if final_norms else None,
        "final_norm_max": max(final_norms) if final_norms else None,
        "boundary_hit_rate": (boundary_hits / boundary_checked) if boundary_checked else None,
        "max_step_angle_deg": max(step_angles) if step_angles else None,
        "mean_step_angle_deg": (sum(step_angles) / len(step_angles)) if step_angles else None,
    }

    # reshape frames_by_channel[ch][sample] -> frames[sample][ch]
    frames = []
    for s in range(sample_count):
        frame = []
        for ch in range(channel_count):
            if s < len(frames_by_channel[ch]):
                frame.append(frames_by_channel[ch][s])
            else:
                frame.append(None)
        frames.append(frame)

    return {
        "clip_id": cb.index,
        "codec": "FnDeltaQFast",
        "sample_count": sample_count,
        "channel_count": channel_count,
        "shift": shift,
        "n_blocks": n_blocks,
        "frames": frames,   # frames[sample_idx][channel_idx] = (x,y,z,w) or None
        "stats": stats,
    }


def load_clips():
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
        if cb and cb.primary and cb.primary.tag == 0x12:
            clip_blocks.append(cb)
    return data, clip_blocks


def main():
    import argparse
    ap = argparse.ArgumentParser(description="FnDeltaQFast validation decoder")
    ap.add_argument("--clip", type=int, default=None, help="decode a single clip ID")
    ap.add_argument("--frame", type=int, default=None, help="print this frame's quaternions (requires --clip)")
    ap.add_argument("--json-out", type=str, default=None, help="write full per-clip results to this JSON file")
    ap.add_argument("--summary-only", action="store_true", help="just print the per-clip summary table")
    args = ap.parse_args()

    data, clip_blocks = load_clips()

    if args.clip is not None:
        cb = next((c for c in clip_blocks if c.index == args.clip), None)
        if cb is None:
            print(f"clip {args.clip} not found among FnDeltaQFast-primary clips")
            return
        result = decode_clip_fndeltaqfast(data, cb)
        s = result["stats"]
        print(f"clip {result['clip_id']}  codec={result['codec']}  "
              f"sample_count={result['sample_count']} channel_count={result['channel_count']} "
              f"shift={result['shift']} n_blocks={result['n_blocks']}")
        print(f"  ref_norm:   mean={s['ref_norm_mean']:.4f} min={s['ref_norm_min']:.4f} max={s['ref_norm_max']:.4f}")
        print(f"  final_norm: mean={s['final_norm_mean']:.4f} min={s['final_norm_min']:.4f} max={s['final_norm_max']:.4f}")
        print(f"  boundary_hit_rate={s['boundary_hit_rate']}")
        print(f"  step_angle: mean={s['mean_step_angle_deg']:.2f} max={s['max_step_angle_deg']:.2f}")

        if args.frame is not None:
            frame = result["frames"][args.frame]
            print(f"\nframe {args.frame}:")
            for ch, q in enumerate(frame):
                if q is None:
                    print(f"  ch {ch:3d}: (missing)")
                else:
                    print(f"  ch {ch:3d}: x={q[0]:+.4f} y={q[1]:+.4f} z={q[2]:+.4f} w={q[3]:+.4f}  norm={quat_norm(q):.4f}")

        if args.json_out:
            with open(args.json_out, "w") as f:
                json.dump(result, f, indent=2)
            print(f"\nwrote {args.json_out}")
        return

    # summary over the whole corpus
    print(f"{'clip':>4} {'samples':>7} {'chans':>5} {'blocks':>6} "
          f"{'ref_mean':>9} {'final_mean':>10} {'bhr':>6} {'step_mean':>9} {'step_max':>8}")
    for cb in clip_blocks:
        r = decode_clip_fndeltaqfast(data, cb)
        s = r["stats"]
        bhr = f"{s['boundary_hit_rate']:.2f}" if s['boundary_hit_rate'] is not None else "n/a"
        print(f"{r['clip_id']:>4} {r['sample_count']:>7} {r['channel_count']:>5} {r['n_blocks']:>6} "
              f"{s['ref_norm_mean']:>9.4f} {s['final_norm_mean']:>10.4f} {bhr:>6} "
              f"{s['mean_step_angle_deg']:>9.2f} {s['max_step_angle_deg']:>8.2f}")


if __name__ == "__main__":
    main()
