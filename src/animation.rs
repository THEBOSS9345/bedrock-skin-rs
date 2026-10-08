//! Moving skins: the built-in motions, poses, and rendering frames and GIFs.
//! See docs/animation.md.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::str::FromStr;

use image::RgbaImage;

use crate::raster::Vec3;
use crate::render::{Camera, RenderOptions, Scene, bounding_box, camera_for_bounds, rasterize};
use crate::{Error, gomath};

/// How one bone moves from where its geometry puts it: `rotation` is added
/// to the bone's own rotation (degrees, the geometry's convention),
/// `position` to its offset from its parent (model units), and, when
/// `scaled`, `scale` multiplies it (about its pivot, carrying its children;
/// 0 hides it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BonePose {
    pub rotation: [f64; 3],
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub scaled: bool,
}

/// A [`BonePose`] for each bone it moves, by name. Bones not in it stay at
/// rest. A name matches a bone exactly, or failing that by its lower-case
/// form, as animation files are matched in game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    bones: HashMap<String, BonePose>,
}

impl Pose {
    /// An empty pose: every bone at rest.
    pub fn new() -> Pose {
        Pose::default()
    }

    /// Sets how a bone moves.
    pub fn insert(&mut self, bone: impl Into<String>, pose: BonePose) {
        self.bones.insert(bone.into(), pose);
    }

    /// The pose for a bone: the exact name, else the bone's name lower-cased,
    /// else any name equal to it ignoring ASCII case - the built-in motions
    /// say "leftArm" where persona models name the bone "leftarm". When
    /// several names match that way the smallest wins.
    /// See docs/geometry-format.md#bone-names-ignore-case.
    pub fn of(&self, bone: &str) -> BonePose {
        if let Some(bp) = self.bones.get(bone) {
            return *bp;
        }
        if let Some(bp) = self.bones.get(&bone.to_lowercase()) {
            return *bp;
        }
        self.bones
            .iter()
            .filter(|(name, _)| crate::render::same_bone(name, bone))
            .min_by(|a, b| a.0.cmp(b.0))
            .map(|(_, bp)| *bp)
            .unwrap_or_default()
    }

    /// Every entry, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &BonePose)> {
        self.bones.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.bones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bones.is_empty()
    }

    /// A copy with each of `extra` applied after the pose this already gives
    /// that bone. A bone's entry is found as [`Pose::of`] finds it and stored
    /// under the name `extra` uses, so no other spelling of the name shadows
    /// it. `extra` is applied in name order.
    pub(crate) fn with(&self, extra: &[(&str, BonePose)]) -> Pose {
        let mut out = self.clone();
        let mut extra = extra.to_vec();
        extra.sort_by(|a, b| a.0.cmp(b.0));
        for (name, q) in extra {
            let bp = out.of(name).then(&q);
            out.bones
                .retain(|other, _| !crate::render::same_bone(other, name));
            out.bones.insert(name.to_string(), bp);
        }
        out
    }
}

impl BonePose {
    /// This with `q` applied after it: rotations and positions add, scales
    /// multiply.
    pub(crate) fn then(&self, q: &BonePose) -> BonePose {
        let (p, r) = (self, q);
        BonePose {
            rotation: [
                p.rotation[0] + r.rotation[0],
                p.rotation[1] + r.rotation[1],
                p.rotation[2] + r.rotation[2],
            ],
            position: [
                p.position[0] + r.position[0],
                p.position[1] + r.position[1],
                p.position[2] + r.position[2],
            ],
            scale: match (p.scaled, r.scaled) {
                (true, true) => [
                    p.scale[0] * r.scale[0],
                    p.scale[1] * r.scale[1],
                    p.scale[2] * r.scale[2],
                ],
                (false, true) => r.scale,
                _ => p.scale,
            },
            scaled: p.scaled || r.scaled,
        }
    }
}

