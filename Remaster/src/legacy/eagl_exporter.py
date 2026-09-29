"""
eagl_exporter.py
----------------
Standalone glTF 2.0 (GLB) exporter for EA Sports EAGL Wii .o model data,
with optional skeleton (eagl_skeleton.SkeletonResult) and animation
(eagl_anm_decoder.AnmFile) embedding.

Usage (mesh only):
    from eagl_exporter import build_gltf
    glb_bytes = build_gltf(result)
    glb_bytes = build_gltf(result, merge_meshes=False)

Usage (mesh + skeleton + animations):
    from eagl_exporter import build_gltf
    from eagl_skeleton import parse_ske_file
    from eagl_anm_decoder import AnmFile

    skel = parse_ske_file("player_skel.ske")
    anm  = AnmFile("player_anims.anm")
    glb_bytes = build_gltf(result, skel=skel, anm=anm)
    glb_bytes = build_gltf(result, skel=skel, anm=anm, clip_indices=[0, 8, 169])

When skel is provided:
  - All 68 bones become proper glTF nodes with parent→children hierarchy,
    bind-pose local translation/rotation, and names from the .ske symbol table.
  - A glTF skin is created with an InverseBindMatrices accessor computed from
    each bone's world_matrix / world_translation.
  - Skinned mesh primitives get JOINTS_0 and WEIGHTS_0 attributes populated
    from MeshChunk.vertex_joints / bone_weights (Layout D / D2 meshes).

When anm is provided without skel:
  - Bone nodes are created as flat, unnamed "bone_NN" placeholders (old
    behaviour) — still functional but lacks hierarchy and bind pose.

When skel is provided without anm:
  - The armature is embedded and the skin is wired; no animation channels are
    written.
"""

import json
import struct
import math
import warnings
from itertools import chain as _chain


# ---------------------------------------------------------------------------
# GLB / glTF constants
# ---------------------------------------------------------------------------

_ARRAY_BUFFER   = 34962
_ELEMENT_ARRAY  = 34963
_FLOAT          = 5126
_UNSIGNED_SHORT = 5123
_UNSIGNED_BYTE  = 5121

