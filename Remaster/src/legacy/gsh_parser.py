"""EA SHPG texture reader, corrected against the complete local DATA corpus.

Header: SHPG, u32LE file size, u32BE directory count, four-byte version.
Directory: four-byte short name and u32BE absolute record offset.
Image record: u8 format, u24BE block size (including 16-byte header),
u16BE width/height/centers and four flag bytes. The size high byte is NOT
a LOD flag. GX texture levels occupy whole tiles; trailing bytes do not
prove another mip level. Only the top mip is exported.

Indexed images have a separate palette record at image-record end:
0x31 RGB565, 0x32 RGB5A3, 0x33 two 32-byte-aligned AR/GB byte-pair planes.
A 0x70 record may hold a full name. Frontend GSHs often have short names only.
Earlier stub/LOD/orphan heuristics were removed: all 3,574 local images are
directory-declared and decode without repairing their boundaries.
See research/FINDINGS.md for evidence, sources and remaining limits.
"""
from __future__ import annotations

import argparse
import json
import struct
from dataclasses import dataclass, field, asdict
from pathlib import Path

# ---------------------------------------------------------------------------
# Optional numpy acceleration for the pixel-decode hot loops (_detile_c8,
# _detile_c4, _detile_gx_rgba8, _detile_gx_16bpp, decode_cmpr, decode_pal8,
# decode_pal4). Every vectorized path below was validated byte-for-byte
# against the original pure-Python loop it replaces (200+ randomized shapes,
# including truncated/malformed input) before being wired in here. numpy is
# optional: if it isn't installed, everything falls back to the original
# pure-Python implementations with no behavior change, just slower.
# ---------------------------------------------------------------------------
try:
    import numpy as _np
    _HAVE_NUMPY = True
except ImportError:
    _HAVE_NUMPY = False