impl<S: Into<String>> FromIterator<(S, BonePose)> for Pose {
    fn from_iter<I: IntoIterator<Item = (S, BonePose)>>(iter: I) -> Pose {
        Pose {
            bones: iter.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }
}

/// Anything that poses a model over time: a built-in [`Motion`], or an
/// [`Animation`](crate::Animation) loaded from a Bedrock animation file.
pub trait Animator {
    /// The length of one loop, in seconds.
    fn duration(&self) -> f64;
    /// The pose `t` seconds in.
    fn pose(&self, t: f64) -> Pose;
}

/// One of the built-in player animations: Minecraft's own movements,
/// recreated as poses over time. They move bones by their standard names
/// (head, body, rightArm, leftArm, rightLeg, leftLeg), so they work on any
/// model that uses them, custom ones included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// Arms and legs swing, each leg opposite its arm.
    Walk,
    /// The gentle breathing sway of an idle player.
    Idle,
    /// The right arm raised, waving.
    Wave,
    /// Leaning forward, creeping.
    Sneak,
}

impl Motion {
    /// Every built-in motion.
    pub const ALL: [Motion; 4] = [Motion::Walk, Motion::Idle, Motion::Wave, Motion::Sneak];

    /// The motion's name: `walk`, `idle`, `wave` or `sneak`.
    pub fn name(self) -> &'static str {
        match self {
            Motion::Walk => "walk",
            Motion::Idle => "idle",
            Motion::Wave => "wave",
            Motion::Sneak => "sneak",
        }
    }
}

impl fmt::Display for Motion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Motion {
    type Err = Error;

    /// The motion named `s`, exactly.
    fn from_str(s: &str) -> Result<Motion, Error> {
        Motion::ALL
            .into_iter()
            .find(|m| m.name() == s)
            .ok_or_else(|| Error::UnknownMotion(s.to_string()))
    }
}

/// The motion named `s`; the same as `s.parse::<Motion>()`.
pub fn parse_motion(s: &str) -> Result<Motion, Error> {
    s.parse()
}

impl Animator for Motion {
    fn duration(&self) -> f64 {
        match self {
            Motion::Idle => 4.0,
            Motion::Sneak => 1.6,
            _ => 1.0,
        }
    }

    /// Rotations follow the geometry's convention, in which a positive X
    /// turn tips a bone's top toward the front: a limb hanging from its
    /// shoulder or hip then swings backward, so swinging it forward is a
    /// negative X. Raising an arm out to its side is a positive Z for the
    /// right arm, negative for the left.
    fn pose(&self, t: f64) -> Pose {
        let phase = 2.0 * std::f64::consts::PI * t / self.duration();
        let rot = |r: [f64; 3]| BonePose {
            rotation: r,
            ..BonePose::default()
        };
        let forward = |deg: f64| rot([-deg, 0.0, 0.0]);
        match self {
            Motion::Walk => {
                // As animation.player.move.arms/legs: each arm opposite its
                // leg, the legs swinging 1.4 times as far.
                let arm = 40.0 * gomath::sin(phase);
                let leg = 56.0 * gomath::sin(phase);
                Pose::from_iter([
                    ("rightArm", forward(arm)),
                    ("leftArm", forward(-arm)),
                    ("rightLeg", forward(-leg)),
                    ("leftLeg", forward(leg)),
                ])
            }
            Motion::Idle => {
                // Minecraft's idle bob: the arms drift out from the body and
                // back, up to 5.7 degrees.
                let out = 2.865 + 2.865 * gomath::cos(phase);
                Pose::from_iter([
                    ("rightArm", rot([0.0, 0.0, out])),
                    ("leftArm", rot([0.0, 0.0, -out])),
                ])
            }
            Motion::Wave => Pose::from_iter([(
                "rightArm",
                rot([-10.0, 0.0, 150.0 + 20.0 * gomath::sin(phase)]),
            )]),
            Motion::Sneak => {
                // Minecraft's own sneak: the whole model leans forward from
                // the feet (root), set back to keep it balanced; the legs
                // turn back against the lean so they stay upright, and the
                // body and head drop. The legs also creep a short step.
                let step = 12.0 * gomath::sin(phase);
                let moved = |p: [f64; 3]| BonePose {
                    position: p,
                    ..BonePose::default()
                };
                Pose::from_iter([
                    (
                        "root",
                        BonePose {
                            rotation: [28.0, 0.0, 0.0],
                            position: [0.0, 1.25, 9.0],
                            ..BonePose::default()
                        },
                    ),
                    ("body", moved([0.0, -2.0, 0.0])),
                    ("head", moved([0.0, -1.0, 0.0])),
                    ("rightArm", rot([-5.7, 0.0, 0.0])),
                    ("leftArm", rot([-5.7, 0.0, 0.0])),
                    ("rightLeg", rot([-28.0 - step, 0.1, 0.1])),
                    ("leftLeg", rot([-28.0 + step, -0.1, -0.1])),
                ])
            }
        }
    }
}

