"""Builds the four lantern-folk characters on one shared rig and exports each as a .glb.

Run inside Blender (through the MCP, or `blender --background --python art/lantern_folk.py`).
It clears the open file, builds everything from scratch, saves `art/lantern-folk.blend` and
writes `crates/game/assets/characters/<id>.glb`.

Conventions the game relies on (`crates/game/src/characters.rs`):
- the character faces -Y in Blender (+Z in glTF), feet at the origin, about 1.8 m tall;
- bones `root` (whole-body hops), `body`, `head` (the lantern and its light source),
  `arm.L`, `arm.R`, `leg.L`, `leg.R`; every part is rigidly skinned to one bone;
- materials `Body`, `Glow` and `Accent` are recoloured per player in game; the rest
  (`Dark`, `Wax`, `Flame`, `Firefly`) keep the colours set here;
- actions `idle`, `hop`, `celebrate` and `slump` key every bone on every channel, so
  blending between them never leaves a bone behind.
"""

import math
import os

import bmesh
import bpy
from mathutils import Euler, Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
BLEND_PATH = os.path.join(HERE, "lantern-folk.blend")
EXPORT_DIR = os.path.join(REPO, "crates", "game", "assets", "characters")

FPS = 30
CHARACTERS = ["paper-lantern", "firefly-jar", "candle", "neon-ring"]
BONES = {
    # name: (head, tail, parent)
    "root": ((0, 0, 0), (0, 0, 0.25), None),
    "body": ((0, 0, 0.32), (0, 0, 1.05), "root"),
    "head": ((0, 0, 1.08), (0, 0, 1.8), "body"),
    "arm.L": ((0.51, 0, 0.93), (0.51, 0, 1.2), "body"),
    "arm.R": ((-0.51, 0, 0.93), (-0.51, 0, 1.2), "body"),
    "leg.L": ((0.2, 0, 0.32), (0.2, 0, 0.55), "root"),
    "leg.R": ((-0.2, 0, 0.32), (-0.2, 0, 0.55), "root"),
}
HEAD_Z = 1.42


# ---------------------------------------------------------------------------------------------
# Scene and materials


def clear():
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for collection in (
        bpy.data.objects,
        bpy.data.meshes,
        bpy.data.armatures,
        bpy.data.materials,
        bpy.data.actions,
        bpy.data.cameras,
        bpy.data.lights,
        bpy.data.collections,
    ):
        for item in list(collection):
            collection.remove(item)
    scene = bpy.context.scene
    scene.render.fps = FPS
    scene.frame_start = 0
    scene.frame_end = 60


def material(name, color, emission=None, strength=0.0, roughness=0.6):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1.0)
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*emission, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    return mat


def materials():
    warm = (1.0, 0.72, 0.38)
    return [
        material("Body", (0.09, 0.085, 0.1), roughness=0.8),
        material("Dark", (0.02, 0.02, 0.025), roughness=0.5),
        material("Glow", (0.08, 0.07, 0.06), warm, 2.0),
        material("Accent", (0.08, 0.07, 0.06), warm, 6.0),
        material("Wax", (0.85, 0.8, 0.68), roughness=0.7),
        material("Flame", (0.1, 0.05, 0.0), (1.0, 0.6, 0.15), 9.0),
        material("Firefly", (0.05, 0.06, 0.0), (0.75, 1.0, 0.3), 12.0),
    ]


# ---------------------------------------------------------------------------------------------
# Shapes: each adds its geometry to a shared bmesh and returns the new vertices.


