//! The same renders as the Go version's benchmarks, timed with a plain loop
//! so there is no benchmark dependency: `cargo bench`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use bedrock_skin::{RenderOptions, View, decode_image, parse_geometry};
use image::{Rgba, RgbaImage};

fn bench(name: &str, f: impl Fn()) {
    f(); // warm up
    let start = Instant::now();
    let mut n = 0u32;
    while start.elapsed() < Duration::from_secs(2) {
        f();
        n += 1;
    }
    let per = start.elapsed() / n;
    println!(
        "{name:<16} {:>10.2} ms/op  ({n} runs)",
        per.as_secs_f64() * 1000.0
    );
}

fn main() {
    let test = RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])
    });
    let bench_tex =
        decode_image(&std::fs::read("testdata/bench-skin/texture.png").unwrap()).unwrap();
    let bench_geo =
        parse_geometry(&std::fs::read("testdata/bench-skin/geometry.json").unwrap()).unwrap();

    bench("RenderHead", || {
        black_box(
            RenderOptions::new(&bench_tex)
                .geometry(&bench_geo)
                .view(View::Head)
                .render()
                .unwrap(),
        );
    });
    bench("RenderBody", || {
        black_box(
            RenderOptions::new(&bench_tex)
                .geometry(&bench_geo)
                .render()
                .unwrap(),
        );
    });
    bench("RenderBody512", || {
        black_box(RenderOptions::new(&test).size(512).render().unwrap());
    });
    bench("RenderAvatar128", || {
        black_box(
            RenderOptions::new(&test)
                .view(View::Avatar)
                .size(128)
                .render()
                .unwrap(),
        );
    });
}
