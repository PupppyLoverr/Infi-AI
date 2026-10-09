//! Software rasterizer for egui output: tessellated meshes painted into an
//! ARGB8888 shm buffer with per-pixel barycentric UV/colour interpolation.
//! Same semantics as GPU egui renderers: `texel * interpolated_vertex_colour`,
//! both premultiplied (egui's `Color32` convention), composited source-over.

use std::collections::HashMap;

use egui::epaint::{ClippedPrimitive, ImageData, ImageDelta, Primitive};
use egui::{Color32, TextureId};

#[derive(Default)]
pub struct Painter {
    textures: HashMap<TextureId, Texture>,
    white: Texture,
}

struct Texture {
    w: usize,
    h: usize,
    px: Vec<u8>, // RGBA8, premultiplied (egui `Color32`)
}

impl Default for Texture {
    fn default() -> Self {
        Texture {
            w: 1,
            h: 1,
            px: vec![255, 255, 255, 255],
        }
    }
}

impl Painter {
    pub fn set_texture(&mut self, id: TextureId, delta: &ImageDelta) {
        let (w, h, rgba) = decode(&delta.image);
        match delta.pos {
            Some([x, y]) => {
                if let Some(tex) = self.textures.get_mut(&id) {
                    for row in 0..h {
                        let dy = y + row;
                        if dy >= tex.h {
                            break;
                        }
                        for col in 0..w {
                            let dx = x + col;
                            if dx >= tex.w {
                                break;
                            }
                            let src = (row * w + col) * 4;
                            let dst = (dy * tex.w + dx) * 4;
                            tex.px[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                        }
                    }
                }
            }
            None => {
                self.textures.insert(id, Texture { w, h, px: rgba });
            }
        }
    }

    pub fn free_texture(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    /// Paint clipped primitives into `buf` (ARGB8888, packed, `w*h*4`),
    /// over the `clear` colour (opaque unless the app asked for a
    /// translucent body), so a frame is never blank even when `prims`
    /// is empty.
    pub fn paint(
        &mut self,
        buf: &mut [u8],
        w: u32,
        h: u32,
        prims: &[ClippedPrimitive],
        ppp: f32,
        clear: [u8; 4],
    ) {
        for (b, c) in buf.iter_mut().zip(clear.iter().cycle()) {
            *b = *c;
        }
        for prim in prims {
            let Primitive::Mesh(mesh) = &prim.primitive else {
                continue; // Primitive::Callback unused by core widgets
            };
            let tex = self.textures.get(&mesh.texture_id).unwrap_or(&self.white);

            let clip = prim.clip_rect;
            let clip = Clip {
                x0: (clip.min.x * ppp).max(0.0) as i32,
                y0: (clip.min.y * ppp).max(0.0) as i32,
                x1: ((clip.max.x * ppp).ceil() as i32).min(w as i32),
                y1: ((clip.max.y * ppp).ceil() as i32).min(h as i32),
            };
            if clip.x1 <= clip.x0 || clip.y1 <= clip.y0 {
                continue;
            }

            for t in 0..mesh.indices.len() / 3 {
                let tri = &mesh.indices[t * 3..t * 3 + 3];
                let v0 = &mesh.vertices[tri[0] as usize];
                let v1 = &mesh.vertices[tri[1] as usize];
                let v2 = &mesh.vertices[tri[2] as usize];
                fill_tri(buf, w, tex, clip, ppp, v0, v1, v2);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Clip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

fn decode(image: &ImageData) -> (usize, usize, Vec<u8>) {
    match image {
        ImageData::Color(img) => {
            let mut px = Vec::with_capacity(img.size[0] * img.size[1] * 4);
            for c in img.pixels.iter() {
                px.extend_from_slice(&c.to_array());
            }
            (img.size[0], img.size[1], px)
        }
    }
}

#[inline]
fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

#[inline]
fn sample(tex: &Texture, u: f32, v: f32) -> [f32; 4] {
    // Nearest sample — cheap and correct enough for the font atlas.
    let x = (u * tex.w as f32).clamp(0.0, tex.w as f32 - 1.0) as usize;
    let y = (v * tex.h as f32).clamp(0.0, tex.h as f32 - 1.0) as usize;
    let i = (y * tex.w + x) * 4;
    [
        tex.px[i] as f32 / 255.0,
        tex.px[i + 1] as f32 / 255.0,
        tex.px[i + 2] as f32 / 255.0,
        tex.px[i + 3] as f32 / 255.0,
    ]
}

#[allow(clippy::too_many_arguments)]
fn fill_tri(
    buf: &mut [u8],
    buf_w: u32,
    tex: &Texture,
    clip: Clip,
    ppp: f32,
    v0: &egui::epaint::Vertex,
    v1: &egui::epaint::Vertex,
    v2: &egui::epaint::Vertex,
) {
    let p0x = v0.pos.x * ppp;
    let p0y = v0.pos.y * ppp;
    let p1x = v1.pos.x * ppp;
    let p1y = v1.pos.y * ppp;
    let p2x = v2.pos.x * ppp;
    let p2y = v2.pos.y * ppp;

    let min_x = p0x.min(p1x).min(p2x).floor() as i32;
    let min_y = p0y.min(p1y).min(p2y).floor() as i32;
    let max_x = p0x.max(p1x).max(p2x).ceil() as i32;
    let max_y = p0y.max(p1y).max(p2y).ceil() as i32;

    let x0 = min_x.max(clip.x0);
    let y0 = min_y.max(clip.y0);
    let x1 = max_x.min(clip.x1);
    let y1 = max_y.min(clip.y1);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let area = edge(p0x, p0y, p1x, p1y, p2x, p2y);
    if area.abs() < 1e-6 {
        return;
    }
    let inv_area = 1.0 / area;

    let c0 = color(v0.color);
    let c1 = color(v1.color);
    let c2 = color(v2.color);

    for y in y0..y1 {
        let py = y as f32 + 0.5;
        let mut px_i = (y as usize * buf_w as usize + x0 as usize) * 4;
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            // Barycentric weights (sign handled by consistent edge order).
            let w0 = edge(p1x, p1y, p2x, p2y, px, py) * inv_area;
            let w1 = edge(p2x, p2y, p0x, p0y, px, py) * inv_area;
            let w2 = edge(p0x, p0y, p1x, p1y, px, py) * inv_area;
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                let u = w0 * v0.uv.x + w1 * v1.uv.x + w2 * v2.uv.x;
                let v = w0 * v0.uv.y + w1 * v1.uv.y + w2 * v2.uv.y;
                let t = sample(tex, u, v);
                let sr = t[0] * (w0 * c0[0] + w1 * c1[0] + w2 * c2[0]);
                let sg = t[1] * (w0 * c0[1] + w1 * c1[1] + w2 * c2[1]);
                let sb = t[2] * (w0 * c0[2] + w1 * c1[2] + w2 * c2[2]);
                let sa = t[3] * (w0 * c0[3] + w1 * c1[3] + w2 * c2[3]);
                if sa > 0.0 {
                    blend(buf, px_i, sr, sg, sb, sa);
                }
            }
            px_i += 4;
        }
    }
}

#[inline]
fn color(c: Color32) -> [f32; 4] {
    [
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
        c.a() as f32 / 255.0,
    ]
}

#[inline]
fn blend(buf: &mut [u8], i: usize, r: f32, g: f32, b: f32, a: f32) {
    // Source-over: premultiplied source onto the unpremultiplied buffer.
    let da = buf[i + 3] as f32 / 255.0;
    let dr = buf[i] as f32 / 255.0;
    let dg = buf[i + 1] as f32 / 255.0;
    let db = buf[i + 2] as f32 / 255.0;
    let oa = a + da * (1.0 - a);
    if oa <= 0.0 {
        return;
    }
    let or_ = ((r + dr * da * (1.0 - a)) / oa).min(1.0);
    let og = ((g + dg * da * (1.0 - a)) / oa).min(1.0);
    let ob = ((b + db * da * (1.0 - a)) / oa).min(1.0);
    buf[i] = (or_ * 255.0) as u8;
    buf[i + 1] = (og * 255.0) as u8;
    buf[i + 2] = (ob * 255.0) as u8;
    buf[i + 3] = (oa * 255.0) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vert(x: f32, y: f32, color: Color32) -> egui::epaint::Vertex {
        egui::epaint::Vertex {
            pos: egui::pos2(x, y),
            uv: egui::pos2(0.0, 0.0),
            color,
        }
    }

    // egui colours are premultiplied: white at alpha 36 over opaque black
    // must land at ~36, not 36 * 36 / 255 ≈ 5.
    #[test]
    fn translucent_white_is_not_squared() {
        let mut buf = vec![0u8, 0, 0, 255].repeat(16);
        let c = Color32::from_white_alpha(36);
        let clip = Clip {
            x0: 0,
            y0: 0,
            x1: 4,
            y1: 4,
        };
        let tex = Texture::default();
        let (a, b, d) = (vert(0.0, 0.0, c), vert(8.0, 0.0, c), vert(0.0, 8.0, c));
        fill_tri(&mut buf, 4, &tex, clip, 1.0, &a, &b, &d);
        assert!((34..=38).contains(&buf[0]), "got {}", buf[0]);
        assert_eq!(buf[3], 255);
    }
}