_GLB_MAGIC   = 0x46546C67
_GLB_VERSION = 2
_CHUNK_JSON  = 0x4E4F534A
_CHUNK_BIN   = 0x004E4942


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def build_gltf(result, *, merge_meshes: bool = True,
               skel=None, anm=None, clip_indices=None,
               mesh_materials: dict | None = None,
               material_images: dict | None = None,
               material_alpha_modes: dict | None = None) -> bytes:
    """
    Build a self-contained GLB (glTF 2.0) from EAGL mesh data, with optional
    skeleton hierarchy and animation tracks.

    Parameters
    ----------
    result : ParseResult
        Object with .meshes (list[MeshChunk]) and .model_name (str).
    merge_meshes : bool, default True
        True  → all MeshChunks as primitives on one named mesh node.
        False → one mesh node per chunk.
    skel : SkeletonResult | None
        Optional parsed .ske file (eagl_skeleton.parse_ske_file).  When
        provided the full 68-bone hierarchy is embedded as glTF nodes with
        correct parent-relative bind poses, and a skin with inverse bind
        matrices is attached to the mesh.
    anm : AnmFile | None
        Optional parsed .anm file (eagl_anm_decoder.AnmFile).  When provided,
        every clip (or the subset given by clip_indices) is embedded as a
        named glTF animation targeting the bone nodes.
    clip_indices : list[int] | None
        If given, only export clips whose .index is in this list.
    mesh_materials : dict[int, set[str]] | None
        Optional mapping of mesh.index -> set of material names, as produced
        by eagl_mesh_material_map.map_meshes_to_materials(). When given, a
        placeholder glTF material (name-only, no texture image data — the
        toolchain doesn't decode texture pixels) is created per distinct
        material name and assigned to each primitive via its "material"
        index, so materials round-trip into the GLB/glTF file with their
        associated meshes. If omitted, primitives are exported without a
        material index, same as before.
    material_images : dict[str, bytes] | None
        Optional mapping of material name -> PNG-encoded image bytes (e.g.
        from eagl_batch_export.resolve_material_images / gsh_parser texture
        decoding). When a name in mesh_materials also has an entry here, the
        placeholder material gets a real baseColorTexture embedded in the
        GLB (as a bufferView-backed image, mirroring the geometry buffers)
        instead of just a flat white color. Materials with no matching image
        fall back to the flat-color placeholder.
    material_alpha_modes : dict[str, str] | None
        Optional mapping of material name -> "opaque" | "mask" | "blend"
        (e.g. from gsh_parser.classify_alpha via
        eagl_batch_export.resolve_material_images). Controls the glTF
        material's alphaMode so textures with a real alpha channel
        (translucent glass, cutout foliage) actually render as
        transparent instead of the glTF-default OPAQUE, which silently
        ignores alpha regardless of what's in the pixels. "mask" also
        gets alphaCutoff=0.5. Names absent from this dict, or mapped to
        "opaque", are left at the glTF default.

    Returns
    -------
    bytes — raw GLB file content.
    """

    # ── mesh validation ────────────────────────────────────────────────────
    ok_meshes = [m for m in result.meshes
                 if m.ok and m.positions and m.normals and m.uvs]
    if not ok_meshes:
        raise ValueError("No valid meshes to export")

    model_name = (result.model_name or "model").strip() or "model"
    has_skel   = skel is not None and skel.ok

    # ── materials — flat placeholder, or textured if material_images given ──
    gltf_materials: list[dict] = []
    gltf_images:    list[dict] = []
    gltf_textures:  list[dict] = []
    gltf_samplers:  list[dict] = []
    _material_name_to_idx: dict[str, int] = {}
    _image_name_to_idx:    dict[str, int] = {}
    _texture_key_to_idx: dict = {}
    material_images = material_images or {}
    material_alpha_modes = material_alpha_modes or {}

    def _image_index_for(name: str, png_bytes: bytes, sampler: dict) -> int:
        key=(name,tuple(sorted(sampler.items())))
        if key in _texture_key_to_idx:return _texture_key_to_idx[key]
        if name not in _image_name_to_idx:
            view_idx = _add_view(png_bytes)
            _image_name_to_idx[name]=len(gltf_images)
            gltf_images.append({"bufferView": view_idx, "mimeType": "image/png", "name": name})
        img_idx=_image_name_to_idx[name]
        tex_idx = len(gltf_textures)
        gltf_textures.append({"source": img_idx})
        if sampler:
            if sampler not in gltf_samplers:gltf_samplers.append(sampler)
            gltf_textures[-1]['sampler']=gltf_samplers.index(sampler)
        _texture_key_to_idx[key] = tex_idx
        return tex_idx

    def _material_index_for(m) -> int | None:
        if not mesh_materials:
            return None
        mats = mesh_materials.get(m.index)
        if not mats:
            return None
        # A mesh should resolve to exactly one material; if the mapper ever
        # flags a genuine multi-material mesh, deterministically use the
        # first (sorted) name rather than silently dropping the primitive.
        name = sorted(mats)[0]
        png_bytes = material_images.get(name)
        from material_bindings import sampler
        texture_sampler=sampler(m,png_bytes) if png_bytes else {}
        material_key=(name,tuple(getattr(m,'apt_color',())),tuple(sorted(texture_sampler.items())),getattr(m,'material_unlit',False))
        if material_key not in _material_name_to_idx:
            _material_name_to_idx[material_key] = len(gltf_materials)
            mat: dict = {
                "name": name,
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor":  0.0,
                    "roughnessFactor": 1.0,
                },
            }
            if hasattr(m,'apt_color'):
                mat['pbrMetallicRoughness']['baseColorFactor']=m.apt_color
                mat['extensions']={'KHR_materials_unlit':{}}
                mat['doubleSided']=True
                if m.apt_color[3]<1:mat['alphaMode']='BLEND'
            elif getattr(m,'material_unlit',False):
                mat['extensions']={'KHR_materials_unlit':{}}
            if png_bytes:
                tex_idx = _image_index_for(name, png_bytes, texture_sampler)
                mat["pbrMetallicRoughness"]["baseColorTexture"] = {"index": tex_idx}
                alpha_mode = material_alpha_modes.get(name, "opaque")
                if hasattr(m,'apt_color') and m.apt_color[3]<1:
                    mat['alphaMode']='BLEND'
                elif alpha_mode == "mask":
                    mat["alphaMode"] = "MASK"
                    mat["alphaCutoff"] = 0.5
                    mat["doubleSided"] = True   # typical for foliage/fence cutouts
                elif alpha_mode == "blend":
                    mat["alphaMode"] = "BLEND"
                # "opaque" (or unclassified) -> leave at glTF default OPAQUE
            gltf_materials.append(mat)
        return _material_name_to_idx[material_key]

    # ── shared binary buffer state ─────────────────────────────────────────
    bin_chunks:   list[bytes] = []
    buffer_views: list[dict]  = []
    accessors:    list[dict]  = []
    byte_offset = 0

    def _pad4(n: int) -> int:
        return (4 - n % 4) % 4

    def _add_view(data: bytes, target: int | None = None) -> int:
        nonlocal byte_offset
        pad = _pad4(len(data))
        bin_chunks.append(data + b"\x00" * pad)
        view: dict = {
            "buffer":     0,
            "byteOffset": byte_offset,
            "byteLength": len(data),
        }
        if target is not None:
            view["target"] = target
        buffer_views.append(view)
        byte_offset += len(data) + pad
        return len(buffer_views) - 1

    def _add_accessor(view_idx, component_type, count, type_,
                      min_=None, max_=None) -> int:
        acc = {
            "bufferView":    view_idx,
            "componentType": component_type,
            "count":         count,
            "type":          type_,
        }
        if min_ is not None:
            acc["min"] = min_
            acc["max"] = max_
        accessors.append(acc)
        return len(accessors) - 1

    # ── determine which meshes are skinned ─────────────────────────────────
    # A mesh is skinnable if it has bone_weights data AND we have a skeleton.
    def _is_skinned(m) -> bool:
        return has_skel and bool(m.bone_weights)

    # ── mesh primitives ────────────────────────────────────────────────────
    def _build_primitive(m, skin_idx: int | None = None):
        vert_map: dict = {}
        u_pos, u_norm, u_uv, u_color = [], [], [], []
        # joint/weight arrays — filled only for skinned meshes
        u_joints:  list[tuple[int, int, int, int]]         = []
        u_weights: list[tuple[float, float, float, float]] = []
        flat_indices = []

        skinned = _is_skinned(m)
        n_bones = len(skel.bones) if has_skel else 0

        for tri in m.faces:
            for key in tri:
                pi, ni, ui = key
                if key not in vert_map:
                    # Bounds check only on first encounter; OOB keys are left
                    # absent from vert_map so any repeat hit also skips cheaply.
                    if (pi >= len(m.positions) or
                            ni >= len(m.normals) or
                            ui >= len(m.uvs)):
                        continue
                    vert_map[key] = len(u_pos)
                    u_pos.append(m.positions[pi])
                    u_norm.append(m.normals[ni])
                    u_uv.append(m.uvs[ui])
                    if hasattr(m,'vertex_colors'):
                        u_color.append(m.vertex_colors[key])

                    if skinned:
                        # vertex_joints[key] is (w0,w1,w2,bone_A,bone_B,bone_C)
                        # stored directly by the parser — no secondary lookup needed.
                        entry = m.vertex_joints.get(key)
                        if entry is not None:
                            w0, w1, w2, b0, b1, b2 = entry
                        else:
                            w0, w1, w2, b0, b1, b2 = 1.0, 0.0, 0.0, 0, 0, 0

                        # Clamp bone indices to valid skeleton range
                        b0 = min(b0, n_bones - 1)
                        b1 = min(b1, n_bones - 1)
                        b2 = min(b2, n_bones - 1)
                        u_joints.append((b0, b1, b2, 0))
                        total = w0 + w1 + w2
                        if total > 0.0:
                            w0 /= total; w1 /= total; w2 /= total
                        else:
                            w0, w1, w2 = 1.0, 0.0, 0.0
                        u_weights.append((w0, w1, w2, 0.0))

                # Guard the append: key absent from vert_map means it was OOB-skipped
                idx = vert_map.get(key)
                if idx is not None:
                    flat_indices.append(idx)

        if not flat_indices:
            return None

        pos_data  = b"".join(struct.pack("<fff", *p) for p in u_pos)
        pos_view  = _add_view(pos_data, _ARRAY_BUFFER)
        xs = [p[0] for p in u_pos]; ys = [p[1] for p in u_pos]; zs = [p[2] for p in u_pos]
        pos_acc   = _add_accessor(pos_view, _FLOAT, len(u_pos), "VEC3",
                                  [min(xs), min(ys), min(zs)],
                                  [max(xs), max(ys), max(zs)])

        norm_data = b"".join(struct.pack("<fff", *n) for n in u_norm)
        norm_view = _add_view(norm_data, _ARRAY_BUFFER)
        norm_acc  = _add_accessor(norm_view, _FLOAT, len(u_norm), "VEC3")

        uv_data   = b"".join(struct.pack("<ff", *uv) for uv in u_uv)
        uv_view   = _add_view(uv_data, _ARRAY_BUFFER)
        uv_acc    = _add_accessor(uv_view, _FLOAT, len(u_uv), "VEC2")

        if len(u_pos) > 65535:
            warnings.warn(
                f"Mesh has {len(u_pos)} unique vertices — exceeds UNSIGNED_SHORT limit (65535). "
                "Index buffer will overflow. Consider splitting the mesh.",
                stacklevel=3,
            )

        idx_data  = struct.pack(f"<{len(flat_indices)}H", *flat_indices)
        idx_view  = _add_view(idx_data, _ELEMENT_ARRAY)
        idx_acc   = _add_accessor(idx_view, _UNSIGNED_SHORT, len(flat_indices), "SCALAR")

        prim: dict = {
            "attributes": {
                "POSITION":   pos_acc,
                "NORMAL":     norm_acc,
                "TEXCOORD_0": uv_acc,
            },
            "indices": idx_acc,
            "mode":    4,
        }

        if skinned and u_joints:
            # JOINTS_0: u8 x4 per vertex
            j_data = b"".join(struct.pack("BBBB", *j) for j in u_joints)
            j_view = _add_view(j_data, _ARRAY_BUFFER)
            j_acc  = _add_accessor(j_view, _UNSIGNED_BYTE, len(u_joints), "VEC4")

            # WEIGHTS_0: f32 x4 per vertex
            w_data = b"".join(struct.pack("<ffff", *w) for w in u_weights)
            w_view = _add_view(w_data, _ARRAY_BUFFER)
            w_acc  = _add_accessor(w_view, _FLOAT, len(u_weights), "VEC4")

            prim["attributes"]["JOINTS_0"]  = j_acc
            prim["attributes"]["WEIGHTS_0"] = w_acc

        if u_color:
            c_data = b''.join(struct.pack('<4f',*c) for c in u_color)
            c_view = _add_view(c_data, _ARRAY_BUFFER)
            prim['attributes']['COLOR_0'] = _add_accessor(c_view, _FLOAT, len(u_color), 'VEC4')

        mat_idx = _material_index_for(m)
        if mat_idx is not None:
            prim["material"] = mat_idx

        return prim

    # ── skeleton bone nodes ────────────────────────────────────────────────
    # bone_node_map[bone_idx] = glTF node index.
    # bone_node_base is set AFTER mesh nodes are appended (Issue 7 fix:
    # pre-calculating n_mesh_nodes was fragile when primitives came out empty).
    gltf_nodes:   list[dict]    = []
    bone_node_map: dict[int, int] = {}

    # Build the bone node list now (structure only); we'll append to
    # gltf_nodes after the mesh nodes so indices are correct.
    bone_nodes: list[dict] = []
    if has_skel:
        for bone in skel.bones:
            tx, ty, tz = bone.local_translation
            qx, qy, qz, qw = bone.quaternion
            node: dict = {
                "name":        bone.name,
                "translation": [tx, ty, tz],
                "rotation":    [qx, qy, qz, qw],
                "scale":       list(bone.scale),
            }
            bone_nodes.append(node)
        # Children arrays filled after bone_node_base is known (below).

    # ── InverseBindMatrices (skeleton only) ────────────────────────────────
    # IBM for bone i is the 4×4 that transforms a vertex from model space to
    # bone-i local space at bind pose:
    #   IBM[i] = inverse(world_matrix[i] | world_translation[i])
    # We store only the bones that the skin references (all 68 by default).
    skin_idx: int | None = None
    ibm_accessor: int | None = None

    if has_skel:
        def _mat4_from_bone(bone) -> list[float]:
            """Column-major 4×4 TRS matrix for this bone's world transform."""
            wm = bone.world_matrix   # 3×3 row-major
            tx, ty, tz = bone.world_translation
            # Build 4×4 row-major, then transpose to column-major for glTF
            row = [
                wm[0], wm[1], wm[2], tx,
                wm[3], wm[4], wm[5], ty,
                wm[6], wm[7], wm[8], tz,
                0.0,   0.0,   0.0,   1.0,
            ]
            # Transpose (row-major → column-major)
            def at(r, c): return row[r * 4 + c]
            col = [at(r, c) for c in range(4) for r in range(4)]
            return col

        def _mat4_inverse_rigid(col: list[float]) -> list[float]:
            """
            Invert a rigid-body (rotation + translation, no scale) 4×4
            column-major matrix.  For a pure rotation R and translation T:
              inv = [R^T | -R^T * T]
            """
            # Extract R (3×3) and T from column-major layout
            # col[c*4 + r] = element at row r, col c
            R = [[col[c * 4 + r] for c in range(3)] for r in range(3)]
            T = [col[12], col[13], col[14]]   # last column, first 3 rows

            # R^T
            Rt = [[R[c][r] for c in range(3)] for r in range(3)]

            # -R^T * T
            nRtT = [-sum(Rt[r][k] * T[k] for k in range(3)) for r in range(3)]

            # Rebuild column-major 4×4
            result = [0.0] * 16
            for r in range(3):
                for c in range(3):
                    result[c * 4 + r] = Rt[r][c]
            result[12] = nRtT[0]
            result[13] = nRtT[1]
            result[14] = nRtT[2]
            result[15] = 1.0
            return result

        ibm_data = b""
        for bone in skel.bones:
            world_mat4 = _mat4_from_bone(bone)
            inv_mat4   = _mat4_inverse_rigid(world_mat4)
            ibm_data  += struct.pack("<16f", *inv_mat4)

        ibm_view     = _add_view(ibm_data)   # no target for IBM
        ibm_accessor = _add_accessor(ibm_view, _FLOAT, len(skel.bones), "MAT4")

        # joint_list built below, after bone_node_map is populated
        skin_idx = 0   # we'll write exactly one skin

    # ── build mesh nodes ───────────────────────────────────────────────────
    gltf_meshes: list[dict] = []

    # Determine skin parameter for primitives
    _skin_param = skin_idx  # None if no skeleton

    if merge_meshes:
        primitives = [p for m in ok_meshes
                      for p in [_build_primitive(m, _skin_param)] if p]
        if not primitives:
            raise ValueError("All meshes produced empty geometry after index validation")
        gltf_meshes.append({"name": model_name, "primitives": primitives})
        # Remove skin from prim (it belongs on the node)
        for prim in primitives:
            prim.pop("skin", None)
        mesh_node: dict = {"mesh": 0, "name": model_name}
        if skin_idx is not None and any(_is_skinned(m) for m in ok_meshes):
            mesh_node["skin"] = skin_idx
        gltf_nodes.append(mesh_node)
    else:
        for m in ok_meshes:
            prim = _build_primitive(m, _skin_param)
            if prim is None:
                continue
            prim.pop("skin", None)
            chunk_name = f"{model_name}_mesh_{m.index}_{m.layout}"
            gltf_meshes.append({"name": chunk_name, "primitives": [prim]})
            mesh_node = {"mesh": len(gltf_meshes) - 1, "name": chunk_name}
            if skin_idx is not None and _is_skinned(m):
                mesh_node["skin"] = skin_idx
            gltf_nodes.append(mesh_node)
        if not gltf_meshes:
            raise ValueError("All meshes produced empty geometry after index validation")

    # Append bone nodes (skeleton or placeholder — either way they go after mesh nodes).
    # bone_node_base is now set here, after all mesh nodes are in gltf_nodes, so the
    # count is exact even if some primitives came out empty (Issue 7 fix).
    if has_skel:
        bone_node_base = len(gltf_nodes)
        for bone in skel.bones:
            bone_node_map[bone.index] = bone_node_base + bone.index
        # Now that bone_node_map is populated, wire up children arrays.
        for bone in skel.bones:
            children_gltf = [bone_node_map[c.index]
                             for c in skel.children_of(bone.index)]
            if children_gltf:
                bone_nodes[bone.index]["children"] = children_gltf
        gltf_nodes.extend(bone_nodes)
    # (placeholder bone nodes for anm-only case added in animation section below)

    # ── animation embedding ────────────────────────────────────────────────
    gltf_animations: list[dict] = []

    if anm is not None:
        clips = anm.clips
        if clip_indices is not None:
            wanted = set(clip_indices)
            clips = [c for c in clips if c.index in wanted]

        if not has_skel:
            # Fall back to flat placeholder nodes (old behaviour)
            used_bones: set[int] = set()
            for clip in clips:
                used_bones.update(clip.rot_by_bone)
                used_bones.update(clip.trans_by_bone)
            bone_node_base = len(gltf_nodes)
            for bone_idx in sorted(used_bones):
                bone_node_map[bone_idx] = bone_node_base + len(bone_node_map)
                gltf_nodes.append({"name": f"bone_{bone_idx:02d}"})

        # Issue 2B: cross-validate animated bone indices against skeleton
        if has_skel:
            skel_indices = {b.index for b in skel.bones}
            for clip in clips:
                animated = set(clip.rot_by_bone) | set(clip.trans_by_bone)
                bad = animated - skel_indices
                if bad:
                    warnings.warn(
                        f"clip {clip.index} ({clip.name!r}): animated bone_idx "
                        f"{sorted(bad)} has no matching bone in skeleton "
                        f"(skeleton has {len(skel.bones)} bones, indices "
                        f"{min(skel_indices)}\u2013{max(skel_indices)})",
                        stacklevel=2,
                    )

        # Build one glTF animation per clip.
        #
        # NOTE: this reads clip.rot_by_bone / clip.trans_by_bone directly
        # (the real decoded-and-mask-gated output from AnmFile, which
        # already ran the clip through exporter.anm_exporter.decode_clip_bones
        # and its sBoneMask gate -- see decoder/bone_mask.py) instead of the
        # old clip.tracks / track.keyframes shape. That old shape no longer
        # matches the current Track dataclass (bone_idx only, no keyframes)
        # and would AttributeError here; reading rot_by_bone/trans_by_bone
        # also means this path is mask-correct for free, with no separate
        # patching needed to keep it in sync with the real pipeline.
        for clip in clips:
            if not (clip.rot_by_bone or clip.trans_by_bone) or not clip.frame_times:
                continue

            clip_name = clip.name or f"clip_{clip.index:03d}"
            samplers: list[dict] = []
            channels: list[dict] = []

            for bone_idx, quats in clip.rot_by_bone.items():
                node_idx = bone_node_map.get(bone_idx)
                if node_idx is None or not quats:
                    continue

                n_kf = len(quats)
                times = clip.frame_times[:n_kf]
                if len(times) < n_kf:
                    times = times + [times[-1]] * (n_kf - len(times))

                t_data = struct.pack(f"<{n_kf}f", *times)
                t_view = _add_view(t_data)
                t_acc = _add_accessor(t_view, _FLOAT, n_kf, "SCALAR",
                                       [min(times)], [max(times)])

                flat_q = [v for q in quats for v in (q if q is not None else (0.0, 0.0, 0.0, 1.0))]
                q_data = struct.pack(f"<{n_kf * 4}f", *flat_q)
                q_view = _add_view(q_data)
                q_acc = _add_accessor(q_view, _FLOAT, n_kf, "VEC4")

                sampler_idx = len(samplers)
                samplers.append({
                    "input":         t_acc,
                    "output":        q_acc,
                    "interpolation": "LINEAR",
                })
                channels.append({
                    "sampler": sampler_idx,
                    "target":  {"node": node_idx, "path": "rotation"},
                })

            # Translation channels: NOT emitted at all by the old code path
            # (it only ever wrote rotation). Emitting them here means this
            # legacy build_gltf(anm=...) entry point actually reflects
            # decoded+masked translation, same as merge_mesh_and_clip_glb.
            for bone_idx, trans in clip.trans_by_bone.items():
                node_idx = bone_node_map.get(bone_idx)
                if node_idx is None or not trans:
                    continue

                n_kf = len(trans)
                times = clip.frame_times[:n_kf]
                if len(times) < n_kf:
                    times = times + [times[-1]] * (n_kf - len(times))

                t_data = struct.pack(f"<{n_kf}f", *times)
                t_view = _add_view(t_data)
                t_acc = _add_accessor(t_view, _FLOAT, n_kf, "SCALAR",
                                       [min(times)], [max(times)])

                flat_t = [v for vec in trans for v in (vec if vec is not None else (0.0, 0.0, 0.0))]
                v_data = struct.pack(f"<{n_kf * 3}f", *flat_t)
                v_view = _add_view(v_data)
                v_acc = _add_accessor(v_view, _FLOAT, n_kf, "VEC3")

                sampler_idx = len(samplers)
                samplers.append({
                    "input":         t_acc,
                    "output":        v_acc,
                    "interpolation": "LINEAR",
                })
                channels.append({
                    "sampler": sampler_idx,
                    "target":  {"node": node_idx, "path": "translation"},
                })

            # Scale channels were already decoded (e.g. S_sk_bag_reach) but
            # discarded by the mesh+animation export path.
            for bone_idx, values in getattr(clip, 'scale_by_bone', {}).items():
                node_idx = bone_node_map.get(bone_idx)
                if node_idx is None or not values:continue
                times = clip.frame_times
                if len(times) != len(values):raise ValueError('Scale/time sample count mismatch')
                t_view = _add_view(struct.pack(f'<{len(times)}f', *times))
                t_acc = _add_accessor(t_view, _FLOAT, len(times), 'SCALAR', [min(times)], [max(times)])
                flat = [x for value in values for x in (value or skel.bones[bone_idx].scale)]
                v_view = _add_view(struct.pack(f'<{len(flat)}f', *flat))
                v_acc = _add_accessor(v_view, _FLOAT, len(values), 'VEC3')
                channels.append({'sampler':len(samplers),'target':{'node':node_idx,'path':'scale'}})
                samplers.append({'input':t_acc,'output':v_acc,'interpolation':'LINEAR'})

            if channels:
                gltf_animations.append({
                    "name":     clip_name,
                    "samplers": samplers,
                    "channels": channels,
                })

    # ── assemble glTF JSON ─────────────────────────────────────────────────
    # Scene root nodes: mesh nodes + skeleton root bones (if skeleton present)
    mesh_node_indices = list(range(len(gltf_meshes) if not merge_meshes else 1))

    if has_skel:
        skel_root_indices = [bone_node_map[b.index]
                             for b in skel.bones if b.is_root]
        scene_nodes = mesh_node_indices + skel_root_indices
    else:
        scene_nodes = list(range(len(gltf_nodes)))

    gltf_json: dict = {
        "asset":       {"version": "2.0", "generator": "EAGL exporter"},
        "scene":       0,
        "scenes":      [{"nodes": scene_nodes, "name": model_name}],
        "nodes":       gltf_nodes,
        "meshes":      gltf_meshes,
        "accessors":   accessors,
        "bufferViews": buffer_views,
        "buffers":     [{"byteLength": byte_offset}],
    }

    if has_skel and ibm_accessor is not None:
        joint_list = [bone_node_map[b.index] for b in skel.bones]
        gltf_json["skins"] = [{
            "name":                 "Armature",
            "inverseBindMatrices":  ibm_accessor,
            "joints":               joint_list,
            "skeleton":             bone_node_map[skel.bones[0].index],
        }]

    if gltf_animations:
        gltf_json["animations"] = gltf_animations

    if gltf_materials:
        gltf_json["materials"] = gltf_materials
        if any('KHR_materials_unlit' in m.get('extensions',{}) for m in gltf_materials):
            gltf_json['extensionsUsed']=['KHR_materials_unlit']
    if gltf_textures:
        gltf_json["textures"] = gltf_textures
    if gltf_samplers:
        gltf_json['samplers'] = gltf_samplers
    if gltf_images:
        gltf_json["images"] = gltf_images

    json_bytes = json.dumps(gltf_json, separators=(",", ":")).encode("utf-8")
    pad = _pad4(len(json_bytes))
    json_bytes += b" " * pad

    bin_blob = b"".join(bin_chunks)

    total_len = 12 + 8 + len(json_bytes) + 8 + len(bin_blob)

    return (
        struct.pack("<III", _GLB_MAGIC, _GLB_VERSION, total_len) +
        struct.pack("<II",  len(json_bytes), _CHUNK_JSON) +
        json_bytes +
        struct.pack("<II",  len(bin_blob), _CHUNK_BIN) +
        bin_blob
    )


