//! Bedrock's geometry.json: parsing both of its formats into one model.
//! See docs/geometry-format.md.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Error;
use crate::jsonread::Reader;

/// One bone of a model: a named node in the tree, holding cubes. The fields
/// mirror geometry.json; see docs/geometry-format.md.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Bone {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parent: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pivot: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rotation: Vec<f64>,
    #[serde(skip_serializing_if = "is_zero")]
    pub inflate: f64,
    #[serde(skip_serializing_if = "is_false")]
    pub mirror: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cubes: Vec<Cube>,

    /// The rest of the schema, kept so a whole file can be read. The
    /// renderer draws cubes only: poly meshes and texture meshes are parsed
    /// but not drawn. See docs/geometry-format.md#everything-else-in-a-bone.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bind_pose_rotation: Vec<f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub locators: BTreeMap<String, Locator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poly_mesh: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_meshes: Option<Value>,
}

/// A named point on a bone - where an item is held, a lead ties, particles
/// start. A file writes one as just an offset, `[x, y, z]`, or as an object
/// with an offset and a rotation; both read into this.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Locator {
    pub offset: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rotation: Vec<f64>,
    #[serde(skip_serializing_if = "is_false")]
    pub ignore_inherited_scale: bool,
}

/// One box of a bone.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Cube {
    pub origin: Vec<f64>,
    pub size: Vec<f64>,
    /// The texture mapping as written: `[u, v]` (box UV) or an object of
    /// faces (per-face UV). See [`Cube::box_uv`] and [`Cube::face_uvs`].
    pub uv: Option<Value>,
    /// Overrides the bone's inflate when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inflate: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    pub mirror: bool,
    /// Turns the cube about `pivot`, in degrees; `pivot` is in model space
    /// and defaults to the cube's centre. See docs/geometry-format.md#rotation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rotation: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pivot: Vec<f64>,
}

/// One face's texture area in a cube's per-face uv form.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FaceUv {
    pub uv: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uv_size: Vec<f64>,
    #[serde(skip_serializing_if = "is_zero")]
    pub uv_rotation: f64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub material_instance: String,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl Cube {
    /// The cube's texture origin when its uv is the box form, `[u, v]`,
    /// which lays all six faces out from that corner.
    pub fn box_uv(&self) -> Option<(f64, f64)> {
        let arr = self.uv.as_ref().and_then(read_f64s)?;
        (arr.len() >= 2).then(|| (arr[0], arr[1]))
    }

    /// Each face's texture area, by face name (north, east, south, west, up,
    /// down), when the cube's uv is the per-face form; None for the box form.
    /// A face it leaves out is not drawn.
    pub fn face_uvs(&self) -> Option<BTreeMap<String, FaceUv>> {
        let v = self.uv.as_ref()?;
        let mut r = Reader::default();
        let mut faces = Vec::new();
        r.map(v, &mut faces, read_face_uv);
        if r.type_error || !v.is_object() {
            return None;
        }
        Some(faces.into_iter().collect())
    }
}

/// A list of numbers, as Go reads one into []float64: None on a type error.
pub(crate) fn read_f64s(v: &Value) -> Option<Vec<f64>> {
    let mut r = Reader::default();
    let mut out = Vec::new();
    r.f64s(v, &mut out);
    (!r.type_error).then_some(out)
}

fn read_face_uv(r: &mut Reader, v: &Value, f: &mut FaceUv) {
    r.object(v, |r, k, v| match k {
        "uv" => r.f64s(v, &mut f.uv),
        "uv_size" => r.f64s(v, &mut f.uv_size),
        "uv_rotation" => r.f64(v, &mut f.uv_rotation),
        "material_instance" => r.string(v, &mut f.material_instance),
        _ => {}
    });
}

