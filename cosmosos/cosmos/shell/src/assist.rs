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

use crate::{draw, icons, ShellState, PANEL_HEIGHT};

const CARD_W: f64 = 300.0;
const ROW_H: f64 = 44.0;
const HEAD_H: f64 = 34.0;
const FOOT_H: f64 = 28.0;
const PAD: f64 = 12.0;
const CARD_R: f32 = 12.0;
const MAX_ROWS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Row(usize),
    Backdrop,
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x00, 0x00, 0x00, 0x90), // free-half dim
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2), // card
            draw::accent_soft(true),                   // selection wash
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF), // border
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x66),
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            draw::accent_soft(false),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    }
}

/// The picker card's rect inside the fullscreen surface — centred in
/// whichever half is free (`assist_fill_left` = the picker sits left).
pub fn card_rect(state: &ShellState) -> (f64, f64, f64, f64) {
    let (w, h) = state.assist_size;
    let (w, h) = (w as f64, h as f64);
    let rows = state.assist_ids.len().min(MAX_ROWS).max(1) as f64;
    let ch = PAD * 2.0 + HEAD_H + rows * ROW_H + FOOT_H;
    let cx = if state.assist_fill_left {
        w / 4.0
    } else {
        w * 0.75
    };
    ((cx - CARD_W / 2.0).max(8.0), (h - ch) / 2.0, CARD_W, ch)
}

/// Row index under (x, y), or Backdrop outside the card.
pub fn hit_test(state: &ShellState, x: f64, y: f64) -> Hit {
    let (cx, cy, cw, ch) = card_rect(state);
    if x < cx || x >= cx + cw || y < cy || y >= cy + ch {
        return Hit::Backdrop;
    }
    let ry = cy + PAD + HEAD_H;
    let rows = state.assist_ids.len().min(MAX_ROWS);
    for i in 0..rows {
        if y >= ry + i as f64 * ROW_H && y < ry + (i + 1) as f64 * ROW_H {
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
    let fill_left = state.assist_fill_left;
    let items = rows(state);
    let (cx, cy, cw, ch) = card_rect(state);
    let (dim, card, sel_bg, sep, fg, fg_dim) = theme(dark);
    let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("assist: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);

    // Dim only the free half — the snapped window stays bright.
    let (dx, dw) = if fill_left {
        (0.0, w as f32 / 2.0)
    } else {
        (w as f32 / 2.0, w as f32 / 2.0)
    };
    draw::fill_rect(&mut pixmap, dx, 0.0, dw, h as f32, dim);

    // Picker card — soft shadow under it like the launcher's.
    draw::shadow(
        &mut pixmap,
        cx as f32,
        cy as f32,
        cw as f32,
        ch as f32,
        CARD_R,
    );
    draw::fill_round_rect(
        &mut pixmap,
        cx as f32,
        cy as f32,
        cw as f32,
        ch as f32,
        CARD_R,
        card,
    );
    draw::stroke_round_rect(
        &mut pixmap,
        cx as f32,
        cy as f32,
        cw as f32,
        ch as f32,
        CARD_R,
        1.0,
        sep,
    );

    draw::text_bold(
        &mut pixmap,
        (cx + PAD) as f32,
        (cy + PAD + 2.0) as f32,
        (cw - PAD * 2.0) as f32,
        18.0,
        13.0,
        "Snap a window",
        fg,
    );

    for (i, (_id, title, app_id)) in items.iter().enumerate() {
        let ry = cy + PAD + HEAD_H + i as f64 * ROW_H;
        if sel == i || hover == Some(i) {
            draw::fill_round_rect(
                &mut pixmap,
                (cx + 6.0) as f32,
                (ry + 4.0) as f32,
                (cw - 12.0) as f32,
                (ROW_H - 8.0) as f32,
                8.0,
                sel_bg,
            );
        }
        icons::icon(
            &mut pixmap,
            &icons::key_for(app_id),
            (cx + PAD + 8.0) as f32,
            (ry + ROW_H / 2.0 - 10.0) as f32,
            20.0,
            icons::tint_for(&icons::key_for(app_id), dark).unwrap_or(glyph),
        );
        draw::text(
            &mut pixmap,
            (cx + PAD + 36.0) as f32,
            (ry + ROW_H / 2.0 - 8.0) as f32,
            (cw - PAD * 2.0 - 44.0) as f32,
            16.0,
            13.0,
            title,
            fg,
        );
    }

    draw::text(
        &mut pixmap,
        (cx + PAD) as f32,
        (cy + ch - FOOT_H + 6.0) as f32,
        (cw - PAD * 2.0) as f32,
        14.0,
        11.0,
        "↑↓ choose · Enter snap · Esc dismiss",
        fg_dim,
    );

    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
    // Only the dimmed free half (below the menubar) takes pointer input.
    // Clicks on the snapped window, the menubar, or the tray hit the
    // surfaces underneath instead of being swallowed as a backdrop
    // press — a tray click must open the flyout, not just cancel this.
    if let Ok(region) = Region::new(&state.compositor_state) {
        region.add(
            dx as i32,
            PANEL_HEIGHT as i32,
            dw as i32,
            (h as i32 - PANEL_HEIGHT as i32).max(0),
        );
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
        Keysym::Down | Keysym::Tab => {
            let n = state.assist_ids.len().min(MAX_ROWS);
            if n > 0 {
                state.assist_sel = (state.assist_sel + 1) % n;
                state.assist_dirty = true;
            }
        }
        Keysym::Up => {
            let n = state.assist_ids.len().min(MAX_ROWS);
            if n > 0 {
                state.assist_sel = (state.assist_sel + n - 1) % n;
                state.assist_dirty = true;
            }
        }
        _ => {}
    }
}
