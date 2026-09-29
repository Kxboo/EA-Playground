import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import tempfile
import struct
import unittest
import uuid
from contextlib import contextmanager
from archives import BigArchive, decompress, safe_target

TEST_TEMP = Path(__file__).resolve().parent / 'tmp'
TEST_TEMP.mkdir(exist_ok=True)

@contextmanager
def workspace_temp():
    # Python 3.14's Windows mode-0700 temp directories drop inherited sandbox
    # ACLs. A regular workspace directory preserves the parent's permissions.
    root=TEST_TEMP/uuid.uuid4().hex
    root.mkdir()
    yield root


class ArchiveTests(unittest.TestCase):
    def test_literal_and_overlapping_copy(self):
        # ABCD literal, then distance=4, length=6 => ABCDABCDAB
        self.assertEqual(decompress(bytes.fromhex('10fb00000ae0414243440c03fc')),b'ABCDABCDAB')

    def test_each_copy_command(self):
        self.assertEqual(decompress(bytes.fromhex('10fb000008e041424344800003fc')),b'ABCDABCD')
        self.assertEqual(decompress(bytes.fromhex('10fb000009e041424344c0000300fc')),b'ABCDABCDA')
        self.assertEqual(decompress(bytes.fromhex('10fb000003ff616263')),b'abc')

    def test_corrupt_streams_rejected(self):
        for value in ('10fb000004e04142','10fb0000060000fc','10fb000003fc','10fb000001ff616263'):
            with self.subTest(value=value),self.assertRaises(ValueError):decompress(bytes.fromhex(value))

    def test_traversal_windows_aliases(self):
        with workspace_temp() as root:
            for name in ('../escape','/absolute','C:/escape','x/../../escape','a:stream','NUL.txt','folder/CON','bad.','bad '):
                with self.subTest(name=name),self.assertRaises(ValueError):safe_target(root,name)
            self.assertEqual(safe_target(root,'a\\b.o'),Path(root)/'a'/'b.o')

    def test_archive_and_no_overwrite(self):
        with workspace_temp() as root:
            path=Path(root)/'test.big'
            directory=struct.pack('>II',32,3)+b'a.o\0'
            path.write_bytes(b'BIGF'+struct.pack('<I',35)+struct.pack('>II',1,28)+directory+b'\0'*4+b'abc')
            a=BigArchive(path)
            self.assertEqual(a.read(a.entries[0]),b'abc')
            a.extract(Path(root)/'out')
            with self.assertRaises(FileExistsError):a.extract(Path(root)/'out')
            path.write_bytes(path.read_bytes()[:-1])
            with self.assertRaises(ValueError):BigArchive(path)


if __name__=='__main__':unittest.main()
