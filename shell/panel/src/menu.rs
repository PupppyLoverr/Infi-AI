//! The menu bar's dropdown menus. Every item does something real: a niri
//! action, opening a folder, a shell command, or a power action behind a
//! confirmation. There are no per-application menus (Wayland has no global
//! menu protocol); the app menu offers what the shell can do to any app.
//!
//! A menu is drawn on a full-screen transparent overlay: a click anywhere
//! outside the menu closes it, as on macOS, and moving across the bar's other
//! titles while a menu is open switches to their menu.

use crate::canvas::{text_width, Canvas, Fonts};
use crate::state::{LaunchTarget, ShellCmd};
use crate::surfaces::{HitAction, Region};
use crate::tokens::{Theme, T_LABEL};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    Solyra,
    App,
    Go,
    Window,
    View,
    Help,
}

impl MenuId {
    /// The titles after the app name, in bar order.
    pub const TITLES: [MenuId; 4] = [MenuId::Go, MenuId::Window, MenuId::View, MenuId::Help];

    pub fn title(self) -> &'static str {
        match self {
            Self::Solyra => "Solyra",
            Self::App => "App",
            Self::Go => "Go",
            Self::Window => "Window",
            Self::View => "View",
            Self::Help => "Help",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub shortcut: Option<&'static str>,
    /// `None` is a separator.
    pub action: Option<HitAction>,
    /// Shown instead of acting on the first click (shut down, restart).
    pub confirm: Option<&'static str>,
}

fn item(label: &str, shortcut: Option<&'static str>, action: HitAction) -> Entry {
    Entry {
        label: label.into(),
        shortcut,
        action: Some(action),
        confirm: None,
    }
}

fn sep() -> Entry {
    Entry {
        label: String::new(),
        shortcut: None,
        action: None,
        confirm: None,
    }
}

fn ipc(json: &'static str) -> HitAction {
    HitAction::Launch(LaunchTarget::Ipc(json))
}

fn sh(cmd: &str) -> HitAction {
    HitAction::Launch(LaunchTarget::Shell(cmd.into()))
}

const MOVE_TO: [&str; 5] = [
    "{\"Action\":{\"MoveWindowToWorkspace\":{\"window_id\":null,\"reference\":{\"Index\":1},\"focus\":true}}}",
    "{\"Action\":{\"MoveWindowToWorkspace\":{\"window_id\":null,\"reference\":{\"Index\":2},\"focus\":true}}}",
    "{\"Action\":{\"MoveWindowToWorkspace\":{\"window_id\":null,\"reference\":{\"Index\":3},\"focus\":true}}}",
    "{\"Action\":{\"MoveWindowToWorkspace\":{\"window_id\":null,\"reference\":{\"Index\":4},\"focus\":true}}}",
    "{\"Action\":{\"MoveWindowToWorkspace\":{\"window_id\":null,\"reference\":{\"Index\":5},\"focus\":true}}}",
];

/// What the focused app is, for the app menu.
pub struct AppCtx<'a> {
    pub name: &'a str,
    /// Command that opens another window of it, if known.
    pub launch: Option<String>,
    pub has_window: bool,
}

