//! Surface materials for the bar, the dock and the notch.
//!
//! The panel paints the tint, the lit edge and the hairline; the compositor
//! does what only it can see, the blur and grain of whatever is behind:
//!
//! * **Solid**: an opaque fill. Nothing shows through.
//! * **See-through**: a light tint; the desktop stays sharp behind it.
//! * **Glass**: what is behind is blurred and saturated a little; the
//!   surface is a clear luminous tint with an optical directional specular rim.
//! * **Acrylic**: blurred, tinted with the wallpaper's own colour, with a
//!   fine grain (the compositor's noise).
//!
//! Blur is requested per shape with `ext-background-effect-v1` (see
//! [`blur_rects`]), so a rounded dock card blurs only the card, not the clear
//! room above it. Grain and saturation come from the layer rules in
//! `solyra-config.kdl`, matched on the namespace suffix from [`namespace`].

use crate::canvas::{Canvas, Icon};
use crate::tokens::Rgba;
#[cfg(test)]
use crate::tokens::Theme;
pub use solyra_conf::look::Material;

/// The layer-shell namespace for a surface of this material, e.g.
/// `solyra-dock.glass`. The compositor's layer rules key on the suffix.
pub fn namespace(base: &str, m: Material) -> String {
    format!("{base}.{}", m.as_str())
}

/// Does the compositor need to blur behind this material?
pub fn blurs(m: Material) -> bool {
    matches!(m, Material::Glass | Material::Acrylic)
}

/// What the panel paints for one surface body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slab {
    pub kind: Material,
    pub fill: Rgba,
    /// Rim around the shape.
    pub edge: Rgba,
    /// Directional top catch-light (glass / acrylic).
    pub highlight: Option<Rgba>,
}

fn luma(c: Rgba) -> u32 {
    (c.0 as u32 * 299 + c.1 as u32 * 587 + c.2 as u32 * 114) / 1000
}

fn is_dark(c: Rgba) -> bool {
    luma(c) < 128
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgba::rgba(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2), a.3)
}

/// Does text on a see-through surface fight the wallpaper behind it? A dark
/// theme over a bright wallpaper (or the reverse) would set the text against
/// its own colour.
fn fights(dark: bool, tint: Option<Rgba>) -> bool {
    tint.is_some_and(|t| if dark { luma(t) > 150 } else { luma(t) < 95 })
}

/// Should content sitting straight on a Glass surface (menu-bar text, dock
/// tray) flip to the opposite ink? Like macOS's automatic menu bar: dark text
/// over a bright wallpaper, light text over a dark one. Other materials carry
/// their own tint, so their ink never flips.
pub fn ink_flips(m: Material, base: Rgba, tint: Option<Rgba>) -> bool {
    // Dark glass is smoked, so white text always reads on it; only the light
    // veil can end up fighting the wallpaper.
    m == Material::Glass && !is_dark(base) && fights(false, tint)
}

/// The slab for `base` (the theme colour the surface would otherwise use).
/// `tint` is the wallpaper's average colour, used by Acrylic's hue and by
/// Clear's legibility lift.
pub fn slab(m: Material, base: Rgba, tint: Option<Rgba>) -> Slab {
    let dark = is_dark(base);
    let fights = fights(dark, tint);
    let boost = |a: u8| if fights { a.saturating_add(70) } else { a };
    let rim = |a: u8| {
        if dark {
            Rgba::rgba(255, 255, 255, a)
        } else {
            Rgba::rgba(0, 0, 0, a)
        }
    };
    let with_alpha = |c: Rgba, a: u8| Rgba::rgba(c.0, c.1, c.2, a);
    match m {
        Material::Solid => Slab {
            kind: m,
            fill: with_alpha(base, 255),
            edge: rim(if dark { 24 } else { 26 }),
            highlight: None,
        },
        Material::Clear => Slab {
            kind: m,
            fill: with_alpha(base, boost(if dark { 110 } else { 120 })),
            edge: rim(if dark { 38 } else { 28 }),
            highlight: None,
        },
        // Frosted glass: a neutral white veil, never a grey one. Darkening a
        // warm wallpaper with grey is what turns sand into mauve; a white veil
        // keeps its hue and only lifts it, the way the macOS dock does.
        // Dark theme: smoked glass, the pearOS / macOS Tahoe Dark dock: a
        // near-black tint over the blur with a 1 px white 20 % rim.
        // Dark theme: real frosted glass (macOS Tahoe dark dock). The
        // compositor blurs the wallpaper behind; the panel adds only a faint
        // grey wash, a white veil that brightens toward the top, a lit rim
        // and a specular top edge, so the wallpaper's colour shows through.
        Material::Glass if dark => Slab {
            kind: m,
            fill: Rgba::rgba(255, 255, 255, boost(26)),
            edge: Rgba::rgba(255, 255, 255, 80),
            highlight: Some(Rgba::rgba(255, 255, 255, 220)),
        },
        Material::Glass => Slab {
            kind: m,
            fill: Rgba::rgba(255, 255, 255, 70),
            edge: rim(24),
            highlight: Some(Rgba::rgba(255, 255, 255, 220)),
        },
        Material::Acrylic => {
            // Windows Acrylic / Mica: wallpaper hue + denser luminosity fill + soft rim.
            let hue = tint.unwrap_or(base);
            let toward = if dark { 0.30 } else { 0.24 };
            Slab {
                kind: m,
                fill: with_alpha(mix(base, hue, toward), if dark { 164 } else { 176 }),
                edge: rim(if dark { 40 } else { 26 }),
                highlight: Some(Rgba::rgba(255, 255, 255, if dark { 68 } else { 165 })),
            }
        }
    }
}

