# EA Layer 3 bank sound evidence

`audio::decode_bank_sound` supports MPEG-1 and MPEG-2 codec `0x17` bank sounds. Their bytes start at header tag `0x88` with a type byte `00`, followed by an EA Layer 3 frame. There is no streamed `SCDl` sample-count/offset preamble. Reads are bounded at the next distinct bank sound offset (shared sound offsets remain valid).

Local executable evidence:

- `CEALayer3Dec::Feed`, `0x802808dc..0x802808ec`, saves the supplied payload pointer, byte size and sample count.
- `Decode`, `0x80280988..0x802809ac`, initializes the first frame at pointer + 1 and stores `0x451` (1105) initial samples to discard.
- `0x802809b4..0x80280a38` advances subsequent frames by frame bytes plus one type byte.
- `0x80280a40..0x80280b9c` handles an `EE` raw PCM splice after a frame: big-endian offset/count, interleaved signed 16-bit samples, replacement beginning at granule length minus offset.
- `0x80280ba0..0x80280bdc` consumes the initial skip across granules before delivering output.
- `CEALayer3::ProcessHeader`, `0x80282b68`, maps MPEG-2 sample-rate indices to long/short band rows 3 through 5.
- `GetSideInfo`, `0x80281a50..0x80281bf8`, reads nine-bit LSF scalefactor compression and omits MPEG-1 scfsi and the explicit preflag bit.
- `DecodeMono`, `0x80282d1c..0x80282d6c`, still reads one granule bit for LSF, then selects `GetLsfScaleFactors`.
- `GetLsfScaleData`, `0x802821c4`, partitions compression ranges below 400, 400 through 499, and 500 through 511 into four scalefactor widths, chooses executable `sNumSfbBlock` partitions and derives preflag. `GetLsfScaleFactors`, `0x80282488`, maps them to long/short/mixed arrays.
- `DecodeHuffman`, `0x8028268c`, keeps the short-block region split at 36 lines for all versions.
- `Dequantize`, `0x80284558..0x8028456c`, uses six long bands for LSF mixed blocks, eight for MPEG-1. `Reorder`, `0x80285128`, uses the version's short band row and keeps the first 36 mixed-block lines long. Hybrid IMDCT and synthesis are shared across sample rates.

Reproduce disassembly with `py _bevy/tools/re_functions.py CEALayer3Dec`, `GetSideInfo`, `GetLsfScaleData`, `GetLsfScaleFactors`, `Dequantize`, or `Reorder`.

The checked local corpus has 19 embedded codec `0x17` sounds, all supported:

| Bank | Sound indices | Format |
| --- | --- | --- |
| `common sfx.abk` | 65, 66, 67, 71, 72 | MPEG-1; 66, 67, 72 are stereo |
| `mg_footie.abk` | 11 | MPEG-1 |
| `nis.abk` | 1 | MPEG-1 |
| `world_sfx.abk` | 2 through 13 | MPEG-2 mono, 22050 Hz |

`abk_layer3_banks` decodes all 19 to exact header sample counts and channels. All channel-granules end exactly at their declared Huffman length, with zero overruns/rewinds; clipping remains below one percent. The 12 MPEG-2 sounds contain 4312 granules, including 471 short granules and zero mixed granules. Their successful complete decode also confirms no intensity-stereo frames; intensity stereo remains explicitly unsupported, as do MPEG-2.5 and reserved versions.

Run `py _bevy/tools/mp3_lsf_oracle.py` for independent executable comparisons. It compiles an isolated optimized `rustc --test` harness by default (no Cargo build), or accepts `--harness <test.exe>`. It checks:

- 1536 LSF vectors: all 512 compression values for long, short and mixed blocks. Actual original PowerPC instructions determine bit consumption, preflag and all 61 mapped scalefactors. Only the external bit-input primitive is hooked.
- 72 spectral scaling vectors and 48 reorder vectors, each with 576 spectral lines, spanning MPEG-1 and MPEG-2 band rows, long/short/mixed blocks, both scale flags and preflag values. Original code chooses every band range and permutation. Its runtime scale table is initialized with the defining powers of two, and only the elementary `ScaleSamples` vector multiplication is hooked.

These comparisons establish the LSF parser, scale mapping and spectral band behavior beyond output size/clipping. They do not claim complete PCM equality to a running console: the retained synthesis uses double precision versus the original single precision routines, and the final PCM waveform has not been independently compared sample-by-sample.

Synthetic Rust tests cover compression boundaries, invalid compression, the LSF granule bit/no-scfsi/no-preflag layout, silent long/short/mixed frames, decoder delay and exact raw PCM splices, truncated frames/splices (including one-to-four-byte splice headers after the sample target), invalid chunk types and short bank headers. Existing MPEG-1 regression still passes all 14 tracks: 409582 channel-granules and 2674 seconds, exact framing/sample counts, clipping and seam checks.

Targeted test filters: `cargo test mp3::tests`, `cargo test bank_layer3`, `cargo test abk_layer3_banks`, `cargo test abk_banks`, `cargo test all_music_decodes_exactly`. No original game bytes are included in this change.
