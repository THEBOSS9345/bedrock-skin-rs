//! Bedrock animation files - what Blockbench exports and Minecraft's own
//! resource packs use. See docs/animation.md#animation-files.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde_json::Value;

use crate::Error;
use crate::animation::{Animator, BonePose, Pose};
use crate::geometry::Geometry;
use crate::molang::{Env, Molang};

/// One animation from a Bedrock animation file.
#[derive(Clone, Debug)]
pub struct Animation {
    /// Its key in the file, e.g. `animation.player.wave`.
    pub name: String,
    /// Whether it starts over at the end; `hold_on_last_frame` keeps the last
    /// pose instead (as does not looping, once it has finished).
    pub looping: bool,
    pub hold_on_last_frame: bool,
    /// How long it runs, in seconds: `animation_length`, or the last
    /// keyframe's time when the file leaves it out.
    pub length: f64,

    time_update: Option<Molang>,
    /// By lower-case bone name, so they match the geometry's bones
    /// case-insensitively, as in game.
    bones: BTreeMap<String, BoneAnimation>,
    bone_names: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct BoneAnimation {
    rotation: Option<Channel>,
    position: Option<Channel>,
    scale: Option<Channel>,
}

/// One of a bone's rotation, position or scale: a value for all time (an
/// expression per axis), or keyframes.
#[derive(Clone, Debug)]
enum Channel {
    Always([Molang; 3]),
    Keys(Vec<Keyframe>),
}

#[derive(Clone, Debug)]
struct Keyframe {
    t: f64,
    pre: [Molang; 3],
    post: [Molang; 3],
    lerp: String, // "linear", "catmullrom" or "step"
}

impl Animation {
    /// The bones the animation moves, as the file names them, sorted.
    pub fn bones(&self) -> &[String] {
        &self.bone_names
    }

    /// The bones the animation moves that `g` does not have, so those parts
    /// of it will do nothing on that model - typically an animation made for
    /// another entity (a wing, a tail). Names match case-insensitively, as
    /// in game. Empty means every part of it applies.
    pub fn missing_bones(&self, g: &Geometry) -> Vec<String> {
        let have: std::collections::HashSet<String> =
            g.bones.iter().map(|b| b.name.to_lowercase()).collect();
        self.bone_names
            .iter()
            .filter(|n| !have.contains(&n.to_lowercase()))
            .cloned()
            .collect()
    }
}

fn err(msg: impl Into<String>) -> Error {
    Error::Animation(msg.into())
}

/// Reads a Bedrock animation file, returning its animations by name. Every
/// Molang expression in it is compiled, so a syntax error is reported here,
/// naming the animation, bone and channel it is in; names an expression
/// reads that this library does not model are simply 0 when it runs.
pub fn parse_animations(raw: &[u8]) -> Result<BTreeMap<String, Animation>, Error> {
    let doc: Value =
        serde_json::from_slice(raw).map_err(|e| err(format!("animation file: {e}")))?;
    let anims = match field(&doc, "animations") {
        Some(Value::Object(map)) => map,
        Some(Value::Null) | None if doc.is_object() || doc.is_null() => {
            return Err(Error::NoAnimations);
        }
        _ => return Err(err("animation file: \"animations\" is not an object")),
    };
    if anims.is_empty() {
        return Err(Error::NoAnimations);
    }
    let mut out = BTreeMap::new();
    let mut names: Vec<&String> = anims.keys().collect();
    names.sort();
    for name in names {
        out.insert(name.clone(), parse_animation(name, &anims[name])?);
    }
    Ok(out)
}

/// An object's field as Go's decoder finds it: the last key equal to the
/// name ignoring case.
fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v.as_object()?
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
        .next_back()
}

