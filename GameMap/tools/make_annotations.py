#!/usr/bin/env python3
"""Write data/annotations.json: reviewed function summaries with their evidence.

Sources: (1) the hand-reviewed dictionary below (every entry is backed by the disassembly and a document/verifier),
(2) the APT handler bindings recovered in data/apt_handlers.tsv (constructors and DoJob* dispatchers).

An annotation may carry `verified_by` when the behaviour was proven by emulation against a reference model
(tools/verify_*.py); otherwise it is *reviewed* only.
"""
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data")

D01, D02, D03, D04, D05, D07 = ("docs/01-executable-and-boot.md", "docs/02-state-machine-and-timing.md", "docs/03-input.md",
                                 "docs/04-frontend-and-menus.md", "docs/05-minigames-and-rules.md",
                                 "docs/07-data-formats-and-hashes.md")
MP = "tools/verify_multiplayer.py (66,264 comparisons vs reference/multiplayer_mode.py)"
PG = "tools/verify_postgame.py (3,000 randomized results vs reference/multiplayer_mode.py:post_game_awards)"
HS = "tools/verify_hashes.py (309 strings, emulated original) + real db.vlt resolves 303 strings"

# mangled name -> (summary, doc, verified_by | None)
MANUAL = {
    "main": ("Entry point: calls MainThread and returns 0.", D01, None),
    "MainThread__FiPv": ("InitAllModules(); BootSequence(false); loops `while (GameState::Update())`; then GameState::Shutdown and "
                         "ShutdownAllModules. The `skipfe` argument comparison is dead code (result discarded).", D01, None),
    "BootSequence__Fb": ("GameState::Init(), then SetNewState(BootFlow) and SetBootFlowState(1).", D01, None),
    "InitAllModules__Fv": ("Module init order: OS/DVD/VI/PAD, MemMgr pools, EA base (thread/print/timer/filesys), renderer + fonts, TRC, "
                           "disc.ini, pgIO/AssetManager, wrist-strap reminder loop, scene, ASYNCFILE, pgIDatabase, Audio, movie player.", D01, None),
    "ShutdownAllModules__Fv": ("Reverse of InitAllModules for the game-level modules.", D01, None),
    "SetBootOptionsFromIni__FPCc": ("Reads disc.ini keys region (eu/us), productcode (SetProductCode) and parentallock (atoi -> SetParentalLock); "
                                    "defaults RET000000 / 0. `authserver` is never read.", D01, None),
    "SetProductCode__FPCc": ("Copies the product code into the global productCode buffer and sets its 'set' flag.", D01, None),
    "SetParentalLock__FUi": ("Stores the parental-lock level and its 'set' flag.", D01, None),
    "Update__9GameStateFv": ("Per-frame driver: runs SYNCTASKs, computes integer-ms dt from the time base (cap gCappedMillisecondsPerFrame=60, "
                             "fixed gSimFixedTimeAmt=16, pause), dispatches the current state through the jump table at 0x804e885c, then updates the Controller.", D02, None),
    "SetNewState__9GameStateFRCQ29GameState5StatePCc": ("Stores sCurrentGameState; entering FrontEnd(5)/Playground(6) sets Audio mode 0/1.", D02, None),
    "STATEFN_UPDATE_BootFlow__9GameStateFUl": ("Boot flow: intro movie(s), boot check, wrist-strap screen; then Boot2FE.", D02, None),
    "STATEFN_UPDATE_Boot2FE__9GameStateFUl": ("One-off: builds memory pools, physics, AI manager, world and cameras, then enters FrontEnd.", D02, None),
    "STATEFN_UPDATE_FrontEnd__9GameStateFUl": ("Front-end frame: TRC checks, audio, selections, FEManager update, sky, world, cameras, APT (AIP::Update), draw x5.", D02, None),
    "STATEFN_UPDATE_FE2PG__9GameStateFUl": ("Front end -> playground: WorldMan::Initialize, camera reinit, SetNewState(Playground), music/ambience, remotes cleanup.", D02, None),
    "STATEFN_UPDATE_PG2FE__9GameStateFUl": ("Playground -> front end: WorldMan::UnInitialize, audio unload, camera reset, SetNewState(FrontEnd).", D02, None),
    "STATEFN_UPDATE_FE2MP__9GameStateFUl": ("Front end -> minigame: WorldMan::StartMinigame(MGID, level, difficulty, Teams, rules), SetNewState(Playground).", D02, None),
    "STATEFN_UPDATE_MP2FE__9GameStateFUl": ("Minigame -> front end: opens screens, SetNewState(FrontEnd), MultiplayerMode::EndMultiplayerGame.", D02, None),
    "STATEFN_UPDATE_Playground__9GameStateFUl": ("Playground frame: FEManager, audio, PGConga, HandleActions, AI, sky, WorldMan::Update, particles, DB, cameras, APT HUD, draw x5.", D02, None),
    "Init__9GameStateFv": ("Builds scenes, big file, Controller, light/shadow managers, sky dome, FEManager, PGConga, CharacterProfile, MultiplayerMode, debug menu; seeds RNG.", D02, None),
    "Shutdown__9GameStateFv": ("Reverses Init.", D02, None),
    "HandleActions__9GameStateFi": ("World-level actions: free camera, opens sticker book cover and report card, debug menu; consults WorldMan::IsInMicrogame.", D02, None),
    "HandleSelections__9GameStateFi": ("Front-end debug-menu input subset.", D02, None),
    "ComputeHash__FPCc": ("Locale key hash: h=0xFFFFFFFF; for each signed byte c: h = h*33 + c (mod 2^32).", D07, HS),
    "StringIndexEntryComparer__FPCvPCv": ("bsearch comparator: unsigned compare of the first word of two {hash,index} records.", D07, None),
    "FindStringIndex__6LocaleCFUi": ("bsearch over the sorted 8-byte {hash,index} records of string.idx; returns index or -1.", D07, None),
    "GetString__6LocaleCFPCc": ("Localised string by id: ComputeHash(id) -> GetString(int); debug flag returns the id itself.", D07, None),
    "GetString__6LocaleCFi": ("FindStringIndex then LOCALE_getstr on the loaded .loc.", D07, None),
    "__ct__6LocaleF9eLocaleDb11eLanguageId": ("Loads data\\locale\\<LANG>.loc and (once) string.idx.", D07, None),
    "StringHash64__6AttribFPCc": ("Attrib key: lookup8 hash (seed 0xABCDEF0011223344) of the string; 0 for null/empty.", D07, HS),
    "hash64__6AttribFPCUcUiUx": ("Bob Jenkins lookup8 hash() with 24-byte blocks and mix64.", D07, HS),
    "StringToKey__6AttribFPCc": ("Tail-jump to StringHash64.", D07, None),
    "StringToAssetID__6AttribFPCc": ("Tail-jump to StringToKey.", D07, None),
    "StringToTypeID__6AttribFPCc": ("Tail-jump to StringToKey.", D07, None),
    "Initialize__10ControllerF11ControlTypei": ("Loads controls*.csv for the ControlType and builds 100-byte rows (action event, state, transition, kind, modifiers, button).", D03, None),
    "ConvertStringToActionEvent__F7CString": ("188-token if-chain: EVENT_* string -> EActionEvent (fallback 190).", D03, None),
    "ConvertStringToControllerState__F7CString": ("STATE_* string -> EControllerState (fallback 31).", D03, None),
    "ConvertStringToControllerEvent__F7CString": ("BUTTON_* string -> button-event kind 0..7 (fallback 9).", D03, None),
    "ConvertStringToButton__F7CString": ("Button token (with optional ~) -> button id 0..13.", D03, None),
    "GetStringField__10cCSVParserFPCcR7CString": ("Looks a column up by header name and returns the (left-trimmed) field.", D03, None),
    "GetString__10cCSVParserFRPcRPcRUic": ("Reads the next comma-delimited token and trims leading whitespace.", D03, None),
    "Update__14PhysicsManagerFi": ("Clamps dt to 200 ms, splits into ceil(dt/60) sub-steps (integer distribution) and calls hkWorld::stepDeltaTime(slice*mult/1000).", D02, None),
    "OpenPostGameScreen__8MinigameFQ25Enums12MinigameTypeP12PostGameInfo": ("Writes results into MultiplayerMode: 1v1 winner +50; team games +50 per member of the winning team; "
                                                                           "free-for-all 50/25/10; then opens the PostGame APT screen.", D05, PG),
    "StartMinigame__8WorldManF4MGIDiQ25Enums23MiniGameDifficultyLevelRC5TeamsQ25Enums16MiniGameDareType": ("Begins a minigame: fade-in, unload audio; construction happens in StartMinigameFadeComplete.", D05, None),
    "StartMinigameFadeComplete__8WorldManFv": ("Saves the player's conversation pose, unspawns the area, constructs the MG* subclass by type, calls SetUpTeams, adds it to the scene.", D05, None),
    "EndMinigame__8WorldManFb": ("Fade out, remove the minigame, restore area/audio/pose, CharacterProfile::EndMiniGame, back to local control; SetNewState(MP2FE).", D05, None),
    "Update__9FEManagerFUi": ("Ticks AIP; in eFrontendState 1 dispatches the per-eFrontEndGameState update through the jump table at 0x804d6e2c.", D04, None),
    "EnterFEGameState__9FEManagerF18eFrontEndGameState": ("Entry work per menu state: opens APT screens (Title, Profile, ConfirmKid, MainMenu), spawns selectable kids, name keyboard, fade; -1 enters the world.", D04, None),
    "EnterState__9FEManagerF14eFrontendState": ("Creates the AIP handler families for the front-end state (0 loading, 1 menus, 3 world/HUD, 4 loading overlay).", D04, None),
    "OpenAptScreen__9FEManagerFPc": ("AptCallFunction(\"OpenScreen\", \"_root\", name).", D04, None),
    "CloseAptScreen__9FEManagerFv": ("AptCallFunction(\"CloseScreen\", \"_root\").", D04, None),
    "OpenAptOverlay__9FEManagerFPc": ("AptCallFunction(\"OpenOverlay\", \"_root\", name).", D04, None),
    "CloseAptOverlay__9FEManagerFv": ("AptCallFunction(\"CloseOverlay\", \"_root\").", D04, None),
    "ReplaceAptScreen__9FEManagerFPc": ("AptCallFunction(\"ReplaceScreen\", \"_root\", name).", D04, None),
    "ClearScreenStack__9FEManagerFv": ("AptCallFunction(\"ClearScreenStack\", \"_root\").", D04, None),
    "SetState__9FEManagerF14eFrontendState": ("Leave/enter eFrontendState (+0x3c).", D04, None),
    "SetFEGameState__9FEManagerF18eFrontEndGameState": ("Leave/enter eFrontEndGameState (+0x40).", D04, None),
}

