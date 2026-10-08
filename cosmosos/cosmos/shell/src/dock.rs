//! The dock: an edge rail — icons racked along the screen edge like the
//! macOS Dock rotated vertical. Default position is the LEFT edge,
//! vertical and centred (the ref-1 sketch); `right` and `bottom` are
//! Settings options via the `dock_position` config key.
//!
//! The rail reserves an exclusive zone (STRIP px on that edge) so
//! maximised and snapped windows never overlap it. The layer surface is
//! STRIP+OVERHANG wide — the overhang is transparent, input-free, and
//! only used by magnified icons and hover labels popping over windows.
//!
//! Layout: [Cosmos start glyph] | pinned apps | running extras, the
//! whole column/row centred along the rail.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::{compositor::Region, shell::WaylandSurface};
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{desktop::AppEntry, draw, glass, icons, ShellState};

/// Pill thickness — the exclusive zone reserves this + the edge margin.
pub const STRIP: u32 = 60;
/// Gap between the pill and the screen edge (exclusive zone = STRIP+MARGIN).
pub const MARGIN: u32 = 10;
/// Transparent margin beyond the rail where labels/magnified icons pop.
/// Not clickable — the input region covers only the rail band.
pub const OVERHANG: u32 = 128;
const CELL: f64 = 56.0;
const ICON_SZ: f32 = 48.0;
/// Extra icon px at the hover centre and its falloff radius.
const MAG_MAX: f32 = 16.0;
const MAG_SPAN: f64 = 115.0;
const PAD: f64 = 9.0;
const SEP_W: f64 = 13.0;
/// Pinned apps, top→bottom on a vertical rail (desktop ids without
/// ".desktop").
pub const PINNED: &[&str] = &[
    "cosmos-terminal",
    "cosmos-files",
    "cosmos-editor",
    "cosmos-monitor",
    "cosmos-settings",
];

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
}

/// Index of the first running-extra entry, when it isn't adjacent to
/// the start glyph's own separator.
fn first_extra(items: &[DockEntry]) -> Option<usize> {
    items.iter().position(|e| e.extra).filter(|fx| *fx > 1)
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
        extra: false,
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
        if !PINNED.contains(&key.as_str()) && !extras.contains(&key) {
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
            extra: false,
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
        });
    }
    out
}

/// Content length along the rail axis for the current item set.
fn content_len(items: &[DockEntry]) -> f64 {
    let n = items.len() as f64;
    let extra_sep = if first_extra(items).is_some() {
        SEP_W
    } else {
        0.0
    };
    n * CELL + SEP_W + extra_sep
}

/// Requested surface size for `pos` — the rail axis size is 0 so the
/// compositor spans the whole edge.
pub fn surface_size(pos: DockPos) -> (u32, u32) {
    match pos {
        DockPos::Left | DockPos::Right => (STRIP + OVERHANG, 0),
        DockPos::Bottom => (0, STRIP + OVERHANG),
    }
}

