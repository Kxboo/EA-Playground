# 10 — Evidence log

Every claim that is stronger than "read from a symbol/table" and how it was checked. Re-run the scripts to reproduce.

| claim | method | script / where |
|---|---|---|
| ELF hash `5cef3efc…3e2c` is the repo's reference binary | SHA-256 | `elf_facts.json` |
| DOL sections are byte-identical to the ELF's | section compare (10/10) | ad-hoc, recorded in [08](08-container-and-boot-files.md) |
| `ComputeHash` = `h=0xFFFFFFFF; h=h*33+(int8)c` | emulated original, 309 strings | `tools/verify_hashes.py` |
| `Attrib::StringHash64` = lookup8, seed `0xABCDEF0011223344` | emulated original, 309 strings **and** resolved 303 pool strings + 30 class names in the real `db.vlt` | `tools/verify_hashes.py`, `tools/vlt_probe.py` |
| enum values of `EActionEvent` (188), `EControllerState`, buttons, button-event kinds | extracted from the `ConvertStringTo*` if-chains (`li r3,N` / `andi.` tails), spot-checked in the disassembly (`STATE_MULTIPLAYER`=29 etc.) | `data/enums/`, `tools/elfmap.py:string_enum` |
| `controls*.csv` schema and enum coverage | 7 real files, 246 rows, 245 fully resolved, 1 deliberate `-NOTUSED` | `tools/check_controls_csv.py` |
| `cCSVParser` matches columns by header name and left-trims | disassembly `GetStringField`/`GetString` (`TrimWhiteSpaceLeft`) + the footie sample with a leading space | [03](03-input.md) |
| `GameState` states, jump table and 16 transition call sites | jump table read + every `SetNewState` call site's state argument | `data/state_transitions.tsv` |
| frame time: cap 60, fixed 16, ×1000 / cycle scaling | disassembly `GameState::Update`, initial values of the named globals | [02](02-state-machine-and-timing.md) |
| physics step model | disassembly `PhysicsManager::Update`, globals `gSimPhysicsSingleUpdateTimeCap=60`, `…TimeMultiplier=1.0` | [02](02-state-machine-and-timing.md) |
| `MultiplayerMode` behaviour | byte-exact emulation compare, 66,264 comparisons / 400 sessions | `tools/verify_multiplayer.py` |
| post-game awards (50 / 50 / 50+25+10) | byte-exact emulation compare with UI callees stubbed, 3,000 randomized results | `tools/verify_postgame.py` |
| 132 APT ↔ native handler registrations and their native targets | all `Register{FS,LV}Handler` call sites; each `DoJob*` path executed in a mini-emulator | `tools/handlers.py`, `data/apt_handlers.tsv` |
| screen-stack wrapper strings (`OpenScreen`, `CloseScreen`, `OpenOverlay`, `CloseOverlay`, `ReplaceScreen`, `ClearScreenStack`) | traced with the emulator incl. tail calls | [04](04-frontend-and-menus.md) |
| `eFrontEndGameState` → update function map | jump table `0x804d6e2c`, all `SetFEGameState` call sites | `data/enums/fe_game_state_update_functions.tsv`, `data/fe_state_transitions.tsv` |
| `WorldMan::StartMinigame` reads DB class `minigames` and fields | string arguments of the function | `data/xref_strings.tsv` |
| VLT chunk table, `ExpN` record layout, class-first ordering, 32 classes / 908 collections | parsed the real `db.vlt`/`db.bin` | `tools/vlt_probe.py`, `data/vlt_schema.tsv` |
| 16:9 TPL is the wrist-strap reminder, RGBA8 853×480, not referenced by the executable | decoded the image; string search of the ELF | [08](08-container-and-boot-files.md) |
| `opening.bnr` structure | parsed IMET, U8, IMD5, LZ10, BNS headers | [08](08-container-and-boot-files.md) |
| DB names/field names | hashed ELF strings + pool strings and matched against `ExpN`/`DatN` | `tools/vlt_probe.py` |

## Methods and their limits

- **Static tables** (`data/`) are exact reads of the ELF image. Names come from the symbol table; demangling is best-effort.
- **Constant propagation** (`elfmap.disasm`) follows `lis/addi/ori/li/mr` and small-data bases within a function, linearly; it does not model loads from memory, so values derived from memory are left unknown (never guessed).
- **Path tracing / emulation**: `handlers._trace` executes a small PowerPC subset to follow dispatch code; `Emu` runs *real* original functions in Unicorn with unrelated callees stubbed. A pass proves the reference model equals the original code for the exercised inputs,
  not for all inputs; the random inputs were chosen to cover all branches of the decoded functions.
- **Not verified**: anything tagged `[I]`/`[U]` in the documents, the semantics of `UpdateInput`, all AI behaviour, and every rendering detail.
