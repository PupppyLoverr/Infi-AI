//! The launcher overlay: dim backdrop + a centred Spotlight-style card —
//! rounded corners, search field up top, result rows, and a Start-menu-style
//! footer strip with system actions (Settings, Log out).

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, Keysym},
    shell::WaylandSurface,
};
use tiny_skia::{Color, PathBuilder, PixmapMut, Stroke, Transform};
use wayland_client::protocol::wl_shm;

use crate::{draw, ShellState, LAUNCHER_WIDTH};

const INPUT_H: f64 = 48.0;
const ROW_H: f64 = 38.0;
const SEC_H: f64 = 26.0;
const FOOTER_H: f64 = 44.0;
const CARD_R: f32 = 12.0;
const MAX_ROWS: usize = 8;
/// Gap between overlay top edge and the launcher box.
const TOP_PAD_FRAC: f64 = 0.20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Item(usize),
    /// Footer action: 0 = Settings, 1 = Log out.
    Action(u8),
    Input,
    List,
    Backdrop,
}

fn box_top(h: u32) -> f64 {
    (h as f64 * TOP_PAD_FRAC).max(56.0)
}

fn box_left(w: u32) -> f64 {
    ((w as f64 - LAUNCHER_WIDTH as f64) / 2.0).max(8.0)
}

fn box_height(n_items: usize) -> f64 {
    INPUT_H + SEC_H + n_items.min(MAX_ROWS) as f64 * ROW_H + FOOTER_H
}

fn footer_top(h: u32, n_items: usize) -> f64 {
    box_top(h) + box_height(n_items) - FOOTER_H
}

pub fn hit_test(x: f64, y: f64, size: (u32, u32), n_items: usize) -> Hit {
    let left = box_left(size.0);
    let top = box_top(size.1);
    let height = box_height(n_items);
    if x < left || x > left + LAUNCHER_WIDTH as f64 || y < top || y > top + height {
        return Hit::Backdrop;
    }
    let ry = y - top;
    if ry >= height - FOOTER_H {
        // Two footer buttons: Settings (left), Log out (right).
        let btn_w = 104.0;
        let mid = LAUNCHER_WIDTH as f64;
        if x < left + 8.0 + btn_w {
            return Hit::Action(0);
        }
        if x > left + mid - 8.0 - btn_w {
            return Hit::Action(1);
        }
        return Hit::List;
    }
    if ry < INPUT_H {
        return Hit::Input;
    }
    let idx = ((ry - INPUT_H - SEC_H) / ROW_H) as usize;
    if ry >= INPUT_H + SEC_H && idx < n_items.min(MAX_ROWS) {
        Hit::Item(idx)
    } else {
        Hit::List
    }
}