# UI compatibility aliases
def build_gltf_animated(result, anm, *, merge_meshes: bool = True,
                        skel=None, clip_indices: list | None = None,
                        mesh_materials: dict | None = None,
                        material_images: dict | None = None,
                        material_alpha_modes: dict | None = None) -> bytes:
    """
    Build mesh + skin + animation in one GLB.

    Prefers the correct decode pipeline (Clip.rot_by_bone/trans_by_bone,
    from the top-level eagl_anm_decoder.py AnmFile wrapper -- validated
    byte-identical against known-good reference clips) whenever `anm`
    exposes it and exactly one clip is requested. Falls back to the
    legacy Track-format path (build_gltf's own anm= handling) otherwise,
    e.g. for older AnmFile implementations or multi-clip exports, which
    this function doesn't yet support via the correct pipeline.
    """
    if (skel is not None and result is not None
            and hasattr(anm, "export_clip_glb")
            and hasattr(anm, "clips")
            and clip_indices and len(clip_indices) == 1):
        clip = anm.clips[clip_indices[0]]
        if hasattr(clip, "rot_by_bone"):
            mesh_glb = build_gltf(result, merge_meshes=merge_meshes, skel=skel,
                                   mesh_materials=mesh_materials,
                                   material_images=material_images,
                                   material_alpha_modes=material_alpha_modes)
            return merge_mesh_and_clip_glb(mesh_glb, skel, clip)

    return build_gltf(result, merge_meshes=merge_meshes, skel=skel, anm=anm,
                       clip_indices=clip_indices, mesh_materials=mesh_materials,
                       material_images=material_images,
                       material_alpha_modes=material_alpha_modes)


