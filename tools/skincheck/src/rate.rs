//! Rendering every skin with both libraries, checking both, and rating what came
//! out.
//!
//! Each image is measured twice: once from the picture this crate drew, and
//! once from the measurements the Go version sent back. The same checks run on
//! both, and every problem is recorded against the side that has it - so a
//! problem only Rust has is a Rust bug, only Go has is a Go bug, and a problem
//! both have belongs to the skin.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use bedrock_skin::*;
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{GoImage, Plan, PlanSkin, hash_image, progress::Bar, read_go};

/// Which library a problem is in: `both` means it is the skin's own doing.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Both,
    Rust,
    Go,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Side::Both => "both",
            Side::Rust => "Rust",
            Side::Go => "Go",
        }
    }
}

/// One thing the checks noticed about a skin, and what it costs the score.
///
/// `kind` and `grade` are owned rather than `&'static str` so a Rating can be
/// read back from ratings.json, which is what lets `skincheck report` rewrite
/// the report without rendering anything again.
#[derive(Serialize, Deserialize, Clone)]
pub struct Issue {
    pub kind: String,
    pub detail: String,
    pub penalty: u32,
    /// Which library the problem is in. A penalty is only charged once, to the
    /// side that has the problem, so the two packages never double-count.
    pub side: Side,
}

/// A skin's result.
#[derive(Serialize, Deserialize, Clone)]
pub struct Rating {
    pub skin: PlanSkin,
    pub score: u32,
    pub grade: String,
    pub issues: Vec<Issue>,
    pub notes: Vec<String>,
    pub verdict: String,
    pub renders: usize,
    pub compared: usize,
    /// Problems only the Go version has - a Go bug, not a skin problem.
    pub go_issues: usize,
    /// Problems only this crate have.
    pub rust_issues: usize,
    /// Images the report shows: a strip of the stills and some GIFs.
    pub strip: Option<String>,
    pub gifs: Vec<(String, String)>,
}

/// What one image looks like, for the checks. Built either from a picture this
/// crate drew or from what the Go version measured, so one set of checks runs on
/// both.
#[derive(Clone)]
struct Look {
    opaque: usize,
    full: usize,
    faithful: usize,
    touches_edge: bool,
    centre_off: f64,
    w: u32,
    h: u32,
}

impl Look {
    fn of(img: &RgbaImage, colours: &HashSet<[u8; 3]>) -> Look {
        let (w, h) = img.dimensions();
        let (mut opaque, mut full, mut faithful, mut edge) = (0, 0, 0, false);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for (x, y, p) in img.enumerate_pixels() {
            let [r, g, b, a] = p.0;
            if a == 0 {
                continue;
            }
            opaque += 1;
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                edge = true;
            }
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if a == 255 {
                full += 1;
                if colours.contains(&[r, g, b]) {
                    faithful += 1;
                }
            }
        }
        Look { opaque, full, faithful, touches_edge: edge, centre_off: centre_off(x0, y0, x1, y1, w, h, opaque), w, h }
    }

    /// The same measurements, as the Go version reported them. Without them -
    /// go.tsv carries hashes only - `ours` stands in: when the hashes match the
    /// images are identical, and when they do not the skin already fails as
    /// "differs from Go"; run `go --looks` to see Go's side of it.
    fn of_go(g: &GoImage, w: u32, h: u32, ours: Option<&Look>) -> Look {
        let Some(l) = &g.l else {
            return ours.cloned().unwrap_or(Look { opaque: 0, full: 0, faithful: 0, touches_edge: false, centre_off: 0.0, w, h });
        };
        let (gw, gh) = (l.x1.max(l.x0) as u32 + 1, l.y1.max(l.y0) as u32 + 1);
        Look {
            opaque: l.o,
            full: l.f,
            faithful: l.c,
            touches_edge: l.e,
            centre_off: centre_off(l.x0 as u32, l.y0 as u32, l.x1 as u32, l.y1 as u32, gw, gh, l.o),
            w: gw.max(1),
            h: gh.max(1),
        }
    }

    fn cover(&self) -> f64 {
        let area = (self.w * self.h) as f64;
        if area == 0.0 { 0.0 } else { self.opaque as f64 / area }
    }
}