fn theme(dark: bool) -> (Color, Color, Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x00, 0x00, 0x00, 0x90), // backdrop dim
            Color::from_rgba8(0x1A, 0x1B, 0x1E, 0xF2), // card
            Color::from_rgba8(0x30, 0x31, 0x35, 0xFF), // selection
            Color::from_rgba8(0x3C, 0x3D, 0x42, 0xFF), // border
            Color::from_rgba8(0x24, 0x25, 0x29, 0xFF), // input field
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x8C, 0x8C, 0x92, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x66),
            Color::from_rgba8(0xFA, 0xFA, 0xFB, 0xF6),
            Color::from_rgba8(0xE0, 0xE0, 0xE2, 0xFF),
            Color::from_rgba8(0xC4, 0xC4, 0xC8, 0xFF),
            Color::from_rgba8(0xEF, 0xEF, 0xF1, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x6A, 0x6A, 0x6E, 0xFF),
        )
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
    let searching = !state.launcher_query.is_empty();
    let (bg, box_bg, sel_bg, sep, input_bg, fg, fg_dim) = theme(state.dark);

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

    draw::fill_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, bg);

    let left = box_left(w) as f32;
    let top = box_top(h) as f32;
    let height = box_height(n_items) as f32;
    // Card: rounded fill + hairline border.
    draw::fill_round_rect(
        &mut pixmap,
        left,
        top,
        LAUNCHER_WIDTH as f32,
        height,
        CARD_R,
        box_bg,
    );
    draw::stroke_round_rect(
        &mut pixmap,
        left + 0.5,
        top + 0.5,
        LAUNCHER_WIDTH as f32 - 1.0,
        height - 1.0,
        CARD_R - 0.5,
        1.0,
        sep,
    );

    // Search field: inset rounded rect + magnifier glyph + query + caret.
    let field_x = left + 12.0;
    let field_y = top + 10.0;
    let field_w = LAUNCHER_WIDTH as f32 - 24.0;
    let field_h = INPUT_H as f32 - 20.0;
    draw::fill_round_rect(
        &mut pixmap,
        field_x,
        field_y,
        field_w,
        field_h,
        6.0,
        input_bg,
    );
    magnifier(&mut pixmap, field_x + 14.0, field_y + field_h / 2.0, fg_dim);

    let query_display = if state.launcher_query.is_empty() {
        "Search apps and actions"
    } else {
        state.launcher_query.as_str()
    };
    let query_color = if state.launcher_query.is_empty() {
        fg_dim
    } else {
        fg
    };
    draw::text(
        &mut pixmap,
        field_x + 36.0,
        field_y + (field_h - 18.0) / 2.0,
        field_w - 44.0,
        18.0,
        14.0,
        query_display,
        query_color,
    );
    let caret_x = field_x + 36.0 + state.launcher_query.len() as f32 * 7.8;
    draw::fill_rect(
        &mut pixmap,
        caret_x + 1.0,
        field_y + (field_h - 16.0) / 2.0,
        1.5,
        16.0,
        if state.dark {
            Color::from_rgba8(0xEC, 0xEC, 0xEE, 0xFF)
        } else {
            Color::from_rgba8(0x18, 0x18, 0x1B, 0xFF)
        },
    );

    // Section header.
    let sec_y = top + INPUT_H as f32;
    draw::text(
        &mut pixmap,
        left + 16.0,
        sec_y + (SEC_H as f32 - 14.0) / 2.0,
        200.0,
        14.0,
        11.0,
        if searching {
            "RESULTS"
        } else {
            "APPS & ACTIONS"
        },
        fg_dim,
    );

    // App rows, rounded selection.
    for (idx, app) in apps.iter().take(MAX_ROWS).enumerate() {
        let ry = top + INPUT_H as f32 + SEC_H as f32 + idx as f32 * ROW_H as f32;
        if idx == state.launcher_sel.min(n_items.saturating_sub(1)) && n_items > 0 {
            draw::fill_round_rect(
                &mut pixmap,
                left + 6.0,
                ry + 2.0,
                LAUNCHER_WIDTH as f32 - 12.0,
                ROW_H as f32 - 4.0,
                6.0,
                sel_bg,
            );
        }
        draw::text(
            &mut pixmap,
            left + 20.0,
            ry + (ROW_H as f32 - 18.0) / 2.0,
            LAUNCHER_WIDTH as f32 * 0.6,
            18.0,
            13.0,
            &app.name,
            fg,
        );
        let hint = if app.action.is_some() {
            "Action"
        } else if app.terminal {
            "Terminal"
        } else {
            "App"
        };
        draw::text(
            &mut pixmap,
            left + LAUNCHER_WIDTH as f32 - 14.0 - 60.0,
            ry + (ROW_H as f32 - 14.0) / 2.0,
            60.0,
            14.0,
            11.0,
            hint,
            fg_dim,
        );
    }
    if apps.is_empty() {
        draw::text(
            &mut pixmap,
            left + 20.0,
            sec_y + SEC_H as f32 + 8.0,
            LAUNCHER_WIDTH as f32 - 40.0,
            18.0,
            13.0,
            "No matching applications",
            fg_dim,
        );
    }

    // Footer (Start-style): separator + Settings / Log out buttons.
    let fy = footer_top(h, n_items) as f32;
    draw::fill_rect(
        &mut pixmap,
        left + 1.0,
        fy,
        LAUNCHER_WIDTH as f32 - 2.0,
        1.0,
        sep,
    );
    let by = fy + (FOOTER_H as f32 - 28.0) / 2.0;
    draw::fill_round_rect(&mut pixmap, left + 8.0, by, 104.0, 28.0, 6.0, input_bg);
    draw::text(
        &mut pixmap,
        left + 24.0,
        by + 5.0,
        88.0,
        18.0,
        12.0,
        "Settings",
        fg,
    );
    let rx = left + LAUNCHER_WIDTH as f32 - 8.0 - 104.0;
    draw::fill_round_rect(&mut pixmap, rx, by, 104.0, 28.0, 6.0, input_bg);
    draw::text(
        &mut pixmap,
        rx + 22.0,
        by + 5.0,
        88.0,
        18.0,
        12.0,
        "Log out",
        fg,
    );

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}

/// Small magnifier glyph: circle + handle.
fn magnifier(pixmap: &mut PixmapMut<'_>, cx: f32, cy: f32, color: CtColor) {
    let c = Color::from_rgba8(color.r(), color.g(), color.b(), 0xFF);
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(c),
        anti_alias: true,
        ..Default::default()
    };
    let stroke = Stroke {
        width: 1.4,
        ..Default::default()
    };
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy - 1.0, 4.0);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::default(), None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(cx + 3.0, cy + 2.0);
    pb.line_to(cx + 6.5, cy + 5.5);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::default(), None);
    }
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