fn parse_animation(name: &str, raw: &Value) -> Result<Animation, Error> {
    if !(raw.is_object() || raw.is_null()) {
        return Err(err(format!("animation {name}: not an object")));
    }
    let mut a = Animation {
        name: name.to_string(),
        looping: false,
        hold_on_last_frame: false,
        length: 0.0,
        time_update: None,
        bones: BTreeMap::new(),
        bone_names: Vec::new(),
    };
    match field(raw, "loop") {
        Some(Value::Bool(true)) => a.looping = true,
        Some(Value::String(s)) if s == "hold_on_last_frame" => a.hold_on_last_frame = true,
        _ => {}
    }
    let length = match field(raw, "animation_length") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => n.as_f64(),
        Some(_) => {
            return Err(err(format!(
                "animation {name}: animation_length is not a number"
            )));
        }
    };
    if let Some(tu) = field(raw, "anim_time_update") {
        let m = molang_value(tu)
            .map_err(|e| err(format!("animation {name}: anim_time_update: {e}")))?;
        a.time_update = Some(m);
    }
    let mut last = 0.0f64;
    let bones = match field(raw, "bones") {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(map),
        Some(_) => return Err(err(format!("animation {name}: bones is not an object"))),
    };
    for (bone, raw_bone) in bones.into_iter().flatten() {
        let chans = match raw_bone {
            Value::Object(map) => Some(map),
            Value::Null => None,
            _ => return Err(err(format!("animation {name}, bone {bone}: not an object"))),
        };
        let mut ba = BoneAnimation::default();
        for (ch_name, raw_ch) in chans.into_iter().flatten() {
            let ch = parse_channel(raw_ch)
                .map_err(|e| err(format!("animation {name}, bone {bone}, {ch_name}: {e}")))?;
            if let Channel::Keys(keys) = &ch
                && let Some(k) = keys.last()
            {
                last = crate::gomath::max(last, k.t);
            }
            match ch_name.to_lowercase().as_str() {
                "rotation" => ba.rotation = Some(ch),
                "position" => ba.position = Some(ch),
                "scale" => ba.scale = Some(ch),
                _ => {}
            }
        }
        a.bones.insert(bone.to_lowercase(), ba);
        a.bone_names.push(bone.clone());
    }
    a.bone_names.sort();
    a.length = last;
    if let Some(l) = length
        && l > 0.0
    {
        a.length = l;
    }
    Ok(a)
}

