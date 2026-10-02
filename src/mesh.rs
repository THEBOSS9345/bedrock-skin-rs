//! Turning a bone tree into textured triangles.
//! See docs/rendering-pipeline.md.

use std::collections::HashMap;

use serde_json::Value;

use crate::animation::{BonePose, Pose};
use crate::geometry::{Bone, Cube, Geometry, read_f64s};
use crate::jsonread::Reader;
use crate::polymesh::add_poly_mesh;
use crate::raster::{Mat4, Triangle, Vec3, Vertex};

/// A texture-pixel rectangle for one cube face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct UvRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub(crate) const FACES: [&str; 6] = ["up", "down", "north", "south", "east", "west"];

/// The six faces of Bedrock's "unwrapped box" layout for a cube of size
/// (w, h, d) with UV origin (u, v), in [`FACES`] order.
/// See docs/geometry-format.md#box-uv-the-common-case.
pub(crate) fn box_uv_rects(u: f64, v: f64, w: f64, h: f64, d: f64) -> [(&'static str, UvRect); 6] {
    [
        (
            "up",
            UvRect {
                x: u + d,
                y: v,
                w,
                h: d,
            },
        ),
        (
            "down",
            UvRect {
                x: u + d + w,
                y: v,
                w,
                h: d,
            },
        ),
        (
            "north",
            UvRect {
                x: u + d,
                y: v + d,
                w,
                h,
            },
        ),
        (
            "south",
            UvRect {
                x: u + d + w + d,
                y: v + d,
                w,
                h,
            },
        ),
        (
            "east",
            UvRect {
                x: u + d + w,
                y: v + d,
                w: d,
                h,
            },
        ),
        (
            "west",
            UvRect {
                x: u,
                y: v + d,
                w: d,
                h,
            },
        ),
    ]
}

/// A cube's size and origin, when both carry the three components the
/// format requires. A relayed client upload has no such guarantee, and every
/// caller skips the cube instead. See docs/design-decisions.md#why-malformed-cubes-are-skipped.
pub(crate) fn cube_dims(c: &Cube) -> Option<([f64; 3], [f64; 3])> {
    if c.size.len() < 3 || c.origin.len() < 3 {
        return None;
    }
    Some((
        [c.size[0], c.size[1], c.size[2]],
        [c.origin[0], c.origin[1], c.origin[2]],
    ))
}

/// The cube's face rectangles by face name, or None when it has no usable
/// uv. Per-face cubes list only the faces they draw.
pub(crate) fn cube_uv_rects(c: &Cube) -> Option<Vec<(String, UvRect)>> {
    let uv = c.uv.as_ref()?;
    if let Some(arr) = read_f64s(uv)
        && arr.len() >= 2
    {
        let (size, _) = cube_dims(c)?;
        return Some(
            box_uv_rects(arr[0], arr[1], size[0], size[1], size[2])
                .into_iter()
                .map(|(f, r)| (f.to_string(), r))
                .collect(),
        );
    }
    per_face_uv_rects(uv)
}

/// Bedrock's per-face form: `{"north": {"uv": [u, v], "uv_size": [w, h]}}`.
fn per_face_uv_rects(v: &Value) -> Option<Vec<(String, UvRect)>> {
    #[derive(Default)]
    struct Entry {
        uv: Vec<f64>,
        uv_size: Vec<f64>,
    }
    let mut r = Reader::default();
    let mut faces: Vec<(String, Entry)> = Vec::new();
    r.map(v, &mut faces, |r, v, e| {
        r.object(v, |r, k, v| match k {
            "uv" => r.f64s(v, &mut e.uv),
            "uv_size" => r.f64s(v, &mut e.uv_size),
            _ => {}
        })
    });
    if r.type_error {
        return None;
    }
    Some(
        faces
            .into_iter()
            .filter(|(_, e)| e.uv.len() >= 2)
            .map(|(name, e)| {
                let (w, h) = if e.uv_size.len() >= 2 {
                    (e.uv_size[0], e.uv_size[1])
                } else {
                    (0.0, 0.0)
                };
                (
                    name,
                    UvRect {
                        x: e.uv[0],
                        y: e.uv[1],
                        w,
                        h,
                    },
                )
            })
            .collect(),
    )
}

fn rect_for<'a>(rects: &'a [(String, UvRect)], face: &str) -> Option<&'a UvRect> {
    // Go keeps these in a map, where a repeated face's last entry wins.
    rects.iter().rev().find(|(f, _)| f == face).map(|(_, r)| r)
}

