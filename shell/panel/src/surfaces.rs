//! Surface painters and hit-test layouts for Bar, Dock (3 variants),
//! Launcher, Control Centre, and Expanded Dynamic Island.

use std::collections::HashMap;

use crate::agent::{Approval, Scope, Status, Summary as AgentSummary, TaskView, Verdict};
use crate::canvas::{lerp_rgba, text_width, wrap_text, Canvas, Fonts, Icon, Rgba};
use crate::launcher::{self, Action as LauncherAction, Item as LauncherItem, Kind as LauncherKind};
use crate::menu::MenuId;
use crate::state::{
    focused_app_menus, load_app_icon, DockStyle, LaunchTarget, ShellCmd, ShellPrefs, Telemetry,
    WallpaperPreset, WindowInfo,
};
use crate::tokens::{
    Theme, ThemeKind, R_CARD, R_CONTROL, R_OVERLAY, R_ROW, R_TILE, T_BODY, T_CAPTION, T_LABEL,
    T_META, T_TITLE,
};

/// macOS menu-bar band: 24–28 px. Exclusive zone tracks this height.
pub const BAR_HEIGHT: u32 = 26;
pub const NOTCH_WIDTH: i32 = 184;

pub const LAUNCHER_W: u32 = 640;
/// Compact macOS Spotlight vibrancy card — fits Top Hit + Apps + Commands without dead space.
pub const LAUNCHER_H: u32 = 362;

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
    /// Open a menu-bar menu.
    OpenMenu(MenuId),
    /// First click on an item that needs confirming (shut down, restart).
    ArmMenuItem(usize),
    /// Play/pause, next or previous on the playing media player.
    Media(crate::media::Control),
    /// Agent desk: 0 Cloud, 1 Local, 2 Worktree. Persisted as `agent_env`.
    SetAgentEnv(u8),
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
    // Optical baseline for 26px Apple menu bar.
    let text_y = (h as f32 * 0.69).round() as i32;
    let icon_cy = h as f32 * 0.5;

    let bar_mat = prefs.look.material(prefs.look.bar_material);
    let slab = crate::material::slab(bar_mat, th.bar, prefs.wall_tint);
    let is_glass_bar = matches!(
        bar_mat,
        crate::material::Material::Glass | crate::material::Material::Clear
    );
    let dark_theme = prefs.theme == ThemeKind::Dark;
    if bar_mat == crate::material::Material::Glass {
        // Near-invisible Apple Liquid Glass bar: delicate top atmospheric feather
        // so bright wallpaper sky shines uninterrupted while white text stays crisp.
        let top_a = if dark_theme { 44 } else { 36 };
        let top = Rgba::rgba(th.bar.0, th.bar.1, th.bar.2, top_a);
        let bot = Rgba::rgba(th.bar.0, th.bar.1, th.bar.2, 0);
        cv.fill_rect_vgrad(0, 0, w, h, top, bot);
    } else {
        crate::material::paint(cv, &slab, 0, 0, w, h, 0.0);
        if matches!(
            bar_mat,
            crate::material::Material::Solid | crate::material::Material::Acrylic
        ) {
            let seam_a = (slab.edge.3 / 3).max(8);
            cv.fill_rect(
                0,
                h - 1,
                w,
                1,
                Rgba::rgba(slab.edge.0, slab.edge.1, slab.edge.2, seam_a),
            );
        }
    }

    // Crisp Apple menu-bar typography over open sky: pure luminous text with
    // whisper-light shadow on translucent dark glass, zero double-vision halo.
    let bar_shadow = if is_glass_bar && dark_theme {
        Some(Rgba::rgba(0, 0, 0, 48))
    } else {
        None
    };
    let bar_text = if is_glass_bar && dark_theme {
        Rgba::rgba(255, 255, 255, 248)
    } else {
        th.text
    };
    let bar_menu_text = if is_glass_bar && dark_theme {
        Rgba::rgba(255, 255, 255, 224)
    } else {
        th.text_med
    };
    let draw_bar_text = |cv: &mut Canvas<'_>,
                         font: Option<&ab_glyph::FontRef<'_>>,
                         s: &str,
                         tx: i32,
                         ty: i32,
                         size: f32,
                         col: Rgba| {
        if let Some(sh) = bar_shadow {
            cv.text_shadow(font, s, tx, ty, size, 0.0, col, sh, 1);
        } else {
            cv.text(font, s, tx, ty, size, col);
        }
    };

    use solyra_conf::look::BarStyle;
    let bar_style = prefs.look.bar;
    let menubar = bar_style == BarStyle::MenuBar;
    let notch_on = prefs.look.notch != solyra_conf::look::NotchStyle::Off;
    let center_reserved_half = if island_msg.is_some() || notch_on {
        178
    } else {
        112
    };
    let max_left_x = (w / 2) - center_reserved_half;

    // Left: crisp nL brand mark. Menu bar → system menu; compact → launcher.
    let mark_h = 16;
    let mark_x = 12;
    let mark_y = (h - mark_h) / 2;
    let mark_w = crate::canvas::nl_mark_width(mark_h, fonts);
    let brand_hover = hover.is_some_and(|(hx, hy)| {
        hx >= (mark_x - 3) as f64 && hx < (mark_x + mark_w + 3) as f64 && hy < h as f64
    });
    if brand_hover {
        cv.round_rect(mark_x - 4, 2, mark_w + 8, h - 4, 5.0, th.tile_hover);
    }
    let drawn = cv.mark_nl(mark_x, mark_y, mark_h, fonts);
    debug_assert_eq!(drawn, mark_w);
    regions.push(Region {
        x: mark_x - 2,
        y: 0,
        w: mark_w + 4,
        h,
        action: if menubar {
            HitAction::OpenMenu(MenuId::Solyra)
        } else {
            HitAction::Shell(ShellCmd::ToggleLauncher)
        },
    });
    let mut x = mark_x + mark_w + 14;

    if menubar {
        // SF-weight hierarchy: focused app name in Bold, menus in Medium.
        let (app_name, _) = focused_app_menus(wins);
        let app_size = 13.0;
        let aw = text_width(fonts.bold.as_ref(), &app_name, app_size);
        if hovered(hover, x - 6, 0, aw + 12, h) {
            cv.round_rect(x - 6, 2, aw + 12, h - 4, 5.0, th.tile_hover);
        }
        draw_bar_text(
            cv,
            fonts.bold.as_ref(),
            &app_name,
            x,
            text_y,
            app_size,
            bar_text,
        );
        regions.push(Region {
            x: x - 6,
            y: 0,
            w: aw + 12,
            h,
            action: HitAction::OpenMenu(MenuId::App),
        });
        x += aw + 18;
        let menu_size = 12.6;
        for id in MenuId::TITLES {
            let t = id.title();
            let mw = text_width(fonts.medium.as_ref(), t, menu_size);
            if x + mw + 8 > max_left_x {
                break;
            }
            let hot = hovered(hover, x - 6, 0, mw + 12, h);
            if hot {
                cv.round_rect(x - 6, 2, mw + 12, h - 4, 5.0, th.tile_hover);
            }
            draw_bar_text(
                cv,
                fonts.medium.as_ref(),
                t,
                x,
                text_y,
                menu_size,
                if hot { bar_text } else { bar_menu_text },
            );
            regions.push(Region {
                x: x - 6,
                y: 0,
                w: mw + 12,
                h,
                action: HitAction::OpenMenu(id),
            });
            x += mw + 16;
        }
        x += 6;
    }

    // Workspace digits (optional in the menu bar, always in the compact bar).
    // A dot marks the active space — no filled pills.
    if prefs.look.show_workspaces || !menubar {
        let slot = 16;
        for &(idx, active) in ws_list {
            if x + slot > max_left_x {
                break;
            }
            let is_hov = hovered(hover, x, 0, slot, h);
            let label = idx.to_string();
            let size = 11.5;
            let font = if active {
                fonts.bold.as_ref()
            } else {
                fonts.medium.as_ref()
            };
            let tw = text_width(font, &label, size);
            let fg = if active || is_hov {
                bar_text
            } else {
                bar_menu_text
            };
            draw_bar_text(cv, font, &label, x + (slot - tw) / 2, text_y, size, fg);
            if active {
                cv.circle(x as f32 + slot as f32 / 2.0, h as f32 - 3.5, 1.4, th.accent);
            }
            regions.push(Region {
                x,
                y: 0,
                w: slot,
                h,
                action: HitAction::Workspace(idx),
            });
            x += slot + 2;
        }
    }

    // Centre: with the notch off, a pure-black Live Activity pill in the bar itself.
    if notch_on {
        // Drawn by the island surface (island.rs).
    } else if let Some(message) = island_msg {
        let tw = text_width(fonts.semibold.as_ref(), message, 12.0);
        let pill_w = (tw + 56).clamp(NOTCH_WIDTH + 24, 340);
        let pill_h = (h - 6).max(18);
        let pill_y = (h - pill_h) / 2;
        let left = (w - pill_w) / 2;
        let pr = pill_h as f32 / 2.0;
        cv.round_rect(left, pill_y, pill_w, pill_h, pr, Rgba::rgb(0, 0, 0));
        let pd = Theme::dark();
        let dot = if is_attention {
            pd.agent_approval
        } else {
            pd.agent_active
        };
        cv.stroke_round_rect(
            left,
            pill_y,
            pill_w,
            pill_h,
            pr,
            1.0,
            Rgba::rgba(255, 255, 255, 34),
        );
        cv.circle(left as f32 + 14.0, icon_cy, 3.2, dot);
        cv.text_truncated(
            fonts.semibold.as_ref(),
            message,
            left + 24,
            text_y,
            12.0,
            pill_w - 40,
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

    // Right: unboxed glyphs, then date and clock in Apple SF-Medium/SemiBold.
    let now = chrono::Local::now();
    let clock_str = now.format(prefs.look.clock_format()).to_string();
    let date_str = if prefs.look.show_date {
        now.format("%a %b %-d").to_string()
    } else {
        String::new()
    };
    let clock_size = 12.6;
    let date_size = 12.4;
    let clock_w = text_width(fonts.semibold.as_ref(), &clock_str, clock_size);
    let date_w = if date_str.is_empty() {
        0
    } else {
        text_width(fonts.medium.as_ref(), &date_str, date_size)
    };
    let date_gap = if date_w > 0 { 8 } else { 0 };

    let right_margin = 14;
    let clock_x = w - right_margin - clock_w;
    draw_bar_text(
        cv,
        fonts.semibold.as_ref(),
        &clock_str,
        clock_x,
        text_y,
        clock_size,
        bar_text,
    );
    let date_x = clock_x - date_gap - date_w;
    if date_w > 0 {
        draw_bar_text(
            cv,
            fonts.medium.as_ref(),
            &date_str,
            date_x,
            text_y,
            date_size,
            bar_menu_text,
        );
    }
    let time_left = if date_w > 0 { date_x } else { clock_x };
    regions.push(Region {
        x: time_left - 4,
        y: 0,
        w: clock_x + clock_w + 8 - (time_left - 4),
        h,
        action: HitAction::Shell(ShellCmd::ToggleIsland),
    });

    // Search, Wi-Fi, volume, battery, control centre — crisp unboxed glyphs.
    let mut edge = time_left - 16;
    edge -= 18;
    let cc_cx = edge as f32 + 9.0;
    let bat = if let Some(pct) = telem.battery_pct {
        edge -= 6 + 22;
        Some((edge as f32 + 11.0, pct))
    } else {
        None
    };
    edge -= 6 + 18;
    let vol_cx = edge as f32 + 9.0;
    edge -= 6 + 18;
    let net_cx = edge as f32 + 9.0;
    let status_left = edge;
    let status_right = time_left - 10;
    let status_w = (status_right - status_left).max(0);
    let status_hov = hovered(hover, status_left, 0, status_w, h);
    if status_hov {
        cv.round_rect(status_left, 2, status_w, h - 4, 6.0, th.tile_hover);
    }
    // Subtle optical shadow behind right-side icons on clear dark glass.
    if let Some(sh) = bar_shadow {
        cv.icon_network(net_cx, icon_cy + 1.0, 13.0, telem.network, sh, sh);
        cv.icon_volume(vol_cx, icon_cy + 1.0, 13.0, prefs.volume, sh);
        if let Some((cx, pct)) = bat {
            cv.icon_battery(cx, icon_cy + 1.0, pct, sh, sh);
        }
        cv.icon_control_centre(cc_cx, icon_cy + 1.0, sh);
    }
    cv.icon_network(
        net_cx,
        icon_cy,
        13.0,
        telem.network,
        bar_text,
        bar_menu_text,
    );
    cv.icon_volume(vol_cx, icon_cy, 13.0, prefs.volume, bar_text);
    if let Some((cx, pct)) = bat {
        cv.icon_battery(cx, icon_cy, pct, bar_text, th.success);
    }
    cv.icon_control_centre(cc_cx, icon_cy, bar_text);
    regions.push(Region {
        x: status_left,
        y: 0,
        w: status_w,
        h,
        action: HitAction::Shell(ShellCmd::ToggleControl),
    });

    let search_w = 22;
    let search_x = status_left - 12 - search_w;
    if search_x > (w / 2) + center_reserved_half {
        if hovered(hover, search_x, 0, search_w, h) {
            cv.round_rect(search_x, 2, search_w, h - 4, 6.0, th.tile_hover);
        }
        if let Some(sh) = bar_shadow {
            cv.icon_search(search_x as f32 + 11.0, icon_cy + 1.0, 13.0, sh);
        }
        cv.icon_search(search_x as f32 + 11.0, icon_cy, 13.0, bar_text);
        regions.push(Region {
            x: search_x,
            y: 0,
            w: search_w,
            h,
            action: HitAction::Shell(ShellCmd::ToggleLauncher),
        });
    }

    regions
}

// ------------------------------------------------------ Spotlight Launcher

/// Layered glass overlay card with subtle depth and directional specular rim.
fn card(cv: &mut Canvas<'_>, th: &Theme, x: i32, y: i32, w: i32, h: i32, r: f32) {
    let dark = th.surface.0 < 128;
    let top = if dark {
        Rgba::rgba(
            th.surface.0.saturating_add(10),
            th.surface.1.saturating_add(10),
            th.surface.2.saturating_add(12),
            240,
        )
    } else {
        Rgba::rgba(255, 255, 255, 246)
    };
    let bot = Rgba::rgba(th.surface.0, th.surface.1, th.surface.2, 244);
    cv.round_rect_vgrad(x, y, w, h, r, top, bot);
    cv.round_rect_glass_rim(
        x,
        y,
        w,
        h,
        r,
        if dark {
            Rgba::rgba(255, 255, 255, 76)
        } else {
            Rgba::rgba(255, 255, 255, 220)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 26)
        } else {
            Rgba::rgba(0, 0, 0, 18)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 16)
        } else {
            Rgba::rgba(0, 0, 0, 26)
        },
    );
}

