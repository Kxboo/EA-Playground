# EA Playground — Bevy Asset Workbench

Open **EAGL-Workbench.exe** in this folder. This is a native Bevy viewer using the recovered EA Playground decoders. No browser, server, Python installation or Internet connection is needed by the packaged application. Keep `EAGL-Decoder.exe`, `_decoder_internal`, `reference`, `research` and `assets` beside it. Original game files remain at their existing DATA paths.

## Menu, game and proof

`EAGL-Workbench.exe` now opens a menu:

| Entry | What it does |
| --- | --- |
| **Asset Viewer** | The workbench below (use **< Menu** to return). |
| **Play — reconstructed game** | Boots from the original files: decoded Wii strap-warning screen, then every layer in `worldfilelist.csv` (5 areas x 4 variants), Alicia with the decoded idle/walk/run clips, and locomotion from `LocalCharacterControl::Update`. WASD/arrows move, Q/E or right-drag orbit, wheel zoom, R swings the camera behind, Esc returns. |
| **Proof of decode** | Shows `docs/proof-report.json`; **Run verification** regenerates it with `tools/prove.py` (needs Python 3.14 and the game files). |

What is and is not original in the game slice is printed in its HUD and in `docs/RECONSTRUCTION.md`. Locomotion and timing constants are generated from the ELF (`tools/extract_constants.py` -> `src/recovered.rs`). The game uses the recovered variable frame policy (integer milliseconds, cap 60 ms) for movement, clouds and animation. Terrain collision, gravity and spawn data are decoded; the character solver and camera framing remain provisional. The original playground jump command is inert, so the slice adds no jump impulse.

Command-line switches (all optional):

```powershell
EAGL-Workbench.exe --mode menu|viewer|game|proof   # start in a mode
EAGL-Workbench.exe --selftest docs\selftest        # boot the game, scripted walk, write report + screenshots, exit
EAGL-Workbench.exe --flow-test --mode menu         # menu>game>menu>viewer>menu>game transition test, writes docs/flow-test.json
EAGL-Workbench.exe --mode proof --shot out.png     # screenshot any mode and exit
$env:EAGL_DATA='D:\path\to\DATA'                  # override the DATA location
py -3.14 tools/prove.py [--with-tests] [--run-selftest]
py -3.14 tools/re_functions.py Update__21LocalCharacterControl   # annotated PowerPC disassembly with resolved constants
```

The self-test and flow test open a window for up to a minute; leave it open.

## Using the viewer

Search the library and select an asset. Models load on demand with resolved GSH textures and per-primitive materials. Images display over a checkerboard; GSH/TPL banks expose individual entries. Other formats show their parsed structure. The catalog includes files inside BIG/VIV/U8 archives; nested members are read directly.

Frontend `.o` files now expose their named shapes in the **Shapes** list. They open facing the camera with unlit materials, decoded colours and normalized texture coordinates. These are individual APT shapes; the APT timeline does not yet compose them into complete menu screens. Models with empty draw lists show their bounds and named nodes in the structural report.

Material resolution now follows shader fields and local archive dependencies. World models automatically use `world.gsh` and `world-misc.gsh` beside `world.big`. Identical duplicate textures are accepted after pixel comparison; genuinely different images with the same identifier remain an explicit conflict. **Texture bindings** shows the resolved count, and **Preview warnings** expands to show actual diagnostics. The structural report includes texture-bank provenance and shader bindings. World vertex colours and stored clamp/repeat settings survive export; authored unlit materials stay unlit when the preview-lighting toggle is changed.

For a character, choose `player_skel.ske`, choose `player_anims.anm`, enable **Apply animation to model**, then select a clip. The animation panel also allows an animation bank to drive a selected model. Compatible prop skeletons are selected automatically when unambiguous. **Pause**, the time slider and **Speed** control playback. **Skeleton overlay** shows the hierarchy.

- Left drag: orbit; right drag: pan; wheel: zoom. Images support zoom.
- **Fit view** restores model framing distance.
- **Unlit materials** helps inspect texture colors. Wii shader behavior is not reconstructed.
- **Inspect decoded structure** opens the underlying report. **Back to live preview** returns to the scene.
- Unsupported assets stay in the library with diagnostics; they are not replaced with fabricated previews.

Renderer fixtures include textured basketball, Alicia with three textures and a 68-bone animated skin, the Wii warning texture, a textured APT shape, the animated RC car and the teeter-totter. GPU screenshots and scene diagnostics are in `docs/captures`. The clip list contains 265 player clips; that is decoder coverage, not proof of exact game timing.

## One engine for viewer and future game

The viewer, cameras, materials, skinning and animation playback already run in **Bevy 0.19.1**. The future game can load the same generated GLB/PNG assets and share Rust asset components in `src/game.rs`. Each loaded scene records its original source and decoder version.

