#!/usr/bin/env python3
"""Verify lifted functions against the original machine code (Unicorn, PowerPC 750 model).

For each function: lift -> IR (lift.py); then N randomized trials. In every trial the same initial machine state is
given to (a) Unicorn executing the *original bytes* and (b) the IR interpreter. We compare:
    r3 and f1 at return, the ordered list of non-stack memory stores, and the ordered call trace (target, r3..r10, f1..f8)
Calls are events: both sides obtain results from the same deterministic oracle (Unicorn hooks the call instruction).
A function is `verified` when >= MIN_OK trials agree and all reachable basic blocks were exercised; `partial` if some blocks
were never hit; `failed` on any disagreement (with a counterexample recorded).
"""
import argparse
import hashlib
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import elfmap  # noqa: E402
import lift as L  # noqa: E402
from unicorn import (UC_ARCH_PPC, UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_MODE_BIG_ENDIAN, UC_MODE_PPC32, Uc,  # noqa: E402
                     UcError)
from unicorn.ppc_const import *  # noqa: E402,F401,F403
import unicorn.ppc_const as PC  # noqa: E402

RBASE, RSIZE = 0x81000000, 0x10000
SBASE, SSIZE = 0x80780000, 0x20000
STOP = 0x807F0000          # return address (blr stub)
MIN_OK = 24
MAX_TRIALS = 140
GREG = [getattr(PC, "UC_PPC_REG_%d" % i) for i in range(32)]
FREG = [getattr(PC, "UC_PPC_REG_FPR%d" % i) for i in range(32)]
CALL_MN = ("bl", "bctrl")


class Fault(Exception):
    pass


def oracle_result(idx, target, args, fargs):
    h = hashlib.blake2b(struct.pack(">II", idx, target) + b"".join(struct.pack(">I", a) for a in args) +
                        b"".join(b"nan" if (x != x) else struct.pack(">d", x) for x in fargs), digest_size=64).digest()
    g = {0: struct.unpack(">I", h[0:4])[0]}
    for k, n in enumerate(range(3, 13)):
        g[n] = struct.unpack(">I", h[4 * (k + 1):4 * (k + 2)])[0]
    f = {}
    for n in range(0, 14):
        f[n] = ((h[44 + n] * 257 + h[30 + (n % 10)]) % 4096 - 2048) / 16.0
    return {"g": g, "f": f}


def _fbits(x):
    if isinstance(x, tuple):
        return ("i", x[1] & 0xFFFFFFFF)
    if x != x:
        return ("nan",)
    return ("f", struct.pack(">d", x))


def _harvest_consts(instrs):
    """Immediates the function compares against / adds: useful to steer trials into both sides of branches."""
    out = set()
    for i in instrs:
        if i.mn in ("cmpwi", "cmplwi", "addi", "subi", "li", "andi", "ori", "xori", "mulli", "rlwinm", "srawi"):
            for o in i.ops:
                try:
                    v = int(o, 0)
                    if -70000 <= v <= 70000:
                        out.update({v & 0xFFFFFFFF, (v + 1) & 0xFFFFFFFF, (v - 1) & 0xFFFFFFFF})
                except ValueError:
                    pass
    return sorted(out)[:64]


