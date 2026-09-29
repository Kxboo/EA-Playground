"""Lightweight, software wireframe and skeleton preview using Tk Canvas."""
import math
import tkinter as tk
from core import eagl_skeleton as ske


def pose(skeleton, clip=None, frame=0):
    transforms = {}
    active = set()
    def world(index):
        if index in transforms:
            return transforms[index]
        if index in active:
            raise ValueError('Cyclic skeleton hierarchy')
        active.add(index)
        bone = skeleton.bones[index]
        q, t, scale = bone.quaternion, bone.local_translation, bone.scale
        if clip:
            qs, ts = clip.rot_by_bone.get(index), clip.trans_by_bone.get(index)
            if qs:
                q = qs[min(frame, len(qs)-1)] or q
            if ts:
                t = ts[min(frame, len(ts)-1)] or t
            scales=getattr(clip,'scale_by_bone',{}).get(index)
            if scales:
                scale=scales[min(frame,len(scales)-1)] or scale
        norm = math.sqrt(sum(x*x for x in q))
        q = tuple(x/norm for x in q) if norm else (0,0,0,1)
        matrix = ske._quat_to_matrix(*q)
        matrix = tuple(matrix[r*3+c]*scale[c] for r in range(3) for c in range(3))
        if bone.parent_idx >= 0:
            pm, pt = world(bone.parent_idx)
            t = tuple(a+b for a,b in zip(pt, ske._mat3_vec_mul(pm,t)))
            matrix = ske._mat3_mul(pm, matrix)
        transforms[index] = matrix,t
        active.remove(index)
        return matrix,t
    return [world(i)[1] for i in range(len(skeleton.bones))]


class Preview(tk.Canvas):
    def __init__(self, parent):
        super().__init__(parent, bg='#17212b', highlightthickness=0, width=520, height=380)
        self.lines = []
        self.yaw, self.pitch, self.zoom = .4, .15, 1.0
        self.center, self.radius = (0,0,0), 1
        self.drag = None
        self.caption = 'Open a model, texture, or animation'
        self.photo = None
        self.bind('<Configure>', lambda e:self.draw())
        self.bind('<ButtonPress-1>', lambda e:setattr(self,'drag',(e.x,e.y)))
        self.bind('<B1-Motion>', self.rotate)
        self.bind('<MouseWheel>', self.wheel)
        self.bind('<Double-Button-1>', lambda e:self.fit())

    def rotate(self, event):
        if self.drag:
            x,y = self.drag
            self.yaw += (event.x-x)*.008
            self.pitch += (event.y-y)*.008
            self.drag = event.x,event.y
            self.draw()

    def wheel(self, event):
        self.zoom = min(20,max(.05,self.zoom * (1.1 if event.delta > 0 else 1/1.1)))
        self.draw()

    def set_lines(self, lines, caption, fit=True):
        self.photo = None
        self.lines = lines
        self.caption = caption
        if fit:
            self.fit()
        else:
            self.draw()

    def fit(self):
        points = [p for a,b in self.lines for p in (a,b)]
        if points:
            lo = [min(p[i] for p in points) for i in range(3)]
            hi = [max(p[i] for p in points) for i in range(3)]
            self.center = tuple((a+b)/2 for a,b in zip(lo,hi))
            self.radius = max(max(b-a for a,b in zip(lo,hi))/2, .001)
        self.zoom = 1
        self.draw()

    def model(self, result):
        count = result.total_faces
        step = max(1, math.ceil(count/6500))
        lines = []
        cursor = 0
        for mesh in result.meshes:
            for face in mesh.faces:
                cursor += 1
                if cursor % step:
                    continue
                ps = [mesh.positions[v[0]] for v in face]
                lines.extend([(ps[0],ps[1]),(ps[1],ps[2]),(ps[2],ps[0])])
        self.set_lines(lines,f'Wireframe · {count:,} triangles' + (f' · preview samples 1/{step}' if step>1 else '') + '\nDrag to orbit · scroll to zoom · double-click to fit')

    def skeleton(self, skeleton, clip=None, frame=0, fit=True):
        points = pose(skeleton,clip,frame)
        lines = [(points[b.parent_idx],points[b.index]) for b in skeleton.bones if b.parent_idx>=0]
        caption = f'{len(points)} bones · bind pose' if not clip else f'{clip.name} · frame {frame+1}/{clip.sample_count} · assumed 30 fps'
        self.set_lines(lines,caption,fit)

    def texture(self, png):
        photo = tk.PhotoImage(data=png)
        factor = max(1, math.ceil(max(photo.width()/max(self.winfo_width()-40,1),photo.height()/max(self.winfo_height()-70,1))))
        self.caption = f'{photo.width()} × {photo.height()} pixels · PNG preview'
        self.photo = photo.subsample(factor,factor)
        self.lines = []
        self.draw()

    def draw(self):
        self.delete('all')
        w,h = max(self.winfo_width(),100), max(self.winfo_height(),100)
        if self.photo:
            self.create_image(w/2,h/2,image=self.photo)
        else:
            cy,sy,cp,sp = math.cos(self.yaw),math.sin(self.yaw),math.cos(self.pitch),math.sin(self.pitch)
            scale = min(w,h)*.38*self.zoom/self.radius
            def point(p):
                x,y,z = (a-b for a,b in zip(p,self.center))
                xx,zz = x*cy+z*sy, -x*sy+z*cy
                yy = y*cp-zz*sp
                return w/2+xx*scale,h/2-yy*scale
            for a,b in self.lines:
                self.create_line(*point(a),*point(b),fill='#73c6b6')
        self.create_text(14,14,anchor='nw',text=self.caption,fill='#e1e8f0',font=('Segoe UI',10),width=max(w-28,100))
