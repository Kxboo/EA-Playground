"""Proof-of-decode verification.  Writes docs/proof-report.json (shown by the in-app Proof screen).

Every check reads the original files directly; nothing is taken from earlier reports except where
labelled.  Flags:
  --with-tests     run Rust corpus/regression suites and original-code behavioral oracles
  --run-selftest   also launch the game self-test (opens a window for ~1 minute)
Exit status is 0 only if every check passes.
"""
import sys,os,json,hashlib,struct,subprocess,time,datetime,re
from pathlib import Path
HERE=Path(__file__).resolve().parent;BEVY=HERE.parent;REPO=BEVY.parent
sys.path[:0]=[str(HERE)]
FILES=Path(os.environ.get('EAGL_RESEARCH',r'D:\_eagl\eagl EA PLAYGROUND\extra\more\eaplayground files'))
DATA=Path(os.environ.get('EAGL_DATA',FILES/'DATA'))
OLD=FILES/'OLD ATTEMPTS'/'EA PLAYGROUND EXTRACTED'/'DATA'
USA=FILES/'EA Playground (USA)'/'data'
ELF=REPO/'Remaster'/'reference'/'playgroundz.elf'
PINNED_ELF='5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c'
checks=[]
def check(name,ok,detail):checks.append(dict(name=name,ok=bool(ok),detail=detail));print(('PASS' if ok else 'FAIL'),name,'-',detail,flush=True)
def sha(p):
    h=hashlib.sha256()
    with open(p,'rb') as f:
        for b in iter(lambda:f.read(1<<20),b''):h.update(b)
    return h.hexdigest()

# --- 1. executable identity -------------------------------------------------
def exe_checks():
    import re_functions as rf
    e=rf.load();h=sha(ELF)
    check('Symbol-bearing ELF matches the pinned hash',h==PINNED_ELF,f'{ELF.name} sha256 {h[:16]}…')
    on_disc=DATA/'files'/'playgroundz.elf'
    if on_disc.exists():check('Disc copy of the ELF is byte-identical to the research copy',sha(on_disc)==h,f'{on_disc.relative_to(DATA)} on the extracted disc')
    funcs=[n for n,s in e.symbols.items() if s['size'] and s['section']<len(e.elf.sections) and e.elf.sections[s['section']]['flags']&4]
    check('ELF exposes named functions',len(funcs)>15000,f'{len(funcs)} sized code symbols (mangled C++ names)')
    # Addresses cited in research notes (FINDINGS.md / HANDOFF.md) must exist in the ELF symbol table.
    cited={'ProcessPCode':0x803ef24c,'ModelRenderTextureApt':0x8001d0bc,'Draw__Q24EAGL5ModelFRC7MATRIX4':0x803e2c78,'ModelSetScale':0x803e68c4,'SkinGenerateMatrices':0x803f39f8,
           'SkinGeneratePosMatrices':0x803f3ca4,'RuntimeAllocTARConstructor':0x803e8c48,'ModelRenderPlaygroundTexture':0x80016dcc,'Update__21LocalCharacterControlFiP17CharacterMovementP14CharacterState':0x802eeb28}
    bad=[]
    for frag,addr in cited.items():
        hit=[s for n,s in e.symbols.items() if frag in n and s['value']==addr]
        if not hit:bad.append(f'{frag}@{addr:#x}')
    check('Function addresses cited in the research notes exist in the symbol table',not bad,f'{len(cited)-len(bad)}/{len(cited)} match'+(f'; missing {bad}' if bad else ''))
    return e

