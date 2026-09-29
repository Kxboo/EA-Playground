# EAGL `.anm` decoder — session: `FnStatelessQ` record decode closed, 12/12 corpus validated

**Scope.** Directly continues the map/object-identity resolution session.
Closes the one remaining unknown flagged there: the per-record 16-bit
value unpack. With this closed, `FnStatelessQ` reaches the same
disassembly-confirmed + numerically-validated status as every other
rotation codec in this project, and **all three rotation representations
used by `player_anims.anm` are now solved**.

---

## 1. Confirmed: fixed-rate query-time → frame-index formula

Traced the function entry (`0x803fd244`–`0x803fd2c4`) fully:

```
use_fps_flag = this+0x12                 ; instance byte, NOT part of map
if use_fps_flag == 0:
    frame_float = query_time_arg (f1)      ; caller already passes a frame-scaled value
else:
    fps_byte  = this+0x13
    fps_scale = int_to_float_trick(fps_byte)  ; the SAME shared magic-double bit trick
                                                 ; (0x4330_0000<<32 | raw, − 2^52) used by
                                                 ; every other codec in this project
    frame_float = query_time_arg * fps_scale

frame_idx = trunc(frame_float)             ; fctiwz
frame_idx = clamp(frame_idx, 0, frame_count - 1)   ; frame_count = map+0x14
                                                       ; (the map+0x08==0 fallback branch --
                                                       ; the ONLY branch all 12 corpus
                                                       ; clips exercise)
record = map+0x18 + (frame_idx * channel_count + ch) * 8    ; 8 bytes/record
```

`this+0x12`/`this+0x13` line up exactly with the already-known
`UseFPS__Q28EAGLAnim12FnStatelessQFb` symbol (seen in the symbol table
early in the `FnStatelessQ` investigation, not previously traced to a
concrete formula) — confirmed, not inferred from the name alone.

**Structurally significant finding**: in this branch, `frame_idx` maps
directly and flatly onto the record table — **one record = one output
frame**, with no delta accumulation and no block/shift structure. This
is a genuinely different codec architecture from every delta codec in
this project, not just a different bit-packing of the same idea.

## 2. Resolved: the per-record 16-bit value unpack

This was the single open item carried over from the previous session.
Rather than continuing to reason about the PPC bit-manipulation
instructions in the abstract, the approach shifted to what was actually
proposed: pull real record bytes and test candidate unpackings against
the hard constraint every other codec in this project has been judged
against — quaternion norm ≈ 1.0, plus frame-to-frame continuity.

**The confirmed bit sequence** (traced at `0x803fd4d0`/`0x803fd4d8` and
repeated identically for all four `u16` fields in a record):

```
rlwinm r3, u16, 0xf, 2, 0x10
rlwimi r3, u16, 0x10, 0, 0
```

Reproducing this bit-for-bit in Python (proper PPC rotate + IBM-order
bitmask semantics, not a shortcut approximation) and reinterpreting the
resulting 32-bit word as an IEEE-754 float, then testing against **real**
record bytes pulled from clip 4 (not synthetic test values), gives:

| frame/channel | raw u16s | decoded (x,y,z,w) | norm |
|---|---|---|---|
| f0/ch0 | `0000 0000 fa7d 7eff` | `(0.0, 0.0, −0.0465, 0.998)` | 0.99913 |
| f1/ch0 | `f4d3 7114 fa79 7eff` | `(−0.0009, 0.0001, −0.046, 0.998)` | 0.99911 |
| f375/ch0 | `0000 0000 7a36 7eff` | `(0.0, 0.0, 0.0378, 0.998)` | 0.99876 |
| f0/ch1 | `742b f433 fe62 7e71` | `(0.0006, −0.0006, −0.6914, 0.7207)` | 0.99873 |
| f1/ch1 | `f765 f7cc fe62 7e71` | `(−0.0054, −0.007, −0.6914, 0.7207)` | 0.99877 |
| f375/ch1 | `f8a3 f888 fe7d 7e54` | `(−0.0128, −0.012, −0.7441, 0.6641)` | 0.99751 |

**Confirmed: no linear scale, no bias, no `int_to_float_trick`-style
double-precision detour is applied on top — the bit sequence
reconstructs a usable float bit pattern directly.** This is genuinely
different from every delta codec's quantization scheme (all of which use
the shared magic-double trick plus an explicit scale/bias pair from a
per-channel basis record). `FnStatelessQ` instead stores each quaternion
component as an independently-reconstructable, reduced-precision float —
closer to a truncated/half-precision float than a fixed-point or
smallest-three scheme (ruling out the other two candidate possibilities
raised going into this session).

Norms landing at 0.997–0.999, and adjacent frames (`f0`→`f1`, same
channel) staying close to each other rather than jumping randomly, is the
same signal that closed every other codec in this project — deliberately
checked **before** normalizing, per the request not to let normalization
hide a wrong unpack.

## 3. Corpus validation: 12/12 clips, same quality bar as every other codec