/// What [`render_frames`] and [`render_gif`] draw: everything in
/// [`RenderOptions`] (its pose is ignored), plus the animation and its
/// timing.
#[derive(Clone)]
pub struct AnimationOptions<'a> {
    pub options: RenderOptions<'a>,
    /// What moves the model: a built-in [`Motion`], or an
    /// [`Animation`](crate::Animation) from a file.
    pub animation: &'a dyn Animator,
    /// Frames per second; zero means 20.
    pub fps: u32,
    /// How many frames to render; zero means one loop.
    pub frames: u32,
    /// How many frames are rasterized at once; zero means every core, one
    /// means one at a time. Frames are independent, so the images are the
    /// same either way; a server already rendering on every core may want 1.
    /// See docs/design-decisions.md#why-animation-frames-render-in-parallel.
    pub workers: usize,
}

impl<'a> AnimationOptions<'a> {
    pub fn new(options: RenderOptions<'a>, animation: &'a dyn Animator) -> Self {
        AnimationOptions {
            options,
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

    fn timing(&self) -> (u32, usize) {
        let fps = if self.fps == 0 { 20 } else { self.fps };
        let frames = if self.frames == 0 {
            (self.animation.duration() * fps as f64).round().max(1.0) as usize
        } else {
            self.frames as usize
        };
        (fps, frames)
    }

    /// The same as [`render_frames`].
    pub fn render_frames(&self) -> Result<Vec<RgbaImage>, Error> {
        render_frames(self)
    }

    /// The same as [`render_gif`].
    pub fn render_gif(&self) -> Result<Vec<u8>, Error> {
        render_gif(self)
    }

    /// The same as [`write_gif`].
    pub fn write_gif<W: Write>(&self, w: W) -> Result<(), Error> {
        write_gif(w, self)
    }
}

/// An animation prepared once: its per-frame scenes and the bounding box they
/// share, kept so a viewer can draw frames one at a time as its own camera
/// moves. [`render_frames`] builds the same thing and draws every frame;
/// `Frames` keeps it, so one frame costs one rasterization and every frame at
/// one camera is framed the same way - root and whole-body motion stay on
/// screen instead of the camera chasing each pose. A viewer that turns the
/// model as it plays draws one frame a tick this way. See docs/animation.md.
pub struct Frames<'a> {
    scenes: Vec<Scene<'a>>,
    frames: usize,
    lo: Vec3,
    hi: Vec3,
    scale: f64,
    flat: Option<RgbaImage>,
}

/// Builds every frame of an animation, and the one camera they share, without
/// rasterizing anything. It is [`render_frames`] split in two; draw the result
/// with [`Frames::draw`].
pub fn prepare_frames<'a>(opts: &AnimationOptions<'a>) -> Result<Frames<'a>, Error> {
    let (fps, frames) = opts.timing();
    let mut scenes = Vec::with_capacity(frames);
    let mut sweep = Vec::new();
    for i in 0..frames {
        let sc = opts
            .options
            .scene(&opts.animation.pose(i as f64 / fps as f64))?;
        if let Some(flat) = sc.flat {
            // Geometry that draws nothing has nothing to move: every frame is
            // the flat crop.
            return Ok(Frames {
                scenes: Vec::new(),
                frames,
                lo: Vec3::default(),
                hi: Vec3::default(),
                scale: opts.options.scale.model,
                flat: Some(flat),
            });
        }
        sweep.extend(sc.framing());
        scenes.push(sc);
    }
    let (lo, hi) = bounding_box(&sweep);
    Ok(Frames {
        scenes,
        frames,
        lo,
        hi,
        scale: opts.options.scale.model,
        flat: None,
    })
}