fn theme(dark: bool) -> (Color, Color, Color, CtColor) {
    if dark {
        (
            Color::from_rgba8(0x16, 0x17, 0x1A, 0xD8), // rail
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
/// the cursor (macOS dock zoom), evaluated along the rail axis.
fn mag(hover: Option<f64>, cell_center: f64) -> f32 {
    let Some(hx) = hover else {
        return 0.0;
    };
    let t = (1.0 - ((hx - cell_center).abs() / MAG_SPAN).min(1.0)) as f32;
    MAG_MAX * t * t * (3.0 - 2.0 * t)
}

/// Surface-local rect of the floating pill (also the input region —
/// the overhang stays click-through). `content` is the icon column's
/// painted length so the pill hugs the icons plus PAD at both ends.
fn rail_rect(pos: DockPos, w: u32, h: u32, content: f64) -> (i32, i32, i32, i32) {
    let len = (content + 2.0 * PAD) as i32;
    match pos {
        DockPos::Left => (
            MARGIN as i32,
            ((h as f64 - len as f64) / 2.0) as i32,
            STRIP as i32,
            len,
        ),
        DockPos::Right => (
            w as i32 - MARGIN as i32 - STRIP as i32,
            ((h as f64 - len as f64) / 2.0) as i32,
            STRIP as i32,
            len,
        ),
        DockPos::Bottom => (
            ((w as f64 - len as f64) / 2.0) as i32,
            h as i32 - MARGIN as i32 - STRIP as i32,
            len,
            STRIP as i32,
        ),
    }
}

/// Along-axis span of the rail (surface height for vertical docks,
/// width for bottom).
fn axis_span(pos: DockPos, w: u32, h: u32) -> f64 {
    match pos {
        DockPos::Left | DockPos::Right => h as f64,
        DockPos::Bottom => w as f64,
    }
}

/// Map a rail-space point (t = along axis, n = across, 0 = rail's
/// screen-ward edge) to surface-local (x, y).
fn axis_xy(pos: DockPos, t: f64, n: f64) -> (f64, f64) {
    match pos {
        DockPos::Left => (n, t),
        DockPos::Right => (OVERHANG as f64 + n, t),
        DockPos::Bottom => (t, OVERHANG as f64 + n),
    }
}

/// Repaint the dock surface.
pub fn draw(state: &mut ShellState) {
    let pos = state.dock_position;
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
    let vertical = pos != DockPos::Bottom;

    let stride = w as i32 * 4;
    let Ok((buffer, canvas)) =
        state
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Abgr8888)
    else {
        tracing::warn!("dock: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, w, h) else {
        return;
    };
    // The rail band is the only painted part; the overhang must be
    // explicitly cleared or stale pool memory shows through.
    pixmap.fill(Color::TRANSPARENT);

    // The floating pill: MARGIN px off the screen edge, vertically
    // centred, sized to its icons. Glass samples the wallpaper at the
    // pill's true screen position (the surface itself is edge-anchored
    // at x=0 for Left).
    let content = content_len(&items);
    let (rx, ry, rw, rh) = rail_rect(pos, w, h, content);
    let (sx, sy) = match pos {
        DockPos::Left => (rx as f32, ry as f32),
        DockPos::Right => ((state.panel_size.0 as f32 - MARGIN as f32 - STRIP as f32).max(0.0), ry as f32),
        DockPos::Bottom => (rx as f32, (state.panel_size.1 as f32 - MARGIN as f32 - STRIP as f32).max(0.0)),
    };
    glass::fill_glass(&mut pixmap, &crate::ShellState::wallpaper_name(), rx as f32, ry as f32, rw as f32, rh as f32, 22.0, sx, sy, state.dark, bg);

    // Icon column/row centred along the rail axis.
    let span = axis_span(pos, w, h);
    let mut a = ((span - content) / 2.0).max(PAD);
    let extra_sep_idx = first_extra(&items);
    for (idx, item) in items.iter().enumerate() {
        if idx == 1 || Some(idx) == extra_sep_idx {
            // Separator after the start glyph, and between pinned
            // apps and running extras (macOS dock divider).
            let (sx, sy) = axis_xy(pos, a + SEP_W / 2.0, STRIP as f64 / 2.0);
            let (sw, sh) = if vertical { (22.0, 1.0) } else { (1.0, 22.0) };
            draw::fill_rect(
                &mut pixmap,
                (sx - sw / 2.0) as f32,
                (sy - sh / 2.0) as f32,
                sw as f32,
                sh as f32,
                sep,
            );
            a += SEP_W;
        }
        let cell_center = a + CELL / 2.0;
        let hovered = dock_hover
            .map(|hx| hx >= a && hx < a + CELL)
            .unwrap_or(false);
        if hovered || item.focused {
            let (hx, hy) = axis_xy(pos, a + 2.0, 2.0);
            let (hw, hh) = if vertical {
                (STRIP as f64 - 4.0, CELL - 4.0)
            } else {
                (CELL - 4.0, STRIP as f64 - 4.0)
            };
            draw::fill_round_rect(
                &mut pixmap,
                hx as f32,
                hy as f32,
                hw as f32,
                hh as f32,
                10.0,
                hover_bg,
            );
        }
        // App icons carry their per-app tint; the start glyph and unknown
        // apps stay neutral. Minimized entries render dimmed either way.
        let base_color = if item.icon == "start" {
            draw::accent(state.dark)
        } else {
            icons::tint_for(&item.icon, state.dark).unwrap_or(glyph)
        };
        let icon_color = if item.minimized {
            Color::from_rgba(
                base_color.red(),
                base_color.green(),
                base_color.blue(),
                base_color.alpha() * 0.38,
            )
            .unwrap_or(base_color)
        } else {
            base_color
        };
        // Icons grow symmetrically around the cell centre as they
        // magnify.
        let icon_sz = ICON_SZ + mag(dock_hover, cell_center);
        let (icx, icy) = axis_xy(pos, cell_center, STRIP as f64 / 2.0);
        icons::icon(
            &mut pixmap,
            &item.icon,
            icx as f32 - icon_sz / 2.0,
            icy as f32 - icon_sz / 2.0,
            icon_sz,
            icon_color,
        );
        // Running indicator: a small dot on the rail's inner edge.
        if !item.windows.is_empty() {
            let dot_r = 1.8f32;
            let (dcx, dcy) = axis_xy(pos, cell_center, STRIP as f64 - 6.5);
            let mut pb = tiny_skia::PathBuilder::new();
            pb.push_circle(dcx as f32, dcy as f32, dot_r);
            if let Some(path) = pb.finish() {
                pixmap.fill_path(
                    &path,
                    &tiny_skia::Paint {
                        // Focused app gets the accent dot; running-only
                        // stays dim.
                        shader: tiny_skia::Shader::SolidColor(if item.focused {
                            draw::accent(state.dark)
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
        a += CELL;
    }

    // Hover label: a name pill floating outward into the overhang —
    // right of a left rail, left of a right rail, above a bottom rail.
    if let Some(idx) = dock_hover.and_then(|hx| hit(hx, pos, span, &items)) {
        let item = &items[idx];
        let name: &str =
            item.app
                .as_ref()
                .map(|a| a.name.as_str())
                .unwrap_or(if item.icon == "start" {
                    "Launcher"
                } else {
                    item.icon.as_str()
                });
        // Recompute the hovered cell's centre the same way the cells
        // were laid out (centred column + separators).
        let extra_sep_idx = first_extra(&items);
        let mut a2 = ((span - content_len(&items)) / 2.0).max(PAD);
        let mut t = a2 + CELL / 2.0;
        for i in 0..items.len() {
            if i == 1 || Some(i) == extra_sep_idx {
                a2 += SEP_W;
            }
            if i == idx {
                t = a2 + CELL / 2.0;
                break;
            }
            a2 += CELL;
        }
        let text_est = name.chars().count() as f32 * 7.0;
        let pill_w = text_est + 16.0;
        let (px, py) = match pos {
            DockPos::Left => (STRIP as f32 + 8.0, (t - 10.0) as f32),
            DockPos::Right => ((OVERHANG as f32 - 8.0 - pill_w).max(2.0), (t - 10.0) as f32),
            DockPos::Bottom => {
                let w_span = w as f32;
                (
                    ((t as f32) - pill_w / 2.0)
                        .max(2.0)
                        .min(w_span - pill_w - 2.0),
                    OVERHANG as f32 - 22.0,
                )
            }
        };
        draw::fill_round_rect(&mut pixmap, px, py, pill_w, 20.0, 10.0, bg);
        draw::stroke_round_rect(&mut pixmap, px, py, pill_w, 20.0, 9.5, 1.0, sep);
        draw::text_bold(
            &mut pixmap,
            px + 8.0,
            py + 4.0,
            pill_w - 16.0,
            13.0,
            11.0,
            name,
            fg,
        );
    }

    let wl_surface = layer.wl_surface().clone();
    buffer.attach_to(&wl_surface).ok();
    wl_surface.damage_buffer(0, 0, w as i32, h as i32);
    // Only the pill is clickable — the transparent overhang must let
    // pointer events reach windows underneath.
    let (rx, ry, rw, rh) = rail_rect(pos, w, h, content_len(&items));
    if let Ok(region) = Region::new(&state.compositor_state) {
        region.add(rx, ry, rw, rh);
        wl_surface.set_input_region(Some(region.wl_region()));
    }
    wl_surface.commit();
}

/// Cell index under along-axis coordinate `a`, accounting for the
/// centred column offset + separators.
fn hit(a: f64, pos: DockPos, span: f64, items: &[DockEntry]) -> Option<usize> {
    let _ = pos;
    let mut a0 = ((span - content_len(items)) / 2.0).max(PAD);
    let extra_sep_idx = first_extra(items);
    for idx in 0..items.len() {
        if idx == 1 || Some(idx) == extra_sep_idx {
            a0 += SEP_W;
        }
        if a >= a0 && a < a0 + CELL {
            return Some(idx);
        }
        a0 += CELL;
    }
    None
}

/// Extract the along-axis pointer coordinate for the current position.
fn axis_coord(pos: DockPos, x: f64, y: f64) -> f64 {
    match pos {
        DockPos::Left | DockPos::Right => y,
        DockPos::Bottom => x,
    }
}

/// Start button launches the Cosmos launcher; an app cell focuses its
/// most recent window (unminimizes via the compositor) or launches it.
pub fn click(state: &mut ShellState, x: f64, y: f64) -> bool {
    let pos = state.dock_position;
    let span = axis_span(pos, state.dock_size.0, state.dock_size.1);
    let items = entries(state);
    let Some(idx) = hit(axis_coord(pos, x, y), pos, span, &items) else {
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
        } else {
            state.record_launch(&app.id);
        }
        return true;
    }
    false
}

/// Hover bookkeeping for the icon highlight + magnification. Icon
/// scale changes continuously with the along-axis position, so any
/// move repaints.
pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let coord = axis_coord(state.dock_position, x, y);
    let changed = state.dock_hover != Some(coord);
    state.dock_hover = Some(coord);
    changed
}

/// Pointer left the dock — drop hover + magnification.
pub fn leave(state: &mut ShellState) -> bool {
    state.dock_hover.take().is_some()
}
