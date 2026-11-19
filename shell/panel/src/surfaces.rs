//! Surface painters and hit-test layouts for Bar, Dock (3 variants),
//! Launcher, Control Centre, and Expanded Dynamic Island.

use std::collections::HashMap;

use crate::canvas::{text_width, Canvas, Fonts, Icon, Rgba};
use crate::state::{
    catalog_apps, dock_entries, focused_app_menus, load_app_icon, AppEntry, DockStyle,
    LaunchTarget, ShellCmd, ShellPrefs, Telemetry, WallpaperPreset, WindowInfo,
};

pub const BAR_HEIGHT: u32 = 34;
pub const NOTCH_WIDTH: i32 = 184;
pub const NOTCH_HEIGHT: i32 = 27;

pub const DOCK_FLOAT_HEIGHT: u32 = 58;
pub const TASKBAR_HEIGHT: u32 = 48;

pub const LAUNCHER_W: u32 = 640;
pub const LAUNCHER_H: u32 = 436;

pub const CONTROL_W: u32 = 348;
pub const CONTROL_H: u32 = 436;

pub const ISLAND_CARD_W: u32 = 372;
pub const ISLAND_CARD_H: u32 = 172;

// Palette tokens (true RGB; Canvas converts to premultiplied BGRA).
pub const ACCENT: Rgba = Rgba::rgb(0xc0, 0x84, 0xfc);
pub const ACCENT_INDIGO: Rgba = Rgba::rgb(0x81, 0x8c, 0xf8);
pub const ON_ACCENT: Rgba = Rgba::rgb(0x12, 0x0d, 0x1d);
pub const TEXT: Rgba = Rgba::rgb(0xf3, 0xef, 0xfa);
pub const TEXT_MED: Rgba = Rgba::rgba(0xe2, 0xdc, 0xf0, 205);
pub const TEXT_DIM: Rgba = Rgba::rgba(0xa8, 0x9f, 0xbc, 195);

#[derive(Clone, Debug)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub action: HitAction,
}

impl Region {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x as f64
            && px < (self.x + self.w) as f64
            && py >= self.y as f64
            && py < (self.y + self.h) as f64
    }
}

#[derive(Clone, Debug)]
pub enum HitAction {
    Workspace(u8),
    FocusWindow(u64),
    Launch(LaunchTarget),
    Shell(ShellCmd),
    ToggleWifi,
    ToggleBt,
    ToggleDnd,
    SetVolumeFromX { x0: i32, w: i32 },
    SetBrightnessFromX { x0: i32, w: i32 },
}

pub fn floating_dock_width(style: DockStyle, wins: &[WindowInfo]) -> u32 {
    let entries = dock_entries(wins);
    match style {
        DockStyle::Solyra => {
            // Start (42) + divider (13) + N apps (42 each, 8 gap) + divider (13) + Agent (42) + 20 pad
            let apps_w = entries.len() as i32 * 42 + (entries.len().saturating_sub(1) as i32) * 8;
            (20 + 42 + 16 + apps_w + 16 + 42).max(220) as u32
        }
        DockStyle::Macos => {
            // Launcher + N apps (44 each, 8 gap) + divider (16) + Trash (44) + 20 pad
            let count = entries.len() as i32 + 1; // + Launchpad
            let apps_w = count * 44 + (count - 1) * 8;
            (20 + apps_w + 16 + 44).max(240) as u32
        }
        DockStyle::Windows => 1280,
    }
}

// ----------------------------------------------------------------- Top Bar

