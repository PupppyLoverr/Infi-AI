//! Surface painters and hit-test layouts for Bar, Dock (3 variants),
//! Launcher, Control Centre, and Expanded Dynamic Island.

use std::collections::HashMap;

use crate::agent::{Approval, Scope, Status, Summary as AgentSummary, TaskView, Verdict};
use crate::canvas::{text_width, wrap_text, Canvas, Fonts, Icon, Rgba};
use crate::launcher::{self, Action as LauncherAction, Item as LauncherItem, Kind as LauncherKind};
use crate::state::{
    dock_entries, focused_app_menus, load_app_icon, DockStyle, LaunchTarget, ShellCmd, ShellPrefs,
    Telemetry, WallpaperPreset, WindowInfo,
};
use crate::tokens::{
    Theme, ThemeKind, R_CARD, R_CONTROL, R_OVERLAY, R_ROW, R_TILE, T_BODY, T_CAPTION, T_LABEL,
    T_META, T_TITLE,
};

pub const BAR_HEIGHT: u32 = 34;
pub const NOTCH_WIDTH: i32 = 184;

pub const DOCK_FLOAT_HEIGHT: u32 = 58;
pub const TASKBAR_HEIGHT: u32 = 48;

pub const LAUNCHER_W: u32 = 640;
pub const LAUNCHER_H: u32 = 436;

pub const CONTROL_W: u32 = 348;
pub const CONTROL_H: u32 = 436;

pub const ISLAND_CARD_W: u32 = 372;
pub const ISLAND_CARD_H: u32 = 172;

/// Live, real state the docks reflect.
pub struct Live<'a> {
    pub agents: AgentSummary,
    pub telem: &'a Telemetry,
    pub prefs: &'a ShellPrefs,
}

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
    ToggleDnd,
    ToggleTheme,
    AgentRequest(String),
    SelectTask(usize),
    /// Pick offered choice `choice` of the open question on task `task`.
    Choose {
        task: usize,
        choice: usize,
    },
    /// Start typing to the selected agent.
    StartReply,
    Decide {
        task: usize,
        verdict: Verdict,
        scope: Scope,
    },
    SetVolumeFromX {
        x0: i32,
        w: i32,
    },
    SetBrightnessFromX {
        x0: i32,
        w: i32,
    },
}

pub fn floating_dock_width(style: DockStyle, wins: &[WindowInfo]) -> u32 {
    let entries = dock_entries(wins);
    match style {
        DockStyle::Solyra => {
            // Start (42) + divider (13) + N apps (42 each, 8 gap) + divider (13) + Agent (42) + 20 pad
            let apps_w = entries.len() as i32 * 40 + (entries.len().saturating_sub(1) as i32) * 8;
            (20 + 40 + 16 + apps_w + 16 + 40).max(220) as u32
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
    th: &Theme,
    fonts: &Fonts,
    ws_list: &[(u8, bool)],
    wins: &[WindowInfo],
    island_msg: Option<&str>,
    attention: Option<&str>,
    activity: Option<&str>,
    prefs: &ShellPrefs,
    telem: &Telemetry,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    // A plain message wins the island; otherwise an agent that needs the
    // human may use it. Approval pills are amber and click through to the
    // agents card.
    let is_attention = island_msg.is_none() && attention.is_some();
    // Plain messages win, then something that needs the human, then what a
    // working agent is doing (quiet, accent dot, opens the agent card).
    let is_activity = island_msg.is_none() && attention.is_none() && activity.is_some();
    let island_msg = island_msg.or(attention).or(activity);
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Translucent dark-tinted glass: lets niri's dual-kawase backdrop blur and
    // the wallpaper hue shine through, just like macOS Sequoia.
    cv.fill_rect(0, 0, w, h, th.bar);
    cv.fill_rect(0, h - 1, w, 1, th.tile);

    // Left: Solyra emblem button -> opens Launcher.
    let brand_hover = hover.is_some_and(|(hx, hy)| hx >= 6.0 && hx < 34.0 && hy < h as f64);
    if brand_hover {
        cv.round_rect(6, 5, 26, 24, 6.0, th.tile_hover);
    }
    cv.icon_solyra(19.0, 17.0, 6.2, th.accent);
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
    let app_w = cv.text(fonts.semibold.as_ref(), &app_name, x, 22, 13.0, th.text);
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
            cv.round_rect(x, 9, 20, 16, 8.0, th.accent);
        } else {
            let bg = if is_hov { th.tile_hover } else { th.tile };
            cv.round_rect(x, 9, 20, 16, 8.0, bg);
        }
        let label = idx.to_string();
        let tw = text_width(fonts.semibold.as_ref(), &label, 11.0);
        let fg = if active { th.on_accent } else { th.text_med };
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
        cv.fill_rect(x - 5, 10, 1, 14, th.tile);
        // Only labels that do something real are drawn. File/Edit/Go/Shell
        // used to open the launcher, which is not what those names promise;
        // per-app menus need a protocol we do not have yet.
        for &menu in menus
            .iter()
            .filter(|m| matches!(**m, "View" | "Window" | "Help"))
        {
            let mw = text_width(fonts.regular.as_ref(), menu, 12.2);
            if x + mw + 12 > max_left_x {
                break;
            }
            let m_hov = hover.is_some_and(|(hx, hy)| {
                hx >= (x - 4) as f64 && hx < (x + mw + 4) as f64 && hy >= 5.0 && hy < 29.0
            });
            if m_hov {
                cv.round_rect(x - 5, 5, mw + 10, 24, 5.0, th.tile_hover);
            }
            cv.text(fonts.regular.as_ref(), menu, x, 22, 12.2, th.text_med);
            let act = match menu {
                "Window" | "View" => {
                    HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}"))
                }
                "Help" => {
                    HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ShowHotkeyOverlay\":{}}}"))
                }
                _ => unreachable!("filtered above"),
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
        cv.round_rect(left, 3, pill_w, 28, 14.0, Rgba::rgb(0x09, 0x08, 0x10));
        // The island is always near-black, in both themes, so its content
        // uses the dark palette. The attention text itself says "needs
        // approval": the amber dot only reinforces it.
        let pd = Theme::dark();
        let dot = if is_attention {
            pd.agent_approval
        } else {
            pd.agent_active
        };
        cv.stroke_round_rect(left, 3, pill_w, 28, 14.0, 1.0, dot);
        cv.circle(left as f32 + 16.0, 17.0, 4.0, dot);
        cv.text_truncated(
            fonts.semibold.as_ref(),
            message,
            left + 28,
            21,
            12.0,
            pill_w - 64,
            pd.text,
        );
        regions.push(Region {
            x: left,
            y: 0,
            w: pill_w,
            h,
            action: HitAction::Shell(if is_attention || is_activity {
                ShellCmd::ToggleAgents
            } else {
                ShellCmd::ToggleIsland
            }),
        });
    } else {
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
    cv.text(fonts.semibold.as_ref(), &clock_str, rx, 22, 12.8, th.text);
    let clock_start = rx - date_w - 8;
    cv.text(
        fonts.regular.as_ref(),
        &date_str,
        clock_start,
        22,
        12.0,
        th.text_med,
    );
    regions.push(Region {
        x: clock_start - 4,
        y: 4,
        w: date_w + clock_w + 12,
        h: 26,
        action: HitAction::Shell(ShellCmd::ToggleIsland),
    });

    // Status pill (Control Centre trigger): Wi-Fi + Volume + Battery + Twin-switches.
    let status_w = 104;
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
        if status_hov { th.tile_hover } else { th.tile },
    );
    cv.icon_network(
        status_x as f32 + 15.0,
        17.0,
        14.5,
        telem.network,
        th.text,
        th.text_dim,
    );
    cv.icon_volume(status_x as f32 + 39.0, 17.0, 14.5, prefs.volume, th.text);
    if let Some(pct) = telem.battery_pct {
        cv.icon_battery(status_x as f32 + 65.0, 17.0, pct, th.text_med, th.success);
    }
    cv.icon_control_centre(status_x as f32 + 91.0, 17.0, th.text);
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
            cv.round_rect(search_x, 5, 24, 24, 7.0, th.tile_hover);
        }
        cv.icon_search(search_x as f32 + 12.0, 17.0, 13.5, th.text_med);
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

