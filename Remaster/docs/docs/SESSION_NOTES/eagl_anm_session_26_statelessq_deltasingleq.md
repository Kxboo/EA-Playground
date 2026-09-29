# EAGL `.anm` decoder — session 26: `FnStatelessQ` and `FnDeltaSingleQ`

**Scope.** Disassembly of the two remaining undecoded codecs (`whole_clip`
container `FnStatelessQ`, and `FnDeltaSingleQ`) directly in `playgroundz.elf`,
using the same capstone/PPC BE methodology as session 25 (`disas.py`,
`.text` at `vaddr=0x80013d00`/`off=0x0fee0`).

**Result up front, honestly scoped:** the channel→bone table mechanism for
*both* codecs is now **fully confirmed** and is byte-identical in shape to
`FnDeltaQFast`'s (session 25 §4.1). The per-key/per-bone *value* decode for
both codecs was only **partially** traced this session — enough to nail the
on-disk record layout and the arithmetic family (the same pivot/scale u16→f32
bit-trick used everywhere else in this project), but **not enough to hand off
a drop-in decoder with the same confidence as `FnDeltaF1`/`F3`/`QFast`**.
`EvalSQT__FnStatelessQ` is 1788 bytes and `EvalSQT__FnDeltaSingleQ` is 4516
bytes — both far larger than the codecs closed out in prior sessions — and
closing them to the same standard needs at least one more focused session
each. Recording what's solid now rather than guessing past that line.

---

## 0. Correction to tag numbers, confirmed against the real corpus

The docstrings in `anm_exporter.py`/prior sessions used tag `0x16` for the
`whole_clip` container and described `FnDeltaSingleQ` as an "anomalous
secondary". Running a corpus-wide tag census this session
(`deltasingleq_basis_probe.py`'s clip loader against all 265 clips) shows
this needs correcting:

```
(whole_clip=False, primary=0x12, secondary=0x14)  224 clips  -- FnDeltaQFast + FnDeltaF3
(whole_clip=True,  tag=0x17)                        12 clips  -- FnStatelessQ (NOT 0x16)
(whole_clip=False, primary=0x13, secondary=0x12)     9 clips  -- FnDeltaSingleQ (PRIMARY, not secondary) + FnDeltaQFast
(whole_clip=False, primary=0x12, secondary=0x15)    20 clips  -- FnDeltaQFast + FnDeltaF1
```

So: `FnStatelessQ`'s container tag is **`0x17`**, not `0x16`, and
`FnDeltaSingleQ` is the **primary track** (tag `0x13`) in its 9 clips, paired
with an ordinary `FnDeltaQFast` (tag `0x12`) secondary -- not an "anomalous
secondary" as previously logged. `0x13` sharing `FnDeltaQFast`'s
`marker+0x06` channel-count offset (confirmed independently via
`GetAnimatedBonesAux` in both codecs, see §2.1) is exactly why it was
previously lumped in with tag `0x12` in `get_channel_count()`
(`validate_bone_mapping.py`) without anyone noticing the tag value itself
differed -- worth fixing in that function too, since right now it silently
treats `0x13` correctly by accident (same offset) rather than by explicit
handling.

All references to these codecs below use the corrected tag values.

## 1. `FnStatelessQ` (the `whole_clip` container, tag 0x17)

### 1.1 Confirmed: channel→bone table — identical mechanism to `FnDeltaQFast`

`GetAnimatedBonesAux__Q28EAGLAnim12FnStatelessQ` (`0x803fcc7c`):

```
r30 = this->marker                  ; marker = *(this+0xC)
channel_count = marker[0x16]         ; u8   <-- NEW offset, StatelessQ-specific
table_ptr     = marker[0x0C]          ; self-relocated ptr, SAME offset as FnDeltaQFast
loop i = channel_count-1 downto 0:
    bone_idx = table_ptr[i]              ; u8, direct (lbzx), no arithmetic
    SetBone(mask, bone_idx, true)          ; same BoneMask::SetBone (0x803fc9d4)
```

Byte-for-byte the same shape as `FnDeltaQFast::GetAnimatedBonesAux`
(session 25 §4.1): same `srawi`/`clrlwi`/mask-word/mask-bit sequence, same
`SetBone` call target. Only the channel-count field offset differs
(`marker+0x16` here vs `marker+0x06` for `FnDeltaQFast`) — consistent with
`FnStatelessQ`'s marker header having a different, larger layout (it also
uses `marker+0x14` as a u16 keyframe count and `marker+0x08`/`+0x0C` as two
separate pointers, per §1.2).

