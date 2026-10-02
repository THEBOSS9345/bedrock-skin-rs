//! Detecting invisible and partly invisible skins: whether each body part
//! has enough opaque texture where its cubes are mapped.
//! See docs/api-reference.md#invisibility-detection.

use std::collections::{BTreeMap, HashMap, HashSet};

use image::RgbaImage;

use crate::geometry::{Bone, parse_geometry};
use crate::mesh::{box_uv_rects, cube_dims, cube_uv_rects};

/// The minimum alpha (0..1) for a pixel to count as visible: anything but
/// fully transparent.
pub const DEFAULT_MIN_VISIBLE_ALPHA: f64 = 0.5 / 255.0;

/// The share of a part's sampled pixels that must be opaque for it to count
/// as visible: a part more than half transparent is effectively invisible.
pub const DEFAULT_MIN_VISIBLE_FRACTION: f64 = 0.5;

/// The size a bone must reach on its largest axis not to be judged too small
/// to see. A flat plane is visible; a bone small on every axis is not.
pub const DEFAULT_MIN_GEOMETRY_SIZE: f64 = 0.5;

/// How many of the six standard body parts must be visible for a skin not
/// to be suspicious.
pub const DEFAULT_MIN_VISIBLE_PARTS: usize = 4;

/// The tunable part of one detection run. A zero field takes its default.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Thresholds {
    pub min_visible_fraction: f64,
    pub min_geometry_size: f64,
    pub min_visible_parts: usize,
}

impl Thresholds {
    pub fn resolved(mut self) -> Thresholds {
        if self.min_visible_fraction <= 0.0 {
            self.min_visible_fraction = DEFAULT_MIN_VISIBLE_FRACTION;
        }
        if self.min_geometry_size <= 0.0 {
            self.min_geometry_size = DEFAULT_MIN_GEOMETRY_SIZE;
        }
        if self.min_visible_parts == 0 {
            self.min_visible_parts = DEFAULT_MIN_VISIBLE_PARTS;
        }
        self
    }
}

/// The visibility of one body part. With geometry, it reflects the part's
/// real cube UV regions; without, the standard vanilla layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinPartResult {
    pub name: String,
    pub visible: bool,
    pub fraction: f64,
    pub pixels: usize,
    pub transparent: usize,
    /// The part came from geometry, not the fallback layout.
    pub from_geo: bool,
    /// The geometry defines the part below the minimum size.
    pub tiny: bool,
}

/// The result of [`validate_skin_visibility`] and
/// [`validate_skin_invisibility`]. `suspicious` flags a half-invisible skin:
/// some parts visible, but fewer of the standard six than the minimum.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinVisibilityResult {
    pub is_invisible: bool,
    pub pass: bool,
    pub suspicious: bool,
    pub parts: Vec<SkinPartResult>,
    pub visible_parts: usize,
    pub invisible_parts: usize,
}

/// A bone smaller than the minimum.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometryViolation {
    pub bone: String,
    pub size: f64,
    pub minimum: f64,
}

/// The result of [`validate_geometry_size`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometrySizeResult {
    pub pass: bool,
    pub violations: Vec<GeometryViolation>,
}

/// The six visible body parts of a standard humanoid skin, against a
/// 64-wide texture: (u, v, width, height, depth).
const STANDARD_BODY_PARTS: [(&str, [f64; 5]); 6] = [
    ("head", [8.0, 8.0, 8.0, 8.0, 8.0]),
    ("body", [20.0, 20.0, 8.0, 12.0, 4.0]),
    ("rightArm", [44.0, 20.0, 4.0, 12.0, 4.0]),
    ("leftArm", [36.0, 52.0, 4.0, 12.0, 4.0]),
    ("rightLeg", [4.0, 20.0, 4.0, 12.0, 4.0]),
    ("leftLeg", [20.0, 52.0, 4.0, 12.0, 4.0]),
];

/// The pre-1.8 64x32 layout. Its left arm and leg point off the texture, so
/// they count as invisible.
const LEGACY32_BODY_PARTS: [(&str, [f64; 5]); 4] = [
    ("head", [0.0, 0.0, 8.0, 8.0, 8.0]),
    ("body", [16.0, 16.0, 8.0, 12.0, 4.0]),
    ("rightArm", [40.0, 16.0, 4.0, 12.0, 4.0]),
    ("rightLeg", [0.0, 16.0, 4.0, 12.0, 4.0]),
];

pub(crate) const STANDARD_PART_NAMES: [&str; 6] =
    ["head", "body", "rightArm", "leftArm", "rightLeg", "leftLeg"];