The decoding layer currently runs as one persistent, packaged Python worker. This preserves the recovered work while making the native interface responsive. Port a decoder to Rust only after its format and fixtures are stable. UI requests use sequence IDs to discard stale results, the library is virtualized, and generated previews are cached by input content. Static views use reactive updates; active animations render continuously. No Electron or web runtime is included.

**Recovered gameplay is partial.** Locomotion, frame/physics timing, controller events, multiplayer scoring and tetherball serve/motion arithmetic have Rust implementations. The world slice now dispatches movement, jump and camera-reorient through the original `controls.csv` and button timers. A 1:1 rewrite still requires Wii device input, full game states, Havok dynamics, AI and minigame execution, plus comparisons against the original game. See `docs/RECONSTRUCTION.md` for the boundary between decoded assets and recovered behavior.

## Headless use

The same executable has a JSON interface. `--headless` does not initialize a window or GPU. Pipe its output when using PowerShell:

```powershell
$app = 'D:\_eagl\_bevy\EAGL-Workbench.exe'
$catalog = & $app --headless catalog | ConvertFrom-Json
$model = ($catalog.value.assets | Where-Object name -eq 'basketball.o' | Select-Object -First 1).source
& $app --headless preview --source $model | ConvertFrom-Json
& $app --headless inspect --source $model | ConvertFrom-Json
```

`preview` accepts `--skeleton`, `--bank`, `--model` and `--index`; its `value.asset` points to a GLB/PNG under this folder's `assets`. `metadata` returns image, clip or frontend shape entries. `inspect` returns a deep format report. Sources may be physical paths or `outer.big::inner.viv::asset.o`.

Archive extraction and decoded exports use the existing bounded research implementation:

```powershell
& $app --headless extract --source 'D:\path\archive.big' --out 'D:\_eagl\_bevy\exports\archive-01' | ConvertFrom-Json
& $app --headless decode --source $model --out 'D:\_eagl\_bevy\exports\model-01' | ConvertFrom-Json
& $app --headless inventory --source 'D:\path\DATA' --deep --out 'D:\_eagl\_bevy\research\new-scan.json' | ConvertFrom-Json
```

Use actual paths in place of the example archive/DATA paths. Export destinations must be new; existing outputs are not overwritten. `decode` also supports `--skeleton` and `--index` for animation. `validate`, `hexdump` (`--offset`, `--length`) and `strings` (`--minimum`) are available. Responses use `{ok, value}` or `{ok:false, error}`. Failed commands return a nonzero exit code.

The decoder itself additionally accepts JSON lines on stdin, one response per request. It supports an extra `textures` array for explicit shared texture banks. `Remaster/EAGL-CLI.exe` retains its original command-line interface.

## Decoding status

The refreshed inventory covers 5,267 records, 3,685 distinct contents and 47 extensions, with no traversal errors. All **836 distinct `.o` files are accounted for: 828 produce partial geometry and eight declare empty draw lists**. There are no remaining model decoder failures in this corpus. This includes 619 frontend files containing 2,361 named shapes, all 49 PlaygroundShadow files, and the previously failing RC car, teeter-totter and compressed equipment/character shadows. The frontend export audit resolves every referenced texture and checks positions against independently stored model bounds.

GSH base images, TPL base images, recovered model layouts, skeletons and several animation codecs are supported. CSV/localization/effect trees and many binary structures are inspectable. Materials are approximate; shadow compositing, APT timelines/masks, sparse animation timing, static extra channels, collision and several other formats remain unresolved. Animation timing still assumes 30 fps. Sound/video playback is not implemented. `../Remaster/research/FINDINGS.md` records the ELF evidence and gaps; `../Remaster/research/model-verification.json` records the full model audit.

## Build and verify

Source needs Rust/MSVC and Python 3.14 with PyInstaller/NumPy. Dependencies are pinned in `Cargo.lock`; a first build needs `cargo fetch --locked`. Then:

```powershell
.\build.ps1
py -3.14 tools/verify_decoders.py
py -3.14 tools/verify_packaged.py
py -3.14 tools/verify_viewer.py D:\_eagl\_bevy\EAGL-Workbench.exe
py -3.14 tools/verify_material_viewer.py
```

`build.ps1 -DecoderOnly` refreshes only the worker, references and catalog. The build uses the shared decoder source in `../Remaster/src`. Generated files are under `assets/generated`, raw working copies under `cache`, and diagnostics under `logs`. Build intermediates under `target` and `build` are not needed to run the application.

Engine reference: [Bevy 0.19.1](https://github.com/bevyengine/bevy/releases/tag/v0.19.1). UI integration: [bevy_egui](https://github.com/vladbat00/bevy_egui). The local Greyhound library-preview document informed the on-demand workflow; no Greyhound implementation was copied.
