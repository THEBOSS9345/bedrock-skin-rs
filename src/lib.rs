//! Render Minecraft Bedrock skins to PNG and GIF, in pure Rust.
//!
//! 3D bodies, heads and avatars, capes, slim and wide arms, custom geometry,
//! persona skins, animations from Blockbench files, and a detector for
//! invisible skins. No GPU, no browser, no C library: a small software
//! rasterizer. It is a port of the Go library
//! [bedrock-skin-go](https://github.com/THEBOSS9345/bedrock-skin-go) and
//! renders the same images, pixel for pixel.
//!
//! ```no_run
//! # fn main() -> Result<(), bedrock_skin::Error> {
//! use bedrock_skin::{RenderOptions, View, Angle};
//!
//! let texture = bedrock_skin::decode_image(&std::fs::read("skin.png").unwrap())?;
//! let png = RenderOptions::new(&texture)
//!     .view(View::Avatar)
//!     .angle(Angle::Iso)
//!     .size(256)
//!     .render_png()?;
//! std::fs::write("avatar.png", png).unwrap();
//! # Ok(()) }
//! ```
//!
//! # Geometry is optional
//!
//! Leaving [`RenderOptions::geometry`] empty is not a shortcut - it is the
//! correct input for most real skins. A Bedrock client sends no mesh at all
//! for a skin that uses one of the built-in models: its login packet carries
//! the literal JSON `null` and names the model only in the skin's resource
//! patch. [`default_geometry`] stands in with the model the client itself
//! would use. When a skin does carry geometry, read it with
//! [`parse_geometry`], and pick the entry with [`parse_resource_patch`].
//!
//! # Animation
//!
//! [`Motion`] holds Minecraft's own player movements, and
//! [`parse_animations`] reads Bedrock animation files with their Molang
//! expressions. [`render_gif`] makes a looping GIF; [`example_animations`]
//! has 33 to try. A viewer that turns the model as it plays uses
//! [`prepare_frames`] and [`Frames::draw`] to draw one frame at a time at its
//! own camera.
//!
//! # Invisible skins
//!
//! [`Skin`] reports whether a skin's body parts actually show, for blocking
//! the "invisible player" trick.
//!
//! The docs/ folder of the repository explains how it all works.

#![forbid(unsafe_code)]

mod animation;
mod animfile;
mod detect;
mod equipment;
mod geometry;
mod geoquery;
mod gomath;
mod invisible;
mod jsonread;
mod mesh;
mod molang;
mod polymesh;
mod raster;
mod render;
mod render2d;
mod wire;

use std::fmt;
use std::io::Cursor;

use image::{ImageFormat, ImageReader, RgbaImage};

pub use animation::{
    AnimationOptions, Animator, BonePose, Frames, Motion, Pose, parse_motion, prepare_frames,
    render_frames, render_gif, write_gif,
};
pub use animfile::{Animation, example_animations, parse_animations};
pub use detect::{PartReport, PartVisibility, Skin, SkinOptions, SkinReport, Verdict};
pub use equipment::{
    Armor, Held, ItemAdjust, ItemAnimationOptions, ItemOptions, Scale, render_item,
    render_item_frames, render_item_gif,
};
pub use geometry::{
    Bone, Cube, FaceUv, Geometry, Locator, ResourcePatch, complexity, default_geometry, find_cape,
    is_empty, parse_geometry, parse_resource_patch, select_geometry,
};
pub use geoquery::{GeometryTree, GeometryValue, parse_geometry_tree};
pub use invisible::{
    DEFAULT_MIN_GEOMETRY_SIZE, DEFAULT_MIN_VISIBLE_ALPHA, DEFAULT_MIN_VISIBLE_FRACTION,
    DEFAULT_MIN_VISIBLE_PARTS, GeometrySizeResult, GeometryViolation, SkinPartResult,
    SkinVisibilityResult, is_skin_invisible, is_skin_tiny, validate_geometry_size,
    validate_skin_invisibility, validate_skin_visibility,
};
pub use polymesh::{PolyMesh, PolyVertex};
pub use render::{
    Angle, AnimatedTexture, AnimatedType, Camera, DEFAULT_SIZE, RenderOptions, View, parse_angle,
    parse_parts, parse_view, render,
};
pub use render2d::render_2d;
pub use wire::{DecodedSkin, WireAnimation, WireSkin};

