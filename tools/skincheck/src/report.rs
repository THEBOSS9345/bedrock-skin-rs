//! The report: work/report.html and work/ratings.json.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::rate::{Rating, Side};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Everything up to the first heading, shared by report.html and failures.html.
const HEAD: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>
:root{--bg:#f6f7f9;--card:#fff;--ink:#16181d;--muted:#5d6470;--line:#e2e5ea;--pass:#1f8a4c;--check:#b7791f;--fail:#c53030;--sky:#dfe7f1}
@media (prefers-color-scheme:dark){:root{--bg:#111317;--card:#1a1d23;--ink:#e8eaee;--muted:#9aa1ad;--line:#2a2f37;--sky:#232a35}}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.45 system-ui,sans-serif}
main{max-width:1180px;margin:0 auto;padding:24px 16px 64px}
h1{margin:0 0 4px;font-size:26px}h2{margin:36px 0 12px;font-size:19px}
.muted{color:var(--muted)}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:10px;margin:18px 0}
.stat{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px}
.stat b{display:block;font-size:24px}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px;margin:12px 0}
.head{display:flex;flex-wrap:wrap;gap:8px 14px;align-items:baseline}
.score{font-weight:700;font-size:18px;min-width:3ch}
.pill{border-radius:99px;padding:1px 9px;font-size:12px;font-weight:600;color:#fff}
.pass{background:var(--pass)}.check{background:var(--check)}.fail{background:var(--fail)}
.pics{display:flex;flex-wrap:wrap;gap:8px;margin-top:10px;align-items:flex-end}
.pics img{background:var(--sky);border-radius:6px;image-rendering:pixelated;max-width:100%}
.strip{overflow-x:auto}
figure{margin:0;text-align:center;font-size:12px;color:var(--muted)}
ul{margin:8px 0 0;padding-left:20px}li.issue{color:var(--fail)}
table{border-collapse:collapse;background:var(--card);border:1px solid var(--line);border-radius:10px;overflow:hidden}
td,th{padding:6px 12px;border-bottom:1px solid var(--line);text-align:left}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:10px}
.mini{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:8px;font-size:12px}
.mini img{width:100%;image-rendering:pixelated;background:var(--sky);border-radius:6px}
code{font-size:12px}
details{background:var(--card);border:1px solid var(--line);border-radius:10px;margin:10px 0;padding:10px 14px}
summary{cursor:pointer;font-weight:600}
nav{margin:10px 0;font-size:14px}nav a{margin-right:14px}
</style></head><body><main>
"#;

/// One skin's pictures, the still strip and whatever GIFs were kept.
fn pics(r: &Rating) -> String {
    let mut h = String::from("<div class=pics>");
    if let Some(s) = &r.strip {
        let _ = write!(
            h,
            "<figure class=strip><img loading=lazy src='img/{s}' height=128 alt='Still views'><br>front · angled · back · side · chest · head · avatar · cape</figure>"
        );
    }
    for (name, file) in &r.gifs {
        let _ = write!(h, "<figure><img loading=lazy src='img/{file}' width=96 height=96 alt='{name}'><br>{name}</figure>");
    }
    h.push_str("</div>");
    h
}

/// A skin's heading line: score, grade, id, how worn, and the verdict.
fn head(r: &Rating) -> String {
    let mut h = String::new();
    let _ = write!(
        h,
        "<div class=head><span class=score>{}</span><span class='pill {}'>{}</span><code>{}</code>\
         <span class=muted>{}{} · detector: {} · {} images</span></div>",
        r.score,
        r.grade,
        r.grade,
        esc(&r.skin.id),
        if r.skin.players > 0 { format!("{} players · ", r.skin.players) } else { String::new() },
        if r.skin.identifier.is_empty() { "default model".to_string() } else { esc(&r.skin.identifier) },
        r.verdict,
        r.renders
    );
    h
}

/// The issues and notes as a list.
///
/// Repeated issues of the same kind are collapsed to one line with a count. One
/// cause usually hits dozens of images - a model that cannot move fails all 37
/// animations - and listing each one buries the other problems.
fn bullets(r: &Rating) -> String {
    if r.issues.is_empty() && r.notes.is_empty() {
        return String::new();
    }
    // Keep the order the checks found them in, first occurrence wins.
    let mut order: Vec<(&str, Side)> = Vec::new();
    let mut groups: HashMap<(&str, Side), (u32, Vec<&str>)> = HashMap::new();
    for i in &r.issues {
        let key = (i.kind.as_str(), i.side);
        if !order.contains(&key) {
            order.push(key);
        }
        let g = groups.entry(key).or_insert((i.penalty, Vec::new()));
        g.0 = g.0.max(i.penalty);
        g.1.push(&i.detail);
    }
    let mut h = String::from("<ul>");
    for key in &order {
        let (penalty, details) = &groups[key];
        let show = details.iter().take(3).cloned().collect::<Vec<_>>().join("; ");
        let more = details.len().saturating_sub(3);
        let _ = write!(
            h,
            "<li class=issue>-{} <b>{}</b>: {}: {}<span class=muted>{}</span></li>",
            penalty,
            key.1.label(),
            key.0,
            esc(&show),
            if more > 0 {
                format!(" (and {} more like this)", more)
            } else {
                String::new()
            }
        );
    }
    for n in &r.notes {
        let _ = write!(h, "<li class=muted>{}</li>", esc(n));
    }
    h.push_str("</ul>");
    h
}

/// What each check means and what it costs a skin, worst first.
const CHECKS: [(&str, u32, &str); 12] = [
    ("differs from Go", 100, "any image or frame not identical to the Go version's"),
    ("render error", 60, "a view fails to render"),
    ("animation error", 40, "an animation fails to render"),
    ("gif error", 40, "the walk GIF fails to encode"),
    ("blank render", 40, "a view draws nothing, though the detector says the skin is visible"),
    ("blank frame", 30, "an animation frame draws nothing"),
    ("wrong colours", 30, "under 95% of solid pixels are colours from the skin"),
    ("cut off", 15, "a whole-body view touches the edge of the image"),
    ("tiny render", 15, "a body view covers almost nothing"),
    ("doesn't move", 15, "an animation that should move this model never does"),
    ("detector disagrees", 10, "the invisible-skin verdict doesn't match what is drawn"),
    ("off centre", 5, "the body is far from the middle of the picture"),
];

/// The table of checks and how many skins each one caught.
fn checks_table(kinds: &BTreeMap<&str, usize>) -> String {
    let mut h = String::from("<table><tr><th>Check</th><th>Points</th><th>Skins</th></tr>");
    for (kind, pts, what) in CHECKS {
        let _ = write!(
            h,
            "<tr><td>{kind}</td><td>-{pts}</td><td>{}</td></tr>\
             <tr><td colspan=3 class=muted style=font-size:13px>{what}</td></tr>",
            kinds.get(kind).unwrap_or(&0)
        );
    }
    h.push_str("</table>");
    h
}

/// How many skins each check caught, counting a skin once per kind - the same
/// number the table shows.
fn kinds_of<'a>(ratings: impl IntoIterator<Item = &'a Rating>) -> BTreeMap<&'a str, usize> {
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for r in ratings {
        let mut seen = HashSet::new();
        for i in &r.issues {
            if seen.insert(i.kind.as_str()) {
                *kinds.entry(i.kind.as_str()).or_default() += 1;
            }
        }
    }
    kinds
}

