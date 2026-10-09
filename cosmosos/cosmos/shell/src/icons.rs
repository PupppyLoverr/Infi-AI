//! Monochrome app glyphs for the dock and launcher — drawn as tiny-skia
//! stroke paths on a 24×24 grid, scaled to the requested size.
//!
//! Icon keys are .desktop ids ("cosmos-terminal"), app_ids ("cosmos.terminal"
//! — normalized by [`key_for`]), or synthetic keys ("start", "generic",
//! "sys.logout", ...). Unknown keys fall back to a rounded-window glyph.

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, PixmapMut, Point,
    SpreadMode, Stroke, Transform,
};

fn stroke(pixmap: &mut PixmapMut<'_>, pb: PathBuilder, s: f32, w: f32, color: Color) {
    let Some(path) = pb.finish() else {
        return;
    };
    pixmap.stroke_path(
        &path,
        &Paint {
            shader: tiny_skia::Shader::SolidColor(color),
            anti_alias: true,
            ..Default::default()
        },
        &Stroke {
            width: w * s,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        },
        crate::draw::xf(),
        None,
    );
}

fn fill(pixmap: &mut PixmapMut<'_>, pb: PathBuilder, color: Color) {
    let Some(path) = pb.finish() else {
        return;
    };
    pixmap.fill_path(
        &path,
        &Paint {
            shader: tiny_skia::Shader::SolidColor(color),
            anti_alias: true,
            ..Default::default()
        },
        FillRule::Winding,
        crate::draw::xf(),
        None,
    );
}

fn rrect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
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
}

/// Continuous-corner superellipse (|x|^5 + |y|^5 = 1) — the squircle
/// app-tile silhouette, smoother than a circular-arc rounded rect.
fn squircle(x: f32, y: f32, size: f32) -> Option<tiny_skia::Path> {
    const N: f32 = 5.0;
    const STEPS: usize = 96;
    let r = size / 2.0;
    let (cx, cy) = (x + r, y + r);
    let mut pb = PathBuilder::new();
    for i in 0..STEPS {
        let t = i as f32 / STEPS as f32 * std::f32::consts::TAU;
        let (st, ct) = t.sin_cos();
        let px = cx + r * ct.signum() * ct.abs().powf(2.0 / N);
        let py = cy + r * st.signum() * st.abs().powf(2.0 / N);
        if i == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    pb.close();
    pb.finish()
}

/// Normalize a toplevel app_id ("cosmos.files") or desktop id
/// ("cosmos-files") to the desktop-id form used as an icon key.
pub fn key_for(id: &str) -> String {
    id.replace('.', "-")
}

/// Per-app tint — the one place colour is allowed on app icons, in the
/// macOS Dock idiom (each app gets a recognisable hue). Falls back to the
/// caller's neutral glyph colour for system icons and unknown apps.
pub fn tint_for(id: &str, dark: bool) -> Option<Color> {
    let t = if dark {
        match id {
            "cosmos-terminal" => Color::from_rgba8(0xE4, 0xE4, 0xE8, 0xFF), // white glyph on dark tile
            "cosmos-files" => Color::from_rgba8(0x5B, 0x9B, 0xFF, 0xFF),    // Finder blue
            "cosmos-editor" => Color::from_rgba8(0xE8, 0xA0, 0x44, 0xFF),   // amber pencil
            "cosmos-settings" => Color::from_rgba8(0x98, 0x99, 0xA3, 0xFF), // system grey
            "cosmos-monitor" => Color::from_rgba8(0x30, 0xC1, 0x58, 0xFF),  // Activity green
            _ => return None,
        }
    } else {
        match id {
            "cosmos-terminal" => Color::from_rgba8(0x1C, 0x1C, 0x1E, 0xFF),
            "cosmos-files" => Color::from_rgba8(0x2F, 0x7D, 0xE0, 0xFF),
            "cosmos-editor" => Color::from_rgba8(0xC8, 0x73, 0x1E, 0xFF),
            "cosmos-settings" => Color::from_rgba8(0x63, 0x64, 0x6E, 0xFF),
            "cosmos-monitor" => Color::from_rgba8(0x22, 0x9A, 0x45, 0xFF),
            _ => return None,
        }
    };
    Some(t)
}

/// Full-colour app tile: a vivid per-app gradient on a squircle
/// (continuous-corner rounded square), with the app's white glyph
/// centred at ~58% scale. The dock, launcher, Start grid and
/// notifications all call this for app icons — `tint_for` stays for
/// the monochrome tray/panel glyphs.
pub fn app_tile(pixmap: &mut PixmapMut<'_>, key: &str, x: f32, y: f32, size: f32) {
    let key = key_for(key);
    // Per-app gradient stops, top → bottom.
    let (c0, c1): ((u8, u8, u8), (u8, u8, u8)) = match key.as_str() {
        "cosmos-terminal" => ((0x3A, 0x3B, 0x4E), (0x14, 0x14, 0x1E)),
        "cosmos-files" => ((0x4E, 0xA1, 0xFF), (0x0B, 0x5F, 0xD7)),
        "cosmos-editor" => ((0xFF, 0xB8, 0x4D), (0xE0, 0x73, 0x1D)),
        "cosmos-settings" => ((0xB7, 0xBB, 0xC7), (0x6E, 0x73, 0x7E)),
        "cosmos-monitor" => ((0x43, 0xDE, 0x6E), (0x12, 0x8A, 0x38)),
        "cosmos-agents" => ((0x9D, 0x7B, 0xFF), (0x5B, 0x2F, 0xD9)),
        "firefox" => ((0xFF, 0xB0, 0x3A), (0xE0, 0x44, 0x0E)),
        _ => ((0x8A, 0x8F, 0x9E), (0x44, 0x46, 0x50)),
    };
    let Some(path) = squircle(x, y, size) else {
        return;
    };
    let Some(lg) = LinearGradient::new(
        Point::from_xy(x, y),
        Point::from_xy(x, y + size),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(c0.0, c0.1, c0.2, 0xFF)),
            GradientStop::new(1.0, Color::from_rgba8(c1.0, c1.1, c1.2, 0xFF)),
        ],
        SpreadMode::Pad,
        Transform::default(),
    ) else {
        return;
    };
    pixmap.fill_path(
        &path,
        &Paint {
            shader: lg,
            anti_alias: true,
            ..Default::default()
        },
        FillRule::Winding,
        crate::draw::xf(),
        None,
    );
    // White glyph centred at ~58% — reads at 48px and 128px alike.
    let inner = size * 0.58;
    icon(
        pixmap,
        &key,
        x + (size - inner) / 2.0,
        y + (size - inner) / 2.0,
        inner,
        Color::WHITE,
    );
}

