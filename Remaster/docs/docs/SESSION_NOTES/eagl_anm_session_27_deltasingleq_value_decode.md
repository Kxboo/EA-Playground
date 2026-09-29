# EAGL `.anm` decoder — session 27: `FnDeltaSingleQ::EvalSQT` value decode

**Scope.** Continued directly from session 26. Traced
`EvalSQT__Q28EAGLAnim14FnDeltaSingleQ` (`0x80408994`, 4516 bytes) from the
top through the first full reference-sample + first delta-sample + final
quaternion write, plus the two helper functions it calls
(`0x8040893c` = allocator, `0x8040873c` = angle→quaternion builder). This
closes the single highest-value open item from session 26 (§2.4 item 1) and
produces a genuinely new, load-bearing finding about what "SingleQ" means.

**Headline finding:** `FnDeltaSingleQ` is a **single-axis-rotation** codec —
each animated channel stores a **quantized rotation angle around one
specific axis** (chosen per-channel from a 3-way selector in the basis
record), converted to a full quaternion via a standard **Euler-angle→
quaternion** construction (sin/cos half-angle composition), not a
"smallest-three" component-dropping scheme as session 26 speculated. This
matches the class name far better than the earlier guess did.

Evidence tier labels as in prior sessions: **Confirmed** = disassembly
traced end-to-end and internally consistent; **Validated, semantic meaning
open** = mechanism seen, exact role/units unconfirmed; **Open** = not yet
traced.

---

## 1. Confirmed: one-time bind-pose / reference initialization (`this[0x24]==0` path)

On a fresh instance (`0x80408a24`–`0x80408bd4`), before any per-frame
decode:

- Calls `GetArrays` (`0x80408978`) and `GetBinSize` (`0x80408954`) — same
  functions session 26 already closed (§2.2).
- Allocates **three** `channel_count × 16`-byte buffers via the small
  allocator-trampoline at `0x8040893c` (`this+0x20`, `this+0x24` — initially
  aliased to the *same* buffer — and `this+0x28`/`this+0x2c` separately).
  16 bytes/channel = 4 floats/channel, i.e. **one quaternion per channel**,
  confirming these are per-channel pose-accumulator buffers, not raw
  bitstream buffers.
- For each channel (loop bound `marker[0x06]`, same channel-count field
  confirmed in session 26): calls `UnQuantize` (`0x80408878`, closed in
  session 26 §2.2) against that channel's 14-byte `MinRange` record, then
  branches on the record's `+0x0C` field (the byte session 26 flagged as
  "shift/flag byte, not a float" — now confirmed to be a **3-way axis
  selector**, values `0`, `1`, or *else* (treated as `2`)):
  - Builds a 3-float vector `(a0, a1, a2)` where **exactly one slot** (the
    one matching the selector) is filled with a value derived from
    `UnQuantize`'s two decoded floats (out `+0x00`/`+0x04`, the "pivot/base"
    pair), and the **other two slots are filled with a constant** (`f26`,
    loaded from the float constant pool at `-0x2828(r2)` — a fixed value,
    not per-channel; likely `0.0`, not yet pinned down numerically).
  - Passes that 3-float vector into `0x8040873c` (see §2) to produce a
    4-float quaternion, written into the `this+0x28`/`this+0x2c` buffers at
    that channel's slot.

This is the **bind-pose reconstruction for this codec's channels**, built
once from the same `MinRange`/`UnQuantize` structure session 26 already
validated corpus-wide — not a new per-frame cost.

## 2. Confirmed: `0x8040873c` is a Euler-angle → quaternion constructor

Full trace of `0x8040873c` (`~200` bytes):

```
in: (a0, a1, a2)  -- three angle-like floats (radians, pending unit check)
scaled = in[i] * K              ; K = float constant, -0x27f8(r2), same for all 3
for i in 0..2: sin_i = sin(scaled[i])   (bl 0x8002eac4)
for i in 0..2: cos_i = cos(scaled[i])   (bl 0x8002eed0)

qx = cx*cy*sz - sx*sy*cz
qy = sx*cy*cz + cx*sy*sz
qz = cx*sy*cz - sx*cy*sz      (approximate term pairing per fmsubs/fmadds
qw = cx*cy*cz + sx*sy*sz         sequence at 0x80408800-0x80408828; exact
                                  sign/term assignment to x/y/z/w slots
                                  should be double-checked numerically
                                  against a known bone before trusting in
                                  a decoder, see open items)
out: (qx, qy, qz, qw), 16 bytes, written to caller-supplied pointer
```

This is a standard **Euler XYZ (or ZYX — order not yet disambiguated)
half-angle composition** — the same shape as any textbook
`euler_to_quaternion`, confirmed via the `sin`/`cos` library calls
(`0x8002eac4`/`0x8002eed0`) and the `fmadds`/`fmsubs` product-combination
pattern immediately after. `bl 0x8002eac4` / `bl 0x8002eed0` were not
independently confirmed as `sinf`/`cosf` by name (no symbol at those
addresses in this ELF's `.symtab` — likely PPC runtime library functions
without exported names), but the usage pattern (paired calls on the same
scaled input, then combined via the classic trig-product quaternion formula)
makes the sin/cos identification essentially certain.