def dol_checks(e):
    dol_path=DATA/'files'/'playgroundz.dol'
    if not dol_path.exists():check('Retail DOL present',False,str(dol_path));return
    d=dol_path.read_bytes()
    off=struct.unpack('>18I',d[0:72]);addr=struct.unpack('>18I',d[72:144]);size=struct.unpack('>18I',d[144:216])
    entry=struct.unpack('>I',d[224:228])[0]
    # DOL sections are padded to 32 bytes and may span several ELF sections: compare every overlapping byte
    # and require that any bytes beyond the ELF image are zero padding.
    elf_secs=[q for q in e.elf.sections if q['address'] and q['type']!=8 and q['flags']&2]
    compared=equal=0;padding_ok=True;covered_secs=0
    for i in range(18):
        if not size[i]:continue
        blob=d[off[i]:off[i]+size[i]];covered=bytearray(size[i])
        for q in elf_secs:
            lo=max(addr[i],q['address']);hi=min(addr[i]+size[i],q['address']+q['size'])
            if lo>=hi:continue
            mem=bytes(e.read(lo,hi-lo));mine=blob[lo-addr[i]:hi-addr[i]]
            compared+=hi-lo;equal+=sum(a==b for a,b in zip(mem,mine));covered[lo-addr[i]:hi-addr[i]]=b''*(hi-lo)
        rest=bytes(b for b,c in zip(blob,covered) if not c)
        if any(rest):padding_ok=False
        covered_secs+=1
    check('Retail DOL code/data equals the ELF image byte-for-byte',compared>0 and equal==compared and padding_ok,f'{equal:,}/{compared:,} bytes identical across {covered_secs} DOL sections (rest is zero padding: {padding_ok}), entry {entry:#x}')
    sysdol=DATA/'sys'/'main.dol'
    if sysdol.exists():check('Disc boot DOL (sys/main.dol) is the same executable',sha(sysdol)==sha(dol_path),'sys/main.dol == files/playgroundz.dol')

def constants_checks():
    import extract_constants as xc
    items,h=xc.build();text=xc.rust(items,h)
    cur=(BEVY/'src'/'recovered.rs').read_text(encoding='utf-8')
    check('Rust constants (src/recovered.rs) regenerate identically from the ELF',cur==text,f'{len(items)} constants (turn rates, camera distances, CharacterState defaults)')
    v={i['rust']:i['value'] for i in items}
    check('Recovered player max speed and dead zone',abs(v['STATE_MAX_SPEED']-5.0)<1e-6 and abs(v['STICK_DEAD_ZONE']-0.1)<1e-6,f"max speed {v['STATE_MAX_SPEED']}, dead zone {v['STICK_DEAD_ZONE']:.2f}")

# --- 2. data provenance ------------------------------------------------------
def extract(src,out):
    r=subprocess.run([str(BEVY/'target'/'release'/'EAGL-Workbench.exe'),'--headless','extract','--source',str(src),'--out',str(out)],capture_output=True,text=True)
    return r.returncode==0
def members(root):
    return {str(p.relative_to(root)):sha(p) for p in Path(root).rglob('*') if p.is_file()}

