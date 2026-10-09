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
    dark: bool,
    /// Blurred + saturation-lifted wallpaper at GW×GH.
    blur: Pixmap,
}

fn dir() -> std::path::PathBuf {
    std::env::var("COSMOS_WALLPAPER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/share/cosmos/wallpapers"))
}

/// Glass luminance bounds (linear Y) under the tint: white text keeps
/// 7:1 on dark glass and ink keeps 4.5:1 secondary on light glass over
/// any wallpaper (v5 §1.2/§1.3).
const DARK_CAP: f32 = 0.10;
const LIGHT_FLOOR: f32 = 0.45;

fn lin(c: f32) -> f32 {
    let c = c / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn unlin(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    255.0
        * if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
}

/// Backdrop material for one blurred wallpaper pixel: saturation ×1.4,
/// then luminance clamped in linear light (scaling keeps the hue, so
/// glass still carries the wallpaper colour).
fn material(px: [f32; 3], dark: bool) -> [f32; 3] {
    let mean = (px[0] + px[1] + px[2]) / 3.0;
    let mut l = px.map(|c| lin((mean + (c - mean) * 1.4).clamp(0.0, 255.0)));
    let y = 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
    if dark && y > DARK_CAP {
        l = l.map(|c| c * DARK_CAP / y);
    }
    if !dark && y < LIGHT_FLOOR {
        let t = (LIGHT_FLOOR - y) / (1.0 - y);
        l = l.map(|c| c + (1.0 - c) * t);
    }
    l.map(unlin)
}

/// Decode + downscale + blur the named wallpaper into the per-mode glass
/// backdrop.
fn load(name: &str, dark: bool) -> Option<Glass> {
    let d = dir();
    let img = ["-1920x1080.png", "-3840x2160.png"]
        .iter()
        .find_map(|s| image::open(d.join(format!("{name}{s}"))).ok())?
        .to_rgba8();
    // Downscale to the working size (fast filter — blurred anyway).
    let small = image::imageops::resize(&img, GW, GH, image::imageops::FilterType::Triangle);
    // ~40px blur at 1080p ≈ sigma ~10 at this downscale.
    let mut blurred = image::imageops::blur(&small, 10.0);
    // Material plus ±1% hash-noise dither so the upscaled blur never
    // bands.
    for (i, px) in blurred.chunks_exact_mut(4).enumerate() {
        let m = material([px[0] as f32, px[1] as f32, px[2] as f32], dark);
        let mut h = (i as u32).wrapping_mul(0x9E37_79B1);
        h ^= h >> 15;
        let n = ((h & 0xFF) as f32 / 255.0 - 0.5) * 0.02 * 255.0;
        for (c, v) in px[0..3].iter_mut().zip(m) {
            *c = (v + n).clamp(0.0, 255.0) as u8;
        }
    }
    // Premultiplied == straight for opaque pixels; RgbaImage is already
    // [r,g,b,a] byte order.
    let pm = Pixmap::from_vec(blurred.into_raw(), tiny_skia::IntSize::from_wh(GW, GH)?)?;
    Some(Glass {
        name: name.to_string(),
        dark,
        blur: pm,
    })
}

/// The blurred wallpaper for the given name — cached; reloads on change.
fn glass_for(name: &str, dark: bool) -> Option<Glass> {
    GLASS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().map(|g| (g.name.as_str(), g.dark)) != Some((name, dark)) {
            *slot = load(name, dark);
        }
        // Cloning a Pixmap is a deep copy of a small buffer — fine at
        // per-frame rates for a handful of surfaces.
        slot.as_ref().map(|g| Glass {
            name: g.name.clone(),
            dark: g.dark,
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
    let Some(g) = glass_for(name, dark) else {
        // Wallpaper not installed (dev shell) — keep the flat colour.
        pixmap.fill_path(
            &path,
            &Paint {
                shader: tiny_skia::Shader::SolidColor(fallback),
                anti_alias: true,
                ..Default::default()
            },
            tiny_skia::FillRule::Winding,
            crate::draw::xf(),
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
        crate::draw::xf(),
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
        crate::draw::xf(),
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
            crate::draw::xf(),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: [f32; 3]) -> f32 {
        let l = c.map(lin);
        0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2]
    }

    fn over(fg: [u8; 4], bg: [f32; 3]) -> [f32; 3] {
        let a = fg[3] as f32 / 255.0;
        [0, 1, 2].map(|i| fg[i] as f32 * a + bg[i] * (1.0 - a))
    }

    /// Text tokens on tinted glass meet 7 / 4.5 / 3 over any wallpaper,
    /// from black through white and saturated primaries.
    #[test]
    fn glass_text_contrast_on_any_wallpaper() {
        let walls = [
            [0.0, 0.0, 0.0],
            [255.0, 255.0, 255.0],
            [153.0, 130.0, 226.0],
            [219.0, 116.0, 253.0],
            [200.0, 183.0, 162.0],
            [131.0, 208.0, 247.0],
            [255.0, 200.0, 40.0],
            [20.0, 200.0, 80.0],
            [40.0, 30.0, 70.0],
        ];
        for dark in [true, false] {
            let p = cosmos_theme::palette(dark);
            for wall in walls {
                let bg = over(p.glass_tint, material(wall, dark));
                for (tok, min) in [
                    (p.text, 7.0),
                    (p.text_secondary, 4.5),
                    (p.text_tertiary, 3.0),
                ] {
                    let (a, b) = (lum(over(tok, bg)), lum(bg));
                    let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    assert!(
                        ratio >= min,
                        "dark={dark} {tok:?} over {wall:?}: {ratio:.2} < {min}"
                    );
                }
            }
        }
    }

    /// Clamping scales in linear light, so the dominant channel survives.
    #[test]
    fn material_keeps_wallpaper_hue() {
        let m = material([219.0, 116.0, 253.0], true);
        assert!(m[2] > m[1] && m[0] > m[1], "{m:?}");
    }
}