/// Everything that can go wrong. Every variant but `Encode` describes bad
/// input rather than an internal failure, so a service can answer all of
/// them with a 4xx.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The texture is missing or has no pixels.
    NoTexture,
    /// The geometry held no entry that could be rendered. An empty geometry
    /// list is not this error - it selects [`default_geometry`].
    NoGeometry,
    /// No bone matched [`RenderOptions::parts`], usually a misspelled name.
    NoMatchingParts,
    /// The view scoped to bones with no cubes, e.g. the head view on a model
    /// with no head bone.
    EmptyView,
    /// [`parse_view`] did not recognise the name.
    UnknownView(String),
    /// [`parse_angle`] did not recognise the name.
    UnknownAngle(String),
    /// [`parse_motion`] did not recognise the name.
    UnknownMotion(String),
    /// [`parse_animations`] found no animations in the file.
    NoAnimations,
    /// [`parse_geometry_tree`] found valid JSON holding no geometry.
    NoGeometryModels,
    /// The input is not valid JSON.
    Json(serde_json::Error),
    /// The geometry is valid JSON but not geometry.
    Geometry(String),
    /// An animation file could not be read.
    Animation(String),
    /// A resource patch could not be read.
    ResourcePatch(String),
    /// The bytes are not a PNG or JPEG image, or `what` was wrong with them.
    Image(String),
    /// Raw pixel data did not match its dimensions.
    Pixels(String),
    /// Encoding the output failed.
    Encode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoTexture => f.write_str("texture is required"),
            Error::NoGeometry => f.write_str("geometry has no usable entries"),
            Error::NoMatchingParts => f.write_str("no bones matched the requested parts"),
            Error::EmptyView => f.write_str("nothing to render for this view"),
            Error::UnknownView(v) => write!(f, "unknown view {v:?}"),
            Error::UnknownAngle(a) => write!(f, "unknown angle {a:?}"),
            Error::UnknownMotion(m) => write!(f, "unknown motion {m:?}"),
            Error::NoAnimations => f.write_str("no animations in the file"),
            Error::NoGeometryModels => f.write_str("no geometry models in the file"),
            Error::Json(e) => write!(f, "geometry: {e}"),
            Error::Geometry(e) => write!(f, "geometry: {e}"),
            Error::Animation(e) | Error::Image(e) | Error::Pixels(e) | Error::Encode(e) => {
                f.write_str(e)
            }
            Error::ResourcePatch(e) => write!(f, "resource patch: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Json(e) => Some(e),
            _ => None,
        }
    }
}

/// Decodes PNG or JPEG bytes into an RGBA image.
///
/// It applies no size limit, and decoding is where a malicious image does
/// its damage: a few-KB file can declare enormous dimensions. Check
/// [`image_dimensions`] first for untrusted input.
pub fn decode_image(data: &[u8]) -> Result<RgbaImage, Error> {
    if data.is_empty() {
        return Err(Error::Image("no image data".into()));
    }
    let reader = image_reader(data)?;
    let img = reader
        .decode()
        .map_err(|e| Error::Image(format!("not a valid image: {e}")))?;
    Ok(img.to_rgba8())
}

fn image_reader(data: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, Error> {
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| Error::Image(format!("not a valid image: {e}")))?;
    match reader.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg) => Ok(reader),
        _ => Err(Error::Image("not a valid image: not a PNG or JPEG".into())),
    }
}

