"""
gltf_writer.py
----------------
Low-level glTF/GLB assembly: accessor/bufferView/buffer bookkeeping and
the per-clip GLB byte assembly. Pulled out of anm_exporter.py so the
codec-dispatch logic and the file-format-writing logic are independently
readable/testable.

Has no knowledge of EAGL codecs -- takes already-decoded rotation/
translation dictionaries (bone_index -> per-frame values) plus a parsed
skeleton, and produces GLB bytes. Frame rate here is a documented
placeholder (30fps); true playback FPS was never independently confirmed
during reverse engineering.
"""

import struct
import json

PLACEHOLDER_FPS = 30.0


# ---------------------------------------------------------------------------
# glTF/GLB building blocks
# ---------------------------------------------------------------------------

class GlbBuilder:
    """Accumulates accessors/bufferViews/buffer bytes for one .glb."""

    def __init__(self):
        self.blob = bytearray()
        self.accessors = []
        self.buffer_views = []

    def _pad4(self):
        while len(self.blob) % 4:
            self.blob.append(0)

    def add_floats(self, flat: list[float], comp_type_count: int, gltf_type: str) -> int:
        """flat = flattened floats; comp_type_count = numbers per element
        (1/3/4); gltf_type = 'SCALAR'/'VEC3'/'VEC4'. Returns accessor index."""
        self._pad4()
        offset = len(self.blob)
        packed = struct.pack(f"<{len(flat)}f", *flat)
        self.blob += packed
        count = len(flat) // comp_type_count
        # min/max per component, required-ish for spec compliance
        mins = [min(flat[i::comp_type_count]) for i in range(comp_type_count)]
        maxs = [max(flat[i::comp_type_count]) for i in range(comp_type_count)]
        bv_idx = len(self.buffer_views)
        self.buffer_views.append({
            "buffer": 0, "byteOffset": offset, "byteLength": len(packed),
        })
        acc_idx = len(self.accessors)
        self.accessors.append({
            "bufferView": bv_idx, "componentType": 5126,  # FLOAT
            "count": count, "type": gltf_type,
            "min": mins, "max": maxs,
        })
        return acc_idx


def build_clip_glb(skel, clip_index: int, clip_name: str,
                    rot_by_bone: dict, trans_by_bone: dict,
                    sample_count: int, scale_by_bone: dict | None = None) -> bytes:
    names = {b.index: b.name for b in skel.bones}
    parents = {b.index: b.parent_idx for b in skel.bones}
    n_bones = len(skel.bones)
    scale_by_bone = scale_by_bone or {}

    gb = GlbBuilder()

    # -- nodes: bind pose, same convention as build_skeleton_gltf --
    nodes = []
    for bone in skel.bones:
        tx, ty, tz = bone.local_translation
        qx, qy, qz, qw = bone.quaternion
        children = [c.index for c in skel.children_of(bone.index)]
        node = {
            "name": bone.name,
            "translation": [tx, ty, tz],
            "rotation": [qx, qy, qz, qw],
            "scale": list(bone.scale),
        }
        if children:
            node["children"] = children
        nodes.append(node)
    root_nodes = [b.index for b in skel.bones if b.is_root]

    # -- shared time accessor --
    times = [i / PLACEHOLDER_FPS for i in range(sample_count)]
    time_acc = gb.add_floats(times, 1, "SCALAR")

    channels = []
    samplers = []

    for bone_idx, quats in rot_by_bone.items():
        flat = []
        for q in quats:
            flat.extend(q if q is not None else (0.0, 0.0, 0.0, 1.0))
        out_acc = gb.add_floats(flat, 4, "VEC4")
        samp_idx = len(samplers)
        samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
        channels.append({"sampler": samp_idx, "target": {"node": bone_idx, "path": "rotation"}})

    for bone_idx, vecs in trans_by_bone.items():
        flat = []
        for v in vecs:
            flat.extend(v if v is not None else (0.0, 0.0, 0.0))
        out_acc = gb.add_floats(flat, 3, "VEC3")
        samp_idx = len(samplers)
        samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
        channels.append({"sampler": samp_idx, "target": {"node": bone_idx, "path": "translation"}})

    for bone_idx, vecs in scale_by_bone.items():
        bind_scale = tuple(skel.bones[bone_idx].scale) if bone_idx < n_bones else (1.0, 1.0, 1.0)
        flat = []
        for v in vecs:
            flat.extend(v if v is not None else bind_scale)
        out_acc = gb.add_floats(flat, 3, "VEC3")
        samp_idx = len(samplers)
        samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
        channels.append({"sampler": samp_idx, "target": {"node": bone_idx, "path": "scale"}})

    gltf_json = {
        "asset": {"version": "2.0", "generator": "anm_exporter.py"},
        "scene": 0,
        "scenes": [{"nodes": root_nodes, "name": clip_name}],
        "nodes": nodes,
        "animations": [{
            "name": clip_name,
            "channels": channels,
            "samplers": samplers,
        }],
        "accessors": gb.accessors,
        "bufferViews": gb.buffer_views,
        "buffers": [{"byteLength": len(gb.blob)}],
    }

    json_bytes = json.dumps(gltf_json, separators=(",", ":")).encode("utf-8")
    if len(json_bytes) % 4:
        json_bytes += b" " * (4 - len(json_bytes) % 4)
    bin_bytes = bytes(gb.blob)
    if len(bin_bytes) % 4:
        bin_bytes += b"\x00" * (4 - len(bin_bytes) % 4)

    _GLB_MAGIC, _GLB_VERSION = 0x46546C67, 2
    _CHUNK_JSON, _CHUNK_BIN = 0x4E4F534A, 0x004E4942

    total_len = 12 + 8 + len(json_bytes) + 8 + len(bin_bytes)
    glb = (
        struct.pack("<III", _GLB_MAGIC, _GLB_VERSION, total_len) +
        struct.pack("<II", len(json_bytes), _CHUNK_JSON) + json_bytes +
        struct.pack("<II", len(bin_bytes), _CHUNK_BIN) + bin_bytes
    )
    return glb


