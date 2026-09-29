"""
fndeltaf3_validate.py
----------------------
Validation decoder for FnDeltaF3 (tag 0x14), the dominant secondary
(translation) track. Deliberately mirrors the ELF-traced dataflow
1:1 -- no algebraic simplification, no merging with FnDeltaQFast's
model, so a wrong assumption shows up as wrong numbers instead of
being smoothed away by a "cleaner" reimplementation.

Session 21 confirmed (via full float-register dataflow trace of
EvalSQT__FnDeltaF3, 0x80402d80-0x80402e30):

  36-byte per-channel input record at marker+0x14 + ch*0x24:
    +0x00/04/08   E_x, E_y, E_z         (f32 BE)
    +0x0C/10/14   B_x, B_y, B_z         (f32 BE)
    +0x18/1A/1C   W0_x, W0_y, W0_z      (u16 BE)
    +0x1E/20/22   W1_x, W1_y, W1_z      (u16 BE)

  per-axis basis (precomputed at runtime by InitBuffersAsRequired,
  reproduced here directly from the file record -- nothing here reads
  playgroundz.elf, only player_anims.anm):
    ref_base    = E
    ref_scale   = B * (1/65534)     [session 26 fix: was wrongly E/65535]
    delta_base  = B * (W0 * (2/65535) - 1)
    delta_scale = B * (W1 * (2/65535)) / 255

  reference sample (one per block, CHANNEL_COUNT x 6 bytes = 3 x u16 BE,
  channel-major -- same region shape as FnDeltaQFast's 6-byte reference,
  just interpreted as XYZ not a quaternion):
    value = ref_base + raw_u16 * ref_scale

  delta samples (sample-major / channel-minor, CHANNEL_COUNT x 3 bytes
  per sample -- same grammar as FnDeltaQFast's delta region):
    d[n]   = delta_base + raw_u8 * delta_scale
    qs[0]  = reference
    qs[n]  = qs[n-1] + d[n]        (forward running accumulation)

Marker header offsets (confirmed, differ from FnDeltaQFast):
    marker+0x0C  SAMPLE_COUNT (u16 BE)
    marker+0x0E  CHANNEL_COUNT (u16 BE)
    marker+0x10  SHIFT (u8)
    marker+0x14  start of the CHANNEL_COUNT x 0x24-byte record table

Block stride (confirmed identical formula to FnDeltaQFast, same
round_down_even step):
    block_size = round_down_even(CHANNEL_COUNT*(6 + 3*(2^SHIFT-1)) + 1)

NOT yet validated this session:
  - the backward/seek path (structurally similar to FnDeltaQFast's
    SubDeltaMask but not traced byte-for-byte)
  - corpus-wide correctness -- that's what this script is for.

This module intentionally does the simplest possible per-axis loop
(no vectorization, no precomputed numpy arrays) so the code reads as
a direct transcription of the traced formulas.
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
INV_65535 = 1.0 / 65535.0
INV_65534 = 1.0 / 65534.0
INV_255 = 1.0 / 255.0


def int_to_float_trick(raw: int) -> float:
    # See fn_delta_qfast.py's int_to_float_trick for the full rationale:
    # this hardware-era PowerPC bit trick is mathematically float(raw)
    # for every input this codec passes it (verified by exhaustive sweep).
    return float(raw)


def read_channel_record(data: bytes, off: int):
    """Read one 36-byte (0x24) per-channel input record."""
    E = struct.unpack_from(">3f", data, off + 0x00)
    B = struct.unpack_from(">3f", data, off + 0x0C)
    W0 = struct.unpack_from(">3H", data, off + 0x18)
    W1 = struct.unpack_from(">3H", data, off + 0x1E)
    return E, B, W0, W1


def compute_basis(record):
    """Direct transcription of InitBuffersAsRequired's precompute, one
    axis at a time -- deliberately not vectorized."""
    E, B, W0, W1 = record
    ref_base = [0.0, 0.0, 0.0]
    ref_scale = [0.0, 0.0, 0.0]
    delta_base = [0.0, 0.0, 0.0]
    delta_scale = [0.0, 0.0, 0.0]
    for i in range(3):
        ref_base[i] = E[i]
        ref_scale[i] = B[i] * INV_65534
        pivot_w0 = W0[i] * (2.0 * INV_65535) - 1.0
        scale_w1 = W1[i] * (2.0 * INV_65535)
        delta_base[i] = B[i] * pivot_w0
        delta_scale[i] = B[i] * scale_w1 * INV_255
    return ref_base, ref_scale, delta_base, delta_scale


def decode_reference_sample(data: bytes, off: int, ref_base, ref_scale):
    raw = struct.unpack_from(">3H", data, off)
    return [ref_base[i] + int_to_float_trick(raw[i]) * ref_scale[i] for i in range(3)]


def decode_delta_sample(data: bytes, off: int, delta_base, delta_scale):
    raw = (data[off], data[off + 1], data[off + 2])
    return [delta_base[i] + int_to_float_trick(raw[i]) * delta_scale[i] for i in range(3)]


def vec_len(v):
    return math.sqrt(sum(c * c for c in v))


def decode_clip_fndeltaf3(data: bytes, cb, chained: bool = True, compute_stats: bool = True):
    """Full validation decode of one FnDeltaF3-secondary clip.

    Session 25+ finding: per-block reference samples are NOT an
    absolute position -- they're redundant with continuous delta
    accumulation and only meaningful for random-access seeking.
    Sequential playback should carry the accumulator across block
    boundaries and ignore the reference's absolute value after the
    first block. `chained=True` (default) reproduces that; pass
    `chained=False` to get the old (buggy) absolute-reference
    behavior for comparison/regression testing.

    compute_stats=False skips the ref/delta/step magnitude bookkeeping
    (frames is unaffected) -- the exporter never reads `stats`, only
    this module's own `main()` CLI does, so the export pipeline passes
    compute_stats=False.

    Returns dict with clip_id, sample_count, channel_count, shift,
    n_blocks, frames[sample][channel] = (x,y,z) or None, stats.
    """
    marker = cb.secondary
    m = marker.abs_off
    sample_count = struct.unpack_from(">H", data, m + 0x0C)[0]
    channel_count = struct.unpack_from(">H", data, m + 0x0E)[0]
    shift = data[m + 0x10]
    record_table_off = m + 0x14
    record_stride = 0x24
    stream_start = record_table_off + channel_count * record_stride

    channels_basis = []
    for ch in range(channel_count):
        rec = read_channel_record(data, record_table_off + ch * record_stride)
        channels_basis.append(compute_basis(rec))

    block_len = 1 << shift
    n_deltas = block_len - 1
    ref_region_len = channel_count * 6
    delta_region_len = n_deltas * channel_count * 3
    raw_block_size = ref_region_len + delta_region_len + 1
    block_size = raw_block_size - (raw_block_size % 2)
    n_blocks = math.ceil(sample_count / block_len)

    frames_by_channel = [[] for _ in range(channel_count)]
    ref_norms = []
    delta_mags = []
    step_lens = []

    carry = [None] * channel_count  # last accumulated value per channel, across block boundaries

    off = stream_start
    for b in range(n_blocks):
        if off + block_size > len(data):
            break
        deltas_this = min(n_deltas, sample_count - 1 - b * block_len) \
            if b == n_blocks - 1 else n_deltas
        deltas_this = max(0, deltas_this)

        for ch in range(channel_count):
            ref_base, ref_scale, delta_base, delta_scale = channels_basis[ch]
            ref_off = off + ch * 6
            if ref_off + 6 > len(data):
                continue
            qs = decode_reference_sample(data, ref_off, ref_base, ref_scale)
            if compute_stats:
                ref_norms.append(vec_len(qs))

            if chained and carry[ch] is not None:
                # Ignore this block's absolute reference value; continue
                # accumulating from where the previous block left off.
                qs = list(carry[ch])
            frames_by_channel[ch].append(tuple(qs))

            for s in range(deltas_this):
                sample_off = off + ref_region_len + s * channel_count * 3 + ch * 3
                if sample_off + 3 > len(data):
                    break
                d = decode_delta_sample(data, sample_off, delta_base, delta_scale)
                qs_new = (qs[0] + d[0], qs[1] + d[1], qs[2] + d[2])
                if compute_stats:
                    delta_mags.append(vec_len(d))
                    step_lens.append(vec_len((qs_new[0] - qs[0], qs_new[1] - qs[1], qs_new[2] - qs[2])))
                frames_by_channel[ch].append(tuple(qs_new))
                qs = qs_new

            carry[ch] = qs

        off += block_size

    frames = []
    for s in range(sample_count):
        frame = []
        for ch in range(channel_count):
            frame.append(frames_by_channel[ch][s] if s < len(frames_by_channel[ch]) else None)
        frames.append(frame)

    def stat(lst):
        return {
            "mean": sum(lst) / len(lst) if lst else None,
            "min": min(lst) if lst else None,
            "max": max(lst) if lst else None,
        }

    stats = {
        "ref_norm": stat(ref_norms),
        "delta_mag": stat(delta_mags),
        "step_len": stat(step_lens),
        "n_ref_samples": len(ref_norms),
        "n_delta_samples": len(delta_mags),
    }

    return {
        "clip_id": cb.index,
        "codec": "FnDeltaF3",
        "sample_count": sample_count,
        "channel_count": channel_count,
        "shift": shift,
        "n_blocks": n_blocks,
        "frames": frames,
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
        if cb and cb.secondary and cb.secondary.tag == 0x14:
            clip_blocks.append(cb)
    return data, clip_blocks


def main():
    import argparse
    ap = argparse.ArgumentParser(description="FnDeltaF3 validation decoder")
    ap.add_argument("--clip", type=int, default=None)
    ap.add_argument("--channel", type=int, default=None, help="print one channel's full trajectory (requires --clip)")
    ap.add_argument("--json-out", type=str, default=None)
    args = ap.parse_args()

    data, clip_blocks = load_clips()
    print(f"{len(clip_blocks)} FnDeltaF3-secondary clips found")

    if args.clip is not None:
        cb = next((c for c in clip_blocks if c.index == args.clip), None)
        if cb is None:
            print(f"clip {args.clip} not found among FnDeltaF3-secondary clips")
            return
        result = decode_clip_fndeltaf3(data, cb)
        s = result["stats"]
        print(f"clip {result['clip_id']}  sample_count={result['sample_count']} "
              f"channel_count={result['channel_count']} shift={result['shift']} "
              f"n_blocks={result['n_blocks']}")
        print(f"  ref_norm:  mean={s['ref_norm']['mean']:.4f} "
              f"min={s['ref_norm']['min']:.4f} max={s['ref_norm']['max']:.4f}  (n={s['n_ref_samples']})")
        print(f"  delta_mag: mean={s['delta_mag']['mean']:.6f} "
              f"min={s['delta_mag']['min']:.6f} max={s['delta_mag']['max']:.6f}  (n={s['n_delta_samples']})")
        print(f"  step_len:  mean={s['step_len']['mean']:.6f} "
              f"min={s['step_len']['min']:.6f} max={s['step_len']['max']:.6f}")

        if args.channel is not None:
            print(f"\nchannel {args.channel} trajectory:")
            for si, frame in enumerate(result["frames"]):
                v = frame[args.channel]
                if v is None:
                    print(f"  sample {si:3d}: (missing)")
                else:
                    print(f"  sample {si:3d}: x={v[0]:+.5f} y={v[1]:+.5f} z={v[2]:+.5f}")

        if args.json_out:
            with open(args.json_out, "w") as f:
                json.dump(result, f, indent=2)
            print(f"\nwrote {args.json_out}")
        return

    print(f"{'clip':>4} {'samples':>7} {'chans':>5} {'blocks':>6} "
          f"{'ref_mean':>9} {'ref_max':>9} {'dmag_mean':>10} {'dmag_max':>9} "
          f"{'step_mean':>10} {'step_max':>9}")
    n_zero_channel = 0
    for cb in clip_blocks:
        r = decode_clip_fndeltaf3(data, cb)
        s = r["stats"]
        if s["ref_norm"]["mean"] is None:
            n_zero_channel += 1
            print(f"{r['clip_id']:>4} {r['sample_count']:>7} {r['channel_count']:>5} {r['n_blocks']:>6}"
                  f"  -- CHANNEL_COUNT==0, no translation channels (anomaly, see writeup)")
            continue
        print(f"{r['clip_id']:>4} {r['sample_count']:>7} {r['channel_count']:>5} {r['n_blocks']:>6} "
              f"{s['ref_norm']['mean']:>9.4f} {s['ref_norm']['max']:>9.4f} "
              f"{s['delta_mag']['mean']:>10.6f} {s['delta_mag']['max']:>9.6f} "
              f"{s['step_len']['mean']:>10.6f} {s['step_len']['max']:>9.6f}")
    if n_zero_channel:
        print(f"\n{n_zero_channel}/{len(clip_blocks)} clips had CHANNEL_COUNT==0 "
              f"(excluded from aggregate stats below)")


if __name__ == "__main__":
    main()
