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
    dock_entries, floating_dock_entries, load_app_icon, AppEntry, DockStyle, LaunchTarget,
    ShellCmd, WindowInfo,
};
use crate::surfaces::{HitAction, Live, Region};
use crate::tokens::Theme;

/// Height of the floating shelf (macOS 27: concentric 48 px icons + 10 px padding = 68 px).
pub const CARD_H: i32 = 68;
/// Corner radius of the shelf (concentric with 11 px squircle radius + 11 px padding = 22 px).
const DOCK_R: f32 = 22.0;
/// Extra room above the card for magnified icons + Liquid Glass caret tooltip.
pub const MAGNIFY_ROOM: i32 = 72;
/// Dimensions of the macOS 27 interactive Downloads stack Liquid Glass popover.
pub const STACK_W: u32 = 304;
pub const STACK_H: u32 = 264;
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
    /// The screen edge the dock is on.
    pub side: solyra_conf::look::DockPosition,
}

/// Turn a bottom dock's surface into a side dock's: the card stands on its
/// end along the left or right edge (no magnification there). The taskbar
/// always stays at the bottom.
pub fn orient(mut spec: Spec, side: solyra_conf::look::DockPosition) -> Spec {
    use solyra_conf::look::DockPosition;
    if side == DockPosition::Bottom || spec.full_width {
        return spec;
    }
    // Callers ask for the unmagnified spec: a side dock does not magnify.
    let hidden = spec.namespace == "solyra-dock-hidden";
    let length = spec.width;
    spec.width = if hidden { HIDDEN_H } else { CARD_H as u32 };
    spec.height = length;
    if !hidden {
        spec.namespace = "solyra-dock";
    }
    spec.side = side;
    spec
}

#[derive(Clone, Copy, Debug)]
enum Item<'a> {
    Start,
    App(&'a AppEntry),
    Agent,
    Trash,
    /// The Downloads folder stack, right of the separator.
    Downloads,
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
        // macOS 27 concentric dock metrics: 48 px icons, 7 px gap, 11 px outer padding.
        DockStyle::Macos => Metrics {
            cell: 48,
            gap: 7,
            pad: 11,
        },
        DockStyle::Solyra => Metrics {
            cell: 44,
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
        // pearOS order: Files, the launcher, the apps, the agents, then a
        // separator, the Downloads stack and the Trash.
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
            v.push(Item::Agent);
            v.push(Item::Separator);
            v.push(Item::Downloads);
            v.push(Item::Trash);
        }
        DockStyle::Minimal => v.extend(entries.iter().map(Item::App)),
        DockStyle::Windows => {}
    }
    v
}

/// The app entries a style shows.
fn entries_for(style: DockStyle, wins: &[WindowInfo]) -> Vec<AppEntry> {
    match style {
        DockStyle::Macos => floating_dock_entries(wins),
        _ => dock_entries(wins),
    }
}

fn base_width(style: DockStyle, wins: &[WindowInfo]) -> i32 {
    let entries = entries_for(style, wins);
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
            side: solyra_conf::look::DockPosition::Bottom,
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
            side: solyra_conf::look::DockPosition::Bottom,
        };
    }
    if magnify {
        // Room for every icon to grow: the gaussian adds (MAX_SCALE - 1) ·
        // σ·√(2π) cells in total (≈ 3 cells), plus a margin so the shelf's
        // rim and shadow never touch the surface edge.
        let grown = (MAX_SCALE - 1.0) * SIGMA * (2.0 * std::f32::consts::PI).sqrt();
        let extra = (grown * m.cell as f32).ceil() as i32 + 2 * m.cell;
        Spec {
            namespace: "solyra-dock-magnify",
            full_width: false,
            exclusive: 0,
            bottom_margin: 12,
            width: (w + extra) as u32,
            height: (CARD_H + MAGNIFY_ROOM) as u32,
            side: solyra_conf::look::DockPosition::Bottom,
        }
    } else {
        Spec {
            namespace: "solyra-dock",
            full_width: false,
            exclusive: 0,
            bottom_margin: 12,
            width: w as u32,
            height: CARD_H as u32,
            side: solyra_conf::look::DockPosition::Bottom,
        }
    }
}

