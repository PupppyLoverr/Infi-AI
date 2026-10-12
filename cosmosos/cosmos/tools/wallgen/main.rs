//! Generates the six CosmosOS ambient wallpapers: layered soft radial
//! blobs on a deep base, a screen-blended light glow, a corner vignette
//! for depth and ~2.5% luminance grain. Emits `<name>` at 1920x1080 and
//! 3840x2160 plus a pastel `<name>-light` at 1920x1080 into ../wallpapers/.

use std::path::{Path, PathBuf};

/// (base rgb, blooms, glow) — a bloom is (u, v, sigma, strength, rgb);
/// the glow is one soft additive light source (u, v, sigma, rgb).
struct Palette {
    base: [f32; 3],
    blooms: Vec<(f32, f32, f32, f32, [f32; 3])>,
    glow: (f32, f32, f32, [f32; 3]),
}

/// The six v6 wallpapers. Six layered blooms each (two broad depth
/// washes, three mid blobs, one small accent), plus a light glow.
fn palettes() -> Vec<(&'static str, Palette)> {
    vec![
        (
            // Aurora violet: the default.
            "violet",
            Palette {
                base: [0.065, 0.045, 0.110],
                blooms: vec![
                    (0.20, 0.18, 0.46, 0.92, [0.42, 0.18, 0.76]),
                    (0.84, 0.26, 0.36, 0.84, [0.74, 0.22, 0.64]),
                    (0.55, 0.92, 0.44, 0.74, [0.24, 0.14, 0.60]),
                    (0.94, 0.88, 0.24, 0.56, [0.62, 0.26, 0.54]),
                    (0.42, 0.48, 0.17, 0.46, [0.58, 0.36, 0.92]),
                    (0.06, 0.82, 0.20, 0.40, [0.20, 0.28, 0.70]),
                ],
                glow: (0.64, 0.30, 0.22, [0.95, 0.80, 1.00]),
            },
        ),
        (
            "ocean",
            Palette {
                base: [0.025, 0.060, 0.110],
                blooms: vec![
                    (0.22, 0.24, 0.46, 0.90, [0.05, 0.34, 0.60]),
                    (0.82, 0.32, 0.38, 0.86, [0.08, 0.54, 0.72]),
                    (0.55, 0.92, 0.44, 0.72, [0.04, 0.22, 0.46]),
                    (0.92, 0.84, 0.26, 0.52, [0.14, 0.46, 0.60]),
                    (0.40, 0.52, 0.16, 0.44, [0.18, 0.66, 0.80]),
                    (0.06, 0.78, 0.20, 0.42, [0.06, 0.30, 0.52]),
                ],
                glow: (0.70, 0.26, 0.22, [0.80, 0.96, 1.00]),
            },
        ),
        (
            "sunset",
            Palette {
                base: [0.100, 0.045, 0.065],
                blooms: vec![
                    (0.22, 0.20, 0.46, 0.90, [0.86, 0.40, 0.20]),
                    (0.82, 0.30, 0.38, 0.86, [0.70, 0.20, 0.44]),
                    (0.55, 0.92, 0.44, 0.72, [0.34, 0.12, 0.38]),
                    (0.92, 0.86, 0.26, 0.56, [0.84, 0.34, 0.38]),
                    (0.40, 0.46, 0.16, 0.48, [0.96, 0.66, 0.30]),
                    (0.06, 0.80, 0.20, 0.40, [0.52, 0.18, 0.40]),
                ],
                glow: (0.38, 0.40, 0.22, [1.00, 0.88, 0.66]),
            },
        ),
        (
            "peach",
            Palette {
                base: [0.105, 0.075, 0.095],
                blooms: vec![
                    (0.22, 0.22, 0.46, 0.90, [0.82, 0.50, 0.38]),
                    (0.82, 0.30, 0.38, 0.86, [0.58, 0.38, 0.68]),
                    (0.55, 0.92, 0.44, 0.72, [0.48, 0.30, 0.42]),
                    (0.92, 0.86, 0.26, 0.54, [0.64, 0.40, 0.56]),
                    (0.42, 0.50, 0.16, 0.46, [0.94, 0.66, 0.52]),
                    (0.06, 0.80, 0.20, 0.40, [0.40, 0.30, 0.56]),
                ],
                glow: (0.34, 0.34, 0.22, [1.00, 0.90, 0.82]),
            },
        ),
        (
            "forest",
            Palette {
                base: [0.025, 0.065, 0.050],
                blooms: vec![
                    (0.22, 0.22, 0.46, 0.90, [0.10, 0.44, 0.28]),
                    (0.82, 0.30, 0.38, 0.86, [0.08, 0.40, 0.42]),
                    (0.55, 0.92, 0.44, 0.72, [0.05, 0.24, 0.16]),
                    (0.92, 0.86, 0.26, 0.54, [0.34, 0.50, 0.20]),
                    (0.40, 0.50, 0.16, 0.46, [0.28, 0.62, 0.46]),
                    (0.06, 0.80, 0.20, 0.40, [0.12, 0.34, 0.30]),
                ],
                glow: (0.66, 0.30, 0.22, [0.86, 1.00, 0.88]),
            },
        ),
        (
            "graphite",
            Palette {
                base: [0.040, 0.040, 0.048],
                blooms: vec![
                    (0.22, 0.22, 0.46, 0.80, [0.20, 0.21, 0.24]),
                    (0.82, 0.30, 0.38, 0.76, [0.27, 0.27, 0.30]),
                    (0.55, 0.92, 0.44, 0.66, [0.13, 0.13, 0.16]),
                    (0.92, 0.86, 0.26, 0.50, [0.24, 0.25, 0.28]),
                    (0.40, 0.50, 0.16, 0.40, [0.33, 0.34, 0.38]),
                    (0.06, 0.80, 0.20, 0.36, [0.16, 0.17, 0.20]),
                ],
                glow: (0.62, 0.30, 0.22, [0.88, 0.90, 1.00]),
            },
        ),
    ]
}