#[allow(clippy::too_many_arguments)]
pub fn draw_bar(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    ws_list: &[(u8, bool)],
    wins: &[WindowInfo],
    island_msg: Option<&str>,
    prefs: &ShellPrefs,
    telem: &Telemetry,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Translucent dark-tinted glass: lets niri's dual-kawase backdrop blur and
    // the wallpaper hue shine through, just like macOS Sequoia.
    cv.vgrad_rect(
        0,
        0,
        w,
        h,
        Rgba::rgba(0x16, 0x12, 0x22, 152),
        Rgba::rgba(0x10, 0x0d, 0x19, 174),
    );
    cv.fill_rect(0, h - 1, w, 1, Rgba::rgba(255, 255, 255, 24));

    // Left: Solyra emblem button -> opens Launcher.
    let brand_hover = hover.is_some_and(|(hx, hy)| hx >= 6.0 && hx < 34.0 && hy < h as f64);
    if brand_hover {
        cv.round_rect(6, 5, 26, 24, 6.0, Rgba::rgba(255, 255, 255, 28));
    }
    cv.icon_solyra(19.0, 17.0, 6.2, ACCENT);
    regions.push(Region {
        x: 4,
        y: 0,
        w: 30,
        h,
        action: HitAction::Shell(ShellCmd::ToggleLauncher),
    });

    // Focused App Name (Semibold) + macOS-style contextual menu bar items.
    let (app_name, menus) = focused_app_menus(wins);
    let mut x = 38;
    let app_w = cv.text(fonts.semibold.as_ref(), &app_name, x, 22, 13.0, TEXT);
    regions.push(Region {
        x: x - 4,
        y: 4,
        w: app_w + 8,
        h: 26,
        action: HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}")),
    });
    x += app_w + 16;

    // Leave generous room before the center notch/island.
    let center_reserved_half = if island_msg.is_some() { 196 } else { 112 };
    let max_left_x = (w / 2) - center_reserved_half;

    // Workspace pills right after the app title/menus (clean & scannable).
    for &(idx, active) in ws_list {
        if x + 22 > max_left_x {
            break;
        }
        let is_hov = hover.is_some_and(|(hx, hy)| {
            hx >= x as f64 && hx < (x + 20) as f64 && hy >= 7.0 && hy < 27.0
        });
        if active {
            cv.dgrad_round_rect(x, 9, 20, 16, 8.0, ACCENT, ACCENT_INDIGO);
        } else {
            let bg = if is_hov {
                Rgba::rgba(255, 255, 255, 48)
            } else {
                Rgba::rgba(255, 255, 255, 24)
            };
            cv.round_rect(x, 9, 20, 16, 8.0, bg);
        }
        let label = idx.to_string();
        let tw = text_width(fonts.semibold.as_ref(), &label, 11.0);
        let fg = if active { ON_ACCENT } else { TEXT_MED };
        cv.text(
            fonts.semibold.as_ref(),
            &label,
            x + (20 - tw) / 2,
            21,
            11.0,
            fg,
        );
        regions.push(Region {
            x: x - 2,
            y: 4,
            w: 24,
            h: 26,
            action: HitAction::Workspace(idx),
        });
        x += 25;
    }

    // Contextual menu labels if space permits before the notch.
    x += 8;
    if x + 48 < max_left_x {
        cv.fill_rect(x - 5, 10, 1, 14, Rgba::rgba(255, 255, 255, 26));
        for &menu in menus {
            let mw = text_width(fonts.regular.as_ref(), menu, 12.2);
            if x + mw + 12 > max_left_x {
                break;
            }
            let m_hov = hover.is_some_and(|(hx, hy)| {
                hx >= (x - 4) as f64 && hx < (x + mw + 4) as f64 && hy >= 5.0 && hy < 29.0
            });
            if m_hov {
                cv.round_rect(x - 5, 5, mw + 10, 24, 5.0, Rgba::rgba(255, 255, 255, 28));
            }
            cv.text(fonts.regular.as_ref(), menu, x, 22, 12.2, TEXT_MED);
            let act = match menu {
                "Window" | "View" => {
                    HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}"))
                }
                "Help" => {
                    HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ShowHotkeyOverlay\":{}}}"))
                }
                _ => HitAction::Shell(ShellCmd::ToggleLauncher),
            };
            regions.push(Region {
                x: x - 4,
                y: 4,
                w: mw + 8,
                h: 26,
                action: act,
            });
            x += mw + 14;
        }
    }

    // Centre: Notch + Dynamic Island.
    if let Some(message) = island_msg {
        let tw = text_width(fonts.semibold.as_ref(), message, 12.0);
        let pill_w = (tw + 68).clamp(NOTCH_WIDTH + 36, 390);
        let left = (w - pill_w) / 2;
        cv.vgrad_round_rect(
            left,
            3,
            pill_w,
            28,
            14.0,
            Rgba::rgb(0x09, 0x08, 0x10),
            Rgba::rgb(0x05, 0x04, 0x09),
        );
        cv.stroke_round_rect(left, 3, pill_w, 28, 14.0, 1.0, Rgba::rgba(0xc0, 0x84, 0xfc, 110));
        // Glowing activity dot on left + live mini waveform bars on right.
        cv.circle(left as f32 + 16.0, 17.0, 4.0, ACCENT);
        cv.circle(left as f32 + 16.0, 17.0, 7.0, Rgba::rgba(0xc0, 0x84, 0xfc, 45));
        cv.text_truncated(
            fonts.semibold.as_ref(),
            message,
            left + 28,
            21,
            12.0,
            pill_w - 64,
            TEXT,
        );
        let wx = left + pill_w - 24;
        cv.round_rect(wx, 13, 3, 8, 1.5, ACCENT);
        cv.round_rect(wx + 5, 10, 3, 14, 1.5, ACCENT_INDIGO);
        cv.round_rect(wx + 10, 14, 3, 6, 1.5, ACCENT);
        regions.push(Region {
            x: left,
            y: 0,
            w: pill_w,
            h,
            action: HitAction::Shell(ShellCmd::ToggleIsland),
        });
    } else {
        cv.draw_notch(w / 2, NOTCH_WIDTH, NOTCH_HEIGHT, 8.0, 13.0);
        regions.push(Region {
            x: (w - NOTCH_WIDTH) / 2 - 8,
            y: 0,
            w: NOTCH_WIDTH + 16,
            h,
            action: HitAction::Shell(ShellCmd::ToggleIsland),
        });
    }

    // Right cluster: Clock & Date, Status Capsule (Wi-Fi, Sound, Battery, Control Centre), Search.
    let now = chrono::Local::now();
    let clock_str = now.format("%H:%M").to_string();
    let date_str = now.format("%a %b %-d").to_string();
    let clock_w = text_width(fonts.semibold.as_ref(), &clock_str, 12.8);
    let date_w = text_width(fonts.regular.as_ref(), &date_str, 12.0);

    let mut rx = w - 14;
    rx -= clock_w;
    cv.text(fonts.semibold.as_ref(), &clock_str, rx, 22, 12.8, TEXT);
    let clock_start = rx - date_w - 8;
    cv.text(fonts.regular.as_ref(), &date_str, clock_start, 22, 12.0, TEXT_MED);
    regions.push(Region {
        x: clock_start - 4,
        y: 4,
        w: date_w + clock_w + 12,
        h: 26,
        action: HitAction::Shell(ShellCmd::ToggleIsland),
    });

    // Status pill (Control Centre trigger): Wi-Fi + Volume + Battery + Twin-switches.
    let status_w = 96;
    let status_x = clock_start - status_w - 12;
    let status_hov = hover.is_some_and(|(hx, hy)| {
        hx >= status_x as f64 && hx < (status_x + status_w) as f64 && hy >= 4.0 && hy < 30.0
    });
    cv.round_rect(
        status_x,
        5,
        status_w,
        24,
        8.0,
        if status_hov {
            Rgba::rgba(255, 255, 255, 38)
        } else {
            Rgba::rgba(255, 255, 255, 18)
        },
    );
    let wifi_col = if prefs.wifi_on { TEXT } else { TEXT_DIM };
    cv.icon_wifi(status_x as f32 + 14.0, 17.0, 13.0, wifi_col);
    cv.icon_volume(status_x as f32 + 35.0, 17.0, 13.0, prefs.volume, TEXT);
    cv.icon_battery(
        status_x as f32 + 59.0,
        17.0,
        telem.battery_pct,
        TEXT_MED,
        Rgba::rgb(0x4a, 0xde, 0x80),
    );
    cv.icon_control_centre(status_x as f32 + 83.0, 17.0, TEXT);
    regions.push(Region {
        x: status_x,
        y: 4,
        w: status_w,
        h: 26,
        action: HitAction::Shell(ShellCmd::ToggleControl),
    });

    // Spotlight Search icon button just left of the status pill.
    let search_x = status_x - 30;
    if search_x > (w / 2) + center_reserved_half {
        let s_hov = hover.is_some_and(|(hx, hy)| {
            hx >= search_x as f64 && hx < (search_x + 24) as f64 && hy >= 5.0 && hy < 29.0
        });
        if s_hov {
            cv.round_rect(search_x, 5, 24, 24, 7.0, Rgba::rgba(255, 255, 255, 34));
        }
        cv.icon_search(search_x as f32 + 12.0, 17.0, 13.5, TEXT_MED);
        regions.push(Region {
            x: search_x,
            y: 4,
            w: 24,
            h: 26,
            action: HitAction::Shell(ShellCmd::ToggleLauncher),
        });
    }

    regions
}

