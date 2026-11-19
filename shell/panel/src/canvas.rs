//! 2D software vector, typography, and icon renderer for Wayland `Argb8888`.
//!
//! On little-endian targets, `wl_shm::Format::Argb8888` stores each pixel as
//! `[B, G, R, A]` with **premultiplied** color channels. Keeping premultiplied
//! Porter-Duff `source-over` exact avoids dark fringes on anti-aliased edges
//! and lets compositor backdrop blur (`niri` `background-effect { blur true }`)
//! shine cleanly through translucent glass fills.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(r, g, b, a)
    }

    #[allow(dead_code)]
    pub fn with_alpha(self, a: u8) -> Self {
        Self(self.0, self.1, self.2, a)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self(
            mix(self.0, other.0),
            mix(self.1, other.1),
            mix(self.2, other.2),
            mix(self.3, other.3),
        )
    }
}

pub struct Fonts {
    pub regular: Option<FontRef<'static>>,
    pub semibold: Option<FontRef<'static>>,
    pub mono: Option<FontRef<'static>>,
}

impl Fonts {
    pub fn load() -> Self {
        Self {
            regular: try_load_font(&[
                "/usr/share/fonts/open-sans/OpenSans-Regular.ttf",
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]),
            semibold: try_load_font(&[
                "/usr/share/fonts/open-sans/OpenSans-Semibold.ttf",
                "/usr/share/fonts/open-sans/OpenSans-Bold.ttf",
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ]),
            mono: try_load_font(&[
                "/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf",
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

pub struct Canvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
}

impl<'a> Canvas<'a> {
    pub fn new(pixels: &'a mut [u8], width: u32, height: u32) -> Self {
        pixels.fill(0);
        Self {
            pixels,
            width,
            height,
            stride: width as usize * 4,
        }
    }

    /// Premultiplied Porter-Duff `source-over` into BGRA (`Argb8888` LE).
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, color: Rgba, coverage: f32) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
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

    /// Overwrite pixel directly with premultiplied BGRA (used to punch out a
    /// crisp shape inside an existing translucent bar).
    #[inline]
    pub fn set_opaque_blend(&mut self, x: i32, y: i32, color: Rgba, coverage: f32) {
        self.blend(x, y, color, coverage);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgba) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend(px, py, color, 1.0);
            }
        }
    }

