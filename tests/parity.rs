//! Checks the port against bedrock-skin-go: testdata/parity holds what the
//! Go library produced for the same inputs (see tools/parity), and every
//! image, pose, report and query result here must come out the same.

use std::collections::BTreeMap;
use std::fs;

use bedrock_skin::*;
use image::{Rgba, RgbaImage};
use serde::Deserialize;
use serde_json::Value;

const DIR: &str = "testdata/parity";

fn read(name: &str) -> Vec<u8> {
    fs::read(format!("{DIR}/{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn json(name: &str) -> Value {
    serde_json::from_slice(&read(name)).unwrap()
}

fn test_texture() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])
    })
}

/// An armor layer, 64x32 as the game's are, with a transparent patch where
/// real chestplates leave the lower arm bare.
fn armor_texture(tint: u8) -> RgbaImage {
    RgbaImage::from_fn(64, 32, |x, y| {
        let a = if (40..56).contains(&x) && y >= 26 {
            0
        } else {
            255
        };
        Rgba([tint, (120 + x * 2) as u8, (140 + y * 3) as u8, a])
    })
}

/// An item sprite of side n: a diagonal blade whose alpha sweeps every
/// value, to pin the held item's alpha cut-off.
fn item_texture(n: u32) -> RgbaImage {
    RgbaImage::from_fn(n, n, |x, y| {
        let d = x as i32 + y as i32 - (n as i32 - 1);
        if !(-1..=1).contains(&d) {
            return Rgba([0, 0, 0, 0]);
        }
        Rgba([
            (x * 16) as u8,
            (200 - y * 5) as u8,
            (60 + d * 50) as u8,
            ((x * 37 + y * 11) % 256) as u8,
        ])
    })
}

fn semi_texture() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([
            (x * 4 + 3) as u8,
            (255 - y * 4) as u8,
            (x * y) as u8,
            ((x * 7 + y * 13) % 256) as u8,
        ])
    })
}

fn custom_texture() -> RgbaImage {
    RgbaImage::from_fn(128, 128, |x, y| {
        Rgba([
            (x * 2) as u8,
            (y * 2) as u8,
            (x ^ y) as u8,
            if x >= 96 { 0 } else { 255 },
        ])
    })
}

fn legacy_texture() -> RgbaImage {
    RgbaImage::from_fn(64, 32, |x, y| Rgba([(x * 4) as u8, (y * 8) as u8, 60, 255]))
}

fn face_texture() -> RgbaImage {
    RgbaImage::from_fn(32, 64, |x, y| {
        let a = if (16..32).contains(&y) {
            ((x * y * 5) % 256) as u8
        } else {
            255
        };
        Rgba([(x * 8) as u8, (y * 4) as u8, 200, a])
    })
}

fn head_only() -> RgbaImage {
    let mut img = test_texture();
    for (x, y, p) in img.enumerate_pixels_mut() {
        if !((8..16).contains(&x) && (8..16).contains(&y)) {
            p.0[3] = 0;
        }
    }
    img
}

fn bench() -> (RgbaImage, Vec<Geometry>) {
    let tex = decode_image(&fs::read("testdata/bench-skin/texture.png").unwrap()).unwrap();
    let geos = parse_geometry(&fs::read("testdata/bench-skin/geometry.json").unwrap()).unwrap();
    (tex, geos)
}

fn parsed(name: &str) -> Vec<Geometry> {
    parse_geometry(&read(name)).unwrap()
}

fn assert_same_image(name: &str, got: &RgbaImage, want_png: &[u8]) {
    let want = decode_image(want_png).unwrap();
    assert_eq!(got.dimensions(), want.dimensions(), "{name}: size");
    let differing = got
        .pixels()
        .zip(want.pixels())
        .filter(|(a, b)| a != b)
        .count();
    if differing > 0 {
        let _ = got.save(format!("target/parity-{name}.png"));
        panic!("{name}: {differing} pixels differ from the Go render (Rust's is in target/)");
    }
}

