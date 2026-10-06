//! The launcher overlay: dim backdrop + centered search box + app list.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, Keysym},
    shell::WaylandSurface,
};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, ShellState, LAUNCHER_WIDTH};

const INPUT_H: f64 = 44.0;
const ROW_H: f64 = 36.0;
const MAX_ROWS: usize = 10;
/// Gap between overlay top edge and the launcher box.
const TOP_PAD_FRAC: f64 = 0.22;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Item(usize),
    Input,
    List,
    Backdrop,
}

fn box_top(h: u32) -> f64 {
    (h as f64 * TOP_PAD_FRAC).max(60.0)
}

fn box_left(w: u32) -> f64 {
    ((w as f64 - LAUNCHER_WIDTH as f64) / 2.0).max(8.0)
}

fn box_height(n_items: usize) -> f64 {
    INPUT_H + n_items.min(MAX_ROWS) as f64 * ROW_H + 16.0
}

pub fn hit_test(x: f64, y: f64, size: (u32, u32), n_items: usize) -> Hit {
    let left = box_left(size.0);
    let top = box_top(size.1);
    let height = box_height(n_items);
    if x < left || x > left + LAUNCHER_WIDTH as f64 || y < top || y > top + height {
        return Hit::Backdrop;
    }
    let ry = y - top;
    if ry < INPUT_H {
        return Hit::Input;
    }
    let idx = ((ry - INPUT_H) / ROW_H) as usize;
    if idx < n_items.min(MAX_ROWS) {
        Hit::Item(idx)
    } else {
        Hit::List
    }
}