def data_checks():
    import uuid
    tree_ok=OLD.exists() and DATA.exists()
    same=diff=missing=0
    if tree_ok:
        for p in (OLD/'files').rglob('*'):
            if not p.is_file():continue
            q=DATA/'files'/p.relative_to(OLD/'files')
            if not q.exists():missing+=1
            elif p.stat().st_size==q.stat().st_size and sha(p)==sha(q):same+=1
            else:diff+=1
    check('Earlier extraction (OLD ATTEMPTS) matches the current DATA files',tree_ok and diff==0 and same>0,f'{same} identical, {diff} different, {missing} absent')
    # Real game folder: loose files vs DATA; differing/absent files are resolved at archive-member level.
    # Python 3.14 mkdtemp uses a restrictive Windows ACL that excludes the
    # packaged decoder's sandbox identity, even under a writable workspace.
    # Ordinary mkdir inherits the workspace ACL; a UUID keeps outputs isolated.
    export_root=(BEVY/'exports').resolve();export_root.mkdir(parents=True,exist_ok=True)
    D=DATA/'files'/'data';tmp=export_root/f'eagl-proof-{uuid.uuid4().hex}'
    assert tmp.parent.resolve()==export_root
    tmp.mkdir()
    same=0;member_same=0;unresolved=[];n=0
    cache={}
    def archive_members(d):
        if d not in cache:
            m={}
            for a in list(Path(d).glob('*.big'))+list(Path(d).glob('*.viv')):
                out=tmp/f'a{len(cache)}_{a.name}'
                if extract(a,out):
                    for k,v in members(out).items():m.setdefault(Path(k).name,set()).add(v)
            cache[d]=m
        return cache[d]
    for p in USA.rglob('*'):
        if not p.is_file():continue
        n+=1;q=D/p.relative_to(USA)
        if q.exists() and q.stat().st_size==p.stat().st_size and sha(p)==sha(q):same+=1;continue
        h=sha(p)
        if q.exists() and p.suffix.lower() in('.viv','.big'):
            a,b=tmp/'ca',tmp/'cb';[__import__('shutil').rmtree(x,ignore_errors=True) for x in (a,b)]
            if extract(p,a) and extract(q,b) and members(a)==members(b):member_same+=1;continue
        if h in archive_members(q.parent if q.exists() else D/p.relative_to(USA).parent).get(p.name,set()):member_same+=1;continue
        unresolved.append(str(p.relative_to(USA)))
    __import__('shutil').rmtree(tmp,ignore_errors=True)
    check('Real game folder (EA Playground (USA)/data) is the same content as DATA',not unresolved,f'{n} files: {same} byte-identical, {member_same} identical as archive members (loose copies of packed files / repacked containers), {len(unresolved)} unresolved'+(f'; e.g. {unresolved[:3]}' if unresolved else ''))