    pub fn vgrad_rect(&mut self, x: i32, y: i32, w: i32, h: i32, top: Rgba, bottom: Rgba) {
        if h <= 0 || w <= 0 {
            return;
        }
        let denom = (h - 1).max(1) as f32;
        for row in 0..h {
            let c = top.lerp(bottom, row as f32 / denom);
            self.fill_rect(x, y + row, w, 1, c);
        }
    }

    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: f32, color: Rgba) {
        self.vgrad_round_rect(x, y, w, h, radius, color, color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn vgrad_round_rect(
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
        let half_w = w as f32 * 0.5;
        let half_h = h as f32 * 0.5;
        let cx = x as f32 + half_w;
        let cy = y as f32 + half_h;
        let denom = (h - 1).max(1) as f32;

        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);

        for py in y0..y1 {
            let color = top.lerp(bottom, (py - y) as f32 / denom);
            let qy = ((py as f32 + 0.5 - cy).abs()) - (half_h - r);
            for px in x0..x1 {
                let qx = ((px as f32 + 0.5 - cx).abs()) - (half_w - r);
                let dist = (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, color, cov);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dgrad_round_rect(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        c0: Rgba,
        c1: Rgba,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let half_w = w as f32 * 0.5;
        let half_h = h as f32 * 0.5;
        let cx = x as f32 + half_w;
        let cy = y as f32 + half_h;
        let denom = (w + h - 2).max(1) as f32;

        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);

        for py in y0..y1 {
            let qy = ((py as f32 + 0.5 - cy).abs()) - (half_h - r);
            for px in x0..x1 {
                let qx = ((px as f32 + 0.5 - cx).abs()) - (half_w - r);
                let dist = (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let t = ((px - x) + (py - y)) as f32 / denom;
                    self.blend(px, py, c0.lerp(c1, t), cov);
                }
            }
        }
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
        let x1 = (x + w + pad).min(self.width as i32);
        let y1 = (y + h + pad).min(self.height as i32);

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

    /// Glass card helper: translucent vertical gradient + 1px specular top-lit border.
    #[allow(clippy::too_many_arguments)]
    pub fn glass_card(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: f32,
        top: Rgba,
        bottom: Rgba,
        rim_top: Rgba,
        rim_bottom: Rgba,
    ) {
        self.vgrad_round_rect(x, y, w, h, radius, top, bottom);
        // Top-lit inner specular rim (stronger at top, subtler at bottom).
        if w <= 0 || h <= 0 {
            return;
        }
        let r = radius.min(w as f32 * 0.5).min(h as f32 * 0.5).max(0.0);
        let half_w = w as f32 * 0.5;
        let half_h = h as f32 * 0.5;
        let cx = x as f32 + half_w;
        let cy = y as f32 + half_h;
        let denom = (h - 1).max(1) as f32;
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        for py in y0..y1 {
            let rim = rim_top.lerp(rim_bottom, (py - y) as f32 / denom);
            let qy = ((py as f32 + 0.5 - cy).abs()) - (half_h - r);
            for px in x0..x1 {
                let qx = ((px as f32 + 0.5 - cx).abs()) - (half_w - r);
                let sdf = (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r;
                let ring_dist = (sdf + 0.5).abs() - 0.5;
                let cov = (0.5 - ring_dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, rim, cov);
                }
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: Rgba) {
        let x0 = (cx - radius - 2.0).floor().max(0.0) as i32;
        let y0 = (cy - radius - 2.0).floor().max(0.0) as i32;
        let x1 = (cx + radius + 2.0).ceil().min(self.width as f32) as i32;
        let y1 = (cy + radius + 2.0).ceil().min(self.height as f32) as i32;
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

    pub fn ring(&mut self, cx: f32, cy: f32, radius: f32, thickness: f32, color: Rgba) {
        let half = thickness * 0.5;
        let pad = radius + half + 2.0;
        let x0 = (cx - pad).floor().max(0.0) as i32;
        let y0 = (cy - pad).floor().max(0.0) as i32;
        let x1 = (cx + pad).ceil().min(self.width as f32) as i32;
        let y1 = (cy + pad).ceil().min(self.height as f32) as i32;
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

    /// Anti-aliased arc segment (`start_rad` .. `end_rad`, clockwise from +X).
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
        let half = thickness * 0.5;
        let pad = radius + half + 2.0;
        let x0 = (cx - pad).floor().max(0.0) as i32;
        let y0 = (cy - pad).floor().max(0.0) as i32;
        let x1 = (cx + pad).ceil().min(self.width as f32) as i32;
        let y1 = (cy + pad).ceil().min(self.height as f32) as i32;
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
    pub fn line(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        color: Rgba,
    ) {
        let half = thickness * 0.5;
        let min_x = (x0.min(x1) - half - 2.0).floor().max(0.0) as i32;
        let min_y = (y0.min(y1) - half - 2.0).floor().max(0.0) as i32;
        let max_x = (x0.max(x1) + half + 2.0).ceil().min(self.width as f32) as i32;
        let max_y = (y0.max(y1) + half + 2.0).ceil().min(self.height as f32) as i32;
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

    /// Smooth Apple-style top-anchored notch with concave top-shoulder fillets,
    /// anti-aliased rounded bottom corners, subtle edge rim, and optical lens.
    pub fn draw_notch(&mut self, center_x: i32, body_w: i32, h: i32, ear_r: f32, bottom_r: f32) {
        let half_w = body_w as f32 * 0.5;
        let cx = center_x as f32;
        let hf = h as f32;
        let x0 = (cx - half_w - ear_r - 2.0).floor().max(0.0) as i32;
        let x1 = (cx + half_w + ear_r + 2.0).ceil().min(self.width as f32) as i32;
        let y1 = (hf + 2.0).ceil().min(self.height as f32) as i32;
        let void = Rgba::rgb(0x06, 0x05, 0x09);
        let rim = Rgba::rgba(0xff, 0xff, 0xff, 26);

        for py in 0..y1 {
            let yf = py as f32 + 0.5;
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx).abs();
                let sdf = if yf < ear_r && dx > half_w {
                    // Concave top-shoulder fillet centered at (half_w + ear_r, ear_r).
                    let ex = half_w + ear_r - dx;
                    let ey = ear_r - yf;
                    ear_r - ex.hypot(ey)
                } else {
                    // Main notch body with rounded bottom corners.
                    let qx = dx - (half_w - bottom_r);
                    let qy = yf - (hf - bottom_r);
                    if yf <= hf - bottom_r {
                        dx - half_w
                    } else {
                        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - bottom_r
                    }
                };

                let fill_cov = (0.5 - sdf).clamp(0.0, 1.0);
                if fill_cov > 0.0 {
                    self.set_opaque_blend(px, py, void, fill_cov);
                }
                if py > 0 {
                    let rim_cov = (0.5 - (sdf + 0.5).abs()).clamp(0.0, 1.0);
                    if rim_cov > 0.0 {
                        self.blend(px, py, rim, rim_cov);
                    }
                }
            }
        }

        // Realistic multi-layer optical camera housing at the center of the notch.
        let ly = (hf * 0.46).clamp(10.0, 15.0);
        self.circle(cx, ly, 4.6, Rgba::rgb(0x13, 0x16, 0x20));
        self.circle(cx, ly, 3.2, Rgba::rgb(0x0c, 0x18, 0x2c));
        self.circle(cx, ly, 1.8, Rgba::rgb(0x18, 0x36, 0x62));
        self.circle(cx - 0.8, ly - 0.8, 0.85, Rgba::rgba(0x7c, 0xc4, 0xff, 190));
    }

    pub fn text(
        &mut self,
        font: Option<&FontRef<'_>>,
        value: &str,
        x: i32,
        baseline: i32,
        size: f32,
        color: Rgba,
    ) -> i32 {
        let Some(font) = font else {
            return 0;
        };
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let mut caret = point(x as f32, baseline as f32);
        for ch in value.chars() {
            let gid = scaled.glyph_id(ch);
            if let Some(outline) = scaled.outline_glyph(gid.with_scale_and_position(scale, caret)) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, cov| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    self.blend(px, py, color, cov);
                });
            }
            caret.x += scaled.h_advance(gid);
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
    pub fn draw_icon(&mut self, x: i32, y: i32, box_px: i32, icon: &Icon) {
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

    /// Solyra eclipse-star emblem centered at `(cx, cy)`.
    pub fn icon_solyra(&mut self, cx: f32, cy: f32, r: f32, color: Rgba) {
        self.ring(cx, cy, r, 1.6, color);
        self.circle(cx + r * 0.28, cy - r * 0.25, r * 0.34, color);
    }

    pub fn icon_search(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        let r = size * 0.34;
        self.ring(cx - size * 0.08, cy - size * 0.08, r, 1.5, color);
        self.line(
            cx + r * 0.55,
            cy + r * 0.55,
            cx + size * 0.38,
            cy + size * 0.38,
            1.6,
            color,
        );
    }

    pub fn icon_wifi(&mut self, cx: f32, cy: f32, size: f32, color: Rgba) {
        let base_y = cy + size * 0.28;
        self.circle(cx, base_y, 1.4, color);
        self.arc(cx, base_y, size * 0.28, 1.45, -135.0, -45.0, color);
        self.arc(cx, base_y, size * 0.50, 1.45, -135.0, -45.0, color);
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
        let w = 18;
        let h = 10;
        let x = cx.round() as i32 - w / 2 - 1;
        let y = cy.round() as i32 - h / 2;
        self.stroke_round_rect(x, y, w, h, 3.0, 1.2, color);
        self.round_rect(x + w + 1, y + 3, 2, 4, 1.0, color);
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
        self.circle(x as f32 + 11.2, y as f32 + 2.5, 1.7, color);
        self.stroke_round_rect(x, y + 7, 14, 5, 2.5, 1.2, color);
        self.circle(x as f32 + 2.8, y as f32 + 9.5, 1.7, color);
    }

    pub fn icon_bluetooth(&mut self, cx: f32, cy: f32, color: Rgba) {
        self.line(cx, cy - 5.5, cx, cy + 5.5, 1.4, color);
        self.line(cx, cy - 5.5, cx + 3.6, cy - 2.5, 1.35, color);
        self.line(cx + 3.6, cy - 2.5, cx - 3.4, cy + 2.8, 1.35, color);
        self.line(cx, cy + 5.5, cx + 3.6, cy + 2.5, 1.35, color);
        self.line(cx + 3.6, cy + 2.5, cx - 3.4, cy - 2.8, 1.35, color);
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

    pub fn icon_lock(&mut self, cx: f32, cy: f32, color: Rgba) {
        let x = cx.round() as i32 - 5;
        let y = cy.round() as i32 - 1;
        self.stroke_round_rect(x, y, 10, 7, 2.0, 1.3, color);
        self.arc(cx, y as f32, 3.0, 1.3, -180.0, 0.0, color);
    }

    pub fn icon_power(&mut self, cx: f32, cy: f32, color: Rgba) {
        self.arc(cx, cy + 0.5, 5.0, 1.5, -55.0, 235.0, color);
        self.line(cx, cy - 5.5, cx, cy - 0.5, 1.5, color);
    }

    // ------------------------------------------------- cohesive squircle badges

    /// Bespoke studio-grade squircle app icon (`size × size`), giving core OS
    /// apps a unified macOS/Fluent material finish instead of mismatched legacy icons.
    pub fn draw_app_squircle(&mut self, x: i32, y: i32, size: i32, role: &str, icon: Option<&Icon>) {
        let r = (size as f32 * 0.26).round();
        let cx = x as f32 + size as f32 * 0.5;
        let cy = y as f32 + size as f32 * 0.5;
        let s = size as f32;

        match role {
            "start" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0xc0, 0x84, 0xfc),
                    Rgba::rgb(0x63, 0x66, 0xf1),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 76));
                let g = s * 0.23;
                self.ring(cx, cy, g, 1.8, Rgba::rgb(255, 255, 255));
                self.circle(cx + g * 0.34, cy - g * 0.32, g * 0.36, Rgba::rgb(255, 255, 255));
            }
            "files" | "thunar" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0x38, 0xbd, 0xf8),
                    Rgba::rgb(0x25, 0x63, 0xeb),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 70));
                let fw = (s * 0.56).round() as i32;
                let fh = (s * 0.42).round() as i32;
                let fx = x + (size - fw) / 2;
                let fy = y + (size - fh) / 2 + 2;
                // Folder back tab + frosted front pocket.
                self.round_rect(fx, fy - 3, (fw as f32 * 0.46) as i32, 6, 2.5, Rgba::rgba(255, 255, 255, 210));
                self.vgrad_round_rect(
                    fx,
                    fy,
                    fw,
                    fh,
                    4.0,
                    Rgba::rgb(0xff, 0xff, 0xff),
                    Rgba::rgb(0xdb, 0xea, 0xfe),
                );
            }
            "terminal" | "foot" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0x27, 0x23, 0x38),
                    Rgba::rgb(0x11, 0x0e, 0x1a),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 48));
                // Crisp `>_` vector chevron + accent underscore.
                let px = cx - s * 0.20;
                let py = cy - s * 0.02;
                let d = s * 0.13;
                self.line(px - d * 0.5, py - d, px + d * 0.6, py, 2.0, Rgba::rgb(0xe9, 0xd5, 0xff));
                self.line(px + d * 0.6, py, px - d * 0.5, py + d, 2.0, Rgba::rgb(0xe9, 0xd5, 0xff));
                self.line(
                    cx + s * 0.02,
                    py + d,
                    cx + s * 0.22,
                    py + d,
                    2.0,
                    Rgba::rgb(0xc0, 0x84, 0xfc),
                );
            }
            "overview" | "workspaces" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0x34, 0xd3, 0x99),
                    Rgba::rgb(0x0d, 0x94, 0x88),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 68));
                let cell = (s * 0.21).round() as i32;
                let gap = (s * 0.08).round() as i32;
                let gx = x + (size - (cell * 2 + gap)) / 2;
                let gy = y + (size - (cell * 2 + gap)) / 2;
                self.round_rect(gx, gy, cell, cell, 2.5, Rgba::rgb(255, 255, 255));
                self.round_rect(gx + cell + gap, gy, cell, cell, 2.5, Rgba::rgba(255, 255, 255, 195));
                self.round_rect(gx, gy + cell + gap, cell, cell, 2.5, Rgba::rgba(255, 255, 255, 195));
                self.round_rect(
                    gx + cell + gap,
                    gy + cell + gap,
                    cell,
                    cell,
                    2.5,
                    Rgba::rgba(255, 255, 255, 145),
                );
            }
            "settings" | "control" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0x64, 0x74, 0x8b),
                    Rgba::rgb(0x33, 0x41, 0x55),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 64));
                self.ring(cx, cy, s * 0.19, 2.2, Rgba::rgb(248, 250, 252));
                for i in 0..6 {
                    let a = (i as f32) * std::f32::consts::FRAC_PI_3;
                    self.circle(
                        cx + a.cos() * s * 0.24,
                        cy + a.sin() * s * 0.24,
                        1.7,
                        Rgba::rgb(248, 250, 252),
                    );
                }
            }
            "agent" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0xf4, 0x72, 0xb6),
                    Rgba::rgb(0x93, 0x33, 0xea),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 72));
                self.ring(cx, cy, s * 0.22, 1.5, Rgba::rgba(255, 255, 255, 220));
                self.circle(cx, cy, s * 0.09, Rgba::rgb(255, 255, 255));
                self.circle(cx + s * 0.16, cy - s * 0.15, 2.0, Rgba::rgb(255, 255, 255));
            }
            "wallpaper" => {
                self.dgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgb(0xfb, 0x92, 0x3c),
                    Rgba::rgb(0xdb, 0x27, 0x77),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 72));
                self.circle(cx - s * 0.10, cy - s * 0.10, s * 0.09, Rgba::rgb(255, 251, 235));
                self.line(
                    cx - s * 0.24,
                    cy + s * 0.20,
                    cx - s * 0.03,
                    cy + s * 0.01,
                    2.2,
                    Rgba::rgb(255, 255, 255),
                );
                self.line(
                    cx - s * 0.03,
                    cy + s * 0.01,
                    cx + s * 0.22,
                    cy + s * 0.20,
                    2.2,
                    Rgba::rgb(255, 255, 255),
                );
            }
            "trash" => {
                self.vgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgba(0x47, 0x55, 0x69, 220),
                    Rgba::rgba(0x1e, 0x29, 0x3b, 235),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 58));
                let bw = (s * 0.38).round() as i32;
                let bh = (s * 0.42).round() as i32;
                let bx = x + (size - bw) / 2;
                let by = y + (size - bh) / 2 + 2;
                self.stroke_round_rect(bx, by, bw, bh, 2.5, 1.4, Rgba::rgb(226, 232, 240));
                self.line(
                    (bx - 2) as f32,
                    (by - 2) as f32,
                    (bx + bw + 2) as f32,
                    (by - 2) as f32,
                    1.5,
                    Rgba::rgb(226, 232, 240),
                );
            }
            _ => {
                self.vgrad_round_rect(
                    x,
                    y,
                    size,
                    size,
                    r,
                    Rgba::rgba(0x36, 0x30, 0x48, 225),
                    Rgba::rgba(0x21, 0x1c, 0x2e, 235),
                );
                self.stroke_round_rect(x, y, size, size, r, 1.0, Rgba::rgba(255, 255, 255, 42));
                if let Some(ic) = icon {
                    let pad = (size as f32 * 0.16).round() as i32;
                    self.draw_icon(x + pad, y + pad, size - pad * 2, ic);
                } else {
                    self.circle(cx, cy, s * 0.16, Rgba::rgb(0xc0, 0x84, 0xfc));
                }
            }
        }
    }
}

pub fn text_width(font: Option<&FontRef<'_>>, value: &str, size: f32) -> i32 {
    let Some(font) = font else {
        return 0;
    };
    let scaled = font.as_scaled(PxScale::from(size));
    value
        .chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum::<f32>()
        .round() as i32
}