def merge_mesh_and_clip_glb(mesh_glb_bytes: bytes, skel, clip) -> bytes:
    """
    Splice a correctly-decoded animation clip (Clip.rot_by_bone /
    trans_by_bone / sample_count, from the top-level eagl_anm_decoder.py
    AnmFile pipeline) into an already-built mesh+skin GLB (from
    build_gltf(result, skel=skel), no anm=).

    Exists because build_gltf's own anm= handling only understands the
    legacy Track.keyframes format, and the correct pipeline's
    gltf_writer.build_clip_glb only knows how to write skeleton+animation
    with no mesh. Bone-to-node mapping is done by NAME (not a fixed index
    offset) so it stays correct regardless of build_gltf's internal node
    ordering.
    """
    gltf, blob = _parse_glb_chunks(mesh_glb_bytes)

    name_to_node = {n["name"]: i for i, n in enumerate(gltf["nodes"]) if "name" in n}
    bone_node = {b.index: name_to_node[b.name] for b in skel.bones if b.name in name_to_node}

    placeholder_fps = 30.0
    times = [i / placeholder_fps for i in range(clip.sample_count)]
    time_acc = _append_float_accessor(gltf, blob, times, 1, "SCALAR")

    channels, samplers = [], []

    for bone_idx, quats in clip.rot_by_bone.items():
        node_idx = bone_node.get(bone_idx)
        if node_idx is None:
            continue
        flat = []
        for q in quats:
            flat.extend(q if q is not None else (0.0, 0.0, 0.0, 1.0))
        out_acc = _append_float_accessor(gltf, blob, flat, 4, "VEC4")
        samp_idx = len(samplers)
        samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
        channels.append({"sampler": samp_idx, "target": {"node": node_idx, "path": "rotation"}})

    for bone_idx, vecs in clip.trans_by_bone.items():
        node_idx = bone_node.get(bone_idx)
        if node_idx is None:
            continue
        flat = []
        for v in vecs:
            flat.extend(v if v is not None else (0.0, 0.0, 0.0))
        out_acc = _append_float_accessor(gltf, blob, flat, 3, "VEC3")
        samp_idx = len(samplers)
        samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
        channels.append({"sampler": samp_idx, "target": {"node": node_idx, "path": "translation"}})

    gltf["animations"] = [{
        "name": clip.name or "clip",
        "channels": channels,
        "samplers": samplers,
    }]

    return _write_glb_chunks(gltf, blob)


