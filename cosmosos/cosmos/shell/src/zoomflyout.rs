//! Zoom flyout — Win11's snap-layouts card that opens when the
//! pointer rests on a window's green zoom button. One small surface
//! anchored under the button; each thumbnail previews a snap layout
//! and its cells snap the window straight into that zone.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, Keysym},
    shell::WaylandSurface,
};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, ShellState};

const PREV_W: f64 = 126.0;
const PREV_H: f64 = 80.0;
const PREV_GAP: f64 = 8.0;
const PAD: f64 = 10.0;
const CARD_R: f32 = 12.0;
const PREV_R: f32 = 8.0;
const CELL_GAP: f64 = 3.0;
const CELL_R: f32 = 4.0;

/// (w, h) the surface needs — one row of layout previews.
pub fn card_size() -> (u32, u32) {
    let n = cosmos_ipc::SNAP_LAYOUTS.len() as f64;
    (
        (PAD * 2.0 + n * PREV_W + (n - 1.0) * PREV_GAP) as u32,
        (PAD * 2.0 + PREV_H) as u32,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// (layout index, cell index) — the zone's SNAP_LAYOUTS coords.
    Zone(usize, usize),
    Backdrop,
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2), // card
            Color::from_rgba8(0x2A, 0x2B, 0x30, 0xFF), // preview bg
            draw::accent_soft(true),                   // hover cell
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF), // border
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0xE8, 0xE8, 0xEB, 0xFF),
            draw::accent_soft(false),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
        )
    }
}

/// Preview rect for layout `li` inside the card.
fn preview_rect(li: usize) -> (f64, f64, f64, f64) {
    (PAD + li as f64 * (PREV_W + PREV_GAP), PAD, PREV_W, PREV_H)
}

/// Zone cell rect inside preview `li`, from its fractional spec.
fn cell_rect(li: usize, cell: (f32, f32, f32, f32)) -> (f64, f64, f64, f64) {
    let (px, py, pw, ph) = preview_rect(li);
    let (fx, fy, fw, fh) = cell;
    let ix = pw as f64 - 2.0 * CELL_GAP;
    let iy = ph as f64 - 2.0 * CELL_GAP;
    (
        px + CELL_GAP + fx as f64 * ix,
        py + CELL_GAP + fy as f64 * iy,
        (fw as f64 * ix - CELL_GAP).max(2.0),
        (fh as f64 * iy - CELL_GAP).max(2.0),
    )
}

/// Which zone cell (x, y) falls in — card-local coords.
pub fn hit_test(x: f64, y: f64) -> Hit {
    let (cw, ch) = card_size();
    if x < 0.0 || x >= cw as f64 || y < 0.0 || y >= ch as f64 {
        return Hit::Backdrop;
    }
    for (li, (_, cells)) in cosmos_ipc::SNAP_LAYOUTS.iter().enumerate() {
        for (ci, (_, fx, fy, fw, fh)) in cells.iter().enumerate() {
            let (x0, y0, w, h) = cell_rect(li, (*fx, *fy, *fw, *fh));
            if x >= x0 && x < x0 + w && y >= y0 && y < y0 + h {
                return Hit::Zone(li, ci);
            }
        }
    }
    Hit::Backdrop
}

/// The zone name a hit maps to.
pub fn zone_name(li: usize, ci: usize) -> Option<&'static str> {
    cosmos_ipc::SNAP_LAYOUTS
        .get(li)
        .and_then(|(_, cells)| cells.get(ci))
        .map(|(n, ..)| *n)
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = card_size();
    if w == 0 {
        return;
    }
    let Some(layer) = state.zoom_surface.clone() else {
        return;
    };
    let dark = state.dark;
    let hover = state.zoom_hover;
    let (card, prev_bg, hover_bg, sep, _fg) = theme(dark);

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("zoomflyout: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);

    // Card — shadowed like every floating surface.
    draw::shadow(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R);
    draw::fill_round_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R, card);
    draw::stroke_round_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R, 1.0, sep);

    for (li, (_, cells)) in cosmos_ipc::SNAP_LAYOUTS.iter().enumerate() {
        let (px, py, pw, ph) = preview_rect(li);
        draw::fill_round_rect(
            &mut pixmap,
            px as f32,
            py as f32,
            pw as f32,
            ph as f32,
            PREV_R,
            prev_bg,
        );
        for (ci, (_, fx, fy, fw, fh)) in cells.iter().enumerate() {
            let (x0, y0, cw, ch) = cell_rect(li, (*fx, *fy, *fw, *fh));
            let color = if hover == Some((li, ci)) {
                hover_bg
            } else {
                card // cell interior = card colour, slightly raised
            };
            draw::fill_round_rect(
                &mut pixmap,
                x0 as f32,
                y0 as f32,
                cw as f32,
                ch as f32,
                CELL_R,
                color,
            );
        }
    }

    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
    // Input region = the whole card; the surface IS the card, so a
    // press anywhere else already misses this surface entirely.
    layer.wl_surface().commit();
}

/// Pointer motion → hovered cell changes.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let h = match hit_test(x, y) {
        Hit::Zone(li, ci) => Some((li, ci)),
        Hit::Backdrop => None,
    };
    if h != state.zoom_hover {
        state.zoom_hover = h;
        true
    } else {
        false
    }
}

/// Keys while the flyout holds keyboard focus: Esc dismisses.
pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    if event.keysym == Keysym::Escape {
        state.close_zoom();
    }
}
