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

/// The island's surface; the shape + detached mode-switcher pill never exceed it.
pub const SURF_W: u32 = 680;
pub const SURF_H: u32 = 400;
/// Concave fillet radius joining a notch to the top edge.
/// Droppy's notch wings: the top corners curve outward 10 px.
const EAR: f32 = 10.0;

/// What the island is showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Idle,
    Message(String),
    Activity {
        who: String,
        text: String,
    },
    Attention(Ask),
    Media(Media),
    /// Droppy expanded media player with synced live lyrics drawer (Reference 2).
    MediaLyrics {
        media: Media,
        /// `(relative_offset, line_text)` where `0` is the active sung line.
        lyrics: Vec<(isize, String)>,
    },
    /// The shelf tab: files parked on the notch (newest first).
    Shelf(Vec<std::path::PathBuf>),
    /// Droppy rich clipboard manager (Reference 4).
    Clipboard {
        items: Vec<crate::clipboard::ClipItem>,
        filter: crate::clipboard::ClipCategory,
        /// Optional open right-click context menu on card index.
        context_menu: Option<usize>,
    },
    /// Droppy modular Droplets tab (Quick Notes, Pomodoro timer, quick status).
    Droplets {
        note: String,
        pomodoro_secs: u32,
        pomodoro_running: bool,
    },
    /// A system HUD (volume, brightness, charging, Focus, unlock) for a moment.
    Hud(crate::hud::Hud),
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
/// on the island). Idle is a short glance. Anything with buttons keeps more
/// height.
fn expanded_shape(style: NotchStyle, content: &Content) -> Shape {
    let island = style == NotchStyle::Island;
    let (w, h, r) = match content {
        Content::Media(_) => (332.0, 146.0, if island { 34.0 } else { 26.0 }),
        Content::MediaLyrics { .. } => (320.0, 316.0, if island { 34.0 } else { 28.0 }),
        Content::Clipboard { .. } => (628.0, 224.0, if island { 30.0 } else { 26.0 }),
        Content::Shelf(_) | Content::Droplets { .. } => (468.0, 174.0, if island { 34.0 } else { 28.0 }),
        Content::Hud(_) => (300.0, 37.0, 18.0),
        Content::Idle => (if island { 348.0 } else { 392.0 }, 132.0, if island { 36.0 } else { 28.0 }),
        _ => (if island { 440.0 } else { 448.0 }, 196.0, if island { 40.0 } else { 30.0 }),
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
    let line = if m.title.is_empty() {
        &m.artist
    } else {
        &m.title
    };
    let text = line
        .chars()
        .map(|c| if c.is_uppercase() { 7.8 } else { 6.3 })
        .sum::<f32>();
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
                    // Droppy's Dynamic Island: a 210 x 37 pill floating 4 px
                    // below the top edge; live activities keep that height.
                    Content::Idle
                    | Content::Shelf(_)
                    | Content::Clipboard { .. }
                    | Content::Droplets { .. } => Shape {
                        w: 136.0,
                        h: 30.0,
                        y: 4.0,
                        r: 15.0,
                    },
                    Content::Hud(_) => Shape {
                        w: 300.0,
                        h: 30.0,
                        y: 4.0,
                        r: 15.0,
                    },
                    Content::Media(m) | Content::MediaLyrics { media: m, .. } => Shape {
                        w: compact_media_width(m, 240.0, 320.0),
                        h: 30.0,
                        y: 4.0,
                        r: 15.0,
                    },
                    Content::Activity { .. } | Content::Message(_) => Shape {
                        w: 320.0,
                        h: 30.0,
                        y: 4.0,
                        r: 15.0,
                    },
                    Content::Attention(_) => Shape {
                        w: 340.0,
                        h: 30.0,
                        y: 4.0,
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
                    // Droppy notch: 180 px, rounded 16 px at the bottom.
                    Content::Idle
                    | Content::Shelf(_)
                    | Content::Clipboard { .. }
                    | Content::Droplets { .. } => Shape {
                        w: 140.0,
                        h: 26.0,
                        y: 0.0,
                        r: 13.0,
                    },
                    Content::Hud(_) => Shape {
                        w: 320.0,
                        h: 26.0,
                        y: 0.0,
                        r: 13.0,
                    },
                    Content::Media(m) | Content::MediaLyrics { media: m, .. } => Shape {
                        w: compact_media_width(m, 212.0, 300.0),
                        h: 30.0,
                        y: 0.0,
                        r: 15.0,
                    },
                    _ => Shape {
                        w: 312.0,
                        h: 30.0,
                        y: 0.0,
                        r: 15.0,
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
        // Critically damped spring (ω ≈ 18): no overshoot, settles in ~120–180 ms
        // for a 6 px hover widen and ~250 ms for a full expansion.
        let k = 1.0 - (-dt * 18.0).exp();
        let lerp = |a: f32, b: f32| a + (b - a) * k;
        self.cur = Shape {
            w: lerp(self.cur.w, self.to.w),
            h: lerp(self.cur.h, self.to.h),
            y: lerp(self.cur.y, self.to.y),
            r: lerp(self.cur.r, self.to.r),
        };
        let close = |a: f32, b: f32| (a - b).abs() < 0.4;
        if close(self.cur.w, self.to.w)
            && close(self.cur.h, self.to.h)
            && close(self.cur.y, self.to.y)
            && close(self.cur.r, self.to.r)
        {
            self.cur = self.to;
            false
        } else {
            true
        }
    }

    /// 0 at rest far from the target, 1 when settled. Content fades in after 0.6.
    pub fn morph_t(&self) -> f32 {
        let remain = (self.cur.w - self.to.w)
            .abs()
            .max((self.cur.h - self.to.h).abs())
            .max((self.cur.r - self.to.r).abs());
        let span = self.to.w.max(self.to.h).max(48.0);
        (1.0 - (remain / span).clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    pub fn moving(&self) -> bool {
        self.cur != self.to
    }
}

/// The shape's rectangle in surface pixels, for the input region.
/// Includes room below an expanded card for Droppy's detached mode-switcher pill.
pub fn input_rect(s: &Shape) -> (i32, i32, i32, i32) {
    let extra_h = if s.h > 80.0 { 44 } else { 0 };
    (
        s.x().floor() as i32,
        s.y.floor() as i32,
        s.w.ceil() as i32,
        (s.h.ceil() as i32 + extra_h).min(SURF_H as i32),
    )
}

/// Where the compositor should blur behind a glass island / notch: the
/// shape itself (for the notch, the body that runs up past the top edge,
/// clipped to the surface).
pub fn glass_rects(style: NotchStyle, s: &Shape) -> Vec<(i32, i32, i32, i32)> {
    let (x, w) = (s.x().round() as i32, s.w.round() as i32);
    let rects = if style == NotchStyle::Island {
        crate::material::blur_rects(x, s.y.round() as i32, w, s.h.round() as i32, s.r)
    } else {
        let lift = s.r.ceil() as i32 + 2;
        let mut v = crate::material::blur_rects(x, -lift, w, s.h.round() as i32 + lift, s.r);
        // The ears beside the notch.
        v.push((x - EAR as i32, 0, EAR as i32, EAR as i32));
        v.push((x + w, 0, EAR as i32, EAR as i32));
        v
    };
    rects
        .into_iter()
        .filter_map(|(rx, ry, rw, rh)| {
            let top = ry.max(0);
            let h = ry + rh - top;
            (h > 0 && rw > 0).then_some((rx, top, rw, h))
        })
        .collect()
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
    /// Smoked Liquid Glass over the compositor's blur instead of hardware black.
    pub glass: bool,
    /// A lower-priority live activity shown as a small indicator, never dropped.
    pub badge: Option<LiveBadge>,
}

/// Compact secondary activity while a higher-priority card owns the island.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveBadge {
    Media,
    Activity,
    Message,
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

    // Solid: hardware black. Glass: smoked Liquid Glass (dark enough that
    // white text and art always read, clear enough that the desktop shows
    // through, blurred by the compositor).
    let pure_black = Rgba::rgb(0, 0, 0);
    match (style, extra.glass) {
        (NotchStyle::Island, glass) => {
            if expanded || glass {
                cv.round_rect_shadow(
                    xi,
                    yi,
                    wi,
                    hi,
                    s.r,
                    8.0,
                    18.0,
                    if expanded { 64 } else { 40 },
                );
            }
            if glass {
                crate::material::smoked_glass(cv, xi, yi, wi, hi, s.r);
            } else {
                cv.round_rect(xi, yi, wi, hi, s.r, pure_black);
            }
            // Near-black, softly reflective: a rim light keeps the island
            // perceptible over dark wallpaper without reading as a border.
            cv.stroke_round_rect(xi, yi, wi, hi, s.r, 1.0, Rgba::rgba(255, 255, 255, 42));
        }
        (_, true) => {
            // Flush with the top edge: the glass body runs up past the
            // screen edge so only its lower rim and corners show.
            let lift = s.r.ceil() as i32 + 2;
            cv.round_rect_shadow(xi, -lift, wi, hi + lift, s.r, 8.0, 18.0, 44);
            crate::material::smoked_glass(cv, xi, -lift, wi, hi + lift, s.r);
            cv.stroke_round_rect(xi, -lift, wi, hi + lift, s.r, 1.0, Rgba::rgba(255, 255, 255, 42));
            let ear = Rgba::rgba(10, 10, 14, 150);
            cv.concave_corner(x - EAR, 0.0, EAR, true, ear);
            cv.concave_corner(x + w, 0.0, EAR, false, ear);
        }
        (_, false) => {
            cv.notch_body(xi, 0, wi, hi, s.r, pure_black);
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
    // Content cross-fades in the last ~40 % of a morph so glyphs never sit
    // on a shrinking pill. Reduced motion snaps, so this is always 1.
    if anim.morph_t() < 0.6 && !settled {
        return regions;
    }
    if let Some(badge) = extra.badge {
        let bx = xi + wi - 14;
        let by = yi + 10;
        let col = match badge {
            LiveBadge::Media => Rgba::rgb(0x32, 0xd7, 0x4b),
            LiveBadge::Activity => Rgba::rgb(0x0a, 0x84, 0xff),
            LiveBadge::Message => Rgba::rgb(0xff, 0x9f, 0x0a),
        };
        cv.circle(bx as f32, by as f32, 3.5, col);
    }

    if expanded {
        let pad_x = if matches!(content, Content::Clipboard { .. }) {
            16
        } else if style == NotchStyle::Island {
            20
        } else {
            18
        };
        let pad_y = if matches!(content, Content::Clipboard { .. }) {
            14
        } else {
            16
        };
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
            Content::Clipboard {
                items,
                filter,
                context_menu,
            } => {
                clipboard_body(
                    cv,
                    &th,
                    fonts,
                    body,
                    items,
                    *filter,
                    *context_menu,
                    hover,
                    &mut regions,
                );
            }
            Content::Droplets {
                note,
                pomodoro_secs,
                pomodoro_running,
            } => {
                droplets_body(
                    cv,
                    &th,
                    fonts,
                    body,
                    note,
                    *pomodoro_secs,
                    *pomodoro_running,
                    extra,
                    hover,
                    &mut regions,
                );
            }
            _ => {
                expanded_body(cv, &th, fonts, body, content, extra, hover, &mut regions);
            }
        }
        // Droppy's detached floating mode-switcher pill directly beneath the notch
        // (Home/Media, Shelf, Clipboard, Droplets).
        if matches!(
            content,
            Content::Idle
                | Content::Media(_)
                | Content::MediaLyrics { .. }
                | Content::Shelf(_)
                | Content::Clipboard { .. }
                | Content::Droplets { .. }
        ) {
            let active_tab = match content {
                Content::Shelf(_) => 1,
                Content::Clipboard { .. } => 2,
                Content::Droplets { .. } => 3,
                _ => 0,
            };
            mode_switcher_pill(cv, fonts, xi + wi / 2, yi + hi + 10, active_tab, hover, &mut regions);
        }
    } else {
        compact_row(cv, &th, fonts, xi, yi, wi, hi, content, style);
    }
    regions
}

/// A HUD inside the island: glyph on the left, a level bar, the value on
/// the right (Droppy's notch HUDs).
#[allow(clippy::too_many_arguments)]
fn hud_row(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    x: i32,
    w: i32,
    mid: f32,
    baseline: i32,
    pad: i32,
    hd: &crate::hud::Hud,
) {
    use crate::hud::HudKind;
    let white = Rgba::rgb(255, 255, 255);
    let gx = (x + pad + 12) as f32;
    match hd.kind {
        HudKind::Volume => cv.icon_volume(
            gx,
            mid,
            14.0,
            if hd.on { 0 } else { hd.level.unwrap_or(50) },
            white,
        ),
        HudKind::Brightness => cv.icon_sun(gx, mid, white),
        HudKind::Charging => cv.icon_battery(
            gx - 2.0,
            mid,
            hd.level.unwrap_or(0),
            white,
            if hd.on { th.success } else { white },
        ),
        HudKind::Focus => cv.icon_moon(
            gx,
            mid,
            if hd.on {
                Rgba::rgb(0xbf, 0x5a, 0xf2)
            } else {
                white
            },
        ),
        HudKind::Unlocked => padlock(cv, gx, mid, true, white),
    }
    let label = crate::hud::label(hd);
    let lw = text_width(fonts.semibold.as_ref(), &label, 12.5);
    let lx = x + w - pad - 6 - lw;
    cv.text(fonts.semibold.as_ref(), &label, lx, baseline, 12.5, white);
    if crate::hud::has_bar(hd) {
        let bx = x + pad + 32;
        let bw = (lx - 12 - bx).max(20);
        let by = mid as i32 - 2;
        cv.round_rect(bx, by, bw, 4, 2.0, Rgba::rgba(255, 255, 255, 54));
        let f = (bw as f32 * hd.level.unwrap_or(0) as f32 / 100.0).round() as i32;
        if f > 0 {
            cv.round_rect(bx, by, f.max(4), 4, 2.0, white);
        }
    }
}

/// A padlock, closed or with its shackle swung open (the iPhone unlock).
pub fn padlock(cv: &mut Canvas<'_>, cx: f32, cy: f32, open: bool, c: Rgba) {
    let (bw, bh) = (12, 9);
    let (bx, by) = (cx as i32 - bw / 2, cy as i32 - 1);
    cv.round_rect(bx, by, bw, bh, 2.0, c);
    // Shackle: an arch over the body; open lifts and shifts it right.
    let (sx, sy) = if open {
        (cx + 4.0, cy - 7.0)
    } else {
        (cx, cy - 4.0)
    };
    cv.ring(sx, sy, 4.0, 1.8, c);
    if !open {
        cv.fill_rect(bx + 2, by - 4, 2, 4, c);
        cv.fill_rect(bx + bw - 4, by - 4, 2, 4, c);
    } else {
        cv.fill_rect(bx + bw - 4, by - 4, 2, 4, c);
    }
}

/// Droppy's detached floating mode-switcher pill centered directly beneath the
/// expanded notch (Home/Media, File Shelf, Clipboard, Droplets).
#[allow(clippy::too_many_arguments)]
fn mode_switcher_pill(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    cx: i32,
    top: i32,
    active_tab: u8,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let pw = 128;
    let ph = 30;
    let px = cx - pw / 2;
    let r = ph as f32 / 2.0;
    cv.round_rect_shadow(px, top, pw, ph, r, 4.0, 14.0, 56);
    cv.round_rect(px, top, pw, ph, r, Rgba::rgb(0, 0, 0));
    cv.stroke_round_rect(px, top, pw, ph, r, 1.0, Rgba::rgba(255, 255, 255, 42));

    let btn_w = 26;
    let btn_h = 22;
    let gap = 4;
    let start_x = px + (pw - (4 * btn_w + 3 * gap)) / 2;
    let by = top + (ph - btn_h) / 2;
    for i in 0..4u8 {
        let bx = start_x + i as i32 * (btn_w + gap);
        let on = active_tab == i;
        let hot = hovered(hover, bx, by, btn_w, btn_h);
        if on || hot {
            cv.round_rect(
                bx,
                by,
                btn_w,
                btn_h,
                btn_h as f32 / 2.0,
                Rgba::rgba(255, 255, 255, if on { 46 } else { 22 }),
            );
        }
        let (icx, icy) = ((bx + btn_w / 2) as f32, (by + btn_h / 2) as f32);
        let ink = Rgba::rgba(255, 255, 255, if on { 255 } else { 155 });
        match i {
            0 => {
                // Home / Media icon
                cv.text(fonts.semibold.as_ref(), "⌂", bx + 7, by + 16, 13.0, ink);
            }
            1 => {
                // File Shelf / Tray icon
                tray_glyph(cv, icx, icy, ink);
            }
            2 => {
                // Clipboard icon
                cv.stroke_round_rect(bx + 8, by + 5, 10, 12, 2.0, 1.3, ink);
                cv.fill_rect(bx + 10, by + 4, 6, 2, ink);
            }
            _ => {
                // Droplets / Widgets 2x2 grid icon
                for dx in [0, 6] {
                    for dy in [0, 6] {
                        cv.round_rect(bx + 8 + dx, by + 6 + dy, 4, 4, 1.0, ink);
                    }
                }
            }
        }
        regions.push(Region {
            x: bx,
            y: top,
            w: btn_w,
            h: ph,
            action: HitAction::Shell(ShellCmd::IslandTab(i)),
        });
    }
}

/// Droppy Rich Clipboard Manager (matches Reference 4): category filter pills,
/// horizontal rich cards (Text, Link, Color, Video), and optional dark context menu.
#[allow(clippy::too_many_arguments)]
fn clipboard_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    items: &[crate::clipboard::ClipItem],
    filter: crate::clipboard::ClipCategory,
    context_menu: Option<usize>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    use crate::clipboard::{ClipCategory, ClipKind};
    let (bx, by, bw, bh) = body;

    // Top row: category filter pills ("Clipboard", orange "Work", green "Personal") + expand circle.
    let mut px = bx;
    let pill_h = 26;
    for cat in ClipCategory::TABS {
        let on = filter == cat;
        let label = cat.label();
        let tw = text_width(fonts.semibold.as_ref(), label, 12.0);
        let pw = tw + 30;
        let bg = if on {
            Rgba::rgba(0x0a, 0x84, 0xff, 64)
        } else {
            Rgba::rgba(255, 255, 255, 20)
        };
        let fg = if on {
            Rgba::rgb(0x40, 0x9c, 0xff)
        } else {
            Rgba::rgba(255, 255, 255, 210)
        };
        cv.round_rect(px, by, pw, pill_h, 13.0, bg);
        if on {
            cv.stroke_round_rect(px, by, pw, pill_h, 13.0, 1.0, Rgba::rgba(0x40, 0x9c, 0xff, 110));
        }
        let dot_col = match cat {
            ClipCategory::All => Rgba::rgb(0x40, 0x9c, 0xff),
            ClipCategory::Work => Rgba::rgb(0xff, 0x9f, 0x0a),
            ClipCategory::Personal => Rgba::rgb(0x32, 0xd7, 0x4b),
        };
        cv.circle((px + 12) as f32, (by + pill_h / 2) as f32, 4.0, dot_col);
        cv.text(fonts.semibold.as_ref(), label, px + 22, by + 17, 12.0, fg);
        regions.push(Region {
            x: px,
            y: by,
            w: pw,
            h: pill_h,
            action: HitAction::Shell(ShellCmd::ClipboardFilter(cat)),
        });
        px += pw + 8;
    }
    // Top-right circular expand button (drawn vector diagonal arrows — no missing font glyph).
    let ex = bx + bw - 26;
    draw_expand_button(cv, ex, by);

    // Horizontal rich cards row.
    let card_y = by + pill_h + 12;
    let card_h = (bh - pill_h - 14).max(120);
    let visible: Vec<(usize, &crate::clipboard::ClipItem)> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| filter == ClipCategory::All || it.category == filter)
        .take(4)
        .collect();
    if visible.is_empty() {
        let msg = "Copy text, links, or #hex colors — they appear here";
        let tw = text_width(fonts.medium.as_ref(), msg, 12.5);
        cv.text(
            fonts.medium.as_ref(),
            msg,
            bx + (bw - tw).max(0) / 2,
            card_y + card_h / 2,
            12.5,
            Rgba::rgba(255, 255, 255, 140),
        );
        return;
    }
    let gap = 10;
    let card_w = ((bw - gap * 3) / 4).max(120);
    let mut menu_anchor: Option<(i32, i32, usize)> = None;
    for (slot, (orig_idx, item)) in visible.into_iter().enumerate() {
        let cx = bx + slot as i32 * (card_w + gap);
        let hot = hovered(hover, cx, card_y, card_w, card_h);
        match item.kind {
            ClipKind::Color(r, g, b) => {
                cv.round_rect(cx, card_y, card_w, card_h, 16.0, Rgba::rgb(r, g, b));
                let luma = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
                let ink = if luma > 140 {
                    Rgba::rgb(0x18, 0x18, 0x1a)
                } else {
                    Rgba::rgb(255, 255, 255)
                };
                // Top badge pill ("Color · now")
                cv.round_rect(cx + 8, card_y + 8, 84, 20, 10.0, Rgba::rgba(255, 255, 255, 220));
                cv.circle((cx + 17) as f32, (card_y + 18) as f32, 4.0, Rgba::rgb(r, g, b));
                cv.text(
                    fonts.semibold.as_ref(),
                    &format!("Color · {}", item.age),
                    cx + 25,
                    card_y + 22,
                    9.8,
                    Rgba::rgb(0x18, 0x18, 0x1a),
                );
                // Top-right `...` circle
                cv.circle((cx + card_w - 18) as f32, (card_y + 18) as f32, 10.0, Rgba::rgba(255, 255, 255, 220));
                cv.text(fonts.bold.as_ref(), "···", cx + card_w - 25, card_y + 21, 10.5, Rgba::rgb(0x18, 0x18, 0x1a));
                // Centered hex
                let hw = text_width(fonts.bold.as_ref(), &item.text, 15.5);
                cv.text(
                    fonts.bold.as_ref(),
                    &item.text,
                    cx + (card_w - hw).max(8) / 2,
                    card_y + card_h / 2 + 6,
                    15.5,
                    ink,
                );
                // RGB footer
                let foot = item.footer();
                let fw = text_width(fonts.medium.as_ref(), &foot, 10.2);
                cv.text(
                    fonts.medium.as_ref(),
                    &foot,
                    cx + (card_w - fw).max(8) / 2,
                    card_y + card_h - 12,
                    10.2,
                    Rgba::rgba(ink.0, ink.1, ink.2, 195),
                );
            }
            ClipKind::Video => {
                cv.round_rect_vgrad(
                    cx,
                    card_y,
                    card_w,
                    card_h,
                    16.0,
                    Rgba::rgb(0x3a, 0x1c, 0x71),
                    Rgba::rgb(0x14, 0x14, 0x1c),
                );
                cv.stroke_round_rect(cx, card_y, card_w, card_h, 16.0, 1.0, Rgba::rgba(255, 255, 255, 28));
                cv.round_rect(cx + 8, card_y + 8, 78, 20, 10.0, Rgba::rgba(255, 255, 255, 220));
                cv.text(
                    fonts.semibold.as_ref(),
                    "Video · clip",
                    cx + 15,
                    card_y + 22,
                    9.8,
                    Rgba::rgb(0x18, 0x18, 0x1a),
                );
                cv.circle((cx + card_w / 2) as f32, (card_y + card_h / 2 + 4) as f32, 16.0, Rgba::rgba(255, 255, 255, 42));
                cv.icon_play((cx + card_w / 2 + 1) as f32, (card_y + card_h / 2 + 4) as f32, 14.0, Rgba::rgb(255, 255, 255));
                cv.text_truncated(
                    fonts.regular.as_ref(),
                    &item.text,
                    cx + 10,
                    card_y + card_h - 12,
                    10.2,
                    card_w - 20,
                    Rgba::rgba(255, 255, 255, 165),
                );
            }
            _ => {
                let bg = if hot {
                    Rgba::rgb(0x1e, 0x1e, 0x22)
                } else {
                    Rgba::rgb(0x14, 0x14, 0x17)
                };
                cv.round_rect(cx, card_y, card_w, card_h, 16.0, bg);
                cv.stroke_round_rect(cx, card_y, card_w, card_h, 16.0, 1.0, Rgba::rgba(255, 255, 255, 24));
                // Top-left white pill badge (`Text · 14 min. ago` / `Link · 6 min. ago`)
                let badge_txt = format!("{} · {}", item.kind.label(), item.age);
                let bw_p = (text_width(fonts.semibold.as_ref(), &badge_txt, 9.6) + 22).min(card_w - 50);
                cv.round_rect(cx + 8, card_y + 8, bw_p, 20, 10.0, Rgba::rgba(255, 255, 255, 232));
                let dot_c = if matches!(item.kind, ClipKind::Link) {
                    Rgba::rgb(0x0a, 0x84, 0xff)
                } else {
                    Rgba::rgb(0x32, 0xd7, 0x4b)
                };
                cv.circle((cx + 16) as f32, (card_y + 18) as f32, 4.0, dot_c);
                cv.text_truncated(
                    fonts.semibold.as_ref(),
                    &badge_txt,
                    cx + 23,
                    card_y + 21,
                    9.6,
                    bw_p - 18,
                    Rgba::rgb(0x18, 0x18, 0x1a),
                );
                // Top-right pin/star + `...` pill
                cv.round_rect(cx + card_w - 40, card_y + 8, 32, 20, 10.0, Rgba::rgba(255, 255, 255, 232));
                let mark_col = if item.pinned {
                    Rgba::rgb(0xff, 0x45, 0x3a)
                } else if item.favorite {
                    Rgba::rgb(0xff, 0xb8, 0x00)
                } else {
                    Rgba::rgb(0x6e, 0x6e, 0x73)
                };
                cv.circle((cx + card_w - 31) as f32, (card_y + 18) as f32, 3.2, mark_col);
                cv.text(fonts.bold.as_ref(), "··", cx + card_w - 23, card_y + 21, 10.0, Rgba::rgb(0x18, 0x18, 0x1a));

                if matches!(item.kind, ClipKind::Link) {
                    // Centered glossy blue link favicon circle
                    cv.circle((cx + card_w / 2) as f32, (card_y + card_h / 2 + 2) as f32, 16.0, Rgba::rgb(0x28, 0x45, 0xd6));
                    cv.circle((cx + card_w / 2) as f32, (card_y + card_h / 2 + 2) as f32, 9.0, Rgba::rgba(255, 255, 255, 190));
                } else {
                    let mut ty = card_y + 48;
                    for line in wrap_text(fonts.regular.as_ref(), &item.text, 11.2, card_w - 20, 4) {
                        cv.text(fonts.regular.as_ref(), &line, cx + 10, ty, 11.2, th.text);
                        ty += 15;
                    }
                }
                let foot = item.footer();
                let fw = text_width(fonts.regular.as_ref(), &foot, 10.0).min(card_w - 16);
                cv.text_truncated(
                    fonts.regular.as_ref(),
                    &foot,
                    cx + (card_w - fw) / 2,
                    card_y + card_h - 12,
                    10.0,
                    card_w - 16,
                    Rgba::rgba(255, 255, 255, 130),
                );
            }
        }
        if context_menu == Some(slot) {
            menu_anchor = Some((cx + card_w / 2, card_y + card_h / 2 + 6, orig_idx));
        }
        regions.push(Region {
            x: cx,
            y: card_y,
            w: card_w,
            h: card_h,
            action: HitAction::Shell(ShellCmd::ClipboardCopy(orig_idx)),
        });
    }

    // Native dark translucent context menu (matches Reference 4).
    if let Some((mx, my, idx)) = menu_anchor {
        let mw = 178;
        let mh = 138;
        let mx = mx.min(bx + bw - mw);
        let my = my.min(by + bh + 48 - mh);
        cv.round_rect_shadow(mx, my, mw, mh, 12.0, 8.0, 24.0, 90);
        cv.round_rect(mx, my, mw, mh, 12.0, Rgba::rgba(36, 38, 46, 242));
        cv.stroke_round_rect(mx, my, mw, mh, 12.0, 1.0, Rgba::rgba(255, 255, 255, 48));
        let rows: [(&str, bool, HitAction); 5] = [
            ("Copy", true, HitAction::Shell(ShellCmd::ClipboardCopy(idx))),
            ("Pin", false, HitAction::Shell(ShellCmd::ClipboardPin(idx))),
            ("Add to favorites", false, HitAction::Shell(ShellCmd::ClipboardFav(idx))),
            ("Open Clipboard", false, HitAction::Shell(ShellCmd::IslandTab(2))),
            ("Delete from history", false, HitAction::Shell(ShellCmd::ClipboardDelete(idx))),
        ];
        let mut ry = my + 6;
        for (lbl, highlight, act) in rows {
            if highlight {
                cv.round_rect(mx + 6, ry, mw - 12, 24, 6.0, Rgba::rgb(0x0a, 0x84, 0xff));
            }
            cv.text(
                fonts.medium.as_ref(),
                lbl,
                mx + 14,
                ry + 16,
                11.8,
                Rgba::rgb(255, 255, 255),
            );
            regions.push(Region {
                x: mx + 6,
                y: ry,
                w: mw - 12,
                h: 24,
                action: act,
            });
            ry += 25;
        }
    }
}

/// Droppy Droplets tab: Quick Notes, Pomodoro focus timer, and live Agent glance.
#[allow(clippy::too_many_arguments)]
fn droplets_body(
    cv: &mut Canvas<'_>,
    th: &Theme,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    note: &str,
    pomodoro_secs: u32,
    pomodoro_running: bool,
    extra: &Extra<'_>,
    _hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    cv.text(fonts.semibold.as_ref(), "Droplets", bx, by + 14, 14.0, th.text);
    let top = by + 26;
    let card_h = (bh - 28).max(80);
    let gap = 10;
    let cw = (bw - gap * 2) / 3;

    // 1. Quick Notes
    cv.round_rect(bx, top, cw, card_h, 14.0, Rgba::rgba(255, 255, 255, 18));
    cv.text(fonts.semibold.as_ref(), "Quick Note", bx + 12, top + 20, 11.8, Rgba::rgb(0xff, 0xd6, 0x0a));
    let mut ny = top + 40;
    for l in wrap_text(fonts.regular.as_ref(), note, 11.2, cw - 24, 3) {
        cv.text(fonts.regular.as_ref(), &l, bx + 12, ny, 11.2, th.text_med);
        ny += 15;
    }

    // 2. Pomodoro Focus Timer
    let x2 = bx + cw + gap;
    cv.round_rect(x2, top, cw, card_h, 14.0, Rgba::rgba(255, 255, 255, 18));
    cv.text(fonts.semibold.as_ref(), "Focus Timer", x2 + 12, top + 20, 11.8, Rgba::rgb(0x32, 0xd7, 0x4b));
    let timer_str = format!("{:02}:{:02}", pomodoro_secs / 60, pomodoro_secs % 60);
    cv.text(fonts.bold.as_ref(), &timer_str, x2 + 12, top + 56, 24.0, th.text);
    cv.text(
        fonts.regular.as_ref(),
        if pomodoro_running { "In focus session" } else { "Ready · 25 min" },
        x2 + 12,
        top + card_h - 14,
        10.8,
        th.text_dim,
    );

    // 3. Agents / Command Center
    let x3 = x2 + cw + gap;
    cv.round_rect(x3, top, cw, card_h, 14.0, Rgba::rgba(255, 255, 255, 18));
    cv.text(fonts.semibold.as_ref(), "Agents", x3 + 12, top + 20, 11.8, th.accent);
    let ag = format!("{} active", extra.glance.agents_running);
    cv.text(fonts.bold.as_ref(), &ag, x3 + 12, top + 54, 18.0, th.text);
    cv.text(fonts.regular.as_ref(), "Open dashboard →", x3 + 12, top + card_h - 14, 10.8, th.text_dim);
    regions.push(Region {
        x: x3,
        y: top,
        w: cw,
        h: card_h,
        action: HitAction::Shell(ShellCmd::ToggleAgents),
    });
}

/// An open tray: a shallow box with a raised lip.
fn tray_glyph(cv: &mut Canvas<'_>, cx: f32, cy: f32, c: Rgba) {
    let (x, y) = (cx as i32 - 7, cy as i32 - 4);
    cv.stroke_round_rect(x, y, 14, 9, 2.5, 1.4, c);
    cv.fill_rect(x + 4, y + 4, 6, 1, c);
}

/// The shelf: up to eight parked files as tiles; click opens, × forgets.
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
    cv.text(fonts.semibold.as_ref(), "Shelf", bx, by + 14, 14.0, th.text);
    let top = by + 28;
    let avail_h = (bh - 28).max(20);
    if items.is_empty() {
        let (dx, dy, dw, dh) = (bx, top, bw, avail_h);
        cv.stroke_round_rect(dx, dy, dw, dh, 14.0, 1.2, Rgba::rgba(255, 255, 255, 60));
        let msg = "Drop files here to keep them close";
        let tw = text_width(fonts.medium.as_ref(), msg, 12.5);
        cv.text(
            fonts.medium.as_ref(),
            msg,
            dx + (dw - tw) / 2,
            dy + dh / 2 + 4,
            12.5,
            Rgba::rgba(255, 255, 255, 150),
        );
        return;
    }
    let cols = 4;
    let gap = 10;
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
            Rgba::rgba(255, 255, 255, if hot { 34 } else { 20 }),
        );
        // A document glyph tinted by kind, then the name.
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
                Rgba::rgba(255, 255, 255, 60),
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
        cv.round_rect(
            x + i as i32 * 4,
            (mid as i32) - h / 2,
            2,
            h,
            1.0,
            Rgba::rgba(255, 255, 255, a),
        );
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
        Content::Idle
        | Content::Shelf(_)
        | Content::Clipboard { .. }
        | Content::Droplets { .. } => {}
        Content::Hud(hd) => hud_row(cv, th, fonts, x, w, mid, baseline, pad, hd),
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
        Content::Media(m) | Content::MediaLyrics { media: m, .. } => {
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
                cv.icon_pause(
                    (trail_x + 9) as f32,
                    mid,
                    10.0,
                    Rgba::rgba(255, 255, 255, 160),
                );
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
        // Drawn by dedicated body painters; HUDs never open.
        Content::Shelf(_) | Content::Clipboard { .. } | Content::Droplets { .. } | Content::Hud(_) => {}
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
            draw_droppy_media_body(cv, fonts, body, m, None, hover, regions);
        }
        Content::MediaLyrics { media, lyrics } => {
            draw_droppy_media_body(cv, fonts, body, media, Some(lyrics), hover, regions);
        }
    }
}