/// Spring-damped magnification state for buttery 60 fps macOS Dock wave motion.
#[derive(Clone, Copy, Debug, Default)]
pub struct MagnifySpring {
    pub x: f32,
    pub strength: f32,
    pub target_x: Option<f32>,
}

impl MagnifySpring {
    pub fn set_target(&mut self, pointer_x: Option<f32>) {
        if let Some(tx) = pointer_x {
            if self.strength < 0.02 {
                self.x = tx;
            }
            self.target_x = Some(tx);
        } else {
            self.target_x = None;
        }
    }

    /// Advance by `dt` seconds. Returns `true` while still animating.
    pub fn step(&mut self, dt: f32, reduced_motion: bool) -> bool {
        let target_s = if self.target_x.is_some() { 1.0 } else { 0.0 };
        if reduced_motion {
            self.strength = target_s;
            if let Some(tx) = self.target_x {
                self.x = tx;
            }
            return false;
        }
        let k = 1.0 - (-dt * 20.0).exp();
        self.strength += (target_s - self.strength) * k;
        if let Some(tx) = self.target_x {
            self.x += (tx - self.x) * k;
        }
        let done_s = (self.strength - target_s).abs() < 0.008;
        let done_x = self.target_x.map_or(true, |tx| (self.x - tx).abs() < 0.4);
        if done_s && done_x {
            self.strength = target_s;
            if let Some(tx) = self.target_x {
                self.x = tx;
            }
            false
        } else {
            true
        }
    }
}

/// Magnification for an icon whose centre is `dist` cells from the pointer,
/// scaled by `strength` (0..=1 from [`MagnifySpring`]).
pub fn magnification_scaled(dist: f32, strength: f32) -> f32 {
    if dist.abs() >= REACH || strength <= 0.001 {
        return 1.0;
    }
    1.0 + (MAX_SCALE - 1.0)
        * strength.clamp(0.0, 1.0)
        * (-dist * dist / (2.0 * SIGMA * SIGMA)).exp()
}

/// Magnification for an icon whose centre is `dist` cells from the pointer.
pub fn magnification(dist: f32) -> f32 {
    magnification_scaled(dist, 1.0)
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
    draw_with_spring(
        cv, th, fonts, style, wins, icons, hover, live, magnify, None, hidden, launching,
    )
}