/// Clothing overlay bones fold into the standard part they cover.
fn overlay_parent(name: &str) -> Option<&'static str> {
    Some(match name {
        "hat" => "head",
        "jacket" => "body",
        "leftSleeve" => "leftArm",
        "rightSleeve" => "rightArm",
        "leftPants" => "leftLeg",
        "rightPants" => "rightLeg",
        _ => return None,
    })
}

/// Accessories are reported but never count: an opaque cape must not make an
/// invisible body pass.
fn is_accessory(name: &str) -> bool {
    name == "cape"
}

/// Checks whether a skin has visible body parts. With geometry (raw
/// geometry.json), every bone with cubes is checked against the texture
/// where its UVs point, so a player cannot hide behind geometry mapped to
/// transparent pixels; without, the standard layout is used.
///
/// Persona geometry - bones, none with cubes - is trusted visible. Geometry
/// that fails to parse is not that: it is checked like no geometry.
pub fn validate_skin_visibility(
    texture: &RgbaImage,
    geometry: &[u8],
    min_visible_fraction: f64,
) -> SkinVisibilityResult {
    let scan = scan_parts(texture, get_geometry(geometry).as_ref());
    match scan {
        Scan::Unusable => unusable_result(),
        Scan::Persona => persona_result(),
        Scan::Parts { parts, strict } => classify(
            parts,
            Thresholds {
                min_visible_fraction,
                ..Thresholds::default()
            }
            .resolved(),
            strict,
        ),
    }
}

enum Scan {
    /// No texture to analyse at all.
    Unusable,
    /// Real bones, no cubes: trusted visible.
    Persona,
    /// Measured parts; `strict` when they came from box-UV geometry.
    Parts {
        parts: Vec<SkinPartResult>,
        strict: bool,
    },
}

/// Fails closed: a missing texture reads as invisible as well as failing.
fn unusable_result() -> SkinVisibilityResult {
    SkinVisibilityResult {
        pass: false,
        is_invisible: true,
        ..SkinVisibilityResult::default()
    }
}

fn persona_result() -> SkinVisibilityResult {
    let parts = STANDARD_PART_NAMES
        .iter()
        .map(|n| SkinPartResult {
            name: n.to_string(),
            visible: true,
            fraction: 1.0,
            from_geo: true,
            ..Default::default()
        })
        .collect();
    SkinVisibilityResult {
        pass: true,
        is_invisible: false,
        parts,
        ..SkinVisibilityResult::default()
    }
}

/// Bones by name; a repeated name's last bone wins, as in Go's map.
type Bones = HashMap<String, Bone>;

fn scan_parts(texture: &RgbaImage, bones: Option<&Bones>) -> Scan {
    let (tw, th) = (texture.width() as f64, texture.height() as f64);
    if tw <= 0.0 || th <= 0.0 {
        return Scan::Unusable;
    }
    let bones_with_cubes = bones.is_some_and(|b| b.values().any(|b| !b.cubes.is_empty()));
    if bones_with_cubes {
        // Box-UV geometry gives authoritative regions, so the verdict can be
        // strict.
        return Scan::Parts {
            parts: check_from_geometry(bones.unwrap(), texture, tw, th),
            strict: true,
        };
    }
    if bones.is_some_and(|b| !b.is_empty()) {
        // This tests parsed bones, not "some bytes were passed": garbage
        // geometry must not switch the detector off.
        // See docs/design-decisions.md#why-persona-detection-tests-parsed-bones.
        return Scan::Persona;
    }
    Scan::Parts {
        parts: check_from_standard_uv(texture, tw, th),
        strict: false,
    }
}

/// The scale from a 64-wide reference layout to the texture. A 64x32 atlas
/// uses its own absolute coordinates.
fn uv_scale(tw: f64, th: f64) -> (f64, f64) {
    if th == 32.0 {
        (1.0, 1.0)
    } else {
        (tw / 64.0, th / 64.0)
    }
}

fn check_from_geometry(
    bones: &Bones,
    texture: &RgbaImage,
    tw: f64,
    th: f64,
) -> Vec<SkinPartResult> {
    let (sx, sy) = uv_scale(tw, th);
    let measure = |name: &str, bone: &Bone| {
        let (total, transparent) = count_bone_texture(bone, texture, sx, sy);
        SkinPartResult {
            name: name.to_string(),
            visible: true,
            pixels: total,
            transparent,
            from_geo: true,
            fraction: if total > 0 {
                (total - transparent) as f64 / total as f64
            } else {
                0.0
            },
            tiny: false,
        }
    };
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    // The six standard parts first, in their fixed order; then the rest by
    // name, so a report is the same every time.
    for name in STANDARD_PART_NAMES {
        if let Some(b) = bones.get(name).filter(|b| !b.cubes.is_empty()) {
            seen.insert(name);
            results.push(measure(name, b));
        }
    }
    let rest: BTreeMap<&str, &Bone> = bones
        .iter()
        .filter(|(n, b)| !seen.contains(n.as_str()) && !b.cubes.is_empty())
        .map(|(n, b)| (n.as_str(), b))
        .collect();
    for (name, b) in rest {
        results.push(measure(name, b));
    }
    results
}