/// An encoded image's dimensions, read from its header without decoding the
/// pixels: the check to make before [`decode_image`] on untrusted uploads.
pub fn image_dimensions(data: &[u8]) -> Result<(u32, u32), Error> {
    if data.is_empty() {
        return Err(Error::Image("no image data".into()));
    }
    image_reader(data)?
        .into_dimensions()
        .map_err(|e| Error::Image(format!("not a valid image: {e}")))
}

/// Encodes an image as PNG.
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, Error> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// Wraps raw non-premultiplied RGBA pixels as an image, which is the form
/// Bedrock sends a skin in: `SkinData` decodes to width*height*4 bytes with
/// no header, the dimensions arriving separately. A length that disagrees
/// with the dimensions is an error rather than a garbled image.
///
/// See docs/skin-data.md#the-texture-is-not-an-image-file.
pub fn texture_from_rgba(pix: Vec<u8>, width: u32, height: u32) -> Result<RgbaImage, Error> {
    if width == 0 || height == 0 {
        return Err(Error::Pixels(format!(
            "invalid dimensions {width}x{height}"
        )));
    }
    let want = width as usize * height as usize * 4;
    if pix.len() != want {
        return Err(Error::Pixels(format!(
            "got {} bytes of pixel data, expected {want} for {width}x{height}",
            pix.len()
        )));
    }
    Ok(RgbaImage::from_raw(width, height, pix).expect("length checked"))
}

/// [`RenderOptions`] with encoded bytes in place of images, for callers who
/// hold file or wire bytes and want PNG bytes back. Every field behaves as
/// its [`RenderOptions`] counterpart.
#[derive(Clone, Debug, Default)]
pub struct BytesOptions<'a> {
    /// An encoded PNG or JPEG. Required. Bedrock sends skins as raw RGBA;
    /// use [`texture_from_rgba`] and [`RenderOptions`] for those.
    pub texture: &'a [u8],
    /// A raw geometry.json. Empty, or the literal `null` a client sends for
    /// a built-in model, uses [`default_geometry`].
    pub geometry: &'a [u8],
    /// An encoded cape texture; empty for none.
    pub cape: &'a [u8],
    /// A persona skin's animation images, encoded; see
    /// [`RenderOptions::animated`].
    pub animated: Vec<(AnimatedType, &'a [u8])>,
    pub identifier: String,
    pub view: View,
    pub angle: Option<Angle>,
    pub parts: Vec<String>,
    pub camera: Option<Camera>,
    pub size: u32,
    /// [`RenderOptions::armor`] with each piece encoded.
    pub armor: ArmorBytes<'a>,
    /// [`RenderOptions`]' hands with the item encoded.
    pub right_hand: HeldBytes<'a>,
    pub left_hand: HeldBytes<'a>,
    pub scale: Scale,
    /// [`RenderOptions::hide_skin`]; `texture` may then be empty.
    pub hide_skin: bool,
}

/// [`Armor`] with each piece's texture encoded as PNG or JPEG; an empty
/// piece is not worn.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArmorBytes<'a> {
    pub helmet: &'a [u8],
    pub chestplate: &'a [u8],
    pub leggings: &'a [u8],
    pub boots: &'a [u8],
    pub elytra: &'a [u8],
}

impl<'a> ArmorBytes<'a> {
    /// [`Armor::set`] for encoded textures.
    pub fn set(layer1: &'a [u8], layer2: &'a [u8]) -> ArmorBytes<'a> {
        ArmorBytes {
            helmet: layer1,
            chestplate: layer1,
            leggings: layer2,
            boots: layer1,
            elytra: &[],
        }
    }
}

/// [`Held`] with the item's sprite an encoded PNG or JPEG; empty holds
/// nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeldBytes<'a> {
    pub item: &'a [u8],
    pub flat: bool,
    pub adjust: ItemAdjust,
}

/// Renders from encoded bytes to PNG bytes: [`render`] with decoding and
/// encoding folded in. Bound image dimensions with [`image_dimensions`]
/// first for untrusted uploads.
pub fn render_bytes(opts: &BytesOptions) -> Result<Vec<u8>, Error> {
    with_decoded(opts, |ro| ro.render_png())
}