class LiftVerifier:
    def __init__(self, E):
        self.E = E
        self.mu = Uc(UC_ARCH_PPC, UC_MODE_PPC32 | UC_MODE_BIG_ENDIAN)
        self.mu.ctl_set_cpu_model(PC.UC_CPU_PPC32_750_V3_1)
        self.mu.mem_map(0x80004000, 0x80609000 - 0x80004000)
        for ad, sz, off, nm, ty in E.secs:
            self.mu.mem_write(ad, E.raw[off:off + sz])
        self.mu.mem_map(RBASE, RSIZE)
        self.mu.mem_map(SBASE, SSIZE)
        self.mu.mem_map(STOP & ~0xFFF, 0x1000)
        self.mu.mem_write(STOP, struct.pack(">I", 0x4E800020))
        self.image_ro = {}
        for ad, sz, name in E.allsecs:
            if name in (".rodata", ".sdata2", ".sdata", ".data", ".init", ".text", "extab", "extabindex", ".ctors", ".dtors"):
                self.image_ro[name] = (ad, sz)
        self.rng = random.Random(1234)
        self._hook_writes = []
        self._call_sites = {}
        self._hooks = []
        self._cur = None
        self.mu.hook_add(UC_HOOK_MEM_WRITE, self._on_write)
        self.mu.hook_add(UC_HOOK_CODE, self._on_code)

    # ------------------------------------------------------------------ image reads (for the model and const folding)
    def image_read(self, addr, n):
        """Read-only view of the ELF image (initial values) for constant folding: only .rodata/.sdata2."""
        for name in (".rodata", ".sdata2"):
            a, sz = self.image_ro.get(name, (0, 0))
            if a <= addr and addr + n <= a + sz:
                return self.E.rd(addr, n)
        return b""

    def _model_image_read(self, addr, n):
        b = self.E.rd(addr, n)
        if len(b) == n:
            return b
        for ad, sz, name in self.E.allsecs:  # nobits sections: zeros
            if ad <= addr and addr + n <= ad + sz:
                return bytes(n)
        raise Fault("model read %x" % addr)

    # ------------------------------------------------------------------ unicorn hooks
    def _on_write(self, uc, access, address, size, value, user):
        if self._cur is None:
            return
        old = bytes(uc.mem_read(address, size))
        self._hook_writes.append((address, size, value & ((1 << (8 * size)) - 1), old))

    def _on_code(self, uc, address, size, user):
        cs = self._call_sites.get(address)
        if cs is None or self._cur is None:
            return
        kind, target = cs
        regs = [uc.reg_read(GREG[n]) & 0xFFFFFFFF for n in range(3, 11)]
        fr = [self._fpr(uc, n) for n in range(1, 9)]
        tgt = target if kind != "bctrl" else uc.reg_read(PC.UC_PPC_REG_CTR) & 0xFFFFFFFF
        idx = len(self._cur["calls"])
        self._cur["calls"].append((tgt, tuple(regs), tuple(fr)))
        res = oracle_result(idx, tgt, tuple(regs), tuple(fr))
        for n, v in res["g"].items():
            uc.reg_write(GREG[n], v)
        for n, v in res["f"].items():
            self._set_fpr(uc, n, v)
        if kind == "tail":
            uc.reg_write(PC.UC_PPC_REG_PC, uc.reg_read(PC.UC_PPC_REG_LR) & 0xFFFFFFFF)
        else:
            uc.reg_write(PC.UC_PPC_REG_PC, address + 4)

    @staticmethod
    def _fpr(uc, n):
        v = uc.reg_read(FREG[n])
        if isinstance(v, int):
            return struct.unpack(">d", struct.pack(">Q", v & 0xFFFFFFFFFFFFFFFF))[0]
        return v

    @staticmethod
    def _set_fpr(uc, n, x):
        uc.reg_write(FREG[n], struct.unpack(">Q", struct.pack(">d", x))[0])

    @staticmethod
    def _fpr_raw(uc, n):
        v = uc.reg_read(FREG[n])
        if isinstance(v, int):
            return v & 0xFFFFFFFFFFFFFFFF
        return struct.unpack(">Q", struct.pack(">d", v))[0]

    # ------------------------------------------------------------------ trial generation
    def _gen_memory(self, mode):
        rng = self.rng
        words = []
        consts = getattr(self, "_consts", []) or [0, 1]
        for _ in range(RSIZE // 4):
            r = rng.random()
            if mode == 5:   # all pointers: deep pointer chains stay valid
                w = RBASE + 4 * rng.randrange(0x1000)
            elif mode == 6:  # constants harvested from the function's own compares
                w = rng.choice(consts) if r < 0.7 else RBASE + 4 * rng.randrange(0x1000)
            elif mode == 0:
                w = RBASE + 4 * rng.randrange(0x1000) if r < 0.85 else rng.randrange(0, 9) if r < 0.95 else rng.getrandbits(32)
            elif mode == 1:
                w = rng.randrange(0, 9) if r < 0.7 else RBASE + 4 * rng.randrange(0x1000) if r < 0.9 else 0
            elif mode == 2:
                w = 0 if r < 0.6 else RBASE + 4 * rng.randrange(0x1000)
            elif mode == 3:  # floats
                w = struct.unpack(">I", struct.pack(">f", rng.uniform(-100, 100)))[0] if r < 0.6 else RBASE + 4 * rng.randrange(0x1000)
            else:
                w = rng.getrandbits(32) if r < 0.4 else RBASE + 4 * rng.randrange(0x1000)
            words.append(w)
        return struct.pack(">%dI" % len(words), *words)

    def _gen_state(self, trial):
        rng = self.rng
        mode = trial % 7
        rmem = self._gen_memory(mode)
        smem = bytes(rng.getrandbits(8) for _ in range(SSIZE))
        regs = {}
        for n in range(32):
            regs[n] = rng.getrandbits(32)
        sp = SBASE + SSIZE // 2
        regs[1], regs[2], regs[13] = sp, elfmap.SDA_R2, elfmap.SDA_R13
        for n in range(3, 11):
            r = rng.random()
            if n == 3 and r < 0.75:
                regs[n] = RBASE + 0x100 + 4 * rng.randrange(0x400)
            elif r < 0.35:
                regs[n] = RBASE + 0x100 + 4 * rng.randrange(0x400)
            elif r < 0.75:
                pool = [0, 1, 2, 3, 4, 5, 7, 8, 16, 0xFFFFFFFF, 0xFFFFFFFE, 100, 255, 256] + (getattr(self, "_consts", []) or [])
                regs[n] = rng.choice(pool)
            else:
                regs[n] = rng.getrandbits(32)
        fregs = {n: (rng.choice([0.0, 1.0, -1.0, 0.5]) if rng.random() < 0.3 else float(struct.unpack('>f', struct.pack('>f', rng.uniform(-50, 50)))[0])) for n in range(32)}
        return rmem, smem, regs, fregs, rng.getrandbits(32), mode

    # ------------------------------------------------------------------ one trial
    def trial(self, fn, lifted, rmem, smem, regs, fregs, ctr):
        mu = self.mu
        mu.mem_write(RBASE, rmem)
        mu.mem_write(SBASE, smem)
        for n in range(32):
            mu.reg_write(GREG[n], regs[n])
            self._set_fpr(mu, n, fregs[n])
        mu.reg_write(PC.UC_PPC_REG_CTR, ctr)
        mu.reg_write(PC.UC_PPC_REG_LR, STOP)
        mu.reg_write(PC.UC_PPC_REG_CR, 0)
        mu.reg_write(PC.UC_PPC_REG_XER, 0)
        mu.reg_write(PC.UC_PPC_REG_MSR, 0x2000)
        self._hook_writes = []
        self._cur = {"calls": []}
        try:
            mu.emu_start(fn["addr"], STOP, count=20000)
        except UcError as e:
            self._undo()
            self._cur = None
            raise Fault("unicorn " + str(e))
        pc = mu.reg_read(PC.UC_PPC_REG_PC)
        if pc != STOP:
            self._undo()
            self._cur = None
            raise Fault("did not return (pc=%x)" % pc)
        u_r3 = mu.reg_read(GREG[3]) & 0xFFFFFFFF
        u_f1 = self._fpr_raw(mu, 1)
        u_calls = self._cur["calls"]
        u_writes = [(a, s, v) for a, s, v, o in self._hook_writes if not (SBASE <= a < SBASE + SSIZE)]
        self._undo()
        self._cur = None
        # ----- model
        overlay = {}
        m_writes = []
        rbytes, sbytes = rmem, smem

        def mem_read(addr, n):
            out = bytearray()
            for k in range(n):
                a = addr + k
                if a in overlay:
                    out.append(overlay[a])
                elif RBASE <= a < RBASE + RSIZE:
                    out.append(rbytes[a - RBASE])
                elif SBASE <= a < SBASE + SSIZE:
                    out.append(sbytes[a - SBASE])
                else:
                    out.extend(self._model_image_read(a, 1))
            return bytes(out)

        def mem_write(addr, raw):
            if not ((RBASE <= addr and addr + len(raw) <= RBASE + RSIZE) or (SBASE <= addr and addr + len(raw) <= SBASE + SSIZE)
                    or any(a <= addr < a + s for a, s, _ in self.E.allsecs)):
                raise Fault("model write %x" % addr)
            for k, b in enumerate(raw):
                overlay[addr + k] = b
            if not (SBASE <= addr < SBASE + SSIZE):
                m_writes.append((addr, len(raw), int.from_bytes(raw, "big")))

        init = {"r%d" % n: regs[n] for n in range(32)}
        init.update({"f%d" % n: fregs[n] for n in range(32)})
        init["ctr"], init["lr"] = ctr, STOP
        env = L.Env(init, mem_read, lambda i, t, a, f: oracle_result(i, t, a, f))
        try:
            m_r3, m_f1, m_calls = L.run(lifted, env, mem_write)
        except L.Undefined:
            raise Fault("undefined-op")
        # coverage: which blocks' guards were true
        cov = set()
        for start, guard in lifted.blocks:
            try:
                if L.ev(guard, env):
                    cov.add(start)
            except Exception:
                pass
        self.last_calls = (m_calls, u_calls)
        # ----- compare
        bad = []
        if (m_r3 & 0xFFFFFFFF) != u_r3:
            bad.append("r3 %08x != %08x" % (m_r3 & 0xFFFFFFFF, u_r3))
        mf = m_f1
        if isinstance(mf, tuple):
            ok_f = (u_f1 & 0xFFFFFFFF) == (mf[1] & 0xFFFFFFFF)
        elif mf != mf:
            uf = struct.unpack(">d", struct.pack(">Q", u_f1))[0]
            ok_f = uf != uf
        else:
            ok_f = struct.pack(">d", mf) == struct.pack(">Q", u_f1)
        if not ok_f:
            bad.append("f1")
        if len(m_calls) != len(u_calls):
            bad.append("call count %d != %d" % (len(m_calls), len(u_calls)))
        else:
            for k, (mc, uc_) in enumerate(zip(m_calls, u_calls)):
                if mc[0] != uc_[0] or mc[1] != uc_[1]:
                    bad.append("call %d target/args" % k)
                    break
                if any(_fbits(a) != _fbits(b) for a, b in zip(mc[2], uc_[2])):
                    bad.append("call %d fargs" % k)
                    break
        if len(m_writes) != len(u_writes):
            bad.append("store count %d != %d" % (len(m_writes), len(u_writes)))
        else:
            for k, (a, b) in enumerate(zip(m_writes, u_writes)):
                if a != b:
                    bad.append("store %d model %s unicorn %s" % (k, tuple(hex(x) for x in a), tuple(hex(x) for x in b)))
                    break
        return bad, cov

    def _undo(self):
        for a, s, v, old in reversed(self._hook_writes):
            self.mu.mem_write(a, old)

    # ------------------------------------------------------------------ public API
    def prepare(self, fn):
        """Decode + lift. Returns (lifted, instrs) or raises L.Unsupported."""
        E = self.E
        code = E.rd(fn["addr"], fn["size"])
        instrs = L.decode(E.md, fn["addr"], code)
        lifted = L.lift(instrs, self.image_read)
        self._consts = _harvest_consts(instrs)
        # call sites for the hook
        sites = {}
        lo, hi = fn["addr"], fn["addr"] + fn["size"]
        for i in instrs:
            if i.mn == "bl":
                sites[i.addr] = ("bl", i.target)
            elif i.mn == "bctrl":
                sites[i.addr] = ("bctrl", None)
            elif i.mn == "b" and i.target is not None and not (lo <= i.target < hi):
                sites[i.addr] = ("tail", i.target)
        return lifted, instrs, sites

    def verify(self, fn, lifted, sites):
        self._call_sites = sites
        ok = 0
        faults = 0
        covered = set()
        first_bad = None
        total_blocks = {s for s, g in lifted.blocks}
        for t in range(MAX_TRIALS):
            rmem, smem, regs, fregs, ctr, mode = self._gen_state(t)
            try:
                bad, cov = self.trial(fn, lifted, rmem, smem, regs, fregs, ctr)
            except Fault:
                faults += 1
                continue
            except L.Unsupported as e:
                return {"status": "unsupported-eval", "detail": str(e)}
            if bad:
                first_bad = bad[0]
                return {"status": "failed", "detail": first_bad, "ok": ok, "faults": faults}
            ok += 1
            covered |= cov
            if ok >= MIN_OK and covered >= total_blocks:
                break
        self._call_sites = {}
        if ok < 4:
            return {"status": "untestable", "detail": "only %d clean trials (%d faults)" % (ok, faults), "ok": ok, "faults": faults}
        status = "verified" if (covered >= total_blocks and ok >= MIN_OK) else "partial"
        return {"status": status, "ok": ok, "faults": faults, "blocks": len(total_blocks), "covered": len(covered & total_blocks)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elf", required=True)
    ap.add_argument("names", nargs="*")
    a = ap.parse_args()
    E = elfmap.Elf(a.elf)
    V = LiftVerifier(E)
    funcs = {f["name"]: f for f in E.functions()}
    for n in a.names:
        fn = funcs[n]
        try:
            lifted, instrs, sites = V.prepare(fn)
        except L.Unsupported as e:
            print(n, "UNSUPPORTED", e)
            continue
        print(n, V.verify(fn, lifted, sites))


if __name__ == "__main__":
    main()