fn count_bone_texture(bone: &Bone, texture: &RgbaImage, sx: f64, sy: f64) -> (usize, usize) {
    let (mut total, mut transparent) = (0, 0);
    for cube in &bone.cubes {
        let Some(rects) = cube_uv_rects(cube) else {
            continue;
        };
        // Go keeps the faces in a map, so a repeated face counts once.
        let mut faces: Vec<&(String, crate::mesh::UvRect)> = Vec::new();
        for r in rects.iter().rev() {
            if !faces.iter().any(|f| f.0 == r.0) {
                faces.push(r);
            }
        }
        for (_, r) in faces {
            let (t, tr) = region_visibility(
                texture,
                (r.x * sx) as i64,
                (r.y * sy) as i64,
                ((r.x + r.w) * sx) as i64,
                ((r.y + r.h) * sy) as i64,
            );
            total += t;
            transparent += tr;
        }
    }
    (total, transparent)
}

fn check_from_standard_uv(texture: &RgbaImage, tw: f64, th: f64) -> Vec<SkinPartResult> {
    let (sx, sy) = uv_scale(tw, th);
    let layout: &[(&str, [f64; 5])] = if th == 32.0 {
        &LEGACY32_BODY_PARTS
    } else {
        &STANDARD_BODY_PARTS
    };
    STANDARD_PART_NAMES
        .iter()
        .map(|&name| {
            let Some((_, [u, v, w, h, d])) = layout.iter().find(|(n, _)| *n == name) else {
                return SkinPartResult {
                    name: name.to_string(),
                    ..Default::default()
                };
            };
            let r = box_uv_rects(u * sx, v * sy, w * sx, h * sy, d * sy)[2].1; // north
            let (px, tr) = region_visibility(
                texture,
                r.x as i64,
                r.y as i64,
                (r.x + r.w) as i64,
                (r.y + r.h) as i64,
            );
            SkinPartResult {
                name: name.to_string(),
                visible: true,
                pixels: px,
                transparent: tr,
                fraction: if px > 0 {
                    (px - tr) as f64 / px as f64
                } else {
                    0.0
                },
                ..Default::default()
            }
        })
        .collect()
}

/// Settles each part's visibility from its fraction and whether the skin as
/// a whole is invisible, counting the six standard parts only: overlays fold
/// into the part they cover and accessories are ignored.
///
/// Strict (box-UV geometry): 0-1 parts visible is invisible, 2-3
/// suspicious. Lenient (layout inferred): only 0 is invisible.
pub(crate) fn classify(
    mut results: Vec<SkinPartResult>,
    th: Thresholds,
    strict: bool,
) -> SkinVisibilityResult {
    let mut visible_parent = HashSet::new();
    for r in &mut results {
        r.visible = r.fraction >= th.min_visible_fraction;
        if is_accessory(&r.name) {
            continue;
        }
        let standard = overlay_parent(&r.name).unwrap_or(&r.name).to_string();
        if r.visible {
            visible_parent.insert(standard);
        }
    }
    let visible = STANDARD_PART_NAMES
        .iter()
        .filter(|n| visible_parent.contains(**n))
        .count();
    let is_invisible = if strict { visible <= 1 } else { visible == 0 };
    SkinVisibilityResult {
        suspicious: visible >= 1 && visible < th.min_visible_parts && !is_invisible,
        pass: !is_invisible,
        is_invisible,
        parts: results,
        visible_parts: visible,
        invisible_parts: STANDARD_PART_NAMES.len() - visible,
    }
}

/// Checks that the geometry's bones are big enough to see: the largest axis
/// of the box around each bone's cubes, inflate included, must reach
/// `min_size`. Violations are ordered by bone name.
pub fn validate_geometry_size(geometry: &[u8], min_size: f64) -> GeometrySizeResult {
    geometry_size_of(get_geometry(geometry).as_ref(), min_size)
}

fn geometry_size_of(bones: Option<&Bones>, min_size: f64) -> GeometrySizeResult {
    let min_size = if min_size <= 0.0 {
        DEFAULT_MIN_GEOMETRY_SIZE
    } else {
        min_size
    };
    let empty = Bones::new();
    let bones = bones.unwrap_or(&empty);
    if !bones.contains_key("head") {
        return GeometrySizeResult {
            pass: false,
            violations: vec![GeometryViolation {
                bone: "head".into(),
                size: 0.0,
                minimum: min_size,
            }],
        };
    }
    let mut names: Vec<&String> = bones.keys().collect();
    names.sort();
    let violations: Vec<GeometryViolation> = names
        .into_iter()
        .filter(|n| !bones[*n].cubes.is_empty())
        .filter_map(|n| {
            let size = bone_world_size(&bones[n]);
            (size < min_size).then(|| GeometryViolation {
                bone: n.clone(),
                size,
                minimum: min_size,
            })
        })
        .collect();
    GeometrySizeResult {
        pass: violations.is_empty(),
        violations,
    }
}