/// Paint a rounded body (radius 0 for full-width bars).
pub fn paint(cv: &mut Canvas<'_>, s: &Slab, x: i32, y: i32, w: i32, h: i32, r: f32) {
    if r <= 0.5 {
        if s.kind == Material::Glass && is_dark(s.fill) {
            // Dark menu bar: a thin smoke that fades out, so white text reads
            // over any wallpaper while the bar still looks clear.
            cv.fill_rect_vgrad(
                x,
                y,
                w,
                h,
                Rgba::rgba(18, 18, 20, 110),
                Rgba::rgba(18, 18, 20, 60),
            );
        } else if s.kind == Material::Glass {
            // A glass bar is only a veil that fades into the wallpaper.
            let top = Rgba::rgba(255, 255, 255, s.fill.3 / 2);
            cv.fill_rect_vgrad(x, y, w, h, top, Rgba::rgba(255, 255, 255, 0));
        } else {
            cv.fill_rect(x, y, w, h, s.fill);
        }
        return;
    }
    match s.kind {
        Material::Glass if s.fill.0 == 255 => {
            // Liquid Glass, Droppy's / macOS 26's recipe: the blurred desktop
            // shows through a thin veil with lensed, lit edges.
            liquid_glass(cv, x, y, w, h, r, s.fill.3, 1.0);
        }
        Material::Glass => {
            let a = s.fill.3;
            cv.round_rect_vgrad(
                x,
                y,
                w,
                h,
                r,
                Rgba::rgba(255, 255, 255, a.saturating_add(20)),
                Rgba::rgba(255, 255, 255, a.saturating_sub(10)),
            );
            let hl = s.highlight.unwrap_or(s.fill);
            cv.round_rect_glass_rim(
                x,
                y,
                w,
                h,
                r,
                hl,
                Rgba::rgba(255, 255, 255, hl.3 / 3),
                Rgba::rgba(255, 255, 255, hl.3 / 5),
            );
        }
        Material::Acrylic => {
            let dark = is_dark(s.fill);
            let top_tint = if dark {
                Rgba::rgba(
                    s.fill.0.saturating_add(34),
                    s.fill.1.saturating_add(34),
                    s.fill.2.saturating_add(36),
                    s.fill.3.saturating_add(16).min(235),
                )
            } else {
                Rgba::rgba(
                    s.fill.0.saturating_add(10),
                    s.fill.1.saturating_add(10),
                    s.fill.2.saturating_add(10),
                    s.fill.3.saturating_add(24).min(242),
                )
            };
            cv.round_rect_vgrad(x, y, w, h, r, top_tint, s.fill);
            let hl = s.highlight.unwrap_or(s.fill);
            cv.round_rect_glass_rim(
                x,
                y,
                w,
                h,
                r,
                hl,
                Rgba::rgba(255, 255, 255, if dark { 52 } else { 96 }),
                Rgba::rgba(
                    if dark { 255 } else { 0 },
                    if dark { 255 } else { 0 },
                    if dark { 255 } else { 0 },
                    if dark { 28 } else { 26 },
                ),
            );
        }
        _ => {
            cv.round_rect(x, y, w, h, r, s.fill);
            cv.stroke_round_rect(x, y, w, h, r, 1.0, s.edge);
        }
    }
}