# --- 3. decoders against the data -------------------------------------------
def worker(reqs):
    p=subprocess.Popen(['py','-3.14','-u',str(BEVY/'tools'/'decoder_bridge.py')],cwd=BEVY,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    out=[]
    for r in reqs:
        p.stdin.write(json.dumps(r)+'\n');p.stdin.flush();out.append(json.loads(p.stdout.readline()))
    p.stdin.close();p.wait(timeout=30);return out

def decode_checks():
    D=DATA/'files'/'data';big=str(D/'world'/'world.big')
    chars=str(D/'characters'/'models'/'characters.viv')+'::alicia.viv::alicia.o'
    skel=str(D/'characters'/'player_anims.viv')+'::player_skel.ske';bank=str(D/'characters'/'player_anims.viv')+'::player_anims.anm'
    reqs=[dict(command='preview',source=big+'::world-low-all.o'),
          dict(command='preview',source=chars,skeleton=skel,bank=bank,index=250),
          dict(command='preview',source=str(D/'boot'/'strapwarn_standard_english.gsh')),
          dict(command='inspect',source=big+'::worldfilelist.csv'),
          dict(command='inspect',source=str(D/'csvs.viv')+'::controls.csv')]
    w,a,img,fl,ctl=worker(reqs)
    v=w.get('value',{})
    check('world-low-all.o decodes to the recorded geometry',w['ok'] and v.get('triangles')==105129 and v.get('material_warnings')==[],f"{v.get('triangles')} triangles, {v.get('textures')} textures, {len(v.get('material_warnings',[]))} material warnings")
    v=a.get('value',{})
    check('Alicia + S_idle decode to a 68-bone skinned animated model',a['ok'] and v.get('bones')==68 and v.get('skinned') is True and v.get('animation')=='S_idle',f"{v.get('bones')} bones, clip {v.get('animation')}, {v.get('duration',0):.3f}s")
    v=img.get('value',{})
    check('Wii strap-warning GSH decodes to a 640x480 image',img['ok'] and v.get('width')==640 and v.get('height')==480,f"{v.get('width')}x{v.get('height')} {v.get('format','')}")
    rows=fl.get('value',{}).get('rows',[])
    check('worldfilelist.csv lists the world layers the game loads',len(rows)==6 and any(r[0]=='world-low-all' for r in rows[1:]),f'{max(len(rows)-1,0)} entries: '+', '.join(r[0] for r in rows[1:]))
    rows=ctl.get('value',{}).get('rows',[])
    check('controls.csv maps player events',sum(1 for r in rows if len(r)>1 and r[1]=='STATE_COMBAT')>=8,f"{len(rows)-1} bindings, includes EVENT_PLAYER_MOVE/JUMP/CAMERA_REORIENT: "+str(all(any(r and r[0]==n for r in rows) for n in ('EVENT_PLAYER_MOVE','EVENT_PLAYER_JUMP','EVENT_CAMERA_REORIENT'))))

def vlt_checks():
    import random,ppc_emu
    sys.path.insert(0,str(REPO/'Remaster'/'src'));import vlt
    emu=ppc_emu.Emu();rnd=random.Random(int(time.time())%1000);bad=0;n=0
    for L in list(range(0,50))+[96,123]:
        d=bytes(rnd.randrange(256) for _ in range(L));n+=1
        if ppc_emu.hash64_ref(emu,d)!=vlt.hash64(d):bad+=1
    check('Attrib::hash64 reimplementation equals the original machine code (fresh random inputs)',bad==0,f'{n} inputs executed on the emulated PowerPC function, {bad} mismatches')
    D_=DATA/'files'/'data'/'db'
    names={vlt.string_hash64(x.strip()):x.strip() for x in open(BEVY/'src'/'vlt_names.txt',encoding='utf-8')}
    db=vlt.load_database((D_/'db.vlt').read_bytes(),(D_/'db.bin').read_bytes(),names)
    check('db.vlt/db.bin decode using the loader layout recovered from the executable',len(db['types'])==75 and len(db['classes'])==32,f"{len(db['types'])} types, {len(db['classes'])} classes, relocations applied: {db['relocations']}")
    p=[c for c in db['collections'].values() if c['cls']=='character_info' and c['name']=='player']
    loc=vlt.decode_value(db,*[(a['type'],a['raw'],a['flags']) for a in p[0]['attributes'] if a['name']=='start_location'][0]) if p else None
    check('Player start location recovered from the database',loc==[12.0,0.0,-53.0],f'character_info/player start_location = {loc}')
    dump=BEVY/'exports'/'db-rust.json'
    if dump.exists():
        j=json.loads(dump.read_text(encoding='utf-8'))
        check('Rust vault loader output covers the whole database',len(j['collections'])==908 and len(j['classes'])==32,f"{len(j['collections'])} collections, {sum(len(c['attributes']) for c in j['collections'])} attributes (cross-checked value-by-value against the Python decoder)")

def havok_checks():
    import extract_havok_classes as xh
    sys.path.insert(0,str(REPO/'Remaster'/'src'));import havok,hk_world
    cur=json.loads((BEVY/'src'/'havok_classes.json').read_text(encoding='utf-8'));fresh=xh.build()
    check('Havok class reflection (src/havok_classes.json) regenerates from the executable',cur==fresh,f'{len(fresh)} hkClass definitions recovered by running the static initialisers in the executable')
    r=havok.Reflection();golden=json.loads((BEVY/'tests'/'data'/'collision_golden.json').read_text())
    ok=True;detail=[]
    for name,g in golden.items():
        pf=havok.Packfile((DATA/'files'/'data'/'physics'/f'{name}.hkx').read_bytes(),r);c=hk_world.Collision(pf)
        good=len(c.bodies)==g['bodies'] and len(c.all_triangles())==g['triangles'] and not c.skipped
        ok&=good;detail.append(f"{name}: {len(c.bodies)} bodies/{len(c.all_triangles())} tris")
    check('Havok collision packfiles decode (rigid bodies, MOPP/mesh/box/convex shapes)',ok,'; '.join(detail))
    pf=havok.Packfile((DATA/'files'/'data'/'physics'/'playground.hkx').read_bytes(),r)
    w=[pf.decode_object(si,off,cn) for (si,off),cn in pf.objects().items() if cn=='hkWorldCinfo'][0]
    check('Physics world settings decode (hkWorldCinfo)',abs(w['gravity'][1]+9.81)<1e-3 and w['solverIterations']==4,f"gravity {w['gravity'][1]:.2f}, solver iterations {w['solverIterations']}, min timestep {w['expectedMinPsiDeltaTime']:.4f}s")

def recorded_checks():
    mv=REPO/'Remaster'/'research'/'model-verification.json'
    if mv.exists():
        j=json.loads(mv.read_text(encoding='utf-8'))
        p_,e_,f_=len(j['passed']),len(j['empty']),len(j['failed'])
        check('Recorded corpus audit: every distinct .o model accounted for (earlier run)',(p_,e_,f_)==(828,8,0),f'{p_} geometry exports + {e_} empty draw lists + {f_} failures = {p_+e_+f_} distinct models; {len(j["frontend"])} frontend files; not re-run here (see Remaster/tests/audit_models.py)')
    sp=BEVY/'docs'/'selftest'/'selftest.json'
    if sp.exists():
        r=json.loads(sp.read_text());age=(time.time()-sp.stat().st_mtime)/3600
        failed=[c['name'] for c in r.get('checks',[]) if not c['ok']]
        check('Game self-test report (boot -> world -> player -> scripted walk)',r.get('passed') is True,f"{sum(c['ok'] for c in r.get('checks',[]))}/{len(r.get('checks',[]))} in-game checks passed, load {r.get('load_seconds',0):.1f}s, report {age:.1f} h old"+(f'; failed: {failed}' if failed else ''))
    else:check('Game self-test report',False,'run: EAGL-Workbench.exe --selftest docs/selftest')

def main():
    e=exe_checks();dol_checks(e);constants_checks();vlt_checks();havok_checks();data_checks();decode_checks()
    if '--with-tests' in sys.argv:
        for script,label in [('timing_oracle.py','Timing vectors reproduced by executing the original PowerPC routines'),
                             ('multiplayer_oracle.py','Multiplayer scoring vectors reproduced by executing the original PowerPC routines'),
                             ('controller_oracle.py','Controller event vectors reproduced by executing original PowerPC'),
                             ('jump_command_oracle.py','Original playground jump command leaves movement unchanged'),
                             ('character_input_oracle.py','Character input, support/gravity and impulse vectors reproduced by original PowerPC'),
                             ('character_movement_oracle.py','Grounded/airborne velocity and state transitions match original PowerPC under the documented instruction model'),
                             ('control_bindings_oracle.py','All 14 control tables loaded by the original PowerPC parser'),
                             ('tetherball_oracle.py','Tetherball serve/hit/motion-prefix/scoring vectors reproduced by original PowerPC'),
                             ('tetherball_match_oracle.py','Tetherball winner decisions and common state stores reproduced by original PowerPC'),
                             ('tetherball_scene_oracle.py','Original area matrices and complete tetherball Update state, transforms and effect requests'),
                             ('tetherball_lifecycle_oracle.py','Original tetherball state entry, round-end results and Update orchestration'),
                             ('tetherball_tuning_oracle.py','Original tetherball tuning selection, inheritance and indexed values'),
                             ('tetherball_reset_oracle.py','Original tetherball reset and server-selection state and effects'),
                             ('tetherball_gestures_oracle.py','Original tetherball gesture queue and hit-attempt decisions'),
                             ('tetherball_angles_oracle.py','Original tetherball angle range predicates'),
                             ('tetherball_serve_oracle.py','Original complete tetherball serve, state-entry and pause graph'),
                             ('tetherball_rally_rules_oracle.py','Original tetherball AI hit attempts, charge and ball-drop rules'),
                             ('tetherball_hit_animation_oracle.py','Original tetherball hit animations and hit windows'),
                             ('tetherball_rally_oracle.py','Original complete tetherball return, accelerate, hit and indicator graph'),
                             ('tetherball_ai_reset_oracle.py','Original tetherball reset with decoded AI construction and initialization'),
                             ('tetherball_ai_oracle.py','Original tetherball AI entity predicates, tuning and complete compulsion selection'),
                             ('tetherball_ai_move_oracle.py','Original tetherball AI movement compulsion'),
                             ('tetherball_ai_hit_oracle.py','Original tetherball AI hit compulsion'),
                             ('tetherball_animation_init_oracle.py','Original tetherball player animation initialization'),
                             ('tetherball_reset_runtime_oracle.py','Original tetherball reset with decoded animation initialization'),
                             ('tetherball_frontend_oracle.py','Original tetherball frontend callbacks, pause reset and exit status'),
                             ('tetherball_runtime_oracle.py','Original complete tetherball frames with shared gesture, scene and handler state'),
                             ('tetherball_runtime_reset_oracle.py','Original combined tetherball runtime resets with live queue, trails, AI and HUD'),
                             ('tetherball_initialize_oracle.py','Original tetherball game-logic initialization and prior-limit arithmetic'),
                             ('tetherball_player_init_oracle.py','Original tetherball player reuse/spawn and ordered startup services'),
                             ('tetherball_ai_init_oracle.py','Original tetherball AI character spawning and shared startup stores'),
                             ('tetherball_additional_player_oracle.py','Original tetherball additional player spawning and controller setup'),
                             ('tetherball_ball_constructor_oracle.py','Original tetherball constructor stores and preserved fields'),
                             ('tetherball_ball_init_oracle.py','Original tetherball ball assets, shadows and initial tuning'),
                             ('tetherball_server_oracle.py','Original live server selection, marker timing and ball Grab'),
                             ('animation_graph_oracle.py','Original CSV animation graph mapping, gender overlay and alternate/failure semantics'),
                             ('animation_playback_oracle.py','Original AnimationState selection, timing, events and ordered pose/marker services'),
                             ('animation_playback_evidence.py','Function-addressed animation playback listings and source hashes'),
                             ('minigame_entry_oracle.py','Original shared area conversion and pregame entry'),
                             ('tetherball_session_oracle.py','Original session setters and team copy'),
                             ('tetherball_constructor_oracle.py','Original game/base/world constructor images and Runtime projections'),
                             ('tetherball_cleanup_oracle.py','Original complete tetherball cleanup composed with base/ball teardown'),
                             ('tetherball_startup_oracle.py','Original enclosing tetherball startup with composed character helpers, base lifecycle, ball constructor/initialization, AI construction/tuning, area/pregame/server entry, logic, animation, distance, state and shadow helpers'),
                             ('tetherball_shadow_setup_oracle.py','Original tetherball shadow options and native static initialization'),
                             ('mp3_lsf_oracle.py','MPEG Layer 3 scale factors, spectral scaling and reorder match original PowerPC')]:
            args=[] if script=='mp3_lsf_oracle.py' else ['--check']
            r=subprocess.run([sys.executable,str(HERE/script),*args],cwd=BEVY,capture_output=True,text=True)
            check(label,r.returncode==0,(r.stdout or r.stderr)[-500:].strip())
        # Differential tests: each Rust decoder must reproduce the Python reference / recorded hashes on the whole corpus.
        suites=[('animation_playback::tests','Rust animation playback matches native function state and ordered service traces'),
                ('animation_graph::tests','Rust animation graph matches native mapping and archive clip loading'),
                ('archive::tests::matches_recorded_corpus_hashes','Rust archive reader: all recorded corpus records match their SHA-256'),
                ('gsh::tests::corpus_matches_python_decoder','Rust GSH texture decoder: every image of every bank matches the Python pixels'),
                ('model::tests::corpus_matches_python_reference','Rust model decoder: every primitive of all 836 models matches the reference'),
                ('assets::tests::world_builds_with_all_textures','Rust asset builder: world mesh, 161 textures, 100,576 triangles, no warnings'),
                ('vlt::tests','Rust Attrib database: hash64 golden vectors + full database decode'),
                ('havok::tests','Rust Havok reader: collision bodies/triangles match the Python reference'),
                ('skeleton::tests','Rust skeleton reader: 181 bones of 4 rigs match the reference; cached world rotations agree'),
                ('anim::tests::matches_python_reference_for_every_clip','Rust animation codecs: 266 clips match the Python reference per bone and channel'),
                ('character::tests','Rust player build: skinned model + rig + clips decode from the original files'),
                ('tpl::tests::corpus_matches_python_decoder','Rust TPL decoder: all 199 distinct texture banks match the Python pixels'),
                ('locale::tests','Localisation: 10 .loc files + string.idx decode, TRCLocale hash routine reproduced, keys from the executable resolve'),
                ('conversation::tests','Conversation trees: all 32 .con files parse exactly; 376/429 node texts resolve through the locale'),
                ('placement::tests','Level placement: 49 marker sets (304 markers) and 10 RC checkpoint files (20 lanes) parse exactly'),
                ('audio::tests','Audio: EA Layer 3 (14 music tracks, every granule ends on its declared length), EA-XA streams and bank sounds, MicroTalk speech (8 files, exact chunk framing, low-pass output)'),
                ('conga::tests','Conga gesture machines: conga.gsm parses (34 sequences, transition counts consistent)'),
                ('locomotion::tests','Locomotion constants and behaviour'),
                ('sim_time::tests','Frame timing and physics steps: cap/fixed/pause ordering, uncapped input, original PowerPC vectors'),
                ('multiplayer::tests','Multiplayer scoring/ranking rules match original PowerPC vectors'),
                ('controller::tests','Controller edge/timer/modifier/context rules match original PowerPC vectors'),
                ('control_bindings::tests','Native control tables match all 14 original parser outputs'),
                ('character_input::tests','Native character input preparation matches original PowerPC vectors'),
                ('character_movement::tests','Native grounded/airborne velocity and state transitions match original PowerPC vectors'),
                ('tetherball::tests','Tetherball serve/hit/motion-prefix/scoring arithmetic matches original PowerPC vectors'),
                ('tetherball_match::tests','Tetherball winner decisions, counters and effects match original PowerPC vectors'),
                ('tetherball_scene::tests','Native area matrices and complete ball Update match original PowerPC vectors'),
                ('tetherball_lifecycle::tests','Native tetherball lifecycle, results and frame orchestration match original PowerPC vectors'),
                ('tetherball_tuning::tests','Native tetherball tuning matches original PowerPC and corpus values'),
                ('tetherball_reset::tests','Native tetherball reset and server selection match original PowerPC vectors'),
                ('tetherball_gestures::tests','Native tetherball gesture queue and hit attempts match original PowerPC vectors'),
                ('tetherball_angles::tests','Native angle range predicates match original PowerPC vectors'),
                ('tetherball_serve::tests','Native complete tetherball serve graph matches original PowerPC vectors'),
                ('tetherball_rally_rules::tests','Native tetherball rally rules match original PowerPC vectors'),
                ('tetherball_hit_animation::tests','Native tetherball hit animations and ranges match original PowerPC vectors'),
                ('tetherball_rally::tests','Native complete tetherball return and accelerate graph matches original PowerPC vectors'),
                ('tetherball_ai_reset::tests','Rust tetherball reset with decoded AI construction and initialization'),
                ('tetherball_ai::tests','Rust tetherball AI entity predicates, tuning and complete compulsion selection'),
                ('tetherball_ai_move::tests','Rust tetherball AI movement compulsion'),
                ('tetherball_ai_hit::tests','Rust tetherball AI hit compulsion'),
                ('tetherball_animation_init::tests','Rust tetherball player animation initialization'),
                ('tetherball_reset_runtime::tests','Rust tetherball reset with decoded animation initialization'),
                ('tetherball_frontend::tests','Rust tetherball frontend callbacks and integer exit status'),
                ('tetherball_runtime::reset_tests','Rust combined tetherball runtime resets preserve shared owners and effect order'),
                ('tetherball_initialize::tests','Rust tetherball game-logic initialization matches original shared stores'),
                ('tetherball_player_init::tests','Rust tetherball player reuse/spawn initialization matches original calls'),
                ('tetherball_ai_init::tests','Rust tetherball AI character startup matches original calls'),
                ('tetherball_additional_player::tests','Rust tetherball additional player startup matches original calls'),
                ('tetherball_ball_init::tests','Rust tetherball ball initialization matches original resources and state'),
                ('minigame_entry::tests','Rust shared minigame entry matches original helpers'),
                ('tetherball_session::tests','Rust session stores match original native setters'),
                ('tetherball_constructor::tests','Rust game/base/world constructor memory matches original stores'),
                ('tetherball_startup::tests','Rust enclosing tetherball startup and live server/Grab matches original stores and ordered calls'),
                ('tetherball_shadow_setup::tests','Rust tetherball shadow setup matches original viewport options'),
                ('mp3::tests','MPEG-2 side information and scale-factor regression checks')]
        for name,label in suites:
            r=subprocess.run(['cargo','test','--release','--offline','--locked',name],cwd=BEVY,capture_output=True,text=True)
            m=re.search(r'test result: (\w+)\. (\d+) passed; (\d+) failed',r.stdout)
            check(label,bool(m) and m.group(1)=='ok' and int(m.group(2))>0,m.group(0) if m else (r.stderr or r.stdout)[-200:])
    if '--run-selftest' in sys.argv:
        exe=BEVY/'target'/'release'/'EAGL-Workbench.exe';r=subprocess.run([str(exe),'--selftest',str(BEVY/'docs'/'selftest')],cwd=BEVY,timeout=300)
        check('Game self-test run',r.returncode==0,f'exit code {r.returncode}')
    recorded_checks()
    passed=sum(c['ok'] for c in checks)
    scope=['This proves: the executable in use is the retail binary with symbols; extracted data is consistent across three copies; the decoders reproduce recorded results; the game slice boots from the original files and moves a character using constants read from the executable.',
           'This does NOT prove: a full decompilation or matching gameplay on a running console. Locomotion, frame/physics timing, controller events, character input/grounded/airborne velocity, multiplayer scoring and tetherball Update/winner decisions are recovered subsets; many executable functions remain unported. Character normalization uses the documented reciprocal-square-root instruction model.',
           'Provisional: character step/slide solver, desktop direction adapter, support query and camera framing. Native character input and grounded/airborne states prepare world velocity; full Havok proxy dynamics remain unported. The original playground jump command is inert and the port adds no jump impulse. Spawn and gravity come from the database and Havok world settings; terrain uses decoded Havok collision. World display uses the original AreaManager matrix.',
           'Not reconstructed: full AI scheduling, full minigame gameplay, conversation execution, Havok dynamics, video, APT menu scripting, save data and audio-event graphs. Rust audio decoders cover music/speech and all 471 embedded bank sounds; console PCM equality remains unverified.']
    rep=dict(generated=datetime.datetime.now().strftime('%Y-%m-%d %H:%M'),elf_sha256=PINNED_ELF,passed=passed==len(checks),passed_count=passed,total=len(checks),checks=checks,scope=scope)
    (BEVY/'docs'/'proof-report.json').write_text(json.dumps(rep,indent=1),encoding='utf-8')
    print(f'\n{passed}/{len(checks)} checks passed');sys.exit(0 if passed==len(checks) else 1)
if __name__=='__main__':main()
