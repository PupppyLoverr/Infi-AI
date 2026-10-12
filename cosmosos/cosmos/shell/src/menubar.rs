//! Menubar app menus — macOS-style titles after the focused app's name.
//! Each opens a glass dropdown of real window actions sent over the
//! compositor IPC; items needing a window dim when none is focused.

use cosmic_text::Color as CtColor;
use smithay_client_toolkit::shell::WaylandSurface;
use tiny_skia::{Color, PixmapMut};
use wayland_client::protocol::wl_shm;

use crate::{draw, glass, ShellState};

/// Menu index of the system menu behind the Cosmos mark (app menus
/// are numbered from 0 in title order).
pub const SYSTEM: usize = 100;
/// Menu index of the desktop context menu (right-click on the wallpaper).
pub const DESKTOP: usize = 101;

pub type Items = &'static [(&'static str, Action)];
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
    ChangeWallpaper,
    NewFolder,
    ViewOptions,
    AskCosmos,
    TerminalHere,
    NewFilesWindow,
    /// Type a chord (evdev codes) into the focused window.
    Keys(&'static [u32]),
    Open(Place),
    About,
    Sleep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Trash,
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
                | Action::ChangeWallpaper
                | Action::NewFolder
                | Action::ViewOptions
                | Action::AskCosmos
                | Action::TerminalHere
                | Action::NewFilesWindow
                | Action::Open(_)
                | Action::About
                | Action::Sleep
        )
    }
}

// evdev codes for the chords the Edit, View and Go menus type into apps.
const CTRL: u32 = 29;
const SHIFT: u32 = 42;
const ALT: u32 = 56;

const FILE: Items = &[
    ("New Window", Action::NewWindow),
    ("Close Window", Action::Close),
];
const FILE_FILES: Items = &[
    ("New Window", Action::NewWindow),
    ("New Folder", Action::Keys(&[CTRL, SHIFT, 49])),
    ("Close Window", Action::Close),
];
const FILE_EDITOR: Items = &[
    ("New Window", Action::NewWindow),
    ("Open…", Action::Keys(&[CTRL, 24])),
    ("Save", Action::Keys(&[CTRL, 31])),
    ("Close Window", Action::Close),
];
const FILE_DESKTOP: Items = &[
    ("New Files Window", Action::NewFilesWindow),
    ("New Folder", Action::NewFolder),
];
const EDIT: Items = &[
    ("Undo", Action::Keys(&[CTRL, 44])),
    ("Cut", Action::Keys(&[CTRL, 45])),
    ("Copy", Action::Keys(&[CTRL, 46])),
    ("Paste", Action::Keys(&[CTRL, 47])),
    ("Select All", Action::Keys(&[CTRL, 30])),
];
const VIEW: Items = &[
    ("Zoom", Action::Zoom),
    ("Tile Window to Left", Action::TileLeft),
    ("Tile Window to Right", Action::TileRight),
];
const VIEW_FILES: Items = &[
    ("Toggle Preview Pane", Action::Keys(&[CTRL, SHIFT, 25])),
    ("Toggle Hidden Files", Action::Keys(&[CTRL, 35])),
    ("Go to Location…", Action::Keys(&[CTRL, 38])),
];
const VIEW_DESKTOP: Items = &[
    ("Change Wallpaper…", Action::ChangeWallpaper),
    ("Show View Options", Action::ViewOptions),
];
const GO_DESKTOP: Items = &[
    ("Home", Action::Open(Place::Home)),
    ("Desktop", Action::Open(Place::Desktop)),
    ("Documents", Action::Open(Place::Documents)),
    ("Downloads", Action::Open(Place::Downloads)),
    ("Pictures", Action::Open(Place::Pictures)),
    ("Trash", Action::Open(Place::Trash)),
];
const GO_FILES: Items = &[
    ("Back", Action::Keys(&[ALT, 105])),
    ("Forward", Action::Keys(&[ALT, 106])),
    ("Enclosing Folder", Action::Keys(&[ALT, 103])),
    ("Home", Action::Open(Place::Home)),
    ("Desktop", Action::Open(Place::Desktop)),
    ("Documents", Action::Open(Place::Documents)),
    ("Downloads", Action::Open(Place::Downloads)),
    ("Pictures", Action::Open(Place::Pictures)),
    ("Trash", Action::Open(Place::Trash)),
];
const WINDOW: Items = &[
    ("Minimize", Action::Minimize),
    ("Zoom", Action::Zoom),
    ("Tile Window to Left", Action::TileLeft),
    ("Tile Window to Right", Action::TileRight),
    ("Move to Next Workspace", Action::NextWorkspace),
];
const HELP: Items = &[
    ("Keyboard Shortcuts", Action::Shortcuts),
    ("Search Apps and Files", Action::Search),
];
const SYSTEM_ITEMS: Items = &[
    ("About CosmosOS", Action::About),
    ("System Settings…", Action::Settings),
    ("Sleep", Action::Sleep),
    ("Restart…", Action::Restart),
    ("Shut Down…", Action::Shutdown),
    ("Lock Screen", Action::Lock),
    ("Log Out…", Action::Logout),
];
pub const DESKTOP_ITEMS: Items = &[
    ("Change Wallpaper…", Action::ChangeWallpaper),
    ("New Folder", Action::NewFolder),
    ("Show View Options", Action::ViewOptions),
    ("Ask Cosmos…", Action::AskCosmos),
    ("Open Terminal Here", Action::TerminalHere),
];