pub(crate) fn read_bone(r: &mut Reader, v: &Value, b: &mut Bone) {
    r.object(v, |r, k, v| match k {
        "name" => r.string(v, &mut b.name),
        "parent" => r.string(v, &mut b.parent),
        "pivot" => r.f64s(v, &mut b.pivot),
        "rotation" => r.f64s(v, &mut b.rotation),
        "inflate" => r.f64(v, &mut b.inflate),
        "mirror" => r.bool(v, &mut b.mirror),
        "cubes" => r.list(v, &mut b.cubes, read_cube),
        "bind_pose_rotation" => r.f64s(v, &mut b.bind_pose_rotation),
        "locators" => {
            let mut list: Vec<(String, Locator)> = b.locators.clone().into_iter().collect();
            r.map(v, &mut list, |_, v, l| *l = read_locator(v));
            b.locators = list.into_iter().collect();
        }
        "poly_mesh" => b.poly_mesh = Some(v.clone()),
        "texture_meshes" => b.texture_meshes = Some(v.clone()),
        _ => {}
    });
}

pub(crate) fn read_cube(r: &mut Reader, v: &Value, c: &mut Cube) {
    r.object(v, |r, k, v| match k {
        "origin" => r.f64s(v, &mut c.origin),
        "size" => r.f64s(v, &mut c.size),
        "uv" => c.uv = Some(v.clone()),
        "inflate" => r.opt_f64(v, &mut c.inflate),
        "mirror" => r.bool(v, &mut c.mirror),
        "rotation" => r.f64s(v, &mut c.rotation),
        "pivot" => r.f64s(v, &mut c.pivot),
        _ => {}
    });
}

/// A locator never fails a model: the array form, else whatever of the
/// object form reads, else nothing.
pub(crate) fn read_locator(v: &Value) -> Locator {
    if let Some(offset) = read_f64s(v) {
        return Locator {
            offset,
            ..Locator::default()
        };
    }
    let mut l = Locator::default();
    if v.is_object() {
        let mut r = Reader::default();
        r.object(v, |r, k, v| match k {
            "offset" => r.f64s(v, &mut l.offset),
            "rotation" => r.f64s(v, &mut l.rotation),
            "ignore_inherited_scale" => r.bool(v, &mut l.ignore_inherited_scale),
            _ => {}
        });
    }
    l
}

macro_rules! deserialize_via {
    ($t:ty, $read:expr) => {
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                let mut r = Reader::default();
                let mut out = <$t>::default();
                $read(&mut r, &v, &mut out);
                if r.type_error || !(v.is_object() || v.is_null()) {
                    return Err(serde::de::Error::custom(concat!(
                        "wrong type of value in a ",
                        stringify!($t)
                    )));
                }
                Ok(out)
            }
        }
    };
}

deserialize_via!(Bone, read_bone);
deserialize_via!(Cube, read_cube);
deserialize_via!(FaceUv, read_face_uv);

impl<'de> Deserialize<'de> for Locator {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(read_locator(&Value::deserialize(d)?))
    }
}

/// One normalized model - a body, a cape - whichever of Bedrock's two wire
/// formats it came from. See [`parse_geometry`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub identifier: String,
    pub texture_width: f64,
    pub texture_height: f64,
    pub bones: Vec<Bone>,

    /// The visible bounds: the box, in blocks, the game uses to decide the
    /// model is on screen. Zero when the file leaves them out.
    pub visible_bounds_width: f64,
    pub visible_bounds_height: f64,
    pub visible_bounds_offset: Vec<f64>,
}

impl Geometry {
    /// The bone with the given name.
    pub fn bone_by_name(&self, name: &str) -> Option<&Bone> {
        self.bones.iter().find(|b| b.name == name)
    }

    /// The bones whose parent is the named bone, in file order.
    pub fn children(&self, name: &str) -> Vec<&Bone> {
        self.bones.iter().filter(|b| b.parent == name).collect()
    }

    /// Finds a locator by name on any bone, with the bone it is on.
    pub fn locator(&self, name: &str) -> Option<(&Locator, &Bone)> {
        self.bones
            .iter()
            .find_map(|b| b.locators.get(name).map(|l| (l, b)))
    }

    /// The number of cubes across every bone. Zero means the entry carries
    /// no mesh at all, which is exactly what a persona skin looks like: real
    /// bones, no cubes.
    pub fn total_cubes(&self) -> usize {
        self.bones.iter().map(|b| b.cubes.len()).sum()
    }
}

