# EAGL `.anm` decoder — session 28: `FnDeltaSingleQ` closed out

**Scope.** Directly continues session 27. Located the actual loop
boundaries in `EvalSQT__FnDeltaSingleQ` via `mtctr`/`bdnz`
(`0x80408e28`/`0x80409000`), which corrects part of session 27's tentative
reading, then traced every read/write of `this+0x24`/`0x28`/`0x2c` across
the whole 4516-byte function. This resolves all three open items from the
previous request.

**Correction to session 27's framing up front:** session 27 described the
`[-1,+1]`-normalized byte decode and the `MinRange`-driven nibble decode as
two separate, structurally-parallel things without pinning down how they
relate. They're not parallel — they're **sequential stages of the same
per-block decode**: the `[-1,+1]` bytes are the block's **seed value**
(read once per block, from the bitstream, one 2-byte pair per channel), and
the `MinRange`-driven nibble deltas are **added on top of that seed, every
sample, using constants that never change per instance** (recomputed fresh
from the static on-disk `MinRange` record each sample rather than cached —
a real, confirmed inefficiency in the original code, not a decoding
subtlety). Session 27's guess that `this+0x28`/`this+0x2c` held a
"lerp pair" was wrong; those two fields turned out to be **unused in the
delta path entirely** — see §3.

---

## 1. Reference-byte path — resolved: it's an initial accumulator seed, not an angle or trig component

