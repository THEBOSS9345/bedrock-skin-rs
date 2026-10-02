//! Rendering: options, framing, and the camera.
//! See docs/views-and-cameras.md.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use image::RgbaImage;

use crate::animation::Pose;
use crate::geometry::{Bone, Geometry, default_geometry, find_cape, select_geometry};
use crate::mesh::build_triangles;
use crate::raster::{Context, Mat4, Texture, Triangle, Vec3};
use crate::render2d::render_2d;
use crate::{Error, gomath};

/// The output edge length used when [`RenderOptions::size`] is zero.
pub const DEFAULT_SIZE: u32 = 512;

/// Which part of the model to render. Bones are included by ancestry rather
/// than from a fixed list, so a custom skin's extra bones (ears, tails,
/// wings, party hats) come along as long as they are parented somewhere
/// under a standard anchor. See docs/views-and-cameras.md.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    /// The full figure.
    #[default]
    Body,
    /// Waist up, arms included.
    Chest,
    /// The head and everything parented under it: hat, ears, horns.
    Head,
    /// A square head icon, framed closer than [`View::Head`].
    Avatar,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            View::Body => "body",
            View::Chest => "chest",
            View::Head => "head",
            View::Avatar => "avatar",
        }
    }
}

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for View {
    type Err = Error;

    /// See [`parse_view`].
    fn from_str(s: &str) -> Result<View, Error> {
        parse_view(s)
    }
}

/// Resolves a view name, as it would arrive in a query string or a config
/// file. Matching ignores case and surrounding space, and blank input is
/// [`View::Body`]. An unrecognised name is [`Error::UnknownView`] rather than
/// a silent fallback, so a request for `avatr` is an error, not a full body.
pub fn parse_view(raw: &str) -> Result<View, Error> {
    match raw.trim().to_lowercase().as_str() {
        "" | "body" => Ok(View::Body),
        "chest" => Ok(View::Chest),
        "head" => Ok(View::Head),
        "avatar" => Ok(View::Avatar),
        _ => Err(Error::UnknownView(raw.to_string())),
    }
}

/// One of the two named camera presets. [`RenderOptions::camera`] bypasses
/// them with an explicit yaw and pitch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Angle {
    /// Straight on.
    Front,
    /// The angled three-quarter look: front, top and one side at once.
    Iso,
}

impl Angle {
    pub fn name(self) -> &'static str {
        match self {
            Angle::Front => "front",
            Angle::Iso => "iso",
        }
    }
}

impl fmt::Display for Angle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Angle {
    type Err = Error;

    /// An angle name; blank is an error here; use [`parse_angle`] to read
    /// blank as "the view's default".
    fn from_str(s: &str) -> Result<Angle, Error> {
        parse_angle(s)?.ok_or_else(|| Error::UnknownAngle(s.to_string()))
    }
}

/// Resolves an angle name the way [`parse_view`] resolves a view. Blank
/// input is None, which [`RenderOptions`] reads as the default for the
/// chosen view.
pub fn parse_angle(raw: &str) -> Result<Option<Angle>, Error> {
    match raw.trim().to_lowercase().as_str() {
        "" => Ok(None),
        "front" => Ok(Some(Angle::Front)),
        "iso" => Ok(Some(Angle::Iso)),
        _ => Err(Error::UnknownAngle(raw.to_string())),
    }
}

/// Splits a comma-separated bone list into trimmed, non-empty names. Blank
/// input is empty, which [`RenderOptions::parts`] reads as "use the view".
pub fn parse_parts(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Positions the view explicitly, instead of letting the view and angle
/// pick a framing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    /// Turns the camera around the vertical axis, in degrees. 0 is straight
    /// on; positive values bring more of the subject's left side into view.
    pub yaw: f64,
    /// Raises the camera, in degrees. Positive values look down.
    pub pitch: f64,
    /// The field of view in degrees. Zero means 35.
    pub fov: f64,
    /// How much room to leave around the subject. Zero means 1.5; 1.0
    /// frames as tightly as possible without clipping.
    pub margin: f64,
}