pub fn entries(id: MenuId, app: &AppCtx<'_>, dark: bool) -> Vec<Entry> {
    match id {
        MenuId::Solyra => vec![
            item("About This Computer", None, sh("solyra-settings --page about")),
            item("System Settings…", Some("Super+S"), sh("solyra-settings")),
            sep(),
            item("Lock Screen", Some("Super+Ctrl+L"), HitAction::Shell(ShellCmd::Lock)),
            item("Log Out…", Some("Super+Shift+E"), ipc("{\"Action\":{\"Quit\":{\"skip_confirmation\":false}}}")),
            sep(),
            Entry {
                label: "Restart…".into(),
                shortcut: None,
                action: Some(sh("systemctl reboot")),
                confirm: Some("Click again to restart"),
            },
            Entry {
                label: "Shut Down…".into(),
                shortcut: None,
                action: Some(sh("systemctl poweroff")),
                confirm: Some("Click again to shut down"),
            },
        ],
        MenuId::App => {
            let mut v = Vec::new();
            if let Some(cmd) = &app.launch {
                v.push(item(&format!("New {} Window", app.name), None, sh(cmd)));
            }
            if app.has_window {
                v.push(item("Close Window", Some("Super+W"), ipc("{\"Action\":{\"CloseWindow\":{\"id\":null}}}")));
            }
            if v.is_empty() {
                v.push(item("Open the Launcher", Some("Super+D"), HitAction::Shell(ShellCmd::ToggleLauncher)));
            }
            v
        }
        MenuId::Go => vec![
            item("Home", None, sh("solyra-open files \"$HOME\"")),
            item("Documents", None, sh("solyra-open files \"$(xdg-user-dir DOCUMENTS 2>/dev/null || echo $HOME/Documents)\"")),
            item("Downloads", None, sh("solyra-open files \"$(xdg-user-dir DOWNLOAD 2>/dev/null || echo $HOME/Downloads)\"")),
            item("Pictures", None, sh("solyra-open files \"$(xdg-user-dir PICTURES 2>/dev/null || echo $HOME/Pictures)\"")),
            sep(),
            item("Computer", None, sh("solyra-open files /")),
            item("Trash", None, sh("solyra-open files trash:///")),
        ],
        MenuId::Window => {
            let mut v = vec![
                item("Fullscreen", Some("Super+Shift+F"), ipc("{\"Action\":{\"FullscreenWindow\":{\"id\":null}}}")),
                item("Maximize Column", Some("Super+F"), ipc("{\"Action\":{\"MaximizeColumn\":{}}}")),
                item("Float / Tile", Some("Super+V"), ipc("{\"Action\":{\"ToggleWindowFloating\":{\"id\":null}}}")),
                item("Centre", Some("Super+C"), ipc("{\"Action\":{\"CenterColumn\":{}}}")),
                sep(),
            ];
            for (i, json) in MOVE_TO.iter().enumerate() {
                v.push(item(&format!("Move to Workspace {}", i + 1), None, ipc(json)));
            }
            v.push(sep());
            v.push(item("Close", Some("Super+W"), ipc("{\"Action\":{\"CloseWindow\":{\"id\":null}}}")));
            v
        }
        MenuId::View => vec![
            item("Overview", Some("Super+Tab"), ipc("{\"Action\":{\"ToggleOverview\":{}}}")),
            item(if dark { "Light Appearance" } else { "Dark Appearance" }, None, HitAction::ToggleTheme),
            item("Show or Hide the Dock", None, HitAction::Shell(ShellCmd::ToggleDockAutohide)),
            sep(),
            item("Control Centre", Some("Super+Shift+C"), HitAction::Shell(ShellCmd::ToggleControl)),
            item("Agent Tasks", Some("Super+Shift+A"), HitAction::Shell(ShellCmd::ToggleAgents)),
        ],
        MenuId::Help => vec![
            item("Keyboard Shortcuts", Some("Super+Shift+/"), ipc("{\"Action\":{\"ShowHotkeyOverlay\":{}}}")),
            item("Search Apps and Commands", Some("Super+Space"), HitAction::Shell(ShellCmd::ToggleLauncher)),
        ],
    }
}

pub const ROW_H: i32 = 28;
pub const SEP_H: i32 = 9;
pub const PAD: i32 = 8;
pub const MIN_W: i32 = 220;

/// The panel's size for `entries`.
pub fn panel_size(fonts: &Fonts, entries: &[Entry]) -> (i32, i32) {
    let w = entries
        .iter()
        .map(|e| {
            let lw = text_width(fonts.regular.as_ref(), &e.label, T_LABEL + 0.5);
            let sw = e
                .shortcut
                .map(|s| text_width(fonts.regular.as_ref(), s, T_LABEL) + 28)
                .unwrap_or(0);
            lw + sw + 36
        })
        .max()
        .unwrap_or(0)
        .max(MIN_W);
    let h: i32 = entries
        .iter()
        .map(|e| if e.action.is_some() { ROW_H } else { SEP_H })
        .sum::<i32>()
        + PAD * 2;
    (w, h)
}

/// A title in the bar, as the menu overlay needs it to switch menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Title {
    pub id: MenuId,
    pub x: i32,
    pub w: i32,
}