def _parse_glb_chunks(data: bytes):
    magic, version, length = struct.unpack("<III", data[0:12])
    off = 12
    chunks = {}
    while off < length:
        clen, ctype = struct.unpack("<II", data[off:off + 8])
        off += 8
        chunks[ctype] = data[off:off + clen]
        off += clen
    return json.loads(chunks[_CHUNK_JSON]), bytearray(chunks.get(_CHUNK_BIN, b""))


def _write_glb_chunks(gltf_json: dict, blob: bytearray) -> bytes:
    json_bytes = json.dumps(gltf_json, separators=(",", ":")).encode("utf-8")
    while len(json_bytes) % 4:
        json_bytes += b" "
    while len(blob) % 4:
        blob += b"\x00"
    total_len = 12 + 8 + len(json_bytes) + 8 + len(blob)
    out = bytearray()
    out += struct.pack("<III", _GLB_MAGIC, _GLB_VERSION, total_len)
    out += struct.pack("<II", len(json_bytes), _CHUNK_JSON)
    out += json_bytes
    out += struct.pack("<II", len(blob), _CHUNK_BIN)
    out += blob
    return bytes(out)


def _append_float_accessor(gltf_json: dict, blob: bytearray, flat: list,
                            comp_count: int, gltf_type: str) -> int:
    while len(blob) % 4:
        blob.append(0)
    offset = len(blob)
    packed = struct.pack(f"<{len(flat)}f", *flat)
    blob += packed
    count = len(flat) // comp_count
    mins = [min(flat[i::comp_count]) for i in range(comp_count)]
    maxs = [max(flat[i::comp_count]) for i in range(comp_count)]
    bv_idx = len(gltf_json["bufferViews"])
    gltf_json["bufferViews"].append({
        "buffer": 0, "byteOffset": offset, "byteLength": len(packed),
    })
    acc_idx = len(gltf_json["accessors"])
    gltf_json["accessors"].append({
        "bufferView": bv_idx, "componentType": 5126,
        "count": count, "type": gltf_type,
        "min": mins, "max": maxs,
    })
    return acc_idx


