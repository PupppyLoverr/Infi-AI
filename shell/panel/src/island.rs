//! The notch and Dynamic Island: one pure-black hardware capsule at the top
//! centre that morphs between an idle pill, a snug compact Live Activity, and
//! a concentric expanded card.
//!
//! Two looks, chosen in Settings (`notch_style`):
//! * **Notch** – hangs from the top edge with concave "ears", like a laptop
//!   display notch; expands downward.
//! * **Island** – a floating pure-black capsule below the top edge; expands
//!   into a concentric rounded card (`r = 44`).
//!
//! It lives on its own layer surface, larger than the shape; only the shape
//! takes input, and the rest is transparent, so the shape can grow and shrink
//! smoothly without a surface resize per frame.

use crate::agent::{AskKind, Scope, Verdict};
use crate::canvas::{text_width, wrap_text, Canvas, Fonts, Rgba};
use crate::media::{Control, Media};
use crate::state::{LaunchTarget, ShellCmd};
use crate::surfaces::{HitAction, Region};
use crate::tokens::{Theme, T_BODY, T_CAPTION, T_LABEL, T_META};
use solyra_conf::look::NotchStyle;

/// The island's surface; the shape never exceeds it. Sized for Droppy's
/// 3-page shelf, concurrent Live Activities, HUDs, and integrated Spotlight.
pub const SURF_W: u32 = 580;
pub const SURF_H: u32 = 420;
/// Concave fillet radius joining a notch to the top edge.
const EAR: f32 = 9.0;

/// System HUDs growing directly out of the Dynamic Island (Droppy System HUDs).
#[derive(Clone, Debug, PartialEq)]
pub enum HudKind {
    Volume(u8),
    Brightness(u8),
    KeyboardBacklight(u8),
}

/// Concurrent compact Live Activity badges that wrap around the camera pill.
#[derive(Clone, Debug, PartialEq)]
pub enum LiveBadge {
    /// Active Pomodoro / Focus countdown in seconds.
    Pomodoro(u32),
    /// Active download progress (0..=100; 100 shows a completion check).
    Download { name: String, pct: u8 },
    /// Connected Bluetooth / AirPods battery percentage.
    AirPods { name: String, pct: u8 },
    /// Active coding agent tool call.
    Agent { who: String, tool: String },
    /// Low-battery warning pill.
    LowBattery(u8),
}

/// Interactive notification card landing in the notch.
#[derive(Clone, Debug, PartialEq)]
pub struct NotifCard {
    pub app: String,
    pub title: String,
    pub body: String,
}

/// Droppy modular shelf droplets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropletInfo {
    pub id: u8,
    pub name: &'static str,
    pub summary: &'static str,
}

pub const DROPLETS: &[DropletInfo] = &[
    DropletInfo { id: 0, name: "Now Playing", summary: "Album art, scrubber & queue" },
    DropletInfo { id: 1, name: "File Tray", summary: "Drop, zip & track ~/Downloads" },
    DropletInfo { id: 2, name: "Pomodoro", summary: "Focus countdown timer" },
    DropletInfo { id: 3, name: "Weather", summary: "Live conditions & temp" },
    DropletInfo { id: 4, name: "System Stats", summary: "CPU, RAM & network graph" },
    DropletInfo { id: 5, name: "Agents", summary: "Live coding-agent tool stream" },
    DropletInfo { id: 6, name: "Clipboard", summary: "Recent copied items" },
    DropletInfo { id: 7, name: "Notes", summary: "Quick Markdown scratchpad" },
    DropletInfo { id: 8, name: "Audio Control", summary: "Per-app volume sliders" },
    DropletInfo { id: 9, name: "Calendar", summary: "Today's upcoming events" },
];

/// What the island is showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Idle,
    Message(String),
    Activity { who: String, text: String },
    Attention(Ask),
    Media(Media),
    /// The Droppy File Tray tab: files parked on the notch (newest first).
    Shelf(Vec<std::path::PathBuf>),
    /// Droppy Home Shelf: multi-widget board (Now Playing + Pomodoro + Weather + System Stats + Agents).
    Widgets {
        media: Option<Media>,
        pomodoro_secs: Option<u32>,
        weather: Option<String>,
        cpu_pct: u8,
        ram_pct: u8,
        clipboard_preview: Option<String>,
    },
    /// Droppy Droplets Picker (Page 2): toggle modular shelf widgets.
    Droplets {
        enabled_mask: u16,
    },
    /// Droppy System HUD (Volume / Brightness / Keyboard Backlight).
    Hud(HudKind),
    /// Interactive Notification landing in the notch.
    Notif(NotifCard),
    /// Apple-style Spotlight integrated directly into the Dynamic Island.
    Spotlight {
        query: String,
        selected: usize,
        ask_mode: bool,
        ask_reply: Option<String>,
    },
}

/// An approval or question waiting on the human.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub task: usize,
    pub who: String,
    pub kind: AskKind,
    pub text: String,
    pub resource: Option<String>,
    pub reversible: Option<bool>,
    pub task_scope: bool,
    pub choices: Vec<String>,
    /// A decision was already sent; waiting for the agent.
    pub sent: bool,
}

/// Facts the idle card shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glance {
    pub battery: Option<u8>,
    pub network: String,
    pub agents_running: usize,
    pub windows: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub w: f32,
    pub h: f32,
    pub y: f32,
    pub r: f32,
}

impl Shape {
    pub fn x(&self) -> f32 {
        (SURF_W as f32 - self.w) / 2.0
    }
}

/// Expanded card size. Media is an Apple Live Activity (`440×156`, radius `44`
/// on the island). Droppy Home Widgets & Droplets expand to `488×204`.
/// Dynamic Island Spotlight expands to `548×104` (empty) or `548×388` (query/Ask).
fn expanded_shape(style: NotchStyle, content: &Content) -> Shape {
    let island = style == NotchStyle::Island;
    let (w, h, r) = match content {
        Content::Media(_) => (440.0, 156.0, if island { 44.0 } else { 30.0 }),
        Content::Shelf(_) => (476.0, 196.0, if island { 36.0 } else { 28.0 }),
        Content::Widgets { .. } | Content::Droplets { .. } => {
            (516.0, 214.0, if island { 36.0 } else { 28.0 })
        }
        Content::Hud(_) => (264.0, 42.0, if island { 21.0 } else { 18.0 }),
        Content::Notif(_) => (440.0, 102.0, if island { 32.0 } else { 24.0 }),
        Content::Spotlight {
            query,
            ask_mode,
            ask_reply,
            ..
        } => {
            let h = if *ask_mode || ask_reply.is_some() {
                216.0
            } else if !query.trim().is_empty() {
                272.0
            } else {
                96.0
            };
            (500.0, h, if island { 30.0 } else { 24.0 })
        }
        Content::Idle => (
            if island { 348.0 } else { 392.0 },
            132.0,
            if island { 44.0 } else { 30.0 },
        ),
        _ => (
            if island { 440.0 } else { 448.0 },
            196.0,
            if island { 44.0 } else { 30.0 },
        ),
    };
    Shape {
        w,
        h,
        y: if island { 6.0 } else { 0.0 },
        r,
    }
}

/// Art, waveform and padding around the title in a compact media capsule.
const MEDIA_CHROME: f32 = 78.0;

/// A compact media capsule hugs its title: art, title, waveform and nothing
/// else. Glyph widths are estimated because the fonts are not at hand here.
fn compact_media_width(m: &crate::media::Media, min: f32, max: f32) -> f32 {
    let line = if m.title.is_empty() { &m.artist } else { &m.title };
    let text = line.chars().map(|c| if c.is_uppercase() { 7.8 } else { 6.3 }).sum::<f32>();
    (MEDIA_CHROME + text).clamp(min, max).round()
}

