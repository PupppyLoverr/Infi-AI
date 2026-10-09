//! Keybind cheatsheet overlay (super+?) — the Omarchy-style "what can I
//! press" card. Fullscreen scrim + centered card listing every binding
//! from `cosmos_ipc::KEYBINDS`. Any press or Esc dismisses it.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, ShellState};

const CARD_W: f32 = 620.0;
const PAD: f32 = 26.0;
const TITLE_H: f32 = 30.0;
const SECTION_H: f32 = 30.0;
const ROW_H: f32 = 26.0;
const KEY_COL_W: f32 = 220.0;
const FOOTER_H: f32 = 34.0;

fn theme(dark: bool) -> (Color, Color, CtColor, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF6), // card
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF), // hairline
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),     // fg
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),     // dim
            CtColor::rgba(0x6A, 0x6A, 0x70, 0xFF),     // section header
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0xC8, 0xC8, 0xCC, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
            CtColor::rgba(0x8A, 0x8A, 0x90, 0xFF),
        )
    }
}

/// Card height derived from the bind table (never hardcodes row count).
fn card_height() -> f32 {
    let rows = cosmos_ipc::KEYBINDS.len() as f32;
    // One section header per contiguous section block.
    let mut sections = 0usize;
    let mut last = "";
    for (s, _, _) in cosmos_ipc::KEYBINDS {
        if *s != last {
            sections += 1;
            last = s;
        }
    }
    PAD * 2.0 + TITLE_H + sections as f32 * SECTION_H + rows * ROW_H + FOOTER_H
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.help_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.help_surface.clone() else {
        return;
    };
    let (card, sep, fg, fg_dim, fg_faint) = theme(state.dark);

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("help: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    // Clear before the scrim blend — stale slot bytes would ghost through.
    pixmap.fill(Color::TRANSPARENT);
    draw::scrim(&mut pixmap, w, h, state.dark);

    let cw = CARD_W.min(w as f32 - 40.0);
    let ch = card_height().min(h as f32 - 40.0);
    let cx = (w as f32 - cw) / 2.0;
    let cy = (h as f32 - ch) / 2.0;
    draw::shadow(&mut pixmap, cx, cy, cw, ch, 12.0);
    draw::fill_round_rect(&mut pixmap, cx, cy, cw, ch, 12.0, card);
    draw::stroke_round_rect(
        &mut pixmap,
        cx + 0.5,
        cy + 0.5,
        cw - 1.0,
        ch - 1.0,
        11.5,
        1.0,
        sep,
    );

    let tx = cx + PAD;
    let tw = cw - PAD * 2.0;
    let mut y = cy + PAD;

    draw::text_bold(
        &mut pixmap,
        tx,
        y + 2.0,
        tw,
        24.0,
        17.0,
        "Keyboard shortcuts",
        fg,
    );
    draw::text(
        &mut pixmap,
        tx,
        y + 4.0,
        tw,
        20.0,
        12.0,
        "CosmosOS",
        fg_faint,
    );
    // Right-align the subtitle.
    y += TITLE_H;

    let mut last_section = "";
    for (section, key, action) in cosmos_ipc::KEYBINDS {
        if *section != last_section {
            last_section = section;
            y += SECTION_H - 18.0;
            draw::text_bold(
                &mut pixmap,
                tx,
                y,
                200.0,
                18.0,
                11.0,
                &section.to_uppercase(),
                fg_faint,
            );
            draw::fill_rect(&mut pixmap, tx, y + 20.0, tw, 1.0, sep);
            y += 8.0;
        }
        draw::text_mono(&mut pixmap, tx, y, KEY_COL_W, 20.0, 13.0, key, fg);
        draw::text(
            &mut pixmap,
            tx + KEY_COL_W,
            y,
            tw - KEY_COL_W,
            20.0,
            13.0,
            action,
            fg_dim,
        );
        y += ROW_H;
    }

    draw::fill_rect(
        &mut pixmap,
        cx + 1.0,
        cy + ch - FOOTER_H,
        cw - 2.0,
        1.0,
        sep,
    );
    draw::text(
        &mut pixmap,
        tx,
        cy + ch - FOOTER_H + 10.0,
        tw,
        18.0,
        12.0,
        "Esc or click anywhere to close",
        fg_faint,
    );

    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
    wl_surface.commit();
}