#[allow(clippy::too_many_arguments)]
pub fn draw_dock(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    style: DockStyle,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    live: &Live<'_>,
) -> Vec<Region> {
    match style {
        DockStyle::Solyra => draw_dock_solyra(cv, th, fonts, wins, icons, hover, live.agents),
        DockStyle::Macos => draw_dock_macos(cv, th, wins, icons, hover),
        DockStyle::Windows => draw_dock_windows(cv, th, fonts, wins, icons, hover, live),
    }
}

fn draw_dock_solyra(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    agents: AgentSummary,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Tight surface geometry (matches `geometry-corner-radius 20` in niri).
    card(cv, th, 0, 0, w, h, 20.0);

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
    cv.fill_rect(x, 14, 1, h - 28, th.tile_hover);
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
                th.accent
            } else {
                th.text_med
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
    cv.fill_rect(x, 14, 1, h - 28, th.tile_hover);
    x += 8;
    let ag_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
    let ay = if ag_hov { base_y - 3 } else { base_y };
    cv.draw_app_squircle(x, ay, cell, "agent", None);
    // Badge only when a real task needs the human (amber, with a count) or is
    // running (accent). No agent activity, no badge.
    if agents.live > 0 {
        let col = if agents.needs_attention() {
            th.agent_approval
        } else {
            th.agent_active
        };
        let (bx, by) = ((x + cell - 5) as f32, (ay + 5) as f32);
        cv.circle(bx, by, 6.0, Rgba::rgb(0x14, 0x11, 0x19));
        cv.circle(bx, by, 4.6, col);
        if agents.needs_attention() {
            let n = (agents.waiting + agents.blocked).min(9).to_string();
            cv.text(
                fonts.semibold.as_ref(),
                &n,
                x + cell - 8,
                ay + 9,
                9.0,
                th.on_accent,
            );
        }
    }
    regions.push(Region {
        x,
        y: 0,
        w: cell,
        h,
        action: HitAction::Shell(ShellCmd::ToggleAgents),
    });

    regions
}