impl Frames<'_> {
    /// How many frames the animation has.
    pub fn len(&self) -> usize {
        self.frames
    }

    /// Whether the animation has no frames. Never true for a prepared set.
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    /// Rasterizes frame `i` at a `size`-square image, using the camera the
    /// frames were prepared with unless `cam` is set, when it refits the shared
    /// framing to `cam`. Refitting is what turns a draw into an orbit: every
    /// frame at one camera still shares a single framing. `i` wraps into range;
    /// a `size` of 0 means the prepared size. A persona skin's flat crop is
    /// returned as prepared, whatever size was asked for.
    pub fn draw(&self, i: usize, size: usize, cam: Option<Camera>) -> RgbaImage {
        if let Some(flat) = &self.flat {
            return flat.clone();
        }
        let sc = &self.scenes[0];
        let (mut fov, mut margin, mut yaw, mut pitch) = (sc.fov, sc.margin, sc.yaw, sc.pitch);
        if let Some(cam) = cam {
            if cam.fov > 0.0 {
                fov = cam.fov;
            }
            if cam.margin > 0.0 {
                margin = cam.margin;
            }
            if self.scale > 0.0 {
                // scene() divides a camera's margin by Scale.Model, so a
                // refit has to as well or a scaled model frames differently.
                margin /= self.scale;
            }
            yaw = cam.yaw;
            pitch = cam.pitch;
        }
        let size = if size == 0 { sc.size } else { size };
        let i = i % self.scenes.len();
        let (eye, center) = camera_for_bounds(self.lo, self.hi, fov, margin, yaw, pitch);
        rasterize(&self.scenes[i].layers, eye, center, fov, size)
    }

    /// Rasterizes every frame with the shared camera - the batch
    /// [`render_frames`] uses.
    fn all(&self, workers: usize) -> Vec<RgbaImage> {
        if let Some(flat) = &self.flat {
            return vec![flat.clone(); self.frames];
        }
        let sc = &self.scenes[0];
        let (eye, center) =
            camera_for_bounds(self.lo, self.hi, sc.fov, sc.margin, sc.yaw, sc.pitch);
        let workers = match workers {
            0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
            n => n,
        }
        .min(self.scenes.len());
        let draw = |sc: &Scene| rasterize(&sc.layers, eye, center, sc.fov, sc.size);
        if workers <= 1 {
            return self.scenes.iter().map(draw).collect();
        }
        // Each frame has its own buffers; the threads share only the read-only
        // scenes and textures, and every frame goes back to its own slot.
        let mut out: Vec<Option<RgbaImage>> = vec![None; self.scenes.len()];
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|w| {
                    let (scenes, draw) = (&self.scenes, &draw);
                    s.spawn(move || {
                        (w..scenes.len())
                            .step_by(workers)
                            .map(|i| (i, draw(&scenes[i])))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for h in handles {
                for (i, img) in h.join().expect("a frame worker panicked") {
                    out[i] = Some(img);
                }
            }
        });
        out.into_iter()
            .map(|f| f.expect("every frame drawn"))
            .collect()
    }
}

/// Renders the animation frame by frame. Every frame shares one camera,
/// fitted around the whole sweep, so the model moves within a still frame
/// rather than the frame chasing it.
pub fn render_frames(opts: &AnimationOptions) -> Result<Vec<RgbaImage>, Error> {
    Ok(prepare_frames(opts)?.all(opts.workers))
}

/// Renders the animation as a looping animated GIF. GIF holds 256 colours a
/// frame, so the frames share a palette of the colours they use most (exact
/// for most skins, which use fewer), and transparency is on or off per
/// pixel, as the renderer's alpha test already makes it.
pub fn render_gif(opts: &AnimationOptions) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write_gif(&mut out, opts)?;
    Ok(out)
}

/// Renders the animation and writes the GIF to `w` - an HTTP response, a
/// file - without holding the encoded bytes first. It writes the same bytes
/// [`render_gif`] returns.
///
/// ```
/// use bedrock_skin::{AnimationOptions, Motion, RenderOptions};
/// let texture = image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 140, 200, 255]));
/// let mut out = Vec::new(); // or a File, or a response body
/// AnimationOptions::new(RenderOptions::new(&texture).size(64), &Motion::Walk)
///     .fps(10)
///     .write_gif(&mut out)?;
/// assert!(out.starts_with(b"GIF89a"));
/// # Ok::<(), bedrock_skin::Error>(())
/// ```
pub fn write_gif<W: Write>(w: W, opts: &AnimationOptions) -> Result<(), Error> {
    let frames = render_frames(opts)?;
    let (fps, _) = opts.timing();
    encode_gif(w, &frames, fps)
}

