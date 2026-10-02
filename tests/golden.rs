//! The Go version's golden renders: the same options must give the same
//! pixels. testdata/golden is copied from bedrock-skin-go.

use bedrock_skin::{Angle, Camera, RenderOptions, View, decode_image};
use image::RgbaImage;

/// The procedural texture the Go tests use: red by x, green by y.
pub fn test_texture() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])
    })
}

/// The largest channel difference, and how many pixels differ at all.
pub fn compare(got: &RgbaImage, want: &RgbaImage) -> (u8, usize) {
    assert_eq!(got.dimensions(), want.dimensions(), "size");
    let mut worst = 0;
    let mut differing = 0;
    for (a, b) in got.pixels().zip(want.pixels()) {
        let d =
            a.0.iter()
                .zip(b.0.iter())
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap();
        if d > 0 {
            differing += 1;
        }
        worst = worst.max(d);
    }
    (worst, differing)
}

#[test]
fn golden_renders_match_go() {
    let tex = test_texture();
    let base = || RenderOptions::new(&tex).size(96);
    let cases: Vec<(&str, RenderOptions)> = vec![
        ("body-front", base().view(View::Body).angle(Angle::Front)),
        ("body-iso", base().view(View::Body).angle(Angle::Iso)),
        ("chest-front", base().view(View::Chest)),
        ("head-default", base().view(View::Head)),
        ("avatar", base().view(View::Avatar)),
        ("slim", base().identifier("geometry.humanoid.customSlim")),
        ("body-cape", base().cape(&tex)),
        ("parts-head-arm", base().parts(["head", "leftArm"])),
        (
            "camera-explicit",
            base().camera(Camera {
                yaw: 200.0,
                pitch: -15.0,
                fov: 50.0,
                margin: 1.2,
            }),
        ),
    ];
    let mut failed = Vec::new();
    for (name, opts) in cases {
        let got = opts.render().unwrap();
        let want =
            decode_image(&std::fs::read(format!("testdata/golden/{name}.png")).unwrap()).unwrap();
        let (worst, differing) = compare(&got, &want);
        if differing > 0 {
            got.save(format!("target/{name}-rust.png")).unwrap();
            failed.push(format!(
                "{name}: {differing} pixels differ, worst by {worst}"
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