**Since only one of the three input angles is ever non-default** (§1), this
reduces at runtime to a **single-axis rotation**: `Euler(0, 0, theta)` or
similar, which is exactly what "SingleQ" — single-axis quaternion — should
mean. This retroactively resolves session 26's open question about why
`FnDeltaSingleQ`'s basis record only decodes 2 of its 3 useful float slots
into an angle-pair per channel (session 26 §2.2 table): the two decoded
floats aren't "two of three quaternion axes" as guessed, they're **the
angle value for two different frames/keys of the *same* single axis**
(reference sample + one more), not two different rotation axes.

## 3. Confirmed: per-block structure — reference sample, then nibble-packed deltas

After the one-time init, `EvalSQT` computes a **block index** from the
requested sample time (`fctiwz f0, f25` at `0x80408bd4`), using an *optional*
keyframe-time table at `marker+0x08` (same field FnStatelessQ uses for its
own keyframe search, session 26 §1.2) if present, else falling back to a
straight `frame >> bin_size` computation via `sraw`
(`0x80408cd0`/`0x80408ce8`) against `marker+7`'s bin_size shift — i.e. the
same `block_len = 1 << shift` grammar as every delta-block codec in this
project, just gated behind an extra optional sparse-keyframe layer this
codec apparently supports and the others don't (or at least, don't use this
way).

**Reference sample per block** (`0x80408d08`–`0x80408de0`): for each
channel, reads **one raw byte** from the block's data pointer
(`stream_base(this+0x14) + block_index*block_size(this+0x18)`, same
formula shape as `FnDeltaQFast`/`F3`/`F1`), converts via the shared
`int_to_float_trick`, and applies a **fixed, non-per-channel** scale/bias
(`f2`/`f1` loaded from the constant pool once, not derived from
`UnQuantize`) — `value = raw_trick * f2 - f1`-ish (`fmsubs`). The record's
`+0x0C` axis-selector byte (same field as §1) again picks which of 3 float
slots this decoded value lands in, mirroring the bind-pose construction
exactly, then the same reference bytes feed a **second**, always-computed
slot (offset `+0xC` of a *separate* 16-byte buffer entry) via the identical
formula — this "point A" / "point B" (`this+0x28` vs `this+0x2c`) dual
buffer looks like it may be a from/to (previous-frame, current-frame) pair
for the block-boundary lerp, structurally parallel to `FnStatelessQ`'s
explicit two-key lerp (session 26 §1.3) rather than a delta accumulator —
flagged **Validated, semantic meaning open**, not fully confirmed which
buffer plays which role.

**Delta samples within the block** (from `0x80408e40` onward): unlike every
other codec in this project (byte-per-channel-per-sample), the delta reads
here are **nibble-packed**: `lbz r11,0(r9)` reads one raw byte, then
`rlwinm r11,r11,0x1c,0x1c,0x1f` extracts the **top 4 bits** (equivalent to
`byte >> 4`) as the actual delta value for one sample. A companion low-
nibble extraction very likely exists for the *next* delta sample sharing the
same byte (2 deltas packed per byte) — the low-nibble counterpart wasn't
independently confirmed this session but is the obvious mate of the
high-nibble path already traced, given the codec exists specifically to
compress single-axis rotation deltas more tightly than the 3-axis codecs.
The extracted nibble goes through the same `int_to_float_trick` +
`pivot/scale` (`fmadds`) formula family as the reference sample, then is
**added onto the previous accumulated value** read via `lfsx` from the
`this+0x24` buffer (forward running-sum accumulation, same convention as
`FnDeltaQFast`/`F3`/`F1` confirmed in sessions 20–25) — the resulting scalar
angle then presumably feeds back through the same `0x8040873c`
Euler-to-quaternion path per sample, though the actual re-call to
`0x8040873c` (or an inlined equivalent) per delta sample was **not directly
observed** in the portion traced this session — flagged **Open**, see §4.

---

## 4. Open items

1. **Low-nibble delta path** — confirm the second (low 4 bits) delta
   extraction exists and its byte-pairing/stride math (how many bytes for N
   deltas × channel_count, given 2 deltas/byte).
2. **Per-delta-sample quaternion reconstruction** — confirm whether
   `0x8040873c` (or an inlined variant) is called once per decoded delta
   angle, or whether the accumulation stays in angle-space for the whole
   block and only converts to quaternion once at read-time. This matters a
   lot for a from-scratch Python decoder's structure.
3. **Exact Euler axis/order and the `K` scale constant** (`-0x27f8(r2)`) —
   is `K` a degrees→radians conversion, a fixed-point scale, or something
   else? Needs either a constant-pool dump (`.rodata` value at that offset)
   or a numeric cross-check against a known bone's expected rotation range.
4. **`f26`/default-axis constant value** (`-0x2828(r2)`) — almost certainly
   `0.0` (angle contributes nothing on the two non-selected axes) but not
   independently read out of `.rodata` this session.
