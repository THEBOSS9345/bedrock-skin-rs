//! Renders a skin file from the command line.
//!
//! ```text
//! cargo run --example render -- skin.png out.png [view] [angle] [size]
//! cargo run --example render -- skin.png out.gif walk
//! cargo run --example render -- --geometry=geometry.json skin.png out.png
//! ```
//!
//! view is body, chest, head or avatar; a motion name (walk, idle, wave,
//! sneak) or an example animation (dance, backflip, ...) makes a GIF.

use std::env;
use std::fs;

use bedrock_skin::{
    AnimationOptions, Motion, RenderOptions, decode_image, example_animations, parse_angle,
    parse_geometry, parse_view,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (flags, args): (Vec<String>, Vec<String>) =
        env::args().skip(1).partition(|a| a.starts_with("--"));
    let geometry = match flags.iter().find_map(|f| f.strip_prefix("--geometry=")) {
        Some(path) => parse_geometry(&fs::read(path)?)?,
        None => Vec::new(),
    };
    let [input, output, rest @ ..] = args.as_slice() else {
        eprintln!(
            "usage: render [--geometry=geometry.json] <skin.png> <out.png|out.gif> [view|motion] [angle] [size]"
        );
        std::process::exit(2);
    };
    let texture = decode_image(&fs::read(input)?)?;
    let what = rest.first().map(String::as_str).unwrap_or("body");
    let angle = parse_angle(rest.get(1).map(String::as_str).unwrap_or(""))?;
    let size = rest.get(2).map(|s| s.parse()).transpose()?.unwrap_or(256);

    let mut opts = RenderOptions::new(&texture).geometry(&geometry).size(size);
    opts.angle = angle;

    if let Ok(motion) = what.parse::<Motion>() {
        fs::write(output, AnimationOptions::new(opts, &motion).render_gif()?)?;
    } else if let Some(anim) = example_animations().get(&format!("animation.player.{what}")) {
        fs::write(output, AnimationOptions::new(opts, anim).render_gif()?)?;
    } else {
        fs::write(output, opts.view(parse_view(what)?).render_png()?)?;
    }
    println!("wrote {output}");
    Ok(())
}
