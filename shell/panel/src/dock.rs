//! The dock, in four variants, with optional magnification and auto-hide.
//!
//! * **Solyra** – a floating card: start cell, pinned and running apps, and an
//!   agent shelf.
//! * **Floating** – a macOS-style dock: apps, the launcher after the first
//!   app, a separator and the trash; dots under running apps.
//! * **Taskbar** – a Windows-style full-width bar: start, search, centred
//!   apps with underlines, tray and clock on the right.
//! * **Minimal** – apps only, small, nothing else.
//!
//! The floating variants can magnify icons under the pointer. Their surface is
//! then taller than the card (the icons grow up into transparent space), and
//! uses a namespace without compositor blur so that space stays clear.

use std::collections::HashMap;

use crate::canvas::{text_width, Canvas, Fonts, Icon, Rgba};
use crate::material;
use crate::state::{
    dock_entries, load_app_icon, AppEntry, DockStyle, LaunchTarget, ShellCmd, WindowInfo,
};
use crate::surfaces::{HitAction, Live, Region};
use crate::tokens::Theme;

/// Height of the floating card itself (macOS Liquid Glass dock ~56px).
pub const CARD_H: i32 = 56;
/// Corner radius of the floating glass pill.
const DOCK_R: f32 = 19.0;
/// Extra room above the card for magnified icons.
pub const MAGNIFY_ROOM: i32 = 40;
/// Height of the Windows-style taskbar.
pub const TASKBAR_H: u32 = 48;
/// What is left of an auto-hidden dock: a strip at the screen edge that
/// brings it back when the pointer touches it.
pub const HIDDEN_H: u32 = 3;
/// Strongest magnification (1.0 = none). pearOS/macOS magnify 80% at the
/// pointer.
const MAX_SCALE: f32 = 1.8;
/// Gaussian width in icon cells (pearOS `amplitud` = 1.5).
const SIGMA: f32 = 1.5;
/// How far, in cells, the magnification reaches either side of the pointer
/// (3 sigma, where the gaussian has faded to nothing).
const REACH: f32 = 4.5;

/// How the layer surface for the dock is set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub namespace: &'static str,
    /// Full-width bar (anchored left and right) rather than a centred card.
    pub full_width: bool,
    pub exclusive: i32,
    pub bottom_margin: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug)]
enum Item<'a> {
    Start,
    App(&'a AppEntry),
    Agent,
    Trash,
    Separator,
    /// Solyra dock tray: weather chip (from `~/.config/solyra/weather.txt`).
    Weather,
    /// Solyra dock tray: live clock.
    Clock,
}

/// Width of the Solyra weather chip (sun glyph plus a short reading).
const WEATHER_W: i32 = 46;
/// Width of the Solyra clock chip. Snugly fits `09:41` / `12:59 PM`.
const CLOCK_W: i32 = 54;

fn item_span(it: &Item<'_>, cell: i32) -> i32 {
    match it {
        // Wider than the 1 px rule so the line has air on both sides.
        Item::Separator => 7,
        Item::Weather => WEATHER_W,
        Item::Clock => CLOCK_W,
        _ => cell,
    }
}

/// First line of `~/.config/solyra/weather.txt`. A bare number gains a degree
/// sign. Missing or empty is `None`: no network lookup and no invented reading,
/// and the dock then shows no weather chip at all rather than a placeholder.
fn normalize_weather(raw: Option<&str>) -> Option<String> {
    let line = raw.map(str::trim).filter(|s| !s.is_empty())?;
    if line.contains('°') {
        return Some(line.to_string());
    }
    let numeric = line
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
        && line.chars().any(|c| c.is_ascii_digit());
    Some(if numeric {
        format!("{line}°")
    } else {
        line.to_string()
    })
}

fn weather_reading() -> Option<String> {
    let path = solyra_conf::config_path("weather.txt");
    let raw = path.and_then(|p| std::fs::read_to_string(p).ok());
    normalize_weather(raw.as_deref().and_then(|s| s.lines().next()))
}

struct Metrics {
    cell: i32,
    gap: i32,
    pad: i32,
}