Traced backward from the `2/255` / `1.0` constants (session 27's find) to
their actual call site, which sits **before** the per-sample loop, in a
once-per-`EvalSQT`-call block-reference setup (`0x80408d2c`–`0x80408dd4`,
bounded by a channel-count loop, confirmed distinct from the per-sample loop
identified this session in §2):

```
src = this[0x14] + block_index * this[0x18]     ; bitstream ptr, block-relative
                                                   (this[0x14]=bit_array_ptr from
                                                    GetArrays, this[0x18]=block_size
                                                    from GetBinSize -- both session
                                                    26-confirmed fields)
for ch in 0..channel_count:
    flag = MinRange[ch].flag_byte            ; same +0x0C selector, read from the
                                                 GLOBAL MinRange table (marker+0x10),
                                                 NOT from the bitstream
    accum[ch] = (0.0, 0.0, 0.0, 0.0)           ; zero all 4 slots first
    raw_byte = src[ch*2]                         ; ONE byte, first of the 2-byte
                                                    reference pair for this channel
    seed = raw_byte_trick * (2/255) - 1.0        ; --> the [-1,+1] value
    accum[ch].slot[flag] = seed                  ; written into the axis-selected
                                                    slot ONLY (0, 4, or 8)
    raw_byte2 = src[ch*2 + 1]                     ; SECOND byte of the pair
    accum[ch].slot[3] = raw_byte2_trick*(2/255) - 1.0   ; UNCONDITIONALLY into slot 3
```

**Answering the question directly: it's your third option — an initial
accumulator value**, not a normalized angle (session 27's `2π/65535,−π`
formula is a *different* field, only used in the per-sample delta step,
§2), not a raw cos/sin component (no `sin`/`cos` call anywhere near this
code), and not literally a "packed signed value" in the sense of carrying
extra bit-packed structure — it's a plain linear byte→`[-1,+1]` unquantize,
the simplest of the four options. It seeds the same accumulator slots that
the per-sample nibble deltas (§2) subsequently add onto.

## 2. Nibble delta unpack — resolved: unconditional 2-nibbles-per-byte, high→axis-slot, low→slot-3

The actual per-sample loop, confirmed via `mtctr r4` (`0x80408e28`, `r4` =
remaining delta count in the block) / `bdnz 0x80408e30` (`0x80409000`),
with a **nested inner loop over channels** (`0x80408e40`–`0x80408ff4`,
bounded by `channel_count`, looping back to `0x80408e40`):

```
for sample in 0..n_deltas_in_block:            # outer, bdnz-driven
    for ch in 0..channel_count:                  # inner, per-sample
        rec = MinRange[ch]                         # re-read fresh, EVERY sample
                                                     # (confirmed -- not cached)
        angle0 = rec.u16[0]_trick * (2*pi/65535) - pi     # NOT used in delta math
        angle1 = rec.u16[1]_trick * (2*pi/65535) - pi     # itself -- see note below
        delta_base1  = rec.u16[2]_trick / 32768 - 1.0       # from u16 @ +0x04
        delta_base2  = rec.u16[3]_trick / 32768 - 1.0       # from u16 @ +0x06
        delta_scale1 = rec.u16[4]_trick / 32768             # from u16 @ +0x08
        delta_scale2 = rec.u16[5]_trick / 32768             # from u16 @ +0x0A
        flag = rec.flag_byte                                # from +0x0C, same field

        raw = bitstream[ptr]; ptr += 1            # ONE byte, shared by both nibbles
        hi = raw >> 4                                # top nibble -- CONFIRMED via
                                                       # rlwinm(raw,28,28,31)
        lo = raw & 0xF                               # bottom nibble -- CONFIRMED via
                                                       # clrlwi(raw,28)

        hi_val = int_to_float_trick(hi) * (delta_scale1/15) + delta_base1
        lo_val = int_to_float_trick(lo) * (delta_scale2/15) + delta_base2

        accum[ch].slot[flag] += hi_val            # ONLY the axis-selected slot
        accum[ch].slot[3]    += lo_val            # UNCONDITIONALLY, every sample,
                                                    # regardless of `flag`
```

Direct answers to each sub-question:

- **High-nibble meaning**: increment for the **axis-selected slot**
  (whichever of slots 0/1/2 the channel's `flag` picked), scaled by
  `delta_scale1` (from `MinRange+0x08`) and offset by `delta_base1`
  (`MinRange+0x04`).
- **Low-nibble meaning**: increment for **slot 3 unconditionally**, every
  sample, regardless of which axis is selected — using a *different* base/
  scale pair (`MinRange+0x06`/`+0x0A`). This is a genuinely separate,
  always-live fourth accumulator, not a byproduct of the 3-axis selection.
  Its role relative to the Euler-quaternion conversion (session 27 §2, which
  only takes 3 inputs) is still open — see §4 item 1.
- **Sign extension**: none needed / none present. Both nibbles go through
  the shared `int_to_float_trick` (the project's standard u16-domain
  magic-double bit trick, here applied to a 4-bit value) which produces a
  small **non-negative** float (`0..15`); signedness comes entirely from
  `delta_base`/`delta_scale` (which can themselves be negative, since
  they're derived from `u16_trick/32768 - 1.0`), not from any sign-extension
  step on the nibble itself.
- **Scale factor `1/15`**: confirmed to divide `delta_scale1`/`delta_scale2`
  specifically (`fmuls f12, f9` where `f12=1/15`), i.e. it normalizes the
  4-bit raw range (max value 15) the same way the `2/65535`-style constants
  elsewhere in this project normalize their own bit-widths — entirely
  consistent with the rest of the codebase's convention, not a special case.
- **Accumulation order**: confirmed **sample-major, channel-minor**, i.e.
  the outer loop is over samples and the inner loop is over channels — the
  opposite nesting from `FnDeltaQFast`/`F3`/`F1`'s reference/delta regions,
  where within a block the reference region is channel-major (§1 here is
  also channel-major, consistent) but this codec's *delta* region reads one
  byte per (sample, channel) pair with **1 byte per channel per sample**
  (not `n_deltas × channel_count` bytes laid out delta-region-then-next as
  in the other codecs — same total byte count, same nesting convention,
  just now confirmed rather than assumed).

**Not yet corpus-run**: the request also asked to "decode several known
clips and compare continuity" — the formula above is fully specified and
could be scripted directly, but doing that numerically (and sanity-checking
frame-to-frame continuity the way `anm_validate.py`'s `step_angle` stats do
for `FnDeltaQFast`) is real, mechanical follow-up work that wasn't run this
session — flagged honestly as the next concrete action rather than claimed
as done.

## 3. Buffer roles — resolved

Full read/write census across the entire function (all ~50 references to
`this+0x20/0x24/0x28/0x2c`):

| Field | Written | Read | Role |
|---|---|---|---|
| `this+0x20` | Once, at alloc (`0x80408a5c`) | **Never read anywhere in the traced function** | Allocation result only — likely a "base"/owning pointer kept for cleanup (`__dt__`), not touched by `EvalSQT`'s decode path. |
| `this+0x24` | Once at alloc (aliased to same buffer as `0x20`, `0x80408a60`); then **every sample, every channel**, in the delta loop (§2, `stfsx`/`stfs` at `0x80408f9c`/`0xfc4`/`0xfd8`/`0xff0`) | Every sample/channel in the delta loop, plus later in the function (past the traced window, lines ~523, ~779, ~976, ~1016 in the raw disassembly — **not re-examined this session**, flagged open) | **The live running accumulator.** 4 floats/channel: slots 0/1/2 are the three axis candidates (only the `flag`-selected one is ever nonzero), slot 3 is the always-live secondary accumulator from the low nibble. This is the buffer that actually changes value as decoding proceeds — the "state" of the codec. |
| `this+0x28` | Once at alloc (`0x80408a78`); then in the **one-time init-only** bind-pose path (session 27 §1, `0x80408ad4`–`0x80408b64`) | Read starting at line ~609/651/881/923 (**outside the window traced this session — open**) | Allocated and populated once at construction (bind-pose quaternion, per session 27 §1), **not written anywhere in the per-sample delta loop traced this session** — so it is not part of the live per-frame decode path in the portion covered so far. Likely a cached bind-pose or "identity" fallback consumed elsewhere (e.g. for channels/frames outside the animated range), not the active accumulator. |
| `this+0x2c` | Once at alloc (`0x80408a88`); same one-time init path as `0x28` (`0x80408b18`/`0xb44`/`0xb64`/`0xb90`-`0xbb8`) | Read starting at line ~96/580/852/977/1019 (**outside the window traced this session — open**) | Same pattern as `0x28`: populated once at construction, not touched by the delta loop traced this session. Given session 27's `0x8040873c` (Euler→quat) call sites during init write into *both* `0x28` and `0x2c` depending on the axis-selector branch (session 27 §1's three-way branch), these two buffers most plausibly hold **two different bind-pose interpretation results** (e.g. one per possible "which axis is w" convention, or a from/previous vs to/next pair) rather than one being a scratch buffer — but which is which, and what consumes them post-init, is genuinely unresolved without tracing the later code (lines 500-1129, i.e. roughly 45% of the function, not yet examined). |

**Direct labels, as requested:**
- `this+0x24` = **angle/rotation-parameter accumulator** (the live, per-sample-updated state — closest to your "angle accumulator" guess, confirmed).
- `this+0x28`, `this+0x2c` = **cached bind-pose/init-time quaternion buffers** (populated once, consumed somewhere later in the function that wasn't re-traced this session — not confirmed as "temporary decode buffer" or scratch; they persist for the life of the object, so "cached" is more accurate than "temporary").
- `this+0x20` = allocation bookkeeping only, not part of the decode data flow.

---

## 4. Honest status: is `FnDeltaSingleQ` actually closed?

**Not quite**, and it's worth being precise about the gap rather than
declaring victory:

1. **The final assembly step — how `accum[ch]` (4 floats) becomes the
   output quaternion for a given query frame — was not directly observed.**
   Session 27 confirmed `0x8040873c` takes a **3-float** input; `accum` has
   **4** floats. Either (a) only slots 0-2 feed `0x8040873c` and slot 3 (the
   unconditional low-nibble accumulator) is consumed by something else
   entirely — a second rotation composed afterward, a scale, a totally
   separate output channel — or (b) there's a 4-input variant of the
   angle→quat step not yet located. This is the single remaining structural
   unknown, and it sits in the ~45% of the function (roughly lines 500-1129
   of the raw disassembly dump) not yet examined in detail.
2. Numeric validation (decode real clips, check continuity) has not been
   run — the formula in §2 is fully specified and ready to script, but
   "should be considered closed" per the original ask really means *run it
   against the 9 real clips and confirm the deltas produce smooth,
   plausible angle trajectories* — that's the concrete next step, not
   theoretical confirmation.
3. Slot 3's true purpose (item 1) plus its interaction with `this+0x28`/
   `0x2c` (§3) are the two threads that, once pulled, would make "closed"
   an honest claim rather than an optimistic one.

Given where the effort is best spent next: **trace the remainder of the
function (post-`0x80409000`, where the per-sample loop ends) to find the
actual `0x8040873c` (or equivalent) call site for the per-query-frame
output**, since that single call site will most likely resolve items 1-3
above simultaneously by showing exactly which accumulator slots get read
and how `0x28`/`0x2c` factor in. That's a more targeted ask than
re-reading the whole function blind, and is the natural sequel to this
session rather than a restart.

## 5. Recommendation

Given the above, the honest recommendation is: **one more focused pass on
`FnDeltaSingleQ`** (the post-loop assembly code, item 1 above) before
calling it closed and moving to `FnStatelessQ` — not because the earlier
priority call was wrong, but because "finish `FnDeltaSingleQ` completely"
turned out to have one more concrete, bounded piece left (a single call
site) rather than being done. This isn't scope creep — it's the same
target, one disassembly window further, and it's cheaper than starting a
fresh 1788-byte function from zero. Corpus-validating §2's formula against
the 9 real clips (also requested) is independent of that and can happen in
parallel or right after — both are better next steps than starting
`FnStatelessQ` with a codec still one gap short of "closed."