/// Whether `raw` carries no geometry at all: nothing, or the literal JSON
/// `null` a Bedrock client sends for a skin whose model is built into the
/// client. Both mean "no mesh supplied", not "broken upload" - use it to
/// tell those apart before calling [`parse_geometry`].
///
/// See docs/skin-data.md#most-skins-send-no-geometry-at-all.
pub fn is_empty(raw: &[u8]) -> bool {
    let trimmed = raw.trim_ascii();
    trimmed.is_empty() || trimmed == b"null"
}

/// Total bones and cubes across every entry, roughly what mesh-building
/// costs. The library enforces no limit itself - what counts as too large is
/// policy. See docs/recipes.md#handling-untrusted-uploads.
pub fn complexity(geos: &[Geometry]) -> (usize, usize) {
    geos.iter()
        .fold((0, 0), |(b, c), g| (b + g.bones.len(), c + g.total_cubes()))
}

#[derive(Default)]
struct Description {
    identifier: String,
    texture_width: f64,
    texture_height: f64,
    visible_bounds_width: f64,
    visible_bounds_height: f64,
    visible_bounds_offset: Vec<f64>,
}

fn read_description(r: &mut Reader, v: &Value, d: &mut Description, legacy: bool) {
    r.object(v, |r, k, v| match k {
        "identifier" if !legacy => r.string(v, &mut d.identifier),
        "texture_width" if !legacy => r.f64(v, &mut d.texture_width),
        "texture_height" if !legacy => r.f64(v, &mut d.texture_height),
        "texturewidth" if legacy => r.f64(v, &mut d.texture_width),
        "textureheight" if legacy => r.f64(v, &mut d.texture_height),
        "visible_bounds_width" => r.f64(v, &mut d.visible_bounds_width),
        "visible_bounds_height" => r.f64(v, &mut d.visible_bounds_height),
        "visible_bounds_offset" => r.f64s(v, &mut d.visible_bounds_offset),
        _ => {}
    });
}

fn geometry_of(d: Description, identifier: String, bones: Vec<Bone>) -> Geometry {
    Geometry {
        identifier,
        texture_width: texture_size(d.texture_width),
        texture_height: texture_size(d.texture_height),
        bones,
        visible_bounds_width: d.visible_bounds_width,
        visible_bounds_height: d.visible_bounds_height,
        visible_bounds_offset: d.visible_bounds_offset,
    }
}

/// Parses `raw` into normalized entries, detecting whichever of Bedrock's
/// two formats it is - bone and cube fields are identical between them, only
/// the wrapper differs.
///
/// Valid JSON carrying no geometry, including the literal `null` a client
/// sends for a built-in model, returns no entries and no error. An error
/// means malformed input.
///
/// Entry order is stable for the same input: modern keeps document order,
/// legacy sorts by identifier.
pub fn parse_geometry(raw: &[u8]) -> Result<Vec<Geometry>, Error> {
    let doc: Value = serde_json::from_slice(raw).map_err(Error::Json)?;
    if let Some(geos) = parse_modern(&doc) {
        return Ok(geos);
    }

    let top = match &doc {
        Value::Object(map) => map,
        Value::Null => return Ok(Vec::new()),
        _ => return Err(Error::Geometry("the top level is not an object".into())),
    };
    let mut entries: Vec<(&String, &Value)> = Vec::new();
    for (key, val) in top {
        if key != "format_version" {
            entries.push((key, val));
        }
    }
    let mut out = Vec::new();
    for (key, val) in entries {
        let mut r = Reader::default();
        let mut desc = Description::default();
        let mut bones = Vec::new();
        read_description(&mut r, val, &mut desc, true);
        r.object(val, |r, k, v| {
            if k == "bones" {
                r.list(v, &mut bones, read_bone);
            }
        });
        if r.type_error || bones.is_empty() {
            continue;
        }
        out.push(geometry_of(desc, key.clone(), bones));
    }
    // See docs/design-decisions.md#why-legacy-entries-are-sorted.
    out.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(out)
}

