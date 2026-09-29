#!/usr/bin/env python3
"""Generate the GameMap reference tables from the EA Playground ELF (read-only).

    python3 GameMap/tools/gen_map.py --elf Remaster/reference/playgroundz.elf

Outputs (under GameMap/data, all plain TSV/JSON so they diff and grep well):
    source_units.tsv        every compilation unit in link order with address range and subsystem
    functions/<tier>-<subsystem>.tsv   all game+engine functions (demangled)
    middleware_units.tsv    per-unit summary of SDK/Havok/APT/Lua/etc. (not itemised)
    classes.tsv             game+engine classes, their units and method counts
    vtables.tsv             virtual-method tables (slot -> implementing function)
    callgraph.tsv           caller_addr -> callee_addr for game+engine callers
    xref_strings.tsv        game+engine function -> string literals it references
    xref_globals.tsv        game+engine function -> named globals it references
    globals.tsv             referenced globals with size, section and initial value
    enums/*.tsv             enums recovered exactly from the executable's own parser code
    elf_facts.json          hashes, sections, entry, symbol counts
Nothing here interprets game *behaviour*; see GameMap/docs for that and its evidence levels.
"""
import argparse
import collections
import hashlib
import json
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import elfmap  # noqa: E402
import mwdemangle  # noqa: E402

DATA_LO, DATA_HI = 0x8041CEE0, 0x80608000