/// Where the shape wants to be.
pub fn target(style: NotchStyle, content: &Content, expanded: bool) -> Shape {
    match style {
        NotchStyle::Island => {
            if expanded {
                expanded_shape(style, content)
            } else {
                match content {
                    // pearOS island: a 170 x 30 black pill 5 px below the
                    // top edge; live activities keep that height and widen.
                    Content::Idle | Content::Shelf(_) | Content::Droplets { .. } => Shape {
                        w: 170.0,
                        h: 30.0,
                        y: 5.0,
                        r: 15.0,
                    },
                    Content::Hud(_) => Shape {
                        w: 256.0,
                        h: 34.0,
                        y: 5.0,
                        r: 17.0,
                    },
                    Content::Notif(_) => Shape {
                        w: 320.0,
                        h: 34.0,
                        y: 5.0,
                        r: 17.0,
                    },
                    Content::Spotlight { .. } => expanded_shape(style, content),
                    Content::Widgets { media: Some(m), .. } => Shape {
                        w: compact_media_width(m, 200.0, 280.0),
                        h: 30.0,
                        y: 5.0,
                        r: 15.0,
                    },
                    Content::Widgets { .. } => Shape {
                        w: 170.0,
                        h: 30.0,
                        y: 5.0,
                        r: 15.0,
                    },
                    Content::Media(m) => Shape {
                        w: compact_media_width(m, 200.0, 280.0),
                        h: 30.0,
                        y: 5.0,
                        r: 15.0,
                    },
                    Content::Activity { .. } | Content::Message(_) => Shape {
                        w: 280.0,
                        h: 30.0,
                        y: 5.0,
                        r: 15.0,
                    },
                    Content::Attention(_) => Shape {
                        w: 310.0,
                        h: 30.0,
                        y: 5.0,
                        r: 15.0,
                    },
                }
            }
        }
        // Off has no surface; the notch numbers are a harmless default.
        NotchStyle::Notch | NotchStyle::Off => {
            if expanded {
                expanded_shape(style, content)
            } else {
                match content {
                    Content::Idle | Content::Shelf(_) | Content::Droplets { .. } => Shape {
                        w: 188.0,
                        h: 32.0,
                        y: 0.0,
                        r: 14.0,
                    },
                    Content::Hud(_) => Shape {
                        w: 256.0,
                        h: 36.0,
                        y: 0.0,
                        r: 16.0,
                    },
                    Content::Spotlight { .. } => expanded_shape(style, content),
                    Content::Media(m) | Content::Widgets { media: Some(m), .. } => Shape {
                        w: compact_media_width(m, 212.0, 300.0),
                        h: 36.0,
                        y: 0.0,
                        r: 16.0,
                    },
                    _ => Shape {
                        w: 312.0,
                        h: 36.0,
                        y: 0.0,
                        r: 16.0,
                    },
                }
            }
        }
    }
}

/// Smoothly approaches its target, frame by frame.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub cur: Shape,
    pub to: Shape,
}

impl Anim {
    pub fn new(at: Shape) -> Self {
        Self { cur: at, to: at }
    }

    /// Advance by `dt` seconds. Returns true while still moving.
    pub fn step(&mut self, dt: f32, reduced_motion: bool) -> bool {
        if reduced_motion {
            self.cur = self.to;
            return false;
        }
        // Critically-damped feel: fast at first, settling without overshoot.
        let k = 1.0 - (-dt * 16.0).exp();
        let lerp = |a: f32, b: f32| a + (b - a) * k;
        self.cur = Shape {
            w: lerp(self.cur.w, self.to.w),
            h: lerp(self.cur.h, self.to.h),
            y: lerp(self.cur.y, self.to.y),
            r: lerp(self.cur.r, self.to.r),
        };
        let close = |a: f32, b: f32| (a - b).abs() < 0.6;
        if close(self.cur.w, self.to.w)
            && close(self.cur.h, self.to.h)
            && close(self.cur.y, self.to.y)
        {
            self.cur = self.to;
            false
        } else {
            true
        }
    }

    pub fn moving(&self) -> bool {
        self.cur != self.to
    }
}

/// The shape's rectangle in surface pixels, for the input region.
pub fn input_rect(s: &Shape) -> (i32, i32, i32, i32) {
    (
        s.x().floor() as i32,
        s.y.floor() as i32,
        s.w.ceil() as i32,
        s.h.ceil() as i32,
    )
}

fn hovered(hover: Option<(f64, f64)>, x: i32, y: i32, w: i32, h: i32) -> bool {
    hover.is_some_and(|(hx, hy)| {
        hx >= x as f64 && hx < (x + w) as f64 && hy >= y as f64 && hy < (y + h) as f64
    })
}

/// Everything the expanded card may need beyond the content itself.
pub struct Extra<'a> {
    pub glance: &'a Glance,
    pub clock: &'a str,
    pub date: &'a str,
    pub reply: &'a str,
    pub typing: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    style: NotchStyle,
    anim: &Anim,
    content: &Content,
    expanded: bool,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
) -> Vec<Region> {
    // Dynamic Island is always pure OLED hardware black — never a grey bar pill.
    let th = Theme::dark();
    let s = anim.cur;
    let (x, y, w, h) = (s.x(), s.y, s.w, s.h);
    let (xi, yi, wi, hi) = (
        x.round() as i32,
        y.round() as i32,
        w.round() as i32,
        h.round() as i32,
    );

    // Dynamic Island: pure OLED black at the camera pill; when expanded into
    // Droppy's shelf or integrated Spotlight (and not in Low Battery Mode < 20%),
    // uses Dynamic Glass — pure black at the top camera band melting smoothly
    // into smoked translucent Liquid Glass below.
    let pure_black = Rgba::rgb(0, 0, 0);
    let low_battery = extra.glance.battery.is_some_and(|b| b <= 20);
    let dynamic_glass = expanded
        && !low_battery
        && matches!(
            content,
            Content::Widgets { .. }
                | Content::Shelf(_)
                | Content::Droplets { .. }
                | Content::Spotlight { .. }
                | Content::Notif(_)
        );
    match style {
        NotchStyle::Island => {
            if expanded {
                cv.round_rect_shadow(xi, yi, wi, hi, s.r, 10.0, 22.0, 64);
            }
            if dynamic_glass {
                // Top camera header pure black, melting smoothly into smoked Dynamic Glass below.
                cv.round_rect_vgrad(
                    xi,
                    yi,
                    wi,
                    hi,
                    s.r,
                    Rgba::rgba(0, 0, 0, 255),
                    Rgba::rgba(14, 14, 18, 224),
                );
                cv.round_rect_glass_rim(
                    xi,
                    yi,
                    wi,
                    hi,
                    s.r,
                    Rgba::rgba(255, 255, 255, 58),
                    Rgba::rgba(255, 255, 255, 34),
                    Rgba::rgba(255, 255, 255, 16),
                );
            } else {
                cv.round_rect(xi, yi, wi, hi, s.r, pure_black);
                if expanded {
                    cv.stroke_round_rect(xi, yi, wi, hi, s.r, 1.0, Rgba::rgba(255, 255, 255, 22));
                }
            }
        }
        _ => {
            if dynamic_glass {
                cv.notch_body(xi, 0, wi, hi, s.r, Rgba::rgba(10, 10, 14, 232));
                cv.notch_body(xi, 0, wi, 36.min(hi), 16.0, pure_black);
            } else {
                cv.notch_body(xi, 0, wi, hi, s.r, pure_black);
            }
            cv.concave_corner(x - EAR, 0.0, EAR, true, pure_black);
            cv.concave_corner(x + w, 0.0, EAR, false, pure_black);
        }
    }

    let mut regions = Vec::new();
    // The whole shape toggles expansion; buttons drawn later take priority
    // because hit-testing walks regions last-first (see `pick`).
    regions.push(Region {
        x: xi,
        y: yi,
        w: wi,
        h: hi,
        action: HitAction::Shell(ShellCmd::ToggleIsland),
    });

    let settled = !anim.moving();
    let to = anim.to;
    // Draw content only once the shape is (nearly) its final size, so text
    // never spills outside it mid-animation.
    let fits = (s.w - to.w).abs() < 10.0 && (s.h - to.h).abs() < 10.0;
    if !(settled || fits) {
        return regions;
    }

    if expanded || matches!(content, Content::Spotlight { .. }) {
        let pad_x = if style == NotchStyle::Island { 22 } else { 18 };
        let pad_y = if style == NotchStyle::Island { 18 } else { 14 };
        let body = (
            xi + pad_x,
            yi + pad_y,
            (wi - 2 * pad_x).max(0),
            (hi - 2 * pad_y).max(0),
        );
        match content {
            Content::Shelf(items) => {
                shelf_body(cv, &th, fonts, body, items, hover, &mut regions);
            }
            Content::Widgets {
                media,
                pomodoro_secs,
                weather,
                cpu_pct,
                ram_pct,
                clipboard_preview,
            } => {
                widgets_shelf_body(
                    cv,
                    &th,
                    fonts,
                    body,
                    media.as_ref(),
                    *pomodoro_secs,
                    weather.as_deref(),
                    *cpu_pct,
                    *ram_pct,
                    clipboard_preview.as_deref(),
                    extra,
                    hover,
                    &mut regions,
                );
            }
            Content::Droplets { enabled_mask } => {
                droplets_picker_body(cv, &th, fonts, body, *enabled_mask, hover, &mut regions);
            }
            Content::Hud(hud) => {
                draw_hud_body(cv, &th, fonts, xi, yi, wi, hi, hud);
            }
            Content::Notif(n) => {
                notif_card_body(cv, &th, fonts, body, n, hover, &mut regions);
            }
            Content::Spotlight {
                query,
                selected,
                ask_mode,
                ask_reply,
            } => {
                spotlight_island_body(
                    cv,
                    &th,
                    fonts,
                    (xi, yi, wi, hi),
                    query,
                    *selected,
                    *ask_mode,
                    ask_reply.as_deref(),
                    hover,
                    &mut regions,
                );
            }
            _ => {
                expanded_body(cv, &th, fonts, body, content, extra, hover, &mut regions);
            }
        }
        // Droppy 3-page tabs centred at the top of the card: Home/Live, File Tray, Droplets.
        if matches!(
            content,
            Content::Idle
                | Content::Media(_)
                | Content::Shelf(_)
                | Content::Widgets { .. }
                | Content::Droplets { .. }
        ) {
            let active_page = match content {
                Content::Shelf(_) => 1,
                Content::Droplets { .. } => 2,
                _ => 0,
            };
            tabs(cv, xi, yi, wi, active_page, hover, &mut regions);
        }
    } else if let Content::Hud(hud) = content {
        draw_hud_body(cv, &th, fonts, xi, yi, wi, hi, hud);
    } else {
        compact_row(cv, &th, fonts, xi, yi, wi, hi, content, style);
    }
    regions
}

