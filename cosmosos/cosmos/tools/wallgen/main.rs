//! Generates the six CosmosOS ambient wallpapers: soft multi-stop
//! radial blobs on a deep base, plus a ~1.5% per-pixel noise dither so
//! smooth gradients never band. Emits `<name>` at 1920x1080 and
//! 3840x2160 plus a pastel `<name>-light` at 1920x1080 into ../wallpapers/.

use std::path::PathBuf;

/// (base rgb, blooms) — a bloom is (u, v, sigma, strength, rgb).
struct Palette {
    base: [f32; 3],
    blooms: Vec<(f32, f32, f32, f32, [f32; 3])>,
}

fn palettes() -> Vec<(&'static str, Palette)> {
    vec![
        (
            "violet",
            Palette {
                base: [0.075, 0.055, 0.115],
                blooms: vec![
                    (0.22, 0.20, 0.42, 0.95, [0.46, 0.20, 0.78]),
                    (0.82, 0.30, 0.34, 0.80, [0.72, 0.20, 0.62]),
                    (0.55, 0.85, 0.40, 0.75, [0.28, 0.16, 0.60]),
                    (0.90, 0.85, 0.30, 0.55, [0.60, 0.24, 0.52]),
                ],
            },
        ),
        (
            "ocean",
            Palette {
                base: [0.040, 0.075, 0.120],
                blooms: vec![
                    (0.25, 0.25, 0.40, 0.90, [0.06, 0.38, 0.62]),
                    (0.80, 0.35, 0.36, 0.85, [0.10, 0.55, 0.72]),
                    (0.55, 0.85, 0.42, 0.70, [0.05, 0.24, 0.48]),
                    (0.90, 0.80, 0.28, 0.50, [0.16, 0.46, 0.58]),
                ],
            },
        ),
        (
            "coral",
            Palette {
                base: [0.105, 0.065, 0.060],
                blooms: vec![
                    (0.24, 0.22, 0.40, 0.90, [0.78, 0.34, 0.22]),
                    (0.80, 0.30, 0.35, 0.85, [0.88, 0.52, 0.24]),
                    (0.52, 0.85, 0.42, 0.70, [0.52, 0.22, 0.24]),
                    (0.90, 0.82, 0.30, 0.50, [0.68, 0.30, 0.30]),
                ],
            },
        ),
        (
            "aurora",
            Palette {
                base: [0.045, 0.085, 0.090],
                blooms: vec![
                    (0.25, 0.22, 0.40, 0.90, [0.10, 0.55, 0.45]),
                    (0.78, 0.30, 0.35, 0.85, [0.14, 0.48, 0.60]),
                    (0.55, 0.85, 0.42, 0.70, [0.08, 0.35, 0.32]),
                    (0.88, 0.82, 0.28, 0.50, [0.24, 0.58, 0.44]),
                ],
            },
        ),
        (
            "peach",
            Palette {
                base: [0.110, 0.080, 0.100],
                blooms: vec![
                    (0.24, 0.24, 0.40, 0.90, [0.80, 0.48, 0.38]),
                    (0.80, 0.32, 0.35, 0.85, [0.56, 0.38, 0.66]),
                    (0.55, 0.85, 0.42, 0.70, [0.48, 0.30, 0.42]),
                    (0.90, 0.82, 0.28, 0.50, [0.62, 0.40, 0.56]),
                ],
            },
        ),
        (
            "indigo",
            Palette {
                base: [0.055, 0.060, 0.125],
                blooms: vec![
                    (0.24, 0.22, 0.42, 0.90, [0.24, 0.30, 0.68]),
                    (0.80, 0.32, 0.36, 0.85, [0.36, 0.28, 0.72]),
                    (0.55, 0.85, 0.42, 0.70, [0.16, 0.22, 0.50]),
                    (0.90, 0.82, 0.28, 0.50, [0.30, 0.34, 0.62]),
                ],
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
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };
    if light {
        Palette {
            base: lerp(p.base, [0.96, 0.95, 0.98], 0.92),
            blooms: p
                .blooms
                .iter()
                .map(|&(u, v, s, k, c)| (u, v, s * 1.1, k * 0.85, lerp(c.map(|x| (x * 1.3).min(1.0)), [1.0; 3], 0.40)))
                .collect(),
        }
    } else {
        Palette {
            base: p.base.map(|x| x * 1.15),
            blooms: p
                .blooms
                .iter()
                .map(|&(u, v, s, k, c)| (u, v, s * 1.08, (k * 1.15).min(1.0), c.map(|x| (x * 1.3).min(1.0))))
                .collect(),
        }
    }
}

fn render(name: &str, p: &Palette, w: u32, h: u32, out_dir: &PathBuf) {
    let mut img = image::RgbImage::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
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
            // ~1.5% luminance-noise dither — kills gradient banding on
            // 8-bit panels without reading as grain.
            let d = noise(x, y, w) * 0.012;
            let f = |x: f32| (x * (1.0 + d) * 255.0).clamp(0.0, 255.0) as u8;
            img.put_pixel(x, y, image::Rgb([f(c[0]), f(c[1]), f(c[2])]));
        }
    }
    let path = out_dir.join(format!("{name}-{w}x{h}.png"));
    img.save(&path).unwrap();
    println!("wrote {}", path.display());
}

fn main() {
    let out_dir = PathBuf::from(
        std::env::var("WALL_OUT").unwrap_or_else(|_| "../wallpapers".to_string()),
    );
    std::fs::create_dir_all(&out_dir).unwrap();
    for (name, p) in palettes() {
        let dark = variant(&p, false);
        render(name, &dark, 1920, 1080, &out_dir);
        render(name, &dark, 3840, 2160, &out_dir);
        // Light variants ship at 1080p only (loaders fall back to it).
        render(&format!("{name}-light"), &variant(&p, true), 1920, 1080, &out_dir);
    }
}