def _ceildiv(a: int, b: int) -> int:
    return -(-a // b)

MAGIC = b"SHPG"
ENTRY_TAIL_MAGIC = bytes.fromhex("20002000")
PALETTE_RECORD_ID = 0x32     # record_id of the CLUT header following a PAL8/PAL4 block
# 0x31 is a second, distinct CLUT-header id (confirmed via reference tool
# dump: "Record ID: 49 | 0x31 | PALETTE 16-BIT 565") seen in world.gsh
# (e.g. entry 85 "lemonadestand256"). Same 16-byte header shape/tail as
# 0x32, but its 16-bit color entries are packed straight RGB565 (no
# alpha channel — always opaque) rather than 0x32's RGB5A3.
PALETTE_RECORD_ID_RGB565 = 0x31
PALETTE_RECORD_ID_RGB5A3 = 0x32
PALETTE_RECORD_IDS = (PALETTE_RECORD_ID_RGB565, PALETTE_RECORD_ID_RGB5A3, 0x33)
PALETTE_TAIL_MAGIC = bytes.fromhex("00000000")
NAME_RECORD_MAGIC = bytes.fromhex("70000000")   # marks the full-name string after an entry

# record_id -> (label, bits per pixel, block format)
RECORD_FORMATS = {
    20: ("RGB565", 16, "rgb565"),
    30: ("N64 CMPR", 4, "cmpr"),    # GameCube/N64-style DXT1 4x4 block compression
    25: ("PAL8",     8, "pal8"),    # 8-bit paletted, GX C8 tiling, 256-color CLUT
    24: ("PAL4",     4, "pal4"),    # 4-bit paletted, GX C4 tiling, <=16-color CLUT
    22: ("RGBA8",   32, "rgba8"),   # uncompressed 32bpp, GX RGBA8 4x4 tiling
    21: ("RGB5A3",  16, "rgb5a3"),  # uncompressed 16bpp, GX 4x4 tiling, translucent-capable
}

# Formats whose image block is followed by a PALETTE_RECORD_ID CLUT header.
PALETTED_RECORD_IDS = (24, 25)


@dataclass
class GshEntry:
    index: int
    name: str
    full_name: str | None
    entry_offset: int
    record_id: int
    format_label: str
    size_of_block: int
    width: int
    height: int
    center_x: int
    center_y: int
    img_offset: int
    img_size: int
    img_end: int
    tail_ok: bool
    mip_levels: list = field(default_factory=list)   # [(w,h,byte_len), ...]
    orphan: bool = False
    palette: list | None = None   # [(r,g,b,a), ...] CLUT for PAL8, if found
    reserved0: int = 0            # legacy name: high byte of the u24 block size
    lod_bias: int = 0             # compatibility field; always zero
    recovered: bool = False       # compatibility field; heuristic repair removed


@dataclass
class GshFile:
    path: str
    signature: str
    file_size: int
    object_count: int
    format_ver: str
    entries: list = field(default_factory=list)


def _mip_chain(width: int, height: int, total_bytes: int, bpp_num: int, bpp_den: int):
    """
    Greedily split total_bytes into a standard halving mip chain
    (level0 = width x height, level1 = w/2 x h/2, ...) using
    bytes_per_level = whole GX tiles at this bit depth, stopping once the chain
    accounts for all of total_bytes or dimensions hit 1x1.

    If the bytes remaining after the last full level don't add up to
    a complete next level, they're trailing padding/footer data (e.g.
    a PAL8 block padded out to a round size) — NOT a truncated mip —
    so they're simply left unaccounted for rather than recorded as a
    bogus partial level.
    """
    levels = []
    w, h = width, height
    remaining = total_bytes
    while remaining > 0 and w >= 1 and h >= 1:
        # Every GX level occupies whole tiles, including narrow UI images.
        bits = bpp_num * 8 // bpp_den
        tw, th = (8, 8) if bits == 4 else (8, 4) if bits == 8 else (4, 4)
        level_bytes = _ceildiv(w, tw) * _ceildiv(h, th) * tw * th * bpp_num // bpp_den
        if level_bytes > remaining:
            break
        levels.append((w, h, level_bytes))
        remaining -= level_bytes
        if w == 1 and h == 1:
            break
        w = max(1, w // 2)
        h = max(1, h // 2)
    return levels


def _try_read_palette(data: bytes, offset: int, max_entries: int = 256):
    """
    Look for a palette-flavoured 16-byte header at `offset` (expected to
    be a PAL8 entry's img_end) and, if found, decode its RGB565 colors.
    Returns a list of (r,g,b,a) tuples, or None if no plausible palette
    header is present there.
    """
    if offset + 16 > len(data):
        return None
    hdr = data[offset:offset + 16]
    record_id = hdr[0]
    if record_id not in PALETTE_RECORD_IDS:
        return None
    size_of_block = int.from_bytes(hdr[1:4], 'big')
    width  = struct.unpack_from(">H", hdr, 4)[0]
    height = struct.unpack_from(">H", hdr, 6)[0]
    tail = hdr[12:16]
    count = width * height
    if count <= 0 or count > max_entries:
        return None

    pal_off = offset + 16
    plane_stride = (count * 2 + 31) & ~31
    pal_size = plane_stride + count * 2 if record_id == 0x33 else count * 2
    declared_pal_size = size_of_block - 16
    # The declared block is sometimes padded out to a round byte boundary
    # (e.g. world.gsh "foot": 94-entry/188-byte CLUT padded to 192 bytes,
    # then the whole 16-byte-header+data block rounded up to 208 so the
    # total is a multiple of 16). The pad sits *after* the real colors as
    # zero bytes, not extra entries, so only require declared >= actual
    # instead of exact equality.
    if declared_pal_size < pal_size or pal_off + pal_size > len(data):
        return None

    pal_bytes = data[pal_off:pal_off + pal_size]
    if record_id == 0x33:
        # Two IA8 tables: AR pairs followed by GB pairs (EA's IA_X2_ARGB).
        # Format identification: EA Graphics Manager common.py / ReverseBox
        # ImageFormats. This mapping is independently implemented here.
        return [(pal_bytes[2*i+1], pal_bytes[plane_stride+2*i],
                 pal_bytes[plane_stride+2*i+1], pal_bytes[2*i]) for i in range(count)]
    decode_color = (
        _rgb565_to_rgba8888
        if record_id == PALETTE_RECORD_ID_RGB565
        else _rgb5a3_to_rgba8888
    )
    colors = []
    for i in range(count):
        v = struct.unpack_from(">H", pal_bytes, i * 2)[0]
        colors.append(decode_color(v))
    return colors


def _try_read_name(data: bytes, offset: int, max_len: int = 64) -> str | None:
    """
    Look for the full-name record at `offset`: a constant 4-byte marker
    (NAME_RECORD_MAGIC = 70 00 00 00) followed by a null-padded ASCII
    string. This sits immediately after every entry's image data (or,
    for paletted formats, immediately after the CLUT block) — confirmed
    across every entry in effects.gsh. Returns the decoded name, or None
    if the marker isn't present at `offset`.

    This is the real, full object name; the object-table `name` field is
    truncated to 4 characters by comparison (e.g. table name "ef_i" vs.
    full_name "ef_icstar_purpletest1").
    """
    if offset + 4 > len(data):
        return None
    if data[offset:offset + 4] != NAME_RECORD_MAGIC:
        return None
    start = offset + 4
    end = data.find(b"\x00", start, start + max_len)
    if end == -1:
        end = min(start + max_len, len(data))
    raw = data[start:end]
    if not raw or not all(32 <= b < 127 for b in raw):
        return None
    return raw.decode("ascii", errors="replace")


def _detile_c8(index_bytes: bytes, width: int, height: int,
                tile_w: int = 8, tile_h: int = 4) -> bytes:
    """
    Undo GameCube GX C8 (8-bit paletted) tiling: pixels are stored as
    8x4-texel tiles (32 bytes/tile), tiles walked left-to-right then
    top-to-bottom, and pixels within each tile walked the same way.
    Returns raw row-major index bytes, width*height long.
    """
    if _HAVE_NUMPY:
        n_tiles_x = _ceildiv(width, tile_w)
        n_tiles_y = _ceildiv(height, tile_h)
        needed = n_tiles_y * n_tiles_x * tile_h * tile_w
        buf = _np.frombuffer(index_bytes, dtype=_np.uint8)
        if buf.size < needed:
            buf = _np.concatenate([buf, _np.zeros(needed - buf.size, dtype=_np.uint8)])
        else:
            buf = buf[:needed]
        grid = buf.reshape(n_tiles_y, n_tiles_x, tile_h, tile_w)
        img = grid.transpose(0, 2, 1, 3).reshape(n_tiles_y * tile_h, n_tiles_x * tile_w)
        return img[:height, :width].tobytes()

    out = bytearray(width * height)
    pos = 0
    n = len(index_bytes)
    for ty in range(0, height, tile_h):
        for tx in range(0, width, tile_w):
            for by in range(tile_h):
                py = ty + by
                for bx in range(tile_w):
                    px = tx + bx
                    if pos < n and px < width and py < height:
                        out[py * width + px] = index_bytes[pos]
                    pos += 1
    return bytes(out)


def _detile_c4(index_bytes: bytes, width: int, height: int,
                tile_w: int = 8, tile_h: int = 8) -> bytes:
    """
    Undo GameCube GX C4 (4-bit paletted) tiling: pixels are stored as
    8x8-texel tiles (32 bytes/tile, 2 texels/byte — high nibble first),
    tiles walked left-to-right then top-to-bottom, and pixels within
    each tile walked the same way. Returns raw row-major index bytes
    (one byte per pixel, values 0-15), width*height long.
    """
    if _HAVE_NUMPY:
        n_tiles_x = _ceildiv(width, tile_w)
        n_tiles_y = _ceildiv(height, tile_h)
        bytes_per_tile = (tile_w * tile_h) // 2
        needed_bytes = n_tiles_y * n_tiles_x * bytes_per_tile
        buf = _np.frombuffer(index_bytes, dtype=_np.uint8)
        if buf.size < needed_bytes:
            buf = _np.concatenate([buf, _np.zeros(needed_bytes - buf.size, dtype=_np.uint8)])
        else:
            buf = buf[:needed_bytes]
        hi = (buf >> 4) & 0xF
        lo = buf & 0xF
        nibbles = _np.empty(buf.size * 2, dtype=_np.uint8)
        nibbles[0::2] = hi
        nibbles[1::2] = lo
        grid = nibbles.reshape(n_tiles_y, n_tiles_x, tile_h, tile_w)
        img = grid.transpose(0, 2, 1, 3).reshape(n_tiles_y * tile_h, n_tiles_x * tile_w)
        return img[:height, :width].tobytes()

    out = bytearray(width * height)
    pos = 0
    n = len(index_bytes)
    for ty in range(0, height, tile_h):
        for tx in range(0, width, tile_w):
            for by in range(tile_h):
                py = ty + by
                for bx in range(0, tile_w, 2):
                    if pos >= n:
                        pos += 1
                        continue
                    byte = index_bytes[pos]
                    pos += 1
                    px0, px1 = tx + bx, tx + bx + 1
                    if px0 < width and py < height:
                        out[py * width + px0] = (byte >> 4) & 0xF
                    if px1 < width and py < height:
                        out[py * width + px1] = byte & 0xF
    return bytes(out)


def _detile_gx_rgba8(block_data: bytes, width: int, height: int) -> bytes:
    """
    Undo GameCube GX RGBA8 tiling: 4x4-texel tiles, 64 bytes/tile, split
    into two 32-byte cache lines per tile — the first holds interleaved
    (A,R) byte pairs for all 16 texels (row-major within the tile), the
    second holds interleaved (G,B) byte pairs for the same 16 texels.
    Returns raw RGBA8 bytes, row-major, width*height*4 long.
    """
    n_tiles_x = _ceildiv(width, 4)
    n_tiles_y = _ceildiv(height, 4)
    needed = n_tiles_x * n_tiles_y * 64
    # The original loop `break`s at the first incomplete tile and leaves
    # everything after it zero -- that's not equivalent to zero-padding a
    # partial tile's leftover bytes, so the fast path only applies when the
    # data is complete. Truncated/malformed input (rare) falls back below.
    if _HAVE_NUMPY and len(block_data) >= needed:
        buf = _np.frombuffer(block_data[:needed], dtype=_np.uint8)
        tiles = buf.reshape(n_tiles_x * n_tiles_y, 64)
        ar = tiles[:, 0:32].reshape(-1, 16, 2)
        gb = tiles[:, 32:64].reshape(-1, 16, 2)
        a = ar[:, :, 0]; r = ar[:, :, 1]
        g = gb[:, :, 0]; b = gb[:, :, 1]
        rgba = _np.stack([r, g, b, a], axis=-1).reshape(n_tiles_y, n_tiles_x, 4, 4, 4)
        rgba = rgba.transpose(0, 2, 1, 3, 4)
        img = rgba.reshape(n_tiles_y * 4, n_tiles_x * 4, 4)[:height, :width, :]
        return img.tobytes()

    out = bytearray(width * height * 4)
    pos = 0
    n = len(block_data)
    for ty in range(0, height, 4):
        for tx in range(0, width, 4):
            if pos + 64 > n:
                break
            ar = block_data[pos:pos + 32]
            gb = block_data[pos + 32:pos + 64]
            pos += 64
            i = 0
            for by in range(4):
                py = ty + by
                for bx in range(4):
                    px = tx + bx
                    a, r = ar[i * 2], ar[i * 2 + 1]
                    g, b = gb[i * 2], gb[i * 2 + 1]
                    i += 1
                    if px < width and py < height:
                        o = (py * width + px) * 4
                        out[o:o + 4] = bytes((r, g, b, a))
    return bytes(out)


def parse_gsh(path: str | Path) -> GshFile:
    path = Path(path)
    data = path.read_bytes()

    if data[0:4] != MAGIC:
        raise ValueError(f"not a SHPG file (got {data[0:4]!r})")

    file_size = struct.unpack_from("<I", data, 4)[0]
    object_count = struct.unpack_from(">I", data, 8)[0]
    if object_count > (len(data)-16)//8:
        raise ValueError('GSH object table exceeds file')
    format_ver = data[12:16].decode("ascii", errors="replace")

    gsh = GshFile(
        path=str(path),
        signature=data[0:4].decode("ascii"),
        file_size=file_size,
        object_count=object_count,
        format_ver=format_ver,
    )

    table_off = 16
    slots = []
    for i in range(object_count):
        o = table_off + i * 8
        name = data[o:o + 4].rstrip(b"\x00").decode("ascii", errors="replace")
        entry_offset = struct.unpack_from(">I", data, o + 4)[0]
        slots.append((i, name, entry_offset))

    def read_entry(idx, name, entry_offset, orphan=False) -> GshEntry | None:
        if entry_offset + 16 > len(data):
            return None
        eh = data[entry_offset:entry_offset + 16]
        record_id = eh[0]
        # SHPG uses a big-endian 24-bit block length at +1, NOT a reserved
        # byte followed by a u16 length. The old LOD-bias hypothesis was wrong.
        size_of_block = int.from_bytes(eh[1:4], 'big')
        if size_of_block < 16 or entry_offset + size_of_block > len(data):
            raise ValueError(f'GSH entry {idx} block length is outside file')
        width = struct.unpack_from(">H", eh, 4)[0]
        height = struct.unpack_from(">H", eh, 6)[0]
        center_x = struct.unpack_from(">H", eh, 8)[0]
        center_y = struct.unpack_from(">H", eh, 10)[0]
        tail = eh[12:16]
        # Only the leading tail byte (0x20) is actually constant across
        # every entry/format observed (including effects.gsh, where the
        # trailing 3 bytes are uninitialized/stale padding rather than a
        # fixed magic — the old "20 00 20 00" assumption from
        # rc_controller/timothy/rccartrack23 does not hold universally).
        tail_ok = (tail[0:1] == ENTRY_TAIL_MAGIC[0:1])

        img_offset = entry_offset + 16
        img_size = max(0, size_of_block - 16)

        label, bpp, block_fmt = RECORD_FORMATS.get(record_id, (f"UNKNOWN(0x{record_id:02x})", 8, "raw"))

        if block_fmt == "cmpr":
            bpp_num, bpp_den = 1, 2     # 0.5 byte/pixel
        elif block_fmt == "pal8":
            bpp_num, bpp_den = 1, 1     # 1 byte/pixel
        else:
            bpp_num, bpp_den = bpp, 8

        # Legacy field name retained for report compatibility: this is the
        # high byte of the 24-bit block length, not a LOD flag.
        reserved0 = eh[1]
        lod_bias = 0
        mips = _mip_chain(width, height, img_size, bpp_num, bpp_den) if width and height else []

        img_end = img_offset + img_size
        mips_used = mips

        return GshEntry(
            index=idx, name=name, full_name=None, entry_offset=entry_offset,
            record_id=record_id, format_label=label,
            size_of_block=size_of_block, width=width, height=height,
            center_x=center_x, center_y=center_y,
            img_offset=img_offset, img_size=img_size, img_end=img_end,
            tail_ok=tail_ok, mip_levels=mips_used, orphan=orphan,
            reserved0=reserved0, lod_bias=lod_bias,
        )

    entries = []
    for i, name, off in slots:
        e = read_entry(i, name, off)
        if e is not None:
            entries.append(e)

    # PAL8/PAL4 entries store their CLUT in a second, palette-flavoured
    # 16-byte header (record_id=PALETTE_RECORD_ID) sitting immediately
    # after the declared image block — see module docstring. PAL4's CLUT
    # is the same structure, just with width<=16 (16-color palette)
    # instead of PAL8's 256.
    for e in entries:
        if e.record_id in PALETTED_RECORD_IDS:
            e.palette = _try_read_palette(data, e.img_end)

    # Every entry (paletted or not) is followed by a full-name record:
    # NAME_RECORD_MAGIC (70 00 00 00) + null-padded ASCII string. For
    # paletted formats this sits after the CLUT block; for others it
    # sits directly at img_end. See _try_read_name().
    for e in entries:
        name_off = e.img_end
        if e.record_id in PALETTED_RECORD_IDS and e.palette is not None:
            pal_hdr = data[e.img_end:e.img_end + 16]
            if len(pal_hdr) == 16:
                pal_size_of_block = int.from_bytes(pal_hdr[1:4], 'big')
                name_off = e.img_end + 16 + max(0, pal_size_of_block - 16)
        e.full_name = _try_read_name(data, name_off)

    entries.sort(key=lambda e: e.entry_offset)
    gsh.entries = entries
    return gsh, data


# ---------------------------------------------------------------------------
# Pixel decoders
# ---------------------------------------------------------------------------

def _rgb565_to_rgb888(v: int):
    r = (v >> 11) & 0x1F
    g = (v >> 5) & 0x3F
    b = v & 0x1F
    r = (r << 3) | (r >> 2)
    g = (g << 2) | (g >> 4)
    b = (b << 3) | (b >> 2)
    return r, g, b


def _rgb565_to_rgba8888(v: int):
    """
    Decode a packed 16-bit CLUT color for PALETTE_RECORD_ID_RGB565 (0x31)
    entries. Field order is R(5) G(6) B(5), matching the "565" name at
    face value -- no alpha channel, always fully opaque.

    NOTE: an earlier revision of this function swapped to BGR order based
    on a hunch that world.gsh's na_bottomoftreehouse "ought to" contain
    blue tones (it doesn't necessarily -- a reddish-brown wood underside
    is perfectly plausible and was never actually confirmed against any
    reference). That swap was DISPROVEN by a real in-game screenshot of
    lemonadestand256 (also 0x31-paletted): the genuine texture has a blue
    mug, white/gray cups, tan wood, and orange "25 cent" text, which only
    decodes correctly in plain RGB order -- BGR order washed the whole
    stand out lavender/purple. Do not swap this again without a
    screenshot or other ground truth to confirm against; a texture
    "looking plausible" in isolation is not sufficient evidence, since
    both RGB and BGR order tend to produce *a* coherent-looking image,
    just with the wrong hues.
    """
    r, g, b = _rgb565_to_rgb888(v)
    return (r, g, b, 255)


def _rgb5a3_to_rgba8888(v: int):
    """
    Decode a GX RGB5A3 16-bit color, used by PALETTE_RECORD_ID_RGB5A3
    (0x32) CLUT entries and direct RGB5A3 texture format (record_id 21).
    Field order is RGB (R high, B low) despite the reference inspector
    tool's field label reading "PALETTE BGR5A3" for this record id --
    that label does NOT reflect true byte order here. Confirmed
    empirically on world.gsh's recovered "orangetree-nofruits" (index 98,
    0x32-paletted): decoding with R-high/B-low order produces correct
    orange/brown canopy tones; swapping to B-high/R-low turns the same
    texture blue-dominant, which is wrong for an orange tree. (Contrast
    with record 0x31 / PALETTE_RECORD_ID_RGB565, which genuinely IS
    BGR order -- see _rgb565_to_rgba8888. The two palette record types
    do not share a channel convention despite their similar-looking GUI
    labels.)

    Two sub-formats selected by the top bit:
      bit15=1: opaque RGB555          (1 RRRRR GGGGG BBBBB), alpha=255
      bit15=0: translucent RGB4443    (0 AAA RRRR GGGG BBBB)
    """
    if v & 0x8000:
        r = (v >> 10) & 0x1F
        g = (v >> 5) & 0x1F
        b = v & 0x1F
        r = (r << 3) | (r >> 2)
        g = (g << 3) | (g >> 2)
        b = (b << 3) | (b >> 2)
        a = 255
    else:
        a = (v >> 12) & 0x7
        r = (v >> 8) & 0xF
        g = (v >> 4) & 0xF
        b = v & 0xF
        r = (r << 4) | r
        g = (g << 4) | g
        b = (b << 4) | b
        a = (a << 5) | (a << 2) | (a >> 1)
    return (r, g, b, a)


def _rgb5a3_to_rgba8888_np(v):
    """Vectorized (numpy array in, 4 numpy arrays out) equivalent of
    _rgb5a3_to_rgba8888 above -- validated byte-for-byte against it."""
    v = v.astype(_np.int64)
    opaque = (v & 0x8000) != 0

    r_o = (v >> 10) & 0x1F; g_o = (v >> 5) & 0x1F; b_o = v & 0x1F
    r_o = (r_o << 3) | (r_o >> 2)
    g_o = (g_o << 3) | (g_o >> 2)
    b_o = (b_o << 3) | (b_o >> 2)
    a_o = _np.full_like(v, 255)

    a_t = (v >> 12) & 0x7
    r_t = (v >> 8) & 0xF; g_t = (v >> 4) & 0xF; b_t = v & 0xF
    r_t = (r_t << 4) | r_t
    g_t = (g_t << 4) | g_t
    b_t = (b_t << 4) | b_t
    a_t = (a_t << 5) | (a_t << 2) | (a_t >> 1)

    r = _np.where(opaque, r_o, r_t)
    g = _np.where(opaque, g_o, g_t)
    b = _np.where(opaque, b_o, b_t)
    a = _np.where(opaque, a_o, a_t)
    return r, g, b, a


# Maps a scalar pixel_fn (as passed to _detile_gx_16bpp) to its vectorized
# equivalent, if one exists. Populated after both are defined; used to pick
# the numpy fast path only for pixel_fn's we've actually ported and verified.
_PIXEL_FN_NP_TABLE = {}
if _HAVE_NUMPY:
    _PIXEL_FN_NP_TABLE[_rgb5a3_to_rgba8888] = _rgb5a3_to_rgba8888_np


def decode_cmpr(block_data: bytes, width: int, height: int) -> bytes:
    """
    Decode GameCube-style CMPR (big-endian DXT1, 2x2 arrangement of 4x4
    sub-blocks per 8x8 tile). Returns raw RGBA8 bytes, row-major,
    width*height*4 long.
    """
    n_tiles_x = _ceildiv(width, 8)
    n_tiles_y = _ceildiv(height, 8)
    n_blocks = n_tiles_x * n_tiles_y * 4
    needed = n_blocks * 8
    # Same caveat as the other detile fast paths: the original loop's
    # `continue`-without-advancing-pos on a truncated subblock effectively
    # skips every remaining subblock too, which zero-padding doesn't
    # reproduce -- so the fast path only applies to complete data.
    if _HAVE_NUMPY and len(block_data) >= needed:
        buf = _np.frombuffer(block_data[:needed], dtype=_np.uint8).reshape(n_blocks, 8)
        c0 = (buf[:, 0].astype(_np.uint32) << 8) | buf[:, 1]
        c1 = (buf[:, 2].astype(_np.uint32) << 8) | buf[:, 3]
        idx_bits = (buf[:, 4].astype(_np.uint32) << 24) | (buf[:, 5].astype(_np.uint32) << 16) \
            | (buf[:, 6].astype(_np.uint32) << 8) | buf[:, 7]

        def _rgb565_np(v):
            r = (v >> 11) & 0x1F
            g = (v >> 5) & 0x3F
            b = v & 0x1F
            r = (r << 3) | (r >> 2)
            g = (g << 2) | (g >> 4)
            b = (b << 3) | (b >> 2)
            return r.astype(_np.int32), g.astype(_np.int32), b.astype(_np.int32)

        r0, g0, b0 = _rgb565_np(c0)
        r1, g1, b1 = _rgb565_np(c1)
        gt = c0 > c1

        r2 = _np.where(gt, (2 * r0 + r1) // 3, (r0 + r1) // 2)
        g2 = _np.where(gt, (2 * g0 + g1) // 3, (g0 + g1) // 2)
        b2 = _np.where(gt, (2 * b0 + b1) // 3, (b0 + b1) // 2)
        r3 = _np.where(gt, (r0 + 2 * r1) // 3, 0)
        g3 = _np.where(gt, (g0 + 2 * g1) // 3, 0)
        b3 = _np.where(gt, (b0 + 2 * b1) // 3, 0)
        a3 = _np.where(gt, 255, 0)

        pal = _np.zeros((n_blocks, 4, 4), dtype=_np.int32)
        pal[:, 0, 0] = r0; pal[:, 0, 1] = g0; pal[:, 0, 2] = b0; pal[:, 0, 3] = 255
        pal[:, 1, 0] = r1; pal[:, 1, 1] = g1; pal[:, 1, 2] = b1; pal[:, 1, 3] = 255
        pal[:, 2, 0] = r2; pal[:, 2, 1] = g2; pal[:, 2, 2] = b2; pal[:, 2, 3] = 255
        pal[:, 3, 0] = r3; pal[:, 3, 1] = g3; pal[:, 3, 2] = b3; pal[:, 3, 3] = a3

        shifts = 30 - 2 * _np.arange(16)
        sel = (idx_bits[:, None] >> shifts[None, :]) & 0x3  # (n_blocks, 16)
        colors = pal[_np.arange(n_blocks)[:, None], sel]     # (n_blocks, 16, 4)

        colors = colors.reshape(n_tiles_y, n_tiles_x, 2, 2, 4, 4, 4)  # ty,tx,suby,subx,py,px,ch
        colors = colors.transpose(0, 2, 4, 1, 3, 5, 6)                # ty,suby,py,tx,subx,px,ch
        img = colors.reshape(n_tiles_y * 8, n_tiles_x * 8, 4)[:height, :width, :]
        return img.astype(_np.uint8).tobytes()

    out = bytearray(width * height * 4)

    def put_pixel(px, py, rgba):
        if 0 <= px < width and 0 <= py < height:
            o = (py * width + px) * 4
            out[o:o + 4] = rgba

    pos = 0
    n = len(block_data)
    for tile_y in range(0, height, 8):
        for tile_x in range(0, width, 8):
            for sub in range(4):
                sub_x = tile_x + (sub % 2) * 4
                sub_y = tile_y + (sub // 2) * 4
                if pos + 8 > n:
                    continue
                c0, c1, idx_bits = struct.unpack_from(">HHI", block_data, pos)
                pos += 8

                col0 = _rgb565_to_rgb888(c0)
                col1 = _rgb565_to_rgb888(c1)
                palette = [col0 + (255,), col1 + (255,)]
                if c0 > c1:
                    c2 = tuple((2 * a + b) // 3 for a, b in zip(col0, col1)) + (255,)
                    c3 = tuple((a + 2 * b) // 3 for a, b in zip(col0, col1)) + (255,)
                else:
                    c2 = tuple((a + b) // 2 for a, b in zip(col0, col1)) + (255,)
                    c3 = (0, 0, 0, 0)
                palette += [c2, c3]

                for py in range(4):
                    for px in range(4):
                        shift = (py * 4 + px) * 2
                        sel = (idx_bits >> (30 - shift)) & 0x3
                        r, g, b, a = palette[sel]
                        put_pixel(sub_x + px, sub_y + py,
                                  bytes((r, g, b, a)))
    return bytes(out)


def _detile_gx_16bpp(block_data: bytes, width: int, height: int, pixel_fn) -> bytes:
    """
    Undo GameCube GX 4x4-texel tiling for 16-bit-per-texel formats
    (RGB5A3, RGB565, IA8): 32 bytes/tile (16 texels * 2 bytes BE),
    tiles walked row-major, texels within a tile walked row-major.
    pixel_fn(u16_value) -> (r,g,b,a) does the per-format color decode.
    Returns raw RGBA8 bytes, row-major, width*height*4 long.
    """
    n_tiles_x = _ceildiv(width, 4)
    n_tiles_y = _ceildiv(height, 4)
    needed = n_tiles_x * n_tiles_y * 32
    pixel_fn_np = _PIXEL_FN_NP_TABLE.get(pixel_fn)
    # Same caveat as _detile_gx_rgba8: the original `break`-on-partial-tile
    # behavior only matches zero-padding when the data is complete, and the
    # vectorized path is only available for pixel_fn's we've ported to numpy.
    if _HAVE_NUMPY and pixel_fn_np is not None and len(block_data) >= needed:
        buf = _np.frombuffer(block_data[:needed], dtype=_np.uint8)
        tiles = buf.reshape(n_tiles_x * n_tiles_y, 16, 2)
        v = (tiles[:, :, 0].astype(_np.uint32) << 8) | tiles[:, :, 1]
        r, g, b, a = pixel_fn_np(v)
        rgba = _np.stack([r, g, b, a], axis=-1).astype(_np.uint8)
        rgba = rgba.reshape(n_tiles_y, n_tiles_x, 4, 4, 4)
        rgba = rgba.transpose(0, 2, 1, 3, 4)
        img = rgba.reshape(n_tiles_y * 4, n_tiles_x * 4, 4)[:height, :width, :]
        return img.tobytes()

    out = bytearray(width * height * 4)
    pos = 0
    n = len(block_data)
    for ty in range(0, height, 4):
        for tx in range(0, width, 4):
            if pos + 32 > n:
                break
            tile = block_data[pos:pos + 32]
            pos += 32
            i = 0
            for by in range(4):
                py = ty + by
                for bx in range(4):
                    px = tx + bx
                    v = (tile[i * 2] << 8) | tile[i * 2 + 1]
                    i += 1
                    if px < width and py < height:
                        r, g, b, a = pixel_fn(v)
                        o = (py * width + px) * 4
                        out[o:o + 4] = bytes((r, g, b, a))
    return bytes(out)


def decode_rgb5a3(block_data: bytes, width: int, height: int) -> bytes:
    """Decode a GX RGB5A3 (16bpp, translucent-capable) block into RGBA8 bytes."""
    return _detile_gx_16bpp(block_data, width, height, _rgb5a3_to_rgba8888)


def classify_alpha(rgba: bytes, mask_mid_threshold: float = 0.15) -> str:
    """
    Inspect a decoded RGBA8888 buffer's alpha channel and classify how a
    glTF material using it as baseColorTexture should be rendered:

      "opaque" — every pixel alpha==255. Alpha channel carries no
                 information; alphaMode should stay OPAQUE (the glTF
                 default) so renderers don't pay blend/sort cost for
                 nothing.
      "mask"   — alpha is overwhelmingly 0 or 255 (a hard cutout, e.g.
                 GX C8/PAL8 foliage or fence textures with a
                 punch-through CLUT entry), with at most a thin band of
                 intermediate values from anti-aliased edges. alphaMode=
                 MASK with a 0.5 cutoff reproduces this without paying
                 BLEND's sort-order cost.
      "blend"  — alpha carries a substantial genuinely-translucent
                 component (true blending, e.g. glass/lens/window
                 content). alphaMode=BLEND is the correct (pricier,
                 sort-order-sensitive) choice.

    GX/Wii hardware supports real per-pixel alpha blending in the TEV/
    pixel-engine stage, so "blend" is a legitimate hardware feature, not
    a fallback — but the CLUT palette only carries the GX RGB5A3
    alpha's 8 quantization steps (0, 36, 73, 109, 146, 182, 219, 255),
    so a handful of anti-aliased edge texels on an otherwise-binary
    cutout texture will always show up as "intermediate" values even
    though the source content is conceptually a hard mask. Treating any
    single non-binary alpha sample as "blend" therefore over-classifies
    ordinary foliage/fence cutouts as full alpha-blended geometry,
    producing sort-order render glitches (flicker/incorrect
    overlap/disappearing faces) instead of the cheap, stable MASK
    behavior these textures actually need.

    Fix: classify by the *proportion* of strictly-intermediate alpha
    values, not merely their presence. A texture with a real population
    of alpha==0 texels (something to punch through against) and only a
    thin (<mask_mid_threshold) band of in-between values from
    edge anti-aliasing is treated as MASK. Anything with either no
    alpha==0 population at all (nothing to cut out against -- e.g. a
    glass panel that's translucent everywhere) or a substantial
    intermediate-alpha fraction is BLEND.

    Confirmed against world.gsh materials: "flowers" (8.2% intermediate,
    56% at alpha=0) and "footy_mesh" (6.8% intermediate, 43% at
    alpha=0) now correctly classify as MASK; "lemonadestand256" (27.3%
    intermediate, 0% at alpha=0 -- a glass panel with no fully-cutout
    pixels at all) correctly stays BLEND.

    This is a content-based classification, not a per-texture allowlist:
    any texture with a real alpha channel gets picked up automatically,
    including ones added to future .gsh archives, with no maintenance.
    """
    alphas = rgba[3::4]
    total = len(alphas)
    if total == 0 or min(alphas) == 255:
        return "opaque"

    n_zero = alphas.count(0)
    n_full = alphas.count(255)
    n_mid = total - n_zero - n_full

    if n_mid == 0:
        return "mask"   # pure 0/255 punch-through, no edge dithering at all

    # Only eligible for MASK if there's an actual zero-alpha population
    # to punch through against; a texture that's translucent everywhere
    # (no true cutout pixels) is BLEND regardless of how small the mid
    # band is.
    if n_zero > 0 and (n_mid / total) < mask_mid_threshold:
        return "mask"

    return "blend"


def decode_pal8(index_bytes: bytes, width: int, height: int, palette: list) -> bytes:
    """
    Decode a GX C8 (8-bit paletted) block into RGBA8 bytes, row-major,
    width*height*4 long. Un-tiles the 8x4 GX index layout first, then
    looks each index up in `palette` (list of (r,g,b,a) tuples).
    """
    idx = _detile_c8(index_bytes, width, height)
    return _palette_gather(idx, width, height, palette)


def decode_pal4(index_bytes: bytes, width: int, height: int, palette: list) -> bytes:
    """
    Decode a GX C4 (4-bit paletted) block into RGBA8 bytes, row-major,
    width*height*4 long. Un-tiles the 8x8 GX index layout first, then
    looks each nibble index (0-15) up in `palette`.
    """
    idx = _detile_c4(index_bytes, width, height)
    return _palette_gather(idx, width, height, palette)


def _palette_gather(idx: bytes, width: int, height: int, palette: list) -> bytes:
    """Shared by decode_pal8/decode_pal4: idx[i] -> palette[idx[i]] (or
    opaque black if out of range), expanded to RGBA8 row-major bytes."""
    n = min(len(idx), width * height)
    pal_len = len(palette)
    if n != width * height or (idx and max(idx) >= pal_len):
        raise ValueError('Texture index buffer is truncated or references a missing palette color')
    if _HAVE_NUMPY:
        idx_arr = _np.frombuffer(idx[:n], dtype=_np.uint8).astype(_np.int64)
        default_row = _np.array([[0, 0, 0, 255]], dtype=_np.uint8)
        if pal_len:
            pal_table = _np.vstack([_np.array(palette, dtype=_np.uint8), default_row])
        else:
            pal_table = default_row
        safe_idx = _np.where(idx_arr < pal_len, idx_arr, pal_len)
        gathered = pal_table[safe_idx]
        if n < width * height:
            pad = _np.zeros((width * height - n, 4), dtype=_np.uint8)
            gathered = _np.vstack([gathered, pad])
        return gathered.tobytes()

    out = bytearray(width * height * 4)
    for i in range(n):
        px = idx[i]
        c = palette[px] if px < pal_len else (0, 0, 0, 255)
        out[i * 4:i * 4 + 4] = bytes(c)
    return bytes(out)


def decode_rgba8(block_data: bytes, width: int, height: int) -> bytes:
    """Decode a GX RGBA8 (uncompressed 32bpp) block into RGBA8 bytes."""
    return _detile_gx_rgba8(block_data, width, height)


def decode_pal8_indices(block_data: bytes, width: int, height: int) -> bytes:
    """
    Fallback for when no palette header could be found: returns the raw
    (still GX-tiled — NOT un-tiled) index values expanded to grayscale
    RGBA (index value repeated into RGB) so the shape/edges of the
    texture are still visible for inspection. Prefer decode_pal8() with
    a real palette whenever entry.palette is available.
    """
    out = bytearray(width * height * 4)
    n = min(len(block_data), width * height)
    for i in range(n):
        v = block_data[i]
        o = i * 4
        out[o:o + 4] = bytes((v, v, v, 255))
    return bytes(out)


def save_png(rgba: bytes, width: int, height: int, out_path: Path):
    from PIL import Image
    img = Image.frombytes("RGBA", (width, height), rgba)
    img.save(out_path)


def rgba_to_png_bytes(rgba: bytes, width: int, height: int) -> bytes:
    """Encode raw RGBA8888 bytes to an in-memory PNG (for GLB embedding)."""
    import io
    from PIL import Image
    buf = io.BytesIO()
    Image.frombytes("RGBA", (width, height), rgba).save(buf, format="PNG")
    return buf.getvalue()


def decode_entry_rgba(entry: "GshEntry", data: bytes) -> tuple[bytes, int, int]:
    """
    Decode a GshEntry's top mip level to raw RGBA8888 bytes.
    Returns (rgba_bytes, width, height). Raises ValueError if the entry
    has no decodable mip level or an unsupported format.

    Single source of truth for entry -> pixels, used by both the texture
    tab's "export as PNG" action and batch model export (material texture
    embedding), so the two paths can't drift.
    """
    if not entry.mip_levels:
        raise ValueError("no decodable mip level for this entry")
    lvl_w, lvl_h, lvl_len = entry.mip_levels[0]
    blob = data[entry.img_offset:entry.img_end][:lvl_len]
    if len(blob) != lvl_len:
        raise ValueError('Truncated GX texture level')
    if entry.record_id in PALETTED_RECORD_IDS and not entry.palette:
        raise ValueError('Missing palette; refusing grayscale placeholder as decoded texture')
    fmt_info = RECORD_FORMATS.get(entry.record_id, (None, None, "raw"))
    block_fmt = fmt_info[2]
    if block_fmt == "cmpr":
        rgba = decode_cmpr(blob, lvl_w, lvl_h)
    elif block_fmt == "rgba8":
        rgba = decode_rgba8(blob, lvl_w, lvl_h)
    elif block_fmt == "rgb5a3":
        rgba = decode_rgb5a3(blob, lvl_w, lvl_h)
    elif block_fmt == "rgb565":
        rgba = _detile_gx_16bpp(blob, lvl_w, lvl_h, _rgb565_to_rgba8888)
    elif block_fmt == "pal8":
        rgba = decode_pal8(blob, lvl_w, lvl_h, entry.palette) if getattr(entry, "palette", None) \
            else decode_pal8_indices(blob, lvl_w, lvl_h)
    elif block_fmt == "pal4":
        rgba = decode_pal4(blob, lvl_w, lvl_h, entry.palette) if getattr(entry, "palette", None) \
            else decode_pal8_indices(blob, lvl_w, lvl_h)
    else:
        raise ValueError(f"unsupported format for decode: {entry.format_label}")
    return rgba, lvl_w, lvl_h


def decode_entry_png_bytes(entry: "GshEntry", data: bytes) -> bytes:
    """Decode a GshEntry straight to in-memory PNG bytes."""
    rgba, w, h = decode_entry_rgba(entry, data)
    return rgba_to_png_bytes(rgba, w, h)


# ---------------------------------------------------------------------------
# CLI / reporting
# ---------------------------------------------------------------------------

def print_report(gsh: GshFile):
    print(f"\n{'=' * 70}")
    print(f"  {gsh.path}")
    print(f"{'=' * 70}")
    print(f"  signature={gsh.signature}  file_size={gsh.file_size}  "
          f"object_count={gsh.object_count}  format_ver={gsh.format_ver}")
    print(f"\n  {'#':>3}  {'name':<24} {'fmt':<10} {'w':>5} {'h':>5} "
          f"{'off':>8} {'img_size':>9}  {'mips':<5} tail")
    for e in gsh.entries:
        tag = " (orphan)" if e.orphan else ""
        disp_name = e.full_name or e.name
        print(f"  {e.index:>3}  {disp_name:<24} {e.format_label:<10} "
              f"{e.width:>5} {e.height:>5} 0x{e.entry_offset:06x} "
              f"{e.img_size:>9}  {len(e.mip_levels):<5} "
              f"{'ok' if e.tail_ok else 'MISMATCH'}{tag}")
    print()


def extract(gsh: GshFile, data: bytes, out_dir: Path, decode_images: bool,
            write_bin: bool = True, write_manifest: bool = True):
    """Write the selected combination of outputs for every entry.

    decode_images   -> write decoded .png (best-effort, per entry)
    write_bin       -> write raw/uncompressed-block .bin (image bytes as
                        stored in the .gsh, mip chain intact, no filtering)
    write_manifest  -> write manifest.json describing every entry
                        (name, offsets, format, dimensions, mip levels,
                        palette, etc.) — the "image data" metadata index

    Any subset may be selected (e.g. manifest-only, bin-only, png-only,
    or any combination) so callers such as a GUI export dropdown can let
    the user pick exactly what they want written.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = asdict(gsh)

    for e in gsh.entries:
        export_name = e.full_name or e.name or "unnamed"
        base = f"{e.index:03d}_{export_name}_0x{e.entry_offset:06x}"
        blob = data[e.img_offset:e.img_end]

        if write_bin:
            (out_dir / f"{base}.bin").write_bytes(blob)

        if decode_images and e.width and e.height and e.mip_levels:
            try:
                rgba, lvl_w, lvl_h = decode_entry_rgba(e, data)
                is_index_fallback = (
                    e.record_id in PALETTED_RECORD_IDS and not e.palette
                )
                suffix = "_indices" if is_index_fallback else ""
                save_png(rgba, lvl_w, lvl_h, out_dir / f"{base}{suffix}.png")
            except Exception as exc:
                print(f"  [warn] decode failed for entry {e.index} ({e.name}): {exc}")

    if write_manifest:
        (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))

    wrote = []
    if write_bin: wrote.append("bin")
    if decode_images: wrote.append("png")
    if write_manifest: wrote.append("manifest")
    print(f"  wrote {len(gsh.entries)} entries ({'+'.join(wrote) or 'nothing'}) -> {out_dir}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("gsh_file")
    ap.add_argument("-o", "--out", default=None, help="output directory for extraction")
    ap.add_argument("--extract", action="store_true", help="extract raw + decoded images")
    ap.add_argument("--no-decode", action="store_true", help="skip PNG decode, raw .bin only")
    ap.add_argument("--no-bin", action="store_true", help="skip writing raw/uncompressed .bin files")
    ap.add_argument("--no-manifest", action="store_true", help="skip writing manifest.json")
    args = ap.parse_args()

    gsh, data = parse_gsh(args.gsh_file)
    print_report(gsh)

    if args.extract:
        out_dir = Path(args.out) if args.out else Path(args.gsh_file).with_suffix("") 
        extract(
            gsh, data, out_dir,
            decode_images=not args.no_decode,
            write_bin=not args.no_bin,
            write_manifest=not args.no_manifest,
        )


if __name__ == "__main__":
    main()