/// Three small round tabs centred at the top of the card:
/// 0 = Home / Now Playing & Widgets, 1 = File Tray, 2 = Droplets Picker.
fn tabs(
    cv: &mut Canvas<'_>,
    x: i32,
    y: i32,
    w: i32,
    active_page: u8,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let d = 22;
    let gap = 6;
    let top = y + 7;
    let left = x + (w - (3 * d + 2 * gap)) / 2;
    for page in 0u8..3u8 {
        let bx = left + page as i32 * (d + gap);
        let on = active_page == page;
        let hot = hovered(hover, bx, top, d, d);
        if on || hot {
            cv.round_rect(
                bx,
                top,
                d,
                d,
                d as f32 / 2.0,
                Rgba::rgba(255, 255, 255, if on { 44 } else { 22 }),
            );
        }
        let (cx, cy) = ((bx + d / 2) as f32, (top + d / 2) as f32);
        let ink = Rgba::rgba(255, 255, 255, if on { 255 } else { 150 });
        match page {
            0 => cv.icon_note(cx, cy, ink),
            1 => tray_glyph(cv, cx, cy, ink),
            _ => droplets_glyph(cv, cx, cy, ink),
        }
        regions.push(Region {
            x: bx,
            y: top,
            w: d,
            h: d,
            action: HitAction::Shell(ShellCmd::IslandPage(page)),
        });
    }
}

/// A 2x2 grid glyph for the Droplets / Widgets Picker tab.
fn droplets_glyph(cv: &mut Canvas<'_>, cx: f32, cy: f32, c: Rgba) {
    let (x, y) = (cx as i32 - 5, cy as i32 - 5);
    cv.round_rect(x, y, 4, 4, 1.0, c);
    cv.round_rect(x + 6, y, 4, 4, 1.0, c);
    cv.round_rect(x, y + 6, 4, 4, 1.0, c);
    cv.round_rect(x + 6, y + 6, 4, 4, 1.0, c);
}

/// An open tray: a shallow box with a raised lip.
fn tray_glyph(cv: &mut Canvas<'_>, cx: f32, cy: f32, c: Rgba) {
    let (x, y) = (cx as i32 - 7, cy as i32 - 4);
    cv.stroke_round_rect(x, y, 14, 9, 2.5, 1.4, c);
    cv.fill_rect(x + 4, y + 4, 6, 1, c);
}

/// The Droppy File Tray: up to eight parked files as tiles, plus inline action
/// pills (Zip, New Folder, Clear) and a Tracked `~/Downloads` chip.
fn shelf_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    items: &[std::path::PathBuf],
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    cv.text(fonts.semibold.as_ref(), "File Tray", bx, by + 15, 13.5, th.text);
    // Inline action pills at top-right (Droppy File Tray actions).
    let actions: &[(&str, ShellCmd)] = &[
        ("Zip", ShellCmd::ShelfZip),
        ("Folder", ShellCmd::ShelfNewFolder),
        ("Clear", ShellCmd::ShelfClear),
    ];
    let mut ax = bx + bw;
    if !items.is_empty() {
        for (label, cmd) in actions.iter().rev() {
            let pw = text_width(fonts.medium.as_ref(), label, 11.0) + 16;
            ax -= pw;
            let py = by + 1;
            let hot = hovered(hover, ax, py, pw, 22);
            cv.round_rect(
                ax,
                py,
                pw,
                22,
                11.0,
                Rgba::rgba(255, 255, 255, if hot { 44 } else { 22 }),
            );
            cv.text(
                fonts.medium.as_ref(),
                label,
                ax + 8,
                py + 15,
                11.0,
                Rgba::rgba(255, 255, 255, 230),
            );
            regions.push(Region {
                x: ax,
                y: py,
                w: pw,
                h: 22,
                action: HitAction::Shell(cmd.clone()),
            });
            ax -= 6;
        }
    }

    let top = by + 30;
    let footer_h = 24;
    let avail_h = (bh - 30 - footer_h).max(24);
    if items.is_empty() {
        let (dx, dy, dw, dh) = (bx, top, bw, avail_h);
        cv.stroke_round_rect(dx, dy, dw, dh, 14.0, 1.2, Rgba::rgba(255, 255, 255, 56));
        let msg = "Drop files here — zip, convert, or drag across spaces";
        let tw = text_width(fonts.medium.as_ref(), msg, 12.0);
        cv.text(
            fonts.medium.as_ref(),
            msg,
            dx + (dw - tw) / 2,
            dy + dh / 2 + 4,
            12.0,
            Rgba::rgba(255, 255, 255, 160),
        );
    } else {
        let cols = 4;
        let gap = 8;
        let tw = (bw - gap * (cols - 1)) / cols;
        let th_ = ((avail_h - gap) / 2).max(30);
        for (i, p) in items.iter().take(8).enumerate() {
            let (c, r) = ((i as i32) % cols, (i as i32) / cols);
            let (tx, ty) = (bx + c * (tw + gap), top + r * (th_ + gap));
            let hot = hovered(hover, tx, ty, tw, th_);
            cv.round_rect(
                tx,
                ty,
                tw,
                th_,
                10.0,
                Rgba::rgba(255, 255, 255, if hot { 38 } else { 22 }),
            );
            let col = file_tint(p);
            cv.round_rect(tx + 8, ty + (th_ - 22) / 2, 17, 22, 3.0, col);
            cv.fill_rect(
                tx + 11,
                ty + (th_ - 22) / 2 + 8,
                10,
                1,
                Rgba::rgba(255, 255, 255, 160),
            );
            cv.fill_rect(
                tx + 11,
                ty + (th_ - 22) / 2 + 12,
                8,
                1,
                Rgba::rgba(255, 255, 255, 160),
            );
            cv.text_truncated(
                fonts.medium.as_ref(),
                &crate::shelf::label(p),
                tx + 31,
                ty + th_ / 2 + 4,
                11.5,
                tw - 40,
                th.text,
            );
            regions.push(Region {
                x: tx,
                y: ty,
                w: tw,
                h: th_,
                action: HitAction::Launch(LaunchTarget::Shell(crate::shelf::open_command(p))),
            });
            if hot {
                let (xx, xy) = (tx + tw - 16, ty + 3);
                cv.circle(
                    (xx + 6) as f32,
                    (xy + 6) as f32,
                    6.5,
                    Rgba::rgba(255, 255, 255, 64),
                );
                cv.text(fonts.semibold.as_ref(), "×", xx + 2, xy + 10, 11.0, th.text);
                regions.push(Region {
                    x: xx - 2,
                    y: xy - 2,
                    w: 16,
                    h: 16,
                    action: HitAction::Shell(ShellCmd::ShelfRemove(i)),
                });
            }
        }
    }

    // Tracked folder footer strip (~/Downloads).
    let fy = by + bh - 18;
    let tracked = crate::shelf::tracked_downloads(2);
    let label = if tracked.is_empty() {
        "Tracked: ~/Downloads".to_string()
    } else {
        let names: Vec<String> = tracked.iter().map(|p| crate::shelf::label(p)).collect();
        format!("~/Downloads · {}", names.join(" · "))
    };
    cv.text_truncated(
        fonts.regular.as_ref(),
        &label,
        bx,
        fy + 12,
        11.0,
        bw,
        Rgba::rgba(255, 255, 255, 138),
    );
}

