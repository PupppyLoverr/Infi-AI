//! CosmosOS design tokens (spec-v3 §2.1). Every surface — uitk apps,
//! the shell, compositor chrome and the greeter — reads its spacing,
//! radii, type, palette, elevation and motion from here.
//!
//! Colours are straight (non-premultiplied) RGBA bytes; convert at the
//! paint site.

/// Straight RGBA.
pub type Rgba = [u8; 4];

/// 4px spacing grid.
pub mod space {
    pub const S4: f32 = 4.0;
    pub const S8: f32 = 8.0;
    pub const S12: f32 = 12.0;
    pub const S16: f32 = 16.0;
    pub const S20: f32 = 20.0;
    pub const S24: f32 = 24.0;
    pub const S32: f32 = 32.0;
    pub const S40: f32 = 40.0;
}

pub mod radius {
    pub const CONTROL: f32 = 8.0;
    pub const ROW: f32 = 6.0;
    pub const CARD: f32 = 12.0;
    pub const WINDOW: f32 = 12.0;
    pub const PANEL: f32 = 14.0;
    pub const DOCK: f32 = 22.0;
}

/// Row height for lists (macOS Mail/Finder density).
pub const ROW_H: f32 = 28.0;

/// A text style: size / line height in px, weight 100..=900.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub line: f32,
    pub weight: u16,
}

pub mod text {
    use super::TextStyle;
    pub const CAPTION: TextStyle = TextStyle { size: 11.0, line: 14.0, weight: 500 };
    /// Tracking for uppercase caption labels.
    pub const CAPTION_TRACKING: f32 = 0.4;
    pub const BODY: TextStyle = TextStyle { size: 13.0, line: 18.0, weight: 400 };
    pub const BODY_STRONG: TextStyle = TextStyle { size: 13.0, line: 18.0, weight: 600 };
    pub const TITLE3: TextStyle = TextStyle { size: 15.0, line: 20.0, weight: 600 };
    pub const TITLE2: TextStyle = TextStyle { size: 20.0, line: 26.0, weight: 600 };
    pub const TITLE1: TextStyle = TextStyle { size: 28.0, line: 34.0, weight: 700 };
    pub const CLOCK: TextStyle = TextStyle { size: 64.0, line: 72.0, weight: 300 };
    pub const MONO: TextStyle = TextStyle { size: 13.0, line: 18.0, weight: 400 };
}

/// One mode's surface + text colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub base: Rgba,
    pub window: Rgba,
    pub sidebar: Rgba,
    pub raised: Rgba,
    pub hairline: Rgba,
    pub text: Rgba,
    pub text_secondary: Rgba,
    pub text_tertiary: Rgba,
    /// Glass tint over the blurred wallpaper.
    pub glass_tint: Rgba,
    /// Glass 1px inner hairline (top edge in light mode).
    pub glass_hairline: Rgba,
}

const fn a(c: [u8; 3], alpha: f32) -> Rgba {
    [c[0], c[1], c[2], (alpha * 255.0 + 0.5) as u8]
}

const DARK_TEXT: [u8; 3] = [0xF4, 0xF1, 0xFA];
const LIGHT_TEXT: [u8; 3] = [0x1A, 0x16, 0x25];

pub const DARK: Palette = Palette {
    dark: true,
    base: a([0x14, 0x11, 0x1F], 1.0),
    window: a([0x1C, 0x17, 0x30], 0.90),
    sidebar: a([0x16, 0x12, 0x2A], 0.78),
    raised: a([0x26, 0x20, 0x40], 1.0),
    hairline: a([0xFF, 0xFF, 0xFF], 0.08),
    text: a(DARK_TEXT, 1.0),
    text_secondary: a(DARK_TEXT, 0.62),
    text_tertiary: a(DARK_TEXT, 0.42),
    glass_tint: [28, 23, 48, 117],
    glass_hairline: a([0xFF, 0xFF, 0xFF], 0.10),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    base: a([0xF3, 0xF1, 0xF8], 1.0),
    window: a([0xFB, 0xFA, 0xFE], 0.92),
    sidebar: a([0xEC, 0xE8, 0xF5], 0.90),
    raised: a([0xFF, 0xFF, 0xFF], 1.0),
    hairline: a([0x00, 0x00, 0x00], 0.08),
    text: a(LIGHT_TEXT, 1.0),
    text_secondary: a(LIGHT_TEXT, 0.64),
    text_tertiary: a(LIGHT_TEXT, 0.50),
    glass_tint: [255, 255, 255, 140],
    glass_hairline: a([0xFF, 0xFF, 0xFF], 0.60),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

pub mod semantic {
    pub const SUCCESS: [u8; 3] = [0x34, 0xC7, 0x7B];
    pub const WARNING: [u8; 3] = [0xF5, 0xA5, 0x24];
    pub const DANGER: [u8; 3] = [0xF2, 0x55, 0x5A];
}

/// Fallback accent when no wallpaper has been sampled.
pub const ACCENT_FALLBACK: [u8; 3] = [0x8B, 0x5C, 0xF6];

/// A drop shadow: y offset, blur radius, alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elevation {
    pub dy: f32,
    pub blur: f32,
    pub alpha: f32,
}

