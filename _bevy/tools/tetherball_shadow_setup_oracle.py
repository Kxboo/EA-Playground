"""Execute SetupShadowOptions and its BSS initializer on the original PPC."""
import hashlib, json, random, struct, sys
from pathlib import Path
from tetherball_scene_oracle import SceneEmu
import re_functions as rf

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '_bevy/tests/data/tetherball_shadow_setup_golden.json'
SHA = '5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
GAME = 0x71000000
SHADOW_MANAGER = 0x71001000


def bits(v): return struct.unpack('>I', struct.pack('>f', v))[0]

def capture(exe, pos):
    e = SceneEmu(exe)
    events = []
    e.wr(GAME + 0x110, struct.pack('>3f', *(struct.unpack('>f', struct.pack('>I', x))[0] for x in pos)))
    sda = e.e.symbols['_SDA_BASE_']['value']
    e.w32(sda - 0x1cfc, SHADOW_MANAGER)

    def set_viewport(em):

        # Include only members consumed by SetViewport; inter-vector padding is not initialized by the retail constructor.
        options = [em.r32(em.r[5] + i * 4) for i in (0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14, 16)]
        events.append(['set_viewport', em.r[3], em.r[4], options])

    e.hooks[0x803c5594] = set_viewport
    # This module initializer owns the constructor's BSS defaults at 0x805f82a0.
    e.call(0x803c63a8)
    e.call(0x8039cb30, (GAME,))
    return dict(position=pos, events=events)


def generate():
    assert hashlib.sha256((ROOT / 'Remaster/reference/playgroundz.elf').read_bytes()).hexdigest() == SHA
    exe = rf.load()
    rng = random.Random(0x8039cb30)
    positions = [
        [bits(0.0), bits(0.0), bits(0.0)],
        [bits(1.25), bits(2.0), bits(-3.5)],
        [bits(-0.0), bits(0.0), bits(-0.0)],
        [bits(-100.0), bits(0.001), bits(250.0)],
    ]
    positions.extend([[bits(rng.uniform(-500.0, 500.0)) for _ in range(3)] for _ in range(20)])
    cases = [capture(exe, p) for p in positions]
    return {
        'elf_sha256': SHA,
        'function': 'SetupShadowOptions__12MGTetherballFv',
        'options_ctor': 'ShadowViewOptions::__ct__ @ 0x803c4f6c',
        'options_bss_initializer': '__sinit_shadowmanager_cpp @ 0x803c63a8',
        'cases': cases,
    }


if __name__ == '__main__':
    result = generate()
    if '--check' in sys.argv:
        assert json.loads(OUT.read_text(encoding='utf-8')) == result, 'SetupShadowOptions differs from original PPC'
        print('Verified original SetupShadowOptions:', len(result['cases']), 'cases')
    else:
        OUT.parent.mkdir(parents=True, exist_ok=True)
        OUT.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8', newline='\n')
        print('Captured original SetupShadowOptions:', len(result['cases']), 'cases')