fn metrics(style: DockStyle) -> Metrics {
    match style {
        DockStyle::Minimal => Metrics {
            cell: 38,
            gap: 6,
            pad: 12,
        },
        // macOS Liquid Glass dock: 42px sculpted squircles with airy spacing
        // inside a luminous glass pill.
        DockStyle::Macos | DockStyle::Solyra => Metrics {
            cell: 42,
            gap: 7,
            pad: 12,
        },
        DockStyle::Windows => Metrics {
            cell: 40,
            gap: 6,
            pad: 14,
        },
    }
}

fn items<'a>(style: DockStyle, entries: &'a [AppEntry], weather: bool) -> Vec<Item<'a>> {
    let mut v = Vec::new();
    match style {
        DockStyle::Solyra => {
            v.push(Item::Start);
            v.push(Item::Separator);
            v.extend(entries.iter().map(Item::App));
            v.push(Item::Separator);
            v.push(Item::Agent);
            v.push(Item::Separator);
            if weather {
                v.push(Item::Weather);
            }
            v.push(Item::Clock);
        }
        DockStyle::Macos => {
            for (i, e) in entries.iter().enumerate() {
                v.push(Item::App(e));
                if i == 0 {
                    v.push(Item::Start);
                }
            }
            if entries.is_empty() {
                v.push(Item::Start);
            }
            v.push(Item::Separator);
            v.push(Item::Trash);
        }
        DockStyle::Minimal => v.extend(entries.iter().map(Item::App)),
        DockStyle::Windows => {}
    }
    v
}

fn base_width(style: DockStyle, wins: &[WindowInfo]) -> i32 {
    let entries = dock_entries(wins);
    let m = metrics(style);
    let its = items(style, &entries, weather_reading().is_some());
    let content: i32 = its.iter().map(|i| item_span(i, m.cell)).sum::<i32>()
        + m.gap * (its.len() as i32 - 1).max(0);
    content + m.pad * 2
}

/// The surface for `style`. `hidden` is the auto-hidden state.
pub fn spec(style: DockStyle, wins: &[WindowInfo], magnify: bool, hidden: bool) -> Spec {
    if style == DockStyle::Windows {
        return Spec {
            namespace: "solyra-taskbar",
            full_width: true,
            exclusive: if hidden { 0 } else { TASKBAR_H as i32 },
            bottom_margin: 0,
            width: 0,
            height: if hidden { HIDDEN_H } else { TASKBAR_H },
        };
    }
    let m = metrics(style);
    let w = base_width(style, wins).max(120);
    if hidden {
        return Spec {
            namespace: "solyra-dock-hidden",
            full_width: false,
            exclusive: 0,
            bottom_margin: 0,
            width: w as u32,
            height: HIDDEN_H,
        };
    }
    if magnify {
        // Room for every icon to grow, centred.
        let extra = ((MAX_SCALE - 1.0) * m.cell as f32 * 3.0).ceil() as i32;
        Spec {
            namespace: "solyra-dock-magnify",
            full_width: false,
            exclusive: 0,
            bottom_margin: 12,
            width: (w + extra) as u32,
            height: (CARD_H + MAGNIFY_ROOM) as u32,
        }
    } else {
        Spec {
            namespace: "solyra-dock",
            full_width: false,
            exclusive: 0,
            bottom_margin: 12,
            width: w as u32,
            height: CARD_H as u32,
        }
    }
}

/// Magnification for an icon whose centre is `dist` cells from the pointer.
pub fn magnification(dist: f32) -> f32 {
    if dist.abs() >= REACH {
        return 1.0;
    }
    // Gaussian falloff, the curve pearOS (and the real Dock) use:
    // 1 + zoom·exp(-d²/2σ²), smooth at every distance.
    1.0 + (MAX_SCALE - 1.0) * (-dist * dist / (2.0 * SIGMA * SIGMA)).exp()
}

/// Bounce height for a launching icon at this point in its cycle: up in
/// 300 ms with OutQuad, down in 300 ms with InQuad, like pearOS's dock.
/// `icon` is the icon's current (magnified) size; the jump is 60% of it.
pub fn bounce_offset(elapsed: std::time::Duration, icon: f32) -> f32 {
    let ms = elapsed.as_millis() as f32 % 600.0;
    let jump = icon * 0.6;
    let p = ms / 300.0;
    if p < 1.0 {
        jump * (1.0 - (1.0 - p) * (1.0 - p))
    } else {
        let q = p - 1.0;
        jump * (1.0 - q * q)
    }
}

