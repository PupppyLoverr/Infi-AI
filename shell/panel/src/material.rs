//! Surface materials for the bar, the dock and the notch.
//!
//! The panel paints the tint, the lit edge and the hairline; the compositor
//! does what only it can see, the blur and grain of whatever is behind:
//!
//! * **Solid**: an opaque fill. Nothing shows through.
//! * **See-through**: a light tint; the desktop stays sharp behind it.
//! * **Glass**: what is behind is blurred and saturated a little; the
//!   surface is a thin tint with a lit top edge and a faint rim.
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
    pub fill: Rgba,
    /// Rim around the shape.
    pub edge: Rgba,
    /// A one-pixel lit line just inside the top edge (glass only).
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

/// The slab for `base` (the theme colour the surface would otherwise use).
/// `tint` is the wallpaper's average colour, used by Acrylic.
pub fn slab(m: Material, base: Rgba, tint: Option<Rgba>) -> Slab {
    let dark = is_dark(base);
    // Adaptive density (the idea behind Liquid Glass adapting to what is
    // behind it, and Acrylic's luminosity layer): when the wallpaper is the
    // same lightness as the text (a bright wallpaper under light-on-dark
    // text, or the reverse) the translucent materials get denser so text
    // keeps its contrast.
    let fights = tint.is_some_and(|t| if dark { luma(t) > 150 } else { luma(t) < 95 });
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
            fill: with_alpha(base, 255),
            edge: rim(if dark { 22 } else { 26 }),
            highlight: None,
        },
        Material::Clear => Slab {
            fill: with_alpha(base, boost(if dark { 110 } else { 120 })),
            edge: rim(if dark { 36 } else { 28 }),
            highlight: None,
        },
        // Liquid Glass–like: light tint, strong lit rim, wallpaper reads through.
        Material::Glass => Slab {
            fill: with_alpha(base, boost(if dark { 88 } else { 108 })),
            edge: rim(if dark { 52 } else { 36 }),
            highlight: Some(Rgba::rgba(255, 255, 255, if dark { 72 } else { 190 })),
        },
        Material::Acrylic => {
            // Windows Acrylic: wallpaper hue + denser fill + soft rim.
            let hue = tint.unwrap_or(base);
            let toward = if dark { 0.34 } else { 0.26 };
            Slab {
                fill: with_alpha(mix(base, hue, toward), if dark { 165 } else { 170 }),
                edge: rim(if dark { 32 } else { 26 }),
                highlight: Some(Rgba::rgba(255, 255, 255, if dark { 28 } else { 120 })),
            }
        }
    }
}

/// Paint a rounded body (radius 0 for full-width bars).
pub fn paint(cv: &mut Canvas<'_>, s: &Slab, x: i32, y: i32, w: i32, h: i32, r: f32) {
    if r <= 0.5 {
        cv.fill_rect(x, y, w, h, s.fill);
    } else {
        cv.round_rect(x, y, w, h, r, s.fill);
        cv.stroke_round_rect(x, y, w, h, r, 1.0, s.edge);
    }
    if let Some(hl) = s.highlight {
        let inset = (r * 0.6).round() as i32;
        cv.fill_rect(x + inset, y + 1, (w - 2 * inset).max(0), 1, hl);
    }
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

/// The theme's bar colour for a material: the notch is black by nature, the
/// bar and dock use the theme.
pub fn notch_base(m: Material) -> Rgba {
    match m {
        Material::Solid => Rgba::rgb(0, 0, 0),
        _ => Rgba::rgb(0x0c, 0x0b, 0x10),
    }
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
    fn glass_gets_denser_over_a_wallpaper_that_fights_the_text() {
        let dark_bar = Theme::dark().bar;
        let calm = slab(Material::Glass, dark_bar, Some(Rgba::rgb(20, 40, 90))).fill.3;
        let bright = slab(Material::Glass, dark_bar, Some(Rgba::rgb(240, 230, 220))).fill.3;
        assert!(bright > calm + 40, "{calm} -> {bright}");
        let light_bar = Theme::light().bar;
        let dark_wall = slab(Material::Glass, light_bar, Some(Rgba::rgb(15, 15, 30))).fill.3;
        let light_wall = slab(Material::Glass, light_bar, Some(Rgba::rgb(230, 220, 240))).fill.3;
        assert!(dark_wall > light_wall + 40);
        assert_eq!(slab(Material::Solid, dark_bar, Some(Rgba::rgb(255, 255, 255))).fill.3, 255);
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
