//! Window switcher (Alt+Tab / Super+Tab): a centered overlay row of the
//! windows the compositor cycles through, visible only while the modifier
//! is held. Purely visual — the surface takes no pointer or keyboard input.
//!
//! Shape borrowed from macOS (centered icon strip) with the Win11 cell
//! contents (icon + title). Selection inverts — the monochrome language's
//! active-state pattern, same as the menubar's focused workspace chip.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, icons, ShellState};

pub const SWITCHER_H: u32 = 84;
const CELL_W: f64 = 104.0;
const ICON_SZ: f32 = 28.0;
const PAD: f64 = 10.0;
const CARD_R: f32 = 12.0;
const CELL_R: f32 = 10.0;
const MAX_CELLS: usize = 9;

/// Content width for a given window count.
pub fn desired_width(count: usize) -> u32 {
    let n = count.clamp(1, MAX_CELLS) as f64;
    (PAD * 2.0 + n * CELL_W).ceil() as u32
}

fn theme(dark: bool) -> (Color, Color, Color, CtColor, Color, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1C, 0x1D, 0x21, 0xE6), // card
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x26), // border
            Color::from_rgba8(0xE8, 0xE8, 0xEC, 0xFF), // icon/text
            CtColor::rgb(0xE8, 0xE8, 0xEC),
            Color::from_rgba8(0xEA, 0xEA, 0xEE, 0xFF), // selected cell
            CtColor::rgb(0x14, 0x15, 0x18),
        )
    } else {
        (
            Color::from_rgba8(0xF7, 0xF7, 0xF9, 0xF2),
            Color::from_rgba8(0x00, 0x00, 0x00, 0x1F),
            Color::from_rgba8(0x20, 0x21, 0x24, 0xFF),
            CtColor::rgb(0x20, 0x21, 0x24),
            Color::from_rgba8(0x22, 0x23, 0x27, 0xFF),
            CtColor::rgb(0xF2, 0xF2, 0xF5),
        )
    }
}

/// Repaint the switcher overlay.
pub fn draw(state: &mut ShellState) {
    let (w, h) = state.switcher_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.switcher_surface.clone() else {
        return;
    };
    let (bg, border, icon_fg, text_fg, sel_bg, sel_fg) = theme(state.dark);
    let order: Vec<u64> = state
        .switcher_order
        .iter()
        .copied()
        .take(MAX_CELLS)
        .collect();
    if order.is_empty() {
        return;
    }
    let cells: Vec<(String, String)> = order
        .iter()
        .map(|id| match state.windows.iter().find(|wi| wi.id == *id) {
            Some(wi) => (
                icons::key_for(&wi.app_id),
                if wi.title.is_empty() {
                    wi.app_id.clone()
                } else {
                    wi.title.clone()
                },
            ),
            None => ("generic".into(), "Window".into()),
        })
        .collect();
    let selected = state.switcher_sel;
    let sel_glyph = Color::from_rgba8(sel_fg.r(), sel_fg.g(), sel_fg.b(), sel_fg.a());

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
    else {
        tracing::warn!("switcher: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    // Clear before painting — edge/corner pixels the card fill misses
    // would otherwise show stale shm pool memory as garbage glyphs.
    pixmap.fill(Color::TRANSPARENT);

    draw::fill_round_rect(
        &mut pixmap,
        0.5,
        0.5,
        w as f32 - 1.0,
        h as f32 - 1.0,
        CARD_R,
        bg,
    );
    draw::stroke_round_rect(
        &mut pixmap,
        0.5,
        0.5,
        w as f32 - 1.0,
        h as f32 - 1.0,
        CARD_R - 0.5,
        1.0,
        border,
    );

    let mut x = PAD;
    for (i, id) in order.iter().enumerate() {
        let (icon_key, title) = &cells[i];
        let is_sel = *id == selected;
        if is_sel {
            draw::fill_round_rect(
                &mut pixmap,
                x as f32 + 2.0,
                5.0,
                CELL_W as f32 - 4.0,
                h as f32 - 10.0,
                CELL_R,
                sel_bg,
            );
        }
        let (glyph, fg) = if is_sel {
            (sel_glyph, sel_fg)
        } else {
            (icon_fg, text_fg)
        };
        let cx = x + CELL_W / 2.0;
        icons::icon(
            &mut pixmap,
            icon_key,
            cx as f32 - ICON_SZ / 2.0,
            14.0,
            ICON_SZ,
            glyph,
        );
        let tw = CELL_W as f32 - 12.0;
        draw::text(
            &mut pixmap,
            (cx - (tw / 2.0) as f64) as f32,
            h as f32 - 30.0,
            tw,
            14.0,
            11.0,
            title,
            fg,
        );
        x += CELL_W;
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.commit();
}
