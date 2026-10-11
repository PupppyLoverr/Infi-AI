//! The dock: a floating glass pill on the screen edge, macOS-style.
//! Default position is the BOTTOM edge, centred (ref-3); `left` and
//! `right` are Settings options via the `dock_position` config key.
//!
//! The pill floats MARGIN px off the edge and reserves an exclusive
//! zone of STRIP+MARGIN so maximised and snapped windows end before it.
//! The layer surface also carries a transparent, input-free OVERHANG
//! for hover labels and magnified icons.
//!
//! Layout: launcher | pinned apps | running extras | Trash, the whole
//! row/column centred along the rail.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{compositor::Region, shell::WaylandSurface};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{desktop::AppEntry, draw, glass, icons, ShellState};

/// Pill thickness across the rail: 56px icon + 10px padding each side.
pub const STRIP: u32 = 76;
/// Gap between the pill and the screen edge (exclusive zone = STRIP+MARGIN).
pub const MARGIN: u32 = 8;
/// Transparent margin beyond the rail where labels/magnified icons pop.
/// Not clickable — the input region covers only the pill.
pub const OVERHANG: u32 = 128;
const ICON_SZ: f64 = 56.0;
/// Icon pitch along the rail: icon + 6px gap.
const CELL: f64 = 62.0;
/// Pill padding: icon to pill edge, across the rail and at both ends
/// (each cell already carries half a gap).
const INSET: f64 = 10.0;
const PAD: f64 = INSET - (CELL - ICON_SZ) / 2.0;
const SEP_W: f64 = 13.0;
const RADIUS: f32 = 22.0;
/// Magnification peak (150%) and its falloff over three neighbours.
const MAG_MAX: f64 = ICON_SZ * 0.5;
const MAG_SPAN: f64 = CELL * 3.0;
/// Hover time before an icon's name label appears.
pub const LABEL_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
/// Default pinned apps, top→bottom on a vertical rail (desktop ids
/// without ".desktop"). The user's order lives in `pins_path()`.
pub const PINNED: &[&str] = &[
    "cosmos-terminal",
    "cosmos-files",
    "cosmos-editor",
    "cosmos-monitor",
    "cosmos-settings",
    AGENTS,
];
/// Agents is pinned for good — it has no "Remove from Dock".
pub const AGENTS: &str = "cosmos-agents";

fn pins_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("cosmos/dock-pins")
}

/// Pins from one-id-per-line text; Agents is re-added if missing.
fn parse_pins(text: &str) -> Vec<String> {
    let mut pins: Vec<String> = Vec::new();
    for id in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !pins.iter().any(|p| p == id) {
            pins.push(id.to_string());
        }
    }
    if !pins.iter().any(|p| p == AGENTS) {
        pins.push(AGENTS.to_string());
    }
    pins
}

pub fn load_pins() -> Vec<String> {
    match std::fs::read_to_string(pins_path()) {
        Ok(text) => parse_pins(&text),
        Err(_) => PINNED.iter().map(|s| s.to_string()).collect(),
    }
}

fn save_pins(pins: &[String]) {
    let path = pins_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(err) = std::fs::write(&path, pins.join("\n") + "\n") {
        tracing::warn!("dock: saving pins failed: {err}");
    }
}

/// Keep in Dock / Remove from Dock. Agents can't be removed.
pub fn set_pinned(state: &mut ShellState, id: &str, keep: bool) {
    let has = state.dock_pins.iter().any(|p| p == id);
    if keep && !has {
        state.dock_pins.push(id.to_string());
    } else if !keep && has && id != AGENTS {
        state.dock_pins.retain(|p| p != id);
    } else {
        return;
    }
    save_pins(&state.dock_pins);
    state.dock_dirty = true;
}

/// Window ids of app `id`, oldest first.
pub fn windows_of(state: &ShellState, id: &str) -> Vec<u64> {
    state
        .windows
        .iter()
        .filter(|w| normalize(&w.app_id) == id)
        .map(|w| w.id)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockPos {
    Left,
    Right,
    Bottom,
}

impl DockPos {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }
}

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
    /// Running extra (not in PINNED) — the extras group gets its own
    /// separator like the macOS dock's pinned/recent divider.
    pub extra: bool,
    /// The Trash cell, always last.
    pub trash: bool,
}

