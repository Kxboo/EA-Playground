#!/usr/bin/env python3
"""Validate controls*.csv files against the enums recovered from the executable (data/enums).

    python3 GameMap/tools/check_controls_csv.py path/to/controls*.csv [--dump]

Fields are looked up **by header name** (cCSVParser::GetStringField compares the header strings) and have
leading whitespace trimmed, so column order and stray leading spaces are harmless in the original.
Each token is resolved exactly the way Controller::Initialize does it (ConvertStringTo* functions):
unknown action events -> 190, unknown states -> 31, unknown button events -> 9, and unknown buttons -> 0
(see data/enums/*.tsv 'default/invalid' notes and the parser disassembly). Reports rows that fall back.
"""
import csv, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
D = os.path.join(HERE, "..", "data", "enums")


def load(name):
    m = {}
    for l in open(os.path.join(D, name), encoding="utf-8"):
        if l.startswith("#") or not l.strip():
            continue
        v, t = l.rstrip("\n").split("\t")[:2]
        m[t] = int(v)
    return m


EV, ST, BE, BT = (load("input_action_events.tsv"), load("input_controller_states.tsv"),
                  load("input_button_event_types.tsv"), load("input_buttons.tsv"))
INVALID = {"event": 190, "state": 31, "kind": 9, "button": None}


def resolve(tok, table, kind):
    if tok in table:
        return table[tok], True
    return INVALID[kind], False


def check(path, dump=False):
    rows = list(csv.reader(open(path, newline="", encoding="utf-8-sig")))
    hdr = rows[0]
    print("== %s   columns=%s" % (os.path.basename(path), ",".join(hdr)))
    issues = 0
    n = 0
    for i, r in enumerate(rows[1:], start=2):
        if not r or all(not c.strip() for c in r):
            continue
        n += 1
        # cCSVParser::GetString trims *leading* whitespace of every field (TrimWhiteSpaceLeft @0x802f32d8)
        r = [c.lstrip() for c in r]
        d = dict(zip([h.strip() for h in hdr], r))
        probs = []
        for col, tab, kind in (("ACTION_EVENT", EV, "event"), ("CONTROLLER_STATE", ST, "state"),
                               ("CONTROLLER_STATE_TRANSITION", ST, "state"), ("CONTROLLER_EVENT", BE, "kind")):
            tok = d.get(col, "")
            if col == "CONTROLLER_STATE_TRANSITION" and tok == "":
                continue  # empty = no transition; the game stores the converter's fallback? (see doc 03)
            v, ok = resolve(tok, tab, kind)
            if not ok:
                probs.append("%s=%r not recognised -> %s" % (col, tok, v))
        for col in ("MOD1", "MOD2", "BUTTON"):
            tok = d.get(col, "")
            if tok == "":
                if col == "BUTTON":
                    probs.append("BUTTON empty")
                continue
            if tok not in BT:
                probs.append("%s=%r not recognised" % (col, tok))
        if probs:
            issues += 1
            print("  line %d: %s   [%s]" % (i, "; ".join(probs), ",".join(r)))
        elif dump:
            print("  line %d: %s" % (i, ",".join(r)))
    print("   %d rows, %d with tokens the game would not recognise" % (n, issues))
    return n, issues


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    tot = [0, 0]
    for p in args:
        a, b = check(p, "--dump" in sys.argv)
        tot[0] += a; tot[1] += b
    print("TOTAL %d rows, %d flagged" % tuple(tot))
