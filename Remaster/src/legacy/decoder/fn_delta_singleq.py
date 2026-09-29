"""
fndeltasingleq_validate.py
----------------------------
Validation decoder for FnDeltaSingleQ (tag 0x13), ELF-confirmed across
sessions 26-29 via disassembly of EvalSQT__FnDeltaSingleQ (0x80408994,
4516 bytes) and its two helpers (UnQuantize 0x80408878, Euler->quat
builder 0x8040873c).

CONFIRMED THIS SESSION (closing session 28's open item #1):
  - 0x8040873c (Euler->quat, sin/cos, 0.5 half-angle) is called EXACTLY
    4 times, all in one-time init, building two cached bind-pose
    quaternions (this+0x28, this+0x2c). NEVER called in the per-frame
    path.
  - Per-frame output is a direct Hamilton quaternion product between
    the live delta accumulator (this+0x24) and one/both cached buffers,
    selected by the same per-channel flag byte used for axis selection:
        flag==0      -> Hamilton(accum, buf_2c)
        flag==1      -> Hamilton3(accum, buf_28, buf_2c)  [NOT fully
                         reduced this session -- approximated below as
                         Hamilton(accum, buf_28) then Hamilton(that, buf_2c),
                         flagged explicitly as unconfirmed term order]
        flag>=2/else -> Hamilton(buf_28, accum)
  - accum's slot 3 is literally the w component, used directly in the
    Hamilton product -- NOT a separate angle value as session 28 guessed.
  - Output written directly to a bone-major pose buffer at
    bone_idx*0x30+0x10, bone_idx read as a plain per-channel u8 (same
    convention as FnDeltaQFast, session 25 Sec 4.1).

CARRIED FROM SESSIONS 26-28 (re-used, not re-derived this session):
  - header: marker+0x04 SAMPLE_COUNT (u16), marker+0x06 CHANNEL_COUNT (u8),
    marker+0x07 SHIFT (u8) -- FnDeltaQFast-style layout, confirmed via
    20/20 (well, 9/9) sample-count cross-match against the primary track
    in the earlier extraction session.
  - MinRange record, 14 bytes/channel, at marker.payload_off:
      +0x00 u16  angle0  (init-only, bind-pose construction)
      +0x02 u16  angle1  (init-only, bind-pose construction)
      +0x04 u16  -> delta_base1  = trick/32768 - 1
      +0x06 u16  -> delta_base2  = trick/32768 - 1
      +0x08 u16  -> delta_scale1 = trick/32768
      +0x0A u16  -> delta_scale2 = trick/32768
      +0x0C u8   flag (axis selector: 0/1/else)
      +0x0D      padding to 14
  - reference region: 2 bytes/channel (channel-major):
      byte0 -> seed = trick*(2/255)-1, written into accum.slot[flag]
      byte1 -> ALWAYS accum.slot[3] = trick*(2/255)-1
  - delta region: 1 byte/channel/sample (sample-major/channel-minor),
    nibble-packed:
      hi nibble -> accum.slot[flag] += trick(hi)*(delta_scale1/15)+delta_base1
      lo nibble -> accum.slot[3]    += trick(lo)*(delta_scale2/15)+delta_base2
  - block stride: NOT independently re-derived this session. Assumed by
    analogy to every other codec in this project:
      block_size = round_down_even(channel_count*2
                                    + (2^SHIFT-1)*channel_count + 1)
    This assumption is the single biggest unvalidated piece below --
    flagged explicitly, see corpus run's byte-boundary check.

NOT resolved: flag==1's exact 3-buffer term order (approximated, not
ELF-exact); block stride formula (assumed, not re-disassembled this
session).
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
INV_255 = 1.0 / 255.0
INV_32768 = 1.0 / 32768.0
INV_65535 = 1.0 / 65535.0
TWO_PI = 2.0 * math.pi


def int_to_float_trick(raw: int) -> float:
    # See fn_delta_qfast.py's int_to_float_trick for the full rationale:
    # this hardware-era PowerPC bit trick is mathematically float(raw)
    # for every input this codec passes it (verified by exhaustive sweep).
    return float(raw)


def read_minrange_record(data: bytes, off: int):
    u = struct.unpack_from(">6H", data, off)
    flag = data[off + 0x0C]
    angle0 = int_to_float_trick(u[0]) * (TWO_PI * INV_65535) - math.pi
    angle1 = int_to_float_trick(u[1]) * (TWO_PI * INV_65535) - math.pi
    delta_base1 = int_to_float_trick(u[2]) * INV_32768 - 1.0
    delta_base2 = int_to_float_trick(u[3]) * INV_32768 - 1.0
    delta_scale1 = int_to_float_trick(u[4]) * INV_32768
    delta_scale2 = int_to_float_trick(u[5]) * INV_32768
    return dict(angle0=angle0, angle1=angle1, delta_base1=delta_base1,
                delta_base2=delta_base2, delta_scale1=delta_scale1,
                delta_scale2=delta_scale2, flag=flag)


def euler_to_quat(a0, a1, a2):
    """0x8040873c, standard half-angle Euler composition. Unit-norm by
    construction regardless of axis/sign-convention ambiguity (session
    27's open question) -- norm is NOT a discriminating test for which
    convention is right, only that the formula shape is a valid product
    of three half-angle rotations."""
    h0, h1, h2 = a0 * 0.5, a1 * 0.5, a2 * 0.5
    sx, sy, sz = math.sin(h0), math.sin(h1), math.sin(h2)
    cx, cy, cz = math.cos(h0), math.cos(h1), math.cos(h2)
    qx = cx * cy * sz - sx * sy * cz
    qy = sx * cy * cz + cx * sy * sz
    qz = cx * sy * cz - sx * cy * sz
    qw = cx * cy * cz + sx * sy * sz
    return (qx, qy, qz, qw)


def hamilton(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


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


def decode_clip_fndeltasingleq(data: bytes, cb, compute_stats: bool = True):
    """compute_stats=False skips the quat_norm/quat_angle_deg bookkeeping
    (frames is unaffected) -- the exporter never reads `stats`, only
    this module's own `main()` CLI does, so the export pipeline passes
    compute_stats=False."""
    marker = cb.primary
    m = marker.abs_off
    sample_count = struct.unpack_from(">H", data, m + 0x04)[0]
    channel_count = data[m + 0x06]
    shift = data[m + 0x07]
    # CORRECTED this session: GetArrays__DeltaSingleQ (0x80408978) computes
    # the MinRange table pointer as map+0x10 directly (addi r6,r3,0x10),
    # NOT marker.payload_off (which is +0x12 for this codec's marker
    # layout). This was the actual bug -- previous "flag reads 127"
    # symptom was this off-by-2 landing one field early into every record.
    payload = marker.abs_off + 0x10
    record_stride = 0x0E  # 14 bytes, confirmed: mulli r0,r0,0xe in GetArrays

    records = [read_minrange_record(data, payload + ch * record_stride)
               for ch in range(channel_count)]

    # init-time cached bind quaternions (this+0x28 / this+0x2c)
    buf28, buf2c = [], []
    for rec in records:
        vec = [0.0, 0.0, 0.0]
        flag = rec["flag"] if rec["flag"] in (0, 1) else 2
        vec[flag] = rec["angle0"]
        q_a = euler_to_quat(*vec)
        vec2 = [0.0, 0.0, 0.0]
        vec2[flag] = rec["angle1"]
        q_b = euler_to_quat(*vec2)
        # session-25/28: which buffer gets which flag's write was not
        # independently re-confirmed this session -- assumed symmetric
        # with the OUTPUT branch structure (flag0->2c, flag>=2->28,
        # flag1->both).
        buf28.append(q_a)
        buf2c.append(q_b)

    stream_start = payload + channel_count * record_stride
    block_len = 1 << shift
    n_deltas = block_len - 1
    ref_region_len = channel_count * 2
    delta_region_len = n_deltas * channel_count * 1
    raw_block_size = ref_region_len + delta_region_len + 1
    block_size = raw_block_size - (raw_block_size % 2)
    n_blocks = math.ceil(sample_count / block_len)

    frames_by_channel = [[] for _ in range(channel_count)]
    norms = []
    step_angles = []

    off = stream_start
    for b in range(n_blocks):
        if off + block_size > len(data):
            break
        deltas_this = min(n_deltas, sample_count - 1 - b * block_len) \
            if b == n_blocks - 1 else n_deltas
        deltas_this = max(0, deltas_this)

        accum = [[0.0, 0.0, 0.0, 0.0] for _ in range(channel_count)]
        for ch in range(channel_count):
            rec = records[ch]
            flag = rec["flag"] if rec["flag"] in (0, 1) else 2
            b0 = data[off + ch * 2]
            b1 = data[off + ch * 2 + 1]
            accum[ch][flag] = int_to_float_trick(b0) * (2 * INV_255) - 1.0
            accum[ch][3] = int_to_float_trick(b1) * (2 * INV_255) - 1.0

        def emit_frame(ch):
            rec = records[ch]
            flag = rec["flag"] if rec["flag"] in (0, 1) else 2
            a = tuple(accum[ch])
            if flag == 0:
                q = hamilton(a, buf2c[ch])
            elif flag == 1:
                q = hamilton(hamilton(a, buf28[ch]), buf2c[ch])
            else:
                q = hamilton(buf28[ch], a)
            return q

        for ch in range(channel_count):
            q = emit_frame(ch)
            if compute_stats:
                norms.append(quat_norm(q))
            frames_by_channel[ch].append(q)

        for s in range(deltas_this):
            for ch in range(channel_count):
                rec = records[ch]
                flag = rec["flag"] if rec["flag"] in (0, 1) else 2
                sample_off = off + ref_region_len + s * channel_count + ch
                if sample_off >= len(data):
                    break
                raw = data[sample_off]
                hi, lo = (raw >> 4) & 0xF, raw & 0xF
                hi_val = int_to_float_trick(hi) * (rec["delta_scale1"] / 15.0) + rec["delta_base1"]
                lo_val = int_to_float_trick(lo) * (rec["delta_scale2"] / 15.0) + rec["delta_base2"]
                accum[ch][flag] += hi_val
                accum[ch][3] += lo_val

                q_prev = frames_by_channel[ch][-1]
                q = emit_frame(ch)
                if compute_stats:
                    step_angles.append(quat_angle_deg(q_prev, q))
                frames_by_channel[ch].append(q)

        off += block_size

    frames = []
    for s in range(sample_count):
        frame = []
        for ch in range(channel_count):
            frame.append(frames_by_channel[ch][s] if s < len(frames_by_channel[ch]) else None)
        frames.append(frame)

    def stat(lst):
        return {"mean": sum(lst) / len(lst) if lst else None,
                "min": min(lst) if lst else None,
                "max": max(lst) if lst else None}

    stats = {
        "norm": stat(norms),
        "step_angle": stat(step_angles),
        "n_samples": len(norms),
    }

    return {
        "clip_id": cb.index,
        "codec": "FnDeltaSingleQ",
        "sample_count": sample_count,
        "channel_count": channel_count,
        "shift": shift,
        "n_blocks": n_blocks,
        "frames": frames,
        "stats": stats,
        "bytes_consumed": off - stream_start + payload + channel_count * record_stride - m,
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
        if cb and cb.primary and cb.primary.tag == 0x13:
            clip_blocks.append(cb)
    return data, clip_blocks


def main():
    data, clip_blocks = load_clips()
    print(f"{len(clip_blocks)} FnDeltaSingleQ-primary clips found\n")
    print(f"{'clip':>4} {'samples':>7} {'chans':>5} {'blocks':>6} "
          f"{'norm_mean':>9} {'norm_min':>8} {'norm_max':>8} "
          f"{'step_mean':>9} {'step_max':>8}")
    for cb in clip_blocks:
        try:
            r = decode_clip_fndeltasingleq(data, cb)
        except Exception as e:
            print(f"{cb.index:>4}  ERROR: {e}")
            continue
        s = r["stats"]
        sm = s["step_angle"]["mean"] if s["step_angle"]["mean"] is not None else float('nan')
        smx = s["step_angle"]["max"] if s["step_angle"]["max"] is not None else float('nan')
        print(f"{r['clip_id']:>4} {r['sample_count']:>7} {r['channel_count']:>5} {r['n_blocks']:>6} "
              f"{s['norm']['mean']:>9.4f} {s['norm']['min']:>8.4f} {s['norm']['max']:>8.4f} "
              f"{sm:>9.2f} {smx:>8.2f}")


if __name__ == "__main__":
    main()