/// Decodes every image and parses the geometry, then hands the resulting
/// [`RenderOptions`] - which borrow them - to `f`.
fn with_decoded<R>(
    opts: &BytesOptions,
    f: impl FnOnce(RenderOptions) -> Result<R, Error>,
) -> Result<R, Error> {
    if opts.texture.is_empty() && !opts.hide_skin {
        return Err(Error::NoTexture);
    }
    let texture = if opts.texture.is_empty() {
        RgbaImage::new(0, 0)
    } else {
        decode_image(opts.texture).map_err(|e| Error::Image(format!("texture: {e}")))?
    };
    let geos = if is_empty(opts.geometry) {
        Vec::new()
    } else {
        parse_geometry(opts.geometry)?
    };
    let cape = if opts.cape.is_empty() {
        None
    } else {
        Some(decode_image(opts.cape).map_err(|e| Error::Image(format!("cape: {e}")))?)
    };
    let mut ro = RenderOptions::new(&texture)
        .geometry(&geos)
        .identifier(opts.identifier.clone())
        .view(opts.view);
    ro.angle = opts.angle;
    ro.parts = opts.parts.clone();
    ro.camera = opts.camera;
    ro.size = opts.size;
    ro.cape = cape.as_ref();
    let animated = opts
        .animated
        .iter()
        .map(|(kind, data)| {
            decode_image(data)
                .map(|img| (*kind, img))
                .map_err(|e| Error::Image(format!("animation {}: {e}", *kind as u32)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (kind, img) in &animated {
        ro = ro.animated(*kind, img);
    }

    // A set shares one texture between several pieces, so each distinct
    // encoding is decoded once.
    const PIECES: [&str; 5] = ["helmet", "chestplate", "leggings", "boots", "elytra"];
    let a = &opts.armor;
    let raw = [a.helmet, a.chestplate, a.leggings, a.boots, a.elytra];
    let mut decoded: Vec<RgbaImage> = Vec::new();
    let mut index: [Option<usize>; 5] = [None; 5];
    for i in 0..raw.len() {
        if raw[i].is_empty() {
            continue;
        }
        if let Some(j) = (0..i).find(|&j| index[j].is_some() && raw[j] == raw[i]) {
            index[i] = index[j];
            continue;
        }
        decoded.push(
            decode_image(raw[i]).map_err(|e| Error::Image(format!("armor {}: {e}", PIECES[i])))?,
        );
        index[i] = Some(decoded.len() - 1);
    }
    let held = |h: &HeldBytes, name: &str| -> Result<Option<RgbaImage>, Error> {
        if h.item.is_empty() {
            return Ok(None);
        }
        decode_image(h.item)
            .map(Some)
            .map_err(|e| Error::Image(format!("{name} item: {e}")))
    };
    let right = held(&opts.right_hand, "right hand")?;
    let left = held(&opts.left_hand, "left hand")?;

    let piece = |i: usize| index[i].map(|k| &decoded[k]);
    ro.armor = Armor {
        helmet: piece(0),
        chestplate: piece(1),
        leggings: piece(2),
        boots: piece(3),
        elytra: piece(4),
    };
    ro.right_hand = Held {
        item: right.as_ref(),
        flat: opts.right_hand.flat,
        adjust: opts.right_hand.adjust,
    };
    ro.left_hand = Held {
        item: left.as_ref(),
        flat: opts.left_hand.flat,
        adjust: opts.left_hand.adjust,
    };
    ro.scale = opts.scale.clone();
    ro.hide_skin = opts.hide_skin;
    f(ro)
}

/// [`ItemOptions`] with the item's sprite an encoded PNG or JPEG.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemBytesOptions<'a> {
    pub item: &'a [u8],
    pub angle: Option<Angle>,
    pub camera: Option<Camera>,
    pub size: u32,
    pub adjust: ItemAdjust,
}

impl ItemBytesOptions<'_> {
    fn with_item<R>(&self, f: impl FnOnce(ItemOptions) -> Result<R, Error>) -> Result<R, Error> {
        if self.item.is_empty() {
            return Err(Error::NoTexture);
        }
        let item = decode_image(self.item).map_err(|e| Error::Image(format!("item: {e}")))?;
        f(ItemOptions {
            item: &item,
            angle: self.angle,
            camera: self.camera,
            size: self.size,
            adjust: self.adjust,
        })
    }
}

/// Renders an item on its own from encoded bytes to PNG bytes:
/// [`render_item`] with decoding and encoding folded in.
pub fn render_item_bytes(opts: &ItemBytesOptions) -> Result<Vec<u8>, Error> {
    opts.with_item(|o| encode_png(&render_item(&o)?))
}

/// [`ItemAnimationOptions`] with the item's sprite encoded.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemAnimationBytesOptions<'a> {
    pub item: ItemBytesOptions<'a>,
    pub duration: f64,
    pub fps: u32,
    pub frames: u32,
}

/// Spins an item on its own from encoded bytes to GIF bytes:
/// [`render_item_gif`] with decoding folded in.
pub fn render_item_gif_bytes(opts: &ItemAnimationBytesOptions) -> Result<Vec<u8>, Error> {
    opts.item.with_item(|item| {
        render_item_gif(&ItemAnimationOptions {
            item,
            duration: opts.duration,
            fps: opts.fps,
            frames: opts.frames,
        })
    })
}

/// [`AnimationOptions`] with encoded bytes in place of images:
/// [`BytesOptions`] plus the animation fields, which behave as their
/// [`AnimationOptions`] counterparts.
#[derive(Clone)]
pub struct AnimationBytesOptions<'a> {
    pub bytes: BytesOptions<'a>,
    pub animation: &'a dyn Animator,
    pub fps: u32,
    pub frames: u32,
    pub workers: usize,
}

