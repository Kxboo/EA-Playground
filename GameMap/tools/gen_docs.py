#!/usr/bin/env python3
"""Generate GameMap/docs/subsystems/*.md from the tables written by gen_map.py.

    python3 GameMap/tools/gen_docs.py

Everything in these pages is mechanically derived from the executable's symbols, code references
and data tables (see GameMap/data). No behaviour is inferred from names; narrative interpretation
lives in the hand-written docs and is labelled with an evidence level there.
"""
import collections
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data")
OUT = os.path.join(HERE, "..", "docs", "subsystems")


def rows(name, ncols=None):
    p = os.path.join(DATA, name)
    out = []
    for l in open(p, encoding="utf-8"):
        if l.startswith("#") or not l.strip():
            continue
        out.append(l.rstrip("\n").split("\t"))
    return out


# per-minigame vocabulary so each page can pull its own inputs, animation states and audio
MG = {
    "mg-freethrow": dict(title="Free Throw / 21 / Dribbling (basketball microgames)",
                         ev=r"^EVENT_(21|FREETHROW)_", st=r"^STATE_(21_|FREETHROW|DRIBBLING)", anim=r"^ANIM_BBALL_",
                         ctl=r"mg21|freethrow|dribbling", bank=r"FreeThrow|BBall", sfx=r"MGSFX_(HUD_DB|Dodgeball)", hud=r"Dribbling|FreeThrow|BallMeter|HitCounter|SavesCounter"),
    "mg-microbug": dict(title="Bug Hunt (bug-catching microgame)", ev=r"^EVENT_BUGHUNT_", st=r"$^", anim=r"^ANIM_BUGHUNT_",
                        ctl=r"bughunt", bank=r"BugCatch", sfx=r"$^", hud=r"Bug"),
    "mg-dartshootout": dict(title="Dart Shootout (incl. Quick Draw and boss encounters)", ev=r"^EVENT_(DARTSHOOTOUT|QUICKDRAW)_",
                            st=r"^STATE_(QD_|RANGED)", anim=r"^ANIM_(DS_|QDRAW_)", ctl=r"dartshootout|quickdraw",
                            bank=r"DartShootout", sfx=r"MGSFX_(Darts|HUD_DS)", hud=r"Dart|Reload|Danger|BlockDart|ShootDart|HealthBar|BossHealth|LowBoss|LevelBoss"),
    "mg-dodgeball": dict(title="Dodgeball", ev=r"^EVENT_DODGEBALL_", st=r"^STATE_DODGEBALL", anim=r"^ANIM_DB_",
                         ctl=r"dodgeball", bank=r"Dodgeball", sfx=r"MGSFX_(Dodgeball|HUD_DB|DB_)", hud=r"Dodge|MegaMeter"),
    "mg-footie": dict(title="Footie (soccer)", ev=r"^EVENT_FOOTIE_", st=r"^STATE_FOOTIE", anim=r"^ANIM_SC_",
                      ctl=r"footie", bank=r"Footie", sfx=r"MGSFX_(Footie|HUD_Footie)", hud=r"Footie|Goal"),
    "mg-paperairplanes": dict(title="Paper Airplanes", ev=r"^EVENT_PA_", st=r"^STATE_PA$", anim=r"^ANIM_PA_",
                              ctl=r"paperairplanes", bank=r"PaperAirplane", sfx=r"MGSFX_(PaperAirp|HUD_PA)", hud=r"PaperAirplane"),
    "mg-rccars": dict(title="RC Cars", ev=r"^EVENT_RCCARS_", st=r"$^", anim=r"^ANIM_RC_", ctl=r"rccars",
                      bank=r"RC_Cars", sfx=r"MGSFX_(RCCars|HUD_RC)", hud=r"RcCars|LapCounter|BoostMeter|MiniMap|FinalLap|Position_"),
    "mg-tetherball": dict(title="Tetherball", ev=r"^EVENT_TETHERBALL_", st=r"^STATE_TB_", anim=r"^ANIM_TB_",
                          ctl=r"tetherball", bank=r"Tetherball", sfx=r"MGSFX_(TB_|Tetherball|HUD_TB)", hud=r"Tether"),
    "mg-wallball": dict(title="Wall Ball", ev=r"^EVENT_WALLBALL_", st=r"^STATE_WALLBALL", anim=r"^ANIM_WB_",
                        ctl=r"wallball", bank=r"WallBall", sfx=r"MGSFX_(WallBall|WSFX|HUD_WB)", hud=r"WallBall|Wallball"),
}


