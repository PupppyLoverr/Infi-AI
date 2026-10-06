//! Shared drawing helpers: premultiplied-RGBA pixmaps + cosmic-text text runs.

use std::cell::RefCell;

use cosmic_text::{Attrs, Buffer, Color as CtColor, Family, FontSystem, Metrics, Shaping};
use tiny_skia::{Color, Paint, Pixmap, PixmapMut, Rect, Transform};

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

/// Soft drop shadow under a card rect — SDF silhouette + gaussian
/// falloff, premultiplied black, blitted SrcOver (the macOS popover
/// look). The pixmap is cached by (w, h, r): the exp() per-pixel cost
/// is paid once per card size, not per repaint.
pub fn shadow(pixmap: &mut PixmapMut<'_>, x: f32, y: f32, w: f32, h: f32, r: f32) {
    // Bleed past each card edge.
    const MARGIN: i32 = 20;
    // Downward bias — shadows hang lower than they float high.
    const DY: i32 = 7;
    // Gaussian falloff width in px.
    const SIGMA: f32 = 9.0;
    // Peak alpha at the silhouette.
    const ALPHA: f32 = 0.34;
    let sw = (w.ceil() as i32 + MARGIN * 2).max(0) as u32;
    let sh = (h.ceil() as i32 + MARGIN * 2).max(0) as u32;
    if sw == 0 || sh == 0 {
        return;
    }
    let px = card_pixmap(sw, sh, r, |buf, pw| {
        let x0 = MARGIN as f32;
        let y0 = (MARGIN + DY) as f32;
        let x1 = x0 + w;
        let y1 = y0 + h;
        let sigma2 = 2.0 * SIGMA * SIGMA;
        for yy in 0..sh as i32 {
            for xx in 0..sw as i32 {
                let fx = xx as f32 + 0.5;
                let fy = yy as f32 + 0.5;
                // Corner-aware distance outside the card rect.
                let dx = (x0 + r - fx).max(fx - (x1 - r)).max(0.0);
                let dy = (y0 + r - fy).max(fy - (y1 - r)).max(0.0);
                let d = ((dx * dx + dy * dy).sqrt() - r).max(0.0);
                let a = ALPHA * (-d * d / sigma2).exp();
                let i = (yy * pw as i32 + xx) as usize;
                buf[i] = tiny_skia::PremultipliedColorU8::from_rgba(
                    0,
                    0,
                    0,
                    (a * 255.0).min(255.0) as u8,
                )
                .unwrap_or(buf[i]);
            }
        }
    });
    let _ = pixmap.draw_pixmap(
        (x - MARGIN as f32).round() as i32,
        (y - MARGIN as f32).round() as i32,
        px.as_ref().as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::default(),
        None,
    );
}

/// Radial scrim for fullscreen overlays — lightest around the focal
/// point so the eye lands on the card, deepening toward the corners
/// (Spotlight's dim is a vignette, not a flat veil). Cached by size +
/// theme.
pub fn scrim(pixmap: &mut PixmapMut<'_>, w: u32, h: u32, dark: bool) {
    // Alpha at the focal point / at the far edge.
    let (base, edge) = if dark { (0.30, 0.62) } else { (0.16, 0.40) };
    // Lightest near the card zone (upper-centre where the card floats).
    let (fx, fy) = (w as f32 * 0.5, h as f32 * 0.42);
    // Distance at which the scrim reaches `edge` alpha: ~55% of the
    // half-diagonal keeps the middle airy without bright spots.
    let max_d = (w as f32 * w as f32 + h as f32 * h as f32).sqrt() * 0.28;
    let key = (w, h, dark as u32);
    let px = keyed_pixmap(&SCRIM_PIXMAPS, key, w, h, |buf, pw| {
        let (r8, g8, b8) = if dark {
            (0u8, 0u8, 0u8)
        } else {
            (255, 255, 255)
        };
        for yy in 0..h as i32 {
            for xx in 0..w as i32 {
                let dx = xx as f32 + 0.5 - fx;
                let dy = yy as f32 + 0.5 - fy;
                let t = ((dx * dx + dy * dy).sqrt() / max_d).min(1.0);
                let a = base + (edge - base) * t * t * (3.0 - 2.0 * t);
                let i = (yy * pw as i32 + xx) as usize;
                buf[i] = tiny_skia::PremultipliedColorU8::from_rgba(
                    r8,
                    g8,
                    b8,
                    (a * 255.0).min(255.0) as u8,
                )
                .unwrap_or(buf[i]);
            }
        }
    });
    let _ = pixmap.draw_pixmap(
        0,
        0,
        px.as_ref().as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::default(),
        None,
    );
}

type PixmapKey = (u32, u32, u32);
type PixmapCache = std::cell::RefCell<std::collections::HashMap<PixmapKey, std::rc::Rc<Pixmap>>>;

thread_local! {
    static SHADOW_PIXMAPS: PixmapCache = std::cell::RefCell::new(std::collections::HashMap::new());
    static SCRIM_PIXMAPS: PixmapCache = std::cell::RefCell::new(std::collections::HashMap::new());
}

fn card_pixmap(
    sw: u32,
    sh: u32,
    r: f32,
    paint: impl FnOnce(&mut [tiny_skia::PremultipliedColorU8], u32),
) -> std::rc::Rc<Pixmap> {
    keyed_pixmap(&SHADOW_PIXMAPS, (sw, sh, r.to_bits()), sw, sh, paint)
}

fn keyed_pixmap(
    cache: &'static std::thread::LocalKey<PixmapCache>,
    key: PixmapKey,
    w: u32,
    h: u32,
    paint: impl FnOnce(&mut [tiny_skia::PremultipliedColorU8], u32),
) -> std::rc::Rc<Pixmap> {
    cache.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 16 {
            c.clear();
        }
        c.entry(key)
            .or_insert_with(|| {
                let mut px = Pixmap::new(w, h).unwrap();
                paint(px.pixels_mut(), w);
                std::rc::Rc::new(px)
            })
            .clone()
    })
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
    text_styled(
        pixmap, x, y, max_w, line_h, font_size, content, color, false,
    );
}

/// [`text`] with an optional semibold weight — the macOS menubar's
/// focused-app name look.
pub fn text_bold(
    pixmap: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    max_w: f32,
    line_h: f32,
    font_size: f32,
    content: &str,
    color: CtColor,
) {
    text_styled(pixmap, x, y, max_w, line_h, font_size, content, color, true);
}

#[allow(clippy::too_many_arguments)]
fn text_styled(
    pixmap: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    max_w: f32,
    line_h: f32,
    font_size: f32,
    content: &str,
    color: CtColor,
    bold: bool,
) {
    FONT_SYSTEM.with(|fs| {
        SWASH_CACHE.with(|cache| {
            let mut fs = fs.borrow_mut();
            let mut cache = cache.borrow_mut();
            let mut buf = Buffer::new(&mut fs, Metrics::new(font_size, line_h));
            buf.set_size(&mut fs, Some(max_w.max(1.0)), Some(line_h));
            let attrs = Attrs::new().family(Family::SansSerif);
            let attrs = if bold {
                attrs.weight(cosmic_text::Weight::SEMIBOLD)
            } else {
                attrs
            };
            buf.set_text(&mut fs, content, &attrs, Shaping::Advanced);
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
