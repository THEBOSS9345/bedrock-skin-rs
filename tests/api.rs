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

/// A prepared frame set draws the frames `render_frames` does, one at a time,
/// and keeps their shared camera: root motion stays on screen rather than the
/// camera chasing each pose.
#[test]
fn prepared_frames_draw_one_at_a_time() {
    let anims = parse_animations(
        br#"{"format_version":"1.8.0","animations":{"animation.test.root":{"loop":true,"animation_length":1.0,"bones":{"root":{"position":{"0.0":[0,0,0],"0.5":[0,4,0],"1.0":[0,0,0]}}}}}}"#,
    )
    .unwrap();
    let t = tex();
    let opts = AnimationOptions::new(
        RenderOptions::new(&t).size(64),
        &anims["animation.test.root"],
    )
    .fps(4);
    let frames = prepare_frames(&opts).unwrap();
    assert_eq!(frames.len(), 4);

    // The root lifts the whole model; one shared camera shows it. A camera
    // refit per pose would cancel it, leaving the head at one row.
    let low = head_top(&frames.draw(0, 64, None));
    let high = head_top(&frames.draw(2, 64, None));
    assert_ne!(low, high, "framing cancelled the jump");

    // Drawn one at a time is drawn as a batch.
    let batch = opts.render_frames().unwrap();
    for (i, want) in batch.iter().enumerate() {
        assert_eq!(
            &frames.draw(i, 64, None),
            want,
            "frame {i} drawn alone differs"
        );
    }
    assert_eq!(
        frames.draw(frames.len(), 64, None),
        frames.draw(0, 64, None),
        "draw should wrap an out-of-range frame index"
    );
    let turned = frames.draw(
        0,
        64,
        Some(Camera {
            yaw: 90.0,
            ..Default::default()
        }),
    );
    assert_ne!(
        turned,
        frames.draw(0, 64, None),
        "a refit camera left the view unchanged"
    );

    // A scaled model refits the same way the batch does: a camera margin is
    // divided by Scale.model exactly as scene() divides it.
    let cam = Camera {
        yaw: 20.0,
        pitch: 10.0,
        fov: 35.0,
        margin: 1.2,
    };
    let scaled = AnimationOptions::new(
        RenderOptions::new(&t)
            .size(64)
            .scale(Scale {
                model: 2.0,
                ..Default::default()
            })
            .camera(cam),
        &anims["animation.test.root"],
    )
    .fps(4);
    let scaled_frames = prepare_frames(&scaled).unwrap();
    let scaled_batch = scaled.render_frames().unwrap();
    for (i, want) in scaled_batch.iter().enumerate() {
        assert_eq!(
            &scaled_frames.draw(i, 64, Some(cam)),
            want,
            "scaled frame {i} differs"
        );
    }
}

/// The first row with an opaque pixel, or `usize::MAX` when nothing is drawn.
fn head_top(img: &RgbaImage) -> usize {
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y).0[3] >= 128 {
                return y as usize;
            }
        }
    }
    usize::MAX
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

/// The bytes path for animations is render_gif and render_frames with the
/// decode and encode folded in: the same GIF, the same frames.
#[test]
fn animation_bytes_match_render() {
    let t = tex();
    let png = encode_png(&t).unwrap();
    let decoded = decode_image(&png).unwrap();
    let bytes = BytesOptions {
        texture: &png,
        size: 48,
        ..BytesOptions::default()
    };
    let opts = AnimationBytesOptions::new(bytes, &Motion::Wave).fps(6);
    let direct = AnimationOptions::new(RenderOptions::new(&decoded).size(48), &Motion::Wave).fps(6);
    assert_eq!(
        render_gif_bytes(&opts).unwrap(),
        direct.render_gif().unwrap()
    );
    let pngs = opts.render_frames_png().unwrap();
    let frames = direct.render_frames().unwrap();
    assert_eq!(pngs.len(), frames.len());
    for (p, f) in pngs.iter().zip(&frames) {
        assert_eq!(&decode_image(p).unwrap(), f);
    }
    let empty = AnimationBytesOptions::new(BytesOptions::default(), &Motion::Walk);
    assert!(matches!(empty.render_gif(), Err(Error::NoTexture)));
}