/// How far the drawn bounds sit from the middle of the picture, as a fraction of
/// its width. Zero when nothing was drawn.
fn centre_off(x0: u32, y0: u32, x1: u32, y1: u32, w: u32, h: u32, opaque: usize) -> f64 {
    if opaque == 0 {
        return 0.0;
    }
    let cx = (x0 + x1) as f64 / 2.0 - (w as f64 - 1.0) / 2.0;
    let cy = (y0 + y1) as f64 / 2.0 - (h as f64 - 1.0) / 2.0;
    (cx * cx + cy * cy).sqrt() / w as f64
}

/// The problems one rendered image has, as (kind, detail, penalty).
///
/// `view` decides which checks make sense. The close-up views are meant to fill
/// the frame - a face that leaves a margin is wrong, not a face that fills it -
/// so "cut off" and "tiny render" only apply to the whole-body views, where the
/// camera fits the model with a margin and anything spilling over the edge is a
/// framing bug.
fn assess(name: &str, view: &str, l: &Look, invisible: bool) -> Vec<(&'static str, String, u32)> {
    let mut out = Vec::new();
    let area = (l.w * l.h) as f64;
    let whole_body = view == "body";
    if l.opaque == 0 && !invisible {
        out.push(("blank render", format!("{name} is empty, but the detector says the skin is visible"), 40));
    } else if l.opaque > 0 && (l.opaque as f64) < area * 0.004 && !invisible && whole_body {
        out.push(("tiny render", format!("{name} covers only {} pixels", l.opaque), 15));
    }
    if l.touches_edge && whole_body {
        out.push(("cut off", format!("{name} touches the edge of the image"), 15));
    }
    if l.full > 50 && (l.faithful as f64) < l.full as f64 * 0.95 {
        out.push((
            "wrong colours",
            format!("{name}: only {:.1}% of pixels are colours from the skin", 100.0 * l.faithful as f64 / l.full as f64),
            30,
        ));
    }
    out
}

