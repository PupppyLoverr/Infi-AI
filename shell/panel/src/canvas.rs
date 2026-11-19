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
    pub semibold: Option<FontRef<'static>>,
    pub mono: Option<FontRef<'static>>,
}

impl Fonts {
    pub fn load() -> Self {
        Self {
            regular: try_load_font(&[
                "/usr/share/fonts/rsms-inter-fonts/Inter-Regular.otf",
                "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
                "/usr/share/fonts/open-sans/OpenSans-Regular.ttf",
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]),
            semibold: try_load_font(&[
                "/usr/share/fonts/rsms-inter-fonts/Inter-SemiBold.otf",
                "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
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

    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: f32, color: Rgba) {
        let (s, sf) = (self.s(), self.s() as f32);
        self.round_rect_raw(x * s, y * s, w * s, h * s, radius * sf, color);
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
        let adv = self.text_raw(font, value, x * s, baseline * s, size * sf, color);
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

    /// Anti-aliased arc segment (`start_rad` .. `end_rad`, clockwise from +X).
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

    fn text_raw(
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

    // ------------------------------------------------------------ app tiles

    /// App tile (`size x size`): a flat dark rounded square. A real system
    /// icon is used when one was found; otherwise a monochrome glyph for the
    /// role, or the role's first letter. One style for every app, so the dock
    /// and launcher read as one set.
    pub fn draw_app_squircle(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        role: &str,
        icon: Option<&Icon>,
    ) {
        const TILE: Rgba = Rgba::rgb(0x2a, 0x24, 0x31);
        const EDGE: Rgba = Rgba::rgba(255, 255, 255, 26);
        const GLYPH: Rgba = Rgba::rgb(0xdc, 0xd6, 0xe2);
        const ACCENT: Rgba = Rgba::rgb(0xb4, 0x8a, 0xad);
        const ON_ACCENT: Rgba = Rgba::rgb(0x14, 0x10, 0x18);

        let s = size as f32;
        let r = (s * 0.24).round();
        let (cx, cy) = (x as f32 + s * 0.5, y as f32 + s * 0.5);
        let start = role == "start";
        self.round_rect(x, y, size, size, r, if start { ACCENT } else { TILE });
        self.stroke_round_rect(x, y, size, size, r, 1.0, EDGE);
        let ink = if start { ON_ACCENT } else { GLYPH };

        if let (false, Some(ic)) = (start, icon) {
            let pad = (s * 0.16).round() as i32;
            self.draw_icon(x + pad, y + pad, size - pad * 2, ic);
            return;
        }
        let u = s * 0.22; // glyph half-extent
        match role {
            "start" | "agent" => self.icon_solyra(cx, cy, u * 0.9, ink),
            "files" | "thunar" => {
                let (w, h) = ((u * 2.1) as i32, (u * 1.5) as i32);
                let (fx, fy) = (cx as i32 - w / 2, cy as i32 - h / 2 + 1);
                self.stroke_round_rect(fx, fy, w, h, 2.5, 1.5, ink);
                self.round_rect(fx, fy - 3, w / 2, 4, 1.5, ink);
            }
            "terminal" | "foot" => {
                self.line(cx - u, cy - u * 0.7, cx - u * 0.1, cy, 1.7, ink);
                self.line(cx - u * 0.1, cy, cx - u, cy + u * 0.7, 1.7, ink);
                self.line(cx + u * 0.1, cy + u * 0.8, cx + u, cy + u * 0.8, 1.7, ink);
            }
            "overview" => {
                let q = (u * 0.8) as i32;
                let (x0, y0) = (cx as i32 - q - 1, cy as i32 - q - 1);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    self.round_rect(x0 + dx * (q + 2), y0 + dy * (q + 2), q, q, 1.5, ink);
                }
            }
            "settings" => {
                for (i, k) in [0.28f32, 0.62, 0.40].into_iter().enumerate() {
                    let ly = cy - u * 0.9 + i as f32 * u * 0.9;
                    self.line(cx - u, ly, cx + u, ly, 1.4, ink);
                    self.circle(cx - u + 2.0 * u * k, ly, 2.2, ink);
                }
            }
            "wallpaper" => {
                self.ring(cx, cy, u * 0.9, 1.5, ink);
                self.circle(cx + u * 0.25, cy - u * 0.2, u * 0.28, ink);
            }
            _ => {
                // Generic window: no real icon was found for this app.
                let (w, h) = ((u * 2.1) as i32, (u * 1.7) as i32);
                let (wx, wy) = (cx as i32 - w / 2, cy as i32 - h / 2);
                self.stroke_round_rect(wx, wy, w, h, 2.5, 1.5, ink);
                self.fill_rect(wx + 1, wy + 4, w - 2, 1, ink);
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
}
