"""Assemble world-space collision geometry from a decoded Havok 4.6 packfile (see havok.py).

Shapes handled: hkMoppBvTreeShape (-> child), hkSimpleMeshShape (vertices + triangles), hkBoxShape,
hkConvexVerticesShape (faces rebuilt from planeEquations), hkConvexTransformShape / hkTransformShape,
hkListShape.  Transforms are hkTransform = three basis vec4 columns + translation vec4 (row-vector, column-major).
"""
import math
import numpy as np
import havok

def _mat(t):
    t=np.array(t,dtype=np.float64).reshape(4,4)  # rows: c0,c1,c2,translation
    m=np.eye(4);m[:3,:3]=t[:3,:3].T;m[:3,3]=t[3,:3];return m

def _apply(m,pts):return (m[:3,:3]@pts.T).T+m[:3,3]

class Collision:
    def __init__(self,pf):
        self.pf=pf;self.bodies=[];self.skipped={}
        for (si,off),cn in pf.objects().items():
            if cn=='hkRigidBody':
                o=pf.decode_object(si,off,cn);ref=o['collidable']['shape']
                if not ref:continue
                m=_mat(o['motion']['motionState']['transform']);tris=self.shape_tris(ref['$ref'],np.eye(4))
                if tris is None or len(tris)==0:continue
                self.bodies.append(dict(name=o['name'],shape=ref['class'],matrix=m,tris=_apply_tris(m,tris),
                                        friction=o['material']['friction'],restitution=o['material']['restitution'],
                                        motion_type=o['motion']['type'],mass_inv=o['motion']['inertiaAndMassInv'][3]))
    def shape_tris(self,ref,m):
        pf=self.pf;cn=pf.virt.get(ref)
        if cn is None:return None
        o=pf.decode_object(ref[0],ref[1],cn)
        if cn=='hkMoppBvTreeShape':
            c=o['child']['childShape'] if o.get('child') else None
            return self.shape_tris(c['$ref'],m) if c else None
        if cn=='hkSimpleMeshShape':
            v=np.array([x[:3] for x in pf.array_elements(o['vertices'])],dtype=np.float64).reshape(-1,3)
            t=pf.array_elements(o['triangles']);idx=np.array([[x['a'],x['b'],x['c']] for x in t],dtype=np.int64).reshape(-1,3)
            return _apply_tris(m,v[idx]) if len(idx) else np.zeros((0,3,3))
        if cn=='hkBoxShape':
            h=np.array(o['halfExtents'][:3]);return _apply_tris(m,_box(h))
        if cn=='hkConvexVerticesShape':
            return _apply_tris(m,_convex(o,pf))
        if cn in('hkConvexTransformShape','hkTransformShape'):
            c=o['childShape']['childShape'] if isinstance(o.get('childShape'),dict) and 'childShape' in o['childShape'] else None
            if not c:return None
            return self.shape_tris(c['$ref'],m@_mat(o['transform']))
        if cn=='hkConvexTranslateShape':
            c=o['childShape']['childShape'] if isinstance(o.get('childShape'),dict) and 'childShape' in o['childShape'] else None
            if not c:return None
            t=np.eye(4);t[:3,3]=o['translation'][:3];return self.shape_tris(c['$ref'],m@t)
        if cn=='hkListShape':
            out=[]
            for ci in pf.array_elements(o['childInfo']):
                s=ci.get('shape') if isinstance(ci,dict) else None
                if s and s.get('$ref'):
                    t=self.shape_tris(s['$ref'],m)
                    if t is not None and len(t):out.append(t)
            return np.concatenate(out) if out else None
        self.skipped[cn]=self.skipped.get(cn,0)+1
        return None
    def all_triangles(self):
        return np.concatenate([b['tris'] for b in self.bodies]) if self.bodies else np.zeros((0,3,3))

def _apply_tris(m,t):
    if len(t)==0:return t
    shp=t.shape;return _apply(m,t.reshape(-1,3)).reshape(shp)

def _box(h):
    x,y,z=h;v=np.array([[sx*x,sy*y,sz*z] for sx in(-1,1) for sy in(-1,1) for sz in(-1,1)])
    faces=[(0,1,3,2),(4,6,7,5),(0,4,5,1),(2,3,7,6),(0,2,6,4),(1,5,7,3)]
    tris=[]
    for a,b,c,d in faces:tris+= [[v[a],v[b],v[c]],[v[a],v[c],v[d]]]
    return np.array(tris)

def _convex(o,pf):
    fv=pf.array_elements(o['rotatedVertices']);n=o['numVertices'];pts=[]
    for f in fv:
        for i in range(4):pts.append([f['x'][i],f['y'][i],f['z'][i]])
    pts=np.array(pts[:n],dtype=np.float64)
    planes=[p for p in pf.array_elements(o['planeEquations'])]
    tris=[]
    for pl in planes:
        nrm=np.array(pl[:3]);d=pl[3]
        on=[p for p in pts if abs(nrm@p+d)<1e-3]
        if len(on)<3:continue
        c=np.mean(on,axis=0);u=np.cross(nrm,[1,0,0]) if abs(nrm[0])<0.9 else np.cross(nrm,[0,1,0]);u/=np.linalg.norm(u);w=np.cross(nrm,u)
        on.sort(key=lambda p:math.atan2((p-c)@w,(p-c)@u))
        for i in range(1,len(on)-1):tris.append([on[0],on[i],on[i+1]])
    return np.array(tris,dtype=np.float64) if tris else np.zeros((0,3,3))

def write_obj(path,tris):
    with open(path,'w') as f:
        for i,t in enumerate(tris):
            for v in t:f.write(f'v {v[0]:.5f} {v[1]:.5f} {v[2]:.5f}\n')
            f.write(f'f {3*i+1} {3*i+2} {3*i+3}\n')