/// How many skins each check caught on each side: Go only, Rust only, or both.
/// "Both" means the skin itself is at fault; a single side means that package
/// is the one that got it wrong.
fn sides_of<'a>(ratings: impl IntoIterator<Item = &'a Rating>) -> BTreeMap<&'a str, [usize; 3]> {
    let mut sides: BTreeMap<&str, [usize; 3]> = BTreeMap::new();
    for r in ratings {
        let mut seen: HashSet<(&str, u8)> = HashSet::new();
        for i in &r.issues {
            let slot = match i.side {
                Side::Go => 0usize,
                Side::Rust => 1,
                Side::Both => 2,
            };
            if seen.insert((i.kind.as_str(), slot as u8)) {
                sides.entry(i.kind.as_str()).or_default()[slot] += 1;
            }
        }
    }
    sides
}

/// The table of checks split by which library has the problem.
fn sides_table(ratings: &[&Rating]) -> String {
    let sides = sides_of(ratings.iter().copied());
    let mut h = String::from(
        "<table><tr><th>Check</th><th>Go only</th><th>Rust only</th><th>Both</th><th>Total</th></tr>",
    );
    for (kind, _, _) in CHECKS {
        let s = sides.get(kind).copied().unwrap_or([0; 3]);
        let total = s.iter().sum::<usize>();
        if total == 0 {
            continue;
        }
        let _ = write!(
            h,
            "<tr><td>{kind}</td><td>{}</td><td>{}</td><td>{}</td><td>{total}</td></tr>",
            s[0], s[1], s[2]
        );
    }
    h.push_str("</table>");
    h
}