fn hovered(hover: Option<(f64, f64)>, x: i32, y: i32, w: i32, h: i32) -> bool {
    hover.is_some_and(|(hx, hy)| {
        hx >= x as f64 && hx < (x + w) as f64 && hy >= y as f64 && hy < (y + h) as f64
    })
}

/// Spotlight card: compact Liquid Glass vibrancy shell with integrated top search.
const LAUNCHER_R: f32 = 20.0;
const SEARCH_H: i32 = 54;
/// Top of the result list and the compact footer band underneath.
const LIST_TOP: i32 = 60;
const FOOTER_H: i32 = 32;

pub fn launcher_list_height() -> i32 {
    LAUNCHER_H as i32 - LIST_TOP - FOOTER_H
}

fn row_verb(item: &LauncherItem) -> &'static str {
    match item.kind {
        LauncherKind::Application => "Open",
        LauncherKind::Window => "Switch",
        LauncherKind::Command => "Run",
        LauncherKind::Agent => "Ask",
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
    let dark = th.surface.0 < 128;

    // Vibrancy glass body: clean translucent obsidian (dark) or frosted pearl
    // (light) so wallpaper bokeh breathes softly through like macOS Spotlight.
    let (body_top, body_bot) = if dark {
        (Rgba::rgba(30, 32, 36, 198), Rgba::rgba(18, 20, 24, 212))
    } else {
        (Rgba::rgba(252, 252, 254, 214), Rgba::rgba(238, 241, 246, 226))
    };
    cv.round_rect_vgrad(0, 0, w, h, LAUNCHER_R, body_top, body_bot);
    // Subtle upper vibrancy luminosity inside the search band.
    cv.round_rect_vgrad(
        1,
        1,
        w - 2,
        SEARCH_H,
        LAUNCHER_R - 1.0,
        if dark {
            Rgba::rgba(255, 255, 255, 18)
        } else {
            Rgba::rgba(255, 255, 255, 120)
        },
        Rgba::rgba(255, 255, 255, 0),
    );
    cv.round_rect_glass_rim(
        0,
        0,
        w,
        h,
        LAUNCHER_R,
        if dark {
            Rgba::rgba(255, 255, 255, 118)
        } else {
            Rgba::rgba(255, 255, 255, 235)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 38)
        } else {
            Rgba::rgba(0, 0, 0, 18)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 22)
        } else {
            Rgba::rgba(0, 0, 0, 28)
        },
    );

    // Integrated Apple Spotlight search header — no inner box-in-a-box.
    let search_cy = SEARCH_H as f32 * 0.5;
    cv.icon_search(28.0, search_cy, 19.0, th.text_med);
    let text_x = 52;
    let esc_w = 36;
    let esc_x = w - 18 - esc_w;
    let text_base = 34;
    if query.is_empty() {
        cv.round_rect(text_x, 16, 2, 22, 1.0, th.accent);
        cv.text(
            fonts.regular.as_ref(),
            "Spotlight Search",
            text_x + 8,
            text_base,
            19.5,
            th.text_dim,
        );
    } else {
        let max_w = esc_x - text_x - 16;
        let qw = cv.text_truncated(
            fonts.regular.as_ref(),
            query,
            text_x,
            text_base,
            19.5,
            max_w,
            th.text,
        );
        cv.round_rect(text_x + qw + 3, 16, 2, 22, 1.0, th.accent);
    }

    // Quiet `esc` keycap pill at top right.
    let esc_y = (SEARCH_H - 22) / 2;
    cv.round_rect(
        esc_x,
        esc_y,
        esc_w,
        22,
        6.0,
        if dark {
            Rgba::rgba(255, 255, 255, 16)
        } else {
            Rgba::rgba(0, 0, 0, 12)
        },
    );
    cv.stroke_round_rect(
        esc_x,
        esc_y,
        esc_w,
        22,
        6.0,
        1.0,
        if dark {
            Rgba::rgba(255, 255, 255, 26)
        } else {
            Rgba::rgba(0, 0, 0, 20)
        },
    );
    let ew = text_width(fonts.medium.as_ref(), "esc", 11.0);
    cv.text(
        fonts.medium.as_ref(),
        "esc",
        esc_x + (esc_w - ew) / 2,
        esc_y + 15,
        11.0,
        th.text_dim,
    );
    regions.push(Region {
        x: esc_x,
        y: esc_y,
        w: esc_w,
        h: 22,
        action: HitAction::Shell(ShellCmd::CloseOverlays),
    });

    // Crisp hairline dividing search header from result list.
    cv.fill_rect(
        0,
        SEARCH_H,
        w,
        1,
        if dark {
            Rgba::rgba(255, 255, 255, 24)
        } else {
            Rgba::rgba(0, 0, 0, 18)
        },
    );

    let list_h = launcher_list_height();
    let only_agent_row = !launcher::has_direct_match(items) && !items.is_empty();

    if items.is_empty() {
        let (title, body) = if query.trim().is_empty() {
            (
                "Spotlight Search",
                "Applications appear here once they are installed.".to_string(),
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
            24,
            LIST_TOP + 30,
            T_TITLE,
            th.text,
        );
        for (i, line) in wrap_text(fonts.regular.as_ref(), &body, T_LABEL, w - 48, 2)
            .iter()
            .enumerate()
        {
            cv.text(
                fonts.regular.as_ref(),
                line,
                24,
                LIST_TOP + 54 + i as i32 * 18,
                T_LABEL,
                th.text_dim,
            );
        }
    } else {
        let sel = selected.min(items.len() - 1);
        let first = launcher::spotlight_first(items, sel, list_h);
        let mut y = LIST_TOP;
        for row in launcher::spotlight_rows(items, first, list_h) {
            if let Some(label) = row.heading {
                cv.text_tracked(
                    fonts.semibold.as_ref(),
                    label,
                    22,
                    y + 15,
                    11.0,
                    0.35,
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
                row.index == 0,
                hover,
            );
            y += launcher::ROW_H;
        }
        if only_agent_row {
            let note = "No app, window or command matches. Enter only queues a request.";
            cv.text_truncated(
                fonts.regular.as_ref(),
                note,
                22,
                LIST_TOP + list_h - 6,
                T_META,
                w - 44,
                th.text_dim,
            );
        }
    }

    // Minimal Apple Spotlight footer: subtle hairline + quiet key hints, zero power chrome.
    let fy = h - FOOTER_H;
    cv.fill_rect(
        16,
        fy,
        w - 32,
        1,
        if dark {
            Rgba::rgba(255, 255, 255, 18)
        } else {
            Rgba::rgba(0, 0, 0, 14)
        },
    );
    cv.text(
        fonts.medium.as_ref(),
        "↑↓ Navigate    ↵ Open    esc Dismiss",
        22,
        fy + 20,
        11.2,
        th.text_dim,
    );

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
    prominent: bool,
    hover: Option<(f64, f64)>,
) {
    let (rx, rw, rh) = (12, w - 24, launcher::ROW_H - 4);
    let runnable = item.is_runnable();
    if selected {
        // Luminous Apple Spotlight selection pill derived from theme accent.
        let sel_top = lerp_rgba(th.accent, Rgba::rgb(255, 255, 255), 0.14);
        let sel_bot = lerp_rgba(th.accent, Rgba::rgb(0, 0, 0), 0.10);
        cv.round_rect_vgrad(rx, y, rw, rh, 10.0, sel_top, sel_bot);
        cv.round_rect_glass_rim(
            rx,
            y,
            rw,
            rh,
            10.0,
            Rgba::rgba(255, 255, 255, 92),
            Rgba::rgba(255, 255, 255, 28),
            Rgba::rgba(0, 0, 0, 28),
        );
    } else if runnable && hovered(hover, rx, y, rw, rh) {
        cv.round_rect(rx, y, rw, rh, 10.0, th.tile_hover);
    }

    let sys_ic = load_app_icon(icons, &item.app_id);
    let icon = if prominent { 28 } else { 26 };
    cv.draw_app_squircle(rx + 9, y + (rh - icon) / 2, icon, &item.role, sys_ic, fonts);
    let title_col = if selected {
        th.on_accent
    } else if runnable {
        th.text
    } else {
        th.text_dim
    };
    let sub_col = if selected {
        Rgba::rgba(th.on_accent.0, th.on_accent.1, th.on_accent.2, 200)
    } else {
        th.text_dim
    };
    let verb = if selected {
        if runnable {
            format!("{} ↵", row_verb(item))
        } else {
            "Unavailable".to_string()
        }
    } else {
        String::new()
    };
    let badge_w = if verb.is_empty() {
        0
    } else {
        text_width(fonts.medium.as_ref(), &verb, 11.5) + 14
    };
    let text_x = rx + 9 + icon + 11;
    let text_w = rw - (text_x - rx) - badge_w - 14;
    let title_size = if prominent { 14.5 } else { 13.8 };
    let (title_y, sub_y) = if item.subtitle.is_empty() {
        (y + rh / 2 + 5, 0)
    } else {
        (y + 16, y + 30)
    };
    let title_font = if selected || prominent {
        fonts.semibold.as_ref()
    } else {
        fonts.medium.as_ref()
    };
    cv.text_truncated(title_font, &item.title, text_x, title_y, title_size, text_w, title_col);
    if !item.subtitle.is_empty() {
        cv.text_truncated(
            fonts.regular.as_ref(),
            &item.subtitle,
            text_x,
            sub_y,
            11.5,
            text_w,
            sub_col,
        );
    }

    if selected {
        let bx = rx + rw - badge_w - 6;
        let by = y + (rh - 20) / 2;
        cv.round_rect(
            bx,
            by,
            badge_w,
            20,
            6.0,
            Rgba::rgba(th.on_accent.0, th.on_accent.1, th.on_accent.2, 34),
        );
        let vw = text_width(fonts.medium.as_ref(), &verb, 11.5);
        cv.text(
            fonts.medium.as_ref(),
            &verb,
            bx + (badge_w - vw) / 2,
            by + 14,
            11.5,
            th.on_accent,
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
            crate::state::on_path("nmtui")
                .then(|| HitAction::Launch(LaunchTarget::Shell(format!("{} -e nmtui", "foot")))),
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
    let item_w = (seg_w - 8) / 4;
    for (i, style) in [
        DockStyle::Solyra,
        DockStyle::Macos,
        DockStyle::Windows,
        DockStyle::Minimal,
    ]
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

    cv.draw_app_squircle(18, 18, 36, "agent", None, fonts);
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
    /// Agent desktop mode: show the project / environment desk card.
    pub desk: bool,
    pub clock: &'a str,
    pub date: &'a str,
    /// Project lines for the desk card. Empty falls back to the placeholder.
    pub project_lines: &'a [&'a str],
    /// 0 Cloud, 1 Local, 2 Worktree.
    pub environment: u8,
    /// `None` when no agent runtime is connected.
    pub agents_running: Option<usize>,
}

fn status_glyph(cv: &mut Canvas<'_>, th: &Theme, status: Status, cx: f32, cy: f32) {
    // Shape and label carry the state; colour only reinforces it.
    match status {
        Status::Running => {
            cv.circle(cx, cy, 5.0, th.agent_active);
        }
        Status::Idle => {
            cv.ring(cx, cy, 4.5, 1.6, th.text_dim);
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
        if view.desk {
            // The empty agents overlay is the desk, not a second stub under the prose.
            let placeholder: &[&str] = &crate::desk::PROJECT_PLACEHOLDER;
            let lines = if view.project_lines.is_empty() {
                placeholder
            } else {
                view.project_lines
            };
            let dx = ((w - crate::desk::W) / 2).max(8);
            let (_, _, hits) = crate::desk::paint(
                cv,
                th,
                fonts,
                &crate::desk::Glance {
                    clock: view.clock,
                    date: view.date,
                    project_lines: lines,
                    environment: view.environment,
                    agents_running: view.agents_running,
                },
                None,
                hover,
                dx,
                78,
            );
            for t in hits {
                regions.push(Region {
                    x: t.x,
                    y: t.y,
                    w: t.w,
                    h: t.h,
                    action: HitAction::SetAgentEnv(t.environment),
                });
            }
            return regions;
        }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{draw_launcher, HitAction, LAUNCHER_H, LAUNCHER_W};
    use crate::canvas::{Canvas, Fonts};
    use crate::launcher;
    use crate::state::{AppEntry, LaunchTarget, ShellCmd};
    use crate::tokens::{Theme, ThemeKind};

    fn app(title: &str, id: &str) -> AppEntry {
        AppEntry {
            window: None,
            window_count: 0,
            is_focused: false,
            app_id: id.into(),
            role: id.into(),
            title: title.into(),
            subtitle: "Open".into(),
            command: LaunchTarget::Shell(id.into()),
        }
    }

    #[test]
    fn empty_launcher_lists_apps() {
        let fonts = Fonts::load();
        let th = Theme::for_kind(ThemeKind::Dark);
        let cat = vec![app("Files", "thunar"), app("Terminal", "foot")];
        let items = launcher::search("", &cat, &[], false);
        assert!(items.iter().any(|i| i.kind == launcher::Kind::Application));
        let mut buf = vec![0u8; (LAUNCHER_W * LAUNCHER_H * 4) as usize];
        let mut cv = Canvas::new(&mut buf, LAUNCHER_W, LAUNCHER_H, 1);
        let mut icons = HashMap::new();
        let regions = draw_launcher(&mut cv, &th, &fonts, "", &items, 0, &mut icons, None);
        let list_bottom = LAUNCHER_H as i32 - 50;
        let apps: Vec<_> = regions
            .iter()
            .filter(|r| r.y < list_bottom && matches!(r.action, HitAction::Launch(_)))
            .collect();
        assert!(apps.len() >= 2, "empty query should offer the catalogue");
        assert!(apps.iter().all(|r| (30..=42).contains(&r.h)));
        assert!(regions.iter().any(|r| {
            matches!(r.action, HitAction::Shell(ShellCmd::CloseOverlays))
        }));
    }
}