// --------------------------------------------------------------- 3 Docks

pub fn draw_dock(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    style: DockStyle,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    match style {
        DockStyle::Solyra => draw_dock_solyra(cv, wins, icons, hover),
        DockStyle::Macos => draw_dock_macos(cv, wins, icons, hover),
        DockStyle::Windows => draw_dock_windows(cv, fonts, wins, icons, hover),
    }
}

fn draw_dock_solyra(
    cv: &mut Canvas<'_>,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Tight surface geometry (matches `geometry-corner-radius 20` in niri).
    cv.glass_card(
        0,
        0,
        w,
        h,
        20.0,
        Rgba::rgba(0x22, 0x1c, 0x33, 158),
        Rgba::rgba(0x14, 0x10, 0x21, 188),
        Rgba::rgba(255, 255, 255, 70),
        Rgba::rgba(255, 255, 255, 20),
    );

    let mut x = 10;
    let base_y = 8;
    let cell = 42;

    // 1. Start squircle (Solyra Launcher).
    let start_hov = hover.is_some_and(|(hx, hy)| {
        hx >= x as f64 && hx < (x + cell) as f64 && hy >= 4.0 && hy < (h - 4) as f64
    });
    let sy = if start_hov { base_y - 3 } else { base_y };
    cv.draw_app_squircle(x, sy, cell, "start", None);
    regions.push(Region {
        x,
        y: 0,
        w: cell,
        h,
        action: HitAction::Shell(ShellCmd::ToggleLauncher),
    });
    x += cell + 8;

    // Hairline divider.
    cv.fill_rect(x, 14, 1, h - 28, Rgba::rgba(255, 255, 255, 32));
    x += 8;

    // 2. Pinned + running apps (one cell per app, Windows-style pill indicator).
    let entries = dock_entries(wins);
    for entry in &entries {
        let is_hov = hover.is_some_and(|(hx, hy)| {
            hx >= x as f64 && hx < (x + cell) as f64 && hy >= 2.0 && hy < (h - 2) as f64
        });
        let iy = if is_hov { base_y - 3 } else { base_y };
        let sys_ic = load_app_icon(icons, &entry.app_id);
        cv.draw_app_squircle(x, iy, cell, &entry.role, sys_ic);

        if entry.window_count > 0 {
            let bar_w = if entry.is_focused { 18 } else { 8 };
            let bar_col = if entry.is_focused {
                ACCENT
            } else {
                Rgba::rgba(0xe9, 0xd5, 0xff, 210)
            };
            cv.round_rect(x + (cell - bar_w) / 2, h - 5, bar_w, 3, 1.5, bar_col);
        }

        let action = match entry.window {
            Some(id) => HitAction::FocusWindow(id),
            None => HitAction::Launch(entry.command.clone()),
        };
        regions.push(Region {
            x,
            y: 0,
            w: cell,
            h,
            action,
        });
        x += cell + 8;
    }

    // 3. Agent Space Shelf on the right.
    cv.fill_rect(x, 14, 1, h - 28, Rgba::rgba(255, 255, 255, 32));
    x += 8;
    let ag_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
    let ay = if ag_hov { base_y - 3 } else { base_y };
    cv.draw_app_squircle(x, ay, cell, "agent", None);
    // Live emerald readiness badge on top-right of Agent squircle.
    cv.circle((x + cell - 5) as f32, (ay + 5) as f32, 4.5, Rgba::rgb(0x14, 0x10, 0x21));
    cv.circle((x + cell - 5) as f32, (ay + 5) as f32, 3.0, Rgba::rgb(0x34, 0xd3, 0x99));
    regions.push(Region {
        x,
        y: 0,
        w: cell,
        h,
        action: HitAction::Shell(ShellCmd::ToggleIsland),
    });

    regions
}