/// The one-paragraph answer to "which package missed what".
fn sides_summary(ratings: &[&Rating]) -> String {
    let mut counts = [0usize; 3];
    let mut by_side = [0usize; 3];
    for r in ratings {
        for i in &r.issues {
            let slot = match i.side {
                Side::Go => 0,
                Side::Rust => 1,
                Side::Both => 2,
            };
            counts[slot] += 1;
        }
        if r.go_issues > 0 {
            by_side[0] += 1;
        }
        if r.rust_issues > 0 {
            by_side[1] += 1;
        }
    }
    let mut h = String::new();
    let _ = write!(
        h,
        "<p class=muted>Both packages drew all {skins} skins and every image was measured on each side. \
         Of the {total} problems found, <b>{go}</b> are Go's alone, <b>{rust}</b> are this crate's alone, \
         and {both} are shared - the skin, not the package. \
         {goskins} skins have at least one Go-only problem and {rustskins} have at least one Rust-only problem.</p>",
        skins = ratings.len(),
        total = counts.iter().sum::<usize>(),
        go = counts[0],
        rust = counts[1],
        both = counts[2],
        goskins = by_side[0],
        rustskins = by_side[1],
    );
    h
}

pub fn write(work: &Path, ratings: &[Rating], compared_go: bool) {
    fs::write(work.join("ratings.json"), serde_json::to_vec_pretty(ratings).unwrap()).unwrap();

    let count = |g: &str| ratings.iter().filter(|r| r.grade == g).count();
    let (pass, check, fail) = (count("pass"), count("check"), count("fail"));
    let renders: usize = ratings.iter().map(|r| r.renders).sum();
    let compared: usize = ratings.iter().map(|r| r.compared).sum();
    let differ = ratings.iter().filter(|r| r.issues.iter().any(|i| i.kind == "differs from Go")).count();
    let kinds = kinds_of(ratings);
    let go_skins = ratings.iter().filter(|r| r.go_issues > 0).count();
    let rust_skins = ratings.iter().filter(|r| r.rust_issues > 0).count();

    let mut h = String::from(HEAD);
    let _ = write!(h, "<title>Skin check</title>");
    let _ = write!(
        h,
        "<h1>Skin check</h1><p class=muted>Every skin rendered in 8 views, with its cape, and every frame of all 37 animations{}. \
         Each skin starts at 100 and loses points for each problem the checks find; the ones worth a look come first.</p>\
         <nav><a href='failures.html'>failures.html</a><a href='failures.csv'>failures.csv</a></nav>",
        if compared_go { ", each image compared with the Go version's" } else { " (not compared with Go this run)" }
    );
    let _ = write!(
        h,
        "<div class=stats><div class=stat><b>{}</b>skins</div><div class=stat><b>{renders}</b>images rendered</div>\
         <div class=stat><b>{compared}</b>compared with Go</div><div class=stat><b>{differ}</b>skins differ from Go</div>\
         <div class=stat><b style=color:var(--pass)>{pass}</b>pass (90+)</div><div class=stat><b style=color:var(--check)>{check}</b>check (60-89)</div>\
         <div class=stat><b style=color:var(--fail)>{fail}</b>fail (below 60)</div></div>\
         <div class=stats><div class=stat><b>{go_skins}</b>skins Go alone got wrong</div>\
         <div class=stat><b>{rust_skins}</b>skins Rust alone got wrong</div></div>",
        ratings.len()
    );

    h.push_str("<h2>Which package got it wrong</h2>");
    h.push_str(&sides_summary(&ratings.iter().collect::<Vec<_>>()));
    h.push_str(&sides_table(&ratings.iter().collect::<Vec<_>>()));
    h.push_str("<p class=muted>A problem on one side only is that package's own bug: the other one drew the \
         skin correctly. Problems on both sides belong to the skin. <b>Go only</b> means the Go library is wrong \
         and this crate is right; <b>Rust only</b> means the reverse.</p>");

    h.push_str("<h2>What the checks look for</h2>");
    h.push_str(&checks_table(&kinds));

    let card = |h: &mut String, r: &Rating| {
        h.push_str("<div class=card>");
        h.push_str(&head(r));
        h.push_str(&bullets(r));
        h.push_str(&pics(r));
        h.push_str("</div>");
    };

    let flagged: Vec<&Rating> = ratings.iter().filter(|r| r.grade != "pass").collect();
    let _ = write!(h, "<h2>Worth a look ({})</h2>", flagged.len());
    if flagged.is_empty() {
        h.push_str("<p>Nothing: every skin passed.</p>");
    }
    for r in &flagged {
        card(&mut h, r);
    }

    let passed: Vec<&Rating> = ratings.iter().filter(|r| r.grade == "pass").collect();
    let mut sample: Vec<&Rating> = passed.iter().copied().filter(|r| !r.gifs.is_empty()).collect();
    sample.sort_by_key(|a| std::cmp::Reverse(a.skin.players));
    let _ = write!(h, "<h2>Passed, most worn and custom models ({})</h2>", sample.len().min(80));
    for r in sample.iter().take(80) {
        card(&mut h, r);
    }

    let _ = write!(h, "<h2>Every passed skin ({})</h2><div class=grid>", passed.len());
    for r in &passed {
        if let Some(s) = &r.strip {
            let _ = write!(h, "<div class=mini><img loading=lazy src='img/{s}' alt=''><div><b>{}</b> <code>{}</code></div></div>", r.score, esc(&r.skin.id));
        }
    }
    h.push_str("</div></main></body></html>");
    fs::write(work.join("report.html"), h).unwrap();
    write_failures(work, ratings, compared_go);
    eprintln!(
        "{} skins: {pass} pass, {check} check, {fail} fail; {renders} images, {compared} compared with Go, {differ} skins differ",
        ratings.len()
    );
    eprintln!("wrote failures.html and failures.csv ({} skins below 60)", fail);
}