/// Menubar titles and items for the focused app id; `None` is the bare
/// desktop, which gets Finder's set.
pub fn menus(app: Option<&str>) -> Vec<(&'static str, Items)> {
    match app {
        None => vec![
            ("File", FILE_DESKTOP),
            ("Edit", EDIT),
            ("View", VIEW_DESKTOP),
            ("Go", GO_DESKTOP),
            ("Window", WINDOW),
            ("Help", HELP),
        ],
        Some("cosmos.files") => vec![
            ("File", FILE_FILES),
            ("Edit", EDIT),
            ("View", VIEW_FILES),
            ("Go", GO_FILES),
            ("Window", WINDOW),
            ("Help", HELP),
        ],
        Some(id) => vec![
            ("File", if id == "cosmos.editor" { FILE_EDITOR } else { FILE }),
            ("Edit", EDIT),
            ("View", VIEW),
            ("Window", WINDOW),
            ("Help", HELP),
        ],
    }
}

fn focused_app(state: &ShellState) -> Option<String> {
    state
        .windows
        .iter()
        .find(|w| w.focused)
        .map(|w| w.app_id.clone())
}

pub fn titles(state: &ShellState) -> Vec<&'static str> {
    menus(focused_app(state).as_deref())
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

/// Items of menu `menu` for the current focus.
pub fn current(state: &ShellState, menu: usize) -> Items {
    match menu {
        SYSTEM => SYSTEM_ITEMS,
        DESKTOP => DESKTOP_ITEMS,
        i => menus(focused_app(state).as_deref())
            .get(i)
            .map(|(_, items)| *items)
            .unwrap_or(&[]),
    }
}

/// (x, w) of each title, laid out from `start` (just after the app name).
pub fn title_rects(start: f64, titles: &[&str]) -> Vec<(f64, f64)> {
    let mut x = start;
    titles
        .iter()
        .map(|t| {
            let w = t.len() as f64 * CHAR_W + TITLE_PAD * 2.0;
            let r = (x, w);
            x += w;
            r
        })
        .collect()
}

pub fn titles_end(start: f64, titles: &[&str]) -> f64 {
    title_rects(start, titles)
        .last()
        .map(|(x, w)| x + w)
        .unwrap_or(start)
}

pub fn surface_size(items: Items) -> (u32, u32) {
    let h = PAD * 2.0 + items.len() as f64 * ROW_H;
    (CARD_W + INSET * 2, h as u32 + INSET * 2)
}

/// Row index under surface-local y.
pub fn item_at(items: Items, x: f64, y: f64) -> Option<usize> {
    let (sw, _) = surface_size(items);
    if x < INSET as f64 || x >= (sw - INSET) as f64 {
        return None;
    }
    let ry = y - INSET as f64 - PAD;
    if ry < 0.0 {
        return None;
    }
    let i = (ry / ROW_H) as usize;
    (i < items.len()).then_some(i)
}

pub fn hover(state: &mut ShellState, x: f64, y: f64) -> bool {
    let h = state.menu_open.and_then(|m| item_at(current(state, m), x, y));
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
    let items = current(state, menu);
    let (w, h) = surface_size(items);
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
    for (row, (label, action)) in items.iter().enumerate() {
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