def esc(s):
    return s.replace("|", "\\|")


def main():
    os.makedirs(OUT, exist_ok=True)
    units = rows("source_units.tsv")
    funcs_by_sub = collections.defaultdict(list)
    sub_of_addr = {}
    tier_of = {}
    for fl in sorted(os.listdir(os.path.join(DATA, "functions"))):
        tier, sub = fl[:-4].split("-", 1)
        for r in rows("functions/" + fl):
            funcs_by_sub[sub].append(r)
            tier_of[sub] = tier
    unit_sub = {}
    for r in units:
        unit_sub[int(r[0], 16)] = (r[5], r[4])
    # address -> subsystem (all tiers) for the call-graph summary
    ranges = sorted((int(r[0], 16), int(r[1], 16), r[5], r[4]) for r in units)
    import bisect
    starts = [x[0] for x in ranges]

    def sub_at(a):
        i = bisect.bisect_right(starts, a) - 1
        if i >= 0 and ranges[i][0] <= a < ranges[i][1]:
            return ranges[i][2]
        return "sdk/crt/other"
    xs = collections.defaultdict(lambda: collections.Counter())
    for r in rows("xref_strings.tsv"):
        xs[sub_at(int(r[0], 16))][(r[2])] += 1
    xg = collections.defaultdict(set)
    for r in rows("xref_globals.tsv"):
        xg[sub_at(int(r[0], 16))].add(r[1])
    glob = {r[3]: r for r in rows("globals.tsv")}
    cg = collections.defaultdict(collections.Counter)
    for r in rows("callgraph.tsv"):
        cg[sub_at(int(r[0], 16))][sub_at(int(r[1], 16))] += 1
    vt = collections.defaultdict(list)
    for r in rows("vtables.tsv"):
        vt[r[0]].append(r)
    enums = {}
    for fn in os.listdir(os.path.join(DATA, "enums")):
        enums[fn[:-4]] = rows("enums/" + fn)
    handlers = rows("apt_handlers.tsv")
    classes = {r[0]: r for r in rows("classes.tsv")}

    index_lines = []
    for sub, L in sorted(funcs_by_sub.items(), key=lambda kv: (tier_of[kv[0]], kv[0])):
        tier = tier_of[sub]
        su = [u for u in units if u[5] == sub and u[4] == tier]
        total = sum(int(u[2]) for u in su)
        md = []
        meta = MG.get(sub)
        md.append("# %s — %s" % (sub, meta["title"] if meta else tier + " subsystem"))
        md.append("")
        md.append("> Generated by `GameMap/tools/gen_docs.py` from `GameMap/data`. Addresses are Wii virtual addresses in the "
                  "unstripped `playgroundz.elf` (SHA-256 `5cef3efc…3e2c`). Names are the executable's own symbols (demangled from Metrowerks "
                  "mangling; raw names are in `data/functions/%s-%s.tsv`). Nothing on this page is inferred behaviour." % (tier, sub))
        md.append("")
        md.append("**Tier:** %s · **Units:** %d · **Functions:** %d · **Code:** %s bytes" % (tier, len(su), len(L), format(total, ",")))
        md.append("")
        md.append("## Source units (link order)")
        md.append("")
        md.append("| start | end | bytes | funcs | unit |")
        md.append("|---|---|---:|---:|---|")
        for u in su:
            md.append("| `%s` | `%s` | %s | %s | `%s` |" % (u[0], u[1], u[2], u[3], u[6]))
        # ---- class table
        cls = collections.defaultdict(list)
        for r in L:
            cls[r[2]].append(r)
        md.append("")
        md.append("## Classes and free functions")
        md.append("")
        md.append("| class | methods | code bytes | ctor | dtor | virtual slots (vtable) |")
        md.append("|---|---:|---:|---|---|---:|")
        for c in sorted(cls, key=lambda c: (c == "", c)):
            if c == "":
                continue
            m = cls[c]
            md.append("| `%s` | %d | %s | %s | %s | %d |" % (c, len(m), sum(int(x[1]) for x in m),
                                                       "`%s`" % classes[c][4] if c in classes and classes[c][4] else "",
                                                       "`%s`" % classes[c][5] if c in classes and classes[c][5] else "",
                                                       len(vt.get(c.split("::")[-1], []))))
        free = cls.get("", [])
        if free:
            md.append("")
            md.append("Free functions: %d (listed at the end)." % len(free))
        # ---- vtables
        vshown = [(c, vt[c.split("::")[-1]]) for c in sorted(cls) if c and c.split("::")[-1] in vt]
        if vshown:
            md.append("")
            md.append("## Virtual-method tables")
            md.append("")
            md.append("Slot → implementing function. A slot implemented in a *different* class shows inheritance/overriding "
                      "(base implementations are reused, not copied).")
            for c, vr in vshown:
                md.append("")
                md.append("**%s** (vtable `%s`)" % (c, vr[0][5]))
                md.append("")
                md.append("| slot | function | implemented in | addr |")
                md.append("|---:|---|---|---|")
                for r in vr:
                    md.append("| %s | `%s` | %s | `%s` |" % (r[1], esc(r[3]), r[4], r[2]))
        # ---- functions per class
        md.append("")
        md.append("## Functions")
        for c in sorted(cls, key=lambda c: (c == "", c)):
            md.append("")
            md.append("### %s" % (("`%s`" % c) if c else "free functions"))
            md.append("")
            md.append("| addr | size | signature |")
            md.append("|---|---:|---|")
            for r in sorted(cls[c], key=lambda r: int(r[0], 16)):
                sig = r[3] + "(" + r[4] + ")" + (" const" if r[5] else "")
                md.append("| `%s` | %s | `%s` |" % (r[0], r[1], esc(sig)))
        # ---- minigame extras
        if meta:
            md.append("")
            md.append("## Inputs, animation states and audio (from the executable's name tables)")
            md.append("")
            for key, label, rx, tab in (("input_action_events", "Input events (`EActionEvent`, value = enum)", meta["ev"], "input_action_events"),
                                        ("input_controller_states", "Input contexts (`EControllerState`)", meta["st"], "input_controller_states"),
                                        ("animation_states", "Animation states (`AnimationStateGraph::AnimStates`)", meta["anim"], "animation_states")):
                sel = [r for r in enums.get(tab, []) if re.search(rx, r[1])]
                if sel:
                    md.append("**%s**" % label)
                    md.append("")
                    md.append(", ".join("`%s`=%s" % (r[1], r[0]) for r in sel))
                    md.append("")
            ct = [r for r in enums.get("input_control_types", []) if re.search(meta["ctl"], r[1])]
            if ct:
                md.append("**Controls CSV (`Controller::ControlType`)**: " + ", ".join("`%s`=%s" % (r[1], r[0]) for r in ct))
                md.append("")
            ab = [r for r in enums.get("audio_banks", []) if re.search(meta["bank"], r[1])]
            if ab:
                md.append("**Audio banks (AEMS)**: " + ", ".join("`%s`=%s" % (r[1], r[0]) for r in ab))
                md.append("")
            hud = [f for f in funcs_by_sub.get("frontend", []) if f[2].endswith("MinigameHandlers") and re.search(meta["hud"], f[3])]
            if hud:
                md.append("**HUD calls into the APT movie** (`MinigameHandlers`, native → menu):")
                md.append("")
                for f in hud:
                    md.append("- `%s(%s)` @`%s`" % (f[3], esc(f[4]), f[0]))
                md.append("")
        # ---- strings & globals
        strs = [(s, n) for s, n in xs[sub].most_common() if len(s) >= 4 and not re.fullmatch(r"[%.\d\s]+", s)]
        if strs:
            md.append("")
            md.append("## String literals referenced by this subsystem's code")
            md.append("")
            md.append("Asset paths, attribute names, tuning keys, sound events, debug-menu labels. (%d distinct; first 150 by frequency)" % len(strs))
            md.append("")
            for s, n in strs[:150]:
                md.append("- `%s`%s" % (s.replace("`", "'"), " ×%d" % n if n > 1 else ""))
        gl = sorted(xg[sub])
        if gl:
            md.append("")
            md.append("## Globals and class statics referenced")
            md.append("")
            md.append("| symbol | section | size | initial value in the ELF image |")
            md.append("|---|---|---:|---|")
            for g in gl[:250]:
                r = glob.get(g)
                if r:
                    md.append("| `%s` | %s | %s | %s |" % (esc(g), r[2], r[1], r[5]))
        cc = cg[sub]
        if cc:
            md.append("")
            md.append("## Direct calls out of this subsystem (by callee subsystem)")
            md.append("")
            md.append("| callee subsystem | call sites |")
            md.append("|---|---:|")
            for s, n in cc.most_common():
                md.append("| %s | %d |" % (s, n))
        fn = "%s-%s.md" % (tier, sub)
        open(os.path.join(OUT, fn), "w", encoding="utf-8", newline="\n").write("\n".join(md) + "\n")
        index_lines.append((tier, sub, len(su), len(L), total, fn, meta["title"] if meta else ""))
    with open(os.path.join(OUT, "README.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# Subsystem reference pages\n\nGenerated (`tools/gen_docs.py`). One page per subsystem of the game and engine layers.\n"
                "SDK, Havok, nw4r, APT runtime, Lua and other middleware are summarised in `data/middleware_units.tsv` only.\n\n"
                "| tier | subsystem | units | functions | code bytes | page |\n|---|---|---:|---:|---:|---|\n")
        for t, s, nu, nf, tb, fn, title in index_lines:
            f.write("| %s | %s%s | %d | %d | %s | [%s](%s) |\n" % (t, s, (" — " + title) if title else "", nu, nf, format(tb, ","), fn, fn))
    # ---- asset name tables -----------------------------------------------------------------------------
    keep = ["kAudioPaths", "kAEMSBankFilenames", "kMusicFilenames", "kMiniGameString", "ControlTypeFilenames",
            "RcCarModelFilenames", "RcCarModelTexturenames", "RcPowerupModelFilenames", "kDartShootoutBossIds",
            "sDartAssetNames", "DSObjectFilenames", "gDSTargetPinupFilenames", "kFreeThrowDatabaseNames",
            "kMicroBugHuntBugEffectsNames", "PAObstacleFilenames", "mMaterialStrings", "fixedResponses",
            "kOptionSpeedStringId", "USER_DIR_LIST"]
    tabs = collections.OrderedDict()
    for r in rows("string_tables.tsv"):
        tabs.setdefault(r[0], []).append(r)
    md = ["# Asset and name tables read from the executable",
          "",
          "> Generated by `tools/gen_docs.py` from `data/string_tables.tsv`. Each table is an array of string pointers in the ELF's data",
          "> sections; the array index is the value the code uses. These are the *names the game builds file paths and database keys from*,",
          "> so they connect the executable to the DATA folder. Some symbols exist in several translation units (identical copies).",
          ""]
    for k in keep:
        if k in tabs:
            L = tabs[k]
            names = [r[4] for r in L]
            for per in range(1, len(L)):  # collapse identical per-unit copies
                if len(L) % per == 0 and names == names[:per] * (len(L) // per):
                    L = L[:per]
                    break
            n = len(L)
            md.append("## `%s`  (table @`%s`, %d entries)" % (k, L[0][2], n))
            md.append("")
            md.append(", ".join("`%s`=%s" % (r[4] if r[4] else "\\0", r[1]) for r in L[:60]))
            md.append("")
    # every other table, compactly
    md.append("## Other pointer tables (see data/string_tables.tsv for entries)")
    md.append("")
    md.append("| symbol | entries |")
    md.append("|---|---:|")
    for k, L in tabs.items():
        if k not in keep and not re.search(r"keyboard|textinput|homebutton|FieldNames|Pane|Sgn|szc|Panes", k):
            md.append("| `%s` | %d |" % (esc(k), len(L)))
    open(os.path.join(HERE, "..", "docs", "asset-name-tables.md"), "w", encoding="utf-8", newline="\n").write("\n".join(md) + "\n")
    print("wrote", len(index_lines), "pages")


if __name__ == "__main__":
    main()
