//! Dynamic island — the droppy-style card that expands from the menubar
//! pill: clipboard history, staged files (drag & drop tray), and room for
//! live activities. The pill itself is drawn inside the menubar by
//! `panel.rs`; this module owns the expanded card surface.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, ShellState};

const CARD_W: f64 = 380.0;
const PAD: f64 = 12.0;
const HEAD_H: f64 = 26.0;
const ROW_H: f64 = 34.0;
const SEC_H: f64 = 22.0;
const CARD_R: f32 = 14.0;
const MAX_CLIP_ROWS: usize = 8;
const MAX_FILE_ROWS: usize = 6;

/// Width of the menubar pill (idle island) — centred in the panel.
pub fn pill_width(state: &ShellState) -> f64 {
    let snip = state
        .clip_history
        .front()
        .map(|t| t.chars().take(22).collect::<String>())
        .unwrap_or_default();
    let staged = state.staged_files.len();
    let base = 56.0
        + if snip.is_empty() {
            0.0
        } else {
            snip.len() as f64 * 7.0
        };
    (base + if staged > 0 { 26.0 } else { 0.0 }).clamp(56.0, 260.0)
}

/// Pill rect inside the panel surface (centred horizontally).
pub fn pill_rect(state: &ShellState) -> (f64, f64, f64, f64) {
    let (w, h) = state.panel_size;
    let pw = pill_width(state);
    ((w as f64 - pw) / 2.0, 4.0, pw, h as f64 - 8.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    ClipRow(usize),
    FileRow(usize),
    Backdrop,
}

fn theme(dark: bool) -> (Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2),
            draw::accent_soft(true),
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF),
            Color::from_rgba8(0x2A, 0x2B, 0x30, 0xFF),
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            draw::accent_soft(false),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            Color::from_rgba8(0xEF, 0xEF, 0xF2, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    }
}

fn clip_rows(state: &ShellState) -> usize {
    state.clip_history.len().min(MAX_CLIP_ROWS)
}

fn file_rows(state: &ShellState) -> usize {
    state.staged_files.len().min(MAX_FILE_ROWS)
}

/// Card height for the current content.
pub fn card_height(state: &ShellState) -> u32 {
    let mut h = PAD + HEAD_H;
    if clip_rows(state) > 0 || !state.staged_files.is_empty() || true {
        // CLIPBOARD section is always shown (empty state text when empty).
        h += SEC_H + clip_rows(state).max(1) as f64 * ROW_H;
    }
    if !state.staged_files.is_empty() {
        h += SEC_H + file_rows(state) as f64 * ROW_H;
    }
    (h + PAD) as u32
}

fn clip_row_y(_state: &ShellState, i: usize) -> f64 {
    PAD + HEAD_H + SEC_H + i as f64 * ROW_H
}

fn file_row_y(state: &ShellState, i: usize) -> f64 {
    clip_row_y(state, clip_rows(state).max(1)) + SEC_H + i as f64 * ROW_H
}