/// Droppy Home Shelf (Page 0): Now Playing + Pomodoro + Weather + System Stats
/// rolling sparkline + Agents + Clipboard preview.
#[allow(clippy::too_many_arguments)]
fn widgets_shelf_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    media: Option<&Media>,
    pomodoro_secs: Option<u32>,
    weather: Option<&str>,
    cpu_pct: u8,
    ram_pct: u8,
    clipboard_preview: Option<&str>,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    let top = by + 22;
    let col_gap = 10;
    let left_w = (bw * 52) / 100;
    let right_w = bw - left_w - col_gap;
    let card_h = bh - 24;

    // Left card: Now Playing (or Glance clock when nothing is playing).
    cv.round_rect(
        bx,
        top,
        left_w,
        card_h,
        16.0,
        Rgba::rgba(255, 255, 255, 18),
    );
    cv.stroke_round_rect(
        bx,
        top,
        left_w,
        card_h,
        16.0,
        1.0,
        Rgba::rgba(255, 255, 255, 24),
    );
    if let Some(m) = media {
        let art = 44;
        cv.draw_album_art(bx + 12, top + 12, art, 10.0);
        cv.text_truncated(
            fonts.semibold.as_ref(),
            if m.title.is_empty() { "Now Playing" } else { &m.title },
            bx + 66,
            top + 30,
            13.5,
            left_w - 78,
            th.text,
        );
        cv.text_truncated(
            fonts.regular.as_ref(),
            &m.artist,
            bx + 66,
            top + 47,
            11.5,
            left_w - 78,
            Rgba::rgba(255, 255, 255, 165),
        );
        // Progress bar
        let frac = match (m.position_us, m.length_us) {
            (Some(p), Some(l)) if l > 0 => (p as f32 / l as f32).clamp(0.0, 1.0),
            _ => 0.35,
        };
        let bar_x = bx + 12;
        let bar_y = top + 72;
        let bar_w = left_w - 24;
        cv.round_rect(bar_x, bar_y, bar_w, 4, 2.0, Rgba::rgba(255, 255, 255, 42));
        cv.round_rect(
            bar_x,
            bar_y,
            ((bar_w as f32 * frac).round() as i32).max(6),
            4,
            2.0,
            Rgba::rgba(255, 255, 255, 235),
        );
        // Transport controls
        let cy = (top + card_h - 26) as f32;
        let mid_x = (bx + left_w / 2) as f32;
        cv.icon_skip(mid_x - 42.0, cy, 14.0, false, Rgba::rgba(255, 255, 255, 215));
        if m.playing {
            cv.icon_pause(mid_x, cy, 16.0, th.text);
        } else {
            cv.icon_play(mid_x, cy, 16.0, th.text);
        }
        cv.icon_skip(mid_x + 42.0, cy, 14.0, true, Rgba::rgba(255, 255, 255, 215));
        for (cx_off, ctrl) in [
            (-42.0f32, Control::Previous),
            (0.0, Control::PlayPause),
            (42.0, Control::Next),
        ] {
            regions.push(Region {
                x: (mid_x + cx_off) as i32 - 16,
                y: cy as i32 - 14,
                w: 32,
                h: 28,
                action: HitAction::Media(ctrl),
            });
        }
    } else {
        cv.text_tracked(
            fonts.bold.as_ref(),
            extra.clock,
            bx + 16,
            top + 52,
            34.0,
            -0.8,
            th.text,
        );
        cv.text(
            fonts.medium.as_ref(),
            extra.date,
            bx + 16,
            top + 74,
            12.5,
            Rgba::rgba(255, 255, 255, 165),
        );
        if let Some(clip) = clipboard_preview {
            cv.text(
                fonts.semibold.as_ref(),
                "CLIPBOARD",
                bx + 16,
                top + 102,
                9.5,
                Rgba::rgba(255, 255, 255, 120),
            );
            cv.text_truncated(
                fonts.regular.as_ref(),
                clip,
                bx + 16,
                top + 118,
                11.5,
                left_w - 32,
                Rgba::rgba(255, 255, 255, 210),
            );
        }
    }

    // Right column: 2 stacked Droppy mini-widgets (Top: Pomodoro + Weather, Bottom: System Stats + Agents).
    let rx = bx + left_w + col_gap;
    let half_h = (card_h - 8) / 2;

    // Top-right: Pomodoro + Weather split
    let sub_w = (right_w - 8) / 2;
    let pomo_hot = hovered(hover, rx, top, sub_w, half_h);
    cv.round_rect(
        rx,
        top,
        sub_w,
        half_h,
        14.0,
        Rgba::rgba(255, 255, 255, if pomo_hot { 32 } else { 18 }),
    );
    cv.text(
        fonts.semibold.as_ref(),
        "Focus",
        rx + 10,
        top + 20,
        11.0,
        Rgba::rgb(0xff, 0x69, 0x61),
    );
    let pomo_str = match pomodoro_secs {
        Some(s) => format!("{:02}:{:02}", s / 60, s % 60),
        None => "25:00".to_string(),
    };
    cv.text(
        fonts.bold.as_ref(),
        &pomo_str,
        rx + 10,
        top + 44,
        18.0,
        th.text,
    );
    regions.push(Region {
        x: rx,
        y: top,
        w: sub_w,
        h: half_h,
        action: HitAction::Shell(ShellCmd::PomodoroToggle),
    });

    let wx = rx + sub_w + 8;
    cv.round_rect(wx, top, sub_w, half_h, 14.0, Rgba::rgba(36, 114, 232, 165));
    cv.text(
        fonts.semibold.as_ref(),
        "Weather",
        wx + 10,
        top + 20,
        11.0,
        Rgba::rgba(255, 255, 255, 210),
    );
    cv.text(
        fonts.bold.as_ref(),
        weather.unwrap_or("16°"),
        wx + 10,
        top + 44,
        18.0,
        th.text,
    );

    // Bottom-right: System Stats sparkline + Coding Agents badge
    let sy = top + half_h + 8;
    cv.round_rect(rx, sy, right_w, half_h, 14.0, Rgba::rgba(255, 255, 255, 18));
    let stats_line = format!("CPU {cpu_pct}%  ·  RAM {ram_pct}%");
    cv.text(
        fonts.semibold.as_ref(),
        &stats_line,
        rx + 10,
        sy + 20,
        11.0,
        Rgba::rgba(255, 255, 255, 215),
    );
    // Rolling sparkline bars
    let samples = [22u8, 34, 28, 46, 52, 40, cpu_pct.max(12), ram_pct.max(18), 38, 44, cpu_pct.max(14)];
    for (i, val) in samples.iter().enumerate() {
        let bar_h = ((*val as f32 / 100.0) * 20.0).round().max(3.0) as i32;
        let sx = rx + 10 + i as i32 * 9;
        cv.round_rect(
            sx,
            sy + half_h - 10 - bar_h,
            5,
            bar_h,
            2.0,
            Rgba::rgba(0x40, 0xc8, 0xe0, 210),
        );
    }
    let agents_str = if extra.glance.agents_running > 0 {
        format!("{} active", extra.glance.agents_running)
    } else {
        "Agents idle".into()
    };
    let aw = text_width(fonts.medium.as_ref(), &agents_str, 10.5);
    cv.text(
        fonts.medium.as_ref(),
        &agents_str,
        rx + right_w - aw - 10,
        sy + half_h - 14,
        10.5,
        th.agent_active,
    );
    regions.push(Region {
        x: rx,
        y: sy,
        w: right_w,
        h: half_h,
        action: HitAction::Shell(ShellCmd::ToggleAgents),
    });
}

/// Droppy Droplets Picker (Page 2): 2x5 toggleable grid of modular shelf widgets.
fn droplets_picker_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    enabled_mask: u16,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    cv.text(
        fonts.semibold.as_ref(),
        "Droplets",
        bx,
        by + 15,
        13.5,
        th.text,
    );
    cv.text(
        fonts.regular.as_ref(),
        "Click to toggle shelf widgets",
        bx + 70,
        by + 15,
        11.0,
        Rgba::rgba(255, 255, 255, 130),
    );
    let top = by + 28;
    let cols = 5;
    let gap = 7;
    let cw = (bw - gap * (cols - 1)) / cols;
    let ch = ((bh - 30 - gap) / 2).max(52);
    for (i, d) in DROPLETS.iter().enumerate() {
        let (c, r) = ((i as i32) % cols, (i as i32) / cols);
        let (cx, cy) = (bx + c * (cw + gap), top + r * (ch + gap));
        let on = (enabled_mask & (1u16 << d.id)) != 0;
        let hot = hovered(hover, cx, cy, cw, ch);
        let bg = if on {
            Rgba::rgba(th.accent.0, th.accent.1, th.accent.2, if hot { 135 } else { 96 })
        } else {
            Rgba::rgba(255, 255, 255, if hot { 32 } else { 16 })
        };
        cv.round_rect(cx, cy, cw, ch, 12.0, bg);
        if on {
            cv.stroke_round_rect(
                cx,
                cy,
                cw,
                ch,
                12.0,
                1.0,
                Rgba::rgba(255, 255, 255, 58),
            );
        }
        cv.circle(
            (cx + cw - 11) as f32,
            (cy + 11) as f32,
            3.2,
            if on {
                Rgba::rgb(0x34, 0xc7, 0x59)
            } else {
                Rgba::rgba(255, 255, 255, 56)
            },
        );
        cv.text_truncated(
            fonts.semibold.as_ref(),
            d.name,
            cx + 8,
            cy + 26,
            11.0,
            cw - 16,
            th.text,
        );
        cv.text_truncated(
            fonts.regular.as_ref(),
            d.summary,
            cx + 8,
            cy + 42,
            9.5,
            cw - 14,
            Rgba::rgba(255, 255, 255, 165),
        );
        regions.push(Region {
            x: cx,
            y: cy,
            w: cw,
            h: ch,
            action: HitAction::Shell(ShellCmd::ToggleDroplet(d.id)),
        });
    }
}