# ---------------------------------------------------------------------------
# Animation-only export — armature + clips, NO mesh/skin required
# ---------------------------------------------------------------------------

def build_anim_only_gltf(skel, anm, *, clip_indices=None) -> bytes:
    """
    Build a self-contained GLB containing ONLY the bone hierarchy + baked
    animation clips — no mesh, no skin. Useful for extracting animation
    data decoupled from any specific character mesh: import this GLB into
    Blender to get an animated armature, then separately import/rig any
    mesh sharing the same 68-bone skeleton onto it (matching bone names).

    Parameters
    ----------
    skel : SkeletonResult (eagl_skeleton.parse_ske_file)
    anm  : AnmFile (eagl_anm_decoder.AnmFile)
    clip_indices : list[int] | None
        If given, only export clips whose .index is in this list.

    Returns
    -------
    bytes — raw GLB file content.
    """
    if skel is None or not skel.ok:
        raise ValueError("A valid SkeletonResult is required")
    if anm is None:
        raise ValueError("An AnmFile is required")

    bin_chunks:   list[bytes] = []
    buffer_views: list[dict]  = []
    accessors:    list[dict]  = []
    byte_offset = 0

    def _pad4(n: int) -> int:
        return (4 - n % 4) % 4

    def _add_view(data: bytes) -> int:
        nonlocal byte_offset
        pad = _pad4(len(data))
        bin_chunks.append(data + b"\x00" * pad)
        buffer_views.append({"buffer": 0, "byteOffset": byte_offset, "byteLength": len(data)})
        byte_offset += len(data) + pad
        return len(buffer_views) - 1

    def _add_accessor(view_idx, component_type, count, type_, min_=None, max_=None) -> int:
        acc = {"bufferView": view_idx, "componentType": component_type, "count": count, "type": type_}
        if min_ is not None:
            acc["min"] = min_
            acc["max"] = max_
        accessors.append(acc)
        return len(accessors) - 1

    # ── bone nodes ──────────────────────────────────────────────────────────
    gltf_nodes: list[dict] = []
    bone_node_map: dict[int, int] = {}
    for bone in skel.bones:
        tx, ty, tz = bone.local_translation
        qx, qy, qz, qw = bone.quaternion
        node: dict = {
            "name":        bone.name,
            "translation": [tx, ty, tz],
            "rotation":    [qx, qy, qz, qw],
            "scale":       list(bone.scale),
        }
        gltf_nodes.append(node)
        bone_node_map[bone.index] = bone.index  # bones are the only nodes -> 1:1

    for bone in skel.bones:
        children = [bone_node_map[c.index] for c in skel.children_of(bone.index)]
        if children:
            gltf_nodes[bone.index]["children"] = children

    # ── animation clips ─────────────────────────────────────────────────────
    clips = anm.clips
    if clip_indices is not None:
        wanted = set(clip_indices)
        clips = [c for c in clips if c.index in wanted]

    skel_indices = {b.index for b in skel.bones}
    gltf_animations: list[dict] = []
    for clip in clips:
        if not clip.tracks or not clip.frame_times:
            continue
        clip_name = clip.name or f"clip_{clip.index:03d}"
        samplers: list[dict] = []
        channels: list[dict] = []

        for track in clip.tracks:
            if not track.keyframes or track.bone_idx not in skel_indices:
                continue
            node_idx = bone_node_map[track.bone_idx]
            n_kf = len(track.keyframes)
            times = (track.frame_times[:n_kf] if track.frame_times else clip.frame_times[:n_kf])
            if len(times) < n_kf:
                times = times + [times[-1]] * (n_kf - len(times))

            t_data = struct.pack(f"<{n_kf}f", *times)
            t_view = _add_view(t_data)
            t_acc  = _add_accessor(t_view, _FLOAT, n_kf, "SCALAR", [min(times)], [max(times)])

            q_data = struct.pack(f"<{n_kf * 4}f", *_chain.from_iterable(track.keyframes[:n_kf]))
            q_view = _add_view(q_data)
            q_acc  = _add_accessor(q_view, _FLOAT, n_kf, "VEC4")

            sampler_idx = len(samplers)
            samplers.append({"input": t_acc, "output": q_acc, "interpolation": "LINEAR"})
            channels.append({"sampler": sampler_idx, "target": {"node": node_idx, "path": "rotation"}})

        if channels:
            gltf_animations.append({"name": clip_name, "samplers": samplers, "channels": channels})

    if not gltf_animations:
        raise ValueError("No animation clips produced usable channels for this skeleton")

    root_nodes = [b.index for b in skel.bones if b.is_root]

    gltf_json: dict = {
        "asset":       {"version": "2.0", "generator": "eagl_exporter.py (anim-only)"},
        "scene":       0,
        "scenes":      [{"nodes": root_nodes, "name": "Armature"}],
        "nodes":       gltf_nodes,
        "animations":  gltf_animations,
        "accessors":   accessors,
        "bufferViews": buffer_views,
        "buffers":     [{"byteLength": byte_offset}],
    }

    json_bytes = json.dumps(gltf_json, separators=(",", ":")).encode("utf-8")
    json_bytes += b" " * _pad4(len(json_bytes))
    bin_blob = b"".join(bin_chunks)
    total_len = 12 + 8 + len(json_bytes) + 8 + len(bin_blob)

    return (
        struct.pack("<III", _GLB_MAGIC, _GLB_VERSION, total_len) +
        struct.pack("<II",  len(json_bytes), _CHUNK_JSON) +
        json_bytes +
        struct.pack("<II",  len(bin_blob), _CHUNK_BIN) +
        bin_blob
    )