fn draw_dock_macos(
    cv: &mut Canvas<'_>,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Lighter frosted Sequoia glass rim + luminous dots under running apps.
    cv.glass_card(
        0,
        0,
        w,
        h,
        20.0,
        Rgba::rgba(0x38, 0x30, 0x4f, 138),
        Rgba::rgba(0x1e, 0x19, 0x2e, 168),
        Rgba::rgba(255, 255, 255, 88),
        Rgba::rgba(255, 255, 255, 28),
    );

    let mut x = 10;
    let cell = 44;
    let base_y = 6;

    // Apps / Launchpad icon first, then core + running apps.
    let entries = dock_entries(wins);
    for (idx, entry) in entries.iter().enumerate() {
        if idx == 1 {
            // Insert Launchpad / Spotlight right after Finder/Files, like macOS.
            let l_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
            cv.draw_app_squircle(x, if l_hov { base_y - 3 } else { base_y }, cell, "start", None);
            regions.push(Region {
                x,
                y: 0,
                w: cell,
                h,
                action: HitAction::Shell(ShellCmd::ToggleLauncher),
            });
            x += cell + 8;
        }

        let is_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
        let iy = if is_hov { base_y - 3 } else { base_y };
        let sys_ic = load_app_icon(icons, &entry.app_id);
        cv.draw_app_squircle(x, iy, cell, &entry.role, sys_ic);

        if entry.window_count > 0 {
            cv.circle(
                x as f32 + cell as f32 * 0.5,
                (h - 4) as f32,
                2.2,
                Rgba::rgba(255, 255, 255, 230),
            );
        }

        let action = match entry.window {
            Some(id) => HitAction::FocusWindow(id),
            None => HitAction::Launch(entry.command.clone()),
        };
        regions.push(Region {
            x,
            y: 0,
            w: cell,
            h,
            action,
        });
        x += cell + 8;
    }

    // Classic macOS full-height separator + Trash on the right.
    cv.fill_rect(x, 10, 1, h - 20, Rgba::rgba(255, 255, 255, 42));
    x += 8;
    let t_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
    cv.draw_app_squircle(x, if t_hov { base_y - 3 } else { base_y }, cell, "trash", None);
    regions.push(Region {
        x,
        y: 0,
        w: cell,
        h,
        action: HitAction::Launch(LaunchTarget::Shell(format!(
            "{} trash:///",
            crate::state::role_binary("files", "thunar")
        ))),
    });

    regions
}

fn draw_dock_windows(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Full-width frosted Fluent taskbar.
    cv.vgrad_rect(
        0,
        0,
        w,
        h,
        Rgba::rgba(0x1b, 0x17, 0x29, 178),
        Rgba::rgba(0x12, 0x0f, 0x1d, 204),
    );
    cv.fill_rect(0, 0, w, 1, Rgba::rgba(255, 255, 255, 38));

    // Left: Weather / Widgets pill (Image 3).
    let w_hov = hover.is_some_and(|(hx, _)| hx >= 10.0 && hx < 142.0);
    if w_hov {
        cv.round_rect(10, 6, 132, 36, 8.0, Rgba::rgba(255, 255, 255, 24));
    }
    cv.circle(26.0, 21.0, 5.5, Rgba::rgb(0xfb, 0xbf, 0x24));
    cv.round_rect(25, 21, 13, 7, 3.5, Rgba::rgba(240, 244, 255, 235));
    cv.text(fonts.semibold.as_ref(), "19°C", 46, 22, 11.5, TEXT);
    cv.text(fonts.regular.as_ref(), "Clear sky", 46, 35, 10.5, TEXT_DIM);
    regions.push(Region {
        x: 10,
        y: 4,
        w: 132,
        h: 40,
        action: HitAction::Shell(ShellCmd::ToggleControl),
    });

    // Centre cluster: Start + Search pill + Pinned/Running apps.
    let entries = dock_entries(wins);
    let search_w = 188;
    let total_w = 40 + 8 + search_w + 10 + (entries.len() as i32) * 44;
    let mut cx = (w - total_w) / 2;

    // Start button.
    cv.draw_app_squircle(cx + 3, 7, 34, "start", None);
    regions.push(Region {
        x: cx,
        y: 4,
        w: 40,
        h: 40,
        action: HitAction::Shell(ShellCmd::ToggleLauncher),
    });
    cx += 48;

    // Windows 11 "Type here to search" capsule.
    let s_hov = hover.is_some_and(|(hx, _)| hx >= cx as f64 && hx < (cx + search_w) as f64);
    cv.round_rect(
        cx,
        8,
        search_w,
        32,
        16.0,
        if s_hov {
            Rgba::rgba(255, 255, 255, 36)
        } else {
            Rgba::rgba(255, 255, 255, 22)
        },
    );
    cv.stroke_round_rect(cx, 8, search_w, 32, 16.0, 1.0, Rgba::rgba(255, 255, 255, 34));
    cv.icon_search(cx as f32 + 18.0, 24.0, 13.0, TEXT_MED);
    cv.text(fonts.regular.as_ref(), "Search or ask…", cx + 32, 28, 12.0, TEXT_DIM);
    regions.push(Region {
        x: cx,
        y: 6,
        w: search_w,
        h: 36,
        action: HitAction::Shell(ShellCmd::ToggleLauncher),
    });
    cx += search_w + 10;

    for entry in &entries {
        let is_hov = hover.is_some_and(|(hx, _)| hx >= cx as f64 && hx < (cx + 40) as f64);
        if is_hov || entry.is_focused {
            cv.round_rect(
                cx,
                5,
                40,
                38,
                8.0,
                Rgba::rgba(255, 255, 255, if is_hov { 30 } else { 18 }),
            );
        }
        let sys_ic = load_app_icon(icons, &entry.app_id);
        cv.draw_app_squircle(cx + 4, 7, 32, &entry.role, sys_ic);
        if entry.window_count > 0 {
            let uw = if entry.is_focused { 16 } else { 6 };
            cv.round_rect(cx + (40 - uw) / 2, h - 4, uw, 3, 1.5, ACCENT);
        }
        let action = match entry.window {
            Some(id) => HitAction::FocusWindow(id),
            None => HitAction::Launch(entry.command.clone()),
        };
        regions.push(Region {
            x: cx,
            y: 4,
            w: 40,
            h: 40,
            action,
        });
        cx += 44;
    }

    // Right tray + stacked clock/date.
    let now = chrono::Local::now();
    let t_str = now.format("%H:%M").to_string();
    let d_str = now.format("%d/%m/%Y").to_string();
    cv.text(fonts.semibold.as_ref(), &t_str, w - 58, 21, 11.5, TEXT);
    cv.text(fonts.regular.as_ref(), &d_str, w - 74, 35, 10.2, TEXT_DIM);
    cv.icon_wifi((w - 134) as f32, 24.0, 12.5, TEXT);
    cv.icon_volume((w - 112) as f32, 24.0, 12.5, 75, TEXT);
    cv.icon_battery(
        (w - 90) as f32,
        24.0,
        96,
        TEXT_MED,
        Rgba::rgb(0x4a, 0xde, 0x80),
    );
    regions.push(Region {
        x: w - 150,
        y: 4,
        w: 142,
        h: 40,
        action: HitAction::Shell(ShellCmd::ToggleControl),
    });

    regions
}