pub mod elevation {
    use super::Elevation;
    pub const E1: Elevation = Elevation {
        dy: 1.0,
        blur: 2.0,
        alpha: 0.20,
    };
    pub const E2: Elevation = Elevation {
        dy: 8.0,
        blur: 24.0,
        alpha: 0.35,
    };
    pub const E3_FOCUSED: Elevation = Elevation {
        dy: 18.0,
        blur: 48.0,
        alpha: 0.45,
    };
    pub const E3_UNFOCUSED: Elevation = Elevation {
        dy: 10.0,
        blur: 28.0,
        alpha: 0.30,
    };
}

pub mod motion {
    pub const STANDARD_MS: u32 = 220;
    pub const OPEN_MS: u32 = 240;
    pub const CLOSE_MS: u32 = 160;
    pub const REDUCED_MS: u32 = 120;
    pub const OPEN_SCALE_FROM: f32 = 0.96;
    /// cubic-bezier(0.2, 0.8, 0.2, 1) control points.
    pub const EASE: (f32, f32, f32, f32) = (0.2, 0.8, 0.2, 1.0);

    /// Evaluate the standard ease at progress `t` in 0..=1.
    pub fn ease(t: f32) -> f32 {
        super::cubic_bezier(EASE, t.clamp(0.0, 1.0))
    }
}

fn cubic_bezier((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    let bez = |t: f32, p1: f32, p2: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    // Bisection on x(t) — monotonic for valid easing curves.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, y1, y2)
}

/// Accent family derived from one base colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accent {
    pub base: [u8; 3],
    pub hover: [u8; 3],
    pub pressed: [u8; 3],
    /// Selection fill (20% alpha).
    pub tint: Rgba,
    /// Text drawn on an accent fill.
    pub on_accent: [u8; 3],
}

impl Accent {
    /// Build the family; the base is darkened until white text on it
    /// reaches 4.5:1 contrast.
    pub fn new(rgb: [u8; 3]) -> Self {
        let (h, s, mut l) = rgb_to_hsl(rgb);
        let mut base = rgb;
        while contrast([255, 255, 255], base) < 4.5 && l > 0.05 {
            l -= 0.02;
            base = hsl_to_rgb(h, s, l);
        }
        let hover = hsl_to_rgb(h, s, (l + 0.08).min(1.0));
        let pressed = hsl_to_rgb(h, s, (l - 0.08).max(0.0));
        Self {
            base,
            hover,
            pressed,
            tint: [base[0], base[1], base[2], 51],
            on_accent: [255, 255, 255],
        }
    }
}

fn lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
pub fn luminance(c: [u8; 3]) -> f32 {
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG contrast ratio between two colours.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn rgb_to_hsl(c: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|v| v as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return [v, v, v];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    /// WCAG 2 contrast of the text tokens over every opaque-ish surface,
    /// composited over the shipped wallpapers' extreme hues (v5 §1.2:
    /// primary >= 7, secondary >= 4.5, tertiary >= 3).
    #[test]
    fn text_tokens_meet_v5_contrast() {
        fn lin(c: f64) -> f64 {
            let c = c / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn lum(c: [f64; 3]) -> f64 {
            0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
        }
        fn over(fg: Rgba, bg: [f64; 3]) -> [f64; 3] {
            let a = fg[3] as f64 / 255.0;
            [0, 1, 2].map(|i| fg[i] as f64 * a + bg[i] * (1.0 - a))
        }
        let walls: [[f64; 3]; 5] = [
            [153.0, 130.0, 226.0],
            [219.0, 116.0, 253.0],
            [40.0, 30.0, 70.0],
            [200.0, 183.0, 162.0],
            [131.0, 208.0, 247.0],
        ];
        for p in [&DARK, &LIGHT] {
            for surface in [p.window, p.sidebar, p.raised] {
                for wall in walls {
                    let bg = over(surface, wall);
                    for (tok, min) in [
                        (p.text, 7.0),
                        (p.text_secondary, 4.5),
                        (p.text_tertiary, 3.0),
                    ] {
                        let (a, b) = (lum(over(tok, bg)), lum(bg));
                        let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                        assert!(
                            ratio >= min,
                            "dark={} {tok:?} over {surface:?}/{wall:?}: {ratio:.2} < {min}",
                            p.dark
                        );
                    }
                }
            }
        }
    }

    use super::*;

    #[test]
    fn accent_meets_contrast() {
        for c in [[0xFF, 0xD0, 0x40], [0x8B, 0x5C, 0xF6], [0x1E, 0x8E, 0xD8]] {
            assert!(contrast([255, 255, 255], Accent::new(c).base) >= 4.5);
        }
    }

    #[test]
    fn hsl_roundtrip() {
        let c = [0x8B, 0x5C, 0xF6];
        let (h, s, l) = rgb_to_hsl(c);
        let back = hsl_to_rgb(h, s, l);
        for i in 0..3 {
            assert!((back[i] as i32 - c[i] as i32).abs() <= 1);
        }
    }

    #[test]
    fn ease_endpoints() {
        assert!(motion::ease(0.0).abs() < 1e-3);
        assert!((motion::ease(1.0) - 1.0).abs() < 1e-3);
    }
}