/// Droppy Media Player & Live Synced Lyrics card (matches References 2 & 3).
#[allow(clippy::too_many_arguments)]
fn draw_droppy_media_body(
    cv: &mut Canvas<'_>,
    fonts: &Fonts,
    body: (i32, i32, i32, i32),
    m: &Media,
    lyrics: Option<&[(isize, String)]>,
    hover: Option<(f64, f64)>,
    regions: &mut Vec<Region>,
) {
    let (bx, by, bw, bh) = body;
    let art = 44;
    cv.draw_album_art(bx, by, art, 10.0);

    // Warm coral/orange equalizer bars (or blue-violet in lyrics view).
    let wave_col = if lyrics.is_some() {
        Rgba::rgb(0x4c, 0x7a, 0xff)
    } else {
        Rgba::rgb(0xff, 0x6b, 0x35)
    };
    let wave_w = 22;
    let wave_x = bx + bw - wave_w - 2;
    for (i, bh_bar) in [5, 11, 8, 13, 6].into_iter().enumerate() {
        let h = if m.playing { bh_bar } else { 3 };
        cv.round_rect(
            wave_x + i as i32 * 4,
            by + art / 2 - h / 2,
            2,
            h,
            1.0,
            wave_col,
        );
    }

    let tx = bx + art + 12;
    let max_title_w = (wave_x - tx - 30).max(20);
    let used_tw = cv.text_truncated(
        fonts.bold.as_ref(),
        &m.title,
        tx,
        by + 18,
        14.5,
        max_title_w,
        Rgba::rgb(255, 255, 255),
    );
    // Crisp `[L]` synced-lyrics toggle badge right next to the track title.
    let badge_x = tx + used_tw + 6;
    let badge_y = by + 6;
    let l_on = lyrics.is_some();
    cv.round_rect(
        badge_x,
        badge_y,
        16,
        16,
        4.0,
        if l_on {
            Rgba::rgb(255, 255, 255)
        } else {
            Rgba::rgba(255, 255, 255, 225)
        },
    );
    cv.text(
        fonts.bold.as_ref(),
        "L",
        badge_x + 5,
        badge_y + 12,
        10.5,
        Rgba::rgb(0, 0, 0),
    );
    regions.push(Region {
        x: badge_x - 2,
        y: badge_y - 2,
        w: 20,
        h: 20,
        action: HitAction::Shell(ShellCmd::ToggleIslandLyrics),
    });

    cv.text_truncated(
        fonts.regular.as_ref(),
        &m.artist,
        tx,
        by + 36,
        12.5,
        wave_x - tx - 10,
        Rgba::rgba(255, 255, 255, 156),
    );

    // Scrubber row with elapsed & remaining (`-2:18`) timestamps.
    let scrub_y = by + art + 16;
    if let Some((elapsed, _total, pct)) = m.elapsed_label() {
        let rem_str = if let (Some(pos), Some(len)) = (m.position_us, m.length_us) {
            let rem_s = ((len - pos).max(0) / 1_000_000) as u64;
            format!("-{}:{:02}", rem_s / 60, rem_s % 60)
        } else {
            "-0:00".into()
        };
        let time_col = Rgba::rgba(255, 255, 255, 148);
        let ew = text_width(fonts.medium.as_ref(), &elapsed, 10.5);
        let rw = text_width(fonts.medium.as_ref(), &rem_str, 10.5);
        cv.text(fonts.medium.as_ref(), &elapsed, bx, scrub_y + 4, 10.5, time_col);
        cv.text(fonts.medium.as_ref(), &rem_str, bx + bw - rw, scrub_y + 4, 10.5, time_col);

        let bar_x = bx + ew + 8;
        let bar_w = (bw - ew - rw - 16).max(24);
        let bar_h = 5;
        let bar_y = scrub_y - 2;
        cv.round_rect(bar_x, bar_y, bar_w, bar_h, 2.5, Rgba::rgba(255, 255, 255, 42));
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
            cv.round_rect(bar_x, bar_y, fill.max(5), bar_h, 2.5, Rgba::rgba(255, 255, 255, 220));
        }
    }

    // 5-icon Droppy transport row: Queue, Prev, Play/Pause, Next, Device.
    let ctrl_y = scrub_y + 18;
    let ctrl_h = 34;
    let cy = ctrl_y + ctrl_h / 2;
    cv.text(fonts.medium.as_ref(), "≡", bx + 6, cy + 5, 16.0, Rgba::rgba(255, 255, 255, 170));
    cv.stroke_round_rect(bx + bw - 22, cy - 6, 16, 10, 2.0, 1.4, Rgba::rgba(255, 255, 255, 175));
    cv.fill_rect(bx + bw - 24, cy + 6, 20, 2, Rgba::rgba(255, 255, 255, 175));

    let btn = 38;
    let gap = 18;
    let row_w = btn * 3 + gap * 2;
    let start = bx + (bw - row_w) / 2;
    for (i, c) in [Control::Previous, Control::PlayPause, Control::Next].into_iter().enumerate() {
        let cx0 = start + i as i32 * (btn + gap);
        let r = (cx0, ctrl_y, btn, ctrl_h);
        if hovered(hover, r.0, r.1, r.2, r.3) {
            cv.round_rect(r.0, r.1, r.2, r.3, 17.0, Rgba::rgba(255, 255, 255, 28));
        }
        let cx = cx0 + btn / 2;
        match c {
            Control::Previous => cv.icon_skip(cx as f32, cy as f32, 18.0, false, Rgba::rgb(255, 255, 255)),
            Control::Next => cv.icon_skip(cx as f32, cy as f32, 18.0, true, Rgba::rgb(255, 255, 255)),
            Control::PlayPause if m.playing => cv.icon_pause(cx as f32, cy as f32, 22.0, Rgba::rgb(255, 255, 255)),
            Control::PlayPause => cv.icon_play(cx as f32, cy as f32, 22.0, Rgba::rgb(255, 255, 255)),
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

    // Live synced lyrics drawer below transport controls (matches Reference 2).
    if let Some(lines) = lyrics {
        let mut ly = ctrl_y + ctrl_h + 26;
        for (offset, text) in lines.iter().take(4) {
            let (font, col) = if *offset == 0 {
                (fonts.bold.as_ref(), Rgba::rgb(255, 255, 255))
            } else {
                (fonts.medium.as_ref(), Rgba::rgba(255, 255, 255, 86))
            };
            for wrapped in wrap_text(font, text, 14.5, bw - 8, 2) {
                if ly > by + bh - 24 {
                    break;
                }
                cv.text(font, &wrapped, bx + 2, ly, 14.5, col);
                ly += 21;
            }
            ly += 8;
        }
        // Bottom-right circular expand-to-fullscreen button (launches Lock/Ambient Media).
        let ex = bx + bw - 26;
        let ey = by + bh - 26;
        draw_expand_button(cv, ex, ey);
        regions.push(Region {
            x: ex,
            y: ey,
            w: 26,
            h: 26,
            action: HitAction::Shell(ShellCmd::Lock),
        });
    }
}

/// Crisp vector diagonal expand icon (`↗↙`) inside a 26 px dark glass circle.
fn draw_expand_button(cv: &mut Canvas<'_>, x: i32, y: i32) {
    cv.circle((x + 13) as f32, (y + 13) as f32, 13.0, Rgba::rgba(255, 255, 255, 26));
    let fg = Rgba::rgba(255, 255, 255, 220);
    // Diagonal shaft from bottom-left (x+9, y+17) to top-right (x+17, y+9)
    for i in 0..8 {
        cv.fill_rect(x + 9 + i, y + 16 - i, 2, 2, fg);
    }
    // Top-right arrowhead
    cv.fill_rect(x + 13, y + 8, 5, 2, fg);
    cv.fill_rect(x + 16, y + 8, 2, 5, fg);
    // Bottom-left arrowhead
    cv.fill_rect(x + 8, y + 16, 5, 2, fg);
    cv.fill_rect(x + 8, y + 13, 2, 5, fg);
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
        // Droppy's Dynamic Island: 210 x 37, 4 px below the top edge.
        assert_eq!((idle.w, idle.h, idle.y), (136.0, 30.0, 4.0));
        assert!(
            (idle.r - idle.h / 2.0).abs() < 0.6,
            "idle island is a capsule"
        );
        let busy = target(NotchStyle::Island, &media, false);
        assert_eq!(busy.h, idle.h, "a live activity keeps the pill's height");
        assert!(busy.w > idle.w, "and widens");
        let open = target(NotchStyle::Island, &media, true);
        assert_eq!(open.w, 332.0, "Droppy expanded media width");
        assert!(
            (130.0..=160.0).contains(&open.h),
            "expanded media height {}",
            open.h
        );
        assert_eq!(open.r, 34.0);
        let glance = target(NotchStyle::Island, &Content::Idle, true);
        assert_eq!(glance.r, 36.0);
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

    #[test]
    fn hover_widen_is_six_pixels_on_the_idle_pill() {
        let idle = target(NotchStyle::Island, &Content::Idle, false);
        assert_eq!(idle.w + 6.0, 142.0);
        let expanded = target(NotchStyle::Island, &Content::Idle, true);
        assert!(expanded.w > idle.w + 6.0);
    }

    #[test]
    fn spring_does_not_overshoot_and_morph_t_gates_content() {
        let a = target(NotchStyle::Island, &Content::Idle, false);
        let mut hover = a;
        hover.w += 6.0;
        let mut an = Anim::new(a);
        an.to = hover;
        // A 6 px hover is a tiny morph: content stays visible (morph_t high).
        assert!(an.morph_t() > 0.9, "hover does not hide content");
        let mut prev = a.w;
        let mut frames = 0;
        while an.step(1.0 / 60.0, false) {
            frames += 1;
            assert!(an.cur.w >= prev - 0.01, "no overshoot backward");
            assert!(an.cur.w <= hover.w + 0.01, "no overshoot past target");
            prev = an.cur.w;
            assert!(frames < 40, "hover widen should settle in ~120–180 ms");
        }
        assert_eq!(an.cur, hover);
        assert_eq!(an.morph_t(), 1.0);
        assert!(frames > 3);

        // A large expand (idle → media card) starts below the 0.6 content gate.
        let media = Content::Media(crate::media::Media {
            bus: "x".into(),
            title: "Clair de Lune".into(),
            artist: "Debussy".into(),
            playing: true,
            position_us: Some(15_000_000),
            length_us: Some(203_000_000),
        });
        let open = target(NotchStyle::Island, &media, true);
        an = Anim::new(a);
        an.to = open;
        assert!(
            an.morph_t() < 0.6,
            "full expand starts gated: {}",
            an.morph_t()
        );
        while an.step(1.0 / 60.0, false) {}
        assert_eq!(an.cur, open);
        assert_eq!(an.morph_t(), 1.0);
    }

    #[test]
    fn secondary_badge_kinds_are_distinct() {
        assert_ne!(LiveBadge::Media, LiveBadge::Activity);
        assert_ne!(LiveBadge::Activity, LiveBadge::Message);
    }
}
