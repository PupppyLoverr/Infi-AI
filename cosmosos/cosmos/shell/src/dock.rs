//! The dock: a floating bottom-centre strip merging the macOS Dock
//! (icon strip, running dots, overlays windows — no exclusive zone) with
//! the Win11 taskbar's centred layout and click-to-focus/launch.
//!
//! Layout: [Cosmos start glyph] | pinned apps | running extras.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{compositor::Region, shell::WaylandSurface};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{desktop::AppEntry, draw, icons, ShellState};

pub const DOCK_H: u32 = 54;
/// Extra surface height above the card so magnified icons can pop
/// over its top edge (macOS Dock). Input stays clipped to the card.
pub const MAG_ROOM: u32 = 24;
pub const SURFACE_H: u32 = DOCK_H + MAG_ROOM;
const CELL_W: f64 = 46.0;
const ICON_SZ: f32 = 28.0;
/// Extra icon px at the hover centre and its falloff radius.
const MAG_MAX: f32 = 16.0;
const MAG_SPAN: f64 = 115.0;
const PAD: f64 = 9.0;
const SEP_W: f64 = 13.0;
const DOCK_R: f32 = 15.0;
/// Pinned apps, left→right (desktop ids without ".desktop").
pub const PINNED: &[&str] = &[
    "cosmos-terminal",
    "cosmos-files",
    "cosmos-editor",
    "cosmos-monitor",
    "cosmos-settings",
];

/// One icon cell on the dock.
pub struct DockEntry {
    /// Icon key for `icons::icon` — desktop id or "start"/"generic".
    pub icon: String,
    pub app: Option<AppEntry>,
    /// Compositor window ids belonging to this entry.
    pub windows: Vec<u64>,
    pub focused: bool,
    /// Every window of this entry is minimized.
    pub minimized: bool,
}

fn normalize(id: &str) -> String {
    icons::key_for(id)
}

/// Assemble the dock model: start button + pinned apps + running extras.
fn entries(state: &ShellState) -> Vec<DockEntry> {
    let mut out = vec![DockEntry {
        icon: "start".into(),
        app: None,
        windows: Vec::new(),
        focused: false,
        minimized: false,
    }];

    let mut extras: Vec<String> = Vec::new();
    for win in &state.windows {
        let key = normalize(&win.app_id);
        if key.is_empty() {
            continue;
        }
        if !PINNED.contains(&key.as_str())
            && !state.apps.iter().any(|a| a.id == key)
            && !extras.contains(&key)
        {
            extras.push(key);
        }
    }

    for id in PINNED {
        let Some(app) = state.apps.iter().find(|a| a.id == *id).cloned() else {
            continue;
        };
        let windows: Vec<u64> = state
            .windows
            .iter()
            .filter(|w| normalize(&w.app_id) == *id)
            .map(|w| w.id)
            .collect();
        let focused = state
            .windows
            .iter()
            .any(|w| normalize(&w.app_id) == *id && w.focused);
        let minimized = !windows.is_empty()
            && state
                .windows
                .iter()
                .filter(|w| normalize(&w.app_id) == *id)
                .all(|w| w.minimized);
        out.push(DockEntry {
            icon: id.to_string(),
            app: Some(app),
            windows,
            focused,
            minimized,
        });
    }

    for key in extras {
        let app = state.apps.iter().find(|a| a.id == key).cloned();
        let windows: Vec<u64> = state
            .windows
            .iter()
            .filter(|w| normalize(&w.app_id) == key)
            .map(|w| w.id)
            .collect();
        if windows.is_empty() {
            continue;
        }
        let focused = state
            .windows
            .iter()
            .any(|w| normalize(&w.app_id) == key && w.focused);
        out.push(DockEntry {
            icon: app.as_ref().map(|a| icons::key_for(&a.id)).unwrap_or(key),
            app,
            windows,
            focused,
            minimized: false,
        });
    }
    out
}

/// Dock content width in px for the current item set.
pub fn desired_width(state: &ShellState) -> u32 {
    let n = entries(state).len() as f64;
    (PAD * 2.0 + n * CELL_W + SEP_W) as u32
}

fn theme(dark: bool) -> (Color, Color, Color, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x16, 0x17, 0x1A, 0xD8), // card
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x2E), // hover cell
            Color::from_rgba8(0x45, 0x46, 0x4C, 0x80), // separator
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFB, 0xFB, 0xFC, 0xDC),
            Color::from_rgba8(0x00, 0x00, 0x00, 0x18),
            Color::from_rgba8(0xC0, 0xC0, 0xC6, 0xAA),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
        )
    }
}

/// Icon magnification: smoothstep bell over MAG_SPAN, peaking under
/// the cursor (macOS dock zoom).
fn mag(hover: Option<f64>, cell_center: f64) -> f32 {
    let Some(hx) = hover else {
        return 0.0;
    };
    let t = (1.0 - ((hx - cell_center).abs() / MAG_SPAN).min(1.0)) as f32;
    MAG_MAX * t * t * (3.0 - 2.0 * t)
}