/// A divider sits before the first running extra and before Trash.
fn sep_before(items: &[DockEntry], i: usize) -> bool {
    i > 0 && (items[i].trash || (items[i].extra && !items[i - 1].extra))
}

fn normalize(id: &str) -> String {
    icons::key_for(id)
}

/// Assemble the dock model: start button + pinned apps + running extras.
fn entries(state: &ShellState) -> Vec<DockEntry> {
    let mut out = vec![DockEntry {
        icon: "launcher".into(),
        app: None,
        windows: Vec::new(),
        focused: false,
        minimized: false,
        extra: false,
        trash: false,
    }];

    let mut extras: Vec<String> = Vec::new();
    for win in &state.windows {
        let key = normalize(&win.app_id);
        if key.is_empty() {
            continue;
        }
        // Every running app not pinned gets a dock cell — taskbar
        // semantics: the dock mirrors all running windows, not just
        // unregistered binaries.
        if !state.dock_pins.contains(&key) && !extras.contains(&key) {
            extras.push(key);
        }
    }

    for id in &state.dock_pins {
        let id = id.as_str();
        let Some(app) = state.apps.iter().find(|a| a.id == id).cloned() else {
            continue;
        };
        let windows: Vec<u64> = state
            .windows
            .iter()
            .filter(|w| normalize(&w.app_id) == id)
            .map(|w| w.id)
            .collect();
        let focused = state
            .windows
            .iter()
            .any(|w| normalize(&w.app_id) == id && w.focused);
        let minimized = !windows.is_empty()
            && state
                .windows
                .iter()
                .filter(|w| normalize(&w.app_id) == id)
                .all(|w| w.minimized);
        out.push(DockEntry {
            icon: id.to_string(),
            app: Some(app),
            windows,
            focused,
            minimized,
            extra: false,
            trash: false,
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
            extra: true,
            trash: false,
        });
    }
    out.push(DockEntry {
        icon: "trash".into(),
        app: None,
        windows: Vec::new(),
        focused: false,
        minimized: false,
        extra: false,
        trash: true,
    });
    out
}

/// Thickness of the dock surface across the rail: the pill, its edge
/// gap, and the transparent overhang for labels and magnified icons.
const THICK: u32 = STRIP + MARGIN + OVERHANG;

pub fn surface_size(pos: DockPos) -> (u32, u32) {
    match pos {
        DockPos::Left | DockPos::Right => (THICK, 0),
        DockPos::Bottom => (0, THICK),
    }
}

/// Glass fallback tint, separator and label text. `fill_glass` adds
/// the 1px inner highlight itself.
fn theme(dark: bool) -> (Color, Color, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x16, 0x17, 0x1A, 0xD8),
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x2A),
            CtColor::rgba(0xEC, 0xEC, 0xEE, 0xFF),
        )
    } else {
        (
            Color::from_rgba8(0xFB, 0xFB, 0xFC, 0xDC),
            Color::from_rgba8(0x00, 0x00, 0x00, 0x22),
            CtColor::rgba(0x18, 0x18, 0x1B, 0xFF),
        )
    }
}

/// Extra length an icon gains under the pointer: a smoothstep bell
/// spanning three neighbours each side, peaking at 150%.
fn mag(hover: Option<f64>, center: f64) -> f64 {
    let Some(hx) = hover else {
        return 0.0;
    };
    let t = 1.0 - ((hx - center).abs() / MAG_SPAN).min(1.0);
    MAG_MAX * t * t * (3.0 - 2.0 * t)
}

/// Along-axis placement of every cell: `(start, len)` per item, the
/// first cell's start and the total content length (separators
/// included). Magnification is measured against the resting layout so
/// the bell doesn't chase its own growth.
fn layout(items: &[DockEntry], span: f64, hover: Option<f64>) -> (Vec<(f64, f64)>, f64, f64) {
    let seps = (0..items.len()).filter(|&i| sep_before(items, i)).count() as f64;
    let rest = items.len() as f64 * CELL + seps * SEP_W;
    let mut a = (span - rest) / 2.0;
    let mut grow = Vec::with_capacity(items.len());
    for i in 0..items.len() {
        if sep_before(items, i) {
            a += SEP_W;
        }
        grow.push(mag(hover, a + CELL / 2.0));
        a += CELL;
    }
    let content = rest + grow.iter().sum::<f64>();
    let start = ((span - content) / 2.0).max(PAD);
    let mut a = start;
    let mut cells = Vec::with_capacity(items.len());
    for (i, g) in grow.into_iter().enumerate() {
        if sep_before(items, i) {
            a += SEP_W;
        }
        cells.push((a, CELL + g));
        a += CELL + g;
    }
    (cells, start, content)
}

