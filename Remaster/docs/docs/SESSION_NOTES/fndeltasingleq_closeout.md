# FnDeltaSingleQ — closed

## The bug and the fix

`this+0x10` (the pointer every flag/record read in `EvalSQT` goes through)
is NOT `marker.payload_off` (marker+0x12, the offset every other codec in
this project uses). It's computed by a dedicated 28-byte helper,
`GetArrays__DeltaSingleQ` (0x80408978), disassembled in full this session:

```
addi r6, r3, 0x10       ; MinRangePtr = map + 0x10   <-- NOT +0x12
stw  r6, 0(r4)
lbz  r0, 6(r3)           ; channel_count, u8 @ +0x06 (unchanged, still correct)
mulli r0, r0, 0xe        ; * 14 -- confirms record stride
add  r0, r6, r0
stw  r0, 0(r5)           ; bytePtr = MinRangePtr + channel_count*14
```

Every one of my earlier reads was 2 bytes into the next field over. This
also retroactively explains the very first "0x7fff sentinel" observation
from many turns back in this investigation -- that was never a sentinel,
it was channel 0's real first field, correctly located at marker+0x10,
misread because I was looking at marker+0x12.

The flag field itself: confirmed via `clrlwi. r26,r23,0x18` immediately
after every `lhz ...,0xc(...)` flag read -- it's a 16-bit field at
record+0x0C, but only the low 8 bits (the byte at +0x0D) are the real
comparand. In practice this didn't matter for THIS corpus once the table
offset was fixed -- every flag decoded to a clean 0 correctly with the
full-halfword compare too, since the high byte is 0 once you're reading
the right record.

## What's now closed (all 9 clips exercise flag==0 only)

- `MinRange` table: `marker+0x10`, 14 bytes/channel, confirmed layout
  (angle pair for bind-pose init, delta base/scale pairs, flag byte)
- Per-frame output: `Hamilton(accum[ch], buf_2c[ch])` -- no trig, ever,
  in the per-frame path; the Euler->quat builder (0x8040873c) only runs
  4 times at construction, building the two cached bind quaternions
- Accumulator's slot 3 = the quaternion `w` component directly, confirmed
  by its use in the Hamilton product
- Bone mapping: direct per-channel u8 table, same convention as
  FnDeltaQFast (session 25)

flag==1's three-buffer branch and flag>=2's other Hamilton order remain
untested against real data (none of the 9 clips exercise them), so they're
disassembly-confirmed but not corpus-validated -- flagged, not hidden.

## Corpus validation (9/9 clips)

| clip | samples | chans | norm mean | norm min | norm max | step mean | step max |
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

Norms match the quality bar set by FnDeltaQFast/F3/F1's corpus runs
(0.99-1.00 range). Step angles are mostly small with occasional larger
jumps at block boundaries, the same pattern the other three codecs show.

Sample-count cross-check against the mirrored secondary track (tag 0x12,
same 9 clips): 9/9 exact match.

## Status

244 (QFast+F3/F1) + 9 (SingleQ, rotation only -- these clips' secondary
track is itself FnDeltaQFast, already decoded) = 253/265 clips now have
a fully decoded rotation stream. Only the 12 whole-clip FnStatelessQ
containers remain.