/// work/failures.html and work/failures.csv: only the skins that failed, so
/// they are not buried under tens of thousands of passes. Grouped by what went
/// wrong, worst check first, because one cause usually explains many skins.
fn write_failures(work: &Path, ratings: &[Rating], compared_go: bool) {
    let failed: Vec<&Rating> = ratings.iter().filter(|r| r.grade == "fail").collect();
    let mut csv = String::from("id,score,verdict,identifier,players,geometry,cape,renders,checks,sides,detail\n");
    for r in &failed {
        // One entry per kind, with how many images it hit, so one row stays one
        // readable line however many renders a skin got wrong.
        let mut order: Vec<(&str, Side)> = Vec::new();
        let mut groups: HashMap<(&str, Side), (u32, usize, Vec<&str>)> = HashMap::new();
        for i in &r.issues {
            let key = (i.kind.as_str(), i.side);
            if !order.contains(&key) {
                order.push(key);
            }
            let g = groups.entry(key).or_insert((i.penalty, 0, Vec::new()));
            g.0 = g.0.max(i.penalty);
            g.1 += 1;
            if g.2.len() < 3 {
                g.2.push(&i.detail);
            }
        }
        let kinds: Vec<String> = order.iter().map(|(k, _)| (*k).to_string()).collect();
        let sides: Vec<String> =
            order.iter().map(|(k, s)| format!("{k}={}", s.label())).collect();
        let detail: Vec<String> = order
            .iter()
            .map(|key| {
                let (penalty, count, examples) = &groups[key];
                format!(
                    "{} [-{}] x{}: {}",
                    key.0,
                    penalty,
                    count,
                    examples.join(" | ")
                )
            })
            .collect();
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{}",
            r.skin.id,
            r.score,
            r.verdict,
            csv_field(&r.skin.identifier),
            r.skin.players,
            r.skin.geometry,
            r.skin.cape,
            r.renders,
            csv_field(&kinds.join("; ")),
            csv_field(&sides.join("; ")),
            csv_field(&detail.join(" ~ ")),
        );
    }
    fs::write(work.join("failures.csv"), csv).unwrap();

    let mut h = String::from(HEAD);
    let _ = write!(h, "<title>Skin check failures</title>");
    let _ = write!(
        h,
        "<h1>Failures ({})</h1><p class=muted>Skins that scored below 60. Every skin was rendered in 8 views, \
         with its cape, and every frame of all 37 animations{}, and each image was compared with the Go version's. \
         Grouped by the check that caught it, so one cause is visible across all the skins it affects.</p>\
         <nav><a href='report.html'>report.html</a><a href='failures.csv'>failures.csv</a></nav>",
        failed.len(),
        if compared_go { "" } else { " (not compared with Go this run)" }
    );
    if failed.is_empty() {
        h.push_str("<p class=muted>None: every skin scored 60 or more.</p>");
    } else {
        let refs: Vec<&Rating> = failed.to_vec();
        h.push_str("<h2>Which package got it wrong</h2>");
        h.push_str(&sides_summary(&refs));
        h.push_str(&sides_table(&refs));

        h.push_str("<h2>What went wrong</h2>");
        h.push_str(&checks_table(&kinds_of(failed.iter().copied())));
        for (kind, _, what) in CHECKS {
            let group: Vec<&Rating> = failed.iter().copied().filter(|r| r.issues.iter().any(|i| i.kind == kind)).collect();
            if group.is_empty() {
                continue;
            }
            let _ = write!(
                h,
                "<details><summary>{} — {} skin{}</summary><p class=muted>{what}</p>",
                kind,
                group.len(),
                if group.len() == 1 { "" } else { "s" }
            );
            for r in group {
                h.push_str("<div class=card>");
                h.push_str(&head(r));
                h.push_str(&bullets(r));
                h.push_str(&pics(r));
                h.push_str("</div>");
            }
            h.push_str("</details>");
        }
    }
    h.push_str("</main></body></html>");
    fs::write(work.join("failures.html"), h).unwrap();
}

/// One CSV field: quoted when it holds a comma, a quote or a newline.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
