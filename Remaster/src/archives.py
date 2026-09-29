"""EA BIGF/BIG4 archives and bounded RefPack decoding. No external tools."""
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
import struct


def is_refpack(data):
    return len(data) >= 2 and data[1] == 0xFB and data[0] & 0x3E == 0x10


def decompress(data, limit=512 * 1024 * 1024):
    if not is_refpack(data):
        return data
    width = 4 if data[0] & 0x80 else 3
    pos = 2 + (width if data[0] & 1 else 0)
    if pos + width > len(data):
        raise ValueError("Truncated RefPack header")
    expected = int.from_bytes(data[pos:pos + width], 'big')
    pos += width
    if expected > limit:
        raise ValueError("RefPack output exceeds the 512 MiB safety limit")
    out = bytearray()

    def take(n):
        nonlocal pos
        if pos + n > len(data):
            raise ValueError("Truncated RefPack stream")
        chunk = data[pos:pos+n]
        pos += n
        return chunk

    while True:
        c = take(1)[0]
        count = distance = 0
        if c >= 0xFC:
            literals = c & 3
        elif c >= 0xE0:
            literals = ((c & 31) + 1) * 4
        elif c >= 0xC0:
            a, b, d = take(3)
            literals = c & 3
            count = ((c & 12) << 6) + d + 5
            distance = ((c & 16) << 12) + (a << 8) + b + 1
        elif c >= 0x80:
            a, b = take(2)
            literals = a >> 6
            count = (c & 63) + 4
            distance = ((a & 63) << 8) + b + 1
        else:
            a = take(1)[0]
            literals = c & 3
            count = ((c & 28) >> 2) + 3
            distance = ((c & 96) << 3) + a + 1
        if len(out) + literals + count > expected:
            raise ValueError("RefPack output exceeds its declared size")
        out.extend(take(literals))
        if count:
            if distance > len(out):
                raise ValueError("Invalid RefPack back-reference")
            for _ in range(count):
                out.append(out[-distance])
        if c >= 0xFC:
            break
    if len(out) != expected:
        raise ValueError(f"RefPack size mismatch: {len(out)} / {expected}")
    return bytes(out)


def safe_target(root, name):
    name = name.replace('\\', '/')
    path = PurePosixPath(name)
    if not name or path.is_absolute() or any(p in ('..', '.') for p in name.split('/')):
        raise ValueError(f"Unsafe archive path: {name!r}")
    reserved = {'CON', 'PRN', 'AUX', 'NUL', *(f'COM{i}' for i in range(1, 10)), *(f'LPT{i}' for i in range(1, 10))}
    for part in path.parts:
        if any(c in part for c in ':<>"|?*') or any(ord(c) < 32 for c in part) or part.endswith((' ', '.')) or part.split('.')[0].upper() in reserved:
            raise ValueError(f"Invalid Windows archive path: {name!r}")
    root = Path(root).resolve()
    target = (root / Path(*path.parts)).resolve()
    if not target.is_relative_to(root):
        raise ValueError(f"Archive path escapes destination: {name!r}")
    return target


@dataclass(frozen=True)
class Entry:
    index: int
    name: str
    offset: int
    size: int
    compressed: bool


class BigArchive:
    def __init__(self, path):
        self.path = Path(path)
        size = self.path.stat().st_size
        self.entries = []
        with self.path.open('rb') as f:
            head = f.read(16)
            if len(head) != 16 or head[:4] not in (b'BIGF', b'BIG4'):
                raise ValueError("Expected a BIGF/BIG4 archive (BIG or VIV)")
            self.magic = head[:4].decode()
            self.declared_size = int.from_bytes(head[4:8], 'little')
            count, end = struct.unpack('>II', head[8:])
            if not 16 <= end <= size or count > (end - 16) // 9:
                raise ValueError("Invalid BIG directory bounds")
            directory = f.read(end - 16)
            pos = 0
            for i in range(count):
                if pos + 8 > len(directory):
                    raise ValueError("Truncated BIG directory")
                offset, length = struct.unpack_from('>II', directory, pos)
                pos += 8
                stop = directory.find(b'\0', pos)
                if stop < 0:
                    raise ValueError("Unterminated BIG filename")
                name = directory[pos:stop].decode('utf-8', errors='replace')
                pos = stop + 1
                if offset < end or offset + length > size:
                    raise ValueError(f"Entry outside archive: {name}")
                f.seek(offset)
                compressed = is_refpack(f.read(min(length, 2)))
                self.entries.append(Entry(i, name, offset, length, compressed))

    def read(self, entry, unpack=True):
        with self.path.open('rb') as f:
            f.seek(entry.offset)
            data = f.read(entry.size)
        if len(data) != entry.size:
            raise ValueError("Archive changed or entry is truncated")
        return decompress(data) if unpack else data

    def extract(self, root, entries=None, unpack=True, progress=None, cancelled=None):
        entries = self.entries if entries is None else list(entries)
        # Validate every path and collision before writing anything.
        targets = [safe_target(root, e.name) for e in entries]
        if len({str(t).casefold() for t in targets}) != len(targets):
            raise ValueError("Archive has duplicate output paths")
        if any(t.exists() for t in targets):
            raise FileExistsError("Destination already contains an entry; select a new output folder")
        written = []
        for i, (entry, target) in enumerate(zip(entries, targets)):
            if cancelled and cancelled():
                raise InterruptedError(f"Cancelled after {len(written)} files")
            data = self.read(entry, unpack)
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as f:
                f.write(data)
            written.append(target)
            if progress:
                progress(i + 1, len(entries), entry.name)
        return written