def w(path, header, rows):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", newline="\n") as f:
        f.write("# " + header + "\n")
        for r in rows:
            f.write("\t".join(str(c) for c in r) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    ap.add_argument("--out", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data"))
    ap.add_argument("--skip-xrefs", action="store_true", help="skip the slow disassembly pass")
    a = ap.parse_args()
    out = os.path.abspath(a.out)
    E = elfmap.Elf(a.elf)
    sha = hashlib.sha256(E.raw).hexdigest()

    # ---- units --------------------------------------------------------------------------
    funcs = E.functions()
    per_unit = collections.defaultdict(lambda: [0, 0])
    for f in funcs:
        if f["file"]:
            per_unit[f["file"] + "@%x" % E.unit_of(f["addr"])["start"]][0] += 1
    rows = []
    for u in E.units:
        n = sum(1 for f in funcs if u["start"] <= f["addr"] < u["end"])
        rows.append(("%08x" % u["start"], "%08x" % u["end"], u["end"] - u["start"], n, u["tier"], u["subsystem"], u["file"]))
    w(out + "/source_units.tsv", "start\tend\tbytes\tfuncs\ttier\tsubsystem\tsource_unit (STT_FILE name, link order)", rows)

    mw = collections.defaultdict(lambda: [0, 0, 0])
    for r in rows:
        if r[4] == "middleware":
            m = mw[r[5]]
            m[0] += 1; m[1] += r[2]; m[2] += r[3]
    w(out + "/middleware_units.tsv", "subsystem\tunits\tcode_bytes\tfuncs (SDK/middleware, deliberately not itemised)",
      [(k, *v) for k, v in sorted(mw.items(), key=lambda kv: -kv[1][1])])

    # ---- functions (game + engine) ------------------------------------------------------------
    mine = [f for f in funcs if f["tier"] in ("game", "engine")]
    by_sub = collections.defaultdict(list)
    for f in mine:
        by_sub[(f["tier"], f["subsystem"])].append(f)
    for (tier, sub), L in by_sub.items():
        L.sort(key=lambda f: f["addr"])
        w(out + "/functions/%s-%s.tsv" % (tier, sub),
          "addr\tsize\tclass\tmethod\targs\tconst\tsource_unit\tmangled",
          [("%08x" % f["addr"], f["size"], f["cls"], f["method"], f["args"], "const" if f["const"] else "",
            f["file"], f["name"]) for f in L])

    classes = collections.defaultdict(lambda: {"units": set(), "n": 0, "bytes": 0, "sub": set(), "ctor": "", "dtor": ""})
    for f in mine:
        if not f["cls"]:
            continue
        c = classes[f["cls"]]
        c["units"].add(f["file"]); c["n"] += 1; c["bytes"] += f["size"]; c["sub"].add(f["subsystem"])
        if f["method"] == "constructor" and not c["ctor"]:
            c["ctor"] = "%08x" % f["addr"]
        if f["method"] == "destructor" and not c["dtor"]:
            c["dtor"] = "%08x" % f["addr"]
    w(out + "/classes.tsv", "class\tsubsystem\tmethods\tcode_bytes\tconstructor\tdestructor\tsource_units",
      [(k, ",".join(sorted(v["sub"])), v["n"], v["bytes"], v["ctor"], v["dtor"], ",".join(sorted(v["units"])))
       for k, v in sorted(classes.items())])

    # ---- vtables ------------------------------------------------------------------------------
    vrows = []
    for addr, sz, t, name, b in E.syms:
        if t == "OBJECT" and name.startswith("__vt__") and sz >= 12 and sz % 12 == 0:
            cls = mwdemangle.demangle("x" + name[4:] + "Fv" if False else name)["cls"] or name[6:]
            # __vt__<len>Name : demangle the tail as a class
            try:
                cls = mwdemangle._qual(mwdemangle._P(name[6:]))
            except Exception:
                cls = name[6:]
            for i in range(0, sz, 12):
                fn = E.u32(addr + i + 8)
                if not fn:
                    continue
                s = E.sym_at(fn)
                if not s or "+" in s:
                    continue
                d = mwdemangle.demangle(s)
                vrows.append((cls, i // 12, "%08x" % fn, mwdemangle.pretty(s), d["cls"], "%08x" % addr))
    w(out + "/vtables.tsv",
      "class\tslot\tfunc_addr\tfunc\timplemented_in\tvtable_addr (12-byte records; function pointer is word 3; words 1-2 unresolved)",
      vrows)

    # ---- enums recovered from string-parser code --------------------------------------------
    enum_specs = [
        ("input_action_events", "ConvertStringToActionEvent__F7CString", "controls*.csv column: EActionEvent"),
        ("input_controller_states", "ConvertStringToControllerState__F7CString", "controls*.csv column: EControllerState"),
        ("input_button_event_types", "ConvertStringToControllerEvent__F7CString", "controls*.csv column: button event kind"),
        ("input_buttons", "ConvertStringToButton__F7CString", "controls*.csv column: button (leading '~' variants map to the same value)"),
    ]
    for fname, sym, desc in enum_specs:
        if sym not in E.by_name:
            continue
        rows_e = E.string_enum(sym)
        rows_e.sort(key=lambda r: (r["value"] is None, r["value"] if r["value"] is not None else 0, r["token"] or ""))
        w(out + "/enums/%s.tsv" % fname, "value\ttoken\tnotes  # %s ; parser %s @%08x ; recovered from the code, not string order" %
          (desc, sym, E.by_name[sym][0]),
          [(r["value"], r["token"], ("default/invalid=%s" % r["fallback"]) if r["fallback"] is not None else "") for r in rows_e])

    # ---- enums that are defined by an ordered name table (array index == enum value) -------------
    table_enums = [
        ("animation_states", "sAnimStateNames__19AnimationStateGraph", "AnimationStateGraph::AnimStates (index into sAnimStateNames)"),
        ("input_control_types", "ControlTypeFilenames", "Controller::ControlType -> controls*.csv file"),
        ("audio_banks", "kAEMSBankFilenames", "AEMS audio bank id -> .abk file"),
        ("dartshootout_pinup_targets", "gDSTargetPinupFilenames", "DartShootout pin-up target model names"),
    ]
    st_rows = collections.defaultdict(list)
    for r in E.string_tables_rows():
        st_rows[r[0]].append(r)
    for fname, tbl, desc in table_enums:
        if tbl in st_rows:
            w(out + "/enums/%s.tsv" % fname, "value\ttoken\tnotes  # %s ; table %s @%s" % (desc, tbl, st_rows[tbl][0][2]),
              [(r[1], r[4], "") for r in st_rows[tbl]])
    mg_tables = [t for t in st_rows if t == "kMiniGameString"]
    if mg_tables:
        rows_m = st_rows["kMiniGameString"]
        n = len(rows_m) // 4 if len(rows_m) % 4 == 0 else len(rows_m)
        w(out + "/enums/minigame_names.tsv", "value\ttoken\tnotes  # kMiniGameString (4 identical copies in different units; first shown)",
          [(r[1], r[4], "") for r in rows_m[:n]])

    # ---- top-level GameState transitions ----------------------------------------------------------
    state_names = {0: "PG2FE", 1: "FE2PG", 2: "Boot2FE", 3: "FE2MP", 4: "MP2FE", 5: "FrontEnd", 6: "Playground", 7: "BootFlow"}
    jt = E.by_name.get("Update__9GameStateFv")
    sn_addr = E.by_name["SetNewState__9GameStateFRCQ29GameState5StatePCc"][0]
    tr_rows = []
    for f in mine:
        if not f["size"] or f["size"] > 30000:
            continue
        rows_f = list(E.disasm(f["addr"], f["size"]))
        for i, r in enumerate(rows_f):
            if r["mn"] == "bl" and r["target"] == sn_addr:
                rg = rows_f[i - 1]["regs"]
                val = E.u32(rg[3]) if rg.get(3) else None
                tr_rows.append(("%08x" % r["addr"], f["cls"] + "::" + f["method"] if f["cls"] else f["method"], val,
                                state_names.get(val, "?")))
    w(out + "/state_transitions.tsv", "call_site\tin_function\tstate_value\tstate_name  # GameState::SetNewState(const State&) ; names from the Update() jump table @0x804e885c",
      sorted(tr_rows))

    # ---- frontend state traces (state value -> calls made on that path) ------------------------
    import handlers as H
    for fn, lo, hi, tag in (("EnterFEGameState__9FEManagerF18eFrontEndGameState", -1, 24, "FEManager_EnterFEGameState"),
                            ("LeaveFEGameState__9FEManagerF18eFrontEndGameState", -1, 24, "FEManager_LeaveFEGameState"),
                            ("EnterState__9FEManagerF14eFrontendState", -1, 12, "FEManager_EnterState"),
                            ("LeaveState__9FEManagerF14eFrontendState", -1, 12, "FEManager_LeaveState")):
        if fn not in E.by_name:
            continue
        fa, fsz, _ = E.by_name[fn]
        frows = list(E.disasm(fa, fsz)); fby = {x["addr"]: x for x in frows}
        trows = []
        for K in range(lo, hi + 1):
            calls = [H._pretty_call(c) for c in H._trace(E, frows, fby, K)
                     if not c.startswith(("GetInstance__", "__save_", "__restore_", "Instance__5Audio"))]
            trows.append((K, " ; ".join(calls)))
        w(out + "/traces/%s.tsv" % tag, "state_value\tcalls on the traced path (emulated with the state in r4; string args in [..])  # %s @%08x" % (fn, fa), trows)

    # ---- front-end state names/transitions ------------------------------------------------------
    upd_tbl = 0x804d6e2c  # FEManager::Update jump table, index = eFrontEndGameState + 1
    fe_upd = {}
    for K in range(-1, 21):
        tgt = E.u32(upd_tbl + 4 * (K + 1))
        nm = None
        for x in E.disasm(tgt, 32):
            if x["mn"] == "bl" and x["target"]:
                nm = E.sym_at(x["target"])
                break
        fe_upd[K] = "" if tgt == 0x80324508 else (nm or "")
    setfe = E.by_name["SetFEGameState__9FEManagerF18eFrontEndGameState"][0]
    setst = E.by_name["SetState__9FEManagerF14eFrontendState"][0]
    ferows = []
    for f in mine:
        if not f["size"] or f["size"] > 30000:
            continue
        rws = list(E.disasm(f["addr"], f["size"]))
        for i, r in enumerate(rws):
            if r["mn"] == "bl" and r["target"] in (setfe, setst):
                rg = rws[i - 1]["regs"]
                val = rg.get(4)
                if val is not None and val >= 1 << 31:
                    val -= 1 << 32
                ferows.append(("SetFEGameState" if r["target"] == setfe else "SetState", "%08x" % r["addr"],
                               (f["cls"] + "::" if f["cls"] else "") + f["method"], val if val is not None else "?"))
    w(out + "/fe_state_transitions.tsv", "setter\tcall_site\tin_function\tvalue  # FEManager::SetFEGameState(eFrontEndGameState) / SetState(eFrontendState)",
      sorted(ferows, key=lambda r: (r[0], str(r[3]), r[1])))
    w(out + "/enums/fe_game_state_update_functions.tsv",
      "value\tper-frame update function (FEManager::Update jump table @%08x; empty = no per-frame work)" % upd_tbl,
      [(k, mwdemangle.pretty(v) if v else "") for k, v in fe_upd.items()])

    # ---- APT handler bindings -------------------------------------------------------------------
    hrows = H.all_rows(E)
    w(out + "/apt_handlers.tsv", "handler_class\tkind(FS=fscommand LV=loadVariables)\tjob_index\tapt_name\tnative_target(s) on that dispatch path\tdispatch_fn\tregistered_in",
      hrows)

    # ---- xref pass ----------------------------------------------------------------------------
    if not a.skip_xrefs:
        objs = [(s[0], s[1], s[3]) for s in E.syms if s[2] == "OBJECT"]
        oaddrs = [o[0] for o in objs]
        import bisect

        def obj_at(addr):
            i = bisect.bisect_right(oaddrs, addr) - 1
            if i >= 0 and objs[i][0] <= addr < objs[i][0] + max(objs[i][1], 1):
                return objs[i][2]
            return None
        callg, xstr, xglob = [], [], []
        gl_seen = {}
        strs_seen = {}
        for n, f in enumerate(sorted(mine, key=lambda f: f["addr"])):
            if f["size"] == 0 or f["size"] > 200000:
                continue
            seen_c, seen_s, seen_g = set(), set(), set()
            for r in E.disasm(f["addr"], f["size"]):
                if r["target"] is not None and r["mn"] == "bl" and r["target"] not in seen_c:
                    seen_c.add(r["target"]); callg.append((f["addr"], r["target"]))
                cand = None
                if r["const"] is not None and r["mn"] in ("addi", "ori", "subi"):
                    cand = r["const"]
                else:
                    m = re.search(r"(-?\w+)\(r(\d+)\)$", r["op"])
                    if m and r["mn"][0] in ("l", "s") and not r["mn"].startswith(("lis", "li")):
                        base = r["regs"].get(int(m.group(2)))
                        try:
                            off = int(m.group(1), 0)
                        except ValueError:
                            off = None
                        if base is not None and off is not None and int(m.group(2)) in (2, 13):
                            cand = (base + off) & 0xFFFFFFFF
                if cand is None or not (DATA_LO <= cand < DATA_HI):
                    continue
                sec = E.section_of(cand)
                s = None
                if sec in (".rodata", ".sdata", ".data", ".sdata2"):
                    prev = E.rd(cand - 1, 1)
                    if prev in (b"\0", b""):  # a literal must start right after a terminator
                        s = E.cstr(cand)
                if s and len(s) >= 3:
                    o = obj_at(cand)
                    # literal (anonymous or '@n' symbol) or inside a big string blob; not a named small global
                    if o is None or o.startswith("@") or E.sym_at(cand) != o:
                        if cand not in seen_s:
                            seen_s.add(cand); xstr.append((f["addr"], cand, s)); strs_seen[cand] = s
                        continue
                o = obj_at(cand)
                if o and o not in seen_g and not o.startswith(("@", "__vt__", "__RTTI")):
                    seen_g.add(o); xglob.append((f["addr"], o)); gl_seen[o] = 1
        w(out + "/callgraph.tsv", "caller_addr\tcallee_addr (direct bl only; virtual/indirect calls not resolved)",
          [("%08x" % c, "%08x" % t) for c, t in callg])
        w(out + "/xref_strings.tsv", "func_addr\tstring_addr\ttext",
          [("%08x" % fa, "%08x" % sa, s.replace("\t", "\\t").replace("\n", "\\n")[:160]) for fa, sa, s in xstr])
        w(out + "/xref_globals.tsv", "func_addr\tglobal_symbol (accessed via r13/r2 small-data or lis/addi)",
          [("%08x" % fa, g) for fa, g in xglob])
        grows = []
        for name in sorted(gl_seen):
            ad, sz, t = E.by_name[name]
            sec = E.section_of(ad) or "?"
            init = ""
            if sec in (".sdata", ".sdata2", ".data", ".rodata") and sz in (1, 2, 4, 8):
                b = E.rd(ad, sz)
                if len(b) == sz:
                    iv = int.from_bytes(b, "big")
                    init = "0x%x" % iv
                    if sz == 4:
                        fv = struct.unpack(">f", b)[0]
                        if fv == fv and 1e-6 < abs(fv) < 1e7:
                            init += " (float %.6g)" % fv
                    if sz == 8:
                        dv = struct.unpack(">d", b)[0]
                        if dv == dv and 1e-6 < abs(dv) < 1e9:
                            init += " (double %.10g)" % dv
            d = mwdemangle.demangle(name)
            grows.append(("%08x" % ad, sz, sec, name, d["cls"], init))
        w(out + "/globals.tsv", "addr\tsize\tsection\tsymbol\tclass\tinitial_value (bytes in the ELF image; .bss has none)", grows)

    # ---- pointer tables (arrays of string pointers in .data/.rodata/.sdata) -------------------
    w(out + "/string_tables.tsv", "table_symbol\tindex\ttable_addr\tstring_ptr\tstring (arrays where >=60% of words point to text)",
      E.string_tables_rows())

    # ---- facts ---------------------------------------------------------------------------------
    facts = {
        "elf_sha256": sha,
        "entry": "0x%08x" % E.elf["e_entry"],
        "sections": [{"name": n, "addr": "0x%08x" % ad, "size": sz} for ad, sz, n in E.allsecs],
        "symbol_counts": {"functions": sum(1 for s in E.syms if s[2] == "FUNC"),
                          "objects": sum(1 for s in E.syms if s[2] == "OBJECT")},
        "units": len(E.units),
        "tiers": {t: sum(1 for u in E.units if u["tier"] == t) for t in ("game", "engine", "middleware")},
        "sda": {"r2": "0x%08x" % elfmap.SDA_R2, "r13": "0x%08x" % elfmap.SDA_R13},
    }
    with open(out + "/elf_facts.json", "w") as fh:
        json.dump(facts, fh, indent=2)
    print("done", out, sha)


if __name__ == "__main__":
    main()