/// Writes `frames` as a looping GIF at `fps` frames a second.
pub(crate) fn encode_gif<W: Write>(w: W, frames: &[RgbaImage], fps: u32) -> Result<(), Error> {
    let palette = gif_palette(frames);
    let delay = ((100.0 / fps as f64).round() as u16).max(2); // hundredths of a second
    let out = w;
    let (w, h) = frames[0].dimensions();
    let mut flat = Vec::with_capacity(palette.len() * 3);
    for c in &palette {
        flat.extend_from_slice(&c[..3]);
    }
    {
        let mut enc = gif::Encoder::new(out, w as u16, h as u16, &flat).map_err(gif_error)?;
        if frames.len() > 1 {
            enc.set_repeat(gif::Repeat::Infinite).map_err(gif_error)?;
        }
        let mut index_of: HashMap<[u8; 3], u8> = HashMap::new();
        for f in frames {
            let mut buf = Vec::with_capacity((w * h) as usize);
            for p in f.pixels() {
                let [r, g, b, a] = p.0;
                if a < 128 {
                    buf.push(0);
                    continue;
                }
                let i = *index_of
                    .entry([r, g, b])
                    .or_insert_with(|| palette_index(&palette, [r, g, b, 255]) as u8);
                buf.push(i);
            }
            let mut frame = gif::Frame::from_indexed_pixels(w as u16, h as u16, buf, Some(0));
            frame.delay = delay;
            frame.dispose = gif::DisposalMethod::Background;
            enc.write_frame(&frame).map_err(gif_error)?;
        }
    }
    Ok(())
}

fn gif_error(e: gif::EncodingError) -> Error {
    Error::Encode(e.to_string())
}

/// Index 0 transparent, then up to 255 opaque colours: every colour the
/// frames use when they fit, otherwise the most used, each of the rest drawn
/// as its nearest.
fn gif_palette(frames: &[RgbaImage]) -> Vec<[u8; 4]> {
    let mut count: HashMap<[u8; 3], usize> = HashMap::new();
    for f in frames {
        for p in f.pixels() {
            let [r, g, b, a] = p.0;
            if a >= 128 {
                *count.entry([r, g, b]).or_default() += 1;
            }
        }
    }
    let mut cols: Vec<[u8; 3]> = count.keys().copied().collect();
    let key = |c: &[u8; 3]| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
    cols.sort_by(|a, b| count[b].cmp(&count[a]).then(key(a).cmp(&key(b))));
    let mut pal = vec![[0, 0, 0, 0]];
    for c in cols.into_iter().take(255) {
        pal.push([c[0], c[1], c[2], 255]);
    }
    if pal.len() == 1 {
        pal.push([0, 0, 0, 255]);
    }
    pal
}

/// Go's color.Palette.Index: the nearest entry by squared distance of the
/// 16-bit premultiplied channels, as its wrapping uint32 arithmetic has it.
fn palette_index(pal: &[[u8; 4]], c: [u8; 4]) -> usize {
    fn rgba(c: [u8; 4]) -> [u32; 4] {
        let a = c[3] as u32;
        let ch = |v: u8| ((v as u32 | (v as u32) << 8) * a) / 0xff;
        [ch(c[0]), ch(c[1]), ch(c[2]), a | a << 8]
    }
    fn sq_diff(x: u32, y: u32) -> u32 {
        let d = x.wrapping_sub(y);
        d.wrapping_mul(d) >> 2
    }
    let cc = rgba(c);
    let (mut ret, mut best) = (0, u32::MAX);
    for (i, v) in pal.iter().enumerate() {
        let vv = rgba(*v);
        let sum = sq_diff(cc[0], vv[0])
            .wrapping_add(sq_diff(cc[1], vv[1]))
            .wrapping_add(sq_diff(cc[2], vv[2]))
            .wrapping_add(sq_diff(cc[3], vv[3]));
        if sum < best {
            if sum == 0 {
                return i;
            }
            ret = i;
            best = sum;
        }
    }
    ret
}