class Builder:
    """One character's mesh: parts in a shared bmesh, each rigidly bound to a bone."""

    def __init__(self, mats):
        self.bm = bmesh.new()
        self.mats = {m.name: i for i, m in enumerate(mats)}
        self.mat_list = mats
        self.parts = []  # (verts, bone)

    def _added(self, before_verts, before_faces, bone, mat):
        verts = [v for v in self.bm.verts if v not in before_verts]
        for f in self.bm.faces:
            if f not in before_faces:
                f.material_index = self.mats[mat]
        self.parts.append((verts, bone))
        return verts

    def _begin(self):
        return set(self.bm.verts), set(self.bm.faces)

    def _bevel(self, edges, offset, segments):
        if offset > 0 and edges:
            bmesh.ops.bevel(
                self.bm,
                geom=edges,
                offset=offset,
                offset_type="OFFSET",
                segments=segments,
                profile=0.5,
                affect="EDGES",
                clamp_overlap=True,
            )

    def box(self, bone, mat, size, loc, bevel=0.06, segments=2, rot=(0, 0, 0)):
        vs, fs = self._begin()
        made = bmesh.ops.create_cube(self.bm, size=1.0)["verts"]
        for v in made:
            v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2]))
        edges = list({e for v in made for e in v.link_edges})
        self._bevel(edges, bevel, segments)
        new = [v for v in self.bm.verts if v not in vs]
        self._place(new, loc, rot)
        return self._added(vs, fs, bone, mat)

    def cylinder(self, bone, mat, radius, depth, loc, bevel=0.04, segments=16, top=None, rot=(0, 0, 0)):
        vs, fs = self._begin()
        made = bmesh.ops.create_cone(
            self.bm,
            cap_ends=True,
            cap_tris=False,
            segments=segments,
            radius1=radius,
            radius2=radius if top is None else top,
            depth=depth,
        )["verts"]
        rings = [
            e
            for e in {e for v in made for e in v.link_edges}
            if abs(e.verts[0].co.z - e.verts[1].co.z) < 1e-6
        ]
        self._bevel(rings, bevel, 2)
        new = [v for v in self.bm.verts if v not in vs]
        self._place(new, loc, rot)
        return self._added(vs, fs, bone, mat)

    def sphere(self, bone, mat, scale, loc, clamp_z=None, u=16, v=10, shape=None):
        vs, fs = self._begin()
        made = bmesh.ops.create_uvsphere(self.bm, u_segments=u, v_segments=v, radius=1.0)["verts"]
        for vert in made:
            co = vert.co
            if shape:
                co = shape(co)
            co = Vector((co.x * scale[0], co.y * scale[1], co.z * scale[2]))
            if clamp_z is not None:
                co.z = max(-clamp_z, min(clamp_z, co.z))
            vert.co = co
        if clamp_z is not None:
            bmesh.ops.remove_doubles(self.bm, verts=made, dist=1e-4)
        new = [vert for vert in self.bm.verts if vert not in vs]
        self._place(new, loc, (0, 0, 0))
        return self._added(vs, fs, bone, mat)

    def torus(self, bone, mat, major, minor, loc, rot=(0, 0, 0), segments=24, ring=8):
        vs, fs = self._begin()
        grid = []
        for i in range(segments):
            a = math.tau * i / segments
            row = []
            for j in range(ring):
                b = math.tau * j / ring
                r = major + minor * math.cos(b)
                row.append(self.bm.verts.new((r * math.cos(a), r * math.sin(a), minor * math.sin(b))))
            grid.append(row)
        for i in range(segments):
            for j in range(ring):
                n, k = (i + 1) % segments, (j + 1) % ring
                self.bm.faces.new((grid[i][j], grid[n][j], grid[n][k], grid[i][k]))
        new = [v for v in self.bm.verts if v not in vs]
        self._place(new, loc, rot)
        return self._added(vs, fs, bone, mat)

    def _place(self, verts, loc, rot):
        m = Matrix.Translation(Vector(loc)) @ Euler([math.radians(a) for a in rot]).to_matrix().to_4x4()
        for v in verts:
            v.co = m @ v.co

    def finish(self, name, armature):
        bm = self.bm
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.verts.index_update()
        mesh = bpy.data.meshes.new(name)
        groups = [(bone, [v.index for v in verts]) for verts, bone in self.parts]
        bm.to_mesh(mesh)
        bm.free()
        for mat in self.mat_list:
            mesh.materials.append(mat)
        mesh.shade_smooth()
        mesh.set_sharp_from_angle(angle=math.radians(40))
        obj = bpy.data.objects.new(name, mesh)
        for bone in BONES:
            obj.vertex_groups.new(name=bone)
        for bone, indices in groups:
            obj.vertex_groups[bone].add(indices, 1.0, "REPLACE")
        obj.parent = armature
        mod = obj.modifiers.new("Armature", "ARMATURE")
        mod.object = armature
        return obj


# ---------------------------------------------------------------------------------------------
# Bodies