#[test]
fn renders_match_go() {
    let (bench, bench_geo) = bench();
    let (test, semi, custom) = (test_texture(), semi_texture(), custom_texture());
    let legacy_tex = legacy_texture();
    let (custom_geo, persona_geo, legacy_geo) = (
        parsed("custom-geometry.json"),
        parsed("persona-geometry.json"),
        parsed("legacy-geometry.json"),
    );
    let (mesh_geo, odd_geo, companion_geo) = (
        parsed("persona-mesh-geometry.json"),
        parsed("persona-mesh-odd.json"),
        parsed("persona-companion-geometry.json"),
    );
    let face = face_texture();
    let scaled: Pose = [
        (
            "head",
            BonePose {
                scale: [1.5; 3],
                scaled: true,
                rotation: [0.0, 30.0, 0.0],
                ..Default::default()
            },
        ),
        (
            "rightarm",
            BonePose {
                scale: [0.0; 3],
                scaled: true,
                ..Default::default()
            },
        ),
        (
            "leftLeg",
            BonePose {
                position: [0.0, 2.0, -3.0],
                rotation: [-40.0, 0.0, 0.0],
                ..Default::default()
            },
        ),
    ]
    .into_iter()
    .collect();
    let cam = |yaw, pitch, fov, margin| Camera {
        yaw,
        pitch,
        fov,
        margin,
    };

    let cases: Vec<(&str, RenderOptions)> = vec![
        (
            "bench-body-iso",
            RenderOptions::new(&bench)
                .geometry(&bench_geo)
                .angle(Angle::Iso)
                .size(128),
        ),
        (
            "bench-avatar",
            RenderOptions::new(&bench)
                .geometry(&bench_geo)
                .view(View::Avatar)
                .size(100),
        ),
        (
            "bench-cape",
            RenderOptions::new(&bench)
                .geometry(&bench_geo)
                .cape(&test)
                .size(120),
        ),
        (
            "bench-chest-camera",
            RenderOptions::new(&bench)
                .geometry(&bench_geo)
                .view(View::Chest)
                .camera(cam(-40.0, 10.0, 0.0, 0.0))
                .size(90),
        ),
        (
            "custom-body",
            RenderOptions::new(&custom)
                .geometry(&custom_geo)
                .angle(Angle::Iso)
                .size(128),
        ),
        (
            "custom-back",
            RenderOptions::new(&custom)
                .geometry(&custom_geo)
                .camera(cam(160.0, 30.0, 0.0, 0.0))
                .cape(&test)
                .size(100),
        ),
        (
            "custom-head",
            RenderOptions::new(&custom)
                .geometry(&custom_geo)
                .view(View::Head)
                .size(80),
        ),
        (
            "custom-parts",
            RenderOptions::new(&custom)
                .geometry(&custom_geo)
                .parts(["tail", "horn"])
                .size(64),
        ),
        (
            "semi-body",
            RenderOptions::new(&semi).angle(Angle::Iso).size(96),
        ),
        (
            "close-camera",
            RenderOptions::new(&test)
                .camera(cam(30.0, 20.0, 70.0, 0.35))
                .size(96),
        ),
        (
            "inside-camera",
            RenderOptions::new(&test)
                .camera(cam(180.0, -5.0, 90.0, 0.1))
                .size(96),
        ),
        (
            "persona-body",
            RenderOptions::new(&semi).geometry(&persona_geo).size(100),
        ),
        (
            "persona-chest",
            RenderOptions::new(&semi)
                .geometry(&persona_geo)
                .view(View::Chest)
                .size(77),
        ),
        (
            "persona-head",
            RenderOptions::new(&semi)
                .geometry(&persona_geo)
                .view(View::Head)
                .size(64),
        ),
        (
            "persona-avatar-8",
            RenderOptions::new(&semi)
                .geometry(&persona_geo)
                .view(View::Avatar)
                .size(8),
        ),
        (
            "persona-128",
            RenderOptions::new(&custom).geometry(&persona_geo).size(50),
        ),
        (
            "legacy-body",
            RenderOptions::new(&test).geometry(&legacy_geo).size(64),
        ),
        (
            "legacy-alpha",
            RenderOptions::new(&legacy_tex)
                .geometry(&legacy_geo)
                .identifier("geometry.alpha")
                .size(64),
        ),
        (
            "sneak-still",
            RenderOptions::new(&test)
                .pose(Motion::Sneak.pose(0.4))
                .size(96),
        ),
        (
            "scaled-pose",
            RenderOptions::new(&test)
                .pose(scaled.clone())
                .angle(Angle::Iso)
                .size(96),
        ),
        ("tiny", RenderOptions::new(&test).view(View::Avatar).size(3)),
        (
            "mesh-body",
            RenderOptions::new(&test).geometry(&mesh_geo).size(96),
        ),
        (
            "mesh-face-iso",
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "mesh-face-head",
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .view(View::Head)
                .size(80),
        ),
        (
            "mesh-face-avatar",
            RenderOptions::new(&semi)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .view(View::Avatar)
                .size(64),
        ),
        (
            "mesh-face-chest",
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .view(View::Chest)
                .cape(&semi)
                .size(72),
        ),
        (
            "mesh-face-back",
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .cape(&semi)
                .camera(Camera {
                    yaw: 150.0,
                    pitch: 20.0,
                    ..Camera::default()
                })
                .size(96),
        ),
        (
            "mesh-parts-hat",
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .parts(["HAT", "leftArm"])
                .size(64),
        ),
        (
            "mesh-odd",
            RenderOptions::new(&test)
                .geometry(&odd_geo)
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "mesh-odd-head",
            RenderOptions::new(&semi)
                .geometry(&odd_geo)
                .view(View::Head)
                .size(64),
        ),
        (
            "mesh-odd-chest",
            RenderOptions::new(&test)
                .geometry(&odd_geo)
                .view(View::Chest)
                .size(64),
        ),
        (
            "mesh-companion",
            RenderOptions::new(&test)
                .geometry(&companion_geo)
                .animated(AnimatedType::Body128, &semi)
                .animated(AnimatedType::Face, &face)
                .angle(Angle::Iso)
                .size(96),
        ),
    ];
    let (a40, a150, a200, a90, a220) = (
        armor_texture(40),
        armor_texture(150),
        armor_texture(200),
        armor_texture(90),
        armor_texture(220),
    );
    let (item16, item24) = (item_texture(16), item_texture(24));
    let diamond = Armor::set(&a40, &a150);
    let winged = Armor {
        elytra: Some(&a220),
        ..diamond
    };
    let sword = Held::new(&item16);
    let flat = Held {
        flat: true,
        ..Held::new(&item16)
    };
    let mut cases = cases;
    cases.extend([
        (
            "armor-iso",
            RenderOptions::new(&test)
                .armor(diamond)
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "armor-back",
            RenderOptions::new(&semi)
                .armor(diamond)
                .cape(&test)
                .camera(cam(160.0, 25.0, 0.0, 0.0))
                .size(96),
        ),
        (
            "armor-mixed-slim",
            RenderOptions::new(&test)
                .identifier("geometry.humanoid.customSlim")
                .armor(Armor {
                    helmet: Some(&a200),
                    boots: Some(&a90),
                    ..Armor::default()
                })
                .angle(Angle::Iso)
                .size(80),
        ),
        (
            "armor-avatar",
            RenderOptions::new(&test)
                .armor(diamond)
                .right_hand(sword)
                .view(View::Avatar)
                .size(64),
        ),
        (
            "armor-chest",
            RenderOptions::new(&test)
                .armor(diamond)
                .right_hand(sword)
                .left_hand(flat)
                .view(View::Chest)
                .size(72),
        ),
        (
            "elytra-back",
            RenderOptions::new(&test)
                .armor(winged)
                .camera(cam(170.0, 15.0, 0.0, 0.0))
                .size(96),
        ),
        (
            "elytra-side",
            RenderOptions::new(&semi)
                .armor(winged)
                .right_hand(sword)
                .camera(cam(-70.0, -20.0, 0.0, 0.0))
                .size(96),
        ),
        (
            "held-side",
            RenderOptions::new(&test)
                .right_hand(sword)
                .camera(cam(-70.0, 10.0, 0.0, 0.0))
                .size(96),
        ),
        (
            "held-24",
            RenderOptions::new(&semi)
                .right_hand(Held::new(&item24))
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "held-left",
            RenderOptions::new(&test)
                .left_hand(sword)
                .camera(cam(60.0, 10.0, 0.0, 0.0))
                .size(96),
        ),
        (
            "held-both-flat",
            RenderOptions::new(&test)
                .right_hand(flat)
                .left_hand(Held {
                    flat: true,
                    ..Held::new(&item24)
                })
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "held-adjust",
            RenderOptions::new(&test)
                .right_hand(Held {
                    adjust: ItemAdjust {
                        offset: [0.5, 2.0, -1.25],
                        rotation: [37.0, -20.0, 11.0],
                        scale: 1.3,
                    },
                    ..Held::new(&item16)
                })
                .left_hand(Held {
                    flat: true,
                    adjust: ItemAdjust {
                        rotation: [0.0, 90.0, 0.0],
                        scale: 0.7,
                        ..ItemAdjust::default()
                    },
                    ..Held::new(&item16)
                })
                .camera(cam(-35.0, 15.0, 0.0, 0.0))
                .size(96),
        ),
        (
            "held-slim",
            RenderOptions::new(&test)
                .identifier("geometry.humanoid.customSlim")
                .right_hand(sword)
                .left_hand(sword)
                .pose(Motion::Wave.pose(0.3))
                .size(80),
        ),
        (
            "held-parts",
            RenderOptions::new(&test)
                .right_hand(sword)
                .left_hand(sword)
                .armor(diamond)
                .parts(["rightArm"])
                .size(64),
        ),
        (
            "held-custom",
            RenderOptions::new(&custom)
                .geometry(&custom_geo)
                .armor(winged)
                .right_hand(sword)
                .left_hand(sword)
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "held-mesh",
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .armor(diamond)
                .right_hand(sword)
                .left_hand(flat)
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "scale-parts",
            RenderOptions::new(&test)
                .armor(diamond)
                .right_hand(sword)
                .scale(Scale {
                    parts: [
                        ("HEAD", 1.6),
                        ("rightarm", 1.3),
                        ("leftLeg", 0.0),
                        ("Body", 0.9),
                    ]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                    ..Scale::default()
                })
                .pose(scaled.clone())
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "scale-model-big",
            RenderOptions::new(&test)
                .armor(winged)
                .right_hand(sword)
                .scale(Scale {
                    model: 1.7,
                    ..Scale::default()
                })
                .angle(Angle::Iso)
                .size(80),
        ),
        (
            "scale-model-small",
            RenderOptions::new(&semi)
                .scale(Scale {
                    model: 0.45,
                    ..Scale::default()
                })
                .camera(cam(20.0, 5.0, 0.0, 1.1))
                .size(80),
        ),
        (
            "solo-armor",
            RenderOptions::equipment()
                .armor(winged)
                .right_hand(sword)
                .angle(Angle::Iso)
                .size(96),
        ),
        (
            "solo-helmet",
            RenderOptions::equipment()
                .armor(diamond)
                .view(View::Head)
                .size(64),
        ),
        (
            "solo-hand",
            RenderOptions::equipment()
                .left_hand(flat)
                .camera(cam(50.0, 10.0, 0.0, 0.0))
                .size(64),
        ),
        (
            "solo-cape-parts",
            RenderOptions::equipment()
                .cape(&test)
                .armor(diamond)
                .parts(["cape", "leftLeg"])
                .size(64),
        ),
    ]);
    let items = [
        ("item-front", ItemOptions::new(&item16).size(64)),
        (
            "item-iso",
            ItemOptions::new(&item24).angle(Angle::Iso).size(80),
        ),
        (
            "item-camera",
            ItemOptions::new(&item16)
                .camera(cam(130.0, -25.0, 50.0, 1.4))
                .size(72),
        ),
        (
            "item-adjust",
            ItemOptions::new(&item16)
                .adjust(ItemAdjust {
                    offset: [3.0, -1.0, 2.0],
                    rotation: [20.0, 33.0, -45.0],
                    scale: 2.5,
                })
                .size(64),
        ),
    ];
    let mut failures = Vec::new();
    for (name, opts) in &cases {
        let got = opts.render().unwrap();
        let want = read(&format!("renders/{name}.png"));
        if let Err(e) = std::panic::catch_unwind(|| assert_same_image(name, &got, &want)) {
            failures.push(e.downcast_ref::<String>().cloned().unwrap_or_default());
        }
    }
    for (name, opts) in &items {
        let got = opts.render().unwrap();
        let want = read(&format!("renders/{name}.png"));
        if let Err(e) = std::panic::catch_unwind(|| assert_same_image(name, &got, &want)) {
            failures.push(e.downcast_ref::<String>().cloned().unwrap_or_default());
        }
    }
    assert_eq!(
        fs::read_dir(format!("{DIR}/renders")).unwrap().count(),
        cases.len() + items.len(),
        "every Go render is checked"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn animation_frames_match_go() {
    let test = test_texture();
    let custom = custom_texture();
    let custom_geo = parsed("custom-geometry.json");
    let molang = parse_animations(&read("molang-test.animation.json")).unwrap();
    let ex = example_animations();

    let mut anims: Vec<(String, &dyn Animator)> = vec![
        ("walk".into(), &Motion::Walk),
        ("idle".into(), &Motion::Idle),
        ("wave".into(), &Motion::Wave),
        ("sneak".into(), &Motion::Sneak),
    ];
    for n in [
        "dance",
        "backflip",
        "jumping_jacks",
        "spin",
        "sword_swing",
        "zombie_walk",
        "levitate",
        "sit",
        "swim",
        "airplane",
    ] {
        anims.push((n.into(), &ex[&format!("animation.player.{n}")]));
    }
    let mut checked = 0;
    let mut check = |name: &str, opts: AnimationOptions| {
        let frames = opts.render_frames().unwrap();
        for (i, f) in frames.iter().enumerate() {
            assert_same_image(
                &format!("{name}-{i:02}"),
                f,
                &read(&format!("frames/{name}-{i:02}.png")),
            );
        }
        assert!(
            fs::metadata(format!("{DIR}/frames/{name}-{:02}.png", frames.len())).is_err(),
            "{name}: frame count"
        );
        checked += frames.len();
    };
    for (name, a) in &anims {
        check(
            name,
            AnimationOptions::new(RenderOptions::new(&test).size(64), *a).fps(6),
        );
    }
    let mesh_geo = parsed("persona-mesh-geometry.json");
    let face = face_texture();
    check(
        "mesh-walk",
        AnimationOptions::new(
            RenderOptions::new(&test)
                .geometry(&mesh_geo)
                .animated(AnimatedType::Face, &face)
                .angle(Angle::Iso)
                .size(64),
            &Motion::Walk,
        )
        .fps(4),
    );
    let mol = &molang["animation.parity.molang"];
    check(
        "molang",
        AnimationOptions::new(
            RenderOptions::new(&custom)
                .geometry(&custom_geo)
                .angle(Angle::Iso)
                .size(72),
            mol,
        )
        .fps(5),
    );
    let (a40, a150, a220, item16) = (
        armor_texture(40),
        armor_texture(150),
        armor_texture(220),
        item_texture(16),
    );
    let winged = Armor {
        elytra: Some(&a220),
        ..Armor::set(&a40, &a150)
    };
    let equipped = RenderOptions::new(&test)
        .armor(winged)
        .right_hand(Held::new(&item16))
        .left_hand(Held {
            flat: true,
            adjust: ItemAdjust {
                rotation: [10.0, 0.0, 0.0],
                ..ItemAdjust::default()
            },
            ..Held::new(&item16)
        })
        .angle(Angle::Iso)
        .size(64);
    check(
        "armored-walk",
        AnimationOptions::new(equipped.clone(), &Motion::Walk).fps(4),
    );
    check(
        "armored-sneak",
        AnimationOptions::new(equipped, &Motion::Sneak).fps(4),
    );
    check(
        "solo-walk",
        AnimationOptions::new(
            RenderOptions::equipment()
                .armor(winged)
                .right_hand(Held::new(&item16))
                .angle(Angle::Iso)
                .size(64),
            &Motion::Walk,
        )
        .fps(4),
    );
    let spin = render_item_frames(&ItemAnimationOptions {
        duration: 1.5,
        fps: 4,
        ..ItemAnimationOptions::new(
            ItemOptions::new(&item16)
                .camera(Camera {
                    yaw: 0.0,
                    pitch: 15.0,
                    fov: 0.0,
                    margin: 0.0,
                })
                .adjust(ItemAdjust {
                    rotation: [0.0, 0.0, 20.0],
                    ..ItemAdjust::default()
                })
                .size(48),
        )
    })
    .unwrap();
    for (i, f) in spin.iter().enumerate() {
        assert_same_image(
            &format!("item-spin-{i:02}"),
            f,
            &read(&format!("frames/item-spin-{i:02}.png")),
        );
    }
    assert!(
        fs::metadata(format!("{DIR}/frames/item-spin-{:02}.png", spin.len())).is_err(),
        "item-spin: frame count"
    );
    checked += spin.len();
    assert_eq!(
        checked,
        fs::read_dir(format!("{DIR}/frames")).unwrap().count()
    );
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GoBonePose {
    rotation: [f64; 3],
    position: [f64; 3],
    scale: [f64; 3],
    scaled: bool,
}

#[test]
fn poses_match_go() {
    let molang = parse_animations(&read("molang-test.animation.json")).unwrap();
    let ex = example_animations();
    let cases = json("poses.json");
    let mut n = 0;
    for c in cases.as_array().unwrap() {
        let name = c["Animation"].as_str().unwrap();
        let t = c["T"].as_f64().unwrap();
        let pose = if let Ok(m) = name.parse::<Motion>() {
            m.pose(t)
        } else if let Some(a) = molang.get(name) {
            a.pose(t)
        } else {
            ex[name].pose(t)
        };
        let want: BTreeMap<String, GoBonePose> =
            serde_json::from_value(c["Pose"].clone()).unwrap_or_default();
        assert_eq!(pose.len(), want.len(), "{name} at {t}: bones");
        for (bone, w) in &want {
            let got = pose
                .iter()
                .find(|(b, _)| b == bone)
                .unwrap_or_else(|| panic!("{name} at {t}: no {bone}"))
                .1;
            let bits = |v: [f64; 3]| v.map(f64::to_bits);
            assert_eq!(
                (
                    bits(got.rotation),
                    bits(got.position),
                    bits(got.scale),
                    got.scaled
                ),
                (bits(w.rotation), bits(w.position), bits(w.scale), w.scaled),
                "{name} at {t}, {bone}: {got:?}"
            );
        }
        n += 1;
    }
    assert!(n > 200);
}

/// Numbers compare by value: Go writes 64.0 as 64.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_json(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w)))
        }
        _ => a == b,
    }
}