/// Maps the corner loop [-1,-1] -> [1,-1] -> [1,1] -> [-1,1] to an offset from
/// the cube's centre for each face, given half-extents. The bottom of a face
/// pairs with the texture's bottom row; see docs/rendering-pipeline.md#face-geometry.
fn face_corner(face: &str, u: f64, v: f64, hx: f64, hy: f64, hz: f64) -> Vec3 {
    match face {
        "up" => Vec3::new(u * hx, hy, v * hz),
        "down" => Vec3::new(u * hx, -hy, -v * hz),
        "north" => Vec3::new(-u * hx, v * hy, -hz),
        "south" => Vec3::new(u * hx, v * hy, hz),
        "east" => Vec3::new(hx, v * hy, -u * hz),
        "west" => Vec3::new(-hx, v * hy, u * hz),
        _ => Vec3::default(),
    }
}

pub(crate) fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;

/// A bone's or cube's rotation in model space: X, then Y, then Z, which in
/// standard right-handed terms is Rz(-z)·Ry(y)·Rx(-x). fauxgl's rotation
/// turns the opposite way to the standard one, so the signs here are +x,
/// -y, +z. See docs/geometry-format.md#rotation.
fn rotation_matrix(r: &[f64]) -> Mat4 {
    Mat4::identity()
        .rotate(Vec3::new(1.0, 0.0, 0.0), at(r, 0) * DEG_TO_RAD)
        .rotate(Vec3::new(0.0, 1.0, 0.0), -at(r, 1) * DEG_TO_RAD)
        .rotate(Vec3::new(0.0, 0.0, 1.0), at(r, 2) * DEG_TO_RAD)
}

