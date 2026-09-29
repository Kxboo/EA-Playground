"""
eagl_anm_decoder.py  (adapter)
------------------------------
Drop-in replacement for the old heuristic eagl_anm_decoder.py. Same
filename, so eagl_ui.py's `_load_anm()` finds it unchanged -- but instead
of the old best-effort quaternion scanner, this wraps the fully-solved
EAGL_ANM_Pipeline (decoder/ + exporter/anm_exporter.py), which gets all
265/265 clips across every codec (FnDeltaQFast, FnDeltaF3/F1,
FnStatelessQ whole-clip, FnDeltaSingleQ).

INSTALL:
  Copy this file next to eagl_ui.py, and copy EAGL_ANM_Pipeline's
  `decoder/` and `exporter/` packages next to it too (or set
  EAGL_ANM_PIPELINE_ROOT to point at the EAGL_ANM_Pipeline folder).

WHAT THIS EXPOSES (matches what eagl_ui.py reads):
  AnmFile(path).clips  -> list[Clip]
    Clip.index          int
    Clip.name            str | None
    Clip.frame_times     list[float]      (placeholder 30fps timeline)
    Clip.duration         float
    Clip.n_tracks         int
    Clip.tracks          list[Track]      (Track.bone_idx only)

  Additionally, each Clip carries the raw decode needed to actually
  write a glb, so the UI's export path can skip eagl_exporter.py's old
  (codec-blind) animation writer entirely:
    Clip.rot_by_bone      dict[int, list[vec4]]
    Clip.trans_by_bone    dict[int, list[vec3]]
    Clip.sample_count     int
    Clip.codec            str   (informational)
    Clip.caveats          list[str]

  AnmFile.export_clip_glb(clip, skel) -> bytes
    Re-decodes `clip` against the REAL skeleton (bone-index range checks
    and F1 bind-pose fallback need actual bone count/bind pose -- the
    load-time decode above uses a size-68 dummy skeleton so clips can be
    listed before a .ske is loaded) and returns ready-to-write .glb bytes
    via the pipeline's own gltf_writer.build_clip_glb. This is what
    eagl_ui.py's _export_selected_clip should call for anm-only export
    (mesh+anim merge is a separate, not-yet-wired path -- see PIPELINE.md).
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Locate the pipeline package (decoder/ + exporter/) and put it on sys.path.
# ---------------------------------------------------------------------------

_HERE = Path(__file__).resolve().parent
_CANDIDATES = [
    Path(os.environ.get("EAGL_ANM_PIPELINE_ROOT", "")),
    _HERE,                              # decoder/ exporter/ copied alongside this file
    _HERE / "EAGL_ANM_Pipeline",        # or as a subfolder
]
for _cand in _CANDIDATES:
    if _cand and (_cand / "decoder").is_dir() and (_cand / "exporter").is_dir():
        if str(_cand) not in sys.path:
            sys.path.insert(0, str(_cand))
        break
else:
    raise ImportError(
        "eagl_anm_decoder adapter: could not find the EAGL_ANM_Pipeline "
        "decoder/ and exporter/ packages. Copy them next to eagl_ui.py, "
        "or set EAGL_ANM_PIPELINE_ROOT to the EAGL_ANM_Pipeline folder."
    )

from decoder.eagl_skeleton import parse_ske_file  # noqa: E402
from exporter.anm_exporter import (  # noqa: E402
    parse_anm_clips, decode_clip_bones,
)
from exporter.gltf_writer import build_clip_glb, PLACEHOLDER_FPS  # noqa: E402

MAX_BONES = 68  # from decoder.eagl_anm_decoder; player_skel.ske's real count


# ---------------------------------------------------------------------------
# Dummy skeleton -- lets us decode bone tables / sample counts for the clip
# list before the user has loaded a .ske, same as the old decoder could
# list clips independently of skeleton state. Real export always re-decodes
# against the actual loaded skeleton (see AnmFile.export_clip_glb).
# ---------------------------------------------------------------------------

@dataclass
class _DummyBone:
    index: int
    local_translation: tuple = (0.0, 0.0, 0.0)
    name: str = "?"

    def __post_init__(self):
        self.name = f"bone{self.index}"


class _DummySkel:
    def __init__(self, n=MAX_BONES):
        self.bones = [_DummyBone(i) for i in range(n)]


# ---------------------------------------------------------------------------
# Old-shape duck-types eagl_ui.py reads directly.
# ---------------------------------------------------------------------------

@dataclass
class Track:
    bone_idx: int


@dataclass
class Clip:
    index: int
    name: str | None
    frame_times: list
    duration: float
    n_tracks: int
    tracks: list

    # extras for direct export, not read by the old UI code but used by
    # AnmFile.export_clip_glb / any caller that wants the raw decode:
    codec: str = ""
    caveats: list = field(default_factory=list)
    sample_count: int = 0
    rot_by_bone: dict = field(default_factory=dict)
    trans_by_bone: dict = field(default_factory=dict)

    # kept for AnmFile.export_clip_glb to re-decode against a real skeleton
    _clip_block: object = None


class AnmFile:
    def __init__(self, path):
        self.path = path
        self._data, self._reloc, self._data_start, self._clip_blocks, clip_names = parse_anm_clips(path)

        dummy_skel = _DummySkel()
        self.clips: list[Clip] = []
        for cb in self._clip_blocks:
            label = clip_names[cb.index] if cb.index < len(clip_names) else None
            result = decode_clip_bones(self._data, self._reloc, self._data_start, dummy_skel, cb)

            if result["status"] == "skip":
                # Still surface the clip (name/index), just with no tracks,
                # so it's visible in the UI table instead of silently
                # vanishing -- mirrors the old decoder's "best effort" spirit
                # but is honest about zero decoded tracks.
                self.clips.append(Clip(
                    index=cb.index, name=label, frame_times=[], duration=0.0,
                    n_tracks=0, tracks=[], codec="unsupported", caveats=[result["reason"]],
                    _clip_block=cb,
                ))
                continue

            rot_by_bone = result["rot_by_bone"]
            trans_by_bone = result["trans_by_bone"]
            sample_count = result["sample_count"]
            animated_bones = sorted(set(rot_by_bone) | set(trans_by_bone))
            frame_times = [i / PLACEHOLDER_FPS for i in range(sample_count)]
            duration = frame_times[-1] if frame_times else 0.0

            self.clips.append(Clip(
                index=cb.index, name=label, frame_times=frame_times, duration=duration,
                n_tracks=len(animated_bones), tracks=[Track(bone_idx=b) for b in animated_bones],
                codec=result["codec"], caveats=result["caveats"], sample_count=sample_count,
                rot_by_bone=rot_by_bone, trans_by_bone=trans_by_bone,
                _clip_block=cb,
            ))

    def export_clip_glb(self, clip: Clip, skel, rotation_only: bool = False) -> bytes:
        """Re-decode `clip` against the real, loaded skeleton and return
        .glb bytes. Use this instead of eagl_exporter.build_anim_only_gltf
        -- that writer doesn't know these codecs.

        rotation_only: if True, the translation track is dropped entirely
        (bones stay at their bind-pose translation for every frame) --
        useful for isolating whether a visual artifact comes from the
        rotation or translation decode."""
        result = decode_clip_bones(self._data, self._reloc, self._data_start, skel, clip._clip_block)
        if result["status"] == "skip":
            raise RuntimeError(f"clip {clip.index} ({clip.name}): {result['reason']}")
        label = clip.name or f"clip_{clip.index}"
        trans_by_bone = {} if rotation_only else result["trans_by_bone"]
        return build_clip_glb(
            skel, clip.index, label,
            result["rot_by_bone"], trans_by_bone, result["sample_count"],
        )
