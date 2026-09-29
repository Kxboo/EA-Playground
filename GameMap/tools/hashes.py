"""Hash functions recovered from the executable (see GameMap/docs/07-data-formats-and-hashes.md).

    locale_hash(s)   -> 32-bit key used by Locale::GetString(const char*)  [ComputeHash @ 0x803ae608]
    attrib_hash64(s) -> 64-bit key used by Attrib::StringToKey             [StringHash64 @ 0x802d8dd4,
                                                                            hash64 @ 0x802d86b0]

`attrib_hash64` is Bob Jenkins' lookup8 `hash()` with the executable's fixed seed.
Both were verified by emulating the original PowerPC code (GameMap/tools/verify_hashes.py).
"""
M64 = (1 << 64) - 1
ATTRIB_SEED = 0xABCDEF0011223344
GOLDEN = 0x9E3779B97F4A7C13


def locale_hash(s):
    """h = 0xFFFFFFFF; for each *signed* byte c: h = h*33 + c (mod 2^32)."""
    h = 0xFFFFFFFF
    for c in (s.encode("latin-1") if isinstance(s, str) else s):
        c = c - 256 if c > 127 else c
        h = (h + (h << 5) + c) & 0xFFFFFFFF
    return h


def _mix64(a, b, c):
    a = (a - b - c) & M64; a ^= c >> 43
    b = (b - c - a) & M64; b ^= (a << 9) & M64
    c = (c - a - b) & M64; c ^= b >> 8
    a = (a - b - c) & M64; a ^= c >> 38
    b = (b - c - a) & M64; b ^= (a << 23) & M64
    c = (c - a - b) & M64; c ^= b >> 5
    a = (a - b - c) & M64; a ^= c >> 35
    b = (b - c - a) & M64; b ^= (a << 49) & M64
    c = (c - a - b) & M64; c ^= b >> 11
    a = (a - b - c) & M64; a ^= c >> 12
    b = (b - c - a) & M64; b ^= (a << 18) & M64
    c = (c - a - b) & M64; c ^= b >> 22
    return a, b, c


def _le(k, i, n):
    return int.from_bytes(k[i:i + n], "little")


def hash64(k, level=ATTRIB_SEED):
    """Bob Jenkins lookup8 hash(): k bytes, 64-bit level (seed)."""
    length = len(k)
    a = b = level & M64
    c = GOLDEN
    i, ln = 0, length
    while ln >= 24:
        a = (a + _le(k, i, 8)) & M64
        b = (b + _le(k, i + 8, 8)) & M64
        c = (c + _le(k, i + 16, 8)) & M64
        a, b, c = _mix64(a, b, c)
        i += 24
        ln -= 24
    c = (c + length) & M64  # low byte of c is reserved for the length
    a = (a + _le(k, i, min(ln, 8))) & M64
    if ln > 8:
        b = (b + _le(k, i + 8, min(ln - 8, 8))) & M64
    if ln > 16:
        c = (c + (_le(k, i + 16, ln - 16) << 8)) & M64
    a, b, c = _mix64(a, b, c)
    return c


def attrib_hash64(s):
    """Attrib::StringToKey: 0 for null/empty strings."""
    k = s.encode("latin-1") if isinstance(s, str) else s
    return hash64(k) if k else 0


if __name__ == "__main__":
    import sys
    for s in sys.argv[1:]:
        print("%s  locale=0x%08x  attrib=0x%016x" % (s, locale_hash(s), attrib_hash64(s)))
