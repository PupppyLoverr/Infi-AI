//! The notch and Dynamic Island: one black shape at the top centre that
//! morphs between an idle size, a compact live activity and an expanded card.
//!
//! Two looks, chosen in Settings (`notch_style`):
//! * **Notch** – hangs from the top edge with concave "ears", like a laptop
//!   display notch; expands downward.
//! * **Island** – a floating pill inside the bar; expands into a rounded card.
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

/// Expanded card size. Media is a Live Activity (~440×156, radius 44 on the
/// island). Idle is a short glance. Anything with buttons keeps more height.
fn expanded_shape(style: NotchStyle, content: &Content) -> Shape {
    let island = style == NotchStyle::Island;
    let (w, h) = match content {
        Content::Media(_) => (440.0, 156.0),
        Content::Idle => (if island { 348.0 } else { 400.0 }, 132.0),
        _ => (if island { 440.0 } else { 452.0 }, 196.0),
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
    let busy = !matches!(content, Content::Idle);
    match style {
        NotchStyle::Island => {
            if expanded {
                expanded_shape(style, content)
            } else if busy {
                // Compact live activity: a capsule just taller than idle.
                Shape {
                    w: 328.0,
                    h: 37.0,
                    y: 4.0,
                    r: 18.5,
                }
            } else {
                // Idle pill, floating just below the top edge.
                Shape {
                    w: 126.0,
                    h: 36.0,
                    y: 4.0,
                    r: 18.0,
                }
            }
        }
        // Off has no surface; the notch numbers are a harmless default.
        NotchStyle::Notch | NotchStyle::Off => {
            if expanded {
                expanded_shape(style, content)
            } else if busy {
                Shape {
                    w: 360.0,
                    h: 36.0,
                    y: 0.0,
                    r: 16.0,
                }
            } else {
                Shape {
                    w: 188.0,
                    h: 32.0,
                    y: 0.0,
                    r: 14.0,
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
    // The shape is always black; what is on it uses the dark palette.
    let th = Theme::dark();
    let s = anim.cur;
    let (x, y, w, h) = (s.x(), s.y, s.w, s.h);
    let (xi, yi, wi, hi) = (
        x.round() as i32,
        y.round() as i32,
        w.round() as i32,
        h.round() as i32,
    );

    let slab = crate::material::slab(
        extra.material,
        crate::material::notch_base(extra.material),
        None,
    );
    let body = slab.fill;
    match style {
        NotchStyle::Island => {
            cv.round_rect(xi, yi, wi, hi, s.r, body);
            if extra.material != crate::material::Material::Solid {
                cv.stroke_round_rect(xi, yi, wi, hi, s.r, 1.0, slab.edge);
            }
        }
        _ => {
            cv.notch_body(xi, 0, wi, hi, s.r, body);
            cv.concave_corner(x - EAR, 0.0, EAR, true, body);
            cv.concave_corner(x + w, 0.0, EAR, false, body);
        }
    }
    if let Some(hl) = slab.highlight {
        if style == NotchStyle::Island {
            let inset = (s.r * 0.6).round() as i32;
            cv.fill_rect(xi + inset, yi + 1, (wi - 2 * inset).max(0), 1, hl);
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
        // Inset past the corner radius. The expanded card replaces the compact
        // row; it is not stacked under it.
        let pad = 16;
        let body = (
            xi + pad,
            yi + pad,
            (wi - 2 * pad).max(0),
            (hi - 2 * pad).max(0),
        );
        expanded_body(cv, &th, fonts, body, content, extra, hover, &mut regions);
    } else {
        compact_row(cv, &th, fonts, xi, yi, wi, hi, content, style);
    }
    regions
}

/// Leading glyph and trailing status, grouped in the middle of the pill so a
/// short title does not leave a wide empty gap.
fn cluster_x(x: i32, w: i32, inset: i32, cluster_w: i32) -> i32 {
    let inner = (w - 2 * inset).max(0);
    let cw = cluster_w.min(inner);
    x + (w - cw).max(0) / 2
}

fn waveform(cv: &mut Canvas<'_>, x: i32, mid: f32, color: Rgba) {
    for (i, bh) in [7, 12, 5, 10].into_iter().enumerate() {
        cv.round_rect(x + i as i32 * 5, (mid as i32) - bh / 2, 3, bh, 1.5, color);
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
    let inset = if style == NotchStyle::Island { 14 } else { 18 };
    let inner = (w - 2 * inset).max(0);
    let gap = 8;
    match content {
        Content::Idle => {}
        Content::Message(t) => {
            let glyph = 16;
            let max_tw = (inner - glyph - gap).max(0);
            let tw = text_width(fonts.semibold.as_ref(), t, 12.0).min(max_tw);
            let start = cluster_x(x, w, inset, glyph + gap + tw);
            cv.icon_bell(start as f32 + 7.0, mid, th.text);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                t,
                start + glyph,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
        }
        Content::Activity { who, text } => {
            let glyph = 14;
            let bars = 18;
            let line = format!("{who} · {text}");
            let max_tw = (inner - glyph - gap - gap - bars).max(0);
            let tw = text_width(fonts.semibold.as_ref(), &line, 12.0).min(max_tw);
            let start = cluster_x(x, w, inset, glyph + gap + tw + gap + bars);
            cv.circle(start as f32 + 5.0, mid, 4.0, th.agent_active);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &line,
                start + glyph,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
            waveform(cv, start + glyph + gap + tw + gap, mid, th.agent_active);
        }
        Content::Attention(a) => {
            let glyph = 16;
            let what = match a.kind {
                AskKind::Permission => "Approval needed",
                AskKind::Question => "Question",
            };
            let line = format!("{what} · {}", a.who);
            let chip = if a.sent { "Sent" } else { "Review" };
            let chip_w = text_width(fonts.semibold.as_ref(), chip, 10.5) + 14;
            let max_tw = (inner - glyph - gap - gap - chip_w).max(0);
            let tw = text_width(fonts.semibold.as_ref(), &line, 12.0).min(max_tw);
            let start = cluster_x(x, w, inset, glyph + gap + tw + gap + chip_w);
            cv.ring(start as f32 + 6.0, mid, 4.5, 1.8, th.agent_approval);
            cv.circle(start as f32 + 6.0, mid, 1.6, th.agent_approval);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &line,
                start + glyph,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
            let cx = start + glyph + gap + tw + gap;
            let cy = y + (h - 18) / 2;
            cv.round_rect(cx, cy, chip_w, 18, 9.0, th.agent_approval);
            cv.text(
                fonts.semibold.as_ref(),
                chip,
                cx + 7,
                cy + 13,
                10.5,
                th.on_accent,
            );
        }
        Content::Media(m) => {
            let art = 18;
            let line = if m.title.is_empty() {
                m.artist.clone()
            } else {
                m.title.clone()
            };
            let elapsed = (!m.playing)
                .then(|| m.elapsed_label().map(|(e, _, _)| e))
                .flatten();
            let trail = if m.playing {
                18
            } else if let Some(elapsed) = elapsed.as_deref() {
                text_width(fonts.semibold.as_ref(), elapsed, 11.0)
            } else {
                12
            };
            let max_tw = (inner - art - gap - gap - trail).max(24);
            let tw = text_width(fonts.semibold.as_ref(), &line, 12.0).min(max_tw);
            let start = cluster_x(x, w, inset, art + gap + tw + gap + trail);
            let art_y = (mid as i32) - art / 2;
            cv.round_rect(start, art_y, art, art, 5.0, Rgba::rgba(255, 255, 255, 32));
            cv.icon_note(start as f32 + art as f32 / 2.0, mid, th.text);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &line,
                start + art + gap,
                baseline,
                12.0,
                max_tw,
                th.text,
            );
            let tx = start + art + gap + tw + gap;
            if m.playing {
                waveform(cv, tx, mid, th.text);
            } else if let Some(elapsed) = elapsed.as_deref() {
                cv.text(
                    fonts.semibold.as_ref(),
                    elapsed,
                    tx,
                    baseline,
                    11.0,
                    th.text,
                );
            } else {
                cv.icon_pause(tx as f32 + 6.0, mid, 10.0, th.text_dim);
            }
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
        Rgba::rgba(255, 255, 255, 40)
    } else {
        Rgba::rgba(255, 255, 255, 24)
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
            // A glance: time, then one quiet line. No button row.
            cv.text(
                fonts.regular.as_ref(),
                extra.clock,
                bx,
                by + 36,
                34.0,
                th.text,
            );
            cv.text(
                fonts.regular.as_ref(),
                extra.date,
                bx,
                by + 58,
                T_LABEL,
                th.text_dim,
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
                    fonts.regular.as_ref(),
                    &facts.join("   ·   "),
                    bx,
                    by + 82,
                    T_META,
                    bw,
                    th.text_med,
                );
            }
        }
        Content::Message(t) => {
            let mut y = by + 16;
            for l in wrap_text(fonts.regular.as_ref(), t, T_BODY, bw, 4) {
                cv.text(fonts.regular.as_ref(), &l, bx, y, T_BODY, th.text);
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
            cv.text(
                fonts.semibold.as_ref(),
                &who.to_uppercase(),
                bx,
                by + 12,
                T_CAPTION,
                th.agent_active,
            );
            let mut y = by + 34;
            for l in wrap_text(fonts.regular.as_ref(), text, T_BODY, bw, 3) {
                cv.text(fonts.regular.as_ref(), &l, bx, y, T_BODY, th.text);
                y += 19;
            }
            let r = (bx + bw - 140, buttons_y, 140, 32);
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
            // Artwork, title and artist, a progress bar with both times,
            // then transport controls with a 24px gap.
            let art = 52;
            cv.round_rect(bx, by, art, art, 14.0, Rgba::rgba(255, 255, 255, 28));
            cv.icon_note(
                bx as f32 + art as f32 / 2.0,
                by as f32 + art as f32 / 2.0,
                th.text,
            );
            let tx = bx + art + 14;
            let tw = (bw - art - 14).max(0);
            cv.text_truncated(
                fonts.semibold.as_ref(),
                &m.title,
                tx,
                by + 22,
                16.0,
                tw,
                th.text,
            );
            cv.text_truncated(
                fonts.regular.as_ref(),
                &m.artist,
                tx,
                by + 42,
                T_LABEL,
                tw,
                th.text_dim,
            );
            if let Some((elapsed, total, pct)) = m.elapsed_label() {
                let bar_y = by + art + 8;
                cv.round_rect(bx, bar_y, bw, 3, 1.5, Rgba::rgba(255, 255, 255, 36));
                let fill = ((bw as f32) * pct).round() as i32;
                if fill > 0 {
                    cv.round_rect(bx, bar_y, fill.max(4), 3, 1.5, th.text);
                }
                let time_base = bar_y + 16;
                cv.text(
                    fonts.regular.as_ref(),
                    &elapsed,
                    bx,
                    time_base,
                    T_CAPTION,
                    th.text_dim,
                );
                let total_w = text_width(fonts.regular.as_ref(), &total, T_CAPTION);
                cv.text(
                    fonts.regular.as_ref(),
                    &total,
                    bx + bw - total_w,
                    time_base,
                    T_CAPTION,
                    th.text_dim,
                );
            }
            let ctrl_h = 32;
            let ctrl_y = by + bh - ctrl_h;
            let btn = 36;
            let gap = 24;
            let row_w = btn * 3 + gap * 2;
            let start = bx + (bw - row_w) / 2;
            let cy = ctrl_y + ctrl_h / 2;
            let ctrls = [Control::Previous, Control::PlayPause, Control::Next];
            for (i, c) in ctrls.into_iter().enumerate() {
                let cx0 = start + i as i32 * (btn + gap);
                let r = (cx0, ctrl_y, btn, ctrl_h);
                if hovered(hover, r.0, r.1, r.2, r.3) {
                    cv.round_rect(r.0, r.1, r.2, r.3, 16.0, Rgba::rgba(255, 255, 255, 30));
                }
                let cx = cx0 + btn / 2;
                match c {
                    Control::Previous => cv.icon_skip(cx as f32, cy as f32, 16.0, false, th.text),
                    Control::Next => cv.icon_skip(cx as f32, cy as f32, 16.0, true, th.text),
                    Control::PlayPause if m.playing => {
                        cv.icon_pause(cx as f32, cy as f32, 16.0, th.text)
                    }
                    Control::PlayPause => cv.icon_play(cx as f32, cy as f32, 16.0, th.text),
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
    cv.text(
        fonts.semibold.as_ref(),
        &head,
        bx,
        by + 12,
        T_CAPTION,
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