/// Surface-local rect of the floating pill (also the input region —
/// the overhang stays click-through).
fn rail_rect(pos: DockPos, start: f64, content: f64) -> (i32, i32, i32, i32) {
    let a = (start - PAD).round() as i32;
    let len = (content + PAD * 2.0).round() as i32;
    match pos {
        DockPos::Bottom => (a, OVERHANG as i32, len, STRIP as i32),
        DockPos::Left => (MARGIN as i32, a, STRIP as i32, len),
        DockPos::Right => (OVERHANG as i32, a, STRIP as i32, len),
    }
}

fn axis_span(pos: DockPos, w: u32, h: u32) -> f64 {
    match pos {
        DockPos::Left | DockPos::Right => h as f64,
        DockPos::Bottom => w as f64,
    }
}

/// Surface point at along-axis `t` and depth `n`, measured from the
/// pill's screen-edge side toward the screen centre.
fn axis_xy(pos: DockPos, t: f64, n: f64) -> (f64, f64) {
    let far = (OVERHANG + STRIP) as f64;
    match pos {
        DockPos::Bottom => (t, far - n),
        DockPos::Left => (MARGIN as f64 + n, t),
        DockPos::Right => (far - n, t),
    }
}

fn dot(pixmap: &mut PixmapMut<'_>, x: f64, y: f64, color: Color) {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.push_circle(x as f32, y as f32, 2.0);
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &tiny_skia::Paint {
                shader: tiny_skia::Shader::SolidColor(color),
                anti_alias: true,
                ..Default::default()
            },
            tiny_skia::FillRule::Winding,
            draw::xf(),
            None,
        );
    }
}

fn label(item: &DockEntry) -> &str {
    if let Some(app) = &item.app {
        return &app.name;
    }
    if item.trash {
        "Trash"
    } else {
        "Launcher"
    }
}

