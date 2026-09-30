//! Original EA matrix operations and AreaManager::CalcRenderingModelMatrix.
//! Matrices retain the executable's row-vector memory layout, translation at 12..14.
pub type Matrix = [f32; 16];
pub const IDENTITY: Matrix = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

pub fn translation(p: [f32; 3]) -> Matrix {
    let mut m = IDENTITY;
    m[12..15].copy_from_slice(&p);
    m
}

pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1].mul_add(b[2], -(b[1] * a[2])),
        a[2].mul_add(b[0], -(a[0] * b[2])),
        a[0].mul_add(b[1], -(a[1] * b[0])),
    ]
}

pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[2].mul_add(v[2], v[0].mul_add(v[0], v[1] * v[1])) as f64).sqrt() as f32;
    let scale = 1.0 / length;
    v.map(|x| x * scale)
}

/// rmMult, including the paired-single multiply / multiply-add order.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|index| {
        let row = index / 4;
        let col = index % 4;
        let x = a[row * 4] * b[col];
        let x = a[row * 4 + 1].mul_add(b[4 + col], x);
        let x = a[row * 4 + 2].mul_add(b[8 + col], x);
        a[row * 4 + 3].mul_add(b[12 + col], x)
    })
}

/// Matrix44FromAxisAngle at 0x8041c74c. Caller supplies a normalized axis.
pub fn axis_angle(axis: [f32; 3], angle: f32) -> Matrix {
    let [x, y, z] = axis;
    let (s, c) = crate::character_input::ea_sin_cos(angle);
    let t = 1.0 - c;
    let tx = t * x;
    let ty = t * y;
    let tz = t * z;
    let sx = s * x;
    let sy = s * y;
    let sz = s * z;
    [
        tx.mul_add(x, c),
        tx.mul_add(y, sz),
        tx.mul_add(z, -sy),
        0.,
        ty.mul_add(x, -sz),
        ty.mul_add(y, c),
        ty.mul_add(z, sx),
        0.,
        tz.mul_add(x, sy),
        tz.mul_add(y, -sx),
        tz.mul_add(z, c),
        0.,
        0.,
        0.,
        0.,
        1.,
    ]
}

/// AreaManager radius at +0x10 and the original disable-curvature flag.
#[derive(Clone, Copy, Debug)]
pub struct AreaTransform {
    pub radius: f32,
    pub disabled: bool,
}
impl AreaTransform {
    /// Original 0x803d78b8, including its orientation basis, not a radial height fit.
    pub fn model_matrix(self, p: [f32; 3]) -> Matrix {
        if self.disabled {
            return translation(p);
        }
        let inverse = 1.0 / self.radius;
        let half_pi = f32::from_bits(0x3fc90fdb);
        let a = -(p[0].mul_add(inverse, -half_pi));
        let b = -(p[2].mul_add(inverse, -half_pi));
        let extent = self.radius + p[1];
        let sin_a = (a as f64).sin() as f32;
        let cos_a = (a as f64).cos() as f32;
        let sin_b = (b as f64).sin() as f32;
        let cos_b = (b as f64).cos() as f32;
        let radial = [sin_b * cos_a, sin_b * sin_a, cos_b];
        let mut m = translation([
            radial[0] * extent,
            radial[1] * extent - self.radius,
            radial[2] * extent,
        ]);
        let normal = [-radial[0], radial[1], -radial[2]];
        let side = cross(normal, [0., 0., 1.]);
        let forward = cross(side, normal);
        let side = normalize(side);
        let forward = normalize(forward);
        for i in 0..3 {
            m[i * 4] = side[i];
            m[i * 4 + 1] = normal[i];
            m[i * 4 + 2] = forward[i];
        }
        m
    }
}
