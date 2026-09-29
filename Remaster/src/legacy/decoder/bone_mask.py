"""
bone_mask.py
------------
Reads the engine's runtime `sBoneMask` table straight out of
`playgroundz.elf` and exposes it as a per-bone translation gate.

WHY THIS EXISTS:
Decoding an .anm channel is not the same as the engine applying it.
FnStatelessF3 (and friends) will happily decode a translation value for
ANY bone index present in its channel table -- but the retail engine
consults `sBoneMask` (EvalSQTMask, disassembly ref 0x803fdfc8, mask
table at ELF vaddr 0x804e9ea6) and skips writing translation for any
bone whose mask byte is 0. Two of the three previously-assumed masked
bones (l_SideCheek, r_SideCheek) verified zero on-disk in this ELF;
the third (pelvis) is NOT part of the on-disk mask in this build --
see `load_translation_mask.__doc__` for the raw bytes this was derived
from. Re-verify against a given playgroundz.elf rather than trusting
any hardcoded assumption carried over from a prior session/build.

NOTE ON ENDIANNESS: playgroundz.elf is a genuine big-endian PowerPC
(Gekko/Broadway) ELF32 -- this is DIFFERENT from the little-endian
pseudo-ELF container format used by player_anims.anm / player_skel.ske,
which is why this module does its own section-table walk in '>' rather
than reusing decoder.eagl_anm_decoder._read_sections (that helper is
'<'-endian and is only valid for the .anm/.ske container format).
"""

import struct

from .paths import ELF_PATH

# Confirmed via disassembly of EvalSQTMask (0x803fdfc8): the mask table
# is a flat array of one byte per bone (NOT a packed bitmask), one entry
# per skeleton bone, non-zero == translation is applied by the engine,
# zero == translation write is skipped and the bone keeps its existing
# transform. Re-derive the file offset per-ELF instead of hardcoding it,
# since it depends on where the .data section lands in a given build.
BONE_MASK_VADDR = 0x804E9EA6
BONE_MASK_LEN = 68  # one byte per bone; matches the 68-bone skeleton


def _read_be_sections(data: bytes):
    """Minimal big-endian ELF32 section-header walk for the real game
    executable (NOT the .anm/.ske pseudo-ELF container, which is little-
    endian and handled by decoder.eagl_anm_decoder._read_sections)."""
    e_shoff = struct.unpack_from(">I", data, 32)[0]
    e_shentsize = struct.unpack_from(">H", data, 46)[0]
    e_shnum = struct.unpack_from(">H", data, 48)[0]
    e_shstrndx = struct.unpack_from(">H", data, 50)[0]

    sections = []
    for i in range(e_shnum):
        o = e_shoff + i * e_shentsize
        sh = struct.unpack_from(">IIIIIIIIII", data, o)
        sections.append({
            "name_idx": sh[0], "type": sh[1], "flags": sh[2],
            "addr": sh[3], "offset": sh[4], "size": sh[5],
        })

    shstrtab = sections[e_shstrndx]
    base = shstrtab["offset"]
    for s in sections:
        end = data.index(b"\x00", base + s["name_idx"])
        s["name"] = data[base + s["name_idx"]:end].decode("ascii", errors="replace")
    return sections


def _vaddr_to_file_offset(sections, vaddr: int):
    for s in sections:
        if s["addr"] and s["addr"] <= vaddr < s["addr"] + s["size"]:
            return s["offset"] + (vaddr - s["addr"])
    return None


def load_translation_mask(elf_path=None, n_bones=None):
    """Return a list[bool] of length `n_bones` (defaults to
    BONE_MASK_LEN): True == engine applies translation for this bone,
    False == engine skips the translation write (bone keeps whatever
    transform it already has -- do NOT substitute a zero translation,
    that is a different, wrong statement: "move to bind pose").

    Raises FileNotFoundError / ValueError if the table can't be located,
    rather than silently falling back to "everything unmasked" -- a
    caller that can't verify the real mask should know that, not quietly
    export unmasked data.
    """
    path = elf_path or ELF_PATH
    with open(path, "rb") as f:
        data = f.read()

    if data[5] != 2:  # EI_DATA: 2 == ELFDATA2MSB (big-endian)
        raise ValueError(
            f"{path}: expected a big-endian ELF (EI_DATA=2), got {data[5]!r} -- "
            "wrong file, or this build's executable isn't the Gekko/Broadway ELF."
        )

    sections = _read_be_sections(data)
    file_off = _vaddr_to_file_offset(sections, BONE_MASK_VADDR)
    if file_off is None:
        raise ValueError(
            f"{path}: BONE_MASK_VADDR 0x{BONE_MASK_VADDR:x} not inside any "
            "section -- table address may differ in this build; re-derive "
            "from a fresh disassembly of EvalSQTMask before trusting this."
        )

    length = n_bones or BONE_MASK_LEN
    raw = data[file_off:file_off + length]
    if len(raw) < length:
        raise ValueError(f"{path}: truncated read for bone mask table at 0x{file_off:x}")

    return [b != 0 for b in raw]


if __name__ == "__main__":
    mask = load_translation_mask()
    masked_out = [i for i, ok in enumerate(mask) if not ok]
    print(f"{len(mask)} bones, {len(masked_out)} masked out of translation export:")
    print(masked_out)
