//! Armor, the elytra, held items, scale, and items on their own.
//! See docs/equipment.md.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use image::RgbaImage;

use crate::Error;
use crate::animation::{BonePose, Pose, encode_gif};
use crate::geometry::{Bone, Geometry, parse_geometry};
use crate::gomath;
use crate::mesh::{at, rotation_matrix};
use crate::raster::{Mat4, Triangle, Vec3, Vertex};
use crate::render::{
    Angle, Camera, DEFAULT_SIZE, ISO_PITCH, ISO_YAW, Layer, camera_for_yaw_pitch, rasterize,
    same_bone,
};

/// The armor a skin wears: one texture per piece, as a resource pack lays
/// them out. The helmet, chestplate and boots take the set's first layer
/// (e.g. `textures/models/armor/diamond_1.png`), the leggings its second
/// (`diamond_2.png`). A piece that is None is not worn, so pieces from
/// different sets mix freely. See docs/equipment.md.
#[derive(Clone, Copy, Debug, Default)]
pub struct Armor<'a> {
    pub helmet: Option<&'a RgbaImage>,
    pub chestplate: Option<&'a RgbaImage>,
    pub leggings: Option<&'a RgbaImage>,
    pub boots: Option<&'a RgbaImage>,
    /// The elytra's texture (`textures/models/armor/elytra.png`), worn on
    /// the back. It takes the chestplate's slot, as in game: with both set,
    /// only the elytra is worn. See docs/equipment.md#elytra.
    pub elytra: Option<&'a RgbaImage>,
}

impl<'a> Armor<'a> {
    /// A full set of one material: `layer1` for the helmet, chestplate and
    /// boots, `layer2` for the leggings.
    pub fn set(layer1: &'a RgbaImage, layer2: &'a RgbaImage) -> Armor<'a> {
        Armor {
            helmet: Some(layer1),
            chestplate: Some(layer1),
            leggings: Some(layer2),
            boots: Some(layer1),
            elytra: None,
        }
    }

    /// Each piece's texture in [`ARMOR_PIECES`] order.
    pub(crate) fn textures(&self) -> [Option<&'a RgbaImage>; 5] {
        let chest = if self.elytra.is_some() {
            None
        } else {
            self.chestplate
        };
        [self.helmet, chest, self.leggings, self.boots, self.elytra]
    }
}

/// The vanilla armor model, one entry per piece: the sizes and inflates of
/// the game's geometry.humanoid.armor1 and armor2, and its geometry.elytra,
/// on the player model's skeleton so a pose moves it with the skin. See
/// docs/equipment.md#the-armor-model.
static ARMOR_GEOMETRY_JSON: &str = include_str!("armor_geometry.json");

/// Each piece's model, in the order [`Armor::textures`] lists them, which is
/// also the order they are drawn.
pub(crate) const ARMOR_PIECES: [&str; 5] = [
    "geometry.humanoid.armor.helmet",
    "geometry.humanoid.armor.chestplate",
    "geometry.humanoid.armor.leggings",
    "geometry.humanoid.armor.boots",
    "geometry.elytra",
];

/// The elytra's index in [`ARMOR_PIECES`].
pub(crate) const ELYTRA_PIECE: usize = 4;

pub(crate) fn armor_geometry(identifier: &str) -> &'static Geometry {
    static GEOS: OnceLock<Vec<Geometry>> = OnceLock::new();
    GEOS.get_or_init(|| {
        parse_geometry(ARMOR_GEOMETRY_JSON.as_bytes())
            .expect("bundled armor_geometry.json is valid")
    })
    .iter()
    .find(|g| g.identifier == identifier)
    .expect("bundled armor_geometry.json has every piece")
}

/// `pose` with the elytra's own resting pose on top, vanilla's
/// animation.elytra.default: the body bone scaled up, the wings spread out
/// and back. See docs/equipment.md#elytra.
pub(crate) fn elytra_pose(pose: &Pose) -> Pose {
    pose.with(&[
        (
            "body",
            BonePose {
                scale: [1.067, 1.067, 1.067],
                scaled: true,
                ..Default::default()
            },
        ),
        (
            "left_wing",
            BonePose {
                position: [4.5, 4.0, -2.0],
                rotation: [15.0, 0.0, -13.0],
                scale: [1.0, 1.0, 2.0],
                scaled: true,
            },
        ),
        (
            "right_wing",
            BonePose {
                position: [-4.5, 4.0, -2.0],
                rotation: [15.0, 0.0, 13.0],
                scale: [1.0, 1.0, 2.0],
                scaled: true,
            },
        ),
    ])
}