/// The Liquid Glass body, Apple's recipe as closely as a 2D painter can get
/// over the compositor's blur:
///
/// * a thin veil that is brighter toward the top (light falling on a droplet);
/// * a **refraction band**: the inner few pixels of the edge are lit, the way
///   a thick lens bends the light at its rim (Liquid Glass's lensing);
/// * a curved **top sheen** over the upper part of the body;
/// * a **specular rim**: sharp white at the top, soft at the sides and a
///   second, weaker catch-light at the bottom (internal reflection);
/// * an outer hairline so the edge reads on any wallpaper.
///
/// `veil` is the body's white alpha; `depth` (0..=1) scales every highlight.
pub fn liquid_glass(
    cv: &mut Canvas<'_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    r: f32,
    veil: u8,
    depth: f32,
) {
    if w <= 2 || h <= 2 {
        return;
    }
    let d = depth.clamp(0.0, 1.0);
    let a = |v: f32| (v * d).round().clamp(0.0, 255.0) as u8;
    // Veil.
    cv.round_rect_vgrad(
        x,
        y,
        w,
        h,
        r,
        Rgba::rgba(255, 255, 255, veil.saturating_add(16)),
        Rgba::rgba(255, 255, 255, veil / 3),
    );
    // Top sheen: a soft dome of light over the upper ~45 %.
    let sh = ((h as f32) * 0.46).round() as i32;
    cv.round_rect_vgrad(
        x + 2,
        y + 2,
        w - 4,
        sh,
        (r - 2.0).max(0.0),
        Rgba::rgba(255, 255, 255, a(34.0)),
        Rgba::rgba(255, 255, 255, 0),
    );
    // Refraction: the edge is a lens. A directional rim light lights the
    // band facing the top-left strongly and the bottom-right softly.
    let band = (r * 0.55).clamp(5.0, 12.0);
    cv.rim_light(
        x,
        y,
        w,
        h,
        r,
        band,
        Rgba::rgba(255, 255, 255, a(150.0)),
        0.5,
    );
    // A crisp 1 px specular line on the lit edge.
    cv.rim_light(
        x,
        y,
        w,
        h,
        r,
        1.4,
        Rgba::rgba(255, 255, 255, a(235.0)),
        0.55,
    );
    // Outer hairline (occlusion) so the glass edge reads on bright wallpaper.
    cv.stroke_round_rect(
        x - 1,
        y - 1,
        w + 2,
        h + 2,
        r + 1.0,
        1.0,
        Rgba::rgba(0, 0, 0, 56),
    );
}

/// pearOS's glass, from its Tahoe / Tahoe Dark dock skins and the
/// `dock_prism` shader: the compositor's strong blur behind; a body of black
/// at 15 % (dark) or white at 15 % (light); one 1 px rim at about 30 %
/// (light grey in the dark skin, white in the light one); and an additive
/// RGB prism glow fading in over a band 1.5 × the radius wide.
/// `body` overrides the body's alpha (the notch uses a darker one so text
/// on it always reads).
#[allow(clippy::too_many_arguments)]
pub fn pear_glass(
    cv: &mut Canvas<'_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    r: f32,
    dark: bool,
    body: Option<u8>,
) {
    let a = body.unwrap_or(38);
    let fill = if dark {
        Rgba::rgba(0, 0, 0, a)
    } else {
        Rgba::rgba(255, 255, 255, a)
    };
    cv.round_rect(x, y, w, h, r, fill);
    let rim = if dark {
        Rgba::rgba(165, 165, 165, 80)
    } else {
        Rgba::rgba(255, 255, 255, 80)
    };
    cv.stroke_round_rect(x, y, w, h, r, 1.0, rim);
    // pearOS: refraction 8 × fringing 2 × 0.004, edge = 1.5 × radius 24.
    cv.prism_rim(x, y, w, h, r, 36.0, 8.0 * 2.0 * 0.004);
}