/// polygons hands back each corner looked up: position, normal and UV, with a
/// missing normal left zero rather than dropping the polygon. A geometry
/// tree reads a poly mesh by path the same way.
#[test]
fn polygons_resolve_corners() {
    let raw = br#"{"minecraft:geometry":[{"description":{"identifier":"g"},"bones":[{"name":"body","poly_mesh":{"normalized_uvs":true,
        "positions":[[0,0,0],[1,0,0],[1,1,0],[0,1,0]],
        "normals":[[0,0,-1]],
        "uvs":[[0,0],[1,0],[1,1],[0,1]],
        "polys":[[[0,0,0],[1,0,1],[2,0,2]],[[0,0,0],[2,5,2],[3,0,3]]]}}]}]}"#;
    let geos = parse_geometry(raw).unwrap();
    let m = geos[0].bones[0].mesh().unwrap();
    assert!(m.normalized_uvs);
    let got = m.polygons();
    assert_eq!(got.len(), 2);
    assert_eq!(
        got[0][2],
        PolyVertex {
            position: [1.0, 1.0, 0.0],
            normal: [0.0, 0.0, -1.0],
            uv: [1.0, 1.0]
        }
    );
    assert_eq!(got[1][1].normal, [0.0; 3]);
    let tree = parse_geometry_tree(raw).unwrap();
    let from_tree = tree
        .get("*/bones/body/poly_mesh")
        .unwrap()
        .poly_mesh()
        .unwrap();
    assert_eq!(from_tree.polygons(), got);
}

/// A skin straight from the wire: raw RGBA, the patch's entry picked, the
/// persona face attached; malformed fields name themselves.
#[test]
fn wire_skin_decodes() {
    let skin = vec![200u8; 64 * 64 * 4];
    let face = vec![255u8; 32 * 64 * 4];
    let cape = vec![0u8; 64 * 32 * 4];
    let geo = std::fs::read("testdata/parity/persona-mesh-geometry.json").unwrap();
    let w = WireSkin {
        skin_data: &skin,
        skin_width: 64,
        skin_height: 64,
        cape_data: &cape,
        cape_width: 64,
        cape_height: 32,
        geometry: &geo,
        resource_patch: br#"{"geometry":{"default":"geometry.persona_test"}}"#,
        animations: vec![WireAnimation {
            animation_type: 1,
            data: &face,
            width: 32,
            height: 64,
        }],
    };
    let d = w.decode().unwrap();
    assert_eq!(d.identifier, "geometry.persona_test");
    assert_eq!(
        (d.geometry.len(), d.cape.is_some(), d.animated.len()),
        (2, true, 1)
    );
    assert!(
        d.options().view(View::Head).render().is_ok(),
        "the face draws the head"
    );

    let fallback = WireSkin {
        skin_data: &skin,
        skin_width: 64,
        skin_height: 64,
        geometry: b"null",
        resource_patch: b"{",
        ..WireSkin::default()
    }
    .decode()
    .unwrap();
    assert!(fallback.geometry.is_empty() && fallback.identifier.is_empty());

    let short = WireSkin {
        skin_data: &skin[..10],
        skin_width: 64,
        skin_height: 64,
        ..WireSkin::default()
    };
    assert!(short.decode().unwrap_err().to_string().starts_with("skin"));
    let bad_cape = WireSkin {
        cape_data: &[1],
        cape_width: 64,
        cape_height: 32,
        ..w.clone()
    };
    assert!(
        bad_cape
            .decode()
            .unwrap_err()
            .to_string()
            .starts_with("cape")
    );

    let clear = vec![0u8; 64 * 64 * 4];
    let invisible = WireSkin {
        skin_data: &clear,
        skin_width: 64,
        skin_height: 64,
        geometry: b"null",
        ..WireSkin::default()
    };
    assert!(invisible.skin().unwrap().is_invisible());
}

/// The writers write exactly what the byte-returning functions return.
#[test]
fn writers_match_renderers() {
    let t = tex();
    let o = RenderOptions::new(&t).size(64);
    let mut png = Vec::new();
    o.write_png(&mut png).unwrap();
    assert_eq!(png, o.render_png().unwrap());
    let a = AnimationOptions::new(o, &Motion::Wave).fps(6);
    let mut gif = Vec::new();
    a.write_gif(&mut gif).unwrap();
    assert_eq!(gif, a.render_gif().unwrap());
}

/// The built-in motions say "leftArm"; a persona model names the bone
/// "leftarm". The pose still finds it.
#[test]
fn pose_names_ignore_case() {
    let mut p = Pose::new();
    p.insert(
        "leftArm",
        BonePose {
            rotation: [10.0, 0.0, 0.0],
            ..BonePose::default()
        },
    );
    assert_eq!(p.of("leftarm").rotation[0], 10.0);
    assert_eq!(p.of("LEFTARM").rotation[0], 10.0);
    assert_eq!(p.of("rightarm").rotation[0], 0.0);
}