MP_METHODS = {
    "constructor": "Zero-initialises the tournament state (active=0, MGID=-1, Teams ctor).",
    "destructor": "Destroys the tournament state.",
    "Create": "Allocates the 0x128-byte singleton (pool name 'MultiplayerMode') and stores it in the global.",
    "Destroy": "Destroys the singleton.",
    "SetupMultiplayerGame": "Marks a session active, copies the MGID and the Teams block (skipping Teams word 1).",
    "EndMultiplayerGame": "Clears the active flag.",
    "SetRules": "Copies 5 rule ints.",
    "StartFreePlay": "Resets wins/ranks; not a point series.",
    "StartPointSeries": "Resets points and wins, sets point-series flag and rounds_left.",
    "AddRoundResults": "Adds round points (players 2,3 only if != -1), re-ranks with previous-rank tie-break, counts the round.",
    "AddWinResults": "Credits up to two winners, re-ranks by wins with tie-break, records last winners.",
    "SetLastPlacement": "Stores last placement; quirk: args c and d share one slot, 4th slot never written.",
    "GetNumRoundsLeft": "Returns rounds_left.", "GetWinTotal": "Returns wins[p].", "GetPointTotal": "Returns points[p].",
    "GetPlayerRank": "Rank from points in a point series, else from wins.",
    "WonLastGame": "True if p is among last_winners.", "GetPlayerNumByRank": "Player index with the given rank, or -1.",
    "GetPlayerPointsInThisMatch": "Points added in the most recent round.",
}


