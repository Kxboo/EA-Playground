"""Execute TetherballMoveCompulsion constructor, Think and expiry in retail PPC."""
import hashlib
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tetherball_ai_oracle as ai
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "_bevy/tests/data/tetherball_ai_move_golden.json"
SHA = "5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c"
CTOR = 0x80395848
EXPIRED = 0x803958D8
THINK = 0x80395990
ACTIVATE = 0x803963AC
DEACTIVATE = 0x803963A0
INTERRUPTIBLE = 0x8039638C
GET_NAME = 0x80396394
COMP = ai.COMP
AI, CHAR, GRAPH, BALL, GAME = ai.AI, ai.CHAR, ai.GRAPH, ai.BALL, ai.GAME


def bits(value):
    return ai.bits(value)


class MoveEmu(ai.EntityEmu):
    def __init__(self, exe):
        super().__init__(exe)
        self.visited = {}

    def step(self, pc):
        self.visited[pc] = self.visited.get(pc, 0) + 1
        return super().step(pc)


def raw_fields(e, size=0x90):
    return e.rd(COMP, size).hex()


def seed_compulsion(e, priority=40, fill=0xA5):
    e.wr(COMP, bytes([fill]) * 0x100)
    e.call(CTOR, [COMP, AI, priority])


def geometry_case(world, inverse, anchor, radius, character):
    return dict(world=list(map(bits, world)), inverse=list(map(bits, inverse)),
                anchor=list(map(bits, anchor)), radius=bits(radius),
                character=list(map(bits, character)))