type Problem = (&'static str, String, u32);

/// Record the problems of one render as measured on both sides.
///
/// A kind both sides report is the skin's own fault and is charged once, as
/// `Side::Both`. A kind only one side reports is that library's bug, and is
/// charged only to it - that is what makes the report answer "which package
/// missed what".
fn push_both(issues: &mut Vec<Issue>, rust: &[Problem], go: &[Problem]) {
    for (kind, detail, penalty) in rust {
        let shared = go.iter().any(|(k, _, _)| k == kind);
        let side = if shared { Side::Both } else { Side::Rust };
        issues.push(Issue { kind: (*kind).into(), detail: detail.clone(), penalty: *penalty, side });
    }
    for (kind, detail, penalty) in go {
        if rust.iter().any(|(k, _, _)| k == kind) {
            continue;
        }
        issues.push(Issue {
            kind: (*kind).into(),
            detail: format!("Go only: {detail}"),
            penalty: *penalty,
            side: Side::Go,
        });
    }
}

/// The same, for one render each side got wrong: an error instead of a picture.
fn push_error(issues: &mut Vec<Issue>, rust: Option<&Error>, go: Option<&str>) {
    match (rust, go) {
        (Some(e), Some(g)) => {
            issues.push(Issue { kind: "render error".into(), detail: format!("both versions failed ({e} / {g})"), penalty: 60, side: Side::Both });
        }
        (Some(e), None) => {
            issues.push(Issue { kind: "render error".into(), detail: format!("Rust only: {e}"), penalty: 60, side: Side::Rust });
        }
        (None, Some(g)) => {
            issues.push(Issue { kind: "render error".into(), detail: format!("Go only: {g}"), penalty: 60, side: Side::Go });
        }
        (None, None) => {}
    }
}

pub fn check(work: &Path, limit: Option<usize>, threads: usize) {
    let plan: Plan = serde_json::from_slice(&fs::read(work.join("plan.json")).expect("run export first")).unwrap();
    let go = read_go(work);
    if go.is_none() {
        eprintln!("no work/go.json: rating without comparing to the Go version");
    }
    let img_dir = work.join("img");
    let _ = fs::remove_dir_all(&img_dir);
    fs::create_dir_all(&img_dir).unwrap();

    let skins: Vec<&PlanSkin> = plan.skins.iter().take(limit.unwrap_or(usize::MAX)).collect();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");
    let done = AtomicUsize::new(0);
    eprintln!("rating {} skins on {threads} of {} cores", skins.len(), num_cores());
    let bar = Bar::new("rust", skins.len());
    let mut ratings: Vec<Rating> = pool.install(|| {
        skins
            .par_iter()
            .map(|s| {
                let r = rate_skin(work, &plan, s, go.as_ref());
                bar.tick(done.fetch_add(1, Ordering::Relaxed) + 1);
                r
            })
            .collect()
    });
    bar.finish();
    ratings.sort_by(|a, b| a.score.cmp(&b.score).then(b.skin.players.cmp(&a.skin.players)));
    crate::report::write(work, &ratings, go.is_some());
}

/// What a skin loses: the cost of each *kind* of problem, once.
///
/// One cause usually affects a lot of images - a model that cannot move fails
/// all 37 animations, a bad texture fails every frame of all of them. Charging
/// per occurrence drove the score to zero and made the number useless, so a
/// kind costs what the report says it costs no matter how many times it fires.
/// The full list of occurrences is still in the issues and the report.
fn penalty_of(issues: &[Issue]) -> u32 {
    let mut worst: HashMap<&str, u32> = HashMap::new();
    for i in issues {
        let slot = worst.entry(i.kind.as_str()).or_insert(0);
        *slot = (*slot).max(i.penalty);
    }
    worst.values().sum()
}

fn num_cores() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn decode(path: &Path) -> RgbaImage {
    decode_image(&fs::read(path).unwrap()).unwrap()
}

fn rate_skin(work: &Path, plan: &Plan, s: &PlanSkin, go: Option<&HashMap<String, Vec<GoImage>>>) -> Rating {
    let base = work.join("skins").join(&s.id);
    let tex = decode(&base.join("texture.png"));
    let cape = s.cape.then(|| decode(&base.join("cape.png")));
    let geo_raw = if s.geometry { fs::read(base.join("geometry.json")).unwrap() } else { Vec::new() };

    let mut issues: Vec<Issue> = Vec::new();
    let mut notes = Vec::new();
    let mut ours: HashMap<String, Vec<String>> = HashMap::new();
    let mut go_renders: HashMap<String, Vec<GoImage>> = HashMap::new();
    // A macro, not a closure: the helpers below take `&mut issues`, which a
    // closure over the same vector would keep borrowed.
    macro_rules! issue {
        ($kind:expr, $detail:expr, $penalty:expr, $side:expr) => {
            issues.push(Issue { kind: ($kind).into(), detail: $detail, penalty: $penalty, side: $side })
        };
    }

    let mut geos = Vec::new();
    if !is_empty(&geo_raw) {
        match parse_geometry(&geo_raw) {
            Ok(g) => geos = g,
            Err(e) => {
                ours.insert("parse".into(), vec![format!("error: {e}")]);
                notes.push(format!("geometry does not parse ({e}); drawn on the default model"));
            }
        }
    }
    let model = select_geometry(if geos.is_empty() { default_geometry() } else { &geos }, &s.identifier).cloned();
    // Geometry that draws nothing - no cubes, no poly mesh - takes the flat
    // fallback, which cannot move.
    let persona = model.as_ref().is_some_and(|g| !g.has_mesh());
    if persona {
        notes.push("bones with nothing to draw: drawn flat, animations hold still".into());
    }
    if let Some(g) = &model
        && !geos.is_empty()
    {
        notes.push(format!("custom model {} ({} bones, {} cubes)", g.identifier, g.bones.len(), g.total_cubes()));
    }

    let skin = Skin::new(tex.clone(), (!geo_raw.is_empty()).then_some(&geo_raw[..]));
    let verdict = skin.report().verdict;
    let invisible = verdict != Verdict::Ok;
    if invisible {
        notes.push(format!("detector: {verdict} (missing {})", skin.invisible_parts().join(", ")));
    }

    let mut colours: HashSet<[u8; 3]> =
        tex.pixels().filter(|p| p.0[3] >= 128).map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
    if let Some(c) = &cape {
        colours.extend(c.pixels().filter(|p| p.0[3] >= 128).map(|p| [p.0[0], p.0[1], p.0[2]]));
    }

    let options = |view: &str, angle: &str, size: u32| {
        let mut o = RenderOptions::new(&tex).geometry(&geos).identifier(s.identifier.clone()).size(size);
        o.view = parse_view(view).unwrap();
        o.angle = parse_angle(angle).unwrap();
        o
    };
    let go_of = |key: &str| -> Option<&GoImage> { go?.get(&format!("{}/{}", s.id, key))?.first() };

    // ---- stills ----
    let mut stills: Vec<(String, RgbaImage)> = Vec::new();
    let mut body_cover = None;
    for st in &plan.stills {
        if st.cape && cape.is_none() {
            continue;
        }
        let mut o = options(&st.view, &st.angle, st.size);
        if st.cape {
            o.cape = cape.as_ref();
        }
        if let Some(c) = st.camera {
            o.camera = Some(Camera { yaw: c.Yaw, pitch: c.Pitch, fov: c.FOV, margin: c.Margin });
        }
        let rendered = o.render();
        let mut err = None;
        let mut rust_look = None;
        match &rendered {
            Err(e) => {
                ours.insert(st.name.clone(), vec![format!("error: {e}")]);
                err = Some(e);
                if let Error::EmptyView = e {
                    notes.push(format!("{}: the model has no bones for this view", st.name));
                }
            }
            Ok(img) => {
                ours.insert(st.name.clone(), vec![hash_image(img)]);
                let l = Look::of(img, &colours);
                if st.name == "body-front" {
                    body_cover = Some(l.cover());
                    if l.centre_off > 0.15 {
                        issue!("off centre", format!("body is {:.0}% of the image off centre", l.centre_off * 100.0), 5, Side::Both);
                    }
                }
                rust_look = Some(l);
                stills.push((st.name.clone(), img.clone()));
            }
        }

        // The same checks on the Go version's picture of the same thing.
        if go.is_some() {
            let g = go_of(&st.name);
            go_renders.insert(st.name.clone(), g.map(|x| vec![x.clone()]).unwrap_or_default());
            let go_err = g.filter(|x| !x.e.is_empty()).map(|x| x.e.as_str());
            let go_problems: Vec<Problem> = match g {
                Some(x) if x.e.is_empty() => assess(&st.name, &st.view, &Look::of_go(x, st.size, st.size, rust_look.as_ref()), invisible),
                _ => Vec::new(),
            };
            // A view the model has no bones for is a fact about the skin, not a
            // failure: it is already recorded as a note, so it costs nothing.
            let empty_view = matches!(err, Some(Error::EmptyView));
            if !empty_view && (go_err.is_some() || err.is_some()) {
                push_error(&mut issues, err, go_err);
            }
            if let Some(rl) = &rust_look {
                push_both(&mut issues, &assess(&st.name, &st.view, rl, invisible), &go_problems);
            } else {
                for (kind, detail, penalty) in go_problems {
                    issue!(kind, format!("Go only: {detail}"), penalty, Side::Go);
                }
            }
        } else if let Some(rl) = &rust_look {
            for (kind, detail, penalty) in assess(&st.name, &st.view, rl, invisible) {
                issue!(kind, detail, penalty, Side::Both);
            }
        }
    }

    // The detector and the picture should agree.
    if let Some(cover) = body_cover {
        if verdict == Verdict::Invisible && cover > 0.06 {
            issue!("detector disagrees", format!("detector says invisible, but the body covers {:.0}% of the image", cover * 100.0), 10, Side::Both);
        }
        if verdict == Verdict::Ok && cover < 0.005 {
            issue!("detector disagrees", "detector says visible, but almost nothing is drawn".into(), 10, Side::Both);
        }
    }

    // ---- animations ----
    let ex = example_animations();
    let mut gifs_wanted: Vec<String> = vec!["walk".into(), "animation.player.dance".into()];
    let mut not_applying = Vec::new();
    let mut still_anims: HashMap<Side, Vec<String>> = HashMap::new();
    for a in &plan.animations {
        let motion = a.name.parse::<Motion>().ok();
        let anim: &dyn Animator = match &motion {
            Some(m) => m,
            None => &ex[&a.name],
        };
        let applies = match (&motion, &model) {
            (None, Some(g)) => {
                let file = &ex[&a.name];
                file.missing_bones(g).len() < file.bones().len()
            }
            _ => true,
        };
        if !applies {
            not_applying.push(short_name(&a.name));
        }
        // One frame at a time: the skins are already spread over the cores.
        let opts = AnimationOptions::new(options("body", &a.angle, a.size), anim).fps(a.fps).workers(1);
        let key = format!("anim:{}", a.name);
        let short = short_name(&a.name);
        match opts.render_frames() {
            Err(e) => {
                ours.insert(key.clone(), vec![format!("error: {e}")]);
                issue!("animation error", format!("Rust: {short}: {e}"), 40, Side::Rust);
                if let Some(g) = go_of(&key).filter(|x| !x.e.is_empty()) {
                    issue!("animation error", format!("Go: {short}: {}", g.e), 40, Side::Go);
                } else {
                    issue!("animation error", format!("Go rendered {short} but Rust failed"), 40, Side::Both);
                }
            }
            Ok(frames) => {
                ours.insert(key.clone(), frames.iter().map(hash_image).collect());
                let gframes: Vec<GoImage> =
                    go.and_then(|g| g.get(&format!("{}/{}", s.id, key))).cloned().unwrap_or_default();
                go_renders.insert(key.clone(), gframes.clone());

                // Whether each version's frames are all the same picture. A
                // still animation is the skin's fault when both agree on it and
                // that package's fault when only one does.
                let first = &frames[0];
                let rust_still = frames.iter().all(|f| f == first);
                let go_still = !gframes.is_empty()
                    && gframes.first().is_some_and(|f| !f.e.is_empty() || f.h == gframes[0].h)
                    && gframes.iter().all(|f| f.h == gframes[0].h);
                if applies && !persona && frames.len() > 1 {
                    let side = match (rust_still, go_still) {
                        (true, true) => Some(Side::Both),
                        (true, false) => Some(Side::Rust),
                        (false, true) => Some(Side::Go),
                        (false, false) => None,
                    };
                    if let Some(side) = side {
                        // Collected, not pushed: a model that cannot move fails
                        // all 37 animations at once, and charging 15 points for
                        // each would take the score to zero and hide the real
                        // reason. It costs 15 once, listing which animations.
                        still_anims.entry(side).or_default().push(short.clone());
                        gifs_wanted.push(a.name.clone());
                    }
                }
                for (i, frame) in frames.iter().enumerate() {
                    let ours = Look::of(frame, &colours);
                    let gl = gframes.get(i).filter(|x| x.e.is_empty()).map(|x| Look::of_go(x, a.size, a.size, Some(&ours)));
                    let label = format!("{short} frame {i}");
                    // The same checks are dropped on both sides. Animation
                    // frames blend overlapping parts, so "tiny render" and
                    // "wrong colours" are noise there; leaving one of them on
                    // only one side would report the difference as the other
                    // package's bug.
                    let skip = |(k, _, _): &Problem| *k != "tiny render" && *k != "wrong colours";
                    let rust_problems: Vec<Problem> =
                        assess(&label, "anim", &ours, invisible)
                            .into_iter()
                            .filter(skip)
                            .collect();
                    let go_problems: Vec<Problem> = gl
                        .as_ref()
                        .map(|l| assess(&label, "anim", l, invisible).into_iter().filter(skip).collect())
                        .unwrap_or_default();
                    if gl.is_some() {
                        push_both(&mut issues, &rust_problems, &go_problems);
                    } else {
                        for (kind, detail, penalty) in rust_problems {
                            issue!(kind, detail, penalty, Side::Both);
                        }
                    }
                }
            }
        }
    }
    // One "doesn't move" issue per side, however many animations it affects.
    for (side, names) in &still_anims {
        let shown = names.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
        let more = names.len().saturating_sub(6);
        issue!(
            "doesn't move",
            format!(
                "{} of the {} animations never move on this model ({}{})",
                names.len(),
                plan.animations.len(),
                shown,
                if more > 0 { format!(" and {more} more") } else { String::new() }
            ),
            15,
            *side
        );
    }
    if !not_applying.is_empty() {
        notes.push(format!("{} example animations move no bone this model has", not_applying.len()));
    }
    if let Err(e) = AnimationOptions::new(options("body", "iso", 64), &Motion::Walk).workers(1).render_gif() {
        issue!("gif error", e.to_string(), 40, Side::Rust);
    }

    // ---- the Go version ----
    let mut compared = 0;
    if let Some(go) = go {
        let mut keys: Vec<&String> = ours.keys().collect();
        keys.sort();
        let mut differ = Vec::new();
        for k in keys {
            let mine = &ours[k];
            let theirs = go.get(&format!("{}/{}", s.id, k));
            match theirs {
                None => differ.push(format!("{k}: Go has no render")),
                Some(v) => {
                    compared += mine.len();
                    let go_err = v.first().is_some_and(|x| !x.e.is_empty());
                    let mine_err = mine.first().is_some_and(|h| h.starts_with("error"));
                    if mine_err || go_err {
                        if mine_err != go_err {
                            differ.push(format!("{k}: one version failed ({} / {})", mine[0], v[0].e));
                        }
                    } else if mine.len() != v.len() {
                        differ.push(format!("{k}: {} frames, Go has {}", mine.len(), v.len()));
                    } else if let Some(i) = mine.iter().zip(v).position(|(a, b)| a != &b.h) {
                        differ.push(if mine.len() > 1 { format!("{k} frame {i}") } else { k.clone() });
                        if let Some(name) = k.strip_prefix("anim:") {
                            gifs_wanted.push(name.to_string());
                        }
                    }
                }
            }
        }
        let extra = go.keys().filter(|k| k.starts_with(&format!("{}/", s.id))).count();
        if extra != ours.len() {
            differ.push(format!("Go made {extra} renders, Rust {}", ours.len()));
        }
        if !differ.is_empty() {
            issue!("differs from Go", differ.join("; "), 100, Side::Both);
        }
    }
    let _ = &go_renders;

    let renders = ours.values().map(Vec::len).sum();
    let penalty = penalty_of(&issues);
    let score = 100u32.saturating_sub(penalty);
    let grade = if score >= 90 {
        "pass"
    } else if score >= 60 {
        "check"
    } else {
        "fail"
    };
    let go_issues = issues.iter().filter(|i| i.side == Side::Go).count();
    let rust_issues = issues.iter().filter(|i| i.side == Side::Rust).count();

    // ---- pictures for the report ----
    let img_dir = work.join("img");
    let strip = (!stills.is_empty()).then(|| {
        let w: u32 = stills.iter().map(|(_, i)| i.width()).sum();
        let mut out = RgbaImage::new(w, 128);
        let mut x = 0;
        for (_, img) in &stills {
            image::imageops::overlay(&mut out, img, x as i64, 0);
            x += img.width();
        }
        let name = format!("{}.png", s.id);
        out.save(img_dir.join(&name)).unwrap();
        name
    });
    let mut gifs = Vec::new();
    gifs_wanted.dedup();
    let show_gifs = grade != "pass" || s.players >= 3 || s.geometry;
    if show_gifs {
        for name in gifs_wanted.iter().take(4) {
            let motion = name.parse::<Motion>().ok();
            let anim: &dyn Animator = match &motion {
                Some(m) => m,
                None => &ex[name],
            };
            if let Ok(gif) = AnimationOptions::new(options("body", "iso", 96), anim).fps(8).workers(1).render_gif() {
                let file = format!("{}-{}.gif", s.id, short_name(name));
                fs::write(img_dir.join(&file), gif).unwrap();
                gifs.push((short_name(name), file));
            }
        }
    }

    Rating {
        skin: s.clone(),
        score,
        grade: grade.into(),
        issues,
        notes,
        verdict: verdict.to_string(),
        renders,
        compared,
        go_issues,
        rust_issues,
        strip,
        gifs,
    }
}

fn short_name(name: &str) -> String {
    name.strip_prefix("animation.player.").unwrap_or(name).to_string()
}