// ------------------------------------------------------ Spotlight Launcher

pub fn filtered_launcher_apps(query: &str) -> Vec<AppEntry> {
    let all = catalog_apps();
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|a| {
            a.title.to_ascii_lowercase().contains(&q)
                || a.subtitle.to_ascii_lowercase().contains(&q)
                || a.app_id.to_ascii_lowercase().contains(&q)
        })
        .collect()
}

pub fn draw_launcher(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    query: &str,
    selected: usize,
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    cv.glass_card(
        0,
        0,
        w,
        h,
        22.0,
        Rgba::rgba(0x1f, 0x19, 0x30, 198),
        Rgba::rgba(0x13, 0x0f, 0x20, 222),
        Rgba::rgba(255, 255, 255, 78),
        Rgba::rgba(255, 255, 255, 22),
    );

    // Search capsule at top.
    cv.round_rect(18, 16, w - 36, 48, 14.0, Rgba::rgba(0x0c, 0x09, 0x15, 165));
    cv.stroke_round_rect(18, 16, w - 36, 48, 14.0, 1.0, Rgba::rgba(0xc0, 0x84, 0xfc, 92));
    cv.icon_search(42.0, 40.0, 16.0, ACCENT);

    if query.is_empty() {
        cv.text(
            fonts.regular.as_ref(),
            "Search applications, workspaces, or ask an agent…",
            62,
            45,
            14.0,
            TEXT_DIM,
        );
        cv.fill_rect(62, 30, 2, 20, ACCENT);
    } else {
        let qw = cv.text(fonts.regular.as_ref(), query, 62, 45, 14.5, TEXT);
        cv.fill_rect(64 + qw, 30, 2, 20, ACCENT);
    }

    // `ESC` badge at right of search box.
    cv.round_rect(w - 68, 28, 38, 24, 6.0, Rgba::rgba(255, 255, 255, 20));
    cv.text(fonts.mono.as_ref(), "esc", w - 59, 44, 11.0, TEXT_DIM);
    regions.push(Region {
        x: w - 68,
        y: 28,
        w: 38,
        h: 24,
        action: HitAction::Shell(ShellCmd::CloseOverlays),
    });

    let items = filtered_launcher_apps(query);
    let sel = selected.min(items.len().saturating_sub(1));

    let mut list_y;

    if query.trim().is_empty() {
        // Image-2 style PINNED APPS 4-column hero grid!
        cv.text(fonts.semibold.as_ref(), "PINNED APPS", 24, 92, 10.8, TEXT_DIM);
        let pinned_count = items.len().min(4);
        let tile_w = (w - 48 - 3 * 12) / 4;
        let tile_h = 88;
        for (i, app) in items.iter().take(pinned_count).enumerate() {
            let tx = 24 + i as i32 * (tile_w + 12);
            let ty = 102;
            let is_hov = hover.is_some_and(|(hx, hy)| {
                hx >= tx as f64
                    && hx < (tx + tile_w) as f64
                    && hy >= ty as f64
                    && hy < (ty + tile_h) as f64
            });
            let is_sel = sel == i;
            let bg = if is_sel {
                Rgba::rgba(0xc0, 0x84, 0xfc, 48)
            } else if is_hov {
                Rgba::rgba(255, 255, 255, 28)
            } else {
                Rgba::rgba(255, 255, 255, 14)
            };
            cv.round_rect(tx, ty, tile_w, tile_h, 14.0, bg);
            if is_sel {
                cv.stroke_round_rect(
                    tx,
                    ty,
                    tile_w,
                    tile_h,
                    14.0,
                    1.0,
                    Rgba::rgba(0xc0, 0x84, 0xfc, 130),
                );
            }
            let sys_ic = load_app_icon(icons, &app.app_id);
            cv.draw_app_squircle(tx + (tile_w - 40) / 2, ty + 12, 40, &app.role, sys_ic);
            let tw = text_width(fonts.semibold.as_ref(), &app.title, 12.0);
            cv.text(
                fonts.semibold.as_ref(),
                &app.title,
                tx + (tile_w - tw) / 2,
                ty + 72,
                12.0,
                TEXT,
            );
            regions.push(Region {
                x: tx,
                y: ty,
                w: tile_w,
                h: tile_h,
                action: HitAction::Launch(app.command.clone()),
            });
        }

        cv.text(
            fonts.semibold.as_ref(),
            "COMMANDS & CUSTOMISATION",
            24,
            212,
            10.8,
            TEXT_DIM,
        );
        list_y = 222;
        for (offset, app) in items.iter().skip(4).take(4).enumerate() {
            let idx = 4 + offset;
            draw_launcher_row(cv, fonts, icons, &mut regions, w, list_y, app, sel == idx, hover);
            list_y += 40;
        }
    } else if items.is_empty() {
        cv.text(
            fonts.semibold.as_ref(),
            "No matching applications",
            28,
            128,
            14.0,
            TEXT,
        );
        cv.text(
            fonts.regular.as_ref(),
            "Press Enter to run in Terminal, or Esc to dismiss.",
            28,
            150,
            12.5,
            TEXT_DIM,
        );
    } else {
        cv.text(fonts.semibold.as_ref(), "RESULTS", 24, 92, 10.8, TEXT_DIM);
        list_y = 102;
        for (idx, app) in items.iter().take(7).enumerate() {
            draw_launcher_row(cv, fonts, icons, &mut regions, w, list_y, app, sel == idx, hover);
            list_y += 42;
        }
    }

    // Bottom session footer (Windows 11 Start + Solyra blend).
    let fy = h - 46;
    cv.fill_rect(18, fy, w - 36, 1, Rgba::rgba(255, 255, 255, 22));
    cv.dgrad_round_rect(24, fy + 10, 26, 26, 13.0, ACCENT, ACCENT_INDIGO);
    cv.text(fonts.semibold.as_ref(), "S", 33, fy + 28, 12.5, ON_ACCENT);
    cv.text(fonts.semibold.as_ref(), "solyra", 58, fy + 27, 12.5, TEXT);
    cv.text(
        fonts.regular.as_ref(),
        "· Solyra OS 0.1 (Perihelion)",
        104,
        fy + 27,
        12.0,
        TEXT_DIM,
    );

    // Lock & Power actions on bottom-right.
    cv.round_rect(w - 140, fy + 9, 54, 28, 8.0, Rgba::rgba(255, 255, 255, 18));
    cv.icon_lock((w - 126) as f32, (fy + 23) as f32, TEXT_MED);
    cv.text(fonts.regular.as_ref(), "Lock", w - 114, fy + 27, 11.5, TEXT_MED);
    regions.push(Region {
        x: w - 140,
        y: fy + 9,
        w: 54,
        h: 28,
        action: HitAction::Launch(LaunchTarget::Shell("swaylock".into())),
    });

    cv.round_rect(w - 78, fy + 9, 54, 28, 8.0, Rgba::rgba(255, 255, 255, 18));
    cv.icon_power((w - 51) as f32, (fy + 23) as f32, Rgba::rgb(0xf8, 0x71, 0x71));
    regions.push(Region {
        x: w - 78,
        y: fy + 9,
        w: 54,
        h: 28,
        action: HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"Quit\":{\"skip_confirmation\":true}}}")),
    });

    regions
}