/// Droppy System HUD (Volume / Brightness / Keyboard Backlight) growing out of the island.
fn draw_hud_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    hud: &HudKind,
) {
    let mid = y as f32 + h as f32 / 2.0;
    let (pct, label) = match hud {
        HudKind::Volume(v) => (*v, format!("{v}%")),
        HudKind::Brightness(v) => (*v, format!("{v}%")),
        HudKind::KeyboardBacklight(v) => (*v, format!("{v}%")),
    };
    let icon_x = (x + 18) as f32;
    match hud {
        HudKind::Volume(v) => cv.icon_volume(icon_x, mid, 14.0, *v, th.text),
        HudKind::Brightness(_) | HudKind::KeyboardBacklight(_) => cv.icon_sun(icon_x, mid, th.text),
    }
    let lw = text_width(fonts.semibold.as_ref(), &label, 11.5);
    let bar_x = x + 36;
    let bar_w = (w - 36 - lw - 24).max(40);
    let bar_y = y + (h - 6) / 2;
    cv.round_rect(bar_x, bar_y, bar_w, 6, 3.0, Rgba::rgba(255, 255, 255, 44));
    let fill_w = ((bar_w as f32 * (pct.min(100) as f32 / 100.0)).round() as i32).max(6);
    cv.round_rect(bar_x, bar_y, fill_w, 6, 3.0, Rgba::rgba(255, 255, 255, 240));
    cv.text(
        fonts.semibold.as_ref(),
        &label,
        x + w - lw - 14,
        y + h / 2 + 4,
        11.5,
        Rgba::rgba(255, 255, 255, 220),
    );
}

/// Interactive Notification Card landing directly in the Dynamic Island.
fn notif_card_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    n: &NotifCard,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    cv.round_rect(bx, by + 6, 36, 36, 10.0, th.accent);
    cv.icon_bell((bx + 18) as f32, (by + 24) as f32, th.on_accent);
    cv.text_truncated(
        fonts.semibold.as_ref(),
        &format!("{} · {}", n.app, n.title),
        bx + 48,
        by + 22,
        13.0,
        bw - 128,
        th.text,
    );
    cv.text_truncated(
        fonts.regular.as_ref(),
        &n.body,
        bx + 48,
        by + 42,
        12.0,
        bw - 128,
        Rgba::rgba(255, 255, 255, 185),
    );
    let r = (bx + bw - 72, by + (bh - 28) / 2, 72, 28);
    button(cv, th, fonts, r, "Dismiss", false, hover);
    regions.push(Region {
        x: r.0,
        y: r.1,
        w: r.2,
        h: r.3,
        action: HitAction::Shell(ShellCmd::Notify(String::new(), String::new())),
    });
}

/// Apple-style Spotlight integrated directly into the Dynamic Island (New MacBook Pro architecture):
/// Pure-black camera pill header seamlessly melting into a Dynamic Glass "Search or Ask" bar,
/// Thunderstorm results (Apps, Files, Calculator/Units, Window Snap, Commands), and Ask AI mode.
#[allow(clippy::too_many_arguments)]
fn spotlight_island_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    card: (i32, i32, i32, i32),
    query: &str,
    selected: usize,
    ask_mode: bool,
    ask_reply: Option<&str>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (cx, cy, cw, ch) = card;
    // Camera pill indicator centered at the very top of the Dynamic Island Spotlight.
    let cam_w = 58;
    let cam_h = 14;
    cv.round_rect(
        cx + (cw - cam_w) / 2,
        cy + 8,
        cam_w,
        cam_h,
        7.0,
        Rgba::rgb(0, 0, 0),
    );
    // Subtle green/blue privacy/active dot beside camera.
    cv.circle(
        (cx + (cw + cam_w) / 2 + 8) as f32,
        (cy + 15) as f32,
        2.5,
        if ask_mode {
            Rgba::rgb(0x34, 0xc7, 0x59)
        } else {
            th.accent
        },
    );

    // Search or Ask input capsule right beneath the camera pill.
    let bar_x = cx + 18;
    let bar_y = cy + 32;
    let bar_w = cw - 36;
    let bar_h = 46;
    cv.round_rect(
        bar_x,
        bar_y,
        bar_w,
        bar_h,
        16.0,
        Rgba::rgba(255, 255, 255, 22),
    );
    cv.stroke_round_rect(
        bar_x,
        bar_y,
        bar_w,
        bar_h,
        16.0,
        1.0,
        Rgba::rgba(255, 255, 255, 40),
    );
    cv.icon_search((bar_x + 22) as f32, (bar_y + bar_h / 2) as f32, 15.0, Rgba::rgba(255, 255, 255, 210));

    let text_x = bar_x + 44;
    let baseline = bar_y + bar_h / 2 + 6;
    if query.is_empty() {
        let placeholder = if ask_mode {
            "Ask Solyra AI anything…"
        } else {
            "Search or Ask  (apps, files, math, units, snap…)"
        };
        cv.text_truncated(
            fonts.regular.as_ref(),
            placeholder,
            text_x,
            baseline,
            16.0,
            bar_w - 130,
            Rgba::rgba(255, 255, 255, 130),
        );
    } else {
        cv.text_truncated(
            fonts.medium.as_ref(),
            query,
            text_x,
            baseline,
            17.0,
            bar_w - 130,
            th.text,
        );
        let qw = text_width(fonts.medium.as_ref(), query, 17.0).min(bar_w - 134);
        cv.round_rect(text_x + qw + 2, bar_y + 11, 2, 24, 1.0, th.accent);
    }

    // "Ask" mode toggle chip on the right side of the search bar (Tab).
    let chip_label = if ask_mode { "Ask AI" } else { "Ask ⇥" };
    let chip_w = text_width(fonts.semibold.as_ref(), chip_label, 11.5) + 18;
    let chip_x = bar_x + bar_w - chip_w - 10;
    let chip_y = bar_y + (bar_h - 26) / 2;
    let chip_hot = hovered(hover, chip_x, chip_y, chip_w, 26);
    cv.round_rect(
        chip_x,
        chip_y,
        chip_w,
        26,
        13.0,
        if ask_mode {
            th.accent
        } else if chip_hot {
            Rgba::rgba(255, 255, 255, 42)
        } else {
            Rgba::rgba(255, 255, 255, 24)
        },
    );
    cv.text(
        fonts.semibold.as_ref(),
        chip_label,
        chip_x + 9,
        chip_y + 17,
        11.5,
        if ask_mode { th.on_accent } else { th.text },
    );
    regions.push(Region {
        x: chip_x,
        y: chip_y,
        w: chip_w,
        h: 26,
        action: HitAction::Shell(ShellCmd::SpotlightToggleAsk),
    });

    // If compact (empty query & not in Ask mode), stop here.
    let res_y = bar_y + bar_h + 12;
    let avail_h = (cy + ch - res_y - 16).max(0);
    if avail_h < 40 {
        return;
    }

    if ask_mode {
        // Integrated Ask AI response area inside the Dynamic Island.
        cv.round_rect(
            bar_x,
            res_y,
            bar_w,
            avail_h,
            16.0,
            Rgba::rgba(255, 255, 255, 14),
        );
        cv.text(
            fonts.semibold.as_ref(),
            "SOLYRA AI · ISLAND ASSISTANT",
            bar_x + 16,
            res_y + 24,
            10.5,
            th.agent_active,
        );
        let body_text = ask_reply.unwrap_or(
            "Press Enter to send your question directly to the local coding agent or assistant without leaving your current window.",
        );
        let mut ty = res_y + 48;
        for line in wrap_text(fonts.regular.as_ref(), body_text, 13.5, bar_w - 32, 9) {
            cv.text(fonts.regular.as_ref(), &line, bar_x + 16, ty, 13.5, th.text);
            ty += 21;
        }
        return;
    }

    // Inline Thunderstorm calculator / converter highlight banner if query evaluates.
    let mut y = res_y;
    if let Some((result_val, expr_desc)) = crate::launcher::eval_quick_math(query) {
        cv.round_rect(bar_x, y, bar_w, 48, 12.0, Rgba::rgba(th.accent.0, th.accent.1, th.accent.2, 72));
        cv.stroke_round_rect(bar_x, y, bar_w, 48, 12.0, 1.0, Rgba::rgba(255, 255, 255, 42));
        cv.text(
            fonts.bold.as_ref(),
            &result_val,
            bar_x + 16,
            y + 24,
            18.0,
            th.text,
        );
        cv.text(
            fonts.regular.as_ref(),
            &expr_desc,
            bar_x + 16,
            y + 40,
            11.0,
            Rgba::rgba(255, 255, 255, 185),
        );
        y += 56;
    }

    // Ranked Spotlight items (Apps, Files, Open Windows, Snap/System Commands, Agent).
    let apps = crate::state::catalog_apps();
    let items = crate::launcher::search(query, &apps, &[], true);
    let row_h = 36;
    let mut idx = 0usize;
    let mut last_heading: Option<&str> = None;
    for item in items.iter().take(6) {
        if y + row_h > cy + ch - 12 {
            break;
        }
        let heading = item.kind.heading();
        if last_heading != Some(heading) {
            cv.text(
                fonts.semibold.as_ref(),
                heading,
                bar_x + 8,
                y + 12,
                11.0,
                Rgba::rgba(255, 255, 255, 140),
            );
            y += 18;
            last_heading = Some(heading);
        }
        if y + row_h > cy + ch - 12 {
            break;
        }
        let is_sel = idx == selected;
        let hot = hovered(hover, bar_x, y, bar_w, row_h);
        if is_sel {
            cv.round_rect(bar_x, y, bar_w, row_h, 10.0, th.accent);
        } else if hot {
            cv.round_rect(bar_x, y, bar_w, row_h, 10.0, Rgba::rgba(255, 255, 255, 24));
        }
        // Small icon badge
        if let Some(icon) = crate::icons::name_for(&item.role)
            .or_else(|| crate::icons::name_for(&item.app_id))
            .and_then(crate::icons::get)
        {
            cv.draw_icon(bar_x + 10, y + 6, 24, icon);
        } else {
            cv.round_rect(bar_x + 10, y + 6, 24, 24, 6.0, Rgba::rgba(255, 255, 255, 36));
        }
        let fg = if is_sel { th.on_accent } else { th.text };
        let sub_fg = if is_sel {
            Rgba::rgba(255, 255, 255, 215)
        } else {
            Rgba::rgba(255, 255, 255, 145)
        };
        let tw = text_width(fonts.semibold.as_ref(), &item.title, 13.5);
        cv.text_truncated(
            fonts.semibold.as_ref(),
            &item.title,
            bar_x + 42,
            y + 23,
            13.5,
            (bar_w / 2).max(120),
            fg,
        );
        cv.text_truncated(
            fonts.regular.as_ref(),
            &item.subtitle,
            bar_x + 42 + tw.min(bar_w / 2) + 10,
            y + 23,
            11.5,
            (bar_w - 60 - tw.min(bar_w / 2)).max(40),
            sub_fg,
        );
        let action = match &item.action {
            crate::launcher::Action::Launch(t) => Some(HitAction::Launch(t.clone())),
            crate::launcher::Action::FocusWindow(id) => Some(HitAction::FocusWindow(*id)),
            crate::launcher::Action::AgentRequest(q) => Some(HitAction::AgentRequest(q.clone())),
            crate::launcher::Action::Unavailable => None,
        };
        if let Some(act) = action {
            regions.push(Region {
                x: bar_x,
                y,
                w: bar_w,
                h: row_h,
                action: act,
            });
        }
        y += row_h + 2;
        idx += 1;
    }
}

