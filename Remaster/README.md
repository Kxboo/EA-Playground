# EAGL Remaster — EA Playground research workbench

This build resumes **parsing and decoding research**. It does not claim the formats are finished. Shared decoders and the original desktop tools live here; the new native Bevy viewer and future game workspace are in `D:\_eagl\_bevy`. The original game, DATA and OLD ATTEMPTS remain untouched.

## Start here

For live textured models and skinned model-plus-animation playback, open **`D:\_eagl\_bevy\EAGL-Workbench.exe`**. Its packaged worker uses these same decoders, including ELF-confirmed frontend shapes and hardware-skinned props/shadows. See `../_bevy/README.md` for the native viewer and headless interface.

- **EAGL-CLI.exe** — headless inspection, inventories, decoding, extraction, hex dumps and strings. Read **HEADLESS.md** for copyable commands. Keep `_cli_internal` and `reference` beside it.
- **EAGL-Remaster.exe** — simple desktop browser, structural inspector, image/skeleton/wireframe previews and exports. Keep `_internal` and `reference` beside it.
- **research/coverage.md** — all 47 observed extensions and their current status. **research/coverage.json** contains paths, signatures, hashes and detailed payload diagnostics.
- **research/FINDINGS.md** — confirmed corrections, evidence, unresolved assumptions and next decoding priorities.

Both executables are portable Windows applications; the built versions need no Python installation. Keep their supporting folders. `src` contains editable source and `build.ps1` rebuilds both.

## This decoding pass

The DATA scan covers 1,210 loose files and 5,267 records after recursive BIG/VIV/U8 expansion (3,685 distinct contents). Identical archives expand once. Unknown formats stay visible; signature recognition, structural parsing and payload decoding have different report statuses.

- Fixed GSH 24-bit record lengths, GX tile padding, RGB565 images and split AR/GB palettes. All 3,574 images across 736 distinct GSH files pass byte-count and palette-index checks. Representative frontend buttons/icons and the safety screen were inspected visually. TPL base-image decoding is also available.
- Fixed skeleton offsets and the RC car animation codec family. All 265 player clips and the RC fixture pass structural/export checks. Clip names now resolve through relocated pointers. Scale channels survive export and preview.
- Every `.o` has an ELF section/symbol/relocation inspector. All 836 distinct files are accounted for: **828 produce partial geometry and eight declare empty draw lists**, with zero model decoder failures in the current corpus. The 619 frontend files expose 2,361 named shapes with resolved textures. RC car, teeter-totter and compressed shadow families now decode from named shader fields and PCode vertex descriptors. Invalid indices and non-finite coordinates block export. Game shader semantics remain incomplete.
- Material lookup warnings are resolved across all 828 renderable models. Shader-field ownership fixes wrong/missing mesh assignments; containment-based texture discovery finds shared world banks; identical image duplicates no longer create false ambiguity. World vertex colours are preserved instead of being interpreted as normals. TAR wrap modes are exported per material. See `research/material-verification.json` for texture identities and provenance.
- Added CSV/TSV quoted-cell parsing, localization strings and indices, LION effect trees, Havok sections/fixups, Nintendo resource blocks, DOL sections and U8 archives.

These results do **not** establish game-exact animation, complete model layouts, every texture's visual correctness, or full semantics of each field. Audio/video, APT UI programs, databases and several binary sidecars remain recognized or unknown. See the findings for specific limitations.

## Desktop use

1. Click **DATA** or **Old attempts**, filter by filename and double-click. All file types are listed. CSV and other structured formats show inspection details; unsupported model layouts fall back to their structure and diagnostics.
2. Open `.big`/`.viv`, select an entry and choose **Open entry** or extract. RefPack decoding is built in. Opening an entry extracts siblings to a new cache folder so models can find related textures.
3. Open `.o` for a wireframe view and OBJ/GLB export. GLB can embed matching sibling GSH textures. **Texture files…** adds shared texture banks; **Load skeleton…** enables character skin export. The preview is wireframe; animation preview displays bones.
4. Open `.gsh` to preview/export PNGs. Open `.anm` with its matching skeleton for playback, scrubbing and GLB export. **Reference animations** points at the recovered player bank. **Clip + model…** combines a selected clip and character model.
5. **Inspect format** provides the same deep report as the CLI for the current file. The CLI handles U8 extraction and TPL decoding; the UI shows their structures.

Exports use new numbered folders under `exports`; source assets and existing exports are not overwritten. Workers keep parsing off the Tk thread. Cancellation occurs between entries. Check **Details / warnings** and **Activity log** for incomplete/failed decodes. Settings and logs stay beside the executable.

**Existing tools** opens the previously installed SSX MultiTool and EA Graphics Manager from their original folders. Their applications are not bundled; BIG/RefPack extraction and the recovered decoders run inside Remaster. Disc-image extraction, archive repacking and game patching are not implemented.

## Evidence and development

Recovered source came from the latest `eagl_anm_adapter` and reference assets from `EAGL_ANM_Pipeline`. `docs/provenance.json` retains original copied-file hashes. `src/legacy` now includes documented corrections; historical notes in `docs/docs` may contain superseded claims. `docs/current-source-hashes.json` identifies this build's source.

Verification: `docs/verification.json` (394 compressed archive entries, all player clips, eight selected models, 229 selected textures); `research/coverage.json` (full inventory/payload checks); `docs/cli-verification.json` (packaged headless executable); `docs/ui-verification.json` (Tk flows); `ui-smoke-test.json` (packaged GUI startup). Successful export checks do not prove complete reverse engineering.

```powershell
py -3.14 -m unittest discover -s tests -v
py -3.14 tests/audit_corpus.py
py -3.14 tests/audit_models.py
py -3.14 tests/audit_materials.py
py -3.14 tests/ui_flow.py
py -3.14 tests/packaged_cli.py
powershell -ExecutionPolicy Bypass -File build.ps1
```

Source requires Python with Tk for the GUI; NumPy is optional for source texture decoding and bundled in the executables. The build uses locally installed PyInstaller and, if needed, pip's vendored `packaging`. No cloud service is used for decoding. Game/reference data stays in this local workspace.