def shared_body(b):
    """The body plan every character shares: chunky torso, stubby limbs, a neck collar."""
    for side, x in (("L", 0.2), ("R", -0.2)):
        b.box(f"leg.{side}", "Body", (0.25, 0.3, 0.34), (x, -0.01, 0.17), bevel=0.09, segments=3)
        b.box(f"arm.{side}", "Body", (0.2, 0.25, 0.5), (x * 2.55, 0, 0.72), bevel=0.09, segments=3)
    b.box("body", "Body", (0.82, 0.64, 0.76), (0, 0, 0.69), bevel=0.2, segments=3)
    b.cylinder("body", "Dark", 0.27, 0.08, (0, 0, 1.08), bevel=0.02)


def eyes(b, front, z=HEAD_Z + 0.03):
    for x in (-0.14, 0.14):
        b.box("head", "Dark", (0.1, 0.06, 0.16), (x, -front, z), bevel=0.025)


def paper_lantern(b):
    """A round paper lantern: ribbed, capped top and bottom, a little hanging loop."""
    half = 0.3
    b.sphere("head", "Glow", (0.42, 0.42, 0.37), (0, 0, HEAD_Z), clamp_z=half)
    for dz in (-0.15, 0.0, 0.15):
        r = 0.42 * math.sqrt(1 - (dz / 0.37) ** 2)
        b.torus("head", "Dark", r + 0.004, 0.014, (0, 0, HEAD_Z + dz), ring=6)
    for dz in (-half, half):
        b.cylinder("head", "Dark", 0.27, 0.07, (0, 0, HEAD_Z + dz), bevel=0.02)
    b.torus("head", "Dark", 0.09, 0.022, (0, 0, HEAD_Z + half + 0.12), rot=(90, 0, 0), ring=6)
    b.sphere("head", "Accent", (0.055, 0.055, 0.055), (0, 0, HEAD_Z + half + 0.23), u=10, v=6)
    eyes(b, 0.405)


def firefly_jar(b):
    """A glass jar full of fireflies, with a screw lid."""
    b.cylinder("head", "Glow", 0.36, 0.6, (0, 0, HEAD_Z), bevel=0.1, segments=20)
    b.cylinder("head", "Glow", 0.29, 0.08, (0, 0, HEAD_Z + 0.33), bevel=0.02, segments=20)
    b.cylinder("head", "Dark", 0.32, 0.13, (0, 0, HEAD_Z + 0.42), bevel=0.03, segments=20)
    b.cylinder("head", "Dark", 0.375, 0.05, (0, 0, HEAD_Z - 0.29), bevel=0.015, segments=20)
    for i in range(10):
        a = math.tau * i / 10
        b.box("head", "Dark", (0.04, 0.03, 0.1), (0.325 * math.sin(a), 0.325 * math.cos(a), HEAD_Z + 0.42), bevel=0.01, rot=(0, 0, -math.degrees(a)))
    for x, z in ((-0.25, 1.26), (0.23, 1.62), (0.2, 1.24), (-0.06, 1.67), (-0.27, 1.58)):
        y = -math.sqrt(max(0.0, 0.36**2 - x**2)) - 0.015
        b.box("head", "Firefly", (0.07, 0.07, 0.07), (x, y, z), bevel=0.015, rot=(0, 0, math.degrees(math.atan2(x, 0.36))))
    eyes(b, 0.365)


def flame_shape(co):
    """A unit sphere pulled up into a teardrop."""
    if co.z > 0:
        taper = 1.0 - 0.85 * co.z
        return Vector((co.x * taper, co.y * taper, co.z * 2.3))
    return co.copy()


def candle(b):
    """A glowing wax candle with melted drips on top and a flame."""
    b.cylinder("head", "Glow", 0.34, 0.62, (0, 0, HEAD_Z), bevel=0.05, segments=20)
    top = HEAD_Z + 0.31
    b.cylinder("head", "Wax", 0.355, 0.07, (0, 0, top), bevel=0.03, segments=20)
    for deg, length in ((40, 0.17), (100, 0.1), (160, 0.22), (215, 0.13), (290, 0.19), (330, 0.11)):
        a = math.radians(deg)
        b.box("head", "Wax", (0.09, 0.06, length), (0.345 * math.sin(a), -0.345 * math.cos(a), top - length / 2 + 0.02), bevel=0.028, rot=(0, 0, deg))
    b.cylinder("head", "Dark", 0.022, 0.09, (0, 0, top + 0.08), bevel=0.0, segments=8)
    b.sphere("head", "Flame", (0.09, 0.09, 0.09), (0, 0, top + 0.2), u=12, v=8, shape=flame_shape)
    eyes(b, 0.345)