#[allow(clippy::too_many_arguments)]
fn draw_launcher_row(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    icons: &mut HashMap<String, Option<Icon>>,
    regions: &mut Vec<Region>,
    w: i32,
    y: i32,
    app: &AppEntry,
    selected: bool,
    hover: Option<(f64, f64)>,
) {
    let rx = 20;
    let rw = w - 40;
    let rh = 38;
    let is_hov = hover.is_some_and(|(hx, hy)| {
        hx >= rx as f64 && hx < (rx + rw) as f64 && hy >= y as f64 && hy < (y + rh) as f64
    });
    if selected {
        cv.round_rect(rx, y, rw, rh, 10.0, Rgba::rgba(0xc0, 0x84, 0xfc, 52));
        cv.stroke_round_rect(rx, y, rw, rh, 10.0, 1.0, Rgba::rgba(0xc0, 0x84, 0xfc, 120));
    } else if is_hov {
        cv.round_rect(rx, y, rw, rh, 10.0, Rgba::rgba(255, 255, 255, 24));
    }

    let sys_ic = load_app_icon(icons, &app.app_id);
    cv.draw_app_squircle(rx + 8, y + 5, 28, &app.role, sys_ic);
    let tw = cv.text(fonts.semibold.as_ref(), &app.title, rx + 44, y + 24, 13.0, TEXT);
    cv.text_truncated(
        fonts.regular.as_ref(),
        &app.subtitle,
        rx + 54 + tw,
        y + 24,
        12.0,
        rw - (tw + 130),
        TEXT_DIM,
    );
    if selected {
        cv.round_rect(rx + rw - 58, y + 8, 48, 22, 6.0, Rgba::rgba(255, 255, 255, 26));
        cv.text(fonts.mono.as_ref(), "Open ⏎", rx + rw - 52, y + 23, 10.8, TEXT);
    }
    regions.push(Region {
        x: rx,
        y,
        w: rw,
        h: rh,
        action: HitAction::Launch(app.command.clone()),
    });
}

// --------------------------------------------------------- Control Centre

