//! Shared drawing helpers: premultiplied-RGBA pixmaps + cosmic-text text runs.

use std::cell::RefCell;

use cosmic_text::{Attrs, Buffer, Color as CtColor, Family, FontSystem, Metrics, Shaping};
use tiny_skia::{Color, Paint, Pixmap, PixmapMut, Rect, Transform};

thread_local! {
    static FONT_SYSTEM: RefCell<FontSystem> = RefCell::new(FontSystem::new());
    static SWASH_CACHE: RefCell<cosmic_text::SwashCache> =
        RefCell::new(cosmic_text::SwashCache::new());
    /// Current accent preset rgb — (dark-mode rgb, light-mode rgb).
    /// Set from the compositor's `accent_rgb` config-map field on every
    /// Config event; defaults to Azure.
    static ACCENT_RGB: std::cell::Cell<([u8; 3], [u8; 3])> =
        std::cell::Cell::new(([0x3D, 0x8B, 0xFF], [0x0A, 0x6E, 0xE8]));
}

/// Update the accent rgb from a config event (called from `apply_config`).
pub fn set_accent(dark_rgb: [u8; 3], light_rgb: [u8; 3]) {
    ACCENT_RGB.with(|a| a.set((dark_rgb, light_rgb)));
}

/// The one Cosmos accent — the accent preset's colour in the
/// macOS/Win11 idiom. Used ONLY for active state (toggles on, slider
/// fill, focused selection, running indicators); chrome, cards and
/// text stay neutral.
pub fn accent(dark: bool) -> Color {
    ACCENT_RGB.with(|a| {
        let (d, l) = a.get();
        let [r, g, b] = if dark { d } else { l };
        Color::from_rgba8(r, g, b, 0xFF)
    })
}

/// A washed-out accent for selection/hover backgrounds behind text.
pub fn accent_soft(dark: bool) -> Color {
    ACCENT_RGB.with(|a| {
        let (d, l) = a.get();
        let ([r, g, b], alpha) = if dark { (d, 0x40) } else { (l, 0x2E) };
        Color::from_rgba8(r, g, b, alpha)
    })
}

thread_local! {
    /// Output scale: surfaces are laid out in logical px and painted
    /// into buffers of logical × SCALE (the viewport maps them back).
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

pub fn set_scale(s: f32) {
    SCALE.with(|c| c.set(s));
}

pub fn scale() -> f32 {
    SCALE.with(|c| c.get())
}

/// Logical → buffer transform for every paint call.
pub fn xf() -> Transform {
    let s = scale();
    Transform::from_scale(s, s)
}

/// Buffer size for a `w`×`h` logical surface.
pub fn phys(w: u32, h: u32) -> (u32, u32) {
    let s = scale();
    (
        ((w as f32 * s).round() as u32).max(1),
        ((h as f32 * s).round() as u32).max(1),
    )
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
        xf(),
        None,
    );
}

/// Rounded-rect path built from cubic circle-arc corners (tiny-skia 0.11 has
/// no RoundedRect primitive).
pub fn round_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
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
        xf(),
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
        xf(),
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
        xf(),
        None,
    );
}

/// Radial scrim for fullscreen overlays — lightest around the focal
/// point so the eye lands on the card, deepening toward the corners
/// (Spotlight's dim is a vignette, not a flat veil). Cached by size +
/// theme.
pub fn scrim(pixmap: &mut PixmapMut<'_>, w: u32, h: u32, dark: bool) {
    // Alpha at the focal point / at the far edge.
    let (base, edge) = if dark { (0.12, 0.20) } else { (0.08, 0.16) };
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
        xf(),
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
        pixmap, x, y, max_w, line_h, font_size, content, color, false, UI_FAMILY,
    );
}

/// Width in px of `content` laid out on one line at `font_size`.
pub fn text_width(font_size: f32, content: &str) -> f32 {
    FONT_SYSTEM.with(|fs| {
        let mut fs = fs.borrow_mut();
        let mut buf = Buffer::new(&mut fs, Metrics::new(font_size, font_size * 1.3));
        buf.set_size(&mut fs, None, None);
        buf.set_text(
            &mut fs,
            content,
            &Attrs::new().family(UI_FAMILY),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(&mut fs, false);
        buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
    })
}

/// [`text`] centred horizontally in `[x, x + max_w]`; truncates like [`text`]
/// when wider than the box.
#[allow(clippy::too_many_arguments)]
pub fn text_centered(
    pixmap: &mut PixmapMut<'_>,
    x: f32,
    y: f32,
    max_w: f32,
    line_h: f32,
    font_size: f32,
    content: &str,
    color: CtColor,
) {
    let tw = text_width(font_size, content).min(max_w);
    let cx = x + ((max_w - tw) / 2.0).max(0.0).floor();
    text(
        pixmap,
        cx,
        y,
        max_w - (cx - x),
        line_h,
        font_size,
        content,
        color,
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
    text_styled(
        pixmap, x, y, max_w, line_h, font_size, content, color, true, UI_FAMILY,
    );
}

#[allow(clippy::too_many_arguments)]
/// UI family for every shell surface: Inter first, DejaVu fallback.
pub const UI_FAMILY: Family<'static> = Family::Name("Inter");

/// Monospace family for key chords / code-ish labels.
pub const MONO_FAMILY: Family<'static> = Family::Name("JetBrains Mono");

#[allow(clippy::too_many_arguments)]
/// Monospaced text — key chords in the cheatsheet.
pub fn text_mono(
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
        pixmap,
        x,
        y,
        max_w,
        line_h,
        font_size,
        content,
        color,
        false,
        MONO_FAMILY,
    );
}

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
    family: Family<'_>,
) {
    FONT_SYSTEM.with(|fs| {
        SWASH_CACHE.with(|cache| {
            let mut fs = fs.borrow_mut();
            let mut cache = cache.borrow_mut();
            // Shaped at buffer resolution so glyphs stay crisp at 125%.
            let k = scale();
            let mut buf = Buffer::new(&mut fs, Metrics::new(font_size * k, line_h * k));
            buf.set_size(&mut fs, Some((max_w * k).max(1.0)), Some(line_h * k));
            // Inter when the image ships it, DejaVu otherwise — fontdb
            // falls back per-glyph, so Name() degrades gracefully.
            let attrs = Attrs::new().family(family);
            let attrs = if bold {
                attrs.weight(cosmic_text::Weight::SEMIBOLD)
            } else {
                attrs
            };
            buf.set_text(&mut fs, content, &attrs, Shaping::Advanced);
            buf.shape_until_scroll(&mut fs, false);
            let ox = (x * k) as i32;
            let oy = (y * k) as i32;
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
