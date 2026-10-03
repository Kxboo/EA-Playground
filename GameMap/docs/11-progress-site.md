# 11 — Progress site (decomp.dev-style) and the code viewer

`GameMap/site/` is a static dashboard covering every function of the executable (19,166, all tiers). Each function is measured
two ways:

- **decoding**: how far it has been mapped, decompiled and *proven*;
- **runtime**: what the Rust/Bevy remake does with it.

It is plain HTML/JS with no build step and no server, published to GitHub Pages.

| section | what it shows |
|---|---|
| Progress | one panel per measurement. Each has headline shares, bars by bytes and by function count, and a legend that doubles as a filter |
| Rust / Bevy reconstruction | the verified-evidence cards from `data/reconstruction_progress.json` and the latest proof report |
| History | stacked share of each state per snapshot (`--snapshot`). Runtime snapshots start with the merged site |
| Function map | squarified treemap: source files as blocks, each function a cell sized by its bytes, coloured by the chosen measurement |
| Source files | every compilation unit with its state mix; filter and sort, click to list its functions |
| Functions | search, decoding and runtime chips, subsystem, file, sort (address, size, most called, most executed, most Rust). Opening a function shows its facts plus the **PowerPC / Ghidra C or Lifted C / Rust** panes |
| Next up | blockers (why functions are not proven yet), quick wins, most-called unproven functions, and the runtime gaps (VM traps) |
| How it's measured | both state definitions, the proof method, scope and caveats |

Two switches apply to the whole page:

- **Code**: Game + engine (the default), Middleware, or Whole executable.
- **Colour by**: Decoding or Runtime.

A function has its own link: `#f<addr>`, for example `#f802e1b78`. Old `#/fn/<addr>` links still work.

## States

### Decoding

Highest applicable state wins (`tools/build_site.py`). Only game + engine code is tracked; middleware shows as *not tracked*.

| state | meaning |
|---|---|
| reviewed | hand-written annotation with evidence (`data/annotations.json`, `verified_by` set) |
| proven | lifted pseudo-C proven equivalent to the machine code by randomized emulation (`tools/verify_lift.py`) |
| partial | lifted, but not every path could be exercised |
| decompiled | Ghidra decompiles it cleanly; readable, **not** proven |
| flagged | Ghidra reported bad instructions/warnings (mostly paired-single float ops) |
| mapped | name / unit / class facts only |

### Runtime

One state per function: the first match in this order.

| state | meaning |
|---|---|
| port | hand-written Rust in `_bevy/src` cites the function, by address or by `` `Class::Method` `` |
| host | the remake's PowerPC VM runs a Rust host function in its place (or observes it) |
| run | original code, entered in at least one recorded scenario |
| native | allowed to run as original code, but not reached yet |
| stub | the VM skips it (returns 0) |
| trap | an engine service with no Rust version; entering it stops the VM |

Runtime inputs come from the remake:

- the VM hook table: `cargo run --release --bin mglab -- classify classify.tsv`;
- function-entry coverage from runs with `EAGL_PPC_COVER=cov_<id>.tsv`. See `_bevy/docs/MGVM.md`.

`tools/ingest_runtime.py` turns both into `data/runtime_functions.tsv`, which holds measurements only.

## Viewing the compiled code next to the decoded code

The site itself contains **no game code**: that would be EA's code, so it is never committed or published. The **Rust** pane
always works. It is the remake's own code, published in `site/rust/` and linked to GitHub.

The PowerPC and C panes read a local code pack:

```bash
python3 GameMap/tools/build_code_pack.py --elf path/to/playgroundz.elf --ghidra-c path/to/decomp.c   # writes GameMap/site/code/ (git-ignored)
python3 -m http.server -d GameMap/site 8000                                                          # then open http://localhost:8000
```

The pack holds, for every function:

- the PowerPC listing, with branch targets named;
- the Ghidra C. `GameMap/decomp/ghidra.jsonl` (`tools/ghidra/ExportDecomp.py`) wins; otherwise `--ghidra-c` is used, a full-program
  export with one `//==== <symbol> @ <addr>` header per function;
- the lifted C, from `GameMap/decomp/lifted/*.c` (`tools/lift_all.py`) and `GameMap/decomp/lifted.jsonl`.

The C pane switches between Lifted C and Ghidra C. It marks lifted C as *proven equivalent* when the function is proven or reviewed.

## Refreshing the numbers

```bash
python3 GameMap/tools/gen_map.py --elf playgroundz.elf          # units / functions / xrefs (after any attribution change)
python3 GameMap/tools/lift_all.py --elf playgroundz.elf --out GameMap/data/lift_status.jsonl --jobs 4
python3 GameMap/tools/make_annotations.py                       # human annotations -> data/annotations.json
python3 GameMap/tools/ingest_runtime.py --elf playgroundz.elf --classify classify.tsv --cover 'cov_*.tsv'
python3 GameMap/tools/build_site.py --snapshot "label"          # rebuilds site/data.js + site/rust/ and appends a history point
```

`build_site.py` also scans `_bevy/src` for Rust references, so rerun it after Rust changes. That keeps the Rust panes and
the *port* state current.

## Publishing on your own page (GitHub Pages)

`.github/workflows/gamemap-pages.yml` publishes `GameMap/site` whenever it changes on `main`/`master`. You can also run it by
hand: *Actions → GameMap progress site → Run workflow*.

One-time setup: repository **Settings → Pages → Source: GitHub Actions**. The workflow refuses to publish if a `site/code/`
folder is present.

For this repository, the configured address is **https://kxboo.github.io/EA-Playground/**.

- Pull requests only validate the HTML entry point, the stylesheet and the JavaScript syntax; they don't deploy.
- After merging into `main`, the deployment job uploads only `GameMap/site/` and publishes through the `github-pages` environment.
- A manual dispatch also deploys only from `main` or `master`.
- Check the **GameMap progress site** workflow in Actions for deployment status. No npm install or HTML build is required.

Any static host works too: upload the contents of `GameMap/site/` without `code/`.