/// Like [`draw`], using a spring-damped [`MagnifySpring`] for live 60 fps dock motion.
#[allow(clippy::too_many_arguments)]
pub fn draw_with_spring(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    style: DockStyle,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    live: &Live<'_>,
    magnify: bool,
    spring: Option<MagnifySpring>,
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
    if style != DockStyle::Windows
        && live.prefs.look.dock_position != solyra_conf::look::DockPosition::Bottom
    {
        return draw_vertical(cv, th, fonts, style, wins, icons, hover, live, launching);
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
        _ => draw_floating(
            cv, th, fonts, style, wins, icons, hover, live, magnify, spring, launching,
        ),
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
    spring: Option<MagnifySpring>,
    launching: &[(String, std::time::Instant)],
) -> (Vec<Region>, Option<Body>) {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let m = metrics(style);
    let entries = entries_for(style, wins);
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
    // When a spring is supplied, it smoothly eases strength in/out even after leave.
    let mag_state: Option<(f32, f32)> = if !magnify {
        None
    } else if let Some(sp) = spring {
        (sp.strength > 0.002).then_some((sp.x, sp.strength))
    } else {
        hover
            .filter(|(_, hy)| *hy >= card_y as f64)
            .map(|(hx, _)| (hx as f32, 1.0))
    };

    let sizes: Vec<i32> = its
        .iter()
        .zip(&centres)
        .map(|(it, c)| match it {
            Item::Separator | Item::Weather | Item::Clock => item_span(it, m.cell),
            _ => match mag_state {
                Some((px, strength)) => {
                    (m.cell as f32 * magnification_scaled((px - c) / m.cell as f32, strength))
                        .round() as i32
                }
                None => m.cell,
            },
        })
        .collect();
    let content: i32 = sizes.iter().sum::<i32>() + m.gap * (its.len() as i32 - 1).max(0);
    // Never wider than the surface: with an icon magnified the shelf grows,
    // and it stays one pixel inside the surface so its rim is never cut.
    let card_w = (content + m.pad * 2).min(w - 2);
    let card_x = ((w - card_w) / 2).max(1);

    let slab = slab_for(live, th.surface);
    let mat = live.prefs.look.material(live.prefs.look.dock_material);
    material::set_glass_tint(live.prefs.look.glass_tint);
    if style == DockStyle::Macos && mat == material::Material::Glass {
        // macOS 27 Golden Gate dock: soft floating shadow + uniform Liquid Glass
        // refraction capsule (respecting the ultraclear-to-tinted slider).
        cv.round_rect_shadow(card_x, card_y, card_w, CARD_H, DOCK_R, 6.0, 18.0, 46);
        material::apple_glass(
            cv,
            card_x,
            card_y,
            card_w,
            CARD_H,
            DOCK_R,
            th.surface.0 < 128,
        );
    } else {
        cv.round_rect_shadow(card_x, card_y, card_w, CARD_H, DOCK_R, 6.0, 14.0, 54);
        material::paint(cv, &slab, card_x, card_y, card_w, CARD_H, DOCK_R);
    }
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
    // Concentric seating with 12 px bottom clearance so the running dot breathes cleanly.
    let base_y = card_y + ((CARD_H - m.cell) / 2 - 2).max(6);
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
                    Item::Start => (
                        if style == DockStyle::Macos {
                            "launchpad"
                        } else {
                            "start"
                        },
                        None,
                        HitAction::Shell(ShellCmd::ToggleLauncher),
                    ),
                    Item::Downloads => (
                        "downloads",
                        None,
                        HitAction::Shell(ShellCmd::ToggleDownloadsStack),
                    ),
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
                // An app's own icon wins over its role's (the browser pin is
                // Firefox when Firefox is what it opens).
                let key = app_id
                    .filter(|id| {
                        crate::icons::user(id).is_some() || crate::icons::name_for(id).is_some()
                    })
                    .unwrap_or(role);
                cv.draw_app_squircle(x, y - bounce, s, key, icon, fonts);
                apply_icon_style(cv, th, live.prefs.look.icon_style, x, y - bounce, s);

                if let Item::App(e) = it {
                    if e.window_count > 0 {
                        running_mark(cv, ink, x, s, card_y, e.is_focused);
                    }
                }
                // Hovered icons name themselves, as on macOS / pearOS.
                if hov {
                    let label = match it {
                        Item::App(e) => e.title.as_str(),
                        Item::Start => "Launchpad",
                        Item::Agent => "Agents",
                        Item::Downloads => "Downloads",
                        Item::Trash => "Trash",
                        _ => "",
                    };
                    if !label.is_empty() {
                        tooltip(cv, th, fonts, label, x + s / 2, y - bounce - 6, w);
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

/// The dock stood on its end along the left or right edge, as macOS does
/// it: the same items top to bottom, separators across, the running dot on
/// the side facing the screen edge, labels opening toward the desktop.
#[allow(clippy::too_many_arguments)]
fn draw_vertical(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    style: DockStyle,
    wins: &[WindowInfo],
    icons: &mut HashMap<String, Option<Icon>>,
    hover: Option<(f64, f64)>,
    live: &Live<'_>,
    launching: &[(String, std::time::Instant)],
) -> (Vec<Region>, Option<Body>) {
    use solyra_conf::look::DockPosition;
    let w = cv.width as i32;
    let h = cv.height as i32;
    let left = live.prefs.look.dock_position == DockPosition::Left;
    let m = metrics(style);
    let entries = entries_for(style, wins);
    let its = items(style, &entries, false);
    let its: Vec<_> = its
        .into_iter()
        .filter(|i| !matches!(i, Item::Weather | Item::Clock))
        .collect();
    let content: i32 = its.iter().map(|i| item_span(i, m.cell)).sum::<i32>()
        + m.gap * (its.len() as i32 - 1).max(0);
    let card_h = (content + m.pad * 2).min(h - 2);
    let (card_x, card_y, card_w) = (0, ((h - card_h) / 2).max(1), w.min(CARD_H));
    let dark = th.surface.0 < 128;
    let mat = live.prefs.look.material(live.prefs.look.dock_material);
    if mat == material::Material::Glass {
        material::apple_glass(cv, card_x, card_y, card_w, card_h, DOCK_R, dark);
    } else {
        let slab = slab_for(live, th.surface);
        material::paint(cv, &slab, card_x, card_y, card_w, card_h, DOCK_R);
    }
    let body = Body {
        x: card_x,
        y: card_y,
        w: card_w,
        h: card_h,
        r: DOCK_R,
    };
    let ink = th.text;
    let mut regions = Vec::new();
    let mut y = card_y + m.pad;
    let x = card_x + (card_w - m.cell) / 2;
    for it in &its {
        if let Item::Separator = it {
            let span = item_span(it, m.cell);
            let sy = y + (span - 1) / 2;
            let mid = Rgba::rgba(ink.0, ink.1, ink.2, 52);
            let end = Rgba::rgba(ink.0, ink.1, ink.2, 0);
            let sep_w = 28;
            let sx = card_x + (card_w - sep_w) / 2;
            cv.fill_rect(sx + 4, sy, sep_w - 8, 1, mid);
            let _ = end;
            y += span + m.gap;
            continue;
        }
        let s = m.cell;
        let hov = hover.is_some_and(|(hx, hy)| {
            hx >= card_x as f64
                && hx < (card_x + card_w) as f64
                && hy >= y as f64
                && hy < (y + s) as f64
        });
        let (role, app_id, action) = match it {
            Item::Start => (
                "launchpad",
                None,
                HitAction::Shell(ShellCmd::ToggleLauncher),
            ),
            Item::Downloads => (
                "downloads",
                None,
                HitAction::Launch(LaunchTarget::Shell(
                    "solyra-open files \"$HOME/Downloads\"".into(),
                )),
            ),
            Item::Agent => ("agent", None, HitAction::Shell(ShellCmd::ToggleAgents)),
            Item::Trash => (
                "trash",
                None,
                HitAction::Launch(LaunchTarget::Shell("solyra-open files trash:///".into())),
            ),
            Item::App(e) => (
                e.role.as_str(),
                Some(e.app_id.as_str()),
                match e.window {
                    Some(id) => HitAction::FocusWindow(id),
                    None => HitAction::Launch(e.command.clone()),
                },
            ),
            _ => {
                y += s + m.gap;
                continue;
            }
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
        let key = app_id
            .filter(|id| crate::icons::user(id).is_some() || crate::icons::name_for(id).is_some())
            .unwrap_or(role);
        // A hovered icon leans toward the desktop; a launch bounces inward.
        let nudge = (if hov { 3 } else { 0 }) + bounce;
        let ix = if left { x + nudge } else { x - nudge };
        cv.draw_app_squircle(ix, y, s, key, icon, fonts);
        if let Item::App(e) = it {
            if e.window_count > 0 {
                let dx = if left {
                    card_x + 6
                } else {
                    card_x + card_w - 6
                };
                let (r, a) = if e.is_focused { (2.1, 235) } else { (2.0, 170) };
                cv.circle(
                    dx as f32,
                    (y + s / 2) as f32,
                    r,
                    Rgba::rgba(ink.0, ink.1, ink.2, a),
                );
            }
        }
        if let Item::Agent = it {
            agent_badge(cv, th, fonts, live, ix, y, s);
        }
        regions.push(Region {
            x: card_x,
            y,
            w: card_w,
            h: s,
            action,
        });
        y += s + m.gap;
    }
    (regions, Some(body))
}

fn running_mark(cv: &mut Canvas<'_>, ink: Rgba, x: i32, s: i32, card_y: i32, focused: bool) {
    // Polished macOS 27 running indicator dot with generous bottom clearance and soft glow.
    let cx = x as f32 + s as f32 / 2.0;
    let cy = (card_y + CARD_H - 5) as f32;
    let halo_a = if focused { 62 } else { 32 };
    cv.circle(cx, cy, 4.0, Rgba::rgba(ink.0, ink.1, ink.2, halo_a));
    let (r, a) = if focused { (2.1, 245) } else { (1.9, 185) };
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

/// Subtle macOS 27 icon appearance overlay (Dark, Tinted, Clear).
fn apply_icon_style(
    cv: &mut Canvas<'_>,
    th: &Theme,
    style: solyra_conf::look::IconStyle,
    x: i32,
    y: i32,
    s: i32,
) {
    use solyra_conf::look::IconStyle;
    let r = (s as f32 * 0.225).max(4.0);
    match style {
        IconStyle::Default => {}
        IconStyle::Dark => {
            cv.round_rect(x, y, s, s, r, Rgba::rgba(10, 10, 14, 72));
            cv.stroke_round_rect(x, y, s, s, r, 1.0, Rgba::rgba(255, 255, 255, 36));
        }
        IconStyle::Tinted => {
            cv.round_rect(x, y, s, s, r, Rgba::rgba(th.accent.0, th.accent.1, th.accent.2, 68));
            cv.stroke_round_rect(x, y, s, s, r, 1.0, Rgba::rgba(255, 255, 255, 58));
        }
        IconStyle::Clear => {
            cv.round_rect(x, y, s, s, r, Rgba::rgba(255, 255, 255, 28));
            cv.rim_light(x, y, s, s, r, 1.4, Rgba::rgba(255, 255, 255, 195), 0.7);
        }
    }
}

/// A macOS 27 Liquid Glass pill label with downward caret above a hovered icon.
fn tooltip(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    label: &str,
    cx: i32,
    bottom: i32,
    w: i32,
) {
    let caret_h = 6;
    let bh = 24;
    if bottom < bh + caret_h + 2 {
        return;
    }
    let tw = text_width(fonts.medium.as_ref(), label, 12.0).min(180);
    let bw = tw + 22;
    let r = bh as f32 / 2.0;
    let bx = (cx - bw / 2).clamp(2, (w - bw - 2).max(2));
    let by = bottom - bh - caret_h;
    let dark = th.surface.0 < 128;
    cv.round_rect_shadow(bx, by, bw, bh, r, 4.0, 12.0, 44);
    material::apple_glass(cv, bx, by, bw, bh, r, dark);
    // Crisp downward-pointing triangular caret seamlessly anchored to the pill bottom.
    let tip_x = cx.clamp(bx + 12, bx + bw - 12);
    let caret_fill = if dark {
        Rgba::rgba(28, 30, 36, 215)
    } else {
        Rgba::rgba(250, 251, 255, 228)
    };
    let caret_rim = Rgba::rgba(255, 255, 255, if dark { 115 } else { 210 });
    for row in 0..caret_h {
        let half = (caret_h - 1 - row).max(0);
        let y = by + bh + row;
        cv.fill_rect(tip_x - half, y, half * 2 + 1, 1, caret_fill);
        cv.fill_rect(tip_x - half, y, 1, 1, caret_rim);
        cv.fill_rect(tip_x + half, y, 1, 1, caret_rim);
    }
    cv.text_truncated(
        fonts.medium.as_ref(),
        label,
        bx + 11,
        by + 16,
        12.0,
        180,
        th.text,
    );
}

/// Read up to 6 most-recently-modified items from `$HOME/Downloads`.
pub fn downloads_entries() -> Vec<std::path::PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return Vec::new();
    };
    let dir = home.join("Downloads");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items: Vec<(std::time::SystemTime, std::path::PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
                return None;
            }
            let mtime = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((mtime, p))
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.into_iter().take(6).map(|(_, p)| p).collect()
}

/// Interactive macOS 27 Liquid Glass Downloads folder stack popover anchored
/// above the Downloads icon on the Dock.
pub fn draw_downloads_stack(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    items: &[std::path::PathBuf],
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    let w = cv.width as i32;
    let h = cv.height as i32;
    let caret_h = 8;
    let card_h = (h - caret_h).max(80);
    let dark = th.surface.0 < 128;
    let mut regions = Vec::new();

    cv.round_rect_shadow(0, 0, w, card_h, 20.0, 10.0, 26.0, 62);
    material::apple_glass(cv, 0, 0, w, card_h, 20.0, dark);
    // Downward caret pointing toward the Downloads icon in the Dock.
    let tip_x = w - 62;
    let caret_fill = if dark {
        Rgba::rgba(26, 28, 34, 210)
    } else {
        Rgba::rgba(250, 251, 255, 224)
    };
    for row in 0..caret_h {
        let half = (caret_h - 1 - row).max(0);
        cv.fill_rect(tip_x - half, card_h + row, half * 2 + 1, 1, caret_fill);
    }

    // Header
    cv.text(fonts.semibold.as_ref(), "Downloads", 16, 26, 13.5, th.text);
    let count = format!("{} items", items.len());
    let cw = text_width(fonts.regular.as_ref(), &count, 11.5);
    cv.text(fonts.regular.as_ref(), &count, w - 16 - cw, 26, 11.5, th.text_dim);

    // 3x2 grid of downloaded files
    let grid_y = 38;
    let footer_h = 36;
    let avail_h = (card_h - grid_y - footer_h - 8).max(80);
    if items.is_empty() {
        let msg = "Downloads folder is empty";
        let mw = text_width(fonts.regular.as_ref(), msg, 12.5);
        cv.text(
            fonts.regular.as_ref(),
            msg,
            (w - mw) / 2,
            grid_y + avail_h / 2,
            12.5,
            th.text_dim,
        );
    } else {
        let cols = 3;
        let gap = 8;
        let cell_w = (w - 32 - gap * (cols - 1)) / cols;
        let cell_h = (avail_h - gap) / 2;
        for (i, path) in items.iter().take(6).enumerate() {
            let col = (i as i32) % cols;
            let row = (i as i32) / cols;
            let cx = 16 + col * (cell_w + gap);
            let cy = grid_y + row * (cell_h + gap);
            let hov = hover.is_some_and(|(hx, hy)| {
                hx >= cx as f64 && hx < (cx + cell_w) as f64 && hy >= cy as f64 && hy < (cy + cell_h) as f64
            });
            cv.round_rect(
                cx,
                cy,
                cell_w,
                cell_h,
                12.0,
                if hov { th.tile_hover } else { th.tile },
            );
            // Document/folder badge icon
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let badge_col = match ext.as_str() {
                "pdf" => Rgba::rgb(0xe5, 0x48, 0x4d),
                "png" | "jpg" | "jpeg" | "webp" => Rgba::rgb(0x34, 0xa8, 0x53),
                "zip" | "tar" | "gz" | "appimage" => Rgba::rgb(0xf5, 0x9e, 0x0b),
                "" if path.is_dir() => Rgba::rgb(0x3b, 0x9b, 0xf2),
                _ => Rgba::rgb(0x5e, 0x9c, 0xff),
            };
            let ix = cx + (cell_w - 28) / 2;
            let iy = cy + 10;
            cv.round_rect(ix, iy, 28, 34, 6.0, badge_col);
            cv.fill_rect(ix + 5, iy + 12, 18, 2, Rgba::rgba(255, 255, 255, 195));
            cv.fill_rect(ix + 5, iy + 18, 14, 2, Rgba::rgba(255, 255, 255, 165));
            let name = crate::shelf::label(path);
            let nw = text_width(fonts.medium.as_ref(), &name, 10.5).min(cell_w - 10);
            cv.text_truncated(
                fonts.medium.as_ref(),
                &name,
                cx + (cell_w - nw) / 2,
                cy + cell_h - 10,
                10.5,
                cell_w - 10,
                th.text,
            );
            regions.push(Region {
                x: cx,
                y: cy,
                w: cell_w,
                h: cell_h,
                action: HitAction::Launch(LaunchTarget::Shell(crate::shelf::open_command(path))),
            });
        }
    }

    // Footer: "Open in Finder ›"
    let fy = card_h - footer_h;
    cv.fill_rect(14, fy, w - 28, 1, th.separator);
    let fhov = hover.is_some_and(|(hx, hy)| hx >= 12.0 && hx < (w - 12) as f64 && hy >= fy as f64 && hy < card_h as f64);
    if fhov {
        cv.round_rect(10, fy + 4, w - 20, footer_h - 8, 9.0, th.tile_hover);
    }
    let foot_lbl = "Open in Finder ›";
    let fw = text_width(fonts.semibold.as_ref(), foot_lbl, 12.0);
    cv.text(fonts.semibold.as_ref(), foot_lbl, (w - fw) / 2, fy + 22, 12.0, th.text);
    regions.push(Region {
        x: 10,
        y: fy + 4,
        w: w - 20,
        h: footer_h - 8,
        action: HitAction::Launch(LaunchTarget::Shell(
            "solyra-open files \"$HOME/Downloads\"".into(),
        )),
    });
    regions
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
        cv.icon_battery(
            cx,
            24.0,
            pct,
            th.text,
            if pct <= 15 { th.danger } else { th.text },
        );
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
    fn floating_dock_follows_the_pearos_order() {
        let e = crate::state::floating_dock_entries(&[]);
        assert_eq!(e[0].role, "files", "Files first");
        assert_eq!(e.last().unwrap().role, "settings", "Settings last");
        let its = items(DockStyle::Macos, &e, false);
        assert!(matches!(its[1], Item::Start), "the launcher second");
        let n = its.len();
        assert!(matches!(its[n - 3], Item::Separator));
        assert!(matches!(its[n - 2], Item::Downloads));
        assert!(matches!(its[n - 1], Item::Trash));
    }

    /// The gap report found the magnified shelf clipped at the surface edge:
    /// with the pointer anywhere along it, the grown shelf must fit inside
    /// the surface with room to spare.
    #[test]
    fn a_magnified_shelf_never_reaches_the_surface_edge() {
        let wins = [];
        let sp = spec(DockStyle::Macos, &wins, true, false);
        let (w, h) = (sp.width, sp.height);
        let fonts = crate::canvas::Fonts::load();
        let th = Theme::dark();
        let t = crate::state::Telemetry {
            mem_used_mib: 0,
            mem_total_mib: 0,
            battery_pct: None,
            network: crate::state::NetworkState::Offline,
            brightness_controllable: false,
            volume_controllable: false,
        };
        let prefs = crate::state::ShellPrefs::default();
        let live = Live {
            agents: Default::default(),
            telem: &t,
            prefs: &prefs,
        };
        let mut x = 0.0;
        while x < w as f64 {
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let mut cv = Canvas::new(&mut buf, w, h, 1);
            let mut icons = HashMap::new();
            let (_, body) = draw(
                &mut cv,
                &th,
                &fonts,
                DockStyle::Macos,
                &wins,
                &mut icons,
                Some((x, h as f64 - 10.0)),
                &live,
                true,
                false,
                &[],
            );
            let b = body.unwrap();
            assert!(
                b.x >= 4 && b.x + b.w <= w as i32 - 4,
                "pointer at {x}: shelf {b:?} in {w}"
            );
            x += 23.0;
        }
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

    #[test]
    fn magnify_spring_smoothly_eases_in_and_out() {
        let mut sp = MagnifySpring::default();
        sp.set_target(Some(240.0));
        assert!(sp.step(0.016, false));
        assert!(sp.strength > 0.1 && sp.strength < 0.9);
        assert!(magnification_scaled(0.0, sp.strength) > 1.0);
        for _ in 0..40 {
            sp.step(0.016, false);
        }
        assert!((sp.strength - 1.0).abs() < 0.02);
        sp.set_target(None);
        assert!(sp.step(0.016, false));
        assert!(sp.strength < 0.9 && sp.strength > 0.05);
        for _ in 0..40 {
            sp.step(0.016, false);
        }
        assert_eq!(sp.strength, 0.0);
    }
}
