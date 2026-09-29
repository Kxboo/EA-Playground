# GameMap — a reference map of the EA Playground (Wii) executable

This folder maps **what the game's code is and how it is organised**, so that later Bevy/Rust reconstruction work can look
things up instead of re-deriving them. It complements the asset work (`Remaster/`, `_bevy/`): those decode *models, textures and
animations*; this maps the *functions, state machines, input, menus, minigame rules and data-file entry points*.

It is a **reference**, not a port. No gameplay has been re-implemented here except a few small, byte-exact models that were verified
against the original code (see [Verified reference models](#verified-reference-models)). It follows the repository's
[reconstruction evidence policy](../_bevy/docs/RECONSTRUCTION.md): nothing is called 1:1 without executable evidence.

## Source and limits

Everything here comes from **one input: `playgroundz.elf`** (SHA-256 `5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`,
the same binary the rest of the repo uses, kept out of git). It is unstripped: 19,166 function symbols, 26,613 data symbols and the
original compilation-unit names. The `.dol` was checked and is **byte-identical** to the ELF's loadable sections (the ELF is a strict
superset). The original `DATA/` folder was **not** available when this map was built, so file formats and CSV/VLT contents are covered
only where the *executable's parsers* reveal them (schemas, hashes, name tables), not by inspecting the files.

Known caveats: units that have no local function symbol have no address anchor, so their functions are attributed to the following
unit (one class-name override is applied for the pad-mode handlers); Metrowerks demangling is best-effort (134 argument lists are kept
raw); the call graph contains direct `bl` calls only (virtual/indirect calls are not resolved).

## Evidence levels used in the documents

| tag | meaning |
|---|---|
| **[C] confirmed** | read from the disassembly and cross-checked (multiple call sites, or executed/emulated and compared) |
| **[D] data** | an exact table or value read from the ELF image (name tables, enum parsers, initial values) |
| **[I] inferred** | plausible from symbol/string names or structure; *not* verified — do not port as 1:1 |
| **[U] unresolved** | known gap; where to look is stated |

## Layout

| path | contents |
|---|---|
| [`docs/01-executable-and-boot.md`](docs/01-executable-and-boot.md) | sections, toolchain, boot sequence, `disc.ini`, main loop |
| [`docs/02-state-machine-and-timing.md`](docs/02-state-machine-and-timing.md) | `GameState` graph, per-state frame order, time step, physics step, tunables |
| [`docs/03-input.md`](docs/03-input.md) | Controller architecture, `controls*.csv` schema, event/state/button enums, pad modes |
| [`docs/04-frontend-and-menus.md`](docs/04-frontend-and-menus.md) | APT movies, FS/LV handler bridge, front-end state → screen flow, HUD calls |
| [`docs/05-minigames-and-rules.md`](docs/05-minigames-and-rules.md) | `Minigame` base, `MultiplayerMode` scoring (verified), the nine minigame families |
| [`docs/06-world-characters-physics-ai.md`](docs/06-world-characters-physics-ai.md) | world, characters, Havok glue, AI compulsions, cameras, animation states, conversations |
| [`docs/07-data-formats-and-hashes.md`](docs/07-data-formats-and-hashes.md) | verified hashes (`string.idx`, VLT keys), file/path tables, how each data file is reached |
| [`docs/08-container-and-boot-files.md`](docs/08-container-and-boot-files.md) | `.dol`/`.elf`, `opening.bnr`, the 16:9 strap `.tpl`, `disc.ini` |
| [`docs/09-roadmap.md`](docs/09-roadmap.md) | suggested reconstruction order, prerequisites and start addresses, open questions |
| [`docs/10-evidence-log.md`](docs/10-evidence-log.md) | every verified claim with address, method and script |
| [`docs/asset-name-tables.md`](docs/asset-name-tables.md) | names the code builds file paths and DB keys from (generated) |
| [`docs/subsystems/`](docs/subsystems/README.md) | one generated page per game/engine subsystem: classes, vtables, every function, strings, globals |
| `data/` | machine-readable tables (TSV/JSON): units, functions, classes, vtables, call graph, xrefs, enums, APT handlers, traces |
| `reference/` | byte-exact Python models verified against the original code |
| `tools/` | the generators and verifiers (below) |

## Regenerating

```bash
pip install pyelftools capstone unicorn
# the ELF is not in git; supply it (the repo convention is Remaster/reference/playgroundz.elf)
python3 GameMap/tools/gen_map.py  --elf Remaster/reference/playgroundz.elf   # writes GameMap/data
python3 GameMap/tools/gen_docs.py                                            # writes GameMap/docs/subsystems + name tables
python3 GameMap/tools/verify_hashes.py      --elf Remaster/reference/playgroundz.elf
python3 GameMap/tools/verify_multiplayer.py --elf Remaster/reference/playgroundz.elf
python3 GameMap/tools/verify_postgame.py    --elf Remaster/reference/playgroundz.elf
python3 GameMap/tools/check_controls_csv.py path/to/controls*.csv            # your own copies of the game data
python3 GameMap/tools/vlt_probe.py --vlt db.vlt --bin db.bin --elf Remaster/reference/playgroundz.elf
```

| tool | purpose |
|---|---|
| `tools/elfmap.py` | ELF/symbol library, compilation-unit ranges, subsystem classification, PowerPC disassembly with register constant-propagation, string-compare enum extraction |
| `tools/mwdemangle.py` | Metrowerks name demangler (never raises; keeps raw text when unsure) |
| `tools/handlers.py` | recovers the APT ↔ native command bindings and traces dispatch code with a small emulator |
| `tools/emu.py` | Unicorn harness for calling original functions (verification only) |
| `tools/hashes.py` | the two recovered hash functions |
| `tools/gen_map.py`, `tools/gen_docs.py` | generators |
| `tools/verify_hashes.py`, `verify_multiplayer.py`, `verify_postgame.py` | emulate the original code and compare with the reference models |
| `tools/check_controls_csv.py` | validate `controls*.csv` files against the enums recovered from the executable |
| `tools/vlt_probe.py` | parse `db.vlt`/`db.bin`: chunk table, export table, classes, collections, resolvable field names (schema only) |

## Verified reference models

These are the only behaviours re-implemented so far, each **byte-exact against the original PowerPC code** by emulation:

| model | file | verification |
|---|---|---|
| `Locale` string-key hash (`ComputeHash`) and `Attrib::StringHash64` (Bob Jenkins lookup8, fixed seed) | `tools/hashes.py` | 309 strings incl. 24/48-byte block boundaries |
| `MultiplayerMode` (point series, wins, ranking, tie-break, quirks) | `reference/multiplayer_mode.py` | 66,264 comparisons over 400 randomized sessions |
| Post-game result scoring (`Minigame::OpenPostGameScreen`: 1v1 / team / free-for-all awards) | `reference/multiplayer_mode.py:post_game_awards` | 3,000 randomized results, UI callees stubbed |

Also validated against **real game files supplied by the project owner** (not committed): the 64-bit key hash resolves 303 strings and 30 class names in the real `db.vlt`; the `controls*.csv` schema and enums resolve 245 of 246 rows of 7 real files
(the 246th is a deliberately disabled row).

## Scale of the problem

Of 4.30 MB of PowerPC code, **966 KB (5,170 functions) is game code** and **351 KB (1,616 functions) is the engine layer**; the other
**2.98 MB (12,380 functions) is middleware** (Wii SDK, Havok 4.x, nw4r/HBM, the APT UI runtime, Lua, EA audio/video/text-input
libraries, Bluetooth stack). Reconstruction should concentrate on the first two; the middleware has public equivalents in Bevy/Rust.
See [`data/middleware_units.tsv`](data/middleware_units.tsv) and [`docs/subsystems/README.md`](docs/subsystems/README.md).