/// The modern format, `minecraft:geometry`, or None when the document is not
/// that, or Go's decoder would have reported a type error reading it.
fn parse_modern(doc: &Value) -> Option<Vec<Geometry>> {
    let mut r = Reader::default();
    let mut models: Vec<(Description, Vec<Bone>)> = Vec::new();
    r.object(doc, |r, k, v| {
        if k == "minecraft:geometry" {
            r.list(v, &mut models, |r, m, (desc, bones)| {
                r.object(m, |r, k, v| match k {
                    "description" => read_description(r, v, desc, false),
                    "bones" => r.list(v, bones, read_bone),
                    _ => {}
                });
            });
        }
    });
    if r.type_error || models.is_empty() {
        return None;
    }
    Some(
        models
            .into_iter()
            .map(|(d, bones)| {
                let id = d.identifier.clone();
                geometry_of(d, id, bones)
            })
            .collect(),
    )
}

/// A declared texture dimension, or Minecraft's default of 64 when the
/// geometry leaves it out - real captures do. Left at zero, every UV divides
/// by zero and the model renders blank.
fn texture_size(v: f64) -> f64 {
    if v > 0.0 { v } else { 64.0 }
}

/// The entry matching `identifier`, falling back to the one with the most
/// cubes when `identifier` is empty or matches nothing. None only for an
/// empty list.
///
/// The fallback is deliberately not the first: bundles commonly list the
/// sparse cape entry first. See docs/design-decisions.md#why-select-by-cube-count.
pub fn select_geometry<'a>(geos: &'a [Geometry], identifier: &str) -> Option<&'a Geometry> {
    if !identifier.is_empty()
        && let Some(g) = geos.iter().find(|g| g.identifier == identifier)
    {
        return Some(g);
    }
    let mut best = geos.first()?;
    for g in &geos[1..] {
        if g.total_cubes() > best.total_cubes() {
            best = g;
        }
    }
    Some(best)
}

/// The entry holding a bone named `cape` that has a cube. Capes live in
/// their own entry, never merged into the body.
pub fn find_cape(geos: &[Geometry]) -> Option<&Geometry> {
    geos.iter()
        .find(|g| g.bone_by_name("cape").is_some_and(|b| !b.cubes.is_empty()))
}

/// A skin's decoded `SkinResourcePatch`: which geometry identifier each
/// render slot uses. See docs/skin-data.md#the-resource-patch-is-the-authoritative-model-selector.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourcePatch {
    /// The body geometry, e.g. `geometry.humanoid.customSlim`. Pass it as
    /// [`RenderOptions::identifier`](crate::RenderOptions::identifier).
    pub default: String,
    /// The cape geometry when the patch names one; most do not.
    pub cape: String,
}

/// Decodes a skin's resource patch.
///
/// The patch is the authoritative wide-vs-slim selector: the login packet's
/// `ArmSize` field disagrees with it on real captures. Empty input, or the
/// literal `null`, gives an empty patch and no error.
pub fn parse_resource_patch(raw: &[u8]) -> Result<ResourcePatch, Error> {
    if is_empty(raw) {
        return Ok(ResourcePatch::default());
    }
    let doc: Value =
        serde_json::from_slice(raw).map_err(|e| Error::ResourcePatch(e.to_string()))?;
    let mut r = Reader::default();
    let mut patch = ResourcePatch::default();
    r.object(&doc, |r, k, v| {
        if k == "geometry" {
            r.object(v, |r, k, v| match k {
                "default" => r.string(v, &mut patch.default),
                "cape" => r.string(v, &mut patch.cape),
                _ => {}
            });
        }
    });
    if r.type_error {
        return Err(Error::ResourcePatch("a field has the wrong type".into()));
    }
    Ok(patch)
}

static DEFAULT_GEOMETRY_JSON: &str = include_str!("default_geometry.json");

/// The vanilla humanoid geometry: the wide (`geometry.humanoid.custom`) and
/// slim (`geometry.humanoid.customSlim`) bodies, and `geometry.cape`.
///
/// This is the right model for most real skins, not merely a fallback: a
/// Bedrock client sends no mesh at all for a skin using a built-in model.
/// See docs/skin-data.md#most-skins-send-no-geometry-at-all.
pub fn default_geometry() -> &'static [Geometry] {
    static GEOS: OnceLock<Vec<Geometry>> = OnceLock::new();
    GEOS.get_or_init(|| {
        let geos = parse_geometry(DEFAULT_GEOMETRY_JSON.as_bytes())
            .expect("bundled default_geometry.json is valid");
        assert!(
            !geos.is_empty(),
            "bundled default_geometry.json has no entries"
        );
        geos
    })
}
