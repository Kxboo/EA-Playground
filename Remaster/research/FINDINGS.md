# EA Playground decoding findings — 2026-09-29

This is an incomplete reverse-engineering project. The current workbench makes uncertainty inspectable rather than treating a successful export as proof of a complete decode. Original DATA, OLD ATTEMPTS and the game folder were not modified.

## Corpus and coverage

`coverage.json` is the current deep inventory: 1,210 loose DATA files, 5,267 records after recursive BIG/VIV/U8 expansion, 3,685 distinct contents and 47 extensions. Identical containers are expanded once. Every record includes its original or virtual path, extension, signature, hash, format and status. `coverage.md` lists **all** observed extensions, including unsupported ones. Unknown compression/container types may hide additional files.

Status meanings: `decoded_text` means text or cells were read, not that every field's game meaning is established; `structural` means a bounded container/record layout was parsed; `partial` means a payload decoder exists with limitations; `recognized` means a known signature only; `unknown` remains unsolved. Payload checks carry separate status and caveats.

## Confirmed corrections

### GSH texture block lengths and palettes

- SHPG image-record bytes +1 through +3 form a **big-endian 24-bit block length**, including the 16-byte header. The old parser read only +2/+3 and called +1 a LOD flag. That interpretation was wrong.
- `boot/strapwarn_standard_english.gsh`: record at 0x30 begins `19 04 b0 20 02 80 01 e0`. It is PAL8, block length 0x4b020, 640×480. Its palette starts at 0x4b050. This resolves the name `strapA_screen` and produces a readable Wii safety screen.
- Palette 0x33 consists of AR byte pairs followed by GB byte pairs, with each table start aligned to 32 bytes. Its nonzero flag bytes do not invalidate the palette. The prior decoder supported only 0x31 RGB565 and 0x32 RGB5A3.
- GX images require complete storage tiles even for narrow dimensions. A 4×25 PAL8 image uses 224 bytes (8×4 tiles), not 100. Cropping the buffer to width×height silently destroyed frontend images.
- Record 0x14 is RGB565. Added it using the existing GX 4×4 tile decoder.
- Removed the now-disproven LOD/stub recovery and orphan-guessing paths. All tested images come from the declared directory. Missing palettes, truncated levels and out-of-range palette indices now raise errors instead of producing placeholder pixels.

All **3,574 images in 736 distinct GSH files** decode with correct pixel counts and valid palette indices. The initial broad scan had 1,836 image errors. The current scan also checks 199 distinct TPL files, supporting all observed GX base-image formats. `texture-contact.png` contains representative frontend outputs with checkerboard transparency. Menu buttons/icons and the safety screen were visually inspected. This does not establish visual accuracy for every texture. Mip counts are still inferred from available tile storage, only base images are exported, and sampler flags remain incompletely understood.

