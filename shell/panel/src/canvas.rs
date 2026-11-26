//! 2D software vector, typography, and icon renderer for Wayland `Argb8888`.
//!
//! On little-endian targets, `wl_shm::Format::Argb8888` stores each pixel as
//! `[B, G, R, A]` with **premultiplied** color channels. Keeping premultiplied
//! Porter-Duff `source-over` exact avoids dark fringes on anti-aliased edges
//! and lets compositor backdrop blur (`niri` `background-effect { blur true }`)
//! shine cleanly through translucent glass fills.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};

pub use solyra_tokens::Rgba;

pub struct Fonts {
    pub regular: Option<FontRef<'static>>,
    pub medium: Option<FontRef<'static>>,
    pub semibold: Option<FontRef<'static>>,
    pub bold: Option<FontRef<'static>>,
    pub mono: Option<FontRef<'static>>,
}

impl Fonts {
    pub fn load() -> Self {
        Self {
            // Prefer Adwaita Sans (design language), then Inter (SF Pro-like).
            regular: try_load_font(&[
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/truetype/adwaita/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/truetype/macos/Inter-Regular.ttf",
                "/usr/share/fonts/rsms-inter-fonts/Inter-Regular.otf",
                "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
                "/usr/share/fonts/open-sans/OpenSans-Regular.ttf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]),
            medium: try_load_font(&[
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Medium.ttf",
                "/usr/share/fonts/truetype/adwaita/AdwaitaSans-Medium.ttf",
                "/usr/share/fonts/truetype/macos/Inter-Medium.ttf",
                "/usr/share/fonts/rsms-inter-fonts/Inter-Medium.otf",
                "/usr/share/fonts/opentype/inter/Inter-Medium.otf",
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/truetype/macos/Inter-Regular.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]),
            semibold: try_load_font(&[
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-SemiBold.ttf",
                "/usr/share/fonts/truetype/adwaita/AdwaitaSans-SemiBold.ttf",
                "/usr/share/fonts/truetype/macos/Inter-SemiBold.ttf",
                "/usr/share/fonts/rsms-inter-fonts/Inter-SemiBold.otf",
                "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/open-sans/OpenSans-Semibold.ttf",
                "/usr/share/fonts/open-sans/OpenSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ]),
            bold: try_load_font(&[
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Bold.ttf",
                "/usr/share/fonts/truetype/adwaita/AdwaitaSans-Bold.ttf",
                "/usr/share/fonts/truetype/macos/Inter-Bold.ttf",
                "/usr/share/fonts/rsms-inter-fonts/Inter-Bold.otf",
                "/usr/share/fonts/opentype/inter/Inter-Bold.otf",
                "/usr/share/fonts/truetype/macos/Inter-SemiBold.ttf",
                "/usr/share/fonts/open-sans/OpenSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ]),
            mono: try_load_font(&[
                "/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf",
                "/usr/share/fonts/truetype/macos/JetBrainsMono-Regular.ttf",
                "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
                "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            ]),
        }
    }
}

fn try_load_font(candidates: &[&str]) -> Option<FontRef<'static>> {
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = FontRef::try_from_slice(Box::leak(bytes.into_boxed_slice())) {
                return Some(font);
            }
        }
    }
    None
}

pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // straight RGBA
}

pub fn decode_png(bytes: &[u8]) -> Option<Icon> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in buf.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        _ => return None,
    };
    Some(Icon {
        width: info.width,
        height: info.height,
        pixels: rgba,
    })
}

#[inline]
pub fn lerp_rgba(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba::rgba(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2), m(a.3, b.3))
}

#[inline]
fn add_light(c: Rgba, boost: f32) -> Rgba {
    let b = boost.clamp(0.0, 1.0);
    let lift = |ch: u8| {
        (ch as f32 + (255.0 - ch as f32) * b)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba::rgba(lift(c.0), lift(c.1), lift(c.2), c.3)
}

/// Signed distance to a rounded rectangle centred at origin with half-size
/// `(hw, hh)` and corner radius `r`, plus approximate outward unit normal.
#[inline]
fn round_rect_sdf_normal(dx: f32, dy: f32, hw: f32, hh: f32, r: f32) -> (f32, f32, f32) {
    let sx = if dx < 0.0 { -1.0 } else { 1.0 };
    let sy = if dy < 0.0 { -1.0 } else { 1.0 };
    let qx = dx.abs() - (hw - r);
    let qy = dy.abs() - (hh - r);
    let mx = qx.max(0.0);
    let my = qy.max(0.0);
    let out_len = mx.hypot(my);
    let sdf = out_len + qx.max(qy).min(0.0) - r;
    let (nx, ny) = if out_len > 1e-4 {
        (sx * mx / out_len, sy * my / out_len)
    } else if qx > qy {
        (sx, 0.0)
    } else {
        (0.0, sy)
    };
    (sdf, nx, ny)
}

/// Continuous-curvature Apple superellipse (squircle) signed distance &
/// outward unit normal for half-size `(hw, hh)` and Lamé exponent `n ≈ 4.6`.
#[inline]
fn squircle_sdf_normal(dx: f32, dy: f32, hw: f32, hh: f32, n: f32) -> (f32, f32, f32) {
    let ax = (dx.abs() / hw.max(1e-3)).max(1e-6);
    let ay = (dy.abs() / hh.max(1e-3)).max(1e-6);
    let ax_n = ax.powf(n);
    let ay_n = ay.powf(n);
    let sum = ax_n + ay_n;
    let p = sum.powf(1.0 / n);
    let g = p - 1.0;
    let factor = sum.powf(1.0 / n - 1.0);
    let gx = factor * ax.powf(n - 1.0) / hw.max(1e-3);
    let gy = factor * ay.powf(n - 1.0) / hh.max(1e-3);
    let grad = gx.hypot(gy).max(1e-5);
    let sdf = g / grad;
    let sx = if dx < 0.0 { -1.0 } else { 1.0 };
    let sy = if dy < 0.0 { -1.0 } else { 1.0 };
    (sdf, sx * gx / grad, sy * gy / grad)
}

/// Exact signed distance to a triangle with vertices `p0`, `p1`, `p2`.
#[inline]
fn triangle_sdf(px: f32, py: f32, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) -> f32 {
    let e0 = (p1.0 - p0.0, p1.1 - p0.1);
    let e1 = (p2.0 - p1.0, p2.1 - p1.1);
    let e2 = (p0.0 - p2.0, p0.1 - p2.1);

    let v0 = (px - p0.0, py - p0.1);
    let v1 = (px - p1.0, py - p1.1);
    let v2 = (px - p2.0, py - p2.1);

    let dot = |a: (f32, f32), b: (f32, f32)| a.0 * b.0 + a.1 * b.1;
    let cross = |a: (f32, f32), b: (f32, f32)| a.0 * b.1 - a.1 * b.0;

    let proj = |v: (f32, f32), e: (f32, f32)| {
        let t = (dot(v, e) / dot(e, e).max(1e-6)).clamp(0.0, 1.0);
        let pq = (v.0 - e.0 * t, v.1 - e.1 * t);
        dot(pq, pq)
    };
    let d_sq = proj(v0, e0).min(proj(v1, e1)).min(proj(v2, e2));

    let s = cross(e0, e2).signum();
    let c0 = s * cross(v0, e0);
    let c1 = s * cross(v1, e1);
    let c2 = s * cross(v2, e2);
    let inside = c0.min(c1).min(c2) >= 0.0;
    let dist = d_sq.sqrt();
    if inside {
        -dist
    } else {
        dist
    }
}

/// A drawing surface. `width` and `height` are *logical* pixels: painters lay
/// everything out in them. The buffer is `scale` times larger in each
/// direction, and every primitive multiplies its coordinates, sizes and font
/// size by `scale`, so shapes and text are redrawn crisply at the real
/// resolution rather than stretched.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub scale: i32,
    /// Physical size and stride of the buffer.
    pw: u32,
    ph: u32,
    pub stride: usize,
}

impl<'a> Canvas<'a> {
    /// Like [`Canvas::new`] but keeps what is already in `pixels` (a
    /// pre-rendered backdrop to draw over).
    pub fn new_keep(pixels: &'a mut [u8], width: u32, height: u32, scale: i32) -> Self {
        let scale = scale.clamp(1, 4);
        let (pw, ph) = (width * scale as u32, height * scale as u32);
        assert!(
            pixels.len() >= (pw * ph * 4) as usize,
            "buffer too small for {pw}x{ph}"
        );
        Self {
            pixels,
            width,
            height,
            scale,
            pw,
            ph,
            stride: pw as usize * 4,
        }
    }

    /// `pixels` must hold `width*scale` by `height*scale` BGRA pixels.
    pub fn new(pixels: &'a mut [u8], width: u32, height: u32, scale: i32) -> Self {
        let scale = scale.clamp(1, 4);
        let (pw, ph) = (width * scale as u32, height * scale as u32);
        assert!(
            pixels.len() >= (pw * ph * 4) as usize,
            "buffer too small for {pw}x{ph}"
        );
        pixels.fill(0);
        Self {
            pixels,
            width,
            height,
            scale,
            pw,
            ph,
            stride: pw as usize * 4,
        }
    }

    // ---- scaled primitives: logical coordinates in, physical pixels out ----

    fn s(&self) -> i32 {
        self.scale
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgba) {
        let s = self.s();
        self.fill_rect_raw(x * s, y * s, w * s, h * s, color);
    }

    pub fn fill_rect_vgrad(&mut self, x: i32, y: i32, w: i32, h: i32, top: Rgba, bottom: Rgba) {
        let s = self.s();
        self.fill_rect_vgrad_raw(x * s, y * s, w * s, h * s, top, bottom);
    }

    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: f32, color: Rgba) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.round_rect_raw(x * s, y * s, w * s, h * s, radius * sf, color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_vgrad(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        top: Rgba,
        bottom: Rgba,
    ) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.round_rect_vgrad_raw(x * s, y * s, w * s, h * s, radius * sf, top, bottom);
    }