fn draw_dock_macos(
    cv: &mut Canvas<'_>,
    th: &Theme,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Lighter frosted Sequoia glass rim + luminous dots under running apps.
    card(cv, th, 0, 0, w, h, 20.0);

    let mut x = 10;
    let cell = 44;
    let base_y = 6;

    // Apps / Launchpad icon first, then core + running apps.
    let entries = dock_entries(wins);
    for (idx, entry) in entries.iter().enumerate() {
        if idx == 1 {
            // Insert Launchpad / Spotlight right after Finder/Files, like macOS.
            let l_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
            cv.draw_app_squircle(
                x,
                if l_hov { base_y - 3 } else { base_y },
                cell,
                "start",
                None,
            );
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
                th.tile_hover,
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
    cv.fill_rect(x, 10, 1, h - 20, th.tile_hover);
    x += 8;
    let t_hov = hover.is_some_and(|(hx, _)| hx >= x as f64 && hx < (x + cell) as f64);
    cv.draw_app_squircle(
        x,
        if t_hov { base_y - 3 } else { base_y },
        cell,
        "trash",
        None,
    );
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
    th: &Theme,
    fonts: &Fonts,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    live: &Live<'_>,
) -> Vec<Region> {
    let (telem, prefs) = (live.telem, live.prefs);
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    // Full-width frosted Fluent taskbar.
    cv.fill_rect(0, 0, w, h, th.bar);
    cv.fill_rect(0, 0, w, 1, th.tile_hover);

    // Left: Weather / Widgets pill (Image 3).
    let w_hov = hover.is_some_and(|(hx, _)| hx >= 10.0 && hx < 142.0);
    if w_hov {
        cv.round_rect(10, 6, 132, 36, 8.0, th.tile);
    }
    cv.circle(26.0, 21.0, 5.5, Rgba::rgb(0xfb, 0xbf, 0x24));
    cv.round_rect(25, 21, 13, 7, 3.5, Rgba::rgba(240, 244, 255, 235));
    cv.text(fonts.semibold.as_ref(), "19°C", 46, 22, 11.5, th.text);
    cv.text(
        fonts.regular.as_ref(),
        "Clear sky",
        46,
        35,
        10.5,
        th.text_dim,
    );
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
        if s_hov { th.tile_hover } else { th.tile },
    );
    cv.stroke_round_rect(cx, 8, search_w, 32, 16.0, 1.0, th.tile_hover);
    cv.icon_search(cx as f32 + 18.0, 24.0, 13.0, th.text_med);
    cv.text(
        fonts.regular.as_ref(),
        "Search or ask…",
        cx + 32,
        28,
        12.0,
        th.text_dim,
    );
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
            cv.round_rect(cx + (40 - uw) / 2, h - 4, uw, 3, 1.5, th.accent);
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
    cv.text(fonts.semibold.as_ref(), &t_str, w - 58, 21, 11.5, th.text);
    cv.text(
        fonts.regular.as_ref(),
        &d_str,
        w - 74,
        35,
        10.2,
        th.text_dim,
    );
    cv.icon_network(
        (w - 134) as f32,
        24.0,
        12.5,
        telem.network,
        th.text,
        th.text_dim,
    );
    cv.icon_volume((w - 112) as f32, 24.0, 12.5, prefs.volume, th.text);
    if let Some(pct) = telem.battery_pct {
        cv.icon_battery((w - 90) as f32, 24.0, pct, th.text_med, th.success);
    }
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

/// Flat overlay card: one fill, one hairline.
fn card(cv: &mut Canvas<'_>, th: &Theme, x: i32, y: i32, w: i32, h: i32, r: f32) {
    cv.round_rect(x, y, w, h, r, th.surface);
    cv.stroke_round_rect(x, y, w, h, r, 1.0, th.hairline);
}

fn hovered(hover: Option<(f64, f64)>, x: i32, y: i32, w: i32, h: i32) -> bool {
    hover.is_some_and(|(hx, hy)| {
        hx >= x as f64 && hx < (x + w) as f64 && hy >= y as f64 && hy < (y + h) as f64
    })
}

/// Top of the result list and the space reserved under it for the footer.
const LIST_TOP: i32 = 74;
const FOOTER_H: i32 = 50;

pub fn launcher_list_height() -> i32 {
    LAUNCHER_H as i32 - LIST_TOP - FOOTER_H
}

fn row_verb(item: &LauncherItem) -> &'static str {
    match item.kind {
        LauncherKind::Application => "Open ⏎",
        LauncherKind::Window => "Switch ⏎",
        LauncherKind::Command => "Run ⏎",
        LauncherKind::Agent => "Queue ⏎",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_launcher(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    query: &str,
    items: &[LauncherItem],
    selected: usize,
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    card(cv, th, 0, 0, w, h, R_OVERLAY);

    // Search field.
    cv.round_rect(18, 16, w - 36, 48, R_TILE, th.field);
    cv.stroke_round_rect(18, 16, w - 36, 48, R_TILE, 1.0, th.selected_border);
    cv.icon_search(42.0, 40.0, 16.0, th.accent);
    if query.is_empty() {
        cv.text(
            fonts.regular.as_ref(),
            "Search apps, windows and commands",
            62,
            45,
            14.0,
            th.text_dim,
        );
        cv.fill_rect(62, 30, 2, 20, th.accent);
    } else {
        let max_w = w - 62 - 90;
        let qw = cv.text_truncated(fonts.regular.as_ref(), query, 62, 45, 14.5, max_w, th.text);
        cv.fill_rect(64 + qw, 30, 2, 20, th.accent);
    }
    cv.round_rect(w - 68, 28, 38, 24, 6.0, th.tile);
    cv.text(fonts.mono.as_ref(), "esc", w - 59, 44, 11.0, th.text_dim);
    regions.push(Region {
        x: w - 68,
        y: 28,
        w: 38,
        h: 24,
        action: HitAction::Shell(ShellCmd::CloseOverlays),
    });

    let list_h = launcher_list_height();
    let only_agent_row = !launcher::has_direct_match(items) && !items.is_empty();

    if items.is_empty() {
        let (title, body) = if query.trim().is_empty() {
            (
                "Nothing to show",
                "No applications were found on this system.".to_string(),
            )
        } else {
            (
                "No matches",
                "Apps, open windows and commands are searched. Files are not.".to_string(),
            )
        };
        cv.text(
            fonts.semibold.as_ref(),
            title,
            28,
            LIST_TOP + 34,
            T_TITLE,
            th.text,
        );
        for (i, line) in wrap_text(fonts.regular.as_ref(), &body, T_LABEL, w - 56, 2)
            .iter()
            .enumerate()
        {
            cv.text(
                fonts.regular.as_ref(),
                line,
                28,
                LIST_TOP + 58 + i as i32 * 18,
                T_LABEL,
                th.text_dim,
            );
        }
    } else {
        let sel = selected.min(items.len() - 1);
        let first = launcher::first_visible(items, sel, list_h);
        let mut y = LIST_TOP;
        for row in launcher::rows_from(items, first, list_h) {
            if let Some(kind) = row.heading {
                cv.text(
                    fonts.semibold.as_ref(),
                    kind.heading(),
                    24,
                    y + 15,
                    T_CAPTION,
                    th.text_dim,
                );
                y += launcher::HEADING_H;
            }
            draw_launcher_row(
                cv,
                th,
                fonts,
                icons,
                &mut regions,
                w,
                y,
                &items[row.index],
                row.index == sel,
                hover,
            );
            y += launcher::ROW_H;
        }
        if only_agent_row {
            // The only thing offered is an agent request. Say so, so Enter
            // is never mistaken for "launch the thing I typed".
            let note = "No app, window or command matches. Enter only queues a request.";
            cv.text_truncated(
                fonts.regular.as_ref(),
                note,
                24,
                LIST_TOP + list_h - 6,
                T_META,
                w - 48,
                th.text_dim,
            );
        }
    }

    // Footer: key hints on the left, session actions on the right.
    let fy = h - FOOTER_H;
    cv.fill_rect(18, fy, w - 36, 1, th.separator);
    cv.text(
        fonts.mono.as_ref(),
        "↑↓ move   ⏎ run   esc close",
        24,
        fy + 29,
        11.0,
        th.text_dim,
    );

    cv.round_rect(w - 150, fy + 10, 56, 30, R_CONTROL, th.tile);
    cv.icon_lock((w - 138) as f32, (fy + 25) as f32, th.text_med);
    cv.text(
        fonts.regular.as_ref(),
        "Lock",
        w - 126,
        fy + 29,
        T_META,
        th.text_med,
    );
    regions.push(Region {
        x: w - 150,
        y: fy + 10,
        w: 56,
        h: 30,
        action: HitAction::Launch(LaunchTarget::Shell("swaylock".into())),
    });

    cv.round_rect(w - 88, fy + 10, 68, 30, R_CONTROL, th.tile);
    cv.icon_power((w - 76) as f32, (fy + 25) as f32, th.danger);
    cv.text(
        fonts.regular.as_ref(),
        "Log out",
        w - 64,
        fy + 29,
        T_META,
        th.text_med,
    );
    // niri asks for confirmation itself; we do not skip it.
    regions.push(Region {
        x: w - 88,
        y: fy + 10,
        w: 68,
        h: 30,
        action: HitAction::Launch(LaunchTarget::Ipc(
            "{\"Action\":{\"Quit\":{\"skip_confirmation\":false}}}",
        )),
    });

    regions
}

#[allow(clippy::too_many_arguments)]
fn draw_launcher_row(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    icons: &mut HashMap<String, Option<Icon>>,
    regions: &mut Vec<Region>,
    w: i32,
    y: i32,
    item: &LauncherItem,
    selected: bool,
    hover: Option<(f64, f64)>,
) {
    let (rx, rw, rh) = (16, w - 32, launcher::ROW_H - 2);
    let runnable = item.is_runnable();
    if selected {
        cv.round_rect(rx, y, rw, rh, R_ROW, th.selected_fill);
        cv.stroke_round_rect(rx, y, rw, rh, R_ROW, 1.0, th.selected_border);
    } else if runnable && hovered(hover, rx, y, rw, rh) {
        cv.round_rect(rx, y, rw, rh, R_ROW, th.tile_hover);
    }

    let sys_ic = load_app_icon(icons, &item.app_id);
    cv.draw_app_squircle(rx + 8, y + 4, 28, &item.role, sys_ic);
    let title_col = if runnable { th.text } else { th.text_dim };
    let badge_w = if selected { 72 } else { 0 };
    let text_w = rw - 52 - badge_w - 8;
    let tw = cv.text_truncated(
        fonts.semibold.as_ref(),
        &item.title,
        rx + 44,
        y + 17,
        T_BODY,
        text_w,
        title_col,
    );
    let _ = tw;
    cv.text_truncated(
        fonts.regular.as_ref(),
        &item.subtitle,
        rx + 44,
        y + 32,
        T_META,
        text_w,
        th.text_dim,
    );

    if selected {
        let label = if runnable {
            row_verb(item)
        } else {
            "Unavailable"
        };
        let bw = if runnable { 64 } else { 84 };
        cv.round_rect(rx + rw - bw - 8, y + 8, bw, 22, 6.0, th.tile_hover);
        cv.text(
            fonts.mono.as_ref(),
            label,
            rx + rw - bw - 2,
            y + 23,
            10.8,
            th.text,
        );
    }

    if runnable {
        let action = match &item.action {
            LauncherAction::Launch(t) => HitAction::Launch(t.clone()),
            LauncherAction::FocusWindow(id) => HitAction::FocusWindow(*id),
            LauncherAction::AgentRequest(q) => HitAction::AgentRequest(q.clone()),
            LauncherAction::Unavailable => return,
        };
        regions.push(Region {
            x: rx,
            y,
            w: rw,
            h: rh,
            action,
        });
    }
}

// --------------------------------------------------------- Control Centre

#[allow(clippy::too_many_arguments)]
pub fn draw_control(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    prefs: &ShellPrefs,
    telem: &Telemetry,
    agents: AgentSummary,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    card(cv, th, 0, 0, w, h, R_CARD);

    // 1. Status tiles. Network is read-only: Solyra reads link state, it does
    //    not manage connections yet, so the tile is not a button.
    let tile_w = (w - 32 - 10) / 2;
    let tile_h = 54;
    let agent_sub = if !agents.runtime_present {
        "No runtime".to_string()
    } else if agents.waiting > 0 {
        format!("{} need approval", agents.waiting)
    } else if agents.live > 0 {
        format!("{} running", agents.live)
    } else {
        "Idle".to_string()
    };
    let tiles: [(&str, String, bool, Option<HitAction>); 4] = [
        (
            "Network",
            telem.network.label().to_string(),
            false,
            crate::state::on_path("nmtui").then(|| {
                HitAction::Launch(LaunchTarget::Shell(format!(
                    "{} -e nmtui",
                    crate::state::role_binary("terminal", "foot")
                )))
            }),
        ),
        (
            "Agents",
            agent_sub,
            agents.live > 0,
            Some(HitAction::Shell(ShellCmd::ToggleAgents)),
        ),
        (
            "Focus",
            if prefs.dnd_on {
                "Holding notices".into()
            } else {
                "Off".to_string()
            },
            prefs.dnd_on,
            Some(HitAction::ToggleDnd),
        ),
        (
            "Appearance",
            if prefs.theme == ThemeKind::Dark {
                "Dark".into()
            } else {
                "Light".to_string()
            },
            false,
            Some(HitAction::ToggleTheme),
        ),
    ];

    for (i, (title, sub, active, act)) in tiles.into_iter().enumerate() {
        let col = (i % 2) as i32;
        let row = (i / 2) as i32;
        let tx = 16 + col * (tile_w + 10);
        let ty = 16 + row * (tile_h + 10);
        let is_hov = act.is_some() && hovered(hover, tx, ty, tile_w, tile_h);
        if active {
            cv.round_rect(tx, ty, tile_w, tile_h, R_TILE, th.accent);
        } else {
            cv.round_rect(
                tx,
                ty,
                tile_w,
                tile_h,
                R_TILE,
                if is_hov { th.tile_hover } else { th.tile },
            );
        }
        let fg = if active { th.on_accent } else { th.text };
        let fg_sub = if active { th.on_accent } else { th.text_dim };
        let (icx, icy) = (tx as f32 + 22.0, ty as f32 + 27.0);
        match i {
            0 => cv.icon_network(icx, icy, 14.0, telem.network, fg, fg_sub),
            1 => cv.icon_solyra(icx, icy, 6.0, fg),
            2 => cv.icon_moon(icx, icy, fg),
            _ => cv.icon_sun(icx, icy, fg),
        }
        cv.text(fonts.semibold.as_ref(), title, tx + 40, ty + 23, 12.2, fg);
        cv.text_truncated(
            fonts.regular.as_ref(),
            &sub,
            tx + 40,
            ty + 40,
            T_CAPTION + 0.4,
            tile_w - 48,
            fg_sub,
        );
        if let Some(action) = act {
            regions.push(Region {
                x: tx,
                y: ty,
                w: tile_w,
                h: tile_h,
                action,
            });
        }
    }

    // 2. Sliders. A slider that cannot do anything on this machine says so
    //    instead of moving.
    let sy = 148;
    cv.round_rect(16, sy, w - 32, 96, R_TILE, th.tile);
    draw_slider_row(
        cv,
        th,
        fonts,
        &mut regions,
        28,
        sy + 14,
        w - 56,
        "Display",
        prefs.brightness,
        true,
        telem.brightness_controllable,
    );
    draw_slider_row(
        cv,
        th,
        fonts,
        &mut regions,
        28,
        sy + 54,
        w - 56,
        "Sound",
        prefs.volume,
        false,
        telem.volume_controllable,
    );

    // 3. Dock style.
    let dy = 258;
    cv.text(
        fonts.semibold.as_ref(),
        "DOCK STYLE",
        20,
        dy + 12,
        T_CAPTION,
        th.text_dim,
    );
    let seg_y = dy + 20;
    let seg_w = w - 32;
    cv.round_rect(16, seg_y, seg_w, 34, R_ROW, th.field);
    let item_w = (seg_w - 8) / 3;
    for (i, style) in [DockStyle::Solyra, DockStyle::Macos, DockStyle::Windows]
        .into_iter()
        .enumerate()
    {
        let ix = 20 + i as i32 * item_w;
        let active = prefs.dock_style == style;
        if active {
            cv.round_rect(ix, seg_y + 3, item_w, 28, R_CONTROL, th.accent);
        }
        let label = style.label();
        let tw = text_width(fonts.semibold.as_ref(), label, 11.8);
        cv.text(
            fonts.semibold.as_ref(),
            label,
            ix + (item_w - tw) / 2,
            seg_y + 21,
            11.8,
            if active { th.on_accent } else { th.text_med },
        );
        regions.push(Region {
            x: ix,
            y: seg_y,
            w: item_w,
            h: 34,
            action: HitAction::Shell(ShellCmd::SetDock(style)),
        });
    }

    // 4. Wallpaper.
    let wy = 328;
    cv.text(
        fonts.semibold.as_ref(),
        "WALLPAPER",
        20,
        wy + 12,
        T_CAPTION,
        th.text_dim,
    );
    let mut sx = 28;
    for preset in WallpaperPreset::ALL {
        let active = prefs.wallpaper == preset;
        if active {
            cv.ring(sx as f32, (wy + 32) as f32, 13.5, 2.0, th.text);
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
        T_LABEL,
        th.text_med,
    );

    // 5. Real telemetry footer.
    let my = 386;
    cv.fill_rect(16, my, w - 32, 1, th.separator);
    let mut label = format!(
        "Memory · {} / {} MiB",
        telem.mem_used_mib, telem.mem_total_mib
    );
    if let Some(pct) = telem.battery_pct {
        label = format!("Battery {pct}% · {label}");
    }
    cv.text_truncated(
        fonts.regular.as_ref(),
        &label,
        20,
        my + 24,
        T_META,
        w - 40 - 110,
        th.text_med,
    );
    let bar_w = 96;
    let bx = w - 20 - bar_w;
    cv.round_rect(bx, my + 15, bar_w, 8, 4.0, th.tile_hover);
    let ratio = (telem.mem_used_mib as f32 / telem.mem_total_mib as f32).clamp(0.02, 1.0);
    cv.round_rect(
        bx,
        my + 15,
        (bar_w as f32 * ratio).round() as i32,
        8,
        4.0,
        th.accent,
    );

    regions
}

#[allow(clippy::too_many_arguments)]
fn draw_slider_row(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    regions: &mut Vec<Region>,
    x: i32,
    y: i32,
    w: i32,
    label: &str,
    pct: u8,
    is_brightness: bool,
    available: bool,
) {
    cv.text(
        fonts.semibold.as_ref(),
        label,
        x,
        y + 10,
        T_META,
        th.text_med,
    );
    let right = if available {
        format!("{pct}%")
    } else {
        "Not available on this device".to_string()
    };
    let pw = text_width(fonts.regular.as_ref(), &right, T_META);
    cv.text(
        fonts.regular.as_ref(),
        &right,
        x + w - pw,
        y + 10,
        T_META,
        th.text_dim,
    );

    let ty = y + 16;
    let th_px = 18;
    cv.round_rect(x, ty, w, th_px, 9.0, th.field);
    if !available {
        return;
    }
    let fill_w = ((w as f32) * (pct.clamp(8, 100) as f32 / 100.0)).round() as i32;
    cv.round_rect(x, ty, fill_w, th_px, 9.0, th.accent);
    if is_brightness {
        cv.icon_sun(x as f32 + 11.0, ty as f32 + 9.0, th.on_accent);
        regions.push(Region {
            x,
            y: ty - 4,
            w,
            h: th_px + 8,
            action: HitAction::SetBrightnessFromX { x0: x, w },
        });
    } else {
        cv.icon_volume(x as f32 + 12.0, ty as f32 + 9.0, 11.0, pct, th.on_accent);
        regions.push(Region {
            x,
            y: ty - 4,
            w,
            h: th_px + 8,
            action: HitAction::SetVolumeFromX { x0: x, w },
        });
    }
}

// -------------------------------------------------- Expanded Island Card

pub fn draw_island_card(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    island_msg: Option<&str>,
    wins: &[WindowInfo],
    telem: &Telemetry,
    agents: AgentSummary,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    card(cv, th, 0, 0, w, h, R_OVERLAY);

    cv.draw_app_squircle(18, 18, 36, "agent", None);
    cv.text(
        fonts.semibold.as_ref(),
        "Activity",
        64,
        33,
        T_TITLE,
        th.text,
    );
    let status = match island_msg {
        Some(m) => m.to_string(),
        None if !agents.runtime_present => "No agent runtime connected".to_string(),
        None if agents.waiting > 0 => format!("{} approval request(s) waiting", agents.waiting),
        None if agents.live > 0 => format!("{} agent task(s) running", agents.live),
        None => "Agents idle".to_string(),
    };
    cv.text_truncated(
        fonts.regular.as_ref(),
        &status,
        64,
        51,
        T_LABEL,
        w - 84,
        th.text_dim,
    );

    cv.round_rect(18, 68, w - 36, 40, R_ROW, th.tile);
    let summary = format!(
        "{} open windows  ·  {} MiB RAM used",
        wins.len(),
        telem.mem_used_mib
    );
    cv.text(
        fonts.regular.as_ref(),
        &summary,
        30,
        93,
        T_LABEL,
        th.text_med,
    );

    let btn_w = (w - 36 - 10) / 2;
    cv.round_rect(18, 120, btn_w, 34, R_ROW, th.accent);
    let t1 = "Agent tasks";
    let tw1 = text_width(fonts.semibold.as_ref(), t1, T_LABEL);
    cv.text(
        fonts.semibold.as_ref(),
        t1,
        18 + (btn_w - tw1) / 2,
        141,
        T_LABEL,
        th.on_accent,
    );
    regions.push(Region {
        x: 18,
        y: 120,
        w: btn_w,
        h: 34,
        action: HitAction::Shell(ShellCmd::ToggleAgents),
    });

    let bx2 = 18 + btn_w + 10;
    cv.round_rect(bx2, 120, btn_w, 34, R_ROW, th.tile);
    let t2 = "Workspaces";
    let tw2 = text_width(fonts.semibold.as_ref(), t2, T_LABEL);
    cv.text(
        fonts.semibold.as_ref(),
        t2,
        bx2 + (btn_w - tw2) / 2,
        141,
        T_LABEL,
        th.text,
    );
    regions.push(Region {
        x: bx2,
        y: 120,
        w: btn_w,
        h: 34,
        action: HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}")),
    });

    regions
}

// ------------------------------------------------------------ Agent tasks

pub const AGENTS_W: u32 = 460;
pub const AGENTS_H: u32 = 520;

/// Everything the agents card draws, borrowed from the app.
pub struct AgentsView<'a> {
    pub runtime_present: bool,
    pub tasks: &'a [TaskView],
    pub selected: usize,
    pub now: u64,
    pub root_hint: &'a str,
    /// Set when a decision could not be recorded; shown until the next action.
    pub error: Option<&'a str>,
    /// What the user has typed so far in the reply field.
    pub reply: &'a str,
    /// Keys are going into the reply field.
    pub typing: bool,
}

fn status_glyph(cv: &mut Canvas<'_>, th: &Theme, status: Status, cx: f32, cy: f32) {
    // Shape and label carry the state; colour only reinforces it.
    match status {
        Status::Running => {
            cv.circle(cx, cy, 5.0, th.agent_active);
        }
        Status::WaitingApproval => {
            cv.ring(cx, cy, 5.5, 2.0, th.agent_approval);
            cv.circle(cx, cy, 1.8, th.agent_approval);
        }
        Status::Blocked => {
            cv.round_rect(cx as i32 - 5, cy as i32 - 5, 10, 10, 2.0, th.warning);
        }
        Status::Finished => {
            cv.line(cx - 4.5, cy, cx - 1.5, cy + 3.5, 2.0, th.success);
            cv.line(cx - 1.5, cy + 3.5, cx + 5.0, cy - 4.0, 2.0, th.success);
        }
        Status::Failed => {
            cv.line(cx - 4.0, cy - 4.0, cx + 4.0, cy + 4.0, 2.0, th.danger);
            cv.line(cx - 4.0, cy + 4.0, cx + 4.0, cy - 4.0, 2.0, th.danger);
        }
    }
}

fn ago(now: u64, ts: u64) -> String {
    let s = now.saturating_sub(ts);
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        _ => format!("{}h ago", s / 3600),
    }
}

fn reversible_line(a: &Approval) -> (&'static str, &'static str) {
    match a.reversible {
        Some(true) => ("Can be undone:", "Yes, as reported by the agent"),
        Some(false) => ("Can be undone:", "No. Solyra cannot reverse this"),
        None => ("Can be undone:", "Not reported by the agent"),
    }
}

const TASK_ROW_H: i32 = 54;
const DETAIL_TOP: i32 = 330;

pub fn draw_agents(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    view: &AgentsView<'_>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();

    card(cv, th, 0, 0, w, h, R_OVERLAY);

    cv.text(
        fonts.semibold.as_ref(),
        "Agent tasks",
        22,
        36,
        15.0,
        th.text,
    );
    let sub = if !view.runtime_present {
        "No runtime connected".to_string()
    } else if view.tasks.is_empty() {
        "No tasks".to_string()
    } else {
        format!("{} task(s)", view.tasks.len())
    };
    cv.text(fonts.regular.as_ref(), &sub, 22, 55, T_META, th.text_dim);
    cv.round_rect(w - 68, 22, 38, 24, 6.0, th.tile);
    cv.text(fonts.mono.as_ref(), "esc", w - 59, 38, 11.0, th.text_dim);
    regions.push(Region {
        x: w - 68,
        y: 22,
        w: 38,
        h: 24,
        action: HitAction::Shell(ShellCmd::CloseOverlays),
    });
    cv.fill_rect(18, 66, w - 36, 1, th.separator);

    if !view.runtime_present || view.tasks.is_empty() {
        let (title, lines): (&str, Vec<String>) = if !view.runtime_present {
            (
                "No agent runtime is connected",
                vec![
                    "Tasks appear here when an agent runtime reports them.".into(),
                    format!("Watching {}", view.root_hint),
                    "Protocol: docs/agent-protocol.md".into(),
                ],
            )
        } else {
            (
                "No tasks right now",
                vec![
                    "Start one from the launcher: type a request and choose “Ask an agent”.".into(),
                ],
            )
        };
        cv.text(fonts.semibold.as_ref(), title, 22, 110, T_TITLE, th.text);
        let mut y = 134;
        for line in &lines {
            for l in wrap_text(fonts.regular.as_ref(), line, T_LABEL, w - 44, 3) {
                cv.text(fonts.regular.as_ref(), &l, 22, y, T_LABEL, th.text_dim);
                y += 18;
            }
            y += 4;
        }
        return regions;
    }

    // Task list (top half).
    let sel = view.selected.min(view.tasks.len() - 1);
    let list_h = DETAIL_TOP - 72;
    let visible = (list_h / TASK_ROW_H).max(1) as usize;
    let first = sel.saturating_sub(visible - 1);
    for (slot, idx) in (first..view.tasks.len()).take(visible).enumerate() {
        let t = &view.tasks[idx];
        let y = 72 + slot as i32 * TASK_ROW_H;
        let (rx, rw, rh) = (14, w - 28, TASK_ROW_H - 4);
        if idx == sel {
            cv.round_rect(rx, y, rw, rh, R_ROW, th.selected_fill);
            cv.stroke_round_rect(rx, y, rw, rh, R_ROW, 1.0, th.selected_border);
        } else if hovered(hover, rx, y, rw, rh) {
            cv.round_rect(rx, y, rw, rh, R_ROW, th.tile_hover);
        }
        status_glyph(cv, th, t.status, rx as f32 + 20.0, y as f32 + 17.0);
        cv.text_truncated(
            fonts.semibold.as_ref(),
            &t.name,
            rx + 38,
            y + 21,
            T_BODY,
            rw - 150,
            th.text,
        );
        let label = if t.is_quiet(view.now) {
            format!(
                "{} · quiet {}",
                t.status.label(),
                ago(view.now, t.last_ts).replace(" ago", "")
            )
        } else {
            t.status.label().to_string()
        };
        let lw = text_width(fonts.regular.as_ref(), &label, T_META);
        cv.text(
            fonts.regular.as_ref(),
            &label,
            rx + rw - lw - 10,
            y + 21,
            T_META,
            th.text_med,
        );
        let line2 = t
            .latest_milestone()
            .map(|m| m.text.as_str())
            .or(t.detail.as_deref())
            .unwrap_or("No activity reported yet");
        cv.text_truncated(
            fonts.regular.as_ref(),
            line2,
            rx + 38,
            y + 38,
            T_META,
            rw - 50,
            th.text_dim,
        );
        regions.push(Region {
            x: rx,
            y,
            w: rw,
            h: rh,
            action: HitAction::SelectTask(idx),
        });
    }
    if view.tasks.len() > first + visible {
        let more = format!("+{} more", view.tasks.len() - first - visible);
        cv.text(
            fonts.regular.as_ref(),
            &more,
            22,
            DETAIL_TOP - 8,
            T_META,
            th.text_dim,
        );
    }

    // Detail (bottom half).
    cv.fill_rect(18, DETAIL_TOP - 2, w - 36, 1, th.separator);
    let t = &view.tasks[sel];
    let mut y = DETAIL_TOP + 20;

    if let Some(ap) = t
        .pending
        .as_ref()
        .filter(|a| a.kind == crate::agent::AskKind::Question)
    {
        let who = t.agent.as_deref().unwrap_or(&t.name).to_uppercase();
        cv.text(
            fonts.semibold.as_ref(),
            &format!("QUESTION FROM {who}"),
            22,
            y,
            T_CAPTION,
            th.agent_approval,
        );
        y += 20;
        for l in wrap_text(fonts.semibold.as_ref(), &ap.action, T_BODY, w - 44, 3) {
            cv.text(fonts.semibold.as_ref(), &l, 22, y, T_BODY, th.text);
            y += 18;
        }
        let by = h - 78;
        if let Some((verdict, _)) = t.decision_sent {
            let msg = if verdict == Verdict::Answer {
                "Sent: your answer. Waiting for the agent."
            } else {
                "Sent. Waiting for the agent."
            };
            card_row(cv, th, fonts, 18, by, w - 36, msg);
        } else if !ap.choices.is_empty() {
            let n = ap.choices.len().min(4) as i32;
            let gap = 8;
            let bw = (w - 36 - gap * (n - 1)) / n;
            for (i, c) in ap.choices.iter().take(4).enumerate() {
                let bx = 18 + i as i32 * (bw + gap);
                cv.round_rect(
                    bx,
                    by,
                    bw,
                    34,
                    R_ROW,
                    if hovered(hover, bx, by, bw, 34) {
                        th.tile_hover
                    } else {
                        th.tile
                    },
                );
                cv.stroke_round_rect(bx, by, bw, 34, R_ROW, 1.0, th.separator);
                cv.text_truncated(
                    fonts.semibold.as_ref(),
                    &format!("{} {}", i + 1, c),
                    bx + 10,
                    by + 21,
                    T_LABEL,
                    bw - 20,
                    th.text,
                );
                regions.push(Region {
                    x: bx,
                    y: by,
                    w: bw,
                    h: 34,
                    action: HitAction::Choose {
                        task: sel,
                        choice: i,
                    },
                });
            }
        } else {
            reply_field(
                cv,
                th,
                fonts,
                18,
                by,
                w - 36,
                view.reply,
                view.typing,
                "Type your answer, Enter to send",
            );
        }
        cv.text_truncated(
            fonts.regular.as_ref(),
            "Your answer goes to the agent as typed. This is not a sandbox.",
            22,
            h - 20,
            T_CAPTION,
            w - 44,
            th.text_dim,
        );
    } else if let Some(ap) = &t.pending {
        cv.text(
            fonts.semibold.as_ref(),
            "APPROVAL REQUESTED",
            22,
            y,
            T_CAPTION,
            th.agent_approval,
        );
        y += 20;
        for l in wrap_text(fonts.semibold.as_ref(), &ap.action, T_BODY, w - 44, 2) {
            cv.text(fonts.semibold.as_ref(), &l, 22, y, T_BODY, th.text);
            y += 18;
        }
        let field = |cv: &mut Canvas<'_>, y: &mut i32, k: &str, v: &str| {
            let kw = cv.text(fonts.regular.as_ref(), k, 22, *y, T_META, th.text_dim);
            cv.text_truncated(
                fonts.regular.as_ref(),
                v,
                22 + kw + 6,
                *y,
                T_META,
                w - 44 - kw - 6,
                th.text_med,
            );
            *y += 16;
        };
        field(
            cv,
            &mut y,
            "Affects:",
            ap.resource
                .as_deref()
                .unwrap_or("Not reported by the agent"),
        );
        if let Some(reason) = &ap.reason {
            field(cv, &mut y, "Why:", reason);
        }
        let (k, v) = reversible_line(ap);
        field(cv, &mut y, k, v);

        let by = h - 78;
        if let Some((verdict, scope)) = t.decision_sent {
            let msg = match (verdict, scope) {
                (Verdict::Approve, Scope::Once) => "Sent: approved once. Waiting for the runtime.",
                (Verdict::Approve, Scope::Task) => {
                    "Sent: approved for this task. Waiting for the runtime."
                }
                (Verdict::Deny, _) => "Sent: denied. Waiting for the runtime.",
                (Verdict::Answer, _) => "Sent: your answer. Waiting for the runtime.",
            };
            cv.round_rect(18, by, w - 36, 34, R_ROW, th.tile);
            cv.text_truncated(
                fonts.regular.as_ref(),
                msg,
                30,
                by + 21,
                T_LABEL,
                w - 60,
                th.text,
            );
        } else {
            // Deny sits first and is not visually subordinate: no pressure.
            let gap = 8;
            let can_task = ap.scopes.contains(&Scope::Task);
            let n = if can_task { 3 } else { 2 };
            let bw = (w - 36 - gap * (n - 1)) / n;
            let mut bx = 18;
            let mut button =
                |cv: &mut Canvas<'_>, label: &str, key: &str, action: HitAction, primary: bool| {
                    if primary {
                        cv.round_rect(bx, by, bw, 34, R_ROW, th.accent);
                    } else {
                        cv.round_rect(
                            bx,
                            by,
                            bw,
                            34,
                            R_ROW,
                            if hovered(hover, bx, by, bw, 34) {
                                th.tile_hover
                            } else {
                                th.tile
                            },
                        );
                        cv.stroke_round_rect(bx, by, bw, 34, R_ROW, 1.0, th.separator);
                    }
                    let fg = if primary { th.on_accent } else { th.text };
                    let full = format!("{label} ({key})");
                    let tw = text_width(fonts.semibold.as_ref(), &full, T_LABEL);
                    cv.text(
                        fonts.semibold.as_ref(),
                        &full,
                        bx + (bw - tw).max(0) / 2,
                        by + 21,
                        T_LABEL,
                        fg,
                    );
                    regions.push(Region {
                        x: bx,
                        y: by,
                        w: bw,
                        h: 34,
                        action,
                    });
                    bx += bw + gap;
                };
            button(
                cv,
                "Deny",
                "D",
                HitAction::Decide {
                    task: sel,
                    verdict: Verdict::Deny,
                    scope: Scope::Once,
                },
                false,
            );
            button(
                cv,
                "Approve once",
                "A",
                HitAction::Decide {
                    task: sel,
                    verdict: Verdict::Approve,
                    scope: Scope::Once,
                },
                false,
            );
            if can_task {
                button(
                    cv,
                    "This task",
                    "T",
                    HitAction::Decide {
                        task: sel,
                        verdict: Verdict::Approve,
                        scope: Scope::Task,
                    },
                    false,
                );
            }
        }
        let note = "Records your answer for the agent runtime. This is not a sandbox.";
        cv.text_truncated(
            fonts.regular.as_ref(),
            note,
            22,
            h - 20,
            T_CAPTION,
            w - 44,
            th.text_dim,
        );
    } else {
        if let Some(obj) = &t.objective {
            cv.text(
                fonts.semibold.as_ref(),
                "OBJECTIVE",
                22,
                y,
                T_CAPTION,
                th.text_dim,
            );
            y += 16;
            for l in wrap_text(fonts.regular.as_ref(), obj, T_LABEL, w - 44, 2) {
                cv.text(fonts.regular.as_ref(), &l, 22, y, T_LABEL, th.text);
                y += 17;
            }
            y += 4;
        }
        let env = t
            .environment
            .as_deref()
            .unwrap_or("Environment not reported by the agent");
        cv.text(
            fonts.semibold.as_ref(),
            "RUNS IN",
            22,
            y,
            T_CAPTION,
            th.text_dim,
        );
        y += 16;
        cv.text_truncated(fonts.regular.as_ref(), env, 22, y, T_LABEL, w - 44, th.text);
        y += 22;
        if let Some(d) = &t.detail {
            let head = match t.status {
                Status::Failed => "ERROR",
                Status::Blocked => "BLOCKED ON",
                _ => "SUMMARY",
            };
            cv.text(fonts.semibold.as_ref(), head, 22, y, T_CAPTION, th.text_dim);
            y += 16;
            for l in wrap_text(fonts.regular.as_ref(), d, T_LABEL, w - 44, 3) {
                cv.text(fonts.regular.as_ref(), &l, 22, y, T_LABEL, th.text);
                y += 17;
            }
        }
        if t.interactive && !t.status.is_terminal() {
            let by = h - 78;
            let hint = if view.typing {
                "Message to the agent, Enter to send"
            } else {
                "Press R to message this agent"
            };
            reply_field(cv, th, fonts, 18, by, w - 36, view.reply, view.typing, hint);
            regions.push(Region {
                x: 18,
                y: by,
                w: w - 36,
                h: 34,
                action: HitAction::StartReply,
            });
        }
        let skipped = t.malformed_lines + t.unknown_events;
        if skipped > 0 {
            let note = format!("{skipped} event(s) in this task's log could not be read.");
            cv.text_truncated(
                fonts.regular.as_ref(),
                &note,
                22,
                h - 20,
                T_CAPTION,
                w - 44,
                th.warning,
            );
        }
    }

    if let Some(err) = view.error {
        cv.round_rect(18, h - 124, w - 36, 28, R_CONTROL, th.tile);
        cv.text_truncated(
            fonts.regular.as_ref(),
            err,
            28,
            h - 105,
            T_META,
            w - 56,
            th.danger,
        );
    }

    regions
}