Format identification was cross-checked against [EA Graphics Manager's palette mapping](https://github.com/bartlomiejduda/EA-Graphics-Manager/blob/main/src/EA_Image/common.py), its [image-format mapping](https://github.com/bartlomiejduda/EA-Graphics-Manager/blob/main/src/EA_Image/ea_image_decoder.py), and [ReverseBox's split-palette interpretation](https://github.com/bartlomiejduda/ReverseBox/blob/master/reversebox/image/image_decoder.py). The local implementation was changed directly; no external decoder dependency was added.

### Skeletons and animation

- Bone-table position is relative to the skeleton symbol (`symbol.value + 16`), not globally fixed at 0x450. The player happens to use 0x450; the RC car uses 0x30. Parent indices and cycles are validated.
- The old RC car pair decodes two bones and its QFast/F3 clip with 33 samples after recognizing its 0x1414 codec family. Its model now decodes too: five meshes, 980 triangles, vertex colours and five texture references, with the two-bone skin retained in GLB.
- Clip names resolve through relocated `table_b` name pointers alongside `table_a` blocks. All 265 player names and the RC name match those tables; a regex string-order guess is no longer used.
- Scale channels were decoded but omitted during export. Both skeleton animation and mesh-plus-animation GLBs now retain them; preview applies scale. `S_sk_bag_reach` is a regression fixture.
- Player clips have finite samples, valid bone targets and matching channel counts. Exported quaternions are normalized; raw sample JSON remains available. **30 fps is still assumed.** Sparse stateless time tables are rejected, and undecoded static extra channels are reported. Player translation masking still depends on the recovered USA executable.

All 265 player clips and the RC fixture pass structural/export checks. That does not prove game-exact poses, timing, masks or all animation variants.

### Models

All `.o` files expose ELF32 sections, symbols and relocations. EA uses little-endian container fields around big-endian asset payloads; its machine field is not evidence that Wii assets contain MIPS instructions.

Among 836 distinct `.o` contents, **828 now produce partial geometry and eight declare empty draw lists**, with no remaining unsupported/invalid models in this corpus. Previously there were 204 geometry successes and 632 failures. This is corpus coverage, not proof that every field or rendering behavior is understood. The established legacy layouts still contain heuristics; non-finite coordinates and invalid face indices block export.

`src/shadow_models.py` decodes all 49 distinct `PlaygroundShadow` objects through ELF shader relocations. Relative to the shader anchor, +0x18 gives position count and +0x1c relocates the BE float XYZ array. The previous description of PCode as a leading mode followed by format pairs was incomplete: the ELF establishes that 8/9 are independent INDEX8/INDEX16 opcodes, each followed by a GX attribute ID. The parser now uses the shared PCode decoder. Generated inspection normals and plain materials do not recreate shadow-volume or alpha behavior.

Eight established sample models, including Alicia, Timothy and world-low-all, pass export buffer/index/finite-value checks. Model descriptor coverage, skinning and materials remain incomplete; successful geometry is classified partial.

### ELF-confirmed frontend and hardware skinning pass

The supplied `reference/playgroundz.elf` has SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`. `research/elf_trace.py` provides read-only symbol-aware disassembly and shader-field extraction. `shader-schemas.json` records 25 named shader schemas with element sizes and count/pointer offsets. Raw instruction words are retained; Gekko paired-single instructions unsupported by the generic disassembler are explicitly left undecoded.

- **PCode:** `ProcessPCode` at `0x803ef24c` establishes opcode 7 as relocated display-list pointer plus BE byte length; 8/9/10 as GX INDEX8/INDEX16/DIRECT with one attribute operand; 11 as position fractional bits. Opcode 5 skips three-byte allocation entries terminated by zero count. The shared bounded parser rejects unknown operations and truncation. See `pcode-disassembly.txt` and `src/pcode.py`.
- **Frontend positions:** `ModelRenderTextureApt` at `0x8001d0bc` sets S16 XYZ with 15 fractional bits. `Model::Draw` at `0x803e2c78` passes model fields +0x4c/+0x5c to `ModelSetScale` at `0x803e68c4`. Thus positions are `packed / 32768 * scale + center`. The model's +0x9c group count and relocated +0xcc draw list identify its actual shader primitives; +0xc8 is not the group count. TextureApt coordinates are at shader count/pointer +56/+60; GouraudApt uses +40/+44.
- **Frontend UVs/materials:** TextureApt's local matrix at +48/+52 transforms positions into pixel UVs. `bindTexture` at `0x80151288` divides by the SHAPE width/height read at +4/+6, with matrix multiplication at `0x801513a8`. Export normalizes only against a resolved texture, preserves raw pixel UVs, applies the stored diffuse RGBA and marks APT materials unlit. Space-padded GSH identifiers now resolve correctly. The independent HelpEditor fixture has declared bounds 0..16, a 30×30 texture, pixel UVs approximately 0..30 and normalized UVs approximately 0..1.
- **Frontend coverage:** 619 distinct files, **2,361 named shapes and 2,511 meshes**, including the pure GouraudApt NunchuckRequired variant. Every frontend texture reference resolves against sibling GSH banks (2,475 embedded images across per-file exports). Every decoded position passes an independent check against its named model's stored bounds. Export flips authoring Y-down coordinates for the Bevy view. The viewer selects individual shapes; full APT timeline placement, dynamic colours, masking and script behavior remain undecoded.
- **Hardware-skinned props/shadows:** `src/hwskin_models.py` reads four named families: PlaygroundTexture_HWSkin, PlaygroundLitPlaceable_HWSkin, PlaygroundCompressShadow_HWSkin and PlaygroundCompressPlaceableShadow_HWSkin. This resolves the five formerly failing assets and replaces heuristic decoding for swingset.o. The RC car and swingset use Q13, while the teeter-totter and character shadow use Q14, as explicitly set per primitive. Lit normals are S16 Q14; colour indices address RGBA8; UVs are BE float pairs. All six files match stored bounds and declared primitive counts. The teeter-totter's old non-finite output came from the wrong field layout.
- **Skin palette:** `SkinGenerateMatrices` at `0x803f39f8` reads three float words and extracts their low bytes as 64-byte bone-matrix indices (`rlwinm` at +0x24/+0x28/+0x2c). `SkinGeneratePosMatrices` at `0x803f3ca4` advances weight records by 16 bytes. The GX direct position-matrix selector is three times the palette entry; lit placeables also carry a texture-matrix selector at attribute 2, offset by 30. Vertex deduplication includes palette/color indices. GLB retains vertex colours, weights and joints. Winding reversal agrees with stored normals on over 99% of the lit prop faces. Game sphere-map/shadow compositing is still not reproduced.
- **Empty models:** eight files have zero declared primitives, including rc_track_car_shadow.o with two empty groups. They retain named model nodes and bounds in structural reports. No triangles are invented.

`model-verification.json` contains the full 828-export/8-empty audit, texture diagnostics and bounds checks. `frontend-verification.json` retains the frontend subset. The 20 regression tests include mixed-width PCode, truncated programs, out-of-bounds GX indices, pixel-to-normalized UV conversion without mutation, Gouraud geometry, zero-offset weight tables, RC skin/colour export and empty draw lists. Existing 265-player-clip and eight-model checks still pass.

### Material warning and viewing corrections

The prior full export report contained **80 models with material lookup warnings**. All **828 renderable models now resolve their diffuse bindings with zero texture lookup or material-ownership warnings** (3,480 per-model texture bindings). The eight structurally empty models request no materials. This does not establish exact Wii TEV, shadow, sphere-map/specular, alpha-state or filter behavior.

- `src/material_bindings.py` uses the 25 executable-confirmed shader schemas to read the primary Texture pointer. It no longer assigns every TAR relocation before the next guessed descriptor to that mesh. The net fixture previously acquired both `metal_yellow` and `net` from trailing model metadata; the exporter alphabetically chose metal for the net. It now assigns metal to mesh 0 and the alpha-textured net to mesh 1. Secondary texture-stage references are preserved in the preview report, not silently treated as diffuse.
- Some legacy descriptor offsets precede the actual PCode. Successful parses now retain their source position, attribute, UV and display-list pointers. Exact position/UV relocation identity recovers material ownership for those meshes; the display-list pointer disambiguates shared arrays. No nearest-material heuristic is used. Empty rejected legacy scan candidates do not request materials.
- `src/asset_links.py` follows the containing archive scopes and loose files beside the outer archive. `world.big` has no internal GSH banks, but its directory contains the required `world.gsh` and `world-misc.gsh`. The viewer and headless decoded exports use these dependencies automatically. Unrelated global banks are not searched.
- Duplicate full names are compared by decoded width, height and SHA-256 of RGBA pixels. Identical duplicates (net, darts, RC wheels/chrome/shocks/spoilers and shared controller textures) resolve safely. Different images remain ambiguous and are not arbitrarily selected. Exact full-name matches precede case-insensitive matches; short aliases cannot override a conflicting known full name. Synthetic tests cover both genuine ambiguity and false prefix matches.
- `RuntimeAllocTARConstructor` (`0x803e8c48`) branches on property 22 at `0x803e91b0`, writes S wrap at `0x803e945c`, and writes T wrap for property 23 at `0x803e9528`. Calls to GXInitTexObjWrapMode establish their hardware meaning. Each non-power-of-two image axis is forced to clamp, matching the checks immediately before these writes. GLB now emits per-binding clamp/repeat/mirrored-repeat samplers, sharing image bytes while keeping different sampler/material instances distinct. Filter properties are not guessed.
- The common PlaygroundTexture / PlaygroundTextureBakedLight families have **RGBA8 Colours, not normals**. The old generic decoder read these bytes as signed normals and discarded the colours. Exports now retain indexed colours in COLOR_0, preserve colour seams, and generate inspection normals from geometry. The vertex-colour materials are unlit to avoid applying extra Bevy lighting over their baked colour values. `ModelRenderPlaygroundTexture` (`0x80016dcc`) sets RGBA8 at `0x80016e88` and disables channel lighting at `0x80017044`/`0x80017064`; the BakedLight variant does likewise at `0x8001932c`/`0x8001934c`. Hardware-skinned PlaygroundTexture colours are retained as well. See `vertex-color-shaders-disassembly.txt`.
- The Bevy viewer previously overwrote imported unlit flags. It now remembers authored lighting per material and restores it after the user disables the global Unlit toggle. Frontend and vertex-colour materials keep their intended baseline lighting. Partial diffuse opacity also takes precedence over texture-only alpha classification for APT materials.
- PlaygroundToonShade props also used the generic signed-byte normal interpretation. `ModelRenderPlaygroundToonShade` explicitly sets S16 Q14 normals at `0x8001ba2c`. These normals now decode from the shader's declared count/pointer at +40/+44, with bounds checks, improving prop lighting without inventing a toon/specular reconstruction.

Evidence: `material-state-disassembly.txt`, `material-shaders-disassembly.txt`, `material-verification.json` and the refreshed `model-verification.json`. The headless preview includes `material_report`, `material_bindings`, `material_warnings` and `texture_banks`; each resolved image records its source entry and pixel hash. Real game shader effects still require further reconstruction, and shadow-volume previews retain that caveat rather than reporting a nonexistent missing texture.

### Other parsed formats

- CSV: comma/tab/semicolon detection, quoted multiline cells, Unicode BOMs, raw string preservation and ragged rows. All 50 observed CSV records parse. `home.csv` is tab-delimited despite its extension. Known animation tables expose asset references; unknown columns retain their original values.
- BIGF/BIG4/VIV: bounded directory and RefPack reads; `.bh` is an external directory whose offsets refer to a companion archive, not its own bytes. Nintendo `.arc` U8 directories recurse too.
- `.loc`: LOCH/LOCL headers, offset table and UTF-16LE strings. `.idx`: observed big-endian key/index pairs. Original identifier strings/hash algorithm remain unknown.
- `.lef`: LION tags, braces and raw properties, with unparsed lines retained; no particle simulation.
- `.hkx`: Havok v4 sections and fixup triples/pairs, no collision/rigid-body reconstruction.
- `.brlyt`, `.brlan`, `.brfnt`: Nintendo resource blocks and layout texture/font references; pane/material/glyph/keyframe semantics are not yet decoded. Structure checked against [NW4R resource definitions](https://github.com/doldecomp/ogws/blob/master/include/nw4r/lyt/lyt_resources.h).
- `.dol`: executable loadable section ranges, entrypoint and BSS; no code decompilation. Header checked against [Dolphin's DOL definition](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/Core/Boot/DolReader.h).

## Next decoding priorities

1. Recover shadow compositing, material/sampler state and remaining heuristic model fields; compare decoded assets and animation poses with the running game.
2. Recover sparse animation timing, static channels and actual frame-rate fields; compare exported poses against the game.
3. Reconstruct APT/CONST timelines/shapes/script records and connect frontend textures to them.
4. Decode Havok object graphs and EA VLT records; investigate `.con`, `.mkr`, `.cpt`, `.atd`, `.gsm` and remaining binary sidecars by signature and references.
5. Audio banks/streams, video, font glyphs and banner containers have only recognition or partial structure. They do not yet export usable audio/video/font assets.

No format is considered fully understood merely because its extension is recognized. The headless inspector, hex dumps, strings, raw samples and coverage catalog are the baseline for continuing this work.