/// Draw the open menu below `title` on a full-screen transparent surface.
/// `armed` is the index of an item waiting for its confirming second click.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    title: Title,
    titles: &[Title],
    entries: &[Entry],
    bar_h: i32,
    armed: Option<usize>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let mut regions = Vec::new();
    let screen_w = cv.width as i32;
    let screen_h = cv.height as i32;

    // The open title stays highlighted in the bar area.
    cv.round_rect(title.x - 5, 5, title.w + 10, bar_h - 10, 5.0, th.tile_hover);

    let (pw, ph) = panel_size(fonts, entries);
    let px = (title.x - 8).clamp(4, (screen_w - pw - 4).max(4));
    let py = bar_h + 2;
    cv.round_rect_shadow(px, py, pw, ph, 12.0, 9.0, 26.0, 46);
    cv.round_rect(px, py, pw, ph, 12.0, th.surface);
    cv.stroke_round_rect(px, py, pw, ph, 12.0, 1.0, th.hairline);

    let mut y = py + PAD;
    for (i, e) in entries.iter().enumerate() {
        let Some(action) = &e.action else {
            cv.fill_rect(px + 12, y + SEP_H / 2, pw - 24, 1, th.separator);
            y += SEP_H;
            continue;
        };
        let (rx, rw) = (px + 5, pw - 10);
        let hov = hover.is_some_and(|(hx, hy)| {
            hx >= rx as f64 && hx < (rx + rw) as f64 && hy >= y as f64 && hy < (y + ROW_H) as f64
        });
        let is_armed = armed == Some(i);
        if is_armed {
            cv.round_rect(rx, y, rw, ROW_H, 6.0, th.danger);
        } else if hov {
            cv.round_rect(rx, y, rw, ROW_H, 6.0, th.accent);
        }
        let fg = if is_armed || hov {
            th.on_accent
        } else {
            th.text
        };
        let label = if is_armed {
            e.confirm.unwrap_or(&e.label)
        } else {
            &e.label
        };
        cv.text(
            fonts.regular.as_ref(),
            label,
            rx + 12,
            y + 19,
            T_LABEL + 0.5,
            fg,
        );
        if let (Some(s), false) = (e.shortcut, is_armed) {
            let sw = text_width(fonts.regular.as_ref(), s, T_LABEL);
            let sc = if hov { th.on_accent } else { th.text_dim };
            cv.text(
                fonts.regular.as_ref(),
                s,
                rx + rw - 12 - sw,
                y + 19,
                T_LABEL,
                sc,
            );
        }
        let act = if e.confirm.is_some() && !is_armed {
            HitAction::ArmMenuItem(i)
        } else {
            action.clone()
        };
        regions.push(Region {
            x: rx,
            y,
            w: rw,
            h: ROW_H,
            action: act,
        });
        y += ROW_H;
    }

    // Other titles: hovering or clicking switches menus. Listed before the
    // catch-all so they win.
    for t in titles {
        if t.id != title.id {
            regions.push(Region {
                x: t.x - 5,
                y: 0,
                w: t.w + 10,
                h: bar_h,
                action: HitAction::OpenMenu(t.id),
            });
        }
    }
    // Anything else closes the menu.
    regions.push(Region {
        x: 0,
        y: 0,
        w: screen_w,
        h: screen_h,
        action: HitAction::Shell(ShellCmd::CloseOverlays),
    });
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> AppCtx<'static> {
        AppCtx {
            name: "Terminal",
            launch: Some("solyra-open terminal".into()),
            has_window: true,
        }
    }

    #[test]
    fn every_menu_has_items_and_no_dead_entries() {
        for id in [
            MenuId::Solyra,
            MenuId::App,
            MenuId::Go,
            MenuId::Window,
            MenuId::View,
            MenuId::Help,
        ] {
            let e = entries(id, &app(), true);
            assert!(e.iter().any(|x| x.action.is_some()), "{id:?} is empty");
            assert!(
                e.iter().all(|x| x.action.is_some() || x.label.is_empty()),
                "{id:?} has a labelled item with no action"
            );
        }
    }

    #[test]
    fn power_actions_need_confirmation() {
        let e = entries(MenuId::Solyra, &app(), true);
        for label in ["Restart…", "Shut Down…"] {
            let item = e.iter().find(|x| x.label == label).unwrap();
            assert!(item.confirm.is_some(), "{label} must confirm");
        }
    }

    #[test]
    fn app_menu_offers_only_what_is_possible() {
        let none = AppCtx {
            name: "Solyra",
            launch: None,
            has_window: false,
        };
        let e = entries(MenuId::App, &none, true);
        assert_eq!(e.len(), 1);
        assert!(!e.iter().any(|x| x.label.contains("Close")));
    }

    #[test]
    fn move_to_workspace_json_is_well_formed() {
        for j in MOVE_TO {
            let v: serde_json::Value = serde_json::from_str(j).unwrap();
            assert!(v["Action"]["MoveWindowToWorkspace"]["reference"]["Index"].is_u64());
        }
    }
}