/// How far back a cape hangs over a chestplate, in model units. The
/// chestplate's body is the body grown by 1.01 on every side, so a cape left
/// where it rests on the back is drawn inside it. Java Edition moves the cape
/// back by the same amount when a chestplate is worn. See
/// docs/equipment.md#capes-over-a-chestplate.
const CHESTPLATE_CAPE_OFFSET: f64 = 1.1;

/// `pose` with the cape moved back clear of a chestplate.
pub(crate) fn chestplate_cape_pose(pose: &Pose) -> Pose {
    pose.with(&[(
        "cape",
        BonePose {
            position: [0.0, 0.0, CHESTPLATE_CAPE_OFFSET],
            ..Default::default()
        },
    )])
}

/// Resizes the figure or any of its bones. The default changes nothing. A
/// held item has its own scale, in [`ItemAdjust`]. See
/// docs/equipment.md#scale.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scale {
    /// The figure's size in the image: 2 draws it twice as large, cropping
    /// what no longer fits; 0.5 half as large. Zero means 1. The camera
    /// frames the model whatever its size, so only this changes how big it
    /// looks.
    pub model: f64,
    /// Scales bones by name, ignoring case, each about its own pivot and
    /// carrying everything parented under it: the armor on it, and an arm's
    /// held item. 0 hides a bone.
    pub parts: BTreeMap<String, f64>,
}

impl Scale {
    /// `pose` with [`Scale::parts`] applied.
    pub(crate) fn parts_pose(&self, pose: &Pose) -> Pose {
        if self.parts.is_empty() {
            return pose.clone();
        }
        let extra: Vec<(&str, BonePose)> = self
            .parts
            .iter()
            .map(|(name, &k)| {
                (
                    name.as_str(),
                    BonePose {
                        scale: [k, k, k],
                        scaled: true,
                        ..Default::default()
                    },
                )
            })
            .collect();
        pose.with(&extra)
    }
}

/// An item held in one hand. The default holds nothing. See
/// docs/equipment.md#held-items.
#[derive(Clone, Copy, Debug, Default)]
pub struct Held<'a> {
    /// The item's sprite, e.g. `textures/items/diamond_sword.png`, drawn
    /// extruded and placed where the game places it, with the arm held
    /// forward. None holds nothing; nor does geometry without the arm.
    pub item: Option<&'a RgbaImage>,
    /// Holds it as the game holds an item that is not a tool or weapon -
    /// food, materials. False holds it upright, as a sword.
    pub flat: bool,
    /// Moves the item from where the game puts it, for an item that
    /// placement does not suit.
    pub adjust: ItemAdjust,
}

impl<'a> Held<'a> {
    /// `item` held upright, unadjusted.
    pub fn new(item: &'a RgbaImage) -> Held<'a> {
        Held {
            item: Some(item),
            ..Default::default()
        }
    }

    /// Item space to model units relative to the grip, in the geometry's
    /// frame: the hand's display, X mirrored and scaled by 16, then the
    /// caller's adjustment.
    pub(crate) fn to_model(self, side: &Hand) -> Mat4 {
        let display = if self.flat { &side.flat } else { &side.tool };
        self.adjust
            .matrix()
            .mul(&scale(-16.0, 16.0, 16.0))
            .mul(display)
    }
}

/// Moves a held item from the game's placement, about its grip and in the
/// hand's frame, so it follows the arm. The default moves nothing. See
/// docs/equipment.md#adjusting-an-item.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemAdjust {
    /// Moves the item, in model units along the model's axes, as a bone's
    /// position moves a bone.
    pub offset: [f64; 3],
    /// Turns the item about its grip, in degrees, as a bone's rotation turns
    /// a bone: a positive X tips its top forward.
    pub rotation: [f64; 3],
    /// Resizes the item about its grip. Zero means 1.
    pub scale: f64,
}

