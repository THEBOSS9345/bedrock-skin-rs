//! Rendering: options, framing, and the camera.
//! See docs/views-and-cameras.md.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use image::ImageEncoder;
use image::RgbaImage;

use crate::animation::Pose;
use crate::equipment::{
    ARMOR_PIECES, Armor, ELYTRA_PIECE, Held, Scale, armor_geometry, build_held_item, elytra_pose,
    hands,
};
use crate::geometry::{Bone, Geometry, default_geometry, find_cape, select_geometry};
use crate::mesh::{bone_world_matrices, build_triangles};
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
    /// The extra textures a persona skin's animations carry - the face, and
    /// animated body parts. Each draws the geometry entry made for it
    /// alongside the main one; a persona skin's head lives only in its face
    /// entry. See docs/geometry-format.md#persona-skins.
    pub animated: Vec<AnimatedTexture<'a>>,
    /// The armor and elytra worn over the skin. The default wears none. See
    /// docs/equipment.md.
    pub armor: Armor<'a>,
    /// The items held in each hand. The default holds nothing. See
    /// docs/equipment.md#held-items.
    pub right_hand: Held<'a>,
    pub left_hand: Held<'a>,
    /// Resizes the figure or any bone. The default changes nothing. See
    /// docs/equipment.md#scale.
    pub scale: Scale,
    /// Draws the equipment alone - armor, elytra, held items and cape -
    /// posed and framed as it would be on the skin, so any piece can be
    /// rendered by itself; `parts` and `view` pick which. The texture is
    /// then not read: [`RenderOptions::equipment`] needs none.
    /// [`Error::EmptyView`] or [`Error::NoMatchingParts`] means no equipment
    /// was left to draw. See docs/equipment.md#equipment-on-its-own.
    pub hide_skin: bool,
}

/// The kind of a skin animation, numbered as the Bedrock protocol numbers
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimatedType {
    /// The face: eyes that blink.
    Face = 1,
    /// A 32x32 animated body part.
    Body32 = 2,
    /// A 128x128 animated body part.
    Body128 = 3,
}

impl AnimatedType {
    /// The protocol's number for a type; None for one this does not know.
    pub fn from_protocol(n: u32) -> Option<AnimatedType> {
        match n {
            1 => Some(AnimatedType::Face),
            2 => Some(AnimatedType::Body32),
            3 => Some(AnimatedType::Body128),
            _ => None,
        }
    }

    /// The identifier prefix of the geometry entry the type draws, e.g.
    /// `geometry.animated_face_persona-<id>`.
    fn entry_prefix(self) -> &'static str {
        match self {
            AnimatedType::Face => "geometry.animated_face",
            AnimatedType::Body32 => "geometry.animated_32x32",
            AnimatedType::Body128 => "geometry.animated_128x128",
        }
    }
}

/// One skin animation's image: its frames stacked top to bottom, as the
/// client sends it.
#[derive(Clone, Copy, Debug)]
pub struct AnimatedTexture<'a> {
    pub kind: AnimatedType,
    pub texture: &'a RgbaImage,
}

