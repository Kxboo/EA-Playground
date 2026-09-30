"""Execute Controller::Initialize and cCSVParser on the private controls corpus.

Only file I/O, CString storage/comparison, allocation and compiler spill helpers
are adapted. CSV tokenization, enum conversion, and binding construction execute
the original PowerPC. Fixtures contain numeric bindings and input hashes only.
"""
import hashlib
import json
import os
import sys
from pathlib import Path
from ppc_emu2 import Emu

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/control_bindings_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
sys.path.insert(0, str(ROOT / 'Remaster/src'))
import research


class CsvEmu(Emu):
    def __init__(self):
        super().__init__()
        self.heap = 0x300000
        self.file = b''
        self.hooks.update({
            0x80415b1c: self.load_file,
            0x803ccc08: lambda e: e.assign(e.r[3], b''),
            0x803ccd20: lambda e: e.assign(e.r[3], e.string(e.r32(e.r[4]))),
            0x803cce0c: lambda e: e.assign(e.r[3], e.string(e.r[4])),
            0x803cce94: lambda e: None,
            0x803cd38c: self.compare,
            0x803cd0fc: lambda e: e.r.__setitem__(3, e.r32(e.r[3]) + e.r[4]),
            0x80035248: lambda e: e.wr(e.r[3], bytes(e.r[6] * e.r[7])),
            0x802f2cf0: lambda e: None,
            0x80411a94: lambda e: None,
            0x80005fb4: lambda e: e.wr(e.r[3], e.rd(e.r[4], e.r[5])),
        })
        self.words = {}

    def word(self, pc):
        if pc not in self.words:
            self.words[pc] = super().word(pc)
        return self.words[pc]

    def step(self, pc):
        w = self.word(pc)
        if w >> 26 == 31 and (w >> 1) & 1023 == 215:  # stbx
            rs, ra, rb = (w >> 21) & 31, (w >> 16) & 31, (w >> 11) & 31
            self.wr((self.r[ra] if ra else 0)+self.r[rb], bytes([self.r[rs] & 255]))
            return pc + 4
        if w >> 26 == 12:  # addic (used by CString result -> bool)
            from ppc_emu2 import sx
            rd, ra = (w >> 21) & 31, (w >> 16) & 31
            value = self.r[ra] + (sx(w, 16) & 0xffffffff)
            self.ca = int(value > 0xffffffff)
            self.r[rd] = value & 0xffffffff
            return pc + 4
        return super().step(pc)

    def string(self, p):
        if not p:
            return b''
        out = bytearray()
        for _ in range(4096):
            c = self.rd(p, 1)[0]
            if not c:
                return bytes(out)
            out.append(c)
            p += 1
        raise ValueError('unterminated CString')

    def assign(self, obj, value):
        self.wr(self.heap, value + b'\0')
        self.w32(obj, self.heap)
        self.heap += len(value) + 1

    def compare(self, e):
        a, b = e.string(e.r32(e.r[3])), e.string(e.r[4])
        e.r[3] = 0 if a == b else 1

    def load_file(self, e):
        e.wr(0x200000, self.file + b'\0')
        e.w32(e.r[4], len(self.file))
        e.r[3] = 0x200000

    def bindings(self, data, control_type):
        self.mem.clear()
        self.heap = 0x300000
        self.file = data
        obj = 0x100000
        self.call(0x8032c6c4, (obj, control_type, 0), max_steps=4_000_000)
        result = []
        for i in range(self.r32(obj + 0xca6c)):
            row = obj + 0x26c + i * 0x64
            result.append(dict(action=self.r32(row), state=self.r32(row+0x44),
                               transition=self.r32(row+0x48), kind=self.r32(row+0x4c),
                               required=[self.r32(row+0x50), self.r32(row+0x54)],
                               forbidden=[self.r32(row+0x58), self.r32(row+0x5c)],
                               button=self.r32(row+0x60)))
        return result


def generate():
    assert hashlib.sha256((ROOT/'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    data = Path(os.environ.get('EAGL_DATA', ROOT/'eagl EA PLAYGROUND/extra/more/eaplayground files/DATA'))
    archive = str(data/'files/data/csvs.viv')
    names = ['controls', *['controlsmg'+s for s in ('21','bughunt','dartshootout','dodgeball','dribbling','footie','paperairplanes','quickdraw','rccars','template','tetherball','wallball','freethrow')]]
    emu = CsvEmu()
    files = []
    for i, name in enumerate(names):
        raw, _ = research.read_virtual(archive+'::'+name+'.csv')
        rows = emu.bindings(raw, i)
        files.append(dict(name=name+'.csv', sha256=hashlib.sha256(raw).hexdigest(), rows=rows))
        print(f'{name}.csv: {len(rows)} original bindings', flush=True)
    header = 'ACTION_EVENT,CONTROLLER_STATE,CONTROLLER_STATE_TRANSITION,CONTROLLER_EVENT,MOD1,MOD2,BUTTON\r\n'
    samples = [
        ('left whitespace and modifiers', header+'\r\n// ignored comment\r\n EVENT_PLAYER_JUMP,\tSTATE_COMBAT,, BUTTON_DOWN,B,~PLUS,C\r\n'),
        ('literal quotes and mid-field trailing whitespace', header+'"EVENT_PLAYER_MOVE",STATE_COMBAT,,BUTTON_PRESSED,,,ANALOG\r\nEVENT_PLAYER_MOVE ,STATE_COMBAT,,BUTTON_PRESSED,,,ANALOG\r\n'),
        ('line trailing whitespace and fallback tokens', header+'EVENT_PLAYER_JUMP,STATE_COMBAT,,BUTTON_PRESSED,,,C \t\r\nDISABLED,UNKNOWN,,INVALID,UNKNOWN,~UNKNOWN,UNKNOWN\r\n'),
        ('reordered columns', 'BUTTON,MOD2,CONTROLLER_EVENT,ACTION_EVENT,MOD1,CONTROLLER_STATE_TRANSITION,CONTROLLER_STATE\r\nA,~MINUS,BUTTON_DOWN,EVENT_ENTER_MINIGAME,B,,STATE_COMBAT\r\n'),
    ]
    synthetic = [dict(name=name, input=raw, rows=emu.bindings(raw.encode(), 0)) for name, raw in samples]
    return dict(elf_sha256=SHA, source='Controller::Initialize and cCSVParser, original PowerPC', files=files, synthetic=synthetic)


if __name__ == '__main__':
    result = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text()) == result, 'binding fixtures differ from original PowerPC'
        print('All 14 control tables match original PowerPC binding fixtures')
    else:
        OUT.write_text(json.dumps(result, indent=2)+'\n')
        print('Wrote', OUT)
