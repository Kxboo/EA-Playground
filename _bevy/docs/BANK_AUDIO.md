# EA Layer 3 bank sound evidence

`audio::decode_bank_sound` supports MPEG-1 codec `0x17` bank sounds. Their bytes start at header tag `0x88` with a type byte `00`, followed by an EA Layer 3 frame. There is no streamed `SCDl` sample-count/offset preamble. Reads are bounded at the next distinct bank sound offset (shared sound offsets remain valid).

Local executable evidence, reproduced with `py _bevy/tools/re_functions.py CEALayer3Dec`:

- `Feed`, `0x802808dc..0x802808ec`, saves the supplied payload pointer, byte size and sample count.
- `Decode`, `0x80280988..0x802809ac`, initializes the first frame at pointer + 1 and stores `0x451` (1105) initial samples to discard.
- `0x802809b4..0x80280a38` advances subsequent frames by frame bytes plus one type byte.
- `0x80280a40..0x80280b9c` handles an `EE` raw PCM splice after a frame: big-endian offset/count, interleaved signed 16-bit samples, replacement beginning at granule length minus offset.
- `0x80280ba0..0x80280bdc` consumes the initial skip across granules before delivering output.

The checked local corpus has 19 embedded codec `0x17` sounds:

| Bank | Sound indices | Result |
| --- | --- | --- |
| `common sfx.abk` | 65, 66, 67, 71, 72 | MPEG-1, supported; 66, 67, 72 are stereo |
| `mg_footie.abk` | 11 | MPEG-1, supported |
| `nis.abk` | 1 | MPEG-1, supported |
| `world_sfx.abk` | 2 through 13 | MPEG-2 header `0x8c`, explicitly unsupported by current `mp3.rs` |

Tests in `audio.rs` verify two silent synthetic frames plus an exact raw splice spanning the 1105-sample skip, truncated frame/splice rejection, bad type rejection, short bank headers, and the explicit MPEG-2 diagnostic. Corpus test `abk_layer3_banks` decodes all seven supported sounds to their exact header sample counts and channels, checks each channel-granule ends at its declared Huffman length, rejects overruns/rewinds and excessive clipping, and confirms all twelve unsupported sounds report MPEG version bits 2.

Targeted verification used a standalone optimized `rustc --edition 2024 --test` harness importing the repository audio, mp3, tables and utk modules, with minimal bridge/skeleton adapters and an unused archive stub. Run the full repository checks with `cargo test bank_layer3` and `cargo test abk_layer3_banks` from `_bevy`. No original game bytes are included in this change.
