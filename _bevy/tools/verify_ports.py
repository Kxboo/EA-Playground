#!/usr/bin/env python3
"""Verify every Rust port (src/mgvm/ports) against the original code on recorded game scenarios.

    python _bevy/tools/verify_ports.py [scenario ...]        (default: all scenarios below)

A port is only compared when it is entered outside another port's verification, so ports that run inside other ported
functions are checked in further rounds with EAGL_PORTS_ONLY limited to the ports not compared yet.
Writes GameMap/data/port_verification.tsv: addr, symbol, calls compared, mismatches, first difference (summed over runs).
Needs the lab binary: cargo build --profile lab --bin mglab
"""
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
BEVY = os.path.dirname(HERE)
ROOT = os.path.dirname(BEVY)
EXE = os.path.join(BEVY, "target", "lab", "mglab.exe" if os.name == "nt" else "mglab")
OUT = os.environ.get("PORT_VERIFY_OUT", os.path.join(ROOT, "GameMap", "data", "port_verification.tsv"))

SCENARIOS = {
    "wbug": ({"EAGL_MG_STICKERS": "24", "EAGL_MG_AREA": "600:11", "EAGL_MG_TP": "1300:47.3,-44",
              "EAGL_MG_PADS": "1360-1370:0800", "EAGL_MG_FRAMES": "3000"}, "99"),
    # Bug Hunt with net swings (BG_SwipeRegular: Y+ then X+, BG_SwipeRegularReverse: Y+ then X-) and the pause menu (Plus)
    "wbug_swing": ({"EAGL_MG_STICKERS": "24", "EAGL_MG_AREA": "600:11", "EAGL_MG_TP": "1300:47.3,-44",
                    "EAGL_MG_PADS": "1360-1370:0800,2850-2856:0010", "EAGL_MG_FRAMES": "3000",
                    "EAGL_MG_ACC": ";".join("%d-%d:512,900,616;%d-%d:%d,512,616" % (f, f + 3, f + 3, f + 6, 900 if (f // 60) % 2 == 1 else 124)
                                            for f in range(1500, 2800, 60))}, "99"),
    "mg8": ({"EAGL_MG_ALLAI": "1", "EAGL_MG_POSTGAME": "done", "EAGL_MG_FRAMES": "12000"}, "8"),
    "wdrib": ({"EAGL_MG_FRAMES": "1500", "EAGL_MG_TP": "500:9.6,-45.08", "EAGL_MG_PADS": "560-570:0800,600-1400:0010"}, "99"),
}


def registered():
    syms = []
    for path in glob.glob(os.path.join(BEVY, "src", "mgvm", "ports", "**", "*.rs"), recursive=True):
        text = open(path, encoding="utf-8").read()
        for m in re.finditer(r'\(\s*"([^"]+)"\s*,\s*[A-Za-z_][A-Za-z0-9_:]*\s*\)', text):
            syms.append(m.group(1).replace("\\\\", "\\"))
    return sorted(set(syms))


def run(name, env_extra, ty, only, work, rnd):
    report = os.path.join(work, "%s_round%d.tsv" % (name, rnd))
    env = dict(os.environ, EAGL_PORTS="verify", EAGL_PORT_REPORT=report, **env_extra)
    if only is not None:
        env["EAGL_PORTS_ONLY"] = ",".join(only)
    log = report[:-4] + ".log"
    with open(log, "w", encoding="utf-8", errors="replace") as fh:
        subprocess.run([work_exe(work), "probe", ty], env=env, stdout=fh, stderr=subprocess.STDOUT, cwd=work, check=False)
    rows = {}
    if os.path.exists(report):
        for line in open(report, encoding="utf-8"):
            if line.startswith("#") or not line.strip():
                continue
            a, sym, calls, bad, first = (line.rstrip("\n").split("\t") + [""])[:5]
            rows[sym] = (a, int(calls), int(bad), first)
    return rows


def work_exe(work):
    return os.path.join(work, os.path.basename(EXE))


def main():
    names = sys.argv[1:] or list(SCENARIOS)
    work = tempfile.mkdtemp(prefix="verify_ports_")
    shutil.copy(EXE, work_exe(work))  # a private copy: builds can replace the original meanwhile
    ports = registered()
    total = {}
    lock = threading.Lock()

    def scenario(name):
        env_extra, ty = SCENARIOS[name]
        pending, only = set(ports), None
        for rnd in range(1, 8):
            rows = run(name, env_extra, ty, only, work, rnd)
            fresh = {s: r for s, r in rows.items() if s in pending and r[1] > 0}
            with lock:
                for s, (a, calls, bad, first) in fresh.items():
                    c0, b0, f0 = total.get(s, (a, 0, 0, ""))[1:]
                    total[s] = (a, c0 + calls, b0 + bad, f0 or first)
                print("%s round %d: %d ports compared (%d with mismatches)" % (name, rnd, len(fresh), sum(1 for r in fresh.values() if r[2])), flush=True)
            pending -= set(fresh)
            if not fresh or not pending:
                break
            only = sorted(pending)

    threads = [threading.Thread(target=scenario, args=(n,)) for n in names]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("# addr\tsymbol\tcalls\tmismatches\tfirst_difference\n")
        for s, (a, calls, bad, first) in sorted(total.items(), key=lambda kv: kv[1][0]):
            fh.write("%s\t%s\t%d\t%d\t%s\n" % (a, s, calls, bad, first))
    ok = [s for s, r in total.items() if r[2] == 0]
    bad = [s for s, r in total.items() if r[2]]
    never = [s for s in ports if s not in total]
    print("\n%d ports: %d match the original, %d differ, %d never entered by the scenarios -> %s" % (len(ports), len(ok), len(bad), len(never), OUT))
    for s in bad:
        print("  DIFFERS  %s: %s" % (s, total[s][3][:300]))
    for s in never:
        print("  NOT RUN  %s" % s)
    shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
