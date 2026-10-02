//! The public API's behaviour: errors, defaults and edge cases.

use bedrock_skin::*;
use image::{Rgba, RgbaImage};

fn tex() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])
    })
}

#[test]
fn default_render_is_512_square() {
    let img = RenderOptions::new(&tex()).render().unwrap();
    assert_eq!(img.dimensions(), (512, 512));
    assert!(img.pixels().any(|p| p.0[3] == 255), "something is drawn");
    assert!(img.get_pixel(0, 0).0[3] == 0, "the background is clear");
}

#[test]
fn parsers_reject_unknown_names() {
    assert_eq!(parse_view(" Avatar ").unwrap(), View::Avatar);
    assert_eq!(parse_view("").unwrap(), View::Body);
    assert!(matches!(parse_view("avatr"), Err(Error::UnknownView(_))));
    assert_eq!(parse_angle("ISO").unwrap(), Some(Angle::Iso));
    assert_eq!(parse_angle("").unwrap(), None);
    assert!(matches!(parse_angle("side"), Err(Error::UnknownAngle(_))));
    assert_eq!(parse_parts(" head, ,leftArm "), vec!["head", "leftArm"]);
    assert!(parse_parts("  ").is_empty());
    assert_eq!("sneak".parse::<Motion>().unwrap(), Motion::Sneak);
    assert!(matches!(parse_motion("Walk"), Err(Error::UnknownMotion(_))));
}

#[test]
fn render_errors() {
    let t = tex();
    assert!(matches!(
        RenderOptions::new(&t).parts(["nope"]).render(),
        Err(Error::NoMatchingParts)
    ));
    let no_head = parse_geometry(
        br#"{"minecraft:geometry":[{"description":{"identifier":"g"},"bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#,
    )
    .unwrap();
    assert!(matches!(
        RenderOptions::new(&t)
            .geometry(&no_head)
            .view(View::Head)
            .render(),
        Err(Error::EmptyView)
    ));
    let empty = RgbaImage::new(0, 0);
    assert!(matches!(
        RenderOptions::new(&empty).render(),
        Err(Error::NoTexture)
    ));
    assert!(matches!(
        render_bytes(&BytesOptions::default()),
        Err(Error::NoTexture)
    ));
}

