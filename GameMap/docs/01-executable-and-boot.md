# 01 — The executable and the boot sequence

Evidence tags: **[C]** confirmed, **[D]** data read from the ELF, **[I]** inferred, **[U]** unresolved (see the [README](../README.md#evidence-levels-used-in-the-documents)).

## What the binary is

- ELF32, big-endian PowerPC (Broadway), entry `0x80006124` (`__start`). Built with Metrowerks CodeWarrior (`.mwcats.text`, MSL C library, MetroTRK debugger stub). **[D]**
- Symbols are **not stripped**: 19,166 functions and 26,613 data symbols with C++ names, and 1,669 `STT_FILE` entries that preserve the original
  source-unit names (`mgdodgeball.cpp`, `FEManager.cpp`, …). 757 of those units own code. **[D]**
- The `.debug`/`.line` sections hold real Metrowerks debug info for one library only — EA's *Exposure* (a Lua reflection/console service, `Expose.cpp`,
  `Console.cpp`, `RVL.cpp`), 89 KB. Everything else must be recovered from symbols and code. **[D]**
- Embedded build paths give component versions: EAGL runtime `1.05.12` (`…\eagl\releases\1.05.12\wii\runtime\src\Skin*.asm`), Exposure `1.01.01`,
  Allocator `1.6.0`. **[D]**
- The **DOL is byte-identical to the ELF's loadable sections** (all ten DOL sections compared), so the ELF carries everything the DOL does plus
  symbols. **[C]**

### Sections

| section | address | size | note |
|---|---|---:|---|
| `.init` | `0x80004000` | `0x24c4` | boot code, `__start` |
| `extab`, `extabindex` | `0x800064e0`, `0x8000d2c0` | `0x6de0`, `0x6a34` | C++ exception tables |
| `.text` | `0x80013d00` | `0x4091c4` | 4.3 MB of code |
| `.ctors`, `.dtors` | `0x8041cee0`, `0x8041d760` | `0x874`, `0xc` | static init/teardown |
| `.rodata` | `0x8041d780` | `0x25550` | strings, tables |
| `.data` | `0x80442ce0` | `0xae200` | vtables, tables, initialised globals |
| `.bss` | `0x804f0f00` | `0x10afc8` | zero-initialised |
| `.sdata` / `.sbss` | `0x805fbee0` / `0x805ffe60` | `0x3f74` / `0x26cc` | small data, base `r13 = 0x80603ee0` |
| `.sdata2` / `.sbss2` | `0x80602540` / `0x80607e60` | `0x5914` / `0x1c` | small const data, base `r2 = 0x8060a540` |

`r1 = 0x80647e80`, `r2`, `r13` are established in `__init_registers` (`0x80006290`). **[C]** Small-data (`r2`/`r13`-relative) accesses are how almost all
tunables and singletons are reached, so `tools/elfmap.py` resolves them.

### Where the code is (link order)

| tier | units | code | functions |
|---|---:|---:|---:|
| game (`aientity.cpp` … `trcUtil.cpp`) | 184 | 965,920 B | 5,170 |
| engine (renderer "Ren"/EAGL, cameras, world, physics glue, pad drivers) | 119 | 350,872 B | 1,616 |
| middleware (Wii SDK, Havok, nw4r/HBM, APT, Lua, EA audio/video/text-input, Bluetooth) | 825 | 2,978,432 B | 12,380 |

Link order is stable and contiguous, which is what makes per-unit ranges reliable. **[C]** Each unit's `__sinit_\<file>_cpp` is emitted at the *end* of
its code, so a unit's range ends at its last local symbol. `data/source_units.tsv` is the complete map.

## Boot sequence

`main` → `MainThread(int, void*)` → `InitAllModules()` → `BootSequence(false)` → `while (GameState::Update()) {}` → `GameState::Shutdown()` →
`ShutdownAllModules()`. **[C]** (`main` @`0x803aefd8`, `MainThread` @`0x803aed34`.)

`InitAllModules` @`0x803ae9fc` runs, in this order **[C]**:

1. Wii SDK: `OSInit`, `DVDInit`, `VIInit`, `PADInit`
2. `MemMgr::Create`, two memory-pool configurations (`InitPoolConfig`), `CString::InitPool`, `UpdateDipSwitches`
3. EA base: `THREAD_init`, `PRINT_init`, `TIMER_init`, filesystem options + `FILESYS_init`, `PAD_init` (and removal of the `PAD_update` sync task)
4. Renderer: `Ren::Engine::Create` + `Initialize`, font driver install (`EAGLdriver`), embedded font (`gEmbeddedFontData`)
5. `TRC::InitCoreHandlers` (Wii technical-requirement handling), `IniFileManager::Create`, **`LoadIniFile("disc.ini")`**, `SetBootOptionsFromIni`
6. `pgIO::Create` (BIG file IO), `AssetManager::Create`, `TRC::InitHomeMenu`
7. **Wrist-strap reminder screen** (`StrapWarningScreen` create/initialize/update/finish/destroy) — a real per-frame loop before anything else renders
8. `Ren::Engine::CreateScene`, home-menu disable/task, `ASYNCFILE_init`, `pgIDatabase::Create` + `InitialiseDatabase` (the attribute/VLT database),
   `Audio::Initialize`, `PGMoviePlayer::Create`, `HighResScreenShot` setup (tile counts default to 3×3, `gScreenShotTilesAcross/Down`)

`BootSequence` @`0x803aed9c` calls `GameState::Init`, then sets the top-level state to `BootFlow` and the boot-flow sub-state to 1 (see
[02](02-state-machine-and-timing.md)). **[C]**

### `disc.ini` is consumed by the game **[C]**

`SetBootOptionsFromIni` @`0x803aee08` reads `disc.ini` through `IniFileManager` and looks up exactly three keys:

| key | handling |
|---|---|
| `region` | compared with `"eu"` and `"us"` (`strcmp`) — selects the region setting |
| `productcode` | copied by `SetProductCode` into a global `productCode` buffer |
| `parentallock` | `atoi` → `SetParentalLock` |

If the file or keys are missing the defaults are product code `RET000000` and parental lock `0`. The supplied file (`region=us`, `productcode=RPXE`,
`parentallock=1`, `authserver=1`) therefore yields region **us**, product code **RPXE**, parental lock **1**. **`authserver` is never referenced by any code
in the executable**, so it belongs to the disc/loader tooling rather than the game. **[C]**

### Other boot details

- `MainThread` compares its first argument with `"skipfe"` but **discards the result**; `BootSequence` is always called with `false`. The option is dead in this build. **[C]**
- Boot wrist-strap reminder (the Wii "securely fasten the wrist strap" screen): `StrapWarningScreen::Initialize` builds `data/boot/strapwarn_` + (`wide_` | `standard_`) + language (`english.gsh`, `french.gsh`,
  `german.gsh`, `spanish_eu.gsh`, `italian.gsh`, `dutch.gsh`) and renders it as `strapA_screen`. **[D]** (The repo's texture work decodes
  `strapwarn_standard_english.gsh`; see [08](08-container-and-boot-files.md) for the separate 16:9 `.tpl`.)
- Other boot resources named in code: `/homeBtn*.arc` (HOME-button UI per language), `data/saveicons/icon0.tpl` and `/data/saveicons/banner.tpl`
  (save-file icon/banner), `/title/00000001/00000002/data/setting.txt` (Wii settings), `/config.txt`. **[D]**
- Audio bank and asset directories are constants: `data/audio/AEMS/`, `data/audio/Banks/`, `data/audio/Music/`, `data/audio/Speech/`
  ([asset tables](asset-name-tables.md)). **[D]**