def generate():
    elf = ROOT / "Remaster/reference/playgroundz.elf"
    assert hashlib.sha256(elf.read_bytes()).hexdigest() == SHA
    exe = rf.load()
    e = MoveEmu(exe)
    rng = random.Random(0x80395848)

    # Resolve the actual tetherball MGID by executing its constructor over the
    # game's SDA string. HasExpired compares this exact object word.
    mgid_scratch = ai.ARG + 0x100
    mgid_text = (exe.symbols["_SDA_BASE_"]["value"] - 0x4730) & 0xFFFFFFFF
    e.call(0x802F2120, [mgid_scratch, mgid_text])
    # The temporary MGID begins at r1+8, and the native compares that first
    # word against MGTetherball's +0x38 identifier.
    tetherball_mgid = e.r32(mgid_scratch)

    constructor = []
    for i, (fill, priority) in enumerate([(0xA5, 0), (0x00, 40), (0x5A, 255)]):
        seed_compulsion(e, priority, fill)
        constructor.append(dict(fill=fill, priority=priority, bytes=raw_fields(e, 0x90)))

    # Complete Think execution: early null binding, exact-byte gate, alternate
    # gate byte, both local-X branches, and varied transforms/position vectors.
    cases = []
    for i in range(96):
        world = ai.IDENTITY[:]
        inverse = ai.IDENTITY[:]
        if i % 5 == 1:
            tx, tz = rng.choice([-12., -1., 0., 8., 30.]), rng.choice([-7., 0., 19.])
            world[12], world[14] = tx, tz
            inverse[12], inverse[14] = -tx, -tz
        if i % 5 == 2:
            world[:12] = [0., 0., 1., 0., 0., 1., 0., 0., -1., 0., 0., 0.]
            inverse[:12] = [0., 0., -1., 0., 0., 1., 0., 0., 1., 0., 0., 0.]
            inverse[12], inverse[14] = -world[14], world[12]
        if i % 13 == 3:
            world[0], world[5], world[10] = 1.25, 0.75, 1.5
            inverse[0], inverse[5], inverse[10] = 0.8, 1.0 / 0.75, 1.0 / 1.5
        anchor = [rng.choice([-3., 0., 2., 12.]), rng.choice([0., 1., 4.]), rng.choice([-9., 0., 7.])]
        if i % 4 == 0:
            anchor = [world[12] + 2., 1., world[14] + 3.]
        radius = rng.choice([0., 0.25, 0.55, 1.25, 3.])
        character = [rng.choice([-8., -2., 0., 1., 5., 16.]), rng.choice([0., 1., 10.]), rng.choice([-9., 0., 7.])]
        if i % 4 == 0:
            character = [world[12] + rng.choice([-1., 0., 1.]), 1., world[14] + 3.]
        c = geometry_case(world, inverse, anchor, radius, character)
        c.update(ms=[-1, 0, 16, 250, 0x7fffffff][i % 5], binding=(0 if i % 17 == 0 else BALL),
                 gate=(1 if i % 19 == 0 else 2 if i % 23 == 0 else 0), fill=(i * 37) & 255)
        seed_compulsion(e, 40, c['fill'])
        e.w32(COMP + 0x88, c['binding'])
        e.wr(COMP + 0x8c, bytes([c['gate']]))
        e.geometry(c)
        result = e.call(THINK, [COMP, c['ms']])
        c['result'] = bool(result)
        c['bytes'] = raw_fields(e)
        cases.append(c)

    # HasExpired's native guards and short circuit: no active minigame, MGID
    # mismatch, non-gameplay state, swinging, in-position and not-in-position.
    expiry = []
    plan = [
        dict(present=False, mgid=True, state=28, animation=0, near=False),
        dict(present=True, mgid=False, state=28, animation=0, near=False),
        dict(present=True, mgid=True, state=27, animation=0, near=False),
        dict(present=True, mgid=True, state=28, animation=63, near=False),
        dict(present=True, mgid=True, state=29, animation=0, near=True),
        dict(present=True, mgid=True, state=28, animation=0, near=False),
        dict(present=True, mgid=True, state=30, animation=91, near=True),
    ]
    for i, c in enumerate(plan):
        seed_compulsion(e, 40, 0xA5)
        e.game_present = c['present']
        e.w32(GAME + 0x38, tetherball_mgid if c['mgid'] else (tetherball_mgid ^ 1))
        e.w32(GAME + 0x34, c['state'])
        e.w32(GRAPH + 0x54, c['animation'])
        world = ai.IDENTITY[:]
        inverse = ai.IDENTITY[:]
        anchor = [0., 1., 0.]
        character = [0.55 if c['near'] else -2., 1., 0.]
        geo = geometry_case(world, inverse, anchor, 0., character)
        e.geometry(geo)
        result = e.call(EXPIRED, [COMP, i % 4])
        expiry.append(dict(**c, result=bool(result)))

    methods = []
    seed_compulsion(e, 23, 0x5A)
    before = raw_fields(e)
    e.call(ACTIVATE, [COMP])
    active = raw_fields(e)
    e.call(DEACTIVATE, [COMP])
    deactivated = raw_fields(e)
    interruptible = bool(e.call(INTERRUPTIBLE, [COMP]))
    name_ptr = e.call(GET_NAME, [COMP])
    methods.append(dict(before=before, active=active, deactivated=deactivated,
                        interruptible=interruptible, name=e.cstring(name_ptr)))

    required = list(range(CTOR, CTOR + 84, 4)) + list(range(EXPIRED, EXPIRED + 184, 4)) + list(range(THINK, THINK + 648, 4))
    coverage = dict(
        ctor=sorted(a for a in e.visited if CTOR <= a < CTOR + 84),
        has_expired=sorted(a for a in e.visited if EXPIRED <= a < EXPIRED + 184),
        think=sorted(a for a in e.visited if THINK <= a < THINK + 648),
        missing=[f"{a:08x}" for a in required if a not in e.visited],
    )
    return dict(elf_sha256=SHA, mgid=tetherball_mgid, constructor=constructor,
                think=cases, has_expired=expiry, methods=methods, coverage=coverage)


if __name__ == "__main__":
    result = generate()
    if "--check" in sys.argv:
        assert json.loads(OUT.read_text(encoding="utf-8")) == result, "MoveCompulsion differs from original PPC"
        print(f"Verified original MoveCompulsion: {len(result['think'])} Think and {len(result['has_expired'])} HasExpired cases")
    else:
        OUT.write_text(json.dumps(result, indent=1) + "\n", encoding="utf-8", newline="\n")
        print("Wrote", OUT)
