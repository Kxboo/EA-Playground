# EAGL `.anm` decoder — consolidated findings

Status snapshot as of this session. This supersedes prior partial
consolidations; where something below contradicts an earlier writeup,
this document is correct and the earlier one should be treated as
superseded (this happened at least twice already — clip 0's "validated"
status in session 20, and `FnDeltaSingleQ`'s angle-space framing in
session 28 — both corrected below).

Evidence tiers used throughout, consistent with the whole investigation:
**Confirmed** = disassembly-traced and/or corpus-validated; **Validated,
semantic meaning open** = mechanism seen, purpose unproven; **Open** =
lead only, not traced.

---

## 1. Container architecture (100% solved)

```
AnimBank
└── table_a[265]                self-relocated pointers, 265/265 resolve
    └── ClipBlock
        +0x00  00 TT <family16>   container tag
        +0x04  → bone/channel-index table (ascending, deduped u16 list —
                  see §6, now understood as a coarse "bones touched"
                  descriptor, NOT a per-channel decode-order map)
        +0x0C  → primary track marker
        +0x10  → secondary track marker
```

265 clips total. Every `ClipBlock` resolves. Tag distribution across the
whole corpus, independently re-verified this session by direct re-parse:

| Slot | Tag | Codec | Clips |
|---|---|---|---|
| primary | `0x12` | `FnDeltaQFast` | 244 |
| primary | `0x13` | `FnDeltaSingleQ` | 9 |
| primary | `None` | (no primary track) | 12 |
| secondary | `0x14` | `FnDeltaF3` | 224 |
| secondary | `0x15` | `FnDeltaF1` | 20 |
| secondary | `0x12` | `FnDeltaQFast` (mirrors the 9 `FnDeltaSingleQ` primaries exactly) | 9 |
| secondary | `0x17` | `FnStatelessQ` | 12 |

The 9 `FnDeltaSingleQ`-primary clips and the 12 `FnStatelessQ`-only clips
are exact, non-overlapping, disjoint sets — every clip in the corpus has
exactly one of these five combinations, 265/265.