impl<'a> AnimationBytesOptions<'a> {
    pub fn new(bytes: BytesOptions<'a>, animation: &'a dyn Animator) -> Self {
        AnimationBytesOptions {
            bytes,
            animation,
            fps: 0,
            frames: 0,
            workers: 0,
        }
    }
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        self
    }
    pub fn frames(mut self, frames: u32) -> Self {
        self.frames = frames;
        self
    }
    pub fn workers(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    fn with_animation<R>(
        &self,
        f: impl FnOnce(AnimationOptions) -> Result<R, Error>,
    ) -> Result<R, Error> {
        with_decoded(&self.bytes, |ro| {
            f(AnimationOptions::new(ro, self.animation)
                .fps(self.fps)
                .frames(self.frames)
                .workers(self.workers))
        })
    }

    /// The same as [`render_gif_bytes`].
    pub fn render_gif(&self) -> Result<Vec<u8>, Error> {
        render_gif_bytes(self)
    }

    /// The same as [`render_frames_png`].
    pub fn render_frames_png(&self) -> Result<Vec<Vec<u8>>, Error> {
        render_frames_png(self)
    }
}

/// Renders an animation from encoded bytes to GIF bytes:
/// [`render_gif`] with decoding folded in.
pub fn render_gif_bytes(opts: &AnimationBytesOptions) -> Result<Vec<u8>, Error> {
    opts.with_animation(|ao| ao.render_gif())
}

/// Renders an animation from encoded bytes and returns every frame as PNG
/// bytes, in order: [`render_frames`] with decoding and
/// encoding folded in.
pub fn render_frames_png(opts: &AnimationBytesOptions) -> Result<Vec<Vec<u8>>, Error> {
    opts.with_animation(|ao| ao.render_frames()?.iter().map(encode_png).collect())
}

impl BytesOptions<'_> {
    /// The same as [`render_bytes`].
    pub fn render_png(&self) -> Result<Vec<u8>, Error> {
        render_bytes(self)
    }
}
