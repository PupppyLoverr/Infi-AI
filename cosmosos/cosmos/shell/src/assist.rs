//! Snap Assist — Win11's picker that fills the free half after a
//! Left/Right snap. A fullscreen overlay like the launcher, but only the
//! free half is dimmed; the snapped window stays bright behind the
//! untouched side.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{
    compositor::Region,
    seat::keyboard::{KeyEvent, Keysym},
    shell::WaylandSurface,
};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, icons, ShellState, PANEL_HEIGHT};

const TILE_W: f64 = 220.0;
const THUMB_H: f64 = 132.0;
const LABEL_H: f64 = 30.0;
const TILE_H: f64 = THUMB_H + LABEL_H;
const TILE_GAP: f64 = 12.0;
const TILE_R: f32 = 10.0;
const HEAD_H: f64 = 30.0;
const FOOT_H: f64 = 26.0;
const PAD: f64 = 14.0;
const CARD_R: f32 = 12.0;
const MAX_ROWS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Row(usize),
    Backdrop,
}

/// (cols, rows) of the tile grid for `n` windows in a free half
/// `free_w` wide: up to 3 columns, as many as fit.
pub fn grid(n: usize, free_w: f64) -> (usize, usize) {
    let n = n.clamp(1, MAX_ROWS);
    let fit = ((free_w - 2.0 * PAD - 16.0 + TILE_GAP) / (TILE_W + TILE_GAP)).floor() as usize;
    let cols = fit.clamp(1, 3).min(n);
    (cols, n.div_ceil(cols))
}

/// Tile `i`'s offset inside the card.
fn tile_offset(i: usize, cols: usize) -> (f64, f64) {
    let (c, r) = ((i % cols) as f64, (i / cols) as f64);
    (
        PAD + c * (TILE_W + TILE_GAP),
        PAD + HEAD_H + r * (TILE_H + TILE_GAP),
    )
}

/// Thumbnail pixel box (physical) the shell fits captures into.
pub fn thumb_box() -> (u32, u32) {
    let s = draw::scale();
    (
        ((TILE_W - 12.0) as f32 * s) as u32,
        ((THUMB_H - 12.0) as f32 * s) as u32,
    )
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x00, 0x00, 0x00, 0x60), // free-half dim
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2), // flat card (Lite)
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14), // tile
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x24), // hairline
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0xB4, 0xB4, 0xBA, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x40),
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x9C),
            Color::from_rgba8(0x00, 0x00, 0x00, 0x1F),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x4A, 0x4A, 0x50, 0xFF),
        )
    }
}

