//! Surface painters and hit-test layouts for Bar, Dock (3 variants),
//! Launcher, Control Centre, and Expanded Dynamic Island.

use std::collections::HashMap;

use crate::agent::{Approval, Scope, Status, Summary as AgentSummary, TaskView, Verdict};
use crate::canvas::{text_width, wrap_text, Canvas, Fonts, Icon, Rgba};
use crate::launcher::{self, Action as LauncherAction, Item as LauncherItem};
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
/// pearOS / macOS menu bar height at 1x.
pub const BAR_HEIGHT: u32 = 30;
pub const NOTCH_WIDTH: i32 = 184;

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
    /// Close the window with this niri id (dock context menu Quit).
    CloseWindow(u64),
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
    // Optical baseline for the 30 px menu bar.
    let text_y = (h as f32 * 0.69).round() as i32;
    let icon_cy = h as f32 * 0.5;

    let bar_mat = prefs.look.material(prefs.look.bar_material);
    let slab = crate::material::slab(bar_mat, th.bar, prefs.wall_tint);
    let is_glass_bar = matches!(
        bar_mat,
        crate::material::Material::Glass | crate::material::Material::Clear
    );
    let dark_theme = prefs.theme == ThemeKind::Dark;
    // Over a bright sky the bar turns dark-on-light, like the automatic macOS
    // menu bar, so it needs no scrim at all and the sky stays untouched.
    let flip = crate::material::ink_flips(bar_mat, th.bar, prefs.wall_tint);
    if bar_mat == crate::material::Material::Glass {
        if !flip {
            if dark_theme {
                // pearOS / Cupertino dark glass menu bar: smoked translucent band + 1px bottom hairline.
                cv.fill_rect_vgrad(
                    0,
                    0,
                    w,
                    h,
                    Rgba::rgba(16, 16, 20, 96),
                    Rgba::rgba(16, 16, 20, 52),
                );
                cv.fill_rect(0, h - 1, w, 1, Rgba::rgba(255, 255, 255, 22));
            } else {
                let top = Rgba::rgba(0, 0, 0, 34);
                cv.fill_rect_vgrad(0, 0, w, h, top, Rgba::rgba(0, 0, 0, 0));
            }
        }
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

    let (bar_text, bar_menu_text, bar_shadow, hover_fill) = if flip && dark_theme {
        (
            Rgba::rgba(0x0c, 0x0c, 0x0e, 238),
            Rgba::rgba(0x0c, 0x0c, 0x0e, 214),
            None,
            Rgba::rgba(0, 0, 0, 16),
        )
    } else if flip {
        (
            Rgba::rgba(255, 255, 255, 246),
            Rgba::rgba(255, 255, 255, 224),
            Some(Rgba::rgba(0, 0, 0, 40)),
            Rgba::rgba(255, 255, 255, 30),
        )
    } else if is_glass_bar && dark_theme {
        (
            Rgba::rgba(255, 255, 255, 244),
            Rgba::rgba(255, 255, 255, 224),
            Some(Rgba::rgba(0, 0, 0, 34)),
            th.tile_hover,
        )
    } else {
        (th.text, th.text_med, None, th.tile_hover)
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
        cv.round_rect(mark_x - 4, 2, mark_w + 8, h - 4, 5.0, hover_fill);
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
        // SF-weight hierarchy: focused app name in Semibold, menus in Regular.
        let (app_name, _) = focused_app_menus(wins);
        let app_size = 14.0;
        let aw = text_width(fonts.semibold.as_ref(), &app_name, app_size);
        if hovered(hover, x - 6, 0, aw + 12, h) {
            cv.round_rect(x - 6, 2, aw + 12, h - 4, 5.0, hover_fill);
        }
        draw_bar_text(
            cv,
            fonts.semibold.as_ref(),
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
        x += aw + 22;
        let menu_size = 14.0;
        for id in MenuId::TITLES {
            let t = id.title();
            let mw = text_width(fonts.regular.as_ref(), t, menu_size);
            if x + mw + 8 > max_left_x {
                break;
            }
            let hot = hovered(hover, x - 6, 0, mw + 12, h);
            if hot {
                cv.round_rect(x - 6, 2, mw + 12, h - 4, 5.0, hover_fill);
            }
            draw_bar_text(
                cv,
                fonts.regular.as_ref(),
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
            x += mw + 22;
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
                fonts.semibold.as_ref()
            } else {
                fonts.regular.as_ref()
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
    let clock_size = 14.0;
    let date_size = 14.0;
    let clock_w = text_width(fonts.regular.as_ref(), &clock_str, clock_size);
    let date_w = if date_str.is_empty() {
        0
    } else {
        text_width(fonts.regular.as_ref(), &date_str, date_size)
    };
    let date_gap = if date_w > 0 { 7 } else { 0 };

    let right_margin = 14;
    let clock_x = w - right_margin - clock_w;
    draw_bar_text(
        cv,
        fonts.regular.as_ref(),
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
            fonts.regular.as_ref(),
            &date_str,
            date_x,
            text_y,
            date_size,
            bar_text,
        );
    }
    let time_left = if date_w > 0 { date_x } else { clock_x };
    regions.push(Region {
        x: time_left - 4,
        y: 0,
        w: clock_x + clock_w + 8 - (time_left - 4),
        h,
        // macOS: clicking the clock/date opens Notification Centre.
        action: HitAction::Shell(ShellCmd::ToggleNotifs),
    });

    // Search, Wi-Fi, volume, battery, control centre — crisp unboxed glyphs.
    // pearOS / macOS spacing: icons ~28 px apart.
    let gap = 12;
    let mut edge = time_left - 18;
    edge -= 18;
    let cc_cx = edge as f32 + 9.0;
    // Focus mode: the bar carries the moon like the real menu bar does.
    let focus_cx = if prefs.dnd_on {
        edge -= gap + 18;
        Some(edge as f32 + 9.0)
    } else {
        None
    };
    let bat = if let Some(pct) = telem.battery_pct {
        edge -= gap + 22;
        Some((edge as f32 + 11.0, pct))
    } else {
        None
    };
    edge -= gap + 18;
    let vol_cx = edge as f32 + 9.0;
    edge -= gap + 18;
    let net_cx = edge as f32 + 9.0;
    let status_left = edge;
    let status_right = time_left - 10;
    let status_w = (status_right - status_left).max(0);
    let status_hov = hovered(hover, status_left, 0, status_w, h);
    if status_hov {
        cv.round_rect(status_left, 2, status_w, h - 4, 6.0, hover_fill);
    }
    // Subtle optical shadow behind right-side icons on clear dark glass.
    if let Some(sh) = bar_shadow {
        cv.icon_network(net_cx, icon_cy + 1.0, 13.0, telem.network, sh, sh);
        cv.icon_volume(vol_cx, icon_cy + 1.0, 13.0, prefs.volume, sh);
        if let Some((cx, pct)) = bat {
            cv.icon_battery(cx, icon_cy + 1.0, pct, sh, sh);
        }
        if let Some(cx) = focus_cx {
            cv.icon_moon(cx, icon_cy + 1.0, sh);
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
        let level = if pct <= 15 { th.danger } else { bar_text };
        cv.icon_battery(cx, icon_cy, pct, bar_text, level);
    }
    if let Some(cx) = focus_cx {
        cv.icon_moon(cx, icon_cy, bar_text);
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
    let search_x = status_left - gap - search_w;
    if search_x > (w / 2) + center_reserved_half {
        if hovered(hover, search_x, 0, search_w, h) {
            cv.round_rect(search_x, 2, search_w, h - 4, 6.0, hover_fill);
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

        // pearOS tray overflow chevron (v) left of Search/status icons.
        let chev_w = 20;
        let chev_x = search_x - 8 - chev_w;
        if chev_x > (w / 2) + center_reserved_half {
            if hovered(hover, chev_x, 0, chev_w, h) {
                cv.round_rect(chev_x, 2, chev_w, h - 4, 6.0, hover_fill);
            }
            let ccx = chev_x as f32 + chev_w as f32 / 2.0;
            let draw_chev = |cv: &mut Canvas<'_>, cy: f32, col: Rgba| {
                cv.line(ccx - 3.6, cy - 1.6, ccx, cy + 2.0, 1.35, col);
                cv.line(ccx, cy + 2.0, ccx + 3.6, cy - 1.6, 1.35, col);
            };
            if let Some(sh) = bar_shadow {
                draw_chev(cv, icon_cy + 1.0, sh);
            }
            draw_chev(cv, icon_cy, bar_menu_text);
            regions.push(Region {
                x: chev_x,
                y: 0,
                w: chev_w,
                h,
                action: HitAction::Shell(ShellCmd::ToggleControl),
            });
        }
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

/// Spotlight: a floating search capsule and, once something is typed, a card of
/// matches under it. Both shapes live on one transparent surface that is sized
/// to fit them; the compositor blurs only the two shapes (`SpotLayout::blur`),
/// and the room around them holds the shadow.
pub const SPOT_W: i32 = 580;
const SPOT_PAD_X: i32 = 40;
const SPOT_PAD_TOP: i32 = 20;
const SPOT_PAD_BOTTOM: i32 = 52;
const CAPSULE_H: i32 = 56;
const CARD_GAP: i32 = 10;
const CARD_PAD: i32 = 8;
const CARD_R: f32 = 24.0;
const MAX_LIST_H: i32 = 304;
const NO_MATCH_H: i32 = 64;
const AGENT_NOTE_H: i32 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where everything in the Spotlight surface sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpotLayout {
    pub w: u32,
    pub h: u32,
    pub capsule: Rect,
    pub card: Option<Rect>,
    /// Height of the visible result list inside the card.
    pub list_h: i32,
}

impl SpotLayout {
    /// The shapes that take input and want blur behind them.
    pub fn shapes(&self) -> Vec<(Rect, f32)> {
        let mut v = vec![(self.capsule, CAPSULE_H as f32 / 2.0)];
        v.extend(self.card.map(|c| (c, CARD_R)));
        v
    }
}

fn only_agent_row(items: &[LauncherItem]) -> bool {
    !launcher::has_direct_match(items) && !items.is_empty()
}

/// Size the Spotlight surface for what it has to show. An empty query is just
/// the capsule: nothing is listed until there is something to match.
pub fn spot_layout(query: &str, items: &[LauncherItem]) -> SpotLayout {
    let capsule = Rect {
        x: SPOT_PAD_X,
        y: SPOT_PAD_TOP,
        w: SPOT_W,
        h: CAPSULE_H,
    };
    let list_h = if query.trim().is_empty() {
        0
    } else if items.is_empty() {
        NO_MATCH_H - 2 * CARD_PAD
    } else {
        let rows: i32 = launcher::spotlight_rows(items, 0, MAX_LIST_H)
            .iter()
            .map(|r| launcher::ROW_H + r.heading.map_or(0, |_| launcher::HEADING_H))
            .sum();
        rows + if only_agent_row(items) { AGENT_NOTE_H } else { 0 }
    };
    let card = (list_h > 0).then(|| Rect {
        x: SPOT_PAD_X,
        y: capsule.y + CAPSULE_H + CARD_GAP,
        w: SPOT_W,
        h: list_h + 2 * CARD_PAD,
    });
    let bottom = card.map_or(capsule.y + CAPSULE_H, |c| c.y + c.h);
    SpotLayout {
        w: (SPOT_W + 2 * SPOT_PAD_X) as u32,
        h: (bottom + SPOT_PAD_BOTTOM) as u32,
        capsule,
        card,
        list_h,
    }
}

/// The glass the capsule and card are made of, and its edge.
fn spot_glass(th: &Theme) -> (Rgba, Rgba) {
    if th.surface.0 < 128 {
        (Rgba::rgba(28, 29, 33, 158), Rgba::rgba(255, 255, 255, 44))
    } else {
        (Rgba::rgba(250, 250, 252, 190), Rgba::rgba(0, 0, 0, 22))
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
    let lay = spot_layout(query, items);
    let mut regions = Vec::new();
    let (glass, edge) = spot_glass(th);

    for (r, radius) in lay.shapes() {
        cv.round_rect_shadow(r.x, r.y, r.w, r.h, radius, 10.0, 28.0, 42);
    }
    for (r, radius) in lay.shapes() {
        cv.round_rect(r.x, r.y, r.w, r.h, radius, glass);
        cv.stroke_round_rect(r.x, r.y, r.w, r.h, radius, 1.0, edge);
    }

    let cap = lay.capsule;
    let cy = cap.y as f32 + cap.h as f32 / 2.0;
    cv.icon_search(cap.x as f32 + 29.0, cy, 18.0, th.text_med);
    let text_x = cap.x + 50;
    let base = cap.y + cap.h / 2 + 7;
    let esc_w = text_width(fonts.regular.as_ref(), "esc", 12.5);
    let esc_x = cap.x + cap.w - 24 - esc_w;
    if query.is_empty() {
        cv.round_rect(text_x, cap.y + 17, 2, 22, 1.0, th.accent);
        cv.text(
            fonts.regular.as_ref(),
            "Search or Ask",
            text_x + 7,
            base,
            19.0,
            th.text_dim,
        );
    } else {
        let qw = cv.text_truncated(
            fonts.regular.as_ref(),
            query,
            text_x,
            base,
            19.0,
            esc_x - text_x - 20,
            th.text,
        );
        cv.round_rect(text_x + qw + 2, cap.y + 17, 2, 22, 1.0, th.accent);
    }
    cv.text(
        fonts.regular.as_ref(),
        "esc",
        esc_x,
        base - 2,
        12.5,
        Rgba::rgba(th.text_dim.0, th.text_dim.1, th.text_dim.2, 150),
    );
    regions.push(Region {
        x: esc_x - 8,
        y: cap.y,
        w: esc_w + 32,
        h: cap.h,
        action: HitAction::Shell(ShellCmd::CloseOverlays),
    });

    let Some(card) = lay.card else {
        return regions;
    };
    let (lx, lw) = (card.x, card.w);
    let top = card.y + CARD_PAD;
    if items.is_empty() {
        let msg = format!("No results for “{}”", query.trim());
        cv.text_truncated(
            fonts.regular.as_ref(),
            &msg,
            lx + 24,
            card.y + card.h / 2 + 5,
            14.5,
            lw - 48,
            th.text_med,
        );
        return regions;
    }

    let sel = selected.min(items.len() - 1);
    let rows_h = lay.list_h - if only_agent_row(items) { AGENT_NOTE_H } else { 0 };
    let first = launcher::spotlight_first(items, sel, rows_h);
    let mut y = top;
    for row in launcher::spotlight_rows(items, first, rows_h) {
        if let Some(label) = row.heading {
            cv.text(
                fonts.semibold.as_ref(),
                label,
                lx + 20,
                y + 15,
                11.5,
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
            (lx, lw),
            y,
            &items[row.index],
            row.index == sel,
            hover,
        );
        y += launcher::ROW_H;
    }
    if only_agent_row(items) {
        cv.text_truncated(
            fonts.regular.as_ref(),
            "No app, window or command matches. Enter only queues a request.",
            lx + 20,
            y + 15,
            11.5,
            lw - 40,
            th.text_dim,
        );
    }
    regions
}

#[allow(clippy::too_many_arguments)]
fn draw_launcher_row(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    icons: &mut HashMap<String, Option<Icon>>,
    regions: &mut Vec<Region>,
    (lx, lw): (i32, i32),
    y: i32,
    item: &LauncherItem,
    selected: bool,
    hover: Option<(f64, f64)>,
) {
    let (rx, rw, rh) = (lx + CARD_PAD, lw - 2 * CARD_PAD, launcher::ROW_H - 4);
    let runnable = item.is_runnable();
    if selected {
        cv.round_rect(rx, y, rw, rh, 12.0, th.accent);
    } else if runnable && hovered(hover, rx, y, rw, rh) {
        cv.round_rect(rx, y, rw, rh, 12.0, th.tile_hover);
    }

    let icon = 28;
    let sys_ic = load_app_icon(icons, &item.app_id);
    cv.draw_app_squircle(rx + 10, y + (rh - icon) / 2, icon, &item.role, sys_ic, fonts);

    let title_col = if selected {
        th.on_accent
    } else if runnable {
        th.text
    } else {
        th.text_dim
    };
    let sub_col = if selected {
        Rgba::rgba(th.on_accent.0, th.on_accent.1, th.on_accent.2, 190)
    } else {
        th.text_dim
    };
    let text_x = rx + 10 + icon + 12;
    let trail = if selected { 30 } else { 12 };
    let text_w = rw - (text_x - rx) - trail;
    let base = y + rh / 2 + 5;
    let used = cv.text_truncated(
        fonts.regular.as_ref(),
        &item.title,
        text_x,
        base,
        14.5,
        text_w,
        title_col,
    );
    let sub_x = text_x + used + 10;
    if !item.subtitle.is_empty() && text_w - used - 10 > 40 {
        cv.text_truncated(
            fonts.regular.as_ref(),
            &item.subtitle,
            sub_x,
            base,
            12.5,
            text_w - used - 10,
            sub_col,
        );
    }
    if selected && runnable {
        cv.text(
            fonts.medium.as_ref(),
            "↵",
            rx + rw - 26,
            base,
            14.0,
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

#[allow(clippy::too_many_arguments)]
/// The notification centre drawer: full-height card at the right edge,
/// history of notifications newest-first, Clear All at the top. With media
/// playing a Now Playing card sits at the top, as in the real thing.
pub fn draw_notifs(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    notifs: &std::collections::VecDeque<(u64, String, String)>,
    media: Option<&crate::media::Media>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let mut regions = Vec::new();
    card(cv, th, 0, 0, w, h, R_CARD);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    cv.text(
        fonts.semibold.as_ref(),
        "Notification Centre",
        16,
        26,
        14.0,
        th.text,
    );
    cv.text(
        fonts.regular.as_ref(),
        &chrono::Local::now().format("%A %-d %B").to_string(),
        16,
        44,
        11.0,
        th.text_dim,
    );
    if !notifs.is_empty() {
        let label = "Clear All";
        let lw = text_width(fonts.regular.as_ref(), label, 11.0);
        let bx = w - 16 - lw - 16;
        let hot = hover.is_some_and(|(hx, hy)| {
            hx >= bx as f64 && hx <= (bx + lw + 16) as f64 && hy >= 12.0 && hy <= 40.0
        });
        cv.round_rect(
            bx,
            14,
            lw + 16,
            24,
            12.0,
            if hot { th.tile_hover } else { th.tile },
        );
        cv.text(
            fonts.regular.as_ref(),
            label,
            bx + 8,
            30,
            11.0,
            th.accent,
        );
        regions.push(Region {
            x: bx,
            y: 14,
            w: lw + 16,
            h: 24,
            action: HitAction::Shell(ShellCmd::ClearNotifs),
        });
    }

    // Now Playing widget (the real NC's top card): album art, title,
    // artist and transport controls.
    let mut y = 58;
    if let Some(m) = media {
        let ch = 72;
        cv.round_rect(12, y, w - 24, ch, 10.0, th.tile);
        cv.text(
            fonts.semibold.as_ref(),
            "NOW PLAYING",
            22,
            y + 14,
            8.5,
            th.text_dim,
        );
        let art = 40;
        cv.draw_album_art(22, y + 24, art, 9.0);
        let tx = 22 + art + 10;
        let tw = w - 24 - tx - 112;
        cv.text_truncated(
            fonts.semibold.as_ref(),
            &m.title,
            tx,
            y + 38,
            11.5,
            tw.max(20),
            th.text,
        );
        cv.text_truncated(
            fonts.regular.as_ref(),
            &m.artist,
            tx,
            y + 56,
            10.5,
            tw.max(20),
            th.text_dim,
        );
        let btn = 32;
        let gap = 4;
        let start = w - 24 - (btn * 3 + gap * 2);
        for (i, c) in [
            crate::media::Control::Previous,
            crate::media::Control::PlayPause,
            crate::media::Control::Next,
        ]
        .into_iter()
        .enumerate()
        {
            let bx0 = start + i as i32 * (btn + gap);
            if hovered(hover, bx0, y + 32, btn, 32) {
                cv.round_rect(bx0, y + 32, btn, 32, 16.0, th.tile_hover);
            }
            let cx = (bx0 + btn / 2) as f32;
            let cy = (y + 48) as f32;
            match c {
                crate::media::Control::Previous => {
                    cv.icon_skip(cx, cy, 13.0, false, th.text)
                }
                crate::media::Control::Next => cv.icon_skip(cx, cy, 13.0, true, th.text),
                crate::media::Control::PlayPause if m.playing => {
                    cv.icon_pause(cx, cy, 15.0, th.text)
                }
                crate::media::Control::PlayPause => cv.icon_play(cx, cy, 15.0, th.text),
                crate::media::Control::SeekTo(_) => {}
            }
            regions.push(Region {
                x: bx0,
                y: y + 32,
                w: btn,
                h: 32,
                action: HitAction::Media(c),
            });
        }
        y += ch + 8;
    }

    if notifs.is_empty() {
        cv.text(
            fonts.regular.as_ref(),
            "No Notifications",
            w / 2 - 52,
            if media.is_some() { y + 60 } else { h / 2 },
            13.0,
            th.text_dim,
        );
        return regions;
    }
    for (ts, app, text) in notifs.iter().rev() {
        let card_h = 48;
        if y + card_h > h - 12 {
            break;
        }
        cv.round_rect(12, y, w - 24, card_h, 10.0, th.tile);
        let age = now.saturating_sub(*ts);
        let ago = if age < 60 {
            "just now".to_string()
        } else if age < 3600 {
            format!("{}m ago", age / 60)
        } else if age < 86400 {
            format!("{}h ago", age / 3600)
        } else {
            format!("{}d ago", age / 86400)
        };
        // App icon and name, like the real centre's per-app cards.
        cv.draw_app_squircle(20, y + 7, 34, app, None, fonts);
        let tx = 62;
        if !app.is_empty() {
            cv.text_tracked(
                fonts.semibold.as_ref(),
                &app.to_uppercase(),
                tx,
                y + 14,
                8.5,
                0.5,
                th.text_dim,
            );
        }
        let aw = text_width(fonts.regular.as_ref(), &ago, 9.5);
        cv.text(fonts.regular.as_ref(), &ago, w - 20 - aw, y + 14, 9.5, th.text_dim);
        cv.text_truncated(
            fonts.regular.as_ref(),
            text,
            tx,
            y + 33,
            11.5,
            w - 24 - tx - 8,
            th.text,
        );
        y += card_h + 6;
    }
    regions
}

// ------------------------------------------------------- About this computer

/// The "About This Computer" card: a centred OS mark, the name, and
/// two-column spec lines (label dim left, value right).
pub fn draw_about(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    specs: &[(String, String)],
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    cv.round_rect_shadow(0, 0, w, h, 14.0, 10.0, 30.0, 48);
    cv.round_rect(0, 0, w, h, 14.0, th.surface);
    cv.stroke_round_rect(0, 0, w, h, 14.0, 1.0, th.hairline);
    let cx0 = w / 2;
    cv.draw_app_squircle(cx0 - 21, 14, 42, "solyra", None, fonts);
    let title = "Solyra OS";
    let tw = text_width(fonts.semibold.as_ref(), title, 15.0);
    cv.text(fonts.semibold.as_ref(), title, cx0 - tw / 2, 74, 15.0, th.text);
    let mut y = 98;
    for (label, value) in specs {
        cv.text(fonts.regular.as_ref(), label, 44, y, 11.0, th.text_dim);
        cv.text(fonts.regular.as_ref(), value, 132, y, 11.0, th.text);
        y += 22;
    }
    Vec::new()
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

    use super::{draw_launcher, spot_layout, HitAction, SpotLayout};
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

    fn paint(query: &str, items: &[launcher::Item]) -> (SpotLayout, Vec<super::Region>) {
        let fonts = Fonts::load();
        let th = Theme::for_kind(ThemeKind::Dark);
        let lay = spot_layout(query, items);
        let mut buf = vec![0u8; (lay.w * lay.h * 4) as usize];
        let mut cv = Canvas::new(&mut buf, lay.w, lay.h, 1);
        let mut icons = HashMap::new();
        let regions = draw_launcher(&mut cv, &th, &fonts, query, items, 0, &mut icons, None);
        (lay, regions)
    }

    #[test]
    fn empty_query_is_a_lone_capsule_that_can_be_dismissed() {
        let cat = vec![app("Files", "thunar"), app("Terminal", "foot")];
        let items = launcher::search("", &cat, &[], false);
        let (lay, regions) = paint("", &items);
        assert!(lay.card.is_none(), "nothing is listed before there is a query");
        assert!(regions.iter().all(|r| !matches!(r.action, HitAction::Launch(_))));
        assert!(regions
            .iter()
            .any(|r| matches!(r.action, HitAction::Shell(ShellCmd::CloseOverlays))));
        let (small, _) = paint("zzzz", &[]);
        assert!(small.h > lay.h, "a query opens the results card");
    }

    #[test]
    fn a_query_lists_matches_in_a_card_sized_to_fit() {
        let cat = vec![app("Files", "thunar"), app("Terminal", "foot"), app("Text Editor", "mousepad")];
        let one = launcher::search("files", &cat, &[], false);
        let two = launcher::search("t", &cat, &[], false);
        let (a, regions) = paint("files", &one);
        let (b, _) = paint("t", &two);
        let card = a.card.expect("results card");
        assert!(b.h > a.h, "more rows, taller card");
        let rows: Vec<_> = regions
            .iter()
            .filter(|r| matches!(r.action, HitAction::Launch(_)))
            .collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| (30..=42).contains(&r.h)));
        assert!(
            rows.iter().all(|r| r.y >= card.y && r.y + r.h <= card.y + card.h),
            "every row sits inside the card"
        );
        assert!(a.capsule.y + a.capsule.h < card.y, "the card floats below the capsule");
        assert!((a.h as i32) >= card.y + card.h, "the surface holds the card and its shadow");
    }
}
