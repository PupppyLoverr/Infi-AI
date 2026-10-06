//! The top panel: launcher button, task buttons, workspace pager, status.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, ShellState};

const LAUNCH_BTN_W: f64 = 76.0;
const WS_CELL_W: f64 = 26.0;
const TASK_W: f64 = 160.0;

fn theme(dark: bool) -> (Color, Color, Color, CtColor, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x14, 0x14, 0x16, 0xFF),
            Color::from_rgba8(0x22, 0x22, 0x26, 0xFF),
            Color::from_rgba8(0x33, 0x33, 0x38, 0xFF),
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
            CtColor::rgba(0x9A, 0x9A, 0xA0, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xF6, 0xF6, 0xF6, 0xFF),
            Color::from_rgba8(0xEA, 0xEA, 0xEA, 0xFF),
            Color::from_rgba8(0xD8, 0xD8, 0xD8, 0xFF),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
            CtColor::rgba(0x55, 0x55, 0x5A, 0xFF),
        )
    }
}

/// Draw the whole panel into a fresh slot-pool buffer.
pub fn draw(state: &mut ShellState) {
    let (w, h) = state.panel_size;
    if w == 0 {
        return;
    }
    let Some(layer) = state.panel.clone() else { return };
    let (bg, hover_bg, sep, fg, fg_dim) = theme(state.dark);
    let ws_count = crate::workspace_count(state);
    let active_ws = state
        .workspaces
        .iter()
        .find(|ws| ws.focused)
        .map(|ws| ws.id)
        .unwrap_or(0);
    let panel_windows: Vec<cosmos_ipc::WindowInfo> = state
        .windows
        .iter()
        .filter(|w| w.workspace == active_ws || w.minimized)
        .cloned()
        .collect();

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) = state
        .pool
        .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
    else {
        tracing::warn!("panel: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };

    draw::fill_rect(&mut pixmap, 0.0, 0.0, w as f32, h as f32, bg);
    draw::fill_rect(
        &mut pixmap,
        0.0,
        h as f32 - 1.0,
        w as f32,
        1.0,
        sep,
    );

    let mid_y = (h as f32 - 18.0) / 2.0 + 14.0; // text baseline-ish center

    // Cosmos launcher button.
    let hover_launch = state.panel_hover.0 < LAUNCH_BTN_W;
    if hover_launch {
        draw::fill_rect(&mut pixmap, 0.0, 0.0, LAUNCH_BTN_W as f32, h as f32, hover_bg);
    }
    draw::text(
        &mut pixmap,
        10.0,
        mid_y - 14.0,
        LAUNCH_BTN_W as f32 - 10.0,
        18.0,
        13.0,
        "Cosmos",
        fg,
    );
    draw::fill_rect(&mut pixmap, LAUNCH_BTN_W as f32, 0.0, 1.0, h as f32, sep);

    // Task buttons for the active workspace's windows.
    let mut x = LAUNCH_BTN_W + 8.0;
    for win in &panel_windows {
        if x + TASK_W > w as f64 - 400.0 {
            break;
        }
        let focused = win.focused;
        let title = if win.title.is_empty() {
            win.app_id.clone()
        } else {
            win.title.clone()
        };
        if focused {
            draw::fill_rect(&mut pixmap, x as f32, 2.0, TASK_W as f32, h as f32 - 4.0, hover_bg);
        }
        draw::text(
            &mut pixmap,
            x as f32 + 8.0,
            mid_y - 14.0,
            TASK_W as f32 - 12.0,
            18.0,
            12.0,
            &title,
            if win.minimized { fg_dim } else { fg },
        );
        x += TASK_W + 4.0;
    }

    // Right side: status text then workspace pager, measured from the right.
    let status = status_text(&state.sysinfo);
    let status_w = status.len() as f64 * 7.0;
    let status_x = (w as f64 - status_w - 12.0).max(x + 20.0);
    draw::text(
        &mut pixmap,
        status_x as f32,
        mid_y - 14.0,
        status_w as f32 + 12.0,
        18.0,
        12.0,
        &status,
        fg,
    );

    // Workspace pager.
    let mut wsx = status_x - ws_count as f64 * WS_CELL_W - 10.0;
    for ws in &state.workspaces {
        let active = ws.focused;
        if active {
            draw::fill_rect(
                &mut pixmap,
                wsx as f32,
                4.0,
                WS_CELL_W as f32 - 4.0,
                h as f32 - 8.0,
                hover_bg,
            );
        }
        draw::text(
            &mut pixmap,
            wsx as f32 + 7.0,
            mid_y - 14.0,
            WS_CELL_W as f32 - 8.0,
            18.0,
            12.0,
            &(ws.id + 1).to_string(),
            if active { fg } else { fg_dim },
        );
        wsx += WS_CELL_W;
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    wl_surface.commit();
}

fn status_text(info: &crate::sysinfo::SysInfo) -> String {
    let net = if info.network.label.is_empty() {
        if info.network.online {
            "Online".to_string()
        } else {
            "Offline".to_string()
        }
    } else {
        info.network.label.clone()
    };
    let vol = match &info.volume {
        Some(v) if v.muted => "Muted".to_string(),
        Some(v) => format!("Vol {}%", (v.level * 100.0).round() as i32),
        None => "Vol --".to_string(),
    };
    let bat = match &info.battery {
        Some(b) if b.present => format!(
            " {}%{} ",
            b.percent,
            if b.charging { "↑" } else { "" }
        ),
        _ => " ".to_string(),
    };
    format!("{net} · {vol} ·{bat}{}", info.clock)
}

/// Click handler — returns whether a repaint is needed.
pub fn click(state: &mut ShellState, x: f64, y: f64) -> bool {
    let _ = y;
    if x < LAUNCH_BTN_W {
        state.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        return true;
    }
    // Task buttons.
    let active_ws = state
        .workspaces
        .iter()
        .find(|ws| ws.focused)
        .map(|ws| ws.id)
        .unwrap_or(0);
    let wins: Vec<u64> = state
        .windows
        .iter()
        .filter(|w| w.workspace == active_ws || w.minimized)
        .map(|w| w.id)
        .collect();
    let mut bx = LAUNCH_BTN_W + 8.0;
    for id in wins {
        if x >= bx && x < bx + TASK_W {
            state.ipc.send(&cosmos_ipc::Request::FocusWindow { id });
            return true;
        }
        bx += TASK_W + 4.0;
    }
    // Workspace pager.
    let count = crate::workspace_count(state);
    let (w, _) = state.panel_size;
    let status_w = status_text(&state.sysinfo).len() as f64 * 7.0;
    let ws_start = (w as f64 - status_w - 12.0) - count as f64 * WS_CELL_W - 10.0;
    if x >= ws_start && x < ws_start + count as f64 * WS_CELL_W {
        let idx = ((x - ws_start) / WS_CELL_W) as usize;
        if idx < count {
            state.ipc.send(&cosmos_ipc::Request::SwitchWorkspace {
                workspace: idx as u8,
            });
            return true;
        }
    }
    // Status area: open Settings for real adjustments.
    if x >= w as f64 - status_w - 12.0 {
        let _ = std::process::Command::new("cosmos-settings").spawn();
        return false;
    }
    false
}

/// Hover updates the launcher-button highlight.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let _ = y;
    let prev = state.panel_hover;
    state.panel_hover = (x, true);
    (prev.0 < LAUNCH_BTN_W) != (x < LAUNCH_BTN_W)
}
