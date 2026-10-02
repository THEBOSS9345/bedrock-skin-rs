//! The flat fallback for persona skins, which have bones but no cubes.
//! See docs/rendering-pipeline.md#the-2d-fallback.

use image::RgbaImage;

use crate::mesh::box_uv_rects;
use crate::render::View;

/// A non-premultiplied RGBA image with its own size, as Go's *image.NRGBA
/// with a zero origin.
struct Nrgba {
    w: usize,
    h: usize,
    pix: Vec<u8>,
}

impl Nrgba {
    fn new(w: usize, h: usize) -> Nrgba {
        Nrgba {
            w,
            h,
            pix: vec![0; w * h * 4],
        }
    }

    fn px(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.w + x) * 4;
        [
            self.pix[i],
            self.pix[i + 1],
            self.pix[i + 2],
            self.pix[i + 3],
        ]
    }
}

/// color.NRGBA's RGBA(): premultiplied, 16 bits a channel.
fn premultiply(c: [u8; 4]) -> [u32; 4] {
    let a = c[3] as u32;
    let ch = |v: u8| ((v as u32 | (v as u32) << 8) * a) / 0xff;
    [ch(c[0]), ch(c[1]), ch(c[2]), a | a << 8]
}

/// Go's NRGBA SetRGBA64: back to straight alpha, 8 bits a channel.
fn unpremultiply(c: [u32; 4]) -> [u8; 4] {
    let [mut r, mut g, mut b, a] = c;
    if a != 0 && a != 0xffff {
        r = r * 0xffff / a;
        g = g * 0xffff / a;
        b = b * 0xffff / a;
    }
    [
        (r >> 8) as u8,
        (g >> 8) as u8,
        (b >> 8) as u8,
        (a >> 8) as u8,
    ]
}

/// Go's draw.Over of one premultiplied pixel onto another.
fn over(dst: [u32; 4], src: [u32; 4]) -> [u32; 4] {
    let a = 0xffff - src[3];
    std::array::from_fn(|i| ((dst[i] * a) / 0xffff + src[i]) & 0xffff)
}

/// Composites a flat front-view "paper doll" by cropping the standard
/// vanilla box-UV regions straight out of the texture. It needs no geometry
/// at all, which is why rendering falls back to it for persona skins: they
/// carry bones but no cubes, so there is nothing to rasterize.
///
/// Coordinates are against a 64-wide texture, scaled for other widths.
pub fn render_2d(texture: &RgbaImage, view: View, size: u32) -> RgbaImage {
    let scale = texture.width() as f64 / 64.0;
    let tex = Nrgba {
        w: texture.width() as usize,
        h: texture.height() as usize,
        pix: texture.as_raw().clone(),
    };

    let front = |ux: f64, uy: f64, w: f64, h: f64, d: f64| -> Nrgba {
        let r = box_uv_rects(ux * scale, uy * scale, w * scale, h * scale, d * scale)[2].1; // north
        crop(
            &tex,
            r.x as i64,
            r.y as i64,
            (r.x + r.w) as i64,
            (r.y + r.h) as i64,
        )
    };
    let head = front(0.0, 0.0, 8.0, 8.0, 8.0);
    let body = front(16.0, 16.0, 8.0, 12.0, 4.0);
    let right_arm = front(40.0, 16.0, 4.0, 12.0, 4.0);
    let left_arm = front(32.0, 48.0, 4.0, 12.0, 4.0);
    let right_leg = front(0.0, 16.0, 4.0, 12.0, 4.0);
    let left_leg = front(16.0, 48.0, 4.0, 12.0, 4.0);

    let canvas = match view {
        View::Head | View::Avatar => head,
        View::Chest => compose_parts(&head, &body, &right_arm, &left_arm, None),
        View::Body => compose_parts(
            &head,
            &body,
            &right_arm,
            &left_arm,
            Some((&right_leg, &left_leg)),
        ),
    };

    let size = size as usize;
    let mut out = RgbaImage::new(size as u32, size as u32);
    if canvas.w == 0 || canvas.h == 0 || size == 0 {
        return out;
    }
    // Nearest neighbour, as x/image/draw scales: the source pixel under each
    // destination pixel's centre. Drawn over a clear image, every pixel ends
    // up premultiplied and back.
    let (dw2, dh2) = (size as u64 * 2, size as u64 * 2);
    for dy in 0..size {
        let sy = ((2 * dy as u64 + 1) * canvas.h as u64 / dh2) as usize;
        for dx in 0..size {
            let sx = ((2 * dx as u64 + 1) * canvas.w as u64 / dw2) as usize;
            let p = unpremultiply(premultiply(canvas.px(sx, sy)));
            out.put_pixel(dx as u32, dy as u32, image::Rgba(p));
        }
    }
    out
}

/// The rectangle's part of the image, as Go's SubImage: clipped to it.
fn crop(img: &Nrgba, x0: i64, y0: i64, x1: i64, y1: i64) -> Nrgba {
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(img.w as i64), y1.min(img.h as i64));
    if x0 >= x1 || y0 >= y1 {
        return Nrgba::new(0, 0);
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut out = Nrgba::new(w, h);
    for y in 0..h {
        let src = ((y0 as usize + y) * img.w + x0 as usize) * 4;
        out.pix[y * w * 4..(y + 1) * w * 4].copy_from_slice(&img.pix[src..src + w * 4]);
    }
    out
}

/// Draws src over dst with its top-left corner at (x, y), clipped to dst.
fn draw_over(dst: &mut Nrgba, src: &Nrgba, x: i64, y: i64) {
    for sy in 0..src.h {
        for sx in 0..src.w {
            let (dx, dy) = (x + sx as i64, y + sy as i64);
            if dx < 0 || dy < 0 || dx >= dst.w as i64 || dy >= dst.h as i64 {
                continue;
            }
            let (dx, dy) = (dx as usize, dy as usize);
            let out = unpremultiply(over(
                premultiply(dst.px(dx, dy)),
                premultiply(src.px(sx, sy)),
            ));
            let i = (dy * dst.w + dx) * 4;
            dst.pix[i..i + 4].copy_from_slice(&out);
        }
    }
}

/// Stacks head above body above arms and legs into a flat paper doll, as
/// seen facing the player: their right arm and leg on the viewer's left.
fn compose_parts(
    head: &Nrgba,
    body: &Nrgba,
    viewer_left_arm: &Nrgba,
    viewer_right_arm: &Nrgba,
    legs: Option<(&Nrgba, &Nrgba)>,
) -> Nrgba {
    let bw = body.w as i64;
    let total_w = viewer_left_arm.w + body.w + viewer_right_arm.w;
    let mut total_h = head.h + body.h;
    if let Some((l, _)) = legs {
        total_h += l.h;
    }
    let mut out = Nrgba::new(total_w, total_h);
    let mid_x = viewer_left_arm.w as i64;
    let mut y = 0i64;

    // Go's integer division rounds toward zero.
    draw_over(&mut out, head, mid_x + (bw - head.w as i64) / 2, y);
    y += head.h as i64;
    draw_over(&mut out, viewer_left_arm, 0, y);
    draw_over(&mut out, body, mid_x, y);
    draw_over(&mut out, viewer_right_arm, mid_x + bw, y);
    y += body.h as i64;
    if let Some((left_leg, right_leg)) = legs {
        let lw = left_leg.w as i64;
        draw_over(&mut out, left_leg, mid_x + bw / 2 - lw, y);
        draw_over(&mut out, right_leg, mid_x + bw / 2, y);
    }
    out
}