### 1.2 Confirmed: keyframe (not delta-block) structure

Unlike every codec closed out so far, `FnStatelessQ` is **not** block/delta
based. `EvalSQT__FnStatelessQ` (`0x803fd220`) opens with a **binary search
over an explicit keyframe-time table**:

- `marker+0x08` → pointer to a table of `u16` **key times**, length
  `marker+0x14` (u16, same field `GetLength` at `0x803fd934` also reads —
  see below).
- `marker+0x14` u16 = **keyframe count** (this is the "sample_count"
  equivalent for this codec; **not** a per-block sample count, an absolute
  keyframe count).
- The search (`0x803fd310`–`0x803fd3a0`) is a textbook binary search:
  `lhz` reads from the u16 time table, compares against a target time
  derived from `f30` (computed at the top of the function from `this+0x12`/
  `0x13`, a byte-pair that looks like an fps-style scale — mirrors the
  `UseFPS` accessor at `0x803fceb8`), and narrows `[lo, hi]` until it lands
  on the bracketing keyframe pair. Output: `r30` = lower keyframe index,
  `f31` = interpolation fraction `t` between the two bracketing keyframes
  (computed via `fdivs`, both branches at `0x803fd3e0`/`0x803fd438` handle
  the "past last key" and "normal" cases separately).
- **`GetLength` (`0x803fd934`) confirms `marker+0x14` semantics independently**:
  if `marker+8` (the time-table pointer) is null, length falls back to
  `marker+0x14` directly (a plain duration in this fallback case); otherwise
  length is read from the *last entry* of the time table
  (`table[(count-2)*2] + 1`, i.e. `table[count-1] + 1` computed via a `-2`
  index trick — consistent with time tables being 0-indexed and
  `count-1` the last valid index).

### 1.3 Confirmed: per-key data record shape and lerp, semantics of scale open

Once the bracketing keyframe pair (`r30`, `r30+1`) is found, the function
(from `~0x803fd470` onward) walks:

- `marker+0x0C` → base pointer to the **per-key SQT data block**
  (`+0x18` past a small embedded header — `addi r0, r31, 0x18` at
  `0x803fd47c`).
- `marker[0x16]` (**the same byte used as channel_count above**) is also
  used here as a **per-key stride multiplier**: `stride = channel_count * 4
  * 2` bytes between successive keyframes' data blocks (`mullw` against
  `slwi r30,2` then `slwi ,1` at `0x803fd474`–`0x803fd48c`) — i.e. each key
  stores `channel_count` entries of 4 × u16 (8 bytes) = the same "quat as
  4×u16" shape used by `FnDeltaQFast`'s reference region, just without
  delta compression.