pub fn draw(state: &mut ShellState) {
    let pos = state.dock_position;
    let (w, h) = state.dock_size;
    if w == 0 || h == 0 {
        return;
    }
    let Some(layer) = state.dock_surface.clone() else {
        return;
    };
    let (bg, sep, fg) = theme(state.dark);
    let items = entries(state);
    let hover = state.dock_hover;
    let vertical = pos != DockPos::Bottom;

    let (pw, ph) = draw::phys(w, h);
    let stride = pw as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("dock: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    // The overhang must be cleared or stale pool memory shows through.
    pixmap.fill(Color::TRANSPARENT);

    let span = axis_span(pos, w, h);
    let mag_hover = hover.filter(|_| state.dock_magnify);
    let (cells, start, content) = layout(&items, span, mag_hover);
    let (rx, ry, rw, rh) = rail_rect(pos, start, content);
    // Glass samples the wallpaper at the pill's true screen position.
    let (sx, sy) = match pos {
        DockPos::Left => (rx as f32, ry as f32),
        DockPos::Right => (
            (state.panel_size.0 as f32 - MARGIN as f32 - STRIP as f32).max(0.0),
            ry as f32,
        ),
        DockPos::Bottom => (
            rx as f32,
            (glass::screen_size().1 - MARGIN as f32 - STRIP as f32).max(0.0),
        ),
    };
    draw::shadow(
        &mut pixmap,
        rx as f32,
        ry as f32,
        rw as f32,
        rh as f32,
        RADIUS,
    );
    glass::fill_glass(
        &mut pixmap,
        &crate::ShellState::wallpaper_name(),
        rx as f32,
        ry as f32,
        rw as f32,
        rh as f32,
        RADIUS,
        sx,
        sy,
        state.dark,
        bg,
    );

    let accent = draw::accent(state.dark);
    let running = if state.dark {
        Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x99)
    } else {
        Color::from_rgba8(0x00, 0x00, 0x00, 0x73)
    };
    for (idx, item) in items.iter().enumerate() {
        let (a, len) = cells[idx];
        if sep_before(&items, idx) {
            let (cx, cy) = axis_xy(pos, a - SEP_W / 2.0, STRIP as f64 / 2.0);
            let (sw, sh) = if vertical { (36.0, 1.0) } else { (1.0, 36.0) };
            draw::fill_rect(
                &mut pixmap,
                (cx - sw / 2.0) as f32,
                (cy - sh / 2.0) as f32,
                sw as f32,
                sh as f32,
                sep,
            );
        }
        // Icons grow outward from the pill's screen-edge padding.
        let size = len - (CELL - ICON_SZ);
        let (icx, icy) = axis_xy(pos, a + len / 2.0, INSET + size / 2.0);
        let (ix, iy) = ((icx - size / 2.0) as f32, (icy - size / 2.0) as f32);
        icons::app_tile(&mut pixmap, &item.icon, ix, iy, size as f32);
        if item.minimized {
            // Minimised-only apps read dimmed under a veil of the glass.
            let veil = Color::from_rgba(bg.red(), bg.green(), bg.blue(), 0.45).unwrap_or(bg);
            draw::fill_round_rect(
                &mut pixmap,
                ix,
                iy,
                size as f32,
                size as f32,
                size as f32 * 0.225,
                veil,
            );
        }
        // Running dot: below the icon on a bottom dock; on the side
        // facing the screen centre on a left/right rail.
        if !item.windows.is_empty() {
            let depth = if vertical {
                STRIP as f64 - INSET / 2.0
            } else {
                INSET / 2.0
            };
            let (dx, dy) = axis_xy(pos, a + len / 2.0, depth);
            dot(
                &mut pixmap,
                dx,
                dy,
                if item.focused { accent } else { running },
            );
        }
    }

    // Hover label: a glass name pill floating outward into the overhang,
    // once the pointer has rested on the cell for LABEL_DELAY.
    let label_idx = state
        .dock_label
        .filter(|(_, t0)| t0.elapsed() >= LABEL_DELAY)
        .map(|(i, _)| i)
        .filter(|_| hover.is_some());
    if let Some(idx) = label_idx.filter(|&i| i < cells.len()) {
        let (a, len) = cells[idx];
        let name = label(&items[idx]);
        let pill_w = draw::text_width(12.0, name) + 20.0;
        let pill_h = 26.0f32;
        let size = len - (CELL - ICON_SZ);
        let out = (INSET + size + 8.0) as f32;
        let t = (a + len / 2.0) as f32;
        let (px, py) = match pos {
            DockPos::Bottom => (
                (t - pill_w / 2.0).clamp(2.0, (w as f32 - pill_w - 2.0).max(2.0)),
                (OVERHANG + STRIP) as f32 - out - pill_h,
            ),
            DockPos::Left => (MARGIN as f32 + out, t - pill_h / 2.0),
            DockPos::Right => ((OVERHANG + STRIP) as f32 - out - pill_w, t - pill_h / 2.0),
        };
        draw::shadow(&mut pixmap, px, py, pill_w, pill_h, 13.0);
        let (lsx, lsy) = (px - rx as f32 + sx, py - ry as f32 + sy);
        glass::fill_glass(
            &mut pixmap,
            &crate::ShellState::wallpaper_name(),
            px,
            py,
            pill_w,
            pill_h,
            13.0,
            lsx,
            lsy,
            state.dark,
            bg,
        );
        draw::text_centered(&mut pixmap, px, py + 6.0, pill_w, 15.0, 12.0, name, fg);
    }

    let wl_surface = layer.wl_surface().clone();
    state.set_viewport(&wl_surface, w, h);
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
    // Only the pill is clickable — the transparent overhang must let
    // pointer events reach windows underneath.
    if let Ok(region) = Region::new(&state.compositor_state) {
        region.add(rx, ry, rw, rh);
        wl_surface.set_input_region(Some(region.wl_region()));
    }
    wl_surface.commit();
}

fn hit_cells(a: f64, cells: &[(f64, f64)]) -> Option<usize> {
    cells.iter().position(|&(a0, len)| a >= a0 && a < a0 + len)
}

