# Headless format research

Run `EAGL-CLI.exe` from PowerShell. It has no GUI dependency and returns JSON on stdout, progress/errors on stderr. Keep `_cli_internal` and `reference` beside it. Source equivalent: `py -3.14 src/cli.py`.

```powershell
Set-Location D:\_eagl\Remaster
$data = 'D:\_eagl\eagl EA PLAYGROUND\extra\more\eaplayground files\DATA'

# Whole corpus, including nested BIG/VIV/U8 files and unknown extensions
.\EAGL-CLI.exe inventory $data --deep --out research\my-scan.json

# Resolve a nested archive member without extracting its siblings
.\EAGL-CLI.exe inspect "$data\files\data\fe\main.big::Main.gsh" --deep
.\EAGL-CLI.exe decode "$data\files\data\fe\main.big::Main.gsh" --out exports\main-textures

# Exact file structure, symbols, relocations and decoder diagnostics
.\EAGL-CLI.exe inspect reference\player_skel.ske --deep
.\EAGL-CLI.exe decode reference\player_anims.anm --skeleton reference\player_skel.ske --clip 0 --out exports\clip-000

# Binary investigation, offsets accept decimal or 0x-prefixed hex
.\EAGL-CLI.exe hexdump reference\player_skel.ske --offset 0 --length 0x100
.\EAGL-CLI.exe strings reference\player_anims.anm --minimum 8
```

Commands:

| Command | Result |
|---|---|
| `inspect FILE [--deep] [--out report.json]` | Format metadata. ELF includes symbols/relocations; CSV includes all rows; deep runs the applicable payload decoder. |
| `inventory PATH [--deep] --out report.json` | Every loose file and recursively discovered supported archive member, hashes, format status, failures, plus a Markdown coverage table. |
| `validate PATH --deep --out report.json` | Same scan; exit 2 if supported payload decoding/structure checks fail. Unknown formats remain explicitly unknown. This is not a completeness or visual-fidelity test. |
| `decode FILE --out DIR` | Models: OBJ/GLB; images: PNG; skeletons: GLB; animation: raw sample JSON and GLBs (requires `--skeleton`; optional zero-based `--clip`). Other structured formats: JSON. |
| `extract ARCHIVE --out DIR` | BIGF/BIG4/VIV or Nintendo U8 files, with RefPack decompression. No disc-image extraction. |
| `hexdump FILE --offset N --length N` | Bounded bytes with hex and ASCII. |
| `strings FILE --minimum N` | ASCII strings and byte offsets. Use `inspect` for UTF-16 localization. |

Use `::` between nested members; member names are case-sensitive. Offsets always refer to **decompressed** member bytes. Scans expand identical archive content once; `members_expanded_at` links duplicate instances. Limits are eight nesting levels and 2 GiB total expanded bytes. Unsupported container types are not recursively unpacked.

Reports and exports refuse existing paths. Image batches preserve per-image failures in `decoded.json`; inspect that report, since completing a batch does not mean every image decoded. CLI exit 0 means the command completed, 1 means an operation failed, and `validate` additionally uses 2 for recorded validation failures. Always supply `--deep` when checking payloads. Animation inventory indexes structures; `decode --skeleton` exercises sample decoding.

Model decoding from a virtual path materializes that member alone in `cache/research`; OBJ/GLB exports from this command contain geometry without automatically resolving sibling textures. The desktop model export can select texture banks. Animation raw JSON preserves codec sample values and describes remaining assumptions; GLB rotations are normalized for glTF.

## Current results

Start with `research/coverage.md`, `research/coverage.json` and `research/FINDINGS.md`. Run `py -3.14 -m unittest discover -s tests -v` for defect regressions; `tests/audit_corpus.py` checks the player bank and selected models. `tests/recheck_textures.py` checks every unique GSH recorded by the initial corpus scan. All tests read the original assets and write only under Remaster.