def neon_ring(b):
    """A rounded box lantern with a neon halo floating over it."""
    b.box("head", "Glow", (0.74, 0.62, 0.6), (0, 0, HEAD_Z), bevel=0.17, segments=3)
    for dz in (-0.32, 0.32):
        b.box("head", "Dark", (0.78, 0.66, 0.07), (0, 0, HEAD_Z + dz), bevel=0.03)
    b.torus("head", "Accent", 0.3, 0.045, (0, 0, HEAD_Z + 0.6), rot=(12, 0, 0), segments=32)
    eyes(b, 0.315)


HEADS = {
    "paper-lantern": paper_lantern,
    "firefly-jar": firefly_jar,
    "candle": candle,
    "neon-ring": neon_ring,
}


# ---------------------------------------------------------------------------------------------
# Rig and animation


def build_rig():
    data = bpy.data.armatures.new("LanternFolk")
    rig = bpy.data.objects.new("LanternFolk", data)
    bpy.context.scene.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    for name, (head, tail, parent) in BONES.items():
        bone = data.edit_bones.new(name)
        bone.head, bone.tail, bone.roll = head, tail, 0.0
        if parent:
            bone.parent = data.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    for pb in rig.pose.bones:
        pb.rotation_mode = "XYZ"
    return rig


def channelbag(action, slot):
    from bpy_extras.anim_utils import action_get_channelbag_for_slot

    return action_get_channelbag_for_slot(action, slot)


def animate(rig, name, keys, sharp=()):
    """Keys every bone on every channel at each of `keys`: (frame, {bone: (loc, rot°, scale)}).

    Bones are all upright, so local Y is up: location Y lifts, +X rotation tips forward,
    +Z rotation swings an arm on the character's left (arm.L) outwards and up.
    Frames in `sharp` get vector handles on root's location, for bounces off the ground.
    """
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    rig.animation_data_create()
    rig.animation_data.action = action
    for frame, pose in keys:
        for pb in rig.pose.bones:
            loc, rot, scale = pose.get(pb.name, ((0, 0, 0), (0, 0, 0), (1, 1, 1)))
            pb.location = loc
            pb.rotation_euler = [math.radians(a) for a in rot]
            pb.scale = scale
            for path in ("location", "rotation_euler", "scale"):
                pb.keyframe_insert(path, frame=frame)
    bag = channelbag(action, rig.animation_data.action_slot)
    for fc in bag.fcurves:
        if fc.data_path == 'pose.bones["root"].location':
            for kp in fc.keyframe_points:
                if int(kp.co.x) in sharp:
                    kp.handle_left_type = kp.handle_right_type = "VECTOR"
    rig.animation_data.action = None
    for pb in rig.pose.bones:
        pb.location, pb.rotation_euler, pb.scale = (0, 0, 0), (0, 0, 0), (1, 1, 1)
    return action


def pose(root=(0, 0, 0), body=(0, 0, 0), body_scale=(1, 1, 1), head=(0, 0, 0), arm_l=(0, 0, 0), arm_r=None, leg_l=(0, 0, 0), leg_r=None):
    """One frame's pose. Arm and leg rotations mirror across X when the right side isn't given."""
    mirror = lambda r: (r[0], -r[1], -r[2])
    return {
        "root": ((0, root[1], 0), (0, 0, 0), (1, 1, 1)),
        "body": ((0, 0, 0), body, body_scale),
        "head": ((0, 0, 0), head, (1, 1, 1)),
        "arm.L": ((0, 0, 0), arm_l, (1, 1, 1)),
        "arm.R": ((0, 0, 0), arm_r if arm_r is not None else mirror(arm_l), (1, 1, 1)),
        "leg.L": ((0, 0, 0), leg_l, (1, 1, 1)),
        "leg.R": ((0, 0, 0), leg_r if leg_r is not None else mirror(leg_l), (1, 1, 1)),
    }