#[test]
fn geometry_helpers() {
    assert!(is_empty(b""));
    assert!(is_empty(b" null\n"));
    assert!(!is_empty(b"{}"));
    assert!(parse_geometry(b"null").unwrap().is_empty());
    assert!(parse_geometry(b"{").is_err());

    let geos = default_geometry();
    let ids: Vec<&str> = geos.iter().map(|g| g.identifier.as_str()).collect();
    for id in [
        "geometry.cape",
        "geometry.humanoid.custom",
        "geometry.humanoid.customSlim",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
    assert_eq!(
        select_geometry(geos, "").unwrap().identifier,
        "geometry.humanoid.custom"
    );
    assert_eq!(
        select_geometry(geos, "nope").unwrap().identifier,
        "geometry.humanoid.custom"
    );
    assert_eq!(find_cape(geos).unwrap().identifier, "geometry.cape");
    let (bones, cubes) = complexity(geos);
    assert!(bones > 10 && cubes > 10);

    let patch = parse_resource_patch(br#"{"geometry":{"default":"geometry.humanoid.customSlim"}}"#)
        .unwrap();
    assert_eq!(patch.default, "geometry.humanoid.customSlim");
    assert_eq!(
        parse_resource_patch(b"null").unwrap(),
        ResourcePatch::default()
    );
}

#[test]
fn bytes_path_matches_image_path() {
    let t = tex();
    let png = encode_png(&t).unwrap();
    assert_eq!(image_dimensions(&png).unwrap(), (64, 64));
    let via_bytes = render_bytes(&BytesOptions {
        texture: &png,
        geometry: b"null",
        view: View::Head,
        size: 64,
        ..Default::default()
    })
    .unwrap();
    let via_image = RenderOptions::new(&t)
        .view(View::Head)
        .size(64)
        .render()
        .unwrap();
    assert_eq!(decode_image(&via_bytes).unwrap(), via_image);
    assert!(decode_image(b"not an image").is_err());
    assert!(image_dimensions(&[]).is_err());
}

#[test]
fn raw_rgba_textures() {
    let t = texture_from_rgba(vec![255; 64 * 64 * 4], 64, 64).unwrap();
    assert_eq!(t.dimensions(), (64, 64));
    assert!(matches!(
        texture_from_rgba(vec![0; 10], 64, 64),
        Err(Error::Pixels(_))
    ));
    assert!(matches!(
        texture_from_rgba(vec![], 0, 64),
        Err(Error::Pixels(_))
    ));
}

#[test]
fn persona_skins_fall_back_to_2d() {
    let persona = parse_geometry(
        br#"{"minecraft:geometry":[{"description":{"identifier":"p"},"bones":[{"name":"root"}]}]}"#,
    )
    .unwrap();
    let t = tex();
    let img = RenderOptions::new(&t)
        .geometry(&persona)
        .size(64)
        .render()
        .unwrap();
    assert_eq!(img.dimensions(), (64, 64));
    let frames = AnimationOptions::new(
        RenderOptions::new(&t).geometry(&persona).size(32),
        &Motion::Walk,
    )
    .fps(4)
    .render_frames()
    .unwrap();
    assert_eq!(frames.len(), 4);
    assert!(frames.iter().all(|f| f == &frames[0]));
}

#[test]
fn example_animations() {
    let ex = bedrock_skin::example_animations();
    assert_eq!(ex.len(), 33);
    let dance = &ex["animation.player.dance"];
    assert!(dance.duration() > 0.0);
    let player = select_geometry(default_geometry(), "").unwrap();
    assert!(dance.missing_bones(player).is_empty());

    let wings = parse_animations(br#"{"animations":{"animation.bird.flap":{"loop":true,"bones":{"leftWing":{"rotation":[0,0,"math.sin(q.anim_time*720)*40"]},"head":{"rotation":[5,0,0]}}}}}"#).unwrap();
    let flap = &wings["animation.bird.flap"];
    assert_eq!(flap.bones(), ["head", "leftWing"]);
    assert_eq!(flap.missing_bones(player), ["leftWing"]);
    assert_eq!(flap.duration(), 1.0, "no keyframes: a second");

    assert!(matches!(
        parse_animations(br#"{"animations":{}}"#),
        Err(Error::NoAnimations)
    ));
    assert!(
        parse_animations(br#"{"animations":{"a":{"bones":{"head":{"rotation":"math.nope(1)"}}}}}"#)
            .is_err()
    );
    assert!(
        parse_animations(br#"{"animations":{"a":{"bones":{"head":{"rotation":[0,0,0,1]}}}}}"#)
            .is_err()
    );
}

#[test]
fn skin_report_json_shape() {
    let skin = Skin::new(tex(), None);
    assert!(skin.ok());
    let report = skin.report();
    assert_eq!(report.verdict, Verdict::Ok);
    assert_eq!((report.visible_parts, report.total_parts), (6, 6));
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["verdict"], "ok");
    assert_eq!(json["parts"][0]["visibility"], "visible");
    assert!(json["parts"][0].get("transparent_pixels").is_some());
    let back: SkinReport = serde_json::from_value(json).unwrap();
    assert_eq!(&back, report);

    let invisible = Skin::new(RgbaImage::new(64, 64), None);
    assert!(invisible.is_invisible());
    assert_eq!(invisible.invisible_parts().len(), 6);
    assert!(!SkinReport::default().ok(), "a default report fails closed");

    let empty = Skin::new(RgbaImage::new(0, 0), None);
    assert!(empty.is_invisible(), "no texture fails closed");
}

#[test]
fn geometry_tree() {
    let raw = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.a","texture_width":64},
        "bones":[{"name":"rightArm","pivot":[-5,22,0],"cubes":[{"origin":[0,0,0],"size":[4,12,4],"uv":[40,16]}]},{"name":"7"}]}]}"#;
    let tree = parse_geometry_tree(raw).unwrap();
    assert_eq!(tree.format_version, "1.12.0");
    assert_eq!(tree.identifiers(), ["geometry.a"]);
    let pivot = tree.get("geometry.a/bones/RIGHTARM/pivot").unwrap();
    assert_eq!(pivot.path, "geometry.a/bones/rightArm/pivot");
    assert_eq!(pivot.as_f64s().unwrap(), [-5.0, 22.0, 0.0]);
    assert_eq!(
        tree.get("*/description/texture_width").unwrap().as_f64(),
        Some(64.0)
    );
    let names: Vec<String> = tree
        .select("*/bones/*")
        .into_iter()
        .map(|v| v.path)
        .collect();
    assert_eq!(
        names,
        ["geometry.a/bones/rightArm", "geometry.a/bones/1"],
        "a numeric name is shown by index"
    );
    let bone = tree.get("*/bones/0").unwrap().bone().unwrap();
    assert_eq!(bone.cubes[0].box_uv(), Some((40.0, 16.0)));
    assert!(tree.select("*/bones/nope").is_empty());
    assert_eq!(tree.geometries().unwrap()[0].bones.len(), 2);
    assert!(matches!(
        parse_geometry_tree(b"{}"),
        Err(Error::NoGeometryModels)
    ));
}

/// A persona's body is poly meshes drawn in 3D; its head comes only from the
/// face texture.
#[test]
fn persona_mesh_renders_in_3d() {
    let geos =
        parse_geometry(&std::fs::read("testdata/parity/persona-mesh-geometry.json").unwrap())
            .unwrap();
    let tex = RgbaImage::from_pixel(64, 64, Rgba([10, 200, 10, 255]));
    let mut face = RgbaImage::new(32, 64);
    for y in 0..16 {
        for x in 0..32 {
            face.put_pixel(x, y, Rgba([255, 0, 0, 255]));
        }
    }
    let body = RenderOptions::new(&tex)
        .geometry(&geos)
        .size(64)
        .render()
        .unwrap();
    assert!(body.pixels().any(|p| p.0[3] > 0));
    let head = RenderOptions::new(&tex)
        .geometry(&geos)
        .view(View::Head)
        .render();
    assert!(matches!(head, Err(Error::EmptyView)), "{head:?}");
    let head = RenderOptions::new(&tex)
        .geometry(&geos)
        .view(View::Head)
        .animated(AnimatedType::Face, &face)
        .size(64)
        .render()
        .unwrap();
    assert_eq!(head.get_pixel(32, 32).0, [255, 0, 0, 255]);
}

/// The detector measures persona meshes on lowercase bones instead of calling
/// the skin invisible.
#[test]
fn detector_measures_persona_mesh() {
    let raw = std::fs::read("testdata/parity/persona-mesh-geometry.json").unwrap();
    let opaque = RgbaImage::from_pixel(64, 64, Rgba([1, 2, 3, 255]));
    let r = validate_skin_invisibility(&opaque, &raw);
    assert!(!r.is_invisible && r.visible_parts == 6, "{r:?}");
    assert!(validate_skin_invisibility(&RgbaImage::new(64, 64), &raw).is_invisible);
}

/// Frames rasterized in parallel are the frames rasterized one at a time.
#[test]
fn parallel_frames_match_serial() {
    let t = tex();
    let opts =
        || AnimationOptions::new(RenderOptions::new(&t).cape(&t).size(48), &Motion::Walk).fps(8);
    let serial = opts().workers(1).render_frames().unwrap();
    let parallel = opts().render_frames().unwrap();
    assert_eq!(serial.len(), parallel.len());
    assert!(serial.iter().zip(&parallel).all(|(a, b)| a == b));
}