"Primary" and "secondary" are just track-slot labels (`ClipBlock+0xC`
and `+0x10`) — **not** a guaranteed rotation/translation split. This
matters for the 12 `FnStatelessQ` clips: that codec occupies the
secondary slot yet writes directly into the pose buffer's *rotation*
quaternion sub-offset (§5.5), and those clips have no primary track at
all. Translation for those 12 clips most likely comes from a **nested
sub-object**, not a separate `ClipBlock`-level track (see §5.5,
`GetAnimatedBonesAux`'s virtual call through `map+0x10`).

---

## 2. `FnDeltaQFast` (tag `0x12`) — CLOSED, corpus-validated 244/244

The first codec solved, and the one every later codec's grammar was
compared against.

**Header** (all ELF-confirmed):
```
marker+0x04  SAMPLE_COUNT (u16 BE)
marker+0x06  CHANNEL_COUNT (u8)
marker+0x10  SHIFT (u8)
marker+0x12  payload_off (basis table start)
```

**Basis table**: `CHANNEL_COUNT × 16` bytes right after the header —
8 × u16 per channel (pivot+scale quad, quaternion x/y/z/w).

**Block structure**: `stream_start = payload_off + CHANNEL_COUNT*16`.
Per block: `CHANNEL_COUNT×6`-byte reference region (channel-major,
packed 12-bit quaternion), then `(2^SHIFT-1)×CHANNEL_COUNT×3`-byte delta
region (sample-major, channel-minor).

**Block stride**, ELF-confirmed byte-for-byte
(`SetAnimMemoryMap__FnDeltaQFast`, `0x804016a8`-`0x804016d0`):
```
block_size = round_down_even(CHANNEL_COUNT*(6 + 3*(2^SHIFT-1)) + 1)
```
`rlwinm r0,r0,0,0,0x1e` = literal "clear the low bit" compiled operation.

**Frame reconstruction**, ELF-confirmed
(`UpdateNextQs`/`AddDeltaMask`, `0x80400994`/`0x804000e8`):
```
qs[0] = block_reference
qs[n] = qs[n-1] + delta[n]      (forward running accumulation)
```
This is the single most important correction in the whole project's
early history: the original model (`qs[n] = reference + delta[n]`,
independently recomputed each sample) was indistinguishable from the
correct model on a single test channel/block, and was wrong. The
correction came from directly reading the `lfsx` (read previous
output) → `fadds` → `stfsx` (store back) instruction sequence — this
exact fingerprint was reused to confirm accumulation in every later
delta-based codec (`FnDeltaF3`, `FnDeltaF1`).

**Corpus validation** (independently re-run this session, not just
re-quoted): 244/244 clips, boundary-hit-rate 100% (1404/1404), reference
norms 0.9998-1.0000 (mean 0.9999), final norms mean 0.9997.

**Bone mapping** (§6): `marker+0x0C` → pointer → `CHANNEL_COUNT × u8`
bone-index table, read directly, no transform — one quaternion channel
= one bone, nothing to split.

**Deliverable**: `anm_validate.py`.

---

## 3. `FnDeltaF3` (tag `0x14`) — CLOSED, corpus-validated 224/224 (8 anomalous, flagged not broken)

Translation codec. Shares `FnDeltaQFast`'s container/stride/accumulation
grammar exactly; only the header offsets/widths and the per-channel
quantization formula differ.

**Header**:
```
marker+0x0C  SAMPLE_COUNT (u16 BE)
marker+0x0E  CHANNEL_COUNT (u16 BE)     -- widened to u16, unlike QFast's u8
marker+0x10  SHIFT (u8)
marker+0x14  per-channel input record table, CHANNEL_COUNT x 0x24 (36) bytes
```

**Per-channel 36-byte record**:
```
+0x00/04/08   E_x, E_y, E_z         (f32 BE)
+0x0C/10/14   B_x, B_y, B_z         (f32 BE)
+0x18/1A/1C   W0_x, W0_y, W0_z      (u16 BE)
+0x1E/20/22   W1_x, W1_y, W1_z      (u16 BE)
```

**Basis formula per axis**:
```
ref_base    = E
ref_scale   = E / 65535
delta_base  = B * (W0 * (2/65535) - 1)
delta_scale = B * (W1 * (2/65535)) / 255
```

**Decode**: reference region `CHANNEL_COUNT×6` bytes (3×u16/channel,
channel-major): `value = ref_base + raw_u16*ref_scale`. Delta region
`(2^SHIFT-1)×CHANNEL_COUNT×3` bytes (3×u8/channel, sample-major):
`d[n] = delta_base + raw_u8*delta_scale`, `qs[n] = qs[n-1] + d[n]`.

**Block stride**: identical formula shape to `FnDeltaQFast`, reused
verbatim (confirmed at the ELF instruction level).

**Corpus validation** (re-run independently this session): 224/224
clips decode without exceptions; 8 clips (41, 64, 65, 66, 90, 124, 171,
233) have `CHANNEL_COUNT==0` at this offset — a distinct, still-
unexplained 12-byte-stride sub-layout, not a decode failure (excluded
from aggregate stats, not silently averaged in). Mean of per-clip
ref_norm maxima: 1.747. Mean of per-clip delta_mag maxima: 0.449. 7/216
clips (of the 216 with `CHANNEL_COUNT>0`) have one outlier-magnitude
channel each, checked by hand and found smooth/bounded/consistent with a
genuinely large-range channel (root/hip translation), not corruption.

**Bone mapping** (§6): `marker+0x04` → pointer → `CHANNEL_COUNT × u16`
table, `bone = raw // 3`, `axis = raw % 3` — one bone's 3D translation
is stored as 3 separate scalar channels, hence the divide-by-3.

**Deliverable**: `fndeltaf3_validate.py`.

---

## 4. `FnDeltaF1` (tag `0x15`) — CLOSED, corpus-validated 20/20

Single-axis sibling of `FnDeltaF3` — same formula shape, same
accumulation, same rounding, one component instead of three. Confirmed
directly by disassembling `InitBuffersAsRequired__FnDeltaF1`
(`0x80405dc8`) and `EvalSQT__FnDeltaF1` (`0x80406a44`), not inferred
from file statistics (an earlier pure-regression attempt got the delta
width right but never converged on the block-reseed structure — logged
as a caution about trusting statistical fitting over disassembly).

**Header**:
```
marker+0x0C  SAMPLE_COUNT (u16 BE)   -- reuses FnDeltaF3's header shape exactly
marker+0x0E  CHANNEL_COUNT (u16 BE)
marker+0x10  SHIFT (u8)
marker+0x14  per-channel input record table, CHANNEL_COUNT x 0x0C (12) bytes
```

**Per-channel 12-byte record**: `E` (f32), `B` (f32), `W0` (u16), `W1`
(u16) — identical field shape to one axis of `FnDeltaF3`'s 36-byte
record (confirmed via `addi r6,r6,0xc` record-stride instruction).

**Basis formula**: identical to `FnDeltaF3`, single axis:
```
ref_base = E, ref_scale = E/65535
delta_base = B*(W0*(2/65535)-1), delta_scale = B*(W1*(2/65535))/255
```

**Decode**: reference region `CHANNEL_COUNT×2` bytes (1×u16/channel).
Delta region `CHANNEL_COUNT×1` byte/sample. Accumulation confirmed via
the same `lfsx`→`fadds`→`stfsx` fingerprint (`0x80406d5c`-`0x80406d68`).

**Block stride**: `round_down_even(CHANNEL_COUNT*(2 + (2^SHIFT-1)) + 1)`
— same instruction shape (`rlwinm r,r,0,0,0x1e`) as the other two.

**Corpus validation**: 20/20 clips, bounded, smooth, zero exceptions.
20/20 sample-count cross-match against the primary track.

**Bone mapping** (§6): same table field/shape as `FnDeltaF3`
(`marker+0x04`, u16, `bone = raw // 3`).

**Deliverable**: `fndeltaf1_validate.py`.

---

## 5. `FnDeltaSingleQ` (tag `0x13`) — CLOSED, corpus-validated 9/9

The hardest codec in the project to close, and the one with the most
corrected wrong turns along the way — documented here rather than
smoothed over, since the corrections are as informative as the final
answer.

### 5.1 What it is

A single-axis-**rotation** codec: each channel stores a quantized
rotation delta around one axis (selected per-channel by a flag), plus a
fourth always-live component. Contrary to the codec's naming implying
"angle-space" storage (an early, reasonable-looking hypothesis), the
accumulator turned out to hold **quaternion components directly**, not
angles — see §5.4.

### 5.2 Header (ELF-confirmed, `GetLength__FnDeltaSingleQ`, `0x803fd934`)
```
marker+0x04  SAMPLE_COUNT (u16 BE)
marker+0x06  CHANNEL_COUNT (u8)
marker+0x07  SHIFT (u8)
marker+0x08  optional non-uniform keyframe-time array pointer
```

### 5.3 `MinRange` record table — the actual bug that blocked this codec longest

**The bug**: `GetArrays__DeltaSingleQ` (`0x80408978`, fully disassembled,
28 bytes) computes the record table pointer as **`map + 0x10`**, not
`marker.payload_off` (`marker+0x12`) — the offset every other codec in
this project uses, and the offset this investigation assumed by analogy
for many sessions:
```
addi r6, r3, 0x10       ; MinRangePtr = map + 0x10
stw  r6, 0(r4)
lbz  r0, 6(r3)           ; channel_count, u8 @ +0x06
mulli r0, r0, 0xe        ; * 14 -- confirms record stride
add  r0, r6, r0
stw  r0, 0(r5)           ; bytePtr = MinRangePtr + channel_count*14
```
This one 2-byte offset error caused every downstream field read to land
one field early, producing a "flag" value of 127/255 (nonsense) instead
of clean small integers. It also retroactively explains an observation
from very early in this investigation, long before the mechanism was
understood: a `0x7fff` pattern spotted at `marker+0x10` was mislabeled a
"sentinel" — it was never a sentinel, it was channel 0's real first
field, correctly located, just misread because the table-start
assumption was 2 bytes off.

**Record layout** (14 bytes/channel, confirmed after the fix):
```
+0x00 u16  angle0   (init-only, feeds bind-pose construction, NOT the
                      per-sample delta path)
+0x02 u16  angle1   (same, init-only)
+0x04 u16  -> delta_base1  = trick/32768 - 1
+0x06 u16  -> delta_base2  = trick/32768 - 1
+0x08 u16  -> delta_scale1 = trick/32768
+0x0A u16  -> delta_scale2 = trick/32768
+0x0C u16  flag (axis selector: 0/1/else) -- the compare masks to the
            low byte via clrlwi, but in practice the high byte is 0 once
            the table offset is correct, so a full-halfword read works
            fine post-fix
```
After the fix, all 9 real clips' channels decode to a clean, uniform
`flag=0` — this corpus only ever exercises the `flag==0` branch (see
§5.5's caveat on the other two branches).

### 5.4 Seed + delta decode, and the corrected accumulator model

**Reference-byte seed** (once per block, 2 bytes/channel):
```
byte0 -> seed = trick*(2/255)-1, written into accum.slot[flag]
byte1 -> ALWAYS accum.slot[3] = trick*(2/255)-1
```

**Per-sample nibble deltas** (1 byte/channel/sample, 2 nibbles packed):
```
hi nibble -> accum.slot[flag] += trick(hi)*(delta_scale1/15) + delta_base1
lo nibble -> accum.slot[3]    += trick(lo)*(delta_scale2/15) + delta_base2
```
Accumulation confirmed sample-major, channel-minor, forward-only
(`qs[n] = qs[n-1] + delta[n]`), same convention as the other three delta
codecs.

**The corrected model**: an earlier pass (session 28) framed
`accum`/`this+0x24` as an "angle accumulator in radians," on the
reasoning that the record's `angle0`/`angle1` fields (via
`2π/65535, −π`) looked like angle quantization. That framing was wrong.
Full trace of the output-construction code (below) shows `accum`'s four
slots are read directly as **quaternion x/y/z/w components** in a
Hamilton product — slot 3 specifically is the `w` component, used
exactly as-is. The `angle0`/`angle1` fields are real but belong only to
the one-time bind-pose construction (§5.5), not the per-sample delta
path — the delta math never touches them, confirmed by their absence
from the traced delta-loop instructions.

### 5.5 Output construction — no per-frame trigonometry

The single biggest architectural finding of this codec: **the
Euler-angle→quaternion trig builder (`0x8040873c`, confirmed via a
`0.5` half-angle constant and paired `sin`/`cos` library calls) is
called exactly 4 times, all inside one-time initialization**
(`0x80408b18`-`0x80408b8c`), building two cached bind-pose quaternions
(`this+0x28`, `this+0x2c`). **It is never called in the per-frame
decode path.** Instead, the per-frame output is a direct Hamilton
quaternion product between the live delta accumulator and one (or, for
one flag value, both) of the cached buffers:
```
flag == 0:      qOut = Hamilton(accum, buf_2c)         [confirmed, all 9 real clips use this]
flag == 1:      qOut = Hamilton3(accum, buf_28, buf_2c)  [disassembly-traced, term order not fully reduced, untested against real data -- no clip in this corpus exercises it]
flag >= 2/else: qOut = Hamilton(buf_28, accum)          [disassembly-traced, untested against real data]
```
Output is written directly into the caller's bone-major pose buffer at
`bone_idx*0x30 + 0x10` (the quaternion sub-offset confirmed in §6.1),
with `bone_idx` read as a plain per-channel u8 — same simple convention
as `FnDeltaQFast`.

### 5.6 Corpus validation (9/9 clips, post-fix)

| clip | samples | chans | norm mean | norm min | norm max | step mean° | step max° |
|---|---|---|---|---|---|---|---|
| 6   | 13 | 5  | 0.9959 | 0.9928 | 1.0001 | 9.40  | 54.42 |
| 100 | 54 | 7  | 0.9942 | 0.9922 | 1.0000 | 1.35  | 11.54 |
| 104 | 32 | 2  | 0.9961 | 0.9938 | 1.0000 | 0.73  | 2.76  |
| 108 | 19 | 5  | 0.9971 | 0.9928 | 1.0001 | 10.23 | 32.40 |
| 110 | 19 | 7  | 0.9956 | 0.9922 | 1.0017 | 3.80  | 35.06 |
| 111 | 97 | 1  | 0.9954 | 0.9922 | 1.0000 | 1.42  | 3.54  |
| 137 | 34 | 9  | 0.9964 | 0.9923 | 0.9996 | 1.34  | 33.21 |
| 142 | 19 | 7  | 0.9960 | 0.9924 | 0.9996 | 2.30  | 18.90 |
| 143 | 17 | 10 | 0.9975 | 0.9937 | 1.0016 | 4.70  | 25.12 |

Matches the quality bar set by the other three codecs (0.99-1.00 norm
range). 9/9 sample-count cross-match against the mirrored `FnDeltaQFast`
secondary track (these 9 clips have primary=`0x13`, secondary=`0x12`,
an exact bijection with zero symmetric difference).

**Deliverable**: `fndeltasingleq_validate.py`.

**Honestly still open**: the `flag==1`/`flag>=2` branches (untested
against real data — nothing in the current corpus exercises them, so if
a different `.anm` file or a future clip hits them, they're unverified);
the exact algebraic term order of the `flag==1` three-buffer
composition (traced but not fully reduced to a clean formula).

---

## 6. Bone mapping (mechanism confirmed; corpus-wide proof not yet run)

### 6.1 Runtime consumer chain (`Skeleton::PoseSQTToGlobal`, `0x803f4524`)

- Pose input buffer is **bone-major**, `0x30` bytes/bone stride, output
  `Transform`/matrix `0x40` bytes/bone.
- Per-bone SQT record (0x30 bytes): `+0x00/04/08` translation,
  `+0x10/14/18/1C` **quaternion** (confirmed independently by every
  codec's output-write instructions landing exactly here), `+0x20`
  extra float (matrix-build input), `+0x0C/24/28` scale factor.
- `BoneMask` bit test, confirmed identical everywhere it appears:
  `word = mask[bone>>5]; bit = 1<<(bone&0x1F); set = word & bit`.
- `BoneInfo` table, `0x70` bytes/bone, `+0x0C` = parent bone index
  (`<0` = root). Hierarchy propagation confirmed: parent-relative →
  global matrix chaining via a function-pointer call.

### 6.2 The real per-channel table (supersedes an earlier falsified hypothesis)

**Falsified, corpus-wide (253/253 clips checked)**: `ClipBlock+0x04`'s
ascending u16 list is **not** a per-channel, decode-order bone table —
`len(bones) == channel_count` failed on every single clip, with
signed, non-constant differences. Range/ascending/uniqueness all
passed, consistent with the list being a coarser "which bones does this
clip touch" descriptor instead.

**Confirmed replacement**, via `GetAnimatedBonesAux` disassembly per
codec:

| Codec | Table field | Width | Decode |
|---|---|---|---|
| `FnDeltaQFast` | `marker+0x0C` | u8 | `bone = raw`, direct |
| `FnDeltaF1` | `marker+0x04` | u16 | `bone = raw // 3` (axis-split) |
| `FnDeltaF3` | `marker+0x04` | u16 | `bone = raw // 3` (shape matches F1; not independently re-derived field-by-field) |
| `FnDeltaSingleQ` | direct u8 table (analogous to QFast) | u8 | `bone = raw`, direct |
| `FnStatelessQ` | `map+0x0C` | u8 | `bone = raw`, direct — count at `map+0x16` |

`FnStatelessQ`'s `GetAnimatedBonesAux` also revealed something new: it
optionally chains to a **nested sub-object's own `GetAnimatedBonesAux`**
via a vtable call through `map+0x10` — meaning some codecs can embed a
second codec instance rather than only appearing at `ClipBlock` level.
Not yet explored for what that nested object actually is (§9, open
item).

`BoneMask::SetBone` (`0x803fc9d4`), confirmed identical everywhere:
```c
word = bone_idx >> 5; bit = 1 << (bone_idx & 0x1F);
if (value) mask[word] |= bit; else mask[word] &= ~bit;
```

### 6.3 What is NOT yet done

Session 25 proposed, and this project has not yet run, the corpus-wide
closing proof: for every clip, compute
`sorted(set(bone_index for entry in <codec-specific table>))` and
compare it exactly against `ClipBlock+0x04`'s ascending list. If it
matches corpus-wide, that retires the bone-mapping question with hard
evidence rather than a plausible-sounding story. **This has not been
run.** `validate_bone_mapping.py` exists but only tests the *already-
falsified* length-agreement hypothesis — it does not yet implement this
closing check.

---

## 7. Corpus coverage summary

| Codec | Status | Clips | Track role |
|---|---|---|---|
| `FnDeltaQFast` | Closed, corpus-validated | 244 | rotation |
| `FnDeltaF3` | Closed, corpus-validated | 224 | translation |
| `FnDeltaF1` | Closed, corpus-validated | 20 | translation |
| `FnDeltaSingleQ` | Closed, corpus-validated | 9 | rotation |
| `FnStatelessQ` | Architecture + quantization confirmed; corpus wiring broken | 12 | rotation (at least) |

- **244 clips**: rotation + translation both fully decoded
  (224 via `FnDeltaF3`, 20 via `FnDeltaF1`).
- **9 clips**: rotation decoded via `FnDeltaSingleQ`; these clips'
  *other* track is `FnDeltaQFast` (already decoded), so once
  `FnDeltaSingleQ`'s output is wired into the same extraction path as
  the others, these 9 also become fully decoded.
- **12 clips**: architecture and per-component math for `FnStatelessQ`
  are solved, but the actual per-clip decode does not yet run
  correctly end-to-end (see §8).
- **0 clips** remain with a genuinely unknown codec family. Every tag
  in the corpus is now attributable to one of these five, named
  functions.

253/265 clips are decodable with currently-working code
(`anm_validate.py` + `fndeltaf3_validate.py`/`fndeltaf1_validate.py` +
`fndeltasingleq_validate.py`, once wired together). 12/265
(`FnStatelessQ`) need one more fix before they join that count.

---

## 8. `FnStatelessQ` — where it currently stands (most recent, still-open work)

Different architecture from all four closed codecs: **keyframe search +
linear interpolation**, not running accumulation — the name "Stateless"
reflects this (no persistent accumulator between frames; every query
independently reconstructs from two bracketing keyframes).

**Confirmed**:
- `map+0x08`: keyframe-time table pointer; when present, triggers a
  real binary search over bracketing keyframes
  (`0x803fd310`-`0x803fd3a4`)
- `map+0x10`: cached last-found keyframe index (temporal-coherence
  optimization for sequential playback) — **note**: this is a
  *different* `map+0x10` than `FnDeltaSingleQ`'s record-table pointer;
  field meaning is codec-local, not shared across the project
- `map+0x12`/`+0x13`: FPS-related flags (ties to the `UseFPS` symbol)
- `map+0x14`: `SAMPLE_COUNT` (u16), via `GetLength`
- `map+0x16`: `CHANNEL_COUNT` (u8), shared with `GetAnimatedBonesAux`
- `map+0x18`: per-keyframe, per-channel record table, **8 bytes/channel/
  keyframe** (4×u16 — full x/y/z/w, not a smallest-three/component-
  dropping scheme as originally speculated many sessions ago)
- Genuine LERP (`fmadds`, not slerp) between two bracketing keyframes'
  converted quaternion components
- Output writes to the same `0x30`-stride bone-major pose buffer at
  `+0x10`, confirmed against §6.1

**Quantization formula — fully traced and confirmed this session**, the
per-component `u16 → float` conversion:
```
r3 = rlwinm(raw_u16, sh=15, mb=2, me=16)
r3 = rlwimi(r3, raw_u16, sh=16, mb=0, me=0)
value = reinterpret_bits_as_f32(r3)     -- via a stack store/reload
                                            round-trip (PPC's standard
                                            "move int bits to FPR"
                                            idiom pre-VSX)
```
Traced every floating-point instruction between the first `rlwinm` and
the first `stfs`/`stfsux`: 8×`lfs`, 4×`fsubs` (delta = keyframeA −
keyframeB per component), 4×`fmadds` (lerp), nothing else. **No
constant is loaded anywhere in this window** — confirmed by exhaustive
grep of the instruction range, not by absence-of-evidence assumption.
This rules out a missing bias step; the bit-construction result is used
directly, unlike every other codec's `int_to_float_trick` (which always
subtracts a `2^52` bias immediately after the equivalent bit-load step).
Hand-verified bit-exact for `V=1` (→ `4.59e-41`, a subnormal float, by
manual IEEE-754 field derivation) and cross-checked against the Python
replication — matches exactly. `V=0x7FFF → ~1.996`, `V=0xFFFF → ~-1.996`
— consistent with a deliberate non-uniform (log-like) quantization: fine
resolution near zero, coarser toward the range's extremes, plausible for
values that are themselves keyframe-to-keyframe deltas clustering near
zero.

**Not yet working**: applying the above corpus-wide, assuming
`this+0xC` ("map") equals `marker.abs_off` exactly as it does for every
other codec, produced garbage (sample counts in the tens of thousands,
9/12 clips reading as empty). `SetAnimMemoryMap__FnStatelessQ` is only 8
bytes — essentially a no-op — meaning `this+0xC` is populated somewhere
this investigation hasn't traced (most likely a shared base-class
loader function, given the vtable-dispatch pattern already seen in
`GetAnimatedBonesAux`'s nested-object call). This is the single
concrete blocking item left in the whole project.

**`FnStatelessF3`**: not started. Given the `FnDeltaQFast`/`FnDeltaF3`
and `FnDeltaSingleQ`/(implied translation partner) pattern, a
`FnStatelessQ`/`FnStatelessF3` split along the same rotation/translation
line is the reasonable working hypothesis, but nothing about it has
been independently verified — flagged as hypothesis, not finding.

---

## 9. Open items, roughly in dependency order

1. **Trace where `FnStatelessQ`'s `this+0xC` is actually populated.**
   Almost certainly a shared loader/base-class function. This is the
   single blocking item for finishing the last codec.
2. **`FnStatelessF3`** — not started at all. Likely mirrors
   `FnStatelessQ`'s keyframe/lerp architecture per the project's
   established Q/F3 pairing pattern, but unverified.
3. **Bone-mapping corpus proof** (§6.3) — the validator exists in
   partial form (`validate_bone_mapping.py`) but tests the wrong
   (already-falsified) hypothesis. Needs the actual closing check:
   codec-specific table → sorted bone set → compare against
   `ClipBlock+0x04`'s list, corpus-wide.
4. **Wire `FnDeltaSingleQ` and (once fixed) `FnStatelessQ` into
   `anm_exporter.py`**, which currently only handles the
   `FnDeltaQFast`+`FnDeltaF3`/`FnDeltaF1` combination (244/265 clips).
5. **`FnDeltaSingleQ`'s `flag==1`/`flag>=2` branches** — disassembly-
   traced, never exercised by real corpus data, so unverified in
   practice.
6. **`FnDeltaF3`'s 8 `CHANNEL_COUNT==0` clips** and their distinct
   12-byte-stride sub-layout — still unexplained.
7. **Nested sub-object mechanism** (`GetAnimatedBonesAux`'s vtable call
   through `map+0x10` for `FnStatelessQ`) — not explored at all; likely
   relevant to where these 12 clips' translation data actually lives.
8. **Frame rate** — `anm_exporter.py` currently uses a documented
   30fps placeholder; true playback FPS not independently confirmed.
9. **Runtime/ground-truth verification** — nothing in this project has
   yet been checked against an actual rendered skeleton or game capture.
   Everything above is internally consistent (norms, sample-count
   cross-matches, ELF-instruction traces) but not externally verified.

---

## 10. Deliverables inventory

| File | Contents |
|---|---|
| `eagl_anm_decoder.py` | Shared container/ELF-relocation parsing infrastructure |
| `anm_validate.py` | `FnDeltaQFast` decoder + corpus validator |
| `fndeltaf3_validate.py` | `FnDeltaF3` decoder + corpus validator |
| `fndeltaf1_validate.py` | `FnDeltaF1` decoder + corpus validator |
| `fndeltasingleq_validate.py` | `FnDeltaSingleQ` decoder + corpus validator |
| `anm_exporter.py` | glTF/GLB exporter — currently covers 244/265 clips |
| `validate_bone_mapping.py` | Falsification test for the (wrong) length-agreement bone-mapping hypothesis; needs extension per §6.3/§9 item 3 |
| `player_anims_extracted.json` | Raw extraction output, 244 clips, rotation+translation per channel per frame |
| `eagl_skeleton.py` | `.ske` parser (bind pose, hierarchy, names) |
| `elf_tools.py` / `gekko_ps.py` / `disasm.py` | ELF32BE parser, Gekko/Broadway paired-single decoder, combined capstone disassembler with word-resync (needed because capstone mis-decodes a handful of `fcmpo`/VSX-adjacent PPC opcodes mid-function) |