def animations(rig):
    # Idle, 2 s: breathing, a slow look around, arms swaying.
    animate(rig, "idle", [
        (0, pose(body_scale=(1, 1, 1), head=(0, 0, 0), arm_l=(0, 0, 4))),
        (15, pose(body_scale=(1.015, 1.03, 1.015), head=(-3, 6, 2), arm_l=(-4, 0, 7))),
        (30, pose(body_scale=(1, 1, 1), head=(0, 0, 0), arm_l=(0, 0, 4))),
        (45, pose(body_scale=(1.015, 1.03, 1.015), head=(-3, -6, -2), arm_l=(4, 0, 7))),
        (60, pose(body_scale=(1, 1, 1), head=(0, 0, 0), arm_l=(0, 0, 4))),
    ])
    # Hop, 0.4 s (HOP_SECONDS in game): straight up, arms flung out, feet tucked; lands still.
    animate(rig, "hop", [
        (0, pose()),
        (2, pose(root=(0, 0.42, 0), body_scale=(0.95, 1.08, 0.95), arm_l=(-20, 0, 70), leg_l=(15, 0, 0))),
        (5, pose(root=(0, 0.7, 0), body_scale=(1, 1.04, 1), head=(-8, 0, 0), arm_l=(-30, 0, 110), leg_l=(25, 0, 0))),
        (8, pose(root=(0, 0.4, 0), arm_l=(-10, 0, 50), leg_l=(10, 0, 0))),
        (10, pose(body_scale=(1.06, 0.93, 1.06))),
        (12, pose()),
    ], sharp=(0, 10))
    # Celebrate, 0.7 s loop: two bounces with both arms waving overhead.
    animate(rig, "celebrate", [
        (0, pose(body_scale=(1.08, 0.9, 1.08), arm_l=(0, 0, 150))),
        (5, pose(root=(0, 0.38, 0), body_scale=(0.96, 1.06, 0.96), head=(-10, 0, 8), arm_l=(-10, 0, 170), arm_r=(-10, 0, -145), leg_l=(18, 0, 0))),
        (10, pose(body_scale=(1.08, 0.9, 1.08), head=(0, 0, 0), arm_l=(0, 0, 150))),
        (15, pose(root=(0, 0.38, 0), body_scale=(0.96, 1.06, 0.96), head=(-10, 0, -8), arm_l=(-10, 0, 145), arm_r=(-10, 0, -170), leg_l=(18, 0, 0))),
        (20, pose(body_scale=(1.08, 0.9, 1.08), arm_l=(0, 0, 150))),
    ], sharp=(0, 10, 20))
    # Slump, 2 s loop: hunched forward, head hanging, arms dangling, a slow sigh.
    animate(rig, "slump", [
        (0, pose(root=(0, -0.03, 0), body=(12, 0, 0), head=(16, 0, 0), arm_l=(-14, 0, 2), leg_l=(-4, 0, 0))),
        (30, pose(root=(0, -0.05, 0), body=(15, 0, 0), body_scale=(1.02, 0.97, 1.02), head=(21, 0, 5), arm_l=(-17, 0, 1), leg_l=(-4, 0, 0))),
        (60, pose(root=(0, -0.03, 0), body=(12, 0, 0), head=(16, 0, 0), arm_l=(-14, 0, 2), leg_l=(-4, 0, 0))),
    ])


# ---------------------------------------------------------------------------------------------


def export(rig, obj, path):
    for o in bpy.context.scene.objects:
        o.select_set(o in (rig, obj))
    bpy.context.view_layer.objects.active = rig
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=False,
        export_skins=True,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_frame_range=False,
        export_cameras=False,
        export_lights=False,
    )


def main():
    clear()
    mats = materials()
    rig = build_rig()
    animations(rig)
    meshes = {}
    for i, name in enumerate(CHARACTERS):
        b = Builder(mats)
        shared_body(b)
        HEADS[name](b)
        obj = b.finish(name, rig)
        collection = bpy.data.collections.new(name)
        bpy.context.scene.collection.children.link(collection)
        collection.objects.link(obj)
        meshes[name] = obj
    os.makedirs(EXPORT_DIR, exist_ok=True)
    for name, obj in meshes.items():
        export(rig, obj, os.path.join(EXPORT_DIR, f"{name}.glb"))
    bpy.ops.wm.save_as_mainfile(filepath=BLEND_PATH)
    return {name: len(obj.data.vertices) for name, obj in meshes.items()}


if __name__ == "__main__":
    result = main()