fn file_tint(p: &std::path::Path) -> Rgba {
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => Rgba::rgb(0xe5, 0x48, 0x4d),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => Rgba::rgb(0x34, 0xa8, 0x53),
        "mp3" | "flac" | "ogg" | "wav" | "m4a" => Rgba::rgb(0xf2, 0x55, 0x8a),
        "mp4" | "mkv" | "webm" | "mov" => Rgba::rgb(0x9b, 0x59, 0xd0),
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" => Rgba::rgb(0xb0, 0x8a, 0x5a),
        "" if p.is_dir() => Rgba::rgb(0x3b, 0x9b, 0xf2),
        _ => Rgba::rgb(0x8e, 0x8e, 0x93),
    }
}

/// Live audio bars in soft white, like the Now Playing Live Activity.
fn media_waveform(cv: &mut Canvas<'_>, x: i32, mid: f32, active: bool) {
    for (i, bh) in [7, 13, 9, 14, 6].into_iter().enumerate() {
        let h = if active { bh } else { 4 };
        let a = if active { 236 } else { 96 };
        cv.round_rect(x + i as i32 * 4, (mid as i32) - h / 2, 2, h, 1.0, Rgba::rgba(255, 255, 255, a));
    }
}

fn agent_waveform(cv: &mut Canvas<'_>, x: i32, mid: f32, color: Rgba) {
    for (i, bh) in [6, 11, 8, 13, 6].into_iter().enumerate() {
        cv.round_rect(x + i as i32 * 4, (mid as i32) - bh / 2, 2, bh, 1.0, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn compact_row(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    content: &Content,
    style: NotchStyle,
) {
    let mid = y as f32 + h as f32 / 2.0;
    let baseline = y + h / 2 + 4;
    let pad = if style == NotchStyle::Island { 10 } else { 14 };
    match content {
        Content::Idle | Content::Shelf(_) | Content::Droplets { .. } | Content::Spotlight { .. } => {}
        Content::Hud(hud) => {
            draw_hud_body(cv, th, fonts, x, y, w, h, hud);
        }
        Content::Notif(n) => {
            cv.icon_bell((x + pad + 8) as f32, mid, th.accent);
            let tx = x + pad + 22;
            let max_tw = (w - pad * 2 - 26).max(20);
            cv.text_truncated(
                fonts.medium.as_ref(),
                &format!("{}: {}", n.app, n.title),
                tx,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
        }
        Content::Widgets { media: Some(m), .. } => {
            let art = (h - 10).clamp(18, 22);
            cv.draw_album_art(x + pad, y + (h - art) / 2, art, 5.5);
            let bars_w = 18;
            let bars_x = x + w - pad - bars_w - 2;
            media_waveform(cv, bars_x, mid, m.playing);
            let line = if m.title.is_empty() { &m.artist } else { &m.title };
            let tx = x + pad + art + 9;
            let max_tw = (bars_x - tx - 10).max(20);
            let tw = text_width(fonts.semibold.as_ref(), line, 12.2).min(max_tw);
            let cx = tx + (max_tw - tw) / 2;
            cv.text_truncated(fonts.semibold.as_ref(), line, cx, baseline, 12.2, max_tw, th.text);
        }
        Content::Widgets { media: None, .. } => {}
        Content::Message(t) => {
            // Snug leading icon + crisp title.
            cv.icon_bell((x + pad + 8) as f32, mid, th.text);
            let tx = x + pad + 22;
            let max_tw = (w - pad * 2 - 26).max(20);
            cv.text_truncated(
                fonts.medium.as_ref(),
                t,
                tx,
                baseline,
                12.2,
                max_tw,
                th.text,
            );
        }
        Content::Activity { who, text } => {
            // Snug leading live indicator + centered activity + snug trailing waveform.
            let dot_x = (x + pad + 7) as f32;
            cv.circle(
                dot_x,
                mid,
                6.0,
                Rgba::rgba(th.agent_active.0, th.agent_active.1, th.agent_active.2, 56),
            );
            cv.circle(dot_x, mid, 3.6, th.agent_active);

            let bars_w = 18;
            let bars_x = x + w - pad - bars_w - 2;
            agent_waveform(cv, bars_x, mid, th.agent_active);

            let line = format!("{who} · {text}");
            let tx = x + pad + 20;
            let max_tw = (bars_x - tx - 10).max(24);
            let tw = text_width(fonts.medium.as_ref(), &line, 12.0).min(max_tw);
            let cx = tx + (max_tw - tw) / 2;
            cv.text_truncated(
                fonts.medium.as_ref(),
                &line,
                cx,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
        }
        Content::Attention(a) => {
            let icon_x = (x + pad + 7) as f32;
            cv.ring(icon_x, mid, 4.8, 1.8, th.agent_approval);
            cv.circle(icon_x, mid, 1.6, th.agent_approval);

            let chip = if a.sent { "Sent" } else { "Review" };
            let chip_w = text_width(fonts.semibold.as_ref(), chip, 10.5) + 14;
            let chip_x = x + w - pad - chip_w;
            let chip_y = y + (h - 20) / 2;
            cv.round_rect(chip_x, chip_y, chip_w, 20, 10.0, th.agent_approval);
            cv.text(
                fonts.semibold.as_ref(),
                chip,
                chip_x + 7,
                chip_y + 14,
                10.5,
                th.on_accent,
            );

            let what = match a.kind {
                AskKind::Permission => "Approval",
                AskKind::Question => "Question",
            };
            let line = format!("{what} · {}", a.who);
            let tx = x + pad + 20;
            let max_tw = (chip_x - tx - 10).max(24);
            cv.text_truncated(
                fonts.medium.as_ref(),
                &line,
                tx,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
        }
        Content::Media(m) => {
            // Iconic Dynamic Island compact media: concentric album art on left,
            // clean title in center, vibrant live waveform snugly on right.
            let art = 21;
            let art_x = x + 8;
            let art_y = (mid as i32) - art / 2;
            cv.draw_album_art(art_x, art_y, art, 6.0);

            let bars_w = 18;
            let trail_x = x + w - 12 - bars_w;
            if m.playing {
                media_waveform(cv, trail_x, mid, true);
            } else {
                cv.icon_pause((trail_x + 9) as f32, mid, 10.0, Rgba::rgba(255, 255, 255, 160));
            }

            let line = if m.title.is_empty() {
                m.artist.clone()
            } else {
                m.title.clone()
            };
            let tx = art_x + art + 10;
            let max_tw = (trail_x - tx - 10).max(24);
            let tw = text_width(fonts.medium.as_ref(), &line, 12.2).min(max_tw);
            let cx = tx + (max_tw - tw) / 2;
            cv.text_truncated(
                fonts.medium.as_ref(),
                &line,
                cx,
                baseline,
                12.2,
                max_tw,
                th.text,
            );
        }
    }
}

fn button(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    rect: (i32, i32, i32, i32),
    label: &str,
    primary: bool,
    hover: Option<(f64, f64)>,
) {
    let (x, y, w, h) = rect;
    let fill = if primary {
        th.accent
    } else if hovered(hover, x, y, w, h) {
        Rgba::rgba(255, 255, 255, 44)
    } else {
        Rgba::rgba(255, 255, 255, 26)
    };
    cv.round_rect(x, y, w, h, h as f32 / 2.0, fill);
    let tw = text_width(fonts.semibold.as_ref(), label, T_LABEL);
    let fg = if primary { th.on_accent } else { th.text };
    cv.text(
        fonts.semibold.as_ref(),
        label,
        x + (w - tw).max(0) / 2,
        y + h / 2 + 4,
        T_LABEL,
        fg,
    );
}

/// Evenly split `n` buttons across `w` at the bottom of the body.
fn button_row(x: i32, y: i32, w: i32, n: i32) -> Vec<(i32, i32, i32, i32)> {
    let gap = 8;
    let bw = (w - gap * (n - 1)) / n.max(1);
    (0..n).map(|i| (x + i * (bw + gap), y, bw, 32)).collect()
}

#[allow(clippy::too_many_arguments)]
fn expanded_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    content: &Content,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    let buttons_y = by + bh - 32;
    match content {
        // Drawn by dedicated Droppy body painters.
        Content::Shelf(_)
        | Content::Widgets { .. }
        | Content::Droplets { .. }
        | Content::Hud(_)
        | Content::Notif(_)
        | Content::Spotlight { .. } => {}
        Content::Idle => {
            // Crisp Apple glance: bold tracked clock, medium date, quiet facts.
            cv.text_tracked(
                fonts.bold.as_ref(),
                extra.clock,
                bx,
                by + 34,
                36.0,
                -0.8,
                th.text,
            );
            cv.text(
                fonts.medium.as_ref(),
                extra.date,
                bx,
                by + 56,
                13.0,
                Rgba::rgba(255, 255, 255, 168),
            );
            let g = extra.glance;
            let mut facts = Vec::new();
            if let Some(b) = g.battery {
                facts.push(format!("{b}%"));
            }
            if !g.network.is_empty() {
                facts.push(g.network.clone());
            }
            if g.agents_running > 0 {
                let word = if g.agents_running == 1 {
                    "agent"
                } else {
                    "agents"
                };
                facts.push(format!("{} {word}", g.agents_running));
            }
            if g.windows > 0 {
                facts.push(format!("{} windows", g.windows));
            }
            if !facts.is_empty() {
                cv.text_truncated(
                    fonts.medium.as_ref(),
                    &facts.join("   ·   "),
                    bx,
                    by + 82,
                    12.0,
                    bw,
                    Rgba::rgba(255, 255, 255, 148),
                );
            }
        }
        Content::Message(t) => {
            let mut y = by + 16;
            for l in wrap_text(fonts.medium.as_ref(), t, T_BODY, bw, 4) {
                cv.text(fonts.medium.as_ref(), &l, bx, y, T_BODY, th.text);
                y += 19;
            }
            let r = (bx + bw - 110, buttons_y, 110, 32);
            button(cv, th, fonts, r, "Dismiss", false, hover);
            regions.push(Region {
                x: r.0,
                y: r.1,
                w: r.2,
                h: r.3,
                action: HitAction::Shell(ShellCmd::Notify(String::new(), String::new())),
            });
        }
        Content::Activity { who, text } => {
            cv.text_tracked(
                fonts.semibold.as_ref(),
                &who.to_uppercase(),
                bx,
                by + 12,
                T_CAPTION,
                0.5,
                th.agent_active,
            );
            let mut y = by + 34;
            for l in wrap_text(fonts.medium.as_ref(), text, T_BODY, bw, 3) {
                cv.text(fonts.medium.as_ref(), &l, bx, y, T_BODY, th.text);
                y += 19;
            }
            let r = (bx + bw - 148, buttons_y, 148, 32);
            button(cv, th, fonts, r, "Open agent tasks", false, hover);
            regions.push(Region {
                x: r.0,
                y: r.1,
                w: r.2,
                h: r.3,
                action: HitAction::Shell(ShellCmd::ToggleAgents),
            });
        }
        Content::Attention(a) => attention_body(cv, th, fonts, body, a, extra, hover, regions),
        Content::Media(m) => {
            // Concentric Apple Dynamic Island Media Live Activity:
            // Top row: 54x54 vibrant album art (concentric curve) + Bold title /
            // Medium artist + colorful top-right waveform.
            let art = 54;
            cv.draw_album_art(bx, by, art, 15.0);

            let wave_w = 20;
            let wave_x = bx + bw - wave_w - 2;
            media_waveform(cv, wave_x, by as f32 + art as f32 * 0.48, m.playing);

            let tx = bx + art + 14;
            let tw = (wave_x - tx - 14).max(0);
            cv.text_truncated(
                fonts.bold.as_ref(),
                &m.title,
                tx,
                by + 23,
                16.0,
                tw,
                Rgba::rgb(255, 255, 255),
            );
            cv.text_truncated(
                fonts.medium.as_ref(),
                &m.artist,
                tx,
                by + 43,
                13.5,
                tw,
                Rgba::rgba(255, 255, 255, 152),
            );

            // Middle row: inline Apple scrubber (`0:15  [====----]  3:23`).
            if let Some((elapsed, total, pct)) = m.elapsed_label() {
                let row_y = by + art + 18;
                let time_col = Rgba::rgba(255, 255, 255, 148);
                let ew = text_width(fonts.medium.as_ref(), &elapsed, 11.2);
                let total_w = text_width(fonts.medium.as_ref(), &total, 11.2);
                cv.text(fonts.medium.as_ref(), &elapsed, bx, row_y + 4, 11.2, time_col);
                cv.text(
                    fonts.medium.as_ref(),
                    &total,
                    bx + bw - total_w,
                    row_y + 4,
                    11.2,
                    time_col,
                );
                let bar_x = bx + ew + 10;
                let bar_w = (bw - ew - total_w - 20).max(20);
                let bar_h = 5;
                let bar_y = row_y - 2;
                cv.round_rect(
                    bar_x,
                    bar_y,
                    bar_w,
                    bar_h,
                    2.5,
                    Rgba::rgba(255, 255, 255, 42),
                );
                // Click anywhere on the bar to jump there (pearos-notch's
                // seekable scrubber): one thin region per slice.
                if let Some(len) = m.length_us.filter(|l| *l > 0) {
                    let n = crate::media::SEEK_STEPS;
                    for i in 0..n {
                        let x0 = bar_x + bar_w * i / n;
                        let x1 = bar_x + bar_w * (i + 1) / n;
                        regions.push(Region {
                            x: x0,
                            y: bar_y - 7,
                            w: (x1 - x0).max(1),
                            h: bar_h + 14,
                            action: HitAction::Media(Control::SeekTo(crate::media::seek_target(i, n, len))),
                        });
                    }
                }
                let fill = ((bar_w as f32) * pct).round() as i32;
                if fill > 0 {
                    cv.round_rect(
                        bar_x,
                        bar_y,
                        fill.max(5),
                        bar_h,
                        2.5,
                        Rgba::rgb(255, 255, 255),
                    );
                }
            }

            // Bottom row: centered SF-Symbols-grade transport controls.
            let ctrl_h = 34;
            let ctrl_y = by + bh - ctrl_h + 4;
            let btn = 42;
            let gap = 26;
            let row_w = btn * 3 + gap * 2;
            let start = bx + (bw - row_w) / 2;
            let cy = ctrl_y + ctrl_h / 2;
            let ctrls = [Control::Previous, Control::PlayPause, Control::Next];
            for (i, c) in ctrls.into_iter().enumerate() {
                let cx0 = start + i as i32 * (btn + gap);
                let r = (cx0, ctrl_y, btn, ctrl_h);
                if hovered(hover, r.0, r.1, r.2, r.3) {
                    cv.round_rect(r.0, r.1, r.2, r.3, 17.0, Rgba::rgba(255, 255, 255, 28));
                }
                let cx = cx0 + btn / 2;
                match c {
                    Control::Previous => {
                        cv.icon_skip(cx as f32, cy as f32, 19.0, false, Rgba::rgb(255, 255, 255))
                    }
                    Control::Next => {
                        cv.icon_skip(cx as f32, cy as f32, 19.0, true, Rgba::rgb(255, 255, 255))
                    }
                    Control::PlayPause if m.playing => {
                        cv.icon_pause(cx as f32, cy as f32, 22.0, Rgba::rgb(255, 255, 255))
                    }
                    Control::PlayPause => {
                        cv.icon_play(cx as f32, cy as f32, 22.0, Rgba::rgb(255, 255, 255))
                    }
                    Control::SeekTo(_) => {}
                }
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: HitAction::Media(c),
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn attention_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    a: &Ask,
    extra: &Extra<'_>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    let buttons_y = by + bh - 32;
    let head = match a.kind {
        AskKind::Permission => format!("{} WANTS TO", a.who.to_uppercase()),
        AskKind::Question => format!("{} ASKS", a.who.to_uppercase()),
    };
    cv.text_tracked(
        fonts.semibold.as_ref(),
        &head,
        bx,
        by + 12,
        T_CAPTION,
        0.5,
        th.agent_approval,
    );
    let mut y = by + 32;
    for l in wrap_text(fonts.semibold.as_ref(), &a.text, T_BODY, bw, 2) {
        cv.text(fonts.semibold.as_ref(), &l, bx, y, T_BODY, th.text);
        y += 18;
    }
    if a.kind == AskKind::Permission {
        let res = a.resource.as_deref().unwrap_or("Not reported by the agent");
        cv.text_truncated(
            fonts.regular.as_ref(),
            &format!("Affects: {res}"),
            bx,
            y + 2,
            T_META,
            bw,
            th.text_med,
        );
        let rev = match a.reversible {
            Some(true) => "Can be undone: yes, as reported",
            Some(false) => "Can be undone: no",
            None => "Can be undone: not reported",
        };
        cv.text_truncated(
            fonts.regular.as_ref(),
            rev,
            bx,
            y + 18,
            T_META,
            bw,
            th.text_dim,
        );
    }
    if a.sent {
        cv.text(
            fonts.regular.as_ref(),
            "Sent. Waiting for the agent.",
            bx,
            buttons_y + 20,
            T_LABEL,
            th.text_med,
        );
        return;
    }
    match a.kind {
        AskKind::Permission => {
            let n = if a.task_scope { 3 } else { 2 };
            let row = button_row(bx, buttons_y, bw, n);
            let mut acts = vec![
                (
                    "Deny (D)",
                    HitAction::Decide {
                        task: a.task,
                        verdict: Verdict::Deny,
                        scope: Scope::Once,
                    },
                    false,
                ),
                (
                    "Allow once (A)",
                    HitAction::Decide {
                        task: a.task,
                        verdict: Verdict::Approve,
                        scope: Scope::Once,
                    },
                    false,
                ),
            ];
            if a.task_scope {
                acts.push((
                    "This task (T)",
                    HitAction::Decide {
                        task: a.task,
                        verdict: Verdict::Approve,
                        scope: Scope::Task,
                    },
                    false,
                ));
            }
            for (r, (label, act, primary)) in row.into_iter().zip(acts) {
                button(cv, th, fonts, r, label, primary, hover);
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: act,
                });
            }
        }
        AskKind::Question if !a.choices.is_empty() => {
            let n = a.choices.len().min(4) as i32;
            let row = button_row(bx, buttons_y, bw, n);
            for (i, (r, c)) in row.into_iter().zip(&a.choices).enumerate() {
                button(cv, th, fonts, r, &format!("{} {c}", i + 1), false, hover);
                regions.push(Region {
                    x: r.0,
                    y: r.1,
                    w: r.2,
                    h: r.3,
                    action: HitAction::Choose {
                        task: a.task,
                        choice: i,
                    },
                });
            }
        }
        AskKind::Question => {
            let (fx, fw) = (bx, bw);
            cv.round_rect(fx, buttons_y, fw, 32, 16.0, Rgba::rgba(255, 255, 255, 22));
            if extra.typing {
                cv.stroke_round_rect(fx, buttons_y, fw, 32, 16.0, 1.0, th.agent_approval);
            }
            if extra.reply.is_empty() {
                cv.text(
                    fonts.regular.as_ref(),
                    "Type your answer, Enter to send",
                    fx + 14,
                    buttons_y + 21,
                    T_LABEL,
                    th.text_dim,
                );
            } else {
                let mut shown = extra.reply.to_string();
                while text_width(fonts.regular.as_ref(), &shown, T_LABEL) > fw - 30
                    && shown.chars().count() > 1
                {
                    shown.remove(0);
                }
                let tw = cv.text(
                    fonts.regular.as_ref(),
                    &shown,
                    fx + 14,
                    buttons_y + 21,
                    T_LABEL,
                    th.text,
                );
                if extra.typing {
                    cv.fill_rect(fx + 15 + tw, buttons_y + 8, 2, 16, th.agent_approval);
                }
            }
            regions.push(Region {
                x: fx,
                y: buttons_y,
                w: fw,
                h: 32,
                action: HitAction::StartReply,
            });
        }
    }
}

/// Region under (`x`, `y`), the most specific (last drawn) first.
pub fn pick(regions: &[Region], x: f64, y: f64) -> Option<HitAction> {
    regions
        .iter()
        .rev()
        .find(|r| r.contains(x, y))
        .map(|r| r.action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_grow_with_content_and_expansion_and_fit_the_surface() {
        for style in [NotchStyle::Notch, NotchStyle::Island] {
            let idle = target(style, &Content::Idle, false);
            let busy = target(style, &Content::Message("x".into()), false);
            let big = target(style, &Content::Idle, true);
            assert!(busy.w > idle.w);
            assert!(big.h > busy.h);
            for s in [idle, busy, big] {
                assert!(
                    s.w <= SURF_W as f32 - 2.0 * EAR && s.y + s.h <= SURF_H as f32,
                    "{style:?} {s:?}"
                );
            }
        }
        assert_eq!(
            target(NotchStyle::Notch, &Content::Idle, false).y,
            0.0,
            "a notch hangs from the edge"
        );
        assert!(
            target(NotchStyle::Island, &Content::Idle, false).y > 0.0,
            "an island floats"
        );
    }

    #[test]
    fn island_targets_match_live_activity_proportions() {
        use crate::media::Media;
        let media = Content::Media(Media {
            bus: "x".into(),
            title: "Clair de Lune".into(),
            artist: "Debussy".into(),
            playing: true,
            position_us: Some(15_000_000),
            length_us: Some(203_000_000),
        });
        let idle = target(NotchStyle::Island, &Content::Idle, false);
        // pearOS idle island: 170 x 30, 5 px below the top edge.
        assert_eq!((idle.w, idle.h, idle.y), (170.0, 30.0, 5.0));
        assert!(
            (idle.r - idle.h / 2.0).abs() < 0.6,
            "idle island is a capsule"
        );
        let busy = target(NotchStyle::Island, &media, false);
        assert_eq!(busy.h, idle.h, "a live activity keeps the pill's height");
        assert!(busy.w > idle.w, "and widens");
        let open = target(NotchStyle::Island, &media, true);
        assert_eq!(open.w, 440.0);
        assert!(
            (130.0..=160.0).contains(&open.h),
            "expanded media height {}",
            open.h
        );
        assert_eq!(open.r, 44.0);
        let glance = target(NotchStyle::Island, &Content::Idle, true);
        assert_eq!(glance.r, 44.0);
        assert!(glance.h < open.h);
        assert!(glance.h > busy.h);
    }

    #[test]
    fn animation_settles_and_reduced_motion_snaps() {
        let a = target(NotchStyle::Notch, &Content::Idle, false);
        let b = target(NotchStyle::Notch, &Content::Idle, true);
        let mut an = Anim::new(a);
        an.to = b;
        let mut frames = 0;
        while an.step(1.0 / 60.0, false) {
            frames += 1;
            assert!(frames < 120, "never settles");
        }
        assert_eq!(an.cur, b);
        assert!(frames > 5, "it actually animates ({frames} frames)");
        let mut snap = Anim::new(a);
        snap.to = b;
        assert!(!snap.step(1.0 / 60.0, true));
        assert_eq!(snap.cur, b);
    }

    #[test]
    fn later_regions_win_hit_tests() {
        let regions = vec![
            Region {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
                action: HitAction::Shell(ShellCmd::ToggleIsland),
            },
            Region {
                x: 10,
                y: 10,
                w: 10,
                h: 10,
                action: HitAction::Shell(ShellCmd::ToggleAgents),
            },
        ];
        assert!(matches!(
            pick(&regions, 15.0, 15.0),
            Some(HitAction::Shell(ShellCmd::ToggleAgents))
        ));
        assert!(matches!(
            pick(&regions, 50.0, 50.0),
            Some(HitAction::Shell(ShellCmd::ToggleIsland))
        ));
        assert!(pick(&regions, 500.0, 5.0).is_none());
    }
}