impl ItemAdjust {
    /// The adjustment as a transform about the origin: scale, then rotation,
    /// then offset.
    fn matrix(&self) -> Mat4 {
        let k = if self.scale == 0.0 { 1.0 } else { self.scale };
        translate(self.offset[0], self.offset[1], self.offset[2])
            .mul(&rotation_matrix(&self.rotation))
            .mul(&scale(k, k, k))
    }
}

/// What differs between the hands: the bone names, where a model without an
/// item bone grips, and the game's display transforms.
pub(crate) struct Hand {
    arm: &'static str,
    item: &'static str,
    /// The item's own bone; no skin uses the name.
    pub bone: &'static str,
    grip_x: f64,
    /// Item space (blocks, the sprite's longer side one block) to the hand's
    /// frame (blocks), as standard right-handed matrices: tools and weapons
    /// upright, anything else flat.
    tool: Mat4,
    flat: Mat4,
}

/// The right hand and the left, in that order. The left is the game's off
/// hand: not a mirror of the right, but its own offset.
pub(crate) fn hands() -> &'static [Hand; 2] {
    static HANDS: OnceLock<[Hand; 2]> = OnceLock::new();
    HANDS.get_or_init(|| {
        [
            Hand {
                arm: "rightArm",
                item: "rightItem",
                bone: "bedrockskin:right_item",
                grip_x: -1.0,
                tool: mul(&[
                    scale(-1.0, -1.0, 1.0),
                    rot_y(180.0),
                    translate(0.1, 0.265, 0.0),
                    scale(0.625, 0.625, 0.625),
                    rot_x(80.0),
                    rot_y(45.0),
                    sprite_item_transform(),
                ]),
                flat: mul(&[
                    scale(-1.0, -1.0, 1.0),
                    translate(0.3125, 0.1875, -0.1875),
                    scale(0.375, 0.375, 0.375),
                    rot_z(60.0),
                    rot_x(-90.0),
                    rot_z(20.0),
                    sprite_item_transform(),
                ]),
            },
            Hand {
                arm: "leftArm",
                item: "leftItem",
                bone: "bedrockskin:left_item",
                grip_x: 1.0,
                tool: mul(&[
                    scale(-1.0, -1.0, 1.0),
                    translate(-0.125, 0.0, 0.0),
                    rot_y(180.0),
                    translate(0.0, 0.265, 0.0),
                    scale(0.625, 0.625, 0.625),
                    rot_x(80.0),
                    rot_y(45.0),
                    sprite_item_transform(),
                ]),
                flat: mul(&[
                    scale(-1.0, -1.0, 1.0),
                    translate(-0.125, 0.0, 0.0),
                    translate(0.3125, 0.1875, -0.1875),
                    scale(0.375, 0.375, 0.375),
                    rot_z(60.0),
                    rot_x(-90.0),
                    rot_z(20.0),
                    sprite_item_transform(),
                ]),
            },
        ]
    })
}

/// The game's legacy item transform, applied before either display.
fn sprite_item_transform() -> Mat4 {
    mul(&[
        scale(1.5, 1.5, 1.5),
        rot_y(50.0),
        rot_z(335.0),
        translate(0.075, -0.245, -0.1),
    ])
}

/// The product of `ms`, left to right.
fn mul(ms: &[Mat4]) -> Mat4 {
    ms[1..].iter().fold(ms[0], |out, m| out.mul(m))
}

