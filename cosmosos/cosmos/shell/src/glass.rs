//! Fake-glass surfaces: one pre-blurred copy of the active wallpaper,
//! sampled at each surface's screen position + tint + 1px inner
//! highlight. Zero per-frame cost — the blur is baked once per
//! wallpaper change.

use std::cell::RefCell;
use tiny_skia::{Color, Paint, Pixmap, PixmapMut, Transform};

use crate::draw::round_rect_path;

/// Downscaled blurred copy of the wallpaper — big enough to stay smooth,
/// small enough that decoding + blurring is free.
const GW: u32 = 480;
const GH: u32 = 270;

thread_local! {
    static GLASS: RefCell<Option<Glass>> = const { RefCell::new(None) };
}

struct Glass {
    name: String,
    /// Blurred + saturation-lifted wallpaper at GW×GH.
    blur: Pixmap,
}

fn dir() -> std::path::PathBuf {
    std::env::var("COSMOS_WALLPAPER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/share/cosmos/wallpapers"))
}

/// Decode + downscale + blur the named wallpaper. Saturates slightly so
/// glass reads tinted rather than washed-out.
fn load(name: &str) -> Option<Glass> {
    let d = dir();
    let img = ["-1920x1080.png", "-3840x2160.png"]
        .iter()
        .find_map(|s| image::open(d.join(format!("{name}{s}"))).ok())?
        .to_rgba8();
    // Downscale to the working size (fast filter — blurred anyway).
    let small = image::imageops::resize(
        &img,
        GW,
        GH,
        image::imageops::FilterType::Triangle,
    );
    // ~40px blur at 1080p ≈ sigma ~10 at this downscale.
    let mut blurred = image::imageops::blur(&small, 10.0);
    // Saturation ×1.2 (spec-v3 §2.2) plus ±2% hash-noise dither so the
    // upscaled blur never bands.
    for (i, px) in blurred.chunks_exact_mut(4).enumerate() {
        let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
        let mean = (r + g + b) / 3.0;
        let mut h = (i as u32).wrapping_mul(0x9E37_79B1);
        h ^= h >> 15;
        let n = ((h & 0xFF) as f32 / 255.0 - 0.5) * 0.04 * 255.0;
        for c in &mut px[0..3] {
            let v = *c as f32;
            *c = (v + (v - mean) * 0.2 + n).clamp(0.0, 255.0) as u8;
        }
    }
    // Premultiplied == straight for opaque pixels; RgbaImage is already
    // [r,g,b,a] byte order.
    let pm = Pixmap::from_vec(blurred.into_raw(), tiny_skia::IntSize::from_wh(GW, GH)?)?;
    Some(Glass {
        name: name.to_string(),
        blur: pm,
    })
}

/// The blurred wallpaper for the given name — cached; reloads on change.
fn glass_for(name: &str) -> Option<Glass> {
    GLASS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().map(|g| g.name.as_str()) != Some(name) {
            *slot = load(name);
        }
        // Cloning a Pixmap is a deep copy of a small buffer — fine at
        // per-frame rates for a handful of surfaces.
        slot.as_ref().map(|g| Glass {
            name: g.name.clone(),
            blur: g.blur.clone(),
        })
    })
}

/// Screen size reported by the last surface configure — the shell only
/// drives one output today.
pub fn set_screen_size(w: f32, h: f32) {
    SCREEN.with(|s| *s.borrow_mut() = (w, h));
}
thread_local! {
    static SCREEN: RefCell<(f32, f32)> = const { RefCell::new((1920.0, 1080.0)) };
}

/// Paint `name`'s blurred wallpaper into the rect, glass-style:
/// wallpaper sample at the surface's screen position + tint + 1px
/// inner highlight. (screen_x, screen_y) is the rect's position on the
/// output — callers know their anchors.
pub fn fill_glass(
    pixmap: &mut PixmapMut<'_>,
    name: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    screen_x: f32,
    screen_y: f32,
    dark: bool,
    fallback: Color,
) {
    let Some(path) = round_rect_path(x, y, w, h, r) else {
        return;
    };
    let Some(g) = glass_for(name) else {
        // Wallpaper not installed (dev shell) — keep the flat colour.
        pixmap.fill_path(
            &path,
            &Paint {
                shader: tiny_skia::Shader::SolidColor(fallback),
                anti_alias: true,
                ..Default::default()
            },
            tiny_skia::FillRule::Winding,
            Transform::default(),
            None,
        );
        return;
    };
    let (ow, oh) = SCREEN.with(|s| *s.borrow());
    // Fill pixel (lx,ly) should sample the blur at screen position
    // (screen_x + lx, screen_y + ly): pattern transform maps pattern
    // space → device space, i.e. scale(GW/ow, GH/oh) then
    // translate(-screen_x, -screen_y).
    let t = Transform::from_row(
        GW as f32 / ow,
        0.0,
        0.0,
        GH as f32 / oh,
        -screen_x * GW as f32 / ow,
        -screen_y * GH as f32 / oh,
    );
    let pattern = tiny_skia::Pattern::new(
        g.blur.as_ref(),
        tiny_skia::SpreadMode::Pad,
        tiny_skia::FilterQuality::Bilinear,
        1.0,
        t,
    );
    pixmap.fill_path(
        &path,
        &Paint {
            shader: pattern,
            anti_alias: true,
            ..Default::default()
        },
        tiny_skia::FillRule::Winding,
        Transform::default(),
        None,
    );
    // Tint + hairline from the cosmos-theme glass tokens.
    let pal = cosmos_theme::palette(dark);
    let [tr, tg, tb, ta] = pal.glass_tint;
    let tint = Color::from_rgba8(tr, tg, tb, ta);
    pixmap.fill_path(
        &path,
        &Paint {
            shader: tiny_skia::Shader::SolidColor(tint),
            anti_alias: true,
            ..Default::default()
        },
        tiny_skia::FillRule::Winding,
        Transform::default(),
        None,
    );
    // 1px inner hairline, inset half a pixel.
    if let Some(inner) = round_rect_path(x + 0.5, y + 0.5, w - 1.0, h - 1.0, (r - 0.5).max(0.0)) {
        pixmap.stroke_path(
            &inner,
            &Paint {
                shader: tiny_skia::Shader::SolidColor({
                    let [r, g, b, a] = pal.glass_hairline;
                    Color::from_rgba8(r, g, b, a)
                }),
                anti_alias: true,
                ..Default::default()
            },
            &tiny_skia::Stroke {
                width: 1.0,
                ..Default::default()
            },
            Transform::default(),
            None,
        );
    }
}