/// Appends one cube's faces (two triangles each), placed by the bone's world
/// transform, with UVs in 0..1. Model space is X-mirrored against the world:
/// X is negated last and each face's U flipped to match.
/// See docs/rendering-pipeline.md#model-space-is-x-mirrored.
fn add_cube(
    triangles: &mut Vec<Triangle>,
    c: &Cube,
    b: &Bone,
    world: &Mat4,
    tex_w: f64,
    tex_h: f64,
) {
    let Some((size, origin)) = cube_dims(c) else {
        return;
    };
    let Some(rects) = cube_uv_rects(c) else {
        return;
    };
    let inflate = c.inflate.unwrap_or(b.inflate);
    let sx = size[0] + 2.0 * inflate;
    let sy = size[1] + 2.0 * inflate;
    let sz = size[2] + 2.0 * inflate;
    let (hx, hy, hz) = (sx / 2.0, sy / 2.0, sz / 2.0);

    let center = Vec3::new(
        origin[0] + size[0] / 2.0,
        origin[1] + size[1] / 2.0,
        origin[2] + size[2] / 2.0,
    );
    let pivot_bone = Vec3::new(at(&b.pivot, 0), at(&b.pivot, 1), at(&b.pivot, 2));
    let rotated = c.rotation.len() >= 3
        && (c.rotation[0] != 0.0 || c.rotation[1] != 0.0 || c.rotation[2] != 0.0);
    let cube_rot = rotated.then(|| rotation_matrix(&c.rotation));
    let cube_pivot = if c.pivot.len() >= 3 {
        Vec3::new(c.pivot[0], c.pivot[1], c.pivot[2])
    } else {
        center
    };
    let place = |local: Vec3| {
        let mut p = center.add(local);
        if let Some(m) = &cube_rot {
            p = m.mul_position(p.sub(cube_pivot)).add(cube_pivot);
        }
        let mut p = world.mul_position(p.sub(pivot_bone));
        p.x = -p.x;
        p
    };
    let mirror = c.mirror || b.mirror;
    const CORNERS: [[f64; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

    for face in FACES {
        // Mirroring flips the texture left to right: east and west trade.
        let src = match face {
            "east" if mirror => "west",
            "west" if mirror => "east",
            f => f,
        };
        let Some(rect) = rect_for(&rects, src) else {
            continue;
        };
        let (mut u0, v0, mut u1, v1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
        // Negating X flips every face; flipping U puts it back. A mirrored
        // cube's own flip cancels that.
        if !mirror {
            std::mem::swap(&mut u0, &mut u1);
        }
        let uv = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        let mut verts = [Vertex::default(); 4];
        for i in 0..4 {
            let local = face_corner(face, CORNERS[i][0], CORNERS[i][1], hx, hy, hz);
            // V is pre-flipped to cancel the sampler's v = 1 - v.
            // See docs/rendering-pipeline.md#the-texture-coordinate-flip.
            verts[i] = Vertex::new(place(local), uv[i][0] / tex_w, 1.0 - uv[i][1] / tex_h);
        }
        triangles.push(Triangle([verts[0], verts[1], verts[2]]));
        triangles.push(Triangle([verts[0], verts[2], verts[3]]));
    }
}

/// A bone's local transform: its rotation about its own origin, then a
/// translation by its pivot less its parent's. A pose adds its rotation and
/// position, and scales when it says to.
fn bone_local_matrix(b: &Bone, parent_pivot: &[f64], p: &BonePose) -> Mat4 {
    let own = &b.pivot;
    let offset = Vec3::new(
        at(own, 0) - at(parent_pivot, 0) + p.position[0],
        at(own, 1) - at(parent_pivot, 1) + p.position[1],
        at(own, 2) - at(parent_pivot, 2) + p.position[2],
    );
    let rot = [
        at(&b.rotation, 0) + p.rotation[0],
        at(&b.rotation, 1) + p.rotation[1],
        at(&b.rotation, 2) + p.rotation[2],
    ];
    let mut m = Mat4::identity();
    if p.scaled {
        m = m.scale(Vec3::new(p.scale[0], p.scale[1], p.scale[2]));
    }
    if rot[0] != 0.0 || rot[1] != 0.0 || rot[2] != 0.0 {
        m = rotation_matrix(&rot).mul(&m);
    }
    m.translate(offset)
}

/// Every bone's absolute transform, composed up the parent chain. A parent
/// cycle resolves to identity rather than recursing forever.
fn bone_world_matrices(geo: &Geometry, pose: &Pose) -> HashMap<String, Mat4> {
    let by_name: HashMap<&str, &Bone> = geo.bones.iter().map(|b| (b.name.as_str(), b)).collect();
    let mut result: HashMap<String, Mat4> = HashMap::new();

    fn resolve(
        name: &str,
        by_name: &HashMap<&str, &Bone>,
        pose: &Pose,
        seen: &mut Vec<String>,
        result: &mut HashMap<String, Mat4>,
    ) -> Mat4 {
        if let Some(m) = result.get(name) {
            return *m;
        }
        let Some(b) = by_name.get(name) else {
            return Mat4::identity();
        };
        if seen.iter().any(|s| s == name) {
            return Mat4::identity();
        }
        seen.push(name.to_string());
        let mut parent_pivot: &[f64] = &[];
        let mut parent_world = Mat4::identity();
        if !b.parent.is_empty()
            && let Some(pb) = by_name.get(b.parent.as_str())
        {
            parent_pivot = &pb.pivot;
            parent_world = resolve(&b.parent, by_name, pose, seen, result);
        }
        let local = bone_local_matrix(b, parent_pivot, &pose.of(&b.name));
        let world = parent_world.mul(&local);
        result.insert(name.to_string(), world);
        world
    }

    for b in &geo.bones {
        resolve(&b.name, &by_name, pose, &mut Vec::new(), &mut result);
    }
    result
}

/// Triangles for every cube and poly mesh whose bone passes `include` (None includes
/// everything), posed by `pose`.
pub(crate) fn build_triangles(
    geo: &Geometry,
    include: Option<&dyn Fn(&str) -> bool>,
    pose: &Pose,
) -> Vec<Triangle> {
    let worlds = bone_world_matrices(geo, pose);
    let mut triangles = Vec::new();
    for b in &geo.bones {
        if let Some(inc) = include
            && !inc(&b.name)
        {
            continue;
        }
        let world = worlds.get(&b.name).copied().unwrap_or(Mat4::identity());
        for c in &b.cubes {
            add_cube(
                &mut triangles,
                c,
                b,
                &world,
                geo.texture_width,
                geo.texture_height,
            );
        }
        if let Some(m) = b.mesh() {
            add_poly_mesh(
                &mut triangles,
                &m,
                b,
                &world,
                geo.texture_width,
                geo.texture_height,
            );
        }
    }
    triangles
}