fn null_as_empty(v: &Value) -> Value {
    if v.is_null() {
        Value::Array(vec![])
    } else {
        v.clone()
    }
}

fn visibility_json(r: &SkinVisibilityResult) -> Value {
    serde_json::json!({
        "IsInvisible": r.is_invisible,
        "Pass": r.pass,
        "Suspicious": r.suspicious,
        "Parts": r.parts.iter().map(|p| serde_json::json!({
            "Name": p.name, "Visible": p.visible, "Fraction": p.fraction, "Pixels": p.pixels,
            "Transparent": p.transparent, "FromGeo": p.from_geo, "Tiny": p.tiny,
        })).collect::<Vec<_>>(),
        "VisibleParts": r.visible_parts,
        "InvisibleParts": r.invisible_parts,
    })
}

fn geometry_size_json(r: &GeometrySizeResult) -> Value {
    serde_json::json!({
        "Pass": r.pass,
        "Violations": r.violations.iter().map(|v| serde_json::json!({"Bone": v.bone, "Size": v.size, "Minimum": v.minimum})).collect::<Vec<_>>(),
    })
}

#[test]
fn reports_match_go() {
    let custom = read("custom-geometry.json");
    let bench_tex = decode_image(&fs::read("testdata/bench-skin/texture.png").unwrap()).unwrap();
    let bench_geo = fs::read("testdata/bench-skin/geometry.json").unwrap();
    let cases: Vec<(&str, RgbaImage, Vec<u8>, SkinOptions)> = vec![
        ("standard", test_texture(), vec![], SkinOptions::default()),
        (
            "transparent",
            RgbaImage::new(64, 64),
            vec![],
            SkinOptions::default(),
        ),
        ("head-only", head_only(), vec![], SkinOptions::default()),
        ("legacy32", legacy_texture(), vec![], SkinOptions::default()),
        (
            "custom",
            custom_texture(),
            custom.clone(),
            SkinOptions::default(),
        ),
        (
            "custom-128",
            custom_texture(),
            read("legacy-geometry.json"),
            SkinOptions::default(),
        ),
        (
            "persona",
            semi_texture(),
            read("persona-geometry.json"),
            SkinOptions::default(),
        ),
        (
            "mesh",
            test_texture(),
            read("persona-mesh-geometry.json"),
            SkinOptions::default(),
        ),
        (
            "mesh-semi",
            semi_texture(),
            read("persona-mesh-geometry.json"),
            SkinOptions::default(),
        ),
        (
            "mesh-odd",
            semi_texture(),
            read("persona-mesh-odd.json"),
            SkinOptions::default(),
        ),
        (
            "mesh-128",
            custom_texture(),
            read("persona-mesh-geometry.json"),
            SkinOptions::default(),
        ),
        (
            "companion",
            head_only(),
            read("persona-companion-geometry.json"),
            SkinOptions::default(),
        ),
        (
            "garbage-geo",
            test_texture(),
            b"{nope".to_vec(),
            SkinOptions::default(),
        ),
        ("semi", semi_texture(), vec![], SkinOptions::default()),
        (
            "strict",
            semi_texture(),
            vec![],
            SkinOptions {
                min_visible_fraction: 0.9,
                min_visible_parts: 6,
                ..Default::default()
            },
        ),
        (
            "big-min",
            custom_texture(),
            custom.clone(),
            SkinOptions {
                min_geometry_size: 3.0,
                ..Default::default()
            },
        ),
        ("bench", bench_tex, bench_geo, SkinOptions::default()),
    ];
    let want = json("reports.json");
    assert_eq!(want.as_object().unwrap().len(), cases.len());
    for (name, tex, geo, opts) in cases {
        let w = &want[name];
        let skin = Skin::with_options(tex.clone(), Some(&geo), opts);
        let mut want_report = w["Report"].clone();
        want_report["parts"] = null_as_empty(&want_report["parts"]);
        let want_report: SkinReport = serde_json::from_value(want_report).unwrap();
        assert_eq!(skin.report(), &want_report, "{name}: report");

        let mut want_vis = w["Visibility"].clone();
        want_vis["Parts"] = null_as_empty(&want_vis["Parts"]);
        let got_vis = visibility_json(&validate_skin_visibility(&tex, &geo, 0.3));
        assert!(
            same_json(&got_vis, &want_vis),
            "{name}: visibility\n got {got_vis}\nwant {want_vis}"
        );

        let mut want_size = w["GeometrySize"].clone();
        want_size["Violations"] = null_as_empty(&want_size["Violations"]);
        let got_size = geometry_size_json(&validate_geometry_size(&geo, 0.0));
        assert!(
            same_json(&got_size, &want_size),
            "{name}: geometry size\n got {got_size}\nwant {want_size}"
        );

        assert_eq!(
            is_skin_invisible(&tex),
            w["IsInvisible"].as_bool().unwrap(),
            "{name}: is_skin_invisible"
        );
        assert_eq!(
            is_skin_tiny(&geo),
            w["IsTiny"].as_bool().unwrap(),
            "{name}: is_skin_tiny"
        );
        let want_list: Vec<String> =
            serde_json::from_value(null_as_empty(&w["InvisibleList"])).unwrap();
        assert_eq!(skin.invisible_parts(), want_list, "{name}: invisible parts");
    }
}