/// The entry an animation type draws.
pub(crate) fn animated_entry(geos: &[Geometry], kind: AnimatedType) -> Option<&Geometry> {
    geos.iter()
        .find(|g| g.identifier.starts_with(kind.entry_prefix()))
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
            animated: Vec::new(),
            armor: Armor::default(),
            right_hand: Held::default(),
            left_hand: Held::default(),
            scale: Scale::default(),
            hide_skin: false,
        }
    }

    /// Options for the equipment alone, with no skin: [`RenderOptions::new`]
    /// with `hide_skin` set and no texture needed.
    pub fn equipment() -> RenderOptions<'a> {
        static NO_SKIN: OnceLock<RgbaImage> = OnceLock::new();
        let mut o = RenderOptions::new(NO_SKIN.get_or_init(|| RgbaImage::new(0, 0)));
        o.hide_skin = true;
        o
    }

    pub fn armor(mut self, armor: Armor<'a>) -> Self {
        self.armor = armor;
        self
    }
    pub fn right_hand(mut self, held: Held<'a>) -> Self {
        self.right_hand = held;
        self
    }
    pub fn left_hand(mut self, held: Held<'a>) -> Self {
        self.left_hand = held;
        self
    }
    pub fn scale(mut self, scale: Scale) -> Self {
        self.scale = scale;
        self
    }
    pub fn hide_skin(mut self, hide: bool) -> Self {
        self.hide_skin = hide;
        self
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
    /// Adds a persona animation texture (see [`RenderOptions::animated`]).
    pub fn animated(mut self, kind: AnimatedType, texture: &'a RgbaImage) -> Self {
        self.animated.push(AnimatedTexture { kind, texture });
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

    /// Renders and writes the PNG to `w` - an HTTP response, a file -
    /// without holding the encoded bytes first. It writes the same bytes
    /// [`RenderOptions::render_png`] returns.
    pub fn write_png<W: std::io::Write>(&self, w: W) -> Result<(), Error> {
        let img = render(self)?;
        image::codecs::png::PngEncoder::new(w)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| Error::Encode(e.to_string()))
    }

    pub(crate) fn scene(&self, pose: &Pose) -> Result<Scene<'a>, Error> {
        if !self.hide_skin && (self.texture.width() == 0 || self.texture.height() == 0) {
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

        // Part scales and holding an item change the pose of the skin, its
        // armor and the item alike.
        let mut pose = self.scale.parts_pose(pose);
        let mut held = Vec::new();
        for (i, h) in [self.right_hand, self.left_hand].into_iter().enumerate() {
            let Some(item) = h.item else {
                continue;
            };
            if let Some((skel, arm)) = hands()[i].skeleton(geo) {
                pose = hands()[i].holding_pose(&pose, &arm);
                held.push((skel, &hands()[i], h, item));
            }
        }
        let pose = &pose;

        // No cubes and no poly mesh anywhere: bones with nothing to draw. A
        // flat crop is the only output left.
        // See docs/design-decisions.md#why-persona-skins-fall-back-to-2d.
        if !geo.has_mesh() && !self.hide_skin {
            return Ok(Scene {
                flat: Some(render_2d(self.texture, view, size as u32)),
                ..Scene::default()
            });
        }

        let (mut fov, mut margin) = (35.0, 1.5);
        let build_posed = |g: &Geometry, pose: &Pose| {
            if !self.parts.is_empty() {
                let by_name = bone_map(g);
                let include =
                    |name: &str| self.parts.iter().any(|p| is_descendant(&by_name, name, p));
                build_triangles(g, Some(&include), pose)
            } else {
                match include_for_view(g, view) {
                    Some(include) => build_triangles(g, Some(&*include), pose),
                    None => build_triangles(g, None, pose),
                }
            }
        };
        let build = |g: &Geometry| build_posed(g, pose);
        let includes = |g: &Geometry, name: &str| {
            if !self.parts.is_empty() {
                let by_name = bone_map(g);
                self.parts.iter().any(|p| is_descendant(&by_name, name, p))
            } else {
                include_for_view(g, view).is_none_or(|include| include(name))
            }
        };
        let empty = || {
            if self.parts.is_empty() {
                Error::EmptyView
            } else {
                Error::NoMatchingParts
            }
        };
        let mut layers = Vec::new();
        if !self.hide_skin {
            let triangles = build(geo);
            let mut drawn = triangles.len();
            layers.push(Layer {
                triangles,
                texture: self.texture,
            });
            for a in &self.animated {
                if let Some(g) = animated_entry(geos, a.kind)
                    && g.identifier != geo.identifier
                {
                    let triangles = build(g);
                    drawn += triangles.len();
                    layers.push(Layer {
                        triangles,
                        texture: a.texture,
                    });
                }
            }
            // With the skin drawn, equipment never decides whether the view
            // has anything in it: that is the skin's to answer.
            if drawn == 0 {
                return Err(empty());
            }
        }
        for (i, tex) in self.armor.textures().into_iter().enumerate() {
            let Some(tex) = tex else {
                continue;
            };
            let g = armor_geometry(ARMOR_PIECES[i]);
            let triangles = if i == ELYTRA_PIECE {
                build_posed(g, &elytra_pose(pose))
            } else {
                build(g)
            };
            layers.push(Layer {
                triangles,
                texture: tex,
            });
        }
        for (skel, side, h, item) in &held {
            if includes(skel, side.bone) {
                let world = bone_world_matrices(skel, pose)[side.bone];
                layers.push(Layer {
                    triangles: build_held_item(item, &h.to_model(side), &world),
                    texture: item,
                });
            }
        }
        if self.parts.is_empty() {
            (fov, margin) = framing_for(view);
        }

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
        if self.scale.model > 0.0 {
            // The camera fits the model's bounds; less room around them
            // draws it larger.
            margin /= self.scale.model;
        }

        // The cape is built before the camera: it hangs behind and below the
        // body, so framing on the body alone can push it out of shot.
        if let Some(cape_texture) = self.cape
            && cape_visible_in(view, &self.parts)
            && let Some(cape_geo) = cape_geometry_for(geos, geo)
        {
            let include = |name: &str| name == "cape";
            let triangles = build_triangles(cape_geo, Some(&include), pose);
            if !triangles.is_empty() {
                layers.push(Layer {
                    triangles,
                    texture: cape_texture,
                });
            }
        }
        if self.hide_skin && layers.iter().all(|l| l.triangles.is_empty()) {
            return Err(empty());
        }
        Ok(Scene {
            layers,
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
/// instead for geometry with nothing to rasterize.
#[derive(Default)]
pub(crate) struct Scene<'a> {
    pub layers: Vec<Layer<'a>>,
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
/// straight on, at 512x512. Persona skins render in 3D from their poly
/// meshes; add their animation images with [`RenderOptions::animated`] to
/// get the head. Geometry whose bones draw nothing falls back to a flat crop
/// (see [`render_2d`](crate::render_2d)).
pub fn render(opts: &RenderOptions) -> Result<RgbaImage, Error> {
    let sc = opts.scene(&opts.pose)?;
    if let Some(flat) = sc.flat {
        return Ok(flat);
    }
    let all = sc.framing();
    let (eye, center) = camera_for_yaw_pitch(&all, sc.fov, sc.margin, sc.yaw, sc.pitch);
    Ok(rasterize(&sc.layers, eye, center, sc.fov, sc.size))
}

/// Triangles drawn with one texture. A scene draws its layers in order: the
/// body, any animated persona parts, then the cape.
pub(crate) struct Layer<'a> {
    pub triangles: Vec<Triangle>,
    pub texture: &'a RgbaImage,
}

impl Scene<'_> {
    /// What the camera is fitted around: every layer.
    pub(crate) fn framing(&self) -> Vec<Triangle> {
        self.layers
            .iter()
            .flat_map(|l| l.triangles.iter().copied())
            .collect()
    }
}

fn bone_map(geo: &Geometry) -> HashMap<&str, &Bone> {
    geo.bones.iter().map(|b| (b.name.as_str(), b)).collect()
}

/// Bone names compare ignoring ASCII case, as Bedrock compares them: persona
/// models name their limbs "leftarm" where vanilla says "leftArm".
/// See docs/geometry-format.md#bone-names-ignore-case.
pub(crate) fn same_bone(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn is_descendant(by_name: &HashMap<&str, &Bone>, name: &str, ancestor: &str) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    let mut cur = name;
    while !cur.is_empty() && !seen.contains(&cur) {
        if same_bone(cur, ancestor) {
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
                || same_bone(name, "body")
                || same_bone(name, "waist")
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
pub(crate) const ISO_YAW: f64 = 35.0;
pub(crate) const ISO_PITCH: f64 = 25.0;

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

/// Draws each layer with its own texture, in order.
pub(crate) fn rasterize(
    layers: &[Layer],
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
    for l in layers {
        if l.triangles.is_empty() {
            continue;
        }
        let tex = Texture::new(l.texture);
        for t in &l.triangles {
            ctx.draw_triangle(t, &matrix, &tex);
        }
    }
    ctx.into_image()
}
