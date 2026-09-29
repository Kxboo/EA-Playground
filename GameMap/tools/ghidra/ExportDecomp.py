# Ghidra (Jython) post-script: decompile a list of functions and dump one JSON record per line.
#   analyzeHeadless <proj> <name> -process <program> -noanalysis -scriptPath <this dir> \
#       -postScript ExportDecomp.py <addr-list.txt> <out.jsonl>
# addr-list.txt: one hex address per line (first token). Output is *local* material: it contains decompiled C text and
# must not be published; GameMap/tools/ingest_ghidra.py keeps only metrics for the progress site.
import json
import time

from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

args = getScriptArgs()
infile, outfile = args[0], args[1]
addrs = []
for line in open(infile):
    line = line.strip()
    if line:
        addrs.append(int(line.split()[0], 16))

ifc = DecompInterface()
ifc.openProgram(currentProgram)
ifc.setSimplificationStyle("decompile")
mon = ConsoleTaskMonitor()
fm = currentProgram.getFunctionManager()
out = open(outfile, "w")
t0 = time.time()
n = 0
for a in addrs:
    addr = toAddr(a)
    f = fm.getFunctionAt(addr)
    rec = {"addr": a}
    if f is None:
        rec.update({"ok": False, "reason": "no-function"})
    else:
        rec["name"] = f.getName()
        rec["params"] = f.getParameterCount()
        rec["proto"] = f.getPrototypeString(False, False)
        rec["body"] = int(f.getBody().getNumAddresses())
        rec["thunk"] = bool(f.isThunk())
        try:
            res = ifc.decompileFunction(f, 90, mon)
            if res.decompileCompleted():
                code = res.getDecompiledFunction().getC()
                rec["ok"] = True
                rec["lines"] = code.count("\n")
                rec["code"] = code
                rec["bad"] = ("halt_baddata" in code) or ("Bad instruction" in code) or ("WARNING: Bad" in code)
                rec["warn"] = code.count("WARNING")
                rec["gotos"] = code.count("goto ")
                rec["loops"] = code.count("while(") + code.count("for(") + code.count("do {")
                rec["switch"] = code.count("switch(")
                rec["unaff"] = code.count("unaff_") + code.count("in_")
            else:
                rec.update({"ok": False, "reason": str(res.getErrorMessage())[:120]})
        except Exception as e:  # noqa
            rec.update({"ok": False, "reason": "exception " + str(e)[:100]})
    out.write(json.dumps(rec) + "\n")
    n += 1
    if n % 200 == 0:
        out.flush()
        print("decompiled %d/%d in %ds" % (n, len(addrs), time.time() - t0))
out.close()
print("done %d functions in %ds" % (n, time.time() - t0))