/// Draw the glyph for `key` centred inside a `size`×`size` box at (x, y).
pub fn icon(pixmap: &mut PixmapMut<'_>, key: &str, x: f32, y: f32, size: f32, color: Color) {
    let s = size / 24.0;
    let ox = x;
    let oy = y;
    let mut pb = PathBuilder::new();
    let w = 1.6; // stroke weight on the 24px grid

    // All coordinates are on the 24×24 design grid, translated by (ox,oy)
    // and scaled by `s`.
    macro_rules! mv {
        ($p:ident, $x:expr, $y:expr) => {
            $p.move_to(ox + $x * s, oy + $y * s)
        };
    }
    macro_rules! ln {
        ($p:ident, $x:expr, $y:expr) => {
            $p.line_to(ox + $x * s, oy + $y * s)
        };
    }
    macro_rules! rr {
        ($p:ident, $x:expr, $y:expr, $w:expr, $h:expr, $r:expr) => {
            rrect(&mut $p, ox + $x * s, oy + $y * s, $w * s, $h * s, $r * s)
        };
    }
    macro_rules! circle {
        ($p:ident, $x:expr, $y:expr, $r:expr) => {
            $p.push_circle(ox + $x * s, oy + $y * s, $r * s)
        };
    }

    let key = key_for(key);
    match key.as_str() {
        "start" => {
            // Cosmos mark: four rounded cells (Launchpad × Windows logo).
            for (gx, gy) in [(4.5f32, 4.5f32), (13.0, 4.5), (4.5, 13.0), (13.0, 13.0)] {
                rr!(pb, gx, gy, 6.5, 6.5, 1.6);
            }
            fill(pixmap, pb, color);
        }
        "clipboard" => {
            // Copy glyph: two overlapping rounded rects.
            rr!(pb, 9.0, 3.5, 11.5, 14.0, 2.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            rr!(pb, 3.5, 9.5, 11.5, 11.0, 2.0);
            fill(pixmap, pb, color);
            let mut pb = PathBuilder::new();
            rr!(pb, 5.5, 7.5, 11.5, 11.0, 2.0);
            stroke(pixmap, pb, s, w, color);
        }
        "search-calc" => {
            // Calculator: rounded square + 4-cell keypad.
            rr!(pb, 4.5, 3.5, 15.0, 17.0, 2.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 7.5, 7.0);
            ln!(pb, 16.5, 7.0);
            stroke(pixmap, pb, s, w, color);
            for (gx, gy) in [
                (7.5f32, 11.0f32),
                (11.0, 11.0),
                (14.5, 11.0),
                (7.5, 15.0),
                (11.0, 15.0),
                (14.5, 15.0),
            ] {
                let mut pb = PathBuilder::new();
                mv!(pb, gx, gy);
                ln!(pb, gx + 2.0, gy);
                stroke(pixmap, pb, s, w, color);
            }
        }
        "search-doc" => {
            // Document: folded-corner page + text lines.
            mv!(pb, 6.0, 3.5);
            ln!(pb, 13.5, 3.5);
            ln!(pb, 18.0, 8.0);
            ln!(pb, 18.0, 20.5);
            ln!(pb, 6.0, 20.5);
            pb.close();
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 13.5, 3.5);
            ln!(pb, 13.5, 8.0);
            ln!(pb, 18.0, 8.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 8.5, 12.5);
            ln!(pb, 15.5, 12.5);
            mv!(pb, 8.5, 15.5);
            ln!(pb, 15.5, 15.5);
            stroke(pixmap, pb, s, w, color);
        }
        "search-ask" => {
            // Ask: chat bubble with a sparkle dot — "ask Cosmos".
            mv!(pb, 4.0, 5.0);
            rr!(pb, 4.0, 4.0, 16.0, 11.0, 3.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 8.0, 15.0);
            ln!(pb, 8.0, 19.5);
            ln!(pb, 12.0, 15.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            circle!(pb, 12.0, 9.5, 1.6);
            fill(pixmap, pb, color);
        }
        "widget-clock" => {
            // Analog clock: face + hands.
            circle!(pb, 12.0, 12.0, 8.5);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 12.0, 7.5);
            ln!(pb, 12.0, 12.0);
            ln!(pb, 15.5, 14.0);
            stroke(pixmap, pb, s, w, color);
        }
        "widget-cpu" => {
            // Chip: square + inner die + 4 pin stubs.
            rr!(pb, 6.0, 6.0, 12.0, 12.0, 1.5);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            rr!(pb, 9.5, 9.5, 5.0, 5.0, 1.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            for x in [9.0f32, 15.0] {
                mv!(pb, x, 3.5);
                ln!(pb, x, 6.0);
                mv!(pb, x, 18.0);
                ln!(pb, x, 20.5);
            }
            for y in [9.0f32, 15.0] {
                mv!(pb, 3.5, y);
                ln!(pb, 6.0, y);
                mv!(pb, 18.0, y);
                ln!(pb, 20.5, y);
            }
            stroke(pixmap, pb, s, w, color);
        }
        "widget-disk" => {
            // Drive: rounded slab + activity dot.
            rr!(pb, 3.0, 7.0, 18.0, 10.0, 2.5);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            circle!(pb, 16.5, 12.0, 1.4);
            fill(pixmap, pb, color);
        }
        "cosmos-terminal" => {
            rr!(pb, 3.5, 5.0, 17.0, 14.0, 2.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 7.0, 9.5);
            ln!(pb, 10.0, 12.0);
            ln!(pb, 7.0, 14.5);
            mv!(pb, 12.5, 15.0);
            ln!(pb, 16.5, 15.0);
            stroke(pixmap, pb, s, w, color);
        }
        "cosmos-files" => {
            mv!(pb, 4.0, 7.0);
            ln!(pb, 8.5, 7.0);
            ln!(pb, 10.5, 9.5);
            ln!(pb, 20.0, 9.5);
            ln!(pb, 20.0, 18.5);
            ln!(pb, 4.0, 18.5);
            pb.close();
            stroke(pixmap, pb, s, w, color);
        }
        "cosmos-editor" => {
            mv!(pb, 7.0, 3.5);
            ln!(pb, 14.5, 3.5);
            ln!(pb, 17.5, 6.5);
            ln!(pb, 17.5, 20.5);
            ln!(pb, 7.0, 20.5);
            pb.close();
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 14.5, 3.5);
            ln!(pb, 14.5, 6.5);
            ln!(pb, 17.5, 6.5);
            mv!(pb, 9.5, 11.0);
            ln!(pb, 15.0, 11.0);
            mv!(pb, 9.5, 14.0);
            ln!(pb, 15.0, 14.0);
            mv!(pb, 9.5, 17.0);
            ln!(pb, 13.0, 17.0);
            stroke(pixmap, pb, s, w, color);
        }
        "cosmos-settings" => {
            circle!(pb, 12.0, 12.0, 3.4);
            for i in 0..6 {
                let a = i as f32 * std::f32::consts::PI / 3.0;
                let (cx, cy) = (a.cos(), a.sin());
                mv!(pb, 12.0 + cx * 6.2, 12.0 + cy * 6.2);
                ln!(pb, 12.0 + cx * 8.6, 12.0 + cy * 8.6);
            }
            stroke(pixmap, pb, s, w, color);
        }
        "cosmos-monitor" => {
            rr!(pb, 3.5, 5.0, 17.0, 13.0, 2.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 6.0, 11.5);
            ln!(pb, 9.0, 11.5);
            ln!(pb, 11.0, 8.0);
            ln!(pb, 13.0, 15.0);
            ln!(pb, 15.0, 11.5);
            ln!(pb, 18.0, 11.5);
            stroke(pixmap, pb, s, w, color);
        }
        "net-on" => {
            // Wi-Fi fan: three arcs opening upward + a base dot.
            for r in [10.0f32, 7.0, 4.0] {
                let mut pb = PathBuilder::new();
                let e = r * std::f32::consts::FRAC_1_SQRT_2;
                mv!(pb, 12.0 - e, 17.5 - e);
                pb.quad_to(
                    ox + 12.0 * s,
                    oy + (17.5 - 1.2929 * r) * s,
                    ox + (12.0 + e) * s,
                    oy + (17.5 - e) * s,
                );
                stroke(pixmap, pb, s, w * 0.9, color);
            }
            let mut pb = PathBuilder::new();
            circle!(pb, 12.0, 17.5, 1.5);
            fill(pixmap, pb, color);
        }
        "net-off" => {
            // Offline: base dot + a strike-through.
            let mut pb = PathBuilder::new();
            circle!(pb, 12.0, 17.5, 1.5);
            fill(pixmap, pb, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 5.5, 7.0);
            ln!(pb, 18.5, 20.0);
            stroke(pixmap, pb, s, w * 0.9, color);
        }
        "vol-on" => {
            // Speaker wedge + two sound arcs.
            mv!(pb, 4.5, 9.5);
            ln!(pb, 8.0, 9.5);
            ln!(pb, 12.5, 5.0);
            ln!(pb, 12.5, 19.0);
            ln!(pb, 8.0, 14.5);
            ln!(pb, 4.5, 14.5);
            pb.close();
            fill(pixmap, pb, color);
            for r in [3.0f32, 5.6] {
                let mut pb = PathBuilder::new();
                let e = r * std::f32::consts::FRAC_1_SQRT_2;
                mv!(pb, 14.0 + e, 12.0 - e);
                pb.quad_to(
                    ox + (14.0 + 1.2929 * r) * s,
                    oy + 12.0 * s,
                    ox + (14.0 + e) * s,
                    oy + (12.0 + e) * s,
                );
                stroke(pixmap, pb, s, w * 0.85, color);
            }
        }
        "vol-mute" => {
            // Speaker wedge + strike X.
            mv!(pb, 4.5, 9.5);
            ln!(pb, 8.0, 9.5);
            ln!(pb, 12.5, 5.0);
            ln!(pb, 12.5, 19.0);
            ln!(pb, 8.0, 14.5);
            ln!(pb, 4.5, 14.5);
            pb.close();
            fill(pixmap, pb, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 15.0, 9.5);
            ln!(pb, 20.0, 14.5);
            mv!(pb, 20.0, 9.5);
            ln!(pb, 15.0, 14.5);
            stroke(pixmap, pb, s, w * 0.85, color);
        }
        "user" => {
            rr!(pb, 8.5, 4.0, 7.0, 7.0, 3.5);
            mv!(pb, 5.0, 20.0);
            pb.cubic_to(
                ox + 5.0 * s,
                oy + 14.0 * s,
                ox + 19.0 * s,
                oy + 14.0 * s,
                ox + 19.0 * s,
                oy + 20.0 * s,
            );
            stroke(pixmap, pb, s, w, color);
        }
        "bluetooth" => {
            mv!(pb, 7.0, 7.5);
            ln!(pb, 17.0, 16.5);
            ln!(pb, 12.0, 20.5);
            ln!(pb, 12.0, 3.5);
            ln!(pb, 17.0, 7.5);
            ln!(pb, 7.0, 16.5);
            stroke(pixmap, pb, s, w, color);
        }
        "focus" => {
            // Crescent moon.
            mv!(pb, 15.0, 4.6);
            pb.quad_to(ox + 2.5 * s, oy + 6.0 * s, ox + 6.5 * s, oy + 17.5 * s);
            pb.quad_to(ox + 12.5 * s, oy + 22.5 * s, ox + 19.4 * s, oy + 15.0 * s);
            pb.quad_to(ox + 11.0 * s, oy + 15.5 * s, ox + 15.0 * s, oy + 4.6 * s);
            pb.close();
            fill(pixmap, pb, color);
        }
        "appearance" => {
            // Half-filled disc.
            circle!(pb, 12.0, 12.0, 8.0);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            let k = 8.0 * 0.5523;
            mv!(pb, 12.0, 4.0);
            pb.cubic_to(
                ox + (12.0 - k) * s,
                oy + 4.0 * s,
                ox + 4.0 * s,
                oy + (12.0 - k) * s,
                ox + 4.0 * s,
                oy + 12.0 * s,
            );
            pb.cubic_to(
                ox + 4.0 * s,
                oy + (12.0 + k) * s,
                ox + (12.0 - k) * s,
                oy + 20.0 * s,
                ox + 12.0 * s,
                oy + 20.0 * s,
            );
            pb.close();
            fill(pixmap, pb, color);
        }
        "lite" => {
            // Bolt.
            mv!(pb, 13.5, 3.0);
            ln!(pb, 5.5, 13.5);
            ln!(pb, 11.0, 13.5);
            ln!(pb, 10.0, 21.0);
            ln!(pb, 18.5, 10.0);
            ln!(pb, 13.0, 10.0);
            pb.close();
            fill(pixmap, pb, color);
        }
        "sun" => {
            circle!(pb, 12.0, 12.0, 3.8);
            fill(pixmap, pb, color);
            let mut pb = PathBuilder::new();
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                mv!(pb, 12.0 + a.cos() * 6.5, 12.0 + a.sin() * 6.5);
                ln!(pb, 12.0 + a.cos() * 9.0, 12.0 + a.sin() * 9.0);
            }
            stroke(pixmap, pb, s, w, color);
        }
        "play" => {
            mv!(pb, 8.0, 5.0);
            ln!(pb, 19.0, 12.0);
            ln!(pb, 8.0, 19.0);
            pb.close();
            fill(pixmap, pb, color);
        }
        "pause" => {
            rr!(pb, 6.5, 5.0, 3.8, 14.0, 1.0);
            rr!(pb, 13.7, 5.0, 3.8, 14.0, 1.0);
            fill(pixmap, pb, color);
        }
        "sys-lock" => {
            rr!(pb, 6.0, 10.5, 12.0, 9.0, 2.0);
            mv!(pb, 8.5, 10.5);
            pb.cubic_to(
                ox + 8.5 * s,
                oy + 6.5 * s,
                ox + 15.5 * s,
                oy + 6.5 * s,
                ox + 15.5 * s,
                oy + 10.5 * s,
            );
            stroke(pixmap, pb, s, w, color);
        }
        "sys-logout" => {
            mv!(pb, 10.5, 4.5);
            ln!(pb, 5.5, 4.5);
            ln!(pb, 5.5, 19.5);
            ln!(pb, 10.5, 19.5);
            mv!(pb, 11.5, 12.0);
            ln!(pb, 19.5, 12.0);
            mv!(pb, 16.5, 9.0);
            ln!(pb, 19.5, 12.0);
            ln!(pb, 16.5, 15.0);
            stroke(pixmap, pb, s, w, color);
        }
        "sys-reboot" => {
            // Circular arrow: 270° arc + arrowhead.
            let r = 7.0f32;
            let cx = 12.0f32;
            let cy = 12.5f32;
            pb.move_to(ox + (cx + r) * s, oy + cy * s);
            // quarter arcs via cubic
            let k = 0.552_284_8 * r * s;
            let rx = r * s;
            let cy_ = oy + cy * s;
            let cx_ = ox + cx * s;
            pb.cubic_to(cx_ + rx, cy_ - k, cx_ + k, cy_ - rx, cx_, cy_ - rx);
            pb.cubic_to(cx_ - k, cy_ - rx, cx_ - rx, cy_ - k, cx_ - rx, cy_);
            pb.cubic_to(cx_ - rx, cy_ + k, cx_ - k, cy_ + rx, cx_, cy_ + rx);
            pb.cubic_to(cx_ + k, cy_ + rx, cx_ + rx, cy_ + k, cx_ + rx, cy_);
            stroke(pixmap, pb, s, w, color);
            let mut pb = PathBuilder::new();
            mv!(pb, 19.0, 5.5);
            ln!(pb, 19.0, 9.5);
            ln!(pb, 15.5, 9.5);
            stroke(pixmap, pb, s, w, color);
        }
        "sys-shutdown" | "power" => {
            mv!(pb, 12.0, 4.0);
            ln!(pb, 12.0, 11.0);
            mv!(pb, 8.0, 7.5);
            pb.cubic_to(
                ox + 5.5 * s,
                oy + 10.0 * s,
                ox + 5.5 * s,
                oy + 15.5 * s,
                ox + 12.0 * s,
                oy + 19.5 * s,
            );
            pb.cubic_to(
                ox + 18.5 * s,
                oy + 15.5 * s,
                ox + 18.5 * s,
                oy + 10.0 * s,
                ox + 16.0 * s,
                oy + 7.5 * s,
            );
            stroke(pixmap, pb, s, w, color);
        }
        "cosmos-agents" => {
            // Bot head: rounded face, two eyes, antenna stub.
            rr!(pb, 5.0, 6.0, 14.0, 12.0, 3.0);
            circle!(pb, 9.6, 11.0, 1.15);
            circle!(pb, 14.4, 11.0, 1.15);
            mv!(pb, 12.0, 6.0);
            ln!(pb, 12.0, 4.0);
            circle!(pb, 12.0, 3.3, 0.9);
            stroke(pixmap, pb, s, w, color);
        }
        "firefox" => {
            // Stylized fox-wrap: outer ring + inner swoosh arc. Original
            // glyph — not the Mozilla asset.
            circle!(pb, 12.0, 12.0, 7.5);
            mv!(pb, 6.5, 13.0);
            pb.cubic_to(
                ox + 8.0 * s,
                oy + 9.0 * s,
                ox + 12.5 * s,
                oy + 6.8 * s,
                ox + 17.5 * s,
                oy + 8.5 * s,
            );
            stroke(pixmap, pb, s, w, color);
        }
        _ => {
            // Generic app: rounded window with a title rule.
            rr!(pb, 4.0, 5.0, 16.0, 14.0, 2.0);
            mv!(pb, 4.0, 9.0);
            ln!(pb, 20.0, 9.0);
            stroke(pixmap, pb, s, w, color);
        }
    }
}

/// Battery tray glyph with a level fill — `pct` 0..=100. Kept out of
/// `icon()` because it takes a live charge level.
pub fn battery(pixmap: &mut PixmapMut<'_>, x: f32, y: f32, size: f32, color: Color, pct: u8) {
    let s = size / 24.0;
    let (ox, oy) = (x, y);
    let mut pb = PathBuilder::new();
    rrect(
        &mut pb,
        ox + 2.5 * s,
        oy + 8.0 * s,
        17.0 * s,
        9.0 * s,
        2.2 * s,
    );
    // Terminal nub.
    rrect(
        &mut pb,
        ox + 20.5 * s,
        oy + 10.5 * s,
        2.4 * s,
        4.0 * s,
        0.8 * s,
    );
    stroke(pixmap, pb, s, 1.5, color);
    let fill_w = (15.2 * pct.min(100) as f32 / 100.0).max(0.0);
    if fill_w > 0.5 {
        let mut pb = PathBuilder::new();
        rrect(
            &mut pb,
            ox + 3.6 * s,
            oy + 9.1 * s,
            fill_w * s,
            6.8 * s,
            1.3 * s,
        );
        fill(pixmap, pb, color);
    }
}
