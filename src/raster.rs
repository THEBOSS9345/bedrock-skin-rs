//! The software rasterizer: a port of the parts of fauxgl the Go version
//! draws with, kept to the same arithmetic in the same order so both render
//! the same pixels. See docs/rendering-pipeline.md.

use crate::gomath::{self, go_int};
use image::RgbaImage;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn add(self, b: Vec3) -> Vec3 {
        Vec3::new(self.x + b.x, self.y + b.y, self.z + b.z)
    }
    pub fn sub(self, b: Vec3) -> Vec3 {
        Vec3::new(self.x - b.x, self.y - b.y, self.z - b.z)
    }
    pub fn mul_scalar(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn dot(self, b: Vec3) -> f64 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }
    pub fn cross(self, b: Vec3) -> Vec3 {
        Vec3::new(
            self.y * b.z - self.z * b.y,
            self.z * b.x - self.x * b.z,
            self.x * b.y - self.y * b.x,
        )
    }
    pub fn normalize(self) -> Vec3 {
        let r = 1.0 / (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        Vec3::new(self.x * r, self.y * r, self.z * r)
    }
    pub fn min(self, b: Vec3) -> Vec3 {
        Vec3::new(
            gomath::min(self.x, b.x),
            gomath::min(self.y, b.y),
            gomath::min(self.z, b.z),
        )
    }
    pub fn max(self, b: Vec3) -> Vec3 {
        Vec3::new(
            gomath::max(self.x, b.x),
            gomath::max(self.y, b.y),
            gomath::max(self.z, b.z),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Vec4 {
    fn xyz(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
    fn outside(self) -> bool {
        let Vec4 { x, y, z, w } = self;
        x < -w || x > w || y < -w || y > w || z < -w || z > w
    }
    fn add(self, b: Vec4) -> Vec4 {
        Vec4 {
            x: self.x + b.x,
            y: self.y + b.y,
            z: self.z + b.z,
            w: self.w + b.w,
        }
    }
    fn sub(self, b: Vec4) -> Vec4 {
        Vec4 {
            x: self.x - b.x,
            y: self.y - b.y,
            z: self.z - b.z,
            w: self.w - b.w,
        }
    }
    fn mul_scalar(self, s: f64) -> Vec4 {
        Vec4 {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
            w: self.w * s,
        }
    }
    fn div_scalar(self, s: f64) -> Vec4 {
        Vec4 {
            x: self.x / s,
            y: self.y / s,
            z: self.z / s,
            w: self.w / s,
        }
    }
    fn dot(self, b: Vec4) -> f64 {
        self.x * b.x + self.y * b.y + self.z * b.z + self.w * b.w
    }
}

/// A 4x4 matrix, row major, as fauxgl's Matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const fn identity() -> Mat4 {
        Mat4([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn translation(v: Vec3) -> Mat4 {
        Mat4([
            [1.0, 0.0, 0.0, v.x],
            [0.0, 1.0, 0.0, v.y],
            [0.0, 0.0, 1.0, v.z],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn scaling(v: Vec3) -> Mat4 {
        Mat4([
            [v.x, 0.0, 0.0, 0.0],
            [0.0, v.y, 0.0, 0.0],
            [0.0, 0.0, v.z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn rotation(v: Vec3, a: f64) -> Mat4 {
        let v = v.normalize();
        let s = gomath::sin(a);
        let c = gomath::cos(a);
        let m = 1.0 - c;
        Mat4([
            [
                m * v.x * v.x + c,
                m * v.x * v.y + v.z * s,
                m * v.z * v.x - v.y * s,
                0.0,
            ],
            [
                m * v.x * v.y - v.z * s,
                m * v.y * v.y + c,
                m * v.y * v.z + v.x * s,
                0.0,
            ],
            [
                m * v.z * v.x + v.y * s,
                m * v.y * v.z - v.x * s,
                m * v.z * v.z + c,
                0.0,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn frustum(l: f64, r: f64, b: f64, t: f64, n: f64, f: f64) -> Mat4 {
        let t1 = 2.0 * n;
        let t2 = r - l;
        let t3 = t - b;
        let t4 = f - n;
        Mat4([
            [t1 / t2, 0.0, (r + l) / t2, 0.0],
            [0.0, t1 / t3, (t + b) / t3, 0.0],
            [0.0, 0.0, (-f - n) / t4, (-t1 * f) / t4],
            [0.0, 0.0, -1.0, 0.0],
        ])
    }

    fn perspective_matrix(fovy: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
        let ymax = near * gomath::tan(fovy * std::f64::consts::PI / 360.0);
        let xmax = ymax * aspect;
        Mat4::frustum(-xmax, xmax, -ymax, ymax, near, far)
    }

    pub fn look_at(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
        let z = eye.sub(center).normalize();
        let x = up.cross(z).normalize();
        let y = z.cross(x);
        Mat4([
            [x.x, x.y, x.z, -x.dot(eye)],
            [y.x, y.y, y.z, -y.dot(eye)],
            [z.x, z.y, z.z, -z.dot(eye)],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn screen(w: usize, h: usize) -> Mat4 {
        let w2 = w as f64 / 2.0;
        let h2 = h as f64 / 2.0;
        Mat4([
            [w2, 0.0, 0.0, w2],
            [0.0, -h2, 0.0, h2],
            [0.0, 0.0, 0.5, 0.5],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// Translate, Scale, Rotate and Perspective apply their matrix after this
    /// one, as fauxgl's methods do: the new matrix times this.
    pub fn translate(self, v: Vec3) -> Mat4 {
        Mat4::translation(v).mul(&self)
    }
    pub fn scale(self, v: Vec3) -> Mat4 {
        Mat4::scaling(v).mul(&self)
    }
    pub fn rotate(self, v: Vec3, a: f64) -> Mat4 {
        Mat4::rotation(v, a).mul(&self)
    }
    pub fn perspective(self, fovy: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
        Mat4::perspective_matrix(fovy, aspect, near, far).mul(&self)
    }

    pub fn mul(&self, b: &Mat4) -> Mat4 {
        let (a, b) = (&self.0, &b.0);
        let mut m = [[0.0; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell =
                    a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
            }
        }
        Mat4(m)
    }

    pub fn mul_position(&self, b: Vec3) -> Vec3 {
        let a = &self.0;
        Vec3::new(
            a[0][0] * b.x + a[0][1] * b.y + a[0][2] * b.z + a[0][3],
            a[1][0] * b.x + a[1][1] * b.y + a[1][2] * b.z + a[1][3],
            a[2][0] * b.x + a[2][1] * b.y + a[2][2] * b.z + a[2][3],
        )
    }

    fn mul_position_w(&self, b: Vec3) -> Vec4 {
        let a = &self.0;
        Vec4 {
            x: a[0][0] * b.x + a[0][1] * b.y + a[0][2] * b.z + a[0][3],
            y: a[1][0] * b.x + a[1][1] * b.y + a[1][2] * b.z + a[1][3],
            z: a[2][0] * b.x + a[2][1] * b.y + a[2][2] * b.z + a[2][3],
            w: a[3][0] * b.x + a[3][1] * b.y + a[3][2] * b.z + a[3][3],
        }
    }
}

/// One corner of a triangle: where it is, its texture coordinate, and its
/// clip-space position once the vertex stage has run.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Vertex {
    pub position: Vec3,
    pub texture: Vec3,
    output: Vec4,
}

impl Vertex {
    pub fn new(position: Vec3, u: f64, v: f64) -> Vertex {
        Vertex {
            position,
            texture: Vec3::new(u, v, 0.0),
            output: Vec4::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Triangle(pub [Vertex; 3]);

/// fauxgl's barycentric interpolation, with b.w the reciprocal of the
/// weights' sum: ((0 + v1*b.x) + v2*b.y + v3*b.z) * b.w.
fn interpolate3(v1: Vec3, v2: Vec3, v3: Vec3, b: Vec4) -> Vec3 {
    Vec3::default()
        .add(v1.mul_scalar(b.x))
        .add(v2.mul_scalar(b.y))
        .add(v3.mul_scalar(b.z))
        .mul_scalar(b.w)
}

fn interpolate4(v1: Vec4, v2: Vec4, v3: Vec4, b: Vec4) -> Vec4 {
    Vec4::default()
        .add(v1.mul_scalar(b.x))
        .add(v2.mul_scalar(b.y))
        .add(v3.mul_scalar(b.z))
        .mul_scalar(b.w)
}

fn interpolate_vertex(v1: &Vertex, v2: &Vertex, v3: &Vertex, b: Vec4) -> Vertex {
    Vertex {
        position: interpolate3(v1.position, v2.position, v3.position, b),
        texture: interpolate3(v1.texture, v2.texture, v3.texture, b),
        output: interpolate4(v1.output, v2.output, v3.output, b),
    }
}

fn barycentric(p1: Vec3, p2: Vec3, p3: Vec3, p: Vec3) -> Vec4 {
    let v0 = p2.sub(p1);
    let v1 = p3.sub(p1);
    let v2 = p.sub(p1);
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);
    let d = d00 * d11 - d01 * d01;
    let v = (d11 * d20 - d01 * d21) / d;
    let w = (d00 * d21 - d01 * d20) / d;
    let u = 1.0 - v - w;
    Vec4 {
        x: u,
        y: v,
        z: w,
        w: 1.0,
    }
}

struct ClipPlane {
    p: Vec4,
    n: Vec4,
}

const CLIP_PLANES: [ClipPlane; 6] = [
    ClipPlane {
        p: Vec4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        n: Vec4 {
            x: -1.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    },
    ClipPlane {
        p: Vec4 {
            x: -1.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        n: Vec4 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    },
    ClipPlane {
        p: Vec4 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
            w: 1.0,
        },
        n: Vec4 {
            x: 0.0,
            y: -1.0,
            z: 0.0,
            w: 1.0,
        },
    },
    ClipPlane {
        p: Vec4 {
            x: 0.0,
            y: -1.0,
            z: 0.0,
            w: 1.0,
        },
        n: Vec4 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
            w: 1.0,
        },
    },
    ClipPlane {
        p: Vec4 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
            w: 1.0,
        },
        n: Vec4 {
            x: 0.0,
            y: 0.0,
            z: -1.0,
            w: 1.0,
        },
    },
    ClipPlane {
        p: Vec4 {
            x: 0.0,
            y: 0.0,
            z: -1.0,
            w: 1.0,
        },
        n: Vec4 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
            w: 1.0,
        },
    },
];

impl ClipPlane {
    fn point_in_front(&self, v: Vec4) -> bool {
        v.sub(self.p).dot(self.n) > 0.0
    }
    fn intersect_segment(&self, v0: Vec4, v1: Vec4) -> Vec4 {
        let u = v1.sub(v0);
        let w = v0.sub(self.p);
        let d = self.n.dot(u);
        let n = -self.n.dot(w);
        v0.add(u.mul_scalar(n / d))
    }
}

fn sutherland_hodgman(points: Vec<Vec4>) -> Vec<Vec4> {
    let mut output = points;
    for plane in &CLIP_PLANES {
        let input = std::mem::take(&mut output);
        if input.is_empty() {
            return Vec::new();
        }
        let mut s = input[input.len() - 1];
        for &e in &input {
            if plane.point_in_front(e) {
                if !plane.point_in_front(s) {
                    output.push(plane.intersect_segment(s, e));
                }
                output.push(e);
            } else if plane.point_in_front(s) {
                output.push(plane.intersect_segment(s, e));
            }
            s = e;
        }
    }
    output
}

fn clip_triangle(t: &[Vertex; 3]) -> Vec<[Vertex; 3]> {
    let (w1, w2, w3) = (t[0].output, t[1].output, t[2].output);
    let (p1, p2, p3) = (w1.xyz(), w2.xyz(), w3.xyz());
    let points = sutherland_hodgman(vec![w1, w2, w3]);
    let mut out = Vec::new();
    for i in 2..points.len() {
        let b1 = barycentric(p1, p2, p3, points[0].xyz());
        let b2 = barycentric(p1, p2, p3, points[i - 1].xyz());
        let b3 = barycentric(p1, p2, p3, points[i].xyz());
        out.push([
            interpolate_vertex(&t[0], &t[1], &t[2], b1),
            interpolate_vertex(&t[0], &t[1], &t[2], b2),
            interpolate_vertex(&t[0], &t[1], &t[2], b3),
        ]);
    }
    out
}

/// A texture sampled nearest-neighbour, as the Go version's fastImageTexture.
pub(crate) struct Texture<'a> {
    width: usize,
    height: usize,
    pix: &'a [u8],
}

impl<'a> Texture<'a> {
    pub fn new(img: &'a RgbaImage) -> Texture<'a> {
        Texture {
            width: img.width() as usize,
            height: img.height() as usize,
            pix: img.as_raw(),
        }
    }

    /// fauxgl's Sample, including its v = 1 - v (mesh.rs pre-flips V to
    /// cancel it), as the texel's bytes: every byte survives byte/255*255
    /// exactly, so the colour needs no floats. For u and v already in [0, 1)
    /// the floors are skipped - u - floor(u) is u there, bar -0 becoming +0,
    /// the same texel. None stands for a coordinate off the texture, where
    /// Go would panic; it draws nothing.
    /// See docs/design-decisions.md#why-the-rasterizer-is-specialised.
    fn sample(&self, u: f64, v: f64) -> Option<[u8; 4]> {
        let mut v = 1.0 - v;
        let mut u = u;
        if !(0.0..1.0).contains(&u) {
            u -= u.floor();
        }
        if !(0.0..1.0).contains(&v) {
            v -= v.floor();
        }
        let x = go_int(u * self.width as f64);
        let y = go_int(v * self.height as f64);
        let i = (y.wrapping_mul(self.width as i64))
            .wrapping_add(x)
            .wrapping_mul(4);
        if i < 0 || i as usize + 3 >= self.pix.len() {
            return None;
        }
        let i = i as usize;
        Some([
            self.pix[i],
            self.pix[i + 1],
            self.pix[i + 2],
            self.pix[i + 3],
        ])
    }
}

/// The alpha test: fragments below half opacity (alpha/255 < 0.5, a byte
/// below 128) are dropped, colour and depth both.
/// See docs/rendering-pipeline.md#alpha-testing.
const ALPHA_THRESHOLD: u8 = 128;

/// A colour and depth buffer to draw triangles into.
pub(crate) struct Context {
    width: usize,
    height: usize,
    color: Vec<u8>,
    depth: Vec<f64>,
    screen: Mat4,
}

impl Context {
    pub fn new(width: usize, height: usize) -> Context {
        Context {
            width,
            height,
            color: vec![0; width * height * 4],
            depth: vec![f64::MAX; width * height],
            screen: Mat4::screen(width, height),
        }
    }

    pub fn into_image(self) -> RgbaImage {
        RgbaImage::from_raw(self.width as u32, self.height as u32, self.color)
            .expect("buffer is width*height*4")
    }

    /// Draws one triangle: the vertex stage (matrix), clipping, then
    /// rasterization with the alpha-tested texture. Back faces are drawn
    /// too: cube winding is not consistent.
    pub fn draw_triangle(&mut self, t: &Triangle, matrix: &Mat4, tex: &Texture) {
        let mut v = t.0;
        for vert in &mut v {
            vert.output = matrix.mul_position_w(vert.position);
        }
        if v.iter().any(|v| v.output.outside()) {
            for t in clip_triangle(&v) {
                self.draw_clipped(t, tex);
            }
        } else {
            self.draw_clipped(v, tex);
        }
    }

    fn draw_clipped(&mut self, v: [Vertex; 3], tex: &Texture) {
        let [mut v0, v1, mut v2] = v;
        let mut ndc0 = v0.output.div_scalar(v0.output.w).xyz();
        let ndc1 = v1.output.div_scalar(v1.output.w).xyz();
        let mut ndc2 = v2.output.div_scalar(v2.output.w).xyz();
        let a = (ndc1.x - ndc0.x) * (ndc2.y - ndc0.y) - (ndc2.x - ndc0.x) * (ndc1.y - ndc0.y);
        if a < 0.0 {
            std::mem::swap(&mut v0, &mut v2);
            std::mem::swap(&mut ndc0, &mut ndc2);
        }
        let s0 = self.screen.mul_position(ndc0);
        let s1 = self.screen.mul_position(ndc1);
        let s2 = self.screen.mul_position(ndc2);
        self.rasterize(&v0, &v1, &v2, s0, s1, s2, tex);
    }

    #[allow(clippy::too_many_arguments)]
    fn rasterize(
        &mut self,
        v0: &Vertex,
        v1: &Vertex,
        v2: &Vertex,
        s0: Vec3,
        s1: Vec3,
        s2: Vec3,
        tex: &Texture,
    ) {
        fn edge(a: Vec3, b: Vec3, c: Vec3) -> f64 {
            (b.x - c.x) * (a.y - c.y) - (b.y - c.y) * (a.x - c.x)
        }

        let lo = s0.min(s1.min(s2));
        let hi = s0.max(s1.max(s2));
        let x0 = go_int(lo.x.floor());
        let x1 = go_int(hi.x.ceil());
        let y0 = go_int(lo.y.floor());
        let y1 = go_int(hi.y.ceil());

        let p = Vec3::new(x0 as f64 + 0.5, y0 as f64 + 0.5, 0.0);
        let mut w00 = edge(s1, s2, p);
        let mut w01 = edge(s2, s0, p);
        let mut w02 = edge(s0, s1, p);
        let a01 = s1.y - s0.y;
        let b01 = s0.x - s1.x;
        let a12 = s2.y - s1.y;
        let b12 = s1.x - s2.x;
        let a20 = s0.y - s2.y;
        let b20 = s2.x - s0.x;

        let ra = 1.0 / edge(s0, s1, s2);
        let r0 = 1.0 / v0.output.w;
        let r1 = 1.0 / v1.output.w;
        let r2 = 1.0 / v2.output.w;
        let ra12 = 1.0 / a12;
        let ra20 = 1.0 / a20;
        let ra01 = 1.0 / a01;

        let width = self.width as i64;
        let mut y = y0;
        while y <= y1 {
            let mut d = 0.0f64;
            let d0 = -w00 * ra12;
            let d1 = -w01 * ra20;
            let d2 = -w02 * ra01;
            if w00 < 0.0 && d0 > d {
                d = d0;
            }
            if w01 < 0.0 && d1 > d {
                d = d1;
            }
            if w02 < 0.0 && d2 > d {
                d = d2;
            }
            d = go_int(d) as f64;
            if d < 0.0 {
                d = 0.0;
            }
            let mut w0 = w00 + a12 * d;
            let mut w1 = w01 + a20 * d;
            let mut w2 = w02 + a01 * d;
            let mut was_inside = false;
            let mut x = x0.wrapping_add(go_int(d));
            while x <= x1 {
                let b0 = w0 * ra;
                let b1 = w1 * ra;
                let b2 = w2 * ra;
                w0 += a12;
                w1 += a20;
                w2 += a01;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    if was_inside {
                        break;
                    }
                    x += 1;
                    continue;
                }
                was_inside = true;
                let i = y.wrapping_mul(width).wrapping_add(x);
                if i < 0 || i as usize >= self.depth.len() {
                    x += 1;
                    continue;
                }
                let i = i as usize;
                let z = b0 * s0.z + b1 * s1.z + b2 * s2.z;
                let bz = z + 0.0;
                if bz > self.depth[i] {
                    x += 1;
                    continue;
                }
                let mut b = Vec4 {
                    x: b0 * r0,
                    y: b1 * r1,
                    z: b2 * r2,
                    w: 0.0,
                };
                b.w = 1.0 / (b.x + b.y + b.z);
                let tc = interpolate3(v0.texture, v1.texture, v2.texture, b);
                let color = match tex.sample(tc.x, tc.y) {
                    Some(c) if c[3] >= ALPHA_THRESHOLD => c,
                    _ => {
                        x += 1;
                        continue;
                    }
                };
                if bz <= self.depth[i] {
                    self.depth[i] = z;
                    self.put(x, y, color);
                }
                x += 1;
            }
            w00 += b12;
            w01 += b20;
            w02 += b01;
            y += 1;
        }
    }

    /// Writes a fragment as fauxgl does: blended over what is there when it
    /// is not fully opaque, else stored, and only inside the image.
    fn put(&mut self, x: i64, y: i64, color: [u8; 4]) {
        let stride = self.width as i64 * 4;
        if color[3] < 255 {
            // color.NRGBA().RGBA(): premultiplied, 16 bits a channel.
            let [r8, g8, b8, a8] = color;
            let a8 = a8 as u32;
            let pre = |c: u8| -> u32 {
                let c = c as u32;
                ((c | (c << 8)).wrapping_mul(a8)) / 0xff
            };
            let (sr, sg, sb, sa) = (pre(r8), pre(g8), pre(b8), a8 | (a8 << 8));
            let a = (0xffff - sa).wrapping_mul(0x101);
            let j = y.wrapping_mul(stride).wrapping_add(x.wrapping_mul(4));
            if j < 0 || j as usize + 3 >= self.color.len() {
                return;
            }
            let j = j as usize;
            for (k, s) in [sr, sg, sb, sa].into_iter().enumerate() {
                let d = self.color[j + k] as u32;
                self.color[j + k] = ((d.wrapping_mul(a) / 0xffff).wrapping_add(s) >> 8) as u8;
            }
        } else if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let j = (y as usize * self.width + x as usize) * 4;
            self.color[j..j + 4].copy_from_slice(&color);
        }
    }
}
