//! The high-level invisibility report: [`Skin`] and [`SkinReport`].

use std::fmt;
use std::sync::OnceLock;

use image::RgbaImage;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::invisible::{
    STANDARD_PART_NAMES, SkinPartResult, SkinVisibilityResult, Thresholds, validate_with,
};

/// Why one body part counts as rendering or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PartVisibility {
    /// Enough opaque pixels to render normally.
    #[default]
    Visible,
    /// No usable opaque pixels.
    Invisible,
    /// Some opaque pixels, but below the minimum fraction.
    Suspicious,
    /// The geometry makes the part too small to see.
    Tiny,
}

impl PartVisibility {
    /// A stable lower-case name: `visible`, `invisible`, `suspicious`, `tiny`.
    pub fn name(self) -> &'static str {
        match self {
            PartVisibility::Visible => "visible",
            PartVisibility::Invisible => "invisible",
            PartVisibility::Suspicious => "suspicious",
            PartVisibility::Tiny => "tiny",
        }
    }
}

impl fmt::Display for PartVisibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Serialized as its name, so JSON reads `"visibility":"invisible"`.
impl Serialize for PartVisibility {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

/// Accepts the names, and the bare integers 0-3 an older client may send.
impl<'de> Deserialize<'de> for PartVisibility {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(s) => match s.as_str() {
                "visible" => Ok(PartVisibility::Visible),
                "invisible" => Ok(PartVisibility::Invisible),
                "suspicious" => Ok(PartVisibility::Suspicious),
                "tiny" => Ok(PartVisibility::Tiny),
                _ => Err(D::Error::custom(format!("unknown part visibility {s:?}"))),
            },
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(0) => Ok(PartVisibility::Visible),
                Some(1) => Ok(PartVisibility::Invisible),
                Some(2) => Ok(PartVisibility::Suspicious),
                Some(3) => Ok(PartVisibility::Tiny),
                _ => Err(D::Error::custom(format!(
                    "part visibility {n} out of range"
                ))),
            },
            other => Err(D::Error::custom(format!(
                "part visibility: unexpected {other}"
            ))),
        }
    }
}

/// The overall judgement on a skin: one value with a fixed set of states,
/// rather than booleans that could contradict each other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// No analysis has been run. A default report does not claim a skin is
    /// fine: [`SkinReport::ok`] is false for it.
    #[default]
    Unknown,
    /// The skin renders normally.
    Ok,
    /// Some standard body parts do not render, but enough do that the skin
    /// is not simply invisible. Worth logging or reviewing, not necessarily
    /// rejecting.
    Suspicious,
    /// Nothing renders, or only a stray limb does.
    Invisible,
}

impl Verdict {
    /// A stable lower-case name: `unknown`, `ok`, `suspicious`, `invisible`.
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Unknown => "unknown",
            Verdict::Ok => "ok",
            Verdict::Suspicious => "suspicious",
            Verdict::Invisible => "invisible",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Serialize for Verdict {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Verdict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(d)?;
        match s.as_str() {
            "unknown" => Ok(Verdict::Unknown),
            "ok" => Ok(Verdict::Ok),
            "suspicious" => Ok(Verdict::Suspicious),
            "invisible" => Ok(Verdict::Invisible),
            _ => Err(D::Error::custom(format!("unknown verdict {s:?}"))),
        }
    }
}

/// One body part after analysis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartReport {
    /// The body part or bone name (head, hat, cape, ...).
    pub name: String,
    pub visibility: PartVisibility,
    /// The share of the part's sampled pixels that are opaque, 0 to 1.
    pub opaque_ratio: f64,
    /// How many texture pixels were sampled, and how many were see-through.
    pub pixels: usize,
    #[serde(rename = "transparent_pixels")]
    pub transparent: usize,
    /// Resolved from real geometry cube UVs rather than the standard layout.
    pub from_geometry: bool,
}

impl PartReport {
    /// Whether the part renders, from its visibility.
    pub fn visible(&self) -> bool {
        self.visibility == PartVisibility::Visible
    }
}

/// The result of analysing a skin. It serializes straight to JSON for an
/// API, the same shape as the Go version's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SkinReport {
    pub verdict: Verdict,
    /// How many of the standard body parts render, out of `total_parts`.
    /// Overlays fold into the part they cover and accessories are ignored,
    /// so an opaque cape cannot mask an invisible body.
    pub visible_parts: usize,
    pub total_parts: usize,
    /// The standard body parts first, in a fixed order, then every other
    /// bone sorted by name.
    pub parts: Vec<PartReport>,
}

impl SkinReport {
    /// Whether the skin is acceptable. False for a default report.
    pub fn ok(&self) -> bool {
        self.verdict == Verdict::Ok
    }

    /// The standard body parts that do not render, in report order.
    pub fn invisible_parts(&self) -> Vec<String> {
        self.parts
            .iter()
            .filter(|p| STANDARD_PART_NAMES.contains(&p.name.as_str()) && !p.visible())
            .map(|p| p.name.clone())
            .collect()
    }
}