- **Per-channel decode loop** (`0x803fd4c8`–`0x803fd5b0`, and the "count not
  multiple of 2" tail loop at `0x803fd5c0`–`0x803fd6a8`): reads 4×u16 from
  the **lower** key's block and 4×u16 from the **upper** key's block for the
  same channel slot, converts each u16 via the same bit-manipulation idiom
  used everywhere else in this project (`rlwinm`/`rlwimi` building a raw
  IEEE float bit pattern from the u16 — the integer-domain twin of the
  `int_to_float_trick`/magic-double subtraction seen in the other codecs'
  Python ports), then does a **per-component linear interpolation**:
  ```
  out[c] = lower[c] + f31 * (upper[c] - lower[c])     ; fsubs + fmadds, c = x,y,z (,w?)
  ```
  confirmed for 3 components at `0x803fd550`–`0x803fd5ac` (x/y/z visible;
  the loop is `bdnz`-driven over `channel_count`, so a 4th (w) component is
  very likely present but wasn't individually confirmed against a register
  this session — **OPEN**, see §1.4).
- **Bone placement**: the per-channel bone index byte comes from a **second**
  small array walked in lock-step (`r6`, incremented `+1`/`+2` per key
  processed — `lbz r7,0(r6)` / `lbz r7,1(r6)` at `0x803fd4c8`/`0x803fd630`),
  multiplied by `0xc` (12 = 3 floats) to index the **output** SQT buffer —
  i.e. this is a **second**, decode-loop-local bone table, distinct from the
  `GetAnimatedBonesAux` table in §1.1. Whether it's the *same* underlying
  `marker+0x0C` table reused, or a third pointer field, was not
  disambiguated this session — **OPEN**.

### 1.4 Open items — `FnStatelessQ`

1. Confirm whether the interpolation writes 3 or 4 components per channel
   (translation-only vs quaternion) — the output stride `*0xc` (12 bytes = 3
   floats) at `0x803fd544`/`0x803fd584` suggests **3 components (a
   translation/position), not a quaternion**, despite the "Q" in the class
   name — this is a real, load-bearing open question, not a formality: it
   changes whether `FnStatelessQ` outputs rotation or translation data. Given
   the class is literally the payload of the `whole_clip`/`FnStatelessQ.cpp`
   container and this project's corpus shows 12 `whole_clip` clips
   overlapping with the "root-motion-like" DOF-122 clips flagged in the
   memory context, a translation-only reading would actually be consistent
   with root motion — worth checking directly against those 12 clip IDs
   next session before assuming either way.
2. Pin down the exact u16→float bit-trick constants used in this codec's
   `rlwinm`/`rlwimi` pair (`0xf`,`2`,`0x10` / `0x10`,`0`,`0`) — same shape as
   the `int_to_float_trick` used elsewhere, but not yet algebraically
   reduced to a closed-form Python equivalent the way the other codecs'
   magic-double trick was.
3. Resolve the second bone-index array (§1.3) — same table as
   `GetAnimatedBonesAux`'s, or separate.
4. The `f30`/time-scale computed from `this+0x12`/`this+0x13` at the top of
   `EvalSQT` (bytes, not part of `marker`) needs its own trace — likely an
   FPS-related per-instance scale, not per-clip, but unconfirmed.

---

## 2. `FnDeltaSingleQ`

### 2.1 Confirmed: channel→bone table — identical mechanism, again

`GetAnimatedBonesAux__Q28EAGLAnim14FnDeltaSingleQ` (`0x80407450`) is
**instruction-for-instruction identical** to `FnStatelessQ`'s version in
§1.1, with one offset difference:

```
channel_count = marker[0x06]         ; u8 -- SAME offset as FnDeltaQFast
table_ptr     = marker[0x0C]          ; self-relocated ptr -- same as QFast/StatelessQ
loop: bone_idx = table_ptr[i]; SetBone(mask, bone_idx, true)
```

So all three of `FnDeltaQFast`, `FnStatelessQ`, and `FnDeltaSingleQ` share
the *exact* `marker+0x0C` u8-table / direct-bone-index mechanism; only the
channel-count field offset varies per codec (`0x06` for `QFast` and
`DeltaSingleQ`, `0x16` for `StatelessQ`). `FnDeltaF1`/`FnDeltaF3` remain the
odd ones out with the `marker+0x04`/u16/÷3 axis-split variant (session 25
§3–4).

### 2.2 Confirmed: per-bone quantization-range record (`DeltaSingleQMinRange`, 14 bytes)

Three small helpers fully trace the **basis record layout**, independent of
the (much larger, untraced) `EvalSQT`/`EvalSQTMasked` bulk-decode functions:

**`GetArrays__DeltaSingleQ`** (`0x80408978`, trivial):
```
range_table = this + 0x10                 ; DeltaSingleQMinRange[N], base
bit_array   = range_table + N*0x0E        ; N = this[0x06] (u8 bone/channel count)
```
confirms the record stride is **0x0E (14) bytes** and that a second,
variable-length array (bit-packed delta stream, almost certainly) begins
immediately after the fixed-size range table — same "table then stream"
shape as every other codec in this project.

**`GetBinSize__DeltaSingleQ`** (`0x80408954`):
```
bin_size = this[7]                          ; u8, bit-shift amount
count    = this[6]                          ; u8, same field as channel_count above
result   = round_down_even( count * ((1 << bin_size) + 1) + 1 )
```
— the same `round_down_even(...)` block-size shape used by
`FnDeltaQFast`/`FnDeltaF3`/`FnDeltaF1` (session 21/24), confirming
`FnDeltaSingleQ` is delta-block-coded like those three, *not* keyframe-based
like `FnStatelessQ`.

**`UnQuantize__DeltaSingleQMinRange`** (`0x80408878`) — confirmed field-by-field,
14-byte input record → 0x1C-byte (7-float-ish) output basis struct:

| in-offset | width | out-offset | formula (confirmed via disassembly) | likely role |
|---|---|---|---|---|
| `+0x00` | u16 | out `+0x00` | `f2*(u16→f) + f1` (`fmadds`) | pivot/base, axis 0 |
| `+0x02` | u16 | out `+0x04` | `f2*(u16→f) + f1` (`fmadds`) | pivot/base, axis 1 |
| `+0x04` | u16 | out `+0x08` | `f4*(u16→f) - f3` (`fmsubs`) | secondary base, axis 0 |
| `+0x06` | u16 | out `+0x0C` | `f4*(u16→f) - f3` (`fmsubs`) | secondary base, axis 1 |
| `+0x08` | u16 | out `+0x10` | `f4*(u16→f)` (`fmuls`) | scale, axis 0 |
| `+0x0A` | u16 | out `+0x14` | `f4*(u16→f)` (`fmuls`) | scale, axis 1 |
| `+0x0C` | u16(as byte) | out `+0x18` | stored raw (`stb`) | shift/flag byte, not a float |

`(u16→f)` here is the same magic-double int-to-float trick used throughout
this project (`lis 0x4330; stw; lfd; fsubs` against the `2^52` bias
constant), confirmed present at every row above (`803fd930`-style pattern,
here at `0x80408898`/`0x804088bc`/etc.).

**This table is a near-exact structural cousin of `FnDeltaF1`'s per-channel
record** (session 25's `E`,`B`,`W0`,`W1` shape) but with **2 axes packed per
record instead of 1**, and the `+0x0C` byte slot repurposed rather than
being a `W1`-style scale — consistent with "SingleQ" meaning **one
quaternion component decoded per record pair**, not a full quat, echoing
`FnDeltaF1`'s "one axis per channel" split but for rotation instead of
translation. This is a plausible reading, not yet nailed down against the
4516-byte `EvalSQT` — flagged as **Validated, semantic meaning open**, same
evidence tier as session 25 used for comparable partial results.

### 2.3 Corpus validation of the confirmed offsets (`deltasingleq_basis_probe.py`)

Ran the confirmed-only offsets (bone table + `MinRange` table bounds) against
all 9 real tag-`0x13` clips rather than trusting the disassembly in
isolation. All 9 pass cleanly:

- Bone tables: 9/9 in-range, e.g. clip 6 → bones `[23,24,25,26,27]`, clip 143
  → bones `[8,9,10,11,12,13,23,24,25,27]` (10 channels, largest in the
  bucket). No out-of-range indices anywhere.
- `MinRange` table bounds (`marker+0x10`, `channel_count × 14` bytes): 9/9
  land fully in-file, no overlap/truncation.
- `bin_size` (the `GetBinSize` shift byte, `marker+0x07`) is **`3` on every
  single one of the 9 clips** — either a real corpus-wide constant for this
  codec (plausible: `FnDeltaSingleQ` is used sparingly, likely for a narrow
  set of similar animation types) or a sign the shift byte's role was
  misread; worth keeping in mind next session rather than treating as
  fully settled.
- Raw `MinRange` u16 values for channel 0 across clips consistently show the
  first entry pinned at `32767` (`0x7FFF`, i.e. the u16 midpoint) in **every**
  clip checked — strongly suggestive of a fixed-point signed-range
  convention (`0x7FFF` = zero-offset center) for at least one of the two
  packed axes, consistent with the "MinRange" name implying a per-axis
  min/max quantization window rather than an arbitrary pivot. Not yet
  confirmed against `UnQuantize`'s actual formula assignment (§2.2) which of
  the two axes this is.

This doesn't decode any sample data (that needs the untraced bitstream, item
3 below) but it does mean the record layout and channel/bone plumbing in
§2.1-2.2 can be trusted as a foundation rather than a guess — every
structural offset survived contact with all 9 real clips with zero
exceptions.

### 2.4 Open items — `FnDeltaSingleQ`

1. `EvalSQT__FnDeltaSingleQ` (`0x80408994`, 4516 bytes) and
   `EvalSQTMasked__...` (`0x80407524`, 4632 bytes) were **not traced this
   session** beyond confirming they exist and are the natural next targets —
   at 2-3x the size of any function closed out in sessions 20-25, each
   deserves its own dedicated session rather than a rushed partial read that
   risks exactly the kind of false-confidence this project's methodology is
   built to avoid.
2. Confirm how many `DeltaSingleQMinRange` records exist per channel (does
   "SingleQ" pack all 4 quaternion components across 2 records of 2 axes
   each, i.e. 2 records/channel? or is each record a full independent
   scalar channel like `FnDeltaF1`, with 4 records/quat?) — the `0x0E`
   stride and `this[6]` count field don't by themselves disambiguate this;
   needs a corpus-wide count check against `channel_count` the same way
   `validate_bone_mapping.py` did for the ascending-list hypothesis in
   session 25 §1.
3. The delta-block bitstream itself (referenced by `GetArrays`' `bit_array`
   pointer) — bin size varies per clip (`this[7]`), meaning this is a
   variable-bit-width packed stream, not the fixed byte/nibble grain used by
   the four already-closed codecs. Bit-level unpacking logic is entirely
   unconfirmed and is the largest remaining unknown for this codec.

---

## 3. Summary table (all six codecs now attempted)

| Codec | Tag | Bone-table location | Bone-table width/decode | Block structure | Status |
|---|---|---|---|---|---|
| `FnDeltaQFast` | 0x12/0x13 | `marker+0x0C` | u8, direct | delta-block, round_down_even | **Closed** (sessions 20-25) |
| `FnDeltaF3` | 0x14 | `marker+0x04` | u16, `÷3` | delta-block, round_down_even | **Closed** (session 21, 24) |
| `FnDeltaF1` | 0x15 | `marker+0x04` | u16, `÷3` | delta-block, round_down_even | **Closed** (session 25) |
| `FnStatelessQ` | 0x17 (`whole_clip`) | `marker+0x0C` | u8, direct | **keyframe + binary search + lerp**, not delta-block | **Partially open** — table/search/record-shape confirmed; interp component count and second bone array open |
| `FnDeltaSingleQ` | 0x13 (primary, paired w/ 0x12 secondary) | `marker+0x0C` | u8, direct | delta-block (`round_down_even`), **variable bit-width** | **Partially open** — bone table + basis-record layout confirmed and corpus-validated (9/9 clips); bulk `EvalSQT` bitstream untraced |

The channel→bone mechanism is now confirmed identical (or the same
u16/÷3 family) across **all six** codecs in the corpus — the single most
load-bearing finding of sessions 25-26 combined, since it means
`anm_exporter.py`'s existing `read_qfast_channel_bones`/
`read_axis_channel_bones` helpers generalize directly to `FnStatelessQ` and
`FnDeltaSingleQ` once their value-decode is closed out, with no new
bone-mapping logic needed.

## 4. Recommended next-session plan

1. `FnStatelessQ`: resolve §1.4 items 1 and 3 first (component count, second
   bone array) — both are cheap, targeted disassembly reads, not full
   function traces, and item 1 directly affects the root-motion / DOF-122
   open item from the memory context.
2. `FnDeltaSingleQ`: trace `EvalSQTMasked` (smaller of the two, 4632 vs 4516
   bytes — comparable size, pick whichever is called on the non-masked path
   first) in a dedicated session, budgeting for it being 2-3x the size of
   any function closed out so far.
3. Once both are closed, write `fnstatelessq_validate.py` /
   `fndeltasingleq_validate.py` mirroring the existing four validator
   scripts' structure, then extend `anm_exporter.py` to cover the remaining
   21 currently-skipped clips (12 `whole_clip` + 9 `FnDeltaSingleQ`).