/// The key a dock click records so the matching icon bounces while the app
/// starts.
pub fn launch_key(target: &LaunchTarget) -> String {
    match target {
        LaunchTarget::Shell(c) => c.clone(),
        LaunchTarget::Ipc(j) => j.to_string(),
        LaunchTarget::Internal(_) => "internal".into(),
    }
}

/// Draw the dock into the whole surface. Returns hit regions.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    style: DockStyle,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    live: &Live<'_>,
    magnify: bool,
    hidden: bool,
    launching: &[(String, std::time::Instant)],
) -> (Vec<Region>, Option<Body>) {
    if hidden {
        // A clear strip: the pointer touching it brings the dock back.
        return (
            vec![Region {
                x: 0,
                y: 0,
                w: cv.width as i32,
                h: cv.height as i32,
                action: HitAction::Shell(ShellCmd::RevealDock),
            }],
            None,
        );
    }
    match style {
        DockStyle::Windows => {
            let body = Body {
                x: 0,
                y: 0,
                w: cv.width as i32,
                h: cv.height as i32,
                r: 0.0,
            };
            (
                draw_taskbar(cv, th, fonts, wins, icons, hover, live),
                Some(body),
            )
        }
        _ => draw_floating(cv, th, fonts, style, wins, icons, hover, live, magnify, launching),
    }
}

/// The painted body of the dock (the card, or the whole taskbar): what the
/// compositor blurs behind for Glass and Acrylic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub r: f32,
}