/// The main check: every bone the geometry defines is checked where its UVs
/// point, and a bone too small to see counts as invisible however opaque it
/// is. Pass empty geometry for standard skins - most real skins send none.
pub fn validate_skin_invisibility(texture: &RgbaImage, geometry: &[u8]) -> SkinVisibilityResult {
    validate_with(texture, geometry, Thresholds::default())
}

pub(crate) fn validate_with(
    texture: &RgbaImage,
    geometry: &[u8],
    th: Thresholds,
) -> SkinVisibilityResult {
    let th = th.resolved();
    let bones = get_geometry(geometry);
    let (mut parts, strict) = match scan_parts(texture, bones.as_ref()) {
        Scan::Unusable => return unusable_result(),
        Scan::Persona => return persona_result(),
        Scan::Parts { parts, strict } => (parts, strict),
    };
    // Size only means something when geometry parsed into bones; without,
    // the missing-head violation would flag an ordinary skin.
    if bones.as_ref().is_some_and(|b| !b.is_empty()) {
        let tiny: HashSet<String> = geometry_size_of(bones.as_ref(), th.min_geometry_size)
            .violations
            .into_iter()
            .map(|v| v.bone)
            .collect();
        for p in &mut parts {
            if tiny.contains(&p.name) {
                p.tiny = true;
                p.fraction = 0.0;
            }
        }
    }
    classify(parts, th, strict)
}

/// Whether the skin is invisible by the standard layout, with no geometry.
pub fn is_skin_invisible(texture: &RgbaImage) -> bool {
    validate_skin_visibility(texture, &[], DEFAULT_MIN_VISIBLE_FRACTION).is_invisible
}

/// Whether the geometry defines body parts too small to see.
pub fn is_skin_tiny(geometry: &[u8]) -> bool {
    !validate_geometry_size(geometry, DEFAULT_MIN_GEOMETRY_SIZE).pass
}

/// (pixels in the region, how many are fully transparent), the region
/// clipped to the texture.
fn region_visibility(img: &RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64) -> (usize, usize) {
    // As Go's image.Rect, corners given either way round: a per-face uv
    // with a negative size (a flipped face) still covers its pixels.
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(img.width() as i64), y1.min(img.height() as i64));
    if x0 >= x1 || y0 >= y1 {
        return (0, 0);
    }
    let mut transparent = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let a = img.get_pixel(x as u32, y as u32).0[3] as u32;
            if (a | a << 8) as f64 / 65535.0 <= DEFAULT_MIN_VISIBLE_ALPHA {
                transparent += 1;
            }
        }
    }
    (((x1 - x0) * (y1 - y0)) as usize, transparent)
}

/// The largest axis of the box around every cube in the bone, inflate
/// included. A box rather than a sum of sizes, so a hundred invisible specks
/// cannot add up to a visible bone. See docs/design-decisions.md#why-bone-size-is-a-bounding-box.
fn bone_world_size(b: &Bone) -> f64 {
    let (mut lo, mut hi) = ([0.0f64; 3], [0.0f64; 3]);
    let mut measured = false;
    for c in &b.cubes {
        let Some((size, origin)) = cube_dims(c) else {
            continue;
        };
        let inflate = c.inflate.unwrap_or(b.inflate);
        for i in 0..3 {
            let (l, h) = (origin[i] - inflate, origin[i] + size[i] + inflate);
            if !measured {
                lo[i] = l;
                hi[i] = h;
                continue;
            }
            if l < lo[i] {
                lo[i] = l;
            }
            if h > hi[i] {
                hi[i] = h;
            }
        }
        measured = true;
    }
    if !measured {
        return 0.0;
    }
    let mut largest = hi[0] - lo[0];
    for i in 1..3 {
        let e = hi[i] - lo[i];
        if e > largest {
            largest = e;
        }
    }
    largest
}

/// The geometry's bones by name, from the entry with the most cubes; None
/// for empty or unreadable input.
fn get_geometry(raw: &[u8]) -> Option<Bones> {
    if raw.is_empty() {
        return None;
    }
    let geos = parse_geometry(raw).ok()?;
    let mut geo = geos.first()?;
    for g in &geos[1..] {
        if g.total_cubes() > geo.total_cubes() {
            geo = g;
        }
    }
    Some(
        geo.bones
            .iter()
            .map(|b| (b.name.clone(), b.clone()))
            .collect(),
    )
}