/// Deterministic per-pixel noise in [-1, 1] (no crates — a tiny hash).
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263) ^ seed.wrapping_mul(69069);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    let v = (h ^ (h >> 16)) & 0xFFFF;
    (v as f32 / 32767.5) - 1.0
}

/// Luminous dark variant: blooms lifted ×1.3 and pushed to full
/// strength. Light variant: pastel base, blooms pulled 40% toward white.
fn variant(p: &Palette, light: bool) -> Palette {
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    if light {
        Palette {
            base: lerp(p.base, [0.96, 0.95, 0.98], 0.92),
            glow: (p.glow.0, p.glow.1, p.glow.2, lerp(p.glow.3, [1.0; 3], 0.5)),
            blooms: p
                .blooms
                .iter()
                .map(|&(u, v, s, k, c)| {
                    (
                        u,
                        v,
                        s * 1.1,
                        k * 0.85,
                        lerp(c.map(|x| (x * 1.3).min(1.0)), [1.0; 3], 0.40),
                    )
                })
                .collect(),
        }
    } else {
        Palette {
            base: p.base.map(|x| x * 1.15),
            glow: p.glow,
            blooms: p
                .blooms
                .iter()
                .map(|&(u, v, s, k, c)| {
                    (
                        u,
                        v,
                        s * 1.08,
                        (k * 1.15).min(1.0),
                        c.map(|x| (x * 1.3).min(1.0)),
                    )
                })
                .collect(),
        }
    }
}

fn render(name: &str, p: &Palette, w: u32, h: u32, out_dir: &Path) {
    let mut img = image::RgbImage::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    // A dark-corner vignette reads as a grey ring on pastel light variants.
    let vig_k = if p.base.iter().sum::<f32>() > 1.5 {
        0.04
    } else {
        0.22
    };
    for y in 0..h {
        let v = (y as f32 + 0.5) / hf;
        for x in 0..w {
            let u = (x as f32 + 0.5) / wf;
            // Aspect-corrected bloom distance so circles stay circles.
            let mut c = p.base;
            for &(bx, by, sigma, strength, bc) in &p.blooms {
                let dx = (u - bx) * (wf / hf);
                let dy = v - by;
                let g = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp() * strength;
                for ch in 0..3 {
                    c[ch] += (bc[ch] - c[ch]) * g.min(1.0);
                }
            }
            // Light glow: screen-blended so it lifts without clipping.
            let (gx, gy, gs, gc) = p.glow;
            let (dx, dy) = ((u - gx) * (wf / hf), v - gy);
            let g = (-(dx * dx + dy * dy) / (2.0 * gs * gs)).exp() * 0.26;
            for ch in 0..3 {
                c[ch] = 1.0 - (1.0 - c[ch]) * (1.0 - gc[ch] * g);
            }
            // Depth: a soft vignette toward the corners.
            let (cx, cy) = ((u - 0.5) * (wf / hf), v - 0.5);
            let vig = 1.0 - vig_k * ((cx * cx + cy * cy) / 0.6).min(1.0);
            let c = c.map(|x| x * vig);
            // ~2.5% luminance grain — visible up close as film grain, and it
            // kills gradient banding on 8-bit panels.
            let d = noise(x, y, w) * 0.025;
            let f = |x: f32| (x * (1.0 + d) * 255.0).clamp(0.0, 255.0) as u8;
            img.put_pixel(x, y, image::Rgb([f(c[0]), f(c[1]), f(c[2])]));
        }
    }
    let path = out_dir.join(format!("{name}-{w}x{h}.png"));
    img.save(&path).unwrap();
    println!("wrote {}", path.display());
}

fn main() {
    let out_dir =
        PathBuf::from(std::env::var("WALL_OUT").unwrap_or_else(|_| "../wallpapers".to_string()));
    std::fs::create_dir_all(&out_dir).unwrap();
    for (name, p) in palettes() {
        let dark = variant(&p, false);
        render(name, &dark, 1920, 1080, &out_dir);
        render(name, &dark, 3840, 2160, &out_dir);
        // Light variants ship at 1080p only (loaders fall back to it).
        render(
            &format!("{name}-light"),
            &variant(&p, true),
            1920,
            1080,
            &out_dir,
        );
    }
}