fn slab_for(live: &Live<'_>, base: crate::tokens::Rgba) -> material::Slab {
    let look = &live.prefs.look;
    material::slab(
        look.material(look.dock_material),
        base,
        live.prefs.wall_tint,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_floating(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    style: DockStyle,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    live: &Live<'_>,
    magnify: bool,
    launching: &[(String, std::time::Instant)],
) -> (Vec<Region>, Option<Body>) {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let m = metrics(style);
    let entries = dock_entries(wins);
    let weather = weather_reading();
    let its = items(style, &entries, weather.is_some());
    let card_y = h - CARD_H;
    let base_w = base_width(style, wins);
    let base_left = (w - base_w) / 2;

    // Base (unmagnified) centre of every item, used to measure distance.
    let mut centres = Vec::with_capacity(its.len());
    let mut x = base_left + m.pad;
    for it in &its {
        let iw = item_span(it, m.cell);
        centres.push(x as f32 + iw as f32 / 2.0);
        x += iw + m.gap;
    }

    // Pointer over the card (not the clear room above it) drives magnification.
    let pointer_x = hover
        .filter(|(_, hy)| *hy >= card_y as f64)
        .map(|(hx, _)| hx as f32)
        .filter(|_| magnify);

    let sizes: Vec<i32> = its
        .iter()
        .zip(&centres)
        .map(|(it, c)| match it {
            Item::Separator | Item::Weather | Item::Clock => item_span(it, m.cell),
            _ => match pointer_x {
                Some(px) => {
                    (m.cell as f32 * magnification((px - c) / m.cell as f32)).round() as i32
                }
                None => m.cell,
            },
        })
        .collect();
    let content: i32 = sizes.iter().sum::<i32>() + m.gap * (its.len() as i32 - 1).max(0);
    let card_w = content + m.pad * 2;
    let card_x = (w - card_w) / 2;

    let slab = slab_for(live, th.surface);
    material::paint(cv, &slab, card_x, card_y, card_w, CARD_H, DOCK_R);
    let body = Body {
        x: card_x,
        y: card_y,
        w: card_w,
        h: CARD_H,
        r: DOCK_R,
    };

    let dark = th.surface.0 < 128;
    let ink = if material::ink_flips(
        live.prefs.look.material(live.prefs.look.dock_material),
        th.surface,
        live.prefs.wall_tint,
    ) {
        if dark {
            Rgba::rgb(0x1c, 0x1c, 0x1e)
        } else {
            Rgba::rgb(0xf5, 0xf5, 0xf7)
        }
    } else {
        th.text
    };
    let mut regions = Vec::new();
    let mut x = card_x + m.pad;
    // Icons sit a touch above centre so the running dots have room beneath them.
    let base_y = card_y + (CARD_H - m.cell) / 2 - 3;
    let clock = chrono::Local::now()
        .format(live.prefs.look.clock_format())
        .to_string();
    for (it, size) in its.iter().zip(&sizes) {
        if let Item::Separator = it {
            let sep_h = 28;
            let sep_y = card_y + (CARD_H - sep_h) / 2;
            let sw = *size;
            let sx = x + (sw - 1) / 2;
            let mid = Rgba::rgba(ink.0, ink.1, ink.2, 52);
            let end = Rgba::rgba(ink.0, ink.1, ink.2, 0);
            cv.fill_rect_vgrad(sx, sep_y, 1, sep_h / 2, end, mid);
            cv.fill_rect_vgrad(sx, sep_y + sep_h / 2, 1, sep_h - sep_h / 2, mid, end);
            x += sw + m.gap;
            continue;
        }
        let s = *size;
        let hov = hover
            .is_some_and(|(hx, hy)| hy >= card_y as f64 && hx >= x as f64 && hx < (x + s) as f64);
        // Bottom-aligned: magnified icons grow upward. Without magnification
        // the hovered icon lifts a little.
        let lift = if !magnify && hov { 3 } else { 0 };
        let y = base_y + (m.cell - s) - lift;
        match it {
            Item::Weather => {
                let cy = (card_y + CARD_H / 2) as f32;
                let wx = x + 4;
                cv.icon_sun(wx as f32 + 7.0, cy, ink);
                cv.text_truncated(
                    fonts.medium.as_ref(),
                    weather.as_deref().unwrap_or_default(),
                    wx + 18,
                    card_y + CARD_H / 2 + 5,
                    12.5,
                    (s - 22).max(8),
                    ink,
                );
                regions.push(Region {
                    x,
                    y: card_y,
                    w: s,
                    h: CARD_H,
                    action: HitAction::Shell(ShellCmd::ToggleControl),
                });
            }
            Item::Clock => {
                let size = 13.0;
                let max_w = (s - 6).max(8);
                let tw = text_width(fonts.semibold.as_ref(), &clock, size).min(max_w);
                cv.text_truncated(
                    fonts.semibold.as_ref(),
                    &clock,
                    x + (s - tw).max(0) / 2,
                    card_y + CARD_H / 2 + 5,
                    size,
                    max_w,
                    ink,
                );
                regions.push(Region {
                    x,
                    y: card_y,
                    w: s,
                    h: CARD_H,
                    action: HitAction::Shell(ShellCmd::ToggleControl),
                });
            }
            _ => {
                let (role, app_id, action) = match it {
                    Item::Start => ("start", None, HitAction::Shell(ShellCmd::ToggleLauncher)),
                    Item::Agent => ("agent", None, HitAction::Shell(ShellCmd::ToggleAgents)),
                    Item::Trash => (
                        "trash",
                        None,
                        HitAction::Launch(LaunchTarget::Shell(
                            "solyra-open files trash:///".into(),
                        )),
                    ),
                    Item::App(e) => (
                        e.role.as_str(),
                        Some(e.app_id.as_str()),
                        match e.window {
                            Some(id) => HitAction::FocusWindow(id),
                            None => HitAction::Launch(e.command.clone()),
                        },
                    ),
                    Item::Separator | Item::Weather | Item::Clock => unreachable!(),
                };
                let icon = app_id.and_then(|id| load_app_icon(icons, id));
                let bounce = match it {
                    Item::App(e) if e.window.is_none() => launching
                        .iter()
                        .find(|(k, _)| *k == launch_key(&e.command))
                        .map(|(_, t0)| bounce_offset(t0.elapsed(), s as f32).round() as i32)
                        .unwrap_or(0),
                    _ => 0,
                };
                cv.draw_app_squircle(x, y - bounce, s, role, icon, fonts);

                if let Item::App(e) = it {
                    if e.window_count > 0 {
                        running_mark(cv, ink, x, s, card_y, e.is_focused);
                    }
                    if hov && magnify {
                        tooltip(cv, th, fonts, &e.title, x + s / 2, y - 6, w);
                    }
                }
                if let Item::Agent = it {
                    agent_badge(cv, th, fonts, live, x, y, s);
                }
                regions.push(Region {
                    x,
                    y: card_y,
                    w: s,
                    h: CARD_H,
                    action,
                });
            }
        }
        x += s + m.gap;
    }
    (regions, Some(body))
}

fn running_mark(cv: &mut Canvas<'_>, ink: Rgba, x: i32, s: i32, card_y: i32, focused: bool) {
    // One quiet dot under a running app, like macOS: the focused app's is only
    // slightly stronger.
    let cx = x as f32 + s as f32 / 2.0;
    let cy = (card_y + CARD_H - 5) as f32;
    let (r, a) = if focused { (2.0, 235) } else { (1.7, 150) };
    cv.circle(cx, cy, r, Rgba::rgba(ink.0, ink.1, ink.2, a));
}

fn agent_badge(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    live: &Live<'_>,
    x: i32,
    y: i32,
    s: i32,
) {
    let agents = live.agents;
    if agents.live == 0 {
        return;
    }
    let col = if agents.needs_attention() {
        th.agent_approval
    } else {
        th.agent_active
    };
    let (bx, by) = ((x + s - 5) as f32, (y + 5) as f32);
    cv.circle(bx, by, 6.2, Rgba::rgb(0x12, 0x13, 0x15));
    cv.circle(bx, by, 4.7, col);
    if agents.needs_attention() {
        let n = (agents.waiting + agents.blocked).min(9).to_string();
        cv.text(
            fonts.semibold.as_ref(),
            &n,
            x + s - 8,
            y + 9,
            9.0,
            th.on_accent,
        );
    }
}

/// A label above a magnified icon, kept on screen.
fn tooltip(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    label: &str,
    cx: i32,
    bottom: i32,
    w: i32,
) {
    if bottom < 22 {
        return;
    }
    let tw = text_width(fonts.medium.as_ref(), label, 11.5).min(180);
    let bw = tw + 18;
    let bx = (cx - bw / 2).clamp(2, (w - bw - 2).max(2));
    let by = bottom - 22;
    cv.round_rect(bx, by, bw, 20, 7.0, th.surface);
    cv.round_rect_glass_rim(
        bx,
        by,
        bw,
        20,
        7.0,
        Rgba::rgba(255, 255, 255, 76),
        Rgba::rgba(255, 255, 255, 28),
        Rgba::rgba(0, 0, 0, 28),
    );
    cv.text_truncated(
        fonts.medium.as_ref(),
        label,
        bx + 9,
        by + 14,
        11.5,
        180,
        th.text,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_taskbar(
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
    let dark = th.surface.0 < 128;
    let mut regions = Vec::new();

    let slab = slab_for(live, th.bar);
    if dark && slab.fill.3 < 150 {
        // Windows 11 dark taskbar uses smoked Acrylic/Mica density so white
        // Search, glyphs, and tray clock stay razor-sharp over bright sand.
        cv.fill_rect_vgrad(
            0,
            0,
            w,
            h,
            Rgba::rgba(22, 24, 28, 174),
            Rgba::rgba(16, 18, 22, 194),
        );
    } else {
        material::paint(cv, &slab, 0, 0, w, h, 0.0);
    }
    // Crisp Windows 11 Acrylic/Mica top hairline catch-light.
    cv.fill_rect(
        0,
        0,
        w,
        1,
        if dark {
            Rgba::rgba(255, 255, 255, 52)
        } else {
            Rgba::rgba(255, 255, 255, 190)
        },
    );

    // Left: task view (the workspace overview), a real action.
    let tv_hov = hover.is_some_and(|(hx, _)| hx >= 8.0 && hx < 44.0);
    if tv_hov {
        cv.round_rect(8, 6, 36, 36, 8.0, th.tile_hover);
    }
    for (dx, dy) in [(0, 0), (9, 0), (0, 9), (9, 9)] {
        cv.round_rect(17 + dx, 15 + dy, 7, 7, 1.8, th.text_med);
    }
    regions.push(Region {
        x: 8,
        y: 4,
        w: 36,
        h: 40,
        action: HitAction::Launch(LaunchTarget::Ipc("{\"Action\":{\"ToggleOverview\":{}}}")),
    });

    // Centre: Windows 11 calm centred start mark, frosted search pill, apps.
    let entries = dock_entries(wins);
    let search_w = 176;
    let search_h = 30;
    let search_y = (h - search_h) / 2;
    let total_w = 40 + 8 + search_w + 10 + (entries.len() as i32) * 44;
    let mut cx = (w - total_w) / 2;
    cv.draw_app_squircle(cx + 3, 7, 34, "start", None, fonts);
    regions.push(Region {
        x: cx,
        y: 4,
        w: 40,
        h: 40,
        action: HitAction::Shell(ShellCmd::ToggleLauncher),
    });
    cx += 48;

    let s_hov = hover.is_some_and(|(hx, _)| hx >= cx as f64 && hx < (cx + search_w) as f64);
    let pill_r = search_h as f32 / 2.0;
    let (pill_top, pill_bot) = if dark {
        (
            Rgba::rgba(255, 255, 255, if s_hov { 34 } else { 22 }),
            Rgba::rgba(255, 255, 255, if s_hov { 24 } else { 14 }),
        )
    } else {
        (
            Rgba::rgba(255, 255, 255, if s_hov { 225 } else { 196 }),
            Rgba::rgba(246, 248, 252, if s_hov { 215 } else { 184 }),
        )
    };
    cv.round_rect_vgrad(cx, search_y, search_w, search_h, pill_r, pill_top, pill_bot);
    cv.round_rect_glass_rim(
        cx,
        search_y,
        search_w,
        search_h,
        pill_r,
        if dark {
            Rgba::rgba(255, 255, 255, 52)
        } else {
            Rgba::rgba(255, 255, 255, 220)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 22)
        } else {
            Rgba::rgba(0, 0, 0, 18)
        },
        if dark {
            Rgba::rgba(255, 255, 255, 16)
        } else {
            Rgba::rgba(0, 0, 0, 24)
        },
    );
    let search_cy = search_y as f32 + search_h as f32 / 2.0;
    cv.icon_search(cx as f32 + 16.0, search_cy, 12.5, th.text_med);
    cv.text(
        fonts.medium.as_ref(),
        "Search",
        cx + 30,
        search_y + 19,
        12.2,
        th.text_med,
    );
    regions.push(Region {
        x: cx,
        y: search_y,
        w: search_w,
        h: search_h,
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
                if is_hov { th.tile_hover } else { th.tile },
            );
            if entry.is_focused {
                cv.round_rect_glass_rim(
                    cx,
                    5,
                    40,
                    38,
                    8.0,
                    Rgba::rgba(255, 255, 255, if dark { 42 } else { 180 }),
                    Rgba::rgba(255, 255, 255, if dark { 18 } else { 90 }),
                    Rgba::rgba(0, 0, 0, 20),
                );
            }
        }
        let sys_ic = load_app_icon(icons, &entry.app_id);
        cv.draw_app_squircle(cx + 4, 7, 32, &entry.role, sys_ic, fonts);
        if entry.window_count > 0 {
            let uw = if entry.is_focused { 16 } else { 6 };
            let col = if entry.is_focused {
                th.accent
            } else {
                th.text_dim
            };
            cv.round_rect(cx + (40 - uw) / 2, h - 4, uw, 3, 1.5, col);
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

    // Right: system icons as one group, a hairline, then weather and the clock.
    let look = &prefs.look;
    let now = chrono::Local::now();
    let t_str = now.format(look.clock_format()).to_string();
    let d_str = now.format("%d/%m/%Y").to_string();
    let time_px = 13.0;
    let tw = text_width(fonts.semibold.as_ref(), &t_str, time_px);
    let dw = if look.show_date {
        text_width(fonts.medium.as_ref(), &d_str, 10.8)
    } else {
        0
    };
    let col_w = tw.max(dw);
    let clock_x = w - 14 - col_w;
    let time_y = if look.show_date { 20 } else { 29 };
    cv.text(
        fonts.semibold.as_ref(),
        &t_str,
        clock_x + col_w - tw,
        time_y,
        time_px,
        th.text,
    );
    if look.show_date {
        cv.text(
            fonts.medium.as_ref(),
            &d_str,
            clock_x + col_w - dw,
            36,
            10.8,
            th.text_dim,
        );
    }
    let mut wx = clock_x;
    if let Some(reading) = weather_reading() {
        let ww = text_width(fonts.medium.as_ref(), &reading, 12.2).min(60);
        wx = clock_x - 14 - (20 + ww);
        cv.icon_sun((wx + 7) as f32, 24.0, th.text);
        cv.text_truncated(
            fonts.medium.as_ref(),
            &reading,
            wx + 17,
            28,
            12.2,
            ww,
            th.text,
        );
    }

    let sep_x = wx - 12;
    cv.fill_rect(sep_x, 14, 1, h - 28, th.separator);

    let mut icon_right = sep_x - 8;
    let place = |right: &mut i32| -> f32 {
        *right -= 26;
        *right as f32 + 13.0
    };
    let bat = telem.battery_pct.map(|pct| (place(&mut icon_right), pct));
    let vol_cx = place(&mut icon_right);
    let net_cx = place(&mut icon_right);
    let tray_left = icon_right;

    cv.icon_network(net_cx, 24.0, 13.0, telem.network, th.text, th.text_dim);
    cv.icon_volume(vol_cx, 24.0, 13.0, prefs.volume, th.text);
    if let Some((cx, pct)) = bat {
        cv.icon_battery(cx, 24.0, pct, th.text, if pct <= 15 { th.danger } else { th.text });
    }
    regions.push(Region {
        x: tray_left,
        y: 4,
        w: w - tray_left,
        h: 40,
        action: HitAction::Shell(ShellCmd::ToggleControl),
    });
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnification_peaks_at_the_pointer_and_fades_smoothly() {
        assert!((magnification(0.0) - MAX_SCALE).abs() < 1e-4);
        assert_eq!(magnification(REACH), 1.0);
        assert_eq!(magnification(-5.0), 1.0);
        let mut last = magnification(0.0);
        for i in 1..=20 {
            let v = magnification(i as f32 * REACH / 20.0);
            assert!(v <= last + 1e-6, "monotonic");
            last = v;
        }
    }

    #[test]
    fn specs_per_style() {
        let wins = [];
        let t = spec(DockStyle::Windows, &wins, true, false);
        assert!(t.full_width && t.exclusive == TASKBAR_H as i32);
        assert_eq!(
            spec(DockStyle::Windows, &wins, false, true).exclusive,
            0,
            "a hidden taskbar reserves nothing"
        );
        let plain = spec(DockStyle::Solyra, &wins, false, false);
        let mag = spec(DockStyle::Solyra, &wins, true, false);
        assert_eq!(plain.namespace, "solyra-dock");
        assert_eq!(
            mag.namespace, "solyra-dock-magnify",
            "no compositor blur over the clear room"
        );
        assert!(mag.width > plain.width && mag.height > plain.height);
        assert_eq!(
            spec(DockStyle::Minimal, &wins, false, true).height,
            HIDDEN_H
        );
        assert!(spec(DockStyle::Minimal, &wins, false, false).width < plain.width);
    }

    #[test]
    fn items_per_style() {
        let e = vec![];
        assert!(matches!(items(DockStyle::Minimal, &e, true)[..], []));
        assert!(matches!(items(DockStyle::Solyra, &e, true)[0], Item::Start));
        assert!(matches!(
            items(DockStyle::Solyra, &e, true).last(),
            Some(Item::Clock)
        ));
        assert!(matches!(
            items(DockStyle::Macos, &e, true).last(),
            Some(Item::Trash)
        ));
        let has = |on| {
            items(DockStyle::Solyra, &e, on)
                .iter()
                .any(|i| matches!(i, Item::Weather))
        };
        assert!(has(true) && !has(false), "no reading, no weather chip");
    }

    #[test]
    fn weather_line_becomes_degrees_or_nothing() {
        let w = |s| normalize_weather(s).unwrap_or_default();
        assert_eq!(w(Some("25")), "25°");
        assert_eq!(w(Some("25°")), "25°");
        assert_eq!(w(Some(" 18 °C ")), "18 °C");
        assert_eq!(w(Some("Partly cloudy")), "Partly cloudy");
        assert_eq!(normalize_weather(None), None);
        assert_eq!(normalize_weather(Some("  ")), None);
    }

    #[test]
    fn solyra_width_includes_the_tray() {
        let wins = [];
        let w = spec(DockStyle::Solyra, &wins, false, false).width;
        // Start, agent, three separators, weather chip, clock chip, padding.
        assert!(w >= 320, "tray missing from width ({w})");
        let minimal = spec(DockStyle::Minimal, &wins, false, false).width;
        assert!(minimal < w);
    }
}