#[test]
fn queries_match_go() {
    let cases = json("queries.json");
    for c in cases.as_array().unwrap() {
        let (file, path) = (c["File"].as_str().unwrap(), c["Path"].as_str().unwrap());
        let tree = parse_geometry_tree(&read(file)).unwrap();
        let got: Vec<Value> = tree
            .select(path)
            .into_iter()
            .map(|v| serde_json::json!({"Path": v.path, "Value": v.value}))
            .collect();
        let want = c["Values"].as_array().unwrap();
        assert!(
            same_json(&Value::Array(got.clone()), &Value::Array(want.clone())),
            "{file} {path:?}\n got {got:?}\nwant {want:?}"
        );
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct GoGeometry {
    identifier: String,
    texture_width: f64,
    texture_height: f64,
    bones: Option<Vec<Bone>>,
    visible_bounds_width: f64,
    visible_bounds_height: f64,
    visible_bounds_offset: Option<Vec<f64>>,
}

#[test]
fn geometry_parsing_matches_go() {
    let cases = json("geometry.json");
    for c in cases.as_array().unwrap() {
        let input = c["Input"].as_str().unwrap();
        let want_err = c["Error"].as_bool().unwrap();
        if let Some(patch) = input.strip_prefix("patch:") {
            let got = parse_resource_patch(patch.as_bytes());
            assert_eq!(got.is_err(), want_err, "patch {patch}");
            if let Ok(p) = got {
                assert_eq!(
                    p.default,
                    c["Patch"]["Default"].as_str().unwrap(),
                    "patch {patch}"
                );
                assert_eq!(
                    p.cape,
                    c["Patch"]["Cape"].as_str().unwrap(),
                    "patch {patch}"
                );
            }
            continue;
        }
        let got = parse_geometry(input.as_bytes());
        assert_eq!(got.is_err(), want_err, "{input}");
        let Ok(got) = got else { continue };
        let want: Vec<GoGeometry> =
            serde_json::from_value(c["Geometries"].clone()).unwrap_or_default();
        assert_eq!(got.len(), want.len(), "{input}: entries");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(g.identifier, w.identifier);
            assert_eq!(
                (g.texture_width, g.texture_height),
                (w.texture_width, w.texture_height),
                "{}",
                g.identifier
            );
            assert_eq!(
                (g.visible_bounds_width, g.visible_bounds_height),
                (w.visible_bounds_width, w.visible_bounds_height)
            );
            assert_eq!(
                g.visible_bounds_offset,
                w.visible_bounds_offset.clone().unwrap_or_default()
            );
            assert_eq!(
                g.bones,
                w.bones.clone().unwrap_or_default(),
                "{}: bones",
                g.identifier
            );
        }
    }
}

#[test]
fn render_gif_round_trips() {
    // Few enough colours for the palette to hold them all exactly.
    let tex = RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([(x / 16 * 60) as u8, (y / 16 * 60) as u8, 90, 255])
    });
    let opts = AnimationOptions::new(RenderOptions::new(&tex).size(48), &Motion::Walk).fps(8);
    let frames = opts.render_frames().unwrap();
    let gif = opts.render_gif().unwrap();
    let mut dec = gif::DecodeOptions::new();
    dec.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = dec.read_info(gif.as_slice()).unwrap();
    let mut n = 0;
    while let Some(f) = dec.read_next_frame().unwrap() {
        assert_eq!(f.delay, 13);
        for (got, want) in f.buffer.chunks(4).zip(frames[n].pixels()) {
            if want.0[3] >= 128 {
                assert_eq!(&got[..3], &want.0[..3]);
            } else {
                assert_eq!(got[3], 0);
            }
        }
        n += 1;
    }
    assert_eq!(n, frames.len());
}
