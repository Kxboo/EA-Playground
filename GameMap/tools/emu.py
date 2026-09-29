"""Tiny Unicorn harness to call functions of the original PowerPC image (for verification only).

    from emu import Emu
    emu = Emu(elfmap.Elf(path))
    r3, r4 = emu.call("StartFreePlay__15MultiplayerModeFv", obj_addr)
    emu.read(addr, n); emu.write(addr, data)

Only pure, self-contained game functions should be called (no OS/GX/DVD): the image is loaded
but no hardware is emulated and calls into unmapped services will fault.
"""
import struct

from unicorn import UC_ARCH_PPC, UC_HOOK_CODE, UC_MODE_BIG_ENDIAN, UC_MODE_PPC32, Uc
from unicorn.ppc_const import (UC_PPC_REG_1, UC_PPC_REG_3, UC_PPC_REG_4, UC_PPC_REG_5, UC_PPC_REG_6,
                               UC_PPC_REG_7, UC_PPC_REG_8, UC_PPC_REG_9, UC_PPC_REG_10, UC_PPC_REG_13,
                               UC_PPC_REG_2, UC_PPC_REG_LR, UC_PPC_REG_PC)

STACK, HEAP, RET = 0x80700000, 0x80740000, 0x80780000
_ARGS = [UC_PPC_REG_3, UC_PPC_REG_4, UC_PPC_REG_5, UC_PPC_REG_6, UC_PPC_REG_7, UC_PPC_REG_8, UC_PPC_REG_9, UC_PPC_REG_10]


class Emu:
    def __init__(self, E):
        import elfmap
        self.E = E
        self.mu = Uc(UC_ARCH_PPC, UC_MODE_PPC32 | UC_MODE_BIG_ENDIAN)
        self.mu.mem_map(0x80004000, 0x80608000 - 0x80004000 + 0x1000)  # image incl. .bss
        for ad, sz, off, nm, _ in E.secs:
            self.mu.mem_write(ad, E.raw[off:off + sz])
        for base in (STACK, HEAP, RET):
            self.mu.mem_map(base, 0x20000)
        self.mu.mem_write(RET, struct.pack(">I", 0x4E800020))  # blr
        self.mu.reg_write(UC_PPC_REG_2, elfmap.SDA_R2)
        self.mu.reg_write(UC_PPC_REG_13, elfmap.SDA_R13)
        self.heap_top = HEAP

    def stub(self, fn, ret=0):
        """Replace a function by `return ret` (r3), so a caller can be run without its UI/hardware callees."""
        addr = self.E.by_name[fn][0] if isinstance(fn, str) else fn

        def hook(uc, address, size, user):
            uc.reg_write(UC_PPC_REG_3, ret & 0xFFFFFFFF)
            uc.reg_write(UC_PPC_REG_PC, uc.reg_read(UC_PPC_REG_LR))
        self.mu.hook_add(UC_HOOK_CODE, hook, begin=addr, end=addr)

    def alloc(self, n, fill=0):
        a = self.heap_top
        self.heap_top += (n + 31) & ~31
        self.mu.mem_write(a, bytes([fill]) * n)
        return a

    def write(self, addr, data):
        self.mu.mem_write(addr, bytes(data))

    def read(self, addr, n):
        return bytes(self.mu.mem_read(addr, n))

    def call(self, fn, *args):
        addr = self.E.by_name[fn][0] if isinstance(fn, str) else fn
        self.mu.reg_write(UC_PPC_REG_1, STACK + 0x18000)
        for r, v in zip(_ARGS, args):
            self.mu.reg_write(r, v & 0xFFFFFFFF)
        self.mu.reg_write(UC_PPC_REG_LR, RET)
        self.mu.emu_start(addr, RET, count=2_000_000)
        return self.mu.reg_read(UC_PPC_REG_3), self.mu.reg_read(UC_PPC_REG_4)