5. **`this+0x28` vs `this+0x2c` buffer roles** (§3) — which is "current
   reference" vs "previous/interpolation target"; affects whether this
   codec does block-boundary interpolation like `FnStatelessQ` or pure
   forward accumulation like the other delta codecs.
6. Once 1–5 close, corpus-validate the same way `fndeltaf1_validate.py`/
   `fndeltaf3_validate.py` did, against all 9 tag-`0x13` clips, before
   wiring into `anm_exporter.py`.

## 5. Addendum — constant pool dump (this closes items 3 and 4 above)

Pulled the actual `.sdata2` float values directly from the ELF rather than
guessing (`r2` = `_SDA2_BASE_` = `0x8060a540`, confirmed via `.symtab`;
`.sdata2` maps `vaddr 0x80602540 → file offset 0x4f1080`). Every constant
`EvalSQT__FnDeltaSingleQ` and its helper reference resolves to a clean,
recognizable value — strong independent confirmation the formula reading in
§1-3 is right, not a misattribution:

| `r2` offset | value | role (now confirmed) |
|---|---|---|
| `-0x27f8` | **`0.5`** | half-angle scale into `0x8040873c`'s `sin`/`cos` calls — **confirms** the Euler→quaternion identification in §2: angle is halved before `sin`/`cos`, exactly the standard formula |
| `-0x2828` | **`0.0`** | default fill for the two non-selected axis slots (§1) — confirmed, as guessed |
| `-0x2824` | `1.0` | bias term paired with the byte-normalize scale below |
| `-0x2820` | **`0.00784314` (`2/255`)** | reference-sample byte decode scale (§3): `raw_byte_trick * (2/255) − 1.0` → normalizes a byte to **`[-1, +1]`**, not directly a radian value |
| `-0x281c` | `3.0518e-05` (`≈ 1/32768`) | secondary per-channel basis scale (delta region setup) |
| `-0x2818` | **`−π`** | angle bias — confirms §1's two `UnQuantize`-decoded "pivot" floats are **angles in radians**, not generic pivots |
| `-0x2814` | **`9.5875e-05` (`= 2π/65535` exactly)** | angle scale, pairs with `−π` above: `angle = u16_trick*(2π/65535) − π`, i.e. a **u16 maps linearly onto the full `[-π, +π]` range** |
| `-0x2810` | **`0.0667` (`= 1/15`)** | nibble delta normalization — confirms the 4-bit (0-15) delta packing identified in §3 |
| `-0x2900`/`-0x28f8`/`-0x2808` | (upper word of `2^52`) | the shared `int_to_float_trick` magic-double constant, same one used by every other codec in this project — not a new value |

### Revised model of the codec (angle-space accumulation, not quaternion-space)

Putting the constants into the formulas actually traced in §1-3 changes the
picture slightly from §3's tentative framing, and answers open item 2 with
reasonable confidence:

- The two floats `UnQuantize` decodes per channel (session 26's "pivot"
  pair) are **angles in radians**, via `u16_trick*(2π/65535) − π` — full-range
  16-bit angle quantization, one value per reference key.
- The **delta accumulator buffer (`this+0x24`) very likely holds a running
  angle in radians, not a quaternion component** — the per-sample update
  reads the previous value with `lfsx` and (per the instructions visible
  before the trace window closed) adds a nibble-decoded delta scaled by
  `(1/15) * (per-channel secondary scale)`, i.e. **plain forward angle
  accumulation**, matching every other delta codec's accumulation
  convention (sessions 20-25) but in angle-space instead of
  quaternion/translation-component-space.
- `0x8040873c` (the Euler→quaternion builder, confirmed via the `0.5`
  half-angle constant) is therefore almost certainly called **once per
  requested output frame**, converting the single accumulated angle (placed
  in its one selected axis slot, others `0.0`) into the final quaternion —
  not once per delta sample. This is the natural reading given accumulating
  in angle-space is cheaper and the conversion function's cost (2× `sin`,
  2× `cos`, several multiplies) is exactly the kind of thing you'd want to
  do only at query time, not on every decode step. Still flagged
  **Validated, semantic meaning open** rather than fully confirmed, since
  the actual call site wasn't directly observed in the disassembly window
  captured this session — but it's now a well-supported inference rather
  than a guess, and is the leading hypothesis to verify first next session.
- The **reference-sample byte decode's `[-1,+1]` normalization** (not
  radians) is the one piece that doesn't yet fit cleanly into "everything is
  an angle" — plausible readings: (a) it's a `cos(angle)`/`sin(angle)`
  value stored directly for the block's reference key rather than the raw
  angle (would explain the different range/formula from the delta path),
  or (b) it feeds a different, still-unidentified derived quantity. This is
  now the single largest remaining open question and the right starting
  point for the next session, rather than the vaguer "trace more
  instructions" framing this document had before the constant dump.

Items 3 and 4 from §4 are now closed. Item 1 (low-nibble path), item 5
(buffer roles), and the refined open question above (reference-byte
semantics) remain for next session.