/// Cell index under along-axis coordinate `a` in the drawn layout.
fn hit(state: &ShellState, a: f64, items: &[DockEntry]) -> Option<usize> {
    let span = axis_span(state.dock_position, state.dock_size.0, state.dock_size.1);
    let hover = state.dock_hover.filter(|_| state.dock_magnify);
    hit_cells(a, &layout(items, span, hover).0)
}

/// Extract the along-axis pointer coordinate for the current position.
fn axis_coord(pos: DockPos, x: f64, y: f64) -> f64 {
    match pos {
        DockPos::Left | DockPos::Right => y,
        DockPos::Bottom => x,
    }
}

/// `~/.local/share/Trash/files` (freedesktop trash), created on demand.
fn trash_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/share")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("Trash/files")
}

/// Launcher toggles Start; Trash opens in Files; an app focuses its
/// latest window, minimises it if it is already focused, or launches.
pub fn click(state: &mut ShellState, button: u32, x: f64, y: f64) -> bool {
    const BTN_RIGHT: u32 = 0x111;
    const BTN_MIDDLE: u32 = 0x112;
    let items = entries(state);
    let Some(idx) = hit(state, axis_coord(state.dock_position, x, y), &items) else {
        return false;
    };
    state.set_menu(None, 0);
    let item = &items[idx];
    if button == BTN_RIGHT {
        open_menu(state, &items, idx);
        return true;
    }
    if button == BTN_MIDDLE {
        if let Some(app) = &item.app {
            if let Err(err) = crate::desktop::launch(app) {
                tracing::warn!("dock: new window {} failed: {err}", app.id);
            }
        }
        return true;
    }
    if item.icon == "launcher" {
        state.ipc.send(&cosmos_ipc::Request::ToggleLauncher);
        return true;
    }
    if item.trash {
        let dir = trash_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(err) = std::process::Command::new("cosmos-files").arg(&dir).spawn() {
            tracing::warn!("dock: open Trash failed: {err}");
        }
        return true;
    }
    if let Some(&id) = item.windows.last() {
        if item.focused && !item.minimized {
            state.ipc.send(&cosmos_ipc::Request::MinimizeWindow { id });
        } else {
            state.ipc.send(&cosmos_ipc::Request::FocusWindow { id });
        }
        return true;
    }
    if let Some(app) = &item.app {
        let app = app.clone();
        if let Err(err) = crate::desktop::launch(&app) {
            tracing::warn!("dock launch {} failed: {err}", app.id);
        } else {
            state.record_launch(&app.id);
        }
        return true;
    }
    false
}

/// Right-click: open windows, New Window, Keep/Remove, Quit — a glass
/// card floating off the pill beside the icon.
fn open_menu(state: &mut ShellState, items: &[DockEntry], idx: usize) {
    use crate::menubar::Action;
    let item = &items[idx];
    let Some(app) = item.app.clone() else {
        return;
    };
    let mut rows: Vec<(String, Action)> = item
        .windows
        .iter()
        .filter_map(|id| state.windows.iter().find(|w| w.id == *id))
        .map(|w| {
            let title = if w.title.is_empty() {
                &app.name
            } else {
                &w.title
            };
            let mut t: String = title.chars().take(30).collect();
            if title.chars().count() > 30 {
                t.push('…');
            }
            (t, Action::FocusWindow(w.id))
        })
        .collect();
    rows.push(("New Window".into(), Action::DockNewWindow));
    let key = icons::key_for(&app.id);
    if key != AGENTS {
        if state.dock_pins.contains(&key) {
            rows.push(("Remove from Dock".into(), Action::DockRemove));
        } else {
            rows.push(("Keep in Dock".into(), Action::DockKeep));
        }
    }
    if !item.windows.is_empty() {
        rows.push(("Quit".into(), Action::DockQuit));
    }
    let (sw, sh) = crate::menubar::surface_size(rows.len());
    let inset = crate::menubar::INSET as f64;
    let (cw, ch) = (sw as f64 - inset * 2.0, sh as f64 - inset * 2.0);
    let span = axis_span(state.dock_position, state.dock_size.0, state.dock_size.1);
    let hover = state.dock_hover.filter(|_| state.dock_magnify);
    let (a, len) = layout(items, span, hover).0[idx];
    let mid = a + len / 2.0;
    let (scr_w, scr_h) = glass::screen_size();
    let off = (MARGIN + STRIP) as f64 + 8.0;
    let (x, y) = match state.dock_position {
        DockPos::Bottom => (mid - cw / 2.0, scr_h as f64 - off - ch),
        DockPos::Left => (off, mid - ch / 2.0),
        DockPos::Right => (scr_w as f64 - off - cw, mid - ch / 2.0),
    };
    state.dock_label = None;
    let key_app = state
        .apps
        .iter()
        .find(|a| a.id == key)
        .cloned()
        .or(Some(app));
    state.open_dock_menu(rows, key_app, x.round() as i32, y.round() as i32);
}