def main():
    funcs = {}
    for fl in os.listdir(os.path.join(DATA, "functions")):
        for l in open(os.path.join(DATA, "functions", fl), encoding="utf-8"):
            if l.startswith("#") or not l.strip():
                continue
            p = l.rstrip("\n").split("\t")
            funcs[p[7]] = {"addr": int(p[0], 16), "cls": p[2], "method": p[3]}
    by_addr = {v["addr"]: k for k, v in funcs.items()}
    ann = {}
    missing = []
    for name, (summary, doc, ver) in MANUAL.items():
        if name not in funcs:
            missing.append(name)
            continue
        ann[name] = {"summary": summary, "doc": doc, **({"verified_by": ver} if ver else {})}
    for name, v in funcs.items():
        if v["cls"] == "MultiplayerMode" and v["method"] in MP_METHODS:
            ann[name] = {"summary": MP_METHODS[v["method"]], "doc": D05, "verified_by": MP}
    # APT handler bindings
    hs = collections.defaultdict(list)
    for l in open(os.path.join(DATA, "apt_handlers.tsv"), encoding="utf-8"):
        if l.startswith("#") or not l.strip():
            continue
        p = l.rstrip("\n").split("\t")
        hs[p[0]].append(p)
    for cls, rows in hs.items():
        names = [r[3] for r in rows]
        kind = rows[0][1]
        what = "fscommand" if kind == "FS" else "loadVariables"
        ctor = int(rows[0][6], 16)
        disp = rows[0][5]
        if ctor in by_addr:
            ann[by_addr[ctor]] = {"summary": "Registers %d APT %s handlers with sequential job indexes: %s." % (len(names), what, ", ".join(names[:8]) + ("…" if len(names) > 8 else "")),
                                  "doc": D04}
        if disp and int(disp, 16) in by_addr:
            ann[by_addr[int(disp, 16)]] = {"summary": "Dispatches APT %s job index -> native handler for %s (%d names)." % (what, cls, len(names)), "doc": D04}
    with open(os.path.join(DATA, "annotations.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(ann, f, indent=0, sort_keys=True)
    print("annotations:", len(ann), "missing manual names:", missing)


if __name__ == "__main__":
    main()