/// One render. Only the texture is required; [`RenderOptions::new`] sets
/// everything else to its default, and the builder methods change one
/// field each.
///
/// ```no_run
/// # fn main() -> Result<(), bedrock_skin::Error> {
/// use bedrock_skin::{RenderOptions, View, Angle};
/// let tex = image::open("skin.png").unwrap().to_rgba8();
/// let img = RenderOptions::new(&tex).view(View::Avatar).angle(Angle::Iso).size(256).render()?;
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct RenderOptions<'a> {
    /// The skin texture. Bedrock skins are normally 64x64 or 128x128, but
    /// any size works.
    pub texture: &'a RgbaImage,
    /// The skin's model, from [`parse_geometry`](crate::parse_geometry).
    /// Empty uses [`default_geometry`], which is what a Bedrock client
    /// draws for a skin that uses a built-in model - most skins.
    pub geometry: &'a [Geometry],
    /// Which entry of the geometry to render, as the skin's resource patch
    /// names it, e.g. `geometry.humanoid.customSlim`. Empty, or naming an
    /// entry that is not there, uses the entry with the most cubes. The
    /// resource patch is the authoritative wide-vs-slim selector, not the
    /// login packet's ArmSize.
    pub identifier: String,
    /// An equipped cape texture. Its mesh comes from a `cape` bone in the
    /// geometry, else the built-in `geometry.cape`. Not drawn for the head
    /// and avatar views.
    pub cape: Option<&'a RgbaImage>,
    /// The framing. Ignored when `parts` is set.
    pub view: View,
    /// The camera preset; None is the view's default ([`Angle::Iso`] for
    /// [`View::Head`], [`Angle::Front`] otherwise). Ignored when `camera` is
    /// set.
    pub angle: Option<Angle>,
    /// Exactly which bones to render, e.g. `["head", "leftArm"]`. Each pulls
    /// in everything parented under it. Empty means use `view`.
    pub parts: Vec<String>,
    /// An explicit camera, overriding the view's framing and the angle.
    pub camera: Option<Camera>,
    /// The output edge length in pixels; the image is square. Zero means
    /// [`DEFAULT_SIZE`].
    pub size: u32,
    /// Moves bones from where the geometry puts them, e.g. a frame of a
    /// [`Motion`](crate::Motion). Empty is the rest pose.
    pub pose: Pose,
}

impl<'a> RenderOptions<'a> {
    /// Options for a full-body, straight-on, 512x512 render of `texture` on
    /// the default model.
    pub fn new(texture: &'a RgbaImage) -> Self {
        RenderOptions {
            texture,
            geometry: &[],
            identifier: String::new(),
            cape: None,
            view: View::Body,
            angle: None,
            parts: Vec::new(),
            camera: None,
            size: 0,
            pose: Pose::new(),
        }
    }

    pub fn geometry(mut self, geometry: &'a [Geometry]) -> Self {
        self.geometry = geometry;
        self
    }
    pub fn identifier(mut self, identifier: impl Into<String>) -> Self {
        self.identifier = identifier.into();
        self
    }
    pub fn cape(mut self, cape: &'a RgbaImage) -> Self {
        self.cape = Some(cape);
        self
    }
    pub fn view(mut self, view: View) -> Self {
        self.view = view;
        self
    }
    pub fn angle(mut self, angle: Angle) -> Self {
        self.angle = Some(angle);
        self
    }
    pub fn parts<S: Into<String>>(mut self, parts: impl IntoIterator<Item = S>) -> Self {
        self.parts = parts.into_iter().map(Into::into).collect();
        self
    }
    pub fn camera(mut self, camera: Camera) -> Self {
        self.camera = Some(camera);
        self
    }
    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
    pub fn pose(mut self, pose: Pose) -> Self {
        self.pose = pose;
        self
    }

    /// The same as [`render`].
    pub fn render(&self) -> Result<RgbaImage, Error> {
        render(self)
    }

    /// Renders and encodes the result as PNG.
    pub fn render_png(&self) -> Result<Vec<u8>, Error> {
        crate::encode_png(&render(self)?)
    }

