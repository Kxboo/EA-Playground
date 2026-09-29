# Local setup and build

## Requirements

The maintained build is Windows with PowerShell, Git, Rust/MSVC and the Windows SDK, plus Python 3.14 available through `py -3.14`. Install the Visual Studio C++ build tools before compiling Rust. The renderer needs a GPU supported by Bevy/wgpu. Headless decoder operations do not create a GPU or window.

```powershell
git clone https://github.com/Kxboo/EA-Playground.git
Set-Location EA-Playground
py -3.14 -m pip install numpy pyinstaller packaging
```

Rust dependencies are pinned in `_bevy/Cargo.lock`. Python dependencies are not yet locked; the commands above install the build requirements, not a reproducible Python environment. Capstone is optional for executable research:

```powershell
py -3.14 -m pip install --target Remaster/research/deps capstone==5.0.9
```

## Supply local inputs

Create `Remaster/reference/` and place these files from your own local research/game extraction there:

| File | Purpose |
| --- | --- |
| `playgroundz.elf` | Supplied symbol-bearing PowerPC executable used by recovered decoders and research |
| `player_skel.ske` | Player skeleton reference |
| `player_anims.anm` | Player animation bank reference |

The audited ELF has SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`. A different executable may have different symbols or layouts; do not reuse addresses without checking. These inputs and extracted DATA are ignored by Git.

Extract the game's DATA with your existing disc extraction workflow. Disc-image extraction is not implemented here. Preserve sibling files and archive layouts so shared texture-bank discovery works. In particular, keep `world.gsh` and `world-misc.gsh` beside `world.big`.

From the repository root, generate a local inventory using your actual path:

```powershell
$dataPath = 'D:\path\to\DATA'
py -3.14 Remaster/src/cli.py inventory $dataPath --deep --out Remaster/research/coverage.json
```

The inventory stores absolute paths and is deliberately untracked. `--out` refuses to replace existing files: for a rescan, write to a new report path, then deliberately copy the chosen report to `Remaster/research/coverage.json`. The worker reads this report or its packaged copy in `_bevy/research/coverage.json`. Regenerate it after relocating DATA; never use another machine's inventory unchanged.

The original research machine uses `D:\_eagl\eagl EA PLAYGROUND\extra\more\eaplayground files\DATA`. Some audit scripts and the older Tk viewer's shortcut buttons still use this default (`Remaster/src/core.py`, `DEFAULT_DATA`). The Bevy library uses source paths from the generated inventory. Adapt legacy fixture paths when running the full corpus tests elsewhere; portable configuration of every historical script is unfinished.

## Run from source

```powershell
Set-Location _bevy
cargo fetch --locked
cargo run --locked --release
```

On a fresh clone, the Rust viewer starts `py -3.14 -u tools/decoder_bridge.py`. If an `EAGL-Decoder.exe` exists beside the viewer, it takes precedence over Python source. Rebuild that worker after decoder edits when using a packaged workspace.

For direct source inspection, from the repository root:

```powershell
py -3.14 Remaster/src/cli.py inspect 'D:\path\to\DATA\world.big' --deep
'{"command":"catalog"}' | py -3.14 _bevy/tools/decoder_bridge.py
```

See [HEADLESS.md](../Remaster/HEADLESS.md) and the [native guide](../_bevy/README.md#headless-use) for extraction, decoding, animation and nested archive source syntax.

## Package Windows executables

After preparing references and coverage, from `_bevy/`:

```powershell
cargo fetch --locked
powershell -ExecutionPolicy Bypass -File build.ps1
```

The build intentionally compiles Rust offline after dependencies are fetched. It packages the worker and copies local references/catalog. Keep `EAGL-Workbench.exe`, `EAGL-Decoder.exe`, `_decoder_internal/`, `reference/`, `research/coverage.json` and the writable `assets/` directory together. `assets/generated`, caches and logs are created on demand. The viewer is not a single self-contained EXE. Local packages contain game reference data; no downloadable package is published by this source import.

Use `build.ps1 -DecoderOnly` for worker-only changes. To package the older Tk UI and standalone CLI, run `Remaster/build.ps1`; their `_internal/` and `_cli_internal/` directories must remain beside their executables.

## Verify

From `Remaster/`, with the required local corpus/fixtures present:

```powershell
py -3.14 -m unittest discover -s tests -v
py -3.14 tests/audit_models.py
py -3.14 tests/audit_materials.py
py -3.14 tests/packaged_cli.py
```

From `_bevy/`, after packaging:

```powershell
py -3.14 tools/verify_decoders.py
py -3.14 tools/verify_packaged.py
py -3.14 tools/verify_material_viewer.py
```

Renderer checks launch the viewer and save screenshots/scene diagnostics. They require a desktop/GPU. The recorded reports are results from the original research workstation; a clone without private fixtures cannot reproduce the complete audit. See [handoff](HANDOFF.md) for expected counts and interpretation.