pub fn hit_test(state: &ShellState, x: f64, y: f64) -> Hit {
    let (cw, ch) = (CARD_W, card_height(state) as f64);
    if x < 0.0 || x >= cw || y < 0.0 || y >= ch {
        return Hit::Backdrop;
    }
    for i in 0..clip_rows(state) {
        let ry = clip_row_y(state, i);
        if y >= ry && y < ry + ROW_H {
            return Hit::ClipRow(i);
        }
    }
    for i in 0..file_rows(state) {
        let ry = file_row_y(state, i);
        if y >= ry && y < ry + ROW_H {
            return Hit::FileRow(i);
        }
    }
    Hit::Backdrop
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = (CARD_W as u32, card_height(state));
    if w == 0 {
        return;
    }
    let Some(layer) = state.island_surface.clone() else {
        return;
    };
    let dark = state.dark;
    let (card, sel, sep, row_bg, fg, fg_dim) = theme(dark);
    let clips: Vec<String> = state
        .clip_history
        .iter()
        .take(MAX_CLIP_ROWS)
        .cloned()
        .collect();
    let files: Vec<String> = state
        .staged_files
        .iter()
        .take(MAX_FILE_ROWS)
        .cloned()
        .collect();
    let hover = state.island_hover;
    // Staged-file rows sit below the clipboard section — precompute the
    // base y before the pool's mutable borrow below.
    let files_base_y = clip_row_y(state, clip_rows(state).max(1)) + SEC_H;

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("island: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);

    draw::shadow(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R);
    glass::fill_glass(&mut pixmap, &crate::ShellState::wallpaper_name(), 0.0, 0.0, w as f32, h as f32, CARD_R, ((state.panel_size.0 as f32 - w as f32) / 2.0).max(0.0), crate::PANEL_HEIGHT as f32 + 4.0, state.dark, card);
    draw::stroke_round_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, CARD_R, 1.0, sep);

    draw::text_bold(
        &mut pixmap,
        PAD as f32,
        (PAD + 3.0) as f32,
        (CARD_W - PAD * 2.0) as f32,
        18.0,
        13.0,
        "Dynamic Island",
        fg,
    );

    // CLIPBOARD section
    let mut y = PAD + HEAD_H;
    draw::text(
        &mut pixmap,
        PAD as f32,
        (y + 4.0) as f32,
        (CARD_W - PAD * 2.0) as f32,
        14.0,
        10.5,
        "CLIPBOARD",
        fg_dim,
    );
    y += SEC_H;
    if clips.is_empty() {
        draw::text(
            &mut pixmap,
            PAD as f32,
            (y + 8.0) as f32,
            (CARD_W - PAD * 2.0) as f32,
            16.0,
            12.0,
            "Copy something — it lands here.",
            fg_dim,
        );
        y += ROW_H;
    } else {
        for (i, text) in clips.iter().enumerate() {
            let ry = PAD + HEAD_H + SEC_H + i as f64 * ROW_H;
            if hover == Some(Hit::ClipRow(i)) {
                draw::fill_round_rect(
                    &mut pixmap,
                    6.0,
                    (ry + 2.0) as f32,
                    (CARD_W - 12.0) as f32,
                    (ROW_H - 4.0) as f32,
                    8.0,
                    sel,
                );
            }
            draw::fill_round_rect(
                &mut pixmap,
                (PAD + 2.0) as f32,
                (ry + 7.0) as f32,
                20.0,
                20.0,
                6.0,
                row_bg,
            );
            draw::text(
                &mut pixmap,
                (PAD + 30.0) as f32,
                (ry + 9.0) as f32,
                (CARD_W - PAD - 38.0) as f32,
                16.0,
                12.0,
                &text.replace('\n', " "),
                fg,
            );
        }
        y += clips.len() as f64 * ROW_H;
    }

    // FILES section (only when files are staged)
    if !files.is_empty() {
        draw::text(
            &mut pixmap,
            PAD as f32,
            (y + 4.0) as f32,
            (CARD_W - PAD * 2.0) as f32,
            14.0,
            10.5,
            "STAGED FILES",
            fg_dim,
        );
        for (i, path) in files.iter().enumerate() {
            let ry = files_base_y + i as f64 * ROW_H;
            if hover == Some(Hit::FileRow(i)) {
                draw::fill_round_rect(
                    &mut pixmap,
                    6.0,
                    (ry + 2.0) as f32,
                    (CARD_W - 12.0) as f32,
                    (ROW_H - 4.0) as f32,
                    8.0,
                    sel,
                );
            }
            let name = path.rsplit('/').next().unwrap_or(path);
            draw::text(
                &mut pixmap,
                (PAD + 6.0) as f32,
                (ry + 9.0) as f32,
                (CARD_W - PAD - 12.0) as f32,
                16.0,
                12.0,
                name,
                fg,
            );
        }
    }

    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
    layer.wl_surface().commit();
}

/// Pointer motion → hovered row changes.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let h = match hit_test(state, x, y) {
        Hit::Backdrop => None,
        h => Some(h),
    };
    if h != state.island_hover {
        state.island_hover = h;
        true
    } else {
        false
    }
}

/// Esc closes the card.
pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    if event.keysym == Keysym::Escape {
        state.close_island();
    }
}