    pub(crate) fn scene(&self, pose: &Pose) -> Result<Scene, Error> {
        if self.texture.width() == 0 || self.texture.height() == 0 {
            return Err(Error::NoTexture);
        }
        let size = if self.size == 0 {
            DEFAULT_SIZE
        } else {
            self.size
        } as usize;
        let geos = if self.geometry.is_empty() {
            default_geometry()
        } else {
            self.geometry
        };
        let geo = select_geometry(geos, &self.identifier).ok_or(Error::NoGeometry)?;
        let view = self.view;

        // No cubes anywhere means a persona skin: real bones, no mesh. A
        // flat texture crop is the only meaningful output, and it is what
        // the client itself shows.
        if geo.total_cubes() == 0 {
            return Ok(Scene {
                flat: Some(render_2d(self.texture, view, size as u32)),
                ..Scene::default()
            });
        }

        let (mut fov, mut margin) = (35.0, 1.5);
        let triangles = if !self.parts.is_empty() {
            let by_name = bone_map(geo);
            let include = |name: &str| self.parts.iter().any(|p| is_descendant(&by_name, name, p));
            let t = build_triangles(geo, Some(&include), pose);
            if t.is_empty() {
                return Err(Error::NoMatchingParts);
            }
            t
        } else {
            let t = match include_for_view(geo, view) {
                Some(include) => build_triangles(geo, Some(&*include), pose),
                None => build_triangles(geo, None, pose),
            };
            if t.is_empty() {
                return Err(Error::EmptyView);
            }
            (fov, margin) = framing_for(view);
            t
        };

        let (yaw, pitch);
        if let Some(cam) = self.camera {
            (yaw, pitch) = (cam.yaw, cam.pitch);
            if cam.fov > 0.0 {
                fov = cam.fov;
            }
            if cam.margin > 0.0 {
                margin = cam.margin;
            }
        } else {
            let angle = self.angle.unwrap_or(if view == View::Head {
                Angle::Iso
            } else {
                Angle::Front
            });
            if angle == Angle::Iso {
                // Offset diagonally rather than pulled straight back, the
                // iso camera needs extra margin not to clip a corner.
                margin *= 1.25;
                (yaw, pitch) = (ISO_YAW, ISO_PITCH);
            } else {
                (yaw, pitch) = (0.0, 0.0);
            }
        }

        // The cape is built before the camera: it hangs behind and below the
        // body, so framing on the body alone can push it out of shot.
        let mut cape = Vec::new();
        if self.cape.is_some()
            && cape_visible_in(view, &self.parts)
            && let Some(cape_geo) = cape_geometry_for(geos, geo)
        {
            let include = |name: &str| name == "cape";
            cape = build_triangles(cape_geo, Some(&include), pose);
        }
        Ok(Scene {
            triangles,
            cape,
            fov,
            margin,
            yaw,
            pitch,
            size,
            flat: None,
        })
    }
}

/// Everything a render works out before placing the camera. `flat` is set
/// instead for a persona skin, which has nothing to rasterize.
#[derive(Default)]
pub(crate) struct Scene {
    pub triangles: Vec<Triangle>,
    pub cape: Vec<Triangle>,
    pub fov: f64,
    pub margin: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub size: usize,
    pub flat: Option<RgbaImage>,
}

/// Renders a skin into a square image.
///
/// With only a texture set, this is the full body of a standard humanoid,
/// straight on, at 512x512. Persona skins are handled rather than rejected:
/// their geometry has bones but no cubes, so there is nothing to rasterize,
/// and this falls back to a flat crop of the texture (see
/// [`render_2d`](crate::render_2d)).
pub fn render(opts: &RenderOptions) -> Result<RgbaImage, Error> {
    let sc = opts.scene(&opts.pose)?;
    if let Some(flat) = sc.flat {
        return Ok(flat);
    }
    let mut all = sc.triangles.clone();
    all.extend_from_slice(&sc.cape);
    let (eye, center) = camera_for_yaw_pitch(&all, sc.fov, sc.margin, sc.yaw, sc.pitch);
    Ok(rasterize(
        &sc.triangles,
        &sc.cape,
        opts.texture,
        opts.cape,
        eye,
        center,
        sc.fov,
        sc.size,
    ))
}

fn bone_map(geo: &Geometry) -> HashMap<&str, &Bone> {
    geo.bones.iter().map(|b| (b.name.as_str(), b)).collect()
}

fn is_descendant(by_name: &HashMap<&str, &Bone>, name: &str, ancestor: &str) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    let mut cur = name;
    while !cur.is_empty() && !seen.contains(&cur) {
        if cur == ancestor {
            return true;
        }
        seen.push(cur);
        cur = by_name.get(cur).map_or("", |b| b.parent.as_str());
    }
    false
}

type Include<'g> = Box<dyn Fn(&str) -> bool + 'g>;

fn include_for_view(geo: &Geometry, view: View) -> Option<Include<'_>> {
    let by_name = bone_map(geo);
    match view {
        View::Head | View::Avatar => {
            Some(Box::new(move |name| is_descendant(&by_name, name, "head")))
        }
        // A bust: head, torso and both arms with their own descendants.
        View::Chest => Some(Box::new(move |name| {
            is_descendant(&by_name, name, "head")
                || is_descendant(&by_name, name, "leftArm")
                || is_descendant(&by_name, name, "rightArm")
                || name == "body"
                || name == "waist"
        })),
        View::Body => None,
    }
}

/// Whether a framing shows the cape: a head or avatar crop does not, and a
/// parts list only when it names `cape`.
fn cape_visible_in(view: View, parts: &[String]) -> bool {
    if !parts.is_empty() {
        return parts.iter().any(|p| p == "cape");
    }
    view != View::Head && view != View::Avatar
}