/// The free-half rect inside the fullscreen surface, minus shell
/// chrome: the menubar band on top, and the dock rail when it sits on
/// the free half's side. Both drawing and the input region use this —
/// chrome (tray, dock) must stay clickable while the picker is open.
pub fn free_half(state: &ShellState) -> (f64, f64, f64, f64) {
    let (w, h) = state.assist_size;
    let (w, h) = (w as f64, h as f64);
    let (mut x0, mut x1) = if state.assist_fill_left {
        (0.0, w / 2.0)
    } else {
        (w / 2.0, w)
    };
    match state.dock_position {
        crate::dock::DockPos::Left => x0 = x0.max(crate::dock::STRIP as f64),
        crate::dock::DockPos::Right => x1 = x1.min(w - crate::dock::STRIP as f64),
        crate::dock::DockPos::Bottom => {}
    }
    let y0 = PANEL_HEIGHT as f64;
    let y1 = if state.dock_position == crate::dock::DockPos::Bottom {
        h - crate::dock::STRIP as f64
    } else {
        h
    };
    (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// The picker card's rect inside the fullscreen surface — centred in
/// whichever half is free (`assist_fill_left` = the picker sits left).
pub fn card_rect(state: &ShellState) -> (f64, f64, f64, f64) {
    let (x0, y0, fw, fh) = free_half(state);
    let (cols, rows) = grid(state.assist_ids.len(), fw);
    let cw = PAD * 2.0 + cols as f64 * TILE_W + (cols as f64 - 1.0) * TILE_GAP;
    let ch = PAD * 2.0 + HEAD_H + rows as f64 * TILE_H + (rows as f64 - 1.0) * TILE_GAP + FOOT_H;
    (
        (x0 + (fw - cw) / 2.0).max(x0 + 8.0),
        y0 + (fh - ch).max(0.0) / 2.0,
        cw,
        ch,
    )
}

/// Tile index under (x, y), or Backdrop outside every tile.
pub fn hit_test(state: &ShellState, x: f64, y: f64) -> Hit {
    let (cx, cy, _, _) = card_rect(state);
    let (_, _, fw, _) = free_half(state);
    let n = state.assist_ids.len().min(MAX_ROWS);
    let (cols, _) = grid(n, fw);
    for i in 0..n {
        let (tx, ty) = tile_offset(i, cols);
        let (tx, ty) = (cx + tx, cy + ty);
        if x >= tx && x < tx + TILE_W && y >= ty && y < ty + TILE_H {
            return Hit::Row(i);
        }
    }
    Hit::Backdrop
}

/// Resolve a candidate id to (title, app_id) from the synced window list.
fn rows(state: &ShellState) -> Vec<(u64, String, String)> {
    state
        .assist_ids
        .iter()
        .take(MAX_ROWS)
        .map(|id| {
            let w = state.windows.iter().find(|x| x.id == *id);
            (
                *id,
                w.map(|x| x.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "Window".to_string()),
                w.map(|x| x.app_id.clone()).unwrap_or_default(),
            )
        })
        .collect()
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.assist_size;
    if w == 0 {
        return;
    }
    let Some(layer) = state.assist_surface.clone() else {
        return;
    };
    let dark = state.dark;
    let sel = state.assist_sel;
    let hover = state.assist_hover;
    let items = rows(state);
    let (cx, cy, cw, ch) = card_rect(state);
    let (dx, dy, dw, dh) = free_half(state);
    let (cols, _) = grid(items.len(), dw);
    let (dim, card, tile, sep, fg, fg_dim) = theme(dark);
    let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());
    let s = draw::scale();

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("assist: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);

    // Dim only the free half (below the menubar, clear of the dock) —
    // the snapped window and every chrome strip stay bright.
    draw::fill_rect(&mut pixmap, dx as f32, dy as f32, dw as f32, dh as f32, dim);

    // Glass card; the surface is fullscreen, so card coords are screen coords.
    let (cxf, cyf, cwf, chf) = (cx as f32, cy as f32, cw as f32, ch as f32);
    draw::shadow(&mut pixmap, cxf, cyf, cwf, chf, CARD_R);
    glass::fill_glass(
        &mut pixmap,
        &ShellState::wallpaper_name(),
        cxf,
        cyf,
        cwf,
        chf,
        CARD_R,
        cxf,
        cyf,
        dark,
        card,
    );
    draw::stroke_round_rect(&mut pixmap, cxf, cyf, cwf, chf, CARD_R, 1.0, sep);

    draw::text_bold(
        &mut pixmap,
        (cx + PAD) as f32,
        (cy + PAD) as f32,
        (cw - PAD * 2.0) as f32,
        18.0,
        13.0,
        "Snap a window",
        fg,
    );

    for (i, (_id, title, app_id)) in items.iter().enumerate() {
        let (ox, oy) = tile_offset(i, cols);
        let (tx, ty) = ((cx + ox) as f32, (cy + oy) as f32);
        let (tw, th) = (TILE_W as f32, TILE_H as f32);
        let active = sel == i || hover == Some(i);
        draw::fill_round_rect(
            &mut pixmap,
            tx,
            ty,
            tw,
            th,
            TILE_R,
            if active {
                draw::accent_soft(dark)
            } else {
                tile
            },
        );
        if active {
            draw::stroke_round_rect(&mut pixmap, tx, ty, tw, th, TILE_R, 2.0, draw::accent(dark));
        }
        let key = icons::key_for(app_id);
        let tint = icons::tint_for(&key, dark).unwrap_or(glyph);
        // Thumbnail, centred in the top box; the app icon if no capture.
        let (bx, by, bw, bh) = (tx + 6.0, ty + 6.0, tw - 12.0, THUMB_H as f32 - 12.0);
        match state.assist_thumbs.get(i).and_then(|t| t.as_ref()) {
            Some(thumb) => {
                let (lw, lh) = (thumb.width() as f32 / s, thumb.height() as f32 / s);
                let (lx, ly) = (bx + (bw - lw) / 2.0, by + (bh - lh) / 2.0);
                let mut clip = tiny_skia::Mask::new(pixmap.width(), pixmap.height());
                if let (Some(clip), Some(path)) =
                    (clip.as_mut(), draw::round_rect_path(lx, ly, lw, lh, 6.0))
                {
                    clip.fill_path(&path, tiny_skia::FillRule::Winding, true, draw::xf());
                    pixmap.draw_pixmap(
                        0,
                        0,
                        thumb.as_ref(),
                        &tiny_skia::PixmapPaint {
                            quality: tiny_skia::FilterQuality::Bilinear,
                            ..Default::default()
                        },
                        draw::xf().pre_translate(lx, ly).pre_scale(1.0 / s, 1.0 / s),
                        Some(clip),
                    );
                }
            }
            None => icons::icon(
                &mut pixmap,
                &key,
                bx + bw / 2.0 - 24.0,
                by + bh / 2.0 - 24.0,
                48.0,
                tint,
            ),
        }
        let ly = ty + THUMB_H as f32;
        icons::icon(&mut pixmap, &key, tx + 10.0, ly + 6.0, 16.0, tint);
        draw::text(
            &mut pixmap,
            tx + 32.0,
            ly + 6.0,
            tw - 42.0,
            16.0,
            12.0,
            title,
            fg,
        );
    }

    draw::text(
        &mut pixmap,
        (cx + PAD) as f32,
        (cy + ch - FOOT_H + 4.0) as f32,
        (cw - PAD * 2.0) as f32,
        14.0,
        11.0,
        "Arrows choose · Enter snap · Esc dismiss",
        fg_dim,
    );

    state.set_viewport(&layer.wl_surface().clone(), w, h);

    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, pw as i32, ph as i32);
    // The input region is exactly the dimmed rect — free half minus
    // menubar and dock — so tray and dock clicks reach their surfaces.
    if let Ok(region) = Region::new(&state.compositor_state) {
        region.add(dx as i32, dy as i32, dw as i32, dh as i32);
        layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
    }
    layer.wl_surface().commit();
}

/// Pointer motion → hovered row index changes.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let h = match hit_test(state, x, y) {
        Hit::Row(i) => Some(i),
        Hit::Backdrop => None,
    };
    if h != state.assist_hover {
        state.assist_hover = h;
        true
    } else {
        false
    }
}

/// Keys while the assist holds keyboard focus.
pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    match event.keysym {
        Keysym::Escape => state.close_assist(),
        Keysym::Return | Keysym::KP_Enter => state.assist_pick_selected(),
        Keysym::Down | Keysym::Right | Keysym::Tab => {
            let n = state.assist_ids.len().min(MAX_ROWS);
            if n > 0 {
                state.assist_sel = (state.assist_sel + 1) % n;
                state.assist_dirty = true;
            }
        }
        Keysym::Up | Keysym::Left => {
            let n = state.assist_ids.len().min(MAX_ROWS);
            if n > 0 {
                state.assist_sel = (state.assist_sel + n - 1) % n;
                state.assist_dirty = true;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_fits_the_free_half() {
        // 1920/2 free half fits 3 tiles; a narrow half falls back to 1.
        assert_eq!(grid(5, 960.0), (3, 2));
        assert_eq!(grid(2, 960.0), (2, 1));
        assert_eq!(grid(4, 300.0), (1, 4));
        assert_eq!(grid(0, 960.0), (1, 1));
        let (cols, _) = grid(9, 960.0);
        let (x, _) = tile_offset(cols - 1, cols);
        assert!(x + TILE_W + PAD <= 960.0 - 16.0);
    }
}
