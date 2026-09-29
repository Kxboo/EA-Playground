"""
fndeltaf1_validate.py
-----------------------
Validation decoder for FnDeltaF1 (tag 0x15), ELF-confirmed this session
via direct disassembly of InitBuffersAsRequired__FnDeltaF1 (0x80405dc8)
and EvalSQT__FnDeltaF1 (0x80406a44). Single-axis sibling of FnDeltaF3 --
same formula shape, same accumulation model, 1 component instead of 3.

CONFIRMED (disassembly, not inference):
  marker+0x0C  SAMPLE_COUNT (u16 BE)
  marker+0x0E  CHANNEL_COUNT (u16 BE)
  marker+0x10  SHIFT (u8)
  marker+0x14  per-channel input record table, CHANNEL_COUNT x 0x0C (12) bytes:
     +0x00  E   (f32 BE)  ref_base
     +0x04  B   (f32 BE)
     +0x08  W0  (u16 BE)
     +0x0A  W1  (u16 BE)

  basis (InitBuffersAsRequired, 0x80405dc8-0x80405f48):
     ref_base    = E
     ref_scale   = E / 65535
     delta_base  = B * (W0 * (2/65535) - 1)
     delta_scale = B * (W1 * (2/65535)) / 255

  stream_start = marker+0x14 + CHANNEL_COUNT*12

  reference region (EvalSQT 0x80406c94-0x80406cd0): CHANNEL_COUNT x 2
  bytes (one u16/channel), channel-major:
     value = ref_base + raw_u16 * ref_scale

  delta region (EvalSQT 0x80406d2c-0x80406d68): CHANNEL_COUNT x 1 byte
  per sample (one byte/channel), sample-major/channel-minor:
     d[n]  = delta_base + raw_u8 * delta_scale
     qs[0] = reference
     qs[n] = qs[n-1] + d[n]     -- confirmed via lfsx (read previous
             output) -> fadds -> stfsx (store back), same fingerprint
             used to prove accumulation in FnDeltaQFast/FnDeltaF3.

  block stride (EvalSQT 0x80406bb0-0x80406bf8), same round_down_even
  shape as the other two codecs:
     block_size = round_down_even(CHANNEL_COUNT*(2 + 1*(2^SHIFT-1)) + 1)

NOT traced this session: the backward/seek path (fsubs mirror loop at
0x80406dd4-0x80406e14) -- irrelevant for full sequential decode.
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
    E = struct.unpack_from(">f", data, off + 0x00)[0]
    B = struct.unpack_from(">f", data, off + 0x04)[0]
    W0 = struct.unpack_from(">H", data, off + 0x08)[0]
    W1 = struct.unpack_from(">H", data, off + 0x0A)[0]
    return E, B, W0, W1


def compute_basis(record):
    E, B, W0, W1 = record
    ref_base = E
    ref_scale = B * INV_65534
    pivot_w0 = W0 * (2.0 * INV_65535) - 1.0
    scale_w1 = W1 * (2.0 * INV_65535)
    delta_base = B * pivot_w0
    delta_scale = B * scale_w1 * INV_255
    return ref_base, ref_scale, delta_base, delta_scale


def decode_reference_sample(data: bytes, off: int, ref_base, ref_scale):
    raw = struct.unpack_from(">H", data, off)[0]
    return ref_base + int_to_float_trick(raw) * ref_scale


def decode_delta_sample(one_byte: int, delta_base, delta_scale):
    return delta_base + int_to_float_trick(one_byte) * delta_scale


def decode_clip_fndeltaf1(data: bytes, cb, chained: bool = True, compute_stats: bool = True):
    """Full validation decode of one FnDeltaF1-secondary clip.

    Mirrors FnDeltaF3's session-25 finding: per-block reference samples
    are NOT an absolute position -- they're redundant with continuous
    delta accumulation and only meaningful for random-access seeking.
    Sequential playback should carry the accumulator across block
    boundaries and ignore the reference's absolute value after the
    first block. `chained=True` (default) reproduces that; pass
    `chained=False` to get the old (buggy) absolute-reference behavior
    for comparison/regression testing.

    compute_stats=False skips the ref/delta/step magnitude bookkeeping
    (frames is unaffected) -- the exporter never reads `stats`, only
    this module's own `main()` CLI does, so the export pipeline passes
    compute_stats=False.
    """
    marker = cb.secondary
    m = marker.abs_off
    sample_count = struct.unpack_from(">H", data, m + 0x0C)[0]
    channel_count = struct.unpack_from(">H", data, m + 0x0E)[0]
    shift = data[m + 0x10]
    record_table_off = m + 0x14
    record_stride = 0x0C
    stream_start = record_table_off + channel_count * record_stride

    channels_basis = []
    for ch in range(channel_count):
        rec = read_channel_record(data, record_table_off + ch * record_stride)
        channels_basis.append(compute_basis(rec))

    block_len = 1 << shift
    n_deltas = block_len - 1
    ref_region_len = channel_count * 2
    delta_region_len = n_deltas * channel_count * 1
    raw_block_size = ref_region_len + delta_region_len + 1
    block_size = raw_block_size - (raw_block_size % 2)
    n_blocks = math.ceil(sample_count / block_len)

    frames_by_channel = [[] for _ in range(channel_count)]
    ref_vals = []
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
            ref_off = off + ch * 2
            if ref_off + 2 > len(data):
                continue
            qs = decode_reference_sample(data, ref_off, ref_base, ref_scale)
            if compute_stats:
                ref_vals.append(qs)

            if chained and carry[ch] is not None:
                # Ignore this block's absolute reference value; continue
                # accumulating from where the previous block left off.
                qs = carry[ch]
            frames_by_channel[ch].append(qs)

            for s in range(deltas_this):
                sample_off = off + ref_region_len + s * channel_count + ch
                if sample_off >= len(data):
                    break
                raw_byte = data[sample_off]
                d = decode_delta_sample(raw_byte, delta_base, delta_scale)
                qs_new = qs + d
                if compute_stats:
                    delta_mags.append(abs(d))
                    step_lens.append(abs(qs_new - qs))
                frames_by_channel[ch].append(qs_new)
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
        "ref_val": stat(ref_vals),
        "delta_mag": stat(delta_mags),
        "step_len": stat(step_lens),
        "n_ref_samples": len(ref_vals),
        "n_delta_samples": len(delta_mags),
    }

    return {
        "clip_id": cb.index,
        "codec": "FnDeltaF1",
        "sample_count": sample_count,
        "channel_count": channel_count,
        "shift": shift,
        "n_blocks": n_blocks,
        "frames": frames,
        "stats": stats,
        "bytes_consumed": off - stream_start + channel_count * record_stride + 0x14,
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
        if cb and cb.secondary and cb.secondary.tag == 0x15:
            clip_blocks.append(cb)
    return data, clip_blocks


def main():
    data, clip_blocks = load_clips()
    print(f"{len(clip_blocks)} FnDeltaF1-secondary clips found\n")
    print(f"{'clip':>4} {'samples':>7} {'chans':>5} {'blocks':>6} "
          f"{'ref_mean':>9} {'dmag_mean':>10} {'step_mean':>10} {'step_max':>9}")
    for cb in clip_blocks:
        r = decode_clip_fndeltaf1(data, cb)
        s = r["stats"]
        print(f"{r['clip_id']:>4} {r['sample_count']:>7} {r['channel_count']:>5} {r['n_blocks']:>6} "
              f"{s['ref_val']['mean']:>9.4f} {s['delta_mag']['mean']:>10.6f} "
              f"{s['step_len']['mean']:>10.6f} {s['step_len']['max']:>9.6f}")


if __name__ == "__main__":
    main()
