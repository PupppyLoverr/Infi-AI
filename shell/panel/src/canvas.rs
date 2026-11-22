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
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Rgba::rgba(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2), m(a.3, b.3))
}

#[inline]
fn add_light(c: Rgba, boost: f32) -> Rgba {
    let b = boost.clamp(0.0, 1.0);
    let lift = |ch: u8| (ch as f32 + (255.0 - ch as f32) * b).round().clamp(0.0, 255.0) as u8;
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

    #[allow(clippy::too_many_arguments)]
    pub fn circle_vgrad(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        top: Rgba,
        bottom: Rgba,
        sheen: f32,
    ) {
        let sf = self.s() as f32;
        self.circle_vgrad_raw(cx * sf, cy * sf, radius * sf, top, bottom, sheen);
    }

    pub fn circle_shadow(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        dy: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        let sf = self.s() as f32;
        self.circle_shadow_raw(cx * sf, cy * sf, radius * sf, dy * sf, spread * sf, max_alpha);
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
        let adv =
            self.text_tracked_raw(font, value, x * s, baseline * s, size * sf, tracking * sf, color);
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
        let adv =
            self.text_tracked_raw(font, value, x * s, baseline * s, size * sf, tracking * sf, color);
        (adv as f32 / sf).round() as i32
    }

    pub fn draw_icon(&mut self, x: i32, y: i32, box_px: i32, icon: &Icon) {
        let s = self.s();
        self.draw_icon_raw(x * s, y * s, box_px * s, icon);
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
        let pad = spread.ceil() as i32 + 2;
        let x0 = (x - pad).max(0);
        let y0 = (y + dy.floor() as i32 - pad).max(0);
        let x1 = (x + w + pad).min(self.pw as i32);
        let y1 = (y + h + dy.ceil() as i32 + pad).min(self.ph as i32);
        let col = Rgba::rgba(0, 0, 0, max_alpha);

        for py in y0..y1 {
            let qy = (py as f32 + 0.5 - cy).abs() - (hh - r);
            for px in x0..x1 {
                let qx = (px as f32 + 0.5 - cx).abs() - (hw - r);
                let sdf = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                if sdf >= spread {
                    continue;
                }
                let u = ((sdf + 1.0) / spread).clamp(0.0, 1.0);
                let falloff = (1.0 - u) * (1.0 - u) * (1.0 - u * 0.35);
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

    #[allow(clippy::too_many_arguments)]
    fn circle_vgrad_raw(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        top: Rgba,
        bottom: Rgba,
        sheen: f32,
    ) {
        let x0 = (cx - radius - 2.0).floor().max(0.0) as i32;
        let y0 = (cy - radius - 2.0).floor().max(0.0) as i32;
        let x1 = (cx + radius + 2.0).ceil().min(self.pw as f32) as i32;
        let y1 = (cy + radius + 2.0).ceil().min(self.ph as f32) as i32;
        let inv_r = 1.0 / radius.max(1e-3);
        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            let ty = ((dy * inv_r + 1.0) * 0.5).clamp(0.0, 1.0);
            let row_col = lerp_rgba(top, bottom, ty);
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let d = dx.hypot(dy) - radius;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let col = if sheen > 0.001 {
                    let lx = dx * inv_r + 0.25;
                    let ly = dy * inv_r + 0.58;
                    let glow = (-(lx * lx + ly * ly) * 1.9).exp() * sheen;
                    add_light(row_col, glow)
                } else {
                    row_col
                };
                self.blend(px, py, col, cov);
            }
        }
    }

    fn circle_shadow_raw(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        dy_off: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        if spread <= 0.5 || max_alpha == 0 {
            return;
        }
        let cy = cy + dy_off;
        let pad = radius + spread + 2.0;
        let x0 = (cx - pad).floor().max(0.0) as i32;
        let y0 = (cy - pad).floor().max(0.0) as i32;
        let x1 = (cx + pad).ceil().min(self.pw as f32) as i32;
        let y1 = (cy + pad).ceil().min(self.ph as f32) as i32;
        let col = Rgba::rgba(0, 0, 0, max_alpha);
        for py in y0..y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let sdf = dx.hypot(dy) - radius;
                if sdf >= spread {
                    continue;
                }
                let u = ((sdf + 1.0) / spread).clamp(0.0, 1.0);
                let falloff = (1.0 - u) * (1.0 - u);
                if falloff > 0.005 {
                    self.blend(px, py, col, falloff);
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
                    let sky = lerp_rgba(
                        Rgba::rgb(0x0f, 0x3b, 0x5c),
                        Rgba::rgb(0x28, 0x76, 0x8a),
                        t,
                    );
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
                    let sea = lerp_rgba(
                        Rgba::rgb(0x1f, 0x7a, 0x7a),
                        Rgba::rgb(0x0a, 0x2c, 0x3d),
                        t,
                    );
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
        self.line(cx - u * 0.55, cy + u * 0.15, cx + u * 0.65, cy - u * 0.45, 1.4, color);
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
        self.round_rect_glass_rim(
            x,
            y,
            w,
            h,
            r,
            Rgba::rgba(255, 255, 255, 58),
            Rgba::rgba(255, 255, 255, 28),
            Rgba::rgba(255, 255, 255, 14),
        );
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
        self.stroke_round_rect(x, y, w, h, 3.0, 1.15, Rgba::rgba(color.0, color.1, color.2, 200));
        self.round_rect(x + w + 1, y + 3, 2, 4, 1.0, Rgba::rgba(color.0, color.1, color.2, 180));
        let fill_w = (((w - 4) as f32) * (pct.min(100) as f32 / 100.0)).round() as i32;
        if fill_w > 0 {
            self.round_rect(x + 2, y + 2, fill_w, h - 4, 1.5, fill_col);
        }
    }

    /// macOS Control Centre twin pill-switches icon.
    pub fn icon_control_centre(&mut self, cx: f32, cy: f32, color: Rgba) {
        let x = cx.round() as i32 - 7;
        let y = cy.round() as i32 - 6;
        self.stroke_round_rect(x, y, 14, 5, 2.5, 1.2, color);
        self.circle(x as f32 + 11.2, y as f32 + 2.5, 1.8, color);
        self.stroke_round_rect(x, y + 7, 14, 5, 2.5, 1.2, color);
        self.circle(x as f32 + 2.8, y as f32 + 9.5, 1.8, color);
    }

    pub fn icon_moon(&mut self, cx: f32, cy: f32, color: Rgba) {
        self.arc(cx + 0.5, cy, 4.8, 1.6, 55.0, 305.0, color);
    }

    pub fn icon_sun(&mut self, cx: f32, cy: f32, color: Rgba) {
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
        let x = cx.round() as i32 - 5;
        let y = cy.round() as i32 - 1;
        self.stroke_round_rect(x, y, 10, 7, 2.0, 1.3, color);
        self.arc(cx, y as f32, 3.0, 1.3, -180.0, 0.0, color);
    }

    #[allow(dead_code)]
    pub fn icon_power(&mut self, cx: f32, cy: f32, color: Rgba) {
        self.arc(cx, cy + 0.5, 5.0, 1.5, -55.0, 235.0, color);
        self.line(cx, cy - 5.5, cx, cy - 0.5, 1.5, color);
    }

    // ------------------------------------------------------------ app tiles

    /// Subtle luminous tint reflection cast beneath an app icon on a Liquid
    /// Glass dock floor.
    pub fn draw_dock_reflection(&mut self, x: i32, y: i32, size: i32, role: &str) {
        let glow = role_reflection(role);
        let rw = (size as f32 * 0.72).round() as i32;
        let rx = x + (size - rw) / 2;
        let ry = y + size + 1;
        self.round_rect(rx, ry, rw, 3, 1.5, glow);
    }

    /// Apple-grade continuous-curvature app squircle: superellipse silhouette,
    /// analytical soft shadow, multi-stop gradient, subtle radial luminosity,
    /// directional glass rim, and layered dimensional vector artwork.
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
        let (cx, cy) = (x as f32 + s * 0.5, y as f32 + s * 0.5);

        // Soft analytical ambient + contact shadow grounding the tile.
        let spread = (s * 0.14).clamp(3.0, 7.0);
        self.squircle_shadow(x, y, size, (s * 0.055).clamp(1.2, 3.0), spread, 72);

        if role == "start" {
            // Product brand mark: white `nL` on pure black continuous squircle.
            self.squircle_vgrad(
                x,
                y,
                size,
                Rgba::rgb(0x18, 0x18, 0x1b),
                Rgba::rgb(0x04, 0x04, 0x06),
                0.10,
            );
            self.squircle_glass_rim(
                x,
                y,
                size,
                Rgba::rgba(255, 255, 255, 92),
                Rgba::rgba(255, 255, 255, 34),
                Rgba::rgba(255, 255, 255, 14),
            );
            let font_px = (s * 0.42).clamp(9.0, 21.0);
            let tw = text_width(fonts.semibold.as_ref(), "nL", font_px);
            let baseline = y + size / 2 + (font_px * 0.36) as i32;
            self.text(
                fonts.semibold.as_ref(),
                "nL",
                x + (size - tw) / 2,
                baseline,
                font_px,
                Rgba::rgb(255, 255, 255),
            );
            return;
        }

        // Core first-party roles always render bespoke vector art so legacy
        // system bitmaps (such as XFCE's Thunar hammer) never degrade the dock.
        let first_party = matches!(
            role,
            "files"
                | "thunar"
                | "terminal"
                | "foot"
                | "solyra-term"
                | "firefox"
                | "browser"
                | "settings"
                | "overview"
                | "launcher"
                | "agent"
                | "trash"
                | "wallpaper"
                | "gedit"
                | "editor"
        );

        if !first_party {
            if let Some(ic) = icon {
                self.squircle_vgrad(
                    x,
                    y,
                    size,
                    Rgba::rgb(0xf6, 0xf7, 0xf9),
                    Rgba::rgb(0xdc, 0xe0, 0xe7),
                    0.12,
                );
                self.squircle_glass_rim(
                    x,
                    y,
                    size,
                    Rgba::rgba(255, 255, 255, 165),
                    Rgba::rgba(255, 255, 255, 65),
                    Rgba::rgba(0, 0, 0, 34),
                );
                let pad = (s * 0.14).round() as i32;
                self.draw_icon(x + pad, y + pad, size - pad * 2, ic);
                return;
            }
        }

        let (top_col, bot_col, sheen) = role_gradient(role);
        self.squircle_vgrad(x, y, size, top_col, bot_col, sheen);

        let u = s * 0.22;
        match role {
            "files" | "thunar" => {
                // Apple Files / Finder-inspired layered sky-blue glass folder
                // on a luminous pearl-silver continuous squircle.
                let fw = (s * 0.64).round() as i32;
                let fh = (s * 0.44).round() as i32;
                let fx = x + (size - fw) / 2;
                let fy = y + (size - fh) / 2 + (s * 0.04).round() as i32;
                let fr = (s * 0.085).clamp(2.5, 5.0);
                // Soft shadow under the folder.
                self.round_rect_shadow(fx, fy, fw, fh, fr, 1.5, 3.5, 54);
                // Back folder tab & upper rear body.
                let tab_w = (fw as f32 * 0.44).round() as i32;
                let tab_h = (s * 0.11).round().max(3.0) as i32;
                self.round_rect_vgrad(
                    fx,
                    fy - tab_h + 2,
                    tab_w,
                    tab_h + 4,
                    (fr * 0.7).max(1.8),
                    Rgba::rgb(0x1f, 0x8d, 0xff),
                    Rgba::rgb(0x00, 0x5c, 0xd1),
                );
                self.round_rect_vgrad(
                    fx,
                    fy - 2,
                    fw,
                    fh,
                    fr,
                    Rgba::rgb(0x12, 0x7a, 0xf2),
                    Rgba::rgb(0x00, 0x51, 0xbf),
                );
                // Crisp inner paper sheet edge peeking out.
                let paper_inset = (s * 0.06).round().max(2.0) as i32;
                self.round_rect(
                    fx + paper_inset,
                    fy + 1,
                    (fw - paper_inset * 2).max(2),
                    (fh / 3).max(3),
                    (fr * 0.5).max(1.0),
                    Rgba::rgba(0xff, 0xff, 0xff, 232),
                );
                // Sculpted luminous front folder flap.
                let flap_y = fy + (s * 0.085).round().max(3.0) as i32;
                let flap_h = (fh - (flap_y - fy)).max(6);
                self.round_rect_vgrad(
                    fx,
                    flap_y,
                    fw,
                    flap_h,
                    fr,
                    Rgba::rgb(0x5c, 0xc8, 0xff),
                    Rgba::rgb(0x0a, 0x7e, 0xf6),
                );
                self.round_rect_glass_rim(
                    fx,
                    flap_y,
                    fw,
                    flap_h,
                    fr,
                    Rgba::rgba(255, 255, 255, 165),
                    Rgba::rgba(255, 255, 255, 45),
                    Rgba::rgba(0, 40, 110, 50),
                );
            }
            "terminal" | "foot" | "solyra-term" => {
                // Brushed titanium bezel framing a recessed deep obsidian screen.
                let inset = (s * 0.085).round().max(2.0) as i32;
                let inner = size - inset * 2;
                self.squircle_vgrad(
                    x + inset,
                    y + inset,
                    inner,
                    Rgba::rgb(0x1c, 0x1e, 0x24),
                    Rgba::rgb(0x09, 0x0a, 0x0d),
                    0.07,
                );
                self.squircle_glass_rim(
                    x + inset,
                    y + inset,
                    inner,
                    Rgba::rgba(0, 0, 0, 120),
                    Rgba::rgba(255, 255, 255, 22),
                    Rgba::rgba(255, 255, 255, 58),
                );
                // Subtle green phosphor ambient glow behind prompt.
                self.circle(
                    cx + u * 0.30,
                    cy + u * 0.45,
                    u * 0.95,
                    Rgba::rgba(0x34, 0xd3, 0x99, 28),
                );
                // Crisp chevron `>` in snow-white + glowing emerald underscore `_`.
                let sw = (s * 0.056).clamp(1.5, 2.6);
                let px0 = cx - u * 0.92;
                let px1 = cx - u * 0.12;
                self.line(px0, cy - u * 0.56, px1, cy - u * 0.04, sw, Rgba::rgb(0xf5, 0xf7, 0xfa));
                self.line(px1, cy - u * 0.04, px0, cy + u * 0.48, sw, Rgba::rgb(0xf5, 0xf7, 0xfa));
                self.line(
                    cx + u * 0.14,
                    cy + u * 0.52,
                    cx + u * 0.92,
                    cy + u * 0.52,
                    sw,
                    Rgba::rgb(0x34, 0xd3, 0x99),
                );
            }
            "overview" | "launcher" => {
                // Emerald-to-teal Workspaces / Mission Control icon (zero purple):
                // three crisp layered translucent workspace cards with depth.
                let cw = (s * 0.34).round().max(6.0) as i32;
                let ch = (s * 0.25).round().max(5.0) as i32;
                let cr = (s * 0.065).clamp(2.0, 4.0);
                // Back-left card.
                let lx = (cx - s * 0.30).round() as i32;
                let ly = (cy - s * 0.22).round() as i32;
                self.round_rect_shadow(lx, ly, cw, ch, cr, 1.2, 2.8, 48);
                self.round_rect_vgrad(
                    lx,
                    ly,
                    cw,
                    ch,
                    cr,
                    Rgba::rgba(255, 255, 255, 232),
                    Rgba::rgba(225, 250, 245, 195),
                );
                // Back-right card.
                let rx = (cx + s * 0.01).round() as i32;
                let ry = (cy - s * 0.16).round() as i32;
                self.round_rect_shadow(rx, ry, cw, ch, cr, 1.2, 2.8, 48);
                self.round_rect_vgrad(
                    rx,
                    ry,
                    cw,
                    ch,
                    cr,
                    Rgba::rgba(255, 255, 255, 210),
                    Rgba::rgba(210, 245, 240, 175),
                );
                // Front focal active workspace card.
                let fw = (s * 0.46).round().max(8.0) as i32;
                let fh = (s * 0.29).round().max(6.0) as i32;
                let fx = (cx - fw as f32 * 0.5).round() as i32;
                let fy = (cy + s * 0.02).round() as i32;
                self.round_rect_shadow(fx, fy, fw, fh, cr, 1.6, 3.6, 72);
                self.round_rect_vgrad(
                    fx,
                    fy,
                    fw,
                    fh,
                    cr,
                    Rgba::rgb(0xff, 0xff, 0xff),
                    Rgba::rgb(0xe2, 0xf6, 0xf2),
                );
                self.round_rect_glass_rim(
                    fx,
                    fy,
                    fw,
                    fh,
                    cr,
                    Rgba::rgba(255, 255, 255, 220),
                    Rgba::rgba(255, 255, 255, 90),
                    Rgba::rgba(0, 70, 65, 40),
                );
            }
            "settings" => {
                // Machined precision aluminum System Settings gear.
                let gr = s * 0.30;
                // Recessed shadow disc under the gear.
                self.circle_shadow(cx, cy, gr * 0.96, 1.5, 3.8, 76);
                // 8 precision gear teeth radiating cleanly.
                let tooth_w = (s * 0.095).clamp(2.2, 4.4);
                for i in 0..8 {
                    let a = (i as f32) * std::f32::consts::FRAC_PI_4;
                    self.line(
                        cx + a.cos() * (gr * 0.55),
                        cy + a.sin() * (gr * 0.55),
                        cx + a.cos() * (gr * 1.02),
                        cy + a.sin() * (gr * 1.02),
                        tooth_w,
                        Rgba::rgb(0x3a, 0x3f, 0x47),
                    );
                }
                // Dark anodized outer gear wheel.
                self.circle_vgrad(
                    cx,
                    cy,
                    gr * 0.84,
                    Rgba::rgb(0x4b, 0x51, 0x5c),
                    Rgba::rgb(0x25, 0x28, 0x2e),
                    0.18,
                );
                // Brushed inner metallic ring.
                self.ring(cx, cy, gr * 0.58, (s * 0.045).clamp(1.2, 2.2), Rgba::rgba(255, 255, 255, 135));
                // Inner hub cavity & machined steel center.
                self.circle_vgrad(
                    cx,
                    cy,
                    gr * 0.36,
                    Rgba::rgb(0xd4, 0xd8, 0xe0),
                    Rgba::rgb(0x8c, 0x93, 0x9e),
                    0.22,
                );
                self.circle(cx, cy, gr * 0.16, Rgba::rgb(0x33, 0x37, 0x3f));
            }
            "firefox" | "browser" => {
                // Sculpted vibrant sunset-coral compass / browser icon.
                let dr = s * 0.34;
                self.circle_shadow(cx, cy, dr, 1.4, 3.6, 68);
                // Luminous translucent glass compass dial.
                self.circle_vgrad(
                    cx,
                    cy,
                    dr,
                    Rgba::rgba(255, 255, 255, 245),
                    Rgba::rgba(255, 238, 228, 225),
                    0.18,
                );
                // Inner ocean-coral recessed dial face.
                self.circle_vgrad(
                    cx,
                    cy,
                    dr * 0.82,
                    Rgba::rgb(0x29, 0x9d, 0xf2),
                    Rgba::rgb(0x00, 0x5a, 0xc7),
                    0.22,
                );
                // Fine cardinal compass ticks.
                for i in 0..8 {
                    let a = (i as f32) * std::f32::consts::FRAC_PI_4;
                    let inner = if i % 2 == 0 { dr * 0.56 } else { dr * 0.65 };
                    self.line(
                        cx + a.cos() * inner,
                        cy + a.sin() * inner,
                        cx + a.cos() * (dr * 0.76),
                        cy + a.sin() * (dr * 0.76),
                        (s * 0.026).clamp(0.9, 1.5),
                        Rgba::rgba(255, 255, 255, 185),
                    );
                }
                // Sleek two-tone compass needle pointing NE.
                let nl = dr * 0.68;
                let nw = dr * 0.22;
                let dx = nl * 0.707;
                let dy = -nl * 0.707;
                let px = nw * 0.707;
                let py = nw * 0.707;
                // Coral-red north needle half.
                self.rounded_triangle(
                    (cx + dx, cy + dy),
                    (cx - px, cy - py),
                    (cx + px, cy + py),
                    0.6,
                    Rgba::rgb(0xff, 0x4f, 0x3b),
                );
                // Pure white south needle half.
                self.rounded_triangle(
                    (cx - dx, cy - dy),
                    (cx - px, cy - py),
                    (cx + px, cy + py),
                    0.6,
                    Rgba::rgb(0xff, 0xff, 0xff),
                );
                self.circle(cx, cy, (s * 0.045).clamp(1.2, 2.2), Rgba::rgb(0xe8, 0xec, 0xf2));
            }
            "agent" => {
                // Luminous sapphire Solyra eclipse orb (zero purple).
                let or = s * 0.30;
                // Soft radiant halo behind the orb.
                self.circle(cx, cy, or * 1.25, Rgba::rgba(0x70, 0xd6, 0xff, 52));
                self.ring(
                    cx,
                    cy,
                    or,
                    (s * 0.055).clamp(1.5, 2.6),
                    Rgba::rgba(255, 255, 255, 232),
                );
                self.arc(
                    cx,
                    cy,
                    or * 0.64,
                    (s * 0.042).clamp(1.2, 2.0),
                    -145.0,
                    35.0,
                    Rgba::rgba(0x9b, 0xe7, 0xff, 210),
                );
                // Radiant inner eclipse jewel.
                self.circle_vgrad(
                    cx + or * 0.24,
                    cy - or * 0.22,
                    or * 0.38,
                    Rgba::rgb(0xff, 0xff, 0xff),
                    Rgba::rgb(0xa8, 0xe2, 0xff),
                    0.30,
                );
            }
            "trash" => {
                // Sculpted brushed-silver macOS wastebasket cylinder.
                let bw = (s * 0.44).round().max(10.0) as i32;
                let bh = (s * 0.46).round().max(10.0) as i32;
                let bx = (cx - bw as f32 * 0.5).round() as i32;
                let by = (cy - bh as f32 * 0.44).round() as i32;
                let br = (s * 0.08).clamp(2.5, 4.5);
                self.round_rect_shadow(bx, by, bw, bh, br, 1.5, 3.2, 65);
                // Brushed aluminum basket body.
                self.round_rect_vgrad(
                    bx,
                    by,
                    bw,
                    bh,
                    br,
                    Rgba::rgb(0xe5, 0xe8, 0xee),
                    Rgba::rgb(0xa5, 0xab, 0xb6),
                );
                // Dark open rim top lip.
                let lid_w = bw + 4;
                let lid_h = (s * 0.11).round().max(3.0) as i32;
                self.round_rect_vgrad(
                    bx - 2,
                    by - lid_h / 2,
                    lid_w,
                    lid_h,
                    (lid_h as f32 * 0.48).max(1.5),
                    Rgba::rgb(0xf6, 0xf8, 0xfa),
                    Rgba::rgb(0xb8, 0xbe, 0xc8),
                );
                self.round_rect(
                    bx + 1,
                    by - lid_h / 2 + 1,
                    (bw - 2).max(2),
                    (lid_h - 2).max(2),
                    1.5,
                    Rgba::rgba(0x3a, 0x3e, 0x46, 170),
                );
                // Subtle vertical fluted ribs.
                let rib_top = by as f32 + lid_h as f32 * 0.72;
                let rib_bot = (by + bh) as f32 - 3.0;
                for i in -1..=1 {
                    let rx = cx + i as f32 * (bw as f32 * 0.24);
                    self.line(
                        rx,
                        rib_top,
                        rx,
                        rib_bot,
                        (s * 0.032).clamp(1.0, 1.6),
                        Rgba::rgba(0x54, 0x59, 0x64, 130),
                    );
                }
            }
            "gedit" | "editor" => {
                // macOS Notes / TextEdit craft: crisp ivory page with warm
                // amber header band and delicate ruled text lines.
                let hdr_h = (s * 0.24).round().max(6.0) as i32;
                self.round_rect_vgrad(
                    x + 2,
                    y + 2,
                    size - 4,
                    hdr_h,
                    (s * 0.18).clamp(4.0, 9.0),
                    Rgba::rgb(0xff, 0xc7, 0x45),
                    Rgba::rgb(0xf2, 0x9e, 0x16),
                );
                let line_l = cx - u * 1.15;
                let line_r = cx + u * 1.15;
                for i in 0..3 {
                    let ly = cy + (i as f32 - 0.35) * (u * 0.75);
                    let r_end = if i == 2 { cx + u * 0.45 } else { line_r };
                    self.line(
                        line_l,
                        ly,
                        r_end,
                        ly,
                        (s * 0.042).clamp(1.2, 2.0),
                        Rgba::rgba(0x78, 0x80, 0x8d, 150),
                    );
                }
            }
            "wallpaper" => {
                // Coastal sky + warm sun + layered waves.
                self.circle(cx + u * 0.48, cy - u * 0.42, u * 0.46, Rgba::rgb(0xff, 0xe2, 0x99));
                self.circle(cx - u * 0.45, cy + u * 0.85, u * 1.15, Rgba::rgba(0x1f, 0x9d, 0xb3, 210));
                self.circle(cx + u * 0.55, cy + u * 1.05, u * 1.25, Rgba::rgba(0x0c, 0x62, 0x8a, 225));
            }
            _ => {
                // Clean neutral app window glyph (zero purple).
                let (w, h) = ((s * 0.56).round() as i32, (s * 0.44).round() as i32);
                let (wx, wy) = ((cx - w as f32 * 0.5).round() as i32, (cy - h as f32 * 0.5).round() as i32);
                let wr = (s * 0.08).clamp(2.0, 4.0);
                self.round_rect_shadow(wx, wy, w, h, wr, 1.2, 3.0, 56);
                self.round_rect_vgrad(
                    wx,
                    wy,
                    w,
                    h,
                    wr,
                    Rgba::rgb(0xff, 0xff, 0xff),
                    Rgba::rgb(0xdc, 0xe1, 0xe8),
                );
                self.fill_rect(wx + 2, wy + (h / 4).max(3), w - 4, 1, Rgba::rgba(0, 0, 0, 40));
            }
        }

        // Optical directional Liquid Glass outer rim on every squircle.
        let (rim_top, rim_side, rim_bot) = if matches!(role, "files" | "thunar" | "gedit" | "editor")
        {
            (
                Rgba::rgba(255, 255, 255, 215),
                Rgba::rgba(255, 255, 255, 85),
                Rgba::rgba(0, 0, 0, 32),
            )
        } else {
            (
                Rgba::rgba(255, 255, 255, 138),
                Rgba::rgba(255, 255, 255, 48),
                Rgba::rgba(0, 0, 0, 46),
            )
        };
        self.squircle_glass_rim(x, y, size, rim_top, rim_side, rim_bot);
    }
}

/// `(top, bottom, sheen)` multi-stop gradient palette per role — ZERO mauve/purple.
fn role_gradient(role: &str) -> (Rgba, Rgba, f32) {
    match role {
        "files" | "thunar" => (
            Rgba::rgb(0xfa, 0xfc, 0xff),
            Rgba::rgb(0xd9, 0xe3, 0xf0),
            0.14,
        ),
        "terminal" | "foot" | "solyra-term" => (
            Rgba::rgb(0x5c, 0x62, 0x6c),
            Rgba::rgb(0x28, 0x2b, 0x32),
            0.16,
        ),
        "firefox" | "browser" => (
            Rgba::rgb(0xff, 0x8a, 0x3d),
            Rgba::rgb(0xdc, 0x35, 0x22),
            0.18,
        ),
        "settings" => (
            Rgba::rgb(0xdf, 0xe3, 0xea),
            Rgba::rgb(0x9a, 0xa1, 0xad),
            0.18,
        ),
        "overview" | "launcher" => (
            // Vivid emerald-to-ocean-teal (replaces previous purple tile).
            Rgba::rgb(0x2c, 0xd4, 0x9b),
            Rgba::rgb(0x0b, 0x7b, 0x83),
            0.18,
        ),
        "agent" => (
            Rgba::rgb(0x2e, 0x9b, 0xff),
            Rgba::rgb(0x00, 0x48, 0xbd),
            0.20,
        ),
        "trash" => (
            Rgba::rgb(0x7c, 0x82, 0x8d),
            Rgba::rgb(0x41, 0x45, 0x4e),
            0.16,
        ),
        "gedit" | "editor" => (
            Rgba::rgb(0xff, 0xff, 0xff),
            Rgba::rgb(0xe6, 0xea, 0xf0),
            0.10,
        ),
        "wallpaper" => (
            Rgba::rgb(0x56, 0xc8, 0xf5),
            Rgba::rgb(0x1f, 0x6f, 0xd6),
            0.18,
        ),
        _ => (
            Rgba::rgb(0x4e, 0x53, 0x5c),
            Rgba::rgb(0x2a, 0x2d, 0x34),
            0.14,
        ),
    }
}

/// Subtle color tint reflected onto the dock glass floor beneath each icon.
fn role_reflection(role: &str) -> Rgba {
    match role {
        "files" | "thunar" => Rgba::rgba(0x38, 0xa8, 0xff, 55),
        "start" => Rgba::rgba(0x00, 0x00, 0x00, 32),
        "terminal" | "foot" | "solyra-term" => Rgba::rgba(0x34, 0xd3, 0x99, 44),
        "overview" | "launcher" => Rgba::rgba(0x20, 0xc4, 0x9c, 55),
        "settings" => Rgba::rgba(0xd0, 0xd6, 0xe0, 48),
        "firefox" | "browser" => Rgba::rgba(0xff, 0x6e, 0x34, 58),
        "agent" => Rgba::rgba(0x1e, 0x88, 0xff, 58),
        "trash" => Rgba::rgba(0x9a, 0xa0, 0xab, 40),
        _ => Rgba::rgba(0xc8, 0xd0, 0xd8, 36),
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