/// A channel's value: a number, a string expression, an array of them, or
/// an object of keyframes by time.
fn parse_channel(raw: &Value) -> Result<Channel, String> {
    if let Value::Object(frames) = raw {
        let mut keys = Vec::new();
        for (ts, raw_key) in frames {
            let t: f64 = ts
                .trim()
                .parse()
                .map_err(|_| format!("keyframe time {ts:?} is not a number"))?;
            keys.push(parse_keyframe(t, raw_key).map_err(|e| format!("keyframe {ts}: {e}"))?);
        }
        keys.sort_by(|a: &Keyframe, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        return Ok(Channel::Keys(keys));
    }
    Ok(Channel::Always(parse_vector(raw)?))
}

fn parse_keyframe(t: f64, raw: &Value) -> Result<Keyframe, String> {
    let mut k = Keyframe {
        t,
        pre: zero3(),
        post: zero3(),
        lerp: "linear".into(),
    };
    let Value::Object(_) = raw else {
        let v = parse_vector(raw)?;
        k.pre = v.clone();
        k.post = v;
        return Ok(k);
    };
    let present = |v: Option<&Value>| v.is_some();
    let pre = field(raw, "pre");
    let post = field(raw, "post");
    match field(raw, "lerp_mode") {
        Some(Value::String(s)) if !s.is_empty() => k.lerp = s.to_lowercase(),
        Some(Value::String(_)) | Some(Value::Null) | None => {}
        Some(_) => return Err("lerp_mode is not a string".into()),
    }
    match (present(pre), present(post)) {
        (true, true) => {
            k.pre = parse_vector(pre.unwrap())?;
            k.post = parse_vector(post.unwrap())?;
        }
        (false, true) => {
            let v = parse_vector(post.unwrap())?;
            k.pre = v.clone();
            k.post = v;
        }
        (true, false) => {
            let v = parse_vector(pre.unwrap())?;
            k.pre = v.clone();
            k.post = v;
        }
        (false, false) => return Err("a keyframe needs pre or post".into()),
    }
    Ok(k)
}

fn zero3() -> [Molang; 3] {
    [
        Molang::constant(0.0),
        Molang::constant(0.0),
        Molang::constant(0.0),
    ]
}

/// A value for three axes: an array of three numbers or expressions, or one
/// value for all three (a number, a string, or a one-element array).
fn parse_vector(raw: &Value) -> Result<[Molang; 3], String> {
    if let Value::Array(items) = raw {
        return match items.len() {
            1 => {
                let m = molang_value(&items[0])?;
                Ok([m.clone(), m.clone(), m])
            }
            3 => Ok([
                molang_value(&items[0])?,
                molang_value(&items[1])?,
                molang_value(&items[2])?,
            ]),
            4 => Err(
                "quaternion rotations are not supported; export Euler rotations from Blockbench"
                    .into(),
            ),
            n => Err(format!("a value has {n} parts; want 1 or 3")),
        };
    }
    let m = molang_value(raw)?;
    Ok([m.clone(), m.clone(), m])
}

/// Compiles a JSON number or string as an expression.
fn molang_value(raw: &Value) -> Result<Molang, String> {
    match raw {
        Value::Number(n) => Ok(Molang::constant(n.as_f64().unwrap_or(0.0))),
        // Go reads null into a float64 as "no change": the constant 0.
        Value::Null => Ok(Molang::constant(0.0)),
        Value::String(s) if s.trim().is_empty() => Ok(Molang::constant(0.0)),
        Value::String(s) => Molang::compile(s),
        other => Err(format!(
            "a value must be a number or a Molang string, not {other}"
        )),
    }
}

/// The blocks a second the movement queries report: about a player walking,
/// so a walk cycle driven by distance moved plays at its in-game pace.
const WALK_SPEED: f64 = 4.3;

impl Animator for Animation {
    /// Its length, or a second for an animation with no keyframes (all
    /// expressions), which has no natural end.
    fn duration(&self) -> f64 {
        if self.length > 0.0 { self.length } else { 1.0 }
    }

    /// The pose `t` seconds in. Its clock is `t`, or what `anim_time_update`
    /// makes of it, looped or held at the end as the file says.
    fn pose(&self, t: f64) -> Pose {
        let mut env = Env {
            queries: HashMap::from([
                ("life_time", t),
                ("delta_time", 1.0 / 20.0),
                ("modified_distance_moved", t * WALK_SPEED),
                ("distance_moved", t * WALK_SPEED),
                ("walk_distance", t * WALK_SPEED),
                ("ground_speed", WALK_SPEED),
                ("modified_move_speed", 1.0),
                ("anim_speed", 1.0),
                ("is_on_ground", 1.0),
                ("is_alive", 1.0),
                ("health", 20.0),
                ("max_health", 20.0),
            ]),
            ..Env::default()
        };
        let mut at = t;
        if let Some(tu) = &self.time_update {
            env.queries.insert("anim_time", t);
            at = tu.eval(&mut env);
        }
        if self.length > 0.0 {
            if self.looping {
                at %= self.length;
                if at < 0.0 {
                    at += self.length;
                }
            } else {
                // Played once, or held: the end pose stays.
                at = crate::gomath::min(at, self.length);
            }
        }
        env.queries.insert("anim_time", at);
        env.queries.insert("anim_pos", at);

        let mut pose = Pose::new();
        for (bone, ba) in &self.bones {
            let mut bp = BonePose::default();
            if let Some(c) = &ba.rotation {
                bp.rotation = c.value(at, &mut env);
            }
            if let Some(c) = &ba.position {
                bp.position = c.value(at, &mut env);
            }
            if let Some(c) = &ba.scale {
                bp.scale = c.value(at, &mut env);
                bp.scaled = true;
            }
            pose.insert(bone.clone(), bp);
        }
        pose
    }
}

fn eval3(v: &[Molang; 3], env: &mut Env) -> [f64; 3] {
    [v[0].eval(env), v[1].eval(env), v[2].eval(env)]
}

impl Channel {
    /// The channel at time `at`: its expressions, or its keyframes
    /// interpolated. Before the first keyframe its value holds, as after the
    /// last.
    fn value(&self, at: f64, env: &mut Env) -> [f64; 3] {
        let keys = match self {
            Channel::Always(v) => return eval3(v, env),
            Channel::Keys(keys) => keys,
        };
        let Some(first) = keys.first() else {
            return [0.0; 3];
        };
        if at <= first.t {
            return eval3(&first.pre, env);
        }
        let last = &keys[keys.len() - 1];
        if at >= last.t {
            return eval3(&last.post, env);
        }
        let i = keys.partition_point(|k| k.t <= at) - 1;
        let (k1, k2) = (&keys[i], &keys[i + 1]);
        let from = eval3(&k1.post, env);
        let to = eval3(&k2.pre, env);
        if k1.lerp == "step" {
            return from;
        }
        let f = (at - k1.t) / (k2.t - k1.t);
        if k1.lerp == "catmullrom" || k2.lerp == "catmullrom" {
            // Through the neighbouring keyframes; a missing one is its
            // neighbour repeated.
            let before = if i > 0 {
                eval3(&keys[i - 1].post, env)
            } else {
                from
            };
            let after = if i + 2 < keys.len() {
                eval3(&keys[i + 2].pre, env)
            } else {
                to
            };
            return std::array::from_fn(|ax| {
                catmull_rom(before[ax], from[ax], to[ax], after[ax], f)
            });
        }
        std::array::from_fn(|ax| from[ax] + (to[ax] - from[ax]) * f)
    }
}

/// The uniform Catmull-Rom spline through p1 and p2 at t in 0..1, shaped by
/// p0 and p3.
fn catmull_rom(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let t2 = t * t;
    let t3 = t * t * t;
    0.5 * (2.0 * p1
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

const EXAMPLE_FILES: [(&str, &str); 3] = [
    (
        "emotes",
        include_str!("../examples/animations/emotes.animation.json"),
    ),
    (
        "fighting",
        include_str!("../examples/animations/fighting.animation.json"),
    ),
    (
        "moves",
        include_str!("../examples/animations/moves.animation.json"),
    ),
];

/// The bundled example animations by name, e.g. `animation.player.dance` -
/// emotes, moves and fighting, made for the player model. They are the same
/// files as in examples/animations, to read or load into Blockbench.
/// See docs/animation.md#example-animations.
///
/// ```
/// use bedrock_skin::{AnimationOptions, RenderOptions, example_animations};
/// let texture = image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 140, 200, 255]));
/// let dance = &example_animations()["animation.player.dance"];
/// let frames = AnimationOptions::new(RenderOptions::new(&texture).size(48), dance)
///     .fps(5)
///     .render_frames()?;
/// assert!(frames.len() > 1);
/// # Ok::<(), bedrock_skin::Error>(())
/// ```
pub fn example_animations() -> &'static BTreeMap<String, Animation> {
    static EXAMPLES: OnceLock<BTreeMap<String, Animation>> = OnceLock::new();
    EXAMPLES.get_or_init(|| {
        let mut out = BTreeMap::new();
        for (file, raw) in EXAMPLE_FILES {
            let anims =
                parse_animations(raw.as_bytes()).unwrap_or_else(|e| panic!("bundled {file}: {e}"));
            out.extend(anims);
        }
        out
    })
}