/// The entry to draw an equipped cape from: never the body entry being
/// rendered (a body with its own cape bone would draw it twice, z-fighting),
/// falling back to the built-in `geometry.cape` - which is what makes capes
/// work at all on custom-mesh skins, whose geometry has no cape bone.
fn cape_geometry_for<'a>(geos: &'a [Geometry], body: &Geometry) -> Option<&'a Geometry> {
    geos.iter()
        .filter(|g| g.identifier != body.identifier)
        .find(|g| g.bone_by_name("cape").is_some_and(|b| !b.cubes.is_empty()))
        .or_else(|| find_cape(default_geometry()))
}

/// The field of view and margin that suit a view: avatar is a tight crop,
/// head leaves more headroom, chest and body fit a much taller subject.
fn framing_for(view: View) -> (f64, f64) {
    match view {
        View::Avatar => (25.0, 1.15),
        View::Head => (30.0, 1.4),
        View::Chest => (35.0, 1.5),
        View::Body => (35.0, 1.6),
    }
}

/// The iso preset shows three faces at once (front, top, one side) without
/// foreshortening any of them away to nothing.
const ISO_YAW: f64 = 35.0;
const ISO_PITCH: f64 = 25.0;

fn bounding_box(triangles: &[Triangle]) -> (Vec3, Vec3) {
    let mut it = triangles
        .iter()
        .flat_map(|t| t.0.iter().map(|v| v.position));
    let Some(first) = it.next() else {
        return (Vec3::default(), Vec3::default());
    };
    let (mut lo, mut hi) = (first, first);
    for p in it {
        lo = Vec3::new(
            gomath::min(lo.x, p.x),
            gomath::min(lo.y, p.y),
            gomath::min(lo.z, p.z),
        );
        hi = Vec3::new(
            gomath::max(hi.x, p.x),
            gomath::max(hi.y, p.y),
            gomath::max(hi.z, p.z),
        );
    }
    (lo, hi)
}

/// Frames the camera from the triangles' actual bounding box, never a fixed
/// distance. yaw 0, pitch 0 sits the camera on the -Z side looking toward
/// +Z, up +Y. See docs/rendering-pipeline.md#stage-4--framing-the-camera.
pub(crate) fn camera_for_yaw_pitch(
    triangles: &[Triangle],
    fov: f64,
    margin: f64,
    yaw: f64,
    pitch: f64,
) -> (Vec3, Vec3) {
    let (lo, hi) = bounding_box(triangles);
    let center = Vec3::new(
        (lo.x + hi.x) / 2.0,
        (lo.y + hi.y) / 2.0,
        (lo.z + hi.z) / 2.0,
    );
    let mut half = gomath::max(
        (hi.x - lo.x) / 2.0,
        gomath::max((hi.y - lo.y) / 2.0, (hi.z - lo.z) / 2.0),
    );
    if half <= 0.0 {
        half = 1.0;
    }
    let half_fov = fov * std::f64::consts::PI / 360.0;
    let distance = half / gomath::tan(half_fov) * margin;
    let (yaw, pitch) = (
        yaw * std::f64::consts::PI / 180.0,
        pitch * std::f64::consts::PI / 180.0,
    );
    let offset = Vec3::new(
        -distance * gomath::sin(yaw) * gomath::cos(pitch),
        distance * gomath::sin(pitch),
        -distance * gomath::cos(yaw) * gomath::cos(pitch),
    );
    (
        Vec3::new(
            center.x + offset.x,
            center.y + offset.y,
            center.z + offset.z,
        ),
        center,
    )
}

/// Draws the body, then the cape with its own texture.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize(
    triangles: &[Triangle],
    cape: &[Triangle],
    texture: &RgbaImage,
    cape_texture: Option<&RgbaImage>,
    eye: Vec3,
    center: Vec3,
    fov: f64,
    size: usize,
) -> RgbaImage {
    let mut ctx = Context::new(size, size);
    // Clip space only - the screen mapping comes after the perspective
    // divide. See docs/design-decisions.md#why-no-viewport-in-the-shader-matrix.
    let matrix =
        Mat4::look_at(eye, center, Vec3::new(0.0, 1.0, 0.0)).perspective(fov, 1.0, 1.0, 500.0);
    let tex = Texture::new(texture);
    for t in triangles {
        ctx.draw_triangle(t, &matrix, &tex);
    }
    if let Some(cape_texture) = cape_texture {
        let tex = Texture::new(cape_texture);
        for t in cape {
            ctx.draw_triangle(t, &matrix, &tex);
        }
    }
    ctx.into_image()
}