pub fn draw(state: &mut ShellState) {
    let (w, h) = state.launcher_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.launcher_surface.clone() else {
        return;
    };
    let apps: Vec<crate::desktop::AppEntry> = state.filtered_apps().into_iter().cloned().collect();
    let n_items = apps.len().min(MAX_ROWS);
    let dark = state.dark;
    let (bg, box_bg, sel_bg, sep, fg, fg_dim) = if dark {
        (
            Color::from_rgba8(0x00, 0x00, 0x00, 0x88),
            Color::from_rgba8(0x18, 0x18, 0x1A, 0xF4),
            Color::from_rgba8(0x2C, 0x2C, 0x30, 0xFF),
            Color::from_rgba8(0x34, 0x34, 0x38, 0xFF),
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x88, 0x88, 0x8C, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x66),
            Color::from_rgba8(0xFA, 0xFA, 0xFA, 0xF6),
            Color::from_rgba8(0xDE, 0xDE, 0xDE, 0xFF),
            Color::from_rgba8(0xC8, 0xC8, 0xC8, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
    };

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
    else {
        tracing::warn!("launcher: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };

    // Backdrop dim.
    draw::fill_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, bg);

    let left = box_left(w) as f32;
    let top = box_top(h) as f32;
    let height = box_height(n_items) as f32;
    draw::fill_rect(
        &mut pixmap,
        left,
        top,
        LAUNCHER_WIDTH as f32,
        height,
        box_bg,
    );
    draw::fill_rect(&mut pixmap, left, top, LAUNCHER_WIDTH as f32, 1.0, sep);
    draw::fill_rect(
        &mut pixmap,
        left,
        top + height - 1.0,
        LAUNCHER_WIDTH as f32,
        1.0,
        sep,
    );
    draw::fill_rect(&mut pixmap, left, top, 1.0, height, sep);
    draw::fill_rect(
        &mut pixmap,
        left + LAUNCHER_WIDTH as f32 - 1.0,
        top,
        1.0,
        height,
        sep,
    );

    // Input line: query + caret.
    let query_display = if state.launcher_query.is_empty() {
        "Type to search".to_string()
    } else {
        state.launcher_query.clone()
    };
    let query_color = if state.launcher_query.is_empty() {
        fg_dim
    } else {
        fg
    };
    draw::text(
        &mut pixmap,
        left + 14.0,
        top + (INPUT_H as f32 - 20.0) / 2.0,
        LAUNCHER_WIDTH as f32 - 28.0,
        20.0,
        14.0,
        &query_display,
        query_color,
    );
    // caret
    let caret_x = left + 14.0 + state.launcher_query.len() as f32 * 7.8;
    draw::fill_rect(
        &mut pixmap,
        caret_x + 2.0,
        top + (INPUT_H as f32 - 18.0) / 2.0,
        1.5,
        18.0,
        if dark {
            Color::from_rgba8(0xEC, 0xEC, 0xEE, 0xFF)
        } else {
            Color::from_rgba8(0x18, 0x18, 0x1B, 0xFF)
        },
    );
    draw::fill_rect(
        &mut pixmap,
        left,
        top + INPUT_H as f32 - 1.0,
        LAUNCHER_WIDTH as f32,
        1.0,
        sep,
    );

    // App rows.
    for (idx, app) in apps.iter().take(MAX_ROWS).enumerate() {
        let ry = top + INPUT_H as f32 + idx as f32 * ROW_H as f32;
        if idx == state.launcher_sel.min(n_items.saturating_sub(1)) && n_items > 0 {
            draw::fill_rect(
                &mut pixmap,
                left + 1.0,
                ry,
                LAUNCHER_WIDTH as f32 - 2.0,
                ROW_H as f32,
                sel_bg,
            );
        }
        draw::text(
            &mut pixmap,
            left + 14.0,
            ry + (ROW_H as f32 - 18.0) / 2.0,
            LAUNCHER_WIDTH as f32 * 0.6,
            18.0,
            13.0,
            &app.name,
            fg,
        );
        let hint = if app.action.is_some() {
            "System"
        } else if app.terminal {
            "Terminal"
        } else {
            app.icon.as_str()
        };
        draw::text(
            &mut pixmap,
            left + LAUNCHER_WIDTH as f32 * 0.62,
            ry + (ROW_H as f32 - 16.0) / 2.0,
            LAUNCHER_WIDTH as f32 * 0.36 - 12.0,
            16.0,
            11.0,
            hint,
            fg_dim,
        );
    }
    if apps.is_empty() {
        draw::text(
            &mut pixmap,
            left + 14.0,
            top + INPUT_H as f32 + (ROW_H as f32 - 18.0) / 2.0,
            LAUNCHER_WIDTH as f32 - 28.0,
            18.0,
            13.0,
            "No matching applications",
            fg_dim,
        );
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}

pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let n = state.filtered_apps().len();
    if let Hit::Item(idx) = hit_test(x, y, state.launcher_size, n) {
        if state.launcher_sel != idx {
            state.launcher_sel = idx;
            return true;
        }
    }
    false
}

pub fn key_press(state: &mut ShellState, event: KeyEvent) {
    match event.keysym {
        Keysym::Escape => {
            state.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        }
        Keysym::Return | Keysym::KP_Enter => state.launch_selected(),
        Keysym::BackSpace => {
            state.launcher_query.pop();
            state.launcher_sel = 0;
            state.launcher_dirty = true;
        }
        Keysym::Down => {
            let n = state.filtered_apps().len();
            if n > 0 {
                state.launcher_sel = (state.launcher_sel + 1).min(n.min(MAX_ROWS) - 1);
                state.launcher_dirty = true;
            }
        }
        Keysym::Up => {
            state.launcher_sel = state.launcher_sel.saturating_sub(1);
            state.launcher_dirty = true;
        }
        Keysym::Tab => {
            let n = state.filtered_apps().len();
            if n > 0 {
                state.launcher_sel = (state.launcher_sel + 1) % n.min(MAX_ROWS);
                state.launcher_dirty = true;
            }
        }
        Keysym::Delete => {}
        _ => {
            if let Some(utf8) = event.utf8.as_deref() {
                for c in utf8.chars() {
                    if !c.is_control() {
                        state.launcher_query.push(c);
                    }
                }
                state.launcher_sel = 0;
                state.launcher_dirty = true;
            }
        }
    }
}
