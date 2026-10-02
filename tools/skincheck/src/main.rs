//! skincheck renders a large set of real skins every way the library can -
//! eight still views, a cape, and every frame of all 37 animations - checks
//! every image against what the Go version draws, and rates each skin, so
//! nobody has to look at thousands of renders to know they are right. The
//! report puts the skins that need a look first.
//!
//! ```text
//! cargo run --release -- export --scout <dir with scout.db and skins/> [--captures <dir>]...
//! cargo run --release -- go        # the Go version renders the same plan
//! cargo run --release -- check     # render, compare, rate, write work/report.html
//! cargo run --release -- all ...   # all three
//! ```
//!
//! Both long phases draw a progress bar on stderr: one line rewritten as the
//! skins go by, with a rate and an ETA.
//!
//! Skins are players' own: everything goes to work/, which git ignores.

mod progress;
mod rate;
mod report;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use bedrock_skin::{Motion, example_animations, parse_resource_patch};
use serde::{Deserialize, Serialize};

const WORK: &str = "work";

#[derive(Serialize, Deserialize, Clone)]
pub struct Plan {
    pub skins: Vec<PlanSkin>,
    pub stills: Vec<Still>,
    pub animations: Vec<Anim>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlanSkin {
    pub id: String,
    pub identifier: String,
    pub geometry: bool,
    pub cape: bool,
    /// Where it came from: a scout skin hash or a capture folder.
    pub source: String,
    /// How many players wear it, when it came from scout.
    pub players: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[allow(non_snake_case)]
pub struct PlanCamera {
    pub Yaw: f64,
    pub Pitch: f64,
    pub FOV: f64,
    pub Margin: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Still {
    pub name: String,
    pub view: String,
    pub angle: String,
    pub camera: Option<PlanCamera>,
    pub size: u32,
    pub cape: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Anim {
    pub name: String,
    pub fps: u32,
    pub size: u32,
    pub angle: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage() };
    let flag = |name: &str| -> Vec<String> {
        args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
    };
    let started = Instant::now();
    let looks = std::env::args().any(|a| a == "--looks");
    let threads = flag("--threads").first().and_then(|s| s.parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or_else(default_threads);
    let limit = flag("--limit").first().and_then(|s| s.parse().ok());
    match cmd.as_str() {
        "export" => export(&flag("--scout"), &flag("--captures")),
        "go" => run_go(threads, limit, looks),
        "check" => rate::check(Path::new(WORK), limit, threads),
        "report" => rewrite_report(),
        "all" => {
            export(&flag("--scout"), &flag("--captures"));
            run_go(threads, limit, looks);
            rate::check(Path::new(WORK), limit, threads);
        }
        _ => usage(),
    }
    eprintln!("done in {:.1}s", started.elapsed().as_secs_f64());
}

/// Rewrites report.html, failures.html and failures.csv from the ratings a
/// previous check wrote, without rendering anything. Ratings change only when
/// the checks do, so changing how the report is laid out should not cost
/// thirteen million renders.
fn rewrite_report() {
    let work = Path::new(WORK);
    let file = fs::File::open(work.join("ratings.json")).expect("run check first");
    let ratings: Vec<rate::Rating> =
        serde_json::from_reader(std::io::BufReader::new(file)).unwrap();
    let compared_go = ratings.iter().any(|r| r.compared > 0);
    report::write(work, &ratings, compared_go);
}

fn usage() -> ! {
    eprintln!("usage: skincheck export --scout <dir> [--captures <dir>]... | go [--threads N] [--looks] | check [--limit N] [--threads N] | report | all ...");
    eprintln!("  --threads N  cores to render on (default {}, or $SKINCHECK_THREADS)", default_threads());
    std::process::exit(2)
}

/// The renders every skin gets.
fn plan_renders() -> (Vec<Still>, Vec<Anim>) {
    let still = |name: &str, view: &str, angle: &str, camera: Option<PlanCamera>, cape: bool| Still {
        name: name.into(),
        view: view.into(),
        angle: angle.into(),
        camera,
        size: 128,
        cape,
    };
    let cam = |yaw, pitch| Some(PlanCamera { Yaw: yaw, Pitch: pitch, FOV: 0.0, Margin: 0.0 });
    let stills = vec![
        still("body-front", "body", "front", None, false),
        still("body-iso", "body", "iso", None, false),
        still("back", "body", "", cam(180.0, 0.0), false),
        still("side", "body", "", cam(90.0, 0.0), false),
        still("chest", "chest", "", None, false),
        still("head", "head", "", None, false),
        still("avatar", "avatar", "iso", None, false),
        still("cape", "body", "", cam(160.0, 10.0), true),
    ];
    let mut animations: Vec<Anim> = Motion::ALL
        .iter()
        .map(|m| Anim { name: m.name().into(), fps: 8, size: 96, angle: "iso".into() })
        .collect();
    for name in example_animations().keys() {
        animations.push(Anim { name: name.clone(), fps: 8, size: 96, angle: "iso".into() });
    }
    (stills, animations)
}

fn export(scout: &[String], captures: &[String]) {
    if scout.is_empty() && captures.is_empty() {
        eprintln!("export needs --scout and/or --captures");
        std::process::exit(2);
    }
    let work = Path::new(WORK);
    let skins_dir = work.join("skins");
    let _ = fs::remove_dir_all(&skins_dir);
    fs::create_dir_all(&skins_dir).unwrap();
    let mut skins = Vec::new();
    for dir in scout {
        skins.extend(export_scout(Path::new(dir), &skins_dir));
    }
    for dir in captures {
        skins.extend(export_captures(Path::new(dir), &skins_dir));
    }
    let (stills, animations) = plan_renders();
    let plan = Plan { skins, stills, animations };
    fs::write(work.join("plan.json"), serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let _ = fs::remove_file(work.join("go.json"));
    eprintln!("exported {} skins", plan.skins.len());
}

fn short(s: &str, n: usize) -> &str {
    &s[..s.len().min(n)]
}

fn fnv(s: &str) -> String {
    let mut h = 14695981039346656037u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{:06x}", h & 0xffffff)
}

/// Every distinct skin scout has seen - texture, model, cape and the model
/// its patch picks - with how many players wear it.
fn export_scout(dir: &Path, out: &Path) -> Vec<PlanSkin> {
    let open = |p: PathBuf| {
        rusqlite::Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    };
    let players = open(dir.join("scout.db"));
    let files = open(dir.join("skins").join("skins.db"));
    let get = |kind: &str, hash: &str| -> Option<Vec<u8>> {
        files
            .query_row("SELECT data FROM files WHERE kind = ?1 AND hash = ?2", [kind, hash], |r| r.get(0))
            .ok()
    };
    // players holds only the skin a player wears now; player_skins holds every
    // skin they have worn, so both are read or a good part of the file store is
    // never rendered. A player seen in the same combination twice counts once.
    let has_history: bool = players
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'player_skins'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    let query = if has_history {
        "SELECT skin_hash, cape_hash, geometry_hash, skin_patch, SUM(n) FROM (
             SELECT skin_hash, cape_hash, geometry_hash, skin_patch, COUNT(*) AS n FROM players
              WHERE skin_hash != '' GROUP BY skin_hash, cape_hash, geometry_hash, skin_patch
             UNION ALL
             SELECT skin_hash, cape_hash, geometry_hash, skin_patch, COUNT(*) AS n FROM player_skins
              WHERE skin_hash != '' GROUP BY skin_hash, cape_hash, geometry_hash, skin_patch)
         GROUP BY skin_hash, cape_hash, geometry_hash, skin_patch"
    } else {
        "SELECT skin_hash, cape_hash, geometry_hash, skin_patch, COUNT(*) FROM players
         WHERE skin_hash != '' GROUP BY skin_hash, cape_hash, geometry_hash, skin_patch"
    };
    let mut stmt = players.prepare(query).unwrap();
    let rows: Vec<(String, String, String, String, u32)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    // Patches differ in whitespace; what matters is the model they pick.
    let mut merged: BTreeMap<String, (PlanSkin, String, String, String)> = BTreeMap::new();
    for (skin, cape, geo, patch, n) in rows {
        let identifier = parse_resource_patch(patch.as_bytes()).map(|p| p.default).unwrap_or_default();
        let id = format!("{}-{}", short(&skin, 12), fnv(&format!("{geo}|{cape}|{identifier}")));
        let entry = merged.entry(id.clone()).or_insert_with(|| {
            let ps = PlanSkin {
                id,
                identifier,
                geometry: !geo.is_empty(),
                cape: !cape.is_empty(),
                source: format!("scout skin {skin}"),
                players: 0,
            };
            (ps, skin.clone(), geo.clone(), cape.clone())
        });
        entry.0.players += n;
    }
    let mut out_skins = Vec::new();
    for (_, (mut ps, skin, geo, cape)) in merged {
        let Some(tex) = get("skin", &skin) else { continue };
        let d = out.join(&ps.id);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("texture.png"), tex).unwrap();
        if ps.geometry {
            match get("geometry", &geo).and_then(|z| {
                let mut s = Vec::new();
                flate2::read::ZlibDecoder::new(&z[..]).read_to_end(&mut s).ok().map(|_| s)
            }) {
                Some(g) => fs::write(d.join("geometry.json"), g).unwrap(),
                None => ps.geometry = false,
            }
        }
        if ps.cape {
            match get("cape", &cape) {
                Some(c) => fs::write(d.join("cape.png"), c).unwrap(),
                None => ps.cape = false,
            }
        }
        out_skins.push(ps);
    }
    out_skins
}

/// Capture folders: each subfolder with texture.png and geometry.json, or
/// loose <name>.png files with an optional <name>-geometry.json.
fn export_captures(dir: &Path, out: &Path) -> Vec<PlanSkin> {
    let mut found: Vec<(String, PathBuf, Option<PathBuf>)> = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        if p.is_dir() && p.join("texture.png").exists() {
            let g = p.join("geometry.json");
            found.push((name, p.join("texture.png"), g.exists().then_some(g)));
        } else if p.extension().is_some_and(|e| e == "png") {
            let g = dir.join(format!("{name}-geometry.json"));
            found.push((name, p.clone(), g.exists().then_some(g)));
        }
    }
    let mut skins = Vec::new();
    for (name, tex, geo) in found {
        let id = format!("capture-{}", fnv(&format!("{}{name}", dir.display())));
        let d = out.join(&id);
        fs::create_dir_all(&d).unwrap();
        fs::copy(&tex, d.join("texture.png")).unwrap();
        if let Some(g) = &geo {
            fs::copy(g, d.join("geometry.json")).unwrap();
        }
        skins.push(PlanSkin {
            id,
            identifier: String::new(),
            geometry: geo.is_some(),
            cape: false,
            source: format!("capture {}", tex.display()),
            players: 0,
        });
    }
    skins
}

/// Has the Go version render the plan, writing work/go.tsv - or, with
/// --looks, work/go.json with every image measured too.
fn run_go(threads: usize, limit: Option<usize>, looks: bool) {
    let tool = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("parity");
    let work = fs::canonicalize(WORK).expect("run export first");
    let status = Command::new("go")
        .args(["run", ".", "-batch"])
        .arg(&work)
        .args(["-threads", &threads.to_string()])
        .arg("-limit")
        .arg(limit.map_or_else(|| "0".to_string(), |n| n.to_string()))
        .args(looks.then_some("-looks"))
        .current_dir(&tool)
        .env("GOWORK", "off")
        .status();
    match status {
        Ok(s) if s.success() => {}
        other => eprintln!("the Go render failed ({other:?}); check will rate without comparing to Go"),
    }
}

/// FNV-1a 64 over the width and height (little-endian u32), then the RGBA
/// bytes: the same as tools/parity's hashImage.
pub fn hash_image(img: &image::RgbaImage) -> String {
    let mut h = 14695981039346656037u64;
    let mut add = |c: u8| {
        h ^= c as u64;
        h = h.wrapping_mul(1099511628211);
    };
    for v in [img.width(), img.height()] {
        for b in v.to_le_bytes() {
            add(b);
        }
    }
    for &c in img.as_raw() {
        add(c);
    }
    format!("{h:016x}")
}

/// One image the Go version drew: its pixel hash and what the picture looks
/// like, so the checks below can be run on Go's output as well as on ours.
#[derive(Serialize, Deserialize, Clone)]
pub struct GoImage {
    #[serde(default)]
    pub h: String,
    #[serde(default)]
    pub e: String,
    #[serde(default)]
    pub l: Option<GoLook>,
}

/// The measurements `rate.rs` needs from one Go image.
#[derive(Serialize, Deserialize, Clone)]
pub struct GoLook {
    #[serde(default)]
    pub o: usize,
    #[serde(default)]
    pub f: usize,
    #[serde(default)]
    pub c: usize,
    #[serde(default)]
    pub e: bool,
    #[serde(default)]
    pub x0: usize,
    #[serde(default)]
    pub y0: usize,
    #[serde(default)]
    pub x1: usize,
    #[serde(default)]
    pub y1: usize,
}

/// What the Go version drew: go.tsv, its hashes - one line per render,
/// `<skin>/<render>`, a tab, then a hash per frame or `!` and the error - or
/// go.json when it was run with --looks and measured every image too.
pub fn read_go(work: &Path) -> Option<HashMap<String, Vec<GoImage>>> {
    if let Ok(text) = fs::read_to_string(work.join("go.tsv")) {
        let mut out = HashMap::new();
        for line in text.lines() {
            let Some((key, rest)) = line.split_once('\t') else { continue };
            // Hashes are hex, so a `!` can only start the error, which ends
            // the line, spaces and all.
            let (hashes, error) = match rest.find('!') {
                Some(at) => (&rest[..at], Some(&rest[at + 1..])),
                None => (rest, None),
            };
            let mut images: Vec<GoImage> = hashes
                .split_whitespace()
                .map(|h| GoImage { h: h.to_string(), e: String::new(), l: None })
                .collect();
            if let Some(e) = error {
                images.push(GoImage { h: String::new(), e: e.to_string(), l: None });
            }
            out.insert(key.to_string(), images);
        }
        return Some(out);
    }
    serde_json::from_slice(&fs::read(work.join("go.json")).ok()?).ok()
}

/// How many cores to render on. Default is half the machine, or 4, whichever
/// is larger: a full run is 13 million images, and taking every core makes the
/// machine unusable meanwhile. SKINCHECK_THREADS or --threads sets it.
fn default_threads() -> usize {
    if let Ok(v) = std::env::var("SKINCHECK_THREADS")
        && let Ok(n) = v.parse::<usize>()
        && n > 0
    {
        return n;
    }
    std::thread::available_parallelism().map(|n| (n.get() / 2).max(4)).unwrap_or(4)
}