pub fn draw_control(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    prefs: &ShellPrefs,
    telem: &Telemetry,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    cv.glass_card(
        0,
        0,
        w,
        h,
        20.0,
        Rgba::rgba(0x21, 0x1b, 0x33, 196),
        Rgba::rgba(0x13, 0x0f, 0x20, 224),
        Rgba::rgba(255, 255, 255, 80),
        Rgba::rgba(255, 255, 255, 24),
    );

    // 1. 2x2 Quick Toggle Bento Grid.
    let tile_w = (w - 32 - 10) / 2;
    let tile_h = 54;
    let tiles = [
        ("Wi-Fi", if prefs.wifi_on { "Solyra-5G" } else { "Off" }, prefs.wifi_on, HitAction::ToggleWifi),
        ("Bluetooth", if prefs.bt_on { "On" } else { "Off" }, prefs.bt_on, HitAction::ToggleBt),
        (
            "Agent Space",
            "WS 7 · Ready",
            prefs.agent_ready,
            HitAction::Launch(LaunchTarget::Ipc(
                "{\"Action\":{\"FocusWorkspace\":{\"reference\":{\"Index\":7}}}}",
            )),
        ),
        ("Focus", if prefs.dnd_on { "Muted" } else { "Off" }, prefs.dnd_on, HitAction::ToggleDnd),
    ];

    for (i, (title, sub, active, act)) in tiles.into_iter().enumerate() {
        let col = (i % 2) as i32;
        let row = (i / 2) as i32;
        let tx = 16 + col * (tile_w + 10);
        let ty = 16 + row * (tile_h + 10);
        let is_hov = hover.is_some_and(|(hx, hy)| {
            hx >= tx as f64
                && hx < (tx + tile_w) as f64
                && hy >= ty as f64
                && hy < (ty + tile_h) as f64
        });
        if active {
            cv.dgrad_round_rect(
                tx,
                ty,
                tile_w,
                tile_h,
                14.0,
                Rgba::rgba(0xc0, 0x84, 0xfc, 220),
                Rgba::rgba(0x81, 0x8c, 0xf8, 220),
            );
        } else {
            cv.round_rect(
                tx,
                ty,
                tile_w,
                tile_h,
                14.0,
                Rgba::rgba(255, 255, 255, if is_hov { 34 } else { 20 }),
            );
        }
        let fg = if active { ON_ACCENT } else { TEXT };
        let fg_sub = if active {
            Rgba::rgba(0x14, 0x10, 0x20, 200)
        } else {
            TEXT_DIM
        };
        let icx = tx as f32 + 22.0;
        let icy = ty as f32 + 27.0;
        match i {
            0 => cv.icon_wifi(icx, icy, 14.0, fg),
            1 => cv.icon_bluetooth(icx, icy, fg),
            2 => cv.icon_solyra(icx, icy, 6.0, fg),
            _ => cv.icon_moon(icx, icy, fg),
        }
        cv.text(fonts.semibold.as_ref(), title, tx + 40, ty + 24, 12.2, fg);
        cv.text(fonts.regular.as_ref(), sub, tx + 40, ty + 40, 11.0, fg_sub);
        regions.push(Region {
            x: tx,
            y: ty,
            w: tile_w,
            h: tile_h,
            action: act,
        });
    }

    // 2. Display & Sound Sliders card.
    let sy = 148;
    cv.round_rect(16, sy, w - 32, 96, 14.0, Rgba::rgba(255, 255, 255, 16));
    draw_slider_row(
        cv,
        fonts,
        &mut regions,
        28,
        sy + 14,
        w - 56,
        "Display",
        prefs.brightness,
        true,
    );
    draw_slider_row(
        cv,
        fonts,
        &mut regions,
        28,
        sy + 54,
        w - 56,
        "Sound",
        prefs.volume,
        false,
    );

    // 3. Live Dock Style Segmented Switcher (Solyra | macOS | Windows).
    let dy = 258;
    cv.text(fonts.semibold.as_ref(), "DOCK STYLE", 20, dy + 12, 10.5, TEXT_DIM);
    let seg_y = dy + 20;
    let seg_w = w - 32;
    cv.round_rect(16, seg_y, seg_w, 34, 10.0, Rgba::rgba(0x0a, 0x08, 0x12, 160));
    let item_w = (seg_w - 8) / 3;
    for (i, style) in [DockStyle::Solyra, DockStyle::Macos, DockStyle::Windows]
        .into_iter()
        .enumerate()
    {
        let ix = 20 + i as i32 * item_w;
        let active = prefs.dock_style == style;
        if active {
            cv.dgrad_round_rect(ix, seg_y + 3, item_w, 28, 8.0, ACCENT, ACCENT_INDIGO);
        }
        let label = style.label();
        let tw = text_width(fonts.semibold.as_ref(), label, 11.8);
        cv.text(
            fonts.semibold.as_ref(),
            label,
            ix + (item_w - tw) / 2,
            seg_y + 21,
            11.8,
            if active { ON_ACCENT } else { TEXT_MED },
        );
        regions.push(Region {
            x: ix,
            y: seg_y,
            w: item_w,
            h: 34,
            action: HitAction::Shell(ShellCmd::SetDock(style)),
        });
    }

    // 4. Live Wallpaper Swatches.
    let wy = 328;
    cv.text(fonts.semibold.as_ref(), "WALLPAPER", 20, wy + 12, 10.5, TEXT_DIM);
    let mut sx = 28;
    for preset in WallpaperPreset::ALL {
        let active = prefs.wallpaper == preset;
        if active {
            cv.ring(sx as f32, (wy + 32) as f32, 13.5, 2.0, TEXT);
        }
        cv.circle(sx as f32, (wy + 32) as f32, 10.0, preset.swatch());
        regions.push(Region {
            x: sx - 14,
            y: wy + 18,
            w: 28,
            h: 28,
            action: HitAction::Shell(ShellCmd::SetWallpaper(preset)),
        });
        sx += 36;
    }
    cv.text(
        fonts.regular.as_ref(),
        prefs.wallpaper.label(),
        w - 82,
        wy + 36,
        11.8,
        TEXT_MED,
    );

    // 5. Live RAM telemetry footer.
    let my = 386;
    cv.fill_rect(16, my, w - 32, 1, Rgba::rgba(255, 255, 255, 20));
    let mem_label = format!(
        "Memory · {} / {} MiB",
        telem.mem_used_mib, telem.mem_total_mib
    );
    cv.text(fonts.regular.as_ref(), &mem_label, 20, my + 24, 11.5, TEXT_MED);
    let bar_w = 96;
    let bx = w - 20 - bar_w;
    cv.round_rect(bx, my + 15, bar_w, 8, 4.0, Rgba::rgba(255, 255, 255, 24));
    let ratio = (telem.mem_used_mib as f32 / telem.mem_total_mib as f32).clamp(0.06, 1.0);
    cv.dgrad_round_rect(
        bx,
        my + 15,
        (bar_w as f32 * ratio).round() as i32,
        8,
        4.0,
        ACCENT,
        ACCENT_INDIGO,
    );

    regions
}

