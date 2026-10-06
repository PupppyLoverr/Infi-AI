//! Shared drawing helpers: premultiplied-RGBA pixmaps + cosmic-text text runs.

use std::cell::RefCell;

use cosmic_text::{Attrs, Buffer, Color as CtColor, Family, FontSystem, Metrics, Shaping};
use tiny_skia::{Color, Paint, PixmapMut, Rect, Transform};

thread_local! {
    static FONT_SYSTEM: RefCell<FontSystem> = RefCell::new(FontSystem::new());
    static SWASH_CACHE: RefCell<cosmic_text::SwashCache> =
        RefCell::new(cosmic_text::SwashCache::new());
}

/// Fill a rect.
pub fn fill_rect(pixmap: &mut PixmapMut<'_>, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let Some(rect) = Rect::from_xywh(x, y, w, h) else {
        return;
    };
    pixmap.fill_rect(
        rect,
        &Paint {
            shader: tiny_skia::Shader::SolidColor(color),
            anti_alias: false,
            ..Default::default()
        },
        Transform::default(),
        None,
    );
}

/// Rounded-rect path built from cubic circle-arc corners (tiny-skia 0.11 has
/// no RoundedRect primitive).
fn round_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r; // cubic approximation of a circular arc
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x0 + r, y0);
    pb.line_to(x1 - r, y0);
    pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x0 + r, y1);
    pb.cubic_to(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r);
    pb.line_to(x0, y0 + r);
    pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
    pb.close();
    pb.finish()
}

/// Fill a rounded rect (anti-aliased).
pub fn fill_round_rect(
    pixmap: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: Color,
) {
    let Some(path) = round_rect_path(x, y, w, h, r) else {
        return;
    };
    pixmap.fill_path(
        &path,
        &Paint {
            shader: tiny_skia::Shader::SolidColor(color),
            anti_alias: true,
            ..Default::default()
        },
        tiny_skia::FillRule::Winding,
        Transform::default(),
        None,
    );
}

/// Stroke a rounded rect (anti-aliased).
pub fn stroke_round_rect(
    pixmap: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    width: f32,
    color: Color,
) {
    let Some(path) = round_rect_path(x, y, w, h, r) else {
        return;
    };
    pixmap.stroke_path(
        &path,
        &Paint {
            shader: tiny_skia::Shader::SolidColor(color),
            anti_alias: true,
            ..Default::default()
        },
        &tiny_skia::Stroke {
            width,
            ..Default::default()
        },
        Transform::default(),
        None,
    );
}

/// Draw text (single line, truncated) at a pixel offset. `max_w` clamps the
/// renderable width. Source-over blends into the pixmap.
pub fn text(
    pixmap: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    max_w: f32,
    line_h: f32,
    font_size: f32,
    content: &str,
    color: CtColor,
) {
    FONT_SYSTEM.with(|fs| {
        SWASH_CACHE.with(|cache| {
            let mut fs = fs.borrow_mut();
            let mut cache = cache.borrow_mut();
            let mut buf = Buffer::new(&mut fs, Metrics::new(font_size, line_h));
            buf.set_size(&mut fs, Some(max_w.max(1.0)), Some(line_h));
            buf.set_text(
                &mut fs,
                content,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut fs, false);
            let ox = x as i32;
            let oy = y as i32;
            let pw = pixmap.width() as i32;
            let ph = pixmap.height() as i32;
            let pixels = pixmap.pixels_mut();
            buf.draw(&mut fs, &mut cache, color, |tx, ty, _w, _h, c| {
                // Per-pixel blend — tiny-skia Pixmap pixels are premultiplied.
                let sx = tx + ox;
                let sy = ty + oy;
                if sx < 0 || sy < 0 || sx >= pw || sy >= ph {
                    return;
                }
                let sa = c.a() as u32;
                if sa == 0 {
                    return;
                }
                let i = (sy * pw + sx) as usize;
                let dst = &mut pixels[i];
                let (dr, dg, db, da) = (dst.red(), dst.green(), dst.blue(), dst.alpha());
                let inv = 255 - sa;
                *dst = tiny_skia::PremultipliedColorU8::from_rgba(
                    ((c.r() as u32 * sa + dr as u32 * inv) / 255).min(255) as u8,
                    ((c.g() as u32 * sa + dg as u32 * inv) / 255).min(255) as u8,
                    ((c.b() as u32 * sa + db as u32 * inv) / 255).min(255) as u8,
                    (sa + da as u32 * inv / 255).min(255) as u8,
                )
                .unwrap_or(*dst);
            });
        });
    });
}