/// The thresholds one [`Skin`] judges by. A zero field takes its default, so
/// the default `SkinOptions` is what [`Skin::new`] uses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinOptions {
    /// The share of a part's pixels that must be opaque for it to count as
    /// visible. Zero means [`DEFAULT_MIN_VISIBLE_FRACTION`](crate::DEFAULT_MIN_VISIBLE_FRACTION).
    pub min_visible_fraction: f64,
    /// The size a bone must reach not to be too small to see; applies only
    /// with geometry. Zero means [`DEFAULT_MIN_GEOMETRY_SIZE`](crate::DEFAULT_MIN_GEOMETRY_SIZE).
    pub min_geometry_size: f64,
    /// How many of the six standard parts must be visible for the skin not
    /// to be suspicious. Zero means [`DEFAULT_MIN_VISIBLE_PARTS`](crate::DEFAULT_MIN_VISIBLE_PARTS).
    pub min_visible_parts: usize,
}

/// A texture with its optional geometry, to ask whether its body parts show.
///
/// ```no_run
/// # let tex = image::RgbaImage::new(64, 64);
/// use bedrock_skin::{Skin, Verdict};
/// let skin = Skin::new(tex, None);
/// match skin.report().verdict {
///     Verdict::Invisible => { /* nothing renders, or only a stray limb */ }
///     Verdict::Suspicious => { /* some standard parts missing */ }
///     _ => {}
/// }
/// ```
///
/// The analysis runs once, on first use, and is shared by every question.
pub struct Skin {
    texture: RgbaImage,
    geometry: Vec<u8>,
    th: Thresholds,
    report: OnceLock<SkinReport>,
}

impl Skin {
    /// A skin judged by the default thresholds. `geometry` is raw
    /// geometry.json, or None for a skin that sends none (most).
    pub fn new(texture: RgbaImage, geometry: Option<&[u8]>) -> Skin {
        Skin::with_options(texture, geometry, SkinOptions::default())
    }

    /// A skin judged by `opts` rather than the defaults.
    pub fn with_options(texture: RgbaImage, geometry: Option<&[u8]>, opts: SkinOptions) -> Skin {
        Skin {
            texture,
            geometry: geometry.unwrap_or_default().to_vec(),
            th: Thresholds {
                min_visible_fraction: opts.min_visible_fraction,
                min_geometry_size: opts.min_geometry_size,
                min_visible_parts: opts.min_visible_parts,
            }
            .resolved(),
            report: OnceLock::new(),
        }
    }

    /// The full analysis.
    pub fn report(&self) -> &SkinReport {
        self.report.get_or_init(|| self.analyze())
    }

    /// The per-part breakdown.
    pub fn parts(&self) -> &[PartReport] {
        &self.report().parts
    }

    /// Whether the whole skin is effectively invisible.
    pub fn is_invisible(&self) -> bool {
        self.report().verdict == Verdict::Invisible
    }

    /// Whether the skin is half-invisible: not wholly, but several body
    /// parts are missing.
    pub fn is_suspicious(&self) -> bool {
        self.report().verdict == Verdict::Suspicious
    }

    /// The standard body parts that do not render.
    pub fn invisible_parts(&self) -> Vec<String> {
        self.report().invisible_parts()
    }

    /// Whether the skin is acceptable: the one question most callers have.
    pub fn ok(&self) -> bool {
        self.report().ok()
    }

    fn analyze(&self) -> SkinReport {
        let base = validate_with(&self.texture, &self.geometry, self.th);
        SkinReport {
            verdict: verdict_of(&base),
            visible_parts: base.visible_parts,
            total_parts: STANDARD_PART_NAMES.len(),
            parts: base
                .parts
                .iter()
                .map(|p| PartReport {
                    name: p.name.clone(),
                    visibility: part_visibility(p, self.th),
                    opaque_ratio: p.fraction,
                    pixels: p.pixels,
                    transparent: p.transparent,
                    from_geometry: p.from_geo,
                })
                .collect(),
        }
    }
}

fn verdict_of(r: &SkinVisibilityResult) -> Verdict {
    if r.is_invisible {
        Verdict::Invisible
    } else if r.suspicious {
        Verdict::Suspicious
    } else {
        Verdict::Ok
    }
}

/// Tiny is checked first: it comes from the size pass, and a tiny part's
/// fraction is zeroed, so it would otherwise read as transparent.
fn part_visibility(p: &SkinPartResult, th: Thresholds) -> PartVisibility {
    let th = th.resolved();
    if p.tiny {
        PartVisibility::Tiny
    } else if !p.visible {
        if p.fraction > 0.0 && p.fraction < th.min_visible_fraction {
            PartVisibility::Suspicious
        } else {
            PartVisibility::Invisible
        }
    } else {
        PartVisibility::Visible
    }
}
