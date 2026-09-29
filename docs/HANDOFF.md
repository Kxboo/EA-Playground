# Developer handoff

Baseline: material decoding pass, decoder **`native-5-materials-4`**, recorded 2026-09-29. Start with the root README, [setup](SETUP.md), and [current findings](../Remaster/research/FINDINGS.md). This is an initial source import into Git; earlier development history survives through notes and provenance, not earlier Git commits.

## Architecture and ownership

| Area | Entry points |
| --- | --- |
| Bevy viewer and material/animation UI | `_bevy/src/main.rs`, `viewer.rs` |
| Worker lifecycle, source/package selection, JSON requests | `_bevy/src/bridge.rs` |
| Future game asset metadata boundary | `_bevy/src/game.rs` |
| Persistent worker, catalog, cached previews | `_bevy/tools/decoder_bridge.py` |
| Shared decode/export integration | `Remaster/src/core.py` |
| Archive traversal, deep inspection and exports | `Remaster/src/research.py`, `archives.py`, `containers.py` |
| Exact material ownership and wrap/colour/normal handling | `Remaster/src/material_bindings.py` |
| Containment-based texture dependencies | `Remaster/src/asset_links.py` |
| ELF-derived model families | `frontend_models.py`, `hwskin_models.py`, `shadow_models.py`, `pcode.py` |
| Recovered model/image/skeleton/animation implementations | `Remaster/src/legacy/` |

The native viewer communicates with one persistent Python worker over JSON lines. The worker reads original files and writes derived GLB/PNG/report previews under `_bevy/assets/generated`. Cache keys include decoder version, request and input content. Increment `VERSION` after semantic decoding changes to invalidate stale previews. Keep both projects adjacent; avoid introducing separate UI-only parsers.

## Material fixes in this baseline

- Read shader-specific named texture fields using ELF-derived schemas instead of guessing from nearby relocation names.
- Bind each mesh through its PCode interval or exact source position/UV array identities when a legacy descriptor begins early.
- Treat textureless shadow shaders as intentionally textureless. Do not silence genuine missing/ambiguous bindings.
- Search containing archives and sibling banks, including shared world banks. Prefer exact full names; only use short aliases for entries without a full name.
- Collapse duplicate names only when decoded dimensions and pixel hashes agree. Preserve different-image conflicts in diagnostics.
- Preserve world RGBA8 vertex colours and their seams, authored unlit state, and TAR wrap modes. NPOT axes clamp according to executable evidence.
- Decode PlaygroundToonShade normals as S16 Q14; the old parser read these as bytes.
- Show binding counts, provenance and detailed warnings in the native viewer/headless reports.

The net fixture catches a real ownership error: its net mesh previously received the handle's material. The buggy catches the normal/lighting correction. The world catches shared banks, 161 texture bindings, vertex colours and wrap state.

## Verification baseline

| Check | Recorded result | Evidence |
| --- | --- | --- |
| Source regression suite | 32 passed | Tests under `Remaster/tests`; `_bevy/docs/build-verification.json` |
| Full model export audit | 828 geometry exports, 8 empty, 0 failures | [model-verification.json](../Remaster/research/model-verification.json) |
| Material resolution | 3,480 bindings, 0 warning models (previously 80) | [material-summary.json](../Remaster/research/material-summary.json), [detailed provenance](../Remaster/research/material-verification.json) |
| Native packaged headless checks | 17 passed | [packaged-verification.json](../_bevy/docs/packaged-verification.json) |
| Standalone packaged CLI | 12 passed | [cli-verification.json](../Remaster/docs/cli-verification.json) |
| Final material renderer checks | 5 passed | [material-render-verification.json](../_bevy/docs/material-render-verification.json) |

The five final renderer fixtures are net, buggy, world, Alicia and frontend. Their existing PNGs and adjacent `.png.json` diagnostics are in [_bevy/docs/captures](../_bevy/docs/captures/). The root README embeds those actual captures and the earlier animated-character capture. Automated scene checks plus visual review establish useful previews, not game-exact pixels.

Reports retain original machine paths and asset hashes for provenance. Some older inventory summaries and `Remaster/docs/docs/SESSION_NOTES` predate fixes; use `FINDINGS.md`, current model/material reports and the final renderer report for current status. The full texture audit covers 3,574 images in 736 distinct GSH files; model/frontend coverage includes 619 frontend files and 2,361 named shapes.

## Executable evidence

Private input: `Remaster/reference/playgroundz.elf`, SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

- `RuntimeAllocTARConstructor` at `0x803e8c48`: wrap properties 22/23 and NPOT clamp behavior.
- `ModelRenderPlaygroundTexture` at `0x80016dcc`: RGBA8 attributes and disabled lighting.
- `ModelRenderPlaygroundToonShade`: S16 Q14 normal setup at `0x8001ba2c`.
- Research disassemblies: `material-state-disassembly.txt`, `material-shaders-disassembly.txt`, `vertex-color-shaders-disassembly.txt`; shader field schemas: `shader-schemas.json`.

`Remaster/research/elf_trace.py` regenerates symbol-aware disassembly using optional Capstone. The binary itself is not versioned. Keep inferred behavior explicitly distinguished from confirmed calls, fields and state.

## Next decoding priorities

1. Recover GX/TEV material state: exact alpha compare/blend, depth/culling, texture filtering/LOD, sphere mapping/specular and dynamic shadow composition. Preserve current identity/provenance tests while adding evidence-specific fixtures.
2. Recover APT timelines, transforms, masks and composition. Individual frontend shapes already export; the previous note about 14 failed models is obsolete.
3. Resolve sparse animation timing, static extra channels and original update rate. Current playback assumes 30 fps; 265 parsed player clips do not prove timing equivalence.
4. Decode Havok/VLT object graphs, collision and remaining recognized-but-uninterpreted formats. Display triangles alone are not collision evidence.
5. Begin a gameplay vertical slice only after input mapping, simulation timing and original state transitions are documented. No original gameplay logic is implemented yet; see [RECONSTRUCTION.md](../_bevy/docs/RECONSTRUCTION.md).

## Working conventions

Read originals without modifying them. Keep unsupported fields and ambiguity visible. Use new export destinations instead of overwriting previous outputs. Validate a decoder correction against the smallest representative fixture, then the affected corpus. Use headless preview/inspect for diagnostics and a real Bevy render for visual changes. Refresh packaged workers before testing executables: the packaged worker takes precedence over source.

Do not commit original DATA/ELF, extracted models/textures, local references, dependency installations, build outputs or caches. The `.gitignore` permits the two maintained projects and repository docs, excluding the older extraction workspace. Screenshots are intentionally included at the user's request. Keep the original provenance records; no blanket license has been assigned to recovered code by this import.
