import capstone

ELF_PATH = "playgroundz.elf"

# .text: addr 0x80013d00, file offset 0x0fee0, size 0x4091c4
TEXT_ADDR = 0x80013d00
TEXT_OFF = 0x0fee0
TEXT_SIZE = 0x4091c4

# .rodata: addr 0x8041d780, off 0x419980, size 0x25550
RODATA_ADDR = 0x8041d780
RODATA_OFF = 0x419980
RODATA_SIZE = 0x25550

# .sdata2: addr 0x80602540, off 0x4f1080, size 0x5914
SDATA2_ADDR = 0x80602540
SDATA2_OFF = 0x4f1080
SDATA2_SIZE = 0x5914

# .sdata: addr 0x805fbee0, off 0x4ed100, size 0x3f74
SDATA_ADDR = 0x805fbee0
SDATA_OFF = 0x4ed100
SDATA_SIZE = 0x3f74

# .data: addr 0x80442ce0 off 0x43eee0 size 0xae200
DATA_ADDR = 0x80442ce0
DATA_OFF = 0x43eee0
DATA_SIZE = 0xae200

with open(ELF_PATH, "rb") as f:
    ELF = f.read()


def addr_to_off(addr):
    if TEXT_ADDR <= addr < TEXT_ADDR + TEXT_SIZE:
        return TEXT_OFF + (addr - TEXT_ADDR)
    if RODATA_ADDR <= addr < RODATA_ADDR + RODATA_SIZE:
        return RODATA_OFF + (addr - RODATA_ADDR)
    if SDATA2_ADDR <= addr < SDATA2_ADDR + SDATA2_SIZE:
        return SDATA2_OFF + (addr - SDATA2_ADDR)
    if SDATA_ADDR <= addr < SDATA_ADDR + SDATA_SIZE:
        return SDATA_OFF + (addr - SDATA_ADDR)
    if DATA_ADDR <= addr < DATA_ADDR + DATA_SIZE:
        return DATA_OFF + (addr - DATA_ADDR)
    return None


def read_bytes(addr, size):
    off = addr_to_off(addr)
    if off is None:
        raise ValueError(f"addr {hex(addr)} not in known section")
    return ELF[off:off + size]


def disasm(addr, size, comment_map=None):
    md = capstone.Cs(capstone.CS_ARCH_PPC, capstone.CS_MODE_BIG_ENDIAN + capstone.CS_MODE_32 + capstone.CS_MODE_PS)
    md.detail = False
    code = read_bytes(addr, size)
    lines = []
    for insn in md.disasm(code, addr):
        line = f"{insn.address:08x}:  {insn.mnemonic} {insn.op_str}"
        lines.append(line)
    return "\n".join(lines)


if __name__ == "__main__":
    import sys
    addr = int(sys.argv[1], 16)
    size = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x200
    print(disasm(addr, size))