#[allow(clippy::too_many_arguments)]
fn draw_slider_row(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    regions: &mut Vec<Region>,
    x: i32,
    y: i32,
    w: i32,
    label: &str,
    pct: u8,
    is_brightness: bool,
) {
    cv.text(fonts.semibold.as_ref(), label, x, y + 10, 11.5, TEXT_MED);
    let pct_str = format!("{pct}%");
    let pw = text_width(fonts.regular.as_ref(), &pct_str, 11.0);
    cv.text(fonts.regular.as_ref(), &pct_str, x + w - pw, y + 10, 11.0, TEXT_DIM);

    let ty = y + 16;
    let th = 18;
    cv.round_rect(x, ty, w, th, 9.0, Rgba::rgba(0x0b, 0x08, 0x13, 165));
    let fill_w = ((w as f32) * (pct.clamp(8, 100) as f32 / 100.0)).round() as i32;
    cv.dgrad_round_rect(x, ty, fill_w, th, 9.0, Rgba::rgb(0xe9, 0xd5, 0xff), ACCENT);
    if is_brightness {
        cv.icon_sun(x as f32 + 11.0, ty as f32 + 9.0, ON_ACCENT);
        regions.push(Region {
            x,
            y: ty - 4,
            w,
            h: th + 8,
            action: HitAction::SetBrightnessFromX { x0: x, w },
        });
    } else {
        cv.icon_volume(x as f32 + 12.0, ty as f32 + 9.0, 11.0, pct, ON_ACCENT);
        regions.push(Region {
            x,
            y: ty - 4,
            w,
            h: th + 8,
            action: HitAction::SetVolumeFromX { x0: x, w },
        });
    }
}

// -------------------------------------------------- Expanded Island Card

pub fn draw_island_card(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    island_msg: Option<&str>,
    wins: &[WindowInfo],
    telem: &Telemetry,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    cv.glass_card(
        0,
        0,
        w,
        h,
        22.0,
        Rgba::rgba(0x0c, 0x0a, 0x15, 236),
        Rgba::rgba(0x07, 0x06, 0x0d, 246),
        Rgba::rgba(0xc0, 0x84, 0xfc, 96),
        Rgba::rgba(255, 255, 255, 24),
    );

    cv.draw_app_squircle(18, 18, 36, "agent", None);
    cv.text(fonts.semibold.as_ref(), "Dynamic Island", 64, 33, 13.5, TEXT);
    let status = island_msg.unwrap_or("Agent Space idle · Workspace 7 isolated");
    cv.text_truncated(fonts.regular.as_ref(), status, 64, 51, 11.8, w - 84, TEXT_DIM);

    // Telemetry strip.
    cv.round_rect(18, 68, w - 36, 40, 10.0, Rgba::rgba(255, 255, 255, 16));
    let summary = format!(
        "{} open windows  ·  {} MiB RAM used",
        wins.len(),
        telem.mem_used_mib
    );
    cv.text(fonts.regular.as_ref(), &summary, 30, 93, 12.0, TEXT_MED);

    // Action buttons.
    let btn_w = (w - 36 - 10) / 2;
    cv.dgrad_round_rect(18, 120, btn_w, 34, 10.0, ACCENT, ACCENT_INDIGO);
    let t1 = "Simulate Agent";
    let tw1 = text_width(fonts.semibold.as_ref(), t1, 12.0);
    cv.text(
        fonts.semibold.as_ref(),
        t1,
        18 + (btn_w - tw1) / 2,
        141,
        12.0,
        ON_ACCENT,
    );
    regions.push(Region {
        x: 18,
        y: 120,
        w: btn_w,
        h: 34,
        action: HitAction::Shell(ShellCmd::Notify("Agent · Organising ~/Downloads".into())),
    });

    let bx2 = 18 + btn_w + 10;
    cv.round_rect(bx2, 120, btn_w, 34, 10.0, Rgba::rgba(255, 255, 255, 24));
    let t2 = "Open Agent Space";
    let tw2 = text_width(fonts.semibold.as_ref(), t2, 12.0);
    cv.text(
        fonts.semibold.as_ref(),
        t2,
        bx2 + (btn_w - tw2) / 2,
        141,
        12.0,
        TEXT,
    );
    regions.push(Region {
        x: bx2,
        y: 120,
        w: btn_w,
        h: 34,
        action: HitAction::Launch(LaunchTarget::Ipc(
            "{\"Action\":{\"FocusWorkspace\":{\"reference\":{\"Index\":7}}}}",
        )),
    });

    regions
}