fn translate(x: f64, y: f64, z: f64) -> Mat4 {
    Mat4([
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, y],
        [0.0, 0.0, 1.0, z],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn scale(x: f64, y: f64, z: f64) -> Mat4 {
    Mat4([
        [x, 0.0, 0.0, 0.0],
        [0.0, y, 0.0, 0.0],
        [0.0, 0.0, z, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;

/// The standard right-handed rotations, in degrees - not [`Mat4::rotate`],
/// which turns the other way (see [`rotation_matrix`]).
fn rot_x(deg: f64) -> Mat4 {
    let (s, c) = (gomath::sin(deg * DEG_TO_RAD), gomath::cos(deg * DEG_TO_RAD));
    Mat4([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, c, -s, 0.0],
        [0.0, s, c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn rot_y(deg: f64) -> Mat4 {
    let (s, c) = (gomath::sin(deg * DEG_TO_RAD), gomath::cos(deg * DEG_TO_RAD));
    Mat4([
        [c, 0.0, s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [-s, 0.0, c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn rot_z(deg: f64) -> Mat4 {
    let (s, c) = (gomath::sin(deg * DEG_TO_RAD), gomath::cos(deg * DEG_TO_RAD));
    Mat4([
        [c, -s, 0.0, 0.0],
        [s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

impl Hand {
    /// `pose` with the hand's arm held out as vanilla's
    /// animation.player.holding holds it: the arm's X turn becomes
    /// this*0.5 - 18, half its swing and 18 degrees forward. `arm` is the
    /// model's name for it.
    pub(crate) fn holding_pose(&self, pose: &Pose, arm: &str) -> Pose {
        let x = pose.of(arm).rotation[0];
        pose.with(&[(
            self.arm,
            BonePose {
                rotation: [-x * 0.5 - 18.0, 0.0, 0.0],
                ..Default::default()
            },
        )])
    }

    /// `geo`'s skeleton with no cubes, plus a bone at the hand's grip for the
    /// item: the model's item bone, else where the standard arm's would be.
    /// Returns it with the model's name for the arm; None when `geo` has no
    /// such arm.
    pub(crate) fn skeleton(&self, geo: &Geometry) -> Option<(Geometry, String)> {
        let by_name: HashMap<&str, &Bone> =
            geo.bones.iter().map(|b| (b.name.as_str(), b)).collect();
        let mut arm = geo.bones.iter().find(|b| same_bone(&b.name, self.arm));
        let mut grip = geo.bones.iter().find(|b| same_bone(&b.name, self.item));
        if let Some(g) = grip {
            match by_name.get(g.parent.as_str()) {
                Some(parent) => arm = Some(parent),
                None => grip = None,
            }
        }
        let arm = arm?;
        let pivot = match grip {
            Some(g) => vec![at(&g.pivot, 0), at(&g.pivot, 1), at(&g.pivot, 2)],
            None => vec![
                at(&arm.pivot, 0) + self.grip_x,
                at(&arm.pivot, 1) - 7.0,
                at(&arm.pivot, 2) + 1.0,
            ],
        };
        let mut bones: Vec<Bone> = geo
            .bones
            .iter()
            .map(|b| Bone {
                name: b.name.clone(),
                parent: b.parent.clone(),
                pivot: b.pivot.clone(),
                rotation: b.rotation.clone(),
                ..Default::default()
            })
            .collect();
        bones.push(Bone {
            name: self.bone.to_string(),
            parent: arm.name.clone(),
            pivot,
            ..Default::default()
        });
        let skel = Geometry {
            identifier: self.bone.to_string(),
            bones,
            ..Default::default()
        };
        Some((skel, arm.name.clone()))
    }
}

/// The sprite's triangles: a front and a back face over the whole sprite,
/// and an edge strip along every side of an opaque texel that has no opaque
/// neighbour there. Opaque means passing the shader's alpha test: alpha/255
/// >= 0.5, a byte of 128 or more.
pub(crate) fn build_held_item(item: &RgbaImage, to_model: &Mat4, world: &Mat4) -> Vec<Triangle> {
    let (w, h) = (item.width() as i64, item.height() as i64);
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let t = 1.0 / w.max(h) as f64;
    let vertex = |x: f64, y: f64, z: f64, u: f64, v: f64| {
        let mut p = world.mul_position(to_model.mul_position(Vec3::new(x, y, z)));
        p.x = -p.x;
        // V is pre-flipped, as in add_cube.
        Vertex::new(p, u, 1.0 - v)
    };
    let mut tris = Vec::new();
    let mut quad = |a: Vertex, b: Vertex, c: Vertex, d: Vertex| {
        tris.push(Triangle([a, b, c]));
        tris.push(Triangle([a, c, d]));
    };
    // Column c spans X from -c*t to -(c+1)*t, row r spans Y from (h-r)*t
    // down to (h-r-1)*t, and the slab runs from Z=0 back to Z=-t.
    let (x0, x1, y1) = (0.0, -(w as f64) * t, h as f64 * t);
    for z in [0.0, -t] {
        quad(
            vertex(x0, 0.0, z, 0.0, 1.0),
            vertex(x1, 0.0, z, 1.0, 1.0),
            vertex(x1, y1, z, 1.0, 0.0),
            vertex(x0, y1, z, 0.0, 0.0),
        );
    }
    let opaque = |c: i64, r: i64| {
        c >= 0 && r >= 0 && c < w && r < h && item.get_pixel(c as u32, r as u32)[3] >= 128
    };
    for r in 0..h {
        for c in 0..w {
            if !opaque(c, r) {
                continue;
            }
            let (u, v) = ((c as f64 + 0.5) / w as f64, (r as f64 + 0.5) / h as f64);
            let (left, right) = (-(c as f64) * t, -((c + 1) as f64) * t);
            let (top, bottom) = ((h - r) as f64 * t, (h - r - 1) as f64 * t);
            let mut edge = |xa: f64, ya: f64, xb: f64, yb: f64| {
                quad(
                    vertex(xa, ya, 0.0, u, v),
                    vertex(xa, ya, -t, u, v),
                    vertex(xb, yb, -t, u, v),
                    vertex(xb, yb, 0.0, u, v),
                );
            };
            if !opaque(c - 1, r) {
                edge(left, bottom, left, top);
            }
            if !opaque(c + 1, r) {
                edge(right, bottom, right, top);
            }
            if !opaque(c, r - 1) {
                edge(left, top, right, top);
            }
            if !opaque(c, r + 1) {
                edge(left, bottom, right, bottom);
            }
        }
    }
    tris
}

/// One item on its own, extruded as a held item is. Only the item is
/// required; [`ItemOptions::new`] sets the rest to their defaults. See
/// docs/equipment.md#an-item-on-its-own.
#[derive(Clone, Copy, Debug)]
pub struct ItemOptions<'a> {
    /// The item's sprite, e.g. `textures/items/diamond_sword.png`.
    pub item: &'a RgbaImage,
    /// The camera preset: [`Angle::Front`] faces the sprite, as an inventory
    /// icon; [`Angle::Iso`] turns it to show its depth. None means front.
    /// Ignored when `camera` is set.
    pub angle: Option<Angle>,
    /// An explicit camera, overriding the angle.
    pub camera: Option<Camera>,
    /// The output edge length; the image is square. Zero means
    /// [`DEFAULT_SIZE`].
    pub size: u32,
    /// Turns and resizes the item about its centre. The camera frames the
    /// item whatever its size or offset, so only the rotation changes the
    /// picture.
    pub adjust: ItemAdjust,
}

impl<'a> ItemOptions<'a> {
    /// A front-on, 512x512 render of `item`.
    pub fn new(item: &'a RgbaImage) -> Self {
        ItemOptions {
            item,
            angle: None,
            camera: None,
            size: 0,
            adjust: ItemAdjust::default(),
        }
    }
    pub fn angle(mut self, angle: Angle) -> Self {
        self.angle = Some(angle);
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
    pub fn adjust(mut self, adjust: ItemAdjust) -> Self {
        self.adjust = adjust;
        self
    }

    /// The same as [`render_item`].
    pub fn render(&self) -> Result<RgbaImage, Error> {
        render_item(self)
    }

    fn size_px(&self) -> usize {
        if self.size == 0 {
            DEFAULT_SIZE as usize
        } else {
            self.size as usize
        }
    }

    /// The item turned `spin` degrees about its upright axis, after its
    /// adjustment.
    fn triangles(&self, spin: f64) -> Result<Vec<Triangle>, Error> {
        let (w, h) = (self.item.width() as f64, self.item.height() as f64);
        if w == 0.0 || h == 0.0 {
            return Err(Error::EmptyView);
        }
        // Item space centred on the origin, so the adjustment and the spin
        // turn it about its middle; then into model units, X mirrored as a
        // held item's is.
        let t = 1.0 / gomath::max(w, h);
        let to_model = rotation_matrix(&[0.0, spin, 0.0])
            .mul(&self.adjust.matrix())
            .mul(&scale(-16.0, 16.0, 16.0))
            .mul(&translate(w * t / 2.0, -h * t / 2.0, t / 2.0));
        Ok(build_held_item(self.item, &to_model, &Mat4::identity()))
    }

    /// The framing: field of view, margin, yaw and pitch.
    fn framing(&self) -> (f64, f64, f64, f64) {
        let (mut fov, mut margin) = (35.0, 1.2);
        let (yaw, pitch);
        if let Some(cam) = self.camera {
            (yaw, pitch) = (cam.yaw, cam.pitch);
            if cam.fov > 0.0 {
                fov = cam.fov;
            }
            if cam.margin > 0.0 {
                margin = cam.margin;
            }
        } else if self.angle == Some(Angle::Iso) {
            margin *= 1.25;
            (yaw, pitch) = (ISO_YAW, ISO_PITCH);
        } else {
            (yaw, pitch) = (0.0, 0.0);
        }
        (fov, margin, yaw, pitch)
    }
}

/// Renders an item on its own: the sprite extruded one texel deep, as the
/// game draws a held item, centred and framed by the camera.
pub fn render_item(opts: &ItemOptions) -> Result<RgbaImage, Error> {
    let triangles = opts.triangles(0.0)?;
    let (fov, margin, yaw, pitch) = opts.framing();
    let (eye, center) = camera_for_yaw_pitch(&triangles, fov, margin, yaw, pitch);
    let layer = Layer {
        triangles,
        texture: opts.item,
    };
    Ok(rasterize(&[layer], eye, center, fov, opts.size_px()))
}

/// Spins an item on its own: one full turn about its upright axis each
/// loop, as a dropped item turns. See docs/equipment.md#an-item-on-its-own.
#[derive(Clone, Copy, Debug)]
pub struct ItemAnimationOptions<'a> {
    pub item: ItemOptions<'a>,
    /// How long one turn takes, in seconds. Zero means 3.
    pub duration: f64,
    /// Frames per second; zero means 20.
    pub fps: u32,
    /// How many frames to render; zero means one turn.
    pub frames: u32,
}

impl<'a> ItemAnimationOptions<'a> {
    /// One turn every 3 seconds at 20 frames a second.
    pub fn new(item: ItemOptions<'a>) -> Self {
        ItemAnimationOptions {
            item,
            duration: 0.0,
            fps: 0,
            frames: 0,
        }
    }
}

/// Renders the spinning item frame by frame. Every frame shares one camera,
/// fitted around the whole turn, so the item turns in a still frame.
pub fn render_item_frames(opts: &ItemAnimationOptions) -> Result<Vec<RgbaImage>, Error> {
    let duration = if opts.duration <= 0.0 {
        3.0
    } else {
        opts.duration
    };
    let fps = if opts.fps == 0 { 20 } else { opts.fps };
    let frames = if opts.frames == 0 {
        (duration * fps as f64).round().max(1.0) as usize
    } else {
        opts.frames as usize
    };
    let mut turns = Vec::with_capacity(frames);
    let mut sweep = Vec::new();
    for i in 0..frames {
        let spin = 360.0 * (i as f64 / fps as f64) / duration;
        let tris = opts.item.triangles(spin)?;
        sweep.extend_from_slice(&tris);
        turns.push(tris);
    }
    let (fov, margin, yaw, pitch) = opts.item.framing();
    let (eye, center) = camera_for_yaw_pitch(&sweep, fov, margin, yaw, pitch);
    Ok(turns
        .into_iter()
        .map(|triangles| {
            let layer = Layer {
                triangles,
                texture: opts.item.item,
            };
            rasterize(&[layer], eye, center, fov, opts.item.size_px())
        })
        .collect())
}

/// Renders the spinning item as a looping animated GIF, as
/// [`render_gif`](crate::render_gif) encodes one.
pub fn render_item_gif(opts: &ItemAnimationOptions) -> Result<Vec<u8>, Error> {
    let frames = render_item_frames(opts)?;
    let fps = if opts.fps == 0 { 20 } else { opts.fps };
    let mut out = Vec::new();
    encode_gif(&mut out, &frames, fps)?;
    Ok(out)
}