/// Hover bookkeeping for labels + magnification; any move repaints.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let coord = axis_coord(state.dock_position, x, y);
    let items = entries(state);
    let cell = hit(state, coord, &items);
    let changed = state.dock_hover != Some(coord);
    state.dock_hover = Some(coord);
    let current = state.dock_label.map(|(i, _)| i);
    if cell != current {
        let now = std::time::Instant::now();
        // Sliding along the dock keeps labels up; a fresh hover waits.
        let shown = state
            .dock_label
            .is_some_and(|(_, t0)| t0.elapsed() >= LABEL_DELAY);
        state.dock_label = cell.map(|i| (i, if shown { now - LABEL_DELAY } else { now }));
        if let (Some(i), false) = (cell, shown) {
            let _ = state.loop_handle.insert_source(
                calloop::timer::Timer::from_duration(LABEL_DELAY),
                move |_, _, state| {
                    if state.dock_label.is_some_and(|(j, t0)| j == i && t0 == now) {
                        state.dock_dirty = true;
                    }
                    calloop::timer::TimeoutAction::Drop
                },
            );
        }
        return true;
    }
    changed && state.dock_magnify
}

/// Pointer left the dock — drop hover + magnification.
pub fn leave(state: &mut ShellState) -> bool {
    state.dock_label = None;
    state.dock_hover.take().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(extra: bool, trash: bool) -> DockEntry {
        DockEntry {
            icon: "x".into(),
            app: None,
            windows: Vec::new(),
            focused: false,
            minimized: false,
            extra,
            trash,
        }
    }

    #[test]
    fn resting_layout_matches_spec_geometry() {
        // launcher, 2 pinned, 1 extra, trash: 5 cells + 2 separators.
        let items = [
            entry(false, false),
            entry(false, false),
            entry(false, false),
            entry(true, false),
            entry(false, true),
        ];
        let (cells, start, content) = layout(&items, 1536.0, None);
        assert_eq!(content, 5.0 * CELL + 2.0 * SEP_W);
        assert_eq!(start, (1536.0 - content) / 2.0);
        assert_eq!(cells[1].0 - cells[0].0, CELL);
        assert_eq!(cells[3].0 - cells[2].0, CELL + SEP_W);
        // Icon 56, gap 6, 10px padding from icon to pill edge.
        assert_eq!(CELL - ICON_SZ, 6.0);
        assert_eq!(cells[0].0 + (CELL - ICON_SZ) / 2.0 - (start - PAD), INSET);
        assert_eq!(STRIP as f64, ICON_SZ + 2.0 * INSET);
    }

    #[test]
    fn magnification_peaks_at_150_and_spans_three_neighbours() {
        assert_eq!(mag(Some(100.0), 100.0), ICON_SZ * 0.5);
        assert!(mag(Some(100.0 + CELL * 2.9), 100.0) > 0.0);
        assert_eq!(mag(Some(100.0 + CELL * 3.0), 100.0), 0.0);
    }

    #[test]
    fn pins_dedupe_and_always_keep_agents() {
        let pins = parse_pins("cosmos-files\n\ncosmos-files\nfirefox\n");
        assert_eq!(pins, vec!["cosmos-files", "firefox", AGENTS]);
        assert!(PINNED.contains(&AGENTS));
    }

    #[test]
    fn separators_before_extras_and_trash_only() {
        let items = [
            entry(false, false),
            entry(false, false),
            entry(true, false),
            entry(true, false),
            entry(false, true),
        ];
        let seps: Vec<_> = (0..items.len())
            .filter(|&i| sep_before(&items, i))
            .collect();
        assert_eq!(seps, vec![2, 4]);
    }
}