/// macOS 26 Liquid Glass, read off Apple's own screenshots: a thin tint
/// (smoke in dark, milk in light) over the compositor's blur, which also
/// bends the backdrop at the edge (the `refraction` layer rule); a sharp
/// specular line lit from the top-left with a softer catch on the
/// bottom-right; a soft lensed band inside the edge; a hairline outside.
/// True when the compositor renders the glass material itself
/// (solyra_glass_v1): the panel then skips its CPU tint and rims, which
/// would double-paint, and keeps only what the compositor cannot know —
/// the hairline outside the shape.
pub static COMPOSITOR_GLASS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn compositor_glass() -> bool {
    COMPOSITOR_GLASS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn apple_glass(cv: &mut Canvas<'_>, x: i32, y: i32, w: i32, h: i32, r: f32, dark: bool) {
    if compositor_glass() {
        cv.stroke_round_rect(
            x - 1,
            y - 1,
            w + 2,
            h + 2,
            r + 1.0,
            1.0,
            Rgba::rgba(0, 0, 0, if dark { 54 } else { 26 }),
        );
        return;
    }
    let fill = if dark {
        Rgba::rgba(18, 18, 22, 92)
    } else {
        Rgba::rgba(255, 255, 255, 86)
    };
    cv.round_rect(x, y, w, h, r, fill);
    let band = (r * 0.6).clamp(5.0, 12.0);
    cv.rim_light(
        x,
        y,
        w,
        h,
        r,
        band,
        Rgba::rgba(255, 255, 255, if dark { 46 } else { 96 }),
        0.6,
    );
    cv.rim_light(
        x,
        y,
        w,
        h,
        r,
        1.6,
        Rgba::rgba(255, 255, 255, if dark { 210 } else { 250 }),
        0.75,
    );
    cv.stroke_round_rect(
        x - 1,
        y - 1,
        w + 2,
        h + 2,
        r + 1.0,
        1.0,
        Rgba::rgba(0, 0, 0, if dark { 54 } else { 26 }),
    );
}

/// Smoked Liquid Glass for the notch and the island: the same optics over a
/// dark tint, so white text and art stay readable on any wallpaper while the
/// desktop still shows through, blurred.
pub fn smoked_glass(cv: &mut Canvas<'_>, x: i32, y: i32, w: i32, h: i32, r: f32) {
    if compositor_glass() {
        cv.stroke_round_rect(x - 1, y - 1, w + 2, h + 2, r + 1.0, 1.0, Rgba::rgba(0, 0, 0, 54));
        return;
    }
    cv.round_rect_vgrad(
        x,
        y,
        w,
        h,
        r,
        Rgba::rgba(10, 10, 14, 150),
        Rgba::rgba(6, 6, 10, 178),
    );
    liquid_glass(cv, x, y, w, h, r, 6, 0.75);
}

/// A rounded rectangle as row strips, for a `wl_region`: corners are
/// stepped one logical pixel at a time, so the compositor's blur follows the
/// curve closely enough that the painted rim hides the steps.
pub fn blur_rects(x: i32, y: i32, w: i32, h: i32, r: f32) -> Vec<(i32, i32, i32, i32)> {
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let r = r.min(w as f32 / 2.0).min(h as f32 / 2.0).max(0.0);
    let ri = r.ceil() as i32;
    if ri == 0 {
        return vec![(x, y, w, h)];
    }
    let mut out = Vec::with_capacity(2 * ri as usize + 1);
    let inset = |row: i32| {
        // Distance from the corner circle's centre to this row's middle.
        let dy = r - (row as f32 + 0.5);
        if dy <= 0.0 {
            0
        } else {
            (r - (r * r - dy * dy).max(0.0).sqrt()).round() as i32
        }
    };
    for row in 0..ri {
        let i = inset(row);
        out.push((x + i, y + row, w - 2 * i, 1));
    }
    if h - 2 * ri > 0 {
        out.push((x, y + ri, w, h - 2 * ri));
    }
    for row in 0..ri {
        let i = inset(row);
        out.push((x + i, y + h - 1 - row, w - 2 * i, 1));
    }
    out
}

/// Average colour of a wallpaper (sampled on a grid), for Acrylic's tint.
pub fn average(img: &Icon) -> Option<Rgba> {
    if img.width == 0 || img.height == 0 {
        return None;
    }
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    let step_x = (img.width / 64).max(1);
    let step_y = (img.height / 64).max(1);
    let mut y = 0;
    while y < img.height {
        let mut x = 0;
        while x < img.width {
            let i = ((y * img.width + x) * 4) as usize;
            r += img.pixels[i] as u64;
            g += img.pixels[i + 1] as u64;
            b += img.pixels[i + 2] as u64;
            n += 1;
            x += step_x;
        }
        y += step_y;
    }
    Some(Rgba::rgb((r / n) as u8, (g / n) as u8, (b / n) as u8))
}

/// The notch / Dynamic Island is pure hardware black — never mauve-tinted.
#[allow(dead_code)]
pub fn notch_base(_m: Material) -> Rgba {
    Rgba::rgb(0, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_is_opaque_and_others_are_not() {
        let base = Theme::dark().bar;
        assert_eq!(slab(Material::Solid, base, None).fill.3, 255);
        for m in [Material::Clear, Material::Glass, Material::Acrylic] {
            assert!(slab(m, base, None).fill.3 < 255, "{m:?}");
        }
        assert!(slab(Material::Glass, base, None).highlight.is_some());
        assert!(blurs(Material::Glass) && blurs(Material::Acrylic));
        assert!(!blurs(Material::Solid) && !blurs(Material::Clear));
    }

    #[test]
    fn clear_gets_denser_over_a_wallpaper_that_fights_the_text() {
        let dark_bar = Theme::dark().bar;
        let calm = slab(Material::Clear, dark_bar, Some(Rgba::rgb(20, 40, 90)))
            .fill
            .3;
        let bright = slab(Material::Clear, dark_bar, Some(Rgba::rgb(240, 230, 220)))
            .fill
            .3;
        assert!(bright > calm + 20, "{calm} -> {bright}");
        let light_bar = Theme::light().bar;
        let dark_wall = slab(Material::Clear, light_bar, Some(Rgba::rgb(15, 15, 30)))
            .fill
            .3;
        let light_wall = slab(Material::Clear, light_bar, Some(Rgba::rgb(230, 220, 240)))
            .fill
            .3;
        assert!(dark_wall > light_wall + 20);
        assert_eq!(
            slab(Material::Solid, dark_bar, Some(Rgba::rgb(255, 255, 255)))
                .fill
                .3,
            255
        );
    }

    #[test]
    fn glass_is_see_through_with_a_lit_edge_in_both_themes() {
        for tint in [
            None,
            Some(Rgba::rgb(240, 200, 170)),
            Some(Rgba::rgb(10, 30, 60)),
        ] {
            let f = slab(Material::Glass, Theme::light().bar, tint).fill;
            assert_eq!((f.0, f.1, f.2), (255, 255, 255), "{tint:?}");
            assert!(f.3 > 0 && f.3 < 120, "{}", f.3);
            let d = slab(Material::Glass, Theme::dark().bar, tint);
            assert!(
                d.fill.3 <= 124,
                "dark glass stays see-through: {:?}",
                d.fill
            );
            assert!(d.highlight.is_some(), "and has a lit edge");
        }
    }

    #[test]
    fn glass_ink_flips_only_when_the_text_would_fight_the_wallpaper() {
        let dark_bar = Theme::dark().bar;
        let light_bar = Theme::light().bar;
        let sand = Some(Rgba::rgb(235, 205, 175));
        let night = Some(Rgba::rgb(14, 30, 60));
        assert!(
            !ink_flips(Material::Glass, dark_bar, sand),
            "smoked glass keeps white ink"
        );
        assert!(!ink_flips(Material::Glass, dark_bar, night));
        assert!(!ink_flips(Material::Glass, dark_bar, None));
        assert!(ink_flips(Material::Glass, light_bar, night));
        assert!(!ink_flips(Material::Glass, light_bar, sand));
        for m in [Material::Solid, Material::Clear, Material::Acrylic] {
            assert!(!ink_flips(m, dark_bar, sand), "{m:?} carries its own tint");
        }
    }

    #[test]
    fn acrylic_takes_the_wallpaper_hue() {
        let base = Theme::dark().bar;
        let plain = slab(Material::Acrylic, base, None).fill;
        let blue = slab(Material::Acrylic, base, Some(Rgba::rgb(0, 60, 200))).fill;
        assert!(blue.2 > plain.2, "more blue: {plain:?} -> {blue:?}");
    }

    #[test]
    fn namespaces_carry_the_material() {
        assert_eq!(
            namespace("solyra-bar", Material::Acrylic),
            "solyra-bar.acrylic"
        );
    }

    #[test]
    fn blur_rects_cover_a_rounded_shape_exactly_once() {
        let (w, h, r) = (120, 58, 20.0);
        let rects = blur_rects(10, 5, w, h, r);
        let mut grid = vec![0u8; (w * h) as usize];
        for (x, y, rw, rh) in rects {
            for yy in y..y + rh {
                for xx in x..x + rw {
                    grid[((yy - 5) * w + (xx - 10)) as usize] += 1;
                }
            }
        }
        assert!(grid.iter().all(|&c| c <= 1), "no overlap");
        // Centre and edge midpoints are covered; the exact corner is not.
        assert_eq!(grid[(h / 2 * w + w / 2) as usize], 1);
        assert_eq!(grid[(h / 2 * w) as usize], 1);
        assert_eq!(grid[0], 0);
        assert_eq!(blur_rects(0, 0, 10, 10, 0.0), vec![(0, 0, 10, 10)]);
        assert!(blur_rects(0, 0, 0, 10, 4.0).is_empty());
    }

    #[test]
    fn average_of_a_flat_image_is_its_colour() {
        let img = Icon {
            width: 4,
            height: 4,
            pixels: [10u8, 20, 30, 255].repeat(16),
        };
        assert_eq!(average(&img), Some(Rgba::rgb(10, 20, 30)));
    }
}
