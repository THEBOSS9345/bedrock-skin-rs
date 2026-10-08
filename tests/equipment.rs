//! Equipment's behaviour beyond its pixels, which tests/parity.rs pins
//! against the Go library: errors, the byte API, and what each option
//! leaves alone.

use bedrock_skin::*;
use image::{Rgba, RgbaImage};

fn skin() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])
    })
}

fn armor() -> RgbaImage {
    RgbaImage::from_fn(64, 32, |x, y| {
        Rgba([40, (120 + x * 2) as u8, (140 + y * 3) as u8, 255])
    })
}

fn item() -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        if x + y == 15 {
            Rgba([200, 220, 240, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

fn png(img: &RgbaImage) -> Vec<u8> {
    encode_png(img).unwrap()
}

#[test]
fn no_equipment_renders_as_before() {
    let tex = skin();
    let plain = RenderOptions::new(&tex).size(64).render().unwrap();
    let zero = RenderOptions::new(&tex)
        .size(64)
        .armor(Armor::default())
        .right_hand(Held::default())
        .scale(Scale::default())
        .render()
        .unwrap();
    assert_eq!(plain, zero);
}

#[test]
fn elytra_takes_the_chestplates_slot() {
    let (tex, a, ely) = (skin(), armor(), skin());
    let both = Armor {
        chestplate: Some(&a),
        elytra: Some(&ely),
        ..Armor::default()
    };
    let elytra = Armor {
        elytra: Some(&ely),
        ..Armor::default()
    };
    let r = |armor| {
        RenderOptions::new(&tex)
            .size(64)
            .armor(armor)
            .render()
            .unwrap()
    };
    assert_eq!(r(both), r(elytra));
}

#[test]
fn hide_skin_with_nothing_to_draw() {
    let a = armor();
    assert!(matches!(
        RenderOptions::equipment().render(),
        Err(Error::EmptyView)
    ));
    let boots = RenderOptions::equipment().armor(Armor {
        boots: Some(&a),
        ..Armor::default()
    });
    assert!(matches!(
        boots.clone().view(View::Head).render(),
        Err(Error::EmptyView)
    ));
    assert!(matches!(
        boots.parts(["head"]).render(),
        Err(Error::NoMatchingParts)
    ));
    let empty = RgbaImage::new(0, 0);
    assert!(matches!(
        RenderOptions::new(&empty).render(),
        Err(Error::NoTexture)
    ));
}

#[test]
fn hide_skin_ignores_the_texture() {
    let (tex, a) = (skin(), armor());
    let alone = RenderOptions::equipment()
        .armor(Armor::set(&a, &a))
        .size(64)
        .render()
        .unwrap();
    let hidden = RenderOptions::new(&tex)
        .hide_skin(true)
        .armor(Armor::set(&a, &a))
        .size(64)
        .render()
        .unwrap();
    assert_eq!(alone, hidden);
}

#[test]
fn bytes_carry_equipment() {
    let (tex, a, it) = (skin(), armor(), item());
    let adjust = ItemAdjust {
        offset: [0.0, 1.0, 0.0],
        rotation: [10.0, 0.0, 0.0],
        scale: 1.2,
    };
    let scale = Scale {
        model: 1.2,
        parts: [("head".to_string(), 1.3)].into_iter().collect(),
    };
    let want = RenderOptions::new(&tex)
        .size(64)
        .armor(Armor {
            elytra: Some(&tex),
            ..Armor::set(&a, &a)
        })
        .right_hand(Held {
            adjust,
            ..Held::new(&it)
        })
        .left_hand(Held {
            flat: true,
            ..Held::new(&it)
        })
        .scale(scale.clone())
        .render_png()
        .unwrap();
    let (tex_png, a_png, it_png) = (png(&tex), png(&a), png(&it));
    let got = render_bytes(&BytesOptions {
        texture: &tex_png,
        size: 64,
        armor: ArmorBytes {
            elytra: &tex_png,
            ..ArmorBytes::set(&a_png, &a_png)
        },
        right_hand: HeldBytes {
            item: &it_png,
            flat: false,
            adjust,
        },
        left_hand: HeldBytes {
            item: &it_png,
            flat: true,
            ..HeldBytes::default()
        },
        scale,
        ..BytesOptions::default()
    })
    .unwrap();
    assert_eq!(got, want);

    let hidden = render_bytes(&BytesOptions {
        hide_skin: true,
        armor: ArmorBytes {
            helmet: &a_png,
            ..ArmorBytes::default()
        },
        size: 32,
        ..BytesOptions::default()
    })
    .unwrap();
    let want = RenderOptions::equipment()
        .armor(Armor {
            helmet: Some(&a),
            ..Armor::default()
        })
        .size(32)
        .render_png()
        .unwrap();
    assert_eq!(hidden, want);

    for (opts, prefix) in [
        (
            BytesOptions {
                texture: &tex_png,
                armor: ArmorBytes {
                    leggings: b"nope",
                    ..ArmorBytes::default()
                },
                ..BytesOptions::default()
            },
            "armor leggings:",
        ),
        (
            BytesOptions {
                texture: &tex_png,
                left_hand: HeldBytes {
                    item: b"nope",
                    ..HeldBytes::default()
                },
                ..BytesOptions::default()
            },
            "left hand item:",
        ),
    ] {
        match render_bytes(&opts) {
            Err(Error::Image(msg)) => assert!(msg.starts_with(prefix), "{msg}"),
            other => panic!("want an image error starting {prefix:?}, got {other:?}"),
        }
    }
}

#[test]
fn items_on_their_own() {
    let it = item();
    let front = ItemOptions::new(&it).size(64).render().unwrap();
    let iso = ItemOptions::new(&it)
        .angle(Angle::Iso)
        .size(64)
        .render()
        .unwrap();
    assert_ne!(front, iso);
    let empty = RgbaImage::new(0, 0);
    assert!(matches!(
        render_item(&ItemOptions::new(&empty)),
        Err(Error::EmptyView)
    ));

    let it_png = png(&it);
    let bytes = ItemBytesOptions {
        item: &it_png,
        size: 64,
        ..ItemBytesOptions::default()
    };
    assert_eq!(render_item_bytes(&bytes).unwrap(), png(&front));
    assert!(matches!(
        render_item_bytes(&ItemBytesOptions::default()),
        Err(Error::NoTexture)
    ));

    let spin = ItemAnimationOptions {
        fps: 4,
        duration: 1.0,
        ..ItemAnimationOptions::new(ItemOptions::new(&it).size(48))
    };
    let frames = render_item_frames(&spin).unwrap();
    assert_eq!(frames.len(), 4);
    assert_ne!(frames[0], frames[1]);
    let gif = render_item_gif(&spin).unwrap();
    let gif_bytes = render_item_gif_bytes(&ItemAnimationBytesOptions {
        item: ItemBytesOptions {
            item: &it_png,
            size: 48,
            ..ItemBytesOptions::default()
        },
        fps: 4,
        duration: 1.0,
        frames: 0,
    })
    .unwrap();
    assert_eq!(gif, gif_bytes);
}
