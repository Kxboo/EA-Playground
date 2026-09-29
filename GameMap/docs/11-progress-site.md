# 11 — Progress site (decomp.dev-style) and the code viewer

`GameMap/site/` is a static dashboard that shows how much of the executable has been mapped, decompiled and *proven*, per
subsystem, compilation unit and function. It is plain HTML/JS with no build step and no server.

| page | what it shows |
|---|---|
| Overview | proven / decoded share by bytes and by function count, per-state cards, history chart, subsystem table, treemap of every compilation unit (colour bands = state mix) |
| Subsystems | one row per subsystem with a progress bar; click to list its functions |
| Units | one row per compilation unit in link order (sortable, filterable) |
| Functions | every game/engine function; filter by state, tier, subsystem, kind, unit, text; sort by size or fan-in; **Decoded only** toggle |
| Next up | why functions are not proven yet (blocker histogram), quick wins, most-called unproven functions, biggest flagged ones |
| How it's measured | the state definitions and the proof method |

## States

Highest applicable state wins (see `tools/build_site.py`):

| state | meaning |
|---|---|
| reviewed | hand-written annotation with evidence (`data/annotations.json`, `verified_by` set) |
| proven | lifted pseudo-C proven equivalent to the machine code by randomized emulation (`tools/verify_lift.py`) |
| partial | lifted, but not every path could be exercised |
| decompiled | Ghidra decompiles it cleanly; readable, **not** proven |
| flagged | Ghidra reported bad instructions/warnings (mostly paired-single float ops) |
| mapped | name / unit / class facts only |

Only game + engine code counts toward percentages; middleware (SDK, Havok, nw4r, Lua, APT runtime, EA libraries) is listed
for size only.

## Viewing the compiled code next to the decoded code

Every function drawer has **Compare / Compiled / Ghidra C / Lifted C** tabs. The site itself contains **no game code**
(it would be EA's code, so it is never committed or published). To light the tabs up on your own machine:

```bash
python3 GameMap/tools/build_code_pack.py --elf path/to/playgroundz.elf   # writes GameMap/site/code/  (git-ignored)
python3 -m http.server -d GameMap/site 8000                              # then open http://localhost:8000
```

`Compare` shows the original PowerPC listing (from your ELF) on the left and either the **lifted C** (proven when the
function's state is proven/reviewed) or the **Ghidra C** (reading aid only) on the right. The pack needs
`GameMap/decomp/ghidra.jsonl` (`tools/ghidra/ExportDecomp.py`) and `GameMap/decomp/lifted/*.c` (`tools/lift_all.py`).

## Refreshing the numbers

```bash
python3 GameMap/tools/gen_map.py --elf playgroundz.elf          # units / functions / xrefs (after any attribution change)
python3 GameMap/tools/lift_all.py --elf playgroundz.elf --out GameMap/data/lift_status.jsonl --jobs 4
python3 GameMap/tools/make_annotations.py                       # human annotations -> data/annotations.json
python3 GameMap/tools/build_site.py --snapshot "label"          # rebuilds site/data.js and appends a history point
```

## Publishing on your own page (GitHub Pages)

`.github/workflows/gamemap-pages.yml` publishes `GameMap/site` whenever it changes on `main`/`master` (or manually via
*Actions → GameMap progress site → Run workflow*). One-time setup: repository **Settings → Pages → Source: GitHub Actions**.
The workflow refuses to publish if a `site/code/` folder is present. The URL will be
`https://<user>.github.io/<repo>/`.

Any static host works too: upload the contents of `GameMap/site/` (without `code/`).