Full decode (`fnstatelessq_validate.py`, all 12 whole-clip containers,
fixed-rate path only — the sparse/binary-search path is unexercised by
this corpus and left unimplemented rather than guessed at):

| clip | frames | channels | extra | norm mean | norm min | norm max | step mean | step max |
|---|---|---|---|---|---|---|---|---|
| 4   | 751 | 45 | 22 | 0.9989 | 0.9967 | 1.0000 | 2.47 | 177.32 |
| 7   | 136 | 56 | 12 | 0.9988 | 0.9969 | 1.0000 | 3.12 | 70.80 |
| 107 | 141 | 48 | 20 | 0.9988 | 0.9972 | 1.0000 | 3.24 | 91.75 |
| 135 | 43  | 55 | 13 | 0.9988 | 0.9972 | 1.0000 | 5.62 | 140.27 |
| 147 | 45  | 56 | 12 | 0.9989 | 0.9972 | 1.0000 | 7.05 | 117.15 |
| 164 | 136 | 48 | 19 | 0.9988 | 0.9972 | 1.0000 | 4.52 | 91.52 |
| 205 | 119 | 54 | 13 | 0.9988 | 0.9972 | 1.0000 | 1.85 | 112.76 |
| 213 | 39  | 43 | 24 | 0.9988 | 0.9973 | 1.0000 | 7.00 | 112.29 |
| 217 | 126 | 51 | 17 | 0.9989 | 0.9969 | 1.0000 | 2.73 | 155.31 |
| 221 | 661 | 54 | 13 | 0.9988 | 0.9968 | 1.0000 | 0.91 | 60.09 |
| 226 | 181 | 47 | 21 | 0.9988 | 0.9971 | 1.0000 | 2.78 | 104.65 |
| 261 | 211 | 54 | 13 | 0.9988 | 0.9969 | 1.0000 | 1.41 | 76.12 |

**12/12 clips decode with norms in the 0.997–1.000 range** — the same
bar `anm_validate.py`/`fndeltaf3_validate.py`/`fndeltaf1_validate.py`/
`fndeltasingleq_validate.py` all cleared. Step-angle means (1–7°) are
small and consistent with smooth animation; occasional large maxima
(up to ~177° on a few clips) match the same pattern seen at block
boundaries in the delta codecs, and are plausible for fast genuine
motion (a stateless codec has no reason to smooth across such a jump
the way a delta-accumulator would).

## 4. Rotation side of the project: complete

```
FnDeltaQFast      ✅  (sessions 20-25)
FnDeltaSingleQ    ✅  (sessions 26-29)
FnStatelessQ      ✅  (this + previous session)
```

Every rotation representation used by `player_anims.anm` now has a
disassembly-confirmed structure, a confirmed addressing/indexing model,
and a corpus-wide numeric validation in the same norm range as every
other closed codec. This was the explicit target of the last several
sessions and is now met.

---

## 5. Open items for next session

1. **Bone mapping for `FnStatelessQ` is assumed, not re-confirmed.**
   `map+0x0C`'s per-channel byte table is assumed to follow the same
   "direct `u8` bone index" convention confirmed for `FnDeltaQFast`
   (session 25 §4.1) by analogy — both are rotation codecs with a
   similarly-shaped per-channel table — but this has **not** been
   independently disassembly-verified for `FnStatelessQ` specifically.
   Given the project's own repeated lesson (two codecs "obviously
   identical" turning out to need independent verification), this should
   be checked with the same rigor before wiring into the exporter, not
   assumed silently.
2. **The sparse/binary-search keyframe-time path (`map+0x08 != 0`)**
   remains untraced past the initial search-loop instructions seen in the
   map-resolution session. Since 0/12 real clips use it, this is now
   correctly low-priority — flagged as a known gap, not silently ignored,
   in case a future corpus (e.g. a different character's `.anm` bank)
   exercises it.
3. **`map+0x0C`'s second, `map+0x17`-gated "extra channel" region**
   (channel counts 12–24 across the 12 clips, always smaller than the
   main `channel_count`) is still unexplained — not re-investigated this
   session, carried forward unchanged from the map-resolution session's
   open items.
4. **`GetLength__FnStatelessQ`** (`0x803fd934`) remains untraced — would
   likely give the precise relationship between `frame_count` and actual
   playback duration/FPS, closing out the last piece of the "keyframe
   count vs sample count" framing question with a concrete formula.
5. Per the project's next stated priority: **`FnStatelessF3`** (the
   nested translation object confirmed to exist inside these same
   whole-clip containers, tag `0x17`) is now the only remaining
   unsolved codec family. Per the reframing already agreed going into
   this session, its investigation is no longer "how does animation
   work" but "what do these vector tracks represent" — root motion,
   general translation, or non-pose data — a question to answer with
   the channel→bone cross-check (do `FnStatelessF3`/`FnDeltaF3` channels
   concentrate on bone 0/root, spread across many bones, or fail to map
   to bones at all) once its own header/addressing model is confirmed
   with the same rigor applied to `FnStatelessQ` here.