fn card_row(cv: &mut Canvas<'_>, th: &Theme, fonts: &Fonts, x: i32, y: i32, w: i32, msg: &str) {
    cv.round_rect(x, y, w, 34, R_ROW, th.tile);
    cv.text_truncated(
        fonts.regular.as_ref(),
        msg,
        x + 12,
        y + 21,
        T_LABEL,
        w - 24,
        th.text,
    );
}

/// A one-line text field. Shows the placeholder hint until something is typed,
/// and a caret while keys are going into it.
#[allow(clippy::too_many_arguments)]
fn reply_field(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    x: i32,
    y: i32,
    w: i32,
    value: &str,
    active: bool,
    hint: &str,
) {
    cv.round_rect(x, y, w, 34, R_ROW, th.field);
    cv.stroke_round_rect(
        x,
        y,
        w,
        34,
        R_ROW,
        1.0,
        if active {
            th.selected_border
        } else {
            th.separator
        },
    );
    if value.is_empty() {
        cv.text_truncated(
            fonts.regular.as_ref(),
            hint,
            x + 12,
            y + 21,
            T_LABEL,
            w - 24,
            th.text_dim,
        );
        if active {
            cv.fill_rect(x + 11, y + 9, 2, 16, th.accent);
        }
    } else {
        // Show the tail so the caret end is always visible.
        let max_w = w - 28;
        let mut shown = value.to_string();
        while crate::canvas::text_width(fonts.regular.as_ref(), &shown, T_LABEL) > max_w
            && shown.chars().count() > 1
        {
            shown.remove(0);
        }
        let tw = cv.text(
            fonts.regular.as_ref(),
            &shown,
            x + 12,
            y + 21,
            T_LABEL,
            th.text,
        );
        if active {
            cv.fill_rect(x + 13 + tw, y + 9, 2, 16, th.accent);
        }
    }
}
