# EAGL_ANM_Pipeline

A complete, reproducible decoder + exporter for EA Playground (EA Sports
Wii/GameCube-era) EAGL `.anm` animation banks, targeting `player_anims.anm`
/ `player_skel.ske`. Every codec is implemented from disassembly of
`playgroundz.elf` and corpus-validated (quaternion norms, bone-index
ranges, cross-track continuity) rather than heuristically guessed.

**265/265 clips export successfully**, across all 4 codec-pair
combinations the file actually uses. See `PIPELINE.md` for the full
architecture and `docs/CODEC_NOTES.md` for per-codec technical detail.

## Quickstart

```bash
# 1. (optional) create a virtual environment -- the pipeline itself has
#    zero external dependencies, so this is only useful for isolation.
python3 -m venv venv
source venv/bin/activate   # Windows: venv\Scripts\activate

# 2. install dependencies (only needed if you want the dev disassembly
#    tool in docs/dev_tools/ -- everything else is stdlib-only)
pip install -r requirements.txt

# 3. run the validators (each prints a corpus-wide table + pass/fail)
python3 validation/validate_qfast.py
python3 validation/validate_singleq.py
python3 validation/validate_statelessq.py
python3 validation/validate_f3.py
python3 validation/validate_f1.py
python3 validation/validate_stateless_f3.py
python3 validation/bone_mapping.py
python3 validation/regression.py        # full 265-clip pass, all codecs

# 4. export every clip to glTF binary (.glb)
python3 exporter/anm_exporter.py
# writes to output/anm_export/*.glb + output/anm_export/_export_report.txt

# 5. open a few .glb files in Blender (File > Import > glTF 2.0) to
#    visually confirm the animation plays back correctly on the skeleton.
```

## Directory layout

```
EAGL_ANM_Pipeline/
├── README.md              <- you are here
├── PIPELINE.md             full architecture + dispatch diagram
├── requirements.txt
│
├── data/                   input files (.anm, .ske, .elf, reference .csv)
├── decoder/                one module per codec + shared ELF/bank parsing
├── exporter/                codec dispatch + glTF/GLB assembly
├── validation/              corpus-wide validators + full regression
├── docs/                    codec/file-format/bone-mapping reference,
│                            session-by-session derivation history,
│                            and the (optional) disassembly dev-tool
└── output/                  generated on first run: exported .glb files
                             + JSON validation dumps
```

## What "reproducible" means here

Every script resolves its data-file paths through `decoder/paths.py`
(relative to this repo's `data/` folder, overridable via environment
variables), not hardcoded absolute paths. Running the quickstart commands
above from a freshly-extracted copy of this repository, in a brand-new
Python environment, should reproduce the same 265/265 export with no
manual path-fixing or dependency chasing. If it doesn't, that's a bug in
the packaging, not an expected manual step -- please treat it as such.

## Provenance

This pipeline is the result of an iterative reverse-engineering process
(disassembly → hypothesis → corpus validation → correction, repeated per
codec). `docs/SESSION_NOTES/` preserves that history, including at least
one documented case where an earlier conclusion was later found wrong and
corrected (`FnStatelessQ`'s lerp behavior) and one case where a bug shipped
in an early version of the exporter and was caught by corpus validation
(`FnDeltaF3`/`FnDeltaF1`'s bone-index formula). Reading the session notes
is optional; `PIPELINE.md` and `docs/CODEC_NOTES.md` are the distilled,
current-truth reference.
