//! Poly meshes: the free-form shape Bedrock sends for persona (character
//! creator) skins instead of cubes.
//! See docs/geometry-format.md#poly-meshes.

use serde_json::Value;

use crate::geometry::{Bone, Geometry};
use crate::jsonread::Reader;
use crate::mesh::at;
use crate::raster::{Mat4, Triangle, Vec3, Vertex};

/// A bone's poly mesh. Positions are in model space, like a cube's origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyMesh {
    /// UVs are 0..1 across the texture, V counting up; otherwise texture
    /// pixels against the entry's declared texture size.
    pub normalized_uvs: bool,
    pub positions: Vec<Vec<f64>>,
    pub normals: Vec<Vec<f64>>,
    pub uvs: Vec<Vec<f64>>,
    /// A list of polygons, each a list of `[position, normal, uv]` index
    /// triples, or the string `"tri_list"` / `"quad_list"` for vertices taken
    /// in order. [`PolyMesh::polygons`] resolves them.
    pub polys: Option<Value>,
}

/// One corner of a polygon, its indices looked up: a position in model
/// space, a normal, and a texture coordinate - 0..1 with V counting up when
/// the mesh's `normalized_uvs` is set, else texture pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PolyVertex {
    pub position: [f64; 3],
    /// Zero when the corner names no usable normal.
    pub normal: [f64; 3],
    pub uv: [f64; 2],
}

impl Bone {
    /// The bone's poly mesh; None when it has none or it does not read.
    pub fn mesh(&self) -> Option<PolyMesh> {
        let v = self.poly_mesh.as_ref()?;
        let mut r = Reader::default();
        let mut m = PolyMesh::default();
        let rows = |r: &mut Reader, v: &Value, out: &mut Vec<Vec<f64>>| {
            r.list(v, out, |r, v, row| r.f64s(v, row))
        };
        r.object(v, |r, k, v| match k {
            "normalized_uvs" => r.bool(v, &mut m.normalized_uvs),
            "positions" => rows(r, v, &mut m.positions),
            "normals" => rows(r, v, &mut m.normals),
            "uvs" => rows(r, v, &mut m.uvs),
            "polys" => m.polys = Some(v.clone()),
            _ => {}
        });
        (!r.type_error).then_some(m)
    }
}

impl PolyMesh {
    /// Every polygon resolved to its corners, `"tri_list"` and `"quad_list"`
    /// included. A polygon with fewer than three corners, or one whose
    /// position or UV index points outside the mesh's lists, is skipped - the
    /// renderer and the detector skip it too. Normals are not drawn, so a
    /// missing one leaves the corner's normal zero rather than dropping the
    /// polygon.
    ///
    /// ```
    /// let geos = bedrock_skin::parse_geometry(br#"{"minecraft:geometry":[{"description":{"identifier":"geometry.persona_x"},
    ///     "bones":[{"name":"body","poly_mesh":{"normalized_uvs":true,
    ///         "positions":[[-4,12,-2],[4,12,-2],[4,24,-2],[-4,24,-2]],
    ///         "normals":[[0,0,-1]],
    ///         "uvs":[[0.25,0.5],[0.375,0.5],[0.375,0.6875],[0.25,0.6875]],
    ///         "polys":[[[0,0,0],[1,0,1],[2,0,2],[3,0,3]]]}}]}]}"#)?;
    /// let mesh = geos[0].bones[0].mesh().unwrap();
    /// let polys = mesh.polygons();
    /// assert_eq!(polys[0].len(), 4);
    /// assert_eq!(polys[0][0].position, [-4.0, 12.0, -2.0]);
    /// assert_eq!(polys[0][0].normal, [0.0, 0.0, -1.0]);
    /// # Ok::<(), bedrock_skin::Error>(())
    /// ```
    pub fn polygons(&self) -> Vec<Vec<PolyVertex>> {
        let Some(polys) = &self.polys else {
            return Vec::new();
        };
        let mut idx: Vec<Vec<Vec<f64>>> = Vec::new();
        match polys {
            // A JSON string or null reads as Go's string; null is "".
            Value::String(_) | Value::Null => {
                let per = match polys.as_str() {
                    Some("tri_list") => 3,
                    Some("quad_list") => 4,
                    _ => return Vec::new(),
                };
                let mut i = 0;
                while i + per <= self.positions.len() {
                    idx.push((0..per).map(|j| vec![(i + j) as f64; 3]).collect());
                    i += per;
                }
            }
            _ => {
                let mut r = Reader::default();
                r.list(polys, &mut idx, |r, v, poly| {
                    r.list(v, poly, |r, v, c| r.f64s(v, c))
                });
                if r.type_error {
                    return Vec::new();
                }
            }
        }

        let mut out = Vec::new();
        for poly in &idx {
            if poly.len() < 3 {
                continue;
            }
            let mut verts = Vec::with_capacity(poly.len());
            for c in poly {
                let (Some(p), Some(t)) =
                    (index(c, 0, &self.positions, 3), index(c, 2, &self.uvs, 2))
                else {
                    break;
                };
                let normal = index(c, 1, &self.normals, 3).map_or([0.0; 3], |n| [n[0], n[1], n[2]]);
                verts.push(PolyVertex {
                    position: [p[0], p[1], p[2]],
                    normal,
                    uv: [t[0], t[1]],
                });
            }
            if verts.len() == poly.len() {
                out.push(verts);
            }
        }
        out
    }
}

/// `corner[slot]` looked up in `list`, wanting at least `n` components.
fn index<'a>(corner: &[f64], slot: usize, list: &'a [Vec<f64>], n: usize) -> Option<&'a [f64]> {
    let f = *corner.get(slot)?;
    if f < 0.0 || f >= list.len() as f64 || f != f.trunc() {
        return None;
    }
    let v = &list[f as usize];
    (v.len() >= n).then_some(v.as_slice())
}

impl Geometry {
    /// Whether any bone draws something: a cube or a poly mesh.
    pub fn has_mesh(&self) -> bool {
        self.bones.iter().any(draws_something)
    }
}

/// Whether a bone renders any pixels.
pub(crate) fn draws_something(b: &Bone) -> bool {
    !b.cubes.is_empty() || b.mesh().is_some_and(|m| !m.polygons().is_empty())
}

/// Appends a poly mesh's polygons, fanned into triangles, placed as a cube is
/// placed: through the bone's world transform from its pivot, then X
/// mirrored. The UVs ride on the vertices, so the mirror needs no U flip.
/// See docs/rendering-pipeline.md#poly-meshes.
pub(crate) fn add_poly_mesh(
    triangles: &mut Vec<Triangle>,
    m: &PolyMesh,
    b: &Bone,
    world: &Mat4,
    tex_w: f64,
    tex_h: f64,
) {
    let pivot = Vec3::new(at(&b.pivot, 0), at(&b.pivot, 1), at(&b.pivot, 2));
    for poly in m.polygons() {
        let verts: Vec<Vertex> = poly
            .iter()
            .map(|c| {
                let mut p = world.mul_position(
                    Vec3::new(c.position[0], c.position[1], c.position[2]).sub(pivot),
                );
                p.x = -p.x;
                // Normalized UVs count V up, as the sampler does; pixel UVs
                // count down from the top, as a cube's do.
                let (mut u, mut v) = (c.uv[0], c.uv[1]);
                if !m.normalized_uvs {
                    (u, v) = (u / tex_w, 1.0 - v / tex_h);
                }
                Vertex::new(p, u, v)
            })
            .collect();
        for i in 1..verts.len() - 1 {
            triangles.push(Triangle([verts[0], verts[i], verts[i + 1]]));
        }
    }
}
