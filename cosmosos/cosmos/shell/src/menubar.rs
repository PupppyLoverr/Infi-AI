//! Menubar app menus — macOS-style titles after the focused app's name.
//! Each opens a glass dropdown of real window actions sent over the
//! compositor IPC; items needing a window dim when none is focused.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, ShellState};

pub const TITLES: &[&str] = &["File", "Window", "Help"];
/// Menu index of the system menu behind the Cosmos mark.
pub const SYSTEM: usize = 3;
const TITLE_PAD: f64 = 10.0;
const CHAR_W: f64 = 7.5;
/// Shadow inset around the card inside the surface.
pub const INSET: u32 = 12;
const CARD_W: u32 = 232;
const ROW_H: f64 = 28.0;
const PAD: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NewWindow,
    Close,
    Minimize,
    Zoom,
    TileLeft,
    TileRight,
    NextWorkspace,
    Shortcuts,
    Search,
    Settings,
    Lock,
    Logout,
    Restart,
    Shutdown,
}

impl Action {
    pub fn needs_window(self) -> bool {
        !matches!(
            self,
            Action::Shortcuts
                | Action::Search
                | Action::Settings
                | Action::Lock
                | Action::Logout
                | Action::Restart
                | Action::Shutdown
        )
    }
}

pub fn items(menu: usize) -> &'static [(&'static str, Action)] {
    match menu {
        0 => &[
            ("New Window", Action::NewWindow),
            ("Close Window", Action::Close),
        ],
        1 => &[
            ("Minimize", Action::Minimize),
            ("Zoom", Action::Zoom),
            ("Tile Window to Left", Action::TileLeft),
            ("Tile Window to Right", Action::TileRight),
            ("Move to Next Workspace", Action::NextWorkspace),
        ],
        2 => &[
            ("Keyboard Shortcuts", Action::Shortcuts),
            ("Search Apps and Files", Action::Search),
        ],
        _ => &[
            ("Settings…", Action::Settings),
            ("Lock Screen", Action::Lock),
            ("Log Out…", Action::Logout),
            ("Restart…", Action::Restart),
            ("Shut Down…", Action::Shutdown),
        ],
    }
}

/// (x, w) of each title, laid out from `start` (just after the app name).
pub fn title_rects(start: f64) -> Vec<(f64, f64)> {
    let mut x = start;
    TITLES
        .iter()
        .map(|t| {
            let w = t.len() as f64 * CHAR_W + TITLE_PAD * 2.0;
            let r = (x, w);
            x += w;
            r
        })
        .collect()
}

pub fn titles_end(start: f64) -> f64 {
    title_rects(start)
        .last()
        .map(|(x, w)| x + w)
        .unwrap_or(start)
}

pub fn surface_size(menu: usize) -> (u32, u32) {
    let h = PAD * 2.0 + items(menu).len() as f64 * ROW_H;
    (CARD_W + INSET * 2, h as u32 + INSET * 2)
}

/// Row index under surface-local y.
pub fn item_at(menu: usize, x: f64, y: f64) -> Option<usize> {
    let (sw, _) = surface_size(menu);
    if x < INSET as f64 || x >= (sw - INSET) as f64 {
        return None;
    }
    let ry = y - INSET as f64 - PAD;
    if ry < 0.0 {
        return None;
    }
    let i = (ry / ROW_H) as usize;
    (i < items(menu).len()).then_some(i)
}

pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let h = state.menu_open.and_then(|m| item_at(m, x, y));
    if h != state.menu_hover {
        state.menu_hover = h;
        true
    } else {
        false
    }
}

pub fn draw(state: &mut ShellState) {
    let Some(menu) = state.menu_open else { return };
    let Some(layer) = state.menu_surface.clone() else {
        return;
    };
    let (w, h) = surface_size(menu);
    let dark = state.dark;
    let has_window = state.windows.iter().any(|w| w.focused);
    let pal = cosmos_theme::palette(dark);
    let t = |v: cosmos_theme::Rgba| CtColor::rgba(v[0], v[1], v[2], v[3]);
    let (fg, fg_off) = (t(pal.text), t(pal.text_tertiary));
    let [br, bg_, bb, ba] = pal.window;
    let fallback = Color::from_rgba8(br, bg_, bb, ba);

    let (pw, ph) = draw::phys(w, h);
    let Ok((buffer, canvas)) = state.pool.create_buffer(
        pw as i32,
        ph as i32,
        pw as i32 * 4,
        wl_shm::Format::Abgr8888,
    ) else {
        tracing::warn!("menubar: pool create_buffer failed");
        return;
    };
    let Some(mut pixmap) = PixmapMut::from_bytes(canvas, pw, ph) else {
        return;
    };
    pixmap.fill(Color::TRANSPARENT);
    let (i, cw, ch) = (INSET as f32, (w - INSET * 2) as f32, (h - INSET * 2) as f32);
    let r = cosmos_theme::radius::CARD;
    draw::shadow(&mut pixmap, i, i, cw, ch, r);
    let (sx, sy) = state.menu_pos;
    glass::fill_glass(
        &mut pixmap,
        &ShellState::wallpaper_name(),
        i,
        i,
        cw,
        ch,
        r,
        sx as f32 + i,
        sy as f32 + i,
        dark,
        fallback,
    );
    for (row, (label, action)) in items(menu).iter().enumerate() {
        let enabled = has_window || !action.needs_window();
        let ry = i + PAD as f32 + row as f32 * ROW_H as f32;
        let hot = enabled && state.menu_hover == Some(row);
        if hot {
            draw::fill_round_rect(
                &mut pixmap,
                i + 5.0,
                ry,
                cw - 10.0,
                ROW_H as f32,
                cosmos_theme::radius::ROW,
                draw::accent(dark),
            );
        }
        let color = if hot {
            CtColor::rgba(0xFF, 0xFF, 0xFF, 0xFF)
        } else if enabled {
            fg
        } else {
            fg_off
        };
        draw::text(
            &mut pixmap,
            i + 16.0,
            ry + 6.0,
            cw - 32.0,
            18.0,
            13.0,
            label,
            color,
        );
    }
    state.set_viewport(&layer.wl_surface().clone(), w, h);
    buffer.attach_to(layer.wl_surface()).ok();
    layer.wl_surface().damage_buffer(0, 0, pw as i32, ph as i32);
    layer.wl_surface().commit();
}