/// Repaint the dock surface.
pub fn draw(state: &mut ShellState) {
    let (w, h) = state.dock_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.dock_surface.clone() else {
        return;
    };
    let (bg, hover_bg, sep, fg) = theme(state.dark);
    let items = entries(state);
    let dock_hover = state.dock_hover;
    let glyph = Color::from_rgba8(fg.r(), fg.g(), fg.b(), fg.a());
    // The card sits at the bottom of the taller surface.
    let card_y = MAG_ROOM as f32;

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
    else {
        tracing::warn!("dock: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };

    // Floating card.
    draw::fill_round_rect(
        &mut pixmap,
        0.5,
        card_y + 0.5,
        w as f32 - 1.0,
        DOCK_H as f32 - 1.0,
        DOCK_R,
        bg,
    );
    draw::stroke_round_rect(
        &mut pixmap,
        0.5,
        card_y + 0.5,
        w as f32 - 1.0,
        DOCK_H as f32 - 1.0,
        DOCK_R - 0.5,
        1.0,
        sep,
    );

    let mut x = PAD;
    for (idx, item) in items.iter().enumerate() {
        if idx == 1 {
            // Separator after the start glyph.
            draw::fill_rect(
                &mut pixmap,
                (x + SEP_W / 2.0) as f32,
                h as f32 / 2.0 - 11.0,
                1.0,
                22.0,
                sep,
            );
            x += SEP_W;
        }
        let hovered = dock_hover
            .map(|hx| hx >= x && hx < x + CELL_W)
            .unwrap_or(false);
        if hovered || item.focused {
            draw::fill_round_rect(
                &mut pixmap,
                x as f32 + 2.0,
                card_y + 4.0,
                CELL_W as f32 - 4.0,
                DOCK_H as f32 - 8.0,
                10.0,
                hover_bg,
            );
        }
        let icon_color = if item.minimized {
            Color::from_rgba8(fg.r(), fg.g(), fg.b(), 0x90)
        } else {
            glyph
        };
        let cell_center = x + CELL_W / 2.0;
        // Bottom edge of the icon stays anchored as it magnifies.
        let icon_sz = ICON_SZ + mag(dock_hover, cell_center);
        let icon_bottom = card_y + DOCK_H as f32 - 9.0;
        icons::icon(
            &mut pixmap,
            &item.icon,
            cell_center as f32 - icon_sz / 2.0,
            icon_bottom - icon_sz,
            icon_sz,
            icon_color,
        );
        // Running indicator: a small dot centred under the icon.
        if !item.windows.is_empty() {
            let dot_r = 1.8f32;
            let cx = cell_center as f32;
            let cy = card_y + DOCK_H as f32 - 6.5;
            let mut pb = tiny_skia::PathBuilder::new();
            pb.push_circle(cx, cy, dot_r);
            if let Some(path) = pb.finish() {
                pixmap.fill_path(
                    &path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(if item.focused {
                            glyph
                        } else {
                            Color::from_rgba8(fg.r(), fg.g(), fg.b(), 0x90)
                        }),
                        anti_alias: true,
                        ..Default::default()
                    },
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::default(),
                    None,
                );
            }
        }
        x += CELL_W;
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    // Only the card band is clickable — the transparent overhang above
    // it must let pointer events reach windows underneath.
    if let Ok(region) = Region::new(&state.compositor_state) {
        region.add(0, MAG_ROOM as i32, w as i32, DOCK_H as i32);
        wl_surface.set_input_region(Some(region.wl_region()));
    }
    wl_surface.commit();
}

/// Click on the dock — returns the entry index under the cursor.
fn hit(x: f64, n_items: usize) -> Option<usize> {
    let mut cx = PAD;
    for idx in 0..n_items {
        if idx == 1 {
            cx += SEP_W;
        }
        if x >= cx && x < cx + CELL_W {
            return Some(idx);
        }
        cx += CELL_W;
    }
    None
}

/// Start button launches the Cosmos launcher; an app cell focuses its
/// most recent window (unminimizes via the compositor) or launches it.
pub fn click(state: &mut ShellState, x: f64) -> bool {
    let items = entries(state);
    let Some(idx) = hit(x, items.len()) else {
        return false;
    };
    let item = &items[idx];
    if item.icon == "start" {
        state.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        return true;
    }
    if let Some(&id) = item.windows.last() {
        state.ipc.send(&cosmos_ipc::Request::FocusWindow { id });
        return true;
    }
    if let Some(app) = &item.app {
        let app = app.clone();
        if let Err(err) = crate::desktop::launch(&app) {
            tracing::warn!("dock launch {} failed: {err}", app.id);
        }
        return true;
    }
    false
}

/// Hover bookkeeping for the icon highlight + magnification. Icon
/// scale changes continuously with x, so any move repaints.
pub fn hover(state: &mut ShellState, x: f64) -> bool {
    let changed = state.dock_hover != Some(x);
    state.dock_hover = Some(x);
    changed
}

/// Pointer left the dock — drop hover + magnification.
pub fn leave(state: &mut ShellState) -> bool {
    state.dock_hover.take().is_some()
}
