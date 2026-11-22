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
use crate::state::ShellCmd;
use crate::surfaces::{HitAction, Region};
use crate::tokens::{Theme, T_BODY, T_CAPTION, T_LABEL, T_META};
use solyra_conf::look::NotchStyle;

/// The island's surface; the shape never exceeds it.
pub const SURF_W: u32 = 480;
pub const SURF_H: u32 = 214;
/// Concave fillet radius joining a notch to the top edge.
const EAR: f32 = 9.0;

/// What the island is showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Idle,
    Message(String),
    Activity { who: String, text: String },
    Attention(Ask),
    Media(Media),
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
    let (w, h) = match content {
        Content::Media(_) => (440.0, 156.0),
        Content::Idle => (if island { 348.0 } else { 392.0 }, 132.0),
        _ => (if island { 440.0 } else { 448.0 }, 196.0),
    };
    Shape {
        w,
        h,
        y: if island { 6.0 } else { 0.0 },
        r: if island { 44.0 } else { 30.0 },
    }
}

/// Where the shape wants to be.
pub fn target(style: NotchStyle, content: &Content, expanded: bool) -> Shape {
    match style {
        NotchStyle::Island => {
            if expanded {
                expanded_shape(style, content)
            } else {
                match content {
                    Content::Idle => Shape {
                        w: 126.0,
                        h: 36.0,
                        y: 4.0,
                        r: 18.0,
                    },
                    // Snug Live Activity capsules — tight leading/trailing, no dead ears.
                    Content::Media(_) => Shape {
                        w: 224.0,
                        h: 37.0,
                        y: 4.0,
                        r: 18.5,
                    },
                    Content::Activity { .. } | Content::Message(_) => Shape {
                        w: 268.0,
                        h: 37.0,
                        y: 4.0,
                        r: 18.5,
                    },
                    Content::Attention(_) => Shape {
                        w: 298.0,
                        h: 37.0,
                        y: 4.0,
                        r: 18.5,
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
                    Content::Idle => Shape {
                        w: 188.0,
                        h: 32.0,
                        y: 0.0,
                        r: 14.0,
                    },
                    Content::Media(_) => Shape {
                        w: 264.0,
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
    /// What the shape is made of. Solid is the classic pure black.
    pub material: crate::material::Material,
}

#[allow(clippy::too_many_arguments)]
/// The shape's area as rectangles, for the compositor's blur region.
pub fn blur_rects(style: NotchStyle, s: &Shape) -> Vec<(i32, i32, i32, i32)> {
    let (x, y, w, h) = (
        s.x().round() as i32,
        s.y.round() as i32,
        s.w.round() as i32,
        s.h.round() as i32,
    );
    match style {
        NotchStyle::Island => crate::material::blur_rects(x, y, w, h, s.r),
        _ => {
            // Square top (it hangs from the screen edge), rounded bottom:
            // the lower half of a rounded rect twice as tall.
            let r = s.r.min(w as f32 / 2.0).min(h as f32).max(0.0);
            let full = crate::material::blur_rects(x, y - h, w, 2 * h, r);
            full.into_iter()
                .filter_map(|(rx, ry, rw, rh)| {
                    let top = ry.max(y);
                    let bot = (ry + rh).min(y + h);
                    (bot > top).then_some((rx, top, rw, bot - top))
                })
                .collect()
        }
    }
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

    let pure_black = match extra.material {
        crate::material::Material::Solid => Rgba::rgb(0, 0, 0),
        crate::material::Material::Clear => Rgba::rgba(0, 0, 0, 232),
        crate::material::Material::Glass => Rgba::rgba(4, 5, 7, 244),
        crate::material::Material::Acrylic => Rgba::rgba(6, 7, 10, 246),
    };
    match style {
        NotchStyle::Island => {
            // Soft outer elevation shadow so the black capsule floats cleanly over bright sky.
            let spread = if expanded { 12.0 } else { 6.0 };
            let sh_a = if expanded { 82 } else { 52 };
            cv.round_rect_shadow(xi, yi, wi, hi, s.r, if expanded { 5.0 } else { 2.0 }, spread, sh_a);
            cv.round_rect(xi, yi, wi, hi, s.r, pure_black);
            // Delicate hardware optical edge rim so black-on-dark never disappears.
            let top_rim = if extra.material == crate::material::Material::Solid {
                Rgba::rgba(255, 255, 255, 28)
            } else {
                Rgba::rgba(255, 255, 255, 58)
            };
            cv.round_rect_glass_rim(
                xi,
                yi,
                wi,
                hi,
                s.r,
                top_rim,
                Rgba::rgba(255, 255, 255, 18),
                Rgba::rgba(255, 255, 255, 14),
            );
        }
        _ => {
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
    let to = anim.to;
    // Draw content only once the shape is (nearly) its final size, so text
    // never spills outside it mid-animation.
    let fits = (s.w - to.w).abs() < 10.0 && (s.h - to.h).abs() < 10.0;
    if !(settled || fits) {
        return regions;
    }

    if expanded {
        let pad_x = if style == NotchStyle::Island { 24 } else { 20 };
        let pad_y = if style == NotchStyle::Island { 20 } else { 16 };
        let body = (
            xi + pad_x,
            yi + pad_y,
            (wi - 2 * pad_x).max(0),
            (hi - 2 * pad_y).max(0),
        );
        expanded_body(cv, &th, fonts, body, content, extra, hover, &mut regions);
    } else {
        compact_row(cv, &th, fonts, xi, yi, wi, hi, content, style);
    }
    regions
}

/// Colourful Apple Music live audio spectrum bars.
fn media_waveform(cv: &mut Canvas<'_>, x: i32, mid: f32, active: bool) {
    let bars: [(i32, Rgba); 5] = [
        (7, Rgba::rgb(0xFF, 0x7A, 0x5A)),
        (13, Rgba::rgb(0xFF, 0xA8, 0x4C)),
        (9, Rgba::rgb(0xF8, 0xCC, 0x58)),
        (14, Rgba::rgb(0x64, 0xD2, 0xFF)),
        (6, Rgba::rgb(0x30, 0xB8, 0xFF)),
    ];
    for (i, (bh, col)) in bars.into_iter().enumerate() {
        let h = if active { bh } else { 4 };
        let c = if active {
            col
        } else {
            Rgba::rgba(255, 255, 255, 96)
        };
        cv.round_rect(x + i as i32 * 4, (mid as i32) - h / 2, 2, h, 1.0, c);
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
        Content::Idle => {}
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
                action: HitAction::Shell(ShellCmd::Notify(String::new())),
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
        assert_eq!(idle.w, 126.0);
        assert_eq!(idle.h, 36.0);
        assert!(idle.y > 0.0);
        assert!(
            (idle.r - idle.h / 2.0).abs() < 0.6,
            "idle island is a capsule"
        );
        let busy = target(NotchStyle::Island, &media, false);
        assert!(busy.h > idle.h, "busy compact is taller than idle");
        assert!((busy.h - 37.0).abs() < 0.6);
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
