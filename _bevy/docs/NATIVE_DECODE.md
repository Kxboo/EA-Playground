# Native corpus decode

`EAGL-Workbench.exe --decode-all <new dir> [--data <DATA>] [--only ext,ext]` decodes the whole game corpus in Rust, with no Python worker. It walks loose files and expands BIG/VIV/U8 archives recursively. Contents are deduplicated by (SHA-256, extension). For each input it writes derived outputs, plus `report.json` and `REPORT.md`. The full run takes about 85 s on the reference machine.

## Result (2026-09-30)

5,267 records, 3,685 distinct contents. Every distinct content decodes except 12 third-party keyboard dictionaries, which stay **partial** by design (see below). There are no failures and no unsupported formats.

| Area | Formats | Output |
|---|---|---|
| Containers | BIG/VIV (656 + 118), U8 `.arc` (10), `.bh` external directory | entries expanded |
| Models | `.o` (836), `.ske` (4), `.anm` (4 banks, 268 clips) | GLB (embedded PNG, unlit), rigs with animations |
| Textures/fonts | GSH (736), TPL (199), EA FntG `.gfn` (15), NW4R `.brfnt` | PNG + JSON |
| UI | APT programs (328) with ActionScript bytecode, `.const` (131), NW4R `.brlyt`/`.brlan`, LION `.lef` (222) | JSON / listings |
| Data | Havok 4.6 `.hkx` (61, all objects), VLT, locale/idx, conversations, markers, checkpoints, Conga gestures, Csis registries, CSV/INI/TXT | JSON |
| Audio | SCHl `.asf/.ast/.dat`, banks `.bnk`, AEMS `.abk` (21, PPC code listed with fixups), SPCH `.hdr/.evt`, NW4R `.brsar`, Wii Remote `.bwav`, banner BNS | WAV + JSON |
| Video | EA VP6 `.vp6` (8): every frame, interleaved audio | PNG frames, WAV, per-frame JSON |
| System | Wii header/boot/bi2/fst/apploader/ticket/tmd/h3/certs, DOL, IMET banner (IMD5 + LZ77), ELF (symbols + full disassembly) | JSON / `.s` |

## Findings from this pass

- **PowerPC disassembler** (`ppc.rs`): checked against Capstone over the whole ELF. 1,041,575 instructions agree and 0 disagree. Capstone does not decode the 12,474 paired-single instructions or the `fcmpo` forms; `tools/ppc_oracle.py` reproduces the comparison. AEMS module code also disassembles with 0 disagreements.
- **VP6** (`vp6.rs`, tables from `tools/extract_vp6_tables.py`): the linked On2 decoder uses two bool routines. `VP6_DecodeBool` renormalises lazily, while `VP6_DecodeBool128` always shifts once and is used for literals, signs and 4MV types. The movies use Huffman coefficient mode, with trees built from the bool probabilities (`VP6_BuildHuffTree`), and the simple profile (bilinear MC, no loop filter). The picture is coded bottom-up. `VP6_CoeffToBand[0]` is −1, so band tables start at entry 0. `VP6_ModeVq` is `[ctx][vector][mode][same, weight]`. Both partitions are consumed exactly on every frame. The trailers run 2,812 frames at 29.97 fps (NTSC) and 2,350 at 25 fps (PAL); the logos run 75 and 65 frames.
- **GSTR audio headers**: `SNDI_patchtohdrgen` (0x80279cbc) presets 1 channel, 48000 Hz and codec 0x0a (EA-XA R2) before reading tags. The `PT` path (`SNDI_patchtohdr` 0x8027aaf4) presets 24000 Hz, mono, codec 7. Movie audio omits tag 0xA0, so it is EA-XA. Decoded lengths match the video (trailer 93.827 s vs 93.825 s; logo sample count equals tag 0x85, 79,599).
- **APT**: characters are `{type, 0x09876543}` with the body at +8. The movie body holds frames, characters, size, frame time, imports (16 bytes each) and exports. Unaligned immediates are big-endian. See `apt.rs`.
- **Animation corrections** (`anim.rs` header): the corrections cover:
  - signed F3 stateless samples;
  - sparse times from the reloc at +0x08, with linear resampling;
  - static extra channels for Q and F3;
  - compound container 0x0f, with children applied lowest index last;
  - F1 channels writing any pose word.

  268 clips decode. 246 match the Python reference, 20 differ with executable evidence, and 2 are newly decoded.
- **Havok**: enum storage size comes from the member flags. Files carrying `__types__` use their own hkClass objects.
- **GSH palettes**: palette record 0x30 is IA8. 0x14 = RGB565 and 0x15 = RGB5A3, per `TARExtension::InitFromShape`.
- **NW4R**: `.bwav` files are raw speaker PCM (s16be, 6000 Hz). RSAR group waves are raw sample data referenced by the RBNK WAVE block.

## Deliberately partial

`keyboard/*.atd` (3, JustSystems ATOK) and `zi.arc::*.zsd` (9, Zi eZiText system lexicons) keep only headers and alphabets. Every `textinput::tistring::WithAtok`/`WithZi` function in the ELF is a 4–20-byte stub, and no code references these files. The engines are not linked, so the executable has no evidence for their bodies. The Zi lexicon is a byte-oriented compressed trie (about 7.1 bits/byte); decoding it without the engine would be guesswork. The `.znd` word lists decode fully.

## Reproduce

```powershell
cargo build --offline --release
.\target\release\EAGL-Workbench.exe --decode-all D:\_eagl\exports\decode-all-new
cargo test --offline --release   # 106 tests
py -3.14 tools/ppc_oracle.py     # needs Capstone and the private ELF
```

Generated tables (`vp6_tables.rs`, `sjis_table.bin`) are regenerated from the pinned ELF by the scripts in `tools/`.