    /// Soft analytical drop shadow under a rounded rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_shadow(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        dy: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.round_rect_shadow_raw(
            x * s,
            y * s,
            w * s,
            h * s,
            radius * sf,
            dy * sf,
            spread * sf,
            max_alpha,
        );
    }

    /// Optical directional glass rim (bright top catch-light wrapping smoothly
    /// around corners, subtle side refraction, faint bottom bounce light).
    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_glass_rim(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        top: Rgba,
        side: Rgba,
        bottom: Rgba,
    ) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.round_rect_glass_rim_raw(
            x * s,
            y * s,
            w * s,
            h * s,
            radius * sf,
            1.1 * sf,
            top,
            side,
            bottom,
        );
    }

    /// Continuous-curvature Apple superellipse squircle with vertical gradient
    /// and soft top-left luminosity sheen.
    #[allow(clippy::too_many_arguments)]
    pub fn squircle_vgrad(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        top: Rgba,
        bottom: Rgba,
        sheen: f32,
    ) {
        let s = self.s();
        self.squircle_vgrad_raw(x * s, y * s, size * s, top, bottom, sheen);
    }

    pub fn squircle_shadow(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        dy: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.squircle_shadow_raw(x * s, y * s, size * s, dy * sf, spread * sf, max_alpha);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn squircle_glass_rim(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        top: Rgba,
        side: Rgba,
        bottom: Rgba,
    ) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.squircle_glass_rim_raw(x * s, y * s, size * s, 1.0 * sf, top, side, bottom);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_round_rect(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        thickness: f32,
        color: Rgba,
    ) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.stroke_round_rect_raw(
            x * s,
            y * s,
            w * s,
            h * s,
            radius * sf,
            thickness * sf,
            color,
        );
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: Rgba) {
        let sf = self.s() as f32;
        self.circle_raw(cx * sf, cy * sf, radius * sf, color);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, radius: f32, thickness: f32, color: Rgba) {
        let sf = self.s() as f32;
        self.ring_raw(cx * sf, cy * sf, radius * sf, thickness * sf, color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn arc(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        thickness: f32,
        start_deg: f32,
        end_deg: f32,
        color: Rgba,
    ) {
        let sf = self.s() as f32;
        self.arc_raw(
            cx * sf,
            cy * sf,
            radius * sf,
            thickness * sf,
            start_deg,
            end_deg,
            color,
        );
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, color: Rgba) {
        let sf = self.s() as f32;
        self.line_raw(x0 * sf, y0 * sf, x1 * sf, y1 * sf, thickness * sf, color);
    }

    pub fn rounded_triangle(
        &mut self,
        p0: (f32, f32),
        p1: (f32, f32),
        p2: (f32, f32),
        corner_r: f32,
        color: Rgba,
    ) {
        let sf = self.s() as f32;
        self.rounded_triangle_raw(
            (p0.0 * sf, p0.1 * sf),
            (p1.0 * sf, p1.1 * sf),
            (p2.0 * sf, p2.1 * sf),
            corner_r * sf,
            color,
        );
    }

    /// Draw text with its baseline at (`x`, `baseline`); returns the advance
    /// in logical pixels.
    pub fn text(
        &mut self,
        font: Option<&FontRef<'_>>,
        value: &str,
        x: i32,
        baseline: i32,
        size: f32,
        color: Rgba,
    ) -> i32 {
        let (s, sf) = (self.s(), self.s() as f32);
        let adv = self.text_tracked_raw(font, value, x * s, baseline * s, size * sf, 0.0, color);
        (adv as f32 / sf).round() as i32
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_tracked(
        &mut self,
        font: Option<&FontRef<'_>>,
        value: &str,
        x: i32,
        baseline: i32,
        size: f32,
        tracking: f32,
        color: Rgba,
    ) -> i32 {
        let (s, sf) = (self.s(), self.s() as f32);
        let adv = self.text_tracked_raw(
            font,
            value,
            x * s,
            baseline * s,
            size * sf,
            tracking * sf,
            color,
        );
        (adv as f32 / sf).round() as i32
    }

    /// Draw crisp text with a soft atmospheric shadow underneath for clean
    /// legibility over photography and clear Liquid Glass.
    #[allow(clippy::too_many_arguments)]
    pub fn text_shadow(
        &mut self,
        font: Option<&FontRef<'_>>,
        value: &str,
        x: i32,
        baseline: i32,
        size: f32,
        tracking: f32,
        color: Rgba,
        shadow: Rgba,
        dy: i32,
    ) -> i32 {
        let (s, sf) = (self.s(), self.s() as f32);
        if shadow.3 > 0 {
            let core = Rgba::rgba(shadow.0, shadow.1, shadow.2, (shadow.3 as f32 * 0.55) as u8);
            let soft = Rgba::rgba(shadow.0, shadow.1, shadow.2, (shadow.3 as f32 * 0.22) as u8);
            self.text_tracked_raw(
                font,
                value,
                x * s,
                (baseline + dy) * s,
                size * sf,
                tracking * sf,
                core,
            );
            // Delicate atmospheric spread — never a hard outline halo.
            for (ox, oy) in [(-1, dy), (1, dy), (0, dy + 1)] {
                self.text_tracked_raw(
                    font,
                    value,
                    (x + ox) * s,
                    (baseline + oy) * s,
                    size * sf,
                    tracking * sf,
                    soft,
                );
            }
        }
        let adv = self.text_tracked_raw(
            font,
            value,
            x * s,
            baseline * s,
            size * sf,
            tracking * sf,
            color,
        );
        (adv as f32 / sf).round() as i32
    }

    pub fn draw_icon(&mut self, x: i32, y: i32, box_px: i32, icon: &Icon) {
        let s = self.s();
        self.draw_icon_raw(x * s, y * s, box_px * s, icon);
    }

    /// Directional rim light for thick glass: inside a rounded rectangle, a
    /// band of `band` logical pixels along the edge is lit according to how
    /// much the edge faces the light. The key light comes from the top-left
    /// (sharp, strong); a fill from the bottom-right is the internal
    /// reflection Liquid Glass shows on the opposite edge. Intensity falls
    /// off inward, so the edge reads as a lens rather than a stroke.
    #[allow(clippy::too_many_arguments)]
    pub fn rim_light(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: f32,
        band: f32,
        color: Rgba,
        fill: f32,
    ) {
        let s = self.s() as f32;
        let (x0, y0, w0, h0) = (x as f32 * s, y as f32 * s, w as f32 * s, h as f32 * s);
        let r = (r * s).min(w0 / 2.0).min(h0 / 2.0).max(0.0);
        let band = (band * s).max(1.0);
        let (cx, cy) = (x0 + w0 / 2.0, y0 + h0 / 2.0);
        let (hx, hy) = (w0 / 2.0 - r, h0 / 2.0 - r);
        let key = (-0.55f32, -0.835f32);
        let back = (0.55f32, 0.835f32);
        let px0 = (x0.floor() as i32).max(0);
        let py0 = (y0.floor() as i32).max(0);
        let px1 = ((x0 + w0).ceil() as i32).min(self.pw as i32);
        let py1 = ((y0 + h0).ceil() as i32).min(self.ph as i32);
        for py in py0..py1 {
            let dy = py as f32 + 0.5 - cy;
            for px in px0..px1 {
                let dx = px as f32 + 0.5 - cx;
                let qx = dx.abs() - hx;
                let qy = dy.abs() - hy;
                let (ox, oy) = (qx.max(0.0), qy.max(0.0));
                let outside = (ox * ox + oy * oy).sqrt();
                let d = outside + qx.max(qy).min(0.0) - r;
                if d > 0.5 || d < -band {
                    continue;
                }
                // Outward normal of the rounded rectangle at this point.
                let (nx, ny) = if outside > 0.0 {
                    (ox / outside * dx.signum(), oy / outside * dy.signum())
                } else if qx > qy {
                    (dx.signum(), 0.0)
                } else {
                    (0.0, dy.signum())
                };
                let k = (nx * key.0 + ny * key.1).max(0.0);
                let b = (nx * back.0 + ny * back.1).max(0.0);
                let facing = 0.1 + k.powf(1.4) * 0.95 + b.powf(1.8) * fill;
                let depth = (-d).max(0.0);
                let fall = (-depth / (band * 0.38)).exp();
                let cov = (0.5 - d).clamp(0.0, 1.0);
                self.blend(px, py, color, (facing * fall * cov).min(1.0));
            }
        }
    }

    /// Apple macOS 27 Golden Gate Liquid Glass optical edge & bevel:
    /// replaces any flat uniform 1 px stroke with:
    /// * an **inner luminous refraction bevel** (5..10 px caustic band that
    ///   concentrates inside curved corners where thick glass bends light);
    /// * **directional specular corner highlights** (razor-sharp white catch-light
    ///   peaking at the top-left corner and a softer internal reflection at the
    ///   bottom-right corner, fading gracefully along straight edges);
    /// * subtle chromatic rim dispersion (whisper of cool cyan top-left, warm
    ///   specular bounce bottom-right).
    #[allow(clippy::too_many_arguments)]
    pub fn liquid_glass_bevel(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, dark: bool) {
        let s = self.s() as f32;
        let (x0, y0, w0, h0) = (x as f32 * s, y as f32 * s, w as f32 * s, h as f32 * s);
        let r = (r * s).min(w0 / 2.0).min(h0 / 2.0).max(0.0);
        let bevel = (r * 0.45).clamp(5.0 * s, 11.0 * s);
        let (cx, cy) = (x0 + w0 / 2.0, y0 + h0 / 2.0);
        let (hx, hy) = (w0 / 2.0 - r, h0 / 2.0 - r);
        let key = (-0.707f32, -0.707f32);
        let bounce = (0.707f32, 0.707f32);
        let px0 = ((x0 - 2.0).floor() as i32).max(0);
        let py0 = ((y0 - 2.0).floor() as i32).max(0);
        let px1 = ((x0 + w0 + 2.0).ceil() as i32).min(self.pw as i32);
        let py1 = ((y0 + h0 + 2.0).ceil() as i32).min(self.ph as i32);

        for py in py0..py1 {
            let dy = py as f32 + 0.5 - cy;
            for px in px0..px1 {
                let dx = px as f32 + 0.5 - cx;
                let qx = dx.abs() - hx;
                let qy = dy.abs() - hy;
                let (ox, oy) = (qx.max(0.0), qy.max(0.0));
                let outside = (ox * ox + oy * oy).sqrt();
                let d = outside + qx.max(qy).min(0.0) - r;
                if d > 1.5 * s || d < -bevel {
                    continue;
                }
                // Whisper-thin outer dark occlusion (only 0..1 px outside) so
                // glass separates cleanly over bright wallpapers without looking stroked.
                if d > 0.0 {
                    let out_a = ((1.2 * s - d) / (1.2 * s)).clamp(0.0, 1.0);
                    self.blend(
                        px,
                        py,
                        Rgba::rgba(0, 0, 0, if dark { 46 } else { 24 }),
                        out_a * out_a,
                    );
                    continue;
                }
                let (nx, ny) = if outside > 0.0 {
                    (ox / outside * dx.signum(), oy / outside * dy.signum())
                } else if qx > qy {
                    (dx.signum(), 0.0)
                } else {
                    (0.0, dy.signum())
                };
                // Corner curvature weight: peaks at 45° corners, lower along flat sides.
                let corner_w = ((nx * ny).abs() * 2.4).clamp(0.0, 1.0);
                let k = (nx * key.0 + ny * key.1).max(0.0);
                let b = (nx * bounce.0 + ny * bounce.1).max(0.0);
                let top_edge = (-ny).max(0.0);
                let depth = (-d).max(0.0);
                let cov = (0.5 - d).clamp(0.0, 1.0);

                // 1. Inner luminous refraction bevel (soft optical caustic band).
                let bevel_fall = (-depth / (bevel * 0.42)).exp();
                let bevel_i = (0.06 + 0.34 * k.powf(1.5) + 0.22 * b.powf(1.8))
                    * (0.55 + 0.65 * corner_w)
                    * bevel_fall;
                let bevel_col = if dark {
                    Rgba::rgba(226, 238, 255, 118)
                } else {
                    Rgba::rgba(255, 255, 255, 165)
                };
                self.blend(px, py, bevel_col, (bevel_i * cov).min(1.0));

                // 2. Directional specular corner catch-light (1.6 px crisp optical rim).
                if depth <= 2.2 * s {
                    let rim_fall = (-depth / (0.85 * s)).exp();
                    let spec_tl = k.powf(2.4) * (0.48 + 0.72 * corner_w);
                    let spec_br = b.powf(2.8) * (0.28 + 0.60 * corner_w);
                    let spec_top = top_edge * 0.22;
                    let spec_total = (0.05 + spec_tl * 0.92 + spec_br * 0.52 + spec_top) * rim_fall;
                    let spec_col = if dark {
                        Rgba::rgba(255, 255, 255, 238)
                    } else {
                        Rgba::rgba(255, 255, 255, 255)
                    };
                    self.blend(px, py, spec_col, (spec_total * cov).min(1.0));
                }
            }
        }
    }

    /// pearOS's dock prism (its `dock_prism.frag`): an additive RGB glow on
    /// the rim of a rounded rectangle, fading inward over `edge` logical
    /// pixels as (1 + d / edge)^2. Red where the edge faces down-right, blue
    /// where it faces down-left, a little green all round. `amount` is the
    /// peak added to each channel (0..1); pearOS uses 8 × 2 × 0.004.
    #[allow(clippy::too_many_arguments)]
    pub fn prism_rim(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, edge: f32, amount: f32) {
        let s = self.s() as f32;
        let (x0, y0, w0, h0) = (x as f32 * s, y as f32 * s, w as f32 * s, h as f32 * s);
        let r = (r * s).min(w0 / 2.0).min(h0 / 2.0).max(0.0);
        let edge = (edge * s).max(1.0);
        let (cx, cy) = (x0 + w0 / 2.0, y0 + h0 / 2.0);
        let dist = |px: f32, py: f32| {
            let qx = (px - cx).abs() - (w0 / 2.0 - r);
            let qy = (py - cy).abs() - (h0 / 2.0 - r);
            let ox = qx.max(0.0);
            let oy = qy.max(0.0);
            (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
        };
        let px0 = (x0.floor() as i32).max(0);
        let py0 = (y0.floor() as i32).max(0);
        let px1 = ((x0 + w0).ceil() as i32).min(self.pw as i32);
        let py1 = ((y0 + h0).ceil() as i32).min(self.ph as i32);
        for py in py0..py1 {
            for px in px0..px1 {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                let d = dist(fx, fy);
                if d > 0.5 || d < -edge {
                    continue;
                }
                let ef = (1.0 + d / edge).clamp(0.0, 1.0).powi(2);
                let gx = dist(fx + 1.0, fy) - dist(fx - 1.0, fy);
                let gy = dist(fx, fy + 1.0) - dist(fx, fy - 1.0);
                let l = (gx * gx + gy * gy).sqrt().max(1e-6);
                let (nx, ny) = (gx / l, gy / l);
                let p = ef * amount * 255.0;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                let add = [
                    (nx + ny).max(0.0) * p * cov,
                    ef * p * 0.25 * cov,
                    (-nx + ny).max(0.0) * p * cov,
                ];
                let off = py as usize * self.stride + px as usize * 4;
                if off + 3 >= self.pixels.len() {
                    continue;
                }
                // BGRA: blue, green, red.
                for (c, v) in [(2usize, add[0]), (1, add[1]), (0, add[2])] {
                    let cur = self.pixels[off + c] as f32;
                    self.pixels[off + c] = (cur + v).round().min(255.0) as u8;
                }
                let a = self.pixels[off + 3] as f32;
                let need = add.iter().cloned().fold(0.0f32, f32::max);
                if need > 0.0 && a < 255.0 {
                    // Keep premultiplied colour valid: alpha at least the
                    // brightest channel added.
                    let m = (self.pixels[off] as f32)
                        .max(self.pixels[off + 1] as f32)
                        .max(self.pixels[off + 2] as f32);
                    self.pixels[off + 3] = a.max(m).min(255.0) as u8;
                }
            }
        }
    }

    /// Draw the Solar glyph `name`, tinted `color`, `size` logical pixels
    /// square, centred on (cx, cy). False when there is no such glyph.
    pub fn glyph(&mut self, name: &str, cx: f32, cy: f32, size: f32, color: Rgba) -> bool {
        let Some(mask) = crate::glyphs::get(name) else {
            return false;
        };
        let s = self.s() as f32;
        let side = (size * s).round().max(1.0);
        let (x0, y0) = (
            (cx * s - side / 2.0).round() as i32,
            (cy * s - side / 2.0).round() as i32,
        );
        let (mw, mh) = (mask.width as f32, mask.height as f32);
        let at = |x: i32, y: i32| -> f32 {
            let x = x.clamp(0, mask.width as i32 - 1) as u32;
            let y = y.clamp(0, mask.height as i32 - 1) as u32;
            mask.pixels[((y * mask.width + x) * 4 + 3) as usize] as f32 / 255.0
        };
        let n = side as i32;
        for py in 0..n {
            for px in 0..n {
                // Bilinear sample of the mask.
                let u = (px as f32 + 0.5) / side * mw - 0.5;
                let v = (py as f32 + 0.5) / side * mh - 0.5;
                let (ui, vi) = (u.floor() as i32, v.floor() as i32);
                let (fu, fv) = (u - ui as f32, v - vi as f32);
                let a = at(ui, vi) * (1.0 - fu) * (1.0 - fv)
                    + at(ui + 1, vi) * fu * (1.0 - fv)
                    + at(ui, vi + 1) * (1.0 - fu) * fv
                    + at(ui + 1, vi + 1) * fu * fv;
                if a > 0.003 {
                    self.blend(x0 + px, y0 + py, color, a);
                }
            }
        }
        true
    }

    /// Premultiplied Porter-Duff `source-over` into BGRA (`Argb8888` LE).
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, color: Rgba, coverage: f32) {
        if x < 0 || y < 0 || x as u32 >= self.pw || y as u32 >= self.ph {
            return;
        }
        let cov = coverage.clamp(0.0, 1.0);
        if cov <= 0.001 || color.3 == 0 {
            return;
        }
        let sa = (color.3 as f32 / 255.0) * cov;
        let inv = 1.0 - sa;
        let off = y as usize * self.stride + x as usize * 4;
        if off + 3 >= self.pixels.len() {
            return;
        }
        let db = self.pixels[off] as f32;
        let dg = self.pixels[off + 1] as f32;
        let dr = self.pixels[off + 2] as f32;
        let da = self.pixels[off + 3] as f32;

        self.pixels[off] = (color.2 as f32 * sa + db * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[off + 1] = (color.1 as f32 * sa + dg * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[off + 2] = (color.0 as f32 * sa + dr * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[off + 3] = (255.0 * sa + da * inv).round().clamp(0.0, 255.0) as u8;
    }

    fn fill_rect_raw(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgba) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.pw as i32);
        let y1 = (y + h).min(self.ph as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend(px, py, color, 1.0);
            }
        }
    }

    fn fill_rect_vgrad_raw(&mut self, x: i32, y: i32, w: i32, h: i32, top: Rgba, bottom: Rgba) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.pw as i32);
        let y1 = (y + h).min(self.ph as i32);
        let denom = (h - 1).max(1) as f32;
        for py in y0..y1 {
            let t = ((py - y) as f32 / denom).clamp(0.0, 1.0);
            let col = lerp_rgba(top, bottom, t);
            for px in x0..x1 {
                self.blend(px, py, col, 1.0);
            }
        }
    }

    fn round_rect_raw(&mut self, x: i32, y: i32, w: i32, h: i32, radius: f32, color: Rgba) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let (half_w, half_h) = (w as f32 * 0.5, h as f32 * 0.5);
        let (cx, cy) = (x as f32 + half_w, y as f32 + half_h);
        for py in y.max(0)..(y + h).min(self.ph as i32) {
            let qy = (py as f32 + 0.5 - cy).abs() - (half_h - r);
            for px in x.max(0)..(x + w).min(self.pw as i32) {
                let qx = (px as f32 + 0.5 - cx).abs() - (half_w - r);
                let sdf = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let cov = (0.5 - sdf).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn round_rect_vgrad_raw(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        top: Rgba,
        bottom: Rgba,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let (half_w, half_h) = (w as f32 * 0.5, h as f32 * 0.5);
        let (cx, cy) = (x as f32 + half_w, y as f32 + half_h);
        let denom = (h - 1).max(1) as f32;
        for py in y.max(0)..(y + h).min(self.ph as i32) {
            let t = ((py - y) as f32 / denom).clamp(0.0, 1.0);
            let col = lerp_rgba(top, bottom, t);
            let qy = (py as f32 + 0.5 - cy).abs() - (half_h - r);
            for px in x.max(0)..(x + w).min(self.pw as i32) {
                let qx = (px as f32 + 0.5 - cx).abs() - (half_w - r);
                let sdf = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let cov = (0.5 - sdf).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, col, cov);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn round_rect_shadow_raw(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        dy: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        if w <= 0 || h <= 0 || spread <= 0.5 || max_alpha == 0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let (hw, hh) = (w as f32 * 0.5, h as f32 * 0.5);
        let (cx, cy) = (x as f32 + hw, y as f32 + hh + dy);
        let (cx0, cy0) = (x as f32 + hw, y as f32 + hh);
        let pad = spread.ceil() as i32 + 2;
        let x0 = (x - pad).max(0);
        let y0 = (y + dy.floor() as i32 - pad).max(0);
        let x1 = (x + w + pad).min(self.pw as i32);
        let y1 = (y + h + dy.ceil() as i32 + pad).min(self.ph as i32);
        let col = Rgba::rgba(0, 0, 0, max_alpha);

        for py in y0..y1 {
            let qy = (py as f32 + 0.5 - cy).abs() - (hh - r);
            let qy0 = (py as f32 + 0.5 - cy0).abs() - (hh - r);
            for px in x0..x1 {
                let qx = (px as f32 + 0.5 - cx).abs() - (hw - r);
                let sdf = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                if sdf >= spread {
                    continue;
                }
                // The shadow never reaches under the shape itself, so a
                // translucent body is not darkened by its own shadow.
                let qx0 = (px as f32 + 0.5 - cx0).abs() - (hw - r);
                let inside = qx0.max(0.0).hypot(qy0.max(0.0)) + qx0.max(qy0).min(0.0) - r;
                let outside = (inside + 0.5).clamp(0.0, 1.0);
                if outside <= 0.0 {
                    continue;
                }
                let u = ((sdf + 1.0) / spread).clamp(0.0, 1.0);
                let falloff = (1.0 - u) * (1.0 - u) * (1.0 - u * 0.35) * outside;
                if falloff > 0.004 {
                    self.blend(px, py, col, falloff);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn round_rect_glass_rim_raw(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        thickness: f32,
        top: Rgba,
        side: Rgba,
        bottom: Rgba,
    ) {
        if w <= 0 || h <= 0 || thickness <= 0.0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let (hw, hh) = (w as f32 * 0.5, h as f32 * 0.5);
        let (cx, cy) = (x as f32 + hw, y as f32 + hh);
        let half_t = thickness * 0.5;
        let pad = thickness.ceil() as i32 + 2;
        let x0 = (x - pad).max(0);
        let y0 = (y - pad).max(0);
        let x1 = (x + w + pad).min(self.pw as i32);
        let y1 = (y + h + pad).min(self.ph as i32);

        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let (sdf, _nx, ny) = round_rect_sdf_normal(dx, dy, hw, hh, r);
                let ring_dist = (sdf + half_t).abs() - half_t;
                let cov = (0.5 - ring_dist).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let col = if ny < 0.0 {
                    lerp_rgba(side, top, (-ny).powf(1.25))
                } else {
                    lerp_rgba(side, bottom, ny.powf(1.25))
                };
                self.blend(px, py, col, cov);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn squircle_vgrad_raw(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        top: Rgba,
        bottom: Rgba,
        sheen: f32,
    ) {
        if size <= 0 {
            return;
        }
        let half = size as f32 * 0.5;
        let (cx, cy) = (x as f32 + half, y as f32 + half);
        let denom = (size - 1).max(1) as f32;
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + size).min(self.pw as i32);
        let y1 = (y + size).min(self.ph as i32);

        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            let ty = ((py - y) as f32 / denom).clamp(0.0, 1.0);
            let row_col = lerp_rgba(top, bottom, ty);
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let (sdf, _, _) = squircle_sdf_normal(dx, dy, half, half, 4.6);
                let cov = (0.5 - sdf).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let col = if sheen > 0.001 {
                    // Subtle overhead radial luminosity (Apple icon depth, not a hard band).
                    let lx = (dx / half) + 0.16;
                    let ly = (dy / half) + 0.72;
                    let d2 = lx * lx * 0.65 + ly * ly * 0.95;
                    let glow = (-d2 * 1.85).exp() * sheen;
                    add_light(row_col, glow)
                } else {
                    row_col
                };
                self.blend(px, py, col, cov);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn squircle_shadow_raw(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        dy_off: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        if size <= 0 || spread <= 0.5 || max_alpha == 0 {
            return;
        }
        let half = size as f32 * 0.5;
        let (cx, cy) = (x as f32 + half, y as f32 + half + dy_off);
        let pad = spread.ceil() as i32 + 2;
        let x0 = (x - pad).max(0);
        let y0 = (y + dy_off.floor() as i32 - pad).max(0);
        let x1 = (x + size + pad).min(self.pw as i32);
        let y1 = (y + size + dy_off.ceil() as i32 + pad).min(self.ph as i32);
        let col = Rgba::rgba(0, 0, 0, max_alpha);

        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let (sdf, _, _) = squircle_sdf_normal(dx, dy, half, half, 4.6);
                if sdf >= spread {
                    continue;
                }
                let u = ((sdf + 0.8) / spread).clamp(0.0, 1.0);
                let falloff = (1.0 - u) * (1.0 - u);
                if falloff > 0.005 {
                    self.blend(px, py, col, falloff);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn squircle_glass_rim_raw(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        thickness: f32,
        top: Rgba,
        side: Rgba,
        bottom: Rgba,
    ) {
        if size <= 0 || thickness <= 0.0 {
            return;
        }
        let half = size as f32 * 0.5;
        let (cx, cy) = (x as f32 + half, y as f32 + half);
        let half_t = thickness * 0.5;
        let pad = thickness.ceil() as i32 + 2;
        let x0 = (x - pad).max(0);
        let y0 = (y - pad).max(0);
        let x1 = (x + size + pad).min(self.pw as i32);
        let y1 = (y + size + pad).min(self.ph as i32);

        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let (sdf, _, ny) = squircle_sdf_normal(dx, dy, half, half, 4.6);
                let ring_dist = (sdf + half_t).abs() - half_t;
                let cov = (0.5 - ring_dist).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let col = if ny < 0.0 {
                    lerp_rgba(side, top, (-ny).powf(1.2))
                } else {
                    lerp_rgba(side, bottom, ny.powf(1.2))
                };
                self.blend(px, py, col, cov);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn stroke_round_rect_raw(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        thickness: f32,
        color: Rgba,
    ) {
        if w <= 0 || h <= 0 || thickness <= 0.0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let half_w = w as f32 * 0.5;
        let half_h = h as f32 * 0.5;
        let cx = x as f32 + half_w;
        let cy = y as f32 + half_h;
        let half_t = thickness * 0.5;

        let pad = (thickness.ceil() as i32) + 2;
        let x0 = (x - pad).max(0);
        let y0 = (y - pad).max(0);
        let x1 = (x + w + pad).min(self.pw as i32);
        let y1 = (y + h + pad).min(self.ph as i32);

        for py in y0..y1 {
            let qy = ((py as f32 + 0.5 - cy).abs()) - (half_h - r);
            for px in x0..x1 {
                let qx = ((px as f32 + 0.5 - cx).abs()) - (half_w - r);
                let sdf = (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r;
                // Center the stroke just inside the outer edge [ -thickness .. 0 ].
                let ring_dist = (sdf + half_t).abs() - half_t;
                let cov = (0.5 - ring_dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    fn circle_raw(&mut self, cx: f32, cy: f32, radius: f32, color: Rgba) {
        let x0 = (cx - radius - 2.0).floor().max(0.0) as i32;
        let y0 = (cy - radius - 2.0).floor().max(0.0) as i32;
        let x1 = (cx + radius + 2.0).ceil().min(self.pw as f32) as i32;
        let y1 = (cy + radius + 2.0).ceil().min(self.ph as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let d = (px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - radius;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    fn ring_raw(&mut self, cx: f32, cy: f32, radius: f32, thickness: f32, color: Rgba) {
        let half = thickness * 0.5;
        let pad = radius + half + 2.0;
        let x0 = (cx - pad).floor().max(0.0) as i32;
        let y0 = (cy - pad).floor().max(0.0) as i32;
        let x1 = (cx + pad).ceil().min(self.pw as f32) as i32;
        let y1 = (cy + pad).ceil().min(self.ph as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let d = ((px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - radius).abs() - half;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    /// Anti-aliased arc segment (`start_deg` .. `end_deg`, clockwise from +X).
    #[allow(clippy::too_many_arguments)]
    fn arc_raw(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        thickness: f32,
        start_deg: f32,
        end_deg: f32,
        color: Rgba,
    ) {
        let half = thickness * 0.5;
        let pad = radius + half + 2.0;
        let x0 = (cx - pad).floor().max(0.0) as i32;
        let y0 = (cy - pad).floor().max(0.0) as i32;
        let x1 = (cx + pad).ceil().min(self.pw as f32) as i32;
        let y1 = (cy + pad).ceil().min(self.ph as f32) as i32;
        let s = start_deg.to_radians();
        let e = end_deg.to_radians();
        let mid = 0.5 * (s + e);
        let half_span = 0.5 * (e - s).abs();

        for py in y0..y1 {
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let dy = py as f32 + 0.5 - cy;
                let r_dist = (dx.hypot(dy) - radius).abs() - half;
                if r_dist >= 0.6 {
                    continue;
                }
                let ang = dy.atan2(dx);
                let mut diff = (ang - mid).rem_euclid(std::f32::consts::TAU);
                if diff > std::f32::consts::PI {
                    diff -= std::f32::consts::TAU;
                }
                let a_dist = (diff.abs() - half_span) * radius;
                let d = r_dist.max(a_dist - half);
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    /// Anti-aliased capsule line segment.
    #[allow(clippy::too_many_arguments)]
    fn line_raw(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, color: Rgba) {
        let half = thickness * 0.5;
        let min_x = (x0.min(x1) - half - 2.0).floor().max(0.0) as i32;
        let min_y = (y0.min(y1) - half - 2.0).floor().max(0.0) as i32;
        let max_x = (x0.max(x1) + half + 2.0).ceil().min(self.pw as f32) as i32;
        let max_y = (y0.max(y1) + half + 2.0).ceil().min(self.ph as f32) as i32;
        let vx = x1 - x0;
        let vy = y1 - y0;
        let len_sq = (vx * vx + vy * vy).max(0.0001);

        for py in min_y..max_y {
            for px in min_x..max_x {
                let wx = px as f32 + 0.5 - x0;
                let wy = py as f32 + 0.5 - y0;
                let t = ((wx * vx + wy * vy) / len_sq).clamp(0.0, 1.0);
                let proj_x = x0 + t * vx;
                let proj_y = y0 + t * vy;
                let d = (px as f32 + 0.5 - proj_x).hypot(py as f32 + 0.5 - proj_y) - half;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    fn rounded_triangle_raw(
        &mut self,
        p0: (f32, f32),
        p1: (f32, f32),
        p2: (f32, f32),
        corner_r: f32,
        color: Rgba,
    ) {
        let pad = corner_r + 2.0;
        let min_x = (p0.0.min(p1.0).min(p2.0) - pad).floor().max(0.0) as i32;
        let min_y = (p0.1.min(p1.1).min(p2.1) - pad).floor().max(0.0) as i32;
        let max_x = (p0.0.max(p1.0).max(p2.0) + pad).ceil().min(self.pw as f32) as i32;
        let max_y = (p0.1.max(p1.1).max(p2.1) + pad).ceil().min(self.ph as f32) as i32;
        for py in min_y..max_y {
            for px in min_x..max_x {
                let d = triangle_sdf(px as f32 + 0.5, py as f32 + 0.5, p0, p1, p2) - corner_r;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text_tracked_raw(
        &mut self,
        font: Option<&FontRef<'_>>,
        value: &str,
        x: i32,
        baseline: i32,
        size: f32,
        tracking: f32,
        color: Rgba,
    ) -> i32 {
        let Some(font) = font else {
            return 0;
        };
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let mut caret = point(x as f32, baseline as f32);
        let mut prev: Option<ab_glyph::GlyphId> = None;
        let count = value.chars().count();
        for (idx, ch) in value.chars().enumerate() {
            let gid = scaled.glyph_id(ch);
            if let Some(p) = prev {
                caret.x += scaled.kern(p, gid);
            }
            if let Some(outline) = scaled.outline_glyph(gid.with_scale_and_position(scale, caret)) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, cov| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    self.blend(px, py, color, cov);
                });
            }
            caret.x += scaled.h_advance(gid);
            if idx + 1 < count {
                caret.x += tracking;
            }
            prev = Some(gid);
        }
        (caret.x - x as f32).round() as i32
    }

    pub fn text_truncated(
        &mut self,
        font: Option<&FontRef<'_>>,
        value: &str,
        x: i32,
        baseline: i32,
        size: f32,
        max_w: i32,
        color: Rgba,
    ) -> i32 {
        let Some(font) = font else {
            return 0;
        };
        if text_width(Some(font), value, size) <= max_w {
            return self.text(Some(font), value, x, baseline, size, color);
        }
        let mut out = String::new();
        let ell_w = text_width(Some(font), "…", size);
        for ch in value.chars() {
            let mut next = out.clone();
            next.push(ch);
            if text_width(Some(font), &next, size) + ell_w > max_w {
                break;
            }
            out = next;
        }
        out.push('…');
        self.text(Some(font), &out, x, baseline, size, color)
    }

    /// Area-averaged (box-filtered) RGBA icon blit for crisp downsampling.
    fn draw_icon_raw(&mut self, x: i32, y: i32, box_px: i32, icon: &Icon) {
        if icon.width == 0 || icon.height == 0 || box_px <= 0 {
            return;
        }
        let sx_ratio = icon.width as f32 / box_px as f32;
        let sy_ratio = icon.height as f32 / box_px as f32;

        for dy in 0..box_px {
            let sy0 = (dy as f32 * sy_ratio).floor().max(0.0) as u32;
            let sy1 = (((dy + 1) as f32 * sy_ratio).ceil() as u32)
                .max(sy0 + 1)
                .min(icon.height);
            for dx in 0..box_px {
                let sx0 = (dx as f32 * sx_ratio).floor().max(0.0) as u32;
                let sx1 = (((dx + 1) as f32 * sx_ratio).ceil() as u32)
                    .max(sx0 + 1)
                    .min(icon.width);
                let mut acc = [0.0f32; 4];
                let mut count = 0.0f32;
                for iy in sy0..sy1 {
                    for ix in sx0..sx1 {
                        let off = ((iy * icon.width + ix) * 4) as usize;
                        let a = icon.pixels[off + 3] as f32 / 255.0;
                        acc[0] += icon.pixels[off] as f32 * a;
                        acc[1] += icon.pixels[off + 1] as f32 * a;
                        acc[2] += icon.pixels[off + 2] as f32 * a;
                        acc[3] += a;
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    let alpha = (acc[3] / count).clamp(0.0, 1.0);
                    if alpha > 0.004 {
                        let inv_a = 1.0 / acc[3].max(0.0001);
                        let r = (acc[0] * inv_a).round().clamp(0.0, 255.0) as u8;
                        let g = (acc[1] * inv_a).round().clamp(0.0, 255.0) as u8;
                        let b = (acc[2] * inv_a).round().clamp(0.0, 255.0) as u8;
                        self.blend(x + dx, y + dy, Rgba::rgb(r, g, b), alpha);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------- vector glyphs

    /// The concave fillet that joins a notch to the top edge: fills the
    /// `r`×`r` square at (`x`, `y`) except a quarter disc. `left` puts the
    /// disc's centre on the square's left side (a fillet on the notch's left).
    pub fn concave_corner(&mut self, x: f32, y: f32, r: f32, left: bool, color: Rgba) {
        let sf = self.s() as f32;
        let (x, y, r) = (x * sf, y * sf, r * sf);
        let (ccx, ccy) = (if left { x } else { x + r }, y + r);
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (x1, y1) = ((x + r).ceil() as i32, (y + r).ceil() as i32);
        for py in y0.max(0)..y1.min(self.ph as i32) {
            for px in x0.max(0)..x1.min(self.pw as i32) {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                if fx < x || fx > x + r || fy < y || fy > y + r {
                    continue;
                }
                let d = (fx - ccx).hypot(fy - ccy);
                let cov = (d - r + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    /// A rectangle whose top corners are square and bottom corners round:
    /// the body of a notch hanging from the top edge.
    pub fn notch_body(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, color: Rgba) {
        let ri = r.ceil() as i32;
        self.round_rect(x, y, w, h, r, color);
        self.fill_rect(x, y, w, ri.min(h), color);
    }

    pub fn icon_note(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("note", cx, cy, 15.0, color) {
            return;
        }
        self.circle(cx - 2.8, cy + 3.6, 2.3, color);
        self.circle(cx + 3.8, cy + 2.2, 2.3, color);
        self.line(cx - 0.8, cy + 3.4, cx - 0.8, cy - 4.4, 1.4, color);
        self.line(cx + 5.8, cy + 2.0, cx + 5.8, cy - 5.8, 1.4, color);
        self.line(cx - 0.8, cy - 4.4, cx + 5.8, cy - 5.8, 2.1, color);
    }

    /// SF Symbols-style rounded Play triangle.
    pub fn icon_play(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        let s = size * 0.48;
        let cr = (size * 0.11).clamp(1.0, 2.6);
        let p0 = (cx - s * 0.55, cy - s * 0.82);
        let p1 = (cx - s * 0.55, cy + s * 0.82);
        let p2 = (cx + s * 0.88, cy);
        self.rounded_triangle(p0, p1, p2, cr, color);
    }

    /// SF Symbols-style rounded Pause twin bars.
    pub fn icon_pause(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        let bar_w = (size * 0.28).round().max(3.0) as i32;
        let bar_h = (size * 0.92).round().max(8.0) as i32;
        let gap = (size * 0.22).round().max(3.0) as i32;
        let r = (bar_w as f32 * 0.44).max(1.2);
        let top = (cy - bar_h as f32 / 2.0).round() as i32;
        let left = (cx - (bar_w as f32 + gap as f32 * 0.5)).round() as i32;
        self.round_rect(left, top, bar_w, bar_h, r, color);
        self.round_rect(left + bar_w + gap, top, bar_w, bar_h, r, color);
    }

    /// SF Symbols-style `forward.fill` / `backward.fill` twin rounded triangles.
    pub fn icon_skip(&mut self, cx: f32, cy: f32, size: f32, forward: bool, color: Rgba) {
        let s = size * 0.36;
        let dir = if forward { 1.0 } else { -1.0 };
        let cr = (size * 0.08).clamp(0.8, 2.0);
        for offset in [-s * 0.72, s * 0.58] {
            let ox = cx + dir * offset;
            let p0 = (ox - dir * s * 0.52, cy - s * 0.72);
            let p1 = (ox - dir * s * 0.52, cy + s * 0.72);
            let p2 = (ox + dir * s * 0.72, cy);
            self.rounded_triangle(p0, p1, p2, cr, color);
        }
    }

    /// Render real album artwork (when provided by MPRIS / LRCLIB cache) clipped
    /// strictly inside a concentric rounded rect with a subtle optical glass sheen;
    /// falls back to the procedural coastal-twilight artwork when `art` is `None`.
    pub fn draw_album_art_opt(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        r: f32,
        art: Option<&Icon>,
    ) {
        let Some(ic) = art else {
            self.draw_album_art(x, y, size, r);
            return;
        };
        if ic.width == 0 || ic.height == 0 {
            self.draw_album_art(x, y, size, r);
            return;
        }
        let sf = self.s() as f32;
        let x0 = (x as f32 * sf).round() as i32;
        let y0 = (y as f32 * sf).round() as i32;
        let sw = (size as f32 * sf).round() as i32;
        let rr = (r * sf).min(sw as f32 / 2.0).max(0.0);
        let cx_r = x0 as f32 + sw as f32 * 0.5;
        let cy_r = y0 as f32 + sw as f32 * 0.5;
        let hx = sw as f32 * 0.5 - rr;
        let hy = sw as f32 * 0.5 - rr;
        let px0 = x0.max(0);
        let py0 = y0.max(0);
        let px1 = (x0 + sw).min(self.pw as i32);
        let py1 = (y0 + sw).min(self.ph as i32);
        for py in py0..py1 {
            let v = ((py - y0) as f32 / sw.max(1) as f32).clamp(0.0, 0.9999);
            let sy = (v * ic.height as f32) as u32;
            for px in px0..px1 {
                let qx = (px as f32 + 0.5 - cx_r).abs() - hx;
                let qy = (py as f32 + 0.5 - cy_r).abs() - hy;
                let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rr;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let u = ((px - x0) as f32 / sw.max(1) as f32).clamp(0.0, 0.9999);
                let sx = (u * ic.width as f32) as u32;
                let si = ((sy * ic.width + sx) * 4) as usize;
                let col = Rgba::rgba(
                    ic.pixels[si],
                    ic.pixels[si + 1],
                    ic.pixels[si + 2],
                    ic.pixels[si + 3],
                );
                self.blend(px, py, col, cov);
            }
        }
        self.rim_light(x, y, size, size, r, 1.2, Rgba::rgba(255, 255, 255, 90), 0.3);
    }

    /// Rich procedural coastal-twilight album artwork clipped strictly inside a
    /// concentric rounded rect (zero corner overhang, zero purple).
    pub fn draw_album_art(&mut self, x: i32, y: i32, size: i32, r: f32) {
        let sf = self.s() as f32;
        let x0 = (x as f32 * sf).round() as i32;
        let y0 = (y as f32 * sf).round() as i32;
        let sw = (size as f32 * sf).round() as i32;
        let rr = (r * sf).min(sw as f32 / 2.0).max(0.0);
        let h_f = sw.max(1) as f32;
        let w_f = sw.max(1) as f32;

        let px0 = x0.max(0);
        let py0 = y0.max(0);
        let px1 = (x0 + sw).min(self.pw as i32);
        let py1 = (y0 + sw).min(self.ph as i32);
        let cx_r = x0 as f32 + w_f * 0.5;
        let cy_r = y0 as f32 + h_f * 0.5;
        let hx = w_f * 0.5 - rr;
        let hy = h_f * 0.5 - rr;

        for py in py0..py1 {
            let ny = (py as f32 + 0.5 - y0 as f32) / h_f;
            for px in px0..px1 {
                let nx = (px as f32 + 0.5 - x0 as f32) / w_f;
                // Analytical rounded-rect SDF clip so all internal layers stay strictly inside.
                let qx = (px as f32 + 0.5 - cx_r).abs() - hx;
                let qy = (py as f32 + 0.5 - cy_r).abs() - hy;
                let d = (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - rr;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }

                // Coastal sunset sky (deep ocean blue -> warm twilight coral/amber)
                // over turquoise twilight sea below horizon (ny > 0.68).
                let mut col = if ny < 0.68 {
                    let t = ny / 0.68;
                    let sky =
                        lerp_rgba(Rgba::rgb(0x0f, 0x3b, 0x5c), Rgba::rgb(0x28, 0x76, 0x8a), t);
                    // Warm golden sunset bloom near horizon.
                    let dx = nx - 0.54;
                    let dy = (ny - 0.64) * 1.15;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let glow = (1.0 - dist * 1.85).clamp(0.0, 1.0);
                    let sun = (1.0 - dist * 4.4).clamp(0.0, 1.0);
                    let c1 = lerp_rgba(sky, Rgba::rgb(0xf4, 0x97, 0x56), glow * glow * 0.78);
                    lerp_rgba(c1, Rgba::rgb(0xff, 0xdf, 0xa8), sun * sun * 0.82)
                } else {
                    let t = (ny - 0.68) / 0.32;
                    let sea =
                        lerp_rgba(Rgba::rgb(0x1f, 0x7a, 0x7a), Rgba::rgb(0x0a, 0x2c, 0x3d), t);
                    let ref_dx = (nx - 0.54).abs();
                    let shimmer = (1.0 - ref_dx * 3.6).clamp(0.0, 1.0) * (1.0 - t * 0.65);
                    lerp_rgba(sea, Rgba::rgb(0xff, 0xc8, 0x82), shimmer * shimmer * 0.55)
                };
                // Subtle overhead liquid specular sheen.
                let sheen = (1.0 - ny * 2.2).clamp(0.0, 1.0) * 0.14;
                col = lerp_rgba(col, Rgba::rgb(255, 255, 255), sheen);
                self.blend(px, py, col, cov);
            }
        }

        let s = size as f32;
        let (cx, cy) = (x as f32 + s * 0.48, y as f32 + s * 0.48);
        if size >= 32 {
            self.icon_note(cx, cy + 1.0, Rgba::rgba(0, 0, 0, 88));
            self.icon_note(cx, cy, Rgba::rgb(0xff, 0xff, 0xff));
        } else {
            self.circle(cx - 2.0, cy + 2.2, 1.8, Rgba::rgb(0xff, 0xff, 0xff));
            self.circle(cx + 2.6, cy + 1.4, 1.8, Rgba::rgb(0xff, 0xff, 0xff));
            self.line(
                cx - 0.5,
                cy + 2.0,
                cx - 0.5,
                cy - 3.2,
                1.2,
                Rgba::rgb(0xff, 0xff, 0xff),
            );
            self.line(
                cx + 4.0,
                cy + 1.2,
                cx + 4.0,
                cy - 4.0,
                1.2,
                Rgba::rgb(0xff, 0xff, 0xff),
            );
            self.line(
                cx - 0.5,
                cy - 3.2,
                cx + 4.0,
                cy - 4.0,
                1.7,
                Rgba::rgb(0xff, 0xff, 0xff),
            );
        }
        self.round_rect_glass_rim(
            x,
            y,
            size,
            size,
            r,
            Rgba::rgba(255, 255, 255, 112),
            Rgba::rgba(255, 255, 255, 42),
            Rgba::rgba(255, 255, 255, 20),
        );
    }

    pub fn icon_bell(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("bell", cx, cy, 16.0, color) {
            return;
        }
        self.arc(cx, cy, 5.0, 1.5, 180.0, 360.0, color);
        self.line(cx - 5.0, cy, cx - 5.5, cy + 4.0, 1.5, color);
        self.line(cx + 5.0, cy, cx + 5.5, cy + 4.0, 1.5, color);
        self.line(cx - 6.5, cy + 4.5, cx + 6.5, cy + 4.5, 1.5, color);
        self.circle(cx, cy + 6.8, 1.4, color);
    }

    pub fn icon_solyra(&mut self, cx: f32, cy: f32, r: f32, color: Rgba) {
        self.ring(cx, cy, r, 1.6, color);
        self.circle(cx + r * 0.28, cy - r * 0.25, r * 0.34, color);
    }

    pub fn icon_cloud(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        if self.glyph("cloud", cx, cy, size * 1.1, color) {
            return;
        }
        let u = size * 0.28;
        self.circle(cx - u * 0.65, cy + u * 0.15, u * 0.72, color);
        self.circle(cx + u * 0.15, cy - u * 0.32, u * 0.92, color);
        self.circle(cx + u * 0.88, cy + u * 0.18, u * 0.64, color);
        self.round_rect(
            (cx - u * 1.15).round() as i32,
            (cy + u * 0.05).round() as i32,
            (u * 2.45).round() as i32,
            (u * 0.82).round().max(2.0) as i32,
            u * 0.4,
            color,
        );
    }

    pub fn icon_laptop(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        if self.glyph("laptop", cx, cy, size * 1.05, color) {
            return;
        }
        let w = (size * 0.78).round() as i32;
        let h = (size * 0.52).round() as i32;
        let x = (cx - w as f32 * 0.5).round() as i32;
        let y = (cy - h as f32 * 0.55).round() as i32;
        self.stroke_round_rect(x, y, w, h, 2.0, 1.3, color);
        self.line(
            cx - size * 0.48,
            cy + size * 0.30,
            cx + size * 0.48,
            cy + size * 0.30,
            1.5,
            color,
        );
    }

    pub fn icon_branch(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        let u = size * 0.30;
        self.line(cx - u * 0.55, cy - u, cx - u * 0.55, cy + u, 1.4, color);
        self.line(
            cx - u * 0.55,
            cy + u * 0.15,
            cx + u * 0.65,
            cy - u * 0.45,
            1.4,
            color,
        );
        self.circle(cx - u * 0.55, cy - u, 1.8, color);
        self.circle(cx - u * 0.55, cy + u, 1.8, color);
        self.circle(cx + u * 0.65, cy - u * 0.55, 1.8, color);
    }

    /// The **nL** brand mark: white semibold letters in a black rounded box,
    /// sized to `h`. Returns the width so callers can size hit targets.
    /// At the menu-bar height (`h` ≈ 22) the box lands around 36–44 px wide.
    pub fn mark_nl(&mut self, x: i32, y: i32, h: i32, fonts: &Fonts) -> i32 {
        let h = h.max(16);
        let (w, size, tw) = nl_metrics(h, fonts);
        let r = (h as f32 * 0.28).clamp(4.0, 10.0);
        self.round_rect(x, y, w, h, r, Rgba::rgb(0, 0, 0));
        let baseline = y + h / 2 + (size * 0.36) as i32;
        self.text(
            fonts.semibold.as_ref(),
            "nL",
            x + (w - tw) / 2,
            baseline,
            size,
            Rgba::rgb(255, 255, 255),
        );
        w
    }

    pub fn icon_search(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        if self.glyph("search", cx, cy, size * 1.05, color) {
            return;
        }
        let r = size * 0.33;
        self.ring(cx - size * 0.08, cy - size * 0.08, r, 1.5, color);
        self.line(
            cx + r * 0.55,
            cy + r * 0.55,
            cx + size * 0.38,
            cy + size * 0.38,
            1.65,
            color,
        );
    }

    pub fn icon_wifi(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        if self.glyph("wifi-full", cx, cy, size * 1.05, color) {
            return;
        }
        let base_y = cy + size * 0.28;
        self.circle(cx, base_y, 1.4, color);
        self.arc(cx, base_y, size * 0.28, 1.45, -135.0, -45.0, color);
        self.arc(cx, base_y, size * 0.50, 1.45, -135.0, -45.0, color);
    }

    /// Wi-Fi arcs when wireless, a plug-like bar when wired, struck through
    /// when offline, so the state is not carried by colour alone.
    pub fn icon_network(
        &mut self,
        cx: f32,
        cy: f32,
        size: f32,
        state: crate::state::NetworkState,
        color: Rgba,
        dim: Rgba,
    ) {
        use crate::state::NetworkState::*;
        match state {
            Wireless => self.icon_wifi(cx, cy, size, color),
            Wired => {
                let w = size * 0.8;
                self.round_rect(
                    (cx - w / 2.0).round() as i32,
                    (cy - 3.0).round() as i32,
                    w.round() as i32,
                    6,
                    1.5,
                    color,
                );
                self.line(cx, cy + 3.0, cx, cy + size * 0.45, 1.5, color);
            }
            Offline => {
                self.icon_wifi(cx, cy, size, dim);
                self.line(
                    cx - size * 0.5,
                    cy - size * 0.4,
                    cx + size * 0.5,
                    cy + size * 0.5,
                    1.6,
                    color,
                );
            }
        }
    }

    pub fn icon_volume(&mut self, cx: f32, cy: f32, size: f32, level: u8, color: Rgba) {
        let v = match level {
            0 => "volume-mute",
            1 => "volume-low",
            _ => "volume",
        };
        if self.glyph(v, cx, cy, size * 1.05, color) {
            return;
        }
        let lx = cx - size * 0.30;
        self.line(lx, cy - 2.2, lx, cy + 2.2, 2.2, color);
        self.line(lx, cy - 1.5, lx + 3.4, cy - 4.2, 1.7, color);
        self.line(lx, cy + 1.5, lx + 3.4, cy + 4.2, 1.7, color);
        self.line(lx + 3.4, cy - 4.2, lx + 3.4, cy + 4.2, 1.6, color);
        if level == 0 {
            self.line(cx + 1.5, cy - 2.5, cx + 5.5, cy + 2.5, 1.4, color);
            self.line(cx + 5.5, cy - 2.5, cx + 1.5, cy + 2.5, 1.4, color);
        } else {
            self.arc(lx + 3.5, cy, 4.2, 1.35, -45.0, 45.0, color);
            if level > 50 {
                self.arc(lx + 3.5, cy, 7.0, 1.35, -48.0, 48.0, color);
            }
        }
    }

    pub fn icon_battery(&mut self, cx: f32, cy: f32, pct: u8, color: Rgba, fill_col: Rgba) {
        let w = 20;
        let h = 10;
        let x = cx.round() as i32 - w / 2 - 1;
        let y = cy.round() as i32 - h / 2;
        self.round_rect(x, y, w, h, 3.0, Rgba::rgba(color.0, color.1, color.2, 26));
        self.stroke_round_rect(
            x,
            y,
            w,
            h,
            3.0,
            1.15,
            Rgba::rgba(color.0, color.1, color.2, 200),
        );
        self.round_rect(
            x + w + 1,
            y + 3,
            2,
            4,
            1.0,
            Rgba::rgba(color.0, color.1, color.2, 180),
        );
        let fill_w = (((w - 4) as f32) * (pct.min(100) as f32 / 100.0)).round() as i32;
        if fill_w > 0 {
            self.round_rect(x + 2, y + 2, fill_w, h - 4, 1.5, fill_col);
        }
    }

    /// macOS Control Centre twin pill-switches icon.
    pub fn icon_control_centre(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("control", cx, cy, 17.0, color) {
            return;
        }
        let x = cx.round() as i32 - 7;
        let y = cy.round() as i32 - 6;
        self.stroke_round_rect(x, y, 14, 5, 2.5, 1.2, color);
        self.circle(x as f32 + 11.2, y as f32 + 2.5, 1.8, color);
        self.stroke_round_rect(x, y + 7, 14, 5, 2.5, 1.2, color);
        self.circle(x as f32 + 2.8, y as f32 + 9.5, 1.8, color);
    }

    pub fn icon_moon(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("moon", cx, cy, 16.0, color) {
            return;
        }
        self.arc(cx + 0.5, cy, 4.8, 1.6, 55.0, 305.0, color);
    }

    pub fn icon_sun(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("sun", cx, cy, 17.0, color) {
            return;
        }
        self.ring(cx, cy, 3.2, 1.4, color);
        for i in 0..8 {
            let a = (i as f32) * std::f32::consts::FRAC_PI_4;
            self.line(
                cx + a.cos() * 5.0,
                cy + a.sin() * 5.0,
                cx + a.cos() * 6.6,
                cy + a.sin() * 6.6,
                1.3,
                color,
            );
        }
    }

    #[allow(dead_code)]
    pub fn icon_lock(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("lock", cx, cy, 16.0, color) {
            return;
        }
        let x = cx.round() as i32 - 5;
        let y = cy.round() as i32 - 1;
        self.stroke_round_rect(x, y, 10, 7, 2.0, 1.3, color);
        self.arc(cx, y as f32, 3.0, 1.3, -180.0, 0.0, color);
    }

    #[allow(dead_code)]
    pub fn icon_power(&mut self, cx: f32, cy: f32, color: Rgba) {
        if self.glyph("power", cx, cy, 16.0, color) {
            return;
        }
        self.arc(cx, cy + 0.5, 5.0, 1.5, -55.0, 235.0, color);
        self.line(cx, cy - 5.5, cx, cy - 0.5, 1.5, color);
    }

    // ------------------------------------------------------------ app tiles

    /// A disc shaded from `inner` at a focus point to `outer` at the far rim.
    /// `focus` is where the lit centre sits, as a fraction of the radius.
    pub fn circle_radial(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        focus: (f32, f32),
        inner: Rgba,
        outer: Rgba,
    ) {
        let sf = self.s() as f32;
        self.circle_radial_raw(cx * sf, cy * sf, radius * sf, focus, inner, outer);
    }

    fn circle_radial_raw(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        focus: (f32, f32),
        inner: Rgba,
        outer: Rgba,
    ) {
        let (fx, fy) = (cx + focus.0 * radius, cy + focus.1 * radius);
        let reach = radius + focus.0.hypot(focus.1) * radius;
        let x0 = (cx - radius - 2.0).floor().max(0.0) as i32;
        let y0 = (cy - radius - 2.0).floor().max(0.0) as i32;
        let x1 = (cx + radius + 2.0).ceil().min(self.pw as f32) as i32;
        let y1 = (cy + radius + 2.0).ceil().min(self.ph as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let cov = (0.5 - (dx.hypot(dy) - radius)).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let t =
                    ((px as f32 + 0.5 - fx).hypot(py as f32 + 0.5 - fy) / reach).clamp(0.0, 1.0);
                self.blend(px, py, lerp_rgba(inner, outer, t.sqrt()), cov);
            }
        }
    }

    /// Blit an icon whose squircle body fills `BODY` of its canvas so that the
    /// *body* covers `size` logical pixels at (`x`, `y`). `grow` scales it
    /// further and `lift` moves it up by that fraction of its size.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_icon_body(&mut self, x: i32, y: i32, size: i32, icon: &Icon, grow: f32, lift: f32) {
        let s = self.s() as f32;
        let body = size as f32 * s;
        let box_px = (body * grow / crate::icons::BODY).round() as i32;
        let cx = x as f32 * s + body * 0.5;
        let cy = y as f32 * s + body * 0.5 - box_px as f32 * lift;
        self.draw_icon_raw(
            (cx - box_px as f32 * 0.5).round() as i32,
            (cy - box_px as f32 * 0.5).round() as i32,
            box_px,
            icon,
        );
    }

    /// An app icon in a `size` px square. First-party roles and well-known
    /// apps use the bundled artwork; `icon` is the system theme's bitmap for
    /// anything else, set on a neutral tile; the brand and agent tiles are
    /// drawn here.
    pub fn draw_app_squircle(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        role: &str,
        icon: Option<&Icon>,
        fonts: &Fonts,
    ) {
        let s = size as f32;
        match role {
            "start" => return self.brand_tile(x, y, size, fonts),
            "agent" => return self.agent_tile(x, y, size),
            _ => {}
        }
        if let Some((ic, grow)) = crate::icons::user(role) {
            self.squircle_shadow(
                x,
                y,
                size,
                (s * 0.05).clamp(1.5, 3.0),
                (s * 0.13).clamp(3.0, 7.0),
                52,
            );
            self.draw_icon_body(x, y, size, ic, grow, 0.0);
            self.squircle_specular_finish(x, y, size);
            return;
        }
        if let Some(name) = crate::icons::name_for(role) {
            if let Some(ic) = crate::icons::get(name) {
                let (grow, lift) = crate::icons::fit(name);
                // Soft per-icon drop shadow onto the Liquid Glass dock + layered specular finish.
                self.squircle_shadow(
                    x,
                    y,
                    size,
                    (s * 0.05).clamp(1.5, 3.0),
                    (s * 0.13).clamp(3.0, 7.0),
                    52,
                );
                self.draw_icon_body(x, y, size, ic, grow, lift);
                if name != "trash" && name != "downloads" {
                    self.squircle_specular_finish(x, y, size);
                }
                return;
            }
        }

        self.squircle_shadow(
            x,
            y,
            size,
            (s * 0.04).clamp(1.0, 2.0),
            (s * 0.10).clamp(2.5, 5.0),
            40,
        );
        self.squircle_vgrad(
            x,
            y,
            size,
            Rgba::rgb(0xfb, 0xfb, 0xfd),
            Rgba::rgb(0xdd, 0xe1, 0xe8),
            0.0,
        );
        if let Some(ic) = icon {
            let pad = (s * 0.14).round() as i32;
            self.draw_icon(x + pad, y + pad, size - pad * 2, ic);
        } else {
            let (w, h) = ((s * 0.52).round() as i32, (s * 0.40).round() as i32);
            let (wx, wy) = (x + (size - w) / 2, y + (size - h) / 2);
            self.round_rect_vgrad(
                wx,
                wy,
                w,
                h,
                (s * 0.07).clamp(2.0, 4.0),
                Rgba::rgb(0x8a, 0x92, 0xa0),
                Rgba::rgb(0x5e, 0x65, 0x72),
            );
            self.fill_rect(wx, wy + (h / 4).max(3), w, 1, Rgba::rgba(255, 255, 255, 70));
        }
        self.squircle_glass_rim(
            x,
            y,
            size,
            Rgba::rgba(255, 255, 255, 190),
            Rgba::rgba(255, 255, 255, 60),
            Rgba::rgba(0, 0, 0, 30),
        );
    }

    /// macOS 27 layered specular squircle lighting: delicate curved top-left
    /// catch-light and subtle bottom rim reflection on app icon tiles.
    fn squircle_specular_finish(&mut self, x: i32, y: i32, size: i32) {
        if size < 24 {
            return;
        }
        let r = (size as f32 * 0.225).max(4.0);
        self.rim_light(
            x,
            y,
            size,
            size,
            r,
            1.3,
            Rgba::rgba(255, 255, 255, 118),
            0.35,
        );
    }

    /// The product mark: white `nL` on a black squircle.
    fn brand_tile(&mut self, x: i32, y: i32, size: i32, fonts: &Fonts) {
        let s = size as f32;
        self.squircle_shadow(
            x,
            y,
            size,
            (s * 0.04).clamp(1.0, 2.0),
            (s * 0.10).clamp(2.5, 5.0),
            56,
        );
        self.squircle_vgrad(
            x,
            y,
            size,
            Rgba::rgb(0x1c, 0x1c, 0x1f),
            Rgba::rgb(0x03, 0x03, 0x04),
            0.0,
        );
        self.squircle_glass_rim(
            x,
            y,
            size,
            Rgba::rgba(255, 255, 255, 70),
            Rgba::rgba(255, 255, 255, 22),
            Rgba::rgba(255, 255, 255, 10),
        );
        let font_px = (s * 0.42).clamp(9.0, 21.0);
        let tw = text_width(fonts.semibold.as_ref(), "nL", font_px);
        self.text(
            fonts.semibold.as_ref(),
            "nL",
            x + (size - tw) / 2,
            y + size / 2 + (font_px * 0.36) as i32,
            font_px,
            Rgba::rgb(255, 255, 255),
        );
    }

    /// The agent shelf: a lit sapphire orb in a graphite squircle.
    fn agent_tile(&mut self, x: i32, y: i32, size: i32) {
        let s = size as f32;
        let (cx, cy) = (x as f32 + s * 0.5, y as f32 + s * 0.5);
        self.squircle_shadow(
            x,
            y,
            size,
            (s * 0.04).clamp(1.0, 2.0),
            (s * 0.10).clamp(2.5, 5.0),
            56,
        );
        self.squircle_vgrad(
            x,
            y,
            size,
            Rgba::rgb(0x2b, 0x2f, 0x38),
            Rgba::rgb(0x0a, 0x0c, 0x10),
            0.0,
        );
        self.circle_radial(
            cx,
            cy,
            s * 0.46,
            (0.0, 0.0),
            Rgba::rgba(0x2f, 0x9b, 0xff, 70),
            Rgba::rgba(0x2f, 0x9b, 0xff, 0),
        );
        let r = s * 0.27;
        self.circle_radial(
            cx,
            cy,
            r,
            (-0.38, -0.44),
            Rgba::rgb(0xd6, 0xf0, 0xff),
            Rgba::rgb(0x06, 0x56, 0xdc),
        );
        self.ring(
            cx,
            cy,
            r - 0.4,
            (s * 0.03).clamp(0.8, 1.4),
            Rgba::rgba(255, 255, 255, 70),
        );
        self.circle(
            cx - r * 0.34,
            cy - r * 0.40,
            r * 0.20,
            Rgba::rgba(255, 255, 255, 120),
        );
        self.squircle_glass_rim(
            x,
            y,
            size,
            Rgba::rgba(255, 255, 255, 80),
            Rgba::rgba(255, 255, 255, 24),
            Rgba::rgba(255, 255, 255, 10),
        );
    }
}

/// Width of [`Canvas::mark_nl`] for `h`, without drawing.
pub(crate) fn nl_mark_width(h: i32, fonts: &Fonts) -> i32 {
    nl_metrics(h, fonts).0
}

/// `(width, font size, text width)` for the nL box at height `h`.
fn nl_metrics(h: i32, fonts: &Fonts) -> (i32, f32, i32) {
    let h = h.max(16);
    let pad_x = (h as f32 * 0.34).round() as i32;
    let size = (h as f32 * 0.56).clamp(11.0, 28.0);
    let tw = text_width(fonts.semibold.as_ref(), "nL", size).max(1);
    // Floor keeps the menu-bar mark (h ≈ 22) inside about 36–44 px.
    let w = (tw + pad_x * 2).max((h as f32 * 1.72).round() as i32);
    (w, size, tw)
}

pub fn text_width(font: Option<&FontRef<'_>>, value: &str, size: f32) -> i32 {
    text_width_tracked(font, value, size, 0.0)
}

pub fn text_width_tracked(
    font: Option<&FontRef<'_>>,
    value: &str,
    size: f32,
    tracking: f32,
) -> i32 {
    let Some(font) = font else {
        return 0;
    };
    let scaled = font.as_scaled(PxScale::from(size));
    let mut x = 0.0f32;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    let count = value.chars().count();
    for (idx, c) in value.chars().enumerate() {
        let gid = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, gid);
        }
        x += scaled.h_advance(gid);
        if idx + 1 < count {
            x += tracking;
        }
        prev = Some(gid);
    }
    x.round() as i32
}

/// Greedy word wrap. `measure` returns the pixel width of a string, so the
/// logic is testable without a font. Over-long words are split by character;
/// if the text needs more than `max_lines`, the last line ends in `…`.
pub fn wrap_with(
    text: &str,
    max_w: i32,
    max_lines: usize,
    measure: impl Fn(&str) -> i32,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut overflowed = false;

    let push_line = |line: String, lines: &mut Vec<String>| -> bool {
        if lines.len() >= max_lines {
            return false;
        }
        lines.push(line);
        true
    };

    'words: for word in text.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if measure(&candidate) <= max_w {
            cur = candidate;
            continue;
        }
        if !cur.is_empty() && !push_line(std::mem::take(&mut cur), &mut lines) {
            overflowed = true;
            break 'words;
        }
        // `word` alone may still be too wide.
        let mut piece = String::new();
        for ch in word.chars() {
            let mut next = piece.clone();
            next.push(ch);
            if measure(&next) > max_w && !piece.is_empty() {
                if !push_line(std::mem::take(&mut piece), &mut lines) {
                    overflowed = true;
                    break 'words;
                }
                piece.push(ch);
            } else {
                piece = next;
            }
        }
        cur = piece;
    }
    if !overflowed && !cur.is_empty() && !push_line(cur, &mut lines) {
        overflowed = true;
    }
    if overflowed {
        if let Some(last) = lines.last_mut() {
            while !last.is_empty() && measure(&format!("{last}…")) > max_w {
                last.pop();
            }
            last.push('…');
        }
    }
    lines
}

pub fn wrap_text(
    font: Option<&FontRef<'_>>,
    text: &str,
    size: f32,
    max_w: i32,
    max_lines: usize,
) -> Vec<String> {
    wrap_with(text, max_w, max_lines, |s| text_width(font, s, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> i32 {
        s.chars().count() as i32 * 10
    }

    #[test]
    fn wraps_on_word_boundaries() {
        let l = wrap_with("aaa bbb ccc ddd", 70, 5, m);
        assert_eq!(l, vec!["aaa bbb", "ccc ddd"]);
    }

    #[test]
    fn splits_a_word_wider_than_the_line() {
        let l = wrap_with("abcdefghij", 40, 5, m);
        assert_eq!(l, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn truncates_with_an_ellipsis_that_still_fits() {
        let l = wrap_with("one two three four five six", 70, 2, m);
        assert_eq!(l.len(), 2);
        let last = l.last().unwrap();
        assert!(last.ends_with('…'));
        assert!(m(last) <= 70, "{last:?} too wide");
    }

    #[test]
    fn empty_and_exact_fit() {
        assert!(wrap_with("   ", 100, 3, m).is_empty());
        assert_eq!(wrap_with("abcd efgh", 90, 3, m), vec!["abcd efgh"]);
        assert_eq!(wrap_with("a b", 100, 0, m), Vec::<String>::new());
    }

    #[test]
    fn nl_mark_is_a_black_box_in_the_bar_band() {
        let fonts = Fonts::load();
        let mut buf = vec![0u8; 96 * 40 * 4];
        let drawn = {
            let mut cv = Canvas::new(&mut buf, 96, 40, 1);
            let w = cv.mark_nl(8, 8, 22, &fonts);
            assert_eq!(w, nl_mark_width(22, &fonts));
            w
        };
        assert!(
            (36..=44).contains(&drawn),
            "bar mark width {drawn} px, want 36–44"
        );
        // Left padding of the box is opaque black (BGRA).
        let i = ((18u32 * 96 + 12) * 4) as usize;
        assert_eq!(&buf[i..i + 4], &[0, 0, 0, 255]);
    }
}
